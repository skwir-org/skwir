# ADR-002: UI budgets revisited after Phase 2

Status: accepted, 2026-09-18

## Context

Phase 2 shipped the GPUI shell. Two UI budgets from ADR-000 were missed:
first frame 143 ms against 100 ms, and idle RSS 143 MB against 100 MB. This
record explains where the numbers come from and what the budgets mean now.

## Findings

### Memory

`/proc/<pid>/smaps_rollup` on the Phase 2 binary, idle on a 5-entry directory:

| Measure          | Baseline | Vulkan drivers restricted to present vendors |
|------------------|---------:|---------------------------------------------:|
| RSS              |  143 MB  |  104 MB |
| PSS              |  100 MB  |   75 MB |
| Private dirty    |   25 MB  |   21 MB |

Only about 25 MB is our own heap (`RssAnon`). The rest is file-backed code:
the Vulkan loader loads every installed ICD when GPUI enumerates devices,
which on this machine means the NVIDIA and Intel drivers that are in use
plus radeon, nouveau, llvmpipe (and its 30 MB LLVM), and drivers for ARM
SoCs and Windows that can never apply. Restricting the loader to the
vendors found in `/sys/class/drm/card*/device/vendor` removes 39 MB of RSS.
What remains is the NVIDIA user-space driver (about 38 MB, shared with the
compositor) and our binary's touched code pages.

### First frame

Of the 143 ms, about 100 ms is inside GPUI before our callback runs, and
`strace` shows 384 font files opened in that window. gpui 0.2.2's Linux text
system constructs cosmic-text's `FontSystem` synchronously, which parses
every system font; the source carries a `todo(linux) make font loading
non-blocking`. Our own window open and first render take about 40 ms.

## Decision

1. **Memory budget is measured as PSS, not RSS.** Pages shared with the
   compositor and other GPU clients are not a cost this app adds to the
   system. The budget stays at 100 MB PSS idle with default plugins.
2. **The binary restricts Vulkan driver loading to present vendors.** At
   startup, before any GPU work, `bibli` sets `VK_LOADER_DRIVERS_DISABLE` for
   ICDs whose hardware is absent (by PCI vendor in sysfs), never touching a
   value the user already set, and never disabling a driver for a vendor
   that is present or for virtio inside a VM.
3. **First frame budget is split.** Total wall time to first frame on the
   reference machine: 150 ms until the toolkit's font loading is lazy
   (tracked upstream); our own share (from the GPUI callback to the first
   painted frame) must stay under 50 ms. The 100 ms total target is
   restored when the toolkit fix lands.

## Consequences

- ADR-000's table is superseded for those two rows by this record.
- `bibli-bench` gains no new check for these: they are measured manually at
  each phase gate until the UI has an automated startup harness (Phase 8).

## Addendum, 2026-09-19: the idle memory gate

Measured on the reference machine (NVIDIA plus Intel, Wayland), release
build, a four-file synthetic directory, idle for seven seconds:

| Build                                   | PSS     | PSS anon |
|-----------------------------------------|---------|----------|
| d8420b3 (before the 2026-09-19 work)    | 118 MB  | 43 MB    |
| a557925 (after: readers, media, previews, usage) | 120 MB | 43 MB |

The gate is 100 MB PSS and stays at 100 MB: nothing here loosens it. The
2026-09-19 work added about 1.5 MB. The excess over the budget predates it:
anonymous memory grew from about 25 MB at the time of this ADR to 43 MB, and
the mapped-file share includes the nouveau and Intel ICDs, which the vendor
filter keeps on a hybrid machine. Owning the gate means a heap profile
(`heaptrack` is in the dev shell) of the 18 MB of anonymous growth and a
decision per contributor: shrink it, or budget it in a new ADR with the reason.
Until then the status line in `docs/PLAN.md` reports the gate as open, and
per-change status notes must not restate the number without a comparison
against the previous commit under identical conditions.
