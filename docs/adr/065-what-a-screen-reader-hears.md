# ADR-065: What a screen reader hears: AccessKit through gpui, the files as list boxes

Status: accepted, 2026-10-18. Closes ADR-001's known gap in part: the
window and its file views are exposed; the other places follow
(`docs/invariants/accessibility.md` lists what is).

## Context

- gpui from Zed's main branch (ADR-064) reports to AccessKit each element
  that has an id and a role, every frame while assistive technology
  listens (`Window::is_a11y_active`), and nothing otherwise. AccessKit
  hands the tree to AT-SPI on Linux, UI Automation on Windows and the
  accessibility API on macOS.
- Before this, a screen reader heard "skwir, frame" and nothing more:
  Skwir's elements had no roles, and gpui's plain strings (`&str`,
  `String`, `SharedString` children) are not in the tree at all.
- The keyboard focus of a pane is on its container: the pane's key
  context, type-ahead and menu live there, and the cursor is the
  workspace's, moved by commands. gpui offers the pattern for that:
  an element under the focused one claims to be the focus
  (`aria_active_descendant`), and a screen reader says it as focused.
- Read on Linux with AT-SPI from a client (`tools/test/a11y.py`, as Orca
  reads it), AccessKit 0.24:
  - a position in a set counts from 0, on the item, and the set's size
    is read from the container (`size_of_set_from_container`), on every
    system's adapter. gpui's example sets both, 1-based, on the items, as
    the web does: there the size never reaches AT-SPI.
  - The Linux adapter passes neither a tree item's level nor whether it
    is expanded (Windows' passes both).
  - libatspi refuses the signature of AccessKit's `RemoveAccessible`
    signal ("Unknown signature so"): a client keeps a removed node until
    it reads its parent's children again.

## Decision

1. **Skwir speaks to assistive technology through gpui's AccessKit**:
   roles, names and states on its own elements. What only a screen reader
   reads (a name, a description, a count) is built only while one
   listens (`Shell::a11y`, read once a frame); a role, a constant, is
   always set.
2. **The window is named by where it is**: "‹location› — skwir", the
   active pane's location, as file managers title their windows. A
   screen reader says it when the window comes forward; the system's
   list of windows shows it too.
3. **The keyboard focus stays where the keys go; the cursor's item is
   the active descendant.** A pane's container keeps the focus and says
   what the pane is (`PaneView::role`), named by its location; the entry
   under the cursor says itself as focused: its name, whether it is
   selected, its place among the entries ("readme.txt, 3 of 120").
4. **The files are list boxes**: the list and the grid a list box of
   options, each column of the columns view and of the board one, the
   tree view a tree of items. An entry says what `entry::behave` gives
   every view (`Seat`: its index, its depth in the tree), so a view
   cannot say an entry its own way.
5. **What is exposed is checked as a reader hears it**: `just
   a11y-check` starts the window with an AT-SPI registry of its own,
   presses keys and reads the focus back. Windows and macOS are compiled
   only: their adapters are AccessKit's, unchanged.

## Alternatives

- **A focus handle per row**, focus moving with the cursor: the pane's
  key context and type-ahead would move with it, and a row scrolled away
  would take the focus with it. gpui's active descendant is made for
  this.
- **The list view as a table of cells**: the keys move by entry, not by
  cell; a table would have a screen reader walk the cells of each row.
  The details (size, type, date) are not said yet: what an entry says
  beyond its name is decided with someone who uses a screen reader
  daily.
- **Waiting for AccessKit's Linux adapter to pass levels**: the tree
  view is exposed now; its items say their names, Windows hears their
  depth, and Linux will when the adapter passes it.

## Consequences

- Every focus place needs an id and a role, or gpui logs that the focused
  element has no node and a screen reader says the window instead
  (`docs/invariants/accessibility.md`). The places not exposed yet: the
  sidebar, the address field, the palette and the other popups, menus,
  dialogs, the previews and the terminal.
- A new view returns its container's role and passes each entry's `Seat`;
  a new place extends `just a11y-check`.
- No cost while no screen reader listens: gpui builds no tree, and
  Skwir builds none of the strings. While one listens, every frame
  rebuilds the nodes of what is on screen (the rows drawn, not the
  listing).
