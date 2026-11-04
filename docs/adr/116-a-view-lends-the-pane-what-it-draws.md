# ADR-116: A view lends the pane what it draws

Status: accepted 2026-11-04 with the user, who weighed it against opening
folders in place; built the same day, as `docs/PLAN.md` ("Usage selects
at any depth") records: checked on Linux in a window of its own;
Windows and macOS not built in this change (nothing in it is one
system's).

## Context

- **The user asked** that Usage select files in subfolders, as the Board
  does. Usage drew the entries inside the folder's own (as many levels as
  its zoom asks), but a click on one selected the folder of the shown
  one it lay in: only what the pane shows may be selected (Delete must
  never take what cannot be seen, `docs/invariants/views.md`), and the
  pane knew nothing of the blocks the map drew from its scan.
- **The Board opens folders in place** (ADR-025): its columns are
  listings kept beside the shown one, at most twelve, one level down, so
  its cards are the pane's entries. Usage doing the same was the first
  plan, and its costs were weighed with the user:
  - A click listed the whole folder to select one entry, and up to two
    above it: Usage is where the folders of 100,000 files are found
    (caches, `node_modules`, mail stores), each then held in memory,
    watched a second time and stat-ed whole wherever a sort or a filter
    reads sizes.
  - The selection waited for that listing: on a large or remote folder
    (a round trip per folder in its server's lane, ADR-077) the click
    showed nothing for a while.
  - The pane's order gained entries the map does not show as rows: Select
    all, Invert selection, a sweep, Quick filters' counts, the footer's
    listed weight and the name filter would each have acted on the
    opened folders' contents unseen, or needed a case of their own.
  - The scan and the listing could disagree (a file gone since), and an
    entry fewer levels no longer drew stayed selected.

## Decision

1. **A view may lend the pane entries it draws** beside what the pane
   lists (`Pane::lend`, each `Entry` with its path), in place of what it
   lent before. A lent entry is shown (`Pane::is_shown`), so it is
   selected, found by `Pane::entry` and `Pane::path_of`, and handed to
   commands by `App::selected_refs` as any entry: what acts on files acts
   on it unchanged, plans checking before they run.
2. **A lent entry is in no order.** `Pane::order` is the listing's and
   its folders opened in place: Select all, Invert selection, ranges,
   sweeps, the filter and every count stay the folder's own. A range to a
   lent entry adds it.
3. **What is no longer lent leaves the selection**, by the rule that
   lets go of what the pane does not show: the view drew it otherwise
   (fewer levels, a rescan that folded it into the rest, hidden files
   hidden), it went (a reading found it gone), the view changed, or the
   pane showed another location or slept. The pane lets go of everything
   lent on navigation and dormancy; the view lends again when the
   version its lending said moves (`Pane::lent_version`). What is lent is
   what the view's levels draw, not what has room on the screen: a block
   too small to open hides what is inside it as a list scrolled away
   does, and resizing the window never changes the selection.
4. **A lent entry is read as a listing reads its own, by its path**,
   when it is selected (`hydrate`, `fs::read_paths`, a job per lane):
   owner, permissions, times, a link's target, tags. An entry lent again
   at the same path keeps what was read of it until it is read again,
   once selected: what drew it again may have seen it change. Until read it is what the
   view knew: Usage lends each with its kind from its scan's stat, so a
   link or a pipe is never taken for a file to read, and lends nothing
   its scan could not type.
5. **Usage lends what it draws inside the folder's own entries**, at
   every level drawn. A click selects the block under the pointer at any
   depth, an open folder by its header; Ctrl adds or takes out; a
   double click on one inside goes there, as before. The arrows move
   among the blocks beside the cursor's in its folder, out to that
   folder's neighbours at its edge. The footer weighs a selection at any
   depth, what lies in a folder selected counted with it once.

## Rejected

- **Opening folders in place to select in them** (the Board's way): the
  costs above, and an order that is not what the map shows.
- **Each command leaving out the opened folders' entries** (Select all,
  Invert, the counts): one case per command, which the next command
  forgets.
- **A selection of paths beside the selection of entries**: every
  command would read two selections, and what the pane shows would no
  longer be what is selected.

## Consequences

- A lent entry has no listing: no watcher of the pane's sees it. What
  changes on disk reaches it through the view (Usage's scan watches each
  folder it counted, and draws again) or through a reading.
- Moving the cursor off a lent entry that went (trashed, deleted) gives
  it to no neighbour: the order that says what comes after it is the
  listing's.
- A lent link has no target until read, and so no preview for the
  moment its reading takes.
- Switching to a view that draws less (List, from Usage) lets go of what
  was selected inside folders: that view does not show it.
- Another view that draws entries no listing holds may lend them the
  same way, without opening a location of its own.
