# ADR-092: The harvest is scoped and yields: what is indexed, when, and the budgets that hold it

Status: accepted, 2026-10-26, the user's choice
(`docs/indexing-and-ai.md`). Not built.
Applies ADR-000's principles 1 and 2 to background indexing; the rules
are `docs/invariants/indexes.md`.

## Context

- The reference home holds 5.8 M entries, 652,336 folders, mostly
  development trees, package stores and caches. What a person searches
  for by content or meaning is a small part of it. The desktops draw
  that line already: GNOME's LocalSearch indexes the XDG user folders
  (and widens with care), Spotlight skips a folder named `.noindex`,
  Baloo and LocalSearch honour `.nomedia`, and build tools mark their
  caches with `CACHEDIR.TAG`.
- The user's one requirement for this layer: never an untimely
  slow-down of the host. ADR-000 budgets idle memory and the first
  frame; nothing yet budgets background work.
- What slows a host: CPU taken from the foreground, reads that evict the
  page cache, a spinning or USB drive seeking for two readers (the copy
  timings on the USB drive vary with that), a battery drained, a machine
  under memory pressure pushed to swap, a remote server asked for files
  no one opened (`docs/invariants/remote.md`).

## Decision

1. **Scope** (`harvest::Scope`, the Index page): roots default to the
   XDG user folders (Documents, Pictures, Videos, Music, Desktop,
   Downloads), never the home folder itself; folders left out by name
   (`extract.exclude`: `.git`, `.hg`, `.svn`, `node_modules`,
   `__pycache__`, `.venv`, `venv`, `.cache`) and always those holding
   `CACHEDIR.TAG`, `.noindex`, `.nomedia` or `.trackerignore`; hidden
   entries off; private folders never (`permissions::Privacy`, asked
   before a folder is entered); a remote never; a removable drive asked
   once per volume; a mount of another device under a root not entered
   unless the root is on it; a file past `extract.max_mb` (32) not read.
2. **Pace** (`harvest::Pace`, read before every batch): the harvest and
   the helper run only with no input to Skwir in the last two seconds
   (`App::last_input`) or on the desktop's idle hint; the one-minute
   load average below the core count; not on battery unless
   `extract.background` is Always; no memory pressure (PSI on Linux,
   the systems' notifications elsewhere). A batch is at most 64 files or
   32 MB, so the harvest pauses within one batch of the user's return.
   Without a window (`skwir index`, ADR-089) the session's idle signal
   is the only sign of the user: where the desktop gives none, the run
   reads nothing.
3. **Unnoticed:** the harvest's thread and the helper in the idle
   scheduling class and I/O priority (`SCHED_IDLE`, the idle I/O class;
   background QoS on macOS; background mode on Windows);
   `posix_fadvise(DONTNEED)` on a file over a megabyte once extracted;
   one file at a time on a rotational drive; one inference at a time in
   the helper; models memory-mapped.
4. **Off by default.** `extract.background` is Off until the user turns
   it on; "learned from looking" (ADR-091) works regardless. Revisited
   once the harvest's cost is measured.
5. **Room.** Each index has its cap (`extract.mb`, `fulltext.mb`,
   `semantic.mb`) and says when it stopped learning, as the tags index
   does with `tags.index_mb`; past it, nothing new is read and the
   footer and the Index page say so.
6. **Budgets,** in `budgets.toml` once measured and kept by
   `just bench-check`: `extract_text_us`, `trigram_segment_us`,
   `trigram_query_us`, `static_embed_us`, `vector_search_us`,
   `harvest_yield_ms` (from the user's input to the harvest pausing, at
   most one batch); and `just idle-pss` with every AI plugin on and
   idle equal to the baseline.

## Consequences

- A settings page, Index, with the scope, the background setting, and
  per root the files read and unread, the proof in words, the room
  used, Pause, Index now and Forget.
- `App::last_input` is a new core atomic the shell stamps on input
  events; nothing else reads it per frame.
- `skwir-bench harvest` measures files per second, the pause latency
  after a simulated input, idle CPU paused, and the helper's PSS loaded
  and after exit.
- Not measured without root: a cold cache; recorded when it is.
  Thermal signals are left for later; the load average stands in.
- A scope that leaves a folder out is said in the footer of a search
  there ("not indexed: left out"), never silently.
