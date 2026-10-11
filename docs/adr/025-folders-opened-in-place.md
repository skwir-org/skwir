# ADR-025: Folders opened in place

Status: accepted, 2026-09-29. Built. Revised by ADR-032: what opens in
place is a location at the entry leading there (`workspace::Opened`),
so tag places open in place too.

## Context

A tree view shows a folder's contents indented under it. Those rows
must be real: selectable (click, range, select all), copied, renamed,
dragged, previewed. A pane's entries were exactly its listing, and
selection, ranges and commands resolved ids against it alone.

Rejected: a view drawing nested rows from kept listings by itself.
Commands would get a selection they cannot resolve, and ranges across
levels would follow another order.

## Decision

- **Opened folders are core pane data** (`Pane::expanded`), like the
  sort, each after its parent. The core `expand` command opens, closes
  or toggles them by entry id (default: the selection), or closes all;
  the tree's triangle and Left/Right run it, as can scripts and agents.
  The tree is a `ViewRole::Alternate` (ADR-007). Navigation closes
  them; dormancy keeps them.
- **Their listings are kept locations** (`keep_only`,
  `App::open_expanded`): listed on a worker, watched and pumped with the
  pane. Going into one adopts its listing.
- **Their entries are the pane's.** `Pane::entry` and `Pane::path_of`
  look in the shown listing, then the opened folders. `Pane::order`
  interleaves each opened folder's entries after it, sorted alike, and
  drives ranges, select all, invert, follow and sweep. The selection
  keeps what any of them holds; closing a folder drops its part.
  `App::selected_refs`, hydration, refresh and selection-reading plugins
  resolve through the pane; views key on `Pane::tree_version`.
- **Only a view that draws them keeps them**
  (`PaneView::opens_in_place`: the tree, later the board). Any other
  view closes them at the next sync, so nothing is acted on unseen.

## Consequences

- Code reading the selection or order goes through `Pane::entry`,
  `Pane::path_of` and `Pane::order`, never `pane.listing` alone.
- Commands about the folder itself (New Folder, paste into it,
  edit-as-buffer) mean the folder shown.
- A selection inside opened folders does not survive dormancy: it is
  restored against the shown listing first.
- Originally only folders under a folder shown opened (not in a search
  or the shelf), and links to folders did not, which kept the tree free
  of cycles. Since ADR-032 the tree works in any location that reads as
  a trail; since links show as what they lead to (2026-09-30), a link
  to a folder is a folder (`Entry::is_dir`) and opens.
