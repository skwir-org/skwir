# ADR-049: A file and its names

Status: accepted, 2026-10-10. Revises ADR-000's principle 6 ("entries
have stable identity: device + inode") and ADR-036's first point: the
device and the inode are the identity of a file, not of a row. Every
point is built (Consequences).

## Context

An entry's identity (`EntryId { dev, ino }`) is what a stat reports of
the file, and bibli keys by it both what belongs to the file and what
belongs to the name it was found under:

| The file (the inode) | The name (an entry in a folder) |
|---|---|
| its bytes, size, blocks, times, owner, mode | the name, and the folder it is in |
| notes in extended attributes (tags, rating, comment) | notes beside it (`x.jpg.xmp`, `._x`, `.ts/x.json`), tags in names |
| thumbnails, fields (a photo's position), what a preview reads | what rename, move, trash and delete act on |
| the space it takes | privacy (a folder is private); shelf, Recent and trash records |

A file can have several names: hard links (pnpm's `node_modules`, a Nix
store optimised, snapshot backups), and in a listing of several folders
the same file reached twice (through a bind mount, or two folders
holding its names). Two names of one file in one listing were one row:
a folder holding `a.txt` and its hard link `b.txt` listed one of them,
and a `content:` search of a home folder lost 725 to 2,036 of its matches
so (`bibli-bench trigram`, measured 2026-10-10). Worse than a row
missing, two bugs could act on the wrong name or leave one behind:

- `EntryRef::resolve` followed a reference whose name was gone, or held
  another file, to any name of the same inode in its folder: a plan to
  trash or delete `b.txt` could act on `a.txt`, for good.
- Empty Trash planned one deletion per inode: of two names of one file
  trashed apart, one stayed in the trash with its record.

What other tools do decided the rest. `du` counts a file of several
links once (POSIX: once under an operand); ncdu counts each file once in
every folder holding it, so a folder's subfolders can add up to more
than it, marks files of several links `H`, and warns that a folder's
size is not what deleting it frees. Windows Explorer counts every name,
which Microsoft documents as overstating the WinSxS folder. `btrfs
filesystem du` shows a Total, what is Exclusive (what deleting frees)
and what is shared. GNU `cp -a` and `tar` keep the hard links among
what they copy; `rsync -a` does not unless asked (`-H`, which costs
memory per file of several links); GUI file managers make independent
copies. Nautilus checks the destination's free space before copying,
and offers Copy Anyway since network shares reported 0 free.

## Decision

1. **Two identities.** A file is `FileId { dev, ino }`, what a stat says
   (`EntryId` before). An entry, a row, is `EntryId { file, at }`, `at`
   a hash of the folder holding its name (that folder's own `FileId`)
   and the name (`EntryId::in_folder`): the directory entry itself, the
   same in a folder's listing, a search's, the trash's and Recent's
   (`stat_entry` names every entry it stats so), kept while a folder
   above is renamed, and two for the same name in two folders (pnpm's).
   A location that knows its entries by path alone and may not read
   their folders (what was dropped on the shelf, a tag's files) hashes
   the path (`EntryId::at_path`); a place keeps its sentinel (ADR-032).
   The stat functions return a `FileId`, so a comparison of a stat with
   a row does not compile until it says which it means. What belongs to the
   file is keyed by the file (thumbnails, with the type they were made
   for; fields; the notes kept in attributes; Usage's links; positions),
   what belongs to the name by the row (rows, selection, cursor, cut,
   the notes snapshot of the stores beside a file, what is withheld from
   an agent).
2. **A rename re-keys a row.** The folder provider pairs the name that
   went with the one that came by the file's identity, as it does now,
   and says so (row X is now row Y); the selection, the cursor, the
   folders opened in place, the cut dimming and the preview follow it.
3. **A reference follows a file only when it knew the file had one
   name** (`EntryRef::only_name`: a folder, or a file whose link count
   was one), and only while one name in its folder holds it. Otherwise
   it resolves at its path, and a name gone is "not there any more", one
   holding another file "now another file". Not known is no: of a file
   with several names, a name gone and a name renamed look alike, and a
   wrong delete cannot be taken back. **Built.**
4. **Items count names; space counts files.** The selection's count, a
   folder's, a search's, the shelf's, the trash's are names. Space is
   each file once within what is totalled (a folder's weight in the
   footer, the selection's size, the trash's, Usage as it already does);
   a file with names outside still counts, as it takes that space. What
   deleting frees is a third figure, the files whose every name goes,
   shown only where deletion is in question (Usage, Delete for good,
   Empty Trash) and only when it differs. A file of several names says
   so where it is shown (Details' Hard links row, a mark in Usage).
5. **Copies keep the links among what they carry.** A copy, and a move
   to another filesystem, link each later name of a file to its first
   copy, from one table the plan's steps share, holding only files of
   more than one link and forgetting each once all its names are
   copied. A file copied alone comes out independent. Where the
   destination refuses links (FAT, exFAT, some network filesystems),
   the data is copied, and counted.
6. **A copy checks the destination's free space first,** against what
   it will write (point 5's count), and offers to go ahead anyway; a
   destination filled meanwhile still fails as it does.
7. **The trash keeps an item per record:** two records whose files are
   one are two items; one trash reached under two paths is not two.
   **Built.**

Hard links are the only sharing a stat shows. Space shared below files
(reflink copies, which bibli's own copies are on btrfs; snapshots;
deduplication) is counted in full, and bibli says that is what its
sizes mean rather than estimate extents, a filesystem tool's work.

## Consequences

- A row's key grows from 16 to 24 bytes and stays `Copy`; each name is
  hashed once, on the provider's thread. `just bench-check` says what
  that costs a listing of a hundred thousand.
- Nothing stored holds an `EntryId` but the throwaway cache, whose
  layout is bumped. The agents' wire format changes (`entries`' ids,
  `select`'s, plans' references) before the first release.
- Notes in attributes are the file's: tagging `a.jpg` tags `b.jpg`.
  Notes beside a file are the name's, so two names can show different
  notes. The tags index learns the other names at its next check
  (setting an attribute moves the change time of every name).
- Undoing an operation on a file renamed since, whose names were not
  known (a row not stat-ed yet, an agent's reference by identity),
  says it is gone where it used to follow: the price of never acting on
  another name. A reference made by a stat (`EntryRef::from_path`), the
  selection's rows once stat-ed and the operations' inverses know.
- On HFS+ (Time Machine before APFS) a folder can have several names;
  point 3 takes a folder for one name, which holds everywhere else.
- Built 2026-10-10: points 3 and 7, with tests of both (ops:
  `entry_ref_never_follows_a_file_to_another_of_its_names`,
  `trashing_or_deleting_a_name_gone_spares_the_files_other_name`; trash:
  `emptying_the_trash_deletes_every_name_of_a_file_trashed_twice`). Then
  points 1 and 2: a folder's listing, a search, the trash, the shelf,
  Recent and a tag's files list each name; a rename is
  `ListingEvent::Renamed`, which the folder's watcher and a search's
  check after a plan send and the pane follows; what waits for a paste
  is each entry by its file and name, so cutting one name dims it alone;
  the notes snapshot is a name's; thumbnails, previews' conversions,
  fields and Usage's hard links are a file's; Usage names its rows as the
  folder's listing does, so its selection is the pane's. Tested by
  `two_names_of_one_file_are_two_entries` (listing),
  `a_file_found_under_several_names_is_a_result_under_each` (query) and
  the pending paste's test. Then point 4: a folder's weight in the
  footer (added up from its listing, or read by a worker, which asks the
  system how many names each file has), the selection's size and Usage's
  selected files count each file once (`entry::weight`); Usage keeps,
  for each folder, what its files with names outside it take on disk
  (`Node::shared`), and says what deleting a folder frees where that is
  less, and of a file that it is one of several names. Left, as rare
  and costly to tell: two folders selected in Usage that share a file
  each count it (a row keeps no table of its links).
  Then point 5: a copy, and a move to another filesystem, link a later
  name of a file to the copy the plan made of it (`StepContext::
  link_copied`, its table forgetting a file once all its names are
  copied), and count its bytes once; a link refused (another volume, one
  that keeps none) copies the data. Tested by
  `a_copy_keeps_the_links_among_what_it_copies`. On Windows `std` says
  nothing of a file's names: there each name is copied, as before.
  Then point 6: a plan the user asked for as one action
  (`Executor::execute_whole`) adds up, after its conflicts are checked,
  what its copies and its moves to another filesystem will write
  (`Operation::writes`) by the volume they write to, against what each
  says is free (`fs::volume::free`), and when one has too little asks
  whether to go on (`op::room`; the review's question, Cancel or Copy
  anyway, `app::Asker::Room`), refusing it with what it needs and what
  is free otherwise. Tested by `a_copy_short_of_room_asks_whether_to_go_on`
  and `room`'s own. Its cost: a copy's sources are walked once more
  before it starts, to size them. Not asked of a trash onto another
  volume (a volume without a trash of its own, rare).
- Then what a deletion keeps: Delete for good of the selection, and
  Empty Trash while its listing shows its items, note in their question
  what stays on the disk, its files having names elsewhere
  (`entry::kept_by_other_names`, from the link counts the listing has:
  nothing is said while one is not known); and a thumbnail is of a file
  as a type (`thumbs::key`, its MIME type hashed in), so `x.jpg` and its
  link `x.bin` are two pictures. Tested by
  `deleting_one_name_of_a_file_says_it_stays_on_the_disk`,
  `emptying_the_trash_deletes_every_name_of_a_file_trashed_twice` and
  `a_picture_is_of_a_file_as_a_type`.
