#!/usr/bin/env bash
# gpui_windows's tests of Skwir's patches (the mesh pass, 0033), built for
# Windows and run under Wine in check.py's copy of vendor/zed. Wine's
# Direct3D 11 draws on OpenGL, through Mesa's llvmpipe on an X display of
# its own: nothing reaches the session. Wine's prefix is this script's
# alone (made on the first run), its server stopped before the display.
# Arguments go to the test binary (a filter, `--nocapture`).
set -eu
if ! command -v wine >/dev/null; then
    exec nix develop .#windows --command "$0" "$@"
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
PYTHONPATH=tools/gpui python3 -c 'import check; check.copy()'
# The flake's Mesa: the system's may want a newer glibc than Wine's.
mesa="$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#mesa)"
xorg="$(nix build --no-link --print-out-paths --inputs-from . nixpkgs#xorg.xorgserver)"
display=:91
"$xorg/bin/Xvfb" "$display" -screen 0 640x480x24 >/dev/null 2>&1 &
xvfb=$!
unset WAYLAND_DISPLAY
export DISPLAY="$display"
export __EGL_VENDOR_LIBRARY_FILENAMES="$mesa/share/glvnd/egl_vendor.d/50_mesa.json"
export WINEPREFIX="$HOME/.cache/skwir-tests/wine/gpui-prefix"
. "$root/tools/cross/wine-env.sh"
trap 'wineserver -k; kill "$xvfb"' EXIT
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine CARGO_INCREMENTAL=0
cd "$HOME/.cache/skwir-tests/gpui-check/zed"
cargo-zigbuild test -p gpui_windows --target x86_64-pc-windows-gnu --lib \
    --features test-support --target-dir "$root/target/gpui-check-windows" \
    -- directx_meshes "$@"
