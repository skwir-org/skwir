#!/usr/bin/env python3
"""GStreamer as Windows' and macOS's packages carry it (ADR-076): GStreamer's
own build of the release tools/package/gstreamer.lock pins, checked against
its sums, cut down to what skwir's media helper plays with, and kept in the
build cache by target, release and recipe. Prints that folder:

- runtime/: what the package carries, laid out as GStreamer finds it: bin
  (Windows: the libraries, the helper beside them) or lib (macOS: the
  libraries, arm64 alone), lib/gstreamer-1.0 the plugins, libexec the
  plugin scanner; on macOS inside a framework bundle,
  GStreamer.framework/Versions/1.0, as a bundle's Frameworks holds one
  (GStreamer's own main binary re-exports libraries the set leaves out:
  this one's is libgstreamer);
- link/: what the helper is linked against, pkg-config's files with the
  import libraries (Windows) or the libraries' names (macOS) of runtime/;
- licenses/: the notices of the components runtime/ holds, and the
  versions GStreamer's build lists.

The set: GStreamer's core, its playback, the containers, every decoder
FFmpeg has (gst-libav: H.264, HEVC and AAC in software, as Linux's
distributions and skwir's AppImage carry them), dav1d for AV1, the
subtitle renderers (pango, libass, bitmaps), the system's video decoders
and audio output (Direct3D 11 and WASAPI; VideoToolbox, AudioToolbox and
Core Audio), and the helper's own (ReplayGain, tempo, spectrum). No GPL
plugin: what they decode (AC-3, MPEG-2), FFmpeg does too.

Windows' build is an installer (Inno Setup 6.7, which innoextract cannot
open): it runs under a WoW64 Wine on an Xvfb display of its own
(`SKWIR_WINE_WOW64`, the `windows` shell). macOS's are installer packages,
opened with xar, pbzx and cpio (the `macos` shell). The libraries are
stripped of their debug information (Windows) or thinned to Apple
silicon (macOS).

    tools/package/gstreamer.py <x86_64-pc-windows-gnu|aarch64-apple-darwin>
"""

import hashlib
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[2]
LOCK = ROOT / "tools/package/gstreamer.lock"
CACHE = pathlib.Path(os.environ.get("XDG_CACHE_HOME") or pathlib.Path.home() / ".cache") / "skwir-build/gstreamer"

COMMON = [
    # The core and playback, and what playbin3 plugs in.
    "coreelements", "typefindfunctions", "playback", "app", "videoconvertscale", "audioconvert",
    "audioresample", "volume", "autodetect", "pbtypes",
    # What the helper adds: ReplayGain, the tempo kept at another rate, the spectrum.
    "replaygain", "audiofx", "spectrum",
    # Containers and parsers.
    "matroska", "isomp4", "ogg", "avi", "flv", "asf", "wavparse", "aiff", "mpegtsdemux",
    "mpegpsdemux", "audioparsers", "videoparsersbad", "id3demux", "apetag",
    # Decoders: FFmpeg's, dav1d's, and those base and good bring on Linux.
    "libav", "dav1d", "opus", "vorbis", "flac",
    # Subtitles: text (pango), styled (libass), bitmaps (DVD, DVB).
    "subparse", "pango", "assrender", "dvdsub", "dvdspu", "dvbsuboverlay",
]
SYSTEM = {
    "windows": ["d3d11", "wasapi2"],
    "macos": ["applemedia", "osxaudio"],
}
# The libraries the helper links (crates/media-worker: gstreamer-rs, and
# GIO through the glib crate), the plugins' own following from them.
LINKED = ["gstreamer-1.0", "gstbase-1.0", "gstapp-1.0", "gstvideo-1.0", "gio-2.0"]


def lock():
    entries, version = [], None
    for line in LOCK.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        key, value = line.split()
        if key == "version":
            version = value
        else:
            entries.append((key, value))
    return version, entries


