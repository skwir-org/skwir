# Skwir — Implementation Plan

Skwir is an experimental file manager in Rust: a GPU-accelerated UI
(gpui), a tiny core, everything else a plugin, built to be driven by
humans, scripts and AI agents alike. This file holds the phases with
their goals and **Done when** gates, the status of each, the deviations
from the plan, and the plans not yet built. Decisions are in
`docs/adr/`; the per-change notes of 2026-09-18 and 09-19, with their
measurements, are in `docs/STATUS-LOG.md`.

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

## Current status (2026-10-11)

- **Where:** named Skwir (skwir.org, application id `org.skwir.skwir`),
  version 0.1.0. Phases 0 to
  4, 5a and 6 pass their gates. Maps and Notes everywhere are built, as
  is much that no phase planned
  ([Built beyond the plan](#built-beyond-the-plan)).
- **Next:** finish [Release 0.1](#release-01-linux): confirm the history
  scan, date the release metadata, commit/tag, build locally and review
  the uploaded GitHub draft (ADR-053).
  Then skwird's steps (ADR-050) and Phase 7, which the user put after
  the release (2026-10-07).
- **Blocked:** Phase 5b needs a Mac and a Windows machine
  (`docs/HELP-WANTED.md`). The user reports hosted CI is too slow;
  routine CI is now reduced to essentials, with full checks and package
  builds retained as manual fallbacks. Remote runs were not inspected.
- **Budgets:** idle own share 94.8 MiB of 100 and first frame median
  145.2 ms of 150 (session display, 2026-10-11); `just bench-check`
  meets all 44 budgets (2026-10-11, load average 3 to 5.5).
- **Latest work (2026-10-07 to 10-10):** drops that ask, plans that
  pause, copies at a USB drive's pace, hard links as entries (ADR-049),
  archives extracted (ADR-048), then about eighty commits removing work
  done while nothing changed.

| Phase or plan | State | Planned size |
|---|---|---|
| [Phase 0: Foundations and toolkit spike](#phase-0-foundations-and-toolkit-spike) | gate passed 2026-09-18 | ~2 weeks |
| [Phase 1: Core kernel](#phase-1-core-kernel) | gate passed by 2026-09-18 | ~4 weeks |
| [Phase 2: Minimal daily-driver](#phase-2-minimal-daily-driver) | gate passed (a week of daily use; recorded 2026-10-04) | ~4 weeks |
| [Phase 3: Visual richness and performance hardening](#phase-3-visual-richness-and-performance-hardening) | gate passed by 2026-09-18 | ~4 weeks |
| [Phase 4: Power features, each as a plugin](#phase-4-power-features-each-as-a-plugin) | gate passed 2026-09-18, holds 2026-10-04 | ~6 weeks |
| [Built beyond the plan](#built-beyond-the-plan) | built 2026-09-18 to 2026-10-10 | — |
| [Maps](#maps) | steps built 2026-09-27; gate not declared | — |
| [Notes everywhere](#notes-everywhere) | steps built 2026-09-30; gate in part (no Mac) | — |
| [Phase 5: Cross-platform](#phase-5-cross-platform) | 5a gate passed 2026-10-06; 5b waits for machines | ~6 weeks |
| [Phase 6: The extension boundary](#phase-6-the-extension-boundary) | gate passed 2026-10-07 | ~4 weeks |
| [Release 0.1: Linux](#release-01-linux) | in progress: last step left | — |
| [Names indexed by skwird](#names-indexed-by-skwird) | planned (ADR-050) | — |
| [Phase 7: AI](#phase-7-ai) | planned, after Release 0.1 | ~10 weeks |
| [Phase 8: Polish and release](#phase-8-polish-and-release) | planned, ongoing | — |

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
  gpui's support, checked at every gpui update.

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

What was built between and beside the phases, by area. The rules these
features follow are in AGENTS.md and the ADRs named; this section keeps
dates, measurements and what is left.

### Previews and readers

- **2026-09-18 and 09-19** (ADR-003 to ADR-009): PDF and Office pages,
  video stills; PDFium and Poppler readers in helper processes with
  text, search, links and outlines, scrolled continuously; video and
  audio through a GStreamer worker (ADR-008; vendored patch 0003 fixes
  a Blade atlas lifetime crash under rapid video switching); ten
  structured preview plugins (ADR-009). Measurements and checks:
  `docs/STATUS-LOG.md`, `docs/video-playback.md`,
  `docs/pdf-comparison.md`. Left then: audible lip sync not assessed;
  native-resolution 4K output, GPU buffer sharing, external subtitle
  files, a second backend; encrypted archives, full-file waveforms.
- **One preview frame** (2026-09-21, `docs/preview-frame.md`): one
  toolbar and its slots for every kind, one find, one contents drawer;
  text in the code view, HTML in reading mode; Details lists everything
  known; archive members and attachments open as files, two containers
  deep. Deviations: no folder total from the Usage cache (a plugin the
  UI cannot depend on); no hit count in the structured find field
  (providers search as they go). EXIF, missing then, came later
  (`skwir_platform::exif`).
- **Music** (2026-09-22): perceptual volume, `scaletempo`, a log-band
  spectrum, Previous and Next through the folder, playlists (M3U, PLS,
  XSPF, CUE), the pin that keeps playing, `playing::NowPlaying` mirrored
  by the `mpris` plugin, ReplayGain, gapless hand-over (one worker
  carries the queue). MPRIS checked on a private session bus with
  `playerctl`.
- **Reading** (2026-09-24, ADR-012 to ADR-015): reading mode keeps a
  document's text CSS; XHTML read as XML; footnotes in popovers;
  superscripts and small capitals (vendored patch 0006); fixed-layout
  pages (ADR-013); MuPDF as a third PDF engine and a book engine
  (ADR-014, AGPL worker); pictures and pages drawn at the size they
  cover, in 512 px steps up to 4096 px, 100 % being real size
  (ADR-015); fixed-layout EPUB and CBZ open in the reader; a Reading
  width preference; printed page labels (`ui.printed_pages`); the
  desktop's thumbnail stands in only after a 200 ms load. Deviation:
  MuPDF's `system-fonts` feature is replaced by our own fontconfig
  loader (its `font-kit` broke the workspace build).
- **One plugin for audio and video** (2026-09-25, ADR-016):
  `preview-audio` folded into `preview-media`.
- **More formats** (2026-09-28):
  - SVG by resvg; OpenEXR and DDS decoded in process; HEIC, AVIF, JPEG
    XL and raw by the desktop's thumbnailer at 1024 px. Code highlighted
    with bat's 213 grammars (two-face): about 1.2 MB more once code is
    first shown. WOFF and WOFF2; JSONC and JSON5 as written; JSON
    Lines, Excel (calamine, cells streamed) and SQLite (the system's
    libsqlite3, read-only, immutable) as tables; ODS and XLS stay with
    LibreOffice, whose readers there allocate the whole grid; Jupyter
    notebooks as documents. Left out: Radiance HDR and farbfeld (the
    MIME database names neither).
  - SVG thumbnails (2026-10-11), drawn as the preview draws a drawing.
    The decoder had refused SVG while `decodes` kept ImageMagick away,
    so only a desktop thumbnailer drew one. 6,000 of a desktop's SVGs
    drew in 0.08 ms at 128 px (median; p99 1.2 ms, the slowest 34 ms,
    Adwaita's blurred `user-trash-full`). A compressed drawing's text
    was never drawn (text was looked for in the compressed bytes).
    A thumbnail whose ink would not read on the window's background
    (under WCAG's 3:1), but does on a light backdrop, is drawn on one:
    dark lines on nothing, on the dark look (`backdrop::behind`); so
    are the preview's picture and a document's pictures. Backdrops
    behind every picture with transparency left light corners around
    rounded icons, and boxes on the light look.
  - Text read where it is shown (ADR-051, 2026-10-11): a text file and
    every format's source scroll whole, numbered from their first line,
    instead of 256 KiB and 32 KiB pages ("1746 lines from byte 32725",
    numbers starting again, Find in the page). A worker counts the rows
    into an index of every 256th; the view reads chunks of 256 rows
    around what it shows, plain first, then colored. Checked on Xvfb,
    debug build: a 200 MB log (2,036,070 lines, the count matching
    `wc -l`) to its end; Find across a 49 MB log both ways and around,
    and "No match in the file" under a second; Copy of a selection,
    exact, and of all of it, stopped at 8 MiB and said; a 2 MB minified
    line, shown at once (colored a chunk at a time, it waited seconds
    when the first chunk was colored before showing); lines unwrapped
    and scrolled sideways, the gutter kept; a terminal's `code.rs:1200`
    showing its line; a log written to while shown followed at its end;
    UTF-16 read in its own units (pages past the first read it as
    UTF-8). gpui handed the clipboard to another X11 program in one
    request, which carries 16 MiB less its header: a 16 MiB copy was
    never answered and the clipboard stopped answering until restarted.
    Vendored patch 0021 (2026-10-12) sends it in pieces (INCR), and Copy
    went back from 8 MiB to 16: xclip read 16 MiB whole, and a reader
    killed mid-transfer left the next served. The byte view reads the
    same way since 2026-10-12, a row of 16 bytes each, its pages asked of
    the bytes view as they come into view: on Xvfb, a 3 MB file to its
    last row (0x2FFFF0), the scrollbar dragged mid-file, Jump to
    0x123450, Find through three matches and "No further match", a row
    copied and decoded. Its pages are asked of the bytes view by name
    (chosen again, the providers before it tried the file first: 2 to 3 s
    a page in a debug build, then about one), and one the view left is
    cancelled: a page came 0.2 s after the scrollbar was let go in a
    release build.
  - Mermaid by merman in `skwir-mermaid-worker`: linked into Skwir it
    added 14.7 MB of code (text 51.9 → 66.6 MB), so it is a helper
    (16.3 MB), a default member. Without it a diagram stays code.
  - 3D models: `preview-models` (~120 KB of code) shows the picture
    Blender, 3MF, FreeCAD, DWG, DXF and G-code files store, with print
    facts; `skwir-model-worker` (pure Rust, 1 MB) draws meshes without
    one (STL, OBJ, PLY, OFF, glTF): 3DBenchy (225,706 triangles) in
    140 ms, volume 15.55 cm³ as published. STEP and IGES stay with f3d
    (nixpkgs' f3d names STEP `application/vnd.step`, which this MIME
    database lacks); OpenCASCADE would be a helper of its own.
    SketchUp, LayOut and Rhino pictures checked on two SketchUp 2021
    models, a LayOut 2021 document and two Rhino 5 models; older
    SketchUp and newer Rhino files only on synthetic ones.
  - Circuits (`preview-circuits`): KiCad boards (since 4), footprints,
    schematics and symbols (KiCad 6 on), matching `kicad-cli`'s SVG on
    KiCad 9 and 10 demo projects; 50 to 600 ms a file at 2048 px
    (release), peak about 25 MB beyond the file (an 84 MB board: 3.3 s,
    111 MB), so files over 32 MiB are not drawn; text in a system font
    (KiCad's stroke font is GPL). Gerber (RS-274X, not the deprecated
    knockout) and Excellon drill files: gerbv's pictures on KiCad 10
    exports and gerbv's test files, ≤ 60 ms.
  - DXF from its entities: ezdxf's geometry on 18 files, 3 to 790 ms
    each. G-code from its moves: 16 real programs in ≤ 140 ms at
    1024 px as at 128; the three that compute moves with parameters
    stay text.
- **Text documents** (2026-10-03, ADR-030): Word, OpenDocument text and
  RTF read by Skwir (`preview-documents`, the default), LibreOffice
  pages or Pandoc, each giving way to the next when missing. Measured
  once (a small synthetic Word file): LibreOffice 1.2 to 2.7 s and up to
  370 MB, Pandoc 0.03 s and 120 MB. Not covered: `.fodt`, presentations
  and spreadsheets as text, find beyond the part shown, links to
  bookmarks in another part.
- **Pictures by ImageMagick** (2026-10-03, ADR-031): the last-resort
  thumbnail and preview for AVIF, HEIC, JPEG XL, JPEG 2000, raw, PSD,
  XCF and DjVu, sandboxed under a policy written per run (256 MiB, one
  thread, two at a time); the desktop's thumbnailers run sandboxed
  where bubblewrap can. A 12-megapixel AVIF to 256 px: ImageMagick
  0.72 s and about 213 MB, `heif-thumbnailer` 0.57 s; ffmpegthumbnailer
  sandboxed about 5 ms slower. Declined: AVIF in process (rav1d, 2.7 MB
  of code with its assembly) and KDE's thumbnailers (a Qt helper for one
  desktop).
- **Archives extracted** (2026-10-09, ADR-048), by libarchive in its
  sandbox.

### The window

- **Settings** (2026-09-18 to 09-20, ADR-010): the dialog; pages follow
  their subject (2026-09-25: `reading.*`, `Settings::rename`, the
  Plugins page by `Plugin::category`); the Keyboard page edits
  `keyboard.bindings` (2026-09-28, ADR-024).
- **Usage** (2026-09-19, ADR-007): a plugin pane view with cached,
  watched, progressive sizes (`docs/STATUS-LOG.md`).
- **Huge folders** (2026-09-25): the UI applied a listing in one drain,
  holding the UI thread 0.9 s for `/nix/store`'s 366,000 entries (warm,
  release). Draining in slices (`PUMP_BUDGET`) and placing arrivals in
  the sorted order: first rows at once, complete in 0.75 s. The columns
  view keeps the folders above open (`App::keep_locations`).
- **Tabs** (2026-09-26, ADR-018): ten dormant tabs cost no PSS beyond
  noise over one (now a budget). The tab left stays drawn while the new
  one gets ready, at most 150 ms; every measured switch, up to 100,000
  entries (89 ms), was ready within it. Keys moved (the user's choice):
  Open Terminal Here Ctrl+T → Shift+F4, Tags… Ctrl+Shift+T →
  Ctrl+Shift+G. Open: a flaky test,
  `listing::pending_opens_report_errors_asynchronously_and_can_be_replaced`,
  failed eight runs in a row, then passed; not investigated.
- **Terminal** (2026-09-26, ADR-019): F4; `alacritty_terminal` stays in
  the plugin (libghostty-vt may replace it once stable and buildable
  without fetching sources). +0.5 MB stripped binary, nothing idle with
  the panel closed, +2.3 MB open. Not yet: mouse reporting, an input
  method's text while composing, search in the history. Open: on Xvfb a
  restored panel appears only at the first input after startup (not
  checked on a real compositor).
- **Keys** (2026-09-28, ADR-024): the `vim` plugin (yazi's keymap, off
  by default), a selection that knows it was picked, key layers,
  sequences (vendored patch 0010). Deviations from yazi: no line modes,
  links, tasks or trash place; Left, Right and Tab keep Skwir's meaning;
  `f` and `s` both search; `u` undoes (vim's).
- **Context menu by file** (2026-09-28): `when` says `kind:image` or
  `has:.gpx`; `registry::MenuSource` names the default application.
  Open starts that application itself (`xdg-open` on Hyprland reads only
  named defaults and `mimeinfo.cache`, so a picture KDE remembered for
  Gwenview opened in a browser). The MIME database took 55 to 85 µs to
  type a name; since then its glob index (vendored xdg-mime patch 0003:
  100,000 names in 0.12 s, was 2.25 s) and rows typed once (2026-10-10).
- **Rows and toolbars** (2026-09-29): the toolbar overhaul. Step A
  moved the views and the View menu into each pane's row; step B put
  search and a filter in the address field (pills, `omnibox.rs`); then
  the application toolbar and the status bar went (each area owns its
  top; per-pane footers). Chrome above a folder's first row: 108 px →
  104 (step A) → 72 (one tab). Step C, a selection action bar (commit
  d9e1ec5), was tried and reverted: not needed beside the context menu
  and the keys.
- **Tree view** (2026-09-29, ADR-025). Deviations: no Ctrl+digit (Usage
  and the Map hold 4 and 5); a nested selection was not restored after
  a tab slept (opened folders are kept since 2026-10-09).
- **Links** (2026-10-03, ADR-029). Not covered: table cells, Details
  comments, lines past a long file's first page.
- **Trails** (2026-10-04 and 05, ADR-032 amended): every location reads
  as a trail and its provider says its title, icon and empty state; a
  folder's trail starts at the deepest sidebar folder holding it, under
  the sidebar's name; a chevron before the first segment lists the
  folders above (`ui.parents_menu`), what a narrow pane wrapped out
  included; a location's own buttons are its provider's (`Verbs::tools`:
  the shelf's Move and Copy to the other pane, the trash's Empty Trash).
  Rejected: leaving drives out of that menu (it needed the mount table
  on a worker and hid the chevron from a drive mounted under home).
- **Icon themes** (2026-10-06 and 07): Linux's theme is read where the
  desktop keeps it (GSettings, Cinnamon's and MATE's, `kdeglobals`,
  xfconf, then GTK's `settings.ini`); themes are known by their folder,
  not their display name (a theme whose two names differed fell back to
  Adwaita); Breeze's small icons are drawn as shapes, so they read on a
  dark theme. `ui.icon_theme` (Appearance) picks any installed theme,
  applied at once; free desktops only.
- **Code themes** (2026-10-09 and 10): code in a theme of bat's or the
  user's, kept light or dark whatever Skwir is.

### Search

- **Words for people** (2026-09-25): `kind:` grouped as the MIME
  database groups types; named periods (`modified:today`, `this-week`,
  `last-month`, `this-year`).
- **Dates are the user's** (2026-09-25, ADR-017): the system's time zone
  and the locale's first weekday; a search showing a named period lists
  again when the period moves on (the clock checked at least every ten
  minutes). Saved searches with dates shift by at most the zone's
  offset.
- **Facets** (2026-09-25): chips for each facet (`query.describe`,
  `query.refine`), Find Similar; vendored patch 0007 (a key's click
  fires only on the element it went down on). The chips became pills in
  the address field on 2026-09-29.
- **Saved searches** (2026-09-25): kept in Skwir's settings
  (`query.saved`), under Searches. Rejected: the GTK bookmarks file
  (every GTK file chooser would show them as broken rows) and one list
  mixing both (an order file of our own over two stores). The core
  `keep` command hands a location to its scheme's `registry::Keeper`.
  `query.save` no longer replaces a search of the same name;
  `query.forget` takes the expression.
- **Searches follow changes** (2026-09-30): after each plan the query
  plugin checks the files it touched under each open search's root.

### Tags, places and the trash

- **Tag-preserving copies** (2026-09-19, ADR-006):
  `core.preserve_tags_on_copy`, on by default (`docs/STATUS-LOG.md`).
- **Tags and notes** (2026-09-30): rating (`user.baloo.rating`),
  comment (`user.xdg.comment`), origin read only; the tag editor applies
  only what moved (`tags.edit`); Details' Notes and Source groups; a
  Rating column; `rating>=4` and `comment:`; copies keep every `user.*`
  attribute (ADR-006 amended).
- **Finder tags and hints** (2026-09-30, ADR-026): grid dots; a Mac's
  Finder tags read and written with their colors; ACLs, capabilities and
  SELinux labels in Details. Measured with `skwir-bench`: a stat-only
  search of a million files 2.2–2.8 s → 0.64–0.9 s through a folder kept
  open (`fs::Statter`); `tag:` is bound by reading attributes (btrfs,
  about 1.6 s per million, not scaling past eight threads), so a search
  asks hints first: the first of 1,000 hits in a million files arrives
  after 0.10 s. Gaps: no Finder attribute a Mac wrote was read (tests
  write Apple's format with our encoder); a cold `tag:` is not measured
  (dropping caches needs root).
- **Tags everywhere** (2026-09-30): Tags in the sidebar; `tags.rename`
  and `tags.delete` over a search, proposed for review; a Mac's `._`
  files read on FAT, exFAT, NTFS, network and FUSE volumes (Apple's
  `copyfile.c` layout); a folder's `._` names read once per listing (a
  per-file lookup costs a third of a `listxattr`). `bench-check` on a
  quiet machine: `flat100k` streams in 49.7 ms (budget 100), `images10k`
  in 10.7 ms (12); `tag:` over a million files 2.1 s. Gaps: no `._` file
  a Mac wrote was read (the layout is built from Apple's source); the
  sidebar suggests only tags Skwir's index has seen, not Baloo's;
  renaming leaves hidden files alone, as the search does.
- **Pills** (2026-10-01): sidebar tags wrap as pills; 20 listed by
  default (was 10).
- **Tags are places** (2026-10-03, ADR-032): `tags:` locations,
  location verbs, a Folder column and Show in Folder (Ctrl+Alt+O), tag
  export as folders, tag mosaics, the board, Narrow by. The Tags view
  and `PaneView::command` went. Bench: a tag location 1.5 s (budget
  4 s). Not checked in a window: an export by a real drag (tested
  through the clipboard).
- **The sidebar is the user's** (2026-10-04, ADR-034): places moved,
  renamed and hidden; volumes named by their filesystem; F6 for the
  keyboard. Not checked in a window: a drag between groups refused.
- **The trash is a place** (2026-10-04 and 05, ADR-034): `trash:` lists
  the home trash and the volumes'; trashed folders open in the trash;
  parts restore alone (`RestorePart`, `ReturnPart`) and merges bring
  their companions (`Unmerge`); moving a file out of the trash restores
  it there (`Untrashed` undoes); a Deleted column and sort; a plan Undo
  cannot take back asks first; a trashed folder cannot be bookmarked.
- **Busy volumes** (2026-09-29): `Place::busy`, `Volume::container`,
  `App::release` (an unmount waits up to 3 s for helpers). Checked with
  fake `lsblk` and `udisksctl`; not on a real drive.
- **Bookmarks** (2026-09-25): rename, remove and reorder, each with Undo
  and Redo; Ctrl+D and the pane's button; the GTK file watched, so
  other file managers' edits appear; lines Skwir does not touch are
  kept.
- **Open With** (2026-09-25): the `open-with` plugin and `ui.choose`;
  "Always use for…" rewrites the user's `mimeapps.list` through
  `AssociationEdit`, which returns its inverse for Undo.

### The consistency review (2026-10-05)

Nine read-only reviews looked for code saying again what other code
already knows, and found about 170 places in ten patterns, fixed in
order the same day:
- Standalone bugs: among them F1 hiding every binding with a `!`
  condition, Shift+F3 splitting the window in a structured preview,
  "Tags out of Name" renaming two files to one name, helpers reading
  files outside the sandbox (`sandbox::untrusted`), Open With typing a
  pipe named `x.png` by its name, `skwir trash:` opening `./trash:`.
- The sidebar from its places' answers: `Place::drop` gone (a folder
  dropped on the Shelf was bookmarked); Rename… and Remove From Sidebar
  built for every place; the names the user gives places reach the core
  (`Registry::set_place_titles`, `App::location_title`).
- One surface for the shell's commands: `ui.*` was a 150-arm switch
  invisible to agents that guessed defaults (`ui.sort {key: "type"}`
  sorted by name); now declared in `commands.rs` and checked
  (`CommandRegistry::check`); 22 cursor commands became `ui.cursor`;
  `keymap::PLACES`; `Shell::toggle_popup`; `ui.list_move`.
- An entry drawn one way in every view (`Shell::look`, `views::entry`);
  the list's columns one table (`views::list::COLUMNS`); seven sizing
  edges one element (`elements::edge`); column widths in the session.
- Safe reads (`fs::file::read_regular`: a pipe named like a sidecar held
  a worker); one follower for document links; `process::Worker` (a
  helper exiting 127 before greeting gives way to the next engine);
  plocate bounded and cancellable; the `OpenLocation` lock rule; one of
  each duplicated helper.
- No plugin's name in the shell; a folder as a location like the
  others; providers take files by type (`mime::of_request`,
  `preview::is_office`). Rejected: `LibreOffice`'s desktop entries as
  the office list (they claim plain text, CSV and PDF).

Left: a menu marks the current item two ways (a check for an option, a
highlight for where one is).

### 2026-10-07 to 10-10

Drops that ask (`core.drop_asks`); plans that pause, and Quit that
waits for them (`Executor::pause`, `Executor::wind_down`); a crash's
half-made copy swept by a later run (`op::staging`); copies at a
removable drive's pace (`op::pace`) that ask when short of room
(`op::room`); hard links as entries (ADR-049); archives extracted
(ADR-048); Usage redrawn; code in bat's themes. On 2026-10-10, about
eighty commits over what woke or worked while nothing changed: idle
threads, startup, work per frame and per batch of metadata, memory
given back.

### 2026-10-11 to 10-12

Names already taken (ADR-054): a copy or a move onto names in its
destination asks once about all of them, before anything is done (keep
both as `name (copy).ext`, replace into the Trash, replace if newer,
skip; a folder onto a folder merged), for all of a kind or row by row;
a copy into its own folder and two items of one name keep both unasked;
agents and scripts say beforehand (`on_conflict`). The answers are the
plan's steps, checked and undone as any. On the way, undoing a trash
restores its own item on Linux (it names its record), so a replace
undone brings back what it replaced. Left: on Windows undoing a trash
restores the latest item from its path (PORTING.md); `link` still
refuses a name taken.

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
  one `listxattr` per file. Old `set_tags`, `set_rating`, `set_comment`
  plans still load. The index learns from `Executor::observe`.
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
  (`crates/plugins-outside/preview-hex`), the same page for page, match
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
system's default security (`docs/PORTING.md`); a command answering a
value does nothing from the palette (`entries`, `tab.list`,
`control.status`). Control stays off by default.

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

**Steps:**

1. **Privacy of the repository.** Scanned 2026-10-07: every blob and
   commit message (4,482 blobs, 324 commits) for this machine's
   identifiers, addresses, home paths, keys, positions and phone
   numbers. Every hit was synthetic or public but two, fixed in the
   tree: a drive's label in a comment (`address.rs`) and a home folder
   in `vendor/patches/0001`'s `diff` lines (the patch recipe now drops
   them). Both stay in history until the one rewrite the user chose
   (2026-10-07), just before the first push, which also gives every
   commit the author address chosen for the project (2026-10-10).
   **Left:** the rewrite, then the scan again.
2. **Panics.** Done 2026-10-07. Counted by clippy's restriction lints,
   tests left out: 4 `unwrap`, 77 `expect`, 5 `unreachable!`, 748
   indexing or slicing, 150 string slices. The `expect`s state what the
   code above made true (where a file is read, each follows a match on
   the node's type or a list built with one item); the string slices
   read cut at an ASCII character found or after a byte check; step 3
   covers the rest. A command that panics no longer takes its caller
   down: the registry catches it, logs it, and the call fails.
3. **Fuzzing** what reads files Skwir did not write. Done 2026-10-07 as
   property tests over any input (proptest; coverage-guided fuzzing
   needs a nightly compiler): each reader gets its own sample mutated
   (bytes changed, multi-byte characters where ASCII is expected, cut
   short; `skwir_core::testing`) and random input, and must not panic.
   Each crate's `any_input` (geo's thirteen formats, `._` files,
   Finder attributes, the tags index, XMP, the query language, names,
   jump's lists, links, volumes, entries, users, XHTML, encodings,
   packages, EXIF, MP4, trash records, an edited buffer, tile archives)
   and `crates/skwir/tests/any_file.rs` (33 samples of 32 types shown
   every way Skwir shows a file in its own process). One run of 20,000
   cases each (64 to 128 for those writing a file a case; 4,000 for the
   preview test); the tests keep 256 (128). Nothing panicked. The font
   preview's test (2026-10-08) hit a panic in `read-fonts`, skrifa's
   parser (a simple glyph with no points but bytes after its
   instructions). It was already reported (fontations#1962) and fixed
   in read-fonts 0.42 / skrifa 0.45 (checked 2026-10-10: 0.44 panics,
   0.45 and 0.48 draw it). skrifa stays at 0.44 while gpui's
   rasterizer, swash 0.2.10, takes none later; the specimen catches the
   panic per glyph and leaves that glyph out (a test builds it).
   **Not covered:** what runs in a helper (PDF, media, archives, 3D
   models, Mermaid), where a crash ends the helper, not Skwir.
4. **Files under failure.** Done 2026-10-07
   (`crates/core/tests/failures.rs`): a copy into a folder refusing
   writes changes nothing; a plan stopped by a failure keeps what it did
   undoable; a folder copy failing partway leaves no part; a name too
   long is refused with nothing changed. Fixed: a cross-filesystem move
   that could not remove all its source, and a delete stopped partway,
   said only "Permission denied", so files looked lost (now: the copy
   is whole, and what is left); failures read as log lines, now in the
   user's words (`op::said`), the chain in the log. Hosted CI exposed a
   test assumption on 2026-10-11: source removal can delete the writable
   file before failing on the protected one. The test now verifies the
   complete destination and unchanged source survivors, and compares the
   error's named file and count with the actual remainder, without
   assuming traversal order. Both creation orders are exercised; locally
   these reproduce one and two surviving files. Found 2026-10-10
   and fixed, each with a test (ADR-049): two names of one file were
   one row; a plan whose name had gone could act on another name of its
   file; Empty Trash left one of two names trashed apart. With it, space
   is counted per file, copies keep hard links among what they carry,
   and a copy short of room asks first. A copy cut short by a kill
   leaves its hidden `.skwir-copy-…` folder in the destination, never a
   part copy under the file's name; since 2026-10-08 a later run sweeps
   it (`op::staging`, where this machine alone writes). The journal is
   memory, so Undo does not outlive Skwir (by design: no database).
5. **What users keep from 0.1 on.** Done 2026-10-07: settings are read
   with defaults for what is missing and nothing refused for what is
   unknown (the session included); renamed keys follow
   (`Settings::rename`). Fixed: a settings file that is not TOML or not
   text stopped Skwir from starting; it is now set aside
   (`settings.toml.unreadable-…`), never overwritten, and Skwir starts
   with defaults and says so (if it cannot set it aside, it saves
   nothing). The cache lives in a folder its layout names (`<cache>/1`,
   `cache::LAYOUT`); a release that would misread an older layout raises
   it, and older layouts are deleted on a thread at start.
6. **In the window.** Planned: the "not checked in a window" items, a
   scripted smoke test on Xvfb, a run of hours, `just idle-pss`, `just
   bench-check` and the first frame on a quiet machine. Done in part:
   - `just smoke` (`tools/test/smoke.py`, 2026-10-07): the release
     window on an X display of its own (Xvfb and Mesa's software Vulkan,
     from :90 up), a home of synthetic files, driven by `skwir cmd`
     (list, search, tag, rename carrying it, mkdir, move, Trash, five
     undos), each checked on disk; four runs in a row passed.
   - In the window (2026-10-07, release, Xvfb): the board's keys, vim's
     keys, the middle button on a tag's pill, Narrow by over 10,000
     pictures (CPU back to 0–1 % within 10 s), a PDF's mail link, the
     unreadable-settings notice, a failure's toast. Fixed: a tag dragged
     into a folder proposed a plan that could never apply (`op::dry_run`
     now counts what earlier steps make); failure toasts led with the
     plan's title and full paths; a missing PDFium library was said as a
     helper to install.
   - A run of 109 minutes (release, Xvfb, `skwir cmd` moving every 10 s
     between 100,000 files, a 10,000-picture grid and a deep tree, with
     searches): no command failed; PSS flat (157 MiB average over the
     first ten minutes, 140 over the last ten, software Vulkan
     included); CPU under 2 % a minute while moved, 0.0 % over the last
     idle minute.
   - 2026-10-10, after the rename, in a user and mount namespace of its
     own (the machine's drives hidden under an empty tmpfs; a btrfs
     subvolume at `/mnt/stick`): a file trashed on the stick cut and
     pasted at home, taken out with its `.trashinfo`, and undone; a drop
     on a trashed folder trashes (ADR-034); Empty Trash over two
     trashes; `stick › Photos`; plocate answering a name search of `/`
     (a file newer than its database not found, as ADR-050 says); files
     dragged from another X11 application (dragon-drop) asking, then
     copied. Fixed: Empty Trash crashed the window (two sorted entries
     removed in one event left the kept order's count one too high), now
     with a property test of the kept order. `just smoke`, poppler-check
     and mupdf-check pass under the new name.
   - Budgets:

     | Date | Check | Result |
     |---|---|---|
     | 2026-10-07 | `just idle-pss` | own share 91.1 MiB (median of 3, budget 100); first frame 151–156 ms (153–172 earlier that day), over 150 on a busy machine; idle CPU 0.4–0.6 % |
     | 2026-10-07 | `just bench-check` | all budgets met |
     | 2026-10-10 | `just bench-check` | all 42 budgets (load average 4.6); a first run under load 8–20 missed three of `images10k`'s, as did the build before the rename, alternated with it |
     | 2026-10-10 | `just idle-pss`, Xvfb, software Vulkan | own share 76.0 MiB (median of 3); idle CPU 0.0 % |
     | 2026-10-11 | `just idle-pss`, the session's display | own share 94.8 MiB (median of 3; 91.1 on 2026-10-07; GPU buffers 19.6 of it); first frame 136–151 ms, median 145.2, within 150 by ADR-000's rule (the least of the runs; one of three above); idle CPU 0.2–0.4 % |

   **Left** (not recorded as checked in a window): F5 in Usage,
   Alt+click on a tag's pill, an office document converted, a missing
   engine library, a drag between sidebar groups refused, a tag
   exported by a real drag, a restore that merges (tests only).
7. **Clean.** Done 2026-10-07 but splitting files: every `dead_code`
   allowance holds test helpers for one system, shared test modules or
   fields read through serde; every `unsafe_code` allowance states its
   safety; 22 dependencies no crate named, found by cargo-machete and
   checked by hand, removed; every crate says `publish = false`.
   `proc-macro-error2`, which a future Rust refuses, came with gpui's
   `stacksafe` 0.1; vendored patch 0015 (2026-10-08) moves gpui to 1.0,
   which uses `syn` alone. Left: no file split (no change needed one).
8. **Licences.** Skwir is Apache-2.0 alone (2026-10-06, ADR-042, the
   user's choice; was MIT or Apache-2.0): `LICENSE`, every crate and
   package. Every dependency built into Skwir allows it. The Poppler
   reader is GPL-3.0 (Poppler's core is GPL 2 or 3), the MuPDF reader
   AGPL (ADR-014); Arch's `skwir`, holding the Poppler reader, says so.
   The dependencies (2026-10-07) are permissive, MPL-2.0 for ten crates.
   Done 2026-10-07: `THIRD-PARTY-NOTICES` in every package (each linked
   crate's licence and text, by cargo-about through
   `tools/package/notices.py`, offline; `linux/check.sh` checks it is
   installed, in the distributions' roots on 2026-10-10); `deny.toml`
   (licences, RustSec advisories, crates.io; `just deny`, a CI step).
   Found by it: Wasmtime 38.0.4 had seventeen advisories (an
   out-of-bounds write in the component model's string transcoding, a
   heap access miscompiled into a sandbox escape on aarch64, loads
   outside the sandbox on x86-64); now 49.0.2 (2026-10-08, commit
   679e874), which needs Rust 1.96 (the workspace minimum, was 1.88)
   and counts the fuel instantiation takes (the helper gave none, so
   every module trapped until fixed); a
   yanked `yoke-derive` updated; the unmaintained `rustls-pemfile`
   replaced by rustls' own PEM reading. The unmaintained `ttf-parser`:
   the font preview moved to `skrifa` and `zeno` (2026-10-08, `fontdue`
   gone) and fixed book pages take metrics from `skrifa`; it now comes
   only with gpui's and resvg's text stacks, so its advisory ignore is
   gone. The toolbar's icons were made for Skwir
   (`crates/ui/assets/toolbar/README.md`). Left: notices for the macOS
   and Windows packages.
9. **Documents.** Done 2026-10-07: the README as a front page (what it
   said of previews, tags and Usage, much no longer true, is now in
   `docs/guide.md`); `CONTRIBUTING.md`, `SECURITY.md`,
   `docs/HELP-WANTED.md` (macOS and Windows step by step, **try** where
   running Skwir and reporting is enough), `.github/ISSUE_TEMPLATE/`
   (warning of personal data in paths and screenshots), `CHANGELOG.md`;
   `skwir --version` and `--help`.
10. **The release.** Named 2026-10-10 (the user had reconsidered the
    name since 2026-10-07): **Skwir**, at skwir.org, its repository
    `https://github.com/skwir-org/skwir`, its application id
    `org.skwir.skwir` (the metainfo, the desktop entry, the window's
    app_id and WM_CLASS, macOS's bundle identifier; one constant,
    `skwir_core::APP_ID`). bibli became Skwir everywhere; nothing reads
    the old names, since no release used them. Version 0.1.0
    everywhere. **Release workflow revised 2026-10-11 (ADR-053):**
    routine GitHub CI is one cached job (formatting, core/API lint and
    tests, release-tool tests). Full checks, native jobs and package
    builds are manual fallbacks. Tags only validate release metadata.
    `just release-build <tag>` requires clean tagged source and dated,
    matching versions, runs the full local gate, builds/install-tests
    the AppImage and Debian/Fedora/Arch packages, and stages a checked
    bundle. `just release-upload <tag>` verifies local assets and the
    remote tag before creating/updating a GitHub draft, never publishing.
    **AUR publication tooling (2026-10-12):** the Linux Nix shell
    provides pacman/makepkg, Git, OpenSSH and curl. `aur-prepare <tag>`
    verifies the release bundle and remote source tag, fills the archive
    checksum and stages PKGBUILD/.SRCINFO in a separate AUR checkout;
    `aur-publish <tag>` commits and pushes those reviewed files, with
    retries and no force push. Validation: all 19 release-tool tests
    pass, including real Nix makepkg against a local AUR stand-in; the
    actual three-package SRCINFO and macOS shell evaluation also pass.
    Dependency checks pass. The benchmark passes 43 of 44 budgets;
    `images10k.stream_complete_us` measures 12,903 us against 12,000 us.
    The full release gate was attempted but stopped at a Clippy
    `items_after_statements` error in the concurrent, uncommitted
    `crates/core/tests/clashes.rs` changes. No package has been published
    by this change. See [Releasing](RELEASING.md). The packages, built by `try.sh` in
    fresh Debian 13, Fedora 44 and Arch roots (2026-10-10), install alone
    and with the others,
    every helper answers `--probe`, and the licences and notices are
    where each distribution keeps them. Found: cargo-about builds its
    program only with `--features cli` since 0.9; Ubuntu's runners
    refuse bubblewrap its namespaces until told not to; the media
    controls named the desktop entry by the old name.
    **App icon** (2026-10-11): the identity's ink tile, the squirrel
    reversed on it (`crates/ui/assets/org.skwir.skwir.svg`). About shows
    it, and every Linux package (the Nix one too, with the desktop entry
    and metainfo it lacked) installs it in hicolor's scalable apps for
    the desktop entry to name. The macOS bundle makes its `.icns` from it
    (librsvg and `iconutil`); `skwir.exe` carries it as an `.ico` its
    build script draws when built on Windows (resource 1, which gpui
    gives the windows), and so does the installer's shortcut. Those two
    never ran.
    **Validation:** the new minimal CI recipe passes locally (285 nextest
    tests). The actual `just release-check` also passes: full lint and
    Windows/macOS cross checks, dependency checks, 1,474 workspace tests
    (19 skipped), the window smoke test, both native readers, all 44
    performance budgets, and 13 release-tool integration tests. Those
    cover staging, failed checks, source/version disagreement, altered
    assets, remote SHA256 disagreement and draft-only uploads. Uploads
    use a fake GitHub CLI in temporary repositories; no real draft was
    created. Package builds recorded above predate this orchestration
    change; its output isolation is tested with synthetic packages.
    GitHub CLI authentication and repository write access are verified;
    no real release upload has been attempted.
    **Left:** confirm the history scan, date and commit the release,
    create/push its tag, run the local release build and review/publish
    its draft. Not done: AUR publication and Ubuntu's own package build.

**Done when:** every step above is done or says why not, the packages
built by the local gate (or manual CI) install and run on Debian, Fedora
and Arch, and the history scan finds nothing personal.

**Status:** in progress: steps 1 to 9 done or say what is left; step
10 waits for the final release metadata, tagged local build and publication;
the history scan's completion has not been reverified here.

---

## Names indexed by skwird

ADR-050. Planned; nothing is built.

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
  says when its database was last updated.
- **D. Then, each decided on its own:** the address field searching as
  one types, Usage of a tree from skwird's folder totals, "recently
  changed", the tags index's checks.

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
  supports accessibility (ADR-001); IME, high-DPI, themes.
- Docs, a plugin gallery, a public release with an opinionated default
  plugin set.

**Status:** planned. Deviation: the first public release is Release 0.1,
before Phase 7.

---

## Cross-cutting rules

AGENTS.md and ADR-000 hold the rules. Per phase: end with a benchmark
run, update its status here (deviations included, honestly), and write
an ADR for any decision that closes a door.
