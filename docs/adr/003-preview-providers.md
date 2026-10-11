# ADR-003: Preview providers and external document renderers

Status: accepted, 2026-09-18. PDF engine superseded by ADR-004 and ADR-005
(readers); the Poppler command-line path below remains their fallback.
Amended by ADR-030 (text documents read as text) and ADR-031 (thumbnailers
isolated).

## Context

The shell decoded images and text itself and launched video thumbnail tools
it could not cancel. PDF previews were deferred (Phase 3 proposed PDFium);
Office documents need a converter as well as a page renderer. One panel and
the peek overlay must show all of it, without GPUI in plugins or office/PDF
engines loaded at startup.

## Decision

- **Contract.** A small core preview contract: a request carries an
  `EntryRef`, MIME type, page or video position and pixel limit; a result
  holds RGBA pixels plus document/video navigation metadata. Providers run on
  scheduler workers, never during rendering.
- **Plugins.** `preview-media` and `preview-documents` register
  independently. Disabling one cancels its requests; a registration change
  invalidates the current preview. Image and text rendering stay in the UI.
- **Tools, not libraries.** Installed FFmpeg/ffprobe for video stills,
  Poppler's `pdfinfo` and `pdftoppm` for PDF pages (instead of PDFium on
  Linux). No renderer library is loaded into skwir at idle.
- **Office.** Installed LibreOffice converts Office/OpenDocument files to
  PDF, shown by the PDF backend (pages, slides, spreadsheet print layouts; no
  interactive grid). ADR-030: a text document can also be read as text, by
  skwir or Pandoc, in the same sandbox.
- **Sandbox.** On Linux, Office conversion requires bubblewrap: only the
  input, system runtime directories and private scratch are mounted; no
  network, macros or link updates; an isolated LibreOffice profile. A sandbox
  failure is reported, never retried outside it. Other platforms need an
  isolation adapter before conversion is enabled.
- **Running tools.** Separate argv values, null stdin, bounded diagnostics,
  deadlines, cancellation. On Unix, kill the child's process group and reap
  the direct child (bubblewrap also destroys its PID namespace). Desktop
  thumbnailers use the same runner with private temporary directories
  (ADR-031: in the converters' sandbox where bubblewrap can isolate, and
  ImageMagick, isolated, draws what no thumbnailer does).
- **Bounds.** Work is serialized per backend instance; only the requested
  page or frame is rendered, at most 2048 px per side. Deadlines: 15 s for
  rendering and metadata, 45 s for Office conversion. Captured stdout and
  stderr: 1 MiB each. Output artifacts are read only under 64 MiB. These are
  safeguards, not latency targets or OS memory limits.
- **Cache.** Compressed pages/frames and converted PDFs live in the plugin's
  disposable cache namespace, keyed by file identity, path, size,
  modification/change times, executable metadata, requested page/time/size
  and a format version; each namespace is trimmed to 128 MiB after a store.
  A source that changed is rejected before new output is published.
- **UI.** GPUI images and controls stay in `skwir-ui`. Pane, entry, path,
  metadata, theme and provider changes invalidate requests; stale generations
  are rejected; close and drop cancel; the UI wakes only when results arrive.
  An existing desktop thumbnail shows while a larger preview is made.

## Consequences

- Installed tools decide availability; the panel explains a missing one.
- No persistent converter, network service or database. Plugins add only
  descriptors and small state at startup; conversion costs are transient and
  measured apart from idle PSS.
- Page navigation, zoom and five-second video seeking are shell commands.
- Left for later: video playback (built, ADR-008), selectable PDF text
  (ADR-004), password entry, spreadsheet cell navigation. Office fidelity
  depends on the installed converter and fonts.
- Default tests cover cancellation, cache bounds and plugin removal. Ignored
  smoke tests in `crates/platform/src/preview.rs` run the real tools on
  synthetic PDF, video and generated DOC/DOCX/ODT, XLS/XLSX/ODS, PPT/PPTX/ODP
  files; opt-in, since the dev shell does not require these tools.

## Amendments

### 2026-09-18: PDF latency

Poppler's PNG compression dominated cold-page latency on a scanned document.

- `pdftoppm` writes uncompressed PPM; skwir encodes the cache with its fast
  lossless PNG encoder. Files may be larger; the namespace stays at 128 MiB,
  and PPM intermediates stay in the request's scratch directory.
- Decode once; skip resampling when the output already fits. Keep only the
  active document's page count (keyed by source and tool metadata) across
  page turns.
- A desktop thumbnail is used only before the file has preview content. Page
  turns and seeks keep the current image until the next is ready; a file's
  cover never replaces a page already shown.

Release build, 2048 px, synthetic noisy scan, median of three (identical
decoded pixels asserted; backend timings, not UI latency or a guarantee for
every PDF):

| Path                                        | Time    |
|---------------------------------------------|---------|
| Poppler PNG output + decoding (before)      | 1777 ms |
| New backend, with metadata and cache store  | 99 ms   |
| Disk-cache hit                              | 24 ms   |

```
cargo test --release -p skwir-platform real_scanned_pdf_fast_path_preserves_pixels -- --ignored --nocapture
```

`RUST_LOG=skwir_platform::preview=debug` logs page, cache hit, render time
and total backend time, without file paths.
