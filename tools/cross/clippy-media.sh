#!/bin/sh
# Lint the media helper for Windows or macOS, which their packages carry
# (ADR-076): it links GStreamer's own build, made once by
# tools/package/gstreamer.py (in the `windows` or `macos` shell it asks
# for) and kept in the build cache, and pkg-config is asked for the
# target's files alone, as tools/package/windows.sh and macos.sh build it.
# Arguments after the target go to clippy.
set -eu
target="$1"
shift
cd "$(dirname "$0")/../.."
gstreamer="$(tools/package/gstreamer.py "$target")"
variable="$(echo "$target" | tr - _)"
export PKG_CONFIG_ALLOW_CROSS=1
export "PKG_CONFIG_PATH_$variable=$gstreamer/link/lib/pkgconfig"
export "PKG_CONFIG_LIBDIR_$variable=$gstreamer/link/lib/pkgconfig"
CARGO_TARGET_DIR=target/cross CARGO_INCREMENTAL=0 exec cargo-zigbuild clippy \
    --target "$target" --all-targets --no-deps -p skwir-media-worker -- -D warnings "$@"
