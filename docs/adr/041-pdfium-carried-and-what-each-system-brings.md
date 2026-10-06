# ADR-041: PDFium carried, and what each system brings

Status: accepted, 2026-10-06 (Phase 5, as the user chose). Amends
ADR-040's packages; builds on ADR-004 (the PDFium reader), ADR-005
(Poppler) and ADR-014 (MuPDF).

## Context

bibli reads PDFs through a helper of one of three engines: PDFium,
Poppler or MuPDF. `auto` prefers PDFium, then Poppler; MuPDF only when
chosen. Each has the same reader features: pages, text selection and
copy, search, links, the outline. No distribution packages PDFium, so
the Linux packages relied on Poppler, whose libraries cost 33–39 MB on
a minimal system, and macOS and Windows had no PDF engine at all. A
PDF's thumbnail, and the pages of an office document converted to PDF,
were drawn by Poppler's `pdftoppm`, which macOS and Windows lack too.

PDFium's prebuilt libraries (bblanchon/pdfium-binaries, the build most
projects ship) are self-contained: 3.5–3.9 MB downloaded per system
(7.9 MB installed on Linux), needing only the C library (glibc 2.16 on
Linux), their licences all permissive (PDFium's BSD-3-Clause, and
FreeType, HarfBuzz, ICU, libjpeg-turbo, OpenJPEG, libpng, LittleCMS,
zlib, Abseil…). A package outside a distribution's own archive brings
what the distribution lacks; Chrome, VS Code and Zed do.

macOS and Windows have no package manager: an application brings what
it needs, and each system has frameworks of its own (Quick Look, the
Windows shell's thumbnail handlers, AVFoundation, Media Foundation).

## Decision

1. **bibli's packages carry PDFium**, the release pinned in
   `tools/package/pdfium.lock` (one tag, each archive's SHA-256), fetched
   and checked by `tools/package/pdfium.sh` (the PKGBUILD names the same,
   which its build checks), its notices installed beside it. The PDFium
   helper looks for it beside itself, and in a Mac bundle's
   `Frameworks`, after the library the settings name and before the
   system's. So a PDF opens, with selection and search, wherever bibli is
   installed.
2. **A PDF is drawn by the reader's engine first**, everywhere bibli
   draws one: its thumbnail and an office document's (`document::
   first_page`, `auto`'s order), `pdftoppm` only where no engine is.
3. **Linux packages bring only what the distribution lacks** (PDFium),
   and depend on, recommend or suggest the rest. What each program links
   is found by the packaging tools, never listed by hand:
   - Debian and Fedora: `bibli` (the app, PDFium and its helper, the
     archive and 3D model helpers), and packages of their own,
     `bibli-media` (recommended: GStreamer's libraries and its plugins),
     `bibli-poppler`, `bibli-mermaid` and `bibli-mupdf` (suggested).
   - Arch: one `bibli` with the Poppler reader and the player, their
     libraries optional (Arch's way), and `bibli-mermaid` and
     `bibli-mupdf` of their own.
   A helper whose libraries are absent says so (`--probe`), and bibli
   shows its engine as not installed.
4. **macOS and Windows bring what is needed, the system's frameworks
   first.** Their packages carry PDFium (in the bundle's `Frameworks`,
   beside the program on Windows) and the type database (ADR-037), and
   leave out Mermaid and MuPDF, as Linux only suggests them:
   - thumbnails are the system's own (`system_thumbs`: Quick Look,
     the shell's `IShellItemImageFactory`), as the Finder and Explorer
     show them: Office documents, RAW, HEIC, films, fonts;
   - so are icons (added the same day, `system_icons`): `NSWorkspace`'s
     on macOS, the shell's system image list on Windows, by a file's
     extension, by its path where the icon is the file's own (an
     application, a shortcut), the trash and drives as the system's
     stock icons; bibli reads no icon theme there;
   - the programs bibli uses when installed (LibreOffice, Pandoc,
     ImageMagick, ffmpeg, exiftool) are found where they are installed:
     Homebrew's and MacPorts' folders searched on macOS (an application
     started from the Finder has the system's folders alone), and
     LibreOffice where it installs itself;
   - audio and video will be played through the system's own framework
     (AVFoundation, Media Foundation: codecs and hardware decoding come
     with the system), not GStreamer bundled (100–200 MB, and its codecs'
     licensing): a helper per system, written on the machines (5b);
   - archives: the archive helper built with libarchive linked in
     statically, on the machines (5b).
5. **We answer for the PDFium we carry**: a newer release is taken with
   each bibli release, or sooner for a security fix (its tag and sums in
   `pdfium.lock` and the PKGBUILD).

## Consequences

- PDFs work out of the box on every system; Poppler becomes an
  alternative engine, suggested.
- The Linux packages carry 7.9 MB more and depend on less: on a minimal
  Debian 13, Poppler's libraries no longer come with bibli.
- PDFium's security updates are ours to ship. Its helper, and the other
  readers', run in the sandbox where the system has one (since the same
  day, `docs/invariants/reader.md`): a malformed PDF that takes over the
  library reaches its document and its scratch folder, nothing of the
  user's.
- On macOS and Windows, nothing of point 4 has run on the machines:
  Quick Look's thumbnails and the packages are compiled only; the
  Windows shell's thumbnails ran under Wine, whose shell draws none,
  and the Windows PDF helper loaded the pinned `pdfium.dll` beside it
  under Wine.
