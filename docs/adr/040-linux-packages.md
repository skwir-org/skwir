# ADR-040: Linux packages: Debian's, RPMs and the AUR

Status: accepted, 2026-10-06 (Phase 5; the user chose the formats).
Amended the same day by ADR-041 (packages carry PDFium; rarely needed
helpers are separate packages, optional libraries on Arch) and ADR-042
(Apache-2.0 alone; the Poppler reader under GPL-3.0).
Amended 2026-10-11 by ADR-052: a conventional AppImage built in an
Ubuntu root, including from a Nix host, supplements these formats; since
2026-10-13, its tree as a tarball too.
Amended 2026-10-11 by ADR-053: local release builds and draft uploads;
hosted package builds are a manual fallback. Decision 7, `skwir-bin` and
`skwir-git` on the AUR, added 2026-10-17.

## Context

Skwir's Linux packages were Nix's (the flake and an AppImage, ADR-035).
A Nix build carries Nix's libc and cannot load another distribution's
GPU driver, so the AppImage likely fails there. People install what
their distribution installs.

Rejected: **Flatpak**. Its sandbox hides the system programs Skwir runs
and the folders it manages unless all are granted, and bubblewrap
(ADR-039) cannot run inside it.

## Decision

1. **Three formats, each built on its distribution:**

   | Format | For | Built with |
   |---|---|---|
   | `.deb` | Debian, Ubuntu | cargo-deb, `tools/package/deb.sh` |
   | `.rpm` | Fedora and other RPM distributions | cargo-generate-rpm, `tools/package/rpm.sh` |
   | PKGBUILD | the AUR | a release's source, Arch's Rust (`tools/package/aur`) |

   `.deb` and `.rpm` use rustup's Rust: a distribution's may be too old.
2. **One layout:** `skwir` in `/usr/bin`; helpers in `/usr/lib/skwir`
   (Debian, Arch) or `/usr/libexec/skwir` (Fedora), where Skwir looks
   after its own folder (`process::helper_folders`), so no wrapper; a
   desktop entry `org.skwir.skwir.desktop` (the window's application
   id) and AppStream metainfo. The shipped programs are one list,
   `tools/package/programs`, which builds and file lists follow.
3. **Dependencies:** what programs link (found by each tool: `$auto`,
   auto-req; by hand for the AUR), plus the Vulkan loader and
   fontconfig, opened at run time. Recommended: a Vulkan driver,
   bubblewrap, basic GStreamer plugins. Suggested: programs Skwir uses
   when present (LibreOffice, Pandoc, ImageMagick, ffmpeg, exiftool,
   plocate, other GStreamer plugins), so no package pulls an office
   suite.
4. **Every helper ships**, MuPDF's included (EPUB pages are MuPDF's).
   Packages state the Poppler reader is GPL and the MuPDF reader AGPL
   (ADR-014), sources in Skwir's repository. A packager may drop both
   (ADR-014).
5. **Each package installs the licence text** (`LICENSE` since ADR-042;
   first `LICENSE-MIT` and `LICENSE-APACHE`).
6. **Tried without containers:** `tools/package/try.sh` downloads a
   distribution's root filesystem (checked against published sums),
   builds the package in it as root in a user namespace (bubblewrap),
   installs it into a fresh copy with the distribution's package
   manager, and checks every program finds its libraries. The host is
   untouched; any Linux with bubblewrap runs it.
7. **Two more packages on the AUR** (2026-10-17, the user asked for
   them), each a package base of its own under `tools/package/aur`:
   `skwir-bin` installs the release's portable tarball (ADR-052) as it
   is, in `/opt/skwir` with `skwir` linked into `/usr/bin`: one package,
   every helper and the libraries it carries with it, standing for the
   three `skwir` makes (it provides and conflicts with them), x86-64
   only. `skwir-git` builds `main` into the same three packages, named
   `-git`, each providing its release twin; its build and package
   functions are the release PKGBUILD's word for word
   (`release-tools-check` compares them). `aur-prepare` publishes all
   three bases at each release.

## Consequences

- A release is four builds (CI, or `try.sh`), each needing the glibc
  it linked or newer: the `.deb` from Debian 13 needs 2.39; Ubuntu's own,
  built on 24.04 since 2026-10-12, depends on Ubuntu's library packages
  and is named apart (`-1ubuntu24.04`); the `.rpm` from Fedora 44 needs
  2.43 (Fedora 44+). In the roots `try.sh` enters as a namespace's only
  user, dpkg runs under fakeroot: Ubuntu's package scripts give folders
  to groups the namespace does not have.
- The AUR package disables makepkg's LTO (`options=('!lto')`): it would
  compile crates' C (MuPDF) to GCC bytecode, which Rust's linker (lld)
  cannot read.
- No font is a dependency. A system lacking one with a symbol Skwir
  draws as text (the sort arrow) shows a box; every desktop has one
  (DejaVu, Noto).
- The Debian maintainer field says "The Skwir authors" until someone
  answers for the package. The AUR maintainer is whoever publishes it
  there; publishing is the user's to do.
- Skwir's icon (2026-10-11; before, the desktop entry named the theme's
  file manager icon) is one SVG, the UI's own
  (`crates/ui/assets/org.skwir.skwir.svg`), installed as
  `/usr/share/icons/hicolor/scalable/apps/org.skwir.skwir.svg`, which
  the desktop entry names and `linux/check.sh` checks.
