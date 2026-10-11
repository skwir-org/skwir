# ADR-002: UI budgets revisited after Phase 2

Status: accepted, 2026-09-18. Decision 1 amended by ADR-033 (2026-10-04):
the idle budget counts Skwir's own share, without the GPU driver's code.

## Context

Phase 2's GPUI shell missed two ADR-000 budgets: first frame 143 ms (budget
100 ms) and idle RSS 143 MB (budget 100 MB).

**Memory**, from `/proc/<pid>/smaps_rollup`, idle on a 5-entry directory:

| Measure       | Baseline | Vulkan drivers of present vendors only |
|---------------|---------:|---------------------------------------:|
| RSS           |   143 MB |                                 104 MB |
| PSS           |   100 MB |                                  75 MB |
| Private dirty |    25 MB |                                  21 MB |

Our heap (`RssAnon`) is about 25 MB; the rest is file-backed code. The Vulkan
loader loads every installed ICD when GPUI enumerates devices: here NVIDIA and
Intel (in use) plus radeon, nouveau, llvmpipe (with a 30 MB LLVM), and ARM SoC
and Windows drivers. Restricting it to the vendors in
`/sys/class/drm/card*/device/vendor` saves 39 MB of RSS. What remains is the
NVIDIA user-space driver (about 38 MB, shared with the compositor) and our
touched code pages.

**First frame.** About 100 ms pass inside GPUI before our callback; `strace`
shows 384 font files opened meanwhile. On Linux, gpui 0.2.2 builds
cosmic-text's `FontSystem` synchronously, parsing every system font
(`todo(linux) make font loading non-blocking`). Our window open and first
render take about 40 ms.

## Decision

1. **Memory is PSS, not RSS.** Pages shared with the compositor and other GPU
   clients are not Skwir's cost. Budget: 100 MB PSS, idle, default plugins.
2. **Vulkan drivers load only for present vendors.** Before any GPU work,
   `skwir` sets `VK_LOADER_DRIVERS_DISABLE` for ICDs whose hardware is absent
   (PCI vendor in sysfs). It never overrides a value the user set, and never
   disables a present vendor's driver or virtio in a VM.
3. **The first frame is split.** Total on the reference machine: 150 ms until
   the toolkit loads fonts lazily (tracked upstream), then 100 ms again. Our
   share, from the GPUI callback to the first painted frame: under 50 ms.

## Consequences

- Supersedes those two rows of ADR-000's table.
- No `skwir-bench` check: both are measured by hand at each phase gate until
  the UI has a startup harness (Phase 8).

## Amendments

### 2026-09-19: the idle memory gate

Reference machine (NVIDIA plus Intel, Wayland), release build, four-file
synthetic directory, idle for seven seconds:

| Build                                            | PSS    | PSS anon |
|--------------------------------------------------|--------|----------|
| dc3e4be (before the 2026-09-19 work)             | 118 MB | 43 MB    |
| f222bcf (after: readers, media, previews, usage) | 120 MB | 43 MB    |

The gate stays 100 MB PSS. That day's work added about 1.5 MB; the excess
predates it. Anonymous memory grew from about 25 MB to 43 MB, and the vendor
filter keeps the nouveau and Intel ICDs on a hybrid machine.

Owning the gate means profiling the 18 MB of anonymous growth (`heaptrack`;
since ADR-011 on a `--no-default-features` build, so heaptrack sees glibc's
malloc) and, per contributor, shrinking it or budgeting it in a new ADR with
the reason. Until then `docs/PLAN.md` reports the gate open, and no status
note restates the number without a back-to-back comparison with the previous
commit under identical conditions.

Closed by ADR-033 (2026-10-04), which budgets the build as it stands.

**Measurement caveat.** PSS splits a shared page among the processes mapping
it. About 60 MB of the idle footprint is the NVIDIA driver and its LLVM,
shared with the compositor and every GPU client. The same binary read:

| Desktop state             | PSS    | Heap share | Binary share |
|---------------------------|--------|------------|--------------|
| Several GPU clients alive | 120 MB | 25.6 MB    | 25.4 MB      |
| Quiet desktop             | 162 MB | 25.6 MB    | 25.4 MB      |

Compare builds back to back under the same desktop state, and report the heap
and binary mappings beside the total.

### 2026-10-04: the first frame

Reference machine, Hyprland on the NVIDIA eGPU, release build, warm caches:
116–169 ms over nine starts (median 146 ms), three above 150 ms. Breakdown,
from the shell's log and timers placed in gpui:

| Stage                                                    | Time     |
|----------------------------------------------------------|----------|
| Our `main` until gpui starts                             | 3 ms     |
| Fonts (fontdb reads 338 files, warm page cache)          | 7 ms     |
| GPU context (`BladeContext::new`), nearly all NVIDIA driver | 58–95 ms |
| Ours: window surface and swapchain                       | 13 ms    |
| Ours: shader                                             | 3–4 ms   |
| Ours: pipelines                                          | 7–14 ms  |

Fonts no longer cost 100 ms. Our surface and pipelines were built twice: the
renderer started transparent, then the window set itself opaque. With only
Intel's driver visible the whole first frame read 36 ms.

Two vendored gpui patches:

- 0013: the Wayland renderer starts with the window's transparency, so
  surface and pipelines are built once (about 10 ms).
- 0014: the GPU context is created on a thread while the fonts and the
  display connection are set up (about the fonts' 7 ms).

Ten warm starts of each build, alternated back to back on a quiet machine:

| First frame | Before   | After    |
|-------------|---------:|---------:|
| Median      | 134.6 ms | 114.5 ms |
| Maximum     | 144.9 ms | 131.0 ms |

Our share read 28–56 ms over thirteen starts: the 50 ms holds on most starts,
not all. The budgets are unchanged.

The GPU is the environment's choice: Blade takes the first device the Vulkan
loader lists, in the order Mesa's device-select layer gives (`DRI_PRIME`,
`MESA_VK_DEVICE_SELECT`). Picking the integrated GPU would start faster on
this machine and is not done (AGENTS.md, "No tuning for the machine it was
measured on").
