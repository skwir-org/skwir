# PDF reader comparison: PDFium, Poppler, MuPDF

Measurements behind ADR-004, ADR-005 and ADR-014; rules in
`docs/invariants/reader.md`. All inputs are synthetic
(`tools/pdf/fixtures.py`).

## Method

- Each worker opens the document (with outlines), renders pages 1, 2, 3, 1
  with text and link metadata, then searches for `needle`.
- Three-page fixtures (text, vector, scan), five runs each, engine order
  alternated, 2048 px maximum side, warm file cache, no PNG disk cache.
- New page: median of pages 2 and 3. Revisit: page 1 again.
- Times include child startup, protocol, rendering, metadata and pixel
  transfer; not the platform manager, GPU upload or painting. **They are not
  UI latencies.**
- CPU: child user+system time for the whole run, at `/proc` clock-tick
  resolution. Peak PSS: sampled every 2 ms plus a final idle sample, so
  short peaks can be missed. Parent memory and GPU resources are excluded;
  shared libraries, allocator retention and host load affect both.

## 2026-09-18: PDFium and Poppler

Linux reference workstation, release builds, PDFium 7749 (`pdfium-render`
0.8.35), Poppler 26.06.0 (`poppler-rs` 0.25). This replaced an earlier
comparison against Poppler's image-only, per-page CLI renderer.

Median times in milliseconds:

| Fixture | Engine | Open | First page | New page | Revisit | Search |
|---|---|---:|---:|---:|---:|---:|
| Text | PDFium | 3.94 | 23.53 | 18.72 | 15.68 | 0.21 |
| Text | Poppler | 13.26 | 31.51 | 17.33 | 15.94 | 0.30 |
| Vector | PDFium | 4.06 | 175.09 | 173.14 | 173.71 | 1.81 |
| Vector | Poppler | 12.67 | 129.19 | 115.54 | 116.47 | 3.74 |
| Scan | PDFium | 6.19 | 73.01 | 59.68 | 62.06 | 0.27 |
| Scan | Poppler | 18.57 | 58.84 | 51.77 | 47.04 | 0.28 |

| Fixture | Engine | New-page run range (ms) | CPU (ms) | Peak worker PSS (MiB) | Idle worker PSS (MiB) |
|---|---|---:|---:|---:|---:|
| Text | PDFium | 17.89–19.58 | 50 | 29.18 | 29.18 |
| Text | Poppler | 17.22–19.18 | 60 | 22.75 | 22.75 |
| Vector | PDFium | 166.48–182.82 | 670 | 29.69 | 5.92 |
| Vector | Poppler | 111.68–132.21 | 450 | 22.69 | 22.69 |
| Scan | PDFium | 51.72–63.24 | 180 | 44.04 | 20.27 |
| Scan | Poppler | 48.43–64.03 | 150 | 33.66 | 33.66 |

- Searches found three matches in text and vector, none in scans (no text
  layer). Every worker exited on close.
- PDFium opened faster; new text pages were level. Poppler had lower peak
  PSS everywhere and was faster on vector and scan pages in this batch: not
  a general ranking.
