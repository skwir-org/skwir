# ADR-003: Preview providers and external document renderers

Status: accepted, 2026-09-18

## Context

The shell decoded images and text itself and launched video thumbnail tools,
with no cancellation of a running process. PDF previews were deferred; Phase 3
proposed PDFium. Office documents need a converter as well as a page renderer.
The same panel and peek overlay must display all these results, without bringing
GPUI into plugins or loading office/PDF engines at application startup.

## Decision

- Add a small core preview contract: requests carry an `EntryRef`, MIME type,
  page or video position, and pixel limit; results contain RGBA pixels and
  document/video navigation metadata. Media and document plugins share this
  contract. Providers execute on scheduler workers, not during UI rendering.
- Register `preview-media` and `preview-documents` independently. Disabling a
  provider cancels its active requests; registration changes invalidate the
  UI's current preview. Image/text rendering remains in the existing UI code.
- Use installed FFmpeg/ffprobe for video stills, and Poppler's `pdfinfo` and
  `pdftoppm` for PDF pages. This replaces the planned PDFium dependency for
  Linux. No renderer library is loaded into bibli at idle.
- Convert supported Office/OpenDocument files to PDF with installed
  LibreOffice. Reuse the PDF backend for pages, slides, and spreadsheet print
  layouts. Conversion does not provide an interactive spreadsheet grid.
  (ADR-030 amends this: a text document can also be read as text, by bibli
  or by Pandoc, in the same sandbox.)
- On Linux, Office conversion requires bubblewrap. Mount only the input,
  system runtime directories, and private scratch space; disable networking,
  macros, and link updates, and use an isolated LibreOffice profile. A sandbox
  failure is reported; conversion is never retried outside it. Other platforms
  need an isolation adapter before Office conversion is enabled there.
- Run tools with separate argv values, null stdin, bounded captured diagnostics,
  deadlines, and cancellation. On Unix, kill the child process group and reap
  the direct child. Bubblewrap also destroys its PID namespace on termination.
  Desktop thumbnailers use the same runner and private temporary directories
  (ADR-031 amends this: where bubblewrap can isolate, they run in the
  converters' sandbox, and ImageMagick, isolated, draws what none does).
- Serialize expensive work within each backend instance. Render only the
  requested page/frame, at most 2048 pixels on either side. Tool deadlines are
  15 seconds for rendering/metadata and 45 seconds for Office conversion;
  captured stdout/stderr are limited to 1 MiB each while running. Read output
  artifacts only below 64 MiB. These are safeguards, not latency targets or
  OS-level memory limits on child processes.
- Store compressed pages/frames and converted PDFs in the plugin's disposable
  cache namespace. Keys include file identity, path, size, modification/change
  times, executable metadata, requested page/time/size, and a format version.
  Trim each namespace to 128 MiB after storing a completed preview. Changed
  source files are rejected before publishing newly rendered output.
- Keep GPUI images and controls in bibli-ui. Invalidate requests on pane,
  entry, path, metadata, theme, and provider changes; reject stale generations,
  cancel on close/drop, and wake the UI only when results arrive. Reuse an
  existing desktop thumbnail while generating a larger preview.

## Consequences

Optional installed tools determine availability; missing tools produce an
explanation in the preview panel. There is no persistent converter process,
network service, or application database. New plugins add only descriptors and
small state at startup; conversion resource costs are transient and measured
separately from bibli's idle PSS.

Page navigation, zoom, and five-second video seeking are shell commands in the
palette and preview controls. Video playback, selectable PDF text, password
entry, and spreadsheet cell navigation remain future work. Office fidelity
still depends on the installed converter and available fonts.

Default tests exercise cancellation, cache bounds, and plugin removal. Explicit
ignored smoke tests in `crates/platform/src/preview.rs` exercise real tools with
synthetic PDFs, video, and generated DOC/DOCX/ODT, XLS/XLSX/ODS, PPT/PPTX/ODP
files. They are optional because the regular dev shell does not require these
external rendering programs.

## PDF latency follow-up

Poppler's PNG compression dominated cold-page latency on a synthetic scanned
document. Request uncompressed PPM pixels from `pdftoppm`, then encode the disk
cache with our fast, lossless PNG encoder. Decode only once and skip resampling
when the renderer's output already fits. Retain just the active document's
page count, keyed by source and tool metadata, across page turns.
Fast compression can produce larger cache files; the namespace remains bounded
to 128 MiB, and raw PPM intermediates live only in the request's scratch directory.

Use a desktop thumbnail only before the selected file has preview content.
Page turns and video seeking retain the current image until the requested one
is ready; a file-level cover must never replace an already displayed page.

Three release-mode samples at 2048 pixels on a synthetic noisy scan gave median
times of 1777 ms for Poppler PNG output plus decoding, 99 ms for the new backend
including metadata and cache storage, and 24 ms for a disk-cache hit. The test
asserts identical decoded pixels; these are backend timings, not end-to-end UI
latencies or guarantees for every PDF. Reproduce with:

```
cargo test --release -p bibli-platform real_scanned_pdf_fast_path_preserves_pixels -- --ignored --nocapture
```

Document preview debug logs include page, cache hit, render time, and total
backend time, without file paths. Enable `RUST_LOG=bibli_platform::preview=debug`
to investigate remaining document-specific delays.
