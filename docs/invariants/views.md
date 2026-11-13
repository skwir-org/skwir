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
  picture (`icons::entry_picture`), the cut dimming, the tags' strip
  (their metadata asked for what is drawn of the kept listing,
  `Shell::request_hydrate_kept`) and an option's name and place in its
  column's list box.
- **One way to say each kind of thing about an entry**, in every view:
  - *What the file is* is a mark on its picture (`icons::marked`,
    `icons::Marks`), whatever the view: a link (leading nowhere, in the
    error color) or a tag's place on the bottom-right corner; on the
    bottom-left, hidden though its name does not say so
    (`Entry::hidden_unsaid`: the system's mark, Windows' attribute or a
    Mac's flag, or a hidden folder it was found in; a name starting with
    a dot says it already, so `.bashrc` is not marked); on the
    top-right, what its repository says of it (ADR-134: `Marks::vcs`,
    `icons::vcs_mark`, read from `keys::VCS`, which the pane sets from a
    `vcs::StatesProvider`'s answer, never a view): each state its own
    shape, so none is told by its color alone (a sign for a conflict, a
    dot for a change, a check for what is staged, a plus for what is
    new), a folder the strongest below it, what is ignored unmarked.
    Marks never fade.
  - *What it is to the view* is said by fading it whole, by `behave`:
    shown, but not wholly part of what the view holds. A cut entry, on
    its way out, at `CUT_OPACITY`; a folder shown only as the way to what
    a filter keeps (`Look::only_leads`, ADR-126, and a search's folders
    on the way, ADR-127) at `ONLY_LEADS_OPACITY`, unless it is cut or
    selected: what those say comes first. A hidden file shown is wholly
    shown: never faded.
  - *What plays* (the program's `playing::NowPlaying`, whichever
    window plays it) is a round badge in the middle of its picture
    (`icons::Marks::plays`, `playing_badge`): a speaker while it plays,
    the pause bars while paused, in the accent on its own ground; a
    share of a tile's picture, the whole of a list's icon, whose glyph
    would be too small to read otherwise (`playing_side`). It is the one
    mark that acts: a click on it pauses or plays on (`player.request`),
    its press its own, the entry neither selected nor dragged. Static:
    it changes when what plays starts, pauses or changes, never with the
    position, and draws no frame of its own. Found by name, then by
    path (`Shell::plays`), so a row that is not it costs a comparison of
    names.
  - Muted ink is for what is said beside an entry (its details, counts),
    never its state. What a copy or a cut holds is said once, in the
    footer (`status-pending`), not on the entries copied.
  - What is drawn is heard: a screen reader hears each of these with the
    name ([accessibility.md](accessibility.md)).
  A new state takes the way that says what it is, never one that already
  says something else (cut and hidden both faded would read alike, as in
  Explorer).
- **An entry's tags are one strip** (`tags::strip`, fitted in prepaint
  by `elements/chips.rs`), wherever it is drawn: each tag a chip (its
  dot, then its own name), and as room shrinks the first ones chips and
  the rest dots, dots only, then fewer dots and "+N". A tag named after
  the color it wears ("Red") is a dot at every step. They stand in one
  order, so a tag is in the same place on every row and tile: those
  pinned to the sidebar first, in its order, then by name
  (`tags::entry_tags`). A tag the place shown says of every entry it
  lists is left out (`LocationProvider::tags_shown`: a tag's own, a
  search's `tag:` terms; on the board, a
  column's tag on its cards): only that tag exactly, never one within
  it. A dot, a "+N" and a chip whose tag is in a category say the whole
  name when pointed at.
- **A strip builds what it may draw** (`ChipStrip::new` takes builders):
  chips are built and measured in order until one passes its width,
  since none after it is drawn whole, and a "+N" is measured only for a
  step whose other parts fit. Its cost follows its width, never how
  many tags a file has: seventy tags a row once laid out seventy chips
  and sixty counts per row at each frame. Only a strip that asks for
  the width of every chip whole (`ChipStrip::natural`, the preview's
  header) builds them all.
- A thumbnail is drawn by `backdrop::Picture::img`, sized and fitted by
  the view. One that lets its background through, whose ink (its
  luminance, weighted by opacity, read once when converted) would not
  read on `bg` (under WCAG's 3:1 for graphics) but reads on
  `picture_backdrop`, is drawn on that (`backdrop::behind`): a drawing's
  dark lines on the dark look. A colored icon reads as it is, and an
  opaque picture has no backdrop to fringe its edges. The preview's
  pictures follow the same rule (previews.md). The map frames its photos
  in its paper.
- A file's own icon (an application's, a shortcut's, where the system
  draws its own) stays while drawn lately, at most `icons::OWN_KEPT`
  beyond what the last two frames drew; the window gives back the
  others' pictures when a frame begins (`icons::let_go`), from GPUI's
  asset cache and the atlas.

## Selection

- A listing says what changed through its versions: `version` for
  anything, `names_version` when entries come, go or are renamed,
  `shown_version` when whether a listed entry is shown may have changed
  (gone, hidden, renamed to a dot name, a companion's file), and
  `updated_since(version)` for the entries that arrived or changed in
  place since a version (`None` once it no longer keeps that far back).
  A pane says the same of the folders opened in place: `tree_version`
  for anything, `tree_members_version` for their entries. A cache of
  what is shown keys on the membership versions, and on `version` only
  when it reads metadata; the selection after a pump and a filter are
  tested again only for what `updated_since` names.
- An arrow or a click selects the item reached, and the selection then
  follows the cursor. Toggles, ranges and select all *pick*: the selection
  stays put.
- **The cursor's entry leaving the view** (trashed, deleted, moved away,
  hidden, filtered out) gives the cursor to the entry after it in the
  order last shown, or the one before at the end, as Dolphin and Nautilus
  do; a selection that emptied selects it, so Delete pressed again takes
  the next file. A folder closed in place takes the cursor that was
  inside it. One place does it, `Selection::retain_near`, from the order
  `Pane::order` last gave.
- **A name saved over never left the view.** An editor saves by writing a
  new file and renaming it onto the name (gedit, Kate, VS Code): the
  listing sends the name's old entry renamed to the new one
  (`ListingEvent::Renamed`), as it does a provisional id a stat corrects,
  so the row keeps its selection and the cursor. A file renamed onto the
  name from beside it is both renames. Renames that arrive together are
  followed in their order (`Selection::rekey`).
- **Only what is shown is selected.** `select` refuses a name the pane
  does not show (a dot file while hidden files are off, a name the filter
  leaves out) and lets go of an id it does not show (`Pane::is_shown`):
  Delete must never take what cannot be seen. Showing a hidden entry by
  name turns hidden files on first (`Pane::select_revealed`).
- **What a view draws beside the listing is lent to the pane**
  (`Pane::lend`, ADR-116): shown while lent, so selected and acted on as
  any entry, but in no order. Select all, Invert selection, ranges,
  sweeps, the filter and every count read `Pane::order`, the listing's
  and its folders opened in place, and never see a lent entry; a range
  to one adds it. What the view stops drawing (fewer levels, a rescan,
  another view, another location) leaves the selection by the rule
  above. A lent entry is read by its path when selected, and again once
  lent again seen otherwise (`Loaned::seen`, `Pane::lent_to_read`,
  `Shell::sync_lent_hydration`, `App::read_lent`), never by a listing,
  what was read shown meanwhile; until then it is what the view knew of
  it, its kind exact (a pipe is never lent as a file) or not lent at
  all. Lending what is lent already changes nothing, and a reading never
  moves `Pane::lent_version`: a view lends again when it moves, and
  lending again may ask for a reading. A reading asked before an entry
  was lent again seen otherwise is not its. The selection follows a lent
  entry renamed, or a name another file took, as a listing's (paired by
  file, else by name). After a lent entry goes, nothing is selected in
  its place (ADR-116: the map reflows).
