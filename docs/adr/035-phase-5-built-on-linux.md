# ADR-035: Phase 5 is built on Linux, run on its systems later

Status: accepted, 2026-10-05. Splits Phase 5's gate (`docs/PLAN.md`).

## Context

Phase 5 ports skwir to macOS and Windows. Neither machine is available
now; one will be later, for testing. The gate (a week of daily use on
each system, budgets passing there) cannot be met before. Everything
short of running the result can be done from Linux.

What Linux can do (measured 2026-10-05, dev shell toolchain plus both
targets):

- **Windows (`x86_64-pc-windows-gnu`).** With Zig as C compiler every C
  dependency builds (ring, mimalloc, psm, SQLite), and gpui's Windows
  backend checks, including patch 0006's DirectWrite code. What failed
  was ours: the Unix-only `xattr` crate, and core's filesystem layer
  (rustix, `std::os::unix`, statx).
- **macOS (`aarch64-apple-darwin`).** C dependencies build. gpui's Metal
  backend does not: its build script generates bindings from Apple's
  SDK headers (`gpui_media`, `dispatch.h`), and Apple's terms allow the
  SDK on Apple hardware only. So `skwir-ui` and `skwir` are not checked
  for macOS; everything else is. Core failed on its manifest
  (dependencies listed as Linux-only), then statx, `syncfs`,
  `copy_file_range`, FICLONE, `mknodat` and the `trash` crate's listing,
  which macOS lacks.
- **Running.** Wine runs Windows test binaries: it answers for logic
  (path forms, identity, Recycle Bin records), not for Windows (its
  Recycle Bin is the freedesktop trash, its shell is not Explorer).
  Nothing runs macOS binaries on Linux.

## Decision

1. **Phase 5 is two parts.**
   - **5a, on Linux:** every system's code written; Windows and macOS
     builds compiled and linted in `just lint-all` (`just
     check-cross`); testable logic tested (Linux, Wine for Windows);
     Linux packages built.
   - **5b, on the machines:** the original gate, a week of daily use on
     each system with its budgets passing.
2. **Checks use what is free here:** the GNU Windows target with Zig,
   and the macOS target without gpui's crates. MSVC or GNU for shipping
   is decided in 5b on Windows; code depending on the difference says
   so.
3. **Code not run on its system says so.** Each Phase 5 status entry
   lists what was compiled, what ran under Wine, and what no machine
   has run. A surface without system code answers with an error naming
   the system ("not available on Windows yet"), never with Linux
   behaviour that would be wrong there.
4. **Linux does not move.** Its tests, budgets and behaviour stay; a
   refactor for the port is checked here like any change.
5. **No macOS or Windows SDK in the repository or the dev shell.** The
   Wine shell (`nix develop .#windows`, 1.5 GB) is separate.

## Consequences

- The macOS UI is first compiled on a Mac or a CI runner. Patches 0006,
  0008 and 0013 touch gpui's macOS code, never compiled here.
- Wine tests run with `WINEPREFIX` and XDG folders in scratch
  directories: Wine's Recycle Bin writes to the freedesktop trash of
  `XDG_DATA_HOME`.
- 5b may find what 5a could not; findings are fixed as findings,
  starting from the list of unrun code.
