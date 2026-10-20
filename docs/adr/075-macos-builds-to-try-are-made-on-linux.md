# ADR-075: macOS builds to try are made on Linux

Status: accepted, 2026-10-20, and built the same day: `skwir.app` for Apple
silicon made on Linux and looked over there (its files, its programs'
architecture, system and libraries, its seal); nothing of it run, which
needs a Mac (`docs/HELP-WANTED.md`). Follows ADR-074, which made Windows'
build to try the same way, and replaces the package of ADR-035's step K for
macOS, `tools/package/macos.sh` for a Mac, written on Linux and never run.

## Context

- **macOS was compiled in part.** `just check-macos` lints every crate for
  `aarch64-apple-darwin` but skwir-ui, skwir and the native helpers: gpui's
  Metal backend made its shader bindings, and compiled its shaders with
  `xcrun metal`, only when the build ran on a Mac.
- **Apple's SDK** holds the headers and the libraries' stubs a link needs.
  On 2026-10-20 the user asked to add xmac to the flake, which downloads it
  from Apple's servers, and accepted Apple's licence for it (`just
  macos-sdk`, the flake's `.#macos` shell). Its licence allows developing
  for Apple's systems and forbids passing the SDK on.
- **What links it:** Zig's linker (cargo-zigbuild) misreads the
  exported-symbols list rustc gives a crate built as a dynamic library too
  (PDFium's and djvu-rs's bindings). LLVM's Mach-O linker reads it, but not
  the stubs of SDK 27, which name an architecture it does not know
  (`arm64e.x1`, LLVM 21 to 23); those of SDK 26.5 it reads.

## Decision

1. **Built on Linux** for Apple silicon (`aarch64-apple-darwin`) against
   SDK 26.5, for macOS 11 and later: compiled by clang and linked by LLVM's
   `ld64.lld` through xmac's wrappers (`XMAC_CLANG`, `XMAC_LLD` in the
   `.#macos` shell), into `target/macos`. Not Intel Macs: a universal build
   is a step of its own.
2. **gpui's Apple build runs for the target, not the system building**
   (vendor patch 0035): the bindings made by cbindgen anywhere, and built
   on another system, the shaders compiled as the renderer starts by
   macOS's own Metal compiler (gpui's `runtime_shaders` path). Built on a
   Mac, nothing changes.
3. **No library of the building system's:** the target's `pkg-config` is
   `false`, so the dev shell's, which would hand over Linux's SQLite, is not
   asked; macOS's own is linked.
4. **`skwir.app`, zipped and in a disk image** (`tools/package/macos.sh`,
   `just package-macos`):
   the programs of `tools/package/macos/programs` in `Contents/MacOS`,
   PDFium in `Contents/Frameworks` (ADR-041), in `Contents/Resources` the
   icon (an `.icns` of every side, drawn from the UI's own), the type
   database (ADR-037) and the licences: skwir's, the crates' notices for
   macOS (`tools/package/notices/macos`), PDFium's. Signed ad hoc by
   rcodesign, as Apple silicon asks of anything it runs; not notarized.
   The disk image holds the bundle beside a link to Applications, an HFS+
   volume made by hfsprogs' mkfs.hfsplus, filled and compressed by
   Mozilla's libdmg-hfsplus (packaged in the flake), as Firefox's are made
   on Linux (`hdiutil` is macOS's alone).
5. **Looked over on Linux** (`tools/package/macos-check.sh`): the zip holds
   the bundle alone, programs executable; the disk image, opened by the
   tools that made it, the same bundle file for file, modes kept, beside
   its link; Info.plist names what is there;
   the icon holds every side; each program is arm64, for macOS 11, linked
   to the system's libraries alone; the seal holds (Info.plist and every
   resource as signed, each program signed).
6. **No folder of the building machine in what is shipped**, here and in
   Windows' package (amending ADR-074): the crates' sources, Rust's and the
   tree's are named `/cargo`, `/rust` and `/skwir` in a panic's location and
   C's `__FILE__`; both checks look for the machine's folders. Windows'
   package is built in `target/windows`, apart from `check-windows`, which
   would otherwise rebuild everything after it.

## Consequences

- **Each start compiles gpui's shaders** from their source (Metal's
  compiler), as a Windows build to try does (ADR-074). Not measured: no
  Mac. Patch 0035 stays, as 0034 does: built on a Mac, gpui compiles its
  shaders ahead as upstream does and the patch's code is not compiled, so
  a release built there compiles nothing at each start (the user's
  decision).
- **macOS asks before opening it:** not notarized, it is "from an
  unidentified developer"; System Settings, Privacy & Security, Open
  Anyway (macOS 15), or `xattr -dr com.apple.quarantine skwir.app`.
  Notarizing needs an Apple developer account: a decision of its own.
- **No archives, player, Poppler, MuPDF or DjVuLibre**, as on Windows: their
  helpers link libraries not built for macOS here (archives since,
  and the player since, amendments below). Mermaid's is left out as
  Linux's packages leave it (ADR-041).
- **Nothing here can run it.** What it does on a Mac, PORTING's macOS
  items included, is for people with one.
- Building needs the SDK on the machine (`just macos-sdk`); it is never
  committed or shipped.

## Amendments

- 2026-10-20: from the next release, `just release-build` makes the zip
  and the disk image with the Linux packages (stopping at once without the
  SDK), and `just release-upload` puts them on the release's page, said
  there to be builds to try (the user's decision, as for Windows,
  ADR-074).
- 2026-10-20 (ADR-041): the archive helper is in the bundle, libarchive
  built through xmac's CMake toolchain and linked into it statically, with
  macOS's own zlib, bzip2, iconv and Expat (`tools/package/libarchive.sh`);
  the check refuses a helper linked to the system's libarchive, whose
  formats vary with macOS. Never run.
- 2026-10-20 (ADR-076): the media helper is in the bundle, GStreamer in
  `Contents/Frameworks/GStreamer` (`tools/package/gstreamer.py`); the
  check looks over each of its files (Apple silicon's, loading the
  system's or GStreamer's own, signed). Never run.