- **What the user puts somewhere is selected there.** What a plan the
  user asked puts in a folder a pane shows is selected there once it is
  done and listed, as Nautilus and Dolphin do: a new folder or file (and
  a folder of that name there already), what was pasted or dropped (the
  other pane's folder for Shift+F5 and a drop on it), an archive made or
  extracted, what was restored. Selected as it is listed, all of it once
  all is; in the pane it was asked from first. Not when the pane shows
  another location by then, or a selection was made meanwhile, even
  between two names listed: that one wins. Where it went is told by the
  executor (`Change::placed`), never read from the plan on the window's
  thread: a name taken settled (`photos (copy)`), and a folder merged
  into as itself, though its steps fill it (`Items::placed`). A rename
  keeps its entry, and its selection, through the listing
  (`Shell::made`).
- Scheme moves say `follow` (`ui.cursor`, `step: follow`): they move a
  following selection and leave a picked one. `pick` is yazi's mark. A
  sweep (`select_sweep`) holds a range while the cursor moves (`sweep` in
  the key context). The standard keys use none of these. A view that
  knows the cursor's row passes it (`at` on `select`): a sweep or a range
  then changes only the rows each step crosses, over the order it last
  drew (`Selection::follow_at`).

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
- **A folder that moves is shown where it went.** Its listing follows it
  and says so (`ListingEvent::Moved`); a path location is then reached at
  its new address (`Listing::address`, which `Pane::location` and `Kept`
  read): the folder shown in its step of the history, so a paste or a new
  folder goes there (`App::pane_dir`), a folder opened in place under its
  new name, with what is open inside it (`Pane::follow_moves`: a watch
  sees only its own folder move). The folder shown is not followed when a
  folder above it moves: nothing watches its parents.
- **A search laid out as a tree (ADR-127)** is the tree view's own
  (`PaneView::lays_found_out_as_tree`): the flat order stays the source
  (`Pane::flat_order`) and `Pane::order` reads the tree a worker made of
  it. Its folders on the way are lent (`Pane::is_way`), so what acts on a
  selection never takes one unseen: only naming one selects it (a click,
  an arrow, the end of a range). What passes over rows goes through
  `Pane::covered` (Select All, Invert, a range, a rubber band) or is
  given `Pane::ways` (a sweep, the neighbour a cursor goes to): it leaves
  them. Count what was found with
  `Pane::found_count`, never the order's length. Folding is local to the
  tree (`Pane::fold`): nothing is opened.
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
    with `select`). A press in a column above goes where a folder or a
    place leads; on a file it goes up to that column's folder, the file
    selected there, as in Finder: the preview shows only the pane's
    selection, so a file pressed anywhere becomes the pane's.
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
  It asks for its columns once for each state of what is open
  (`Pane::expanded_version`): another view closes them (`Shell::sync`,
  the tree's Left), and coming back the board asks again.
  Grouped (ADR-131, 3), its columns are the pane's groups in their
  order, each its entries in the sort's (`BoardView::group_columns`, made
  again only when the order or its spans change), every group's cards
  there, a folded one's too (the board draws no heading to open it
  again, so nothing is folded away under it), at most `MAX_COLUMNS`:
  past them, the last column
  holds the cards of every group left, in order, and says how many
  groups it holds ("15 more groups"), so no card goes unseen and no frame
  lays out thousands of columns (a search grouped by Folder). A card
  dragged to another column does nothing (a group is what a file is, not
  where), and the folders it had opened in place are closed once, their
  files being in no group. What arrives is placed among each column's
  cards once per placement (`ListState::rows`' key, the columns drawn),
  in one pass over it, never per frame; Left and Right go past a column
  with neither a card nor an entry.

## Rubber band

- `elements/marquee.rs` sits over a view's scroll area: it reads its
  bounds in prepaint, converts the pointer to content coordinates with the
  scroll offset, and asks pure, tested geometry (a view's `Geometry`)
  which entries a rectangle covers.
- Its release listener is registered every frame: a quick click releases
  before the repaint that would register it.
- A band being drawn sends what changed since its last move (`select`
  with mode `change`: `ids` added, `removed` taken out, typed through
  `Shell::run_args`), never the whole selection again; what was selected
  before it began is never taken out, and only the ids added are checked
  against what the pane shows. The release settles it once, kept then
  covered in the order shown (`band_change`).

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
  line wrapper when the order drawn changes (its identity: arrivals count
  once placed in it, and a name changed in place sorts again), or the
  zoom, the font or the files' text size; never per frame, per batch of arrivals or per batch
  of metadata. The label is clamped to that count, so a disagreement
  costs an ellipsis, not an overlap.
- A tile's height is its name's and the captions chosen, never what its
  metadata holds: an entry's tags or details arriving never move a row.
  Its tags are a strip over the picture's top edge, as wide as the
  picture (`tags::StripOn::Picture`), its chips opaque so they read on
  any picture; names from the 128 px step (`tags::NAMED_FROM`), dots
  below.
- **Captions** (`grid::CAPTIONS`, `ui.grid_caption_*`, off until chosen
  in Settings or the View menu's Under names, a submenu beside its row,
  as Sort by's, listing them in the sort's order and groups): a line
  under every name
  for each one chosen that applies here (`Shell::caption_applies`, as
  the list's columns apply), said or blank, the same lines on every
  tile; `GridView::tile_height` counts them, `measure` still counts only
  names. The tags as a caption leave the picture to itself. A detail is
  read as the list reads its rows (`Shell::rows_showing`).
- Fill thumbnails (`ui.grid_fill`) crops pictures to the box.

## Names and zoom

- A name too long keeps both ends (`text_ellipsis_middle`), in every view:
  the extension tells files apart. A grid label clamped to several lines
  keeps its first lines whole and its end on the last (vendored patch 0037).
- **A name on several lines breaks where it reads best**
  (`name_lines::broken`), in the grid and under a drag's icon alike: as
  many lines as gpui's wrapper gives it (`GridView::measure` counts them,
  and the rows keep their heights), balanced, each break costing what it
  reads like (nothing at a space or before the extension, little after a
  separator, any punctuation or symbol as Dolphin has them, or at a
  change from letters to digits or into a capital, more in the middle of
  a word, most before a separator or an apostrophe, which holds `l'été`
  together), the extension never split; a line ends only between two
  grapheme clusters, each priced by what it starts with, so an accent
  typed apart, a virama or a Hangul jamo goes with what it follows;
  drawn with those breaks as newlines. Widths are the
  wrapper's own (`layout_width`, kept per font and size) less a pixel,
  so gpui wraps nothing more; a name broken is kept, and a frame looks
  it up. One its lines cannot hold, or holding a newline of its own, is
  left to gpui. The search stays linear in the name times a line's
  length (it stops at a line too wide).
- Ctrl+wheel zooms through a capture-phase listener
  (`elements::zoom_wheel`, `Zoomed::View`: grid, list, columns and Usage;
  previews.md for the panel), keeping the first visible item in place;
  zoom keys keep the cursor in view (a scroll they ask survives the
  anchor). `Zoomed::View` names the pane it was drawn for: the wheel
  zooms the view under the pointer, active or not. The grid's tiles are
  one size, the rows of the list, the tree and the columns another they
  share, Usage's levels a third. Keys come from `keymap::ZOOM_KEYS`.
- A wheel listener asks `Hitbox::should_handle_scroll`, not `is_hovered`.
- An absolute overlay states its `top` and `left`, or it is laid out after
  its siblings, below the area it should cover.

## Text size (ADR-117)

- A pane's view is drawn in the files' rem (`elements::rem`, from
  `Theme::sizes`, the Files text size), where the named sizes
  (`sizes::TextSizes`) are the files': a name is Body in every view (the
  list's rows, the grid's labels, the board's cards), a detail, a
  column's header and a caption Small. A grid label is a name: never
  smaller than the list's. Text a view's element paints itself (Usage's
  map) takes the same sizes from the rem in force (`sizes::painted`),
  and the lengths around it grow with it.
- A length that holds text is written at 100 % and drawn at the files'
  scale, in whole pixels (`sizes::Scale`): a row (`list::row_height`),
  a grid label's line and least width, a caption's line, a column's and
  a card's size, the gaps between details. Read from the theme in
  `render` (`list::files_scale`), from the rem in an element.
- Pictures and icons keep the zoom's size: a row is its text's height or
  its icon's and margin, whichever is taller; a grid label is the
  picture's width or the least a name needs at its size.
- A column's width is kept at 100 % (`Session::column_widths`), dragged
  at any size: a width dragged is divided by the scale.

## List columns

- One table (`views::list::COLUMNS`: setting, label, sort, width,
  alignment) feeds the header, rows, the header's menu, the View menu's
  sorts and Settings. A column that applies only somewhere says where
  (`Shell::column_applies`).
- **What sorts, shows** (ADR-130, amended 2026-11-12): every criterion the
  pane sorts by is a column and a caption under the grid's names, but the
  name and its extension, and the tags, which a row shows by the name.
  A new `SortKey` gets its `Detail`. A row says each detail it is asked
  for once (`RowData::said`, built by `Shell::rows_showing`), read by the
  list's cells and the grid's captions alike. What is read on demand
  (Taken, Dimensions, Duration: `keys::READ_ON_DEMAND`) is read for the
  pane's files while its view shows it (`Shell::fields_shown`,
  `App::show_fields`), as a sort by it reads it, and shown only while a
  plugin reads it. A type's kind is read with its name, on a worker
  (`TypeNames::kind`); an owner by name from the system's table, by
  number where it has none. The Git column (`Detail::Vcs`, ADR-134) is
  offered wherever a version control provider is, in a folder, not only
  once a repository answered: a column that came as the answer did
  would move the others a moment after the folder showed. It says a
  file's state ("Changed"), and of a folder what it holds ("Holds
  changes"), as a screen reader hears the entry and the grid's caption
  writes it.
- Name and detail columns sit in one wrapping flex line one row tall
  (`views/list.rs::line`, shared by header and rows): columns drop whole,
  last first, and the name, the only child that grows, takes the room.
- The tags' strip takes what the name leaves; the name never gives way
  to it.
- Where files come from many folders (a search, a tag, the shelf,
  Recent), the Folder column comes first among details and goes last, its
  full path in the tooltip. Show in Folder (`show_in_folder`, Ctrl+Alt+O)
  goes there with the file selected.
- Columns are chosen in the header's menu (`Shell::open_column_menu`),
  through `settings.apply`, so it never disagrees with Settings.
- **Folders first** is the user's setting (`location::FOLDERS_FIRST`),
  never a pane's: it is a field of `SortSpec`, so every order, cache and
  sort job keys on it, and the workspace gives it to each pane as the
  pane is made (`Workspace::push_pane`) and to all when it changes; the
  View menu's row is the setting (`settings.apply`). Off, folders and
  files are sorted together. A key a folder has none of
  (`SortKey::applies_to`: its size, which is not its files') sorts it as
  none, below every value, and nothing waits for it to be read. Orders
  that are no pane's sort (the address's completions, an archive's tree,
  Usage) keep folders first.
- **A folder that remembers its order** (ADR-132) is shown in it in every
  pane: `Pane::sort` is what is shown, the pane's own set aside in
  `Pane::own` meanwhile (`show_order`, from `App::open_in` alone), and
  what is kept of a pane (its tab, a tab opened from it) is
  `Pane::own_sort`, never `sort`. A change of the sort or the groups
  made where it remembers is the folder's (`Workspace::remember_order`,
  every pane showing it told). Its record is looked up in memory before
  the folder opens, so it is read for that order from the first listing.
  Each column beside the pane is in its own folder's order
  (`Pane::kept_sorting`: the one it remembers, else the pane's own),
  never the pane's `sort`, with keys of its own where the pane's do not
  answer it. The view it remembers (a list or a grid,
  `ViewSpec::remembered`) is the shell's to show (`PaneUi::own_view`),
  only while the pane's own is one of those: a pane in the columns, the
  tree or a lens is never moved out of it by a folder. A lens is shown
  over the pane's own, never in its place (`PaneUi::own_under_lens`):
  the list or grid left for it is still the pane's own when a view is
  chosen from it, as it would be from that view.
- **A folder comes read for an order that reads it** (ADR-120,
  amended): a pane sorted or grouped by what a stat or the metadata
  providers say (`SortSpec::needs_metadata`) opens its locations saying
  so (`OpenContext::reads_metadata`, from `App::open_in` and `open_for`),
  and the folder provider reads every entry's (`hydrate`, its names all
  known) before its listing completes: the folder appears in its order
  and its groups at once, never every file in one group, then moved. A
  provider that cannot leaves it to the reading the listing asks once
  whole, as an order chosen while a folder is shown does (ADR-121,
  amended: `Listing::reading_wanted`, asked by `Pane::jobs_wanted`): the
  order before stays shown, what moves in it is left where it was, and
  once everything is read the order is made whole on a worker, never
  placed batch by batch; the footer says "Reading… N of M" past 200 ms.
  The shell asks no reading of a whole pane.
