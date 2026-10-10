# ADR-033: The idle memory budget counts skwir's own share

Status: accepted, 2026-10-04. Amends ADR-002 (decision 1, and its
addendum's open gate) and ADR-000's idle memory row.

## Context

ADR-002 budgeted 100 MB of PSS, idle, with the default plugins, and its
addendum left the gate open at 118 to 120 MB: each contributor was to be
shrunk or budgeted in a new record with its reason. Phase 2's other gate,
a week of daily use, has passed. The build is budgeted as it stands rather
than holding the phase open on this number.

The same addendum found that the total does not measure skwir alone: the
same binary read 120 MB with several GPU clients alive and 162 MB on a
quiet desktop, its heap and code identical. The difference is the GPU
driver's code, whose pages PSS divides among every process that maps
them.

## Measurements (2026-10-04)

Reference machine (ADR-000), Hyprland on the NVIDIA eGPU, release build
of 8105f90 (35 plugins), a four-entry folder, XDG directories of its own
with the caches filled by a first start, 15 s idle, three starts, by
`tools/measure/idle-pss.py`:

| PSS, MiB                         | run 1 | run 2 | run 3 |
| -------------------------------- | ----: | ----: | ----: |
| anonymous (heap, stacks)         |  25.6 |  25.7 |  25.6 |
| skwir's binary                   |  37.3 |  37.5 |  37.3 |
| GPU buffers (textures, swapchain)|  19.6 |  19.6 |  19.6 |
| other files and libraries        |   6.6 |   6.6 |   6.6 |
| **own share**                    |  89.2 |  89.4 |  89.1 |
| GPU driver code                  |  44.6 |  44.6 |  44.6 |
| total                            | 133.7 | 134.0 | 133.7 |

- The GPU driver code is the NVIDIA user-space driver (38.2 MiB, with
  four other processes mapping it: Xwayland, the desktop portal, a
  terminal, the polkit agent) and Intel's Vulkan ICD with Mesa's LLVM
  (6.5 MiB, kept by ADR-002's vendor filter on a hybrid machine).
- `smaps_rollup` reads 33.8 MiB of `Pss_Anon` (the addendum's "PSS anon"
  column read 43 MB, its cache state not recorded): it also counts the
  pages of mapped files written since mapping, listed above under their
  files.
- The binary's share read 25.4 MB in the addendum. The growth was not
  attributed by a back-to-back build.
- A first start, with empty caches, read 163.5 MiB (three starts within
  0.6 MiB): about 15 MiB more driver (NVIDIA's shader compiler, filling
  its cache) and 14 MiB more anonymous memory. Later starts do not pay it.

## Decision

1. **The budget counts skwir's own share:** PSS without the pages of the
   GPU driver's code (the user-space driver libraries, the Vulkan ICDs
   and the LLVM they load). Their share depends on the GPU vendor and on
   how many other programs map them, which no change to skwir moves: a
   budget on them measured the reference machine's hardware and the
   desktop of the day. GPU buffers stay counted: they are skwir's.
2. **The budget is 100 MiB of own share,** idle, the default plugins, the
   caches filled. The build above reads 89.2 MiB (median). Counted as
   ADR-002 counted it, the same build reads 134 MiB: this loosens that
   gate by its driver share, and says so.
3. **`tools/measure/idle-pss.py` is the measurement** (`just idle-pss`):
   it starts the release build on the session's display, fills the caches
   once, measures three starts and exits non-zero over the budget. It
   reports the total and the driver's share beside the own share, so the
   whole cost stays visible.
4. The caches are filled before measuring, as ADR-000 measures with a
   warm page cache: a first start costs more once (above).

## Consequences

- ADR-002's open gate is closed. Its caveat stays: compare builds back to
  back under the same desktop state; the own share moves little with it,
  the total a lot.
- ADR-000's "AI plugins compiled in but disabled: equal to the above"
  compares own shares.
- ADR-002's first-frame budget is unchanged. The same day, nine warm
  starts read 116 to 169 ms (median 146 ms): within 150 ms by ADR-000's
  rule (the minimum over runs), but three of the nine were above it.
- Still measured by hand at phase gates, with the script, until the UI
  has a startup harness (Phase 8).
