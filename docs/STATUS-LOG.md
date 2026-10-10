# Status log

Verbatim record of the per-change status paragraphs that used to live at the
top of `docs/PLAN.md` (through 2026-09-19), kept for the measurements and
validation notes they contain. Newer entries go at the end. `docs/PLAN.md`
holds only the current status of each phase; benchmark reruns and one-off
memory samples belong in commit messages, not there.

Status: Phases 0 to 4 built (2026-09-18). Phase 2's gate (one week of daily use) is still
yours to run. Phase 4 gate: thirteen plugins, each proven by a test to disable at runtime with
no trace in any registry and to re-enable; the trait surface did not change for the last
three plugins written (jump, tags, shell). Idle PSS with all plugins is 99.9 MB (budget 100).
Deviations recorded: the plan's `search` item is folded into `query`; `view-dual` is a UI
feature (per-pane state in the shell) rather than a crate; the UI surfaces (review modal,
prompts, treemap, timeline strip) live in `skwir-ui` per the Phase 3 decision; `edit-as-buffer`
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
turns, skwir plus PDFium measured about 201 MiB PSS versus 177 MiB with Poppler;
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
apply without restarting skwir. This adds one defaulted method to the Phase 4
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

Poppler reader follow-up (ADR-005): a separate `skwir-poppler-worker` keeps
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
are unchanged). After 50 page turns, combined skwir/worker PSS stabilized near
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
binding. Toolkit-dependent rendering stays bundled in `skwir-ui`; an external
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

Navigation cancellation follow-up: Usage requests now follow the pane's location
lifetime, so navigation, Back and pane closure cancel them without waiting for
the next UI synchronization. Reopening the same path starts a fresh request.
Native recursive watch setup is replaced by individual directory registrations
during the cancellable traversal; completed cached branches remain reusable.
An already-issued operating-system read must still return before its worker can
observe cancellation. Filesystem open/stat/watch setup now runs on the scheduler,
allowing navigation and repainting while a drive wakes. A one-shot 200 ms delay
shows a per-pane loading message for slow listings; asynchronous open errors
appear in that pane and refresh can recover. Regression tests cover cancellation
during watch registration, independent panes, Back/close, pending opens, stale
results and recovery after a missing directory is created.
Validation: strict lint and all 177 default tests pass (six optional native-tool
tests skipped), with the affected Usage/listing tests repeated after the final
cancellation changes. An isolated Xvfb debug run injected a five-second directory
open delay and verified the loading banner, navigation while that read was still
blocked, open errors, switching away from an unfinished Usage scan, and reopening
the same path. The 10k-image grid and 100k-entry list were also paged; after their
background work settled, CPU measured 1.3% over three seconds. This used software
rendering and synthetic files, not a physical sleeping drive; UI memory budgets
were not remeasured. The first microbenchmark run narrowly missed image streaming
(12.66 ms against 12 ms); the final standard `just bench-check` passed every
unchanged budget (9.62 ms image streaming).