- **The sort and the groups take one list of criteria** (ADR-130,
  `SortKey::ALL`), offered where they apply (`Shell::sort_keys`,
  `group_keys`: Rating while ratings are kept, Deleted in the trash,
  Folder where files come from many folders, what only the sorter reads
  while there is one, Group by only then; Taken, Dimensions and
  Duration while a provider reads their field, ADR-131). By Folder, the
  sort asks the pane's location where each entry is
  (`grouping::Folders`, made by `Pane::sorting` from
  `OpenLocation::path_of`, which answers from memory), only while it
  sorts or groups by it, and keys are asked with
  it (`grouping::Of`); a file of the trash is where it was put there
  from, as the column says. A
  criterion chosen starts in its natural direction
  (`SortKey::starts_ascending`), the header's click included; chosen again,
  it turns. A direction reverses its criterion, never the names: ties
  are A to Z either way. What has none (a date, a rating, tags, an
  extension, a folder's size) is the lowest.
- **A criterion read on demand** (ADR-131, 4: `SortKey::field`, Taken,
  Dimensions, Duration) is a field (`App::fields`), never read for
  every listing: sorted or grouped by one, the pump asks it of the
  entries the pane's order holds that may have it, its own listing's and
  those of the folders it opened in place (`Pane::fields_to_read`: each
  listing once whole, then what came or changed since
  (`Listing::updated_since`), every entry again only when the fields, a
  file (`fields_version`) or the providers changed; never per row), read
  on a worker in their lane, in batches, and the answers, a value or the
  none the file says, are the listing's (`Pane::take_fields`,
  `Listing::set_fields`), placed as they come, each only while its file
  is the one read (its identity). An answer not given (no provider, a
  read that failed: `Fields::answer_all`) leaves the field unknown, asked
  again once the providers change: never a none kept for good. What is
  not read yet waits after the rest
  (`SortKey::waits`). The listing keeps what was read while the entry's
  size and modification time are the same (`keys::READ_ON_DEMAND`), so a
  hydration does not drop it. No view reads a file to sort it.
