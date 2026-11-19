#!/usr/bin/env python3
"""Check the real artifact in a runtime-only root (no Nix or build libraries).

The AppImage or the tarball: extract it into a path with spaces and an
apostrophe; check every helper and ELF, render synthetic PDFs, then run the
existing real-window smoke test. The caller also uses this on other
distributions in CI.
"""

import argparse
import fnmatch
import hashlib
import json
import os
import pathlib
import re
import selectors
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import time
import wave
import zipfile

from gstreamer import PREFIX
from stage import HOST_ABI

ROOT = pathlib.Path(__file__).resolve().parents[3]


def run(argv, **kwargs):
    return subprocess.run([str(a) for a in argv], check=True, timeout=240, **kwargs)


def check_elf(app):
    count = 0
    for path in app.rglob("*"):
        if path.is_symlink():
            assert not os.path.isabs(os.readlink(path)), (path, "absolute symlink")
            assert path.resolve().is_relative_to(app.resolve()) and path.exists(), (path, "broken or escaping symlink")
            continue
        if not path.is_file():
            continue
        with path.open("rb") as stream:
            if stream.read(4) != b"\x7fELF":
                continue
        count += 1
        # The host keeps its libc and graphics drivers, including Mesa's
        # display libraries. Do not accidentally ship a driver via codecs.
        assert path.name != "libc.so.6" and not path.name.startswith("ld-linux"), path
        assert not any(fnmatch.fnmatch(path.name, pattern) for pattern in HOST_ABI), path
        dynamic = run(["readelf", "-d", path], capture_output=True, text=True).stdout
        assert "(RPATH)" not in dynamic, (path, "inherited library path")
        for line in dynamic.splitlines():
            if "RPATH" in line or "RUNPATH" in line or "NEEDED" in line:
                assert "/nix/store/" not in line and "/build/" not in line and "/src/" not in line, (path, line)
            if "(RUNPATH)" in line:
                value = re.search(r"\[(.*?)\]", line).group(1)
                for directory in value.split(":"):
                    assert directory == "$ORIGIN" or directory.startswith("$ORIGIN/"), (path, directory)
                    resolved = pathlib.Path(directory.replace("$ORIGIN", str(path.parent))).resolve()
                    assert resolved.is_relative_to(app.resolve()), (path, directory, "escaping library path")
        versions = run(["readelf", "--version-info", path], capture_output=True, text=True).stdout
        needs = [tuple(map(int, v)) for v in re.findall(r"GLIBC_(\d+)\.(\d+)", versions)]
        assert not needs or max(needs) <= (2, 35), (path, max(needs))
        libraries = run(["ldd", path], capture_output=True, text=True).stdout
        assert "not found" not in libraries, (path, libraries)
        baked = built_here(path)
        assert not baked, (path, sorted(baked), "a path of the build's prefix, looked for on the user's machine")
    print(f"ok: {count} ELFs resolve their libraries; glibc <= 2.35, relative runtime paths, "
          f"no path of {PREFIX} looked for", flush=True)


# What of the build's prefix (gstreamer.py's PREFIX, which exists only
# where the AppImage is built) a staged ELF may still name, by library:
# what the media helper's environment points elsewhere, or what playback
# never reads. Any other path of it is one a library would look for on
# the user's machine in vain (the system's settings, a server's socket:
# libpulse's and libva's did).
NAMED_IN = {
    # Where it finds its plugins and its scanner when not told: the media
    # helper tells it (GST_PLUGIN_SYSTEM_PATH_1_0, GST_PLUGIN_SCANNER_1_0).
    # Under share, presets, which playback never reads.
    "libgstreamer-1.0.so.*": {"/lib", "/lib/gstreamer-1.0", "/libexec/gstreamer-1.0/gst-plugin-scanner",
                              "/share"},
    # The helper of a PTP network clock, which playback never makes.
    "libgstnet-1.0.so.*": {"/libexec/gstreamer-1.0/gst-ptp-helper"},
    # Run only when a program asks for missing plugins to be installed;
    # under share, encoding targets, which playback never reads.
    "libgstpbutils-1.0.so.*": {"/libexec/gst-install-plugins-helper", "/share"},
}


