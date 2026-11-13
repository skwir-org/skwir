# ADR-135: What a plan brings is drawn where it will land

Status: accepted, 2026-11-13, on the user's answers (asked on
2026-11-03 with mock-ups, set aside then for the strip alone, ADR-114
amended; asked again on 2026-11-13: in place, a tint behind the row, a
press opening the action's card in Activity). Built on Linux.

## Context

- **A copy shows nothing where it goes until it is whole.** It is made
  in a staging folder in its destination and renamed into place once
  whole (`docs/invariants/operations.md`, "Staging"): a folder pasted
  shows nothing there until all of it is copied, and one waiting its
  turn, nothing either. The strip says that it runs (ADR-114), not
  where.
- **The staging folder is no cue.** It names what it holds apart from
  where each lands, holds nothing while its copy waits, and says how
  much is written, not how much is left; on a remote, each look inside
  it is a request. Other programs' copies make none.
- **The executor knows it all already**: each plan's items, where each
  lands under the name it was settled to (`Items::placed`), its steps,
  and its meter (ADR-113).
- **A view's rows were its positions** until grouping made headings
  rows that are no position (ADR-130, 9; `views::groups::RowMap`).

## Decision

1. **The executor says what plans bring into a folder**
   (`op::arriving`, `Executor::arriving_in`).
   - **What is brought:** each item that puts something there, by the
     folder it lands in and its name there, in the order asked: a copy,
     a move to another folder, what a step makes (an archive extracted),
     a file put back from the trash. Never a link or a rename, which are
     there at once.
   - **Kept** from the plan asked until its run ends. Its items as
     asked first, read on a worker in the lane of what its first step
     reads; from its run's start, the plan as it runs, its steps
     unfolded and its names settled.
   - **What each will be** (`Arrival::like`): what it is made from,
     stat-ed once, under its name there, with its kind, size and times
     (a copy keeps them); what a step makes, by what the step says,
     dated now.
   - **How far each has come**, read from its plan's meter when asked:
     waiting, starting, a share, paused. The share is the bytes of its
     step, or its steps ended where it has several; whole once a batch
     finished it ahead of its turn (ADR-078).
   - A plan cancelled brings nothing from then on. Nothing is read from
     the destination, nothing is kept beyond the plan's run, and there
     is no event or wake of its own.
2. **It stands where its file will land** (`Pane::stands`,
   `Listing::standing`).
   - **Placed** among the listing's order shown by the pane's sort, as
     an entry would be, searched among the order's ranks; among the
     others as the order would have them; in the group it would be in,
     where the pane groups.
   - **Not shown** where the pane would not show it: a hidden name while
     hidden files are not shown, or a filter that does not keep it or
     cannot tell yet (a term the copy's source would only answer once
     read).
   - **An item whose name the folder lists already** is that entry,
     drawn with how far it has come: a folder merged into, a file a
     camera writes in place, one placed whose step has not ended.
3. **What arrives is a row, never an entry** (`RowMap`, `Row::Arriving`,
   `Tile::Arriving`).
   - **Never a position:** the rows that are no position are the
     headings and what arrives. The cursor, ranges, Select All, the
     rubber band and a screen reader's count work in positions, as with
     headings, and the keys go past it.
   - **Placed again** only when the order, its groups or what arrives
     change; otherwise each sync takes how far each has come.
   - **Shown** once its action lasted 0.3 s (`work::SHOWN_AFTER`, the
     strip's), by one timer.
4. **It is drawn as its file will be, dimmed** (`views::arriving`), in
   the list, the tree and the grid.
   - **Its look:** its picture (its icon: nothing is there yet to make a
     thumbnail of), name and details at 60 %. A tint behind it from its
     start to how far it has come (`highlight`, the user's choice over a
     reveal or a bar). A pill after its name, at a tile's top right:
     "42 %" on the accent; "waiting", "starting", "paused".
   - **Pointed at**, it says where it comes from, how it goes and how
     long its action has left.
   - **Pressed**, it opens its action's card in Activity (`ui.activity`
     with `plan`: its Pause, or its Cancel while it waits). It is never
     selected.
   - **A screen reader** hears it as an option that cannot be chosen,
     with how far it has come.

## Consequences

- **Memory.** Each item is kept while its plan runs: its name, its
  source's path and the fields a view reads of it, about 1.35 KB in all
  (measured, 100,000 items: 132 MB), mostly the fields' map, one node of
  it whatever their number. Ten thousand files pasted keep 13 MB while
  they come. A compact form rebuilt as an entry where it is placed would
  keep a few hundred bytes for more work at each placing: not chosen.
