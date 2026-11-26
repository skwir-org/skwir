# ADR-074: Windows builds to try are made on Linux

Status: accepted, 2026-10-20, and built the same day: the zip and the
installer made on Linux and tried under Wine (their programs, their files,
installing and removing); the window never drawn there, and nothing yet
run on Windows (`docs/HELP-WANTED.md`). Replaces the package of ADR-035's
step K for Windows, `tools/package/windows.ps1` and a WiX 5 installer
written on Linux and never run.

## Context

- **Skwir is built for Windows from Linux already** (ADR-035):
  `just check-windows` lints every crate built with cargo-zigbuild
  (`x86_64-pc-windows-gnu`, MinGW-w64's runtime from Zig),
  `just test-windows` runs the core's tests under Wine,
  `just gpui-check-windows` gpui's Direct3D 11 mesh pass. Windows itself,
  Phase 5b, waits for a machine.
- **The user asked**, 2026-10-20, whether Windows and macOS builds could be
  made on this machine for people to try, then for a portable build and an
  installer for Windows.
- **What a release build needed of Windows:**
  - gpui compiles its shaders ahead with the Windows SDK's `fxc.exe`, in
    its build script, only when the build runs on Windows. Built
    elsewhere, a release build lacks the bytes it includes; a debug build
    compiles them from the source file's path on the build machine.
  - The resources, gpui's manifest (per-monitor DPI awareness, Common
    Controls 6) and skwir.exe's icon, need a resource compiler, which
    vendor patch 0027 and `crates/skwir/build.rs` asked for only on
    Windows. A build script's dependencies follow the system it runs on,
    so the icon's were not even built elsewhere.
  - skwir.exe was a console program: a console window opens behind it
    from Explorer or the Start menu.
- **What Wine lacks** that a gpui window needs: DirectComposition (gpui
  goes without it where `GPUI_DISABLE_DIRECT_COMPOSITION` says so), Direct
  Manipulation, DirectWrite's font sets, Windows' `icuuc.dll` (until
  gpui patch 0036, 2026-10-23: gpui calls ICU no more); its HLSL
  compiler, vkd3d's, stops on gpui's shadow shader. Windows 10 and 11 have
  them all.

## Decision

1. **Built on Linux** for x86-64, with cargo-zigbuild, as the checks
   build, into the same `target/cross`. A build with MSVC on Windows may
   come with 5b; nothing here needs it.
2. **Shaders compiled as the renderer starts** where built on another
   system (vendor patch 0034): from the source the program carries, by the
   system's own compiler (`D3DCompile`, d3dcompiler_47.dll), optimized as
   fxc's `/O3`. Built on Windows, they are compiled ahead as upstream does.
3. **Resources wherever a resource compiler is named** (`RC`,
   `RC_<target>`, as embed-resource reads them): vendor patch 0027 and
   skwir's build script embed them on Windows, or where one is named;
   `tools/package/windows.sh` names Nix's llvm-rc. A check names none and
   leaves them out. skwir's build dependencies are no longer Windows-only.
4. **A program of the window subsystem** where it ships (release builds):
   no console behind it. Started from a terminal, it takes that console
   unless its output already goes elsewhere (`attach_console`, `main.rs`),
   so `skwir --version`, `skwir cmd` and an agent's `skwir mcp` still
   speak.
