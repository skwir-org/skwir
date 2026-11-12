# Shell invariants: the window

Read this before changing the window in `crates/ui`: toolbars, colors
and fonts, popups, the top rows, menus, drops, feedback, the sidebar,
keys, tabs, windows, the terminal panel, the player. Binding, like
`AGENTS.md`.
Views are in [views.md](views.md); what locations decide, in
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
  pane-specific element IDs. A button names its pane in its command's
  arguments only where they accept one (`CommandSpec::accepts`, as the
  registry checks them): a place's Eject takes none, and an argument a
  command does not take is refused.
- An element is known by what it shows, never by where it stands
  (`elements::known_by`): gpui keeps an element's tooltip as it was
  built, its press and its screen reader's node by its id. A row known by
  its index hands them to whatever comes to stand there, and an element
  whose id stays while what it shows changes keeps saying the old (a
  server's row under an ejected mount kept the mount's "1.0 PB free").
  A file's row is known by its entry, a crumb by its place, a fact by its
  text, a picker's option by its item, a preview's rows by the file shown
  and their place in it; what is inside a row is known within it. Where
  two items may show the same, their position joins what they are known
  by.
- A focused control is pressed by Enter and Space through
  `modal::pressed`: where it has the keyboard (`toolbar_button`) the keys
  run `ui.toolbar_activate`, which it answers with its press, so no other
  binding of theirs (a dialog's Enter, the player's Space) answers behind
  it. gpui's own click on a key's release comes only where the key's
  press reached the control, which a binding stops, and only if focus has
  not moved since (its focus generation): a control whose keys are bound
  is never left to it, and nothing presses twice.
