#!/bin/sh
# In the Ubuntu root: compile, stage, bundle and verify before publishing.
#
#     tools/package/appimage/build.sh <source date, seconds since 1970>
set -eu
source_date="$1"
export CARGO_TARGET_DIR=/build/target
export CARGO_INCREMENTAL=0
python3 tools/package/appimage/gstreamer.py
# A film for the bundle's check (check.py's check_film), made by the
# GStreamer built here: twelve seconds of VP8 and Vorbis in Matroska.
/opt/skwir-gstreamer/bin/gst-launch-1.0 -q \
    videotestsrc num-buffers=360 ! video/x-raw,width=320,height=240,framerate=30/1 ! vp8enc deadline=1 ! queue ! mux. \
    audiotestsrc num-buffers=360 samplesperbuffer=1600 ! audio/x-raw,rate=48000,channels=2 ! audioconvert ! vorbisenc ! queue ! mux. \
    matroskamux name=mux ! filesink location=/out/check-film.mkv
. tools/build/order-link-env.sh
# SQLite built in: Ubuntu 22.04's is older than rusqlite needs, and the
# systems the AppImage runs on may be as old (ADR-022).
# shellcheck disable=SC2046 # the shared list is one package per line
cargo build --locked --profile dist --features skwir/bundled-sqlite $(sed 's/^/-p /' tools/package/programs)
# What ships has what a start runs laid out together (ADR-128).
tools/build/link-ordered --check "${CARGO_TARGET_DIR:-target}/dist/skwir"
tools/package/pdfium.sh linux-x64 target/pdfium
python3 tools/package/fonts.py target/fonts
for tool in linuxdeploy appimagetool; do
    rm -rf "/opt/$tool"
    mkdir -p "/opt/$tool"
    (cd "/opt/$tool" && "/inputs/$tool.AppImage" --appimage-extract >/dev/null)
done
python3 tools/package/appimage/stage.py
rm -f /build/AppDir/AppRun
install -m755 tools/package/appimage/AppRun /build/AppDir/AppRun
desktop-file-validate /build/AppDir/usr/share/applications/org.skwir.skwir.desktop
tools/package/linux/l10n.sh installed /build/AppDir/usr/share
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
site="$(sed -n 's/^homepage = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
image="/build/skwir-$version-x86_64.AppImage"
# What AppImage's updaters read (ADR-142): the image says where the latest
# release's zsync file is, on skwir.org, which sends it on to the release
# (its `/latest/` redirect); that file, named without a version, says
# where this image is, by an address of its own (`/files/`), never
# relative to a redirect.
zsync=skwir-x86_64.AppImage.zsync
ARCH=x86_64 /opt/appimagetool/squashfs-root/AppRun \
    --runtime-file /inputs/runtime-x86_64 -u "zsync|$site/latest/$zsync" /build/AppDir "$image"
# appimagetool's own zsync file, named after the image, relative: not
# what the update information names.
rm -f "$image.zsync" "./$(basename "$image").zsync"
zsyncmake -u "$site/files/v$version/$(basename "$image")" -o "/build/$zsync" "$image"
# The same usr tree as a tarball, for a system without FUSE or a folder
# of the user's choosing: the program at its top, none of the AppImage's
# own files. Names sorted, owned by root and dated by the source, so the
# archive depends on the tree alone.
tarball="skwir-$version-linux-x86_64"
rm -rf /build/tarball
mkdir -p "/build/tarball/$tarball"
cp -al /build/AppDir/usr "/build/tarball/$tarball/usr"
# How the copy came (ADR-142): the tarball's own word, not the
# AppImage's, which the link would share.
rm "/build/tarball/$tarball/usr/share/skwir/channel"
install -m644 tools/package/channel/tarball "/build/tarball/$tarball/usr/share/skwir/channel"
ln -s usr/bin/skwir "/build/tarball/$tarball/skwir"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$source_date" \
    -C /build/tarball -cf - "$tarball" | gzip -n > "/build/$tarball.tar.gz"
# The caller checks these in a separate runtime-only root.
install -m755 "$image" "/out/$(basename "$image")"
install -m644 "/build/$tarball.tar.gz" "/out/$tarball.tar.gz"
install -m644 "/build/$zsync" "/out/$zsync"
for artifact in "$(basename "$image")" "$tarball.tar.gz"; do
    (cd /out && sha256sum "$artifact" > "$artifact.sha256")
done
