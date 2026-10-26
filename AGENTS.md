# Working on Skwir

Skwir is an experimental file manager in Rust with a GPU-accelerated UI
(gpui): a small core, everything else a plugin, driven alike by people,
scripts and AI agents. This file is the context every contributor, human
or model, needs before touching the code. Read it once, fully. Before
working in an area, also read that area's rules (see
[Area rules](#area-rules)): they are as binding as this file.

## Where to look

- `docs/PLAN.md`: phases, gates, current status, recorded deviations and
  what is left. It also describes planned work: check the source before
  assuming a crate, command or API exists. `docs/STATUS-LOG.md` is the
  dated record behind it: what was built, measured and checked.
- `docs/adr/`: decisions that closed a door ([index](docs/adr/README.md)).
  ADR-000 holds the principles and budgets.
- `docs/invariants/`: the binding rules of each area.
- `CLAUDE.md` includes this file; put shared contributor guidance here.

## Privacy and fixtures

- Never add personally identifying information to the repository: tests,
  docs, screenshots, captured logs, commit messages. Paths and file names
  from a bug report are private input, not fixtures.
- Reproduce bugs with synthetic names that keep the relevant properties
  (apostrophes, whitespace, Unicode, nesting). Use temporary directories
  or generic example paths; never hard-code a real home directory.
- Review the staged diff and new artifacts for personal data before
  committing. Sanitize session output before copying it into a file.

## Principles (ADR-000, binding)

1. **Always responsive.** No blocking work on the UI thread, ever.
   Listings stream; every provider call is async, batched and
   cancellable; visible work goes first. The UI sleeps until the core
   wakes it (`App::set_waker`); never run a `request_animation_frame`
   loop.
2. **Light.** Idle memory and startup are budgeted (ADR-000, revised by
   ADR-002 and ADR-033: Skwir's own share of PSS, not the GPU driver's
   code). `just bench-check` and `just idle-pss` stay green. Budgets are
   tightened when the reference improves, never loosened without an ADR.
3. **Stateless and idempotent.** No daemon, no database, one deletable
   cache (`skwir_core::cache`). Re-applying an applied plan is a no-op.
4. **Harness the environment.** Reuse OS thumbnails, icon themes, the
   MIME database, bookmarks, zoxide, recently-used and installed tools
   (`ffmpeg`, `plocate`, `$EDITOR`, `$TERMINAL`) before building our own.
5. **Small core.** Only what two plugins must agree on lives in
   `skwir-core`. Everything else is a plugin with a runtime toggle.
6. **Agent-ready.** Commands are serializable data with JSON schemas.
   Mutations are plans reviewed before they apply. A file has a stable
   identity (`FileId`: device + inode, told apart by its mount where a
   FUSE program makes inodes up), and an entry is a file under one of its
   names (`EntryId`, ADR-049). Metadata carries provenance. AI output is
   a suggestion until a human confirms it.

## Engineering rules

- **Fix structurally, not with band-aids.** A symptom seen a second time,
  or a fix that needs hand-maintained constants, means an invariant is
  broken. Find it, make the violation impossible, document the rule where
  the next author will read it, and remove the workarounds. (The grid
  reflowed a frame late after every panel toggle; the fix moved
  size-dependent layout into the element's prepaint,
  `crates/ui/src/view.rs`, instead of patching each toggle.)
- **No tuning for the machine it was measured on.** The reference machine
  measures budgets and regressions; it is not the target. Choose
  behaviour for its hardware (GPU vendor or driver, hardware decoder,
  core count, memory) only when the gain holds with certainty on the
  machines Skwir runs on (other GPUs, no GPU, fewer cores, less memory),
  by a mechanism that does not depend on that hardware. When one machine
  is all the evidence, record what was measured and keep the platform's
  default (see the decoder note in `docs/invariants/media.md`).
- **UI layout.** Never derive layout from a measured size in `render`.
  Size-dependent layout lives in an element's `prepaint`, where bounds are
  real (`crates/ui/src/elements/`). The only later reads of prepaint
  values allowed are for keyboard paging. When prepaint changes something
  that shows only in the next frame (a scroll offset), ask for that frame
  once with `window.on_next_frame`: a `refresh` asked while drawing is
  dropped.
- **Tokens.** Colors, radii, shadows and fonts come from `theme.rs` (its
  header lists the scales). State colors are tokens too (`hover`,
  `selection_*`, `success`/`warning`/`error`, `scrim`); never derive one
  locally. A new color token is derived in `Theme::derive` from the
  look's `Base`, its contrast guarded, or kept as Skwir's (`Kept`) when no
  desktop's colors change what it means (ADR-071). A font is a role
  (`theme.font`, `reading_font`, `code_font`, ADR-073), read where it is
  drawn and never copied: a change shows at once.
- **Toolkit isolation.** gpui appears only in `skwir-ui` and the binary.
  Views implement `PaneView` in `skwir-ui`; a toolkit-independent
  `ui-api` crate waits until a third-party view exists.
- **Vendored gpui.** `vendor/zed` is gpui's crates from a commit of
  Zed's main branch plus our patches, each a diff in `vendor/patches/`
  in Zed's layout, ready to upstream; `tools/gpui/vendor.py` makes and
  updates it, with Zed's pins as Zed has them (ADR-064). Read
  `vendor/zed/README.skwir.md` before touching it. gpui's API moves
  between commits: read the vendored source, not memory. Always set an
  explicit font family (a default stack that fails to resolve costs
  35–50 ms per frame).
- **Plugins are independent.** A plugin depends only on `skwir-core` (and
  `skwir-platform`), never on another plugin; plugins talk through
  core-owned data (entries, metadata fields, selection, locations). Every
  contribution is tagged with the plugin id and withdrawn on disable;
  `crates/skwir/tests/plugins_toggle.rs` proves each plugin disables
  without a trace. Settings go under the plugin's own `plugin_id.*`
  namespace; state stays in the plugin's own struct.
- **No plugin hook per frame or per row.** Providers (metadata,
  thumbnails, locations) work in batches and streams on the scheduler.
- **The shell names no plugin.** The UI asks a location, provider or
  place what it needs; it never branches on a scheme or a plugin id
  (`docs/invariants/locations.md`).
- **Commands are the one surface.** Every user action is a command
  (`skwir_core::command`), shared by the keymap, the palette, the CLI,
  MCP and agents. Commands that change files return a `Plan`; the caller
  decides to apply it. A user's single action applies at once and is
  undoable; batch results and anything from a background task or an agent
  go through `App::propose` for review (`docs/invariants/commands.md`). A
  stopped action going on by itself once its server answers again is the
  user's action continuing (ADR-080, 9), not a new one.
- **Operations have inverses**, a dry-run `check`, and idempotent
  semantics (`docs/invariants/operations.md`).
- **Provenance.** Metadata fields record where they came from
  (`Provenance::{Filesystem, User, Plugin, Model}`); model output is a
  suggestion until promoted to `User`.
- **Locks and re-entry.** Snapshot what you need under a lock, then drop
  the guard before dispatching a command, navigating, calling a plugin or
  doing I/O: these can take the same workspace or registry locks. One
  exception, by contract: an open location's `path_of`, `location_of` and
  `close` answer from memory or cancel, so the workspace calls them under
  its lock (`OpenLocation`); a pane let go of closes in place.
- **Pane ownership.** Resolve the target `PaneId` when work starts and
  carry it through commands and results. Never redirect a delayed action
  to whichever pane is active when it finishes. Layout callbacks that use
  shell state restore the intended pane with `Shell::with_pane`.
- **Network.** Only the `map-tiles` plugin makes requests (ADR-020), and
  only once the user chose a background that downloads; nothing a file
  names is fetched. The core has no network layer, and Skwir speaks no
  file-system protocol: a remote folder is a mount an installed tool
  makes (sshfs, GVfs, KIO's kio-fuse, rclone, the system's), which the
  `remote` plugin starts when an address is opened, and Skwir holds no
  credential (ADR-077, ADR-081, `docs/invariants/remote.md`). Servers
  nearby are found by Avahi and wsdd while the Nearby place is shown
  (ADR-084). A new use of the network needs an ADR of its own.
- **Every system (ADR-035).** Skwir is built for Linux, macOS and
  Windows; `just lint-all` compiles all three (`check-cross`).
  - Code only one system has sits behind `cfg` where it is used, with the
    others' answer beside it: their own code, or an error naming the
    system (`skwir_core::system::not_yet`: "Listing a folder is not
    available on Windows yet"). Never Linux's behaviour where it would be
    wrong.
  - Reach the filesystem through core's seams, not `std::os::unix` or
    rustix's `fs`: `fs::Dir` (a folder held open, listed and stat-ed),
    `fs::stat` (`stat::identity`, `stat::changed` for metadata in hand),
    `fs::xattr` (a volume that keeps none, on Windows), `op/sys.rs`.
  - A name is bytes through `OsStr::as_encoded_bytes`, and back through
    `os_path::from_bytes`.
  - Tests: `#[cfg(unix)]` for links, modes, scripts standing in for
    programs; `#[cfg(target_os = "linux")]` for `mknodat` pipes, the
    freedesktop trash, `/proc`; for what Windows has and Wine lacks (its
    shell's trash, alternate streams), ask
    `skwir_core::system::under_wine` and say why the test passes there.
  - Windows code is tried under Wine (`just test-windows`: its own prefix
    and XDG folders, no display). What Wine cannot answer, and everything
    for macOS, is compiled only, and says so where it is recorded.
    `docs/PORTING.md` lists what is still Linux-only.
- **Unsafe.** The workspace sets `unsafe_code = "warn"`, and `just lint`
  makes warnings errors. An exception needs a narrow `#[allow]` with a
  justification, as in `crates/skwir/src/gpu.rs` (setting an environment
  variable before threads exist). Never weaken the workspace lint.
- **Decisions.** Anything that closes a door gets an ADR in `docs/adr/`.
  Every phase updates its status in `docs/PLAN.md`, deviations included,
  honestly, and what it leaves in PLAN.md's "What is left"; the detail
  (measurements, what was checked and how) goes in a dated entry at the
  end of `docs/STATUS-LOG.md`, not in the plan.

## Files and entries

- **A file and an entry are two identities (ADR-049).** A `FileId`
  (device + inode, `fs::stat::id_of`; a stat names it through
  `fs::stat::file`, which tells a made-up inode number apart by its
  mount, ADR-077) is the file; what belongs to the file is keyed by
  it: thumbnails, fields, notes in attributes, Usage's hard links, an
  `EntryRef`'s identity. What the cache keeps beyond a run knows a file
  by `FileId::kept_as`. An `EntryId` is the file under one
  of its names: its `at` hashes the folder holding the name and the name
  (`EntryId::in_folder`, which `stat_entry` gives every entry it stats).
  A location that knows entries by path alone (the shelf, a tag's) uses
  `EntryId::at_path`. Two hard links are two rows.
- Rows, the selection, the cursor and what waits for a paste are kept by
  entry. Compare a stat with a row through `entry.file()`.
- A rename is an entry renamed: the provider pairs the name that went
  with the one that came by their file and sends `ListingEvent::Renamed`;
  the pane follows (`Listing::take_renamed`, `Selection::rekey`). A name
  another file takes (a rename onto it, an editor saving) is its row
  still: the provider sends the old id renamed to the new. A folder shown
  that moves says so (`ListingEvent::Moved`), and the pane shows it where
  it went. A listed entry named by a reference is found at its path
  (`Pane::entry_at`).
- **References.** Reference entries by `EntryRef` (id + path hint), never
  by path alone. A reference follows a file to a new name only when it
  knew the file had one name (`EntryRef::only_name`: from
  `EntryRef::from_path`, `Entry::is_only_name`, carried into inverses with
  `with_only_name`), never to another name of a file with several. Not
  known, it resolves at its path.
- **Symlinks.** A link is an entry of its own (`kind: Symlink`, the
  link's identity) that knows its target (`Entry::target`: kind, identity
  and resolved path, followed to the end when listed or stat-ed). It is
  shown as what it opens: `Entry::is_dir` is true for a link to a folder;
  its type comes from `Entry::type_name` (the target's name, never
  `entry.name`); its size, times, owner and permissions are the target's
  (a broken link keeps its own). What operations act on is `kind`, and a
  selection's `EntryRef` names the link.
- **What is read** to show an entry is `Entry::content`: a link's
  target, with its own identity. Preview and thumbnail requests carry it,
  so a provider may type a file by `req.entry.path` and read it. Code
  holding only a path types it with `fs::content_path` (on a worker) or,
  where nothing may be read, `App::selected_typed`; never by the path's
  own `file_name()`. A search matches type terms (`type:`, `kind:`,
  `mime:`) and content on what a link opens, name terms on its own name,
  and never descends through a link.
- **Special files.** A pipe, a socket or a device
  (`EntryKind::is_special`), or a link to one, is what it is whatever its
  name says (`inode/fifo`…) and is never opened: opening a pipe waits for
  a writer, and a device may act. `Entry::content` and `fs::content_path`
  are `None` for it (and for a link that leads nowhere), so no preview,
  thumbnail, field or plugin is asked to read it. Type an entry through
  `mime::of_entry` (its `Entry::inode_type` first, then its type name).
- Code that opens a file it was handed uses `file::open_regular` (core's
  `fs::file`, re-exported by the platform). A file another application
  keeps beside a file (a sidecar, a `.trashinfo`, a `._` file) is read
  whole with `read_regular`, which refuses one larger than expected
  rather than truncating it.
  A file replaced whole is written beside it and put in its place with
  `file::take_place`: Windows refuses a plain replace of a file anyone
  has open.
- **Entries arrive before metadata.** Treat missing fields as unknown,
  request hydration in batches, and react to `Updated` events; never stat
  rows while rendering. A fact too costly for every listing (a picture's
  position) is a field read on demand (`App::fields`, ADR-023): a plugin
  registers a `FieldProvider`, a caller on a worker asks `Fields::get`,
  answers are kept by file identity, and a labelled key shows in
  Properties. A remembered field also belongs to its provider id/version
  and registry generation, captured with the provider; an old read never
  fills a replacement's memory. Only successful answers (including no
  value) are cached: read failures remain retryable, not permanent absence.
- **Virtual locations.** A search or the shelf is not a directory: use
  `OpenLocation::path_of` or `App::selected_refs`. A command that works in
  a folder but can run from a search uses `App::work_dir`, not
  `pane_dir`.

## Paths, types and dates

- Keep paths as `Path`/`PathBuf` and process arguments separate for as
  long as possible. Shell quoting, query quoting and URI encoding are
  different grammars; a display string is an escaped form of none of
  them.
- The XDG directories come from `skwir_core::xdg` (re-exported as
  `skwir_platform::xdg`: an empty variable takes its default, relative
  paths are ignored). Never read the variables elsewhere, nor add a
  distribution's folders beside them (NixOS names its own in
  `XDG_DATA_DIRS`).
- A plugin that needs a search dispatches `query.search` with separate
  `expr`, `root` and `pane` arguments (see `timeline.select`). The query
  plugin owns literal escaping (`lang::quote`): never interpolate raw
  paths into `in:` expressions, and never depend on the query plugin to
  reuse its helpers. Handle a disabled command or provider as a normal
  error.
- **Dates are the user's (ADR-017).** Query text records no zone. Every
  calendar boundary (dates, `today`, `this-week`, the timeline's bars) is
  a day of `skwir_platform::calendar::Calendar`: the system's time zone
  with its clock changes, and the locale's first weekday.
  - Count days as civil day numbers; make instants only with
    `Calendar::day_start`. A day is not 86 400 seconds.
  - A week, a month or a year is a `calendar::Period` (`floor`, `next`,
    `around`, `before`, `count`); never count months where needed. A date
    read from text is `civil_day`, which refuses one that does not exist.
  - Query date ranges include the whole end date: a half-open bucket
    `[start, end)` is `modified>=start modified<end`.
  - When changing date behaviour, test boundary instants explicitly, with
    an explicit calendar (`lang::parse_in`).
- **Types (ADR-037).** Read types only through `skwir_platform::mime`:
  the system's own registry first, the desktop's database behind it. On
  Linux they are one; on macOS (UTType) and Windows (the registry's
  classes) the system names a file by its extension and the database may
  refine it, reads the bytes and holds the aliases and hierarchy.
  - `kind:` groups files as the desktop's MIME database does
    (`mime::Kind`, from each type's generic icon and descent from
    `text/plain`; on macOS, what the type conforms to). Keep no lists of
    types beside it.
  - A provider takes files by type, never by an extension list: the
    request's type (`mime::of_request`, read from the bytes where a name
    is ambiguous, e.g. `.otf`), tested against a kind or a `mime::Types`
    (one name per type, matched through the database's aliases, case
    ignored).
  - Desktop entries say what an application opens, not what its
    documents are (LibreOffice claims plain text, CSV and PDF): office
    documents are one list (`preview::is_office`), asked by previews,
    thumbnails and stored pictures alike.
  - Two providers that hand a file to each other ask one question
    (`preview::shown_as_pages`, `preview::playlist`).
  - A code view's language stays the name's.

## Background work and streamed state

- `CommandRegistry::dispatch` runs on the caller's thread; dispatching
  does not move a command to a worker. A command that may wait (a plugin
  from outside answering) asks `scheduler::on_window_thread`: there, it
  waits on a worker and answers through a proposal or a notification;
  elsewhere (an agent's call, a script's) it waits where it is.
- `LocationProvider::open` and `OpenLocation::hydrate` are synchronous
  entry points: enqueue expensive work and return promptly.
  `pump_until_complete` and the executor's blocking methods are for
  headless tests and tools, never UI callbacks.
- **Listings stream in slices.** `Listing::drain` takes what was queued
  when called, for at most `workspace::PUMP_BUDGET`; a sync that leaves a
  backlog gives the event loop a turn (a real timer: a zero one is a
  ready task in gpui) and syncs again. So a folder of hundreds of
  thousands of entries draws as it streams, under the loading banner.
  `Listing::sorted` places arrivals in the cached order (ranks kept while
  the listing streams, a quarter of the order at a time); any other
  change sorts again. Never loop on a channel until it is empty while its
  producer runs.
- **Waking.** Send listing events through `OpenContext::channel` /
  `EventSink`, which wake the UI after enqueueing. Other background
  results call `App::wake` or a captured `waker_handle` after publishing
  (a handle calls the hook installed when it is called, so one taken at
  registration works). `Complete` ends the initial listing; updates and
  removals may still follow.
- A wake applies what the core sent (`Shell::sync`), then asks for a
  draw; it never leaves the applying to the draw. A window the
  compositor is not showing (another workspace, minimized) is never asked
  to draw, so a media key or a track that ended must not wait for the
  window to be looked at. `render` syncs too, for what the user did
  since; a sync that finds nothing is cheap.
- A window draws only when something asks for a frame (a notify, a
  refresh, `on_next_frame`), on Wayland where gpui asks the compositor
  for a frame only then. X11 still polls at the display's rate. Each wake costs a draw: a source that wakes the core with nothing
  new is an idle redraw loop.
- **Watchers act on changes, never access.** notify reports a file
  opened (`IN_OPEN`), so a watcher that rescans on any event rescans
  forever on its own reads. Drop `EventKind::Access`, as `fs/watch.rs`
  and Usage do.
- **Remote paths (ADR-077).** A path may be on a server that stops
  answering, where `stat` waits for good. Work that reads one runs in its
  remote's lane (`fs::reach::lane`, `Scheduler::spawn_in`,
  `Shell::off_thread`, whose lane comes first), never holding a lock other folders share
  across the call; whether a path is remote is asked of the mount table
  in memory (`fs::reach`), never by reading it. What is read unasked
  there (thumbnails, read-ahead, walks below a root, Usage) follows
  `docs/invariants/remote.md`.
- **Cancellation.** The scheduler skips cancelled jobs before they start;
  running jobs check their token themselves. Use child tokens for work
  tied to the app or an open location, and cancel superseded requests.
  Accept only results for the current request (e.g. the generation check
  in `crates/ui/src/preview.rs`): cancellation alone does not keep stale
  results out. In the UI, work asked one at a time (a note, a picture, a
  search) is a `latest::Latest`: ask, forget, take, cancelled on drop.
- A worker idle a second after its jobs gives back what they freed
  (`scheduler::release_when_idle`, the allocator's `collect`, wired by the
  binary). A plugin thread that allocates and then waits keeps it: wait on
  the scheduler instead where you can. What another thread frees of what
  workers allocated stays with them until they run again: after letting
  go of something large they built, call `scheduler::release_on_workers`
  (the grammars, `code_preview`).
- **Pane-owned work** links to `Pane::location_lifetime`, so navigation,
  Back and closing the pane cancel it independently of the next render.
  Show locations through `App::show` (navigate, Back, Forward) and
  `Pane::replace_open`, so no navigation path misses this.
- Directory open, stat and watch setup run on workers too: a sleeping
  disk can block any of them. Opening failures arrive as listing errors.
- **A folder is listed once** while something shows it. A pane keeps
  open, beside its location, what its view also shows
  (`PaneView::kept_locations`: the columns view's folders above), kept
  current by watchers and pumped with the pane. Going to one adopts its
  listing as it stands; the folder just left waits until the next sync
  for the view to keep it or let it go (`App::keep_locations`). A view
  never opens a location of its own.

## Conventions

- Key bindings use gpui notation: `ctrl-shift-n`, `G` for Shift+G,
  `g g` for a sequence. gpui leaves out the Shift a symbol took: bind
  `ctrl-?` and `ctrl-+`; `ctrl-shift-?` parses and never fires.
- `when` is a space-separated list of tokens that must all hold:
  `selection`, `single`, `dir`, `file`, `empty`, `folder`, `search`,
  `sweep`, `place`, `refs`, `drops`, `creates`, `restores`, `completes`,
  and a place by its token (`pane`, `address`, `palette`, `sidebar`,
  `preview`, `menu`…, each listed once in `keymap::PLACES` with its key
  context and what F1 calls it). `a|b` holds when either does, `!a` when it does not.
- Menu `order` groups by its hundreds: 0xx open, 1xx edit (the file
  itself: its name, the clipboard), 2xx create, 3xx destructive, 4xx
  misc, 5xx metadata (tags, ratings, a picture's position), 9xx the end,
  after the submenus (Properties). The UI draws separators between
  groups.
- `ui.*` commands are carried out by the shell; a plugin may bind to
  them.
- Zoom keys come from one table (`keymap::ZOOM_KEYS`). F5 refreshes in
  every state. A key never changes meaning with the layout (the split's
  transfers are Shift+F5 and Shift+F6).
- What the pointer does has a key: the middle button's background tab is
  Ctrl+Enter; the right button's menu is the Menu key and Shift+F10;
  Ctrl+click is Ctrl+arrows then Ctrl+Space.

## Area rules

Binding. Read the file for the area before working in it.

| File | Area |
| --- | --- |
| [shell.md](docs/invariants/shell.md) | the window: toolbars, popups and focus, the top rows and trail, layout, address field, search pills, context menu, drag and drop, feedback and footers, details and links, the sidebar, keys, tabs, what the window remembers, the terminal panel, the player |
| [views.md](docs/invariants/views.md) | views: one look per entry, selection, cut entries, folders opened in place, the board, rubber band, grid, zoom, list columns, scrolling, plugin views, Usage |
| [locations.md](docs/invariants/locations.md) | locations: what the shell asks of them, places, trails and verbs, tag locations, the trash as a place, searches and narrowing |
| [notes.md](docs/invariants/notes.md) | tags, ratings and comments: stores, reading, writing, merging, categories, the tags index, companion files |
| [operations.md](docs/invariants/operations.md) | plans and steps, running a plan, staging, copies, volumes, the trash's operations |
| [commands.md](docs/invariants/commands.md) | declaring commands, agents and scripts, the permission layer, what leaves the machine, plugins from outside |
| [settings.md](docs/invariants/settings.md) | declaring settings, the dialog, plugin requirements, helpers' `--probe` and `--build` |
| [previews.md](docs/invariants/previews.md) | structured previews: inspections, archives, HTML reading mode, EPUB and email, code folds, the preview frame |
| [reader.md](docs/invariants/reader.md) | PDF readers: PDFium, Poppler and MuPDF workers, continuous scrolling, page coordinates, atlas eviction |
| [media.md](docs/invariants/media.md) | audio and video: GStreamer worker, frame slots, generations, fullscreen controls, the atlas |
| [accessibility.md](docs/invariants/accessibility.md) | what a screen reader hears: roles and names, built only while one listens, focus and the active descendant, lists, what is exposed, `just a11y-check` |
| [remote.md](docs/invariants/remote.md) | remote folders: reach and lanes, what differs on a network (thumbnails, refresh, walks, trash, references), addresses and their redirect, connectors, credentials, testing against servers on this machine |
| [indexes.md](docs/invariants/indexes.md) | derived indexes (designed, not built): every index an accelerator, coverage never overstated, scope, one read per file, pace, freshness, formats |
| [models.md](docs/invariants/models.md) | models (designed, not built): providers by task, the helper, what leaves the machine, answers are data, suggestions, plans from models |

## Layout

```
crates/core        skwir-core: entries, locations, commands, ops/plans, registries,
                   plugin host, scheduler, cache, settings, thumbs, metadata
crates/platform    per-OS adapters: MIME, icons, thumbnails, places, open, terminal
crates/ui          the gpui shell: views, panels, prompts, keymap, elements
crates/skwir       the binary: wires plugins and starts the UI
crates/*-worker    optional helper processes: pdf (PDFium, loaded at runtime),
                   poppler, mupdf (AGPL, ADR-014), djvulibre and djvu
                   (djvu-rs, pure Rust; ADR-061), media (GStreamer), archive
                   (libarchive), mermaid and model (pure Rust), module (a
                   plugin's WebAssembly module, Wasmtime), kmtp (a phone
                   KDE's kmtpd holds, as a FUSE folder; ADR-097)
crates/plugins/*   one crate per plugin
crates/plugin-api  what a plugin from outside Skwir is and speaks (ADR-045, ADR-046)
crates/extend      the host of plugins from outside: programs and WebAssembly modules
crates/plugins-outside/*  plugins from outside built here: the template, the hex
                   view as a module, a Netpbm thumbnailer
crates/bench       synthetic trees and the performance regression check
vendor/zed         gpui from Zed's main branch, patched (vendor/zed/README.skwir.md)
vendor/xdg-mime    xdg-mime 0.4.0, built on every system (ADR-037)
vendor/shared-mime-info  the type database the macOS and Windows packages compile
spikes/            the Phase 0 toolkit spikes, kept runnable
docs/              PLAN.md, STATUS-LOG.md, adr/, invariants/, plugins/, the guide
```

## Entry points

- `crates/skwir/src/main.rs` wires the plugins; `crates/ui/src/shell.rs`
  routes commands and keeps the panes' UI state. The built-in list, grid
  and columns views are in `crates/ui/src/views/`, not plugin crates.
- A new plugin needs the workspace dependency alias, the binary's
  dependency and registration, and an entry in
  `crates/skwir/tests/plugins_toggle.rs`. Extend that test's snapshot
  when adding a provider scheme or a new kind of contribution (its
  provider-scheme list is explicit).
- `Plugin::register(&App)` runs again on re-enable.
  `App::unregister_owner` withdraws registered contributions;
  `Plugin::on_disable` cancels work and releases resources held outside
  registries. Test disable and re-enable, with in-flight work when the
  plugin starts background tasks.
- Integration tests involving several plugins belong in
  `crates/skwir/tests/` (see `timeline_query.rs`), so plugin crates stay
  independent.

## Build, test, lint

The dev environment is the Nix flake (`flake.nix`, loaded by direnv).
Without an active shell, run `nix develop --command just <recipe>`. Its
Rust is the one `rust-toolchain.toml` pins (rustup follows it
too, in CI's macOS and Windows jobs): a new release's lints arrive only
with the commit that moves the pin and fixes them.

`just --list` describes every recipe. The usual ones:

```
just build / test / lint   default members (no native workers); tests via nextest;
                           lint is rustfmt --check + clippy -D warnings
just workers               build the Poppler, MuPDF, DjVuLibre, GStreamer and libarchive workers
just test-all / lint-all   every crate, workers included; lint-all adds check-cross
just check-cross           clippy for Windows and macOS from Linux (ADR-035)
just macos-sdk             Apple's macOS SDK, by xmac, for building for macOS (you accept Apple's licence)
just test-windows          the Windows build's tests under Wine (args go to nextest)
just bench-check           generate bench-data/ on demand and enforce budgets
just idle-pss              idle memory and first frame on this display (ADR-033)
just smoke                 the release window on an X display of its own (CI)
just a11y-check            what a screen reader hears of it, read over AT-SPI
just remote-check          remote folders against servers on this machine (sshfs, GVfs, KIO, rclone from nixpkgs; NFS and AFP in namespaces; AFC against a stand-in iPhone; servers found nearby on a network of its own)
just mtp-check             a phone plugged in over MTP, through a GVfs of its own (writes a folder there, removed after)
just screenshots           the README's screenshots, made again (docs/images)
just deny / notices        dependency licences and advisories; the packages' notices
just plugins               build the plugins from outside as modules (target/plugins)
just package-nix/deb/rpm/aur, package-try <distro>   Linux packages (ADR-040)
just package-windows       Windows' zip and installer, made on Linux, tried under Wine (ADR-074)
just package-macos         skwir.app for Apple silicon, made on Linux, looked over (ADR-075)
just ci-check              cheap routine GitHub CI (core/API, formatting, release tools)
just release-check         full required checks, locally or on demand (ADR-053)
just release-build/upload <tag>   verified Linux bundle / GitHub draft (docs/RELEASING.md)
just run [dir] / dist / flame [dir]   run the release build; build for packaging; profile
```

- **Done means the full checks are green:** `just release-check` runs
  `lint-all`, `deny`, `notices-check`, `test-all`, `smoke`, `a11y-check`,
  `poppler-check`, `mupdf-check`, `djvu-check`, `bench-check` and
  `release-tools-check`.
  Routine GitHub CI runs only `ci-check`; the full suite and Linux
  packaging are also manual workflows (ADR-053). A green routine CI alone
  is not the full gate. The native workers are outside `default-members`,
  so the default build needs no Poppler, MuPDF, DjVuLibre, GStreamer or
  libarchive; run the `-all` recipes before committing worker changes.
  `spikes/` is a separate workspace, not covered by the root checks.
- A dependency added or updated passes `just deny`: fix an advisory by an
  update, or ignore it in `deny.toml` with its reason. Then `just notices`
  makes the packages' notices again: they are kept with the source, in
  `tools/package/notices`, for the packages to copy (ADR-063).
- Clippy runs pedantic with the allowances in the root `Cargo.toml`; an
  allowance in code needs a reason in a comment.
- A new helper program is listed in `tools/package/programs` (and
  `tools/package/optional` when a package of its own holds it), in the
  packages' file lists (`crates/skwir/Cargo.toml`'s metadata, the
  PKGBUILDs: `skwir-git`'s functions stay the release's, which
  `release-tools-check` compares), in `tools/package/macos/programs` and
  `tools/package/windows/programs` where it ships, and its notices are made
  (`just notices`). It answers
  `--probe` (ADR-040, ADR-041) and `--build` (its `build.rs` includes
  `tools/build/identity.rs`; see `docs/invariants/settings.md`).

### Tests

- Run tests through nextest (`just test`), never `cargo test`. nextest
  runs `tools/test/isolate-xdg.sh` once per run (`.config/nextest.toml`),
  pointing every test's `XDG_DATA_HOME`, `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME`, `XDG_STATE_HOME` and `XDG_RUNTIME_DIR` at
  `~/.cache/skwir-tests/xdg`: one set of folders, shared by the tests of
  every run (a test wanting folders of its own makes them), so on Linux
  and under Wine a trashed fixture never lands in the user's Trash. On
  macOS and Windows skwir's folders and the trash are the system's, which
  these variables do not move (`docs/HELP-WANTED.md` says so to testers).
- Tests that need a real filesystem feature (trash, xattrs) use a temp
  dir under `~/.cache/skwir-tests/`, never `/tmp` (tmpfs) and never real
  user files.
- Use `AppConfig::ephemeral` for headless tests so settings and cache
  writes stay isolated. Await listing completion or events with a
  deadline, never fixed sleeps (`crates/core/tests/common/mod.rs` has
  examples).
- Test the user-facing command path for cross-plugin bugs, not just the
  helper that formats a string. Include synthetic special-character paths
  and assert the returned entries, not merely that parsing succeeds.
- Operation property tests verify that undo restores the original tree
  and redo the applied state, partial failures included. Applied steps
  can cancel out (two swaps): a nonzero applied count does not imply a
  different tree. Investigate and keep reproducible failure cases; a
  passing randomized rerun does not resolve an earlier failure.
- `just bench-check` enforces the microsecond budgets in
  `crates/bench/budgets.toml`; `bench-data/` is generated by
  `skwir-bench gen` and git-ignored. It does not check UI startup, frame
  times or idle PSS: `just idle-pss` measures idle memory and the first
  frame; frame times need the real-app checks below.

## Verifying UI changes

- Run the real app on `bench-data/images10k` (grid) and
  `bench-data/flat100k` (list); scroll; check that idle CPU returns to
  ~0% (`top -p $(pgrep -x skwir)`).
- **On the session (Hyprland):**
  `hyprctl dispatch 'hl.dsp.send_shortcut({mods="CTRL", key="2", window="class:org.skwir.skwir"})'`
  sends keys to the window; `grim` captures it (`-s 0.5` for the whole
  layout). Drags need a virtual pointer on the wlr protocol (recreate the
  Phase 3 dragtool from `wayland-protocols-wlr` if needed).
- **On a display of its own**, which never touches the session:
  `Xvfb :97 -screen 0 1280x860x24`, then run the app with
  `WAYLAND_DISPLAY` unset, `DISPLAY=:97` and
  `VK_ICD_FILENAMES=/run/opengl-driver/share/vulkan/icd.d/lvp_icd.x86_64.json`
  (NVIDIA's driver cannot create a surface on Xvfb; lavapipe can). Send
  input with `xdotool` (`nix build --print-out-paths nixpkgs#xdotool`),
  capture with `import -window root`. Point `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME` and `XDG_DATA_HOME` at scratch directories: settings,
  the cache and the Trash live under them.
- **Measure:** the first frame from the shell's log line; idle memory
  with `just idle-pss` (own share, driver and total by mapping); other
  memory from `/proc/<pid>/smaps_rollup` (PSS); corrections and traces
  with `RUST_LOG=skwir_ui=trace`. The binaries use a tuned mimalloc
  (`crates/alloc`, ADR-011) and heaptrack sees only glibc's malloc, so
  profile a build made with `--no-default-features`. Compare memory
  between builds back to back under the same conditions: PSS shares pages
  with whatever else runs.

## Shell hygiene

- Never put backticks inside a double-quoted `git commit -m "..."`: bash
  runs them. Write the message to a file and use `git commit -F`.
- `pkill -f <pattern>` matches the shell running it when the pattern is
  in your own command line; use `pkill -x name`, or find pids with
  `hyprctl clients -j`.
- Chain verification with `&&` before a commit, so a failing lint or test
  cannot be committed.
- `target/` grows without bound (cargo never collects stale dependency
  artifacts; one day of parallel agents left 141 GB on a root filesystem
  that also hosts `/tmp`). When builds fail with ENOSPC, check
  `du -sh target` and remove `target/debug`. Real-window checks delete
  their fixture trees under `/tmp`.

## Working as several agents

- Scaffold crates and register workspace dependencies and aliases before
  parallel work starts, so nobody edits the root `Cargo.toml`
  concurrently.
- Each agent owns its crates only. Plugin agents never edit `crates/core`,
  `crates/platform`, `crates/ui` or `crates/skwir`: they work around gaps
  locally and report the exact signature they wished for; the integrator
  applies core changes.
- Subagents never commit. The integrator verifies (lint, tests,
  bench-check) and commits per track.
- Reports state what was verified and how, measurements with units, and
  gaps. Never describe as working something that was not exercised.
