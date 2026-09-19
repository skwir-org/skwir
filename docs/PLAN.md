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

Status: Phases 0 to 4 built (2026-09-18). Phase 2's gate (one week of daily use) is still
yours to run. Phase 4 gate: thirteen plugins, each proven by a test to disable at runtime with
no trace in any registry and to re-enable; the trait surface did not change for the last
three plugins written (jump, tags, shell). Idle PSS with all plugins is 99.9 MB (budget 100).
Deviations recorded: the plan's `search` item is folded into `query`; `view-dual` is a UI
feature (per-pane state in the shell) rather than a crate; the UI surfaces (review modal,
prompts, treemap, timeline strip) live in `bibli-ui` per the Phase 3 decision; `edit-as-buffer`
uses `$VISUAL`/`$EDITOR` in a terminal instead of an in-app editor; query results are not
cached across opens (walk is fast, content search dominates); the query language's date parser
and the timeline's calendar math are duplicated (candidate for a shared crate); earlier
deviations (AccessKit, views in-crate) still stand. Preview follow-up (2026-09-18,
ADR-003): two additional toggleable plugins provide PDF/Office pages and seekable
video stills through installed tools. Initially Poppler replaced the deferred PDFium path;
Office conversion uses isolated LibreOffice on Linux. Real-tool smoke tests cover
PDF paging, video seeking, and nine Office/OpenDocument formats with synthetic
fixtures. Spreadsheet cell navigation and continuous video playback remain
future work. Lint, 113 default tests, four real-tool smoke tests, and all
microbench budgets pass. Manual checks exercised PDF paging, Office rendering,
video seeking, and scrolling the 10k-image grid and 100k-entry list. The Phase 4
PSS figure above predates this follow-up: with a 1334×1214 window and preview
closed on a three-file synthetic directory, this session measured 117 MiB PSS
(previous revision under the same conditions: 145 MiB). Both exceed the historical
100 MB idle budget; no increase was observed, but that UI gate remains open.
Warm first-frame samples were 132–151 ms; sampled idle CPU was about 1% of one core.
PDF latency follow-up: page turns retain the displayed page; fast lossless PNG
encoding and metadata reuse reduce a synthetic scanned page's backend latency
from 1777 ms to 99 ms (24 ms cached), with identical pixels (ADR-003).
PDF reader follow-up (ADR-004): optional PDFium sessions add per-page text
selection/copy, whole-document search, internal/web links and outlines, including
converted Office files. The native library is loaded only in a cancellable child
process; Poppler remains selectable and is the missing-dependency fallback.
Both engines share the Office conversion cache. State lasts for the selected
preview; password entry, OCR, editing and cross-page selection are deferred.
Synthetic renderer and full-adapter comparisons are in `docs/pdf-comparison.md`.
Resource verification also found preview textures surviving in GPUI's atlas;
page replacement and close now explicitly evict them. PDFium page buffers
transfer into GPUI without full-page copies. The unrelated operation
property test now accepts cancelling pairs of swaps while still verifying
undo/redo, with its failure seed retained. A shell-test output race is also
fixed by publishing its fixture record atomically. Lint and 119 default tests
pass; the native smoke checks cover PDFium, Poppler, video and nine converted
Office formats. UI checks exercised selection/copy, search, outlines, internal
links, rotation, and grid/list scrolling with synthetic inputs. The historical
UI memory budget remains an open gate; measurements include the child process
and are recorded separately from filesystem microbenchmarks. After 50 page
turns, bibli plus PDFium measured about 201 MiB PSS versus 177 MiB with Poppler;
both stabilized across turns. Fresh idle was 115–117 MiB, still above target.
Continuous document follow-up (2026-09-19): PDFium and Poppler share a toolbar
switch between single-page and continuous scrolling, also available as
`ui.reader_continuous` in the command palette. A virtual list draws visible
pages, retaining a logical reading anchor across mixed page sizes, resizing
and zoom. Rendering is serialized and cancellable; neighbouring pages are
prefetched and offscreen pixels are evicted over a 64 MiB cache target.
Visible pages and the original preview are retained independently of that
soft cache target. Single-page wheel scrolling pans zoomed content first,
then changes page at an edge; backward turns start at the previous page's
bottom. Text selection remains per page; search, links, contents and page entry
navigate within the chosen mode. No native worker protocol change is needed.
Validation: lint, all 153 default tests and every microbenchmark budget pass
(six optional native-tool tests skipped). Synthetic UI checks with both engines
cover wheel turns, continuous navigation through mixed page sizes, rotated-page
copy, search and mode switching. Additional checks exercise zoomed-page edge
turns, worker cleanup and the 10k-image grid/100k-entry list. These used a debug
build; the historical release UI memory gate was not remeasured.

