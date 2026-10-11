#!/bin/sh
# In the Ubuntu root: compile, stage, bundle and verify before publishing.
set -eu
export CARGO_TARGET_DIR=/build/target
export CARGO_INCREMENTAL=0
python3 tools/package/appimage/gstreamer.py
# Notices read the locked graph offline, including target-specific crates
# that this machine does not compile.
cargo fetch --locked
# shellcheck disable=SC2046 # the shared list is one package per line
cargo build --locked --profile dist $(sed 's/^/-p /' tools/package/programs)
tools/package/notices.py
tools/package/pdfium.sh linux-x64 target/pdfium
for tool in linuxdeploy appimagetool; do
    rm -rf "/opt/$tool"
    mkdir -p "/opt/$tool"
    (cd "/opt/$tool" && "/inputs/$tool.AppImage" --appimage-extract >/dev/null)
done
python3 tools/package/appimage/stage.py
rm -f /build/AppDir/AppRun
install -m755 tools/package/appimage/AppRun /build/AppDir/AppRun
desktop-file-validate /build/AppDir/usr/share/applications/org.skwir.skwir.desktop
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
image="/build/skwir-$version-x86_64.AppImage"
ARCH=x86_64 /opt/appimagetool/squashfs-root/AppRun \
    --runtime-file /inputs/runtime-x86_64 /build/AppDir "$image"
# The caller checks this in a separate runtime-only root.
install -m755 "$image" "/out/$(basename "$image")"
(cd /out && sha256sum "$(basename "$image")" > "$(basename "$image").sha256")
