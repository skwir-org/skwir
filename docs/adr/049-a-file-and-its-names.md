# ADR-049: A file and its names

Status: accepted, 2026-10-10. Revises ADR-000's principle 6 ("entries
have stable identity: device + inode") and ADR-036's first point:
device and inode identify a file, not a row. All points built
2026-10-10.

## Context

`EntryId { dev, ino }` keyed both what belongs to the file and what
belongs to the name it was found under:

| The file (the inode) | The name (an entry in a folder) |
|---|---|
| its bytes, size, blocks, times, owner, mode | the name, and the folder it is in |
| notes in extended attributes (tags, rating, comment) | notes beside it (`x.jpg.xmp`, `._x`, `.ts/x.json`), tags in names |
| thumbnails, fields (a photo's position), what a preview reads | what rename, move, trash and delete act on |
| the space it takes | privacy (a folder is private); shelf, Recent and trash records |

A file can have several names: hard links (pnpm's `node_modules`, an
optimised Nix store, snapshot backups), or one file reached twice in a
multi-folder listing (a bind mount, two folders holding its names). Two
names in one listing were one row: a folder with `a.txt` and its hard
link `b.txt` listed one, and a `content:` search of a home folder lost
725 to 2,036 matches (`skwir-bench trigram`, 2026-10-10). Two bugs
could act on the wrong name:

- `EntryRef::resolve` followed a reference whose name was gone, or held
  another file, to any name of the same inode in its folder: a plan to
  trash or delete `b.txt` could act on `a.txt`, for good.
- Empty Trash planned one deletion per inode: of two names trashed
  separately, one stayed in the trash.

Prior art:

| Tool | Behaviour |
|---|---|
| `du` | a multi-link file once (POSIX: once per operand) |
| ncdu | once per folder holding it (subfolders can sum to more than their parent); marks them `H`; warns size is not what deleting frees |
| Windows Explorer | every name (Microsoft documents it overstating WinSxS) |
| `btrfs filesystem du` | Total, Exclusive (what deleting frees), shared |
| GNU `cp -a`, `tar` | keep hard links among what they copy |
| `rsync -a` | only with `-H` (memory per multi-link file) |
| GUI file managers | independent copies |
| Nautilus | checks free space first; Copy Anyway since network shares reported 0 free |

## Decision

1. **Two identities.**
   - A file is `FileId { dev, ino }` (formerly `EntryId`). Stat
     functions return it, so comparing a stat with a row does not
     compile until it says which it means.
   - An entry (a row) is `EntryId { file, at }`, `at` hashing the
     holding folder's `FileId` and the name (`EntryId::in_folder`;
     `stat_entry` sets it). It is the same in a folder listing, a
     search, the trash and Recent, survives renaming a folder above, and
     differs for the same name in two folders.
   - Locations that know entries by path alone and may not read their
     folders (shelf, a tag's files) hash the path (`EntryId::at_path`).
     Places keep their sentinel (ADR-032).
   - By file: thumbnails (with the type made for: `thumbs::key` hashes
     in the MIME type, so `x.jpg` and its link `x.bin` are two
     pictures), preview conversions, fields, notes in attributes,
     Usage's links, positions. By row: rows, selection, cursor, cut
     (cutting one name dims it alone), the notes snapshot of stores
     beside a file, what is withheld from an agent. Usage names rows as
     the listing does, so its selection is the pane's.
2. **A rename re-keys a row.** The folder provider pairs the name that
   went with the one that came by file identity and sends
   `ListingEvent::Renamed` (row X is now row Y; a search's post-plan
   check sends it too). Selection, cursor, folders opened in place, cut
   dimming and preview follow.
3. **A reference follows a file only when it knew the file had one
   name** (`EntryRef::only_name`: a folder, or link count one), and only
   while one name in its folder holds it. Otherwise it resolves at its
   path: "not there any more", or "now another file". Unknown counts as
   no: for a multi-name file, gone and renamed look alike, and a wrong
   delete cannot be undone.
4. **Items count names; space counts files.**
   - Counts (selection, folder, search, shelf, trash) are names.
   - Space counts each file once within the total (`entry::weight`:
     footer weight, selection size, trash, Usage); a file with names
     outside still counts. The footer's worker asks the system each
     file's name count.
   - What deleting frees (files losing every name) is a third figure,
     shown only where deleting is in question and only when different:
     Usage keeps per folder what its files with outside names take
     (`Node::shared`); Delete for good and Empty Trash say in their
     question what stays on disk (`entry::kept_by_other_names`, from
     listed link counts; silent while one is unknown).
   - A multi-name file says so (Details' Hard links row, a mark in
     Usage).
5. **Copies keep the links among what they carry.** A copy, or a
   cross-filesystem move, links each later name of a file to its first
   copy (`StepContext::link_copied`), via a table shared by the plan's
   steps, holding only multi-link files and dropping each once all its
   names are copied; bytes count once. A file copied alone comes out
   independent. Where links are refused (FAT, exFAT, another volume,
   some network filesystems) the data is copied and counted.
6. **A copy checks free space first.** A plan run as one action
   (`Executor::execute_whole`) sums, after conflict checks, what its
   copies and cross-filesystem moves write (`Operation::writes`, point
   5's count) per volume against `fs::volume::free`. If short it asks
   (`op::room`: Cancel or Copy anyway, `app::Asker::Room`), else
   refuses with needed and free sizes. Filling up meanwhile still fails
   as before.
7. **The trash keeps one item per record:** two records of one file are
   two items; one trash under two paths is not two.

Hard links are the only sharing a stat shows. Sharing below files
(reflinks, which Skwir's copies are on btrfs; snapshots; dedup) is
counted in full, and Skwir says so rather than estimate extents.

## Consequences

- A row key grows from 16 to 24 bytes, still `Copy`; each name is
  hashed once on the provider's thread (`just bench-check` measures it
  on 100,000 entries).
- Only the throwaway cache stored `EntryId`s (layout bumped). The agent
  wire format (`entries` and `select` ids, plan references) changed
  before the first release.
- Notes in attributes are the file's (tagging `a.jpg` tags `b.jpg`);
  notes beside it are the name's, so two names can differ. The tags
  index learns other names at its next check (an attribute write moves
  every name's change time).
- Undoing an operation on a file renamed since, whose names were not
  known (a row not stat-ed yet, an agent's reference by identity), says
  it is gone instead of following: the price of never acting on another
  name. References from a stat (`EntryRef::from_path`), stat-ed rows
  and operation inverses know.
- On HFS+ (pre-APFS Time Machine) a folder can have several names;
  point 3 treats folders as single-named, true everywhere else.
- Built 2026-10-10, all points. Tests: ops'
  `entry_ref_never_follows_a_file_to_another_of_its_names`,
  `trashing_or_deleting_a_name_gone_spares_the_files_other_name`,
  `a_copy_keeps_the_links_among_what_it_copies`,
  `a_copy_short_of_room_asks_whether_to_go_on`,
  `deleting_one_name_of_a_file_says_it_stays_on_the_disk`; trash's
  `emptying_the_trash_deletes_every_name_of_a_file_trashed_twice`;
  listing's `two_names_of_one_file_are_two_entries`; query's
  `a_file_found_under_several_names_is_a_result_under_each`;
  `a_picture_is_of_a_file_as_a_type`; `room`'s own; the pending paste's.
- **Gaps:**
  - Two folders selected in Usage that share a file each count it (rows
    keep no link table): rare and costly to tell.
  - On Windows `std` reports no names: each name is copied, as before.
  - The free-space check walks a copy's sources once more to size them.
  - Trashing onto another volume (one without its own trash, rare)
    does not check free space.
