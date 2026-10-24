# Notes invariants: tags, ratings, comments, companions

Read this before changing how notes are read, written, merged, indexed or
copied (`skwir_core::notes`, `skwir_core::companions`, the `tags`, `xmp`,
`tagspaces` and `name-tags` plugins). Binding, like `AGENTS.md`.
Reasoning: ADR-027 (stores), ADR-028 (companions), ADR-006 (copies).

## Stores

Notes live where the desktop and other applications keep them, never in
a database of ours. Each place is a store (`notes::NotesStore`,
registered in `App::notes` by its plugin); all share one model,
`notes::Notes`, where a color belongs to its tag.

| Plugin | Stores |
| --- | --- |
| `tags` | `user.*` attributes (`xdg.rs`, Dolphin's keys); a Mac's Finder attributes, under whichever name they reached Linux (`finder.rs`); `._` files on volumes without attributes (`appledouble.rs`, which also keeps the desktop's attributes as `xdg.tags`) |
| `xmp` | sidecars `x.jpg.xmp`, and Adobe's `x.xmp` while its stem is its file's alone, edited in place so unknown content stays; embedded XMP, read for Properties only |
| `tagspaces` | `.ts/x.json`, a folder's `.ts/tsm.json` (exact color kept, read as the palette's nearest), `.ts/tsi.json` as a search hint |
| `name-tags` | `name[a b].ext` (TagSpaces, read only where a `.ts` folder exists); `name -- a b.ext` (filetags, off until asked) |

- A store in names is never reported as differing, never made to agree,
  never started, and holds no tag in a category. Writing a name is a
  rename: a plan of its own (`names.tag`; `names.untag` first has the
  other stores keep the tags), or, with its Write set to Update (Off by
  default), a `Rename` beside an edit of a file whose name is read as
  holding tags (ADR-027's 2026-10-18 amendment). The edit unfolds into
  both on the plan's thread (`SetNotes::unfolds`, from
  `NotesStore::renamed`), never when it is made; the name keeps its tags'
  order.

## Reading

- A folder's batch is read from every store with one `listxattr` per
  file (`NoteStores::read`), in the provider. A new store joins that read,
  never a read of its own per row.
- A folder's `._` names are read once per listing; `x` is read again when
  `._x` changes.

## Writing

- An edit is a core `set_notes` step built with `app.notes.edit(entry)`,
  capturing the write policy when the plan is made. It writes every store
  the policy allows: those holding the file's notes, those set to create,
  and where the file's attributes are refused, the store its folder
  already uses, else AppleDouble.
- Its inverse (`restore_notes`) puts back each store's exact bytes, and
  refuses if another application changed them since.
- A `._` file is rewritten whole as macOS lays it out, keeping every
  other attribute and its mode; one laid out otherwise is never written.
- An edit of several files applies the change (`tags.edit`: add these,
  remove those), never a list written over each file.
- Commands take `entries`, so Properties and the tag editor act on what they
  showed, not on the selection at the time.
- Stores that disagree are merged against what Skwir last wrote (a
  snapshot in the cache): another application's later edit wins; an
  emptied store is not an edit. Properties shows where notes are and which
  stores differ; `notes.agree` aligns them.
- A snapshot is the file's under its name in the folder holding it, that
  folder as its listing opens it (a link to it followed): the notes keep
  it so (`snapshot_key`), whatever id a reader holds, so a tag's, the
  shelf's and a folder's rows of one file merge alike.

## Tags

- **Color:** the user's (`tags.colors`), else the file's (a Finder tag,
  `keys::TAG_COLORS`), else the user's for the nearest category, else the
  name's (`metadata::tag_color`, a category's at the top).
- **Categories** are in names: `Places/France/Paris`
  (`metadata::TAG_SEPARATOR`, as digiKam; Lightroom's `a|b` reads as
  `a/b`). A category holds what is within it everywhere
  (`metadata::tag_within`): searches, index counts (each file once),
  renames (`Places` takes `Places/France` along, pin and color included),
  removals. Nothing writes categories onto files. A chip shows the leaf
  (`metadata::tag_leaf`), Properties the path. `tags.move` moves tags into a
  category.
- **Exclusive categories** are Skwir's `key:value`: a category holding
  one tag at a time (`tags.exclusive`, `metadata::TAG_EXCLUSIVE_SETTING`)
  replaces its value on every edit (`metadata::with_exclusive`), takes
  one tag in the tag editor, shows as its own Properties row (`Status:
  Draft`), and keeps the mark when renamed.

## The tags index

- A search by tag asks the indexes (`skwir_core::hints`, ADR-026: Baloo,
  and the plugin's `index.rs`, kept in pieces by folder, bounded by
  `tags.index_mb`), checks what they name, and walks anyway.
- The index learns from what providers read and from applied changes
  (`Executor::observe`).
- Renaming or removing a tag everywhere (`tags.rename`, `tags.delete`;
  several merge into one plan) takes its files from the search, as a plan
  for review. Under home, the tag's pin and color follow as a step of the
  same plan (`follow.rs`), so Undo restores them.
- **Counting** (`count.rs`): a tag location teaches the index every
  tagged file under its folder, covering the folder as of the count's
  start. Shown again, even after a restart, it lists the counts at once,
  then checks: only files (and companions) whose change time moved, plus
  folders not seen before (by device, inode and path) and filesystems
  whose change times do not tell (`filesystem::change_times_tell`:
  network, FUSE). A folder checked once per run is checked again only on
  F5; between, the index follows what panes list and Skwir applies.
  `tags.counted` reports the state.

## Companions (ADR-028)

- A companion (`._x`, an XMP sidecar, `.ts/x.json`) is declared by name
  rules (`companions::Companion`, registered in `App::companions`). The
  core agrees on them in three places:
  1. **Listing:** marked (`Listing::with_companions`), hidden when their
     file is listed unless hidden files and `core.show_companions` are
     both on. An orphan is an ordinary file.
  2. **Notes:** a change to a companion is a change to its file's notes.
  3. **Operations:** rename, move, copy, swap, trash, restore and delete
     carry companions in the same step, inverses included; a companion's
     destination taken is a conflict.
- Copying onto a volume that refuses attributes keeps them beside the
  file (`._` copied whole or created); where attributes are kept, a `._`
  file's are taken in. A copy fails only when nothing can keep them.
  Copies keep every `user.*` attribute (ADR-006).
- Properties lists a file's companions, and an orphan's owner.
- A batch whose folder's names are all known is filled with them
  (`MetadataProvider::fill_listed`): stores find companions in the
  listing, not the folder.
- Tests needing a volume without attributes use `SKWIR_NO_XATTR_DIR` (a
  `bindfs --xattr-none` mount).
