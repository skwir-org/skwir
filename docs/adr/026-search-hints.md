# ADR-026: Indexes hint, the walk answers

Status: accepted, 2026-09-30. Built with the first hint, a `tag:` search,
answered by Baloo and by an index the tags plugin keeps.

## Context

Tags, a rating and a comment live on the file, in its extended
attributes (AGENTS.md, "Notes live on the file"). Nothing else holds
them, so a search by tag reads every file's attributes under its root.
Measured on the reference machine (btrfs, warm cache, one file in a
thousand tagged, `bibli-bench search deep1m tag:bench`):

| per million files          | 1 thread | 8 threads | 20 threads |
|----------------------------|----------|-----------|------------|
| `statx`                    | 2.15 s   | 0.51 s    | 0.32 s     |
| `listxattr`                | 3.93 s   | 1.59 s    | 1.88 s     |
| `tag:` search, whole       |          |           | 3.0–3.8 s  |

Listing a file's attribute names costs about twice a stat on one thread
and stops scaling at about eight (btrfs searches its tree for each), so
no scan of a million files answers `tag:` in under about 1.5 s here.
The same measurements found that the search opened each file's folder
again to stat it (`stat_path`): through a folder kept open
(`fs::Statter`), a search that stats every file went from 2.2–2.8 s to
0.64–0.9 s, and `tag:` from 3.8 s to 3.0 s. The rest is the attributes.
A cold cache is slower still, by an amount this machine cannot measure
without root (`bibli-bench search` prints each run, the first after
`echo 3 > /proc/sys/vm/drop_caches` being the cold one).

An index answers in milliseconds. ADR-000 rules out a daemon and a
database; ADR-003 already allows an index that is "an accelerator, never
a requirement", in the deletable cache, and says the system's indexes
(Baloo, Tracker, Spotlight) may serve a term when present.

## Decision

- **A hint is not an answer.** `bibli_core::hints`: a plugin registers a
  `HintProvider` that names files that may have a value in a field
  (`keys::TAGS` first). A search whose every match must have a tag (a
  `tag:` term, alone or in an AND) asks the providers first, considers
  the files they name as it considers any file (stat, providers,
  every term), publishes those that match, then walks as it would have,
  skipping the files it already considered. What the walk would not
  reach stays out: a hidden file unless `hidden:true`, one deeper than
  `depth:`, one through a folder that is a link. An index out of date
  costs a few checks; the result is the walk's, always.
- **The system's index first.** The tags plugin asks Baloo
  (`baloosearch -d root tag:"…"`) while Baloo's settings say it indexes
  (`baloofilerc`, `Indexing-Enabled`); Baloo indexes the desktop's tags,
  which are ours. bibli never starts or configures it.
- **An index of our own, learned, not built.** The tags plugin keeps the
  tagged files it has seen in the cache (namespace `tags`, key `index`):
  every file its provider reads, in a folder shown or in a search's
  walk, says whether it has tags, so the index follows edits made here
  or elsewhere the next time the file is read. It is read from the cache
  the first time a worker needs it, never on the way to the first frame
  (the sidebar's Tags ask it at background priority); files gone are
  forgotten when it answers; it holds at most 50 000 files. Deleting it
  makes the next search's first answers slower, nothing else.
- The walk is not skipped when an index claims to be complete: no index
  here knows every file the walk reaches (Baloo excludes folders and
  hidden files; ours knows what was read), and one wrong "complete" is a
  missing result.

## Consequences

- A second `tag:` search of the million-file tree shows its 1 000 files
  after 0.10 s instead of several seconds; it completes when the walk
  does (3.3–4.0 s). Tested against a private Baloo
  (`bibli-plugin-tags`, ignored test `a_private_baloo_names_the_tagged_files`,
  which runs `baloo_file` on a D-Bus session of its own).
- `content:` can use the same hook when a full-text index lands
  (ADR-003); Tracker and Spotlight would be further providers.
- What the index made possible (2026-09-30): the sidebar's Tags (the
  tags on the most files it knows, each a `tag:` search of the home
  folder), published when it changes. Its operations tell it what they
  wrote, since a file edited while no pane shows it is not read again
  soon. Renaming or removing a tag everywhere does not trust it: the
  files come from the search, walk included, and the plan is proposed
  for review.
- `bibli-bench` measures the provider (`tags_fill_us`) and the search
  (`tag_search_us`); their budgets are in `budgets.toml`.
- Amended (2026-10-01): the index also covers folders. The Tags view's
  count (`count.rs`) teaches it every tagged file under a folder, outside
  hidden folders, and records the folder as covered as of when the count
  began, with the folders it saw (each by device, inode and path). A
  later count only checks: files whose change time moved since are read
  (setting an attribute or renaming moves it; a companion's time stands
  for its file's), folders not seen there are read whole (new, or moved
  in with their old times), and so are folders on a filesystem whose
  change times do not tell (network, FUSE: `statfs`). This is the one
  place the index is trusted as complete, and only for counting: a
  `tag:` search still walks, as above. The cover ends when the index is
  full, or when bibli moves the folder. `tags_check_us` measures a check
  that finds nothing changed on the million-file tree (1.3 s, against
  3.0 s for the search).
