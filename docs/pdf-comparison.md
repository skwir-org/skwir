# PDFium and Poppler comparison

Measured 2026-09-18 on the Linux reference workstation, release builds,
PDFium 7749 (`pdfium-render` 0.8.35) and Poppler 26.06.0. All inputs are generated
by `tools/pdf/fixtures.py`; no personal documents were used or stored.

## Renderer measurements

Five runs per three-page fixture, 2048-pixel maximum side, warm OS file cache.
The PDFium worker opens once and renders pages 1, 2, 3, 1, then searches for
`needle`. Poppler starts `pdftoppm` once per page and outputs uncompressed PPM,
as our image-preview backend does. Timings include process/protocol and pixel
file transfer; they exclude GPU upload, painting, the platform manager, and
Poppler's PNG disk-cache encoding/decoding. PDFium additionally extracts page
text/links; these engines are not providing identical features in this test.

Median times in milliseconds:

| Synthetic fixture | PDFium open | First page | New page | Revisit (rerender) | Poppler page |
|---|---:|---:|---:|---:|---:|
| Text, including a rotated page | 4.21 | 25.28 | 19.68 | 15.16 | 33.45 |
| 1500 vector strokes/page | 4.29 | 164.64 | 159.55 | 158.20 | 335.29 |
| Noisy grayscale scan | 4.29 | 52.21 | 46.70 | 37.03 | 63.00 |

Open is separate from PDFium's first-page time. New-page time is the median
of pages 2 and 3 in each run; the Poppler column includes four fresh renderer
invocations, without the bibli disk cache. Search took 0.21, 1.96, and 0.18 ms
respectively. The scanned fixture correctly has no text matches; no OCR runs.

| Synthetic fixture | PDFium peak PSS (MiB) | Poppler peak PSS (MiB) | PDFium CPU (ms) | Poppler CPU (ms) |
|---|---:|---:|---:|---:|
| Text | 29.29 | 21.13 | 50 | 119.70 |
| Vector | 29.83 | 21.41 | 610 | 1323.32 |
| Scan | 38.98 | 27.38 | 130 | 206.66 |

PSS is sampled from the child every 2 ms, so brief peaks may be missed. CPU is
child user+system time for four renders (plus PDFium open/search); PDFium's
`/proc` counter has clock-tick resolution. Parent-process CPU/memory and GPU
resources are excluded. Median PDFium idle PSS after these requests was about
29, 6, and 39 MiB; allocator retention depends on the document/workload. Closing
every measured session reaped its worker. No PDFium library is mapped in bibli.

## Full platform adapter

A separate release test includes the platform manager and Poppler's actual
compressed cache. On its two-page synthetic noisy scan, at 2048 pixels:

| Operation | PDFium | Poppler |
|---|---:|---:|
| Open document | 4.96 ms | Included in first page |
| First page | 42.23 ms | 112.70 ms |
| Next page | 45.61 ms | 84.05 ms |
| Revisit first page | 51.06 ms | 32.96 ms (disk cache) |

This is one sample, not a statistical latency guarantee. It excludes GPUI
conversion/upload/display. PDFium currently rerenders revisited pages while
retaining the parsed document; Poppler can win on cached pages, particularly
complex vector documents. Neither path replaces an already displayed page
with the document cover while waiting.

## Interpretation

PDFium reduced rendering latency and CPU for these fixtures, with higher child
peak memory and an idle process while the document remains open. It also
provides text selection, search, links and outlines. Prefer it when available
for those reader features; retain `preview-documents.backend = "poppler"` for
the image-only preview, existing disk-cache behavior, or installations without
PDFium. This comparison does not include MuPDF and makes no speed claim about it.

The UI's total memory is a separate gate. Previous preview work already exceeded
the historical 100 MB idle PSS target (see PLAN). Renderer-only numbers above
must not be interpreted as bibli's total footprint. Resource checks also exposed
retained preview textures in GPUI's atlas: replacement/close now explicitly
removes the old image, instead of relying on `Arc<RenderImage>` destruction.
The PDFium handoff also transfers one owned pixel buffer into GPUI without
full-page copies; a regression test checks that the allocation is preserved.

## Whole application navigation check

With a 1334×1214 Wayland window, the side panel displaying the synthetic text
PDF, and 50 alternating page turns (2048-pixel render limit):

| Engine | bibli PSS after 10 turns | After 50 turns | Idle child PSS after 50 turns |
|---|---:|---:|---:|
| PDFium | 172.44 MiB | 172.11 MiB | 29.18 MiB |
| Poppler | 177.79 MiB | 177.47 MiB | 0 MiB |

These are single-session observations, not a portable memory budget. Both
stabilized across navigation; PDFium's combined footprint was about 201 MiB
versus 177 MiB for Poppler. Closing the preview removed the native worker.
Idle CPU samples ranged from 0–3% of one core. Startup with no document open
measured roughly 115–117 MiB PSS in fresh isolated-config sessions, still above
the historical 100 MB UI target. GPU/allocator retention means closing a
preview does not necessarily return the parent to its initial footprint.

Before the buffer-transfer fix, the PDFium parent alone stabilized around
366 MiB in the same navigation exercise. Eliminating full-page copies reduced
that to about 172 MiB. The default tests enforce allocation-preserving handoff;
texture eviction is also explicit when replacing or closing preview images.

Manual checks additionally covered drag selection and clipboard copy, search
navigation from its input field, highlights on rotated pages, outlines, internal
links, and scrolling the 10k-image grid and 100k-entry list. External URI targets
were checked in native metadata; opening a real browser was not exercised.

## Reproduce

Enter `nix develop .#pdf` (and make Poppler available on `PATH`), then:

```sh
cargo build --release -p bibli -p bibli-pdf-worker
python3 tools/pdf/check.py --library "$BIBLI_PDFIUM_LIBRARY"
python3 tools/pdf/compare.py --library "$BIBLI_PDFIUM_LIBRARY" --runs 5
BIBLI_PDF_WORKER="$PWD/target/release/bibli-pdf-worker" cargo test --release -p bibli-platform real_pdfium_and_poppler_backend_comparison -- --ignored --nocapture
```

Run measurements with other builds/tests stopped. Keep reports aggregate and
synthetic; never commit screenshots, logs or filenames from private documents.
