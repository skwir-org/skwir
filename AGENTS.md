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
2. **Light.** Idle memory and startup are budgeted (ADR-000, revised in ADR-002:
   PSS, not RSS). `just bench-check` must stay green. Budgets are tightened when
   the reference improves and never loosened without an ADR.
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
  values are for keyboard paging.
- **Toolbars.** Reuse command-backed buttons and bundled symbolic SVGs in
  `crates/ui/src/toolbar.rs`. Responsive rows select variants by intrinsic
  measurement in `elements/adaptive.rs` prepaint; do not maintain pixel
  breakpoints beside changing labels. Tooltips, focus styles and keyboard
  activation belong to the shared button. Pane actions capture their target
  during rendering and need pane-specific element IDs. Popovers restore focus
  when dismissed; retain a focused button's handle when it becomes disabled.
  GPUI already emits Enter/Space clicks on key-up: do not dispatch again on
  key-down.
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
  `set_status_about` adds a link to the setting that fixes the problem. The
  status bar holds state (counts, selection size, the Undo button), never
  messages. A key whose command needs a selection carries `selection` in its
  `when`, so it does nothing instead of reporting "nothing selected". The only
  animation allowed is a bounded entrance (under 200 ms) that ends by itself;
  the player's play/pause flash (`media::FLASH_MS`) is the one longer one.
- **Application row and pane row.** The application toolbar holds what is
  about the window: views, the Sort and display menu, New folder, Search, the
  Split and Preview toggles (in the order of what they open: the middle of
  the window, then its edge), and More (everything else, so the
  narrowest variant loses nothing). Split panes sit side by side or stacked,
  and the preview is at the right or the bottom (`layout.rs`): each a choice
  in Sort and display (`ui.layout`) whose default, Automatic, follows the
  window's shape, decided in `Shell::render` from the window's size like the
  sidebar. The toggles' icons show the arrangement they open; Shift+F3
  splits the other way for that split only. Navigation is per pane: Back, Forward, Up and the
  breadcrumb trail live in each pane's own row (`address.rs`). The trail and
  the list's detail columns use the same layout-only technique, a wrapping
  flex line one row tall, so the segments nearest the current folder stay
  when a pane is narrow. Clicking beside the trail, or Ctrl+L, edits the path.
- **Context menu.** An item about the folder itself (New Folder, Refresh, a
  view) says `empty` in its `when`: it belongs to the menu opened on the
  background, which first clears the selection. One that needs a folder
  (New Folder, Bookmark This Folder, Open Folder With) also says `folder`,
  so a search or the shelf never offers it. Rarely used items set
  `submenu`; the menu expands that group in place. Give items an `icon`.
- **Nothing to list is a state, not a blank.** `placeholder.rs` says why a
  pane is empty (empty folder, no results, empty shelf) or why it failed, in
  the user's words with the system's message as the detail, and offers the
  way out. It renders inside the pane container, so drops and the folder's
  menu still work. The banner above the view is only for an error beside
  existing entries.
- **Tokens.** Colors, radii and shadows come from `theme.rs` (its header
  lists the scales). State colors are tokens: `hover`, `selection_*`,
  `success`/`warning`/`error`, `scrim`. Do not derive one locally.
- **Sidebar.** Places carry a theme icon name (one the theme has only as
  `-symbolic` is drawn tinted, `icons::themed`), optionally `space` (filled in
  by a worker: asking a volume can block), an `eject` command, `unavailable`
  (why it cannot be opened: the row dims and this is its tooltip) and `menu`,
  the place's own right-click items with their arguments. A rescan publishes
  at once and keeps what the worker measured last (`carry_over`) until the
  worker measures again, so nothing blinks. A row is a drop target like any
  folder; a folder dropped beside the rows is bookmarked through
  `places.bookmark`, which writes the desktop's shared GTK bookmarks.
  Bookmarks are edited only through `BookmarkEdit` (`bibli_platform::places`):
  each edit returns its inverse, which the notification offers as Undo
  (then Redo, `App::notify_with_action`), and lines other applications
  wrote are kept. The plugin watches the file, so other file managers'
  edits appear. Keeping a location is one gesture whatever it is: Ctrl+D
  and the pane's button run the core `keep` command, which hands the
  location shown to the `Keeper` registered for its scheme (bookmarks for
  folders, saved searches for queries); a place the user kept carries
  `forget`, which the button runs while the pane shows it, and `reorder`,
  which lets it be dragged within its group. A new kind of kept place
  registers a keeper and fills those two fields; the UI needs no branch.
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
- **What waits for a paste is core data.** The clipboard's owner publishes
  it in `App::pending` (entry ids and whether a paste will move them); the
  shell snapshots it once per frame. Every view dims cut entries
  (`views::list::CUT_OPACITY`) and the status bar shows the count, because
  the clipboard is invisible once the pane shows another folder. Escape with
  nothing selected, or a click on that count, forgets it.
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
  an ellipsis, not an overlap. A name too long keeps both ends
  (`text_ellipsis_middle`, vendored patch 0005): the extension tells files
  apart. "Fill thumbnails"
  (`ui.grid_fill`, in Sort and display) crops pictures to the box. Ctrl+wheel
  zooms through a capture-phase listener over the grid, which keeps the first
  visible item in place; the keys follow the cursor instead. A wheel listener
  asks `Hitbox::should_handle_scroll`, not `is_hovered`, and an absolute
  overlay states its `top`/`left`: without them it is laid out after its
  siblings, below the area it was meant to cover.
