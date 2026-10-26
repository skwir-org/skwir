# Index invariants: the harvest, extract, fulltext, semantic

Read this before working on `skwir_core::harvest`, the `extract`,
`fulltext` or `semantic` plugins, the hints' coverage, or anything that
reads files no one asked to see. Binding, like `AGENTS.md`. Reasoning:
ADR-089 (lifetimes), ADR-090 (trigrams), ADR-091 (the extract provider),
ADR-092 (scope and pace), ADR-094 (vectors); the design is
`docs/indexing-and-ai.md`. Nothing here is built yet: the rules bind
what is built.

## Every index is an accelerator

- Absent, damaged, of another layout, behind or over its room, an index
  changes what is fast, never what is found: the scan, the walk or a
  read answers instead. Nothing waits for an index to be built.
- An index lives in the deletable cache, in segments added and merged in
  the background, never rewritten whole. Its header records the layout
  version and every array's width; a mismatch rebuilds. A save that
  fails keeps what was in memory and its "as of".
- An index is memory-mapped when a search needs it and let go of after
  minutes unused; nothing is loaded before the first frame or kept for
  the window's lifetime.

## Coverage is never overstated

- Every answer from an index carries a proof (`harvest::Proof`): live
  since a time (every change applied up to a barrier), checked at a time
  (reconciled against the folders then), as of a time (loaded, not yet
  reconciled), or not covered and why. The four words are skwird's; the
  footer says them (`location::AsOf`), and `entries` answers them.
- A claim never skips the scan; a proof does (ADR-050). Under a covered
  root a search considers what the index names and nothing else; under
  any other it hints and scans, or, for meaning, lists what it has and
  says what it leaves out.
- A dropped change, a root over its room, a folder denied, a remote or a
  removable volume lowers the proof at once for the subtree concerned.
- A failed read is unread, counted with its reason, retried at the next
  reconcile; never an empty answer, never evidence of absence.
- The scan is the correctness reference for `content:`; a test runs the
  bench's queries through the index and the scan and compares.

## Scope

- Only roots the user set (`extract.roots`, the XDG user folders by
  default) and never the home folder itself; folders left out by name
  and by marker (`CACHEDIR.TAG`, `.noindex`, `.nomedia`,
  `.trackerignore`); hidden entries off; a mount of another device not
  entered unless the root is on it.
- A private folder is never entered, never read, never indexed, never
  named to a helper: `Scope::enters` asks `Privacy` before a folder, on a
  worker.
- A remote is never read unasked (`docs/invariants/remote.md`); a
  removable volume is asked once per volume, and the answer kept by its
  UUID.
- A file past `extract.max_mb`, a special file or a link's own entry is
  not read; a link's target is read under its own name only.

## One read per file

- A derived fact is a field (`FieldProvider`, ADR-023): the harvest asks
  `Fields::get_all` for the union of the indexers' keys, so a file is
  read once per version for every index; an index never opens a file.
- The readers are the previews' (`preview::is_office`, `shown_as_pages`,
  the reader helpers, Pandoc), under the previews' quick bounds; a
  picture is read from the thumbnail service, never the original; sound
  from the media helper's PCM. What the index holds is what the preview
  shows.
- Two keys: identity and version, as every field; and the content hash
  (`extract/by-hash`), computed in the same read when the file was read
  whole, so a file met under another identity is not read again.
- A preview or a thumbnail that already has a fact teaches it
  (`Fields::teach`); a plugin never depends on `extract` for it.

## Pace

- The harvest is one lane at `Priority::Background`, one batch in
  flight, on a thread in the system's idle scheduling class and idle
  I/O priority; the helper likewise when the harvest calls it.
- Before every batch, `harvest::Pace` holds: no input to Skwir for two
  seconds (`App::last_input`), load below the core count, not on battery
  unless Always, no memory pressure. A batch is at most 64 files or
  32 MB. The pause after input is a budget (`harvest_yield_ms`).
- `posix_fadvise(DONTNEED)` after extracting a file over a megabyte;
  one file at a time on a rotational drive; one inference at a time.
- `extract.background` is Off by default. Each index has a cap
  (`<id>.mb`) and says when it stopped learning.
- Without a window (`skwir index`), the session's idle signal stands for
  `App::last_input`; a desktop that gives none leaves the run reading
  nothing, and the Index page says so. One harvest runs at a time for a
  user (a lock in the `extract` namespace); a windowless run ends after
  its batch when a window opens.
- The service manager's entry is the switch's (`extract.while_closed`):
  written and removed by it alone, stopped before it is removed, removed
  with the plugin, taken back when the manager fails. Tests never touch
  the user's manager.
- `just idle-pss` with every AI plugin on and idle equals the baseline.

## Freshness

- Where skwird covers a root live, the harvest consumes its journal
  (`changes {root, since}`) from the token it kept; "too old" or a
  coverage drop falls back to the check.
- The check: folders whose change time moved are listed again (the time
  read before the entries, a second change within a tick read again
  next time, device and inode compared); then every file in scope is
  stat-ed and re-read where its version moved. Where change times do
  not tell (`filesystem::change_times_tell`), every folder is listed.
- Skwir's own plans tell the indexers what they changed
  (`Executor::observe`): renames and moves re-key, trashing forgets,
  writes re-read.
- A loaded index is "as of" until the session's first reconcile ends.

## Formats

- Trigram segments: about 32 MB of text each, a document table, the
  segment's trigrams sorted with a posting list each, tombstones;
  queries planned from the regex's HIR (`crates/bench/src/trigram.rs`
  is the reference).
- Vector segments: per root and model; a file table, a chunk table with
  byte ranges into `extract.text`, int8 vectors with a scale each,
  tombstones; the model's id, version and dimension in the header, so
  another model's vectors never pass for these. Brute force with SIMD;
  an approximate index only past a million vectors in a root, by
  measurement.
- A root is keyed by its folder's kept identity (`FileId::kept_as`), so a
  root moved keeps its index.

## Tests

- Fixtures are synthetic: bilingual names, generated documents, pictures
  of shapes and rendered words; never a real path, never a real name.
- Each way coverage can be lost has a test: a change while Skwir was
  down, a folder whose times `tar` set back, a folder denied then
  readable, a read cut short, a root over its room, a remote under a
  root, a `.private` marker added later.
- The harvest pauses within one batch of a simulated input, and resumes;
  a private folder added mid-harvest is not read; an index deleted
  mid-search answers from the scan.
