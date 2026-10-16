#!/bin/sh
# In the Ubuntu root: compile, stage, bundle and verify before publishing.
#
#     tools/package/appimage/build.sh <source date, seconds since 1970>
set -eu
source_date="$1"
export CARGO_TARGET_DIR=/build/target
export CARGO_INCREMENTAL=0
python3 tools/package/appimage/gstreamer.py
# shellcheck disable=SC2046 # the shared list is one package per line
cargo build --locked --profile dist $(sed 's/^/-p /' tools/package/programs)
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
# The same usr tree as a tarball, for a system without FUSE or a folder
# of the user's choosing: the program at its top, none of the AppImage's
# own files. Names sorted, owned by root and dated by the source, so the
# archive depends on the tree alone.
tarball="skwir-$version-linux-x86_64"
rm -rf /build/tarball
mkdir -p "/build/tarball/$tarball"
cp -al /build/AppDir/usr "/build/tarball/$tarball/usr"
ln -s usr/bin/skwir "/build/tarball/$tarball/skwir"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$source_date" \
    -C /build/tarball -cf - "$tarball" | gzip -n > "/build/$tarball.tar.gz"
# The caller checks these in a separate runtime-only root.
install -m755 "$image" "/out/$(basename "$image")"
install -m644 "/build/$tarball.tar.gz" "/out/$tarball.tar.gz"
for artifact in "$(basename "$image")" "$tarball.tar.gz"; do
    (cd /out && sha256sum "$artifact" > "$artifact.sha256")
done