- **Scrolling.** Every view that scrolls carries a scrollbar
  (`elements/scrollbar.rs`) over its right edge: the view hands it whichever
  handle it already scrolls with, and the thumb is placed in prepaint from
  that handle's numbers. A `gpui::list` measures only what it renders, so a
  view built on one gives its items a height estimate
  (`ListState::reset_with_uniform_height`, vendored patch 0004) or the bar
  describes the few rows on screen instead of the document; real heights
  replace the estimate as rows are drawn. The gutter takes the pointer only while there is a
  bar in it, so a press beside the rows never selects one. Wrapping a
  scrolling element to overlay a bar must preserve its flex context — a body
  that grew in a column needs a column — or it silently stops scrolling.
- **What the window remembers.** The shape of the last window (view, split
  and its arrangement, preview with its place, width and height, timeline,
  zoom) is application data, not preference:
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
  does not take its room from the preview.
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
  as its top edge is dragged; with the panel closed the status bar keeps a
  strip of its controls, an exception to "state, never messages" only in
  that a player is state. See `docs/invariants/media.md`.
- **Volumes.** The places plugin lists mounted volumes from mountinfo and
  present-but-unmounted ones from `lsblk` (on a worker). A place with
  `activate` runs that command instead of navigating: `places.mount`, or
  `ui.unlock_prompt` for a locked container, which asks for the passphrase in
  a masked input and dispatches `places.unlock` directly, never through
  `Shell::run` (it traces arguments). The passphrase reaches udisks on
  standard input, is never stored, logged or put on a command line, and its
  argument struct has no `Debug`. Unmounting locks the container under the
  filesystem. Test this with fake `lsblk`/`udisksctl` scripts first on PATH,
  never on a real drive.
