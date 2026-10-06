# Working on bibli

bibli is an experimental file manager: Rust, GPU-accelerated UI (gpui), a tiny
core, everything else a plugin, designed from the first commit to be driven by
humans, scripts, and AI agents alike. This file is the context every agent
(human or model) needs before touching the code. Read it fully once.

Start here: `docs/PLAN.md` (phases, gates, current status and recorded
deviations), `docs/adr/` (decisions that closed a door; ADR-000 is the
principles and budgets), then the crate you are working in.

`docs/PLAN.md` includes future designs as well as implemented work. Check the
current source before assuming a planned crate, command, or API exists.
`CLAUDE.md` includes this file; keep shared contributor guidance here.

## Privacy and fixtures

- Never add personally identifying information to the repository, including
  tests, documentation, screenshots, captured logs, and commit messages.
  Paths and filenames from a bug report are private input, not test fixtures.
- Reproduce bugs with synthetic names that preserve the relevant properties
  (apostrophes, whitespace, Unicode, nesting, etc.). Use temporary directories
  or generic example paths; never hard-code a real user's home directory.
- Review the staged diff and new artifacts for personal data before committing.
  Do not copy raw session output into repository files without sanitizing it.

## Principles (ADR-000; binding)

1. **Always responsive.** No blocking work on the UI thread, ever. Listings
   stream; every provider call is async, batched, and cancellable; visible work
   has priority. The UI sleeps until the core wakes it (`App::set_waker`); never
   run a `request_animation_frame` loop.
2. **Light.** Idle memory and startup are budgeted (ADR-000, revised in ADR-002
   and ADR-033: PSS, counting bibli's own share, not the GPU driver's code).
   `just bench-check` and `just idle-pss` must stay green. Budgets are tightened
   when the reference improves and never loosened without an ADR.
3. **Stateless and idempotent.** No daemon, no database. One deletable cache
   (`bibli_core::cache`). Re-applying an applied plan is a no-op.
4. **Harness the environment.** Reuse OS thumbnails, icon themes, the MIME
   database, bookmarks, zoxide, recently-used, installed tools (`ffmpeg`,
   `plocate`, `$EDITOR`, `$TERMINAL`) before building our own.
5. **Small core.** Only what two plugins must agree on lives in `bibli-core`.
   Everything else is a plugin with a runtime toggle.
6. **Agent-ready.** Commands are serializable data with JSON schemas. Mutations
   are plans reviewed before apply. Entries have stable identity (device +
   inode). Metadata carries provenance. AI output is a suggestion until a human
   confirms it.

## Engineering rules

- **Fix structurally, not with band-aids.** When a symptom appears a second
  time, or a fix needs hand-maintained constants to stay correct, an invariant
  is being violated. Find it, make the violation impossible, document the rule
  where the next author will read it, and remove the workarounds. Example: the
  grid reflowed a frame late after every panel toggle; the fix was moving
  size-dependent layout into the element's prepaint (`crates/ui/src/view.rs`),
  not patching each toggle.
- **No tuning for the machine it was measured on.** The reference machine
  measures budgets and regressions; it is not the target. Do not choose
  behaviour for its hardware (its GPU vendor or driver, a hardware decoder,
  its core count, its memory) unless the gain holds on the machines bibli
  runs on (other GPU vendors, no GPU, fewer cores, less memory) with
  certainty, by a mechanism that does not depend on that hardware. When one
  machine is all the evidence, record what was measured and keep the
  platform's default: see the decoder note in `docs/invariants/media.md`.
- **UI layout rule.** Never derive layout from a measured size in `render`.
  Size-dependent layout lives in an element's `prepaint`, where the bounds are
  real (`crates/ui/src/elements/`). The only permitted later reads of prepaint
  values are for keyboard paging. What prepaint changes that shows only
  in the next frame (a scroll offset) asks for that frame with
  `window.on_next_frame`, once: a `refresh` asked while drawing is
  dropped.
- **Toolbars.** Reuse command-backed buttons and bundled symbolic SVGs in
  `crates/ui/src/toolbar.rs`. Responsive rows select variants by intrinsic
  measurement in `elements/adaptive.rs` prepaint; do not maintain pixel
  breakpoints beside changing labels. Tooltips, focus styles and keyboard
  activation belong to the shared button. A text cut to fit (`truncate`,
  `text_ellipsis_middle`, a clamp) is drawn through `elements/clip.rs`:
  pointed at while cut, it says itself whole in a tooltip, before the
  tooltip of what it lies in (`clipped_with`); whole, it says nothing.
  Whether it was cut is the layout's answer, read when the tooltip is
  asked for, never in `render`. Pane actions capture their target
  during rendering and need pane-specific element IDs. Popovers restore focus
  when dismissed; retain a focused button's handle when it becomes disabled.
  GPUI already emits Enter/Space clicks on key-up: do not dispatch again on
  key-down. The click is only the element's the key went down on (vendored
  patch 0007), so a dialog closed with Enter may give focus back to its
  button.
  Preview types share their frame in `preview.rs` and one toolbar,
  `preview_bar.rs`: a kind lists its tools in slots and never builds a row
  (`docs/invariants/previews.md`, "The preview frame"). Preview menus
  belong to the selected preview and close when its content is replaced by a
  different file or its presentation closes.
  Anchor popovers to their visible trigger through `elements/popover.rs`;
  measure anchors in the current prepaint, including after responsive reflow.
  GPUI cannot nest deferred draws: keep peek in the normal overlay tree and
  defer its popovers, below modal dialogs.
