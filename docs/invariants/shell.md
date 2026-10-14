# Shell invariants: the window

Read this before changing the window in `crates/ui`: toolbars, popups,
the top rows, menus, drops, feedback, the sidebar, keys, tabs, the
terminal panel, the player. Binding, like `AGENTS.md`. Views are in
[views.md](views.md); what locations decide, in
[locations.md](locations.md).

## Toolbars and buttons

- Reuse the command-backed buttons and bundled symbolic SVGs of
  `toolbar.rs`. Tooltips, focus styles and keyboard activation belong to
  the shared button. Keep a focused button's handle when it is disabled.
- Responsive rows pick a variant by intrinsic measurement in prepaint
  (`elements/adaptive.rs`, `AdaptiveRow`); no pixel breakpoints.
- Text cut to fit (`truncate`, `text_ellipsis_middle`, a clamp) is drawn
  through `elements/clip.rs`: while cut, its tooltip shows it whole,
  before its container's tooltip (`clipped_with`). Whether it was cut is
  read from layout when the tooltip is asked for, never in `render`.
- Pane actions capture their target pane while rendering and use
  pane-specific element IDs.
- gpui clicks on Enter/Space key-up, and only on the element the key went
  down on (vendored patch 0007): never dispatch again on key-down.
- Previews share one frame (`preview.rs`) and one toolbar
  (`preview_bar.rs`): a kind lists tools in slots, never builds a row
  ([previews.md](previews.md), "The preview frame"). Preview menus close
  when the content is replaced by another file or its presentation closes.

## Popups and focus

- Anchor popovers to their visible trigger through `elements/popover.rs`,
  measured in the current prepaint.
- gpui cannot nest deferred draws: peek stays in the normal overlay tree
  and defers its popovers, below modal dialogs. The settings dialog is not
  deferred (its dropdowns are) and stays the shell root's last overlay
  child.
- **Popups own the keyboard:** a menu or popover takes focus on open,
  returns it on close, and has its own key context (`menu.rs`).
- **Focus never dangles:** when the focused element leaves the window,
  the shell's focus-lost listener gives the keyboard to the pane drawn as
  active (`Shell::drawn_focus`), or to the Settings dialog while it is
  open, never to the pane behind it. Never refocus by hand after closing
  something.
- **One popup at a time,** all through `Shell::toggle_popup`. A press
  outside closes it and says so (`pointer` in its close command,
  `Shell::note_dismissed`); if that press is on the popup's own button,
  it stays closed. Any button's click closes the open popup the same way.
