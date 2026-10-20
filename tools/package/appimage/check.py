#!/usr/bin/env python3
"""Check the real artifact in a runtime-only root (no Nix or build libraries).

The AppImage or the tarball: extract it into a path with spaces and an
apostrophe; check every helper and ELF, render synthetic PDFs, then run the
existing real-window smoke test. The caller also uses this on other
distributions in CI.
"""

import argparse
import fnmatch
import json
import os
import pathlib
import re
import selectors
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import wave
import zipfile

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
    print(f"ok: {count} ELFs resolve their libraries; glibc <= 2.35, relative runtime paths", flush=True)


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
    artifact = pathlib.Path(sys.argv[1]).resolve()
    base = pathlib.Path.home() / ".cache/skwir-tests"
    base.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="package check's ", dir=base) as directory:
        work = pathlib.Path(directory)
        app, launcher = unpack(artifact, work)
        check_elf(app)
        if artifact.name.endswith(".AppImage"):
            run([artifact, "--version"], env=dict(os.environ, APPIMAGE_EXTRACT_AND_RUN="1"))
        run([app / launcher, "--version"])
        programs = (ROOT / "tools/package/programs").read_text().splitlines()
        for program in programs:
            if program != "skwir":
                run([app / "usr/lib/skwir" / program, "--probe"])
                print(f"ok: {program} --probe", flush=True)
        for engine in ("pdfium", "poppler", "mupdf"):
            worker = "skwir-pdf-worker" if engine == "pdfium" else f"skwir-{engine}-worker"
            run(["python3", ROOT / "tools/pdf/check.py", "--engine", engine,
                 "--worker", app / "usr/lib/skwir" / worker])
        check_media(app, work)
        check_window(app, work, launcher)
        print(f"{artifact.name}: passed", flush=True)


if __name__ == "__main__":
    main()
