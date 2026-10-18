# ADR-001: UI toolkit

Status: accepted, 2026-09-18. Built on gpui 0.2.2, vendored with patches,
until ADR-064 (2026-10-17) moved it to a commit of Zed's main branch
(`vendor/zed`). Amended 2026-10-11 (accessibility waits for gpui) and
2026-10-17 (gpui has AccessKit).

## Context

Phase 0 had to pick the GPU-accelerated Rust toolkit for the `ui` crate.
Shortlist: GPUI (Zed) and Floem (Lapce). Ruled out earlier: iced (list
virtualization), egui (look), Xilem (maturity).

Both were built as the same spike (`spikes/`): a 100,000-row virtualized
list, 48 px rows, 40 px thumbnails decoded lazily from real PNGs with
viewport priority and staleness cancellation, a scripted 7 s scroll, shared
metrics (`spike-common`). gpui 0.2.2 and floem 0.2.0, each run alone, twice,
on the Iris Xe and the GTX 1060 of ADR-000's machine; the warm run is kept.
Displays are 2560x1440 at 59.95 Hz with vsync, so intervals sit at 16.7 ms:
main-thread CPU per frame and the interval tail are what discriminate.

| Metric                        | GPUI nvidia | GPUI intel | Floem nvidia | Floem intel |
|-------------------------------|------------:|-----------:|-------------:|------------:|
| First frame (ms)              |        95.5 |       67.8 |         89.6 |        57.1 |
| Frame CPU p50 (us)            |        3521 |       3085 |         2305 |        2592 |
| Frame CPU p95 (us)            |        4151 |       3855 |         7750 |        6081 |
| Frame CPU max (us)            |       12569 |      23251 |        47644 |       25265 |
| Frame interval p99 (us)       |       18990 |      18785 |        30761 |       30125 |
| Frame interval max (us)       |       20620 |      32589 |        51069 |       33209 |
| Process CPU over 7 s (ms)     |        1845 |       1581 |         1613 |        1497 |
| RSS (MB)                      |       135.3 |      109.4 |        279.7 |       137.5 |
| Thumbnails decoded            |         564 |        564 |          564 |         564 |

Raw reports: `spikes/results/*.json`.

- Floem's median frame CPU is lower, but its p95 nears the 8.3 ms (120 Hz)
  budget, its worst frames are 2–4x GPUI's, and its p99 interval is a dropped
  frame. GPUI's p99 is within vsync jitter.
- First frame: both within 100 ms, GPUI on NVIDIA at the edge.
- RSS includes ~37 MB of thumbnails held on purpose. GPUI stays well under
  100 MB; Floem on NVIDIA does not (its wgpu path and driver).

Findings:

- **GPUI.** `uniform_list` with a pixel-precise scroll offset fits a file
  list; `ImageSource::Render` goes straight to the sprite atlas. The default
  font stack does not resolve here, and the failed lookup repeats every frame
  (35–50 ms) until a font family is set. Wayland and Vulkan (blade, honoring
  the ICD restriction) needed no setup. 150 lines; thin docs, every API read
  from source.
- **Floem.** No per-frame hook. `img()` decodes and SHA-256-hashes the whole
  image on every update. The vger renderer keeps images in the glyph atlas and
  clears all of it, glyphs included, past 70 % use: a thumbnail grid would
  thrash it, the strongest finding against Floem. Readable, Leptos-like
  signals, 190 lines.

## Decision

**GPUI**: tighter frame-time tail, lower memory, an atlas built for many
sprites, and the only candidate proven on Linux, macOS and Windows in a
shipping application of this shape.

Mitigations:

- Pin an exact `gpui` version; upgrade deliberately.
- Set an explicit font family at the root and verify it resolves at startup.
- Keep `skwir-ui` thin over `skwir-core` so the toolkit stays replaceable.

Not verified by the spike:

- 120 Hz: no such display. Main-thread CPU p95 is the proxy; GPUI passes with
  a 2x margin.
- Drawing on the Intel iGPU: the compositor runs on the NVIDIA card and Intel
  Vulkan surfaces are never displayed (`vkcube` too). Intel numbers measure
  CPU cost into an undisplayed swapchain.
- External drag-and-drop and IME on Wayland: they need a human; left for
  Phase 3.

## Consequences

- Phase 2 builds `skwir-ui` on gpui 0.2.2.
- **Known gap: accessibility.** gpui 0.2.2 has no AccessKit (no `accesskit`
  dependency), so the plan's "AccessKit from day one" cannot be met.
  Accessibility follows gpui's support: each gpui update checks for it
  (`vendor/gpui/README.skwir.md`, "Updating gpui"), and Skwir wires it in
  once gpui provides it.
- The spikes stay runnable as a benchmark: `just spike <toolkit> <gpu>`.

## Amendments

### 2026-10-11: accessibility waits for gpui

The revisit no longer has a date of Skwir's own ("if still absent by
Phase 5, evaluate contributing it upstream or an adapter in `skwir-ui`"):
it depends on gpui supporting accessibility, checked at each gpui update.

### 2026-10-17: gpui has AccessKit

ADR-064 vendors gpui from Zed's main branch, which integrates AccessKit:
the window starts with it. The gap now is Skwir's own: its elements
declare no roles or labels yet. The pin of 0.2.2 is revised there too.

### 2026-10-18: the window and the files are exposed

ADR-065: the window is named by its location and the file views are list
boxes whose cursor a screen reader follows; the other places follow
(`docs/invariants/accessibility.md`).
