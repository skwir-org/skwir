# ADR-035: Phase 5 is built on Linux, run on its systems later

Status: accepted, 2026-10-05. Splits Phase 5's gate (`docs/PLAN.md`).

## Context

Phase 5 ports skwir to macOS and Windows. Neither machine is available
for now; one will be later, for testing. The gate, a week of daily use
on each system with the budgets passing there, cannot be met before.
What can be done is everything short of running the result: the code
for each system, compiled and linted from Linux, and the logic that
does not need the system tested where it can be.

What a Linux machine can do, measured on 2026-10-05 with the dev
shell's toolchain plus the two targets:

- **Windows (`x86_64-pc-windows-gnu`).** With Zig as the C compiler,
  every C dependency builds (ring, mimalloc, psm, the SQLite binding),
  and gpui's Windows backend checks, the DirectWrite code patch 0006
  changed included. What fails is ours: the `xattr` crate is Unix-only,
  and core's filesystem layer is rustix, `std::os::unix` and statx
  throughout.
- **macOS (`aarch64-apple-darwin`).** The C dependencies build the same
  way. gpui's Metal backend does not: its build script generates
  bindings from Apple's SDK headers (`gpui_media`, `dispatch.h`), and
  Apple's terms allow its SDK on Apple hardware only. So `skwir-ui`
  and `skwir` are not checked for macOS; everything else is. Core fails
  first on its manifest (ordinary dependencies listed as Linux-only),
  then on statx, `syncfs`, `copy_file_range`, FICLONE, `mknodat` and the
  `trash` crate's listing, which macOS lacks.
- **Running.** Wine runs a Windows test binary, which answers for the
  logic (path forms, identity, the Recycle Bin's records) but not for
  Windows: its Recycle Bin is the freedesktop trash, its shell is not
  Explorer's. Nothing runs a macOS binary on Linux.

## Decision

1. **Phase 5 is two parts.** 5a is done on Linux: every system's
   code written, the Windows and macOS builds compiled and linted in
   `just lint-all` (`just check-cross`), the logic that can be tested
   tested (Linux, and Wine for Windows), the Linux packages built.
   5b is the original gate, on the machines: the week of daily use on
   each system and the budgets passing there.
2. **The checks use what is free to use here:** the GNU Windows
   target, with Zig for C, and the macOS target without the crates
   built on gpui. Which Windows toolchain ships (MSVC or GNU) is
   decided in 5b, on a Windows machine; code that depends on the
   difference says so.
3. **Code not run on its system says so.** Each Phase 5 entry in the
   plan's status lists what was compiled, what ran under Wine, and what
   no machine has run yet. A surface whose system code does not exist
   yet answers with an error naming the system ("not available on
   Windows yet"), never a Linux behaviour that would be wrong there.
4. **Linux does not move.** Every step keeps the Linux tests, budgets
   and behaviour as they were; a refactor for the port is checked
   here like any other change.
5. **No macOS SDK and no Windows SDK in the repository or the dev
   shell.** The Wine shell (`nix develop .#windows`) is separate, so
   the default shell does not grow by its 1.5 GB.

## Consequences

- The macOS UI is first compiled on a Mac (or a CI runner). Patches
  0006, 0008 and 0013 touch gpui's macOS code, never compiled here.
- Windows tests under Wine run with `WINEPREFIX` and the XDG folders
  in scratch directories: Wine's Recycle Bin writes to the freedesktop
  trash of whatever `XDG_DATA_HOME` says.
- 5b may find what 5a could not; its findings are fixed as findings,
  with this record's list of unrun code as the place to start.
