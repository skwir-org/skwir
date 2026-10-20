# ADR-041: PDFium carried, and what each system brings

Status: accepted, 2026-10-06 (Phase 5, as the user chose). Amends
ADR-040; builds on ADR-004 (PDFium), ADR-005 (Poppler), ADR-014
(MuPDF). Built on Linux; macOS and Windows compiled only.

## Context

Skwir reads PDFs through a helper for PDFium, Poppler or MuPDF (`auto`:
PDFium, then Poppler; MuPDF only when chosen), all with the same reader
features.

- No distribution packages PDFium, so Linux packages relied on Poppler
  (33-39 MB of libraries on a minimal system). macOS and Windows had no
  PDF engine and no `pdftoppm`, which drew PDF and office thumbnails.
- PDFium's prebuilt libraries (bblanchon/pdfium-binaries) are
  self-contained: 3.5-3.9 MB per system, 7.9 MB installed on Linux,
  needing only glibc 2.16, all licences permissive (BSD-3-Clause and
  FreeType, HarfBuzz, ICU, zlib…).
- Packages from outside a distribution bring what it lacks (Chrome, VS
  Code, Zed do). macOS and Windows have no package manager but have
  frameworks (Quick Look, shell thumbnail handlers, AVFoundation, Media
  Foundation).

## Decision

1. **Skwir's packages carry PDFium**, pinned in
   `tools/package/pdfium.lock` (tag, SHA-256 per archive), fetched and
   checked by `tools/package/pdfium.sh` (the PKGBUILD checks the same),
   with its notices. The helper looks for it after the library the
   settings name and before the system's: beside itself, or in a Mac
   bundle's `Frameworks`.
2. **A PDF is drawn by the reader's engine first**, thumbnails and
   office documents included (`document::first_page`, `auto`'s order);
   `pdftoppm` only where no engine is.
3. **Linux packages bring only what the distribution lacks** (PDFium);
   packaging tools find the rest:

   | Distribution | Packages |
   |---|---|
   | Debian, Fedora | `skwir` (app, PDFium, archive and 3D helpers); `skwir-media` (recommended: GStreamer); `skwir-poppler`, `skwir-mermaid`, `skwir-mupdf` (suggested) |
   | Arch | `skwir` with the Poppler reader and player, their libraries optional; `skwir-mermaid`, `skwir-mupdf` |

   A helper missing its libraries says so (`--probe`); Skwir shows its
   engine as not installed.
4. **macOS and Windows: system frameworks first.** Packages carry PDFium
   (bundle `Frameworks`; beside the program on Windows) and the type
   database (ADR-037), without Mermaid or MuPDF.
   - Thumbnails (`system_thumbs`) and icons (`system_icons`, added the
     same day) are the system's: Quick Look and `NSWorkspace`;
     `IShellItemImageFactory` and the system image list. Icons by
     extension, by path for a file's own (an app, a shortcut), stock
     icons for trash and drives; no icon theme.
   - Installed programs (LibreOffice, Pandoc, ImageMagick, ffmpeg,
     exiftool) are searched in Homebrew's and MacPorts' folders (a
     Finder-started app has only system paths) and LibreOffice's own.
   - Audio and video will use AVFoundation and Media Foundation (system
     codecs and hardware decoding), not a bundled GStreamer (100-200 MB,
     codec licensing): a helper per system, written on the machines
     (5b).
   - The archive helper links libarchive statically, built on the
     machines (5b).
5. **We answer for the PDFium we carry:** updated with each release, or
   sooner for a security fix.

## Consequences

- PDFs work out of the box everywhere; Poppler becomes a suggested
  alternative.
- Linux packages grow 7.9 MB and depend on less: a minimal Debian 13 no
  longer pulls Poppler's libraries.
- PDFium security updates are ours. Its helper runs sandboxed where the
  system can (`docs/invariants/reader.md`): a hostile PDF reaches its
  document and scratch folder, nothing of the user's.
- Point 4 has not run on the machines: Quick Look and the packages are
  compiled only. Under Wine, the Windows shell drew no thumbnails and
  the PDF helper loaded the pinned `pdfium.dll` beside it.

## Amendments

- 2026-10-20 (ADR-074, ADR-075): the archive helper's libarchive is built
  on Linux, as Windows' and macOS's packages are, not on the machines
  (`tools/package/libarchive.sh`): static, from nixpkgs' sources as
  nixpkgs patches them, pinned by the flake's lock, with xz's liblzma,
  zstd and lz4, and on Windows zlib and bzip2. macOS gives its own zlib,
  bzip2, iconv and Expat, Windows its cryptography (CNG) and XmlLite; the
  helper reads what Linux's libarchive reads, LZO aside. Its security
  updates are ours, as PDFium's: they come with nixpkgs' (`nix flake
  update`).
- 2026-10-20: ADR-076, proposed, would replace point 4's helpers per
  system for audio and video by the GStreamer helper Linux has, with a
  trimmed GStreamer carried.
