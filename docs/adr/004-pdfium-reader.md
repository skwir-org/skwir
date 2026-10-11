# ADR-004: Optional PDFium document reader

Status: accepted, 2026-09-18. Supersedes ADR-003's PDF engine and
single-request lifetime for the optional reader; Poppler stays supported
(ADR-005 gives it the same reader). Amended by ADR-014 (MuPDF as a third
engine) and ADR-015 (pages rendered at the size shown). ADR-041 carries
PDFium in the packages.

## Context

Image-only page previews cannot select text, search, follow links or show
outlines, and every new page paid for a command start and a document parse.
A reader needs a document session, without losing Skwir's small startup or
responsive UI. A permissive engine is preferred over MuPDF's AGPL/commercial
choice.

## Decision

- **Engine.** The installed PDFium shared library through `pdfium-render`, in
  a separate `skwir-pdf-worker`; no PDF engine is linked into Skwir. The worker
  is built by the normal recipes; the library is optional (`nix develop .#pdf`
  supplies the pinned runtime). The binding is pinned to 0.8.35 with the
  `pdfium_6721` ABI surface, exercised against PDFium 7749; any library or
  binding upgrade reruns the native checks.
- **Choice.** `preview-documents.backend` is `auto`, `pdfium` or `poppler`
  (ADR-014 adds `mupdf`). Auto prefers PDFium and falls back only when its
  worker or library is missing; an explicit PDFium choice reports what is
  missing. Office documents keep ADR-003's isolated LibreOffice conversion and
  feed the PDF to the reader.
- **Process boundary.** Core owns toolkit-neutral document data and a session
  trait; the platform manager owns the child, a private scratch directory and
  a serialized request queue. The child opens one document, answers metadata
  as bounded JSON lines and pages as RGBA files, and releases page and bitmap
  handles after each request. The parent hands an owned buffer to GPUI and
  swizzles it in place (no full-page copy, no allocator retention across
  scheduler threads). No native library or document handle enters the GPUI
  process.
- **Lifetime.** A session follows the preview selection and the provider's
  registration. Dropping the UI handle never blocks: a manager thread kills
  and reaps the child and removes scratch files. A cancelled or failed request
  discards the child; the next request reopens it. A change of the source's
  size, mtime or ctime invalidates the session. No daemon, no document
  database.
- **Limits.** 2048 px per side (ADR-015: now the size shown), 32 MiB metadata
  replies, 1 MiB diagnostics, 200,000 characters per page, 4096 links or
  outline entries, 500 search matches, 4096-byte queries, 15 s to open, 30 s
  per request. Cancellation kills the Unix process group. None of this caps
  the child's memory. The process isolates crashes and cancellation; it is
  not a filesystem sandbox.
- **UI.** The active preview keeps selection, page, zoom, find results and
  outline visibility. A custom element computes fit and zoom in prepaint and
  uses one transform for pixels, highlights and hit testing. Only the
  foreground reader is interactive when peek overlays the side panel. Actions
  are `ui.reader_*` commands. Requests for visible pages cancel a running
  search. Internal page links and HTTP(S) URIs work; other action schemes,
  editing, form interaction and JavaScript do not.

Licensing and alternatives:

- [PDFium](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/LICENSE):
  BSD-style and Apache-2.0 terms; distributions keep the applicable
  third-party notices. The [binding](https://github.com/ajrcarey/pdfium-render)
  is MIT OR Apache-2.0.
- MuPDF: rejected for its AGPL/commercial licensing (revisited by ADR-014).
- [PDF.js](https://github.com/mozilla/pdf.js/blob/master/LICENSE)
  (Apache-2.0): rejected, it brings a browser/webview runtime into a native
  GPUI application.

## Consequences

- With PDFium loaded: text selection and copy, whole-document search, links
  and outlines, converted Office documents included. Selection is per page,
  state lasts while the document is selected, and scans need an existing text
  layer.
- Deferred: password prompts, OCR, annotations and editing, cross-page
  selection, saved reading positions. Continuous scrolling came later in the
  shared UI, with no protocol change (`docs/PLAN.md`).
- Default tests cover worker lifetime, cancellation and restart, source
  invalidation, selection and fit geometry, without PDFium.
  `tools/pdf/check.py` checks real text and search geometry on normal,
  cropped and rotated pages, outlines, links, missing libraries and corrupt
  files, with generated documents.
- Speed and resource numbers live in `docs/pdf-comparison.md`; they are
  renderer numbers, not end-to-end UI measurements or a guarantee for any
  PDF.