- **Placing** costs a search among the order's ranks for each item, made
  again when the order changes (each landing changes it): 11.2 ms for
  10,000 items among 100,000 entries, on the window's thread
  (`arriving_stands_us`, release). The rows that are no entry are
  searched, never walked.
- **Limits kept for later** (the user's list): the columns and the board
  draw no arrivals; in a grouped view, an item whose group holds no file
  yet stands after every group; sorted by Created or Accessed, a file
  may move as it lands; what another program writes (a download, a
  `cp`) is not marked.
- **Other limits:** only the pane's own folder shows what arrives, not
  one opened in place; undos and redos are not shown; a server's batch
  says a file only once it is whole, so it fills at once.

## Amended 2026-11-13: the follow-ups and the limits

On the user's answers the same day (a group no file is in yet its own
dimmed heading; a batch's items copying, rsync telling each; a "writing"
pill on what another program writes; the full tint stopping short; a
title naming the folder), the follow-ups and the limits found are built:

- **What arrives is read as an entry would be**
  (`Executor::set_reader`, the App's `fs::hydrate_path`): stat-ed, then
  its notes (tags, rating, comment) read by the metadata providers, so a
  filter, a sort and groups by them place it. A copy, and a move to
  another volume, is a file made anew: created and last read when asked
  (`arriving::moved_to`); a move within its volume keeps its file's dates.
- **Undos and redos bring what they put back** (a move undone, a file
  put back from the Trash): noted as their run starts, let go of as it
  ends (`Brought`); nothing while one waits its turn.
- **Folders opened in place** (the tree, the board's columns): each is a
  folder the executor answers for (`Pane::opened_listings`,
  `Pane::stands_in`); what arrives in one is placed among its entries,
  one level deeper, in its folder's group (`ListState::sync_arriving`).
- **The board** draws what arrives as cards in the column of the folder
  it lands in, or of its group, among the cards and never one of them
  (each column its `RowMap`); **the columns view** in the folder shown's
  column, and in each column beside it, placed in that folder's own
  order (`columns::side_arriving`).
- **A group no file is in yet** has a heading of its own where that group
  stands among the pane's (`Pane::fresh_before`, `groups::Fresh`,
  `Row::Fresh`, `Drawn::Fresh` in a grid's lines): dimmed, "arriving"
  where a count is, no stop of the keys, nothing to fold or select; never
  one of the pane's groups, whose numbers stay the core's. A grouped board
  draws no column for it.
- **A batch's items say they copy** (ADR-078): the meter keeps the steps
  a batch under way took (`Meter::batching`), each "copying" at the
  batch's share where its transfer says nothing of each until whole
  (`Arrives::Copying`), by its own bytes where it does: a transfer tells
  them (`Told::writing`); rsync's from what it says, its order the names'
  bytes, a folder's line as it begins it, each file once whole, the bytes
  between counted to the name it is in (`rsync::Writing`).
- **What another program writes** (a download, a copy in a terminal) is
  marked as it comes: the watcher tells which names it tells are still
  written (`Changes::writing`: written to and not closed since on Linux,
  written in the last `QUIET`, 2 s, elsewhere, then told again), the
  folder's provider marks the entry (`keys::WRITING`), and the views draw
  a "writing" pill beside its size, never dimmed; no name is taken for a
  download's by its suffix.
- **The tint stops short** of a row's or a tile's end (`arriving::SHORT`,
  12 px), so one whole never reads as chosen.
- **A transfer's title names its folder** (`op::transfer_title`,
  `op::folder_said`: "copy 3 items to Projects", "copy “a.txt” to
  Projects"), its whole path on the card's tooltip while it runs or waits
  (`Executor::brings_into`): titles are drawn on one line, by a card, a
  toast and the History.

Left: a grouped board shows no arrival in a group no file is in yet;
an undo waiting its turn shows nothing; sftp, GVfs, KIO and rclone say
nothing of a file until it is whole (their items say "copying" at the
batch's share; OpenSSH's `sftp`, run with its lines on its input, says
nothing until a session's files end, its meter drawn on a terminal
alone); rsync's order taken as its names' bytes, an item it
copies out of that order shows the bytes in flight on the next one until
its line comes; macOS and Windows tell a file being written by its
writes, not its close. Two folders of one name are told apart by the
card's tooltip, not its title.
