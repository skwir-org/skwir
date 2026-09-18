# ADR-004: Optional PDFium document reader

Status: accepted, 2026-09-18. Supersedes ADR-003's PDF engine choice and its
single-request lifetime for the optional reader; Poppler remains supported.
ADR-005 subsequently upgrades Poppler to the same reader protocol and features.

## Context

Image-only page previews cannot provide text selection, search, link navigation
or outlines. Repeated command startup and document parsing also cost time on
new pages. A complete reader needs a document session, while bibli must retain
its small startup footprint and responsive UI. Prefer a permissively licensed
engine over MuPDF's AGPL/commercial distribution choices.

## Decision

Use the installed PDFium shared library through `pdfium-render`, in a separate
`bibli-pdf-worker` executable. Do not statically link a PDF engine into bibli.
The worker is a workspace member built by the normal recipes; the native
library is optional. `nix develop .#pdf` supplies the pinned Nix runtime.
The binding is pinned to 0.8.35 with the `pdfium_6721` ABI surface, exercised
against PDFium 7749. Future library/binding upgrades must rerun native checks.

`preview-documents.backend` is `auto`, `pdfium`, or `poppler`. Auto prefers
PDFium and falls back only when its worker or library is unavailable. An
explicit PDFium choice reports missing dependencies. Office conversion retains
ADR-003's isolated LibreOffice path and feeds the converted PDF to the reader.
Poppler keeps its existing compressed disk cache and image-only controls.

Core owns toolkit-neutral document data and a session trait. The platform
manager owns a child, private scratch directory, and serialized request queue.
The child opens one document, returns metadata as bounded JSON lines and pages
as RGBA files, and releases page/bitmap handles after each request. The parent
transfers an owned pixel buffer into GPUI and swizzles it in place, avoiding
full-page copies and allocator retention across scheduler threads. No native
library or document handle enters the GPUI process.

Session lifetime is linked to both preview selection and provider registration.
Dropping the UI handle never blocks: a manager thread kills/reaps the child and
removes scratch files. Cancelled/failed requests discard the child; a later
request can reopen it. Source size/mtime/ctime changes invalidate the session.
No daemon or persistent document database is introduced.

Limits: 2048 pixels per side, 32 MiB metadata replies, 1 MiB diagnostics,
200,000 characters per page, 4096 links/outlines, 500 search matches, 4096-byte
queries, 15-second open and 30-second request deadlines. Cancellation kills the
Unix process group. These bounds do not impose an OS-level child memory cap.
The PDFium process provides crash/cancellation isolation, not a filesystem
sandbox; the existing Office converter sandbox is unchanged.

The UI keeps selection, current page, zoom, find results and outline visibility
in the active preview. A custom element computes fit/zoom in prepaint and uses
the same transform for pixels, highlights and hit testing. Only the foreground
reader surface is interactive when peek overlays the side panel. All actions
route through `ui.reader_*` commands. Visible page requests cancel an ongoing
search. Internal page links and HTTP(S) URI actions are supported; other action
schemes do not execute. Editing, forms interaction and JavaScript actions are
not exposed by the reader.

## Licensing and alternatives

[PDFium's current license](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/LICENSE)
contains BSD-style and Apache-2.0 terms; distributions must retain the applicable
third-party notices as well. The
[Rust binding](https://github.com/ajrcarey/pdfium-render) is MIT OR Apache-2.0.
This avoids adding MuPDF's AGPL/commercial licensing choice to this integration.
[PDF.js](https://github.com/mozilla/pdf.js/blob/master/LICENSE) is another
permissively licensed option (Apache-2.0), but embedding its reader would add a
browser/webview runtime to the native GPUI application.

## Consequences and validation

Text selection/copy, whole-document search, links and outlines are available
when PDFium loads, including converted Office documents. Selection is currently
per page, state lasts for the selected document, and scans need an existing
text layer. Password prompts, OCR, annotations/editing, cross-page selection,
continuous scrolling and persistent reading positions are deferred.

Default tests cover worker lifetime, cancellation/restart, source invalidation,
selection and fit geometry without native PDFium. `tools/pdf/check.py` exercises
actual text/search geometry on normal, cropped and rotated pages, outlines, links,
missing libraries and corrupt files with generated documents. Record speed and
resource measurements separately in `docs/pdf-comparison.md`; renderer numbers
are not end-to-end UI measurements or a guarantee for arbitrary PDFs.
