# ADR-011: mimalloc, tuned, is the allocator of the skwir binaries

Status: accepted, 2026-09-24. Amended 2026-10-06 (idle workers give back
what they freed).

## Context

Holding the down arrow through about 130 mixed files (large photos, PDFs,
archives, office documents, text) with the preview open took RSS from 205 MB
to 660 MB in two passes, where it stayed, idle or not. heaptrack showed no
leak: at most 242 MB was live, and nothing a preview decoded outlived it. The
rest was freed memory never returned:

- The preview decodes a photo at full size (tens of MB) before scaling it,
  on whichever worker runs the request, and a started decode cannot stop.
  Holding a key ran one on every worker at once.
- glibc's malloc keeps freed memory in the thread's arena, and after a large
  free it raises its dynamic mmap threshold, so later large blocks come from
  the arenas too. RSS settles at the worst overlap of the workers' decodes.

Same commit, same private X display (Xvfb with lavapipe, so totals include a
software GPU), five fast down-and-up passes with the preview open; only the
allocator differs:

| Allocator | Idle heap at start | RSS after 5 passes | Memory returned | 100k listing streamed |
|---|---:|---:|---|---:|
| glibc (before) | 48 MB | 545–660 MB | never | 45 ms |
| glibc, mmap threshold 4 MB | 47 MB | 306 MB | at once | 45 ms |
| mimalloc, defaults | 88–94 MB | 360 MB | not while idle | 28 ms |
| mimalloc, tuned (this) | 52 MB | 263 MB | at once | 28 ms |
| jemalloc, defaults | 56 MB | 310 MB | not while idle | 28 ms |
| jemalloc, background purge | 56 MB | 260 MB | within 1 s | 28 ms |

- "100k listing streamed" is `flat100k.stream_complete_us` from
  `skwir-bench check`, two rounds each; other benchmarks and the first frame
  moved within noise.
- Stripped binary: +0.13 MB (mimalloc), +0.67 MB (jemalloc). Clean release
  build: +50 s (mimalloc), +4 min (jemalloc's configure script).

## Decision

1. **mimalloc is the global allocator of `skwir` and `skwir-bench`**,
   declared once in `crates/alloc` (`skwir-alloc`) and enabled by each
   binary's default `mimalloc` feature, so benchmarks measure what ships.
2. **Tuned:** purge delay 0 (freed memory returns at once; the v3 default
   waits a second and never purges an idle process) and arena eager commit
   off (the default commits each arena when reserved: 30 MB more at startup).
   An ELF constructor sets both before `main`, because the runtime allocates
   before `main` and the first arena takes the setting in force when
   reserved. The options are set by their number in mimalloc v3's enum, which
   the bindings do not name; the crates are pinned and a test checks that each
   number reads its option's default.
3. **The preview decodes one picture at a time** (`preview::DECODE_TURN`). A
   request that waited gives up its turn when the panel moved on, so
   passed-over files are never decoded. Peak live heap over the same browse:
   242 MB before, 164 MB after, whatever the allocator.

Alternatives: jemalloc (slightly lower after browsing, but +9 MB at startup,
a background thread, a slower build); glibc with a fixed mmap threshold (free
at startup, but keeps glibc's slower listing).

## Consequences

- Idle heap at startup is 4 MB above glibc's (52 against 48 MB); this record
  is their reason. ADR-002's 100 MB PSS budget is unchanged (still open then;
  closed by ADR-033).
- heaptrack sees only glibc's malloc: profile a build without the allocator,
  `cargo build --release -p skwir --no-default-features`.
- Upgrading `mimalloc` / `libmimalloc-sys` is deliberate: check the option
  enum and run `skwir-alloc`'s test.
- Worker processes (PDF, media, archives) keep the system allocator. Not
  measured; the media worker lives as long as a track plays and may deserve
  the same look.

## Amendments

### 2026-10-06: idle workers give back what they freed

mimalloc returns a thread's emptied pages only when that thread allocates
again, so a scheduler worker idle after a burst (a listing's metadata, a
trash read) kept them, possibly for hours. Forcing a collection on idle
workers measured 1 MiB less anonymous memory at idle, with or without the
plugins behind the burst.

- A worker idle for a second after a job calls `skwir_alloc::collect`
  (`mi_collect(true)`, which also returns what arenas keep free) once, until
  its next job (`skwir_core::scheduler::release_when_idle`, wired by the
  binary beside `tune`). A worker busy again within the second does not
  collect, so bursts never pay.
- `just idle-pss`, alternated back to back with the previous build: own share
  92.8–93.0 MiB, then 91.5–91.6 MiB.
- Threads outside the scheduler (GPUI's executor, a plugin's own watcher) are
  not covered: work that allocates belongs on the scheduler.
