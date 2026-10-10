# ADR-001: UI toolkit

Status: accepted, 2026-09-18

## Context

Phase 0 required choosing the GPU-accelerated Rust toolkit for the `ui` crate.
The candidates shortlisted in the plan were GPUI (Zed) and Floem (Lapce), with
iced, egui, and Xilem ruled out earlier for weaker list virtualization, look,
or maturity respectively.

Both candidates were implemented as the same demo (`spikes/`): a 100,000-row
virtualized list, 48 px rows, 40 px thumbnails decoded lazily from real PNGs
with viewport priority and staleness cancellation, a scripted 7 s scroll, and
identical metrics from `spike-common`. Each was run alone, twice, on the Intel
Iris Xe and on the NVIDIA GTX 1060, keeping the warm run.

Versions: gpui 0.2.2, floem 0.2.0, both from crates.io.

## Measurements

Reference machine as in ADR-000. Both displays are 2560x1440 at 59.95 Hz and
both toolkits present with vsync, so wall-clock frame intervals sit at 16.7 ms
by construction. The discriminating numbers are main-thread CPU time per
frame (layout + paint + submit, independent of vsync), the tail of the frame
interval distribution (dropped frames), first frame, and RSS.

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

Reading the table:

- Both toolkits fit the per-frame budget at the median. Floem's median is
  lower, but its p95 is close to the 8.3 ms budget for 120 Hz and its worst
  frames are 2 to 4 times longer than GPUI's. GPUI's p99 interval is within
  jitter of vsync; Floem's p99 is a dropped frame.
- First frame is within the 100 ms budget for both, GPUI on NVIDIA being at
  the edge.
- RSS includes roughly 37 MB of decoded thumbnails held by `spike-common`
  on purpose. GPUI's own footprint is well under the 100 MB budget; Floem on
  NVIDIA is not, largely from its wgpu path and driver.

## Qualitative findings

GPUI:

- `uniform_list` with a pixel-precise scroll offset did exactly what a file
  list needs. `ImageSource::Render` bypasses the asset cache and goes straight
  to the sprite atlas, which is the thumbnail fast path.
- The default system font stack does not resolve on this machine and the
  failed lookup repeats every frame, costing 35 to 50 ms per frame until an
  explicit font family is set. skwir must always set a verified font family.
- No Cargo features were needed for Wayland; Vulkan through blade honored the
  ICD restriction. The code was compact (150 lines). Every API had to be read
  from the crate source since documentation is thin.

Floem:

- No per-frame hook exists; the spike drives itself from a custom view's
  `paint`. `img()` decodes and SHA-256 hashes the whole image on every update,
  so thumbnails needed a custom view calling the renderer directly.
- The vger renderer routes images through the glyph atlas and clears the
  whole atlas, glyphs included, whenever usage passes 70 percent. A thumbnail
  grid would thrash it. This is the most significant finding against Floem
  for this application.
- Readable source, Leptos-like signals, pleasant to write (190 lines).

## Decision

**GPUI.** It has the tighter frame-time tail, the lower memory footprint, an
image atlas designed for many sprites, and it is the only candidate proven on
Linux, macOS, and Windows in a shipping application of this shape.

Mitigations adopted with it:

- Pin an exact `gpui` version and upgrade deliberately; expect API churn.
- Set an explicit font family at the root and verify it resolves at startup.
- Keep `skwir-ui` thin over `skwir-core` so the toolkit remains replaceable.

## What the spike could not verify

- The 120 Hz budget: no display above 60 Hz is available. Main-thread CPU
  p95 is the proxy and GPUI passes it with a 2x margin.
- Rendering on the Intel iGPU: in this Hyprland session the compositor's DRM
  device is the NVIDIA card, and Vulkan surfaces created on the Intel ICD are
  never displayed (`vkcube` shows the same). Intel numbers measure work
  submitted to an undisplayed swapchain, which is still representative of
  CPU cost.
- External drag-and-drop and IME on Wayland need a human at the keyboard and
  remain open items for Phase 3.

## Consequences

- Phase 2 builds `skwir-ui` on gpui 0.2.2.
- **Known gap: accessibility.** gpui 0.2.2 has no AccessKit integration
  (verified: no `accesskit` dependency in the crate). The plan's "AccessKit
  from day one" requirement cannot be met on this toolkit version. Tracked
  as an open item to revisit at each gpui upgrade; if it is still absent
  by Phase 5, evaluate contributing it upstream or a dedicated
  AccessKit adapter in `skwir-ui`.
- The spike workspace stays in the repo as a reproducible benchmark; both
  spikes remain runnable through `just spike <toolkit> <gpu>`.