def built_here(path):
    """The paths of PREFIX `path` names and may not: in any section but
    the dynamic string table, where patchelf leaves the RUNPATH it
    replaced (the RUNPATH itself is checked above), and other than an
    assertion's message naming a header (PREFIX/include)."""
    data = path.read_bytes()
    named = re.compile(re.escape(str(PREFIX).encode()) + rb"[\x21-\x7e]*")
    allowed = set().union(*(paths for pattern, paths in NAMED_IN.items() if fnmatch.fnmatch(path.name, pattern)))
    # ELF64, little-endian (x86-64): the section headers and their names.
    offset, = struct.unpack_from("<Q", data, 0x28)
    size, count, names = struct.unpack_from("<HHH", data, 0x3A)
    headers = [struct.unpack_from("<IIQQQQ", data, offset + i * size) for i in range(count)]
    strings = headers[names][4] if headers else 0
    found = set()
    for name, kind, _, _, start, length in headers:
        section = data[strings + name:data.index(b"\0", strings + name)]
        # NOBITS holds nothing in the file.
        if kind == 8 or section == b".dynstr":
            continue
        for match in named.finditer(data, start, start + length):
            rest = match.group().decode()[len(str(PREFIX)):]
            if not rest.startswith("/include/") and rest not in allowed:
                found.add(match.group().decode())
    return found


def unpack(artifact, work):
    """The tree an artifact holds, extracted as its user would, and the
    program that starts it."""
    if artifact.name.endswith(".AppImage"):
        run([artifact, "--appimage-extract"], cwd=work, stdout=subprocess.DEVNULL)
        return work / "squashfs-root", "AppRun"
    top = artifact.name.removesuffix(".tar.gz")
    with tarfile.open(artifact) as archive:
        members = archive.getmembers()
    for member in members:
        path = pathlib.PurePosixPath(member.name)
        assert path.parts[0] == top and ".." not in path.parts and not path.is_absolute(), member.name
        assert member.isfile() or member.isdir() or member.issym() or member.islnk(), \
            (member.name, "not a file, folder or link")
        if member.islnk():
            target = pathlib.PurePosixPath(member.linkname)
            assert target.parts[0] == top and ".." not in target.parts, (member.name, member.linkname)
        assert (member.uid, member.gid, member.uname, member.gname) == (0, 0, "", ""), (member.name, "owner")
    assert {pathlib.PurePosixPath(m.name).parts[1:2] for m in members} == {(), ("skwir",), ("usr",)}, \
        "the tarball holds more than the program and its usr tree"
    run(["tar", "-xzf", artifact], cwd=work)
    assert os.readlink(work / top / "skwir") == "usr/bin/skwir"
    print("ok: the tarball holds one folder, its program and usr tree, owned by root", flush=True)
    return work / top, "skwir"


def check_media(app, work):
    audio = work / "a quiet recording.wav"
    with wave.open(str(audio), "wb") as stream:
        stream.setparams((1, 2, 16_000, 0, "NONE", "not compressed"))
        stream.writeframes(b"\x00\x00" * 16_000)
    (work / "video.bgra").touch()
    env = dict(os.environ, G_DEBUG="fatal-criticals", SKWIR_MEDIA_TEST_AUDIO="1", SKWIR_MEDIA_AUDIO_ONLY="1")
    diagnostics = tempfile.TemporaryFile()
    process = subprocess.Popen([str(app / "usr/lib/skwir/skwir-media-worker"), str(audio), str(work)],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=diagnostics, env=env)
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            deadline = time.monotonic() + 30
            buffered = b""
            while time.monotonic() < deadline:
                if not selector.select(timeout=max(0, deadline - time.monotonic())):
                    break
                chunk = os.read(process.stdout.fileno(), 65536)
                assert chunk, "media worker exited before playing the recording"
                buffered += chunk
                while b"\n" in buffered:
                    line, buffered = buffered.split(b"\n", 1)
                    reply = json.loads(line)
                    assert not isinstance(reply, dict) or not {"Error", "Unavailable"} & reply.keys(), reply
                    if reply == "Ready":
                        process.stdin.write(b'{"Control":{"generation":1,"command":{"Play":true}}}\n')
                        process.stdin.flush()
                    if isinstance(reply, dict) and reply.get("State", {}).get("playback", {}).get("status") == "Ended":
                        print("ok: bundled GStreamer decodes and plays a synthetic WAV", flush=True)
                        return
            raise AssertionError("media worker did not finish playback within 30 seconds")
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        diagnostics.seek(0)
        output = diagnostics.read().decode(errors="replace")
        diagnostics.close()
        if output:
            print(output, file=sys.stderr)
        # G_DEBUG also applies to the scanner child, whose crash can be
        # hidden by GStreamer's fallback and plugin blacklisting.
        assert "CRITICAL" not in output, "media runtime or plugin scanner reported a critical error"


