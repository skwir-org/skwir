# View invariants

Read this before changing a view (`crates/ui/src/views/`) or how entries
are drawn, selected and scrolled. Binding, like `AGENTS.md`. The window
around the views is in [shell.md](shell.md).

## One look per entry

- An entry looks the same in every view (`views/entry.rs`). What a view
  draws of it is read once under the lock (`Shell::look`): picture
  (`Look::picture`), highlight, name and tag chips. What it answers
  (press, click, drag of the selection, cut dimming, a dropped tag or
  files) is `behave`, with what a screen reader hears of it (its name,
  selection and `Seat`; [accessibility.md](accessibility.md)).
- A row, a tile or a card draws its own shape around these, never its
  own copy of them. The columns view's parent folders and the folder
  under its cursor are not the pane's entries: they take only the
  picture (`icons::entry_picture`), the cut dimming and an option's
  name and place in its column's list box.
- Every view marks a link, and a tag's place, with `icons::Mark`.
- A thumbnail is drawn by `backdrop::Picture::img`, sized and fitted by
  the view. One that lets its background through, whose ink (its
  luminance, weighted by opacity, read once when converted) would not
  read on `bg` (under WCAG's 3:1 for graphics) but reads on
  `picture_backdrop`, is drawn on that (`backdrop::behind`): a drawing's
  dark lines on the dark look. A colored icon reads as it is, and an
  opaque picture has no backdrop to fringe its edges. The preview's
  pictures follow the same rule (previews.md). The map frames its photos
  in its paper.

## Selection

- An arrow or a click selects the item reached, and the selection then
  follows the cursor. Toggles, ranges and select all *pick*: the selection
  stays put.
- Scheme moves say `follow` (`ui.cursor`, `step: follow`): they move a
  following selection and leave a picked one. `pick` is yazi's mark. A
  sweep (`select_sweep`) holds a range while the cursor moves (`sweep` in
  the key context). The standard keys use none of these.

## What waits for a paste

- The clipboard is the system's (ADR-062): Copy and Cut hand their files
  to the window (`Outcome::Files`), which puts them there as Linux file
  managers do (both families: GNOME's list, MATE's, the URI list and
  KDE's cut flag, with the paths as text). Skwir's own clipboard keeps
  them by identity while they are there; once anything else is copied,
  here or in another application, the window forgets them
  (`on_clipboard_files_gone`, `clipboard_clear`), and once Skwir's own is
  empty (a cut all pasted) the window empties the system's of them.
- A paste with nothing in Skwir's own answers `WantsClipboard`; the window
  reads the system's off the UI thread, reads each file from the disk on a
  worker, and runs the paste again with them as `clipboard`, into the pane
  it was asked in. Only `file://` URIs are files (ADR-021); text is never
  taken for paths. An agent's paste never reads the user's clipboard.
- The clipboard's owner publishes it in `App::pending` (each entry by
  file and name, and whether pasting moves it); the shell snapshots it
  once per frame.
- Every view dims cut entries (`views::list::CUT_OPACITY`), and the last
  pane's footer shows the count, since the clipboard is invisible once the
  pane moves on. A click on the count forgets it (`clipboard_clear`).
  Escape does not: a cut waits through it, to be pasted where it leads
  (out of a search, [shell.md](shell.md)).

## Folders opened in place (ADR-025)

- The tree view opens folders under the one shown (`Pane::expanded`, the
  core `expand` command); their entries are selected and acted on like the
  listing's. What is opened is a location at the entry that leads there
  (`workspace::Opened`), so it works with places (a tag's) too.
- Read a pane's entries through `Pane::entry`, `Pane::path_of`,
  `Pane::location_of` and `Pane::order`, never `pane.listing` alone.
- Only a view that says `opens_in_place` keeps them open; the shell
  closes them under any other.
- The columns view's columns are a folder's parents or the places of a
  location's trail (`PaneView::kept_locations` is given the trail).
  Wider than the pane, they scroll sideways, as Finder's do, never cut,
  and follow their right end while they are at it (`columns::follow_end`):
  a folder opened lands there, and the folder shown stays in view as the
  pane narrows. Scrolled away, they stay where they were put.
- After the pane's column comes the folder under the cursor (yazi's
  preview of it, Finder's next column), always there, empty on a file. It
  is listed once the cursor has rested on it (`columns::SETTLE`), so a
  key held down lists none of the folders it passes, and it is a kept
  location: going into it adopts its listing. The folder just left going
  up keeps its listing for it, never listed again.
  - Right goes into it and never opens a file (Enter does); arriving from
    it (Right, Open, a double click), the first entry is selected once
    listed, unless something was meanwhile (`ColumnsView::land`). Left is
    `up`, the folder left selected.
  - A click on a folder selects it, and its column follows. A press in
    that column goes into it, the row pressed selected there (`navigate`
    with `select`); a press in a column above goes where the row leads.
- A hydration is remembered by the listing that holds the entry
  (`Pane::listing_of`), so an entry moved between opened folders is asked
  for again.
- **The board** (`views/board.rs`) opens every folder or place of the one
  shown, at most `MAX_COLUMNS`, as columns of cards after a column for
  what the place holds itself. A card is an entry (`views::entry`); one
  dragged to another column is dropped on that folder or place
  (`Shell::drop_on_place`, told where it came from, so a tag's drop takes
  the old tag). An emptied column stays, to take cards back. Its keys move
  in the pane's order (`PaneView::nav_delta`), so Shift extends.

## Rubber band

- `elements/marquee.rs` sits over a view's scroll area: it reads its
  bounds in prepaint, converts the pointer to content coordinates with the
  scroll offset, and asks pure, tested geometry (a view's `Geometry`)
  which entries a rectangle covers.
- Its release listener is registered every frame: a quick click releases
  before the repaint that would register it.

## Grid

- A tile is what the pointer can hit: the picture standing on the bottom
  of its box, the name right under it, up to three lines.
- An icon fills its box like a square thumbnail (`GridView::icon`). A
  thumbnail is given its fitted size (`grid::fitted`), never `max_w` and
  `max_h` alone: gpui keeps the aspect ratio over `max_h`, and a portrait
  overlapped the name above.
- Tiles sit in slots (`elements/uniform_grid.rs::math::Slots`): tile plus
  least gutter; spare width widens gutters up to a cap, the rest becomes
  margins. The grid and the rubber band use the same `Slots`, so a press in
  a gutter or under a short name starts a band.
- Columns are uniform; a row is as tall as its longest name needs. A tile
  keeps its own height in a taller row, hanging from the top, so its
  highlight and hit area hug it.
- `GridView::measure` counts each name's lines with the text system's
  line wrapper when names (`Listing::names_version`, untouched by
  hydration), order or zoom change; never per frame or metadata batch. The
  label is clamped to that count, so a disagreement costs an ellipsis, not
  an overlap.
- Nothing that would change a tile's height goes on it: tags are dots on
  the picture's corner (`tags::tile_dots`), named in its tooltip.
- Fill thumbnails (`ui.grid_fill`) crops pictures to the box.

## Names and zoom

- A name too long keeps both ends (`text_ellipsis_middle`, vendored patch
  0005), in every view: the extension tells files apart.
- Ctrl+wheel zooms through a capture-phase listener
  (`elements::zoom_wheel`, `Zoomed::View`: grid, list, columns and Usage,
  each with its own size; previews.md for the panel), keeping the first
  visible item in place; zoom keys keep the cursor in place. Keys come
  from `keymap::ZOOM_KEYS`.
- A wheel listener asks `Hitbox::should_handle_scroll`, not `is_hovered`.
- An absolute overlay states its `top` and `left`, or it is laid out after
  its siblings, below the area it should cover.

## List columns

- One table (`views::list::COLUMNS`: setting, label, sort, width,
  alignment) feeds the header, rows, the header's menu, the View menu's
  sorts and Settings. A column that applies only somewhere says where
  (`Shell::column_applies`).
- Name and detail columns sit in one wrapping flex line one row tall
  (`views/list.rs::line`, shared by header and rows): columns drop whole,
  last first, and the name, the only child that grows, takes the room.
- Tag chips take what the name leaves, placed in prepaint
  (`elements/chips.rs`); those that do not fit drop whole behind "+N".
- Where files come from many folders (a search, a tag, the shelf,
  Recent), the Folder column comes first among details and goes last, its
  full path in the tooltip. Show in Folder (`show_in_folder`, Ctrl+Alt+O)
  goes there with the file selected.
- Columns are chosen in the header's menu (`Shell::open_column_menu`),
  through `settings.apply`, so it never disagrees with Settings.

## Scrolling

- Every scrolling view has a scrollbar (`elements/scrollbar.rs`) on its
  right edge, and on its bottom edge when it scrolls sideways
  (`scrollbar::horizontal`). The view hands it the handle it scrolls with;
  the thumb is placed in prepaint. Where an area scrolls both ways, each
  bar leaves the corner to the other while it shows (`Scrollbar::across`
  when the two drive different handles: a board's columns and its row).
- **The wheel scrolls along its own axis** (vendored patch 0022): alone,
  up and down; with Shift, which every platform reports sideways, across.
  Scrollers take the wheel together, from the innermost out, so one that
  lent the other axis's turn to its own would move with a turn meant for
  another: a board's column with Shift, its row of columns without it.
  Only a strip of one line that nothing scrolls around takes either
  (`restrict_scroll_to_axis = Some(false)`, the search's narrowing pills).
  An element with a wheel handler of its own moves each axis by its own
  delta (`elements/rows.rs`, `Shell::reader_wheel`): the platform has
  already turned Shift's turn sideways.
- A `gpui::list` measures only what it renders: give it a height estimate
  (`ListState::reset_with_uniform_height`), or the bar
  describes only the rows on screen.
- The gutter takes the pointer only while it holds a bar.
- Wrapping a scrolling element to overlay a bar must keep its flex
  context (a body that grew in a column needs a column), or it silently
  stops scrolling.

## Plugin views

- Plugin views register owner-tagged `ViewSpec::named` descriptors; their
  renderers implement `PaneView` in `skwir-ui`, resolved in
  `views::named_renderer`. Never hard-code one in the toolbar or palette.
- A view shows the pane's entries; what is no file is a place among them
  (ADR-032), never a selection of the view's own. A view never opens a
  location itself (`PaneView::kept_locations` asks the pane).
- Reconcile registration identity, including disable and re-enable
  between frames; fall back to List when the active view disappears.
- Publish data, advance the descriptor revision, then wake; never query
  plugin data per frame.
- A view says what F5 does (`PaneView::refresh`) and whether it draws its
  own empty state (`draws_empty`).

## Usage (ADR-007)

- Its toolbar holds the zoom's steps (`ui.zoom`, in the View menu's
  words: `usage::STEPS`) around the count of levels, which goes first
  when the pane is narrow (`elements::adaptive::AdaptiveRow`). The
  summary asks for no room: it takes what the tools leave.
- The cancellable scanner installs non-recursive watches as it visits
  each directory, before reading it. Never hand a recursive walk to a
  watcher API: it cannot see our cancellation.
- Cache only completed breakdowns; invalidate data gathered without
  reliable watch coverage. Scope invalidation and deadlines to the
  affected subscriptions: eviction is not a content change, and an
  unrelated event never rescans every pane.
- Keep queued/running state apart from visible progress, so a quick cache
  hit never flashes a busy indicator.
- Progressive snapshots are lower bounds, marked incomplete, kept out of
  the completed cache, bounded, with entry identity kept as blocks
  reorder.
- A finished scan's chart stays through refreshes and is replaced
  atomically; it may still be incomplete (read errors). Revalidation never
  brings back unknown blocks or busy flashes.
