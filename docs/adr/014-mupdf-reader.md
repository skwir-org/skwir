# ADR-014: MuPDF as an optional third reader engine

Status: accepted, 2026-09-24. Built. Amends ADR-004, which preferred PDFium
partly to keep MuPDF's AGPL out of the reader. Amended 2026-10-06 by
ADR-042: skwir's code, the worker's included, is Apache-2.0 alone; the
worker executable is still distributed under the AGPL.

## Context

MuPDF renders PDF (and XPS, EPUB, FictionBook, CBZ, SVG, images) in little
memory and paginates reflowable EPUB with its own HTML/CSS engine. We want
to measure it against PDFium and Poppler, and try it as an "original
layout" EPUB engine. It is AGPL-3.0 or commercial.

## Decision

- **A third worker,** `skwir-mupdf-worker`, implements the document
  protocol (ADR-004/005) in its own process. It links MuPDF 1.27.2 through
  the `mupdf` crate 0.8, pinned exactly, default features off (14 standard
  PDF fonts, no JavaScript, no OCR, no system-font lookup). `mupdf-sys`
  builds MuPDF from its bundled source with MuPDF's own third-party
  libraries (it needs its own lcms2 fork).
- **Outside `default-members`,** like Poppler's. `just workers`, `just run`,
  `just test-all`, `just lint-all` and CI build it; `just mupdf-check` runs
  the shared reader fixtures against it, in CI too.
- **Opt-in:** `preview-documents.backend` gains `mupdf`; `auto` stays
  PDFium, then Poppler, until measurements say otherwise.
- **Licensing.** The main binary never links MuPDF and talks to the worker
  only over its protocol; no other crate depends on it (as with the GPL
  Poppler worker). The worker executable is distributed under the AGPL-3.0
  with MuPDF's source; packagers who refuse that omit it, and the plugin
  reports it missing. The AGPL's network clause concerns software offered
  over a network; skwir is a desktop application.

## Consequences

- A first build compiles MuPDF: ~70 s debug, ~95 s release for the three
  PDF workers at six jobs (reference machine, 2026-09-24).
- Stripped: MuPDF worker ~5.8 MB; PDFium's 1.8 MB plus a 4.7 MB library;
  Poppler's 0.6 MB plus system libraries.
- On the synthetic comparison (`docs/pdf-comparison.md`), MuPDF opened
  fastest and used the least memory on every fixture, matched the others on
  text and scans, and was much slower on 1,500 page-sized strokes (~1.4 s a
  page in `mutool` itself).
- Same reader features as the others; encrypted documents are refused.

## Amendments

### 2026-09-24: books, text size and fonts

- **Book engine:** `preview-epub.backend` is `builtin` (also `auto`:
  reading mode, drawn fixed-layout pages, ADR-012/013) or `mupdf`, which
  opens every book in the document reader. With `mupdf` the plugin answers
  `open_document`, never `inspect`; a missing worker is an error naming the
  setting.
- **Layout is an argument:** `--layout=width,height,em` (points, bounded);
  the worker paginates before greeting (`DocumentInfo::reflowable`). Books
  use a 420 × 595 pt page, body text at the preview's text size
  (`PreviewRequest::text_size`, 16 px by default).
- **Text size:** on a reflowable document Ctrl++ / Ctrl+- / Ctrl+0 change
  the text size ("Larger text", "Smaller text") and reopen the book in a
  new worker (`reader_reopen`); on a PDF they zoom. The reader returns to
  the passage, not the page number: the anchor (`reader::Anchor`) is the
  page's first words and its share of the book, located in the new layout
  by `WorkerRequest::Locate` (at most 24 pages each way). Engines whose
  pages never move answer `Located(None)`. Consecutive steps keep the first
  anchor, so they do not drift. MuPDF's bookmarks cannot do this: they
  point into one process's layout.
- **Fonts from the desktop:** a worker-side `FontLoader` asks fontconfig
  (`fc-match`, nothing linked) for missing fonts: a named family (if
  fontconfig has it, else MuPDF's standard fonts), a generic family, and a
  font for a script nothing covers (CJK, Arabic, Devanagari…), asked by
  language. Rejected: the crate's `system-fonts` feature, which matches by
  name only and whose `font-kit` clashes with gpui's over `fontconfig`,
  breaking `cargo build --workspace`.
