#!/usr/bin/env python3
"""Build the pinned playback runtime against the glibc compatibility baseline.

Jammy's GStreamer 1.20 races playbin3's stream selection, so the AppImage
carries GStreamer built here, every part of it at one version: the core,
base, good, bad, ugly and gst-libav, against Jammy's libraries (its FFmpeg
4.4 among them), none of Ubuntu's 1.20 plugins beside them (ADR-052
amended). libva is built here too, the newest, which loads a host's VA
drivers old and new where Jammy's refused newer ones; and PulseAudio's
client library: Jammy's 15.99
stopped a film for good after a pause and a seek while paused or a play
and pause at once, against every server, its own included; 17.0's plays
on against them all, Jammy's too. Good, bad and
ugly bring ADR-076's set alone, what the media helper plays with, as
Windows' and macOS's packages have it, with Linux's own decoders (VA-API,
NVDEC) and audio output (PulseAudio; ALSA is base's): every other feature
is off, and those named are required, so a missing library stops the
build rather than leaving a plugin out. Base builds no network source.
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
# Built in this order, each against those before it: libva before bad's
# VA-API decoders (and in place of Jammy's, which FFmpeg's libraries link),
# PulseAudio's client library before good, whose pulsesink links it.
PARTS = ("orc", "libva", "gstreamer", "gst-plugins-base", "pulseaudio", "gst-plugins-good",
         "gst-plugins-bad", "gst-plugins-ugly", "gst-libav")
# Where a host keeps its VA-API drivers: Debian's and Ubuntu's, Fedora's,
# Arch's, Gentoo's, NixOS's. libva tries each, in order.
VA_DRIVERS = ("/usr/lib/x86_64-linux-gnu/dri", "/usr/lib64/dri", "/usr/lib/dri",
              "/usr/lib64/va/drivers", "/run/opengl-driver/lib/dri")
# What a library built beside GStreamer looks for on the machine it runs
# on is the host's, never under PREFIX, which exists only here: the
# system's settings (/etc/pulse/client.conf, /etc/libva.conf), the
# system-wide sound server's socket (/var/run/pulse), the sound server
# a client starts, its messages' translations. check.py fails on a path
# of PREFIX left in a staged library.
HOST = ("-Dsysconfdir=/etc", "-Dlocalstatedir=/var")
# The parts that are not GStreamer's, and their own options.
BESIDE = {
    # DRM and X11, which FFmpeg's libraries link; no GLX or Wayland. Newer
    # than any driver, it loads old and new alike, where Jammy's 2.14
    # refused drivers built for a later one.
    "libva": HOST + ("-Ddriverdir=" + ":".join(VA_DRIVERS), "-Dwith_x11=yes", "-Dwith_glx=no",
                     "-Dwith_wayland=no", "-Denable_docs=false"),
    # The client library alone, which nothing else of it serves: no daemon
    # to spawn, no X11, D-Bus or systemd.
    "pulseaudio": HOST + ("-Dbindir=/usr/bin", "-Dlocaledir=/usr/share/locale",
                          "-Ddaemon=false", "-Dclient=true", "-Ddoxygen=false", "-Dman=false",
                          "-Dtests=false", "-Dauto_features=disabled",
                          "-Dbashcompletiondir=no", "-Dzshcompletiondir=no"),
}
# ADR-076's set from good, bad and ugly (tools/package/gstreamer.py's
# COMMON), with Linux's decoders and audio output; bad's AV1 decoder is
# aom's, its dav1d being gst-plugins-rs's since 1.26, and gst-libav takes
# no decoder of FFmpeg's that wraps another library.
FEATURES = {
    "gst-plugins-good": ("autodetect", "replaygain", "audiofx", "spectrum", "videofilter", "matroska",
                         "isomp4", "avi", "flv", "wavparse", "audioparsers", "id3demux", "apetag",
                         "flac", "vpx", "pulse"),
    "gst-plugins-bad": ("aiff", "mpegtsdemux", "mpegdemux", "videoparsers", "aom", "assrender",
                        "dvdspu", "dvbsuboverlay", "va", "nvcodec"),
    "gst-plugins-ugly": ("asfdemux", "dvdsub"),
}


def main():
    inputs = json.loads(LOCK.read_text())
    names = [next(n for n in inputs if n.startswith(part + "-") and n.endswith((".tar.xz", ".tar.bz2")))
             for part in PARTS]
    names.append(next(n for n in inputs if n.startswith("meson-") and n.endswith(".whl")))
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
        source = BUILD / name.removesuffix(".tar.xz").removesuffix(".tar.bz2")
        subprocess.run(["tar", "--no-same-owner", "-xf", str(pathlib.Path("/inputs") / name), "-C", str(BUILD)], check=True)
        build = source.with_name(source.name + "-build")
        part = next(p for p in PARTS if name.startswith(p + "-"))
        options = ["--prefix=" + str(PREFIX), "--libdir=lib", "--buildtype=release", "--wrap-mode=nofallback"]
        # Each part's own options (meson stops at one it does not have).
        if part in BESIDE:
            build_part(command, env, source, build, options + list(BESIDE[part]))
            continue
        options += ["-Dtests=disabled"]
        if part not in ("gst-plugins-ugly", "gst-libav"):
            options += ["-Dexamples=disabled"]
        if part == "orc":
            options += ["-Dbenchmarks=disabled", "-Dhotdoc=disabled", "-Dorc-test=disabled"]
        else:
            options += ["-Ddoc=disabled"]
        if part not in ("orc", "gst-libav"):
            options += ["-Dnls=disabled"]
        if part in ("gstreamer", "gst-plugins-base", "gst-plugins-bad"):
            options += ["-Dintrospection=disabled"]
        if part == "gstreamer":
            options += ["-Dbenchmarks=disabled"]
        elif part == "gst-plugins-base":
            # Replace the distribution's entire base plugin package,
            # including codecs whose development headers are optional
            # upstream. Renamed plugins must never coexist with 1.20.
            options += ["-D" + feature + "=enabled" for feature in
                        ("alsa", "cdparanoia", "libvisual", "ogg", "opus", "pango",
                         "theora", "vorbis", "xvideo", "orc")]
            # No network source (ADR-052, ADR-020): TCP's, and GIO's, which
            # reads any scheme GIO knows. The helper plays file:// alone,
            # which the core's filesrc reads, and types by its bytes.
            options += ["-D" + feature + "=disabled" for feature in ("tcp", "gio", "gio-typefinder")]
        elif part in FEATURES:
            options += ["-Dauto_features=disabled", "-Dorc=enabled"]
            options += ["-D" + feature + "=enabled" for feature in FEATURES[part]]
        build_part(command, env, source, build, options)
    marker.write_text(key)


def build_part(command, env, source, build, options):
    """`source` configured with `options`, compiled and installed, the
    loader told of what it installed."""
    subprocess.run(command + ["setup", str(build), str(source)] + options, env=env, check=True)
    subprocess.run(command + ["compile", "-C", str(build), "-j", os.environ.get("CARGO_BUILD_JOBS", "4")],
                   env=env, check=True)
    subprocess.run(command + ["install", "-C", str(build), "--no-rebuild"], env=env, check=True)
    subprocess.run(["ldconfig"], check=True)


if __name__ == "__main__":
    main()
