# ADR-000: Principles and performance budgets

Status: accepted, 2026-09-18

## Context

skwir is an experimental file manager. The plan in `docs/PLAN.md` lists the
principles; this record makes them binding and attaches numbers so that
"fast" and "light" are checked by CI rather than argued about.

## Decision

### Principles

1. **Always responsive.** No blocking work on the UI thread. Listings stream,
   every provider call is async, batched, and cancellable. Viewport-visible work
   has priority over everything else.
2. **Light.** Idle memory and startup time are budgeted (below) and measured.
3. **Stateless and idempotent.** No daemon, no database. One content-addressed
   cache directory that can be deleted at any time. Re-applying an applied plan
   is a no-op.
4. **Harness the environment.** Reuse OS thumbnails, icon themes, the MIME
   database, bookmarks, existing indexes, and installed tools before building
   our own.
5. **Small core.** Only what two plugins must agree on lives in `skwir-core`.
   Everything else is a plugin with a runtime toggle.
6. **Agent-ready from the first commit.** Commands are serializable data.
   Mutations are plans reviewed before apply. Entries have stable identity.
   Metadata carries provenance. AI output is a suggestion until confirmed.

### Budgets

Reference machine: Intel i7 (Alder Lake) with Iris Xe iGPU and a GTX 1060 eGPU,
NVMe, btrfs with zstd, NixOS on Hyprland/Wayland. Warm page cache.

Filesystem primitives (enforced by `just bench-check`, values in
`crates/bench/budgets.toml` are 3x the reference measurement):

| Profile   | Metric                          | Reference | Budget  |
|-----------|---------------------------------|-----------|---------|
| flat100k  | first 1024 entries (getdents)   | 0.34 ms   | 1 ms    |
| flat100k  | full listing, names + d_type    | 27 ms     | 90 ms   |
| flat100k  | stat all entries, parallel      | 11 ms     | 40 ms   |
| deep1m    | recursive walk, parallel        | 63 ms     | 200 ms  |
| images10k | full listing                    | 3.1 ms    | 10 ms   |

UI (enforced by the toolkit spikes in Phase 0, then by the `ui` crate from Phase 2):

| Metric                                                  | Budget            |
|---------------------------------------------------------|-------------------|
| Cold start to first frame                               | < 100 ms          |
| 100k-entry directory: first rows painted after listing starts | < 16 ms     |
| Scrolling a 100k-entry list                              | at display refresh; main-thread CPU p95 per frame < 8.3 ms (120 Hz proxy) |
| Idle RSS, default plugins, no AI                        | < 100 MB          |
| Idle RSS with AI plugins compiled in but disabled       | equal to the above |

### Rules

- Budgets are tightened when the reference improves and never loosened without an ADR.
- The bench compares the minimum over runs, since the budget is about what the
  machine can do, not scheduler noise.
- Synthetic trees are generated on demand under `bench-data/` and never committed.

## Consequences

- Every phase ends with `just bench-check` green.
- Feature work that regresses a budget is not merged until the regression is fixed
  or the budget is revised through an ADR.
