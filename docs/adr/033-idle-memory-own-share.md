# ADR-033: The idle memory budget counts Skwir's own share

Status: accepted, 2026-10-04. Built (`just idle-pss`). Amends ADR-002
(decision 1, and its addendum's open gate) and ADR-000's idle memory
row.

## Context

ADR-002 budgeted 100 MB of PSS, idle, default plugins; its addendum
left the gate open at 118–120 MB. Phase 2's other gate, a week of daily
use, has passed, so the build is budgeted as it stands rather than
holding the phase open.

The addendum also found the total does not measure Skwir alone: the
same binary read 120 MB with several GPU clients alive and 162 MB on a
quiet desktop, heap and code identical. The difference is the GPU
driver's code, whose pages PSS divides among every process mapping
them.

## Measurements (2026-10-04)

Reference machine (ADR-000), Hyprland on the NVIDIA eGPU, release build
of d2cc3ed (35 plugins), a four-entry folder, own XDG directories with
caches filled by a first start, 15 s idle, three starts,
`tools/measure/idle-pss.py`:

| PSS, MiB                         | run 1 | run 2 | run 3 |
| -------------------------------- | ----: | ----: | ----: |
| anonymous (heap, stacks)         |  25.6 |  25.7 |  25.6 |
| Skwir's binary                   |  37.3 |  37.5 |  37.3 |
| GPU buffers (textures, swapchain)|  19.6 |  19.6 |  19.6 |
| other files and libraries        |   6.6 |   6.6 |   6.6 |
| **own share**                    |  89.2 |  89.4 |  89.1 |
| GPU driver code                  |  44.6 |  44.6 |  44.6 |
| total                            | 133.7 | 134.0 | 133.7 |

- Driver code: NVIDIA's user-space driver (38.2 MiB, also mapped by
  Xwayland, the desktop portal, a terminal and the polkit agent) and
  Intel's Vulkan ICD with Mesa's LLVM (6.5 MiB, kept by ADR-002's
  vendor filter on a hybrid machine).
- `smaps_rollup`'s `Pss_Anon` reads 33.8 MiB (the addendum's 43 MB,
  cache state unrecorded): it also counts written pages of mapped
  files, listed above under their files.
- The binary's share was 25.4 MB in the addendum; the growth was not
  attributed by a back-to-back build.
- A first start with empty caches read 163.5 MiB (three starts within
  0.6 MiB): ~15 MiB more driver (NVIDIA's shader compiler filling its
  cache) and 14 MiB more anonymous memory. Later starts do not pay it.

## Decision

1. **The budget counts Skwir's own share:** PSS minus the GPU driver's
   code (user-space driver libraries, Vulkan ICDs, their LLVM). That
   share depends on the GPU vendor and on other programs mapping it,
   which no change to Skwir moves. GPU buffers stay counted: they are
   Skwir's.
2. **The budget is 100 MiB of own share**, idle, default plugins,
   caches filled. The build above reads 89.2 MiB (median). Counted as
   ADR-002 did, it reads 134 MiB: this loosens that gate by the
   driver's share, openly.
3. **`tools/measure/idle-pss.py` is the measurement** (`just
   idle-pss`): release build on the session's display, caches filled
   once, three starts measured, non-zero exit over budget. It reports
   the total and driver share beside the own share.
4. Caches are filled first, as ADR-000 measures with a warm page cache.

## Consequences

- ADR-002's open gate is closed. Its caveat stays: compare builds back
  to back under the same desktop state (the own share moves little, the
  total a lot).
- ADR-000's "AI plugins compiled in but disabled: equal to the above"
  compares own shares.
- ADR-002's first-frame budget is unchanged. Same day, nine warm starts:
  116–169 ms (median 146 ms), within 150 ms by ADR-000's rule (minimum
  over runs), but three of nine above it.
- Measured by hand at phase gates with the script until the UI has a
  startup harness (Phase 8).
