# ADR-026: Indexes hint, the walk answers

Status: accepted, 2026-09-30. Built: `tag:` searches hinted by Baloo
and by the tags plugin's index. Amended by ADR-050 (2026-10-10, not
built): an index that proves its coverage answers; and by ADR-094
(2026-10-26): hints gain a proof and a score for terms that rank.

## Context

Tags live in extended attributes (`docs/invariants/notes.md`),
so a `tag:` search reads every file's attributes. Reference machine,
btrfs, warm cache, one file in 1 000 tagged,
`skwir-bench search deep1m tag:bench`:

| per million files    | 1 thread | 8 threads | 20 threads |
|----------------------|----------|-----------|------------|
| `statx`              | 2.15 s   | 0.51 s    | 0.32 s     |
| `listxattr`          | 3.93 s   | 1.59 s    | 1.88 s     |
| `tag:` search, whole |          |           | 3.0–3.8 s  |

`listxattr` stops scaling at about eight threads, so no scan of a
million files answers `tag:` under ~1.5 s here. (Stat-ing through a
folder kept open, `fs::Statter`, already took `tag:` from 3.8 s to
3.0 s.) Cold cache is slower, by an amount not measurable without root.

An index answers in milliseconds. ADR-000 rules out a daemon and a
database; ADR-003 allows an index that is "an accelerator, never a
requirement", in the deletable cache, and the system's indexes.

## Decision

- **A hint is not an answer.** A `HintProvider` (`skwir_core::hints`)
  names files that may have a value in a field (`keys::TAGS` first). A
  search whose every match needs a tag asks providers first, checks the
  files they name like any file (stat, providers, every term), publishes
  matches, then walks as before, skipping files already checked. What
  the walk would not reach stays out (hidden files unless `hidden:true`,
  beyond `depth:`, through a linked folder). A stale index costs a few
  checks; the result is always the walk's.
- **The system's index first.** The tags plugin asks Baloo
  (`baloosearch -d root tag:"…"`) while `baloofilerc` says
  `Indexing-Enabled`; Baloo indexes the desktop's tags, which are ours.
  Skwir never starts or configures it.
- **Our index is learned, not built.** The tags plugin caches the
  tagged files it has seen (namespace `tags`, key `index`): every file
  its provider reads updates it. Loaded when a worker first needs it,
  never before the first frame. Deleting it only slows the next
  search's first answers.
- **Never skip the walk on a claim of completeness.** No index knows
  every file the walk reaches (Baloo excludes folders and hidden files;
  ours knows what was read); one wrong "complete" is a missing result.

## Consequences

- A second `tag:` search of the million-file tree shows its 1 000 files
  after 0.10 s; it completes with the walk (3.3–4.0 s). Tested against a
  private Baloo (ignored test `a_private_baloo_names_the_tagged_files`).
- `content:` can use the same hook (ADR-003); Tracker and Spotlight
  would be further providers.
- The sidebar's Tags (most-used tags) come from the index. Operations
  tell it what they wrote, since a file edited while no pane shows it
  is not read again soon. Renaming or removing a tag everywhere does
  not trust it: files come from the search, and the plan is reviewed.
- Budgets: `tags_fill_us`, `tag_search_us` (`budgets.toml`).

## Amendments

### 2026-10-01: the index covers folders

A count (`count.rs`: the Tags view's then, a tag location's since
ADR-032) teaches the index every tagged file under a folder (outside
hidden folders) and records the folder as covered from the count's
start, with the folders seen (device, inode, path). Later counts only
check: files whose change time moved, folders not seen before, and
folders on filesystems whose change times do not tell (network, FUSE)
are read. This is the one place the index is trusted as complete, for
counting only; `tag:` still walks. Coverage ends when the index is full
or Skwir moves the folder. `tags_check_us`: 1.3 s for a no-change check
of the million-file tree (search: 3.0 s).

### 2026-10-01: the index is bounded by room

The 50 000-file cap became the user's `tags.index_mb` (64 MB default,
about a million tagged files). Tag names are stored once by number,
files by folder, with per-folder tag counts so a count sums folders.
Stored in 256 pieces by folder, each loaded on demand and dropped after
three minutes unused; written by the index's thread at most every 2 s
and on exit. Past its room it learns no new file and covers no folder;
a tag location says so with a link to the setting.

| 500 000 tagged files | before (50 000 cap) | after  |
|----------------------|---------------------|--------|
| memory               | 12 MB for 50 000    | 30 MB  |
| stored               |                     | 14 MB  |
| count of home folder | 345 ms              | 0.4 ms |
| one change written   | 34 MB rewritten     | 0.5 ms |

Kept by `tags_index_learn_us`, `tags_index_tally_us`,
`tags_index_change_us`.

### 2026-10-10: an index that proves its coverage answers (ADR-050)

A claim still never skips the walk; a proof does. Not built.

- **Proofs**, for a root and the facts a search's terms need:
  - **live**: every change on the root delivered, none dropped since
    the last full read, applied up to a barrier set when the search
    began (skwird's sync);
  - **checked**: every folder's change time equals the provider's
    record, on a filesystem whose change times tell
    (`filesystem::change_times_tell`). Proves names and kinds, never
    sizes or times.
- **Under a covered root** the search checks the files the provider
  names and does not walk; it still walks what is not covered (a mount
  below, a denied folder, untrustworthy change times).
- **The provider must reach what the walk reaches**: hidden names, no
  link followed, every mount below reported. It may name more files
  than match, never fewer.
- **plocate proves nothing**: a search of `/` it answers keeps skipping
  the walk, as before, but dates its answer to the database's update
  (built 2026-10-20, ADR-050's amendment).
- `HintProvider` gains coverage; skwird is the first provider; the tags
  count could be a second.
