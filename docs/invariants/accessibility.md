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
  (read once a frame in `Shell::render`; `SettingsView::a11y` in the
  settings, `Window::is_a11y_active` in a view of its own). A role, a
  state or a string already in hand (a `&'static str`, a `SharedString`
  the control draws) is set always. Never compute a string for it
  otherwise: a frame without a screen reader costs what it did.
- Nodes are the same node across frames when their global element ids
  are: an id that changes says a node went and another came.
- **The tree reads as the screen does.** Where the layout gives children
  in another order than they read (the trail, laid out from the right so
  that its first folders wrap out of sight), the container puts its
  node's children back in reading order (`a11y_synthetic_children`, after
  they are laid out), never the layout's.

## Focus

- **Every element that tracks a focus handle has an id and a role.**
  Without them gpui logs "focused element … has no accessibility node",
  and a screen reader says the window instead of the place.
- **The focus stays where the keys go.** A list keeps the keyboard focus
  on its container; the item under the cursor claims
  `aria_active_descendant`, and a screen reader says it as the focused
  one. The claim holds only inside the focused element: set it on the
  cursor's item whatever has the focus.
- **A picker's field keeps the keyboard; its highlighted option is said
  as focused** (`aria_active_descendant_for`, patch 0030): the palette,
  Jump, Choose and the tag editor build their lists with `modal::options`
  and `modal::option`. A field typed in and a list beside it are the
  web's combobox: a reader hears the option the keys reach, and echoes
  what is typed.
- **Every part takes the keyboard** without the pointer: F6 goes round
  the sidebar, the files and the preview (`ui.focus_next_part`,
  shell.md), F4 in and out of the terminal, Tab along the toolbars.
- The window is named by the active pane's location ("‹location› —
  skwir", `Shell::sync_title`).

## Controls and dialogs

- **The shared controls say what they are** (`controls.rs`): a switch and
  whether it is on, radio buttons in a group, a button by its words, an
  icon button by the name it is given, a dropdown with its value. One that
  draws no words of its name (a switch, a choice, a dropdown) is named by
  its caller from the label beside it: a setting's control by the
  setting's title, described by its description (`SettingsView::named`).
- **A toolbar button** (`toolbar::Button`) is a button, a menu item in a
  popup, named by the words it shows, else by its title, which otherwise
  describes it; it says its keys, that it is dimmed, that what it stands
  for is on (pressed in a toolbar, selected in a menu), and a dropdown
  whether its menu is open.
- **A text field** (`input.rs`) is named by `TextInput::set_label` where
  its placeholder is a hint rather than a name; its text is one run of
  graphemes (the units the caret moves by) with the caret and selection
  in them. A secret is a password field and says bullets.
- **A dialog** is `modal::panel`: a modal dialog with an id of its own,
  named by its caller; a question asked before acting is an alert dialog.
- **What is announced** is a live region (`aria_live`, patch 0031): a
  toast, polite, or assertive for a failure, named by its message. Its
  buttons are `Live::Off`, or they are announced with it. A running
  plan's toast is a progress indicator, never live: it would speak at
  every step. A status that changes with every key (the footer) is a
  status line read when asked, never live.

## Previews and the terminal

- **What is previewed is named once** (`preview::said`: the file's name
  and the facts the header shows): the panel's landmark, Peek's dialog,
  the whole screen's, the picture, the player, a document and the
  inspector take it from there. Peek is named by the file it shows, so a
  reader says the next one as the arrows move to it.
- A document in the reader says its page (`reader::said`) and carries
  the page's text, a run a line as its engine broke them
  (`reader::expose`); the reader's view is the document, the picture
  place around it says nothing more.
- A label's words are its value (`aria_value`), as gpui's text gives
  them; a name set on a label is not said.
- **A terminal is read as text**: each row of the screen a text run, the
  blank cells between its runs as spaces, the cursor as the caret, built
  in the panel's `a11y_synthetic_children` from the screen it draws
  (`terminal::read_out`).

## Lists

- A list is a `ListBox` of `ListBoxOption`s, a tree a `Tree` of
  `TreeItem`s. The pane's lists are multiselectable.
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

| Place | Role | Name |
| --- | --- | --- |
| window | frame | "‹location› — skwir" |
| list and grid | list box of options | the location |
| tree | tree of items | the location |
| columns, board | group of list boxes | the location; each column its folder |
| an empty or failed pane | group, described by its placeholder | the location |
| sidebar | tree: groups, then their places (selected: the one shown) | "Sidebar"; a place described by its space or state |
| address field, prompts, settings fields | entry (password text for a secret) | its label, else its placeholder |
| the trail | navigation of links in reading order, the last selected; buttons for its menus | "Path"; each place; a menu by its tooltip |
| palette, Jump, Choose, tag editor | dialog: field, list box beside it | the dialog's; "Commands", "Places", "Choices", "Tags" |
| context menu, toolbar popups, history menus | menu of menu items | "Context menu"; each item its title |
| dialogs, confirmations | dialog, alert dialog | its title or question |
| settings | dialog; its pages a tab list; its controls | "Settings"; each control its setting's title |
| toolbars, tabs | buttons; a tab list of tabs | their titles; each tab its location |
| toasts | status (alert for a failure), live; progress indicator | the message; what runs |
| preview panel | landmark (complementary) | "Preview", described by the file and its facts |
| Peek, the whole screen | modal dialog | the file; its facts and keys |
| a picture, a film | image (its size), video | the file |
| a document in the reader | document of the page's text, a run a line | the file; the page shown |
| a map (a track, places) | figure | "Map: ‹file›"; what it holds and its keys |
| the Map view | figure; its status a line | "Map"; what is placed |
| the hex view | document of lines | the file; each line its offset, bytes and characters |
| text, Markdown, structured previews | document of its lines | the file |
| an archive's members, a message's parts, a table's rows | tree, list box; the row the arrows reach said as focused | the file; each row its first cell, the others after their columns' names ("Size: 6 B") |
| the media player | group; its seek bar a slider of seconds | "Player: ‹file›"; "Position", "0:02 of 0:03" |
| terminal | terminal: its screen as text, the cursor as the caret | "Terminal"; the program's title |
| footer | status line, never announced | what it shows: the count, the selection |
| search chips | buttons; a tag's category a link | their titles |

Not yet: a map's places one by one (a picture placed, a track's
points); a scanned page's text (no engine reads it).

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
