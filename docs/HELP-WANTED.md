# Helping bibli reach macOS and Windows

bibli 0.1 is released for Linux alone. Everything it does is written for
macOS and Windows too, and compiles for both (`just check-cross`); the
Windows build's tests pass under Wine. But none of it has run on a Mac
or on a Windows machine: nobody working on bibli has one. This page is
what a release on each needs, in the order it is best done, so that
anyone with the machine can take a step. `docs/PORTING.md` holds the
details each step points to; ADR-035 says how the work was split.

Each step says what to do and how you know it is done. Steps marked
**try** need no programming: running bibli and saying what happened
helps as much. Steps marked **code** need someone who can write Rust for
that system.

## Before you start

- **Build it.** Install Rust with [rustup](https://rustup.rs) (stable;
  bibli needs 1.96 or later). On a
  Mac, Xcode's command line tools (`xcode-select --install`); on
  Windows, the MSVC build tools. Then, in the repository,
  `cargo build --release -p bibli` and run `target/release/bibli`. The
  helpers that link native libraries (Poppler, MuPDF, GStreamer,
  libarchive) are not needed to start.
- **Run the tests.** `cargo install cargo-nextest`, then
  `cargo nextest run -p bibli-core -p bibli-platform`. A failure is a
  finding: say which test and what it printed.
- **Report** in an issue: the system and its version, the machine (and
  its GPU), what you did, what you expected and what happened, and the
  log (`RUST_LOG=bibli=debug` before the command). Rename anything
  personal in paths and screenshots before you share them: a folder
  called `taxes 2026` can be `folder a`.

## macOS

1. **try** It opens. The window draws, a folder lists, the sidebar shows
   the home folder's places and the volumes under `/Volumes`; scrolling a
   large folder stays smooth; with nothing happening, the CPU goes back
   to about 0% (Activity Monitor). PORTING sections 1, 6.
2. **try** The tests above pass natively. PORTING "Tests that run on
   Linux alone" says which are skipped and why.
3. **try** Files. Copy, move, rename, rename two files into each other's
   names (a swap), move to the Trash and undo each, Finder's tags kept on
   a copy. These use `renameatx_np`, `clonefile` and `trashItemAtURL`,
   which have never run (PORTING sections 3, 4, 5). Nothing may be lost:
   compare the files before and after.
4. **try** Types and pictures. Files show the right kind and icon
   (UTType, `NSWorkspace`), and thumbnails come from Quick Look
   (PORTING sections 7, 8). Say which files look wrong.
5. **try** Keys. Cmd is the standard modifier, Ctrl where macOS keeps
   Cmd for itself (PORTING "10–15"). Say which shortcut does the wrong
   thing; a drag's copy modifier should be Option (not done).
6. **code** Isolation. Converters (LibreOffice, Pandoc, ImageMagick)
   and plugins from outside run under `sandbox-exec` with bibli's
   profile (`platform/src/sandbox.rs`, ADR-039); find what the profile
   must let through for each to start. Without it they are refused.
7. **code** What is not written: ejecting a volume (DiskArbitration),
   audio and video through AVFoundation (a helper of its own, ADR-041),
   the trash's listing (`~/.Trash` keeps no record of where a file came
   from), Finder's favorites as bookmarks, media keys (Now Playing).
   PORTING sections 4, 6, 8, "10–15".
8. **code** The package. `tools/package/macos.sh` makes `bibli.app` and
   a disk image; it has never run. It must also carry the crates'
   notices (`tools/package/notices.py`, as the Linux packages do) in
   `Contents/Resources/licenses`. Signing and notarization need an
   Apple developer identity: the project decides that with you.
9. **code** CI. The `native` job in `.github/workflows/ci.yml` builds,
   lints and tests on `macos-latest`; it does not block merging until
   it passes.

## Windows

1. **try** It opens. The window draws, a folder lists, the drives are
   in the sidebar, scrolling stays smooth, idle CPU goes back to about
   0% (Task Manager). PORTING sections 1, 6.
2. **try** The tests pass natively, not only under Wine. Those that need
   what Wine lacks (the shell's Recycle Bin, a file's alternate streams)
   run only here: PORTING sections 1, 3, 4.
3. **try** Files. Copy (with progress and Cancel), move, rename, the
   Recycle Bin and undoing each, a downloaded file keeping its mark of
   where it came from (an alternate stream). A swap is refused there by
   design; a bulk rename parks a name instead (PORTING section 3).
4. **try** The terminal panel (ConPTY): it starts, takes keys, and
   follows the folder shown (PORTING section 9).
5. **code** Security of the control channel. Agents and scripts reach
   the window through a named pipe that has Windows' default security,
   which lets other accounts open it, and the client does not check the
   server's account. Before Control may be turned on there, the pipe
   needs a DACL of the user alone and the client a check of the server
   (`plugins/control/src/channel.rs`, PORTING "10–15").
6. **code** Isolation. Windows has none yet (ADR-039): converters that
   need it are refused, and plugins from outside that run as programs
   do not run. An AppContainer is the plan (`platform/src/sandbox.rs`).
7. **code** What is not written: ejecting a drive, drives plugged in
   appearing at once (`WM_DEVICECHANGE`), audio and video through Media
   Foundation (a helper of its own), owners by SID, Quick Access as
   bookmarks, Windows Search for hints, media keys (SMTC). PORTING
   sections 6, 7, 8, "10–15".
8. **code** The package. `tools/package/windows.ps1` builds what ships
   and an installer from `tools/package/windows/bibli.wxs` (WiX); it
   has never run. It must also carry the crates' notices
   (`tools/package/notices.py`). Signing needs a code-signing
   certificate: the project decides that with you.
9. **code** CI. The `native` job runs on `windows-latest` too.

## When a system is ready

A release on a system needs its steps done or said not to be, its tests
passing in CI, a package that installs and runs on a fresh machine, and
`docs/PLAN.md` saying what is left there. The Linux release's checklist
(`docs/PLAN.md`, "Release 0.1") is the model.