- Toolbar popup rows are buttons: arrows walk them, Home/End reach the
  ends (`ui.toolbar_first`, `ui.toolbar_last`). The row reached scrolls
  into view in prepaint (`toolbar::RevealRow`, read from the focused
  button's key context, `row`).
- A menu that depends on a toolbar variant is built once per variant and
  chosen in the popover's deferred prepaint from `AdaptiveRow::chosen`
  (`PopoverAnchor::popup_for`).

## The top of the window

No window row: each area owns its top, so a one-tab window has one row
of bars, split or not.

- **Sidebar header** (`address::sidebar_header`): the sidebar toggle and
  More (everything without a button). Without a sidebar they start the
  first pane's row (`Shell::corner_pane`). More hangs from
  `Shell::more_anchor`.
- **Split and Preview toggles** mirror them on the right: in the preview
  panel's bar when it is open at the right, else at the end of the tab
  strip, else at the end of the rightmost (or top, when stacked) pane's
  row. One rule decides (`Shell::toggles_at`). Their labels show where
  there is room (`address::panel_toggles`). A preview at the bottom has no
  bar.
- **Tab strip** (`tabs.rs`): shown with two or more tabs, or one with
  `ui.single_tab`.
- **Pane row** (`render_address`): Back, Forward; the address (trail or
  edited text), ending with the keep-in-sidebar button and then one button
  per other way to see the folder (`registry::Aspect`, e.g. Tags, Alt+G),
  last so it stays under the pointer; then the view buttons and the View
  menu.
  - No Up button (the trail's parent segment, Alt+Up) and no New Folder
    button (the folder's menu, Ctrl+Shift+N).
  - View buttons are the `ViewRole::Listing` views (list, grid,
    columns). `Alternate` (tree, board) and `Lens` (Usage) views get a
    button only while shown. The view states its role, never the shell.
  - The View menu (views, sort, hidden files, zoom, Fill thumbnails) hangs
    from the active pane's row.
  - Two `AdaptiveRow` variants: all buttons while the address keeps
    `FIELD_MIN`, else one button opening the View menu. The address has
    no width of its own when variants are measured.

### The trail

- A folder's trail starts at the deepest sidebar folder holding it, under
  the sidebar's name (`sidebar::roots`, re-read only when places change;
  `address::crumbs`): `Papers › 2026`, `stick › Photos`, else Home. With
  `ui.full_path`, every trail starts at `/` under real names.
- Tabs and history use the sidebar's name (`App::location_title`);
  listings, toasts and Properties use the folder's own. Up goes to the real
  parent; Ctrl+L edits the real path.
- Back and Forward list history on right-click or a long press, as in a
  browser (`ui.history_menu`; `back`/`forward` take `steps`). A long
  press is any button's `toolbar::Button::on_hold`: held `HOLD`, it runs
  its command, and the release clicks nothing.
- Chevrons list on a worker (`nav_menu.rs`): each one the folders in the
  segment before it; one after the last the shown folder's places
  (`LocationProvider::places_of`), a separator, then its subfolders
  (`ui.subfolder_menu`); one before the first, when the trail starts below
  `/`, the folders above from the root down (`ui.parents_menu`).
- In a narrow pane, the first segment in sight lists everything above it,
  wrapped-out segments included. Which segment that is comes from layout
  (`PaneUi::trail_shown`, from `on_children_prepainted`), read on demand.
- The trail and the list's detail columns share one layout technique: a
  wrapping flex line one row tall, so the nearest segments stay. A click
  beside the trail edits the path.

### Layout

- More's Layout group (`ui.layout`) places the timeline, the terminal,
  the split, the preview, and an archive member's preview. Split and
  preview default to Automatic, decided in `Shell::render` from the
  window's size (`layout.rs`). Shift+F3 splits the other way, for that
  split only.
- Closing a split closes the pane `core.close_split` names
  (`layout::CloseSplit`); Split's tooltip says which.

## The address field (`omnibox.rs`)

- Ctrl+F (`ui.search`) starts a search, with where it looks as a pill;
  Ctrl+L edits the path; over a search, both edit the search.
- Edited text opens a dropdown saying what Enter does: go to a path or a
  scheme; search where the pill says (Enter) or home (Alt+Enter); search
  contents (`query.refine`, facet `content`); a place Jump knows; `>` hands
  the rest to the palette.
- The keyboard never leaves the field (Up/Down are bound in its context).
  Rows run on press with `prevent_default`.
- A search is `query.search` with `expr` and `root`: the UI knows no query
  syntax.
- Words typed over a folder filter it at once (the core's `filter`,
  applied by `Pane::order`, so selection ranges and views see only what is
  shown; filtered-out entries are deselected). Showing another location
  clears it; Escape (`filtered` key context) shows everything; once the
  field lets go, the filter stays as a pill on the trail.
- What narrows the folder shown (the filter typed, a Narrow by choice, a
  tag location's `with`) is a pill after the trail with a funnel
  (`filter`); a search's pills are drawn in the trail's place, its words
  with the query's own icons. A funnel never stands for a search.

### Search pills (`search_bar.rs`)

- A search shows as pills: where it looks, each facet `query.describe`
  says is set, then "+ Filter". Each choice runs the invocation the plugin
  gave (`query.refine`, or `ui.ask` for a typed value), and navigates, so
  Back undoes it.
- The query plugin owns the meaning (`facets.rs`). Pills are read once
  per location, and again when the sidebar's names change; never per
  frame.
- The text is the one truth: a part no facet holds is a pill of its own;
  clicking beside the pills edits the text.
- The pills are one line, the richest that fits (`search_bar::Fit`, an
  `AdaptiveRow`): every pill as it reads; else where it looks and
  "+ Filter" as icons and each facet as its icon and value; else the
  facets' values alone, cut short with both ends kept, the later giving
  way first. Nothing is hidden by wrapping: a facet narrowed away is
  still a pill, said in full in its tooltip.
- Required tags are chips read as paths (`Places › France`) in the tag's
  color, "+" between them; an excluded tag (`not_tag`) is hollow.
- An affordance on what it acts on (a pill's ×) takes no focus; the
  pill's menu reaches it from the keyboard.

## Context menu

- `when` decides who sees an item: `empty` for items about the folder
  itself (the background menu, which clears the selection first); also
  `folder` for those that need a real folder, so searches and the shelf
  never offer them; `kind:image` (every selected entry) or `has:.gpx` (at
  least one) for items about some files (`registry::MenuItem`). Never
  offer an item to every file and let the command refuse.
- Words that depend on the environment ("Open With Image Viewer") come
  from a `MenuSource`, answered from memory as the menu opens; the plain
  items stay registered for the palette.
- Rare items set `submenu` (expanded in place). Give items an `icon`.

## Drag and drop

- **A drop asks.** Held keys decide (`dnd::held`: Shift moves, Ctrl
  copies, Ctrl+Shift links); with none, a menu at the pointer asks
  (`core.drop_asks`, `Shell::ask_drop`), the default highlighted for Enter
  (`dnd::drop_op`: ours move within a filesystem and copy across; other
  applications' copy).
- Each row runs the core command (`move`, `copy`, `link`) with `entries`,
  so it acts on what was dropped, not the selection by then.
- Plugins add rows through the registry's `drop` menu (order 100–899;
  their command gets `entries` and `dest`): Extract Here needs no branch
  in the shell.
- **Send to the other pane… (Shift+F7)** asks with the same menu, for the
  selection and the other pane's folder (`Shell::ask_other_pane`), hung
  from the cursor's row: extracting across a split is a drop's row.
- A drop on a place uses its location's verb (`Verbs::drop`). A drag
  carrying places is not asked about.

## Feedback

- **Every user action gets an answer** in `toast.rs`: a sentence built
  from the applied `Change`s ("Moved “a.txt” to Trash"), with Undo when
  the journal can take it back, Redo after an undo. Progress bars:
  [operations.md](operations.md).
- `Shell::set_status` is a toast; `set_status_about` (core:
  `App::notify_about_setting`) links to the setting that fixes it.
- **Footers hold state, never messages** (`status.rs`). A pane's footer:
  counts, the selection's size, or with nothing selected what its files
  weigh (`weight.rs`, on a worker; `PaneView::weighs` for a view that
  measures folders). The window's state (Undo, pending paste, plugin
  counts via `tally::Tally`, proposals, the player's strip) ends the last
  pane's footer. Side panels have no footer.
- A key whose command needs a selection has `selection` in its `when`,
  so it does nothing rather than report "nothing selected".
- **Animation:** only a bounded entrance under 200 ms, plus the player's
  play/pause flash (`media::FLASH_MS`).
- **Empty is a state.** `placeholder.rs` says why a pane is empty
  (`LocationProvider::empty_state` for non-folders) or failed (the
  system's message as detail) and offers a way out, inside the pane so
  drops and the menu still work. The banner above the view is only for an
  error beside existing entries.

## Properties and links

- A provider's fact can carry a command (`Detail::with_action`), a link
  run through `Shell::run`. Links show over the end of the row's last
  line while the pointer is on it, taking no room otherwise; those that
  are all a row says or that stop or start what runs always show, after
  the value ([previews.md](previews.md), "Header and Properties").
  Clicking the row copies the value; only the link acts.
- **Links (ADR-029).** A link written in a document answers a click. One
  found in raw text (`skwir_platform::links`: addresses by scheme, never a
  bare domain; paths in the terminal) answers Ctrl+click, drawn as a link
  only while Ctrl is held over it (`Run::found`, `Shell::armed_link`);
  Shift adds a tab.
- Addresses open only through `ui.open_url`; files and folders only
  through `ui.reveal` (with `line`, the preview shows it), never opened.
- Ctrl+Shift+O labels the links in view (`links.rs`).
- A terminal path resolves against the folder its line was printed in,
  stamped into the grid by the plugin (`plugins/terminal/src/links.rs`).
  Stamp new kinds of found links there too, never resolve them at click.

## The sidebar

- **Icons.** A place has a theme icon name. Symbolic-only or
  scheme-colored icons (Breeze's small ones) are shapes drawn tinted
  (`icons::Pic::Shape`); full-color SVGs are rasterized for the pixels
  they cover (`icons::element`). The theme is `ui.icon_theme` while
  installed, else the desktop's, changed only through
  `IconCache::set_theme`. On macOS and Windows the name maps to a system
  icon (`system_icons::for_name`).
- **Place fields:** `space` (filled by a worker: asking a volume can
  block), `eject`, `unavailable` (dims the row; the reason is its
  tooltip), `busy` ("Unlocking…": a still hourglass, nothing to click
  until done), `menu` (its own items). A rescan publishes at once and
  keeps the last measurements (`carry_over`), so nothing blinks.
- A place with a `color` is a tag-colored pill; pills wrap together
  (`sidebar::wrap_pills`, layout only).
- The sidebar repeats nothing its location says.
- **Keeping.** Ctrl+D and the pane's keep button run the core `keep`
  command, which hands the location to the `Keeper` registered for its
  scheme (bookmarks, saved searches). A kept place carries `forget` (the
  button runs it while shown) and `reorder`. A new kind of kept place
  registers a keeper and fills those two fields; no UI branch.
- **Bookmarks** are edited only through `BookmarkEdit`
  (`skwir_platform::places`); each edit returns its inverse, offered as
  Undo/Redo (`App::notify_undoable`, as for any edit outside a plan).
  Other applications' lines are kept; the file is watched. A folder
  dropped beside the rows is bookmarked (`places.bookmark`, GTK
  bookmarks).
- Files the desktop shares (bookmarks, `mimeapps.list`) are rewritten
  only through `skwir_platform::file::rewrite`, by an edit returning its
  inverse.
- **Tag pills:** pinned tags (`tags.pin`, ordered by dragging, unpinned
  by the keep button), else suggestions from the index
  (`Place::suggested`, outlined), then All Tags (`tags:/`). A home tag
  folder, not narrowed, is kept as its pin (`Keeper::keeps`).
  Shift+click narrows to files that also have the tag; Alt+click shows its
  files in the folder shown (`tags.pill`). The tag editor offers pinned
  tags first.
- **Drops:** a place takes what its location takes (`Verbs::drop`); one
  that takes nothing (Recent, a search) swallows the drop, never
  bookmarking it. A place dragged onto a file's row (`dnd::takes_gifts`)
  gives that file, or the selection containing it, what it takes
  (`takes_gift_for`).
- **Arrangement (ADR-034).** Groups are the user's (`sidebar::Sections`,
  part of the window's shape): click folds, drag moves, the sidebar's
  menu shows or hides each (`ui.sidebar_section`). A plugin names a
  place's group, never where it goes.
- Places are arranged by who keeps them (`sidebar::Role`): the user's
  kept places are their owner's to order, rename (`Place::rename`) and
  take out; suggested ones can be hidden; offered ones (Home, Recent, the
  Trash, volumes) are moved, renamed and hidden in the sidebar only
  (`ui.sidebar_place`, `Sections::places` by `Place::key`), never in the
  owner's files. A place moves only among those the same command moves
  (`sidebar::mover`); `arrange` is the one reading of all this.
- A group with every place hidden keeps its header, whose menu lists
  them; places offered hidden (`Place::hidden`) appear there unchecked.
- Rename… (`Place::rename`, F2) and Remove From Sidebar
  (`Place::forget`) are the sidebar's own items, never the owner's.
- F6 (`ui.focus_sidebar`, `Sidebar` context, `ui.sidebar_key`) gives it
  the keyboard: arrows walk, Enter opens and returns the keyboard, Menu,
  Alt+arrows move, F2, Delete. A click never takes the keyboard
  (`prevent_default`).
- Volumes: [operations.md](operations.md).

## Asking the user

A plugin needing text dispatches `ui.ask` with the command to complete;
one needing a choice dispatches `ui.choose` with its choices or a command
listing them on a worker (`choose.rs`; `option` adds a flag such as
"Always use for…"). Never a prompt or picker of its own.

- **A click answers what a key answers.** A dialog that waits for an
  answer ends with `modal::footer`: what its keys do, muted, then Cancel
  and the answer, named by its verb (Rename, Unlock, Delete; `ui.ask`
  takes it as `accept`, OK when absent). A picker's rows are its buttons.
  A press outside cancels too, but is never the only way: what is only
  read (About, F1's overview) closes from its ✕, as Settings does.
- **A dialog keeps the keyboard.** Tab walks its field, Cancel and its
  answer, round their ends (`Shell::walk_dialog`, `modal::tab_among`),
  never the window behind it; a list one picks from keeps it in its
  field. A focused footer button is pressed by Enter and Space, before
  the dialog's own Enter (a focused Cancel cancels); a press on one
  leaves the keyboard in the field; Escape that nothing in the dialog
  answered cancels it (`modal::modal`).
- **A question names what it is about when asked.** `ui.ask` fills the
  asked command's `pane`, `entries` (the selection) and `in` (the folder
  shown) that the asker left out, as they are when it opens: Rename
  renames the file it was opened for, whatever is selected by the
  answer.

## Keys

- **F1** (`shortcuts.rs`) lists the bindings of the focused element's
  key contexts; a new context needs only a name in
  `keymap::context_label`. A command run three times with the pointer gets
  one hint naming its shortcut, once ever: call `Shell::note_pointer_use`
  from new pointer-only entry points.
- **Layers (ADR-024),** lowest first: plugin defaults
  (`Registry::keymap`), the shell's (`keymap::BUILTIN`), key schemes
  (`Registry::schemes`, e.g. `vim`), the user's (`keyboard.bindings`).
  `keymap::install` installs them in that order. A binding hidden by a
  later layer is left out of hints and tooltips. A user's record without a
  command frees its keys (`gpui::NoAction`); one whose command nothing
  registers waits for its plugin, bound to nothing (`keymap::dormant`).
- **Text fields keep their characters.** A binding whose first stroke
  types a character (a letter, `.`, Space, with Shift at most), wherever
  it is bound, is bound to nothing in the text field's context
  (`keymap::typed_in_fields`, after the bindings it shadows), as the
  terminal keeps its keys: `.` bound anywhere still types in the address
  bar, a search, a rename.
- **Titles.** An action is called by `keymap::title`: its command's form
  for its arguments (`Declared::forms`), never by its command's title
  alone. A key bound with arguments gets a form where its command is
  declared (the shell's in `commands::KEY_FORMS`); `keymap_titles.rs`
  holds it.
- **The Keyboard page** reads the keymap's bindings, hidden ones too
  (`keymap::bindings`), as one row per action and place
  (`settings_keys.rs`), draws them in one list (`settings_keys_view.rs`)
  and asks for a new shortcut's command (`settings_keys_new.rs`). It
  writes `keyboard.bindings` only through `settings_keys::apply`, where a
  record written goes last and takes out the user's own it covers, so it
  is the one that runs and nothing it replaced comes back. It says what
  new keys would do through `settings_keys::verdict`, which reads
  precedence as GPUI does (the deepest place, then the latest), and what
  any change leaves of the other actions' keys, and whether it may be
  made, through `settings_keys::effects` (the keymap built before and
  after; worked out when the keys or the change change, never per
  frame). Keys are taken with `App::intercept_keystrokes`, so no binding
  runs while one is recorded; a bar with no keys to take gives its answer
  the keyboard. A place is grouped by its label in `keymap::PLACES`,
  which also says whether it is inside a dialog or a menu; a command that
  must keep a key says so (`Declared::keeps_a_key`), and no change leaves
  it without one in a place.
- Type-ahead is on only while no binding takes an unmodified character in
  the file list (`Keymap::typeahead`).
- Multi-stroke bindings wait without a timeout (vendored patch 0010);
  `sequence.rs` shows what can follow.
- A plugin that changes standard keys is off until asked
  (`Plugin::enabled_by_default`). A scheme key meaning a standard gesture
  says so (`keys.like`: vim's `d` is Delete) and takes that key's bindings
  and conditions, never a copy.
- Notation and `when` tokens: `AGENTS.md`, "Conventions".

## Tabs (ADR-018)

- Tabs are core data (`tab.*` commands); a tab holds one pane or two.
- A hidden tab is dormant: its panes keep history, sort and selection,
  and release listing, watchers and kept columns (`Pane::suspend`).
  Showing it reopens them through `App::show`. `core.sleep_hidden_tabs`
  off keeps every tab listed (`App::apply_tabs_sleep`).
- While the tab shown gets ready (`Shell::place_pane`), the window keeps
  drawing the old one for at most `tabs::HOLD`; the core keeps it open
  until `App::settle_tabs`. A user command first draws the tab shown
  (`Shell::present`).
- The shell never creates or drops a `PaneUi`: it changes the workspace
  and `Shell::sync_tabs` follows its revision, so `Shell::panes` is always
  the panes on screen (`is_split` and `other_pane` rely on it). Hidden
  panes wait in `Shell::hidden`, views reset; a pane gone is released
  there whoever closed it.
- Per-pane plugin state ends with `location_lifetime`, which dormancy
  ends too.
- Tabs are remembered only with `core.startup = last`
  (`core.last_tabs`); only the shown tab opens at startup.

## What the window remembers

- The last window's shape (view, split and share, sidebar width and
  sections, preview place and size, archive-member placement, timeline,
  zoom, Usage's levels and mode, column widths) is application data, not
  preference: `session.rs` keeps it under one key, written only when it
  changes. A new pane opens in the current pane's view.
- **Edges** dragged to size an area are one element (`elements/edge.rs`):
  double click restores the default; shared room via `edge::share`.
- The preview panel's width is bounded by what the panes and sidebar
  need (`preview::max_width`, from the window's width, no fixed cap); a
  small window draws it narrower without rewriting the remembered width.
  The same at the bottom (`preview::max_height`). The pinned player's dock
  is bounded by the panel (`Shell::dock_height_limit`); until the edge is
  dragged, the default height adds the dock's.
- The sidebar's width is bounded likewise (`sidebar::max_width`), and the
  sidebar hides below a window width computed in `Shell::render`.
- The split edge stores a share, not pixels (the panes' flex grow); the
  drag reads the container bounds from `DragMoveEvent`. Each pane keeps
  `PANE_KEEP` across, `PANE_KEEP_HEIGHT` stacked. The share belongs to its
  tab (`tab_splits`, swapped in `Shell::present`); a tab gaining or losing
  its split, by any route, starts even again (`sync_tabs`);
  only the shown tab's share is remembered.

## The terminal panel (ADR-019)

- It draws core's `terminal::Screen` and sends `terminal::Input`; the VT
  engine (`alacritty_terminal`) stays inside the `terminal` plugin.
- The session's threads parse, build screens (at most one per 8 ms) and
  wake the app; the UI thread only compares a revision. The shell starts
  on a worker.
- With the keyboard, every root binding is shadowed (`NoAction` in the
  `Terminal` context, built by `keymap::install`) except
  `TERMINAL_PASSTHROUGH`; add a key there if it must work from the
  terminal.
- The terminal's own keys (copy, paste, paging, links) are bound in its
  place (`when: terminal`). The shadows are bound before every other
  binding, so a key of the terminal's wins over the same keys bound to
  the window in any layer. A key it takes never reaches the program: an
  unbound Ctrl+Shift+C was Ctrl+C to it. Test a new one as GPUI reads the
  keymap (`keymap::tests::the_terminals_keys_are_its_own_there`).
- Its menu (`Shell::open_terminal_menu`) opens with the right button,
  or the Menu key, standing above the cursor (`MenuHost::Terminal`, its
  row marked by the grid in prepaint). Shift+F10 stays the program's,
  as the other F-keys are. On a link, the menu says where a link a
  program wrote leads before it is followed, as its tooltip does.
- The plugin types a `cd` only while the shell's process group has the
  terminal. Test with a real `/bin/sh`
  (`crates/plugins/terminal/tests/session.rs`).

## The player

- What plays is core data: the player publishes `playing::NowPlaying` in
  `App::player`; others (media keys via a plugin, scripts, agents) leave a
  `PlayerRequest`, taken at the next wake.
- A pinned player (`player.rs`) keeps playing while the selection moves,
  in the dock at the preview panel's foot (`dock.rs`); with the panel
  closed, a strip in the last pane's footer. Playback rules:
  [media.md](media.md).
