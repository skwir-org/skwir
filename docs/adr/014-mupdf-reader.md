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
no system-font lookup. `mupdf-sys` builds MuPDF from the source it bundles,
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
  features and a presentation of its own.
