# ADR-038: What the budgets measure on macOS and Windows

Status: accepted, 2026-10-05 (Phase 5, step L; ADR-035). The
measurements are 5b's, on the machines.

## Context

ADR-000 budgets idle memory and the first frame; ADR-033 made the
memory budget skwir's own share: Linux's PSS without the pages of the
GPU driver's code, which no change to skwir moves. `tools/measure/
idle-pss.py` reads `/proc/<pid>/smaps` and the shell's log line for the
first frame. Neither exists on macOS or Windows, whose memory is
counted otherwise:

- **macOS** has no proportional count. The kernel's own figure for "the
  memory this process costs" is `phys_footprint` (`proc_pid_rusage`,
  `RUSAGE_INFO_V4`: what Activity Monitor shows as Memory): its dirty
  private pages, compressed ones included, and IOKit allocations it
  owns (GPU buffers through Metal). It leaves out clean file-backed
  pages (the binary's code, shared libraries), which the system can
  drop and read again.
- **Windows** counts a working set (pages resident), split into shared
  and private, and a private commit. The private working set
  (`PROCESS_MEMORY_COUNTERS_EX2::PrivateWorkingSetSize`, what Task
  Manager shows as Memory) is the resident pages no other process
  shares: heap, stacks, written pages of mappings. The driver's code is
  shared and so out of it; its private allocations for skwir are in.

None of these is Linux's own share. Each is what the system itself
calls a program's cost, and each leaves the driver's shared code out
as ADR-033 does.

## Decision

1. **Idle memory is read on each system as the system reads it:**
   own share of PSS on Linux (ADR-033), `phys_footprint` on macOS, the
   private working set on Windows; same conditions (idle, the default
   plugins, a four-entry folder, the caches filled, three starts, the
   median).
2. **The number stays 100 MiB on each,** until 5b measures: a budget
   is not set from a guess. If a system's measure of an unchanged build
   is above it for reasons of the measure (macOS counting GPU buffers
   differently, say), 5b records that in an amendment with the numbers,
   as ADR-033 did, and says what the budget is there.
3. **The first frame is the same event everywhere:** from the process
   start to the shell's log line of its first drawn frame, which skwir
   writes on every system; the minimum over runs, as ADR-000 has it.
4. **The measuring script grows a reader per system** (in 5b, where it
   runs): `idle-pss.py` keeps Linux's, and reads `phys_footprint`
   through `proc_pid_rusage` on macOS and the private working set
   through `GetProcessMemoryInfo` on Windows. Until then it says, off
   Linux, that it measures nothing there.
5. **`just bench-check`'s microsecond budgets are Linux's.** They time
   listing and stat calls of the reference machine's filesystem; on the
   other systems they are reported, not enforced, until 5b sets
   budgets of their own from measurements there.

## Consequences

- The three numbers are not comparable across systems; each is compared
  only with itself between builds, back to back (ADR-002's caveat).
- 5b's gate ("the budgets pass on all three") means these measures, on
  the machines, after their amendments if any.