class Player:
    """The bundle's media helper on `film`, its frames taken from the slot
    in `work` and acknowledged, as the window does (crates/core's
    `media` protocol)."""

    def __init__(self, app, film, work, env):
        (work / "video.bgra").touch()
        self.diagnostics = tempfile.TemporaryFile()
        self.process = subprocess.Popen([str(app / "usr/lib/skwir/skwir-media-worker"), str(film), str(work)],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=self.diagnostics, env=env)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.buffered = b""
        self.generation = 0
        self.state = {}
        self.frames = 0
        self.frame_generation = None

    def control(self, command):
        self.generation += 1
        self.send({"Control": {"generation": self.generation, "command": command}})

    def send(self, message):
        self.process.stdin.write(json.dumps(message).encode() + b"\n")
        self.process.stdin.flush()

    def until(self, condition, what, timeout=20):
        deadline = time.monotonic() + timeout
        while not condition():
            left = deadline - time.monotonic()
            assert left > 0 and self.selector.select(timeout=left), f"the media helper never {what}"
            chunk = os.read(self.process.stdout.fileno(), 65536)
            assert chunk, f"the media helper exited before it {what}"
            self.buffered += chunk
            while b"\n" in self.buffered:
                line, self.buffered = self.buffered.split(b"\n", 1)
                reply = json.loads(line)
                assert not isinstance(reply, dict) or not {"Error", "Unavailable", "Rejected"} & reply.keys(), reply
                if isinstance(reply, dict) and "State" in reply:
                    if reply["State"]["generation"] == self.generation:
                        self.state = reply["State"]["playback"]
                if isinstance(reply, dict) and "Frame" in reply:
                    self.frames += 1
                    self.frame_generation = reply["Frame"]["generation"]
                    self.send("Consumed")

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.diagnostics.seek(0)
        output = self.diagnostics.read().decode(errors="replace")
        self.diagnostics.close()
        if output:
            print(output, file=sys.stderr)
        assert "CRITICAL" not in output, "media runtime or plugin scanner reported a critical error"


