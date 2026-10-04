# ADR-002: UI budgets revisited after Phase 2

Status: accepted, 2026-09-18. Decision 1 amended by ADR-033 (2026-10-04):
the idle budget counts bibli's own share, without the GPU driver's code.

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
(`heaptrack` is in the dev shell; since ADR-011 profile a build with
`--no-default-features`, which keeps glibc's malloc for heaptrack to see) of
the 18 MB of anonymous growth and a
decision per contributor: shrink it, or budget it in a new ADR with the reason.
Until then the status line in `docs/PLAN.md` reports the gate as open, and
per-change status notes must not restate the number without a comparison
against the previous commit under identical conditions.

Closed by ADR-033 (2026-10-04), which budgets the build as it stands.

Measurement caveat: PSS divides each shared page by the number of processes
mapping it. About 60 MB of our idle footprint is the NVIDIA user-space driver
and its LLVM, shared with the compositor and every other GPU client, so the
same binary read 120 MB with several GPU clients alive and 162 MB on a quiet
desktop, with identical heap (25.6 MB) and binary (25.4 MB) shares. Compare
builds back to back under the same desktop state, and report the heap and
binary mappings next to the total.

## Addendum, 2026-10-04: the first frame

On the reference machine (Hyprland on the NVIDIA eGPU, release build, warm
caches), the first frame read 116 to 169 ms (median 146 ms, nine starts),
three of the nine above 150. Where it went, from the shell's log and
timers placed in gpui for the measurement:

- our `main` until gpui starts: 3 ms;
- fonts: 7 ms. No longer the 100 ms found above: with a warm page cache
  fontdb reads the 338 font files in that time;
- the GPU context (`BladeContext::new`): 58 to 95 ms, nearly all of it the
  NVIDIA driver. With only Intel's driver visible the whole first frame
  read 36 ms;
- our share: the window's surface and swapchain 13 ms, the shader 3 to
  4 ms, the pipelines 7 to 14 ms, and they were built twice: the renderer
  started transparent and the window then set itself opaque.

Two changes to gpui. The Wayland renderer starts with the transparency the
window will have, so its surface and pipelines are built once (vendored
patch 0013, about 10 ms), and the GPU context is created on a thread while
the fonts and the display connection are set up (0014, about the fonts'
7 ms here). Ten warm starts of each build, alternated back to back on a
quiet machine: median 134.6 to 114.5 ms, maximum 144.9 to 131.0 ms. Our
share read 28 to 56 ms over thirteen starts: the 50 ms holds on most, not
all. The budgets are unchanged.

Which GPU draws is the environment's, not bibli's: Blade takes the first
device the Vulkan loader lists, in the order Mesa's device-select layer
gives (`DRI_PRIME`, `MESA_VK_DEVICE_SELECT`). Choosing the integrated GPU
would start faster on this machine and is not done for it (AGENTS.md, "No
tuning for the machine it was measured on").