- **The group comes first in the order** (`SortSpec::group`), so each is
  one run of it; folders first acts within each group, and a group of
  only folders comes first while it is on, whatever the groups' direction.
  Same as sort is the sort criterion's groups in its direction. Where
  each group starts is the listing's (`Listing::spans_shown`, from the
  order's ranks) mapped onto what the pane shows (`Pane::groups`): never
  asked of the keys again per row.
- **A heading is a row, never an entry** (ADR-130, 9): a view that draws
  groups keeps one map from its rows to positions in the order
  (`views::groups::RowMap`, made with `ListState::sorted` from
  `Pane::groups`, a search among the groups for every question, never a
  walk of the rows). Drawing, scrolling and the pointer work in rows; the
  cursor, ranges, Select All, the rubber band's selection and a screen
  reader's position and count in positions, as when nothing is grouped.
  A view asked to scroll to a position turns it into a row where it
  draws (`ListView::asked`), with the rows as they are then. A heading
  is a row as tall as an entry, a chevron at its start, its label Body
  and semibold as the names it heads (their lines, 1.2 times the text),
  its count beside it Small and muted, a line above it but the first;
  the one at the top is
  pinned (`groups::Pinned`, a decoration of the list, computed after it
  scrolled), pushed up by the next, and a row brought into view from
  above stops below it; a group no file is in yet's is pinned as theirs
  are (`RowMap::drawn_at`, `groups::pinned_fresh`). Where its own row is
  not drawn, the pinned heading is the one a screen reader hears, and
  the keyboard's menu hangs from it while the keys are on it. The tree
  groups its top level: a folder opened
  in place keeps its entries under it, in its group, whose heading
  counts its own (`Span::count`). Columns, Usage and the map draw no
  headings; the board, grouped, makes each group a column (above).
