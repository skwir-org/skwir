# ADR-128: Code is laid out in the order a start runs it

Status: accepted 2026-11-09 with the user. Built for Linux on x86-64
(`dist` builds, the flake's Nix package included); not for macOS or
Windows.
Answers PLAN's "find why so much of the code is resident".

## Context

0.3.0 missed ADR-000's idle budget by 3.7 MiB, and idle memory had
followed code size for releases: about 86 % of `.text` read as resident
at idle on the session, a program that runs nothing then.

Measured with Intel Processor Trace on 0.3.1 (a start to idle, every
branch), against which pages were resident (`/proc/<pid>/pagemap`):

| 0.3.1's code | MiB |
| --- | --- |
| `.text` | 68.3 |
| functions a start ran | 12.8 |
| 64-byte blocks it ran | 5.5 |
| resident at idle | 48.9 |
| resident and never run | 29.0 |

Idle ran ten blocks. Linux maps the 64 KiB around a fault in code already
in the page cache, and the linker leaves what a start runs scattered: a
start touches 73 % of all 64 KiB windows, so those are mapped whole and
stay so. Each MiB of code added cost 0.73 MiB idle, used or not. What
0.3.0 added was spread over many crates (text +26 %, the functions run
+22 %, the windows touched +23 %); no feature was the cause.

Dropping the pages after a start (an `MADV_DONTNEED` shim) read 30 MiB
own share against 86: it proves the pages are not needed, but they come
back with use and the layout stays as it was.

## Decision

1. **`dist` builds put what a start runs first, together.** lld's
   `--symbol-ordering-file` is given the functions a start runs, in the
   order it first runs them.
2. **The list names functions demangled** (`tools/build/startup-order.txt`,
   checked in): a symbol's hashes depend on how its crate was built, its
   version and features and the compiler's, so an exact list is good for
   one build only. `tools/build/link-ordered`, the linker for a `dist`
   build (`order-link-env.sh`, sourced by every build that ships), links
   skwir once, finds the list's names in its symbol table, and links it
   again in that order. Anything else it is given is the real linker's.
   The flake's Nix package names it by `--config`, in `flake.nix`:
   nixpkgs' build sets cargo's linker variable itself, and an unused
   wrapper would say nothing. `package.nix` knows nothing of it, written
   to stand in nixpkgs (ADR-124).
3. **A binary out of order is heavier, never broken.** No list, no lld
   (the wrapper asks whether rustc chose it), no symbols, a second link
   that fails: the first link stands, and the build says why. Silent
   when it worked, as rustc shows what a linker prints as a warning.
4. **Made by tracing real starts** (`just startup-order`,
   `tools/build/startup_order.py`): Intel PT, as sampling misses what runs
   once, on a Wayland compositor and an X server of its own, with software
   Vulkan and a session bus and folders of its own. What every display
   runs comes first, then what each runs alone, so a start keeps one run
   of code. Only the list's maker needs Intel PT.
5. **Made again before each release**, after the last fix
   (`docs/RELEASING.md`), and a new list links skwir again
   (`crates/skwir/build.rs`). A list 74 commits old recovered about half
   of the gain, and a function not on it costs a whole window: it is
   worth making fresh, and harmless when it is not.
6. **`just idle-pss` reads what ships**, the `dist` binary, no longer the
   development build.

## Consequences

Measured on a headless sway with Intel's driver (the two builds
alternated, three rounds, the first start of each filling the caches),
the same commit linked both ways:

| PSS, MiB | as it comes | ordered |
| --- | --- | --- |
| the binary | 61.5 | 31.9 |
| its code | 51.9 | 22.4 |
| windows of code resident | 834 | 360 |
| first frame, ms | 58.4 | 55.9 |

On the session's display (NVIDIA), `just idle-pss` read 83.2 MiB of own
share, within ADR-000's 100, where 0.3.1's tarball read 103.8: the binary
35.8 MiB against 58.6.

With the binary evicted from the page cache a first experiment started
in 82 ms against 94. What idle pays for new code is now what a start
runs of it.

- The list is 1.7 MiB of text, regenerated each release.
- What is left of the code (4.8 MiB above the list) is mostly what only a
  real session runs (a portal on the bus) and a driver's own paths, which
  a recipe that touches no desktop cannot see; 676 small functions there
  cost a window each.
- The list covers a start. What browsing and previews run stays
  scattered; how much returns with use is not measured.
- **The flake's Nix package** (its pinned Rust brings lld), two builds of
  `minimal` on the same headless sway: the binary 61.4 and 32.6 MiB. A
  copy in nixpkgs, whose Rust has no lld, is not laid out.
- **Not built: macOS and Windows.** Their budgets' figures leave out
  clean code pages (ADR-038: `phys_footprint` says so; Windows' private
  working set by Microsoft's definition, not measured), so the idle
  saving is Linux's alone and what is left there is a cold start. The
  linkers can (`-order_file`; `/order:`, though the Windows build links
  through zig and MinGW, which name no such option), but Apple silicon has
  no Intel PT, Wine does not open the window (ADR-074), and neither can be
  measured here.
- `-z pack-relative-relocs` read 3.3 MiB lower but needs glibc 2.36, above
  the tarball's floor; not taken.
- `panic=abort` was not an option: `catch_unwind` guards the scheduler's
  jobs, commands and plans. `--icf=all` saved 2 MiB of code and 0.5 of
  PSS; not taken.