- **A popup's submenu opens beside its row**, as a context menu's
  (`toolbar::Submenu`, `Button::opens_beside`, the preview's More's Open
  With): the same rules (below, "Context menu"), the same look and
  `menu::REST`. Its rows have the keyboard's focus as the popup's do, but
  stand outside gpui's tab order, drawn beside the popup: the keys walk
  them by `ui.toolbar_submenu` (`toolbar_submenu`), Right on its row
  opens it (the row's `submenu` flag), and the arrows or Escape on the
  popup's own rows close it first. A press on it is no press outside the
  popup (`Submenu::area`). A row with a handle of its own
  (`track_focus`) takes gpui's tab stop from the handle, not the element:
  make it `tab_stop(true)` where the arrows walk it.
- Previews share one frame (`preview.rs`) and one toolbar
  (`preview_bar.rs`): a kind lists tools in slots, never builds a row
  ([previews.md](previews.md), "The preview frame"). Preview menus close
  when the content is replaced by another file or its presentation closes.

## Colors and fonts (ADR-071, ADR-073)

- Every token of a look is derived from its `Base` (`Theme::derive`):
  the desktop's scheme for that look, from where `ui.colors_from` says
  (Automatic: the desktop's own, then GTK's theme, then Qt's scheme; or
  GTK's or Qt's alone: `DesktopStyle::schemes`, `gtk_colors`,
  `qt_colors`, `Desktop::schemes`), else Skwir's own (`Base::own`)
  wearing the desktop's accent; Skwir's own chosen, no accent either. Text, the accent and the outcomes are
  guarded to their contrast (7:1 when the desktop asks for more); a
  token no desktop changes is `Kept`. Never a color beside the tokens.
- **Skwir's palettes** (`palettes.rs`) are values of Colors from beside
  Automatic, GTK and Qt: a `Base` a look each, worn whatever the desktop
  says (more contrast excepted), with their own accent, and the
  terminal colors and code theme their makers publish (Default on the
  Reading page draws in it, `desktop_style::palette_code`). A palette
  names no token: never a platform's palette or a company's colors
  (ADR-071, 2026-11-05). A new one passes the guard tests
  (`every_palette_reads_in_either_contrast`,
  `a_palette_looks_like_itself_as_drawn`), and its credits go in
  `tools/package/notices.py`.
- A scheme is one look's. A forced other look is Skwir's own colors with
  the desktop's accent; Automatic takes the scheme's look when the
  desktop has one look's (`Theme::look`). An accent the system names
  replaces a scheme's, and the selection is then derived, unless it is
  the scheme's own said again.
- What the desktop says is read on `desktop_style`'s thread, never while
  drawing; the first frame draws what was read last (the cache). A
  reading that differs from what was said wakes the window; one that
  does not, nothing. It follows the portal's signals and the files the
  colors and fonts come from (`kdeglobals`, `lxqt.conf`, the Qt
  platform theme's settings and the scheme they name, GTK's
  `settings.ini`, the user's GTK sheets and what they import, the
  folder of Omarchy's theme; their folders watched, access dropped),
  asked again before each reading (`Own::followed`), so a file named
  anew is followed where it is now, and a write to it once named is
  never missed; Windows' registry keys (the
  accent, a contrast theme), never a timer. macOS tells only AppKit's own windows: what it says is read
  again on a worker when the window's look changes or it is activated
  (`desktop_style::read_again`, `FOLLOWED`), each reading worn in the
  order it was asked; what is kept for the next start is written only
  when it changed. The portal's signals are followed on the shared bus,
  the text scaling factor among them; each reading asks the portal on a
  connection of its own with a call timeout (`Own::asking`), goes without
  a portal that does not answer, and is made again when the portal
  starts (`NameOwnerChanged`). Windows' keys are watched before the
  first reading. A source that names nothing is never said to be where
  the fonts or colors come from. Toolkits
  are never loaded: a GTK theme is read as the colors its sheet names,
  the user's own sheet laid over it as GTK lays it (`gtk_theme::
  UserSheet`: GTK 3's `gtk.css`, else GTK 4's), libadwaita's names read
  where GTK 3's are not named; a desktop shell is read where it writes
  for the toolkits, never from a palette of its own (Omarchy, which
  writes for none, excepted: `omarchy`).
- The fonts are the theme's tokens (`font`, `reading_font`,
  `code_font`), resolved with it, read where they are drawn. A view
  gpui draws on its own, outside the window's text style (a tooltip,
  `toolbar::Tip`; a drag's preview, `dnd::DragPreview`), names the
  theme's font itself: gpui's default, `.SystemUIFont`, is a family
  Linux seldom has (IBM Plex Sans), then gpui's fallbacks. A font's
  size is read with its family, from the same settings
  (`DesktopStyle::font_px`, `monospace_px`); GTK's and Qt's fonts are
  read apart too (`gtk`, `qt`), and `ui.fonts_from` chooses Automatic
  (the desktop's own, then GTK's in GTK's order: on Wayland the
  portal's, else GSettings', before `settings.ini`, on X11 after it;
  then Qt's: the platform theme's, qt6ct, qt5ct, qtengine or
  hyprqt6engine, their files followed; Plasma's
  `kdeglobals` as KConfig cascades them; macOS's and Windows' interface
  font), Skwir's own, or GTK's or Qt's alone
  (`desktop_style::Source`, as Colors from's), apart from Colors from;
  a print writes in the same (`read_fonts`). The text scaling factor is
  followed whatever is chosen, counted once: on X11 an `Xft.dpi` that
  carries it (GNOME's settings daemon writes 96 × the factor there,
  which gpui takes as the window's scale) leaves it out.
  What a font's Automatic and each size step come to is said beside
  them (`settings::show_resolved`, declared again when it changes).
- **The window's grounds are painted once.** Nothing is painted under
  the window's areas: each paints its own ground, once, over nothing
  else of the window (`Theme::window_bg` for the files, the terminal,
  the files' Quick filters; `Theme::preview_grounds` for what a preview
  shows, its paper or its letterbox whatever the kind, and Properties
  beside it; `window_bar` for the bars, the sidebar and its header,
  the tab strip, the timeline, the footer, the preview panel's bars and
  dock), so that what is behind the window shows through them as much
  as their opacity lets it (ADR-119: Appearance's three sliders,
  `theme::Opacity::chosen`, whole while the desktop asks for more
  contrast; the window asks the system to blur what is behind it while
  any lets it through, `shell::show_behind`, and says it is opaque
  otherwise). As cards (`Theme::cards`, ADR-123) the frame is one more
  ground, the chrome's opacity: painted once by `elements::frame` around
  the cards (holes where they are while the window lets the desktop
  through, under them while it is whole), and what lies on it (the
  sidebar and its header, the tab strip) paints none
  (`Theme::chrome_ground`). A container of areas (the shell's root,
  the pane's column, the preview panel) paints nothing; an area that
  shows a tint where a drag is over it lays it on its ground
  (`window_bg().blend(drop_target)`). A band laid on an area (an
  alternate row, a column's header, a code block, a notice, pinned
  contents, a bar a view draws in the files: Usage's, the Map's) tints
  its ground (`Theme::on_bg`, `color::overlay`, as `hover` does) rather
  than laying a second ground on it, and lets through what the ground
  lets through (`docs/invariants/previews.md`, Surfaces and colours);
  `toolbar::row` paints none of its own. A part of a bar drawn in a
  ground of its own (the address field and the view buttons' group in a
  pane's row, the tab shown in the strip or the sidebar's list) paints
  it at its bar's opacity, in a hole of the bar's ground: the bar paints
  around it once (`elements::ground`; the frame, as cards, around the
  tab shown). What is drawn on an area as a thing of its own (a picture,
  a page, a selection, a chip, a card's mark) and what is drawn over the
  window (menus, popovers, tooltips, toasts, dialogs, Peek, the whole
  screen) keeps `bg` and `surface`, opaque; so does Activity standing above the footer, its popover kept
  open, read at a glance whatever is behind. A part drawn both in the
  panel and over the window (a preview's content area, Properties, a
  container's kept rows) is handed the grounds it paints
  (`theme::Grounds`).
- **Text sizes (ADR-117).** The window's rem is the interface's size
  (`Window::set_rem_size`, from `Theme::sizes`, set where the theme is
  resolved), a pane's view the files' (`elements::rem`). Every place
  names a size (`sizes::TextSizes`, `painted`, `Sizes::body`):
  `sizes::every_place_names_a_text_size` reads the code without comments
  and white space and fails on gpui's scale, a number or a length given
  as a text size (`text_size`, `shape_line`'s and `line_wrapper`'s size,
  a `font_size:`) or a number as a line height, but where
  `SIZED_AS_WRITTEN` says why (a preview's content, at its reader's
  size), as many times as it says. A bar, a row, a control, a chip, a
  menu's row and the padding around text are written at 100 % in rems
  (`sizes::sized`), so they grow with the text; what is counted (a
  popup's height before it is laid out, a list's uniform estimate, a
  panel's limits, an element's own layout) is scaled
  (`Sizes::scale`, `Scale::of`). A width the user drags (the sidebar,
  the list's columns) is kept at 100 % and drawn at the size. Icons
  keep their size.

## Popups and focus

- Anchor popovers to their visible trigger through `elements/popover.rs`,
  measured in the current prepaint.
- A deferred draw inside another is drawn in a later round (gpui since
  2026-10-18, ten deep). Peek stays in the normal overlay tree
  and defers its popovers, below modal dialogs. The settings dialog is not
  deferred (its dropdowns are) and stays the shell root's last overlay
  child.
- **Popups own the keyboard:** a menu or popover takes focus on open,
  returns it on close, and has its own key context (`menu.rs`).
- **A press cancels a tooltip waiting to show** (gpui patch 0038): what
  the press opens is never covered by the tooltip of what was pressed.
  A tooltip already shown goes at the press, as upstream has it.
- **Focus never dangles:** when the focused element leaves the window,
  the shell's focus-lost listener gives the keyboard to the pane drawn as
  active (`Shell::drawn_focus`), or to the Settings dialog while it is
  open, never to the pane behind it. Never refocus by hand after closing
  something.
- **One popup at a time,** all through `Shell::toggle_popup`. A press
  outside closes it and says so (`pointer` in its close command,
  `Shell::note_dismissed`); if that press is on the popup's own button,
  it stays closed. Any button's click closes the open popup the same way,
  but a popup's row that switches one of several choices it shows (which
  views have a button) keeps it open, its row keeping the keyboard
  (`Button::keeps_open`), as does a setting's value.
- **What hears outside acts on its own gesture.** gpui's
  `on_mouse_down_out` and `on_mouse_up_out` fire wherever their element
  is not hovered, and an element a dialog covers never is: under About
  or the overview they hear every press and release in the window. Such
  a handler ends only what its element began (a popup open, a text's
  selection: `Shell::selecting_text`; a seek bar's knob: `media::scrub`)
  and otherwise runs nothing. While About or the overview is open, any
  command closes it (`Shell::admit`): a text preview's release did, and
  the release on About's ✕ that closed it then opened it again.
- **A dialog that holds the window's commands opens over no popup.**
  Settings, the overview and About (`Shell::takes_now`) close the popup
  open as they appear, and what is on its way to one (a menu of places
  being read, a note read from another chapter), before they take the
  keyboard (`Shell::close_popups_for_dialog`): About too, which appears
  once what it says is read, a menu opened meanwhile. A popup left open is
  drawn over the dialog, and its press outside, one in the dialog
  included, runs a close the dialog refuses (Settings: the popup stays)
  or closes for (the overview, About).
- **A setting in a popup is one row** (`toolbar::MenuPart::Choice`): its
  name, then its values (`Button::segment`), an icon each, Automatic in
  words; or tiles, four to a row (`Button::tile`). The value chosen is
  filled; while Automatic is chosen, the value it gives now is outlined,
  and Automatic's tooltip says why. Left and Right walk the values as Up
  and Down do (their key context's `toolbar::CHOICE`), Enter or Space
  chooses, and the popup stays open.
- **One menu look.** A popup that is a menu (`toolbar::menu_panel`) is
  drawn as a context menu is: on the chrome, as wide as its widest row,
  rows of `toolbar::MENU_ROW` in the window's text with their keys, the
  row under the pointer or the keyboard highlighted, and what is on said
  by a check, never by the highlight. Its rows are `Button::menu`; a
  context menu's rows (`menu::render`) keep the same metrics.
- **A switch only heads a group.** What turns a whole group of a popup
  on or off is its heading (`MenuPart::Heading`, `Button::switch`): the
  group's name, a switch at its end, the whole heading pressed, a stop
  of the arrows like any row, named by the group to a screen reader and
  said on or off; the group's rows wait under it, dimmed, while it is
  off. A single thing on or off stays a check on its row.
- Toolbar popup rows are buttons: arrows walk them, Home/End reach the
  ends (`ui.toolbar_first`, `ui.toolbar_last`). The row reached scrolls
  into view in prepaint (`toolbar::RevealRow`, read from the focused
  button's key context, `row`).
- gpui puts a tab group's own focus inside the group, ordered among its
  stops by its own `tab_index`: a popup's focus keeps index 0, before its
  rows, or Down and Tab pass them and leave for the window's first
  button (`toolbar::Popup`).
- A menu that depends on a toolbar variant is built once per variant and
  chosen in the popover's deferred prepaint from `AdaptiveRow::chosen`
  (`PopoverAnchor::popup_for`).

## The top of the window

No window row: each area owns its top, so a one-tab window has one row
of bars, split or not.

- **Sidebar header** (`address::sidebar_header`): the sidebar toggle and
  More (everything without a button). Without a sidebar they start the
  row its header would share: the tab strip while it shows, else the
  first pane's row. One rule decides (`Shell::corner_at`). More hangs
  from `Shell::more_anchor`.
- **Split, Properties and Preview toggles** mirror them on the right:
  in the side panel's bar when it is open at the right, else at the end
  of the tab strip, else at the end of the rightmost (or top, when
  stacked) pane's row. One rule decides (`Shell::toggles_at`). Their
  labels show where there is room (`address::panel_toggles`). A panel at
  the bottom has no bar. Properties and Preview are the panel's two
  switches, the panel there while either is on (ADR-058, decision 8,
  [previews.md](previews.md), "Header and Properties"). As cards, wherever they are, these and the sidebar's toggle and More
  lie on the frame ("Cards" under Layout).
- **Tab strip** (`tabs.rs`): shown while the tabs are listed (two or
  more, or one with `ui.single_tab`) and not by the sidebar drawn this
  frame (`Shell::strip_shown`). With `ui.tabs_at` = sidebar, the tabs
  are the sidebar's first group and the panes' rows are the window's
  top; a sidebar put away or a window too narrow for it gives the strip
  back, so the tabs are always in sight.
- **Pane row** (`render_address`): Back, Forward, and Up where the user
  asked for it; the address (trail or edited text), ending with Search,
  the keep-in-sidebar button and then one button per other way to see the
  folder (`registry::Aspect`, e.g. Tags, Alt+G), last so it stays under
  the pointer; then the view buttons and the View menu.
  - Search is Ctrl+F (`ui.search`), the one sign in sight that the field
    searches: there while the trail is, where Ctrl+F has a folder, a tag
    or Recent to look in or a search to edit, and something searches
    (`address::search_title`). Typed in, the field's token wears its
    magnifier, and the card's switch says Ctrl+Shift+F. Never a second
    field: the address is where words look (ADR-098).
  - No Up button by default (the trail's parent segment, Alt+Up) and no
    New Folder button (the folder's menu, Ctrl+Shift+N): each has a way in
    sight. Up comes after Forward with `ui.up_button` (the buttons' menu,
    Settings › General › Address), pressed only where the `up` command
    goes somewhere (`address::goes_up`), read from the trail shown, never
    from its provider while drawing.
  - View buttons are the `ViewRole::Listing` views (list, grid,
    columns). `Alternate` (tree, board) and `Lens` (Usage) views get a
    button only while shown. The view states its role, never the shell.
  - The user may give any view a button or take one away
    (`ui.view_button`, `address::ViewButtons`): from the buttons'
    right-click, taken in the capture phase so no button takes the
    keyboard first, the Menu key on one (`view_buttons`), or View ›
    Buttons…. Kept with their data (`ui.view_buttons`, ADR-057), only
    where it differs from the role, so a view added later comes with its
    role's. The view shown always keeps its button, so the group never
    goes and always says which view is shown.
  - The buttons' menu holds the row's buttons: whether the views have
    any (`ui.view_buttons_shown`, the switch of the View buttons
    heading), which views do, and whether Up is there (a check). With the view buttons off, the row has only the variant of one
    button (`address::row_variants`), which still says the view shown,
    and its right-click and Menu key open the same menu, so they come
    back from where they went; the views' rows wait there, dimmed, as
    chosen.
  - The View menu hangs from the active pane's row: every view as a tile
    (Buttons… at the end of their title), Sort by and Group by each one
    row saying how they are (`Button::value`: "Name, A to Z"), their
    fly-out the direction in the key's words on its first row
    (`FlyRow::Choice`, one stop: Left and Right choose, Left from the
    first goes back, Enter and Space the other) and the criteria under
    it; Group by's direction is always there, greyed until a criterion of
    its own is chosen, so nothing moves under the pointer; Folders first
    under them, the order's too (it orders folders within each group).
    Then what is listed (hidden files, companion files, what the location
    switches), then what is drawn with it (Quick filters and the
    timeline, Fill thumbnails…), and the size on one row, each part apart
    by a line. What it shows of a pane is that pane's: its view, sort, hidden
    files, Quick filters above its files and the timeline under them
    (`PaneUi::quick_filters_on`, `PaneUi::timeline`), which a pane made
    from it (a split, a tab) takes. More holds the window's panel without
    a button (Terminal) and the layout's settings, one row each
    (`address::layout_values`).
  - `AdaptiveRow` variants: all buttons while the address keeps
    `FIELD_MIN`, else one button opening the View menu (only that one
    with the view buttons off), and narrower still the field without its
    own buttons (Search, keep, an aspect's: Ctrl+F, Ctrl+D, Alt+G) and
    without `FIELD_MIN`, so a narrow pane's row fits. The address has no
    width of its own when variants are measured. As a card, the band is
    cut at its end: nothing it holds is drawn over a notch.

### The trail

- A folder's trail starts at the deepest sidebar folder holding it, under
  the sidebar's name (`sidebar::roots`, re-read only when places change;
  `address::crumbs`): `Papers › 2026`, `stick › Photos`, else Home. With
  `ui.full_path`, every trail starts at `/` under real names.
- Tabs and history use the sidebar's name (`App::location_title`);
  listings, toasts and Properties use the folder's own. Up goes to the real
  parent; Ctrl+L edits the real path. Whatever goes up (Up, a place of the
  trail, of the sidebar or of the columns view, a path typed), the way
  back down is selected there once listed (`App::way_down`: the folder on
  the path, or the next place's label in the place before it), as every
  file manager does; quietly (`Pane::reveal_way_down`): only if shown, so a
  hidden folder left turns no hidden files on. An entry asked for
  (`navigate`'s `select`) wins.
- Back and Forward list history on right-click or a long press, as in a
  browser (`ui.history_menu`; `back`/`forward` take `steps`). A long
  press is any button's `toolbar::Button::on_hold`: held `HOLD`, it runs
  its command, and the release clicks nothing.
- Each step of a pane's history keeps the entry the cursor was on when
  the pane left it (`Pane::go_on`, `go_to`); Back and Forward select it
  again once listed, so a link followed is gone back from to the document
  it was in. A link to a file in the folder shown adds a step without
  listing the folder again (`App::step_here`).
- Each step keeps what filtered it when the pane left it, recorded as
  every location is shown (`Pane::leave_step` in `App::open_in`, and in
  `step_here`), and Back, Forward and the field's step taken back show it
  so (ADR-109, `Pane::filter_at`); a closed tab opened again keeps them
  (`Pane::steps_left`). Nothing else changes a step's filter behind the
  pane's back: a filter changed in place is the step's as it is left.
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

- More's Layout group (`ui.layout`) places the terminal, the split, the
  preview, and an archive member's preview. Split and
  preview default to Automatic, decided in `Shell::render` from the
  window's size (`layout.rs`). Shift+F3 splits the other way, for that
  split only.
- Closing a split closes the pane `core.close_split` names
  (`layout::CloseSplit`); Split's tooltip says which.
- **Cards (ADR-123, `ui.cards`).** Each pane, the preview panel and the
  terminal are cards on the window's frame, each wrapped in `frame::card`,
  which keeps what it holds within its corners (a rounded content mask,
  gpui patch 0053) and tells the frame where to leave its hole. A card
  runs to the window's edge wherever it meets it; only the top keeps a
  gutter (`Shell::frame_inset`), gone where the window meets another there
  (`Window::tiling`, patch 0054). A card's corners are found from where it
  is laid out, never listed by arrangement: round where it stands apart,
  square where an edge is on the window's. Nothing in a card draws an edge
  of the window's layout: the edge between two cards is the gutter between
  them (`edge::gutter`, 6 px, inserted by the shell between the panes and
  above the terminal, by the preview before its card), measured from its
  middle, since a strip on a card's border would be cut by its corners.
  The cards are outlined, in either look, a device pixel on the frame
  just outside each card's shape (`Theme::card_edge`, painted by
  `elements::frame` within the window, so not on the window's edge): the
  card's edge, not a line between areas. No
  line between areas (`Theme::lines`): a pane's row and its footer are
  bands of `bar` over its card (`Theme::bar`: in a light look between
  the frame, the chrome's gray, and the files); a bar's row is then as
  tall as the bar (`Theme::line_width`). While the window is split, the pane commands act
  on has its row underlined in the accent and the other's grounds step
  down (`Shell::lowered`, `Theme::files_ground`, `bar_ground`): what paints
  a pane's ground asks them, never `window_bg` or `window_bar`. Apart
  by lines, the window is drawn as before: kept working, not grown. An
  area added is designed as a card first, and given the lines look only
  as far as it needs to work there (ADR-123, "Kept, the default").
- **As cards, the preview panel is cards** (`preview::cards_under`), never
  one: Properties with the header, the preview with its toolbar, a
  container's kept rows and its member (`members::split`), the pinned
  player; the header heads the first. The edges between them (Properties',
  the member's, the dock's) are gutters (`properties::gutter`,
  `Shell::dock_gutter`), and what sizes itself by the room (Properties'
  share) does so on the card around it.
- **As cards, the window's buttons lie on the frame**, never in a card:
  Split, Properties and Preview, and the sidebar's toggle and More while
  the sidebar is put away (`toggles_at`, `corner_at` unchanged). In a pane's
  row they are a notch of its card (`frame::notch`, `address::on_frame`):
  the band ends before them, rounded, and comes down to the files with a
  concave corner whose fillet the frame paints in the band's ground. Above
  the preview at the right, its bar is a row of the frame and its card
  starts under it, level with the files.

## The address field (`omnibox.rs`, ADR-070, ADR-098)

- **A scope before the text.** The field may hold a token: where words
  look. Ctrl+Shift+F (`ui.filter`) and Ctrl+F (`ui.search`) start with
  the folder shown, Ctrl+L with none and the path selected, Ctrl+J
  (`ui.jump`) with none and nothing typed, places first; over a search,
  Ctrl+F and Ctrl+L edit it, its root the scope, and Ctrl+Shift+F shows
  the folder it looks in filtered by its words. Only the scope is a
  token; terms stay text.
- **How deep words look** (`omnibox::Depth`, ADR-098, amended): in the
  folder shown alone, or in it and its subfolders. Ctrl+Shift+F starts
  on the first (and so does Ctrl+L, words then filtering the folder
  shown), Ctrl+F on the second; typed in, each switches to its own,
  keeping the words, as the switch on the card's first line does, which
  says each side's keys (`keys_for`) and what the words keep there. The
  token wears a funnel for the folder alone, a magnifier below it (a
  place searched as it lists, its own glyph). Another scope chosen is
  searched; none, words filter the folder shown.
  - A place that says depths of its own (`App::depths`, ADR-108: a
    tag's, `deep` off and on) is filtered at both: the switch reads its
    words ("This tag | With sub-tags", at the top "These tags | Every
    tagged file"), the depth chosen is shown in place of the field's
    step, nothing is searched as typed, and the token keeps its funnel.
    Ctrl+F opens it with everything within; the other ways in keep the
    depth shown. What only a search tells (`ReadFilter::waits`) waits
    for Enter, then searches the place as shown (`in:`), from what the
    pane showed, or, at a depth that lists no file of its own (the top's
    tags), with everything within it, the Run row saying so. Over a
    search of such a place the switch reads its depths; words edited
    filter the place again, and the scope taken away or chosen keeps the
    depth. Whether words filter here is one answer
    (`Shell::filters_here`), asked wherever the field decides it.
  - The folder alone is the folder's own listing, filtered at each key
    (below): still a folder to paste into, drop on or make things in.
  - Below it is the search Enter runs, shown as the words are typed
    (`Suggestion::live`), `LIVE_PAUSE` (400 ms) after the last key, the
    one waiting before dropped; a switch shows it at once. It goes in
    place of one step of the pane's history, the field's own
    (`App::step_here`, then `navigate_in_place`): Back from it goes to
    what was shown before. A search that reads the files (contents, a
    picture's date or place), looks over the whole disk, or below a
    folder on a server (each walk round trips) waits for Enter, and the
    first row says so (the query plugin leaves `live` empty).
  - Enter with no row chosen keeps what the words show and gives the
    files the keyboard: the folder filtered; the search shown, now
    remembered (`query.search` leaves a search shown as it is), or run
    when it waited. A row chosen goes from what the pane showed: the
    field's step is taken back first, and a source's row runs over the
    folder filtered as it was (as Escape puts it back), never by the
    words typed. Words no source answers (the query plugin off) offer no
    Go row: Enter keeps what they show. Enter on an empty field shows the
    location again with the lens kept beside the text (Ctrl+J, Ctrl+L);
    only a text that held the lens, emptied, takes it off.
  - Escape takes the field's step back (`App::take_step_back`, as if it
    had never been gone to), puts back the filter the folder had (its
    lens with it), and gives the files the keyboard. The pane's address is
    never written into the field while it is typed in.
  - The field's step is its own only while the pane shows what the field
    put there, at the place in the history it took it (`Omni::step`).
    Gone elsewhere by other means (Back, Up and Forward run in the field
    too), even to a step showing the same location, Escape and Enter take
    no step back and cut no history, and the filter the field began with
    is put back only where the pane shows what it showed then.
  - The field lets go when it loses the keyboard in a window still
    active (a click on a place or the files, Tab, F6; `omnibox_left`):
    what its words show stays, as Enter keeps it, the search waiting for
    its pause is dropped, and the field shows where the pane is; nothing
    typed, its step is taken back, as Enter does. No state of the field
    outlives its focus to act on what the pane shows later.
  - A search shown after the pause replaces only what the pane showed
    when it was asked (a place clicked meanwhile stays), and is known by
    its words and its scope (`Omni::shown`): another scope chosen with
    words typed shows its own search at once, as a switch does.
  - Backspace at the start of the text takes the scope away; Left there
    selects the token first (`input::AtStart`, sent only where the field
    asks, `send_start`).
  - A click on the token, Down on it selected, Alt+Down
    (`ui.omnibox_scopes`), or `@` typed first list scopes: the folder
    shown, the one above, home, the whole disk, the sidebar's places
    words can search, those whose names go with what follows `@`; under
    them the query plugin's "Search for “@…”", for a name that starts so.
  - Ctrl+F on a highlighted row that has a scope (a folder, a tag,
    Recent) makes it the scope and clears the text for words; with none,
    words look below the folder shown, its scope.
  - What is a scope is its provider's answer (`App::searchable`,
    `LocationProvider::searchable`): a folder, a tag, every tag, Recent;
    never the trash or the shelf. Ctrl+F, the rows, the scopes listed and
    the Search button ask it, never a scheme.
- **The Search button** (`address::search_title`) ends the field, as
  Ctrl+F (`ui.search`): nothing else in sight says the field searches.
  "Search this folder and its subfolders" on a folder, "Search" and the
  place's title where words can search it (a tag, every tag, Recent),
  "Edit this search" over one; none where words cannot look (the
  trash), while the field is typed in (its token wears the magnifier),
  or while no source searches words (`suggest::searched`, a source's
  `SuggestSource::searches`: the query plugin off). It names no
  command of a plugin.
- **Rows come from sources** (`skwir_core::suggest`): each asked again at
  every change of the text, in a job of its own in its lane, the jobs
  before cancelled; an answer is kept by source until the next. A key
  takes no row away: each source's rows stay until its answer to the new
  text comes, so the card never closes, shrinks or jumps for a frame
  between keys (it did whenever a frame was drawn before the answers).
  A row answering an earlier text (what Enter does, completions, the
  reading, other scopes) draws after the text only what its completion
  still adds to it (`rest_after`); Enter and Tab take rows only once
  they are the text's (`settle_rows`, below). Rows go by `Section`, then
  rank; a row the arrows chose stays chosen when answers arrive. The
  shell names no plugin: it runs the rows' invocations and the core's
  commands (`filter`, `navigate`, `open`).
  - The query plugin answers what Enter does with words (the search in
    the scope, or why it cannot run: Enter then waits), what the term at
    the caret goes on with, the words read as terms ("Did you mean",
    applied only when picked), the same search elsewhere, and before
    anything is typed with a scope the terms as buttons, recent and
    saved searches; and the spans the text is drawn with.
  - Jump, the tags plugin and the shell (`suggestions.rs`: other tabs,
    the pane's history, the sidebar's places, a few commands with their
    keys) answer the rest.
  - Words are mostly a search, so a source offers a row only for words it
    finds as the row reads: Jump each word unbroken in the path from `~`,
    in the name before its folders, never letters strewn over a path.
  - The field's own source stays the paths and places it completes
    (below), read in their lane.
- **The rows and the field are one card** (ADR-098, amended): typed in
  and with rows, the field's card stands around it (`popover::
  popup_around`, at the popups' priority), from just before its left
  edge, as wide as the field (at least 430 px, moved left to stay in the
  window, never short of the field's right edge); the rows go under a
  line where the eye already is. The field's text and scope draw above
  the card (deferred one priority higher, `omnibox::CARD_PRIORITY`), so
  their key context, focus and pointer stay theirs; the card's top leaves
  them the field's height (`address::FIELD_HEIGHT`), never a size read in
  `render`. While words are typed, or about to be, the card's first line
  is how deep they look (above) and what they keep there, the folder's
  count or the search's: the card hides the list's top.
- **The card never leaves the window**: never wider than it, and never
  taller than what it leaves under the field (`popup_around`'s room).
  Its rows scroll there, under a scrollbar, between the depth line and
  the footer, which stay in sight; however many a source offers (every
  tag the user has, for `tag:`), each is reached by the wheel
  and the keys. They are a `gpui::list` that builds only the rows in
  view (`omnibox::draw_piece`), never a child per row: two thousand tags
  cost what a screenful does. A child of the list is a piece
  (`omnibox::pieces`): a row, a section's title travelling with its first
  row, a strip of buttons whole. The row the arrows reach is brought into
  view with what it is (`Omni::reveal`); rows for another text start
  from their top, and rows that come later for the same stay where they
  were scrolled. A strip that wraps (Add a term, Search in) counts as
  tall as it is drawn: the list takes the card's width (`w_full`) and is
  kept from one opening to the next (`PaneUi::card_rows`), so it knows
  that width; the first opening in a window, or the first after the
  field's width changed, takes one frame more (`views.md`, Scrolling).
- **`?` alone** lists what the field takes: the shell's grammar (a path,
  a place's address from the schemes providers open, `@`, words, `>`)
  and the query plugin's terms with examples; Enter or Tab writes how the
  row starts (`~/`, a scheme, `@`, `>`, a term's key such as `kind:`),
  never the example.
- **Before anything is typed**, the dropdown shows what the sources offer
  then, nothing highlighted: Enter does what the text says (the location
  shown again, a search's words again). A row the arrows reach runs as
  any row (a place, a tab, a recent search), and Tab takes one that
  writes text (`completes` holds for it).
- The card says the keys bound (`keys_for`: its footer, the highlighted
  row's, the depth switch), never keys written in its words: a scheme or
  the user may bind others, and one bound to none is not said.
- The keyboard never leaves the field (Up/Down are bound in its context).
  Rows run on press with `prevent_default`. The rows are a list box
  (`modal::options`); the row the arrows reach is the field's active
  descendant, so typing still reads back the field's text and caret.
  Shift+Delete (`ui.omnibox_forget`) forgets a recent search highlighted.
- **The text is drawn with the spans** the query plugin reads
  (`TextInput::set_styles`): keys in the accent, values on `term`,
  operators muted, what cannot be read with a wavy line in `error`. An
  edit keeps the spans wholly before it, which still cover the same
  characters, until a source has read the new text; replacing the text
  clears them. The UI knows no query syntax.
- **Completion (ADR-070, `complete.rs`).** A path lists the names of the
  folder typed that go on with the name typed: folders, then files, at
  most eight, matched as `complete.rs` says (case ignored unless a
  capital is typed; whole, start, a word's start, anywhere; hidden names
  only for a dot or where they are shown), ranked by `jump.scores`. Go is
  first only while no name is typed or none matches: otherwise Enter goes
  to the first name, whose rest is drawn after the text
  (`TextInput::set_hint`). A source's row draws its `rest` the same way.
  - The folder typed is read on a worker, once while the field is typed
    in, the one read before stopped; a folder a pane lists gives its
    listing's names. Never on the UI thread: matching is in memory.
    `./` and `../` are the folder shown's, never the process's.
  - A folder part that names no folder is taken a name's start at a
    time (`~/w/sk/`), each level ranked, at most four folders: the rows
    then say which folder, and nothing is drawn after the text.
  - A place's text completes through its provider
    (`LocationProvider::complete`), matched the same way.
  - Tab (`ui.omnibox_complete`, bound in `address completes`): a row
    chosen with the arrows; else words leaving one name shown become its
    path; else the part the names share; else the highlighted row; else
    the first is highlighted. A folder taken ends with its separator and is
    listed in turn. Taking is an edit (Ctrl+Z). Right or End at the end
    takes what is drawn. Where nothing completes, Tab walks on; while the
    names it would choose among are read, it waits for them (below).
  - A row keeps its exact path: a name that is not UTF-8 goes where it
    says, though the field can only write it approximately.
- Enter on a path stats it on a worker (`reveal_path`): a folder is gone
  to, a file shown selected in its folder (hidden files then shown, and
  said), one not there said. Shift+Enter opens a file (`open`). A
  `file://` URI typed or pasted is read as the path it names here
  (`uri::path_of`, `Shell::omni_text`), the field keeping what was typed. A relative path with no folder shown (a search, a
  tag, the trash) goes nowhere, and says why.
- Enter or Tab pressed before every source answered the text asks them
  all there and then (`settle_rows`, `suggest::each`; they answer from
  memory), so what the key takes is always the text's. A row clicked
  runs as it reads.
- Enter or Tab pressed while the names a path or a place goes on with are
  still read (the folder typed, a provider's `complete`: on workers, never
  here) waits for them (`Omni::held`, `completing`): the key is done once
  they come, as if pressed then, so Enter goes to the first name as it
  would have. Another text, the arrows, Escape or the field letting go
  forget it; the card says the folder read once it takes a moment. A
  Tab held that then finds nothing keeps the field rather than walking on
  later.
- **Words typed over a folder, looking in it alone, filter it at once**,
  as the core's `filter` reads them through the query plugin
  (`lens::read_filter`): the longest start that parses; words and `name:`
  hold for folders too, other terms (`ext:` among them) for files, every
  folder staying; contents and a picture's date or place need a search
  below it, which the footer says; what a
  filter that reads metadata needs of the files no view draws, Quick
  filters' count asks for (locations.md). It runs at each
  key on the window's thread, so a type is asked once per suffix
  (`mime::BySuffix`, as `mime::by_names` asks a selection): 6 to 13 ms a key
  over 100 000 names in release. Showing another location keeps its lens
  (ADR-107, locations.md) and lets go of its words; Escape (`filtered`
  key context) shows everything; once the field lets go, its facets stay
  as pills after the trail (below). Ctrl+Shift+F over a filtered folder
  edits its words, the lens's among them, Ctrl+F takes them below it;
  begun on a path or on nothing (Ctrl+L over a folder, Ctrl+J), the field
  keeps the lens before the words typed (`suggest::Ask::lens`), which add
  to it. Every search the field runs says its lens whole (`as_is`). Over the folder alone, the search's row is none,
  and a term completed or the words read as terms are written by Enter,
  as by Tab.
- A search keeps the trail of the folder it looks in (the place it
  lists, `in:recent:`: `LocationProvider::trail`), its own crumb drawn
  as its pills; Escape with nothing selected goes to that folder (`up`,
  `search` in the key context), as Escape in a folder filtered shows it
  whole. A tag location keeps its trail and is a place: Escape never
  leaves it, as it never leaves a folder (Backspace and Alt+G do).
- What filters a location that is no search (a folder, the trash, a
  tag's place: its lens and the words typed, a tag's sub-tags) is the
  search's own chips right after the trail, read once per change
  (`query.describe_filter`, `search_bar::Filters`): the same labels,
  icons and menus, each choice changing it in place (`query.filter`, or
  going where the location leads without it), and "+ Filter", which adds
  to them and offers no facet that reads the files. A pill of the lens
  (`Chip::lens`) says in its tooltip that it stays as the pane goes
  elsewhere. Only the field's token and the depth pill wear a funnel: how
  deep the words look says filter from search. The first pill says how
  deep the words look, a dropdown: a folder's "This folder only"
  (`query.describe_filter`'s `depth`), its subfolders searched for its
  lens and words (`query.below`); a search's "And subfolders" (or
  "This folder only", searched so), its folder filtered by its words
  when the folder alone can tell them, else searched alone
  (`query.here`). Its tooltip says the key that does the same from the
  field.
- The trail and the pills after it give way alike, each by its natural
  width (`adaptive::Fit`, drawn from its left edge): the trail wraps its
  first places out, the pills draw the richest of their fits that holds
  (the depth pill its icon alone first), a folder's as a search's. The
  room after them edits the address.

### Search pills (`search_bar.rs`)

- A search shows as pills after its folder's trail: how deep it looks,
  each facet `query.describe` says is set, then "+ Filter". Each choice
  runs the invocation the plugin
  gave (`query.refine`, or `ui.ask` for a typed value), and navigates, so
  Back undoes it. A change that takes the last term away shows the place
  searched (the trail's: the folder, or what it lists), as Escape does
  and as a folder's last pill gone shows it whole: an empty search is
  asked for, never left over. One asked with nothing (`in:/x`) stays a
  search while its depth or hidden files change.
- The where pill's choices include Hidden files (`hidden:`) and Folders
  left out (`excluded:`, ADR-125), its value naming those on; neither lets
  the folder alone filter the words (`facets::as_filter`).
- The query plugin owns the meaning (`facets.rs`). Pills are read once
  per location, and again when the sidebar's names change; never per
  frame.
- The text is the one truth: a part no facet holds is a pill of its own;
  clicking beside the pills edits the text.
- The pills are one line, the richest that fits (`search_bar::Fit`):
  every pill as it reads; else how deep as its icon; else that and
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
  items stay registered for the palette. Its command takes what its menu
  gives (`pane`, always; a place's `location`, a tag's `tags`, the
  preview's file shown, `entries`): one that
  refuses an argument answers nothing, unsaid
  (`crates/skwir/tests/plugins_toggle.rs` gives every source its menu's).
- Rare items set `submenu`. Give items an `icon`.
- **State is said, never written into a title.** An item that is on or
  off sets `MenuItem::checked`; the values of one setting set `choice`
  (`registry::Choice`). A choice is one row where its values allow it
  (`ChoiceLook`): its name, then buttons (an icon each, five at most),
  swatches (colors) or stars. A press chooses a value and the menu stays
  open with its checks updated (`Shell::choose`). From the keys, Left and
  Right move a ring along the values, choosing nothing, and Enter or Space
  chooses the one ringed; Left from the first value of a submenu's
  choice goes back to its row, as from any other row. Values that need their words are rows, each checked
  or not, and a submenu they fill says the one chosen on its row (a
  server's Thumbnails). To a screen reader a choice's row is a radio
  group, its values radio items.
- **A submenu opens beside its row** (`PopoverAnchor::popup_beside`):
  its first row level with its row, on the right or, without room, on
  the left, moved up to stay in the window. A submenu's row stands where
  its first item's order puts it, in that item's group, with the first
  icon among its items. The pointer opens one
  after resting `menu::REST` on its row, or at once with a click; resting
  on another row closes it, reaching it in time keeps it. Right, Enter or
  Space open it and move into it; Left or Escape go back to its row. A
  press on it is no press outside the menu (`MenuState::in_sub`). A
  submenu holds no submenu.
- **A check in a submenu keeps the menu open** (the tags in Tags):
  pressed, it turns over and the menu stays for the next, so its command
  toggles (`tags.toggle`), a second press undoing the first. A check at
  the menu's own level closes it, as any item does. A check on for some of
  what it is about is `Check::Partly`, a dash.
- **The file menu** (ADR-099): Open, Open With › (the type's other
  applications, Other Application…, Run Command…), Open Terminal Here;
  Cut, Copy, Copy Path, Add to Shelf, Add to Bookmarks; Rename (Rename
  Several… with several), Move to Trash, Delete Permanently; Extract Here,
  Compress ›, Find Similar ›; Tags › (Edit Tags…, a check per tag, the
  name tools), Rating, Comment…, the GPS items; Properties. Paste is the
  folder's own menu's, on its background.
- **Never taller than the window.** The menu stands where it opened (at
  the pointer, or under the keyboard's row), moved to stay in the
  window, and takes at most the window's height less its margins, in
  prepaint (`PopoverAnchor::popup_at`, `popup_first`: `Room::Window`,
  where a toolbar's popup shrinks to the room on its button's side
  instead). Its rows scroll inside its padding; the row the keyboard
  reaches is brought into view in the frame that draws it
  (`MenuState::reveal`, the separators counted among the rows'
  children, `menu::child_of`).
- **A tag's menu, where a file shows it.** A right press on a tag's chip
  or dot (`Shell::tag_act`: an entry's in a view, the preview's header,
  Properties) selects the entry as its row's right press does
  (`Shell::open_chip_menu`) and opens the tag's own menu, asked with
  `files` (`tags.tag_menu`): its files in the folder shown, its place,
  the tag editor of the selected files (Edit Tags…: where a file shows
  its tags, its chips are the way to it), taking it from them, then
  what its pill's menu offers.
  The keys reach the same: the context menu of selected files gives each
  of their tags (at most four, pinned first) a submenu, `Tag “draft”`
  (`tags.files_menu`), with its color too (the editor is the menu's own
  Tags…). A chip in a side column or
  on another place's card is no entry of the pane: a press there opens
  nothing.

## Drag and drop

- **A drop asks.** Held keys decide (`dnd::held`: Shift moves, Ctrl
  copies, Ctrl+Shift links); with none, a menu at the pointer asks
  (`core.drop_asks`, `Shell::ask_drop`), the default highlighted for Enter
  (`dnd::drop_op`: ours move within a filesystem and copy across; other
  applications' copy).
- **Only the destination is marked.** While files are dragged (from a
  pane, or `ExternalPaths` from another application), a drop target
  (`dnd::droppable_dir`, `droppable_place`, a sidebar place that takes
  nothing) hides what is behind it from the pointer
  (`dnd::marked_alone`, `block_mouse_except_scroll`): over a folder's
  row, a column, a tab, a crumb or a place, that one is marked and takes
  the drop, never the pane, the column or the sidebar holding it as
  well; the wheel still scrolls them. Asked of the drag's payload
  (`App::has_active_drag_of`, gpui patch 0046), never of any drag: an
  edge resized, a tab or a place moved pass a target by, so their
  release reaches what is behind it (`Shell::resize_hook`).
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
- **A drag of files shows what it took** as the views draw it
  (`dragicon.rs`): the row pressed, else the selection's first, and as
  many more as `ui.drag_image` lays out (`dragicon::Layout`: a fan, the
  default, a pile, a mosaic, or the one alone), one of each other type
  first. Each is its thumbnail as the window keeps it
  (`ThumbCache::kept`, whichever size a view drew), else its icon, drawn
  for the drag on GPUI's executor (`icons::pixels`) while a glyph stands
  in; a badge counts them, the mosaic's last quarter says how many more.
  One file is its picture alone in every layout. It never asks the
  core's thumbnails: the window takes what it draws out of them
  (`Thumbnails::take`). The picture is kept for the drag's length: its
  view let go (`on_release`, `DragPreview::let_go`) whether the drag was
  dropped, cancelled, ended by the compositor or left with its window.
- **A drag's name is under its icon, as the grid writes names**
  (`dragname.rs`): centred on the accent's pill, up to three lines at
  most `dragname::WIDTH` wide, a longer one keeping its first lines and
  its end on the last (gpui patch 0037); a short one on a pill as wide as
  it, measured with the text system, as gpui lays a drag's view out at
  its least width; on several lines it breaks as a grid's name does
  (`name_lines`, views.md), the pill as wide as its widest line. The
  text's box is given that width in whole pixels
  and one more, as laid out an edge lands on a whole device pixel and a
  box a hair narrower wraps the last word ("3" over "items"). Its own
  font is named (gpui draws a drag's view
  outside the window's text style). A key held says what the drop does
  before the name ("Copy …"), in the window only.
- **The same picture inside and out.** If a drag of files leaves the
  window (Wayland), the compositor shows the icon with its name under
  it, drawn as the window draws them (`dragname::picture`: laid out as
  gpui's text element lays it out, at the same width, so it breaks where
  the window's does; each glyph from the text system, gpui patch 0048)
  at the window's scale, which the compositor shows at its
  logical size (patch 0049, through the viewporter for a fractional
  one): the text as sharp as the window's. Brought back into the
  window, the drag is still the compositor's, which draws it there too:
  the window does not draw it again (patch 0047).
- A drop on the terminal types the files' paths at its prompt and asks
  nothing: no file is copied or moved (see the terminal panel).

## Feedback

- **An answer is said where it was asked**, where the user is looking.
  A field or popup that asks something answers in itself: a find's "No
  match", why it did not look, its failure, are its popup's status line
  (`preview_bar::Field::say`, gone when the text changes), announced as
  a toast is. A toast is for an answer with no place of its own in view,
  and for one whose place is gone (a script's find, no field open).
- **Every user action gets an answer**, with no place of its own in
  `toast.rs`: a sentence built from the applied `Change`s ("Moved
  “a.txt” to Trash"), with Undo when the journal can take it back (of
  the plan it reports, `undo {"plan"}`: refused, saying why, once a plan
  asked after it touched what it did), Redo after an undo, Retry first
  for a plan a step's error stopped (ADR-080; a toast holds its buttons
  in order, `Toast::actions`). A plan that made one thing new (`Change::
  Create`: a PDF, an archive, a folder) in a folder no pane shows offers
  Show first (`toast::made_alone`, `navigate` with `select`): what it
  made in a folder shown is selected there instead (`Shell::made`).
- **What was found done is never said done.** An action that finds its
  work in place (an archive extracted again, a copy pasted where it is
  whole, a file trashed that is gone, a tag a file has, an undo of what
  was put back by hand) answers from the run's `tally`
  (`docs/invariants/operations.md`, "What a run found done"): all of it
  found, it says so, by name or count, as information and with no Undo
  ("“photos.zip” was already extracted to Downloads", "3 items were
  already in Docs", "Already undone: …", `toast::found_done`); some of
  it, after what was done ("Copied 3 items to Docs; 2 were already
  there", `toast::found_done_too`). What was made is offered to Show
  as when it is made. Never a question: Replace would make what is
  there again, and a folder that differs from what it would make is a
  name taken, asked about as one (ADR-054).
- **What runs is docked, never over the files** (`activity.rs`,
  ADR-114). Work under way is the window's state: a strip at the end of
  the last pane's footer, before Undo, says how far it has gone in as
  many words as the footer has room for (`Fit`: what and how, how, the
  share alone). Its buttons act on what it says: one action alone, its
  own Pause and Cancel; several under way or waiting, those stopping
  aside, the whole queue's Pause all (Resume all once every plan is
  paused; none while only tasks run) and Cancel all, named by its count
  (`activity::offers`, `ui.activity_pause_all`, `ui.activity_cancel_all`:
  what waits is cancelled first, so none starts as another ends). It
  counts the spell of work (ADR-114, amended 2026-11-03): from an action asked
  with nothing under way until nothing runs, waits or goes on by itself
  (`Work::settle`), each action counting one, those ended whole (a
  task too), those running by their own share, those waiting nothing
  ("2 of 3 · 47 % · about 3 min left · 1 waiting"). What counts as an
  action is one test, the core's, for what waits, the card and what
  ended (`Waiting::counts`, `ExecEvent::Started::counts`): several
  steps, or a heavy one; never a single rename. The bar never goes back
  as one ends: one cancelled before it did anything leaves the count,
  its share nothing, and one admitted and not yet told started counts
  as waiting, so the strip stays through a hand-over ("1 of 2 · 50 % ·
  1 waiting", or "1 waiting" before anything started). It goes back
  only as more is asked, or as an action stopped goes on, retried or by
  itself (`ExecEvent::Retried`, `GoesOn`): the same action, the run
  that stopped leaving what ended, counted again by what it has left.
  The time left is what runs, never a guess at what waits; what waits
  is a count the executor keeps cheap (`Executor::waiting_count`),
  never a look at what each waits for, which only Activity's list
  reads, and the strip only while it announces a waiting action or
  nothing else runs (below). "N waiting" counts those that count; Activity's Waiting
  heading says how many quick ones it lists beside them. Activity's
  header says the same over a bar of one segment an action, one bar
  past twelve; the taskbar and the presence follow it, with no share
  while only something waits and nothing ended, and no Pause with
  nothing to pause. **The strip is never silent** (ADR-114, amended
  again 2026-11-03): an action still under way 0.3 s after it joined,
  waiting or started, is shown and counted whatever its steps
  (`Work::watch`, `SHOWN_AFTER`: nothing quick flashes, nothing slow is
  silent); before its first step or byte it says "getting ready…"
  (`activity::getting_ready`), in the strip and on its card; while
  only something waits, the strip says what waits first and what for.
  **It announces** each action once at that moment: its own sentence
  on the selection's tint for 2 s (`ANNOUNCED_FOR`, the latest of
  several), then the strip's words as ever; its buttons hidden while
  it names another than the one they act on. The program is woken for
  the strip's own changes by one timer at a time, holding `Work`, not
  the window (`work::wake_after`), never a loop. A click, or `ui.activity` (Ctrl+Shift+J),
  opens Activity above it (one popup at a time, `toggle_popup`): each
  action's card under a header over the queue with Cancel all while
  several are under way or waiting, what waits (each with its Cancel,
  Cancel waiting for several), and Earlier, this session's answers the
  Undo history does not hold, with their Retry and Undo, twenty at most, in memory only:
  an action that failed, was cancelled or that no journal keeps, and
  every answer a toast says that is no action's (a print's, a message);
  an action done and kept by the journal is the History's alone
  (`Work::ended`); a message said again as the last one was is that one,
  moved to now (`activity::keep`). Its heading (its name, and what
  folds it) stays while its rows scroll beneath it, beside a scrollbar.
  **Activity unfolds for a long action** (ADR-114, amended 2026-11-07):
  once an action asked in this window has been under way, waiting or
  running, for `ui.activity_unfold_ms` (1.5 s by default, a slider in
  General's Actions), it stands above the last pane's footer, taking
  room rather than covering the files, never taking the keyboard, and
  stays for the spell of work, until nothing runs, waits or stopped
  (`Shell::sync_unfolded`, `Work::unfolds_in`; the program woken then,
  by the strip's one timer). An action is asked in the window that ran
  its command (`finishing::ran`, a reviewed proposal's too), else in the
  window in front as it joined (an undo, a retry, a script's; a task, a
  print). Quicker
  ones, and those of other windows, the strip says alone. Its chevron
  folds it into the strip (`ui.activity_pin`, remembered with the
  window's shape: `activity_folded`); the popup's pin, pressed while it
  is folded, unfolds it, standing at once with something to show.
  Standing, it is at most `TRAY_MOST` (260 px, about three cards; half
  a low pane) and scrolled beyond, and the strip steps aside; it shows
  the action an error stopped and folds the rest of Earlier at its foot,
  a line until unfolded (`ui.activity_earlier`). Open as a popup, it
  stays where it opened until closed. Standing,
  it is a place of its own (`keymap::ACTIVITY`): `ui.activity` gives it
  the keyboard, Escape gives it back to the files. A click on the strip
  or on Activity standing leaves the keyboard where it was, so the keys go
  on acting on the pane. What is under way is read once a frame
  (`Shell::under_way`, passed to the strip, Activity and the footer).
  The button the keyboard reaches in Activity scrolls into view, once,
  as it arrives (`activity::Reveal`). An action an error stopped stays in the strip, in the error's
  colour, until retried (from anywhere), Activity opened, or dismissed;
  only one that was a card there. Progress bars:
  [operations.md](operations.md). A card's bar moves by the plan's bytes
  where its total is known (sized before it began), its items counted
  beside; under the bar, quieter, its speed and time left as the core
  read them (`toast::pace`, ADR-113: never reckoned in the window),
  hidden while paused; into a mount that sends after, "then sent to the
  server". Cancelled, a card says it stops ("stopping at"), its bar
  muted, no speed nor time left, and its Cancel becomes "Stopping…",
  said, not offered (muted, no pointer, nothing to press, disabled for
  assistive technology, the focus kept), until the plan let go of what
  it held (operations.md, "Running"). Work that is no plan and nothing
  undoes (a print being made and sent) holds a task while it runs
  (`App::tasks`, `task::Task`): its card is drawn beside the plans', its
  title, how far it went and Cancel, gone when the task is dropped; a
  task cancelled from its card says it was not done, as no error.
- **The History** (`history.rs`, ADR-079, 13): the footer's Undo is
  split in two, its left part taking back the latest at once (as
  Ctrl+Z, off when there is nothing to undo), its chevron, a right click
  on Undo and `ui.undo_history` (Ctrl+Alt+Z) opening the History above
  it (one popup at a time). A chevron points where its menu opens: the
  toolbars' down, the footer's up (down while the History is open, to
  fold it back). Like the rest of the footer, the two are no
  stops of the Tab order: their keys are the commands', and a click on
  them leaves the keyboard where it was, as on the strip (Enter or Space
  would press Undo again); the History closed gives it back there. The
  History lists the core's rows (`Executor::history`), newest first,
  each with its own Undo, an undone one's Redo, and "Undo both" ("Undo
  with N after it") when the actions after it that block it can be
  undone with it. A button that cannot act now is said, not offered
  (`controls::inert_button`, no frame but the keyboard's), and the line
  under its title says why: the action that holds it is named by a link
  that lights that one's row and gives its Undo the keyboard. Earlier's
  and a toast's Undo answer the same way, by the plan they report
  (`history::undo_held`). The rows are read again only when
  `Executor::history_version` moves, once for the History, Earlier and
  the toast (`history::Read`, each row by the plans that made it), and
  only those in view are built (a `gpui::list`, which says its width,
  `history::WIDTH`, so a line that wraps counts in its height from the
  first frame: `views.md`, Scrolling); a name longer than the line wraps
  in it. The keys walk the rows
  read, so one out of view is reached too and comes into view. Undo,
  Redo and "Undo both" from it leave it open, its rows saying what each
  became; no toast is drawn while it is open, and one said meanwhile
  keeps its time, which begins once it is drawn.
- **One answer at a time** (`toast.rs`), over the files at the last
  pane's lower right, above the strip or Activity standing, where the list
  shows sizes and dates rather than names; over the preview on the whole
  screen. Not while Activity or the History is open: one said meanwhile
  is held back and drawn once it closes, for its time.
  A plan that ran
  5 s or more and moved bytes says them in its toast, and how long it
  ran ("4.8 GB in 3 min 20 s"). A toast stays at least as long as its
  words take to read. It names the file a step acts on as the user knows
  it: an item of a trash by the name it had, which the executor reads
  from its record before the step takes it away (`Progress::named`),
  never by the trash's (`a.txt.2`, `$R…`).
- **The desktop is told of long actions it was not watched for**
  (ADR-114, `skwir_platform::desktop`): an action that ran 5 s or more
  and finished or stopped, or one waiting for a server, while the window
  does not have the focus (`Shell::window_active`), is a notification,
  one at a time, replacing the last, withdrawn once the window has the
  focus or a waiting one goes on; a click brings the window forward and
  opens Activity. A cancelled one tells nothing, and nothing is undone
  from a notification. While something runs, the taskbar's entry shows
  how far, and the machine is kept awake unless all of it is paused
  (`ui.notify_desktop`, `ui.keep_awake`). Every call to the desktop
  returns at once; nothing is told from a headless run.
- **What runs outlives the window** (ADR-115, `finishing.rs`). What runs
  and what was said of it is `Work` (`work.rs`), the program's for its
  life (`Finishing`): the window's shell shares it and never moves it,
  so nothing it took goes with a window that closed or could not open.
  The program goes on once the last window closes while something keeps
  it (under way, paused, waiting for its server, stopped since, a
  question waiting). The close button closes, asking nothing;
  Quit asks while something is under way: Finish in the background (the
  default), Stop and quit, Keep the window (`Confirmed::Quit`). Without
  a window the program takes the executor's events in as the window
  did, tells the desktop every end, however short, shows its presence
  (`Desktop::presence`: the tray's or notification area's icon, else a
  notice that stays; the Dock on macOS); its panes sleep. A later launch,
  handed over, opens the windows again (ADR-122: the program takes
  launches over for its life). The window opened again
  shows `Work` as it stands. Closing, the window answers Cancel to what
  it was asking (`Shell::leave`), and an action stopped it showed is let
  go; from then on its shell takes nothing (`Shell::sync`), though gpui
  removes the window only later, in a task of its own: what the core
  sends meanwhile is the program's, what it asks waits for the next
  window. The core's wakes go through one loop for the program's life
  (`finishing::listen`), never one of the window's: a window opened
  again would miss what the closed one's loop took.
- `Shell::set_status` is a toast; `set_status_about` (core:
  `App::notify_about_setting`) links to the setting that fixes it.
  Toasts are drawn under the overlays, but over the preview on the whole
  screen, which covers the window (previews.md, "The whole screen").
- **Footers hold state, never messages** (`status.rs`). A pane's footer:
  counts, the selection's size, or with nothing selected what its files
  weigh (`weight.rs`: each file shown as the listing stat-ed it, else as
  a worker in the folder's lane stat-ed it once per listing, kept by its
  entry; a filter, hidden files shown or a file come adds up again and
  stats only what neither knows; a size learnt adds up again once the
  request running has answered; `PaneView::weighs` for a view that
  measures folders). What a listing's answer rests on is its state too:
  an index's "as of" (ADR-050), a search's "· N excluded"
  (`Listing::left_out`, ADR-125, its tooltip naming `excluded:yes`),
  both in what a screen reader is told. The window's state (Undo,
  pending paste, plugin counts via `tally::Tally`, proposals, the
  player's strip, what runs) ends the last pane's footer. Side panels
  have no footer.
- **A pane with only hidden entries says so** (`placeholder.rs`): its
  listing complete, nothing filtered out, hidden files off, it says "Only
  hidden files here" with Show hidden files (`toggle_hidden`) rather than
  what its place says empty (a tag's place listing a hidden folder alone,
  a folder of dot files).
- A key whose command needs a selection has `selection` in its `when`,
  so it does nothing rather than report "nothing selected".
- **Animation:** only a bounded entrance under 200 ms, plus the player's
  play/pause flash (`media::FLASH_MS`).
- **Empty is a state.** `placeholder.rs` says why a pane is empty
  (`LocationProvider::empty_state` for non-folders) or failed (the
  system's message as detail) and offers a way out, inside the pane so
  drops and the menu still work. The banner above the view says an error
  beside existing entries, or what the pane still waits for
  (`loading::message`), with the ways out of a wait, as the toolbar's
  buttons (`loading::Way`): Cancel for a location still opening (an
  address connecting), Go Back for one on a server that does not answer
  (Go Home where the pane has no Back), and Disconnect where the place
  holding that server offers Eject (its invocation, asked of the place:
  the banner names no plugin). Leaving a location stops what opens or
  lists it; the server stays connected.
- **A folder appears whole (ADR-120).** A pane goes to a folder at once
  (address, trail, an empty view, its files drawn meanwhile, below) and
  shows its entries together, sorted,
  once a worker has listed the whole folder (`Listing::filled_elsewhere`),
  however long it takes. While it is filled, the footer says nothing,
  then, with the banner after 200 ms, "Loading…" and how many entries
  were read (`Listing::filling_count`); the placeholder waits for the
  listing to end (a held listing is not complete). What streams
  (`LocationProvider::streams`: a search, the Trash, Nearby) shows as it
  comes. A refresh reads into the listing shown and never empties the
  view. Navigating, the address and the trail change at once and the
  files of the folder left stay drawn while the one gone to is filled
  (ADR-120, amended: `ListState::holds_left`), `tabs::HOLD` at most, as
  a tab left is: a folder ready within it takes their place with no empty
  frame between, one that is not shows empty, "Loading…" said in time.
  Any command ends the wait first (`Shell::admit_here`): what it acts on
  is the folder gone to. Their entries are the pane's to draw only
  (`Pane::drawn_entry`: the folder left's listing kept, its location
  closed, `Pane::drawn_left`), never what a command finds
  (`Pane::entry`). Whatever draws a row or a tile reads them so, its
  picture included (`Shell::thumb`: one made already is shown, none is
  asked for, its path being no location's now).
- **An order that waits for its reading says so** (ADR-121, amended):
  while the listing reads what its order needs of the entries
  (`Listing::reading`), the footer adds "Reading… N of M" once it has
  for 200 ms (`loading::State::reading`, a timer as for "Loading…",
  never a clock read while drawing), and the order before stays drawn.

## Properties and links

- **A section's rows are the lines of a paragraph** (`properties::LINE`,
  1.4, rather than gpui's default 1.618; a row 1 px above and below its
  line, for its hover): a value that wraps keeps the rows' pitch, and the
  sections stand apart under their rules and headings.
- A provider's fact can carry a command (`Detail::with_action`), a link
  run through `Shell::run`. Links show over the end of the row's last
  line while the pointer is on it, taking no room otherwise; those that
  are all a row says or that stop or start what runs always show, after
  the value ([previews.md](previews.md), "Header and Properties").
  Clicking the row copies the value; only the link acts.
- **Links (ADR-029).** A link written in a document answers a click. One
  found in raw text (`skwir_platform::links`: addresses by scheme, never a
  bare domain; paths in the terminal and those a file's text writes)
  answers Ctrl+click, drawn as a link
  only while Ctrl is held over it (`Run::found`, `Shell::armed_link`);
  Shift adds a tab.
- Addresses open only through `ui.open_url`; files and folders only
  through `ui.reveal` (with `line`, the preview shows it), never opened.
- Ctrl+Shift+O labels the links in view (`links.rs`).
- A terminal path resolves against the folder its line was printed in,
  stamped into the grid by the plugin (`plugins/terminal/src/links.rs`).
  Stamp new kinds of found links there too, never resolve them at click.
- A path found in text is looked up only through `links::resolve`, which
  asks `fs::volume::Here` first: never a server named by the path
  (`\\host\share`) nor below an automount point not mounted yet
  (`/net/host`), since the lookup alone reaches that host (ADR-029).

## The sidebar

- **Icons.** A place has a theme icon name. An icon in KDE's color
  scheme (its `current-color-scheme` stylesheet: most of Breeze) names
  its colors by their part and is drawn in full color with them filled
  from the theme (`icons::Pic::Scheme`, `icons::Ink`: its text in the
  color text is drawn in there, its accent and states the theme's), as
  KDE's icon loader fills them, never flattened into one color: a large
  folder's body is the accent, its flap and shadow laid over it. Each
  raster is kept by its ink; one being made in new colors (a palette, a
  chosen row) draws the file as it was last drawn, never a blank. What
  was drawn in colors the theme no longer has is given back once they
  are settled, not while Colors from's list shows palettes, and only the
  colors an ink reads count (`icons::colors_now`, `icons::InkColors`).
  A symbolic icon (GTK's `-symbolic`) is a shape
  drawn tinted (`icons::Pic::Shape`); every SVG is rasterized for the
  pixels it covers (`icons::element`). The theme is `ui.icon_theme` while
  installed, else the desktop's, changed only through
  `IconCache::set_theme`. On macOS and Windows the name maps to a system
  icon (`system_icons::for_name`).
- **Place fields:** `space` (filled by a worker: asking a volume can
  block), `eject`, `unavailable` (dims the row; the reason is its
  tooltip), `busy` ("Unlocking…": a still hourglass, nothing to click
  until done), `menu` (its own items). A rescan publishes at once and
  keeps the last measurements (`carry_over`), so nothing blinks. How
  full a volume is changes with no rescan to follow it, so its owner asks
  again when a window comes to the front (`Plugin::on_looked_at`), when
  a plan ends, and, for this machine's disks, every minute while a
  window has the keyboard (`App::looked_at`; a disk's filesystem answers
  from memory, so this wakes no drive); a remote's in its lane, never on
  a timer (`remote.md`). The places are published again only when a
  number changed.
- **A tag looks the same wherever it is shown:** its chip, a dot and a
  tint of its color (`tags::chip`): on files' rows and tiles, in
  Properties and the preview's header, in the sidebar
  (`tags::place_chip`, a place with a `color`; chips wrap together,
  `sidebar::wrap_pills`), in the address's trail, and in menus (an item
  with a `MenuItem::color`: the Tags submenu, the Tags section's menu).
  The one shown is filled deeper (`Theme::tag_shown`). A tag the sidebar
  only suggests keeps its dot without the tint: an outline in a tag's
  color means the files without it (a narrowing's pill, `toolbar::Tint`),
  never a fainter tag.
- **Pointed at, a place rings its files where they are.** A place whose
  location says tags of what it lists (a tag's pill, a saved search for
  one: `LocationProvider::tags_shown`) rings the entries the pane shows
  that have them or a tag within them, in the tag's color around them
  and its tint under them (`Shell::ring_tags_of`, `Look::ring`, drawn by
  `entry::highlight` and the board's cards), until it is left. Asked of
  the location's text, never of a plugin: a new kind of place that says
  tags rings its files with no branch here.
- The sidebar repeats nothing its location says.
- **Keeping.** Ctrl+D and the pane's keep button run the core `keep`
  command, which keeps what the pane shows as it shows it (ADR-109): the
  location and the pane's filter go to the `Keeper` that keeps them, the
  one of the location's scheme when nothing filters it (bookmarks, saved
  searches, pinned tags), the one that keeps any location shown filtered
  otherwise (`Keeper::filtered`: the query plugin's saved places); with
  none of those, the location's own, which keeps it plain and is handed
  the filter left out as `unkept` for its toast to say (`App::keeper_of`).
  The button is filled only for the place as shown, filter included
  (`App::kept_place`, `Place::is_kept_as`: a remote folder by its
  address). A kept place carries `forget` (the button runs
  it while shown), `reorder`, and how it opens (`Place::opens`: as kept,
  or with the pane's lens). A new kind of kept place registers a keeper
  and fills those fields; no UI branch.
- **Bookmarks** are edited only through `BookmarkEdit`
  (`skwir_platform::places`); each edit returns its inverse, offered as
  Undo/Redo (`App::notify_undoable`, as for any edit outside a plan).
  Other applications' lines are kept; the file is watched. A folder
  dropped beside the rows is bookmarked (`places.bookmark`, GTK
  bookmarks).
- Files the desktop shares (bookmarks, `mimeapps.list`) are rewritten
  only through `skwir_platform::file::rewrite`, by an edit returning its
  inverse.
- **Tags in the sidebar:** pinned tags (`tags.pin`, ordered by dragging,
  unpinned by the keep button), else suggestions from the index
  (`Place::suggested`, their dots without the tint), then All Tags
  (`tags:/`). A home tag
  folder, not narrowed, is kept as its pin (`Keeper::keeps`).
  Shift+click adds the tag to the pane's lens (files that also have it,
  ADR-107); Alt+click shows its
  files in the folder shown (`tags.pill`). The tag editor offers pinned
  tags first.
- **Drops:** a place takes what its location takes (`Verbs::drop`); one
  that takes nothing (Recent, a search) swallows the drop, never
  bookmarking it. Under files dragged, the sidebar is marked only where a
  drop bookmarks (beside the places), a place alone where it is the
  destination (`dnd::marked_alone`). A place dragged onto a file's row
  (`dnd::takes_gifts`) gives that file, or the selection containing it,
  what it takes (`takes_gift_for`).
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
  (`sidebar::mover`); `arrange` is the one reading of all this. Within a
  group, what is no folder comes after the folders (a server the tools
  know after the mounts) unless it moves: what moves keeps its owner's
  order (a remote folder's bookmark among the folders', as the file has
  them).
- A group with every place hidden keeps its header, whose menu lists
  them; places offered hidden (`Place::hidden`) appear there unchecked.
- Rename… (`Place::rename`, F2) and Remove From Sidebar
  (`Place::forget`) are the sidebar's own items, never the owner's.
- F6 (`ui.focus_next_part`) goes round the parts of the window shown,
  as in other applications: the sidebar, the files, the preview (what it
  shows takes the keys, `Shell::shown_focus`), a part not shown passed.
  Forward only: Shift+F6 is the split's. The terminal is not one of them:
  its programs have F6, and F4 takes the keyboard in and out of it.
  `ui.focus_sidebar` (the palette's) brings a sidebar put away back.
- In the sidebar (`Sidebar` context, `ui.sidebar_key`): arrows walk,
  Enter opens and returns the keyboard, Menu, Alt+arrows move, F2,
  Delete, Escape goes back to the files. A click never takes the
  keyboard (`prevent_default`).
- **Tabs in the sidebar** (`ui.tabs_at`, `sidebar::TABS`): the first
  group, drawn while the tabs are listed: a row per tab
  (`tabs::sidebar_rows`), then New tab. It folds as any group but is not
  moved or put away; the setting takes it away. Its rows take the
  sidebar's keys (`Item::Tab`: Enter shows the tab, Delete closes it,
  Menu opens its menu, Alt+arrows move it; `Item::NewTab`), drops into
  the folder shown, a drag along them (a line above or below the row),
  the middle button and the right button, as in the strip.
- Volumes: [operations.md](operations.md).

## Asking the user

A plugin needing text dispatches `ui.ask` with the command to complete;
one needing a choice dispatches `ui.choose` with its choices or a command
listing them on a worker (`choose.rs`; `option` adds a flag such as
"Always use for…"). Never a prompt or picker of its own.

- **A file to write** in a form (`form::Kind::Save`, `form/save.rs`) is
  one line, folder and name, completed as the address field completes a
  path (`complete.rs`: the folder typed read once on a worker in its
  lane, each key matched in memory), the asker's folders listed only
  when asked for (its button, Down while folded: `FormState::offered`).
  What it holds is read one way, by the window and by its asker alike
  (`form::path`: `~`, `..` as `cd` takes it, a name joined to a folder).
  A folder is known once it answered (`Reads`): a read let go of for the
  next one, or one that could not tell (no right to list it, a server
  that failed: `Resolved::unread`), says nothing against the folder and
  is asked again.
  Its list is a popover (deferred over the form, scrolling with its
  scroll bar, the row highlighted kept in view), folded once the field
  loses the keyboard but to its own button, which sits inside the field
  at its end (`inner_chevron`, as for any choice that takes text: up
  while the list shows). A folder chosen keeps the name; Enter takes
  the text unless a row was gone into with the arrows
  (`FormState::typed`, as for any choice that takes text); Tab does as
  the address field's; what the window puts there is an edit Ctrl+Z
  takes back, no typing (`put`). Its name follows the form's fields
  until one is typed, and comes back when it is left with a folder
  alone. A path no file can be saved at (a folder that is none or
  relative, a name the system refuses) is said under it in the error's
  color, its frame too, and holds the form's answer back (`FormState::blocked`: the answer's button drawn
  inert, Enter refused); a name already there is said, not held back.
  Never a folder picker of its own beside it.
- **A form's lists fold on a press elsewhere in it** (`form_fold`, the
  panel's inner frame; the field unfolded and its popover keep theirs),
  and on Escape. **A choice clicked that shows a field to type in** (Pages…,
  a PDF's Save as) gives that field the keyboard (`form_row_click`); the
  arrows on a row of choices move no keyboard, so Left back to Printer
  stays on the row.
- **One text field** (`input.rs`), for every field: a rename, the
  address, a form's, a find. A line longer than the field scrolls to
  keep the caret in view (`scrolled`, in prepaint). A drag selects
  wherever the pointer goes in the window, not only over the field,
  along the line alone (straying above or below never jumps to an end);
  held past an end, it selects on and the line scrolls, faster the
  further out (`autoscroll_x`, a timer that lives only while it is held
  there), until the pointer comes back, the button is let go or the text
  ends.

- **A click answers what a key answers.** A dialog that waits for an
  answer ends with `modal::footer`: what its keys do, muted, then Cancel
  and the answer, named by its verb (Rename, Unlock, Delete; `ui.ask`
  takes it as `accept`, OK when absent); the buttons go under the hint
  when beside it they would leave it less than `modal::HINT_LEAST`. A
  picker's rows are its buttons.
  A press outside cancels too, but is never the only way: what is only
  read (About, F1's overview) closes from its ✕, as Settings does.
- **A dialog keeps the keyboard.** Tab walks its field, Cancel and its
  answer, round their ends (`Shell::walk_dialog`, `modal::tab_among`),
  never the window behind it; a list one picks from keeps it in its
  field. A focused footer button is pressed by Enter and Space, before
  the dialog's own Enter (a focused Cancel cancels): a footer holds only
  `modal::Pressed` controls; a press on one
  leaves the keyboard in the field; Escape that nothing in the dialog
  answered cancels it (`modal::modal`).
- **A form is described, never drawn, by its plugin** (`skwir_core::form`,
  `form.rs`, ADR-086): fields of text, numbers, secrets, switches,
  choices, picks, and rows of a few choices side by side (`Kind::Row`: a
  radio group, segments, or cards when a choice has a picture). A
  picture is marks the plugin places (`form::Picture`: boxes and text),
  drawn in the window's tokens as on paper, never colors or an image of
  the plugin's. A field shown only while another holds a value
  (`Field::when`) is neither walked by Tab nor refused while hidden; a
  choice may name the answer's button while chosen (Print…, Save). A
  row has the keyboard as one: Up and Down, or Space, change its choice,
  Enter answers the form as from a field.
- **A question names what it is about when asked.** `ui.ask` fills the
  asked command's `pane`, `entries` (the selection) and `in` (the folder
  shown) that the asker left out, as they are when it opens: Rename
  renames the file it was opened for, whatever is selected by the
  answer.
- **A question before deleting lists what goes** (`deleting.rs`): each
  item as the user knows it (an item of a trash by its own name, from
  where it came, read from its record), what it is, what it weighs or a
  folder holds, and whether companion files go with it: a mark after its
  name, "+ metadata" while the whole name fits beside it, else "+"
  (`AdaptiveRow`), which says where they are when pointed at. The name
  has the room first, a companion's name never cuts it, and too long it
  is cut in its middle, its extension kept, as the views cut names. Its
  folder on each row only when they are not all in one, which the line
  under the list says once with what they hold in all. The rows come
  from the plan's paths at once and never wait for the disk: a worker
  reads the rest, then counts each item as deleting it takes it
  (`fs::count::count_each`: a link is itself, a file of several names
  counts once), handing over what it has as it goes, stopped when the
  question closes. The question's height is set when it opens.
- **What a dialog lists scrolls, all of it** (`modal::Rows`): a box of
  rows of one height, a set number in sight, only those on screen built,
  scrolled by the wheel and by Up, Down and the page keys
  (`ui.list_move`) while the dialog has the keyboard; never cut at a
  count, nor clipped unsaid. What a deletion takes, a proposal's steps
  (each line cut in its middle, so it ends with what it comes to) and
  what a question to allow says.
- **A proposal says once the folder its steps share** ("In ~/Projects"),
  and its lines say their paths from there (`Change::within`: `rename
  a.txt -> b.txt`, `move c.txt -> archive/c.txt`); a path outside it
  stays whole. The folder is the deepest holding each path's own folder
  (a step on a folder is said from the one above), none at a drive's
  root. A `Change` said alone (`Display`, what agents read) keeps every
  path whole.

## Keys

- **F1** (`shortcuts.rs`, ADR-100) lists the bindings of the focused
  element's key contexts; a new context needs only a name in
  `keymap::context_label`. One row per action, its keys side by side
  whichever place each works in; a family's actions (`Form::family`) on
  one row, the modifiers they all hold said once. Rows stand under their
  command's topic (`CommandSpec::topic`), the topics of the place nearest
  the focus first, and flow down as many columns as fit
  (`elements::columns`: whole topics, a row stacked only where it does
  not fit its column, all counted from one measure of each row, never
  from a size read in `render`); what does not fit scrolls under the
  overlay scrollbar. Its field has the keyboard when it opens:
  what is typed narrows the list and finds the bindings that do not run
  here too ("Elsewhere", each under its place and condition). Escape
  empties the field, then closes. **All shortcuts…** opens the Keyboard
  page with what was typed (`ui.settings { page, filter }`).
- **One way to find a shortcut** (`keymap::finds`), for F1, the Keyboard
  page's filter and the Settings search: every word in what the action
  does, its place (never its conditions: "not in the trash" is no answer
  to "trash") or its command; or, when the text names keys (a `+`, a
  modifier, a named key alone: `ctrl t`, `Shift+Delete`, `F2`), a
  binding holding them all. A letter alone is a word. A command run three times with the pointer gets
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
- **A key held down runs again only a step (ADR-066).** The system's
  repeats of a key reach the window's root first
  (`Shell::capture_run_command`), which drops a command that does not
  repeat by its declaration (`CommandSpec::repeats`); a command that
  steps (the cursor, a page, the volume) declares it where it is
  declared (the shell's in `commands::repeats_of`). A switch never
  repeats: a held key would flip it over and over. Whether a key is held
  is gpui's `Window::key_down_event().is_held` (patch 0032), never a
  timer of ours. `keymap::tests::held_keys_repeat_only_steps` holds that
  every built-in key that moves repeats.
- **Peek's key, held, peeks until let go.** `ui.peek` run by a key's
  press keeps the key (`preview::PeekPress`): a tap leaves peek open; a
  press the system repeated closes it when let go (the root's key-up),
  unless peek was used meanwhile (a command other than `ui.cursor`, a
  click in it). Opening or closing peek any other way forgets the press.
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
- **The Exclusions page** (ADR-125) is the exclusions' catalog as data
  (`settings_exclusions.rs`: rows for a role, each edit from one catalog to
  the next, what it takes from agents) drawn in one list
  (`settings_exclusions_view.rs`), walked in its own place
  (`setting_exclusions`, `ui.settings_exclusion`). It reads a folder's syntax,
  checks an entry and judges what a change takes from agents only
  through core (`exclusions::Written`, `Matcher::check`,
  `Matcher::one_path`, `Matcher::looked_up`, `exclusions::here`,
  `exclusions::exposed`, which
  `settings.apply` refuses to an agent): never a reading of its own. It
  saves both tables in one change (`change_drawn`). Whether a folder is
  on this computer is looked up on a worker (`exclusions::here`: a path
  once, a glob a level at a time, a remote's taken as there, never
  read); a name, a `**` or a regex always shows.
  Anything that lets agents read a folder asks first, Keep private
  focused and Escape's answer; nothing else asks, a removal included.
  Every category is folded when the page opens (the folds are the
  dialog's, never saved), but what a filter typed or Changed shows, the
  Secrets opened from Privacy (`role: agents`), and the category of what
  was just shown, added, edited or moved. A change is said at the page's
  foot, a polite live region, with no Undo: a tick or a switch undoes
  itself, as on every page. A row's menu is the window's context menu
  (`Shell::open_settings_menu`, `MenuHost::Settings`), drawn above the
  dialog, which lets the menu's keys through while it shows
  (`takes_now`); its items run `ui.settings_exclusion_item`, the user's
  alone, its Move to a submenu. From the keyboard it hangs from the
  cursor's row, which the page draws the anchor of; closed, the keyboard
  is back on the list. The page knows it is open by what the menu holds
  (`SettingsMenu::held`), never by a flag of its own. The Searches role
  is said "Searches and tags" wherever the page names it. A press on a
  category's heading folds or unfolds it, as Left and Right do; only the
  switch at its end turns the category on or off for the role, as Space
  and Enter do on the heading. The page is called Exclusions where it is
  said (its title; the words come from one place each,
  `settings_exclusions::ENTRY`, `ENTRIES`, `EXCLUDED_FROM`, `excluded_from`),
  what an entry names "files and folders", never "folders" for both.
- Type-ahead is on only while no binding takes an unmodified character in
  the file list (`Keymap::typeahead`).
- **A name typed** (`list.rs`, `Typed`) goes on until a pause of
  `TYPING_PAUSE` or another listing shown, and goes forward:
  - A new name, and one letter typed again and again ("d d d", which
    looks for "d", never "ddd"), look past the entry under the cursor, so
    each press reaches the next name; a name typed on stays on the entry
    while its name still begins so. Round the end after the last.
  - A name begins with it as the order folds names
    (`collate::Prefix`: case, the accents of Latin letters), so "ecole"
    reaches "École", which sorts beside it.
  - One walk of the order shown per character (`seek_from`), no index
    made: a folder streaming changes its order at every slice, and the
    name is looked for again as it brings names (`seek_typed_again`).
  - The entry reached is brought to the middle of the view
    (`PaneView::scroll_to_middle`), unless it is wholly in view.
  - **Space** goes on with a name only when a name in the listing goes
    on so ("my" then Space reaching "My file.txt"); asked before its
    binding runs (the pane's capture of `RunCommand`), so otherwise Peek
    opens as ever. Space begins no name.
  - While a name goes on, a pill at the foot of the pane shows what
    names must begin with and a caret (a space typed shows), and "No
    match" once the listing, whole, has none. One redraw takes it away
    when the pause ends; nothing is drawn per frame for it.
- A key that types a character hides the pointer until the mouse moves
  (`CursorHideMode::OnTyping`, set at startup in `lib.rs`). A key that
  only moves, such as an arrow, leaves it. The fullscreen preview hides
  it with its controls.
- Multi-stroke bindings wait without a timeout (vendored patch 0010);
  `sequence.rs` shows what can follow.
- A plugin that changes standard keys is off until asked
  (`Plugin::enabled_by_default`). A scheme key meaning a standard gesture
  says so (`keys.like`: vim's `d` is Delete) and takes that key's bindings
  and conditions, never a copy.
- Notation and `when` tokens: `AGENTS.md`, "Conventions".

## Tabs (ADR-018)

- Tabs are core data (`tab.*` commands); a tab holds one pane or two.
- **How a tab is drawn** (`tabs.rs`, one `Label` for the strip and the
  sidebar). In the strip, a pill floating in the row: the tab shown a
  card in the panes' ground (`bg`, a `border` line, no shadow), the
  others their names alone until pointed at (`hover`). In the sidebar, a
  row like a place's, the tab shown the same card, never the selection's
  color: that says which place the pane shows.
- A tab's × is on the tab shown and the one under the pointer
  (`Shell::tab_hovered`, `tabs::closes`), never on a lone tab.
- A tab is named like the place it shows (`App::location_title`). Split,
  it is named by both panes, in the order they are laid out, a line
  between them; the pane with the keys in the text's color, the other
  `text_muted`, so the names stay put when the keys move. Its icon is
  the split's, as the Split button draws it for that tab's arrangement
  (`Shell::tab_stacked`). Short of room, the name with more letters
  gives way and the other keeps up to half (`tabs::gives_way`). Said
  aloud and dragged, the names are joined ("Field Guide and Maps"); the
  tooltip has each pane's location.
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
  (`core.last_windows`, each window's); only each window's shown tab
  opens at startup. Each pane comes back showing its location filtered
  as it was (`SavedTab::filters`, read again by the filter's reader,
  ADR-109) and sorted as it was (`SavedTab::sorts`: its criterion and
  direction, its groups and theirs, hidden files; folders first stays
  the user's, `Workspace::push_pane`); its history is not kept, nor its
  groups folded. A sort written only where a pane's differs from a new
  one's; one that cannot be read (a later Skwir's criterion) is a new
  pane's, never a session lost.

## Windows (ADR-122)

- **One program, many windows.** The core holds the windows
  (`Workspace::windows`, `WindowTabs`): each its tabs and the tab it
  shows; one is active, where a command naming no pane acts. A gpui
  window draws one core window (`Shell::window_id`); `finishing.rs` is
  the program, holding every window, and follows the core's
  (`follow_windows`): a window the core made opens (`window.new`, a tab
  moved out, a window reopened), one it closed goes, one a command made
  active comes forward. Never open or close a gpui window for a core
  window by hand: change the core and let the program follow.
- **The program takes, each window draws.** A wake is taken once
  (`finishing::woken`): what the executor says of plans goes to the
  window that ran each one (`finishing::ran`, recorded by `Shell::run`),
  else to the active window; a window's command asked from elsewhere
  (`request_front`) to the window of the pane it names, else the active;
  notifications, reviews, questions, clashes, secrets and forms to the
  active window; then each window syncs (`Shell::sync`, which takes its
  `inbox`). The desktop is told once for the program (looked at while one
  window has the focus), and what it asks (a notice clicked, Pause all)
  is answered there. A shell never takes a program-wide queue of the
  core's.
- **What is one, what is each window's.** One for the program: the
  queue, the History and Undo, Activity's work (`Work`), the shelf, cut
  files, settings, the keymap, the player, the agents' channel, the
  presence while no window is up. One Settings dialog: asked in another
  window, the one open comes forward (`finishing::settings_elsewhere`).
  Each window: its tabs, split, sidebar, preview, terminal panel, address
  field, palette, toasts, its shape (`Workspace::set_shape`, kept with
  its tabs), its thumbnails' frames (`Shell::thumb_asker`: a window
  drawing never makes another's requests stale).
- **One plays at a time**: whatever starts playing in a window (a
  button, a key, the dock, the desktop's controls, the next track)
  claims the player (`finishing::claim_player`): at once from the
  panel's Play (`Shell::media_started`), else as its session says it
  plays (`SeenPlaying::window`, followed from the sessions' state, never
  from what was asked, so the updates before a pause took are no new
  start). The window that played before pauses where it is, pin and
  panel (`Shell::pause_playing`), and plays on from there when started
  again. The desktop's controls steer, and are told what plays by, the
  window that started playing last (`finishing::steers_player`,
  `publish_now_playing`): another's paused track never covers it.
  Closing a window stops what it plays.
- **A pane is a window's.** `Shell::pane` and the panes it draws are its
  window's; this window's active pane is `Shell::window_pane`, never
  `Workspace::active_id` (the active window's). A tab moved to another
  window is drawn there anew, carrying what its panes show their files
  as (`Carried`: the view, Quick filters, the timeline), given before any
  window follows (`give_away_moved`) or as its own sync lets it go
  (`sync_tabs`): a `PaneUi` is made in its window and never moves.
- **What the user does in a window acts in it.** `Shell::run` and
  `run_args` pass one gate, `admit`, which makes this window the core's
  active one before the command runs: an agent's or a script's
  `window.select`, `tab.move` or `window.new` moves the active window
  without moving the focus, and a key pressed here is not theirs to
  redirect. What is asked from elsewhere (`Inbox::front`) goes through
  `admit_here`, and leaves the active window as it is.
- **The active window is the one used last** (ADR-122): the core keeps
  the windows in the order they were used (`Workspace::used`), so closing
  the active one goes to the one used before it. That change is the
  close's, not a command's: `finishing::follow_windows` does not bring it
  forward (the compositor gives the focus where it goes).
- **What a window closing holds is handed on.** Reviews, questions,
  clashes, secrets and forms, shown or waiting their turn, and commands
  waiting for a dialog go to the next window up (`Shell::take_held`,
  `finishing::hand_on`, taken at the next `deliver`) and are asked again
  there. Answered Cancel only when the last window leaves.
- **What names no window goes to one that is up** (`finishing::front_of`:
  the core's active window while it is up, else the one opened last). A
  window that cannot open (`open_window`) is taken out of the core, so no
  ghost stays active.
- **Opening and closing.** Ctrl+N (`window.new`) opens a window on the
  folder shown, shaped as its window (unsplit); Open in New Window on a
  folder (`window.open`) or a sidebar place (`sidebar::open_in_window`),
  Move to New Window on a tab of a window with more than one. Closing a
  window (its button, Ctrl+Shift+W, `ui.close_window`; `tab.close` of its
  last tab run in the window, Ctrl+W, `Shell::closes_last_window`, the
  last window included) with others up
  closes the core's window, its tabs kept to be reopened together
  (Ctrl+Shift+T); the last one leaves as ADR-115 says. Quit closes every
  window (`Shell::close_every_window`), none forgotten.
- **A tab is not dragged between windows** (ADR-122, Rejected): a tab's
  drag moves it along its own window's tabs; Move to New Window and
  `tab.move` with a `window` carry it elsewhere, on every system alike.
- **Remembered as they change**: the program records every window's tabs
  and shape for the next start (`finishing::record_windows`, at most
  every 2 s, saved on a worker), so a window closed alone is forgotten
  and a crash loses none. A new window takes its own shape (restored,
  reopened), else its source window's, else `ui.session`, the shape of
  the window used last.
- **Where a window was and how big** is in its shape
  (`session::Placement`, ADR-122 point 7): its size as a window,
  whether it was maximized, and its place with the display it was on (a
  uuid and the display's bounds, in the coordinates gpui gives both on
  that system). `Shell::window_moved` follows the bounds and remembers
  them once they settle (500 ms), recording the windows itself: a move
  wakes nothing. Maximized or on the whole screen, a window keeps the
  windowed size and place it had before (`Placement::of`): X11 says only
  the bounds it has. `Placement::opening` places a window: its place
  while its display is there as it was, else centered on the main
  display, its size fitted; a step from the window it comes from
  (`finishing::CASCADE`). Wayland ignores a place asked.
- **Launches come to the running program** (`skwir_platform::handover`,
  taken over from start to quit, `finishing::take_over`): only a launch
  of the same build, settings and state files, and display
  (`handover::Instance::here`, which keys the bus name, the marker and
  the pipe), so a test's window with folders of its own never opens in
  the user's; `--new-instance` keeps a launch apart
  (`handover::keep_apart`). With windows up a launch opens a window on
  its folder (`App::open_window`: as it is, no pane's lens, shaped as the
  window used last), else, with `ui.launch_opens` = `tab`, a tab of the
  window in use, a bare launch then bringing that window forward
  (`finishing::reopen`). With none up, the windows come back (ADR-115).
  Two launches at once both find no one there: the one refused the name
  (`NotTaken::Elsewhere`) hands itself to the one that holds it and ends
  (`handed_away`), never a second program. Taking over is asked for before
  anything is shown, and stops only once no launch waits
  (`TakingOver::close_unless`, `finishing::end_unless_handed`).
  macOS: `on_open_urls` hands the Finder's and the Dock's openings in;
  a launch from a terminal is a program of its own there.
- **A wake draws the windows it concerns** (`finishing::woken`): the
  listings are pumped once for all the windows, then a window syncs and
  draws if the wake was for all (`App::wake`, an
  unnamed `waker_handle`: the queue, the tasks, settings, plugins), named
  it (`App::window_waker`, `Shell::waker`: its media sessions, previews,
  readers, terminal, map tiles; `Thumbnails::wake_asker_with`: a picture
  it asked for), or named a pane in it (`App::pane_waker`: the pane's
  listings, kept and opened in place included), or if the pump moved a
  pane in it (`PumpReport::panes_changed`, `Shell::take_pumped`), its
  tab wherever it is now. A thumbnail asked by two windows wakes both
  (`Flight::askers`), and stays asked until both have moved on. What
  answers one question (`latest::Latest`) wakes every window, once. A
  window with something taken for it (`Shell::has_to_sync`) syncs too; a
  window a wake does not concern otherwise takes it in at its own next
  draw (`render` syncs): so a wake that must act in a window not on
  screen (a media key) names that window or names none.

## What the window remembers

- A window's shape (where it was and how big, view, split and share, sidebar width and sections,
  preview place and size, archive-member placement, Properties' folds,
  the types shown as their source, the active pane's Quick filters and
  timeline, what it measures and counts, which every pane starts with,
  zoom, Usage's levels and mode, column widths) is application data, not
  preference: `session.rs` keeps it with the window's tabs
  (`Workspace::set_shape`) and, for the window used last, under one key
  (`ui.session`) that a new window without another to take from starts
  with, written only when it changes. A new pane opens in the current
  pane's view. The windows' tabs, each pane's location and filter, are
  remembered with `core.startup = last` (Tabs and Windows, above).
- **Edges** dragged to size an area are one element (`elements/edge.rs`):
  double click restores the default; shared room via `edge::share`,
  which holds the edge where the areas ship (a split's middle) while the
  pointer is within `edge::SNAP` px of it, a magnet. An
  area that grows by the pointer's travel (a list column, a table
  preview's) measures it from where the edge was pressed
  (`Shell::edge_pressed`), never from the first move reported: gpui
  starts a drag on a move past 2 px and reports moves only after it.
- The preview panel's width is bounded by what the panes and sidebar
  need (`preview::max_width`, from the window's width, no fixed cap); a
  small window draws it narrower without rewriting the remembered width.
  The same at the bottom (`preview::max_height`). The pinned player's dock
  is bounded by the panel (`Shell::dock_height_limit`); until the edge is
  dragged, the default height adds the dock's.
- The sidebar's width is bounded likewise (`sidebar::max_width`), and the
  sidebar hides below a window width computed in `Shell::render`.
- The split edge stores a share, not pixels (the panes' flex grow); the
  drag reads the container bounds from `DragMoveEvent`. Brought near the
  middle, the edge goes there (`edge::share`'s magnet), so the panes are
  even again by hand. Each pane keeps
  `PANE_KEEP` across, `PANE_KEEP_HEIGHT` stacked. The share belongs to its
  tab (`tab_splits`, swapped in `Shell::present`); a tab gaining or losing
  its split, by any route, starts even again (`sync_tabs`);
  only the shown tab's share is remembered.

## The timeline strip (`timeline.rs`)

- Each pane has its own (`PaneUi::timeline`), under its files, shown or
  put away from the View menu or Ctrl+Shift+L (`ui.timeline`), acting on
  the active pane: it never moves to another pane, and a split shows two
  places' dates side by side. What it measures and the time it counts
  are its own; a pane made from another takes them (`TimelineState::like`).
  A pane gone dormant counts again when shown.
- The width pressed is the width shown (`Histogram::bucket`). Unpicked,
  the bars are the finest that show every date (`fits`); a width picked
  holds while the pane shows what it showed then (a `Zoom` ends with
  `location_lifetime`), so each place opens on its whole span. A width
  finer than `fits` is a window of `MAX_BARS` bars, moved by the wheel
  over the bars and the arrows (`ui.timeline_pan`), anchored to a bar's
  instant (`from`, `until`) the plugin places; a width picked over a
  moved window ends where it did.
- Dates far from all the others (`outside`) never decide the width: the
  plugin leaves out strays (a hundredth of the files at most, further
  from the rest than it spans and by more than a year).
- The bars are one element (`elements/bars.rs`): labels, ticks and counts
  are chosen in prepaint from each bar's rank, the finest level whose
  labels all have room. Bars are dated on the plugin's calendar
  (`Calendar::system`), so a label is the bar's period.
- A bar's click, or a drag across bars, shows their period in one term
  of the time counted (`modified:`, `created:`, `accessed:`, `taken:`;
  `timeline.select`). Over a folder counted alone, by a time its listing
  knows, it is the pane's lens (`query.narrow`), as Quick filters' year
  is: kept as the pane goes until taken off, a bar again in place of the
  one before. Over the subtree, or by pictures' clocks, which a lens
  cannot decide, it is a search, which Back leaves; in a search, it
  refines that term (`query.refine`), so a bar of a bar's files is a
  click away. A folder's bars count its own files: its lens shows the
  folders inside whatever it keeps, so a bar counts what it shows. Only
  a folder's subtree is walked: a search lists what it lists.
- What the strip keeps of a visit (`timeline::Visit`, ending with
  `location_lifetime`): the width picked, its window, the keys' bar and
  where a range began, bars kept by their first instant so a count again
  or a moved window finds them. What it measures (files, size) and the
  time it counts are the window's, remembered (`session.rs`).
- The selection is the plugin's to count (`Bar::selected`): the pane's
  selection revision is a recount's, throttled with the dates
  (`RECOUNT`). A picture's clock (`keys::TAKEN`) is read in the plugin's
  background count, as a subtree is walked (`Counted::Listing`), through
  the core's fields; a count again of the same visit shows the last one
  meanwhile.
- The bars take the keyboard (`keymap::TIMELINE`, in F6's round between
  the files and the preview); its keys are steps that repeat when held.
  To a screen reader the chart is a list box whose bars are elements of
  no size built under `shell.a11y`, the keys' one claiming the active
  descendant: gpui moves the focus only to a real element's claim, never
  to a synthetic child's.

## The terminal panel (ADR-019)

- It draws core's `terminal::Screen` and sends `terminal::Input`; the VT
  engine (`alacritty_terminal`) stays inside the `terminal` plugin.
- It is written in the code font at Code's size (`Sizes::body(Part::Code)`),
  the size the code view writes its code at: the desktop's monospace size
  at 100 %, as the desktop's own terminal is. Never a size of its own
  scaled beside it: a 13 px from before code followed the desktop
  (ADR-117) left it 7 % smaller than the code until 2026-11-07.
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
- Paths reach the prompt quoted by the session for its shell
  (`TerminalSession::quote`, the family `cd` uses), as a paste, never
  with an Enter: files dropped on the panel, the selection with
  Ctrl+Shift+Enter (`ui.terminal_paths`, from the files or the
  terminal), or files on the clipboard pasted (ADR-062: their text, a
  path a line, would run line by line in a shell that takes no
  bracketed paste). A shell whose quoting is not known (nu, xonsh) is
  typed nothing, and says so; neither is a name a line cannot carry.
- The plugin types a `cd` only while the shell's process group has the
  terminal. Test with a real `/bin/sh`
  (`crates/plugins/terminal/tests/session.rs`).

## The player

- What plays is core data: the player publishes `playing::NowPlaying` in
  `App::player`; others (media keys via a plugin, scripts, agents) leave a
  `PlayerRequest`, taken at the next wake.
- A pinned player (`player.rs`) keeps playing while the selection moves,
  drawn in one place whatever the panel's switches (`PinPlace`): the
  preview when it shows the track; else the dock at the panel's foot
  (`dock.rs`), under the preview or under Properties alone; with the panel
  closed, a strip in the last pane's footer. A click that shows the track
  (the dock, the strip's title) turns Preview on. Playback rules:
  [media.md](media.md).
