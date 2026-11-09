# ADR-127: A search is laid out as the tree of its folders in the Tree view

Status: accepted 2026-11-09 with the user, as the other half of ADR-126:
`Ctrl+Shift+F` keeps the tree and `Ctrl+F` can show one. Built on Linux;
Windows and macOS compiled.

## Context

A search lists what it finds flat, each row its own path. Over a large
tree a reader wants to see where the results are. ADR-126 gave a filter
its pruned tree; a search had none.

## Decision

1. **The Tree view is the switch** (`PaneView::lays_found_out_as_tree`,
   true for the list view's tree; `App::set_found_as_tree`). The List view
   stays flat; there is no setting of its own.
2. **The flat order stays the source.** A worker lays it out
   (`found_tree::make`, `FoundAsked`) from `OpenLocation::path_of`, rooted
   at `OpenLocation::found_under`: a location that gives no root (the
   default) stays flat. One job at a time; what arrived meanwhile starts
   the next, so a search streaming pays for what it can show.
3. **The folders on the way are lent to the pane** (ADR-116, `FOUND_TREE`):
   stat-ed once each (a cache kept with the tree), dimmed as a filter's
   way is (`Pane::is_way`, ADR-126).
4. **Order:** folders by name first, the direction honoured; files in the
   pane's sort.
5. **Folding is the tree's own** (`Pane::fold`, the `expand` command
   tried first): nothing is listed to open one, and a folder found with
   nothing below it has no triangle (`Pane::opens_here`).
6. **Ways are not what was found.** Select All skips them, and a range
   takes one only when it ends on it, so a Delete cannot take a whole
   folder unseen. The footer and the preview's header count what was found
   (`Pane::found_count`).

## Rejected

- **`Ctrl+F` always a tree.** A flat list is the better answer for few
  results, and every other view shows a search flat.
- **A setting beside the views.** The view shown already says how.

## Consequences

- Folders in the tree sort by name whatever column the pane sorts files by.
- Only the list view's tree lays a search out.