- Two preliminary batches varied with host load (Poppler's vector new-page
  median 134.52–213.39 ms, PDFium's 166.78–168.78 ms), hence the alternating
  order and the reported range.
- Outcome: `auto` keeps preferring PDFium; both stay selectable.

## 2026-09-24: with MuPDF

Same workstation, method and fixtures; release builds, PDFium 7749,
Poppler 26.06.0, MuPDF 1.27.2 (`mupdf` crate 0.8, ADR-014); engine order
rotated between runs. The desktop was busy (load average 4–10 on 20 cores),
so times are higher than above: compare engines within this table only.

Median times in milliseconds:

| Fixture | Engine | Open | First page | New page | Revisit | Search |
|---|---|---:|---:|---:|---:|---:|
| Text | PDFium | 4.15 | 26.41 | 21.69 | 14.52 | 0.29 |
| Text | Poppler | 12.67 | 32.26 | 19.60 | 16.92 | 0.42 |
| Text | MuPDF | 2.35 | 27.26 | 18.97 | 17.11 | 0.27 |
| Vector | PDFium | 5.92 | 248.46 | 237.14 | 236.99 | 2.87 |
| Vector | Poppler | 15.58 | 167.23 | 153.24 | 155.27 | 4.21 |
| Vector | MuPDF | 3.28 | 1740.48 | 1704.61 | 1685.66 | 2.27 |
| Scan | PDFium | 8.02 | 96.82 | 84.60 | 71.08 | 0.39 |
| Scan | Poppler | 23.87 | 80.43 | 72.16 | 67.63 | 0.51 |
| Scan | MuPDF | 4.63 | 90.33 | 78.55 | 73.22 | 0.45 |

| Fixture | Engine | New-page run range (ms) | CPU (ms) | Peak worker PSS (MiB) | Idle worker PSS (MiB) |
|---|---|---:|---:|---:|---:|
| Text | PDFium | 18.97–21.90 | 50 | 29.44 | 29.44 |
| Text | Poppler | 19.11–19.79 | 60 | 23.74 | 23.74 |
| Text | MuPDF | 18.32–19.26 | 50 | 14.01 | 14.01 |
| Vector | PDFium | 162.27–249.96 | 930 | 29.94 | 6.21 |
| Vector | Poppler | 108.87–172.11 | 610 | 23.54 | 23.54 |
| Vector | MuPDF | 1255.87–1768.53 | 6900 | 13.86 | 13.86 |
| Scan | PDFium | 60.52–90.36 | 240 | 42.46 | 42.46 |
| Scan | Poppler | 61.11–83.88 | 220 | 34.54 | 34.54 |
| Scan | MuPDF | 75.38–80.57 | 220 | 23.14 | 23.14 |

- MuPDF opened fastest and used the least memory on every fixture; it was
  level on text and scanned pages.
- The vector fixture is 1,500 stroked lines, each across most of the page.
  MuPDF's anti-aliased rasterizer spends about 1.2 ms on each: `mutool draw
  -r 184` (MuPDF 1.27.2, nixpkgs) takes 1.39 s on that page with default
  anti-aliasing, 0.04 s without. The cost is MuPDF's, not the worker's.
  Anti-aliasing stays at MuPDF's default; watch documents with many large
  strokes (plans, maps, charts).
- Search results as above; every worker exited on close.

## Whole application

PDFium and Poppler only (date not recorded). Fresh isolated configurations,
an 1189×1334 Wayland window, synthetic text PDF in the side panel, 50
alternating page turns:

| Engine | Skwir PSS after 10 turns | After 50 turns | Worker after 50 turns | Combined after 50 turns |
|---|---:|---:|---:|---:|
| PDFium | 161.85 MiB | 161.89 MiB | 29.13 MiB | 191.02 MiB |
| Poppler | 161.56 MiB | 161.46 MiB | 23.47 MiB | 184.93 MiB |

- Memory stabilized for both; neither native library was mapped in Skwir.
- Closing the preview reaped the worker. Skwir kept about 161.5 MiB: GPU and
  allocator caches need not shrink back. Idle CPU over 5 s: 0.6–1.0% of one
  core.
- Single-session observations, not budgets. The 100 MB UI idle target is
  still open (`docs/PLAN.md`).

## Other checks

- Real Poppler UI: drag selection and copy, search and highlights on a
  rotated page, outlines, internal links; the 10k-image grid and 100k-entry
  list scrolled. HTTP(S) targets were checked in the metadata; opening a
  browser was not exercised.
- Installed-renderer tests: selectable, searchable previews of nine Office
  formats with PDFium and Poppler.
- Shared native checks: all four rotations with crop, named destinations,
  empty pages, missing outlines, dimensions, query limits, corrupt
  documents. `just poppler-check` and `just mupdf-check` run in CI.

## Reproduce

In `nix develop .#pdf`:

```sh
cargo build --release -p skwir -p skwir-pdf-worker -p skwir-poppler-worker -p skwir-mupdf-worker
python3 tools/pdf/check.py --library "$SKWIR_PDFIUM_LIBRARY"
python3 tools/pdf/check.py --engine poppler --worker target/release/skwir-poppler-worker
python3 tools/pdf/check.py --engine mupdf --worker target/release/skwir-mupdf-worker
python3 tools/pdf/compare.py --library "$SKWIR_PDFIUM_LIBRARY" --runs 5
SKWIR_PDF_WORKER="$PWD/target/release/skwir-pdf-worker" SKWIR_POPPLER_WORKER="$PWD/target/release/skwir-poppler-worker" cargo test --release -p skwir-platform real_pdfium_and_poppler_backend_comparison -- --ignored --nocapture
```

- Without PDFium, the default shell runs `just poppler-check` and
  `just mupdf-check`; `compare.py --engines poppler,mupdf` compares those
  two (`--library` is still required; only PDFium reads it).
- The installed-tool tests also need Poppler's CLI tools, LibreOffice,
  bubblewrap and FFmpeg on PATH.
- Measure with other builds, tests and test windows stopped. Keep reports
  aggregate and synthetic.

## Reading modes

The tables above predate continuous scrolling.

- Continuous scrolling is the default (`reading.continuous`, toggled by
  `ui.reader_continuous`). Pages scroll vertically and pan sideways when
  zoomed wider than the view; fit, zoom, per-page selection, search, links,
  contents and page entry all work.
- Single page: the wheel turns pages when the page fits; a larger page
  scrolls to its edge first. A backward turn opens the previous page at its
  bottom. Trackpad deltas accumulate before a turn; no turn starts while a
  page request is pending.
- Continuous mode draws only visible rows and requests nearby pages one at a
  time, behind placeholders, keeping the page and offset anchored.
- The UI targets 64 MiB of cached page pixels (`CACHE_BYTES`), evicting
  offscreen pages and their textures together. Visible and prefetched pages
  may exceed it, and the opening preview is kept apart: not a process memory
  cap. Closing or replacing the preview cancels requests and frees the
  cache.
