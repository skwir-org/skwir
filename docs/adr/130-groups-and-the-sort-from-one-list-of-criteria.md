# ADR-130: Groups and the sort from one list of criteria

Status: accepted, 2026-11-11, on the user's answers to the proposal
"Grouping" (a page shared in the session, with mock-ups of the View
menu, a list grouped by date and a grid grouped by kind). Built in
phases (below): phases 1 to 4 on 2026-11-11, run on Linux, compiled
for Windows and, but for the shell, macOS (`just check-cross`). Amends
ADR-099 (the View menu's sort) and follows
ADR-121 (an order asked anew is made on a worker). Amended by ADR-131
(2026-11-11): headings the keys reach (9, 12), and phase 5 settled and
built; and on 2026-11-12 (below): groups told from their counts, costly
keys asked on every core.

## Context

- **Asked for** (issue #10, 2026-11-10): folders and files sorted
  together by date. Folders first became a setting (f34cab8e); the reply
  on the issue promised grouping by other criteria too. The user named
  Dolphin's Group By (26.08: None, Same as Sort, then criteria, in a
  submenu) as the model.
- **What others do**, read for the proposal: Dolphin's groups of an
  explicit criterion always run ascending (dates oldest first), sets
  folders apart before grouping (a heading can appear twice, once among
  the folders, once among the files: its own defaults turn Folders First
  off in Downloads), and steps dates by weeks, then one group per month
  for good. Finder keeps a fixed order per criterion; Windows Explorer
  gives the groups a direction of their own, apart from the sort's.
  Nautilus, Nemo and Thunar do not group.
- **Skwir's order is one flat list** of entries (`Listing::order_shown`),
  sorted on a worker when asked anew (ADR-121), which every view draws
  and everything acts on (the arrows, ranges, Select All, an agent's
  index). The sort had five keys (name, size, modified, rating, deleted),
  each from what core reads itself.
- **Core knows neither types nor the calendar** (ADR-037, ADR-017): a
  kind, a type's name, an owner's name and a day's bounds are the
  platform's, which plugins reach. The query plugin already owns kinds,
  date periods, sizes and tags, with their query terms (ADR-107).

## Decision

1. **One list of criteria** (`SortKey`) for the sort and the group:
   Name, Kind, Type, Extension, Size, Modified, Created, Accessed,
   Deleted (in the Trash), Rating (while ratings are kept), Tags, Owner,
   and Folder (where files come from many folders; phase 4). The sort
   gains every one it lacked; every list column sorts, Type and Folder
   included. A criterion is offered where its column would apply.
2. **Group by** (`SortSpec::group`): None, Same as sort, or a criterion
   with a direction of its own (`SortSpec::group_ascending`). Same as
   sort groups by the sort's criterion in the sort's direction. The group
   comes first in what an order is sorted by, so each group is one run
   of the one order; the order is made as any order asked anew is, on a
   worker (ADR-121). What a pane shows says where each group starts, its
   label and its count (`Pane::groups`).
3. **A criterion starts in its natural direction**, for the sort and the
   group alike: words A to Z (Name, Kind, Type, Extension, Tags, Owner,
   Folder), sizes largest first, dates newest first, ratings highest
   first. Choosing the criterion shown again flips it, as before; a
   column's header does the same. (Before, only Rating and Deleted
   started from the top.)
4. **A direction reverses its criterion, never the names**: entries that
   tie on it are listed A to Z, then by identity, whichever the
   direction. Sorted by name, the name is the criterion.
5. **Folders first acts within each group**: a group lists its folders,
   then its files, and each heading appears once. A group that holds only
   folders (Folders, by size or extension, where folders have none; the
   kind Folders) comes first while Folders first is on, whatever the
   group's direction; while it is off, it goes where the criterion puts
   it (by size, as having none).
6. **What has none is the lowest**: no date, no rating, no tags, no
   extension, a folder's size: first going up, last going down, as
   folders sorted by size already were. What is not read yet (a size
   before the stat) sorts after the rest in either direction, as before.
