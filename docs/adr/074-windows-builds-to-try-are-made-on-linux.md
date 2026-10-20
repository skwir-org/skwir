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
  Manipulation, DirectWrite's font sets, Windows' `icuuc.dll`; its HLSL
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
   prefix with a stand-in for `icuuc.dll`):
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
  installed. Mermaid's helper is left out as Linux's packages leave it
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
