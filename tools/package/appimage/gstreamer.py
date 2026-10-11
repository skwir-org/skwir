#!/usr/bin/env python3
"""Build the pinned playback runtime against the glibc compatibility baseline.

Jammy's GStreamer 1.20 races playbin3's stream selection. A current core
and base plugins keep the existing player protocol and the old libc ABI.
Other distribution plugins use GStreamer's stable 1.x plugin ABI.
"""

import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import zipfile

PREFIX = pathlib.Path("/opt/skwir-gstreamer")
BUILD = pathlib.Path("/build/gstreamer")
LOCK = pathlib.Path("tools/package/appimage/downloads.json")


def main():
    inputs = json.loads(LOCK.read_text())
    names = [next(n for n in inputs if n.startswith(component + "-") and n.endswith(suffix))
             for component, suffix in (("orc", ".tar.xz"), ("gstreamer", ".tar.xz"),
                                       ("gst-plugins-base", ".tar.xz"), ("meson", ".whl"))]
    glib = subprocess.check_output(["pkg-config", "--modversion", "glib-2.0"])
    key = hashlib.sha256(pathlib.Path(__file__).read_bytes() + glib
                         + LOCK.with_name("build-deps").read_bytes()
                         + json.dumps({n: inputs[n] for n in names}, sort_keys=True).encode()).hexdigest()
    marker = PREFIX / ".build-key"
    if marker.exists() and marker.read_text() == key:
        return
    shutil.rmtree(PREFIX, ignore_errors=True)
    shutil.rmtree(BUILD, ignore_errors=True)
    BUILD.mkdir(parents=True)
    meson = BUILD / "meson"
    with zipfile.ZipFile(pathlib.Path("/inputs") / names[-1]) as wheel:
        wheel.extractall(meson)
    command = [sys.executable, "-m", "mesonbuild.mesonmain"]
    env = dict(os.environ, PYTHONPATH=str(meson), PKG_CONFIG_PATH=str(PREFIX / "lib/pkgconfig"),
               PATH=str(PREFIX / "bin") + ":" + os.environ["PATH"])
    # Confined to this build root: build tools and ldd must resolve the
    # new runtime, including orcc before compiling the base plugins.
    pathlib.Path("/etc/ld.so.conf.d/00-skwir-gstreamer.conf").write_text(str(PREFIX / "lib") + "\n")
    for name in names[:-1]:
        source = BUILD / name.removesuffix(".tar.xz")
        subprocess.run(["tar", "--no-same-owner", "-xJf", str(pathlib.Path("/inputs") / name), "-C", str(BUILD)], check=True)
        build = source.with_name(source.name + "-build")
        options = ["--prefix=" + str(PREFIX), "--libdir=lib", "--buildtype=release", "--wrap-mode=nofallback",
                   "-Dtests=disabled", "-Dexamples=disabled"]
        if name.startswith("orc-"):
            options += ["-Dbenchmarks=disabled", "-Dhotdoc=disabled", "-Dorc-test=disabled"]
        else:
            options += ["-Dintrospection=disabled", "-Ddoc=disabled", "-Dnls=disabled"]
        if name.startswith("gstreamer-"):
            options += ["-Dbenchmarks=disabled"]
        elif name.startswith("gst-plugins-base-"):
            # Replace the distribution's entire base plugin package,
            # including codecs whose development headers are optional
            # upstream. Renamed plugins must never coexist with 1.20.
            options += ["-D" + feature + "=enabled" for feature in
                        ("alsa", "cdparanoia", "libvisual", "ogg", "opus", "pango",
                         "theora", "vorbis", "xvideo", "orc")]
        subprocess.run(command + ["setup", str(build), str(source)] + options, env=env, check=True)
        subprocess.run(command + ["compile", "-C", str(build), "-j", os.environ.get("CARGO_BUILD_JOBS", "4")], env=env, check=True)
        subprocess.run(command + ["install", "-C", str(build), "--no-rebuild"], env=env, check=True)
        subprocess.run(["ldconfig"], check=True)
    marker.write_text(key)


if __name__ == "__main__":
    main()