def check_film(app, work, film):
    """A film played through the bundle's own pulsesink and PulseAudio
    client, the clock of every film the window plays, against a PulseAudio
    of the check's own (a null sink, nothing heard): paused, then sought or
    played and paused at once, and played again, eight times, its frames
    and its position going on each time. With Jammy's client library
    (15.99) the picture stopped for good, at the seventh here, against
    every server tried (ADR-052 amended)."""
    # Its socket's path plain and short: PulseAudio's module arguments are
    # split at spaces, and `work`'s name has a space and an apostrophe.
    runtime = pathlib.Path(tempfile.mkdtemp(prefix="skwir-pulse-"))
    socket = runtime / "native"
    log = tempfile.TemporaryFile()
    server = subprocess.Popen(
        ["pulseaudio", "-n", "--daemonize=no", "--exit-idle-time=-1", "--disable-shm=yes",
         "--log-target=stderr", "-L", "module-null-sink",
         "-L", f"module-native-protocol-unix socket={socket} auth-anonymous=1"],
        env=dict(os.environ, HOME=str(runtime), XDG_RUNTIME_DIR=str(runtime)),
        stdout=subprocess.DEVNULL, stderr=log)
    player = None
    try:
        deadline = time.monotonic() + 20
        while not socket.exists():
            assert server.poll() is None and time.monotonic() < deadline, "PulseAudio did not start"
            time.sleep(0.1)
        slot = work / "film"
        slot.mkdir()
        env = dict(os.environ, G_DEBUG="fatal-criticals", PULSE_SERVER=f"unix:{socket}")
        player = Player(app, film, slot, env)
        player.until(lambda: player.frames and player.state.get("duration"), "showed the film's first frame")
        player.control({"Mute": True})
        player.until(lambda: player.state.get("muted"), "muted")
        # Eight times what the window does, paused: a seek (a frame shown
        # again, a scrub), or play and pause at once (peek closed as it
        # opened); then play. 1.20's stopped at the third round or so.
        for round in range(8):
            player.control({"Play": True})
            before, position = player.frames, player.state.get("position", 0)
            player.until(lambda: player.frames > before + 5 and player.state.get("position", 0) > position + 0.1,
                         f"played on, round {round + 1}", timeout=10)
            if round == 0:
                sinks = run(["pactl", "list", "short", "sink-inputs"], capture_output=True, text=True,
                            env=dict(os.environ, PULSE_SERVER=f"unix:{socket}")).stdout
                assert sinks.strip(), "the film's sound did not go to PulseAudio"
            player.control({"Play": False})
            player.until(lambda: player.state.get("status") == "Paused", "paused")
            if round % 2:
                player.control({"Play": True})
                player.control({"Play": False})
                player.until(lambda: player.state.get("status") == "Paused", "paused again")
            else:
                player.control({"Seek": 1.0 + round})
                sought = player.generation
                player.until(lambda: player.frame_generation == sought, "showed the frame sought while paused")
        print("ok: the bundle's pulsesink plays a film on after pauses, seeks while paused and"
              " plays and pauses at once", flush=True)
    finally:
        if player:
            player.close()
        server.terminate()
        server.wait(timeout=10)
        shutil.rmtree(runtime, ignore_errors=True)
        log.seek(0)
        said = log.read().decode(errors="replace")
        log.close()
        if said and sys.exc_info()[0]:
            print(said, file=sys.stderr)


def fuse_here():
    """Whether this system lets a user mount FUSE, as the helpers
    tools/package/fuse lists need: its device, and the program that mounts
    it on PATH."""
    return os.path.exists("/dev/fuse") and any(shutil.which(name) for name in ("fusermount3", "fusermount"))


def listed(name):
    """A list of tools/package: one program a line, `#` starting a comment."""
    lines = (ROOT / "tools/package" / name).read_text().splitlines()
    return [line for line in lines if line and not line.startswith("#")]


def check_helpers(app, build):
    """Every helper starts as skwir's own build (`--build`) and works here
    (`--probe`). One that needs FUSE, in a root without it, may only say
    so: the systems the package goes to may have none either."""
    fuse = set(listed("fuse"))
    for program in listed("programs"):
        if program == "skwir":
            continue
        helper = app / "usr/lib/skwir" / program
        said = run([helper, "--build"], capture_output=True, text=True).stdout.strip()
        assert said == build, (program, "is not skwir's build", said, build)
        probe = subprocess.run([str(helper), "--probe"], capture_output=True, text=True, timeout=240)
        why = (probe.stderr.strip().splitlines() or ["(it said nothing)"])[-1]
        if probe.returncode == 0:
            print(f"ok: {program} --probe", flush=True)
        elif program in fuse and not fuse_here():
            print(f"ok: {program} --build; this root has no FUSE, as its probe says: {why}", flush=True)
        else:
            raise AssertionError(f"{program} --probe failed: {why}")