- **What arrives is a row, never an entry** (ADR-135): what plans bring
  into the pane's folder and has not landed (`ListState::arriving`,
  from `Executor::arriving_in` at each sync, once it lasted
  `work::SHOWN_AFTER`) is placed in the same map, where it will land
  (`Pane::stands`, then `RowMap::arriving`: before the entry it goes
  before, in its group), and so is never a position: the cursor, ranges,
  Select All, the rubber band and a screen reader's count pass it over,
  and the arrows go past it (`heading_nav`, `GridView::tile_step`). A
  grid's tiles are its entries and what arrives (`RowMap::at_tile`,
  `tile_of`, `tile_lens`); what the grid is asked in positions (scroll
  to, an anchor, the band, Up and Down) it turns into tiles with the
  rows it last drew (`GridView::drawn`). What arrives is placed again
  only when the order, its groups or what arrives change, never because
  the listing's version moved (metadata read): each item is keyed once
  for how the pane shows it (`Pane::keying`, `Pane::keyed`,
  `ListState::arriving_keys`), then placed by a search among the order's
  ranks (`Listing::stand`), no key made again; one gone (landed,
  cancelled) leaves its row, the others where they were. Otherwise a
  sync takes how far each has come, the executor telling each plan's
  items under its meter's locks, nothing copied
  (`Executor::arriving_in`, `Meter::steps_read`). What landed is drawn
  whole until its folder lists it (`Arrives::Landed`), `LANDED_SHOWN` at
  most, its plan ended meanwhile or not: its row and its entry's follow
  each other with no gap; listed, nothing of it is drawn. It is drawn by `views::arriving`
  (dimmed at `OPACITY`, its tint stopping `SHORT` of the end, its pill)
  and answers its own press (Activity, never a selection). What arrives
  in a folder opened in place is placed among that folder's entries, a
  level deeper, in its group (`ListState::arriving_depth`). The board
  keeps a `RowMap` for each column, its cards and what arrives among them
  (in the folder's column, or its group's); the columns view one for the
  folder shown's column, from the pane's placements without headings,
  and one for each column beside it, placed in its folder's own order
  (`side_arriving`). An entry already listed under its name
  (`ListState::arriving_on`) is drawn as itself with the tint and the
  pill; one another program writes (`Look::writing`, `keys::WRITING`) with
  a "writing" pill.
