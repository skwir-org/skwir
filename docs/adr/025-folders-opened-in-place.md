# ADR-025: Folders opened in place

Status: accepted, 2026-09-29.

## Context

A tree view (Dolphin's expandable folders, the Finder's list) shows a
folder's contents under it, indented, without leaving the folder shown.
The rows under an opened folder must be as real as the others: selected
by a click, a range or select all, copied, renamed, dragged, previewed.

Until now a pane's entries were exactly its listing. The selection kept
only ids that listing held (`Selection::retain_in`); ranges, select all
and the moves followed `Listing::sorted`; commands and plugins resolved a
selected id with the shown location's `path_of`. The columns view already
keeps other folders open beside the one shown (`PaneView::kept_locations`),
but only to draw them: nothing selects in an ancestor column.

A view could draw nested rows from kept listings on its own, but every
command would then see a selection it cannot resolve, and a range drawn
across levels would be computed over another order.

## Decision

**The folders opened in place are core data of the pane**
(`Pane::expanded`), like its sort: paths under the folder shown, each
after its parent. The core `expand` command opens, closes or toggles them
by entry id (the selected folders when none are given), or closes them
all; the tree view runs it from its triangle and its Left and Right
keys, and so can a script or an agent. The tree is a
`ViewRole::Alternate` (ADR-007): in the View menu, with a toolbar button
only while it is shown. Navigation closes them; dormancy keeps them and the
pane opens them again with its tab.

**Their listings are kept locations.** `keep_only` always wants them and
`App::open_expanded` opens the missing ones, so they are listed on a
worker, watched and pumped with the pane like the columns' ancestors, and
going into one adopts its listing.

**Their entries are the pane's.** `Pane::entry` and `Pane::path_of` look
in the shown listing and then in the opened folders; `Pane::order` is the
shown listing sorted with each opened folder's entries after it, sorted
the same way, cached on the identity of the orders it interleaves. The
selection keeps what any of them holds, and closing a folder lets go of
what it held. Ranges, select all, invert, follow and sweep use
`Pane::order`; `App::selected_refs`, hydration, refresh and the plugins
that read the selection (open-with, shelf, rename-bulk) resolve through the
pane. A view's order (`ListState::sync`) is `Pane::order`, keyed on
`Pane::tree_version` beside the listing's version.

**Only a view that draws them keeps them** (`PaneView::opens_in_place`,
the tree). Under any other view the shell closes them at its next sync,
so no entry is ever selected or acted on unseen.

## Consequences

A pane's entries can come from several listings; code that reads the
selection or the order goes through `Pane::entry`, `Pane::path_of` and
`Pane::order`, never `pane.listing` alone. Commands about the folder
itself (New Folder, paste into it, edit-as-buffer) still mean the folder
shown. Only folders under a folder shown open in place, not in a search
or the shelf. A selection inside opened folders does not survive the
tab going dormant: it is restored against the shown listing before the
opened folders are listed again. An opened folder's entries resolve to
its path joined with their names, as the path provider does; a symbolic
link to a folder is not a folder here and does not open, which also keeps
the tree free of cycles.