def run(*args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def fetch(path, sum_, into):
    """The file at GStreamer's `path`, downloaded once, checked against its sum."""
    into.mkdir(parents=True, exist_ok=True)
    out = into / pathlib.Path(path).name
    if not out.exists() or sha256(out) != sum_:
        partial = out.with_name(out.name + ".part")
        print(f"Downloading {out.name}", file=sys.stderr)
        run("curl", "-fsSL", "-o", partial, f"https://gstreamer.freedesktop.org/data/pkg/{path}")
        if sha256(partial) != sum_:
            partial.unlink()
            sys.exit(f"{out.name}: its sum is not the one pinned")
        partial.rename(out)
    return out


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as file:
        for block in iter(lambda: file.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def install_windows(installer, into, work):
    """Inno Setup's installer, run quietly by a WoW64 Wine in a prefix of its
    own, on an X display of its own (it draws a window it never shows)."""
    wine = pathlib.Path(os.environ["SKWIR_WINE_WOW64"])
    display = next(n for n in range(90, 200) if not pathlib.Path(f"/tmp/.X11-unix/X{n}").exists()
                   and not pathlib.Path(f"/tmp/.X{n}-lock").exists())
    xvfb = subprocess.Popen(["Xvfb", f":{display}", "-screen", "0", "1024x768x24", "-nolisten", "tcp"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    # As tools/cross/wine-env.sh: no Mono or Gecko, no winemenubuilder (it
    # writes the prefix's menus and types into the user's desktop), XDG
    # data and config beside the prefix.
    env = {"PATH": f"{wine}:{os.environ['PATH']}", "HOME": str(pathlib.Path.home()),
           "WINEPREFIX": str(work / "prefix"), "WINEDEBUG": "-all",
           "WINEDLLOVERRIDES": "mscoree,mshtml=;winemenubuilder.exe=d", "DISPLAY": f":{display}",
           "XDG_DATA_HOME": str(work / "xdg" / "data"), "XDG_CONFIG_HOME": str(work / "xdg" / "config")}
    try:
        for _ in range(100):
            if pathlib.Path(f"/tmp/.X11-unix/X{display}").exists():
                break
            time.sleep(0.1)
        run(wine / "wineboot", "-i", env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        into.mkdir(parents=True)
        where = lambda p: run(wine / "winepath", "-w", p, env=env, capture_output=True, text=True).stdout.strip()
        log = work / "install.log"
        subprocess.run([str(wine / "wine"), str(installer), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART",
                        "/SP-", "/CURRENTUSER", "/TYPE=full", f"/DIR={where(into)}", f"/LOG={where(log)}"],
                       env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        run(wine / "wineserver", "-w", env=env)
    finally:
        subprocess.run([str(wine / "wineserver"), "-k"], env=env, stderr=subprocess.DEVNULL)
        xvfb.terminate()
        xvfb.wait()
    said = log.read_bytes().decode("utf-8", "replace") if log.exists() else ""
    if "Installation process succeeded" not in said or not (into / "bin/libgstreamer-1.0-0.dll").is_file():
        sys.exit(f"GStreamer's installer did not install it (its log: {log})")


def unpack_macos(packages, into, work):
    """Each component of each installer package, its payload (pbzx, then
    cpio) unpacked where the framework's Versions/1.0 would be; static
    libraries left out."""
    into.mkdir(parents=True)
    for package in packages:
        opened = work / f"xar-{package.stem}"
        opened.mkdir()
        run("xar", "-xf", package, cwd=opened)
        for payload in sorted(opened.glob("*.pkg/Payload")):
            stream = subprocess.Popen(["pbzx", "-n", str(payload)], stdout=subprocess.PIPE)
            run("cpio", "-idmu", "--quiet", "--nonmatching", "*.a", stdin=stream.stdout, cwd=into)
            if stream.wait() != 0:
                sys.exit(f"pbzx could not read {payload}")
        shutil.rmtree(opened)


def windows_imports(path):
    text = run("llvm-objdump", "-p", path, capture_output=True, text=True).stdout
    return [line.split(":", 1)[1].strip() for line in text.splitlines() if "DLL Name:" in line]


def macos_loads(path):
    text = run("llvm-otool", "-L", path, capture_output=True, text=True).stdout
    names = [line.split()[0] for line in text.splitlines()[1:] if line.strip()]
    # A library names itself first.
    return [n for n in names if pathlib.Path(n).name != path.name]


def trim(system, installed, out):
    """The plugins of the set, the libraries they and the helper load,
    followed through the tree's own, and the scanner: copied into
    `out/runtime`; what the system has (Windows' DLLs, macOS's frameworks)
    stays the system's."""
    runtime = out / "runtime"
    names = COMMON + SYSTEM[system]
    if system == "windows":
        libraries = installed / "bin"
        plugin = lambda n: installed / f"lib/gstreamer-1.0/libgst{n}.dll"
        scanner = installed / "libexec/gstreamer-1.0/gst-plugin-scanner.exe"
        linked = [libraries / f"lib{n}-0.dll" for n in LINKED]
        loads = windows_imports
        found = {p.name.lower(): p for p in libraries.glob("*.dll")}
        resolve = lambda name: found.get(name.lower())
        into_libraries = runtime / "bin"
    else:
        libraries = installed / "lib"
        plugin = lambda n: installed / f"lib/gstreamer-1.0/libgst{n}.dylib"
        scanner = installed / "libexec/gstreamer-1.0/gst-plugin-scanner"
        linked = [libraries / f"lib{n}.0.dylib" for n in LINKED]
        loads = macos_loads
        resolve = lambda name: (libraries / name.removeprefix("@rpath/")) if name.startswith("@rpath/") else None
        into_libraries = runtime / "lib"
    plugins = [plugin(n) for n in names]
    missing = [p.name for p in [*plugins, scanner, *linked] if not p.is_file()]
    if missing:
        sys.exit(f"Not in GStreamer {lock()[0]}: {', '.join(missing)}")
    needed, queue = set(linked), [*plugins, scanner, *linked]
    while queue:
        for name in loads(queue.pop()):
            library = resolve(name)
            if library is None:
                continue
            if not library.is_file():
                sys.exit(f"{name}, which the set loads, is not in the tree")
            if library not in needed:
                needed.add(library)
                queue.append(library)
    for path, into in [*((p, into_libraries) for p in needed),
                       *((p, runtime / "lib/gstreamer-1.0") for p in plugins),
                       (scanner, runtime / "libexec/gstreamer-1.0")]:
        into.mkdir(parents=True, exist_ok=True)
        copy_binary(system, path, into / path.name)
    return needed


def copy_binary(system, source, target):
    """A library or program of the set: Windows' stripped of its debug
    information; macOS's Apple silicon's slice alone, rewritten by
    llvm-objcopy, which leaves the linker's ad hoc signature in place of
    its maker's (MoltenVK's carries LunarG's, with flags the bundle's
    signing would keep): the bundle signs each the same way."""
    source = source.resolve()
    if system == "windows":
        shutil.copy2(source, target)
        run("x86_64-w64-mingw32-strip", "--strip-debug", target)
        target.chmod(0o755)
        return
    archs = run("llvm-lipo", "-archs", source, capture_output=True, text=True).stdout.split()
    if archs == ["arm64"]:
        run("llvm-objcopy", source, target)
    elif "arm64" in archs:
        thin = target.with_name(target.name + ".thin")
        run("llvm-lipo", source, "-thin", "arm64", "-output", thin)
        run("llvm-objcopy", thin, target)
        thin.unlink()
    else:
        sys.exit(f"{source.name} has no arm64 code: {archs}")
    target.chmod(0o755)


def link_tree(system, installed, out, needed):
    """What the helper is linked against: the pkg-config files of the
    libraries it links and of what they require, no other (GStreamer's
    tree has a SQLite's, which a crate of skwir's would then take), their
    prefix being where they are; and for each library of runtime/ the
    import library (Windows) or the names the linker looks for (macOS)."""
    link = out / "link/lib"
    (link / "pkgconfig").mkdir(parents=True)
    wanted = [n.replace("gst", "gstreamer-", 1) if n != "gstreamer-1.0" else n for n in LINKED]
    seen = set()
    while wanted:
        name = wanted.pop()
        if name in seen:
            continue
        seen.add(name)
        source = installed / "lib/pkgconfig" / f"{name}.pc"
        if not source.is_file():
            sys.exit(f"{name}.pc, which the helper needs, is not in the tree")
        # Without the rpath macOS's add to where they are: the helper would
        # name a folder of this machine, and finds them in the bundle.
        text = re.sub(r"[ \t]*-Wl,-rpath,\S+", "", source.read_text())
        (link / "pkgconfig" / source.name).write_text(text)
        for line in text.splitlines():
            if line.startswith(("Requires:", "Requires.private:")):
                wanted.extend(re.split(r"[\s,]+", re.sub(r"[<>=]+\s*\S+", "", line.split(":", 1)[1])))
        wanted = [w for w in wanted if w]
    if system == "windows":
        # Under the name Zig's linker looks for (`name.lib`), not MinGW's
        # (`libname.dll.a`); the tree's static `libname.a` never.
        for imported in (installed / "lib").glob("lib*.dll.a"):
            name = imported.name.removeprefix("lib").removesuffix(".dll.a")
            shutil.copy2(imported, link / f"{name}.lib")
        return
    carried = {p.resolve().name for p in needed}
    for entry in (installed / "lib").glob("*.dylib"):
        if entry.resolve().name in carried:
            (link / entry.name).symlink_to(f"../../runtime/{FRAMEWORK}/lib/{entry.resolve().name}")


def main():
    if len(sys.argv) != 2 or sys.argv[1] not in ("x86_64-pc-windows-gnu", "aarch64-apple-darwin"):
        sys.exit(f"usage: {sys.argv[0]} <x86_64-pc-windows-gnu|aarch64-apple-darwin>")
    target = sys.argv[1]
    system = "windows" if "windows" in target else "macos"
    tools = ["Xvfb", "x86_64-w64-mingw32-strip", "llvm-objdump"] if system == "windows" else ["xar", "pbzx", "cpio", "llvm-lipo", "llvm-otool", "llvm-objcopy"]
    if (system == "windows" and not os.environ.get("SKWIR_WINE_WOW64")) or not all(shutil.which(t) for t in tools):
        os.execvp("nix", ["nix", "develop", f"{ROOT}#{system}", "--command", sys.argv[0], *sys.argv[1:]])
    version, entries = lock()
    files = [(p, s) for p, s in entries if p.startswith("windows/" if system == "windows" else "osx/")]
    key = hashlib.sha256(LOCK.read_bytes() + pathlib.Path(__file__).read_bytes() + target.encode()).hexdigest()[:16]
    out = CACHE / f"{target}-{version}-{key}"
    if (out / "done").exists():
        print(out)
        return
    work = CACHE / f".{target}-{version}-{key}"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    downloads = [fetch(p, s, CACHE / "downloads") for p, s in files]
    installed = work / "installed"
    print(f"GStreamer {version} for {target}: installing and trimming (minutes, once)", file=sys.stderr)
    if system == "windows":
        install_windows(downloads[0], installed, work)
    else:
        unpack_macos(downloads, installed, work)
    stage = work / "out"
    needed = trim(system, installed, stage)
    link_tree(system, installed, stage, needed)
    licenses(system, installed, stage, needed)
    if system == "macos":
        framework(stage / "runtime", version)
    (stage / "done").write_text(f"{version}\n")
    # The builds of other releases or recipes let go, with what a failed one left.
    for old in [*CACHE.glob(f"{target}-*"), *CACHE.glob(f".{target}-*")]:
        if old != work and old.is_dir():
            shutil.rmtree(old)
    stage.rename(out)
    shutil.rmtree(work)
    print(out)


# Whose each library of runtime/ is, as GStreamer's build names its
# components (share/licenses, versions.txt): a library none names stops
# the recipe, so nothing is carried without its notice. GStreamer's own
# modules are under the core's licence (LGPL); good, ugly and gst-libav
# have no folder of their own.
OWNERS = [
    (r"libgst.*", ["gstreamer-1.0", "gst-plugins-base-1.0", "gst-plugins-good-1.0",
                   "gst-plugins-bad-1.0", "gst-plugins-ugly-1.0", "gst-libav-1.0"]),
    (r"lib(avcodec|avformat|avfilter|avutil|swresample|swscale)", ["ffmpeg"]),
    (r"lib(glib|gio|gmodule|gobject|gthread)", ["glib"]),
    (r"lib(gcc_s_seh|stdc\+\+|winpthread)", ["mingw-runtime"]),
    (r"libdav1d", ["dav1d"]), (r"libass", ["libass"]), (r"libbz2", ["bzip2"]),
    (r"libcairo.*", ["cairo"]), (r"libexpat", ["expat"]), (r"libffi", ["libffi"]),
    (r"libFLAC", ["flac"]), (r"libfontconfig", ["fontconfig"]), (r"libfreetype", ["freetype"]),
    (r"libfribidi", ["fribidi"]), (r"libharfbuzz.*", ["harfbuzz"]), (r"libiconv", ["libiconv"]),
    (r"libintl", ["proxy-libintl"]), (r"libogg", ["libogg"]), (r"libopus", ["opus"]),
    (r"liborc", ["orc"]), (r"libpango.*", ["pango"]), (r"libpcre2", ["pcre2"]),
    (r"libpixman", ["pixman"]), (r"libpng16", ["libpng"]), (r"libvorbis.*", ["libvorbis"]),
    (r"libz", ["zlib"]), (r"libMoltenVK", ["moltenvk"]),
]


def owners(path):
    """The components a library of runtime/ belongs to, by its name less
    its version and extension."""
    stem = re.sub(r"([-.][0-9].*)?\.(dll|dylib)$", "", path.name)
    for pattern, components in OWNERS:
        if re.fullmatch(pattern, stem):
            return components
    return None


# Where macOS's runtime is, in its framework bundle.
FRAMEWORK = "GStreamer.framework/Versions/1.0"


def framework(runtime, version):
    """macOS's runtime made a framework bundle, as a bundle's Frameworks
    holds code (rcodesign signs it, each library and plugin in it): the
    versioned layout, its Info.plist, libgstreamer its main binary."""
    inside = runtime / FRAMEWORK
    inside.mkdir(parents=True)
    for entry in ("lib", "libexec"):
        (runtime / entry).rename(inside / entry)
    (inside / "Resources").mkdir()
    (inside / "GStreamer").symlink_to("lib/libgstreamer-1.0.0.dylib")
    (runtime / "GStreamer.framework/Versions/Current").symlink_to("1.0")
    for entry in ("GStreamer", "Resources"):
        (runtime / "GStreamer.framework" / entry).symlink_to(f"Versions/Current/{entry}")
    (inside / "Resources/Info.plist").write_text(f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>org.freedesktop.gstreamer</string>
    <key>CFBundleName</key><string>GStreamer</string>
    <key>CFBundleExecutable</key><string>GStreamer</string>
    <key>CFBundlePackageType</key><string>FMWK</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleVersion</key><string>{version}</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
</dict>
</plist>
""")


def licenses(system, installed, out, needed):
    """Each carried component's notices from GStreamer's tree, the
    versions of those components, and where their sources are."""
    runtime = out / "runtime"
    carried = {"gstreamer-1.0", "gst-plugins-rs"}
    unowned = []
    for path in sorted(p for p in runtime.rglob("*") if p.is_file()):
        if path.suffix not in (".dll", ".dylib") or path.parent.name == "gstreamer-1.0":
            # Plugins and the scanner: GStreamer's modules, gst-plugins-rs's dav1d.
            continue
        found = owners(path)
        if found is None:
            unowned.append(path.name)
        else:
            carried.update(found)
    if unowned:
        sys.exit(f"No component known for {', '.join(unowned)}: add it to OWNERS in {__file__}")
    into = out / "licenses/gstreamer"
    notices = installed / "share/licenses"
    for component in sorted(carried):
        if (notices / component).is_dir():
            shutil.copytree(notices / component, into / component)
    versions = (installed / "share/versions.txt").read_text().splitlines()
    kept = [line for line in versions if line.split(" ", 1)[0] in carried]
    (into / "VERSIONS").write_text("\n".join(kept) + "\n")
    version = lock()[0]
    (into / "SOURCES").write_text(
        f"These libraries are GStreamer {version}'s own build for {system}, unchanged but for\n"
        "their debug information (Windows) and the other architecture (macOS) left out.\n"
        f"GStreamer's sources: https://gstreamer.freedesktop.org/src/ (each module's {version}).\n"
        f"Every other component's, as the build took them: https://gitlab.freedesktop.org/gstreamer/cerbero/-/tree/{version}\n"
        "(its recipes name each source and its version, as VERSIONS lists them).\n")


if __name__ == "__main__":
    main()
