# ADR-061: DjVu read by DjVuLibre or djvu-rs

Status: accepted, 2026-10-15. Built. DjVuLibre's worker on Linux;
djvu-rs's on every system (macOS and Windows compiled only).

## Context

DjVu is the format of many scanned books. Skwir showed it only as
ImageMagick draws it (ADR-031), where ImageMagick's build reads it: the
first page as a picture of at most 1024 pixels, no other page, no text,
no outline. ImageMagick decodes the whole page before shrinking it: an A4
page to 1024 px took 0.13 s and 99 MB at 300 dpi in black and white,
0.31 s and 176 MB in colour, 0.57 s and 304 MB at 600 dpi (reference
machine, synthetic pages, 2026-10-15).

Options:

| Option | Measured | Verdict |
|---|---|---|
| DjVuLibre (`libdjvulibre`, C++, GPL v2 or later), the library every DjVu viewer uses | `ddjvu` to 1024 px: 0.02 s and 10 MB (300 or 600 dpi, black and white), 0.09 s and 24 MB in colour; 2.2 MB of library, which needs libjpeg and the C++ runtime | Chosen, in a worker of its own: the reference, but a system library (none on Windows), and memory-safety bugs fixed as late as 2025 |
| djvu-rs 0.42 (pure Rust, MIT), from April 2026 | 2 to 28 ms a page at 1024 px, 10 MB; its text, outline and links read alike | Chosen too, in a worker of its own: no library to install anywhere; but young (0.1 to 0.42 in six months, one author), about 100 `unsafe` blocks, no fuzzing said |
| DjVuLibre's command-line tools (`ddjvu`, `djvutxt`, `djvused`), a process a page | as `ddjvu` above, plus a process and a sandbox each | Rejected: the worker protocol keeps a document open; the tools re-read it for each page |

## Decision

### Two engines behind the reader's protocol

- **Two workers** speak the protocol of the PDF readers (ADR-004/005,
  `skwir_core::document::worker`): `skwir-djvulibre-worker` links
  DjVuLibre through its C API (`ddjvuapi.h`, declared by hand: its
  s-expressions are copied into Rust values and given back at once);
  `skwir-djvu-worker` uses djvu-rs, pinned exactly (`=0.42.0`, its default
  features: decoding). Both are `Engine`s of `skwir_platform::document`,
  started, isolated (`sandbox::untrusted_reading`), timed out and
  restarted by the session manager every document engine shares.
- **The `djvu` plugin** (Previews) takes `image/vnd.djvu` (a page, "DjVu
  image") and `image/vnd.djvu+multipage` ("DjVu document"): the reader
  for both, and the first page as a thumbnail at the fallback stage (the
  desktop's thumbnailers first, ImageMagick last).
- **`djvu.engine`:** Automatic (the default) tries DjVuLibre, then
  djvu-rs; or one of them. As for PDFs, Automatic gives way only to an
  engine that is absent (no helper, or its library missing), never to one
  that fails on the document. A thumbnail takes Automatic's order
  whatever the reader's.
- **Packages:** djvu-rs's helper is in every package (Linux, macOS,
  Windows) and in `default-members`; DjVuLibre's is outside, like
  Poppler's (`just workers`, `just test-all`), and a package of its own
  where packages are split (`skwir-djvulibre`, suggested; on Arch, in
  skwir with DjVuLibre an optional dependency). Not on macOS or Windows
  yet: their packages carry no native library for a helper.
- **Licensing:** the main program links neither. DjVuLibre's helper,
  Skwir's own code (Apache-2.0) linked with a GPL v2-or-later library, is
  distributed under the GPL v3, as Poppler's is (ADR-042).

### What both engines read

- A page is drawn whole at the side asked, its own size (what 100% shows)
  its pixels at its resolution: an A4 page is 794 × 1123 CSS pixels at
  any dpi. A page the file says is turned is drawn turned, its words
  turned with it (`Rect::turned`): the text layer is in the page's
  pixels as stored, links in the page's as shown.
- Words: the text layer to its words (a word's characters, when it has
  them, give their edges). Words made into the reader's characters and
  searched there by `skwir_core::document::words`, the code the pages of
  a plugin from outside Skwir already used (ADR-047), moved there for the
  three. Words lying over one another join into one run over the box
  around them: many scans' text layers place lines and give each word
  its line's box.
- Links: an area (a rectangle, an oval, a polygon's box) to a page
  (`#12`, `#+1`, `#-1`, a page's file name) or out. The outline (`NAVM`)
  to headings and their pages.
- Not read: an indirect document (its pages in files of their own, which
  the isolated helper cannot see: the page asked says so), annotations
  other than links, metadata.

### djvu-rs, drawn smooth and in place

- **Smoothed by halves.** Shrunk to half or less (and up to 4096 px),
  djvu-rs draws the page at twice the side and averages each two by two
  pixels: black and white text no longer breaks up. Its Lanczos filter
  is exact too, but decodes the whole page first (a 600 dpi A4 page to
  1024 px: 0.37 s and 162 MB, against 0.01 s and 18 MB): rejected.
- **At sizes it draws in place.** djvu-rs 0.42 steps through a page by
  `page × 16 / size` sixteenths of a pixel, truncated: at a size whose
  step is no whole number of sixteenths, the drawing is larger than the
  page, by 2% when it is shrunk to two thirds, and the words' boxes miss
  their ink. The helper draws at the largest size below the side asked
  whose steps are truncated by at most 0.2% (at most 7% smaller; the
  reader shows any size at the page's own). `just djvu-check` measures
  where the ink lands for both engines.
- **Kept in memory:** the document file (djvu-rs reads it whole: up to
  1 GiB), and 64 MB of decoded pages beside the page drawn last.

### DjVuLibre

- Three pages are kept decoded: the reader draws a page small, then
  sharp. What the library says of an error is the error.
- Page labels: a page titled otherwise than its file's name (`ii`).
  djvu-rs does not say them.

## Consequences

- Measured once through the protocol (release, reference machine,
  2026-10-15; synthetic A4 pages):

  | Engine | Black and white 300 dpi, 1024 / 2048 px | Colour 300 dpi, 1024 / 2048 px | 600 dpi, 1024 / 2048 px | Peak memory |
  |---|---|---|---|---|
  | DjVuLibre | 6 / 26 ms | 70 / 130 ms | 7 / 20 ms | 67 MB (three pages, one in colour), 22 MB (600 dpi) |
  | djvu-rs | 13 / 12 ms | 73 / 15 ms | 13 / 46 ms | 29 MB, 57 MB |

- Stripped: djvu-rs's helper 1.3 MB; DjVuLibre's 0.6 MB beside its
  2.2 MB library.
- Ink where the page has it, on `just djvu-check`'s documents: DjVuLibre
  within 0.18% of the page, djvu-rs within 0.32% (one pixel at 300 px).
- A one-page DjVu is a picture by its type (`mime::Kind::Image`), a
  document of pages a document; both are named `.djvu`, which only the
  bytes settle. The whole screen's walk through the pane's pictures now
  takes a file for one only where its name settles its type
  (`fullscreen::is_picture`): it passes every DjVu, which would otherwise
  land on a book and leave the arrows to its pages.
- djvu-rs moves weekly: an update is read, its `djvu-check` run, and
  taken as any pinned dependency is.
