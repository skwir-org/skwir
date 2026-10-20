# Skwir — Implementation Plan

Skwir is an experimental file manager in Rust: a GPU-accelerated UI
(gpui), a tiny core, everything else a plugin, built to be driven by
humans, scripts and AI agents alike. This file holds the phases with
their goals and **Done when** gates, the status of each, the deviations
from the plan, what is left, and the plans not yet built. Decisions are
in `docs/adr/`. The dated record of what was built, with its
measurements and what was checked, is `docs/STATUS-LOG.md`: work adds
its entry there and says here only what changed in a status and what is
left.

Durations are relative sizes for one developer working with an AI
assistant, not commitments. A phase starts once the previous gate
passes, unless a deviation below says otherwise.

## Principles

Binding, from ADR-000 (AGENTS.md has the working rules):

1. **Always responsive:** no blocking work on the UI thread; everything
   streams, everything cancels.
2. **Light:** idle memory and startup are budgeted and measured
   (ADR-000, revised by ADR-002 and ADR-033).
3. **Stateless and idempotent:** no daemon, no database, one deletable
   cache; re-applying a plan is a no-op.
4. **Harness the environment:** OS thumbnails, icons, the MIME database,
   bookmarks, indexes, installed tools.
5. **Small core:** only what two plugins must agree on; everything else
   is a plugin with a runtime toggle.
6. **Agent-ready:** commands are data, plans before apply, stable
   identity (ADR-049), metadata with provenance.

## Current status (2026-10-20)