7. **Groups don't overlap**: each entry is in one, the first that holds
   it.
   - **Dates** (Modified, Created, Accessed, Deleted): Today, Yesterday,
     Earlier this week, Last week, Earlier this month, Last month,
     Earlier this year, then one group per year; In the future; No date.
     Days of the user's calendar (ADR-017), weeks from the locale's first
     weekday; a group left empty is not shown; the groups are made again
     at midnight.
   - **Size**: Empty, Under 100 KB, 100 KB to 1 MB, 1 to 100 MB, 100 MB
     to 1 GB, Over 1 GB; Folders.
   - **Name**: the first letter as the name sort folds it (É with E),
     0–9, and Other for a name that begins with neither; letters of
     other scripts each a group of their own.
   - **Kind**: the type database's kinds (`mime::Kind`), a file of
     several in its first; **Type**: the type's name; **Extension**: the
     extension, No extension, Folders; **Rating**: one group per rating,
     Unrated; **Tags**: the whole set a file has ("draft, print"), No
     tags, so no file is shown twice; **Owner**: the owner's name.
   - **Same as sort** is the sort criterion's groups: it moves no entry
     but a name that begins with neither a letter nor a digit, which
     gathers in Other.
8. **What a plugin knows, a plugin answers** (`registry::Sorter`, the
   query plugin's): for a sort, `Keys` that give an entry's value for a
   criterion core cannot read (Kind, Type, Owner) and an entry's group
   with its label, and when they next change. Keys answer from memory,
   never blocking: a type's name not read yet is read on a worker, the
   type itself standing for it meanwhile, and the order is made again
   once it is (`Keys::changed`). Core reads Name, Size, the dates,
   Rating, Extension and Tags itself. Keys are made again when the sort
   or the group changes, when the plugin is turned on or off, and when
   they say they change (midnight). With no sorter (the plugin off),
   Group by and the criteria it answers are not offered, and a pane
   sorted by one is in name order.
9. **Headings are rows, never entries** (phases 2 and 3). A view that
   groups keeps one map from its rows to positions in the order; drawing,
   scrolling and the pointer work in rows, everything else in positions.
   The cursor, ranges, Select All and the rubber band act on entries
   alone, and a screen reader's position and count are entries'. A
   heading is a row as tall as an entry, its label in Small text with
   the count beside it (ADR-117), the one at the top pinned. (Amended
   2026-11-12, the user: its label is Body, as the names it heads, its
   count Small.) The tree
   groups its top level only; a folder opened in place keeps its entries
   under it. The grid starts each group on a new row under a heading as
   wide as the pane, and Up and Down go to the tile drawn above or
   below. Columns, Board, Usage and Map do not group at first.
10. **The View menu** (amends ADR-099, decision 5): Sort by, its
    direction in the criterion's words, over a row that names the
    criterion and opens a fly-out of them all; Group by the same, its
    fly-out adding None and Same as sort, its direction shown only for a
    criterion of its own. Each is a command: `sort` as before, `group`
    (`by`, `ascending`, `pane`), with palette forms ("Group by kind");
    Group by has no default key. (Amended 2026-11-12, the user: Sort by
    and Group by are one row each saying how they are, "Name, A to Z",
    "Kind, Z to A", "None"; their fly-out holds the direction on its
    first row, then the criteria. Group by's direction is always there,
    greyed while not grouped by a criterion of its own, so a choice moves
    no row under the pointer. And, the user's go: Folders first stands
    under them, with the order it changes, no longer with what is shown;
    what is listed and what is drawn with it apart by a line.)
11. **Per pane, as the sort**: not saved across restarts. A place that
    remembers its own (Downloads by date) is for an ADR of its own.
    (Amended 2026-11-12, the user: the sort and the groups are saved
    with the pane's tab and come back with it when the windows do,
    `core.startup = last`; a place's own is still for an ADR of its own.)
12. **A heading acts** (phase 4): a click selects its group; its menu
    offers Select these and Show only these, which sets the group's
    query term as the lens (ADR-107). Collapsing comes later.
13. **Not done**: a second sort key (grouping is the second key: grouped
    by type, sorted newest first, each type's files are listed newest
    first under headings); folders sized by how many items they hold, as
    Dolphin does, which reads every folder shown (a request per folder
    on a remote mount) before the order can settle.

## Phases

1. Core and the query plugin: the criteria, `GroupBy`, the keys and
   their sorter, the order with groups first, the spans, the `group`
   command, the sort's new criteria and directions, the menu. Every date
   boundary tested against an explicit calendar.
2. List and Tree: the headings, the row map, the pinned heading, what a
   screen reader hears.
3. The grid: its breaks, Up and Down by what is drawn.
4. Group by Folder (the sort learns each entry's folder), Show only
   these, a heading's click and menu.
5. Later: collapsing; a Board with a column per group; criteria read on
   demand (the day a picture was taken). Settled by ADR-131 and built on
   2026-11-11, with headings as key stops.

## Consequences

- A pane sorted by date starts newest first, and one sorted by size
  largest first: a change for anyone used to the old starts.
- Every order carries the group, the file flag, the unknown flag, the
  value, the name and the identity; an order's ranks name the group and
  a criterion's words by a number into a table beside them, which points
  at what every entry with them shares: some 25 bytes an entry more
  while a listing is shown, and nothing while it is neither grouped nor
  sorted by words. A rank stays as big as before (64 bytes).
- Placing what changed no longer merges every rank (`Listing::place`):
  the comparisons grow with what changed, and the order is moved once,
  in blocks. Two costs in memory, chosen by the user (2026-11-11): what
  moved keeps what it was placed by until it is placed again (24 bytes
  an entry waiting: some 24 KB a batch once a folder is read, up to
  600 KB while 100 000 entries stream in), and each sorted listing keeps
  the order before last to write the next over (24 bytes an entry shown:
  2.4 MB at 100 000). A folder of 100 000 entries hydrated by size in 98
  batches takes about 110 ms and one entry grown 0.4 ms, against about
  400 ms and 1.1 ms before groups (620 ms with groups, merged as before).
- Keys are a plugin's closures run per entry on a worker (and for what
  arrives, on the window's thread): cheap by contract, as a lens is.
- By Folder, the sort asks the location where each entry is, as a column
  does, on the worker that sorts: `path_of` answers from memory by
  contract. A folder's group is called by its path from the home folder
  (`~/notes`) and has no term: a filter tests a file by itself, not where
  it is, so its heading's menu offers Select these alone.

## Amended 2026-11-12: groups told from their counts, costly keys on every core

Grouping `/nix/store` (385 113 entries) was very slow (the user's
report); three of its four causes were here, the fourth ADR-121's (an
order that reads waits for its reading, amended the same day).

- **A group's span is told from how many it holds** (`Runs`, kept with
  the order's ranks): made with the order, on the worker that sorts
  (`ranks_of`), then one more or one fewer for each entry placed or
  taken out (`Listing::place`). Every change in a grouped order told
  its groups by a pass over the whole order: 3.7 ms for one file of a
  folder of 385 000 entries, 1.2 s for 377 batches, and 33 ms of the
  window's thread (debug build) to take an order made on a worker. Now
  as many steps as groups (0.3 ms for the 377 batches, 0.1 ms to take
  an order).
- **Keys that read are asked on every core** in a sort made on a worker
  (`Keys::costly`, set by the query plugin for Kind and Type, whose
  answer is the type read from the name in the type database: about a
  microsecond a name). Only the answers are made there, which entries
  share (`answered_of`, scoped threads); each entry's key is made by the
  sort's own thread, so what the order keeps is allocated by a thread
  that lives on. Keys that answer from groups they share in nanoseconds
  (a date's, an extension's) are asked on one: twenty threads asking
  them at once made a sort by date four times as long. On the window's
  thread, never: keys answer there from memory and leave what reads to
  a worker. Grouping `/nix/store` by Kind took 1.3 s in the release
  build, about 0.3 s now (17 s and 1.9 s in the debug build); 16 bytes
  an entry while the sort runs.
- **A name's type is read by the suffixes a pattern can be** (the
  vendored xdg-mime's patch 0004): looking up every suffix of a name
  cost the square of its length (3.2 µs for a store name's, against
  0.9 µs for a short one); now only those starting with a byte a
  pattern starts with and no longer than the longest, both read from
  the database.
