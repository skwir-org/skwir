#!/usr/bin/env python3
"""Build libheif, with libde265 its HEVC decoder, for the HEIF helper.

Jammy's libheif (1.12) has no heif_init, which the helper calls, nor the
calls that decode a picture's tiles one by one (1.19); its libde265 is
1.0.8 (ADR-148). The AppImage carries the newest of both, built here
against the glibc baseline: libde265 for HEIC's HEVC; for AVIF's AV1,
Jammy's libaom, which GStreamer's AV1 decoder links already (one copy of
it carried). Both decoders are built into libheif, not loaded as plugins:
no plugin folder of this build's is looked for on the user's machine.
Nothing else of libheif's is built: no encoder, tool or gdk-pixbuf
loader. Installed in a prefix of its own, first on the loader's path
here, so the helper links and stage.py carries it rather than Jammy's
1.12, which the Debian package's build dependencies bring.
"""

import hashlib
import json
import os
import pathlib
import shutil
import subprocess

PREFIX = pathlib.Path("/opt/skwir-heif")
BUILD = pathlib.Path("/build/heif")
LOCK = pathlib.Path("tools/package/appimage/downloads.json")
# Built in this order, libheif against the libde265 before it.
PARTS = {
    # The library alone: no player (SDL), decoding or encoding tool.
    "libde265": ("-DENABLE_SDL=OFF", "-DENABLE_DECODER=OFF", "-DENABLE_ENCODER=OFF"),
    # Decoders built in, no plugin loading; every encoder and other codec
    # off, and what is not decoding (examples, tests, gdk-pixbuf).
    "libheif": ("-DENABLE_PLUGIN_LOADING=OFF",
                "-DWITH_LIBDE265=ON", "-DWITH_LIBDE265_PLUGIN=OFF",
                "-DWITH_AOM_DECODER=ON", "-DWITH_AOM_DECODER_PLUGIN=OFF",
                "-DWITH_AOM_ENCODER=OFF", "-DWITH_X265=OFF", "-DWITH_X264=OFF",
                "-DWITH_OpenH264_DECODER=OFF", "-DWITH_DAV1D=OFF", "-DWITH_LIBSHARPYUV=OFF",
                "-DWITH_EXAMPLES=OFF", "-DWITH_GDK_PIXBUF=OFF", "-DBUILD_TESTING=OFF"),
}


def main():
    inputs = json.loads(LOCK.read_text())
    names = [next(n for n in inputs if n.startswith(part + "-") and n.endswith(".tar.gz")) for part in PARTS]
    key = hashlib.sha256(pathlib.Path(__file__).read_bytes()
                         + LOCK.with_name("build-deps").read_bytes()
                         + json.dumps({n: inputs[n] for n in names}, sort_keys=True).encode()).hexdigest()
    marker = PREFIX / ".build-key"
    if marker.exists() and marker.read_text() == key:
        return
    shutil.rmtree(PREFIX, ignore_errors=True)
    shutil.rmtree(BUILD, ignore_errors=True)
    BUILD.mkdir(parents=True)
    env = dict(os.environ, PKG_CONFIG_PATH=str(PREFIX / "lib/pkgconfig"))
    # Found first by the loader in this build root (ldd, stage.py), ahead
    # of Jammy's libheif.so.1 of the same name.
    pathlib.Path("/etc/ld.so.conf.d/00-skwir-heif.conf").write_text(str(PREFIX / "lib") + "\n")
    jobs = os.environ.get("CARGO_BUILD_JOBS", "4")
    for name, (part, options) in zip(names, PARTS.items()):
        subprocess.run(["tar", "--no-same-owner", "-xzf", str(pathlib.Path("/inputs") / name), "-C", str(BUILD)],
                       check=True)
        source = BUILD / name.removesuffix(".tar.gz")
        build = source.with_name(source.name + "-build")
        subprocess.run(["cmake", "-S", str(source), "-B", str(build), "-DCMAKE_BUILD_TYPE=Release",
                        "-DCMAKE_INSTALL_PREFIX=" + str(PREFIX), "-DCMAKE_INSTALL_LIBDIR=lib",
                        "-DCMAKE_PREFIX_PATH=" + str(PREFIX), *options], env=env, check=True)
        subprocess.run(["cmake", "--build", str(build), "-j", jobs], env=env, check=True)
        subprocess.run(["cmake", "--install", str(build)], env=env, check=True)
        subprocess.run(["ldconfig"], check=True)
        print(f"{part}: built from {name}", flush=True)
    marker.write_text(key)


if __name__ == "__main__":
    main()