5. **Two artifacts of one folder** (`tools/package/windows.sh`,
   `just package-windows`): a zip, used where it is unpacked, and an
   installer made by msitools' wixl (WiX 3's format,
   `tools/package/windows/skwir.wxs`), per user in
   `%LocalAppData%\Programs\skwir` with a Start menu entry, no
   administrator asked. Every file of the folder goes in it
   (wixl-heat): nothing lists them twice. The folder holds:
   - the programs of `tools/package/windows/programs`: skwir and the
     helpers that carry no library of their own (PDF through PDFium, DjVu
     through djvu-rs, 3D models, plugins' modules);
   - PDFium beside them (ADR-041), the type database in `share\mime`
     (ADR-037);
   - in `licenses`: skwir's licence, the crates' notices for Windows
     (`tools/package/notices/windows`, made by `notices.py` with
     cargo-about reading the Windows target's graph), PDFium's, and
     MinGW-w64's runtime's (ZPL 2.1, which asks for its notice in a
     binary's distribution).

   Neither is signed.
6. **Checked under Wine** (`tools/package/windows-check.sh`, in its own
   prefix; with a stand-in for `icuuc.dll` until gpui patch 0036):
   - the zip unpacked into a folder whose name has a space and an
     apostrophe, every file it should hold there, every program answering
     (`skwir.exe --version`, each helper's `--probe`, PDFium loaded from
     beside it);
   - the installer put in quietly, its programs answering where it put
     them, its Start menu entry, then removed with nothing left.
7. **`windows.ps1` is gone.** `skwir.wxs` is wixl's.

## Consequences

- **Each start compiles gpui's eighteen shaders.** Under Wine with
  Microsoft's compiler (winetricks' d3dcompiler_47), the renderer took
  about 0.28 s from its devices to its window. Not measured on Windows.
  Keeping the compiled shaders in the cache, or compiling them ahead under
  Wine with Microsoft's compiler, would win it back.
- **No archives, player, Poppler, MuPDF or DjVuLibre:** their helpers link
  libraries not built for Windows here, and skwir says the engine is not
  installed (archives and the player since, amendments below). Mermaid's helper is left out as Linux's packages leave it
  (ADR-041).
- **Windows warns before either runs** (SmartScreen: "Windows protected
  your PC", More info, Run anyway), since nothing is signed. Signing needs
  a certificate: a decision of its own.
- **Wine cannot try the window.** It got as far as the renderer, its
  shaders compiled by Microsoft's compiler, DirectComposition off, then
  stopped at Direct Manipulation and, past it, at fonts. What the window
  does is for people on Windows to try (`docs/HELP-WANTED.md`).
- **In a terminal, the prompt may come back before skwir's output**: a
  terminal does not wait for a program of the window subsystem.
- `tools/gpui/vendor.py export` keeps what an editor's build leaves in
  `vendor/zed` (`target`, `Cargo.lock`), which it used to delete.
- macOS is not made here: the user offered Apple's SDK (2026-10-20), and a
  build with it is a decision of its own.

## Amendments

- 2026-10-20: vendor patch 0034 stays. It acts only where gpui is built
  for Windows on another system (the build script sets
  `gpui_shaders_at_start` only then); built on Windows, gpui compiles its
  shaders ahead with fxc, as upstream does, and the patch's code is not
  compiled. So a release built on Windows compiles nothing at each start,
  and builds made on Linux keep working (the user's decision).
- 2026-10-20 (ADR-075): what is shipped names no folder of the machine
  that built it (`/cargo`, `/rust`, `/skwir` in a panic's location and
  C's `__FILE__`), which the check now looks for; the package is built in
  `target/windows`, apart from `check-windows`.
- 2026-10-20: from the next release, `just release-build` makes the zip
  and the installer with the Linux packages, and `just release-upload`
  puts them on the release's page, said there to be builds to try (the
  user's decision). The first ones, from 5a0bb2b, were added to 0.2.0's
  page, with a section of its notes saying so.
- 2026-10-20 (ADR-041): the archive helper is in the package, libarchive
  built with Zig and linked into it statically
  (`tools/package/libarchive.sh`: about 2.5 minutes, once for each
  version, kept in the build cache). Windows' libarchive reads a name no
  archive says the charset of in the OEM code page: the helper reads a
  tar's and a cpio's as UTF-8, as Unix's tools write them, and writes a
  ZIP's in UTF-8, flagged so. The check makes and lists, under Wine, each
  format skwir makes; the helper's tests and the archives plugin's pass
  there, but for the trash's steps.
- 2026-10-20 (ADR-076): the media helper is in the package, in
  `gstreamer\bin` among the libraries of the GStreamer it carries
  (`tools/package/gstreamer.py`); the check plays an H.264 and AAC clip
  and a VP9 and Opus one through it under Wine.
- 2026-11-11 (ADR-129): the formulas' helper is in the package, beside
  skwir. Linux's packages keep it apart so that its weight (about 5 MB)
  is the user's choice; this one has no way to add it apart, and without
  it formulas would never be drawn. Under Wine it typesets a batch (a
  fraction, a number set apart by its `\tag`, chemistry; broken TeX
  refused with its reason). Mermaid's stays out.
- 2026-11-17 (PR #12): what skwir starts, it starts with no console
  window. A program of the window subsystem has no console, so each
  console program it started opened one of its own: one flashed at each
  Git state read, and one would stand behind each helper that serves,
  as long as it runs. Every
  program is made by `process::command` (`skwir_platform`), which sets
  `CREATE_NO_WINDOW`; clippy refuses `std::process::Command::new`
  elsewhere (`clippy.toml`), but in tests, build scripts and a helper's
  own children, which share its unseen console. A terminal asks for a
  console again (`CREATE_NEW_CONSOLE`). Wine shows no console windows:
  the flashing was seen on Windows, by the PR's author; the rest is
  compiled only.
- 2026-11-23, the user's choice (found by skwird's session, whose
  engine, started by its client, held the client's standard output on
  Windows): what skwir starts inherits none of skwir's own standard
  handles. Windows gives a child every inheritable handle its parent
  holds, and std's `Command` lets it: started with pipes for its input
  and output (`skwir mcp`, a script reading `skwir cmd`), skwir would
  have passed them to every helper, and whoever read skwir's output to
  its end waited for the last helper to go. At start, skwir marks its
  standard handles not inheritable (`process::keep_own_handles`); a
  program asked to share them is given copies by std, as before.
- 2026-11-26, the user's decision (ADR-050, amended the same day): the
  zip and the installer carry skwird, skwir's index of names, as its own
  release built it: skwird 0.2.0's Windows zip, pinned by its sum in
  `tools/package/skwird.lock` and checked before it is unpacked
  (`tools/package/skwird.py`). `skwird.exe` and `skwirdw.exe` (skwird
  with no console, which its logon task runs) stand beside `skwir.exe`,
  where skwir looks for skwird once `PATH` has none; its licences
  (`LICENSE-MIT`, `LICENSE-APACHE`) and the notices of the crates its
  program links for Windows (`tools/package/notices/windows/skwird`, made
  by `notices.py` from skwird's own `Cargo.lock` at that tag: its release
  ships none) in `licenses\skwird`. skwird registers its logon task
  (`skwird logon add`) with the path of the program it runs, so a task
  added from the carried copy names skwir's folder: the installer's
  removal runs that copy's `skwird.exe logon remove` first, deferred, as
  the user, its answer ignored (none registered: nothing lost), not when
  an upgrade removes the older skwir (`skwir.wxs`). wixl 0.106 places an
  action `Before="RemoveFiles"` ahead of `InstallInitialize`, where a
  deferred one fails the removal: it is placed after `InstallInitialize`.
  skwird 0.2.0 names one logon task a user, `skwird-<SID>`, which the
  copy that ran `logon add` last owns (`/F`), and takes it back by that
  name without reading what it runs: removing Skwir takes back the task
  of a skwird installed on its own too, which that skwird's own `skwird
  logon add` registers again. A removal that takes back only a task
  naming its own copy is asked of skwird. The action runs `skwird.exe`,
  a console program, which Windows gives a console of its own, msiexec
  having none: a window flashes as Skwir goes. `skwirdw.exe` would not
  spare it: skwird 0.2.0 starts `schtasks.exe` through std's `Command`,
  which sets no `CREATE_NO_WINDOW`, so from a program with no console
  `schtasks` opens one of its own; and skwirdw, unless `SKWIRD_LOG` names
  a log, first makes skwird's state folder and its `engine.log`, which a
  removal would leave behind for a user who never ran skwird. wixl 0.106
  carries one extension, WiX's dialogs, not WixUtilExtension's quiet
  exec.
  An engine running from the carried copy holds its program: one its
  logon task started (`serve --forever`, which `logon remove` does not
  end in 0.2.0) until the user logs off, and one a search started for
  the minute it outlives its last client, when Skwir is removed or an
  upgrade removes the older copy (the older package's sequence: the
  action above does not run then). The package authors no FilesInUse or
  MsiRMFilesInUse dialog, nor `MSIRESTARTMANAGERCONTROL`,
  `MSIRMSHUTDOWN` or `REBOOT` (`msiinfo`), so Windows Installer's
  defaults apply: it asks Restart Manager (`MSIRESTARTMANAGERCONTROL`
  0, the default), and, as Microsoft's pages say of installing, a file
  still in use is replaced or deleted when the system restarts, a
  restart asked. They say neither whether a removal with no UI restarts
  by itself, nor how Restart Manager closes a program with no window
  (skwirdw.exe; a console program is sent `CTRL_C_EVENT`). Nothing ends
  the engine first: skwird 0.2.0 has no command that stops one;
  `taskkill` finds a process by its program's name, not its path, so it
  would end a skwird installed on its own too; `schtasks /End` of the
  logon task ends only the engine the task started, whichever copy's,
  and the installer could name the task only by `UserSID`, which
  Microsoft documents up to Vista alone; and PowerShell, which could
  end a process by its path, opens a console of its own. A stop of the
  engine of the copy that runs it alone is asked of skwird; the
  installer would run it as an immediate action before
  `InstallValidate`, on every removal, an upgrade's included, from the
  first package that carries it.
  The installer is per user and never elevated, so it cannot take back a
  system service; skwird's `service add`, an administrator's, is not for
  a copy in the user's own folder, which that user, unelevated, could
  replace under a service running as LocalSystem. The zip has no
  uninstaller: a task added from it is its user's to take back (`skwird
  logon remove`) before the folder goes. Under Wine
  (`windows-check.sh`): the zip holds both programs and their licences,
  byte for byte the release's; `skwird.exe` starts from the zip and the
  installed copy; the installer's action is the one meant (`msiinfo`: its
  type, program, command, condition and step); and a logon task added
  from the installed copy (Wine keeps it, its actions left out: Wine runs
  no task) is gone once skwir is removed. Not checked under Wine: the
  task starting skwird at a logon, a running engine holding
  `skwird.exe` or `skwirdw.exe` as the files go, at a removal or an
  upgrade's, and the console window the action opens (Wine shows none).
