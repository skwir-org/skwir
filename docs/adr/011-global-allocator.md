# ADR-011: mimalloc, tuned, is the allocator of the skwir binaries

Status: accepted, 2026-09-24; addendum 2026-10-06 (idle workers give back
what they freed).

## Context

Holding the down arrow through a folder of about 130 mixed files (large
photos, PDFs, archives, office documents, text) with the preview open took
the process from 205 MB to 660 MB RSS in two passes, where it stayed: idle,
and through more passes over the same files. heaptrack showed no leak. At
most 242 MB was live at any moment, and nothing a preview decoded outlived
it. The rest was memory already freed:

- The preview decodes a photo at full size before scaling it down, tens of
  megabytes per picture, on whichever scheduler worker runs the request, and
  a decode that has started cannot be stopped. Holding a key ran one on every
  worker at once.
- glibc's malloc keeps a thread's freed memory in that thread's arena, and
  after a large block is freed it raises the size it serves from the OS
  directly (the dynamic mmap threshold), so the next large blocks come from
  the arenas as well. RSS settles at the worst overlap of each worker's
  decodes and stays there.

## Measurements

Same commit, same private X display (Xvfb with lavapipe, so totals include a
software GPU and differ from a desktop session), five fast down-and-up passes
with the preview open, the allocator the only difference:

| Allocator | Idle heap at start | RSS after 5 passes | Memory returned | 100k listing streamed |
|---|---:|---:|---|---:|
| glibc (before) | 48 MB | 545–660 MB | never | 45 ms |
| glibc, mmap threshold 4 MB | 47 MB | 306 MB | at once | 45 ms |
| mimalloc, defaults | 88–94 MB | 360 MB | not while idle | 28 ms |
| mimalloc, tuned (this) | 52 MB | 263 MB | at once | 28 ms |
| jemalloc, defaults | 56 MB | 310 MB | not while idle | 28 ms |
| jemalloc, background purge | 56 MB | 260 MB | within 1 s | 28 ms |

"100k listing streamed" is `flat100k.stream_complete_us` from
`skwir-bench check`, two rounds each; the other benchmarks moved within their
noise. First-frame times did not differ beyond run-to-run noise. The stripped
binary grows by 0.13 MB with mimalloc and 0.67 MB with jemalloc; a clean
release build takes 50 s more with mimalloc, 4 minutes more with jemalloc
(its configure script).

## Decision

1. **mimalloc is the global allocator of `skwir` and `skwir-bench`**, declared
   once in `crates/alloc` (`skwir-alloc`) and enabled by each binary's default
   `mimalloc` feature, so the benchmarks measure what ships.
2. **Tuned:** purge delay 0 (freed memory goes back to the OS at once; the v3
   default waits a second, and never purges an idle process) and arena eager
   commit off (the default commits each arena when it is reserved, 30 MB more
   at startup). Both are set by an ELF constructor that runs before `main`:
   the runtime allocates before `main`, and the first arena takes the eager
   commit setting in force when it is reserved. The two options are set by
   their number in mimalloc v3's enum, which the Rust bindings do not name;
   the crates are pinned and a test checks that each number reads its
   option's default.
3. **The preview decodes one picture at a time** (`preview::DECODE_TURN`). A
   request that waited for its turn gives it up when the panel moved on, so
   passed-over files are never decoded. Peak live heap over the same browse:
   242 MB before, 164 MB after. This is independent of the allocator.

jemalloc was the close alternative: slightly lower after browsing, but
+9 MB at startup, a background thread and the slower build. glibc with a
fixed mmap threshold costs nothing at startup but keeps glibc's slower
listing path.

## Consequences

- Idle heap at startup is 4 MB above glibc's (52 MB against 48 MB). The idle
  budget of ADR-002 is not changed: it stays 100 MB PSS and stays open; this
  record is the reason for those 4 MB.
- heaptrack sees only glibc's malloc. Profile the heap with a build without
  the allocator: `cargo build --release -p skwir --no-default-features`.
- Upgrading `mimalloc` / `libmimalloc-sys` is deliberate: check the option
  enum and run `skwir-alloc`'s test.
- The worker processes (PDF, media, archives) keep the system allocator.
  They were not measured here; the media worker lives as long as a track
  plays and may deserve the same look.

## Addendum (2026-10-06): idle workers give back what they freed

mimalloc returns a thread's emptied pages to the OS only when that thread
allocates again. A scheduler worker that ran a burst of jobs (a listing's
metadata, a trash read) and then waits for the next one, possibly for
hours, kept them: forcing a collection on idle workers measured 1 MiB less
anonymous memory at idle, with or without the plugins that caused the
burst. So each worker, once idle for a second after running a job, calls
`skwir_alloc::collect` (`mi_collect(true)`, which also gives back what the
arenas keep free), once until it runs another
(`skwir_core::scheduler::release_when_idle`, wired by the binary beside
`tune`). A burst never pays for it: a worker that gets another job within
the second does not collect. `just idle-pss`, alternated back to back with
the build before: own share 92.8–93.0 MiB to 91.5–91.6.

Threads that are not the scheduler's (GPUI's executor, a plugin's own
watcher) are not covered: work that allocates belongs on the scheduler.