Settings follow-up: a schema-driven modal dialog opens from the toolbar,
command palette, or Ctrl+comma. Scalar preferences of enabled plugins have
boolean/choice controls, text/numeric inputs, per-field defaults, Save and Cancel.
Saved-query collections remain managed by query commands. The `settings.apply`
command validates whole batches and atomically persists only edited keys on a
worker before publishing values; failures retain the draft. Document and thumbnail
providers refresh through an explicit plugin settings hook, so engine changes
apply without restarting bibli. This adds one defaulted method to the Phase 4
plugin trait. The dialog uses the existing shell rather than a separate native
window and does not yet expose plugin enable/disable controls. Lint and 126
default tests pass, including failed-save recovery, batch validation, queued
save/disable handling and provider refresh. Real UI checks used isolated config
and synthetic documents: backend switching with an open PDF, persistence across
restart, defaults, draft cancellation, numeric errors, text with spaces, and
keyboard scrolling through the dialog. The 10k-image grid and 100k-entry list
were also scrolled. All filesystem/thumbnail microbenchmark budgets pass. After
opening and closing Settings in a fresh release window on four synthetic files,
PSS was 106.9 MiB and idle CPU sampled 0.8% of one core over five seconds. The
historical 100 MB UI memory gate remains open.

Poppler reader follow-up (ADR-005): a separate `bibli-poppler-worker` keeps
Poppler/GLib/Cairo outside the UI process and implements the same document
protocol as PDFium. Both engines share the session manager, reader controls,
selection/copy, search, links and outlines, including converted Office PDFs.
Auto prefers PDFium then the Poppler reader, with the legacy CLI image path
only when neither reader is available. Explicit engine selection requires the
selected worker. The default Nix shell supplies Poppler build/runtime libraries;
PDFium remains an optional runtime. Native parity checks cover all rotations
with cropping, text/search/link geometry, named destinations, empty pages,
missing outlines, limits and corrupt PDFs. CI runs the Poppler fixture suite.
Validation: lint, 126 default tests and five installed-renderer tests pass,
including nine Office formats with both readers. Real UI checks covered copy,
search/rotation, outlines, links and grid/list scrolling. Microbenchmarks pass
on an isolated rerun (an initial image listing sample missed by 135 µs; budgets
are unchanged). After 50 page turns, combined bibli/worker PSS stabilized near
185 MiB for Poppler and 191 MiB for PDFium; closing either preview reaped its
worker. Equivalent-feature timing ranges, CPU/PSS and measurement limitations
are recorded in `docs/pdf-comparison.md`. The existing UI memory gate stays open.