- **Feedback.** Anything the user did gets an answer in `crates/ui/src/toast.rs`:
  a sentence built from the applied `Change`s ("Moved “a.txt” to Trash",
  "Copied 3 items to Archive"), with Undo when the journal can take it back
  and Redo after an undo. A plan with more than one step shows a progress bar
  with Cancel (`Executor::cancel`). `Shell::set_status` is a toast;
  `set_status_about` adds a link to the setting that fixes the problem.
  Footers hold state, never messages (`status.rs`): each pane's footer
  its counts and selection size, and with nothing selected what its files
  weigh (`weight.rs`: summed by a worker, the listing knowing only the
  sizes of the rows drawn); the window's state (Undo, what waits for
  a paste, the counts plugins publish (`tally::Tally`: the shelf's),
  proposals, the player's strip) ends the last pane's
  footer, at the window's bottom right either way a split runs. The side
  panels have no footer: they run to the window's foot. A key whose command needs a selection carries `selection` in its
  `when`, so it does nothing instead of reporting "nothing selected". The only
  animation allowed is a bounded entrance (under 200 ms) that ends by itself;
  the player's play/pause flash (`media::FLASH_MS`) is the one longer one.
- **Each area owns its top.** There is no window row: a window of one
  tab has one row of bars above its lists, split or not. The sidebar's
  header (`address::sidebar_header`, a row's height) holds its toggle and
  More (everything without a button); a sidebar not drawn leaves them at
  the start of the first pane's row (`Shell::corner_pane`). Split and
  Preview mirror them at the right, in the order of what they open (the
  middle of the window, then its edge): in the preview panel's own bar
  while it is open at the right (a row's height, like the sidebar's
  header), else at the right end of the tab strip, else at the end of the
  row of the pane at the window's right, or of the top one when stacked.
  One rule decides (`Shell::toggles_at`), so they are never in two places.
  They say their names beside their icons where they have room
  (`address::panel_toggles`, each place's richest `AdaptiveRow` variant):
  in a pane's row before the view buttons fold, in the strip while every
  tab keeps its full width.
  A preview at the bottom has no bar: its top is mid-window. Tabs are a
  strip above the panes, what a tab changes, when there are two or more,
  or one and `ui.single_tab` asks for it (`tabs.rs`); a second tab
  brings it and moves the panes down one row. More hangs from
  `Shell::more_anchor`, wherever its button is this frame.
  Everything about one pane is in that pane's own row (`render_address`):
  Back, Forward, the address (the breadcrumb trail, or the text being
  edited) ending with the button that keeps the location in the sidebar,
  then how the pane shows its folder.
  A folder's trail starts at the deepest folder the sidebar shows that
  holds it, under the sidebar's name (`sidebar::roots`, read again only
  when the places or their arrangement change; `address::crumbs`):
  `Papers › 2026` for a folder of Documents the user named Papers in the
  sidebar, `stick › Photos` on a drive, Home when nothing deeper holds
  it. A tab and the history name such a folder as the sidebar does
  (`App::location_title`); a listing, a toast and Details keep the
  folder's own name. Up still goes to the real parent, and Ctrl+L edits
  the real path.
  There is no Up button: the trail's parent segment and Alt+Up do it.
  Back and Forward list the history that way on a right-click
  (`ui.history_menu`, `back`/`forward` with `steps`); each chevron of the
  trail lists the folders inside the segment before it, and one after the
  last those inside the folder shown (`ui.subfolder_menu`, read on a
  worker, `nav_menu.rs`); one before the first, when the trail starts at
  a folder of the sidebar (Home, a drive: anything but `/`), lists the
  folders above it from the root down, by their own names as every
  chevron's menu does (`ui.parents_menu`). In a narrow pane, the first
  segment left in sight lists instead what is above it, what the pane
  wrapped out included (the same menu: a folder's folders from the
  root, a place's the trail's places before it): which segment that is
  is the layout's answer (`PaneUi::trail_shown`, from
  `on_children_prepainted`), read when its chevron is pointed at or
  pressed, as a cut text's tooltip is. Every popup's toggle goes
  through `Shell::toggle_popup`: opening one closes the one open (one at
  a time, whichever slot), a press outside closes it and says so
  (`pointer` in its close command, `Shell::note_dismissed`), and the click
  that press begins on its own button leaves it closed. A button's click
  closes the popup open the same way, so no list says which command keeps
  which popup. A toolbar popup's rows are buttons: the arrows walk them,
  and Home and End reach its ends (`ui.toolbar_first`, `ui.toolbar_last`,
  walking GPUI's tab order while the keyboard stays in the popup); the
  row reached scrolls into view (`toolbar::RevealRow`: the shell reads
  which from the focused button's key context, its id under `row`, and
  the row scrolls its popup in prepaint, where its bounds are real). The
  view buttons are the views that show the listing (`ViewRole::Listing`:
  list, grid, columns); a shape chosen now and then (`ViewRole::Alternate`:
  the tree, the board) and a lens, a view that gathers data of its own
  (`ViewRole::Lens`: Usage), have a button only while shown, and the view
  states its role, never the shell. Beside them the View menu: how that
  pane shows its folder (every view, sort, direction, hidden files, zoom,
  and in the grid Fill thumbnails). A pane row has two variants chosen by
  `AdaptiveRow`: the buttons while the address keeps `FIELD_MIN`, else
  one button with the current view's icon that opens the View menu; the
  address has no width of its own when a variant is measured, so a long
  path never folds the buttons. The View menu hangs from the active pane's
  row, the pane it acts on. How the window is arranged is More's Layout
  groups (the timeline, the terminal, where the split and the preview go,
  and where a file of an archive is previewed beside the archive's view).
  Split panes sit side by side or stacked, and the preview is at the right
  or the bottom (`layout.rs`): each a choice in More's Layout (`ui.layout`)
  whose default, Automatic, follows the window's shape, decided in
  `Shell::render` from the window's size like the sidebar. The toggles'
  icons show the arrangement they open; Shift+F3 splits the other way for
  that split only. New folder has no button: it is the folder's own menu
  and Ctrl+Shift+N. The trail and the list's detail columns use the same
  layout-only technique, a wrapping flex line one row tall, so the segments
  nearest the current folder stay when a pane is narrow. Clicking beside
  the trail, or Ctrl+L, edits the path.
- **Context menu.** An item about the folder itself (New Folder, Refresh, a
  view) says `empty` in its `when`: it belongs to the menu opened on the
  background, which first clears the selection. One that needs a folder
  (New Folder, Bookmark This Folder, Open Folder With) also says `folder`,
  so a search or the shelf never offers it. An item for some files only
  says which in its `when`: `kind:image` (every selected entry is one) or
  `has:.gpx` (one is), by kind, MIME type or extension, read from names
  (`registry::MenuItem`); never offer it to every file and let the command
  refuse. Words that depend on the environment ("Open With Image Viewer")
  come from a `MenuSource`, a command that answers from memory while the
  menu opens and replaces the plain items, which stay registered for the
  palette. Rarely used items set `submenu`; the menu expands that group in
  place. Give items an `icon`.
- **Details lead somewhere.** A provider's fact can carry a command
  (`Detail::with_action`, drawn as a link after the value, run through
  `Shell::run`): a position shows the file on the map. The row itself
  still copies its value; the link is the only thing that acts.
- **Links** (ADR-029). A link written in a document answers a click; one
  found in raw text (`bibli_platform::links`: addresses by their scheme,
  never a bare domain; in the terminal, paths) answers Ctrl+click and is
  drawn as one only while Ctrl is held over it (`Run::found`,
  `Shell::armed_link`), Shift added for a tab of its own. An address goes
  to the desktop only through `ui.open_url`; a file or folder only
  through `ui.reveal` (with `line`, the preview shows it), never opened.
  Ctrl+Shift+O labels the links in view for the keyboard (`links.rs`). A
  terminal path is read from the folder its line was printed in: the
  plugin stamps it into the grid (`plugins/terminal/src/links.rs`), so a
  new kind of found link is stamped there too, never resolved at click.
- **Notes live on the file.** Tags, a rating and a comment are kept where
  the desktop and other applications keep them, never in a database of
  ours (ADR-027): each place is a store (`bibli_core::notes::NotesStore`,
  registered in `App::notes` by the plugin that knows it), and the one
  model every store and surface share is `notes::Notes`, where a color
  belongs to its tag. The tags plugin's stores: the desktop's `user.*`
  attributes (`xdg.rs`, Dolphin's keys), a Mac's Finder attributes under
  whichever name they reached Linux (`finder.rs`), and the `._` files a
  Mac writes on volumes without attributes (`appledouble.rs`, which also
  keeps the desktop's attributes, as `xdg.tags`); the xmp plugin's,
  XMP sidecars (`x.jpg.xmp`, and Adobe's `x.xmp` while its stem is its
  file's alone), edited in the text as it is so what bibli does not know
  stays, and XMP inside a file read for Details only; the tagspaces
  plugin's, `.ts/x.json` sidecars and a folder's `.ts/tsm.json` (a tag
  kept keeps its exact color, read as the nearest of the palette), and
  a location's `.ts/tsi.json` as a search hint; the name-tags plugin's,
  tags in names (`TagSpaces`' `name[a b].ext`, read only where a `.ts`
  folder shows `TagSpaces` is used; filetags' `name -- a b.ext`, off
  until asked). A store in names is never written by an edit nor said
  to differ: writing a name is a rename plan of its own (`names.tag`,
  and `names.untag`, which first has the other stores keep the tags). A batch of one folder's
  files is read from every store with one `listxattr` per file for all of
  them (`NoteStores::read`), in the provider: a new store joins it, never
  a read of its own per row; a folder's `._` names are read once for its
  listing. An edit is a core `set_notes` step, built with
  `app.notes.edit(entry)` so the policy it writes with is captured when
  the plan is made; it writes every store that policy allows (those
  holding the file's notes, the ones set to create, and where the file's
  own attributes are refused the store its folder already uses, else
  AppleDouble), and its inverse (`restore_notes`) puts back each store's
  bytes exactly, refusing if another application changed them since.
  Stores that disagree are merged against what bibli last wrote (a
  snapshot in the cache): another application's later edit wins, and a
  store emptied is not an edit; Details says where notes are kept and
  which stores differ, and `notes.agree` makes them agree. A
  `._` file is rewritten whole as macOS lays it out, keeping every other
  attribute and its mode; one laid out otherwise is read, never written.
  An edit of several files applies what the user changed
  (`tags.edit`: add these, remove those), never a list written over each
  file. Commands that act on entries take `entries` so a surface acts on
  what it showed (Details, the tag editor), not the selection at the time.
  A tag's color is the user's choice (`tags.colors`), else the one its
  file gives it (a Mac's Finder tag, `keys::TAG_COLORS`), else the user's
  choice for its nearest category, else its name's
  (`metadata::tag_color`, a category's at the top); copies keep every
  `user.*` attribute (ADR-006). A tag is in a category by its name
  (`Places/France/Paris`, `metadata::TAG_SEPARATOR`, as digiKam writes
  them; Lightroom's `a|b` is read as `a/b`), and a category holds what is
  within it everywhere (`metadata::tag_within`): a search, the index's
  counts (each file once), a rename (`Places` renamed takes
  `Places/France` along, and its pin and color), a removal. Nothing writes
  the categories onto the file; a row's chip says the tag's own name
  (`metadata::tag_leaf`), Details its categories too. `tags.move` puts
  tags into a category, keeping their names. A store in names holds no
  tag in a category. A category the user marks as holding one tag at a
  time (`tags.exclusive`, `metadata::TAG_EXCLUSIVE_SETTING`) is a field:
  a value given takes the place of the one there was
  (`metadata::with_exclusive`, in every edit), the tag editor takes one
  of its tags, Details shows it as a row of its own (`Status: Draft`),
  and the mark follows the category renamed. This is what `key:value`
  is in bibli: no syntax of its own, a category of tags.
  A folder listing reads `x` again when `._x` changes. A search by tag
  asks the indexes first (`bibli_core::hints`, ADR-026: Baloo when it
  runs, the tags plugin's own (`index.rs`: in pieces by folder, read when
  needed, as much room as `tags.index_mb` gives it), learned as the
  provider reads files and from each applied change, `Executor::observe`), checks every file they
  name, and walks all the same: an index hints, the walk answers. So a
  tag renamed or removed everywhere (`tags.rename`, `tags.delete`;
  several at once merge or go in one plan) takes its files from the
  search, not the index, as a plan proposed for review; under the home
  folder the tag's pin and color follow it, a step of the same plan
  (`follow.rs`), so its undo puts them back. The tags under a folder
  are browsed as folders, in any view (`tags:` locations, ADR-032,
  below); there is no Tags view of its own. A tag location counts its
  folder: a count (`count.rs`) teaches the index every
  tagged file under the folder and makes it cover the folder as of when
  it began; shown again, even after a restart, a tag location lists
  those counts at once and checks, complete once checked: it reads only
  the files (and companions)
  whose change time moved since, and whole the folders it did not see
  there (by device, inode and path: new or moved in) and those on a
  filesystem whose change times do not tell (`filesystem::
  change_times_tell`: network, FUSE). A folder counted or checked once
  since bibli started is not checked again until F5. Between, the index
  follows what panes list and what bibli applies (tags, moves, trash),
  and the tag locations open list again at once. `tags.counted` says
  whether a folder's tags are being counted, and since when a count
  covers them.
- **Files beside a file go where it goes** (ADR-028). A companion (a
  Mac's `._x`, an XMP sidecar, TagSpaces' `.ts/x.json`) is declared by
  name rules (`bibli_core::companions::Companion`, registered in
  `App::companions` by the plugin that uses it); the core agrees on them
  in three places. The listing marks them (`Listing::with_companions`)
  and hides one whose file is listed unless hidden files and
  `core.show_companions` are both on (General, and the View menu under
  Hidden files); an orphan is a file like any other. A change to one is
  a change to its file's notes. The built-in operations (rename, move,
  copy, swap, trash, restore, delete) carry them in the same step, their
  inverses bringing them back; a companion's place taken is a conflict
  as the file's own is. A copy takes a `._` file's attributes in where
  the destination keeps attributes, and where it refuses them keeps the
  file's beside it (the `._` copied whole, or started from them), so a
  copy onto a stick keeps its tags; it fails only when nothing can keep
  them. Details lists a file's companions, and what an orphan belonged
  to. A batch of a folder whose names are all known is filled with them
  (`MetadataProvider::fill_listed`), so a store looks for companions in
  the listing, not the folder. Tests that need a volume refusing
  attributes run against one named by `BIBLI_NO_XATTR_DIR` (a
  `bindfs --xattr-none` mount).
- **Places, trails and verbs** (ADR-032). A location that is no folder
  can list places, entries that are no files but locations of their own
  (`EntryId::place`): every view shows one as a folder (with its tag's
  mark, `icons::Mark`, for a tag's), opening it goes where
  `OpenLocation::location_of` says (`Pane::location_of`), and it has no
  path, so what acts on files says `!place` in its `when`. A folder a
  location does not let open at its path (a trashed folder) leads where
  `location_of` says too: opening it, in a tab, in place and dropping on
  it all go there, and it keeps its path for what acts on files. Its provider
  counts what it holds (`keys::FILES`: its size column, its sort by
  size). Such a location reads as a trail (`LocationProvider::trail`,
  from its text: the folder it is of, its places, what narrows it; one
  that gives none is one place, its name, `App::trail`), which the
  address draws as a folder's (`trail.rs`, read once per location),
  whose chevrons list `LocationProvider::places` on a worker (after the
  last place only when it holds places, `Trail::holds_places`: a tag,
  the trash and its folders, not the shelf), and which Up walks back (`App::parent`: a search goes
  up to the folder it looks in). What gestures mean there is the location's
  (`LocationProvider::verbs`): a drop or a paste (`drop`), Delete
  (`remove`, run by the core `take_out`), New Folder's key and item
  (`create`, `ui.create_here`), F2 on a place (`rename`), and buttons of
  its own in its pane's row (`tools`: the shelf's Move and Copy to the
  other pane, the trash's Empty Trash), each run with the pane it names
  (`ToolPane`: the one shown, or the other pane's folder) and pressed
  only while the location lists something and that pane can take it
  (never the shelf into the shelf). A gesture it
  does not give keeps a folder's meaning or does nothing. Keys follow the
  place, commands and menu items keep their meaning: on the shelf Delete
  unshelves, beside a Move to Trash that still trashes. The menu adds the
  location's own items in its words (`menu::verb_items`), and the pane's
  key context says `refs`, `drops`, `creates` and `place` for bindings.
  How such a location is shown is its provider's alone: its name
  (`title`), the bundled glyph that stands for it (`icon`: its tab, the
  history, its pane when empty) and what it says with nothing to list
  (`empty_state`). The shell never picks a word, a glyph, a trail or a
  button by a location's scheme: a new kind of location (a flat one, as
  the trash, the shelf and Recent are, gives `title`, `icon` and
  `empty_state`) is shown as the others are, with no branch in the UI.
- **The shell names no plugin.** What the UI needs to know of a
  location, a provider or a place it asks of it, never of a scheme or a
  plugin's id: whether a location is a search
  (`LocationProvider::searches`), whether the notes keep a rating
  (`NoteStores::held`, the Rating column; rating, comment and
  `notes.agree` are the core's `notes.*` commands), what a click with a
  modifier on a place runs (`Place::clicks`), what a command leaves on
  the clipboard (`Outcome::Clipboard`), a count for the footer
  (`tally::Tally`: the shelf's), which preview shows a file's bytes
  (`PreviewProvider::shows_bytes`), what F5 does in a view and whether
  it draws its own empty state (`PaneView::refresh`, `draws_empty`), a
  place's kind (`keys::PLACE_KIND`). A folder is a location like the
  others: its trail is a `Trail`, its chevrons list `App::places_in`,
  Shift+F5 and Shift+F6 give the selection to what the other pane shows
  as a drop there does (`Shell::give_to_pane`), and what it holds open
  is `OpenLocation::holds`.
- **Tags are places** (ADR-032). The tags plugin provides `tags:`
  locations (`location.rs`): `tags:/Places/France` is the tag
  `Places/France` of the home folder (`in` names another folder), its
  tags listed as places, each counting the files within it, and its own
  files those tagged with it exactly, or everything within it with
  `deep` (the View menu's Include Sub-Tags, a `Trail::toggles`). What
  narrows it (`with`, `without`, `kind`, `year`) is kept while its
  trail is walked, a pill each after it; the Narrow by bar offers it
  (`Verbs::narrow`, `tags.narrow`). Its listing is what the tags index
  knows (`TagIndex::files_under`), shown at once where a count covered
  the folder and checked, or counted on a first visit, complete once
  counted; it lists again when the index changes, and F5 checks. A tag
  location's verbs (`browse.rs`): files dropped or pasted are given its
  tag, moved from another tag they lose that one (`tags.drop`), tags
  dropped go into it; Delete takes its tag off them, a tag selected is
  taken off every file, a plan to review (`tags.untag`); New Folder's
  key makes a tag in it, listed until a file has it (`tags.new`: no
  file, no tag, nothing kept); F2 on a tag renames it everywhere; a tag
  selected has its own items (`tags.place_menu`). A tag dropped or pasted
  in a folder is copied there as a folder of its files, with one inside
  for each tag within it (`Verbs::export`, `tags.export`: a plan to
  review); cut and pasted in a tag, it goes inside. In the grid a tag's
  tile shows a few of its pictures, the last names first, framed in its
  color (`keys::PLACE_PICTURES`, each with what a thumbnail is asked
  with, `Shell::thumb_of`).
- **Nothing to list is a state, not a blank.** `placeholder.rs` says why a
  pane is empty (an empty folder; any other location in its provider's
  words, `LocationProvider::empty_state`: no results, an empty shelf) or
  why it failed, in the user's words with the system's message as the
  detail, and offers the way out. It renders inside the pane container, so drops and the folder's
  menu still work. The banner above the view is only for an error beside
  existing entries.
- **Tokens.** Colors, radii and shadows come from `theme.rs` (its header
  lists the scales). State colors are tokens: `hover`, `selection_*`,
  `success`/`warning`/`error`, `scrim`. Do not derive one locally.
- **Sidebar.** Places carry a theme icon name (one the theme has only as
  `-symbolic` is drawn tinted, `icons::themed`; a full-color SVG, any
  icon's, is rasterized for the pixels it covers, never at the size its
  file declares, `icons::element`), optionally `space` (filled in
  by a worker: asking a volume can block), an `eject` command, `unavailable`
  (why it cannot be opened: the row dims and this is its tooltip), `busy`
  (what its owner is doing to it, "Unlocking…": a still hourglass and those
  words, nothing to click again, until the work ends) and `menu`,
  the place's own right-click items with their arguments. A rescan publishes
  at once and keeps what the worker measured last (`carry_over`) until the
  worker measures again, so nothing blinks. A row is a drop target like any
  folder; a folder dropped beside the rows is bookmarked through
  `places.bookmark`, which writes the desktop's shared GTK bookmarks.
  Bookmarks are edited only through `BookmarkEdit` (`bibli_platform::places`):
  each edit returns its inverse, which the notification offers as Undo
  (then Redo, `App::notify_undoable`, as every edit made outside a plan), and lines other applications
  wrote are kept. The plugin watches the file, so other file managers'
  edits appear. Keeping a location is one gesture whatever it is: Ctrl+D
  and the pane's button run the core `keep` command, which hands the
  location shown to the `Keeper` registered for its scheme (bookmarks for
  folders, saved searches for queries); a place the user kept carries
  `forget`, which the button runs while the pane shows it, and `reorder`,
  which lets it be dragged within its group. A new kind of kept place
  registers a keeper and fills those two fields; the UI needs no branch.
  A place with a `color` is a pill of that tag color, the pills of a
  group wrapping together, left-aligned (`sidebar::wrap_pills`: layout
  only): the tags plugin's Tags, those the user pinned (`tags.pin`,
  from a pill's or a Details chip's menu; dragged into order, unpinned
  by the pane's keep button like any kept place; a tag folder of the
  home folder, not narrowed, is kept as its pin, by the button and
  Ctrl+D, which a keeper offers only where it keeps, `Keeper::keeps`)
  or, until one is, the
  tags on the most files its index knows, as suggestions
  (`Place::suggested`, an outline), each opening its tag folder of the
  home folder (`tags:/family`), and last All Tags (`tags:/`). Shift+click
  on a tag's pill narrows what is shown to the files that have it too,
  or no longer; Alt+click shows its files in the folder shown
  (`tags.pill`, which leaves a search to `query.refine`). The tag
  editor offers the pinned tags first. A place takes what its location
  takes, the same as in a pane: a folder's row is a folder, any other's
  runs its location's `Verbs::drop` (the Trash trashes, the shelf
  shelves, a tag gives its tag and, dragged from another tag, takes the
  files out of that one); one that takes nothing (Recent, a search)
  swallows the drop, never bookmarking it. The sidebar repeats nothing
  its location says. Dragged onto a file's row (`dnd::takes_gifts`), it
  gives that file what it takes, or the selection when the row is in it
  (a column of the folders above, `takes_gift_for`: that file). Rename…
  (`Place::rename`, F2) and Remove From Sidebar (`Place::forget`) are
  the sidebar's items for every place that has them, never its owner's
  menu's.
  The groups are the user's to arrange (`sidebar::Sections`, part of
  the window's shape): a click on a header folds its group, a drag moves
  it, and the sidebar's menu lists every group with a check while it is
  shown, as the list header's does columns (`ui.sidebar_section`).
  A plugin names a place's group, never where the group goes. So are
  the places (ADR-034), by who keeps them (`sidebar::Role`): one the
  user keeps (`forget`, `reorder`) is its owner's to order, rename
  (`Place::rename`) and take out; one its owner suggests can be hidden;
  one it offers (Home, Recent, the Trash, a volume) is moved, renamed
  and hidden in the sidebar (`ui.sidebar_place`, kept per group in
  `Sections::places` by `Place::key`), never written to the owner's
  files. A place moves only among those the same command moves
  (`sidebar::mover`). `arrange` is the one reading of it, for the rows,
  the menus and the keys; a group whose places are all hidden keeps its
  header, whose menu lists its places with a check while shown. A place
  its owner offers hidden (`Place::hidden`) is listed there unchecked.
  F6 (`ui.focus_sidebar`) gives it the keyboard (`Sidebar` context,
  `ui.sidebar_key`): the arrows walk headers and places, Enter opens
  and gives the keyboard back to the files, Menu opens the place's
  menu, Alt+arrows move, F2 renames, Delete takes out or hides; a click
  never takes the keyboard (`prevent_default` on the column).
  A plugin that needs a line of text from the user dispatches
  `ui.ask` with the command to complete, not a prompt kind of its own;
  one that needs a choice dispatches `ui.choose` with its choices or the
  command that lists them on a worker (`choose.rs`), not a picker of its own
  (an `option` switch adds a flag to the chosen command: "Always use for…").
  Files the desktop shares (bookmarks, `mimeapps.list`) are rewritten only
  through `bibli_platform::file::rewrite`, by an edit that returns its inverse.
- **Learning the keys.** F1 (`shortcuts.rs`) lists the bindings that apply
  to the focused element, read from its key contexts, so a new context needs
  only a name in `keymap::context_label`. A command run three times with the
  pointer gets one hint naming its shortcut, once ever; call
  `Shell::note_pointer_use` from any new pointer-only entry point.
- **Keys come in layers** (ADR-024), lowest first: the plugins' defaults
  (`Registry::keymap`), the shell's own (`keymap::BUILTIN`), key schemes
  (`Registry::schemes`, the `vim` plugin), the user's
  (`keyboard.bindings`, a setting the core declares, edited on the
  Keyboard page). `keymap::install` lists them in that order, so a later
  layer wins whatever order plugins were enabled in; a binding a later one
  hides is left out of the hints, so no menu or tooltip names a key that
  does something else. A user's record without a command frees its keys
  (`gpui::NoAction`). Type-ahead is on only while no binding takes an
  unmodified character in the file list (`Keymap::typeahead`). A binding
  of several strokes waits for the next without a timeout (vendored patch
  0010) and `sequence.rs` shows what can follow. A plugin that changes
  standard keys is off until asked (`Plugin::enabled_by_default`). A
  scheme's key that means a standard gesture says so (`keys.like` with the
  standard keys: vim's `d` is Delete) and takes that key's bindings,
  conditions and all, so it never copies a condition that could drift.
- **The selection knows whether it was picked.** An arrow or a click
  selects the item reached, which then follows the cursor; toggles, ranges
  and select all pick. A scheme's moves say `follow` (`ui.cursor` with
  `step: follow`), which moves a selection that follows and leaves a picked
  one; `pick` is yazi's mark; a sweep (`select_sweep`) holds a range while
  the cursor moves and the pane's key context says `sweep` meanwhile. The
  standard keys use none of these and behave as they always did.
- **Folders opened in place are the pane's** (ADR-025): the tree view
  opens folders under the one shown (`Pane::expanded`, the core `expand`
  command), and their entries are selected and acted on like the shown
  listing's. What is opened is a location, at the entry that leads there
  (`workspace::Opened`): a folder, or a place (a tag's, ADR-032), so the
  tree works in a tag location too. Read a pane's entries through
  `Pane::entry`, `Pane::path_of`, `Pane::location_of` and `Pane::order`,
  never `pane.listing` alone. Only a view that says `opens_in_place`
  keeps them; the shell closes them under any other. The columns view's
  columns are a folder's parents, or the places of a location's trail
  (`PaneView::kept_locations` is given the trail). The board
  (`views/board.rs`, an alternate shape) opens every folder or place of
  the one shown in place, at most `MAX_COLUMNS`, side by side as columns
  of cards, a first column for what the place shown holds itself: a card
  is drawn and dragged as any entry (`views::entry`), so one dragged to
  another column is dropped there as on that folder or place
  (`Shell::drop_on_place`, told the place it was dragged from: a tag's
  drop takes the tag it leaves), and a column emptied stays while the
  board shows the place, to take cards back. Its keys are moves in the
  pane's order to the card they reach (`PaneView::nav_delta`), so Shift
  extends there as anywhere. A hydration asked
  for is remembered by the listing that holds the entry
  (`Pane::listing_of`), so one moved between two opened folders is
  asked for again.
- **What waits for a paste is core data.** The clipboard's owner publishes
  it in `App::pending` (entry ids and whether a paste will move them); the
  shell snapshots it once per frame. Every view dims cut entries
  (`views::list::CUT_OPACITY`) and the last pane's footer shows the count, because
  the clipboard is invisible once the pane shows another folder. Escape with
  nothing selected, or a click on that count, forgets it.
- **An entry is one look in every view** (`views/entry.rs`): what a
  view draws of an entry is read once under the lock (`Shell::look`:
  its name, icon, link or tag mark, selection, cursor, cut, tags, where
  it leads), its picture is `Look::picture`, its highlight `highlight`,
  its name and chips `name` and `tags`, and what it answers (pressed,
  clicked, dragged as the selection, dimmed while cut, taking a dragged
  tag and files dropped where it leads) is `behave`. A row, a tile or a
  card draws its own shape around these, never its own copy of them; the
  columns view's folders above are not the pane's entries, and take only
  the picture (`icons::entry_picture`) and the cut dimming.
- **Rubber band.** `elements/marquee.rs` sits over a view's scroll area: it
  reads its bounds in prepaint, converts the pointer to content coordinates
  with the view's scroll offset, and asks pure, tested geometry which
  entries a rectangle covers. A view that wants it supplies a `Geometry`.
  The release listener is registered every frame: a quick click presses and
  releases before the repaint that would register it.
- **Grid.** A tile is what the pointer can hit: the picture standing on the
  bottom of its box, so the name is always right under it, and up to three
  lines of name. An icon fills its box like a square thumbnail
  (`GridView::icon`): drawn smaller, it floats above its name with a blank
  on top. Tiles sit in slots (`elements/uniform_grid.rs::math::Slots`):
  a slot is a tile plus the least gutter, spare width widens the gutters up
  to a cap in prepaint, and the rest becomes the margins. Columns are uniform,
  rows are not: a row is as tall as its longest name needs, so rows of short
  names give the space back, and a tile keeps its own height inside a taller
  row, hanging from the top, so its highlight and hit area hug its own
  picture and name. The grid places tiles with `Slots` and the rubber
  band hit-tests with the same value, so a press in a gutter, or under a
  short name beside a long one, always starts a band. Row heights need the lines each name wraps to before anything is
  drawn: `GridView::measure` counts them with the text system's line wrapper
  when the names (`Listing::names_version`, which hydration leaves alone),
  their order or the zoom change, never per frame or per metadata batch, and
  the label is clamped to that count so a disagreement with the shaper costs
  an ellipsis, not an overlap. What would change a tile's height stays
  off it: tags are dots over the picture's corner (`tags::tile_dots`),
  their names in its tooltip. A name too long keeps both ends
  (`text_ellipsis_middle`, vendored patch 0005): the extension tells files
  apart. "Fill thumbnails"
  (`ui.grid_fill`, in the View menu) crops pictures to the box. Ctrl+wheel
  zooms through a capture-phase listener over the view (`views::zoom_wheel`,
  over the grid, the list and the columns, each with a size of its own), which
  keeps the first visible item in place; the keys follow the cursor instead. A wheel listener
  asks `Hitbox::should_handle_scroll`, not `is_hovered`, and an absolute
  overlay states its `top`/`left`: without them it is laid out after its
  siblings, below the area it was meant to cover.
- **Scrolling.** Every view that scrolls carries a scrollbar
  (`elements/scrollbar.rs`) over its right edge, and over its bottom edge
  when it scrolls sideways (`scrollbar::horizontal`: the board's columns):
  the view hands it whichever handle it already scrolls with, and the thumb is placed in prepaint from
  that handle's numbers. A `gpui::list` measures only what it renders, so a
  view built on one gives its items a height estimate
  (`ListState::reset_with_uniform_height`, vendored patch 0004) or the bar
  describes the few rows on screen instead of the document; real heights
  replace the estimate as rows are drawn. The gutter takes the pointer only while there is a
  bar in it, so a press beside the rows never selects one. Wrapping a
  scrolling element to overlay a bar must preserve its flex context — a body
  that grew in a column needs a column — or it silently stops scrolling.
- **What the window remembers.** The shape of the last window (view, split
  and its arrangement and share, sidebar width, preview with its place, width
  and height, where a file of an archive goes beside its archive's view and
  their shares, timeline, zoom, the list's column widths) is application
  data, not preference:
  `session.rs` keeps it under one key, restores it at startup and writes it
  only when it changes, never per frame. A new pane opens in the view the
  current one shows. The preview panel is as wide as the user drags it, up to
  what the panes and the sidebar do not need (`preview::max_width`, from the
  window's width, never a fixed cap); a window too small draws it narrower
  without rewriting the remembered width, and a double click on its edge
  returns it to the default. At the bottom the same holds for its height
  (`preview::max_height`); the pinned player's dock stays at the panel's
  foot, bounded by the panel's height (`Shell::dock_height_limit`), and
  until the edge is dragged the default height adds the dock's, so a pin
  does not take its room from the preview. Every edge dragged to size an
  area is one element (`elements/edge.rs`: the handle's color under the
  pointer, a double click back to the area's own size, nothing following
  the pointer), its payload telling the area's owner, and a shared room
  is shared by `edge::share`. The sidebar's right edge is dragged the
  same way (`sidebar::max_width`: the panes keep theirs and the panel its
  minimum; the panel counts the sidebar as drawn, so it gives way first).
  The edge between split panes stores a share, not pixels: the panes' flex
  grow is the share, and the drag reads the bounds GPUI hands its
  `DragMoveEvent` (the panes' container), so nothing is measured; each pane
  keeps `PANE_KEEP` across, `PANE_KEEP_HEIGHT` stacked. The share, like the
  split's own arrangement, belongs to its tab (`tab_splits`, swapped in
  `Shell::present`); a tab that gains or loses its split, by any route,
  starts even again (`sync_tabs`), and only the shown tab's is remembered.
- **Tabs** (ADR-018) are core data: the workspace groups its panes in tabs
  (one pane, or two when that tab is split) and the `tab.*` commands change
  them. A tab not shown is dormant: its panes keep history, sort and
  selection and let go of listing, watchers and kept columns
  (`Pane::suspend`); showing it opens them again through `App::show`, and
  the selection returns once the listing is complete. While the tab shown
  gets ready (listed, selection and scroll back, first rows hydrated:
  `Shell::place_pane`), the window keeps drawing the tab left, for at most
  `tabs::HOLD`; the core keeps it open until `App::settle_tabs`. A user
  command draws the tab shown first (`Shell::present`), so nothing acts on
  a tab not drawn. `core.sleep_hidden_tabs` (on by default) turns
  dormancy off: every tab stays listed (`App::apply_tabs_sleep`). The
  shell never
  creates or drops a `PaneUi` itself: it changes the workspace and
  `Shell::sync_tabs` follows its revision, so `Shell::panes` is always the
  panes on screen (`is_split`, `other_pane` rely on it), hidden panes wait
  in `Shell::hidden` with their views reset, and a pane gone is released
  there whoever closed it. Anything a plugin keeps per pane ends with the
  pane's `location_lifetime`, which dormancy ends too. The window's tabs
  are remembered only with `core.startup = last` (`core.last_tabs`), and
  only the shown tab opens at startup.
- **A menu that depends on a toolbar variant** is built once per variant and
  chosen in the popover's deferred prepaint from `AdaptiveRow::chosen`, in the
  same frame (`PopoverAnchor::popup_for`). More holds what has no button.
- **What plays is core data,** like what waits for a paste. The player
  publishes `playing::NowPlaying` in `App::player`; whoever shows or steers it
  (the desktop's media controls through a plugin, a script, an agent) reads it
  and leaves a `PlayerRequest`, which the shell takes when it next wakes.
  Music or a film can be pinned to keep playing while the selection moves
  (`crates/ui/src/player.rs`); the dock at the foot of the preview panel
  (`dock.rs`) then holds it, cover or picture, meter and transport, as tall
  as its top edge is dragged; with the panel closed the last pane's footer
  keeps a strip of its controls, an exception to "state, never messages"
  only in that a player is state. See `docs/invariants/media.md`.
- **The terminal panel** (ADR-019) draws core's `terminal::Screen` and
  sends `terminal::Input`; the VT engine (`alacritty_terminal`) never
  leaves the `terminal` plugin, so it can be replaced there alone. The
  session's own threads parse, build screens (at most one per 8 ms) and
  wake the app; the UI thread only compares a revision, and the shell
  starts on a worker. While it has the keyboard every root binding is the
  shell's (`NoAction` in the `Terminal` context, built by
  `keymap::install`) except `TERMINAL_PASSTHROUGH`: a new window-wide key
  is shadowed there automatically, and one that must also work from the
  terminal is added to that list. The plugin types a `cd` only while the
  shell's process group has the terminal; test it with a real `/bin/sh`
  (`crates/plugins/terminal/tests/session.rs`), not a mock.
- **Volumes.** The places plugin lists mounted volumes from mountinfo and
  present-but-unmounted ones from `lsblk` (on a worker). A place with
  `activate` runs that command instead of navigating: `places.mount`, or
  `ui.unlock_prompt` for a locked container, which asks for the passphrase in
  a masked input and dispatches `places.unlock` directly, never through
  `Shell::run` (it traces arguments). Opened in a tab (a middle click, the
  place's menu: `sidebar::open_in_tab`), it runs the same command with
  `tab`, carried through the prompt, and the volume mounted opens in a
  tab behind the one shown, the pane left where it was. The passphrase reaches udisks on
  standard input, is never stored, logged or put on a command line, and its
  argument struct has no `Debug`. Before unmounting, `App::release` lets
  go of everything under the mount point (an open listing holds its
  folder, `OpenLocation::holds`, and the kernel refuses to unmount a
  volume anything holds): a
  pane showing it goes home, folders kept or opened there close. Then it
  writes the volume out (`places::flush`, `syncfs`) while still mounted:
  the kernel takes a mount out of mountinfo as its unmount begins, before
  writing what it held, so the row would look unmounted while the drive
  is written and a lost write would go unsaid; the row stays busy under
  the volume's device too (`Busy::device`). A copy or a move onto a drive
  the user can unplug (`fs::volume::is_removable`: removable, or on USB,
  FireWire or an SD card, through LUKS or RAID) is written through
  (`op::builtin::Pace`): every chunk's worth of bytes waits for the drive,
  so progress is the drive's and the step ends with the copy on it. That
  one predicate is also what draws a mounted volume as removable media,
  wherever it is mounted (`lsblk`'s HOTPLUG says the same of one not
  mounted); Eject is offered for what udisks mounted for the user
  (`Mount::udisks`), not system volumes. mountinfo is read one way
  (`fs::volume::mountinfo`). The end
  of a mount or an unmount corrects the volumes the last `lsblk` saw
  (`end_busy`), rather than waiting for it to run again. An unlocked
  container is listed as its cleartext volume (`Volume::container`):
  what is said of the container holds for it. Unmounting locks the container under the
  filesystem. A volume is named by its filesystem (`places::identity`:
  its UUID from udev's database, the container's for a cleartext volume,
  and the subvolume mounted), so the user's arrangement of it holds
  mounted or not; one udisks says not to show (`UDISKS_IGNORE`: EFI,
  recovery) is offered hidden, the size guess only where udev says
  nothing. Test this with fake `lsblk`/`udisksctl` scripts first on PATH,
  never on a real drive.
- **The trash is a place** (ADR-034): the trash plugin's `trash:` lists
  what every trash holds (`bibli_platform::trash`: the home trash and
  the volumes' the sidebar shows), each file under the name it had,
  where it came from in `keys::TRASHED_FROM` (the Folder column) and
  when in `keys::TRASHED` (the Deleted column, and a sort by it, newest
  first: `SortKey::Deleted`, where the location's files are the trash's,
  not in a trashed folder). A trashed folder opens in the trash
  (`trash:` and its path, `Trash › old project › sub`), listed by the
  core's own folder listing (`Trail::one_folder`: no Folder column). The
  trash plugin reads; the core acts: Restore names the item
  (`Restore::item`) and merges a folder into one there again, with the
  companions trashed with it; a file inside a trashed folder is restored
  alone (`RestorePart`: its folders made again as they are in the trash,
  the rest left there, undone by `ReturnPart`); a file of the trash
  moved elsewhere (a drop, a paste after Cut, Shift+F5, `move`) is taken
  out there the same way (`Move` is a `RestorePart` to its place): an
  item under the name it had (the trash's `draft.txt.2` is `draft.txt`
  again), with its `.trashinfo`, and its companions trashed with it go
  beside it (`take_trashed_with`), all of it back as the trash had it
  undone (`Untrashed`); Delete takes the `.trashinfo` along wherever a
  trashed file is deleted from; and Trash refuses a file in a trash at
  any depth (`op::in_trash`), and moves within the trash only a trashed
  folder's files. A `.trashinfo` is read one way
  (`op::parse_trash_info`, which the platform re-exports). A location
  whose files are the trash's says `Verbs::restore` (the `restores`
  token): Restore in their menu, no Move to Trash, a drop or a paste
  trashes, Cut takes them out where they are pasted, and Delete deletes
  for good. A step Undo cannot take back
  (`Operation::undoable`: deleting for good) makes its plan ask before a
  gesture applies it (`Shell::apply_user_plan`), whatever the gesture.
- **Search chips.** A pane showing a search draws it in its address field
  as pills (`search_bar.rs`): where it looks first, then each facet
  `query.describe` says is set, then "+ Filter", which lists the facets
  not set; each choice is the invocation the plugin gave (`query.refine`,
  or `ui.ask` for a value to type). The query plugin owns the meaning
  (`facets.rs`: the facets, their presets, how a query is cut into them
  and written back, and the search cut in two, `root` and `text`, for
  whoever edits it); the UI knows no query syntax and reads the chips once
  per location (and again when the sidebar's names change: where it
  looks is named as the sidebar names it), never per frame. The text stays the one truth: a part no
  facet holds is a pill of its own, and a click beside the pills (or
  Ctrl+L) edits its text, where it looks staying a pill. A choice navigates, so Back undoes it.
  The tags a search requires are a set, each a chip of its own read as a
  path (`Places › France`, a category going back up to it; "+" between
  tags), and none is a NOT (`not_tag`). Over a search with a tag, a bar
  offers where it can narrow to (`narrow.rs`): the tags within each of
  its tags, the other tags its files have, their kinds and years, each
  with how many files, counted from the results when the listing changes;
  a click refines the search (a tag within one takes its place), Alt+click
  leaves a tag out. Over a folder the same bar offers its files' kinds,
  years and tags when the View menu's Narrow by asks for it
  (`ui.narrow_folders`) or while the folder is narrowed: a choice narrows
  the folder (`query.narrow`), kept while the pane shows it or a folder
  under it, a pill each after the trail. The narrowing is pane state like
  the name filter (core's `workspace::Narrowed`, applied by
  `Pane::order`): the query plugin's words, their pills, and a test of
  each file from what the listing knows of it, never reading the file.
  Its folders always show; every file is asked for its metadata
  (`Pane::files_unknown`), and one the test cannot tell yet waits hidden.
  The sidebar suggests the tags at the top: a category
  stands for those within it. An
  affordance that goes with what it acts on (a pill's ×) takes no focus;
  the keyboard reaches the same through the pill's menu. A location is
  named in words by its provider (`LocationProvider::title`: a search as
  what it looks for and where, the folder as the sidebar names it,
  `report in Papers`), for tabs and menus. A search does not
  watch the folders it walked, but what bibli changes it sees at once:
  after each plan the query plugin (an executor observer,
  `provider::Searches`) checks again the files the plan touched under
  each open search's root, as the walk would, and adds, updates or
  takes out rows, never listing again.
- **The address field is where one types** (`omnibox.rs`): Ctrl+F
  (`ui.search`) starts a search with where it looks as a pill, Ctrl+L
  edits the path; a pane showing a search edits that search either way.
  Once the text is edited, a dropdown says what Enter does and offers the
  rest: a path or a scheme a provider opens goes there; words search where
  the pill says (Enter), the home folder (Alt+Enter), files containing
  them (`query.refine`, facet `content`), or go to a place Jump knows;
  `>` hands the rest to the palette. The keyboard never leaves the field
  (Up and Down are bound in its context); a row runs on the press with
  `prevent_default`, so nothing under it takes the focus. The search is
  `query.search` with the text as `expr` and the pill as `root`: the UI
  still knows no query syntax. Words typed over a folder (not a search)
  filter it at once: the core's `filter`, which `Pane::order` applies, so
  select all, ranges and the views see only what is shown, and what is
  selected and filtered out is deselected. Showing another location
  clears it; Escape in the field or in the pane (`filtered` in its key
  context) shows everything; once the field lets go, the filter stays as a
  pill on the trail.
- **Popups own the keyboard.** A menu or popover takes focus when it opens
  and returns it when it closes, with its own key context (see `menu.rs`):
  otherwise keys act on a selection hidden behind it. Focus never
  dangles: when the focused element leaves the window, GPUI routes keys
  from above the shell's root, where nothing is bound, so the shell's
  focus-lost listener gives the keyboard to the pane drawn as active
  (`Shell::drawn_focus`). Do not refocus by hand after closing something.
- **List columns** are one table (`views::list::COLUMNS`: each column's
  setting, label, sort, width and alignment), which the header, the rows,
  the header's menu, the View menu's sorts and Settings read; a column
  that applies only somewhere says where (`Shell::column_applies`). They
  give way by layout alone: the name and the detail columns
  sit in one wrapping flex line one row tall (`views/list.rs::line`), so the
  columns drop whole, last first, and the name — the only child that grows —
  takes what they leave instead of sitting at its minimum beside the blank of
  a column that went. A row's tag chips take what the name leaves and are
  placed in prepaint (`elements/chips.rs`): those that do not fit drop
  whole, and "+N" says how many. Header and rows share that one function. A name too
  long keeps both ends (`text_ellipsis_middle`) in every view: the extension
  tells files apart. Where files from many folders are listed (a search,
  a tag, the shelf, Recent) the Folder column says each one's, first of
  the details and the last to give way, its whole path in its tooltip,
  and Show in Folder (`show_in_folder`, Ctrl+Alt+O) goes there with the
  file selected. Which columns show is
  chosen where they are — the header's own menu (`Shell::open_column_menu`),
  not the Settings dialog — and still through `settings.apply`, so the
  dialog and the header never disagree. The sidebar gives way to
  the panes below a window width computed in `Shell::render`; the window's
  width is known before layout, an element's is not.
- **Commands are the one surface.** Every user action is a command
  (`bibli_core::command`), so the keymap, palette, CLI, MCP server, and agents
  all share it. The shell's own (`ui.*`) are declared in the registry like
  the rest (`commands.rs`, owner `ui`: a title, what it does, the schema of
  its arguments), checked before they run (`CommandRegistry::check`: a
  name or an argument a command does not take is refused, never guessed),
  and carried out by the shell; dispatched from elsewhere (an agent, a
  script) one waits for the window (`App::request_front`). A user's
  shortcut is checked against them when saved. A new `ui.*` command is
  declared there, and the test of the declarations covers its keys and
  palette entries. The palette offers the shell's as `UI_COMMANDS` lists
  them, each with the arguments that make it one thing to do.
  Commands that mutate the filesystem return a `Plan`; the caller decides to
  apply it. User-initiated single actions apply immediately and are undoable;
  batch results and anything from a background task or an agent go through
  `App::propose` for review.
- **Operations have inverses**, a dry-run `check`, and idempotent semantics.
  A step that changes metadata states its words (`op::Words`: what it will
  do, for the review, and what it did, for the toast); what a plan left out
  goes in its note (`Plan::with_note`), under the title, never in it.
  Reference entries by `EntryRef` (id + path hint), never by path alone.
- **Plugins depend only on `bibli-core` (and `bibli-platform`), never on each
  other.** They talk through core-owned data (entries, metadata fields,
  selection, locations). Every contribution is tagged with the plugin id and
  withdrawn on disable; `crates/bibli/tests/plugins_toggle.rs` proves each
  plugin disables without a trace. Plugins register settings under their own
  `plugin_id.*` namespace and keep state in their own struct.
- **No plugin hook runs per frame or per row.** Providers (metadata,
  thumbnails, locations) work in batches and streams on the scheduler.
- **Locks and re-entry.** Snapshot the inputs needed under a lock, then drop
  the guard before dispatching another command, navigating, invoking a plugin,
  or doing I/O. These calls can acquire the same workspace or registry locks.
  One exception, by contract: an open location's `path_of`, `location_of`
  and `close` answer from memory or cancel, so the workspace asks them
  under its lock (`OpenLocation`); a pane let go of closes in place.
- **Pane ownership.** Resolve the target `PaneId` when work starts and carry it
  through commands and results. Do not redirect a delayed action to whichever
  pane happens to be active when it finishes. Layout callbacks that use shell
  state must restore the intended pane with `Shell::with_pane`.
- **Provenance.** Metadata fields record where they came from
  (`Provenance::{Filesystem, User, Plugin, Model}`); model output is a
  suggestion until promoted to `User`.
- **Toolkit isolation.** gpui appears only in `bibli-ui` (and the binary).
  Views implement `PaneView` in `bibli-ui`; a toolkit-dependent `ui-api`
  crate is deferred until a third-party view exists.
- **Vendored gpui.** `vendor/gpui` is gpui 0.2.2 from crates.io plus our
  patches, wired through `[patch.crates-io]`; each change is a diff in
  `vendor/patches/` in Zed's layout, ready to upstream. Read
  `vendor/gpui/README.bibli.md` before touching it. gpui's API moves between
  releases: read the vendored source, not memory. Always set an explicit font
  family (the default stack costs 35-50 ms per frame when it fails to resolve).
- **Conventions.** Key bindings use gpui notation (`ctrl-shift-n`, `G` for
  Shift+G, `g g` for a sequence); `when` is
  a space-separated list of tokens that must all hold (`selection`, `single`,
  `dir`, `file`, `empty`, `folder`, `sweep`, `place`, `refs`, `drops`,
  `creates`, `restores`; a place by its token, `pane`, `address`,
  `palette`, `sidebar`, `preview`, `menu`…, each once in
  `keymap::PLACES` with its key context and what F1 calls it);
  `a|b` holds when either does, `!a` when it does not. A command that works in a folder but can
  run from a search uses `App::work_dir`, not `pane_dir`. Menu `order` groups
  by the hundreds digit (0xx open, 1xx edit, 2xx create, 3xx destructive,
  4xx misc); the UI draws separators between groups. `ui.*` commands are
  handled by the shell; a plugin may bind to them. GPUI leaves out the
  Shift a symbol took: bind `ctrl-?` and `ctrl-+`; `ctrl-shift-?` parses
  and never fires. Zoom keys come from one table (`keymap::ZOOM_KEYS`) for everything
  that zooms. F5 refreshes in every state; a key does not change meaning
  with the layout (the split's transfers are Shift+F5 and Shift+F6). What
  the pointer does has a key: the middle button's background tab is
  Ctrl+Enter, the right button's menu the Menu key and Shift+F10, Ctrl+click
  is Ctrl+arrows then Ctrl+Space.
- **Network.** Only the `map-tiles` plugin makes requests (ADR-020), and
  only once the user chose a background that downloads; nothing a file
  names is fetched. The core has no network layer and bibli speaks no
  file-system protocol (ADR-021). A new use of the network is a decision
  with an ADR of its own.
- **Every system** (ADR-035). bibli is built for Linux, macOS and
  Windows, and `just lint-all` compiles all three (`check-cross`), so
  code only one system has sits behind `cfg` where it is used, with the
  others' answer beside it: their own code, or an error naming the
  system (`bibli_core::system::not_yet`: "Listing a folder is not
  available on Windows yet"), never Linux's behaviour where it would be
  wrong there. The filesystem is reached through core's seams, not
  `std::os::unix` or rustix's `fs`: a folder held open, listed and
  stat-ed (`fs::Dir`, `fs::stat`, with `stat::identity` and
  `stat::changed` for metadata in hand), extended attributes
  (`fs::xattr`: on Windows a volume that keeps none), what an
  operation asks of the system (`op/sys.rs`). A name is bytes through
  `OsStr::as_encoded_bytes` and back through `os_path::from_bytes`. A
  test that needs Unix (a link, a mode, a script standing in for a
  program) says `#[cfg(unix)]`; one that needs Linux (a pipe made with
  `mknodat`, the freedesktop trash, `/proc`) says
  `#[cfg(target_os = "linux")]`; one that needs what Windows has and
  Wine lacks (its shell's trash, alternate streams) asks
  `bibli_core::system::under_wine` and says why it passes there. Code
  for Windows is tried under Wine (`just test-windows`, a prefix and
  XDG folders of its own, no display); what Wine cannot answer and
  everything for macOS is compiled only, and says so where it is
  recorded. `docs/PORTING.md` lists what is still Linux's alone.
- **Unsafe.** The workspace sets `unsafe_code = "warn"`; `just lint` promotes
  warnings to errors. Any exception needs a narrowly scoped `#[allow]` with a
  justification, as in `crates/bibli/src/gpu.rs` (setting an environment
  variable before threads exist). Do not weaken the workspace lint.
- **Decisions.** Anything that closes a door gets an ADR in `docs/adr/`.
  Every phase updates the status paragraph in `docs/PLAN.md`, including
  deviations from the plan, honestly.

## Layout

```
crates/core        bibli-core: entries, locations, commands, ops/plans, registries,
                   plugin host, scheduler, cache, settings, thumbs, metadata
crates/platform    per-OS adapters: MIME, icons, thumbnails, places, open, terminal
crates/ui          the gpui shell: views, panels, prompts, keymap, elements
crates/bibli       the binary: wires plugins and starts the UI
crates/pdf-worker  optional native PDF reader process (runtime-loaded PDFium)
crates/poppler-worker optional native PDF reader process (linked poppler-glib)
crates/mupdf-worker optional native PDF reader process (linked MuPDF, AGPL; ADR-014)
crates/media-worker optional audio/video player process (linked GStreamer)
crates/archive-worker optional archive reader process (linked libarchive)
crates/mermaid-worker optional Mermaid diagram layout process (pure Rust, merman)
crates/model-worker optional 3D mesh reader and renderer process (pure Rust)
crates/plugins/*   one crate per plugin (see docs/PLAN.md Phase 2 and 4)
crates/bench       synthetic trees and the performance regression check
vendor/gpui        patched gpui 0.2.2 (see vendor/gpui/README.bibli.md)
vendor/xdg-mime    xdg-mime 0.4.0, built on every system (ADR-037; its README.bibli.md)
vendor/shared-mime-info  the type database's source the macOS and Windows packages compile
spikes/            the Phase 0 toolkit spikes, kept runnable
docs/              PLAN.md (plan and current status), STATUS-LOG.md, adr/,
                   invariants/ (binding area rules)
```

## Implementation entry points

- `crates/bibli/src/main.rs` wires plugins; `crates/ui/src/shell.rs` routes
  commands and maintains pane UI state. Built-in list/grid/columns views are
  in `crates/ui/src/views/`, not separate plugin crates.
- Plugin views register owner-tagged `ViewSpec::named` descriptors; their GPUI
  renderers implement `PaneView` in `bibli-ui` and are resolved in
  `views::named_renderer`. A view shows the pane's entries: what is no
  file is a place among them (ADR-032), never a selection of the view's
  own. Do not hard-code plugin views into the toolbar or
  palette. Reconcile registration identity, including disable/re-enable between
  frames, and fall back to List when the active view disappears. Publish data,
  advance the descriptor revision, then wake the app; never query plugin data
  on every frame. See ADR-007 for the Usage cache and watcher lifecycle.
- New plugins need the workspace dependency alias, the binary dependency and
  registration, and an entry in `crates/bibli/tests/plugins_toggle.rs`.
  Extend that test's snapshot when adding a provider scheme or a new kind of
  contribution; its provider-scheme list is explicit.
- `Plugin::register(&App)` runs again on re-enable. `App::unregister_owner`
  withdraws registered contributions; `Plugin::on_disable` must cancel work
  and release resources held outside registries. Test disable and re-enable,
  including in-flight work when the plugin starts background tasks.
- Integration tests involving several plugins belong in `crates/bibli/tests/`
  (see `timeline_query.rs`), keeping plugin crates independent of each other.

## Background work and streamed state

- `CommandRegistry::dispatch` executes on the caller's thread; dispatching a
  command does not move it to a worker. Likewise, `LocationProvider::open` and
  `OpenLocation::hydrate` are synchronous entry points: enqueue expensive work
  and return promptly. `pump_until_complete` and executor blocking methods are
  for headless tests/tools, never UI callbacks.
- The UI applies listing events a slice at a time: `Listing::drain` takes
  what was queued when called, for at most `workspace::PUMP_BUDGET`, and a
  sync that leaves a backlog gives the event loop a turn (a real timer: a
  zero one is a ready task in gpui) and syncs again, so a folder of
  hundreds of thousands of entries draws as it streams, under the loading
  banner, instead of holding the UI thread until its last batch.
  `Listing::sorted` places arrivals in the cached order (by ranks kept
  while the listing streams, a quarter of the order at a time) rather than
  sorting again; any other change sorts again. Never loop on a channel
  until it is empty while its producer runs.
- Send listing events through `OpenContext::channel` / `EventSink`, which wake
  the UI after enqueueing. Other background results must call `App::wake` or a
  captured `waker_handle` after publishing (a handle calls the hook installed
  when it is called, so one taken at registration works). `Complete` ends the
  initial listing;
  updates and removals may still arrive afterward.
- A wake applies what the core sent (`Shell::sync`) and then asks for a
  draw; it never leaves the applying to the draw. A window the compositor is
  not showing (a workspace off screen, a minimized window) is not asked to
  draw at all, on Wayland or X11, and a media key's request or a track that
  ended must not wait for the window to be looked at. `render` syncs too,
  for what the user did since; a sync that finds nothing is cheap. A window
  draws only when something asks for a frame (a notify, a refresh,
  `on_next_frame`; vendored patch 0012 on Wayland, where an idle window
  otherwise woke at the display's rate); the X11 backend still polls at
  that rate. Each wake notifies the shell, so it costs a draw: a source
  that wakes the core with nothing new is a redraw loop, idle.
- A watcher acts on changes, never on access: notify reports a file opened
  (`IN_OPEN`), so a watcher that rescans on any event rescans forever on its
  own reads (the places plugin did, from the first time anything opened the
  GTK bookmarks). Drop `EventKind::Access`, as `fs/watch.rs` and Usage do.
- The scheduler skips cancelled jobs before they start; running jobs must
  check their token themselves. Use child tokens for work tied to the app or
  an open location, and cancel superseded requests. Only accept results for the
  current request (e.g. the generation check in `crates/ui/src/preview.rs`);
  cancellation alone does not prevent stale results. In the UI, work of
  which one is asked at a time (a note, a picture, a search) is a
  `latest::Latest`: ask, forget, take, cancelled on drop.
- Pane-owned background work links to `Pane::location_lifetime`; navigation,
  Back and pane close cancel it independently of the next UI render. Show
  locations through `App::show` (navigate, Back, Forward) and
  `Pane::replace_open` so no navigation path misses this.
  Directory open/stat/watch setup belongs on workers too: a sleeping disk can
  block any of those calls. Opening failures arrive as listing errors.
- A folder is listed once while something shows it. A pane keeps open, beside
  the location it shows, what its view shows too (`PaneView::kept_locations`:
  the columns view's folders above), kept current by their watchers and
  pumped with the pane; going to one adopts its listing as it stands, and the
  folder just left waits until the next sync for the view to keep it or let
  it go (`App::keep_locations`). A view never opens a location of its own.
- Usage installs non-recursive watches as its cancellable scanner visits each
  directory, before reading it. Never delegate a whole recursive walk to a
  watcher API: it cannot observe our request cancellation. Cache only completed
  breakdowns, and invalidate data collected without reliable watch coverage.
- Scope filesystem invalidation and fallback deadlines to affected subscriptions.
  Cache eviction is not a content change; an unrelated event must not rescan every
  pane. Keep queued/running state separate from delayed visible progress so quick
  cache hits do not flash a busy indicator.
- Progressive Usage snapshots are lower bounds, explicitly marked incomplete.
  Keep partial trees out of the completed cache, bound the visible snapshot,
  and preserve entry identity as blocks reorder. Once a scan has finished,
  retain that chart through subsequent refreshes and replace it atomically;
  a finished result can still be incomplete because of read errors. Automatic
  revalidation must not reintroduce unknown blocks or busy flashes. See ADR-007.
- A link is an entry of its own (`kind: Symlink`, its identity the
  link's) that knows what it leads to (`Entry::target`: its kind,
  identity and resolved path, followed to the end when listed or
  stat-ed). It is shown as what it opens: `Entry::is_dir` is true for a
  link to a folder, its type is read from `Entry::type_name` (the
  target's name, never `entry.name`), and its size, times, owner and
  permissions are the target's (a broken link keeps its own). What the
  entry is on disk, what an operation acts on, is `kind`, and a
  selection's `EntryRef` names the link. Every view marks a link with
  `icons::Mark` (`views::entry::Look::picture`).
- What is read to show an entry is `Entry::content`: a link's target,
  with its own identity. Preview and thumbnail requests carry it, so a
  provider may type a file by `req.entry.path` and read it. Code holding
  only a path types it by `fs::content_path` (on a worker) or, where
  nothing may be read, by `App::selected_typed`; never by the path's own
  `file_name()`. A search matches type terms (`type:`, `kind:`,
  `mime:`) and what reads a file on what a link opens, name terms on its
  own name, and never descends through a link.
- A pipe, a socket or a device (`EntryKind::is_special`), or a link to
  one, is what it is whatever its name says (`inode/fifo`…), and is
  never opened: opening a pipe waits for a writer, and a device may act.
  `Entry::content` and `fs::content_path` are `None` for it (and for a
  link that leads nowhere), so a preview, a thumbnail, a field or a
  plugin is never asked to read it. Type an entry through
  `mime::of_entry` (its `Entry::inode_type` first, then its type name).
  Code that opens a file it was handed opens it with
  `file::open_regular` (the core's `fs::file`, which the platform
  re-exports), and one another application keeps beside a file (a
  sidecar, a `.trashinfo`, a `._` file) reads it whole with
  `read_regular`, which refuses one larger than it expects rather than
  cutting it.
- Entries arrive before metadata. Treat missing fields as unknown, request
  hydration in batches, and react to `Updated` events instead of stat-ing each
  row during rendering. A fact too costly for every listing (a picture's
  position) is a field read on demand (`App::fields`, ADR-023): a plugin
  registers a `FieldProvider`, a caller on a worker asks `Fields::get`,
  answers are kept by the file's identity; a labelled key shows in Details. For virtual locations, use `OpenLocation::path_of` or
  `App::selected_refs`; a query/shelf location is not a filesystem directory.

## Settings

- Declare preferences with the `SettingSpec` constructors (`toggle`, `choice`,
  `integer`, `number`, `text`, `path`, `command`; `records` for a list the
  user builds in the dialog, such as tile servers; `data` for collections such
  as saved queries, which stay application data managed by their commands).
  What only the owner can check goes in `validate_with`. A secret is a
  record's `secret` field: it goes to the desktop's keyring
  (`bibli_platform::secrets`, on a worker) and the settings file only says
  it is there; never put a secret in any other kind of setting, and test
  the keyring only through `tools/test/private-keyring.sh`, never against
  the user's.
  The owner states the title, description, unit, step, choice labels and
  section, so the dialog, the palette and agents need no key-specific
  branches. Titles are sentence case without the plugin's name; descriptions
  say what changes for the user, not what the key is called.
- A preference goes where its subject is, and the namespace is the page.
  One about a plugin's content (an engine, the audio and video player) is
  that plugin's, on its page, even when the shell reads it. One the shell
  applies to several plugins' content is a namespace of the shell's own,
  with a page of its own: `reading.*` (the reader and reading mode) is
  shared by documents, books, comics and Markdown; `notes.*` (Notes, the
  core's: each store's Read and Write, declared by
  `App::register_notes_store`) is shared by the stores of several
  plugins. `ui.*` is only how
  bibli looks (Appearance), `core.*` what it does with files (General).
- Renaming or moving a key: call `Settings::rename(old, new)` before
  declaring it, so the value the user chose follows.
- A row's title and description keep `TEXTS_MIN` of width whatever the
  control: a choice is segmented buttons only while they fit beside that
  (`elements::adaptive::Fit` measures both forms, no character counts),
  else a dropdown, and a text field narrows before the texts do. Keep
  choice labels to a word or two; the description explains them.
- The dialog (`crates/ui/src/settings.rs`, rendering in `settings_view.rs`,
  controls in `controls.rs`) is a self-contained entity: it holds the `App`,
  never the shell, and closes through an event. Keep it that way so it can
  move to a window of its own.
- Changes apply immediately, each through `settings.apply`: switches and
  choices on click, text and numbers on Enter or blur. The core accepts one
  save at a time, so the view queues later changes and sends them as one
  batch when the running save ends; it waits the same way for a save started
  by another surface. A failed save restores the live value and shows the
  reason on the row. Invalid text never leaves its field.
- The view reconciles against `Settings::revision` and `PluginHost::revision`
  when the core wakes the UI; it never reads the schema per frame. Plugins
  are toggled with `plugins.set_enabled`, which runs the lifecycle callbacks
  on a worker and persists the choice.
- `Plugin::requirements` lists external tools with a probe. Probes touch the
  filesystem: the dialog runs them on a worker when it opens, never during
  registration or rendering. One of bibli's own helpers is probed by
  starting it with `--probe` (`process::Worker::requirement`, `probe`),
  the first thing every helper answers: it checks what it opens itself
  (PDFium's library, GStreamer's playback plugins) and exits 0 when it
  can work, and one whose linked libraries are missing never starts, so
  it is not installed where only its program is. A new helper answers
  `--probe` too (each has a test of it).
- `ui.settings` takes `{ "key": "plugin.option" }` or `{ "page": "plugins" }`.
  Report a problem whose remedy is a setting with `App::notify_about_setting`
  (or `Shell::set_status_about`); its toast links to it.
- The dialog is not a deferred draw, because its dropdowns are and GPUI cannot
  nest them. It stays the last overlay child of the shell root.
- Plugins that capture preferences at registration implement
  `on_settings_changed`; refresh only their affected providers. Read-through
  preferences need no hook. Lifecycle callbacks are serialized and must not
  recursively enable/disable plugins; registry and settings locks are released
  before invoking them. Never invoke lifecycle callbacks from a UI render.
- Never trace settings values; paths and shell preferences can be private.
  Exercise saves against an isolated config directory, never the user's file.

## Area invariants (binding, read before working in the area)

The rules for the larger subsystems live next to their designs so this file
stays readable. Each is as binding as this file; read the one for the area you
are about to touch, in full:

- `docs/invariants/reader.md`: PDF readers (PDFium and Poppler workers,
  continuous scrolling, page coordinates, atlas eviction).
- `docs/invariants/media.md`: video and audio playback (GStreamer worker,
  frame slots, generations, fullscreen controls, Blade atlas retirement).
- `docs/invariants/previews.md`: structured previews (inspections, archives,
  HTML reading mode, EPUB and email, code folds).

## Paths and generated queries

- Build copy operations with `Copy::from_settings` so every command, shelf,
  clipboard and drop path captures the same tag-preservation preference in
  the serialized plan. Never read live preferences during apply or redo.
  Copy tags before restoring read-only permissions and publish the staged
  copy only after metadata succeeds; failed copies must not look complete.

- The XDG directories come from `bibli_core::xdg` (`bibli_platform::xdg`
  re-exports it: an empty variable takes its default, relative paths are
  ignored); do not read the variables again elsewhere, nor add a
  distribution's folders beside them (NixOS names its own in
  `XDG_DATA_DIRS`).
- Keep paths as `Path` / `PathBuf` and process arguments separate for as long
  as possible. Shell quoting, query quoting, and URI encoding are different
  grammars; a display string is not an escaped representation of any of them.
- When another plugin needs a search, dispatch `query.search` with separate
  `expr`, `root`, and `pane` arguments (see `timeline.select`). The query plugin
  owns literal escaping in `lang::quote`; do not interpolate raw paths into
  `in:` expressions or add a dependency on the query plugin to reuse helpers.
  Handle a disabled command/provider as a normal error.
- Query date ranges include the whole end date. A half-open histogram bucket
  `[start, end)` needs `modified>=start modified<end`. Dates are the
  user's (ADR-017): the query text records no zone, and every calendar
  boundary (dates, `today`, `this-week`, the timeline's bars) is a day of
  `bibli_platform::calendar::Calendar`, the system's time zone with its
  clock changes and the locale's first weekday. Count days as civil day
  numbers and make instants only with `Calendar::day_start`; a day is not
  86 400 seconds. A week, a month or a year is a `calendar::Period`
  (`floor`, `next`, `around`, `before`, `count`), never months counted
  where they are needed; a date read from text is `civil_day`, which
  refuses one that does not exist. Test boundary instants explicitly, with an explicit
  calendar (`lang::parse_in`), when changing date behavior.
- Types are the system's own registry's first, the desktop's database
  behind it (ADR-037, `bibli_platform::mime`): on Linux they are one;
  on macOS (UTType) and Windows (the registry's classes) the system
  names a file by its extension and the database may only say it more
  precisely, reads the bytes and holds the aliases and the hierarchy.
  Read types only through `mime`, never a system's registry beside it.
  `kind:` groups files as the desktop's MIME database does
  (`bibli_platform::mime::Kind`, from each type's generic icon and its
  descent from `text/plain`, and on macOS what the type conforms to);
  do not keep lists of types beside it.
  A provider takes files by type, never by an extension list: the
  request's type (`mime::of_request`, read from the bytes where a name is
  ambiguous: `.otf`), tested against a kind or a `mime::Types`, the types
  its reader reads, one name per type, matched through the database's
  aliases whichever it calls canonical, without case. An application's
  desktop entries say what it opens, not what its documents are
  (`LibreOffice` claims plain text, CSV and PDF): office documents are
  one list (`preview::is_office`), which the previews, the thumbnails
  and the stored pictures all ask. Two providers that hand a file to
  each other ask one question (`preview::shown_as_pages`,
  `preview::playlist`). A code view's language stays the name's.

## Build, test, lint

The dev environment is the Nix flake (`flake.nix`, loaded by direnv). Inside it:

```
just build          cargo build (default members: no native workers)
just workers        build the Poppler, MuPDF, GStreamer and libarchive workers
just test           cargo nextest run (default members)
just test-all       every crate, native workers included (what CI runs)
just lint           cargo fmt --check + clippy --all-targets -D warnings
just lint-all       the same over the whole workspace, then check-cross (CI)
just check-cross    clippy for Windows and macOS from Linux (ADR-035)
just test-windows   the Windows build's tests under Wine (args go to nextest)
just package-nix    Linux's package, by Nix, from the git tree
just package-deb    Debian's and Ubuntu's (on Debian or Ubuntu); package-rpm, package-aur
just package-try d  a Linux package built and installed in debian|fedora|arch's root (bubblewrap)
just bench-check    generate bench-data/ on demand and enforce budgets
just run [dir]      run the release binary
just dist           build bibli and every helper without debug info (target/dist)
just flame [dir]    profile with samply
```

A new helper program is listed in `tools/package/programs` (and in
`tools/package/optional` when a package of its own holds it), in the
packages' file lists (`crates/bibli/Cargo.toml`'s metadata, the
PKGBUILD) and the macOS and Windows scripts where it ships, and answers
`--probe` (ADR-040, ADR-041).

Without an active dev shell, use `nix develop --command just <recipe>`, as CI
does. `spikes/` is a separate workspace and is not covered by the root checks.

CI runs lint-all, test-all, poppler-check, mupdf-check and bench-check; all must pass before
a commit is considered done. The native workers are outside `default-members`
so the default build and tests need no Poppler, MuPDF, GStreamer or libarchive;
run the `-all` recipes (inside `nix develop`) before committing worker changes. Clippy runs pedantic with the allowances listed in the root
`Cargo.toml`; do not add allowances in code without a reason in a comment.

Tests that need a real filesystem feature (trash, xattrs) use a temp dir under
`~/.cache/bibli-tests/`, never `/tmp` (tmpfs) and never real user files.
nextest runs `tools/test/isolate-xdg.sh` before every test (`.config/nextest.toml`):
each test process gets `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME`
under `~/.cache/bibli-tests/xdg`, so a trashed fixture never lands in the
user's Trash. Run tests through nextest (`just test`), not `cargo test`.
`bench-data/` is generated by `bibli-bench gen` and is git-ignored.

- Use `AppConfig::ephemeral` for headless tests so settings and cache writes
  stay isolated. Await listing completion or events with a deadline rather
  than relying on fixed sleeps; `crates/core/tests/common/mod.rs` has examples.
- Test the user-facing command path for cross-plugin bugs, not just the helper
  that formats a string. Include synthetic special-character paths and assert
  the returned entries, not merely that parsing succeeds.
- For operation property tests, verify undo restores the original tree and
  redo restores the applied state, including partial failures. Applied steps
  can cancel each other out (e.g. two swaps); a nonzero applied count does not
  imply a different final tree. Investigate and preserve reproducible failure
  cases; a passing randomized rerun does not resolve a previous failure.
- `just bench-check` enforces the microsecond budgets in
  `crates/bench/budgets.toml`. It does not validate UI startup, frame times, or
  idle PSS: `just idle-pss` measures idle memory and the first frame on the
  session's display (ADR-033), frame times need the real-app checks below.

## Verifying UI changes

- Run the real app on `bench-data/images10k` (grid) and `bench-data/flat100k`
  (list); scroll; check idle CPU returns to ~0% (`top -p $(pgrep -x bibli)`).
- Drive it without touching the user's session: Hyprland
  `hyprctl dispatch 'hl.dsp.send_shortcut({mods="CTRL", key="2", window="class:bibli"})'`
  sends keys to the window; `grim` captures a screenshot (`-s 0.5` for the whole
  layout). For drags, a virtual pointer on the wlr protocol is the only way
  (the dragtool used in Phase 3 lives in the session scratchpad; recreate it
  from `wayland-protocols-wlr` if needed).
- Or drive it on a display of its own, which never touches the session:
  `Xvfb :97 -screen 0 1280x860x24`, then run the app with `WAYLAND_DISPLAY`
  unset, `DISPLAY=:97` and
  `VK_ICD_FILENAMES=/run/opengl-driver/share/vulkan/icd.d/lvp_icd.x86_64.json`
  (the NVIDIA driver cannot create a surface on Xvfb; lavapipe can). Send
  input with `xdotool` (`nix build --print-out-paths nixpkgs#xdotool`) and
  capture with `import -window root`. Point `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME` and `XDG_DATA_HOME` at scratch directories: settings,
  the cache and the Trash all live under them, and a test that trashes a
  fixture otherwise puts it in the user's real Trash.
- Measure: first frame from the shell's log line, idle memory with
  `just idle-pss` (own share, driver and total by mapping), other memory from
  `/proc/<pid>/smaps_rollup` (PSS), corrections/traces from `RUST_LOG=bibli_ui=trace`.
  The binaries use a tuned mimalloc (`crates/alloc`, ADR-011); heaptrack sees
  only glibc's malloc, so profile a build made with `--no-default-features`.
  Compare memory between builds back to back under the same conditions:
  PSS shares pages with whatever else runs.

## Shell hygiene (learned the hard way)

- Never put backticks inside a double-quoted `git commit -m "..."`; bash runs
  them. Write the message to a file and use `git commit -F`.
- `pkill -f <pattern>` matches the shell running it when the pattern appears
  in your own command line; use `pkill -x name` or look up pids via
  `hyprctl clients -j`.
- Chain verification with `&&` before a commit so a failing lint or test
  cannot be committed.
- `target/` grows without bound: cargo never garbage-collects stale
  dependency artifacts, and one day of parallel agents left 141 GB there on a
  root filesystem that also hosts `/tmp`. Check `du -sh target` when builds
  start failing with ENOSPC and remove `target/debug` (rebuilt by the next
  test run). Real-window checks must delete their fixture trees under `/tmp`.

## Working as several agents

- Scaffold crates and register workspace dependencies/aliases before parallel
  work starts, so nobody edits the root `Cargo.toml` concurrently.
- Each agent owns its crates only. Plugin agents never edit `crates/core`,
  `crates/platform`, `crates/ui`, or `crates/bibli`; they work around gaps
  locally and report the exact signature they wished for. The integrator
  applies core changes.
- Do not commit from a subagent; the integrator verifies (lint, tests,
  bench-check) and commits per track.
- Reports state what was verified and how, measurements with units, and gaps.
  Never describe something as working that was not exercised.
