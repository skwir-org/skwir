# ADR-038: What the budgets measure on macOS and Windows

Status: accepted, 2026-10-05 (Phase 5, step L; ADR-035). Measurements
are 5b's, on the machines.

## Context

ADR-000 budgets idle memory and the first frame; ADR-033 counts skwir's
own share of PSS, without the GPU driver's code. `tools/measure/
idle-pss.py` reads `/proc/<pid>/smaps`, which macOS and Windows lack.
Each has its own cost figure, which also leaves the driver's shared code
out:

| System | Figure (what the system monitor shows) | Counts |
|---|---|---|
| macOS | `phys_footprint` (`proc_pid_rusage`, `RUSAGE_INFO_V4`) | dirty private and compressed pages, owned IOKit allocations (Metal buffers); not clean file-backed pages |
| Windows | private working set (`PrivateWorkingSetSize`) | resident pages no other process shares, incl. the driver's private allocations |

## Decision

1. **Idle memory is read as each system reads it**: own PSS on Linux,
   `phys_footprint` on macOS, private working set on Windows. Same
   conditions: idle, default plugins, a four-entry folder, caches
   filled, median of three starts.
2. **The budget stays 100 MiB until 5b measures.** If a system's figure
   for an unchanged build exceeds it because of how it counts (e.g.
   GPU buffers on macOS), 5b records the numbers and that system's
   budget in an amendment, as ADR-033 did.
3. **First frame: same event everywhere**, process start to the shell's
   first-drawn-frame log line; minimum over runs (ADR-000).
4. **`idle-pss.py` gains a reader per system in 5b**
   (`proc_pid_rusage`, `GetProcessMemoryInfo`). Until then it says, off
   Linux, that it measures nothing.
5. **`just bench-check`'s microsecond budgets are Linux's.** Elsewhere
   they are reported, not enforced, until 5b measures there.

## Consequences

- Figures are not comparable across systems; each is compared with
  itself between builds, back to back (ADR-002).
- 5b's gate ("the budgets pass on all three") means these measures, on
  the machines, after any amendments.
