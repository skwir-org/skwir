# bibli — Implementation Plan

Experimental file manager in Rust. GPU-accelerated UI, tiny core, everything else a plugin,
designed from the first commit to be driven by humans, scripts, and AI agents alike.

Principles (non-negotiable, every phase is checked against them):

1. Always responsive: no blocking work on the UI thread, ever. Everything streams, everything cancels.
2. Light: idle RSS and startup time are budgeted and measured in CI.
3. Stateless and idempotent: no daemon, no database. One throwaway cache. Re-applying a plan is a no-op.
4. Harness the environment: reuse OS thumbnails, icons, MIME db, bookmarks, indexes, installed tools.
5. Core = only what two plugins must agree on. Everything else is a plugin with a runtime toggle.
6. Commands are data. Plans before apply. Stable entry identity. Metadata carries provenance.

Durations are relative sizes for one developer working with an AI assistant, not commitments.
Each phase has a **Done when** gate. Do not start the next phase before the gate passes.

Status (2026-09-20): Phases 0 to 4 are built; Phase 2's gate (one week of daily use) is
still yours to run. Phase 4's gate holds: every plugin (now 26, including nine structured
preview plugins) is proven by `crates/bibli/tests/plugins_toggle.rs` to disable at runtime
with no trace and to re-enable. Beyond the plan, the tree now has: a settings dialog
(ADR-010: category column, search, controls chosen by the schema, changes applied
immediately through the atomic save, plugin toggles with tool requirements, a read-only
shortcut list); two native PDF readers (PDFium and
Poppler, ADR-004/005) with continuous scrolling; embedded video and audio playback through
a GStreamer worker (ADR-008) with a vendored atlas fix (patch 0003); ten structured preview
plugins (ADR-009); tag-preserving copies (ADR-006); and Usage as a persistent plugin pane
view with cached, watched, progressive sizes (ADR-007). Recorded deviations: `search` is
folded into `query`; `view-dual` is per-pane shell state rather than a crate; the review
modal, prompts, treemap and timeline strip live in `bibli-ui`; `edit-as-buffer` uses
`$VISUAL`/`$EDITOR` in a terminal; the query language's date parser and the timeline's
calendar math are duplicated; AccessKit is unavailable in gpui 0.2.2 (ADR-001); the native
workers are workspace members outside `default-members`, so the default build needs no
Poppler, GStreamer or libarchive. Open gate: idle PSS is about 118 to 120 MB against the
100 MB budget of ADR-002 on the reference machine (see the ADR-002 addendum); it has not
been loosened. The per-change validation history that used to sit here is in
`docs/STATUS-LOG.md`.