- **Where:** named Skwir (skwir.org, application id `org.skwir.skwir`),
  version 0.2.0, released 2026-10-20 on GitHub and the AUR's three
  bases; 0.1.0 was [released](#release-01-linux) 2026-10-13. Phases 0 to
  4, 5a and 6 pass their gates. Maps and Notes everywhere are built,
  their gates not closed, as is much that no phase planned
  ([Built beyond the plan](#built-beyond-the-plan)).
- **Next:** skwird's steps (ADR-050) and Phase 7, which the user put
  after the release (2026-10-07). [What is left](#what-is-left) gathers
  the rest. From 0.3 a release's page is its changelog section above
  `### All changes` (docs/RELEASING.md).
- **Blocked:** Phase 5b needs a Mac and a Windows machine
  (`docs/HELP-WANTED.md`). A Windows build to try is made on Linux
  (ADR-074, `just package-windows`, 2026-10-20): a zip and a per-user
  installer, tried under Wine, its window never drawn there. A macOS
  build to try is made on Linux too (ADR-075, `just package-macos`):
  skwir.app for Apple silicon, looked over, never run. Both were added
  to 0.2.0's release page (built from 5a0bb2b); from the next release,
  `release-build` makes them with the Linux packages. The user reports
  hosted CI is too slow; routine CI is reduced to essentials, with full
  checks and package builds kept as manual fallbacks. The manual full
  workflow, whose `native` job tests on macOS and Windows runners, has
  never run (no run on 2026-10-20).
- **Budgets:** idle own share 92.5 MiB of 100 on Zed's gpui (session
  display, quiet, 2026-10-18; 92.3 for gpui 0.2.2, ADR-064). First frame
  of 150 ms: a median of 119 ms on Zed's gpui and 109 on 0.2.2, the least
  110 and 100 (ten starts each, alternated). `just bench-check` meets all
  44 budgets (2026-10-18, quiet).
- **Latest work (2026-10-14 to 10-20), in 0.2.0:** fonts by role and
  the desktop's colors and fonts (ADR-071, ADR-073); settings by subject
  (ADR-072); address completion (ADR-070); 3D models turned on the GPU,
  STEP, IGES and Rhino meshes (ADR-067, ADR-069); Full preview
  (ADR-068); a held key's repeats (ADR-066); screen readers (ADR-065);
  gpui from Zed's main branch on wgpu (ADR-064); the desktop's file
  clipboard (ADR-062); DjVu (ADR-061); code colored by Ferroni (ADR-060);
  the preview's fullscreen and slideshow, links found in text, the
  terminal's menu and keys, the AUR's `skwir-bin` and `skwir-git`. Since
  0.2.0: builds to try for Windows and macOS made on Linux (ADR-074,
  ADR-075). Earlier work, day by day: `docs/STATUS-LOG.md`.

| Phase or plan | State | Planned size |
|---|---|---|
| [Phase 0: Foundations and toolkit spike](#phase-0-foundations-and-toolkit-spike) | gate passed 2026-09-18 | ~2 weeks |
| [Phase 1: Core kernel](#phase-1-core-kernel) | gate passed by 2026-09-18 | ~4 weeks |
| [Phase 2: Minimal daily-driver](#phase-2-minimal-daily-driver) | gate passed (a week of daily use; recorded 2026-10-04) | ~4 weeks |
| [Phase 3: Visual richness and performance hardening](#phase-3-visual-richness-and-performance-hardening) | gate passed by 2026-09-18 | ~4 weeks |
| [Phase 4: Power features, each as a plugin](#phase-4-power-features-each-as-a-plugin) | gate passed 2026-09-18, holds 2026-10-04 | ~6 weeks |
| [Built beyond the plan](#built-beyond-the-plan) | built 2026-09-18 to 2026-10-19 | — |
| [Maps](#maps) | steps built 2026-09-27; gate not declared | — |
| [Notes everywhere](#notes-everywhere) | steps built 2026-09-30; gate in part (no Mac) | — |
| [Phase 5: Cross-platform](#phase-5-cross-platform) | 5a gate passed 2026-10-06; 5b waits for machines | ~6 weeks |
| [Phase 6: The extension boundary](#phase-6-the-extension-boundary) | gate passed 2026-10-07 | ~4 weeks |
| [Release 0.1: Linux](#release-01-linux) | released 2026-10-13 (0.1.0) | — |
| [Names indexed by skwird](#names-indexed-by-skwird) | C built 2026-10-20; A, B and D wait for skwird (ADR-050) | — |
| [Colors and font from the desktop](#colors-and-font-from-the-desktop) | A, B and the fonts by role built 2026-10-19; C to E planned (ADR-071, ADR-073) | — |
| [Phase 7: AI](#phase-7-ai) | planned, after Release 0.1 | ~10 weeks |
| [Phase 8: Polish and release](#phase-8-polish-and-release) | planned, ongoing | — |

---

## What is left

Gathered on 2026-10-20 from the notes of this plan and of
`docs/STATUS-LOG.md`, each as recorded on the date it gives. Later work
may have covered one: check the source before starting it. Work that
closes an item removes its line; work that leaves one adds it here.

**Planned** (in the sections below): Phase 7 (AI); skwird's steps
(ADR-050); the desktop's colors on macOS and Windows (step E); Phase 5b
on the machines (`docs/HELP-WANTED.md`, `docs/PORTING.md`, which lists
what is still Linux's); Phase 8. Remote file systems are a direction,
nothing built (ADR-021: mounts listed and opened first, rclone later).

**Gates not closed:**
- Maps: idle CPU after panning on geotagged pictures (`images10k` has
  none). `skwir-bench gen` makes no geotagged pictures and no
  100,000-point GPX, and `budgets.toml` has no budget for the GPX parse
  or the EXIF position scan.
- Notes everywhere: the round trip through an exFAT stick read by a
  Mac's Finder.
- Phase 5b: everything run on macOS and Windows, and the budgets
  measured there (ADR-038).

**Previews and readers:**
- Media (2026-09-19, ADR-008): audible lip sync not assessed; native 4K
  output, GPU buffer sharing, external subtitle files, a second backend;
  full-file waveforms.
- Archives: 7z and RAR decryption (libarchive has none), passwords for
  agents, Merge for an extraction onto a folder (2026-10-13); a Mac's
  `__MACOSX` beside an archive's one folder still makes a folder around
  them (2026-10-15).
- Text documents (2026-10-03, ADR-030): `.fodt`, presentations and
  spreadsheets as text, find beyond the part shown, links to bookmarks in
  another part.
- DjVu (2026-10-15, ADR-061): indirect documents, annotations other than
  links, djvu-rs's page titles; DjVuLibre's helper in the macOS and
  Windows packages.
- Code (2026-10-14, ADR-060): Vue Component's two patterns Ferroni
  refuses (ferroni#227); the patches offered to syntect and two-face;
  the 7 MiB or so the code view keeps beyond plain text, not looked into.
- Links (ADR-029): table cells and Properties' comments (2026-10-03);
  the structured-data views made whole, bare names (2026-10-14); an
  Obsidian note elsewhere in the vault (waits for skwird's index),
  notes embedded with `![[…]]`.
- 3D models (2026-10-19, ADR-067): textures; the window on NVIDIA (the
  user's session); a screen reader.
- The screen kept awake while playing (2026-10-15): macOS; a desktop
  that idles by its own rules (its D-Bus inhibitor).
- Not exercised in a window: a reader's or a converter's failure shown
  as source (2026-10-15); after a step read ahead, a HiDPI panel asking a
  sharper picture, a slow disk (2026-10-19).

**The window:**
- Terminal (ADR-019): mouse reporting, an input method's text while
  composing, search in the history (2026-09-26); a menu item for Follow
  the folder shown (2026-10-19, the shell would name a plugin's key). On
  Xvfb a restored panel appears only at the first input after startup,
  not checked on a compositor.
- Keys (2026-10-13, ADR-024): a key scheme chosen from the Keyboard
  page; "Set shortcut…" from F1 or the palette. A broad default moved
  frees narrower keys of other actions, which the page says rather than
  binds again.
- Menus (2026-10-05): the current item is marked two ways, a check for
  an option, a highlight for where one is.
- Accessibility (ADR-065): never listened to with Orca speaking;
  AccessKit's Linux adapter passes no tree level or expanded state; a
  scanned page's text (`docs/invariants/accessibility.md`).
- Properties (2026-10-13, ADR-058), not checked in a window: a read-only
  drive's links taken away, a drive keeping no permissions (FAT).
- A flaky test, `pending_opens_report_errors_asynchronously_and_can_be_replaced`
  (`crates/core/tests/listing.rs`), failed eight runs in a row on
  2026-09-26, then passed; not investigated.

**Search, tags and places:**
- Never read: Finder attributes or `._` files a Mac wrote (the tests
  write Apple's format from its source); a cold `tag:` not measured
  (2026-09-30, dropping caches needs root).
- The sidebar suggests only the tags Skwir's index has seen, not
  Baloo's; renaming a tag leaves hidden files alone, as the search does
  (2026-09-30).
- Companion files (ADR-028): an orphan offered back to its file.
- Not checked in a window: a tag exported by a real drag, a drag between
  sidebar groups refused, a busy volume on a real drive (fake `lsblk` and
  `udisksctl` only), F5 in Usage, Alt+click on a tag's pill, an office
  document converted, a missing engine library, a restore that merges
  (Release 0.1, step 6).

**Maps** (2026-09-27 to 10-05): not built, the Tracks menu, `geo.track`,
`geo.open_external` and `geo.copy_coordinates`, backgrounds from a file.
Not verified: the dark look's map tokens, OpenStreetMap's own server,
the `{s}`, `{-y}`, `{q}` and `{r}` server forms live, a keyring that asks
to be unlocked, the double click that opens, a folder of thousands of
pictures, a real file of each flight and drone format, a video's
position in Properties, a real offline map, a background chosen in a
window that downloads.

**The extension boundary** (Phase 6): the packages carry no module; no
typed WIT world (payloads are JSON); a module has no network; program
plugins are isolated on Linux alone (compiled only on macOS, absent on
Windows, where none runs); the Windows control pipe keeps the system's
default security.

**Other systems,** beside `docs/PORTING.md`: files on the system's
clipboard on macOS and Windows (ADR-062); Windows changes access by path,
so a link swapped in between its check and its change is followed
(2026-10-13); macOS's quarantine passed to what is extracted, compiled
only; on Windows, undoing a trash restores the latest item from its path
(2026-10-11).

---

## Phase 0: Foundations and toolkit spike

**Goal:** a repo that can measure itself, and a toolkit decision backed
by numbers.

**Scope:**
- Cargo workspace: `crates/core`, `crates/platform`, `crates/ui`,
  `crates/plugins/*`, `crates/bench`.
- Nix flake dev shell, `just` recipes, CI (lint, test, bench on Linux).
- A benchmark harness generating synthetic trees on demand (never
  committed): a flat 100k-entry folder, a 1M-file deep tree, a folder of
  10k large images.
- Performance budgets, enforced by `bench` as CI regression checks:
  first frame < 100 ms cold; a 100k-entry folder's first rows painted
  < 16 ms after listing starts, scrolling at display refresh rate; idle
  RSS < 100 MB with default plugins, the same with AI plugins disabled
  as not compiled.
- Toolkit spike: the same demo in GPUI and Floem (a virtualized 100k-row
  list with lazy thumbnails) on Wayland (Hyprland), on the Intel iGPU
  and the NVIDIA eGPU: first paint, frame time while scrolling, RSS,
  external drag-and-drop and IME once.
- ADR-001 records the toolkit choice and its numbers; ADR-000 the
  principles.

**Done when:** budgets are documented, `bench` runs in CI, the ADR is
written, and the winning spike renders 100k rows at refresh rate on
both GPUs.

**Status:** gate passed 2026-09-18. GPUI chosen (ADR-001: its p99 frame
interval stays within vsync jitter on both GPUs; Floem's is a dropped
frame). `just bench-check` enforces `crates/bench/budgets.toml` (42
budgets on 2026-10-10). Routine CI is now minimal; the budget check is
part of the local release gate and manual full workflow (ADR-053).

**Deviations:**
- The UI budgets were revised after Phase 2 (ADR-002): idle memory is
  PSS, not RSS; the first frame may take 150 ms until gpui loads fonts
  lazily, our own share under 50 ms. ADR-033 (2026-10-04) counts Skwir's
  own share of PSS, without the GPU driver's code.
- First frame and idle memory are measured by `just idle-pss`, not by
  `bench` (ADR-002, until an automated startup harness in Phase 8).

---

## Phase 1: Core kernel

**Goal:** `crates/core` is complete enough that plugins can be written
against it, with no UI.

**Scope:**
- **Entries.** `EntryId` = device and inode on Unix, the file ID on
  Windows. Streaming listing via `getdents` batches (rustix), `d_type`
  to avoid a stat, lazy stat on demand. A metadata bag of typed fields
  with provenance (Filesystem, User, Plugin(name), Model(name,
  confidence)).
- **Watching:** `notify`, per open location, coalesced, feeding the same
  entry stream.
- **Locations:** `LocationProvider` (string → entry stream). Only the
  path provider ships in core.
- **Commands as data:** a serde struct with typed arguments, registered
  by name, dispatched by the core. Keymap and palette consume the
  registry. No command captures UI state.
- **Operations and plans:** `Operation` with `apply` and `inverse`; a
  `Plan` is an ordered list of operations referencing `EntryId`s, with a
  dry-run `diff()`. The executor runs plans in the background with
  progress, pause and cancellation, and writes an undo journal.
  Re-applying an applied plan is a no-op. Built-in: copy (reflink when
  possible), move, rename (atomic exchange for swaps), trash, delete,
  mkdir.
- **Registries:** views, panels, sidebar and context-menu contributions,
  the settings schema.
- **Plugins:** compile-time crates, a runtime toggle in settings,
  depending only on core, never on each other.
- **Cache and settings:** a content-addressed cache folder, deletable at
  any time; plugins get a namespaced slot in cache and settings,
  nothing else.
- **Scheduler:** one priority queue, visible work first, everything
  cancellable; provider calls batched, async, timed; slow providers
  surfaced.
- Tests: property tests that `inverse(apply(op))` restores the tree;
  idempotent plans; listing benchmarks over the synthetic trees.

**Done when:** a headless test drives a plan (list, select, rename,
trash, undo) through commands only, and listing the 1M-file tree
streams without ever blocking the caller.

**Status:** gate passed; built by 2026-09-18.

**Deviations:**
- Identity was split (ADR-049, 2026-10-10): `FileId` (device and inode)
  is the file, `EntryId` the file under one of its names, so hard links
  are two entries. On Windows a 128-bit file ID is folded into 64 bits
  (ADR-036).

---

## Phase 2: Minimal daily-driver

**Goal:** replace the file manager you use every day, even if ugly.

**Scope:**
- `crates/ui` on the chosen toolkit: window, panes, theme tokens,
  command palette, keymap, address bar (paths only), status and
  progress. AccessKit from the start.
- Plugins: `view-list`, `places` (XDG user folders, GTK bookmarks,
  udisks2 volumes), `ops-basic` (the built-in operations as commands
  with keys), `open-with` (xdg-mime), `undo` (journal UI).
- `crates/platform` on Linux: shared-mime-info through `xdg-mime`, XDG
  icon theme lookup, the freedesktop trash, a read-only freedesktop
  thumbnail cache lookup.
- Keyboard first: every action in the palette.
- Dogfooding starts here.

**Done when:** you have used it as your only file manager for one week,
budgets still pass, and every action taken that week went through a
`Command`.

**Status:** gate passed: a week as the only file manager, then two
weeks of improvements from it (recorded 2026-10-04). Budgets:

- ADR-002 (2026-09-18) revised the two UI budgets Phase 2 missed (first
  frame 143 ms against 100; idle RSS 143 MB against 100).
- The idle memory gate stayed open from 2026-09-19 (118 to 120 MB PSS
  against 100, ADR-002's addendum) until ADR-033 (2026-10-04) budgeted
  the own share: 89.2 MiB against 100 (133.7 MiB in all, 44.6 of it the
  driver's), by `just idle-pss`.
- Found with that measurement and fixed the same day: an idle Wayland
  window woke at the display's rate (vendored patch 0012: it draws when
  asked; idle CPU 1.0 → 0.2 %); the first frame read a median of 146 ms
  (patches 0013 and 0014: pipelines built once, the GPU context created
  beside the fonts; 134.6 → 114.5 ms back to back); the places plugin
  rescanned about twice a second once anything had opened the GTK
  bookmarks file (notify reports opens; access events are dropped).

**Deviations:**
- AccessKit: gpui 0.2.2 has none (ADR-001). Accessibility waits for
  gpui's support, checked at every gpui update. Zed's main branch has it
  (ADR-064); exposing Skwir's places started with ADR-065.

---

## Phase 3: Visual richness and performance hardening

**Goal:** it looks and feels like the fast, GPU-native app it is.

**Scope:**
- **Thumbnails:** `ThumbnailProvider` keyed by MIME type; the
  freedesktop cache read first, generation on a miss written back with
  spec-compliant names; installed `*.thumbnailer` files discovered;
  decoding on a pool, visible first, cancellable, uploaded to the GPU
  atlas.
- Plugins: `view-grid`, `view-columns` (Miller columns), `preview`
  (image, highlighted text, PDF via PDFium, a video's first frame;
  Space to peek).
- Drag and drop: internal first, then external on Wayland.
- A performance pass over the synthetic trees; a flame-graph recipe.

**Done when:** the 10k-image folder scrolls at refresh rate with
thumbnails appearing in visible-first order, and all three views pass
the budgets.

**Status:** gate passed; built by 2026-09-18.

**Deviations:**
- The views are built into `skwir-ui` (`crates/ui/src/views/`), not
  plugin crates: gpui stays in `skwir-ui`, and a `ui-api` crate waits
  for a third-party view.
- The preview became many plugins: PDF through PDFium, Poppler or MuPDF
  readers in helper processes (ADR-003, 004, 005, 014), video and audio
  through GStreamer (ADR-008), structured previews (ADR-009). See
  [Built beyond the plan](#built-beyond-the-plan).

---

## Phase 4: Power features, each as a plugin

**Goal:** the UX bets. Ordered by leverage; each ships and toggles on
its own.

**Scope:**
1. `query`: a query language (`*.pdf modified<7d size>10M`) as a
   `LocationProvider`, so search results are a location. Saved searches
   kept in the sidebar like bookmarks.
2. `search`: a parallel subtree walk (`ignore`), results cached by
   folder mtime; `plocate` when present; content search with the
   ripgrep crates.
3. `shelf`: collect entries from anywhere, apply one plan to all.
4. `rename-bulk`: patterns, regex, live preview; produces a plan.
5. `edit-as-buffer`: the listing as an editable text buffer, diff →
   plan (oil.nvim's model).
6. `jump`: frecency jump from zoxide's database and
   `recently-used.xbel`; `nucleo` matching.
7. `shell`: open a terminal here, mirror the folder, drop the selection
   as arguments.
8. `tags`: xattrs with the XDG and Finder keys; tags are metadata with
   User provenance.
9. `view-dual`: orthodox two-pane mode over the same model.
10. `usage`: GPU treemap, btrfs-aware.
11. `timeline`: files by mtime as a scrubbable histogram.

By the end there are 10+ internal plugins: the signal that the traits
are ready to freeze.

**Done when:** every plugin above can be disabled at runtime with no
trace in memory or UI, and the trait surface has not changed for the
last three plugins written.

**Status:** gate passed 2026-09-18: thirteen plugins, each proven by
`crates/skwir/tests/plugins_toggle.rs` to disable with no trace in any
registry and to re-enable; the trait surface did not change for the
last three written (jump, tags, shell). The test still holds on
2026-10-04 with 36 plugins, 14 of them previews.

**Deviations:**
- `search` is folded into `query`; results are not cached across opens
  (the walk is fast; content search dominates).
- `view-dual` is per-pane state in the shell, not a crate.
- The review modal, prompts, treemap and timeline strip live in
  `skwir-ui`.
- `edit-as-buffer` opens `$VISUAL`/`$EDITOR` in a terminal, not an
  in-app editor.
- The native workers are workspace members outside `default-members`,
  so the default build needs no Poppler, MuPDF, GStreamer or libarchive.
- The settings dialog (2026-09-18) added one defaulted method to the
  plugin trait (its settings hook).
- The Finder tag key was read only from 2026-09-30.
- Closed 2026-10-05: the query's date parser and the timeline's calendar
  math were duplicated; both now read `skwir_platform::calendar::Period`
  (`civil_day` refuses `2026-02-31`; `Calendar::year_of`).

---

## Built beyond the plan

What was built between and beside the phases. The rules these features
follow are in AGENTS.md and the ADRs named; the record of each (dates,
measurements, what was checked) is in `docs/STATUS-LOG.md`, under "Built
beyond the plan, by area" up to 2026-10-10 and under each day after; what
it left is in [What is left](#what-is-left).

- **Previews and readers:** PDF and Office pages and video stills through
  installed tools (ADR-003); PDFium, Poppler and MuPDF readers in helpers
  (ADR-004, 005, 014); video and audio through GStreamer, one plugin
  (ADR-008, 016); structured previews (ADR-009) in one frame
  (`docs/preview-frame.md`); music, playlists and MPRIS; reading mode and
  fixed-layout pages (ADR-012, 013); pages and pictures at the size shown
  (ADR-015); more formats (SVG, EXR, code, tables, notebooks, Mermaid,
  circuits, DXF, G-code); text documents (ADR-030); ImageMagick's
  pictures, thumbnailers isolated (ADR-031); text read where it is shown
  (ADR-051); archives extracted (ADR-048) and made (ADR-059); code
  colored by Ferroni (ADR-060); DjVu (ADR-061); links in text (ADR-029);
  fullscreen, slideshow, find marks and zoom around the pointer
  (2026-10-15 and 16); a film's one picture, posters at the size shown
  (2026-10-16 and 17); 3D models turned on the GPU, STEP, IGES and Rhino
  (ADR-067, 069); Full preview (ADR-068); pictures read ahead
  (2026-10-19).
- **The window:** settings (ADR-010), by subject (ADR-072); Usage
  (ADR-007); huge folders drawn as they stream (2026-09-25); tabs
  (ADR-018); the terminal (ADR-019); keys and key schemes (ADR-024), a
  held key's repeats (ADR-066); the context menu by file; rows and
  toolbars (2026-09-29); the tree view (ADR-025); trails (ADR-032); icon
  and code themes; names already taken (ADR-054); the settings file
  shared, the window's state and what the user makes kept apart (ADR-055
  to 057); Properties (ADR-058); files on the desktop's clipboard
  (ADR-062); gpui from Zed's main branch (ADR-064); screen readers
  (ADR-065); address completion (ADR-070); the desktop's colors and
  fonts (ADR-071, 073).
- **Search:** words for people and named periods; dates the user's
  (ADR-017); facets, then pills in the address; saved searches (ADR-057);
  searches following changes; hints from indexes (ADR-026).
- **Tags, places and the trash:** tag-preserving copies (ADR-006); notes
  (ratings, comments) and Finder tags; tags as places (ADR-032); the
  sidebar the user's and the trash a place (ADR-034); busy volumes;
  bookmarks; Open With.
- **Upkeep:** the consistency review (2026-10-05); drops that ask, plans
  that pause, copies at a drive's pace, hard links as entries (ADR-049,
  2026-10-07 to 10); what woke while nothing changed (2026-10-10); the
  review before tagging 0.1.0 (2026-10-13).

---

## Maps

ADR-020; ADR-022 for what came later. Two plugins: `geo` (formats,
preview, thumbnails, the Map view) and `map-tiles` (backgrounds, the
only network code). Each step shipped on its own.

**Goal:** GPS tracks and routes preview on a map, and a Map pane view
shows the files that carry a position (photos first) where they were
taken.

**Steps:**

- **A. Track previews, offline.** Planned: core `geo` types (`LatLon`,
  `Bounds` aware of the ±180° meridian, a bounded `GeoDoc`; a waypoint
  has a kind and an altitude with its reference, so step D′ needs no
  core change), `InspectionKind::Map`; `elements/map.rs` (Web Mercator,
  pan, zoom, placement in prepaint); a map preview kind (Fit, Zoom,
  Tracks, Open in Maps through a `geo:` URI); GPX, KML/KMZ and GeoJSON
  parsed on a worker, thinned per zoom; distance, times, climb and
  extremes in Details; a themed graticule.
  *Built 2026-09-27.* A 6 MB, 100,000-point GPX reads in about 80 ms
  (release). Added: a scale bar, degree labels, start and end marks,
  place labels while at most 40 are in view. Deviations: no
  Douglas–Peucker levels (past `MAX_POINTS`, 200,000, every other point
  is kept, the facts counting all; a line is thinned to the pixels it
  covers when drawn, `elements::map::runs`); a line without times in KML
  or GeoJSON is a route; a wheel notch zooms half a level without Ctrl.
  Not built: the Tracks menu (every line is drawn). Not verified: the
  dark theme's map tokens.
- **B. Backgrounds.** Planned: a core `tiles` contract; `map-tiles` with
  the outline (Natural Earth, the default), OpenStreetMap once chosen,
  a custom URL; the tile cache, attribution, a link to the setting.
  *Built 2026-09-27.* Sources answer through a callback (downloads never
  hold a scheduler worker); `map_tiles::TileStore` asks in prepaint,
  cancels tiles that left the view, keeps 48 MB of decoded tiles while a
  map shows; a coarser tile stands in for a finer one. Verified against
  a local tile server: each tile asked once, from the cache after a
  restart, revalidated with `If-None-Match` (304), the User-Agent naming
  Skwir. Not verified: OpenStreetMap's own server.
- **B′. Tile servers.** *Built 2026-09-27* (ADR-020 and ADR-010
  addenda): servers as a records setting, addresses in the common forms
  (`{s}`, `{-y}`, `{q}`, `{r}`, `{key}`), headers, an API key in the
  keyring, a Test button, the background chosen from the map's toolbar.
  Verified in a private keyring against a local server that requires the
  key. Not verified live: the `{s}`, `{-y}`, `{q}` and `{r}` forms
  (tested as pure functions), a keyring that asks to be unlocked.
- **B″. Loading as map libraries do.** *Built 2026-09-27*: the middle of
  the view first, every tile kept, finer tiles standing in when zooming
  out, HTTP/2 through hyper instead of `ureq`. At 150 ms a tile: the
  middle shows in 175 ms (it came last), the whole view in 431 ms (was
  626), zooming out two levels about 1 s (was 1.9). Vendored patch 0008
  fixes a darker tile edge at fractional zooms.
- **C. The Map view.** Planned: a `ViewRole::Lens`; `geo` scans the
  location for EXIF positions and track files, visible first, cached by
  identity; markers with thumbnails, merged into clusters; selection,
  Enter, the preview and the rubber band as in other views; counts in
  the status bar; a placeholder offering subfolders; commands
  `geo.points`, `geo.status`, `geo.track`, `geo.open_external`,
  `geo.copy_coordinates`.
  *Built 2026-09-27*: `geo.scan`, `geo.points` (with the counts),
  `geo.release`, `geo.search_subfolders`; Ctrl+5. Deviations: the counts
  sit in a row at the top of the view with Subfolders and Fit;
  "Include subfolders" is a search (`query.search`); the scan goes in
  listing order (nothing says what is in view before positions are
  known); `geo.track`, `geo.open_external` and `geo.copy_coordinates`
  are not built (the preview has Open in Maps). Fixed with it in core:
  `App::waker_handle` and `OpenContext` call the hook installed when
  they are called. Verified on Xvfb with synthetic geotagged JPEGs and
  a GPX. Not verified: the double click that opens; a folder of
  thousands of pictures; the dark theme.
  Spreading (ADR-020 addendum): files at one place open as a spiral in
  the order they were taken; 307 pictures at one spot drawn at 0.5 %
  idle CPU. Deviations: the planned "+N" tile became dots; Escape does
  not fold (a click beside does).
  A Background button ends the Map view's row (2026-10-05). Not chosen
  in a window: OpenStreetMap, which downloads.
- **D. More formats and polish.** Planned: TCX, FIT, IGC, NMEA; track
  thumbnails; an elevation profile; positions of videos; "Show on map"
  on Details' position row. *Built 2026-09-27*: FIT by a bounded decoder
  of its own; thumbnails drawn to scale (tiny-skia, after the desktop's
  thumbnailers); video positions from `moov/udta/©xyz` or QuickTime's
  keys (`skwir_platform::mp4`); `Detail::action` in core.
- **D′. Flight and drone plans.** Planned: Garmin `.fpl`, MSFS/FSX
  `.pln`, X-Plane `.fms`, Little Navmap `.lnmpln`; QGroundControl
  `.plan`, MAVLink `.waypoints`, Litchi CSV, DJI WPML `.kmz`; claimed by
  extension, confirmed by content; departure, destination, legs,
  distance, altitude and waypoint count in Details.
  *Built 2026-09-27*, MAVLink also as `.mission` and `.txt`. Every
  format of D and D′ is confirmed by its first 4 KiB (the MIME database
  calls a `.fit` a FITS image, a `.pln` a PlanPerfect spreadsheet, a
  `.plan` a Calligra project), and the provider comes before tables and
  text, so a Litchi `.csv` is a mission. The Map view does not look
  inside `.txt` and `.csv`. An altitude of 0 in a flight plan means none
  given (the formats write 0 for a fix without one). Not verified: a
  real file of any of these formats (each was written from its
  documentation), a video's position in Details, the dark theme.
- **Later** (ADR-022): search by place (`near:`, "This area"),
  geotagging from a track and removing locations, offline place names:
  *built 2026-09-27* (ADR-023 for fields read on demand). Verified on
  Xvfb: pictures matched along a track through the user's time zone,
  their modification time kept to the nanosecond; a location removed
  and undone; `near:` within 500 m. Local PMTiles and MBTiles files
  preview as their maps (2026-10-05, ADR-022's part 4; checked on files
  the reference `pmtiles` tool wrote); not checked: a real offline map,
  Brotli or Zstandard archives (refused with words), the dark theme.
  Backgrounds from a file remain proposed.

Tests and fixtures use invented coordinates only. Planned and not
built: geotagged pictures and a 100,000-point GPX from `skwir-bench gen`,
and budgets for the GPX parse and the EXIF position scan per 1,000 files
(`crates/bench/budgets.toml` has none).

**Done when:** a GPX, a KML and a GeoJSON file, a flight plan and a
drone mission preview with their facts; a folder of geotagged pictures
shows on the Map view with shared selection; both plugins disable with
no trace (`plugins_toggle.rs`), cancelling tile requests in flight; no
request leaves Skwir until a network source is chosen; idle CPU returns
to 0 % after panning on `images10k`.

**Status:** every step built 2026-09-27; the gate is not declared. Met:
the previews and the Map view (synthetic files), both plugins in
`plugins_toggle.rs` (`map-tiles` cancels its downloads on disable), no
request before a network source is chosen. Not measured: idle CPU after
panning on `images10k`, which has no geotagged pictures.

---

## Notes everywhere

ADR-027, ADR-028.

**Goal:** a file's tags, rating and comment on any system (Linux, macOS,
Windows, USB drives) with nothing lost going back and forth; Skwir reads
every store it knows by default and writes the ones the user chooses.

**Steps** (each built 2026-09-30):

- **A. Stores, no change in behavior.** Core `notes`: the model, the
  owner-tagged `NotesStore` registry, `set_notes` whose inverse puts
  back every store's bytes. The tags plugin's XDG, Finder and
  AppleDouble code becomes three stores; AppleDouble also keeps the XDG
  attributes.
  *Built:* an edit captures the policy when the plan is made, writes the
  file's attributes first and, where refused, the store its folder
  already uses, else AppleDouble (now created); `restore_notes` refuses
  when another application changed a store since. One batch per folder,
  one `listxattr` per file. The index learns from `Executor::observe`.
- **B. Merge against what Skwir wrote.** A snapshot per file in the
  cache; another application's edit in one store wins; the union
  without a snapshot. Details lists where the notes are and offers to
  make the stores agree.
  *Built:* an emptied store is not an edit; the first store's edit wins
  over a later one's; each store says what it can hold
  (`NotesStore::fit`: no comma in a desktop tag, no rating in the
  Finder), so it differs only when it could agree; `notes.agree`. Undo
  forgets the snapshot.
- **C. Companions.** Name rules in a core registry; the listing pairs
  and hides them (`core.show_companions`); rename, move, copy, swap,
  trash, restore and delete carry them; a copy converts notes to what
  the destination keeps (ADR-006 amended).
  *Built:* as planned, the resource fork and Finder info kept as
  `user.com.apple.*`. A no-replace rename the filesystem refuses
  (bindfs, sshfs, many FUSE ones, where every copy used to fail) now
  looks, then renames. Tested against a `bindfs --xattr-none` mount
  (`SKWIR_NO_XATTR_DIR`). Deviation: a batch hydrated before its
  folder's listing completes still reads the folder once for `._` names.
- **D. Settings.** A Notes page (`notes.*`): each store read or not,
  written Off, Update existing or Create; on a volume without
  attributes, the folder's convention, else AppleDouble.
  *Built* (`App::register_notes_store`). Deviation: stores are listed by
  title, not the platform's own first.
- **E. XMP.** Sidecars (`x.ext.xmp`, `x.xmp` with
  `photoshop:SidecarForExtension`): `dc:subject`, `xmp:Rating`,
  `dc:description`, `xmp:Label` as a tag; updated where present, created
  on request; XMP inside files read on demand for Details.
  *Built:* `x.xmp` follows its file only while no other file shares its
  stem; ratings in whole stars (`fit` rounds a half); edits spliced into
  the text as it is, so darktable's history and unknown properties stay.
  Deviation: nested keywords are read for their last part, and a new tag
  is written at the top of the hierarchy.
- **F. TagSpaces.** `.ts/x.json` sidecars and a folder's `tsm.json`
  (hex colors to the nearest palette color, the exact value kept);
  `tsi.json` as a search hint (ADR-026).
  *Built:* the sidecar's thumbnail (`.ts/x.ext.jpg`) follows it; the
  description is the comment (TagSpaces has no rating); every other key
  stays. Deviation: `tsi.json`'s layout comes from its documentation,
  not from an index TagSpaces wrote; it is read tolerantly.
- **G. Names.** TagSpaces' `x[a b].ext` (where a `.ts` folder shows it
  is used) and filetags' `x -- a b.ext` (on request); writing them is a
  rename plan of its own.
  *Built:* the `.ts` condition matters because yt-dlp names downloads
  `title [id].ext`; filetags is off until turned on
  (`NotesStore::default_read`); `names.tag` and `names.untag`.
  Deviation, found 2026-10-18: ADR-027 made a name's writing opt-in, but
  the Notes page had no Write setting for one, so an edit never reached
  a name. Amended that day: Write is Off or Update for a name, and
  Update has an edit rename a file whose name holds tags, a `Rename`
  step unfolded beside it on the plan's thread (`Operation::unfolds`).

Tests use synthetic files built from each format's specification (not
our own encoders) and, where an application runs headless, its output.
The operations' property tests cover trees with companions.

**Done when:** a file tagged in Skwir keeps its notes copied to an exFAT
stick, read by a Mac's Finder there (checked on a Mac or with a `._` file
a Mac wrote), and copied back to a Linux disk; an edit made in another
application's store wins over Skwir's older one; undo restores every
store byte for byte; `tags_fill_us` holds its budget.

**Status:** the gate passes in part. The round trip was checked on a
FUSE mount refusing attributes, not on exFAT with a Mac reading it.
Checked: another application's later edit wins; undo is byte for byte;
`tags_fill_us` within budget with every store running (`flat100k`
227 ms of 550, `images10k` 41.5 ms of 120; `tag:` over a million files
2.66 s; load average 15). Windows' and macOS's own stores wait for
Phase 5b.

---

## Phase 5: Cross-platform

**Goal:** macOS and Windows builds that feel native in the ways that
matter.

**Scope:**
- macOS: QuickLook thumbnails, NSWorkspace icons and Open With, Trash,
  DiskArbitration volumes, Spotlight as a search provider, Finder tag
  xattr compatibility.
- Windows: the shell thumbnail cache, SHGetFileInfo icons, the Recycle
  Bin, WM_DEVICECHANGE volumes, Windows Search as a provider, the NTFS
  USN journal for fast subtree walks.
- File identity, paths and reflink/clone (APFS clonefile, ReFS block
  clone) abstracted in core.
- CI on all three systems; budgets measured on each.
- Packaging: Nix flake and AppImage/Flatpak, dmg, msi.

**Split (ADR-035, 2026-10-05):** no Mac or Windows machine is
available. 5a is done on Linux: every system's code compiled and linted
here (`just check-cross`: the GNU Windows target and macOS, Zig
compiling their C), tested where Linux or Wine can. 5b, on the machines,
is the original gate. `docs/PORTING.md` lists what is still Linux's;
each step removes its items. The dev shell has both targets, Zig and
cargo-zigbuild; `nix develop .#windows` adds Wine. At the start, every
C dependency and gpui's backend built for Windows and only Skwir's code
failed; for macOS, gpui's backend needs Apple's SDK.

"Compiled only" below means nothing of it ran; "under Wine" means it
ran there, in a prefix of its own.

**Steps:**

- **A. It compiles everywhere.** *Done 2026-10-05.* `just check-cross`
  is part of `just lint-all`: clippy with warnings as errors over the
  default members and their tests for `x86_64-pc-windows-gnu`, and over
  the workspace but `skwir-ui`, `skwir` and the native workers for
  `aarch64-apple-darwin`. Core reaches the system through its own seams
  (`fs::Dir`, `fs::stat`, `fs::xattr`, `op/sys.rs`, `os_path`); what has
  no code for a system says so (`system::not_yet`), and predicates that
  cannot know answer "unknown" (listed in `docs/PORTING.md`). Off Linux
  the terminal has no primary selection, types no `cd` and does not
  follow the shell's folder; ConPTY starts on Windows, never run. Linux
  behaviour unchanged. Nothing ran on Windows or macOS; `skwir-ui` is
  not compiled for macOS here.
- **B. Identity, paths and listing.** *Done 2026-10-05, but Usage*
  (ADR-036). Windows lists through the IDs its directory records carry
  (no open per row) and folds a 128-bit ID into `EntryId`'s 64 bits,
  NTFS's exactly; `Entry::hidden` carries Windows' attribute and a
  Mac's `UF_HIDDEN`. Paths: Windows' name rules, `PATHEXT`, no `\\?\`
  from canonicalization, WTF-8 names, `~\`, `file:///C:/`, a drive as
  one trail segment. `just test-windows`: core's 166 tests pass under
  Wine, four skipping their trash part. Gaps: Wine has no 128-bit
  directory records, so only the fallback ran; off Linux a folder moved
  while shown is not followed; Usage stays Unix's. SQLite is built in on
  Windows (ADR-022 amended).
- **C. Operations.** *Done 2026-10-05, but write-through off Linux*
  (waits for F). Planned: no-replace rename and swap (`renamex_np`;
  Windows `MoveFileExW`, no swap), system copies, write-through
  (unbuffered `CopyFileExW` or `fsync` per window, `FlushFileBuffers` or
  `F_FULLFSYNC` once per plan: a flush per file costs what a commit does
  on Linux), attributes, permissions, special files. Built: Windows'
  `CopyFileExW` (progress and Cancel, alternate streams kept, a
  download's mark among them) and `MoveFileExW`, a swap refused as a
  conflict; macOS `clonefile` when attributes are kept, every attribute
  kept but the two the system sets, pipes copied with `mkfifo`. Under
  Wine: every core copy test, not the streams (Wine has none). macOS
  compiled only.
- **D. Trash.** *Done 2026-10-05, but listing a Mac's trash.*
  `op::records` reads and writes freedesktop's pairs and the Recycle
  Bin's `$R` item and `$I` record (versions 1 and 2), kept as bytes in
  plans; `trash:` lists the user's folder (by SID) in each local drive's
  bin. The records ran on Linux from fixtures; the Windows trash never
  ran (Wine's bin is the freedesktop trash). macOS trashes with
  `trashItemAtURL`, keeping the resulting path for undo; the Mac's trash
  records no origin, so `trash:` lists nothing there and only an undo
  restores. Compiled only.
- **E. The environment.** *In part.* Done: settings and cache folders
  per system (`core::folders`); the keyring (Credential Manager under
  Wine; Keychain compiled only); a terminal (Windows Terminal, else a
  console of its own; macOS `open -a`, no program run in it yet); the
  first weekday (under Wine). Types (ADR-037, 2026-10-06): the system's
  registry first, shared-mime-info behind it; macOS's UTType compiled
  only, Windows' registry classes under Wine; the vendored `xdg-mime`
  reads on Windows (under Wine); the macOS and Windows packages carry
  the database compiled from `vendor/shared-mime-info` (2.5.1) by
  `tools/package/mime-database.sh`. Icons (2026-10-06, ADR-041 point 4,
  `system_icons`): NSWorkspace compiled only; Windows' system image list
  under Wine, whose stock icons are a stub (no trash icon there). Not
  done: Open With (Launch Services, Windows' handlers), owners by SID.
- **F. Places and volumes.** *In part, 2026-10-05.* Windows lists local
  drives as devices and the user's folders (under Wine); macOS its root,
  `/Volumes` as the Finder shows it (`getmntinfo`) and the user's
  folders (compiled only). Not done: eject and mount off Linux, Finder's
  favorites, Quick Access, hearing a drive plugged in on Windows.
  Windows writes removable drives through by default (quick removal);
  Skwir's write-through there and on a Mac flushes once per plan
  (`docs/PORTING.md`, section 3).
- **G. Thumbnails and isolation.** *In part, 2026-10-06* (ADR-039,
  ADR-041). macOS isolates converters, decoders and the document readers
  with `sandbox-exec` under Skwir's profile (compiled, never run);
  callers ask `sandbox::command` for the paths a program is given. Pandoc
  runs in its own `--sandbox` (2.15 or later) everywhere, and alone
  where nothing isolates (Windows, Linux without bubblewrap); tested on
  Linux both ways. LibreOffice and ImageMagick stay refused without
  isolation. Windows' AppContainer is decided and left for 5b. The
  system's thumbnails: Quick Look compiled only; the Windows shell
  under Wine, whose shell draws none. On every system the PDFium,
  Poppler and MuPDF helpers now run sandboxed
  (`sandbox::untrusted_reading`): a document opens 10 to 20 ms later
  (PDFium 6–8 → 25–27 ms, Poppler 18–19 → 26, MuPDF 4 → 13), pages no
  slower.
- **H. Processes and the terminal.** *In part, 2026-10-05.* Whether a
  process runs is asked of each system (cache sweeps used `/proc`); a
  helper and its children share a job object on Windows (under Wine);
  the shell's folder is read on macOS (`proc_pidinfo`, compiled only);
  the `cd` is quoted for PowerShell and `cmd` too (tested on Linux).
  Not done: ConPTY run; on Windows nothing types the `cd` (who has the
  terminal is not known there).
- **I. Media keys and search hints.** *In part, 2026-10-05.* MPRIS is
  registered on the free desktops alone; Spotlight answers a tag search
  (`mdfind`, compiled only). Not written: a Mac's Now Playing, Windows'
  media controls, Windows Search.
- **J. Keys and the window.** *Done 2026-10-05, but the outgoing drag.*
  Standard shortcuts are written `secondary` (Cmd on a Mac, Ctrl
  elsewhere) and compared as GPUI reads them (`keymap::normal_keys`);
  caps say ⌘ and ⌥; Ctrl+click, Ctrl+wheel and a kept band are Cmd on a
  Mac; what a Mac reserves (Cmd+H, Cmd+Space, Ctrl+arrows) keeps Ctrl;
  each system's fonts follow Inter. Not done: an outgoing drag on macOS
  and Windows (a gpui patch each; patch 0001 is Wayland's), Option as a
  Mac's copy modifier, Option+arrows for words. Nothing ran on a Mac.
- **K. Packages and CI.** *Linux's built and tried 2026-10-06; the
  others written, never run.* Linux: Debian's, RPMs and the AUR
  (ADR-040; [Release 0.1](#release-01-linux) step 10), each built on its
  distribution from one list of programs (`tools/package/programs`);
  helpers in `/usr/lib/skwir` (`/usr/libexec/skwir` on Fedora), found by
  `process::helper_folders`; a desktop entry and AppStream metainfo that
  validate. Every package carries PDFium (ADR-041,
  `tools/package/pdfium.lock`), so PDFs open everywhere; PDF thumbnails
  and office pages are drawn by the engine, `pdftoppm` only where none
  is. Debian and Fedora split `skwir` (the app, PDFium, archives, 3D
  models) from `skwir-media` (recommended) and `skwir-poppler`,
  `skwir-mermaid`, `skwir-mupdf` (suggested); Arch has one `skwir`
  whose Poppler and GStreamer libraries are optional, plus
  `skwir-mermaid` and `skwir-mupdf`. The main Debian package went from
  26 MB (85 MB installed) to 20 MB (67 MB). `tools/package/try.sh`
  builds each in a fresh Debian 13, Fedora 44 or Arch root (bubblewrap)
  and installs `skwir` alone, then with the rest; on Debian and Fedora
  the window drew through the distribution's Mesa. Every helper answers
  `--probe` (`process::probe`), so Settings says a helper is installed
  only where it works and a package may leave its libraries optional.
  The Nix-based AppImage recipe was retained but never verified. ADR-052
  replaces it with an x86-64 AppImage built from this Nix host in a
  rootless Ubuntu 22.04 build root (`just package-appimage`), with all
  programs from the shared list, pinned PDFium, a pinned GStreamer
  core/base built against the same glibc baseline, codecs and
  licence notices. Relative library paths keep host programs' environment
  intact; bundled helpers retain their private runtime inside the sandbox.
  A separate runtime-only root checks the actual image, every helper,
  synthetic PDFs and playback, the window's commands, sandboxed archive
  extraction and undo, and the host terminal's environment. *Built and
  tried 2026-10-11:* a 156 MB image, all 573 ELFs checked against glibc
  2.35, passed in runtime-only Ubuntu 22.04, Ubuntu 24.04 and Fedora 44
  roots. On this NixOS host, `appimage-run` passed the window's command checks on both
  Xvfb/lavapipe and Wayland/NVIDIA. The packaged player's synthetic
  video checks cover seeking, audio and subtitle tracks, rate, end and
  restart. Local workspace lint, cross checks, 1,474 nextest tests,
  dependency checks, reader checks and performance budgets pass; tests
  and budgets were rerun after the heavy build load. CI's same-artifact
  checks on Ubuntu 24.04 and Fedora 44 passed locally, not run remotely;
  other hardware and actual audio output have not been tried.
  Also written: the Nix package (`just package-nix`);
  the `.app` and `.dmg` (`tools/package/macos.sh`) and the `.msi`
  (`tools/package/windows/`), unsigned, without the native workers'
  libraries, never run. Those leave Mermaid and MuPDF out; there Skwir
  finds LibreOffice where it installs itself and searches Homebrew's
  and MacPorts' folders (compiled only); the Windows PDF helper loaded
  the pinned `pdfium.dll` under Wine. The manual full workflow's `native`
  job for macOS and Windows (not blocking until 5b) and the Linux package
  containers remain fallbacks; their hosted results have not been verified
  here. Left for the machines: audio and
  video through AVFoundation and Media Foundation, libarchive linked
  statically, everything run.
- **L. Budgets per system.** *Decided 2026-10-05* (ADR-038): the own
  share is `phys_footprint` on macOS and the private working set on
  Windows, 100 MiB each until 5b measures. `just idle-pss` says it
  measures nothing off Linux.

**Done when (5a, on Linux):** `just check-cross` is part of `lint-all`
and green; every step has its code or is recorded as not done, with
what ran under Wine and what no machine ran; the Linux packages build
and start; Linux's tests and budgets are unchanged.

**Done when (5b, the original gate):** the Phase 2 dogfood week is
repeated on each system, and the budgets pass on all three.

**Status:** 5a passed 2026-10-06; 5b waits for the machines and starts
from these steps, `docs/PORTING.md` and `docs/HELP-WANTED.md`. Evidence
for 5a:
- `just check-cross` green inside `just lint-all`; `just test-all` 1,176
  passed, poppler-check and mupdf-check pass; under Wine 766 passed and
  15 ignored (all but `skwir-ui`, `skwir` and the native workers, whose
  gpui imports Windows' ICU, which Wine lacks).
- The Linux packages build and install alone and together, every
  program found and working, the licences in place (`linux/check.sh`
  found Fedora's dropped on a system installed without documents;
  fixed).
- Budgets against the commit before 5a (`b52dfcb`): `bench-check` meets
  all 36 on an idle machine (the listing stream's three timings read
  over only while the machine compiled or was loaded); `idle-pss`,
  alternated back to back: own share 95.8 MiB against 95.4 (the
  binary's 0.4), first frame no slower. The first frame itself could
  not be measured that day: under load average 4 to 10, the build that
  read 114.5 ms on 2026-10-04 read 180 to 851 ms.
- The own share had grown 3.9 MiB since 2026-10-04 (91.5 → 95.4 MiB),
  before Phase 5. Bisected and fixed the same day: `img()` drew
  full-color SVG icons at twice their declared size (the Trash's
  `user-trash-full`, a 128 px SVG with filters in Adwaita, became 256 ×
  256 for a 16 px row), so icons are rasterized for the pixels they
  cover (`icons::element`); an idle worker gives back what its jobs
  freed (ADR-011's addendum); a name is typed without an allocation per
  pattern (vendored xdg-mime patch 0002: 1,411 allocations to type
  `notes.txt`, now 6; about 13 µs a name). Own share 95.3 → 92.9 (icons)
  → 91.5 MiB, what the 2026-10-04 commit read that day.

**Deviations:**
- Split into 5a and 5b (ADR-035); Phase 6 and Release 0.1 came before
  5b's gate (the user's choice).
- Packaging: no Flatpak (its sandbox hides the programs Skwir runs, and
  bubblewrap cannot run inside it). ADR-040 chose Debian's, RPMs and the
  AUR over an AppImage carrying Nix's C library, which cannot load other
  systems' graphics drivers. ADR-052 adds a conventional AppImage built
  against Ubuntu's compatibility baseline instead.
- No step yet covers the NTFS USN journal.

---

## Phase 6: The extension boundary

**Goal:** open the plugin system beyond the repo, safely. Only now,
because the traits are stable.

**Scope:**
- **Permission layer.** Capabilities per plugin (read contents, write,
  network, spawn processes) and per-folder policies ("never leaves this
  machine" for `~/.ssh`, anything under a `.private` marker). Required
  before any model provider exists.
- **Level 2, external processes.** A small line-delimited protocol
  (LSP-style) for provider processes; first for thumbnailers and tools
  we already call.
- **Level 1, WASM plugins.** wasmtime and the component model for
  providers, commands, metadata. Not views.
- **CLI and MCP server.** Thin consumers of the command registry:
  `skwir cmd rename ...`, and an MCP server exposing commands and plans
  so an external agent (e.g. Claude Code) can drive the app.
- Plugin authoring docs and a template crate.

**Done when:** one existing internal plugin is re-implemented as WASM
with no user-visible difference, and an external agent completes a
rename-and-move task over MCP that ends in a reviewable plan.

**Status:** gate passed 2026-10-07. Built that day, in this order:

- **CLI and MCP** (ADR-043): the `control` plugin (off by default)
  listens on a Unix socket in a folder of the user's alone (a named pipe
  on Windows); `skwir mcp` carries an agent's MCP there (`rmcp`);
  `skwir cmd` runs one command and prints JSON, in a windowless Skwir
  when none listens. An agent's plan is proposed and its tool answers
  with the verdict (`App::propose_awaiting`); `apply_plan` hands its
  plan back, so nothing skips review; commands take `entries` by
  absolute path (`App::targets`), so an agent's call never acts on what
  the user selected meanwhile.
- **Permission layer** (ADR-044): `Plugin::capabilities`, shown on the
  Plugins page; private folders (the Privacy page) never reach an agent
  or a plugin from outside; `command::Effect` says what each command
  does; an agent's acting call waits for the user in the review's queue,
  a user-only one is refused, answers lose private paths. Checked on
  Xvfb with a scripted agent: a `.ssh` and a `.private` folder refused
  by name; the home folder it listed held `files` where the user saw
  `.ssh`, `diary` and `files`.
- **Plugins from outside** (ADR-045): a folder with a `plugin.toml` in
  the data folders' `skwir/plugins` or `SKWIR_PLUGIN_PATH`, off until
  turned on; fields, thumbnails, previews and commands through one JSON
  protocol; files by handle, read only with `read`; plans need `write`
  and are reviewed. Level 2 runs it as a program under bubblewrap (no
  network unless declared; not on Windows). Checked by
  `crates/extend/tests/programs.rs`; the first thumbnailer from outside
  is `crates/plugins-outside/netpbm`.
- **Level 1** (ADR-046): Wasmtime (49.0.2 since 2026-10-08, minimum
  Rust 1.96; release step 8 says why), no WASI, Skwir's two imports,
  bounded work and memory, in `skwir-module-worker`, so Wasmtime never
  enters Skwir's process. Measured 2026-10-07 with Wasmtime 38: linked
  in, it cost 1.6 MiB of idle memory (94.8 against 93.2 MiB own share);
  a module is compiled once and kept in the plugin's cache folder, so
  the hex module answers in 4.0 ms against 188 ms compiling (release,
  median of 7), and a sandboxed helper restart took 23 ms against
  231 ms the first time.
- **Gate, first half:** the hexadecimal preview as a module
  (`crates/plugins-outside/hex-view`), the same page for page, match
  for match and error for error (`crates/extend/tests/modules.rs`, nine
  views of four synthetic files), and in the window the preview panel
  differs by no pixel.
- **Gate, second half:** first a scripted MCP client on Xvfb with the
  release binary (a rename, then a move, applied with Enter in the
  review; a trash discarded with Escape). Then Claude Code, given
  `skwir mcp`, against its own Skwir on Xvfb with synthetic files, from
  the tools' descriptions alone. It found what scripted agents had not,
  fixed: a listing's `total` still counted redacted private files, so a
  content search told whether a private file held a word (ADR-044's
  fifth point, `App::withheld`); `navigate` answered before listing
  (`entries` waits); reading a folder moved the user's view (`entries`
  takes `location`); `mkdir` only in the folder shown (`in`); reviews
  did not say an agent proposed (`Plan::by`); `undo` did not say what
  it took back; search syntax and some descriptions were written for
  Skwir's developers.
- **Previews from outside** (ADR-047): a plugin may answer with a file
  Skwir shows (`convert`), or a document a page at a time (`page`), read
  by Skwir's reader with text, links and outline; links open only if
  the plugin may use the network. Checked on Xvfb: a PPM shown as
  Netpbm's PNG; a `.log` made Markdown by a Python program plugin; a
  log shown in pages, its text selected and searched.

Left: the packages carry no module (the built-in hex preview stays
theirs); no typed WIT world (payloads are JSON); a module has no
network; program isolation is compiled only on macOS and absent on
Windows, where no program plugin runs; the Windows pipe keeps the
system's default security (`docs/PORTING.md`). Control stays off by
default. A command that only answers (`entries`, `tab.list`,
`control.status`) is data, which the palette leaves out
(`Declared::data`, 2026-10-13).

**Deviations:**
- Begun before 5b's gate, as the user chose: 5b needs machines this
  phase does not.
- The smoke test planned for Release 0.1 drives `skwir cmd`, not
  `skwir mcp`.

---

## Release 0.1: Linux

**Goal:** a first public release people can install and trust with
their files, and a repository others can help with. It comes before
Phase 7.

**Decided by the user (2026-10-07):**
- **Linux alone.** macOS and Windows need machines that are not here
  (5b); what each needs is written as steps others can take
  (`docs/HELP-WANTED.md`).
- **Plugins stay on or off by default as they are.**
- **The plugin protocol is experimental**, and its three unpublished
  versions are one, version 1 (ADR-047, its sixth point revised). Done
  2026-10-07.
- **The repository is published and CI set up last**, by the user.

**Steps** (how each was done, with what was checked: `docs/STATUS-LOG.md`,
"Release 0.1's steps"):

1. **Privacy of the repository.** Done 2026-10-11: every blob and commit
   message scanned for this machine's identifiers, two strings fixed,
   the history rewritten once before the first push; the commits made
   since are searched before each push.
2. **Panics.** Done 2026-10-07: counted by clippy's restriction lints,
   each `expect` stating what the code above made true; a command that
   panics fails its call, not its caller.
3. **Fuzzing** what reads files Skwir did not write. Done 2026-10-07 as
   property tests over any input (each crate's `any_input`,
   `crates/skwir/tests/any_file.rs`). Not covered: what runs in a helper,
   where a crash ends the helper, not Skwir.
4. **Files under failure.** Done 2026-10-07
   (`crates/core/tests/failures.rs`), with the hard links' fixes of
   2026-10-10 (ADR-049).
5. **What users keep from 0.1 on.** Done 2026-10-07: settings read with
   defaults for what is missing, a file that is no TOML set aside, the
   cache in a folder its layout names.
6. **In the window.** Done in part: `just smoke`, checks in the window, a
   run of 109 minutes, the budgets. Left: what was not checked in a
   window ([What is left](#what-is-left)).
7. **Clean.** Done 2026-10-07 but splitting files (no change needed one).
8. **Licences.** Apache-2.0 alone (ADR-042); every dependency allows it;
   the notices in every package, kept with the source (ADR-063), the
   Windows and macOS builds' included (ADR-074, ADR-075); `just deny`.
9. **Documents.** Done 2026-10-07: the README, `CONTRIBUTING.md`,
   `SECURITY.md`, `docs/HELP-WANTED.md`, the issue templates,
   `CHANGELOG.md`, `skwir --version` and `--help`.
10. **The release.** Named Skwir (2026-10-10); local releases and a
    minimal routine CI (ADR-053, `docs/RELEASING.md`); the AUR's tools;
    the app icon; Ubuntu's own packages; the tarball (ADR-052); released
    2026-10-13 as `v0.1.0`; the AUR's `skwir-bin` and `skwir-git`
    (2026-10-17).

**Done when:** every step above is done or says why not, the packages
built by the local gate (or manual CI) install and run on Debian, Ubuntu,
Fedora and Arch, and the history scan finds nothing personal.

**Status:** released 2026-10-13 as 0.1.0. Steps 1 to 10 are done or say
what is left.

---

## Names indexed by skwird

ADR-050. Step C built 2026-10-20; A, B and D wait for skwird's phase 1
(its repository; its phase 0, the measurements, done 2026-10-11).

**Goal:** name searches that answer from an index where it proves it is
current, and walk only where it does not. skwird is a project of its
own (its `docs/DESIGN.md` and `docs/PLAN.md`); these are Skwir's steps,
after Release 0.1 and after skwird's phase 1.

- **A. Coverage in the hints** (ADR-026, amended). `HintProvider` says,
  for a root and the facts a search needs, whether it covers them (live,
  checked, as of a time, not covered and why). A search under a covered
  root considers what the provider names and does not walk there.
  Tested with a fake provider that covers some roots and not others: the
  answer equals the walk's.
- **B. A skwird provider** in the query plugin, speaking protocol 0
  through skwird's client crate. It finds a system service, then a
  user's, then starts skwird on demand where it is installed. Tests use
  a fake skwird on a socket of the test's own, plus the real engine
  where it is installed.
- **C. plocate as of its time.** A search of `/` that plocate answers
  says when its database was last updated. **Built 2026-10-20**
  (ADR-050's amendment): a listing keeps what its answer rests on
  (`ListingEvent::AsOf`), the footer says "as of Yesterday 03:00" with a
  tooltip, `entries` answers `as_of`. Not checked: Orca reading the
  footer; a real system database (none on the reference machine).
- **D. Then, each decided on its own:** the address field searching as
  one types, Usage of a tree from skwird's folder totals, "recently
  changed", the tags index's checks.

---

## Colors and font from the desktop

ADR-071, ADR-073. Steps A and B built 2026-10-19: Linux's portal is read;
Plasma's scheme, GTK3 themes, macOS and Windows not yet (C to E). The
fonts by role (ADR-073) built the same day. How each step was built and
checked: `docs/STATUS-LOG.md`, "Colors and font from the desktop".

**Goal:** Skwir wears what the user chose on their desktop (the accent,
more contrast, a color scheme, the font's family) on controls that stay
its own, every color readable on what it is drawn on.

- **A. One derivation, guarded.** `theme.rs` derives every token from a
  few base colors; Skwir's light and dark palettes are that derivation
  over its own. The contrast guard (4.5:1 for text, 3:1 for the accent;
  7:1 and 4.5:1 when more contrast is asked) moves a foreground's
  lightness until it passes. **Built 2026-10-19.** Not checked in a
  window: the warning's color in Settings, the drag's count, link labels.
- **B. The portal on Linux.** Accent, contrast and fonts, read on a
  thread of its own and followed through `SettingChanged`; the last
  answers kept in the cache for the first frame; the `ui.desktop_colors`
  switch. **Built 2026-10-19.** Not checked: Xfce, Cinnamon, MATE and
  Plasma reading the font (their tests read the files and programs
  alone).
- **C. Plasma's color scheme** from `kdeglobals`, watched; tested on
  synthetic schemes.
- **D. GTK3 themes**, found as the icon theme is; `@define-color`s,
  `@import`s and GTK's color functions read from synthetic themes, then
  real ones (adw-gtk3, Arc, Materia).
- **E. macOS and Windows:** the accent, more contrast, Windows' contrast
  themes; read again on appearance changes and activation. Windows tried
  under Wine for what it answers; macOS compiled only.
- **Fonts by role (ADR-073).** Three choices, Automatic or a family:
  `ui.font` (Appearance), `reading.font` (Reading › Text), `ui.code_font`
  (Reading › Code); Automatic is the desktop's font while Desktop colors
  is on, else Skwir's. The fonts are tokens of the theme, so a change
  shows at once; a long list of choices narrows as you type. **Built
  2026-10-19.** Not checked: Plasma, Xfce, Cinnamon and MATE reading
  their fonts, a screen reader on the long list, the session's own
  display.

---

## Phase 7: AI

**Goal:** agents that organize, rename, clean, tag and summarize, always
ending in a plan the user reviews.

**Scope:**
1. `extract`: a content extraction provider (text from PDFs and
   documents, OCR, EXIF, audio and video metadata), cached by content
   hash, using installed tools where present. Helps plain search at
   once.
2. `cleaners`: deterministic: exact duplicates (hash), near-duplicate
   images (perceptual hash), empty folders, stale downloads. No model.
   Each produces a plan.
3. `fulltext`: a keyword index on Tantivy (ADR-003), embedded, no
   service; one index per subtree, built only when asked, refreshed
   incrementally by mtime and content hash, in the deletable cache,
   pausable. It answers `content:` when an index covers the query's
   root; otherwise the scan runs, so the index only accelerates. Text
   comes from `extract`. System indexes (Tracker, Baloo, Recoll,
   Spotlight, Windows Search) and a running Meilisearch are optional
   providers for the same term, never required.
4. `models`: a `ModelProvider` trait (chat with tool calling,
   embeddings, image understanding, transcription): a local runtime
   (lazy-loaded), Ollama when detected, remote APIs only by explicit
   opt-in with a persistent "leaves your machine" indicator. Zero idle
   cost when disabled.
5. `models-small`: embeddings, CLIP-style image tagging and search,
   OCR, a document classifier, on CPU or iGPU. They do the volume; the
   LLM orchestrates.
6. `semantic`: a scoped, on-demand, incremental subtree index in the
   cache, visible and pausable. Natural language becomes a
   `LocationProvider` in the same address bar.
7. `agent`: the LLM plugin. Tools are the command registry; its output
   is a `Plan`; it never executes. Prompts treat file names and contents
   as untrusted. Flows: organize this folder, rename these consistently,
   what is safe to delete here, tag the selection, summarize in the
   preview.
8. Metadata from models carries `Model(name, confidence)` provenance and
   shows as suggestions until confirmed, then becomes User provenance
   written to xattrs.
9. An MCP client, so the agent can use external tools the user
   configures.

**Done when:** "organize this folder" on a messy Downloads produces a
correct, undoable plan using only local models, and disabling every AI
plugin returns idle memory to the Phase 2 baseline.

**Status:** planned; waits for Release 0.1 (the user's choice,
2026-10-07). Measured ahead, nothing built: a trigram index of what
`content:` reads, timed against the scan (`skwir-bench trigram`,
2026-10-10; ADR-050's context has the numbers). The permission layer it
needs exists (Phase 6, ADR-044).

---

## Phase 8: Polish and release

**Scope** (ongoing):
- Accessibility audit (a screen reader on each system) once gpui
  supports accessibility (ADR-001; it does since ADR-064, and ADR-065
  started exposing Skwir's places on 2026-10-18); IME, high-DPI, themes.
- Docs, a plugin gallery, a public release with an opinionated default
  plugin set.

**Status:** planned. Deviation: the first public release is Release 0.1,
before Phase 7.

---

## Cross-cutting rules

AGENTS.md and ADR-000 hold the rules. Per phase: end with a benchmark
run, update its status here (deviations included, honestly), and write
an ADR for any decision that closes a door.
