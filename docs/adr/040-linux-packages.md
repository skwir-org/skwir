# ADR-040: Linux packages: Debian's, RPMs and the AUR

Status: accepted, 2026-10-06 (Phase 5; the user chose the formats).
Amended the same day by ADR-041: the packages carry PDFium, and the
helpers few need, or whose libraries most systems do not, are packages
of their own (on Arch, optional libraries). Amended the same day by
ADR-042: skwir is under Apache-2.0 alone (`LICENSE`), and the Poppler
reader is distributed under the GPL version 3.

## Context

skwir had two Linux packages, both from Nix (ADR-035, step K): the
flake's package, for NixOS and Nix elsewhere, and an AppImage of it. A
program built by Nix carries Nix's C library; on another distribution
it cannot load that system's graphics driver, which is built against
the system's own, and skwir draws with the GPU: the AppImage is likely
to fail there. People install what their distribution installs.

Flatpak was considered and left out: its sandbox hides the programs
installed on the system that skwir runs (LibreOffice, Pandoc, ffmpeg,
exiftool…) and the folders it manages unless granted all of them, and
bubblewrap, which isolates the converters (ADR-039), cannot run inside
it.

## Decision

1. **Three formats, each built on the distribution it is for**, with
   its libraries and dependencies: a `.deb` for Debian and Ubuntu
   (cargo-deb, `tools/package/deb.sh`), an `.rpm` for Fedora and the
   distributions that install RPMs (cargo-generate-rpm,
   `tools/package/rpm.sh`), and a PKGBUILD for the AUR, built from a
   release's source with Arch's own Rust (`tools/package/aur`). The
   Debian and RPM builds use rustup's Rust: a distribution's may be
   older than skwir needs.
2. **One layout**: `skwir` in `/usr/bin`, its helpers where the
   distribution keeps a program's own (`/usr/lib/skwir` on Debian and
   Arch, `/usr/libexec/skwir` on Fedora), which skwir now looks in
   after its own folder (`process::helper_folders`), so no link or
   wrapper script stands between them; a desktop entry (`org.skwir.skwir.desktop`,
   the window's application id) and AppStream metainfo
   (`org.skwir.skwir`). Which programs ship is one list,
   `tools/package/programs`, which the builds read; the packages'
   file lists name the same programs.
3. **Dependencies**: what the programs link, found by each tool
   (`$auto`, auto-req; listed by hand for the AUR), and what skwir opens
   at run time, which no tool sees: the Vulkan loader and fontconfig.
   A Vulkan driver, bubblewrap and the basic GStreamer plugins are
   recommended; the programs skwir uses when they are there
   (LibreOffice, Pandoc, ImageMagick, ffmpeg, exiftool, plocate, the
   other GStreamer plugins) are suggested, so a package never pulls a
   suite of office programs.
4. **Every helper ships**, the MuPDF reader included (EPUB's pages are
   MuPDF's): the packages say the Poppler reader is distributed under
   the GPL and the MuPDF reader under the AGPL (ADR-014), whose
   sources are skwir's repository. A packager who wants neither drops
   those programs, as ADR-014 says.
5. **The licence texts** are in the repository (`LICENSE-MIT`, without
   a holder's line, as Rust's own; `LICENSE-APACHE`), installed by each
   package.
6. **Tried without containers**: `tools/package/try.sh` downloads a
   distribution's root filesystem (checked against its published
   sums), builds the package in it as its root user in a user namespace
   (bubblewrap), installs it into a fresh copy with the distribution's
   own package manager, which brings its dependencies, and checks that
   every program finds its libraries there. Nothing of this system is
   touched; it runs from any Linux with bubblewrap.

## Consequences

- A release is three builds on three distributions (CI, or `try.sh`
  here), each asking for the C library its build linked against or
  newer: the `.deb` made on Debian 13 asks for glibc 2.39 (Ubuntu 24.04
  has it, and the same library packages; not tried there), the `.rpm`
  made on Fedora 44 for 2.43 (Fedora 44 and later).
- The AUR package turns makepkg's LTO off (`options=('!lto')`): it
  compiles the C that crates build (MuPDF) into GCC's intermediate
  code, which Rust's linker (lld, its default) cannot read.
- A system with no font holding a symbol skwir draws as text (the sort
  arrow) shows a box there; every desktop installs one (DejaVu, Noto),
  so no font is depended on.
- The Debian maintainer field says "The skwir authors" until someone
  answers for the package; the AUR's maintainer is whoever publishes
  it there, which is the user's to do.
- skwir's icon (2026-10-11; until then the desktop entry named the
  theme's file manager icon) is one SVG, the UI's own
  (`crates/ui/assets/org.skwir.skwir.svg`), installed as
  `/usr/share/icons/hicolor/scalable/apps/org.skwir.skwir.svg`, which
  the desktop entry names and `linux/check.sh` checks.