def elf_section(path, wanted):
    """The bytes of the section named `wanted` of the 64-bit ELF file at
    `path` (an AppImage's runtime), read by its section headers."""
    with path.open("rb") as elf:
        head = elf.read(64)
        assert head[:4] == b"\x7fELF" and head[4] == 2, f"{path.name}: not a 64-bit ELF file"
        (table_at,) = struct.unpack_from("<Q", head, 0x28)
        size, count, names_index = struct.unpack_from("<HHH", head, 0x3A)
        elf.seek(table_at)
        table = elf.read(size * count)

        def section(index):
            name, _, _, _, offset, length = struct.unpack_from("<IIQQQQ", table, index * size)
            return name, offset, length

        _, names_at, names_length = section(names_index)
        elf.seek(names_at)
        names = elf.read(names_length)
        for index in range(count):
            name, offset, length = section(index)
            if names[name:names.index(b"\0", name)] == wanted:
                elf.seek(offset)
                return elf.read(length)
    raise AssertionError(f"{path.name}: no {wanted.decode()} section")


def check_updates(image):
    """What AppImage's updaters read (ADR-142): the image's update
    information names the latest release's zsync file on skwir.org, and the
    zsync file beside it, named without a version, describes this image at
    an address of its own."""
    cargo = (ROOT / "Cargo.toml").read_text()
    site = re.search(r'^homepage = "(.*)"$', cargo, re.M).group(1)
    version = re.search(r'^version = "(.*)"$', cargo, re.M).group(1)
    said = elf_section(image, b".upd_info").rstrip(b"\0").decode()
    assert said == f"zsync|{site}/latest/skwir-x86_64.AppImage.zsync", said
    zsync = image.with_name("skwir-x86_64.AppImage.zsync")
    head = {}
    with zsync.open("rb") as described:
        for line in described:
            if line == b"\n":
                break
            key, _, value = line.decode().rstrip("\n").partition(": ")
            head[key] = value
    assert head.get("URL") == f"{site}/files/v{version}/{image.name}", head.get("URL")
    sha1 = hashlib.sha1()
    with image.open("rb") as read:
        while chunk := read.read(1 << 20):
            sha1.update(chunk)
    assert head.get("SHA-1") == sha1.hexdigest(), "the zsync file describes another image"
    assert int(head.get("Length", -1)) == image.stat().st_size, "the zsync file's length"
    print(f"ok: the update information and {zsync.name}", flush=True)


def check_channel(app, word):
    """How the copy came, as skwir reads it (ADR-142, tools/package/channel):
    the AppImage's word, or the tarball's."""
    said = (app / "usr/share/skwir/channel").read_text()
    assert said == f"{word}\n", (word, said)
    print(f"ok: the channel says {word}", flush=True)


def check_fonts(app):
    """Skwir's own fonts where skwir finds them, with their licences
    (ADR-118, tools/package/fonts.py)."""
    fonts = app / "usr/share/skwir/fonts"
    for name in ("Inter-Regular", "Inter-Italic", "Inter-Medium", "Inter-SemiBold",
                 "Inter-Bold", "Inter-BoldItalic", "JetBrainsMono-Regular",
                 "JetBrainsMono-Italic", "JetBrainsMono-Bold", "JetBrainsMono-BoldItalic"):
        assert (fonts / f"{name}.ttf").is_file(), (name, "is not carried")
    licences = app / "usr/share/licenses/skwir/fonts"
    for licence in ("Inter/LICENSE.txt", "JetBrainsMono/OFL.txt"):
        assert (licences / licence).is_file(), (licence, "is not carried")
    print("ok: skwir's own fonts and their licences", flush=True)