- **Search chips.** A pane showing a search has a row of chips under its
  address row (`search_bar.rs`): what `query.describe` says the search is,
  facet by facet, each choice the invocation the plugin gave
  (`query.refine`, or `ui.ask` for a value to type). The query plugin owns
  the meaning (`facets.rs`: the facets, their presets, how a query is cut
  into them and written back); the UI knows no query syntax and reads the
  chips once per location, never per frame. The text stays the one truth:
  a part no facet holds is a chip of its own, and the address bar edits
  the whole. A choice navigates, so Back undoes it. An affordance that
  goes with what it acts on (a chip's ×) takes no focus; the keyboard
  reaches the same through the chip's menu.
- **Popups own the keyboard.** A menu or popover takes focus when it opens
  and returns it when it closes, with its own key context (see `menu.rs`):
  otherwise keys act on a selection hidden behind it.
- **List columns** give way by layout alone: the name and the detail columns
  sit in one wrapping flex line one row tall (`views/list.rs::line`), so the
  columns drop whole, last first, and the name — the only child that grows —
  takes what they leave instead of sitting at its minimum beside the blank of
  a column that went. Header and rows share that one function. A name too
  long keeps both ends (`text_ellipsis_middle`) in every view: the extension
  tells files apart. Which columns show is
  chosen where they are — the header's own menu (`Shell::open_column_menu`),
  not the Settings dialog — and still through `settings.apply`, so the
  dialog and the header never disagree. The sidebar gives way to
  the panes below a window width computed in `Shell::render`; the window's
  width is known before layout, an element's is not.
- **Commands are the one surface.** Every user action is a command
  (`bibli_core::command`), so the keymap, palette, CLI, MCP server, and agents
  all share it. The UI handles `ui.*` names itself and dispatches the rest.
  Commands that mutate the filesystem return a `Plan`; the caller decides to
  apply it. User-initiated single actions apply immediately and are undoable;
  batch results and anything from a background task or an agent go through
  `App::propose` for review.
- **Operations have inverses**, a dry-run `check`, and idempotent semantics.
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
- **Conventions.** Key bindings use gpui notation (`ctrl-shift-n`); `when` is
  a space-separated list of tokens that must all hold (`selection`, `single`,
  `dir`, `file`, `empty`, `folder`; `pane`/`address`/`palette` for context);
  `a|b` holds when either does. A command that works in a folder but can
  run from a search uses `App::work_dir`, not `pane_dir`. Menu `order` groups
  by the hundreds digit (0xx open, 1xx edit, 2xx create, 3xx destructive,
  4xx misc); the UI draws separators between groups. `ui.*` commands are
  handled by the shell; a plugin may bind to them.
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
crates/plugins/*   one crate per plugin (see docs/PLAN.md Phase 2 and 4)
crates/bench       synthetic trees and the performance regression check
vendor/gpui        patched gpui 0.2.2 (see vendor/gpui/README.bibli.md)
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
  `views::named_renderer`. Do not hard-code plugin views into the toolbar or
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
  captured `waker_handle` after publishing. `Complete` ends the initial listing;
  updates and removals may still arrive afterward.
- A wake applies what the core sent (`Shell::sync`) and then asks for a
  draw; it never leaves the applying to the draw. A window the compositor is
  not showing (a workspace off screen, a minimized window) is not asked to
  draw at all, on Wayland or X11, and a media key's request or a track that
  ended must not wait for the window to be looked at. `render` syncs too,
  for what the user did since; a sync that finds nothing is cheap.
- The scheduler skips cancelled jobs before they start; running jobs must
  check their token themselves. Use child tokens for work tied to the app or
  an open location, and cancel superseded requests. Only accept results for the
  current request (e.g. the generation check in `crates/ui/src/preview.rs`);
  cancellation alone does not prevent stale results.
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
- Entries arrive before metadata. Treat missing fields as unknown, request
  hydration in batches, and react to `Updated` events instead of stat-ing each
  row during rendering. For virtual locations, use `OpenLocation::path_of` or
  `App::selected_refs`; a query/shelf location is not a filesystem directory.

## Settings

- Declare preferences with the `SettingSpec` constructors (`toggle`, `choice`,
  `integer`, `number`, `text`, `path`, `command`; `data` for collections such
  as saved queries, which stay application data managed by their commands).
  The owner states the title, description, unit, step, choice labels and
  section, so the dialog, the palette and agents need no key-specific
  branches. Titles are sentence case without the plugin's name; descriptions
  say what changes for the user, not what the key is called.
- A preference goes where its subject is, and the namespace is the page.
  One about a plugin's content (an engine, the audio and video player) is
  that plugin's, on its page, even when the shell reads it. One the shell
  applies to several plugins' content is a namespace of the shell's own,
  with a page of its own: `reading.*` (the reader and reading mode) is
  shared by documents, books, comics and Markdown. `ui.*` is only how
  bibli looks (Appearance), `core.*` what it does with files (General).
- Renaming or moving a key: call `Settings::rename(old, new)` before
  declaring it, so the value the user chose follows.
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
  registration or rendering.
- `ui.settings` takes `{ "key": "plugin.option" }` or `{ "page": "plugins" }`.
  Report a problem whose remedy is a setting with `App::notify_about_setting`
  (or `Shell::set_status_about`); the status bar links to it.
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
  86 400 seconds. Test boundary instants explicitly, with an explicit
  calendar (`lang::parse_in`), when changing date behavior.
- `kind:` groups files as the desktop's MIME database does
  (`bibli_platform::mime::Kind`, from each type's generic icon and its
  descent from `text/plain`); do not keep lists of types beside it.

## Build, test, lint

The dev environment is the Nix flake (`flake.nix`, loaded by direnv). Inside it:

```
just build          cargo build (default members: no native workers)
just workers        build the Poppler, MuPDF, GStreamer and libarchive workers
just test           cargo nextest run (default members)
just test-all       every crate, native workers included (what CI runs)
just lint           cargo fmt --check + clippy --all-targets -D warnings
just lint-all       the same over the whole workspace (what CI runs)
just bench-check    generate bench-data/ on demand and enforce budgets
just run [dir]      run the release binary
just flame [dir]    profile with samply
```

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
  idle PSS; those require the real-app measurements below and ADR-002.

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
- Measure: first frame from the shell's log line, memory from
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