Audio and video (2026-09-25, ADR-016): `preview-audio` is folded into
`preview-media` ("Preview: audio and video"), which declares every playback
preference, the shell player's included (keep playing while you browse, on by
default; play on selection; stop, play next or shuffle when a track ends).
Settings pages follow their subject (ADR-010 addendum): the reader's
preferences, shared by several preview plugins, moved from Appearance to a
Reading page (`reading.*`), and a moved key keeps its value
(`Settings::rename`). The Plugins page lists plugins under the category
each declares (`Plugin::category`: browsing, files, previews, desktop), which
`plugins.list` also reports.
Huge folders (2026-09-25): the UI applied a listing in one drain that lasted
as long as the worker kept sending, holding the UI thread and the workspace
lock (0.9 s for the 366 000 entries of `/nix/store`, warm cache, release; longer
on a slow disk), so no frame, banner included, was drawn. The drain is now a
slice per sync (`PUMP_BUDGET`) and arrivals are placed in the sorted order
instead of sorting again: the same folder draws its first rows at once, shows
"Loading location…" with a growing count, and is complete in 0.75 s.
The columns view no longer lists its folders a second time: the pane keeps
the folders above open beside the one shown, and going up or down adopts the
listing already there (`App::keep_locations`), so `/nix/store` comes back at
once, complete, and its column is whole as soon as a child is entered.
Bookmarks (2026-09-25): beyond adding, the sidebar's bookmarks can be
renamed, removed and put in order (a right-click menu that each place fills
through `Place::menu`, and dragging one bookmark onto another), every edit
answered by a toast with Undo and then Redo. Ctrl+D and the background menu
bookmark the folder shown, and a button in each pane's row shows whether it
is bookmarked and toggles it. The GTK file is watched, so edits made in other
file managers appear, and a bookmark whose folder is gone is dimmed with the
reason. Edits keep the lines they do not touch, remote bookmarks included.
Open With (2026-09-25): the `open-with` plugin lets the user choose the
application that opens the selection (Open With…, Shift+Enter) or the folder
shown (Open Folder With… on the background). The applications come from the
desktop's entries and `mimeapps.list` files (`bibli_platform::apps`): the
default first, then those that declare the type, then those that open a
parent type (plain text for a C source), then every other application, read
on a worker and read again when an install or a changed default moves the
stamp. The list is the shell's `ui.choose`, a filterable picker a plugin
fills with choices or with a command that lists them, as `ui.ask` asks for a
line of text. The picker's switch, "Always use for PNG image files" (Alt+A,
offered when the selection is one type), also makes the chosen application
the type's default: `bibli_platform::apps::AssociationEdit` rewrites the
user's `mimeapps.list` (the one that already decides, a desktop's own list
first), keeping the lines others wrote, and returns the edit that reverts it,
which the toast offers as Undo, then Redo, as bookmark edits do; both files
go through `file::rewrite`, which follows links and refuses a read-only one.
Saved searches (2026-09-25): kept like bookmarks, under their own Searches
heading below Bookmarks. They stay in bibli's settings (`query.saved`), not
in the GTK file, which every GTK file chooser reads and would show as broken
rows; one list mixing both would need an order file of our own over two
stores. What is shared is the gesture: Ctrl+D and the pane's button run the
core `keep` command, which hands the location shown to the plugin that keeps
its scheme (`registry::Keeper`: the places plugin for folders, the query
plugin for searches), and the button is filled while a place with a
`forget` command is the location shown. A search is saved under a name made
from it (`ext:pdf in Documents`) and, from its right-click menu, renamed,
edited (its name and place stay) or removed; any place with a `reorder`
moves by dragging within its group. Each edit is a `saved::Edit` applied on
a worker that returns its inverse, offered as Undo, then Redo. A saved
search is known by its expression, as a bookmark by its folder, so
`query.save` and `query.forget` changed: saving no longer replaces a search
of the same name, and forgetting takes the expression (or, as before, the
name). A theme icon that exists only as a `-symbolic` variant, as
`folder-saved-search` does in current Adwaita and Breeze, is used as GTK
does, drawn in the text color (`icons::themed`; the worker that finds it
reads it, so drawing it never reads the disk). The background menu of a
search or the shelf no longer offers what needs a folder (New Folder,
Bookmark This Folder, Open Folder With, Edit as Text, Usage, Timeline,
Search here): those items say `empty folder`, a token that holds while the
pane shows a folder. The text input scrolls a line longer than its field
to keep the cursor in view and clips it to the field, so a long search's
address no longer runs under the pane's buttons; unfocused, it shows its
start. A `when` token may list alternatives (`selection|folder`, a GPUI
`(selection || folder)` for keys): Open Terminal Here and Run Command say
so, and work in `App::work_dir`, the folder shown or, in a search, the
nearest folder holding the selection; Paste, always into the folder
shown, says `folder`. Run Command asked for no command and failed
everywhere; it now asks through `ui.ask`.
Query words for people (2026-09-25), the first step toward a query designer
that needs no syntax: `kind:` (`kind:image,video`, `kind:documents`,
`kind:music`) groups files as the desktop's MIME database does, from each
type's generic icon (`x-office-document` covers PDF, EPUB, Markdown and word
processor files; `package-x-generic` archives) and from descent from
`text/plain` for text and code (`bibli_platform::mime::Kind`, cached per
type); and named periods (`modified:today`, `this-week`, `last-month`,
`this-year`, with `<`, `>=` and ranges as for dates) are calendar periods in
UTC like the dates, weeks starting on Monday. The search prompt's example
uses both.
Dates are the user's (2026-09-25, ADR-017): the query text records no zone;
dates, named periods and the timeline's bars are days of
`bibli_platform::calendar::Calendar`, the system's time zone with its clock
changes (a day of 23 or 25 hours, a midnight that does not exist) and the
locale's first weekday, asked of `locale` once on a worker. Durations are
counted on the calendar (`7d` across a clock change is seven days; `1mo`
works). A search showing a named period lists again when it moves on,
checking the wall clock at least every ten minutes so a sleep past midnight
is not missed. Saved searches with dates shift by at most the zone's offset;
the timeline's bars, labels and the queries they become now name the same
day.
Search chips (2026-09-25), a query designer that needs no syntax: a pane
showing a search has a row of chips under its address row (Name, Kind,
Modified, Size, Tag, Contains, In <folder>), each opening its choices in a
popover anchored to it (the kinds, periods from Today to Older than a year,
sizes, Custom… for a value to type). A choice refines the search and moves
the pane, so results follow every click and Back undoes it; a set chip
reads its value and has a × and a Remove item. An empty search (Enter with
nothing typed) is every entry below the folder, which the chips then
narrow. The query plugin owns the meaning (`query.describe`,
`query.refine`, `facets.rs`), so the CLI and agents get the same chips as
data; a part no facet holds (an OR across facets, a NOT, `ext:`) is a chip
of its own showing its text, so nothing typed is lost. The Tag chip picks
from the tags on the files the search found (`query.tag_choices` through
`ui.choose`, reading them with `tags.list { read }` on a worker; there is no
index of tags to ask, by design), most used first, or one typed; with the
tags plugin off it asks for one. Find Similar, on a file's menu, searches
where the pane searches for files of the same kind, modified the same day,
of about its size (half to twice) or sharing a tag, built on a worker from
the file's own properties. A keyboard click fires only on the element its
key went down on (vendored patch 0007): a picker closed with Enter gave the
focus back to the chip that opened it and the key's release reopened it.
Tabs (2026-09-26, ADR-018): a tab is an arrangement of panes, one or two
when that tab is split, and the core workspace owns them (`Workspace::tabs`,
`bibli_core::tabs`, the `tab.*` commands, so scripts and agents list, open,
show, move and close them). A tab not shown is dormant: its panes keep
history, sort and selection and let go of listing, watchers and kept
columns; showing it lists again and gives the selection back once the
listing is complete, scrolled to. The strip appears above the panes with
two tabs, names each after its place or folder, takes drops like a sidebar
row, reorders by dragging, and closes with its ×, a middle click or
Ctrl+W, answered by a toast with Undo (Ctrl+Shift+T reopens with the
history). Middle-click on a folder, a place or a breadcrumb, or "Open in
New Tab", opens it behind the current tab. With `core.startup = last` the
tabs come back at startup, only the shown one listed. The shell no longer
makes or drops a pane's UI itself: `Shell::sync_tabs` follows the
workspace's revision, which also gave closing a split one release path.
Two keys moved (the user's choice): Open Terminal Here from Ctrl+T to
Shift+F4, Tags… from Ctrl+Shift+T to Ctrl+Shift+G. Measured: ten dormant
tabs cost no PSS beyond noise over one (ADR-018 addendum, now a budget).
Switching tabs no longer draws a blank or half-listed pane (ADR-018
addendum): the tab left stays drawn until the tab shown is listed, has its
selection, scroll and first rows' metadata back, for at most 150 ms; every
measured switch (up to 100 000 entries, 89 ms) was ready within it.
Open: `listing::pending_opens_report_errors_asynchronously_and_can_be_replaced`
is flaky ("cancelled open must not publish late events"): it failed eight
runs in a row at the parent commit and on this work, then passed in later
full runs; not investigated here.

---

Video playback follow-up (ADR-008): `preview-media` now opens an optional
GStreamer worker session, initially paused, with play/pause, seek dragging,
volume/mute, speed, audio/subtitle tracks, fullscreen and keyboard controls in
the shared preview frame. `auto` retains FFmpeg still previews when playback is
unavailable; explicit GStreamer selection reports missing dependencies. The Nix
shell includes playback plugins and `just run` builds the worker. Frame delivery
uses one reusable RAM-backed slot and a latest-frame mailbox; provider disable,
selection changes and closing cancel the session without waiting on the UI.
The worker owns A/V synchronization, and events wake the UI. Output is capped
at a 1920-pixel side; native-resolution 4K output, GPU buffer sharing, external
subtitle-file selection and a second backend are deferred. The synthetic check
in `tools/media/check.py` covers paused seeking, two audio tracks, embedded
subtitles, volume, rate, end-of-stream and replay. Validation and measurements
are recorded in `docs/video-playback.md`. Lint, 148 default tests and the
filesystem/thumbnail benchmark gate pass. Native checks cover both resolutions
and the real audio backend with muted output; audible lip sync is unverified.
The historical UI memory gate remains open. Native checks also compare paused
pixels with subtitles on/off. Track changes re-preroll and restore position and
play state, avoiding paused output-reconfiguration races.

Video-switching crash follow-up: rapid selection changes exposed a Blade atlas
lifetime bug in vendored GPUI. An evicted texture could still have queued
initialization/uploads, and its index could be reused before those uploads ran.
Patch 0003 cancels queued work before recycling the index and retires native
textures after the previous GPU submission completes. Two explicit GPU
regressions cover removal before presentation and reuse by a smaller texture;
`just atlas-check` runs them without adding GPUI dev dependencies to the workspace.
Lint, 148 default tests, both GPU tests and the benchmark gate pass. The release
app survived 320 rapid video selections and reaped its workers on preview close;
the unpatched build reproduced the reported panic after 49 switches. Scrolling
50 pages each in the 10k-image grid and 100k-entry list also passed; the final
five-second idle CPU sample was 1.4% of one core.

Video interaction follow-up: seek intent remains visible across mouse release;
both status and frame messages reject older command generations. Flushing seeks
retain the target until the entire pipeline finishes preroll. Initial paused
previews retain the shared thumbnail service's image until Play or Seek.
Fullscreen uses a black, aspect-preserving canvas that upscales small sources,
with an overlaid toolbar collapsed by its button or C. Real-window checks verified
320×180 upscaling to a 2560×1440 display, black side bars for 320×240 video,
controls, restoration of window size, and a stable seek bar across mouse release.
Five repeated native seek/track tests plus a 1080p run passed. Lint, 148 default
tests and all microbenchmark budgets pass; the optional native-tool tests remain
skipped in the default suite.

Video poster/controls follow-up: fullscreen controls use a cancellable one-second
inactivity timer; pointer movement reveals them, while hover, scrubbing, open
menus and keyboard focus prevent hiding. A cached native-resolution FFmpeg
poster replaces the thumbnail asynchronously, bounded to 8192 pixels per side
without upscaling. Starting playback or seeking cancels extraction and rejects
late results. Posters sample a representative timestamp independently of the
desktop thumbnail, whose timestamp is unavailable. Native checks verify exact
3840×2160 output and cache reuse, no upscaling of small input, and cancellation.
The separate Hide/Show button is removed. Real-window checks cover idle and
playing video, movement resetting the deadline, hover protection, menu focus
and dismissal, and fullscreen exit. Lint, 148 default tests, two explicit native
video tests and the benchmark gate pass.

Reading fidelity (2026-09-24, ADR-012): reading mode keeps the part of a
document's CSS that says how text reads, through a cascade over the parsed tree
with the selector engine scraper already uses: a book's class-borne italics,
centred titles, verse indents and hidden text now show, in the reader's theme.
XHTML chapters are read as XML (a self-closed `<title/>` emptied the chapter);
every `id` is a link target, footnotes gather at the end of their chapter and
page numbers leave the text; SVG cover pages and drawings show; a fixed-layout
book of pictures shows page by page. A book's or a document's pictures are
decoded as they come into view (ADR-012 addendum), so a chapter shows every
illustration it has. Superscripts, subscripts and small capitals are drawn
(vendored gpui patch 0006: a run's own size and height in its line), and a
note reference opens its note in a popover, from another chapter too.
Fixed-layout pages are drawn (ADR-013): a converted comic's lettering, set
over its picture in the book's fonts, comes out in its balloons. MuPDF is a
third, optional reader engine (ADR-014, AGPL worker), chosen explicitly and
compared with PDFium and Poppler in `docs/pdf-comparison.md`. Lint and the
default tests pass; a synthetic book was checked in a window on an isolated
display, including following a footnote and its way back. MuPDF is also a
book engine (ADR-014 addendum, `preview-epub.backend`, built-in by default):
it lays books out into pages for the reader, with the desktop's fonts
(fontconfig) for named, generic and uncovered-script text. Ctrl++ / Ctrl+-
work on every book: reading mode sizes its text, a drawn page or a comic
is shown as a picture (Fit by default, Fit width, zoom, as images are,
never in the reading column), and a MuPDF book is laid out again at the new size and reopened on
the page holding what was being read (`Locate`). Deviation: the crate's
`system-fonts` feature was dropped for a loader of our own, because its
`font-kit` broke the whole-workspace build. Verified in a window on
synthetic books and a comic; `just mupdf-check` covers layout and locate.
The reading column's width is the Reading width preference (Narrow,
Comfortable, Wide, Full width), shared by books and Markdown, with a Full
width button beside the text size, where a document has Fit width; zoom steps on any picture start
from the scale Fit shows, so Zoom out never enlarges. A picture zoomed
past its pixels (a drawn page, a comic's page, an image) is drawn or
decoded again at the side it covers, in 512 px steps up to 4096 px, where
it is; 100% is its own size, not its pixels (an image's Details now give
its real dimensions). Verified in a window on synthetic pages: a drawn
page at 400% redrawn at 3584 px, a 3000 × 4500 comic page at 100% at
4096 px, a 5000 × 3000 image at 48% at 2560 px. Then (ADR-015) books of
pages and comics became documents: a fixed-layout EPUB and a CBZ open in
the reader, continuously or a page at a time, through in-process sessions,
and the inspector's page view went. The reader renders every page at the
side it covers (512 px steps up to 4096 px, for every engine) instead of a
fixed 2048 px, and a page's size is its own (CSS pixels), so 100% is real
size. Verified in a window on synthetic files: a fixed EPUB and a comic in
both modes, a zoom from Fit redrawing pages at 1024 to 2560 px, a PDF at
100% at its size in points; all three PDF engines pass `tools/pdf/check.py`. The
reading toolbar now mirrors the reader's: a chapter field between previous
and next (the reader's page field, shared), a text size picker between
smaller and larger, Full width where Fit width is, and the rendered/source
switch last in the view group, where Continuous is. The desktop's
thumbnail no longer flashes before a preview: it waits aside and stands
in only once a load has lasted 200 ms, where the preview will be (an
image's at its own size, a document's as its first page), and stays as
the picture of a file with no preview of its own. A book's Contents are its
table of contents alone (no landmarks, no printed page list, no second
NCX copy), and a book that lists its printed pages can be counted by them
in the toolbar (`ui.printed_pages`); the reader does the same with a PDF's
page labels and a fixed-layout book's page list, read by all three
engines.

Music (2026-09-22): the audio preview became a small player. Sound: a
perceptual volume, `scaletempo` so speed keeps the pitch, a spectrum folded
into logarithmic bands. Tracks: Previous and Next through the folder's audio
files with hand-over at the end of a track; playlists (M3U, PLS, XSPF, CUE) as
a plugin whose tracks open in the player, which then walks the playlist. Keep
playing: a pin that lets playback outlive the selection, steered from the
status bar. What plays is core data (`playing::NowPlaying`, `PlayerRequest`),
which an `mpris` plugin mirrors onto the session bus: media keys, the
desktop's widget, `playerctl`. ReplayGain is a
setting, and a pinned player hands over without a gap: one worker carries the
queue, told the next file ahead of time. Verified in a window on
an isolated display and, for MPRIS, on a private session bus with `playerctl`.

One preview frame (2026-09-21, `docs/preview-frame.md`): the structured
previews were built apart from the reader, the player and the picture viewer,
and it showed. What is read is now on paper rather than the panel's grey; one
toolbar builder places every kind's tools in the same slots, with a More menu
that holds only what has no button; find, the contents drawer and the keys are
one each; every text file is the code view and HTML opens in reading mode;
Details lists everything known about an entry, filling the panel when there is
nothing to preview; a member of an archive or an attachment opens as the file
it is, two containers deep, which lifts the "nested sessions in containers"
limit noted below; tables have a grid, sized columns and sorting, the byte view
decodes the selected row. Deviations: no EXIF (no metadata crate in the tree),
no folder total from the Usage cache (a plugin the UI cannot depend on), no hit
count in the structured find field (providers search as they go). 283 default
tests and the bench budgets pass; every kind was checked in a window on an
isolated display.

Extended previews: ten independent runtime plugins add audio,
hexadecimal, Markdown reading mode, archives, EPUB reading mode, CSV/TSV tables,
JSON/YAML/TOML trees, font specimens, email and comics. Audio reuses GStreamer;
archives/comics use the optional libarchive helper. Semantic content is rendered
by a shared virtualized GPUI viewer. EPUB publisher CSS/fixed layouts, encrypted
archives, full-file audio waveforms and nested PDF/video sessions in containers
remain outside this initial implementation. See ADR-009 for dependencies, bounds
and behavior; `docs/previews.md` describes controls and installation. Strict lint,
163 default tests (six optional tests skipped), and all microbenchmark budgets
pass. Native checks exercise archive member reads, output limits and symlink
rejection, plus audio tags, spectrum, playback, seek and pause with a fake audio
sink; the paused audio worker measured 0.0% CPU and 12.6 MiB PSS. The existing
native video regression also passes. Real-window checks cover all ten formats,
text selection, search focus, EPUB links, section navigation, archive members,
worker cleanup and scrolling images10k/grid and flat100k/list. A settled debug
UI sample after the latter measured 2.2% CPU; this does not establish the release
UI idle/startup budgets or audible playback quality on a real device.

Archive presentation follow-up: contents use the file list's row density,
themed icons, aligned human-readable sizes, folder-first ordering and a summary
footer, with expand/collapse-all controls. Lint, 164 tests and isolated X11 UI
checks pass, including narrow-panel truncation and member preview/back navigation.
An initial benchmark run exceeded images10k first-batch (2664 us / 2500 us budget)
and completion (20363 us / 12000 us) timings; the cause was not established.
The subsequent ASF validation run passed all budgets (1663 us and 11753 us).

ASF preview follow-up: the media provider recognizes application/vnd.ms-asf,
application/x-ms-asf and the .asf extension. Provider ownership controls startup,
including when a generic binary MIME would otherwise select the hex fallback.
The GStreamer worker reports audio-only streams and enables their spectrum;
video streams keep the existing player. Lint and 166 tests pass, plus synthetic
WMV/WMA ASF playback/seek/pause checks and the existing native video regression.


Structured preview follow-up: JSON/YAML/TOML use syntax-highlighted code lines,
line numbers and parser-derived folds, with expand/collapse-all controls. JSON
gets two-space formatting; TOML/YAML retain syntax, comments, order and indentation.
Source mode, search and selection/copy remain available, including blank lines.
Parsing/highlighting stay on workers; the viewer virtualizes visible lines and
preserves logical indices when folded. Invalid or oversized input falls back to
paginated source. Lint and 174 tests pass (six optional native tests skipped);
isolated X11 checks exercise all three formats, folding, source and search.
The TOML grammar loads separately from precompiled default syntaxes to avoid
rebuilding them on first preview. The first benchmark run exceeded images10k
completion (12966 us / 12000 us); a rerun after concurrent builds finished passed
all unchanged budgets (9897 us). UI startup/idle budgets remain unestablished by
these functional checks.

## Phase 0 — Foundations and toolkit spike (~2 weeks)

Goal: a repo that can measure itself, and a toolkit decision backed by numbers.

- Cargo workspace: `crates/core`, `crates/platform`, `crates/ui`, `crates/plugins/*`, `crates/bench`.
- Nix flake dev shell (this is a NixOS machine), `just` recipes, CI (lint, test, bench on Linux).
- Benchmark harness generating synthetic trees on demand (never committed):
  flat 100k-entry dir, 1M-file deep tree, dir of 10k large images.
- Written performance budgets, enforced by `bench` as CI regression checks:
  - cold start to first frame < 100 ms
  - 100k-entry directory: first rows painted < 16 ms after listing starts, scroll at display refresh rate
  - idle RSS < 100 MB with default plugins, identical with AI plugins disabled vs not compiled
- Toolkit spike: the same demo in **GPUI** and **Floem**: a virtualized 100k-row list with lazy
  thumbnails, on Wayland (Hyprland), on the Intel iGPU and the NVIDIA eGPU. Measure first paint,
  frame time while scrolling, RSS, and test external drag-and-drop and IME once.
- ADR-001 records the toolkit choice and the numbers. ADR-000 records the principles above.

**Done when:** budgets are documented, `bench` runs in CI, the ADR is written, and the winning
spike renders 100k rows at refresh rate on both GPUs.

---

## Phase 1 — Core kernel (~4 weeks)

Goal: `crates/core` is complete enough that plugins can be written against it, with no UI.

- **Entry model.** `EntryId` = (device, inode) on Unix, file id on Windows. Streaming listing via
  `getdents` batches (rustix), `d_type` used to avoid stat, lazy stat on demand. Entries carry a
  metadata bag: typed fields with **provenance** (Filesystem, User, Plugin(name), Model(name, confidence)).
- **Watching.** `notify`-based, per open location, coalesced, feeds the same entry stream.
- **Locations.** `LocationProvider` trait: string → entry stream. Only the path provider ships in core.
- **Commands as data.** `Command` = serde struct with typed args, registered by name, dispatched by
  the core. Keymap and palette are consumers of the registry. Nothing in a command captures UI state.
- **Operations and plans.** `Operation` trait with `apply` and `inverse`. `Plan` = ordered list of
  operations referencing `EntryId`s, with a dry-run `diff()`. Executor runs plans in the background
  with progress and cancellation and writes an undo journal. Re-applying an applied plan is a no-op.
  Built-in ops: copy (reflink when possible), move, rename (atomic exchange for swaps), trash, delete, mkdir.
- **Registries.** Views, panels, sidebar contributions, context-menu contributions, settings schema.
- **Plugin lifecycle.** `Plugin` trait: `register(&mut Registry)`, `enable`, `disable`. Compile-time
  crates, runtime toggle in settings. Plugins depend only on core, never on each other.
- **Cache and settings.** Content-addressed cache dir, deletable at any time. Plugins get a namespaced
  slot in cache and in settings, nothing else.
- **Work scheduler.** One priority queue: viewport-visible work first, everything cancellable.
  Provider calls are batched, async, timed; slow providers are surfaced.
- Tests: property tests that `inverse(apply(op))` restores the tree; idempotency of plans; listing
  benchmarks against the synthetic trees.

**Done when:** a headless test drives a plan (list, select, rename, trash, undo) through commands only,
and listing the 1M-file tree streams without ever blocking the caller.

---

## Phase 2 — Minimal daily-driver (~4 weeks)

Goal: replace the file manager you use every day, even if ugly.

- `crates/ui` on the chosen toolkit: window, layout with panes, theme tokens, command palette,
  keymap, address bar (paths only for now), status/progress area. **AccessKit wired in from the start**
  (blocked: gpui 0.2.2 has no AccessKit support, see ADR-001; re-check at every gpui upgrade).
- Plugins: `view-list`, `places` (XDG user dirs, GTK bookmarks, udisks2 volumes over D-Bus),
  `ops-basic` (the built-in operations exposed as commands with keybindings), `open-with` (xdg-mime),
  `undo` (journal UI).
- `crates/platform` Linux: shared-mime-info via the `xdg-mime` crate, XDG icon theme lookup,
  freedesktop trash, **read-only** freedesktop thumbnail cache lookup.
- Keyboard-first: every action reachable from the palette; mouse works but is secondary.
- Dogfooding starts here: use it daily, file issues.

**Done when:** you have used it as your only file manager for one week, budgets still pass,
and every action taken during that week went through a `Command`.

---

## Phase 3 — Visual richness and performance hardening (~4 weeks)

Goal: it looks and feels like the fast, GPU-native app it is.

- **Thumbnail pipeline.** `ThumbnailProvider` trait keyed by MIME. Freedesktop cache read first,
  generation on miss written back with spec-compliant naming (other apps benefit). Discovery of
  installed `*.thumbnailer` files. Decode on a rayon pool, viewport-priority, cancellable, GPU atlas upload.
- Plugins: `view-grid` (gallery), `view-columns` (Miller columns), `preview` (image, syntax-highlighted
  text, PDF via pdfium, video first frame; spacebar peek).
- Drag-and-drop: internal first, then external on Wayland; measured against the spike findings.
- Perf pass: profile with the synthetic trees, fix every budget regression, add flame-graph recipe.

**Done when:** the 10k-image directory scrolls at refresh rate with thumbnails appearing in
visible-first order, and all three views pass the budgets.

---

## Phase 4 — Power features, each as a plugin (~6 weeks)

Goal: the UX bets. Order is by leverage; each is independently shippable and toggleable.

1. `query` — query language (`*.pdf modified<7d size>10M`), a `LocationProvider`, so search results
   are a location. Saved searches are kept in the sidebar like bookmarks.
2. `search` — parallel subtree walk (`ignore` crate walker), results cached by directory mtime;
   uses `plocate` when present. Content search via the ripgrep crates.
3. `shelf` — collect entries from anywhere, apply one plan to all of them.
4. `rename-bulk` — patterns, regex, live preview; produces a plan.
5. `edit-as-buffer` — the directory listing as an editable text buffer, diff → plan (oil.nvim model).
6. `jump` — frecency jump reusing zoxide's db and recently-used.xbel when present; `nucleo` matching.
7. `shell` — open terminal here, mirror cwd, drop selection as arguments.
8. `tags` — xattrs using the XDG and Finder keys; tags are metadata with User provenance.
9. `view-dual` — orthodox two-pane mode over the same model.
10. `usage` — GPU treemap, btrfs-aware.
11. `timeline` — files by mtime as a scrubbable histogram.

By the end there are 10+ internal plugins. That count is the signal that the traits are ready to freeze.

**Done when:** every plugin above can be disabled at runtime with no trace in memory or UI, and
the trait surface has not changed for the last three plugins written.

---

## Phase 5 — Cross-platform (~6 weeks)

Goal: macOS and Windows builds that feel native in the ways that matter.

- `crates/platform` macOS: QuickLook thumbnail service, NSWorkspace icons and open-with, Trash,
  DiskArbitration volumes, Spotlight as a search provider, Finder tag xattr compatibility.
- `crates/platform` Windows: shell thumbnail cache, SHGetFileInfo icons, Recycle Bin, WM_DEVICECHANGE
  volumes, Windows Search as a provider, NTFS USN journal for fast subtree walks.
- File identity, path handling, and reflink/clone (APFS clonefile, ReFS block clone) abstracted in core.
- CI on all three OSes; budgets measured on each.
- Packaging: Nix flake and AppImage/Flatpak, dmg, msi.

**Done when:** the Phase 2 dogfood week is repeated on each OS, and the budgets pass on all three.

---

## Phase 6 — The extension boundary (~4 weeks)

Goal: open the plugin system beyond the repo, safely. Only now, because the traits are stable.

- **Permission layer.** Capabilities per plugin (read contents, write, network, spawn processes) and
  per-directory policies ("never leaves this machine" for `~/.ssh`, anything under a `.private` marker).
  Required before any model provider exists.
- **Level 2, external processes.** Small line-delimited protocol (LSP-style) for provider processes.
  Used first for thumbnailers and external tools we already call.
- **Level 1, WASM plugins.** wasmtime + component model for providers, commands, metadata. Not views.
- **CLI and MCP server.** Both are thin consumers of the command registry: `bibli cmd rename ...`,
  and an MCP server exposing commands and plans so an external agent (e.g. Claude Code) can drive the app.
- Plugin authoring docs and a template crate.

**Done when:** one existing internal plugin is re-implemented as WASM with no user-visible difference,
and an external agent completes a rename-and-move task over MCP that ends in a reviewable plan.

---

## Phase 7 — AI (~10 weeks)

Goal: agents that organize, rename, clean, tag, and summarize, always ending in a plan the user reviews.

1. `extract` — content extraction provider: text from PDFs/docs, OCR, EXIF, audio/video metadata.
   Cached by content hash. Uses installed tools where present. Benefits plain search immediately.
2. `cleaners` — deterministic: exact duplicates (hash), near-duplicate images (perceptual hash),
   empty dirs, stale downloads. No model. Each produces a plan.
3. `fulltext` — keyword search index on Tantivy (ADR-003), embedded, no service. One index per
   subtree, built on demand and only when asked, refreshed incrementally by mtime and content hash,
   stored in the throwaway cache, pausable and deletable. It feeds the query language's `content:`
   term when an index covers the query root; otherwise the existing scan runs, so the index is only
   an accelerator. Text comes from the `extract` provider. Existing system indexes (Tracker, Baloo,
   Recoll, Spotlight, Windows Search) and a running Meilisearch are optional providers for the same
   term, used when present, never required.
4. `models` — `ModelProvider` trait: chat with tool calling, embeddings, image understanding,
   transcription. Implementations: local runtime (lazy-loaded), auto-detected Ollama, remote APIs
   behind explicit opt-in with a persistent "leaves your machine" indicator. Idle cost when disabled: zero.
5. `models-small` — embeddings, CLIP-style image tagging/search, OCR, document classifier.
   Run on CPU or iGPU. These do the volume; the LLM orchestrates.
6. `semantic` — scoped, on-demand, incremental subtree index in the throwaway cache; visible and
   pausable. Natural language becomes a `LocationProvider` in the same address bar.
7. `agent` — the LLM plugin. Tools = the command registry. Output = a `Plan`. Never executes.
   Prompts treat file names and contents as untrusted input. Flows: organize this folder,
   rename these consistently, what is safe to delete here, tag the selection, summarize in preview.
8. Metadata from models carries `Model(name, confidence)` provenance and renders as suggestions
   until confirmed, at which point it is promoted to User provenance and written to xattrs.
9. MCP client, so the agent can use external tools the user configures.

**Done when:** "organize this folder" on a messy Downloads produces a correct, undoable plan using
only local models, and disabling every AI plugin returns idle RSS to the Phase 2 baseline.

---

## Phase 8 — Polish and release (ongoing)

- Accessibility audit (screen reader on each OS), IME, high-DPI, themes.
- Docs, plugin gallery, first public release with an opinionated default plugin set enabled.

---

## Cross-cutting rules

- Every phase ends with a benchmark run and an ADR for any decision that closes a door.
- Features enter as plugins by default. Promotion into core requires the "two plugins must agree" argument.
- No plugin hook may run per frame or per row. Batches and streams only.
- Any AI-derived data is a suggestion until a human confirms it.

## Immediate next steps (first week)

1. Init the workspace, flake, CI, and `bench` with the synthetic-tree generator.
2. Write ADR-000 (principles and budgets).
3. Build the GPUI spike, then the Floem spike, run both on both GPUs, record numbers.
4. Write ADR-001 (toolkit decision).
