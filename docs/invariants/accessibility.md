# Accessibility invariants

Read this before adding a place that takes the keyboard's focus, a
control, or anything a screen reader should hear (ADR-065). Binding,
like `AGENTS.md`.

## How the tree is made

- gpui reports an element to AccessKit when it has an id and a role
  (`.id(…).role(…)`), every frame while assistive technology listens
  (`Window::is_a11y_active`), and nothing otherwise. Plain strings
  (`&str`, `String`, `SharedString` children) are not in the tree: a
  control is named by `aria_label`, never by the text it draws.
- **Built only while a reader listens.** What only assistive technology
  reads (a name, a description, a count) is built under `shell.a11y`
  (read once a frame in `Shell::render`). A role, a constant, is set
  always. Never compute a string for it otherwise: a frame without a
  screen reader costs what it did.
- Nodes are the same node across frames when their global element ids
  are: an id that changes says a node went and another came.

## Focus

- **Every element that tracks a focus handle has an id and a role.**
  Without them gpui logs "focused element … has no accessibility node",
  and a screen reader says the window instead of the place.
- **The focus stays where the keys go.** A list keeps the keyboard focus
  on its container; the item under the cursor claims
  `aria_active_descendant`, and a screen reader says it as the focused
  one. The claim holds only inside the focused element: set it on the
  cursor's item whatever has the focus.
- The window is named by the active pane's location ("‹location› —
  skwir", `Shell::sync_title`).

## Lists

- A list is a `ListBox` of `ListBoxOption`s, a tree a `Tree` of
  `TreeItem`s.
- **Position and count follow AccessKit, not the web**: an item's
  `aria_position_in_set` counts from 0, and the count is
  `aria_size_of_set` on the container. gpui's example puts both on the
  items, 1-based, as ARIA does; set that way, no reader hears the count.
- A tree item says its level (`aria_level`, from 1) and, for a folder,
  whether it is opened (`aria_expanded`), not its position: a reader
  counts siblings by level.
- **An entry says what `entry::behave` gives it** (`views/entry.rs`): its
  name, whether it is selected, its `Seat` (index, depth in a tree), and
  on the cursor the active descendant. A view passes the seat; it never
  names or states an entry itself. A view's container takes its role
  from `PaneView::role` (a list box, a tree, or a group of list boxes:
  the columns, the board's), named by the location, with the count; a
  pane that lists nothing says its placeholder as the description.

## What is exposed

| Place | Role | Name | Since |
| --- | --- | --- | --- |
| window | frame | "‹location› — skwir" | ADR-065 |
| list and grid | list box of options | the location | ADR-065 |
| tree | tree of items | the location | ADR-065 |
| columns, board | group of list boxes | the location; each column its folder | ADR-065 |
| an empty or failed pane | group, described by its placeholder | the location | ADR-065 |

Not yet: the sidebar, the address field, the palette and the other
popups, menus, dialogs and settings, toolbars and tabs, toasts and the
footer, the previews, the terminal.

## Checking

- `just a11y-check` (`tools/test/a11y.py`) starts the window on a display
  and a session bus of its own, with the AT-SPI registry, raises the flag
  Orca raises, presses keys and reads the focus back over AT-SPI. A place
  newly exposed adds its step there.
- By hand: `just a11y-check --keep` leaves the window, its display and
  its session bus running, and prints the bus's address for a client of
  one's own; never read the user's session.
- Windows and macOS are compiled only (`just check-cross`): their
  adapters are AccessKit's.

## Known gaps (AccessKit 0.24, Linux)

- The adapter passes neither a tree item's level nor whether it is
  expanded: on Linux the tree view's items say their names only.
- libatspi refuses the signature of AccessKit's `RemoveAccessible`
  ("dbind-WARNING … Unknown signature so"): a client keeps a removed
  node until it reads its parent's children again.
