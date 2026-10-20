# Helping Skwir reach macOS and Windows

Skwir 0.2 is released for Linux only. Its code for macOS and Windows
compiles (`just check-cross`) and the Windows build's tests pass under
Wine, but none of it has run on a real Mac or Windows machine: nobody
working on Skwir has one. If you do, you can help.

Below are the steps each release needs, best done in order. Details are in
[`docs/PORTING.md`](PORTING.md) ("PORTING"); ADR-035 explains the split.

- **try**: no programming. Run Skwir and report what happened.
- **code**: needs someone who writes Rust on that system.

## Before you start

**Build.** Install Rust with [rustup](https://rustup.rs); in the repository
it uses the version `rust-toolchain.toml` pins (Skwir builds with 1.96 or
later). On a Mac, install Xcode's command line tools
(`xcode-select --install`); on Windows, the MSVC build tools. Then:

```
cargo build --release -p skwir
target/release/skwir
```

The helpers that link native libraries (Poppler, MuPDF, GStreamer,
libarchive) are not needed to start.

**Test.**

```
cargo install cargo-nextest
cargo nextest run -p skwir-core -p skwir-platform
```

nextest first runs `tools/test/isolate-xdg.sh`, so tests never touch your
Trash, settings or cache; on Windows Git's `sh` runs it, as in CI. A failing
test is a finding: report which and what it printed.

**Report** in [the issue tracker](https://skwir.org/issues): open a new
issue with its "macOS or Windows" form, naming the step (for example
"macOS 3, Files"). Include:

- the system's version and the machine (processor, GPU);
- what you did, what you expected, what happened;
- the log: run with `RUST_LOG=skwir=debug` (PowerShell:
  `$env:RUST_LOG="skwir=debug"` first).

Rename anything personal in paths and screenshots before sharing: a folder
called `taxes 2026` can be `folder a`.

## macOS

A build to try is made on Linux for Apple silicon (ADR-075, `just
package-macos`): `skwir-<version>-macos-arm64.dmg`, a disk image to drag
`skwir.app` from to Applications, or the same in a zip,
`skwir-<version>-macos-arm64.zip`; for macOS 11 or later. It is signed ad hoc but not notarized, so macOS refuses
it at first: open it once from System Settings, Privacy & Security ("Open
Anyway"), or run `xattr -dr com.apple.quarantine skwir.app`. It reads PDFs,
DjVu and 3D models; archives, audio and video, and the Poppler and MuPDF
readers are not in it yet. It has never run anywhere: everything below is
new. Intel Macs have no build yet.

1. **It opens** (try). The window draws, a folder lists, the sidebar shows
   the home folder's places and the volumes under `/Volumes`, scrolling a
   large folder stays smooth, and idle CPU returns to about 0% (Activity
   Monitor). The window compiles its shaders as it starts: say how long it
   takes to appear (`first frame … after start` in the log, run
   `skwir.app/Contents/MacOS/skwir` from a terminal to see it). PORTING
   sections 1, 6.
2. **Tests** (try). The tests above pass natively. PORTING "Tests that run
   on Linux alone" lists what is skipped and why.
3. **Files** (try). Copy, move, rename, swap two names (rename each to the
   other's), move to Trash, and undo each; Finder's tags survive a copy.
   These use `renameatx_np`, `clonefile` and `trashItemAtURL`, which have
   never run (PORTING sections 3, 4, 5). Nothing may be lost: compare the
   files before and after.
4. **Types and pictures** (try). Files show the right kind and icon (UTType,
   `NSWorkspace`); thumbnails come from Quick Look (PORTING sections 7, 8).
   Report which files look wrong.
5. **Keys** (try). Cmd is the standard modifier; Ctrl where macOS keeps Cmd
   for itself (PORTING "10–15"). Report shortcuts that do the wrong thing.
   A drag's copy modifier should be Option (not done).
6. **Isolation** (code). Converters (LibreOffice, Pandoc, ImageMagick) and
   plugins from outside run under `sandbox-exec` with Skwir's profile
   (`platform/src/sandbox.rs`, ADR-039). Find what the profile must let
   through for each to start; without it they are refused.
7. **What is not written** (code): ejecting a volume (DiskArbitration);
   audio and video through AVFoundation (a helper of its own, ADR-041); the
   trash's listing (`~/.Trash` records no origin); Finder's favorites as
   bookmarks; media keys (Now Playing). PORTING sections 4, 6, 8, "10–15".
8. **The package** (code). `tools/package/macos.sh` makes `skwir.app` on
   Linux (ADR-075); try it on a fresh account, and moved to Applications.
   An Intel build, the helpers that link libraries (libarchive,
   AVFoundation's player, Poppler, MuPDF) and a disk image are not done.
   Notarization needs an Apple developer identity: the project decides that
   with you.
9. **CI** (code). The `native` job in `.github/workflows/ci.yml` builds,
   lints and tests on `macos-latest`. It does not block merging until it
   passes.

## Windows

A build to try is made on Linux (ADR-074, `just package-windows`): a zip,
`skwir-<version>-windows-x86_64.zip`, to unpack anywhere and start
`skwir.exe` from, or an installer, `skwir-<version>-windows-x86_64.msi`,
which puts it in `%LocalAppData%\Programs\skwir` with a Start menu entry
and asks no administrator. Neither is signed: Windows warns first ("Windows
protected your PC": More info, then Run anyway). It reads PDFs, DjVu and 3D
models; archives, audio and video, and the Poppler and MuPDF readers are not
in it yet. Wine started its programs and installed it, but cannot draw its
window: everything below is new.

1. **It opens** (try). The window draws, a folder lists, the drives are in
   the sidebar, scrolling stays smooth, and idle CPU returns to about 0%
   (Task Manager). The window compiles its shaders as it starts: say how
   long it takes to appear (`first frame … after start` in the log), and
   whether a console opens behind it (it should not). PORTING sections 1, 6.
2. **Tests** (try). The tests pass natively, not only under Wine. Those that
   need what Wine lacks (the shell's Recycle Bin, alternate streams) run
   only here: PORTING sections 1, 3, 4.
3. **Files** (try). Copy (with progress and Cancel), move, rename, Recycle
   Bin, and undo each; a downloaded file keeps its mark of origin (an
   alternate stream). A swap is refused by design; a bulk rename parks a
   name instead (PORTING section 3).
4. **The terminal** (try). The terminal panel (ConPTY) starts, takes keys,
   and follows the folder shown (PORTING section 9).
5. **Control channel security** (code). Agents and scripts reach the window
   through a named pipe with Windows' default security, which lets other
   accounts open it, and the client does not check the server's account.
   Before Control can be turned on there, the pipe needs a DACL of the user
   alone and the client a check of the server
   (`plugins/control/src/channel.rs`, PORTING "10–15").
6. **Isolation** (code). Windows has none yet (ADR-039): converters that
   need it are refused, and plugins from outside that run as programs do
   not run. The plan is an AppContainer (`platform/src/sandbox.rs`).
7. **What is not written** (code): ejecting a drive; drives plugged in
   appearing at once (`WM_DEVICECHANGE`); audio and video through Media
   Foundation (a helper of its own); owners by SID; Quick Access as
   bookmarks; Windows Search for hints; media keys (SMTC). PORTING sections
   6, 7, 8, "10–15".
8. **The package** (code). `tools/package/windows.sh` makes the zip and the
   installer on Linux (ADR-074); try the installer on a fresh account:
   installing, the Start menu entry, updating over an older one, removing.
   The helpers that link libraries (libarchive, Media Foundation's player,
   Poppler, MuPDF) need those built for Windows. Signing needs a
   code-signing certificate: the project decides that with you.
9. **CI** (code). The `native` job runs on `windows-latest` too.

## When a system is ready

A release on a system needs:

- every step done, or stated as not needed;
- its tests passing in CI;
- a package that installs and runs on a fresh machine;
- `docs/PLAN.md` saying what is left there.

The model is the Linux release's checklist (`docs/PLAN.md`, "Release 0.1").
