# Toolkit spikes (Phase 0)

The same demo built with each candidate toolkit, measured identically
through `spike-common`. The result decided ADR-001.

## The demo

- A 1200x800 window with a virtualized list of `--rows` entries (default
  100,000).
- Rows of 48 px: a 40 px thumbnail, file name, size, modified date.
- Thumbnails are real PNGs from `bench-data/images10k`, decoded lazily on
  worker threads, most recently requested first, dropped when scrolled
  away.
- After the first frame a scripted scroll runs: 5 s continuous at
  3000 px/s, then 2 s of random jumps. Every frame calls `Metrics::frame`.
- At the end the process prints a JSON report and exits.

## Contract for a spike binary

1. `let metrics = Metrics::start()` is the first statement in `main`.
2. Parse `spike_common::parse_args()`.
3. Build `Thumbs::new(&args.images, 4)` and call `set_on_ready` with a
   redraw request, so decoded thumbnails appear without input.
4. Once per frame: `thumbs.tick()`, `let t = metrics.frame()`, set the
   scroll offset to `driver.offset_at(t)`; when `driver.done(t)`, build the
   report and call `spike_common::finish`.
5. Rows come from `spike_common::row(i)`, thumbnails from `thumbs.get(i)`.

## Running

    just spike gpui intel
    just spike gpui nvidia
    just spike floem intel
    just spike floem nvidia

Reports land in `spikes/results/<toolkit>-<gpu>.json`.

## Results (2026-09-18)

The table and the decision are in `docs/adr/001-ui-toolkit.md`; raw reports
are committed in `results/`. Both displays are 60 Hz, so `frame_p50_us` is
the vsync period: compare `frame_cpu_*` and the tails.