def check_window(app, work, launcher):
    # Reuse the real-window harness, then exercise a packaged helper
    # through its user-facing command and a host program through Skwir.
    sys.path.insert(0, str(ROOT / "tools/test"))
    import smoke

    observed = work / "host-environment.json"
    terminal = work / "terminal"
    keys = ["PATH", "LD_LIBRARY_PATH", "LD_PRELOAD", "GST_PLUGIN_PATH", "GST_PLUGIN_PATH_1_0",
            "GST_PLUGIN_SYSTEM_PATH", "GST_PLUGIN_SYSTEM_PATH_1_0",
            "GST_PLUGIN_SCANNER", "GST_PLUGIN_SCANNER_1_0"]
    terminal.write_text(
        "#!/usr/bin/python3\nimport json, os\n"
        f"keys = {keys!r}\n"
        "with open(os.environ['SKWIR_TEST_ENV'], 'w') as out:\n"
        "    json.dump({key: os.environ.get(key) for key in keys}, out)\n"
    )
    terminal.chmod(0o755)

    class Window(smoke.Window):
        def env(self):
            return dict(super().env(), TERMINAL=str(terminal), SKWIR_TEST_ENV=str(observed))

    icds = list(pathlib.Path("/usr/share/vulkan/icd.d").glob("lvp_icd*.json"))
    assert len(icds) == 1, f"expected one lavapipe ICD, found {icds}"
    window = Window(argparse.Namespace(skwir=str(app / launcher), xvfb="/usr/bin/Xvfb",
                                      icd=str(icds[0])))
    passed = False
    try:
        window.start()
        smoke.run(window)
        archive = window.files / "a traveller's files.zip"
        # Two members, so they go in a folder named after the archive: one
        # alone is placed as itself (ADR-048), and the smoke run's notes/
        # would make it a name taken, a question no script answers.
        with zipfile.ZipFile(archive, "w") as zipped:
            zipped.writestr("notes/café's draft.txt", "unpacked\n")
            zipped.writestr("read me.txt", "kept beside\n")
        window.cmd("archive.extract", {"entries": [str(archive)]}, apply=True)
        unpacked = archive.with_suffix("") / "notes/café's draft.txt"
        assert smoke.until(unpacked.exists), "sandboxed archive extraction did not finish"
        assert unpacked.read_text() == "unpacked\n"
        window.cmd("undo")
        assert smoke.until(lambda: not archive.with_suffix("").exists())
        print("ok: packaged archive helper extracts through the sandbox; undo removes the result", flush=True)
        window.cmd("navigate", {"location": str(window.files)})
        window.cmd("open_terminal")
        assert smoke.until(observed.exists), "the host terminal did not run"
        assert json.loads(observed.read_text()) == {key: window.env().get(key) for key in keys}
        print("ok: host terminal receives its original library, plugin and search paths", flush=True)
        passed = True
    finally:
        window.stop()
        if passed:
            shutil.rmtree(window.root, ignore_errors=True)
        else:
            print(f"window diagnostics: {window.root / 'skwir.log'}", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact", type=pathlib.Path)
    # Made where the AppImage is built (build.sh); elsewhere, not checked.
    parser.add_argument("--film", type=pathlib.Path)
    args = parser.parse_args()
    artifact = args.artifact.resolve()
    base = pathlib.Path.home() / ".cache/skwir-tests"
    base.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="package check's ", dir=base) as directory:
        work = pathlib.Path(directory)
        app, launcher = unpack(artifact, work)
        check_elf(app)
        if artifact.name.endswith(".AppImage"):
            run([artifact, "--version"], env=dict(os.environ, APPIMAGE_EXTRACT_AND_RUN="1"))
            check_updates(artifact)
        said = run([app / launcher, "--version"], capture_output=True, text=True).stdout.strip()
        print(said, flush=True)
        assert said.startswith("skwir "), said
        check_helpers(app, said.removeprefix("skwir "))
        check_fonts(app)
        check_channel(app, "appimage" if artifact.name.endswith(".AppImage") else "tarball")
        for engine in ("pdfium", "poppler", "mupdf"):
            worker = "skwir-pdf-worker" if engine == "pdfium" else f"skwir-{engine}-worker"
            run(["python3", ROOT / "tools/pdf/check.py", "--engine", engine,
                 "--worker", app / "usr/lib/skwir" / worker])
        check_media(app, work)
        if args.film:
            check_film(app, work, args.film.resolve())
        else:
            print("not checked: a film through the bundle's pulsesink (no --film)", flush=True)
        check_window(app, work, launcher)
        print(f"{artifact.name}: passed", flush=True)


if __name__ == "__main__":
    main()
