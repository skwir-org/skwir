# ADR-014: MuPDF as an optional third reader engine

Status: accepted, 2026-09-24. Amends ADR-004, which preferred PDFium partly
to keep MuPDF's AGPL out of the reader integration.

## Context

MuPDF renders PDF (and XPS, EPUB, FictionBook, CBZ, SVG and images) with a
small memory footprint, and lays reflowable EPUB out into pages with its own
HTML/CSS engine. We want to measure it against PDFium and Poppler on the
same requests, and to try it later as an "original layout" alternative for
EPUB. It is licensed under the AGPL-3.0 or commercially.

## Decision

Add `bibli-mupdf-worker`, a third implementation of the shared document
protocol (ADR-004/005), in its own process like the others. It links
MuPDF 1.27.2 through the `mupdf` crate 0.8 (AGPL-3.0), pinned exactly, with
default features off: the 14 standard PDF fonts only, no JavaScript, no OCR,
and not the crate's own system-font lookup (see the addendum for the fonts
it gets instead). `mupdf-sys` builds MuPDF from the source it bundles,
with MuPDF's own third-party libraries (MuPDF relies on its own lcms2 fork,
so linking the system's would not match); the default Nix shell already has
the C toolchain and libclang its bindings need.

The worker is outside `default-members`, like the Poppler worker: the
application, plugins and default tests build without it. `just workers`,
`just run`, `just test-all`, `just lint-all` and CI build it; `just
mupdf-check` runs the shared synthetic reader fixtures against it, in CI too.

`preview-documents.backend` gains `mupdf`. `auto` is unchanged (PDFium, then
Poppler): MuPDF is used only when chosen, until measurements say otherwise.

## Licensing

Our worker source stays MIT OR Apache-2.0 like the rest of bibli. The worker
executable links MuPDF, so whoever distributes it distributes it under the
AGPL-3.0, with MuPDF's source; as with the GPL Poppler worker, the main
binary never links it and talks to it only over the worker protocol, and no
other crate depends on it. Packagers who do not want the AGPL omit this one
executable; the plugin reports the requirement as missing and the other
engines work as before. The AGPL's network clause concerns software offered
over a network; bibli is a desktop application.

## Consequences

- A first build compiles MuPDF: about 70 s in debug and 95 s in release for
  the three PDF workers at six jobs on the reference machine. The stripped
  worker is about 5.8 MB (PDFium's worker 1.8 MB plus its 4.7 MB library;
  Poppler's 0.6 MB plus the system's libraries).
- On the synthetic comparison (`docs/pdf-comparison.md`), MuPDF opened
  fastest and used the least worker memory on every fixture, rendered text
  and scanned pages as fast as the others, and was much slower on the
  vector fixture of 1,500 page-sized strokes, where its anti-aliased
  rasterizer costs about 1.4 s a page in `mutool` itself.
- The worker offers the same reader features as the others: pixels, text
  geometry and selection, search, page and web links, outlines, fit and zoom.
  Encrypted documents are refused, as by the other engines.
- EPUB through MuPDF is not wired yet: it needs the `epub` and `html`
  features and a presentation of its own (done, see the addendum).

## Addendum (2026-09-24): books, text size and fonts

- **A book engine, not a button.** `preview-epub.backend` chooses who reads
  a book, like `preview-documents.backend` for a PDF: `builtin` (reading
  mode, and drawn pages for fixed-layout books, ADR-012/013), or `mupdf`,
  which opens every book in the document reader, laid out into pages by
  MuPDF. `auto` is the built-in engine. With `mupdf` the plugin answers
  `open_document` and never `inspect`; a missing worker is an error that
  names the setting.
- **Layout is an argument.** The worker takes `--layout=width,height,em`
  (points, bounded) and lays a reflowable document out before greeting;
  `DocumentInfo::reflowable` says it did. A book is laid out on a 420 × 595
  point page with its body text at the preview's text size
  (`PreviewRequest::text_size`, 16 px unless changed).
- **Size what is read.** In the reader, Ctrl++ / Ctrl+- / Ctrl+0 on a
  reflowable document change the text size and open the book again at it,
  in a new worker (`reader_reopen`); a PDF's keys still zoom. The toolbar
  says "Larger text" and "Smaller text" then. The reader lands where it was
  read, not at the same page number: the first words of the page and its
  share of the book are its anchor (`reader::Anchor`), and the new worker is
  asked for the page nearest that share holding the words
  (`WorkerRequest::Locate`, searched outward, at most 24 pages each way).
  Engines whose pages never move answer `Located(None)`. Size steps with no
  move between them keep the first anchor, so repeated steps do not drift.
  MuPDF's own bookmarks cannot do this: they point into the laid-out tree of
  one process.
- **Fonts from the desktop.** A worker-side `FontLoader` asks fontconfig
  (`fc-match`, no linked library) for what a document uses but does not
  carry: a family by name (taken only if fontconfig has that family; else
  MuPDF's standard fonts stand in), a style's generic family (`serif`,
  `sans-serif`, `monospace`, as any reader on the desktop would), and text in
  a script no font covers (Chinese, Japanese, Arabic, Devanagari…), asked for
  with the script's language and taken from the first face whose coverage
  includes it. The crate's `system-fonts` feature is not used: it answers
  by name only, and its `font-kit` shares a `fontconfig` binding with gpui's
  copy of `font-kit`, whose dlopen feature breaks it when the workspace is
  built as one (`cargo build --workspace` fails).