- **A group no file is in yet** (ADR-135, amended) is a heading of its
  own (`Row::Fresh`, `RowMap::fresh`), placed where the group stands
  among the pane's (`Pane::fresh_before`), what arrives in it under it:
  never one of the pane's groups, so a heading's number is always the
  core's (`group_fold`, `ui.select_group`), never a key stop, nothing to
  fold or select, dimmed, "arriving" where a count is. A grid's lines are
  its groups in the order drawn (`RowMap::drawn`, `Drawn`), which the
  grid's heading maker turns back into one of the pane's or a fresh one.
- **A heading is a key stop** (ADR-131, 1; `list::HeadingStop`, the
  window's, never the core's): Up from a group's first entry and Down
  from the group before's last stop on its heading, drawn with the
  cursor's ring (`groups::ring`), the cursor left where it was and its
  ring not drawn; Up and Down from it go on to the entries, Shift and
  Ctrl with the arrows go from entry to entry as before. On it, Enter
  and Space select its group, Left folds it, Right unfolds it, the Menu
  key and Shift+F10 open its menu (`Shell::heading_command`, before
  the keymap's command). The stop knows its group by its place and by
  itself, found again as the order changes, and lets go as soon as the
  core's cursor moves any other way (a press, a command, a name typed):
  each sync compares it with the cursor the stop was made at. A command
  run from the heading (its group selected, folded) keeps it.
- **A group folds** (ADR-131, 2; `group_fold`, `Pane::fold_groups`):
  its entries leave the pane's order (`Pane::is_shown`, as a closed
  folder's in the tree), so the keys, Select All, a range and what
  commands act on pass them over, and what was selected of them is let
  go; its span holds no entry (`len` 0) and keeps its count, so its
  heading stays. Folds are the pane's, keyed by the group's rank and key
  (`FoldKey`), kept while it is grouped by the same criterion in either
  direction, forgotten when it is grouped by another; a fold bumps the
  pane's order (`folds_version`), and every map from rows to positions
  (`RowMap`, the grid's lines) holds a heading with no entries. Folds
  take files out only under a view that draws the headings, the way
  back to them (`PaneView::draws_headings`: the list, the tree, the
  grid; told by the shell's sync, `App::set_folds_drawn`): under the
  columns, the board, Usage or the map every file stays shown, the folds
  kept for when headings are drawn again. The footer counts what folded
  groups hold among the items, as their headings count them, and says
  how many are folded ("40 folded", `ListState::listed`,
  `Pane::order_unfolded`); what the files weigh weighs them too. Every
  group folded, the view draws their headings alone, the way back to the
  files: never an empty folder's placeholder (`placeholder::of` asks
  `RowMap::grouped`), and Up and Down, with no entry to be on, stop on the
  first heading or the last. A page that lands below the last entry (a
  folded group's heading, what arrives after every entry) goes to the last
  (`RowMap::entry_near`). A fold or an unfold, wherever it is asked (a
  heading, its menu, the palette), keeps the view on what was at its top:
  the same entry where it is now, or, one folded away, its group's heading
  at the top (`RowMap::kept_row`, `GridView::keep_top`), never what slid
  under the same pixels or took the same tile's number.
- **A heading acts** (ADR-130, 12, `groups::acting`): lit where the
  pointer is; a press selects what it counts (`ui.select_group`, its own
  entries in the tree; with Ctrl, beside what is selected), from the first
  to the last as a band would, the view left where it is; the right button
  opens its menu (`Shell::open_group_menu`): Select these (not while
  folded), Show only these where a query term keeps the group's files,
  the filter narrowed by it (`query.filter`'s `also`: the reader of the
  filter's words keeps the term besides them, each part still holding;
  the shell writes no query text), Fold or Unfold, Fold the others, and
  Unfold
  all while one is folded. Its chevron folds and unfolds; a press beside
  it selects, then stops the keys on the heading. The pinned heading
  does the same, and its pane is made the active one as an entry's press
  makes it. `ui.select_group` without a group
  selects the cursor's: the palette's way to it.
- **The grid is lines** (`uniform_grid::math::Slots`): rows of tiles and,
  where groups are drawn, each group's heading before its rows, a group
  starting a row of its own; a tile's line is found by a search. The
  element draws the headings as wide as the grid and the one at the top
  pinned, in its prepaint (`GridGroups`, `Slots::pinned_at`); a heading's
  words begin where the first column's pictures do. Up and Down go to
  the tile drawn over or under (`Slots::above`, `below`): its column in
  the next row, or the last of a shorter row, stopping on a heading
  between as the list does, and a heading the keys stop on, folded or
  not, is brought into view (`UniformGridScrollHandle::scroll_to_heading`,
  `Slots::offset_for_heading`; the list's `Asked::Row`); a page from a
  heading goes from the tile under it, or the one before it. What a
  reflow keeps (`Slots::anchor`) is the tile under the pinned heading,
  so a grid at its top stays there when it is grouped. The rubber band
  hit-tests the same lines, and begins on no heading, the pinned one
  included (`Geometry::Cells::pinned`).
- **What changed is placed, never merged rank by rank**
  (`Listing::place`): a hydrated batch in a big folder is a thousand
  entries among a hundred thousand, a hundred times while it is read.
  What moved keeps what it was placed by (`Moved`) and is found by a
  search; what is placed gallops back from the end (`gallop_back`); then
  every rank moves once, with those beside it in a block (`shift`). What
  went is told by its id in one pass, most ranks by a bit (`Taken`), as
  going costs the listing a pass anyway. Comparisons grow with the batch
  and the log of the gaps; what grows with the order is bulk copying. A
  rank stays a plain value (`Copy`): what an entry is grouped or sorted
  by in words lives beside the ranks (`Extras`), each rank's freed as it
  is taken out, so no rank is counted, cloned or dropped one by one. The
  ids are written over the order before last once no one holds it
  (`ids_of`): a list made anew per batch cost a third of hydrating in
  pages faulted in again. Code that tells two orders apart by their
  pointer holds the one it compares with (`Arc::ptr_eq`), never an
  address alone: a list no one holds is written over.
- **What core cannot read, the sorter's keys answer** (`grouping::Keys`,
  `registry::Sorter`, the query plugin's): a kind, a type's name, an
  owner's name, each entry's group. Keys answer from memory, never
  blocking; what they must read they read on a worker and say so
  (`Keys::changed`), and the order is made again. A pane's keys are made
  at each pump where its sort wants others (`Pane::keys_wanted`, whatever
  path changed the sort, a sorter turned on or off) and again when they
  say they change (midnight). Every listing a pane shows is sorted with
  `Pane::sorting` (its spec and keys), never its spec alone, which would
  lose the groups and every criterion only the sorter reads.
- **A sort asked anew** (a header clicked, the direction, hidden files,
  Folders first, a group, keys made again) is made on a worker (ADR-121):
  the header says it at once, the rows keep the order before
  (`Listing::order_shown`, kept current) until the new one is back, and
  what acts on the order (the arrows, ranges, Select all) acts on the one
  drawn. A view reads a listing's order through `order_shown` with the
  pane's sorting, never `sorted`.

## Scrolling

- **An entry brought into view:** a key moving the cursor scrolls the
  least (`PaneView::scroll_to`: nothing when the entry is wholly in view,
  else it stops at the edge it came in by), so a step past an edge moves
  the view by that step, never by a page. A jump (a name typed) brings it
  to the middle unless it is wholly in view (`scroll_to_middle`), as near
  the middle as the order's ends let it be, so what is around it shows.
  A view that cannot falls back on `scroll_to`.
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
- A `gpui::list` as tall as its rows (`ListSizingBehavior::Infer`, a
  popup's) sets its width, so rows that wrap count as tall as they are
  drawn (gpui 0052): one it says (`w(sized(..))`, the History's
  `history::WIDTH`) from the first frame; its parent's (`w_full()`, the
  address card's) once drawn at it, so keep its `ListState` from one
  showing to the next (`PaneUi::card_rows`): only a first showing, or the
  first at a new width, takes one frame more. One whose width is its rows'
  measures them unwrapped, and a row that wraps is cut under a scrollbar.
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
  location itself (`PaneView::kept_locations` asks the pane). Entries it
  draws that no listing holds (Usage's inside folders) it lends to the
  pane (ADR-116), never selects on its own.
- Reconcile registration identity, including disable and re-enable
  between frames; fall back to List when the active view disappears.
- Publish data, advance the descriptor revision, then wake; never query
  plugin data per frame.
- A view says what F5 does (`PaneView::refresh`) and whether it draws its
  own empty state (`draws_empty`).
- The Map view's scan (`geo.scan`) runs one at a time per pane: a listing
  that changes while it runs waits for it to finish, then is scanned on
  from what was looked at (`Scans::again`); a scan is cancelled only by
  another location or the pane's lifetime. While a folder is still
  listed, the view asks again at most every `map_view::RESCAN_GAP`
  (500 ms), one timer bringing what came during the gap; another
  location, or the listing complete, asks at once.

## Usage (ADR-007)

- **Every block drawn is an entry** (ADR-116): the folder's own are the
  pane's, those drawn inside them are lent to it (`Pane::lend`, from
  `usage::lent_of`, by the rules the blocks are built by: `drawn`,
  `opens`, `Node::lent_id`), lent again when what is drawn changes (the
  plugin's answer for this pane, `UsageState::answer`, never the
  plugin's revision, which any pane's scan moves; the levels, hidden
  files; `LentFrom`) or the pane let go of them (`Pane::lent_version`).
  Lent in `Shell::sync_usage`, never in `render`: a wake applies it,
  drawn or not. Each is lent with what the scan saw of it
  (`Node::seen`), so one seen as before keeps its reading; the scan
  gives each its kind (`Node::entry_kind`). A click selects the block
  under the pointer, an open folder by its header, and is the whole
  press (nothing on the map is dragged): a plain click in a selection of
  several makes it that one. The arrows move among the blocks beside the
  cursor's in its folder where they were last laid out, out to that
  folder's neighbours at its edge (`UsageView::beside`), from the
  nearest laid block that holds the cursor's when that one is not laid
  (too small to open); on the folder's own entries they move as before.
  The footer weighs the selection by its blocks (`usage::weigh`): what
  lies in a folder selected counts with it once. Leaving the view lets
  go of what it lent.
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
- **A change is counted at once; changes that keep coming wait.** A
  change that comes within `REVALIDATE_DUTY` (4) times the duration of
  the last recount a change asked for waits until then, remembered
  (`Active::changed`, `counts_changes_at`), and the worker wakes for it:
  a folder written into without pause (a copy) is counted again at most
  a fifth of the time, as a revalidation is. A request's first scan
  holds nothing back: it reads the whole tree, a change only its folder
  and those above it.
- Keep queued/running state apart from visible progress, so a quick cache
  hit never flashes a busy indicator.
- Progressive snapshots are lower bounds, marked incomplete, kept out of
  the completed cache, bounded, with entry identity kept as blocks
  reorder.
- A finished scan's chart stays through refreshes and is replaced
  atomically; it may still be incomplete (read errors). Revalidation never
  brings back unknown blocks or busy flashes.
- A block's hue follows its entry, never its rank by size or its place
  (`usage::Hues`): the map reflows as a scan goes on. An entry of the
  folder shown is given a hue once, when first drawn, and keeps it while
  it is drawn, however others come and go (a count trims the smallest
  into the rest, hidden files shown); another folder shown begins again.
  Its fills and what is written on them are the theme's (`Theme::block`,
  `block_age`, `block_kind`, `block_text`), and a test holds that text
  at the ratio the desktop's contrast asks of text (4.5:1, 7:1 where it
  asks for more) on every fill of both looks: a fill added is added to
  it.