Toolbar follow-up: the application groups view, pane and analysis controls with
bundled symbolic SVGs; navigation sits beside each pane's address. PDFium and
Poppler share one top reader toolbar with page entry, zoom presets, Fit page/width,
search popover, contents drawer, copy and external opening. Intrinsic measurement
in prepaint selects compact rows and overflow controls without render-time width
feedback. Shared buttons dispatch commands, preserve focus when disabled and use
GPUI's keyboard click handling. Pane address controls have separate element IDs.
UI verification uses isolated settings and synthetic PDFs, covering compact/full
rows, page validation, rotated pages, search, outlines, menu keyboard activation,
split-pane Back/Up and grid/list paging. Lint, all 126 default tests and every
microbenchmark budget pass. A fresh 654×1214 release window on four synthetic
files measured 104.7 MiB PSS, 0.6% idle CPU over five seconds and a 147 ms first
frame. This is a single sample at a different window size, not a before/after
comparison; the historical 100 MB UI memory gate remains open.

Preview toolbar follow-up: images and video stills now share the PDF reader's
icon buttons, zoom presets, Fit image/width and adaptive overflow menus. Text,
folder, error and unsupported-file previews share the header, Close and Open
externally actions. Both side panels and peek use one preview frame; closing
peek preserves an open side panel, and changing files clears preview menus.
PDF-specific tools remain in the reader while zoom and popup state are shared.
Image Fit width now uses the source aspect ratio for layout, avoiding GPUI's
intrinsic pixel-height letterboxing. Synthetic UI checks exercised image fit,
video seeking in narrow panels, text, error states, peek, external opening and
PDF search. Lint, all 126 default tests and every microbenchmark budget pass
(five optional native tests skipped). Release checks also exercised the
10k-image grid and 100k-entry list. With the latter loaded, PSS was 160.2 MiB
and idle CPU sampled 0.8% of one core over five seconds; this is not a fresh
startup measurement. The historical UI memory gate remains open.

Popover follow-up: Search, Zoom/Fit and overflow panels anchor below the visible
trigger, measured during the current prepaint, and clamp to window edges.
Responsive rows use More when the original control is hidden. Peek now stays
in the normal overlay tree so its popovers do not nest deferred draws, which
GPUI rejects; modal dialogs remain above them. Synthetic UI checks exercised
image zoom menus and PDF Search in peek and narrow/resized side panels.
Lint, 126 tests and microbenchmarks pass. One test run timed out waiting for
PDF-worker cleanup; the unchanged test passed in isolation (157 ms) and in
the full rerun (184 ms). Release checks covered peek menus and grid/list
paging; after these checks the loaded 100k-entry list used 173.9 MiB PSS with
0.8% idle CPU over five seconds. The existing UI memory gate remains open.

Tag-copy follow-up (ADR-006): the default-on `core.preserve_tags_on_copy`
setting applies to direct copies, clipboard paste, shelf copies and external
drops. Copy plans serialize their preference; recursive copies retain raw
file/directory tags before restoring read-only permissions. A private staging
tree is published only after metadata succeeds, with cleanup on failure.
The tag provider and copier share the xattr key without a plugin dependency.
Synthetic command tests cover both settings, nested tags, read-only files,
symlinks, serialization, idempotence, undo/redo and failed-copy cleanup.
Lint and 129 tests pass (five optional native tests skipped). A debug UI check
verified the default-on toggle, saving both values and persistence after restart;
grid/list paging was also exercised. Initial full-suite microbenchmarks missed the
12 ms image-streaming budget: 13.0 ms with this change and 14.9 ms on the
unchanged commit in a separate checkout. Isolated 15-run image measurements
were below budget for both (3.4 ms and 2.5 ms minima respectively). The final
full-suite run passed all budgets after a competing release build finished;
budgets were not changed.

