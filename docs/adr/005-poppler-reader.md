# ADR-005: Poppler reader with the shared document protocol

Status: accepted, 2026-09-18. Supersedes ADR-004's image-only Poppler
integration.

## Context

ADR-004 gave PDFium a reader (selection, search, links, outlines) but left
Poppler as image-only pages from its command-line tools. PDFium's library is
optional and the default dev shell provides only Poppler, so without PDFium
there was no reader.

## Decision

- **Worker.** `skwir-poppler-worker` uses `poppler-rs` 0.25 and the
  installed `poppler-glib` with Cairo, keeping one document open per session.
  It is optional at runtime and built by the workspace and run recipes;
  building it needs Poppler, GLib and Cairo development packages (in the
  default Nix shell), building `skwir` alone does not. Neither worker is a
  dependency of the main binary. PDFium's runtime comes from the `pdf` shell,
  via a git-ignored `.envrc.local` containing `use flake .#pdf`.
- **One protocol, one UI.** Both workers implement the bounded document
  protocol (page pixels, character geometry, search, URI and page links,
  outlines) behind one session manager and one reader UI. Poppler's missing
  safe bindings are confined to a documented native adapter. Text geometry is
  normalized from top-left coordinates, search and link geometry from
  bottom-left; the native APIs already handle crop and rotation.
- **Engine order.** `auto` prefers PDFium, then the Poppler worker, then the
  Poppler CLI image fallback with its disk cache. An explicit engine choice
  requires that reader, and a document error never switches engines.
  Converted Office PDFs take the same route.
- Revisited pages are rendered again from the parsed document kept open, as
  with PDFium; the UI keeps the displayed page until the new one is ready.

## Consequences

- Both backends have the same features and limits: per-page selection,
  text-layer search, page and HTTP(S) links, outlines, fit/zoom, page
  navigation; no OCR, password prompts, annotation editing, forms or
  cross-page selection. Continuous scrolling came later in the shared UI with
  no protocol change.
- Native workers isolate crashes and cancellation; they are not filesystem
  sandboxes.
- Poppler stays
  [GPL](https://gitlab.freedesktop.org/poppler/poppler/-/blob/master/COPYING).
  A separate process relicenses neither the library nor the linked helper;
  distributors honor the helper's linked dependencies. Our adapter and the
  MIT bindings do not change Poppler's terms.
- Both workers run the same synthetic native checks, all four rotations with
  cropping included. Benchmarks use equivalent requests (metadata and search
  included), report child PSS and CPU, and keep renderer numbers apart from
  application latency and footprint, in `docs/pdf-comparison.md`.
