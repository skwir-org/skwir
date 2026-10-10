#!/bin/sh
# Run the Windows build's tests under Wine (ADR-035), from Linux: built
# with Zig through cargo-zigbuild, run by nextest with Wine as the runner.
# Wine answers for the logic, never for Windows itself.
#
# Nothing reaches the user's own: a Wine prefix of its own under
# ~/.cache/skwir-tests, no display (Wine opens no window in the session),
# and nextest's XDG isolation (Wine's Recycle Bin is the freedesktop trash
# of XDG_DATA_HOME). Arguments go to nextest (`-p skwir-core`, a filter).
set -eu
if ! command -v wine >/dev/null; then
    exec nix develop .#windows --command "$0" "$@"
fi
scratch="$HOME/.cache/skwir-tests/wine"
mkdir -p "$scratch"
export WINEPREFIX="$scratch/prefix"
export WINEDEBUG=-all
# No Mono or Gecko: their installers would ask on first start.
export WINEDLLOVERRIDES="mscoree,mshtml="
unset DISPLAY WAYLAND_DISPLAY
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine
# Tests that need what Wine lacks (its shell does not trash) skip, saying so.
export SKWIR_TEST_UNDER_WINE=1
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/cross}"
# The type database a package carries, where it puts it, beside the
# program (ADR-037): in `share` above the test programs.
tools/package/mime-database.sh "$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/debug/share/mime" >/dev/null
# Not incremental, as `just check-cross`: its caches only fill the disk.
export CARGO_INCREMENTAL=0
REAL_CARGO="$(command -v cargo)"
export REAL_CARGO
CARGO="$(pwd)/tools/cross/zig-nextest" exec cargo-zigbuild test --target x86_64-pc-windows-gnu "$@"