Usage view follow-up (ADR-007): Usage is a plugin-controlled pane view, with
registry-driven toolbar/palette entries and per-pane state. Folder navigation
keeps the view, and selection uses the shared open/preview/context actions.
The lazy worker caches bounded directory breakdowns, reuses unchanged branches,
and watches scanned subtrees for changes. Coalesced notifications invalidate
affected branches and ancestor totals; unavailable monitoring uses an explicit
stale warning and periodic revalidation. Generations and cancellation prevent
superseded requests publishing, and plugin disable releases worker resources.
Ctrl+R forces recalculation without changing the existing split-pane F5 copy
binding. Toolkit-dependent rendering stays bundled in `bibli-ui`; an external
view SDK remains deferred. Lint and 139 tests pass (five optional native tests
skipped), including cache reuse, deep mutations, watch expansion/root replacement,
event bounds, eviction, alias replacement, symlink cycles, cancellation and view
registration lifetime.
Notification follow-up: events now refresh only intersecting panes, fallback
deadlines belong to each active path, and lost coverage is re-established before
reusing data. Progress checkpoints delay the visible updating indicator by
200 ms; queued/running status remains available to command callers. Regression
tests cover unrelated events, isolated alias fallback, independent deadlines and
delayed/cancelled progress. A fresh debug UI check verified live totals after an
edit and paging the image grid and large list. Settled Usage measured 133.5 MiB
PSS and 2.0% CPU over five seconds in that debug run.
Debug UI checks exercised folder double-click/keyboard navigation, independent
split views, file selection/preview and automatic updates after file edits.
Release checks covered the shared toolbar, Ctrl+R, and paging the 10k-image grid
and 100k-entry list. In a 1189×1334 window, Usage on a small synthetic tree used
116.0 MiB PSS and 1.2% idle CPU over five seconds; after grid/list paging the
settled list used 175.1 MiB and 2.2% CPU. A busy-session first-frame sample was
989 ms; these are observations, not a controlled before/after UI comparison,
and the existing UI budget gates remain open. An initial full microbenchmark
run passed all budgets. Later runs missed image streaming (21.6 ms against
12 ms); the preserved pre-change binary also missed it (20.1 ms). Isolated
15-run minima were 11.2 ms for this change and 13.6 ms for the older binary.
A full-suite 15-run check passed every unchanged budget (image streaming
10.4 ms). The final standard five-run `just bench-check` also passed every budget
(image streaming 9.7 ms). Earlier runs were variable; no budgets were loosened.

Progressive Usage follow-up: a shallow snapshot shows files and unknown folders
before recursive watch setup, then bounded snapshots publish growing subtree
totals at most every 200 ms. Unfinished blocks are striped and totals labelled
as counted so far; hover identity and colors remain stable as sizes reorder.
The initial unmonitored snapshot is reread after watch registration, and only
complete, readable breakdowns enter the cache. This improves time to useful
display; full totals still require the recursive traversal. Lint and 143 tests
pass (five optional native tests skipped), including early publication, exact
final totals, nested cache reuse, cancellation and watch-setup revalidation.
Debug UI checks on the synthetic million-file tree observed the initial blocks,
growing counts and completed chart, and confirmed readable striped labels;
the 10k-image grid and 100k-entry list were also paged. After those checks the
debug process used 228.4 MiB PSS and 2.2% idle CPU over five seconds, including
retained Usage state; this is not a controlled memory comparison. The unchanged
benchmark binary initially missed several image budgets (streaming 24.0 ms
against 12 ms); a full 15-run check passed every unchanged budget (9.1 ms image
streaming). Timing variability and the historical UI memory gate remain open.

Progressive refresh correction: a finished snapshot now remains visible through
subsequent scans, including fallback revalidation. Automatic refresh publishes
no intermediate progress; explicit refresh reports progress over the retained
chart. Both replace it atomically on completion. Snapshot completion is tracked
separately from incomplete totals due to read errors. Lint and 144 tests pass;
the new regression covers repeated automatic refresh, manual refresh and read
errors without relying on filesystem timing. An isolated debug UI check through
a synthetic directory alias confirmed identical completed-chart screenshots
across fallback scans and retention during Ctrl+R. The standard `just bench-check`
passed every unchanged budget. Fallback scanning itself is unchanged and remains
necessary when monitoring is unavailable.

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
   are a location. Saved queries = smart folders.
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
