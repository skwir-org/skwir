# ADR-005: Poppler reader with the shared document protocol

Status: accepted, 2026-09-18. Supersedes ADR-004's image-only Poppler integration.

## Decision

Add `skwir-poppler-worker`, using `poppler-rs` 0.25 and the installed
`poppler-glib` library with Cairo. Keep one document open for the session.
The executable is optional at runtime, built by workspace and run recipes.
Building it requires Poppler, GLib and Cairo development packages, supplied
by the default Nix shell. Building `skwir` alone does not require them.
Neither worker is a dependency of the main binary. The default shell provides
Poppler only; PDFium's optional runtime comes from the `pdf` shell, entered
through a git-ignored `.envrc.local` containing `use flake .#pdf`.

Both workers implement the existing bounded document protocol: page pixels,
character geometry, search, URI/page links and outlines. Reuse one platform
session manager and one reader UI, including lifecycle cancellation, source
invalidation, page/zoom controls, text selection/copy and navigation. Poppler's
missing safe bindings are confined to a documented native adapter. Normalize
text geometry from top-left coordinates and search/link geometry from
bottom-left coordinates; native APIs already account for crop and rotation.

`auto` prefers PDFium, then the Poppler worker when PDFium is unavailable.
If neither reader is available, retain the existing Poppler CLI image fallback
and disk cache. Explicit engine choices require the selected reader; document
errors never trigger another engine. Converted Office PDFs use the same route.
The Poppler reader rerenders revisited pages, retaining the parsed document,
as PDFium does; the shared UI retains the displayed page while work completes.

## Consequences

The two backends expose the same current reader features and limitations:
per-page selection, text-layer search, page links/HTTP(S) links, outlines,
fit/zoom and page navigation. Neither adds OCR, password prompts, annotation
editing, forms or cross-page selection. Continuous scrolling was subsequently
added to the shared UI without changing this protocol. Native workers
isolate crashes and cancellation, but are not filesystem sandboxes.

Poppler retains its [GPL license](https://gitlab.freedesktop.org/poppler/poppler/-/blob/master/COPYING).
Process separation does not relicense the library or the linked helper;
distributors must honor the helper's linked dependencies. Our Rust adapter
source and its MIT Rust bindings do not change Poppler's licensing terms.

Use the same synthetic native checks for both workers, including all four
rotations with cropping. Benchmark equivalent requests (including metadata
and search), report child PSS and CPU, and distinguish renderer measurements
from full application latency and footprint in `docs/pdf-comparison.md`.
