#!/bin/sh
# Run the Windows build's tests under Wine (ADR-035), from Linux: built
# with Zig through cargo-zigbuild, run by nextest with Wine as the runner.
# Wine answers for the logic, never for Windows itself.
#
# Nothing reaches the user's own: a Wine prefix of its own under
# ~/.cache/skwir-tests that writes nothing into the desktop's menus or
# types (wine-env.sh), no display (Wine opens no window in the session),
# and nextest's XDG isolation (Wine's Recycle Bin is the freedesktop trash
# of XDG_DATA_HOME). Arguments go to nextest (`-p skwir-core`, a filter);
# given none, every default member's tests, one crate at a time from one
# build (test-windows-each.sh, run here in this environment).
set -eu
if ! command -v wine >/dev/null; then
    exec nix develop .#windows --command "$0" "$@"
fi
scratch="$HOME/.cache/skwir-tests/wine"
mkdir -p "$scratch"
export WINEPREFIX="$scratch/prefix"
. tools/cross/wine-env.sh
# Several runs may share the prefix and the target folder (agents, two
# terminals): what this one puts there is put in place at once, by a
# rename, and only when it changed, so a run already testing never reads
# a file or a folder half made.
#
# Windows keeps its fonts in its Fonts folder, where a program that reads
# their files finds them (printing does, ADR-111); Wine keeps its own in
# its data folder, leaving the prefix's empty. Its TrueType faces (Tahoma
# among them, as Windows has) are put where Windows keeps them.
wine_fonts="$(dirname "$(readlink -f "$(command -v wine)")")/../share/wine/fonts"
fonts="$WINEPREFIX/drive_c/windows/Fonts"
mkdir -p "$fonts"
for font in "$wine_fonts"/*.ttf; do
    [ -f "$font" ] || continue
    name="$(basename "$font")"
    if ! cmp -s "$font" "$fonts/$name"; then
        cp "$font" "$fonts/.$name.$$"
        mv -f "$fonts/.$name.$$" "$fonts/$name"
    fi
done
unset DISPLAY WAYLAND_DISPLAY
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER=wine
# Tests that need what Wine lacks (its shell does not trash) skip, saying so.
export SKWIR_TEST_UNDER_WINE=1
# rclone's own Windows release, pinned, for its remote control and
# configuration as Windows has them (ADR-086); without it they skip.
if rclone="$(tools/test/rclone-windows.sh)"; then
    SKWIR_TEST_RCLONE="$(winepath -w "$rclone" 2>/dev/null)"
    export SKWIR_TEST_RCLONE
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/cross}"
# The type database a package carries, where it puts it, beside the
# program (ADR-037): in `share` above the test programs. Made aside, then
# renamed into place, or exchanged with the one there (a rename too).
share="$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/debug/share"
mkdir -p "$share"
made="$(mktemp -d "$share/.mime.XXXXXX")"
trap 'rm -rf "${made:?}"' EXIT
tools/package/mime-database.sh "$made/mime" >/dev/null
if ! diff -rq "$made/mime" "$share/mime" >/dev/null 2>&1; then
    mv -T "$made/mime" "$share/mime" 2>/dev/null ||
        mv -T --exchange "$made/mime" "$share/mime"
fi
rm -rf "${made:?}"
trap - EXIT
# Not incremental, as `just check-cross`: its caches only fill the disk.
export CARGO_INCREMENTAL=0
# Line tables alone for debugging: `release-check` builds this at every
# release, and a test program with the whole of its debug information
# was 1.4 GB, 83 % of it DWARF (2026-11-25), one set of them 59 GB. A
# failure's backtrace still names each file and line.
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
REAL_CARGO="$(command -v cargo)"
export REAL_CARGO
# What test-windows-each.sh asks to know it runs here.
export SKWIR_WINE_HARNESS=1
if [ "$#" -eq 0 ]; then
    exec tools/cross/test-windows-each.sh
fi
CARGO="$(pwd)/tools/cross/zig-nextest" exec cargo-zigbuild test --target x86_64-pc-windows-gnu "$@"
