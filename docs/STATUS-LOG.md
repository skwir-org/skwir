# Status log

The dated record of what was built: what was measured, what was checked
and how, what was found and fixed. `docs/PLAN.md` holds each phase's
current status and what is left; this file keeps the detail behind it.
Newer entries go at the end, a section per day of work or per step of a
plan; benchmark reruns and one-off memory samples belong in commit
messages.

The first two sections are the notes from the top of `docs/PLAN.md` on
2026-09-18 and 09-19. The sections from "Built beyond the plan, by
area" to "Colors and font from the desktop" were `docs/PLAN.md`'s until
2026-10-20, moved here as they were written.

Memory figures in the first two sections are PSS of the whole process
unless said otherwise; they predate ADR-033's own-share budget and are
observations, not controlled before/after comparisons.

## 2026-09-18

- **Phases 0 to 4 built.** Phase 4's gate: thirteen plugins, each
  proven by a test to disable with no trace in any registry and to
  re-enable; the trait surface unchanged for the last three written
  (jump, tags, shell). Idle PSS with every plugin: 99.9 MB (budget
  100). Phase 2's week of daily use not yet run. The deviations then
  recorded are in `docs/PLAN.md`, Phase 4.
- **Previews through installed tools** (ADR-003): PDF and Office pages
  and seekable video stills, two toggleable plugins; Poppler first, in
  place of the deferred PDFium; Office through isolated LibreOffice.
  Left: spreadsheet cell navigation, continuous video playback.
  - 1334×1214 window, preview closed, three synthetic files: 117 MiB
    (the previous revision, same conditions: 145 MiB). Warm first frame
    132–151 ms; idle CPU about 1 % of a core.
  - A synthetic scanned page's backend time: 1777 → 99 ms (24 ms
    cached), same pixels (fast lossless PNG, metadata reused).
- **PDFium reader** (ADR-004): per-page selection and copy,
  whole-document search, links, outlines, converted Office files too;
  the library loaded only in a cancellable child process; Poppler
  selectable and the fallback. Deferred: passwords, OCR, editing,
  selection across pages. Found: preview textures stayed in GPUI's
  atlas (now evicted on replace and close); PDFium page buffers now
  reach GPUI without a copy. After 50 page turns: Skwir with PDFium
  about 201 MiB, with Poppler 177, both stable; fresh idle 115–117 MiB.
- **Settings dialog:** schema-driven, from the toolbar, the palette or
  Ctrl+comma; `settings.apply` validates whole batches and saves the
  edited keys atomically on a worker; a failed save keeps the draft;
  providers refresh through a plugin settings hook (one defaulted method
  added to the plugin trait). A fresh release window, four synthetic
  files, after opening and closing it: 106.9 MiB, idle CPU 0.8 %.
- **Poppler reader** (ADR-005): `skwir-poppler-worker` keeps Poppler,
  GLib and Cairo out of the UI process and speaks PDFium's protocol;
  Auto prefers PDFium, then Poppler, then the command-line image path.
  Parity checked on rotations with cropping, text, search and link
  geometry, named destinations, empty pages, missing outlines, limits
  and corrupt PDFs. After 50 page turns, Skwir and its worker: about
  185 MiB with Poppler, 191 with PDFium; closing a preview reaps its
  worker. Timings in `docs/pdf-comparison.md`.
- **Toolbars:** bundled symbolic SVGs; one reader toolbar for both
  engines; rows pick compact variants by intrinsic measurement in
  prepaint. A fresh 654×1214 release window, four synthetic files:
  104.7 MiB, idle CPU 0.6 %, first frame 147 ms (one sample).

## 2026-09-19

- **Continuous documents:** single page or continuous; a virtual list
  keeps a reading anchor across mixed page sizes, resizes and zoom;
  rendering serialized and cancellable, neighbours prefetched, offscreen
  pixels evicted over a 64 MiB target (visible pages kept regardless).
  No worker protocol change.
- **Preview toolbar and popovers:** images and video stills share the
  reader's buttons, zoom presets and Fit; side panel and peek share one
  frame; Fit width uses the source's aspect ratio (GPUI letterboxed by
  intrinsic pixel height). Popovers anchor below their visible trigger,
  measured in the current prepaint; peek stays in the normal overlay
  tree, as GPUI rejects nested deferred draws. With the 100k-entry list
  loaded: 160.2 and later 173.9 MiB, idle CPU 0.8 %.
- **Tag-preserving copies** (ADR-006): `core.preserve_tags_on_copy`
  (on) for copies, paste, the shelf and drops; plans serialize the
  preference; tags copied before read-only permissions are restored; a
  staging tree published only after metadata succeeds. Bench: image
  streaming read 13.0 ms against 12 (the unchanged commit 14.9);
  isolated 15-run minima 3.4 and 2.5 ms; the final full run passed once
  a competing build ended.
- **Usage as a plugin pane view** (ADR-007): bounded cached breakdowns,
  unchanged branches reused, scanned subtrees watched, affected
  branches invalidated; a stale warning and periodic revalidation where
  watching is unavailable; Ctrl+R recalculates. Notifications refresh
  only intersecting panes, with per-path fallback deadlines; the busy
  indicator waits 200 ms.
  - Settled Usage (debug): 133.5 MiB, CPU 2.0 %. Release, 1189×1334,
    small tree: 116.0 MiB, idle CPU 1.2 %; after paging grid and list
    175.1 MiB, 2.2 %. A first frame of 989 ms on a busy session.
  - Bench: image streaming 21.6 ms against 12 (the pre-change binary
    20.1); isolated minima 11.2 and 13.6 ms; a full 15-run check passed
    (10.4 ms), then `just bench-check` (9.7 ms). No budget loosened.
- **Progressive Usage:** a shallow snapshot first, then bounded
  snapshots of growing totals at most every 200 ms; unfinished blocks
  striped, totals labelled as counted so far; only complete, readable
  breakdowns cached; a finished chart stays through later scans and is
  replaced atomically. Debug, the million-file tree: 228.4 MiB and idle
  CPU 2.2 % after the checks. Bench: image streaming 24.0 ms once, 9.1
  in a full 15-run check.
- **Navigation cancels:** Usage requests follow the pane's location
  lifetime; recursive watches replaced by per-folder registrations
  during the cancellable walk; folder open, stat and watch run on the
  scheduler, so the window navigates and paints while a drive wakes; a
  loading message after 200 ms; open errors shown in the pane. Checked
  on Xvfb with a 5 s open delay injected (not a sleeping drive). An
  issued system read must still return before its worker sees the
  cancellation. Bench: 12.66 ms once, then 9.62 (budget 12).
- **Video playback** (ADR-008): an optional GStreamer worker, paused
  first, with seek, volume, speed, audio and subtitle tracks,
  fullscreen and keys; `auto` keeps FFmpeg stills without it; one
  RAM-backed frame slot and a latest-frame mailbox; output capped at a
  1920-pixel side. Deferred: native 4K output, GPU buffer sharing,
  external subtitle files, a second backend. Audible lip sync not
  assessed. Measurements: `docs/video-playback.md`.
  - A crash under rapid video switching: an evicted Blade atlas texture
    could still have queued uploads while its index was reused. Vendored
    patch 0003 cancels queued work before recycling and retires
    textures after the previous GPU submission (`just atlas-check`).
    The patched release survived 320 rapid selections; the unpatched
    one panicked after 49. Idle CPU after: 1.4 %.
  - Fullscreen on a black, aspect-preserving canvas (320×180 upscaled to
    2560×1440); controls hide after 1 s idle; a cached
    native-resolution FFmpeg poster (≤ 8192 px a side, never upscaled)
    replaces the thumbnail, cancelled by Play or Seek; 3840×2160
    checked exact.
- **Structured previews** (ADR-009): ten plugins (audio, hex, Markdown,
  archives, EPUB, CSV/TSV, JSON/YAML/TOML, fonts, email, comics);
  archives and comics through the optional libarchive helper. The
  paused audio worker: 0.0 % CPU, 12.6 MiB. Left out then: EPUB
  publisher CSS and fixed layouts, encrypted archives, full-file
  waveforms, nested PDF and video sessions in containers.
  - Follow-ups: archive contents at the list's density; ASF
    (`application/vnd.ms-asf`, `.asf`) by the media provider, audio-only
    streams with a spectrum; JSON, YAML and TOML as highlighted code
    with folds, invalid or oversized input as paged source.
  - Bench: `images10k` first batch 2664 µs (budget 2500) and completion
    20,363 µs (12,000) once, cause not found; 1663 and 11,753 µs on the
    next run. Completion 12,966 µs once more, 9897 after concurrent
    builds ended.

## Built beyond the plan, by area (2026-09-18 to 10-17)

What was built between and beside the phases, by area. The rules these
features follow are in AGENTS.md and the ADRs named.

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
  `preview-audio` folded into `media`.
- **One picture for a film** (2026-10-16, ADR-008 amended): a film
  selected showed its desktop thumbnail full size, then the same
  thumbnail at 256 px once its player opened, then its poster, another
  frame (recorded at 30 fps on Xvfb: 230, 270 and 900 ms after the
  click, a 1080p H.264 clip, ffmpegthumbnailer's film strip). It now
  shows one frame of its own: the poster, else the player's first
  frame; the thumbnail only when no frame comes. The poster is the first
  keyframe from a tenth of the film on, only keyframes decoded, told at
  its own time (`ffmpeg` alone: 440 to 100 ms at 1080p, 970 to 150 ms
  at 4K). Measured the same way: an uncached poster at about 330 ms at
  1080p and 470 ms at 4K (600 and 730 before), "Loading…" from 230 ms;
  a cached one at 200 ms, with no "Loading…"; without `ffmpeg`, the
  player's frame at 200 ms; with autoplay, frames playing at 200 ms.
  Left then: the poster was read only once the player had started.
- **Posters at the size shown** (2026-10-17, ADR-008 amended): a film's
  or a track's picture is read while its player opens
  (`PreviewProvider::media`), at the side the panel shows it in 512 px
  steps, as JPEG, laid out by the film's own size; the still and the
  poster are one request. Measured on grainy H.264 clips (`ffmpeg`
  alone): 215 to 102 ms at 1080p, 804 to 281 ms at 4K, entries 4 and
  16 MB of PNG to 122 and 59 KB. In the window (Xvfb, recorded at 30
  fps): an uncached poster at about 130 to 170 ms (330 and 470 before),
  a cached one at 33 ms (200), the player's toolbar from the first
  picture on; a track's halo at 70 ms; a track stepped to while one
  plays still plays. A track's glow changed hue when its cover went
  from the desktop's thumbnail to its own, as it did before (fixed
  below). A poster drawn larger than its
  pixels (a wider panel, the whole screen) is read again alone at the
  next step, the same frame: in a window on Xvfb, a 4K film's 1024 px
  poster was replaced by a 1536 px one in fullscreen, 0.3 % of the
  pixels changing, all on edges, and playing asked for nothing more.
- **A cover's colour at any size** (2026-10-17): the halo's tint was
  read from every n-th pixel in a row's order, so only a few columns
  when the width was a multiple of n: 16 of a 256 px thumbnail, 4 of a
  1024 px cover. The test pattern's thumbnail glowed magenta (297°), its
  own cover green (139°). Read at the middles of a 64 by 64 grid, they
  are 140° and 149°. In a window on Xvfb, an uncached track's glow kept
  its colour from the thumbnail to the cover (the ring's mean moved by
  0.1 of 255), the disc 0.4 % of its pixels sharper.
- **A track's thumbnail in its figure** (2026-10-17): a track whose
  cover and player both came after the wait (the first track a window
  opens, while GStreamer reads its registry) showed its thumbnail as a
  plain picture under "Loading…", then the halo. The thumbnail standing
  in for a track, the provider saying the file is media, is now its
  picture in the player's figure, with the player's bar
  (`Shell::shows_player`). In a window on Xvfb: the halo with the
  thumbnail at about 270 ms, the cover in the same place at 500 ms,
  0.7 % of the disc's pixels changing.
- **A clip with one keyframe** (2026-10-17): a film with no keyframe
  from its tenth on (a short clip whose one keyframe is its first) had
  no poster: `ffmpeg` fails when it has no frame to write, and the
  failure ended the picture. With GStreamer the player's frame came
  instead, later; with Still frames, no preview. What `ffmpeg` printed
  now tells, and the frame at the tenth is decoded exactly. Found by an
  opt-in test of a 2 s phone film; in a window, such a clip got its
  poster.
- **Posters fitted by the film's shape** (2026-10-17, ADR-008 amended):
  the poster was asked at the panel's longer side, so a landscape film
  in a panel taller than wide was read up to twice as large as shown.
  The panel now measures its content's area as a box (`media_box`, each
  side in 512 px steps) and the provider fits the film's own size into
  it (`PreviewRequest::fit`), turned as a phone's film is shown. In a
  window on Xvfb with a 420 px panel: a 1080p film's poster at 512×288
  (1024×576 before), a phone's portrait film at 512×911, upright.
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
  - 3D models: `models` (~120 KB of code) shows the picture
    Blender, 3MF, FreeCAD, DWG, DXF and G-code files store, with print
    facts; `skwir-model-worker` (pure Rust, 1 MB) draws meshes without
    one (STL, OBJ, PLY, OFF, glTF): 3DBenchy (225,706 triangles) in
    140 ms, volume 15.55 cm³ as published. Since 2026-10-19 a window that
    draws meshes on a GPU turns them instead (ADR-067). STEP is meshed by
    the same helper with ogeom (ADR-069), and IGES alike.
    SketchUp, LayOut and Rhino pictures checked on two SketchUp 2021
    models, a LayOut 2021 document and two Rhino 5 models; older
    SketchUp and newer Rhino files only on synthetic ones.
  - Circuits (`circuits`): KiCad boards (since 4), footprints,
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
  RTF read by Skwir (`documents`, the default), LibreOffice
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
  `keyboard.bindings` (2026-09-28, ADR-024), in place since 2026-10-13.
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
  (`query.saved`), under Searches; since 2026-10-12 in its data file
  (ADR-057). Rejected: the GTK bookmarks file
  (every GTK file chooser would show them as broken rows) and one list
  mixing both (an order file of our own over two stores). The core
  `keep` command hands a location to its scheme's `registry::Keeper`.
  `query.save` no longer replaces a search of the same name;
  `query.forget` takes the expression.
- **Searches follow changes** (2026-09-30): after each plan the query
  plugin checks the files it touched under each open search's root.

### Tags, places and the trash

- **Tag-preserving copies** (2026-09-19, ADR-006):
  `core.preserve_tags_on_copy` (now `core.preserve_attributes_on_copy`),
  on by default (`docs/STATUS-LOG.md`).
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

## 2026-10-05: the consistency review

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

## 2026-10-07 to 10-10

Drops that ask (`core.drop_asks`); plans that pause, and Quit that
waits for them (`Executor::pause`, `Executor::wind_down`); a crash's
half-made copy swept by a later run (`op::staging`); copies at a
removable drive's pace (`op::pace`) that ask when short of room
(`op::room`); hard links as entries (ADR-049); archives extracted
(ADR-048); Usage redrawn; code in bat's themes. On 2026-10-10, about
eighty commits over what woke or worked while nothing changed: idle
threads, startup, work per frame and per batch of metadata, memory
given back.

## 2026-10-11 to 10-12

Names already taken (ADR-054): a copy or a move onto names in its
destination asks once about all of them, before anything is done (keep
both as `name (copy).ext`, replace into the Trash, replace if newer,
skip; a folder onto a folder merged), for all of a kind or row by row;
a copy into its own folder and two items of one name keep both unasked;
agents and scripts say beforehand (`on_conflict`). The answers are the
plan's steps, checked and undone as any. On the way, undoing a trash
restores its own item on Linux (it names its record), so a replace
undone brings back what it replaced. Left: on Windows undoing a trash
restores the latest item from its path (PORTING.md). Then links: Link
Here and `link` onto a name taken ask as a copy does, a file's choices
for a link to a folder too (never merged); kept both, a link is named
`name (link).ext`; one into its own folder is kept beside, unasked.

The settings file is shared (ADR-055): a save writes the keys this
process changed over the file as it stands, and what changes there
(another skwir, a hand edit) is taken in while skwir runs, a plugin
turned on or off included. The flake has a Home Manager module
(`programs.skwir`) that merges declared settings at each activation, as
Home Manager's `mutableSettings` does, checked by `checks.home-manager`;
and an overlay.

The window's state is not a setting (ADR-056): its layout, tabs, last
folder and hints shown are kept in `$XDG_STATE_HOME/skwir/state.toml`.
The settings
file may then be read-only: a save writes through a link and says when it
cannot, the dialog says so. `settings.check` checks a file against what
skwir declares, with no window too; the Home Manager module runs it while
the configuration builds, and links the settings read-only by default
(`mutableSettings` merges them).

What the user makes in skwir is not a setting either (ADR-057): saved
searches, pinned tags, tags' colours and exclusive categories are kept in
`$XDG_DATA_HOME/skwir/data.toml`, so a read-only settings file no longer
stops them being saved. The data file is followed as the settings are,
set aside when it is no TOML, never written over; `settings.check` says a
key kept apart is no setting to declare. Checked in a window on Xvfb
with a read-only linked settings file: a search saved and a tag pinned by
`skwir cmd` reached the data file, showed in the sidebar and came back
after a restart, the settings file untouched; Link Here onto a taken
name asked ("Nothing is linked until you choose"), kept both as
`plan (link).txt`, and Undo took it back.

## 2026-10-13

The Keyboard page edits in place (ADR-024, amended): one row per action
and place with all its keys, a bar under the list that takes the next
keys pressed (Ctrl+Q is recorded, not run) and says what they would
replace or leave running before they are saved, a struck default with
Restore, Changed by you with a way back, Find by keys, and New shortcut
for a command with arguments (picked by name, its arguments asked as
its schema says them). Actions are called by their commands' forms
(`keymap_titles.rs` holds it for every built-in key); a shortcut for a
plugin turned off waits for it instead of blocking every save. Checked
in a window on Xvfb: a key changed, saved, struck and restored; the
palette's last key refused; a Go to location shortcut made and saved;
the list walked with the arrows; the place's caption kept at the list's
top. Not done: a key scheme chosen from the page, and "Set shortcut…"
from F1 or the palette.

Properties (ADR-058): Details renamed, the context menu's last item
(after the submenus, Alt+Enter). A
folder's total counted on request by the core (`fs::count`), not by
Usage as first planned (its requests are a pane's view's); room on the
drive, the drive, the system's flags; several entries' kinds, total and
shared access. Access changed by plans (`set_access`: chmod's grammar,
a group, what folders hold; no owner, no administrator rights), offered
in words. Plugins add rows through property sources: Open with's Opens
with, a new checksums plugin (SHA-256, MD5, BLAKE3, compared with one
copied). Checked in a window on Xvfb: Properties from the menu counted a
folder at once (a hard link once); `/` counted with 16 drives mounted
inside left out; the Nix store's count stopped and kept; a sparse 50 MB
image said 0 B on disk; Allow running made a script 755 and Undo 644; a
folder's others set to None with what it holds (750 and 640) and undone;
a group changed to one of the user's and back; `u+q` refused in words;
root's `/etc` read-only with why; a default application changed (written
to the test's own lists); a SHA-256 equal to `sha256sum`'s and compared
with its copied line. Not checked in a window: a read-only drive's
links taken away, a drive keeping no permissions (FAT), Windows' and
macOS' code (compiled and linted only).
Then made compact (ADR-058 decision 7): over a preview at most half the
room, above it or beside it in a room wider than tall, the edge between
them dragged; groups folded from their headings and remembered
(Checksums and Volume ship folded, the drive's gist on its heading); a
row's links under the pointer only,
over its last line, but for Count, Compute, Add… on nothing, Stop and
Count again. Checked on Xvfb: half the room over a picture, dragged to
three quarters and kept across a restart; beside the preview with the
panel at the bottom, dragged, and back to half on a double click; a
folder still filling the panel; Checksums unfolded and remembered,
Compute shown, Copy and Compare over a digest's last line. The mode, put
on Permissions' heading at first, went back among its rows: it looked out
of place there. A first try wrapped the value
and its links in one line that wraps: short values were laid out a
letter per line until the value grew into its line.
Then every folded group says its gist, as Volume's did: each row gives
its part (`Row::gist`, a source's `Property::gist`), joined once render
has added its rows, so a folder's count shows as it goes; a path is cut
at its start. Unfolded, a heading says only its title, so the mode
stays among Permissions' rows. Checked on Xvfb: a text file's groups
folded one by one (size, rating, path cut at its start, owner and mode,
modification time, the drive), a SHA-256 computed and folded (its first
digits, as `sha256sum` gives them), a picture's dimensions, a folder's
size once counted and nothing before, a selection's count and its
shared owner where the modes differ.

Archives made (ADR-059): Compress to ZIP, Compress… (the name's ending
picks the format: ZIP, 7z, tar and gzip, bzip2, xz or Zstandard tars) and
Compress with a Password… (an AES-256 ZIP), by the libarchive helper fed
on its standard input: skwir walks and reads the files, the helper opens
none. Extraction asks a ZIP's password, puts back the attributes a copy
keeps, and a name taken there is asked about with a file's choices,
through a core hook for steps that make something (`Operation::makes`);
Shift+F7 asks, as a drop does, what the selection does in the other
pane. Checked in a window on Xvfb: the three items; a ZIP made, made
again unchanged (nothing to do), changed and kept beside as `(2)`; a
`.tar.zst` from a typed name; an encrypted ZIP asked twice, which 7-Zip
reads as AES-256 Deflate with the password and refuses without; Shift+F7
offering Extract Here into the other pane, a wrong password asked again,
then extracted; the name taken asked with Extract and kept beside;
Escape on a password cancelling with nothing left; Compress… onto a name
taken, replaced, and undone. Tags through a tar, a pipe left out,
companions and pausing are checked by tests, not in the window. Not
done: Merge for an extraction onto a folder (its undo would need each
file it placed), passwords for agents, 7z and RAR decryption (libarchive
has none).

Plugin ids lost their `preview-` prefix with their titles (`media`,
`documents`, `structured-data`…; crates and folders alike), and the
migrations carried for earlier skwirs went, no one using it yet: settings
renamed at start, the session's old field and widths, state and data
moved out of the settings file, map tiles' single server, the tag notes'
old operations and index formats, copy plans' `preserve_tags`, keys bound
by old command names, `path` for `location`, the `when` token `none`.
`Settings::rename` and the cache's layout number stay, for the first
change after people use skwir.

A review before tagging, of the day's 22 commits by area, found what the
full checks had passed; all of it was fixed before the tag:

- **Access:** a recursive change walks folders held open and changes each
  item through a hold on it (`O_PATH` and `/proc/self/fd` on Linux,
  `AT_SYMLINK_NOFOLLOW` on macOS), so a link swapped in mid-walk is not
  followed; a folder its owner can still open is changed before what it
  holds, any other after, and Undo and Redo order by the modes they put
  back (`u=`, `u=r` and a Redo under a folder without search reach every
  item); Undo says what it could not put back; a group change leaves the
  set-ID bits the system cleared; a clause naming no class leaves out the
  umask, read from `/proc/self/status`, refused where it cannot be read;
  the dry run walks what it would change (a change applied again is done,
  another user's item inside is named, a trash inside is left alone) and
  stops with its plan.
- **Archives:** libarchive 3.6 (the AppImage's) only warns of a member
  whose CRC or authentication fails: such a member now stops extraction;
  done means unchanged as a copy's does (times, links, nothing more);
  Replace never trashes a step's own input; Compress… is the user's
  alone, never an agent's with a destination the prompt did not show; a
  one-file archive kept beside keeps its extension; a file that changes
  as it is compressed fails the step; the room is checked; empty
  passwords are asked again, attempts counted per member.
- **Keys and settings:** the Keyboard page's records are written last and
  take out the user's own they cover (a key changed back runs again), a
  key pressed again changes nothing, Restore and the way back never
  leave an action that keeps a key without one, what a change frees
  beside it is said before saving, a character bound anywhere still types
  in text fields, the keyboard never falls behind the Settings dialog; a
  key kept apart (the window's state, what you make) replaces the
  settings file's copy whole, so a colour taken away stays away;
  `settings.check` reports shortcuts to no command again.
- **Dialogs and the palette:** Tab walks a dialog's field and buttons,
  never the toolbars behind it; a focused Cancel cancels on Enter; a
  question names what it is about when asked (Rename renames the file it
  was opened for); the palette leaves out only data commands
  (`Declared::data`), so Copy paths and plugins from outside's commands
  are offered again, and offers the sorts the View menu offers.
- **Properties:** a count is kept by what it counts and stops where no
  folder is shown; a property source says its group ships folded, so the
  shell names no plugin's; several selected say their shared access
  apart, with their own drives' read-only state. Checksums asked from the
  window wait in one lane, every one asked of a file in one read, so a
  large file holds one worker; a non-UTF-8 name no longer fails.
- **Release notes:** what came after the date sat under "Unreleased",
  which the release's notes would have left out: folded into 0.1.0, and
  `release-metadata` refuses any section above the release's.

Not done: Windows changes access by path (a link swapped in between its
check and its change is followed, PORTING.md); a broad default moved on
the Keyboard page frees narrower keys of other actions, which the page
says rather than binds again; macOS' quarantine passed to what is
extracted is compiled, never run.

## 2026-10-14

A window left open overnight held 688 MB, its heap live data: the
largest part the grammars code had been colored with, each compiled once
and kept for good, with regex-automata's caches per worker. Code is now
colored by Ferroni, a pure-Rust port of Oniguruma, through vendored
syntect and two-face (ADR-060), with Oniguruma's set of bat's grammars
(PowerShell, Sass and four more arrive). The compiled grammars are let
go of a minute after code was last colored, and the workers that
compiled them are asked to release (`scheduler::release_on_workers`).
Measured in a window on Xvfb, 22 code files: +34 MiB after five passes
(+305 before), the heap back to 35 MiB a minute after leaving them (16
in an empty folder at start, 27 after the same files as plain text);
six rounds of browsing and leaving settle near 39 MiB.

Not done: Vue Component's two patterns Ferroni refuses (ferroni#227)
said with code points in the grammar; the patches offered to syntect and
two-face; what the code view leaves beyond plain text (some
7 MiB, not its grammars) not looked into.

The terminal's copy and paste keys, which ADR-019 named, had never been
bound: Ctrl+Shift+C reached the program as Ctrl+C and interrupted it.
They are bound in the terminal's place, and the window's keys shadowed
there come before every binding, so the terminal's win whatever layer
binds the same keys to the window; Shift+Insert pastes the primary
selection on Linux (ADR-019, amendment of 2026-10-14). The terminal
has a menu (the right button, the Menu key): copy, paste, select all,
clear the history, and a link's items. Files dropped on it, or the
selection with Ctrl+Shift+Enter, are typed at its prompt, quoted for
its shell.

A path the terminal found was looked up wherever it led: on Windows
`\\host\share` connects to that host with the user's credentials, and on
macOS `/net/host` mounts from it, so printing a file could make Skwir
reach a host the file names. Paths are looked up only where that stays
on this machine (`fs::volume::Here`, ADR-029 amendment of 2026-10-14).
Not tried: macOS's mount table (compiled only).

A link's fragment is a place in the file it leads to: `guide.md#setup`
shows that heading once the guide's preview arrives, `main.rs#L40` that
line; a link to the document itself scrolls in it. Back after a link
selected nothing: each step of a pane's history now keeps the entry its
cursor was on, which Back and Forward select again, and a link to a file
beside the one shown no longer lists the folder again. A line asked of a
small text as its preview arrived was said not to exist (the index had
not counted it yet): that held for the terminal's `file:12` links too.

The paths a file's text writes are links under Ctrl, as the terminal's
are: in HTML, Markdown and code source and in plain text, what quotes
hold, a link's target, `url(…)` and a word with a slash after a name,
looked up from the file's folder as a chunk is colored (ADR-029
amendment). A document's own links are followed only where that stays
on this machine. Not covered: the structured-data views made whole and
bare names.

A document inside an archive or a message followed its relative links
on the disk beside the archive. They now name the container's own files,
found from its rows' tree and previewed in the document's place.

Obsidian's `[[links]]` are links: the note named beside the note or from
the vault's top, a heading by its slug (ADR-029 amendment). Not done:
a search of the vault for a note elsewhere (waits for skwird's index),
notes embedded with `![[…]]`.

## 2026-10-15

Extract Here gives an archive's one folder or file as it is, whatever it
is called: `v1.0.zip` holding `project-1.0/` makes `project-1.0`, not
`v1.0/project-1.0` (ADR-048 amendment). Several members still go in a
folder named after the archive, and so does a hidden member alone. The
name comes from the archive's listing: a step's `makes` now takes the
plan's context, and settling fixes the name it found in the step, so the
dialog asks about a name taken under the name it will be placed under.
Not done: a Mac's `__MACOSX` beside the one folder still makes two
members, so a folder around them.

The wheel scrolls along its own axis everywhere: alone up and down, with
Shift across (vendored patch 0022). gpui let a scroller that moves one
way take the wheel's other way too, and scrollers inside each other all
moved: a board's column scrolled down with Shift while the board went
sideways, and a table went sideways with every turn of the wheel.

What scrolls sideways has its bar along the bottom: a table and bytes,
and a document's pages zoomed wider than the panel (a page at a time
gets its vertical bar too). Where both bars show, each leaves the corner
to the other.

A picture zoomed wider than the panel scrolls sideways. Its wrapper kept
the panel's width, so the picture spilled out on both sides and its left
was out of reach.

The columns view scrolls sideways, as Finder's does, instead of cutting
the folders farthest up in a narrow pane, and follows its right end,
where the folder shown is: a folder opened lands there, and it stays in
view as the pane narrows.

Up selects the folder left, in every view, as other file managers do;
in a tag's categories, the place left. `navigate` takes `select`, an
entry to select once listed.

Ctrl and the wheel do what the zoom keys do over every preview, not only
a picture: a document's pages zoom (a book laid out by its engine, its
text), text and structured previews change their text size, a container's
view kept above a member too. The pane's views had a capture-phase
overlay for it; the panel now shares it (`elements::zoom_wheel`). A
zoomed picture no longer scrolls as it zooms, a trackpad has to mean it,
and a film, which has no zoom keys, no longer zooms.

Fullscreen is the preview's, no longer the player's: a picture, a
document's pages, text and music go on the whole screen as a film did,
with their own bar over the top, hidden when the pointer rests (F, the
bar's Fullscreen, a double click on a picture). It stays while the file
changes: the arrows walk the pane's pictures, skipping other files, or
turn the page, and a slideshow plays the pictures from the one shown to
the last, at a pace chosen in its bar (`reading.slideshow`). Each wait
starts once the picture has arrived, so a slow one is shown its whole
time. `ui.media_fullscreen` and `ui.media_controls` became
`ui.preview_fullscreen` and `ui.fullscreen_controls`.

The screen stays awake while a slideshow or a film plays, through a new
gpui patch (0023, `Window::set_idle_inhibited`): Wayland's idle
inhibitor, X11's screen saver suspended, Windows' display requirement
(compiled only). Not done: macOS; a desktop that idles by its own rules
outside these (its D-Bus inhibitor).

On the whole screen the next picture is read ahead, the way the last
step went: decoded at the screen's side while one is looked at, shown in
the very frame of the step, adopted by the step when it is still
decoding. Only pictures skwir decodes itself, which no plugin takes: no
player, reader or other program is started for a file not yet asked for.

Source is remembered by type: turned on for a Markdown file, every
Markdown file opens on its source, after a restart too, until it is
turned off; other types keep their presentation. Kept with the window's
shape (`preview::SourceTypes`), decided on the worker where the type is
settled, so the first answer is already the source; a file of the type
that is no text opens as usual. The tests now go through the worker's
own step (`preview::inspect_shown`).

A preview that fails on a text file shows its text, saying why: an HTML
page over 2 MiB, an invalid JSON or KML file, a drawing that does not
draw, whatever failed (a plugin, a conversion, a player, a reader, a
picture). One rule on the worker (`preview::source_instead`) replaced
the two that structured data and circuits kept each their own way. A
file that is no text says its failure with View as hex in the bar, as
does a file with no preview: the one way left to look there, in More
elsewhere. An SVG drawing has a Source, as rendered text does, kept by
type. Not exercised in a window: a reader's or a converter's failure.
On the whole screen, the bar of pages or text stays at the top, what is
shown under it; a picture's, a film's and music's lie over them and hide
when the pointer rests, as every bar did before, covering a first line
or a notice while it showed. The pointer hides with a floating bar and
comes back with it, over the window only.

The columns view moves as yazi's and Finder's do. After the pane's
column comes the folder under the cursor, listed once the cursor rests
on it; a click on a folder shows it, with no double click. Right goes
into it, landing on its first entry, and never opens a file; a press in
that column goes into it with the row selected.

DjVu documents open in the reader (ADR-061, `djvu` plugin), their first
page their thumbnail: pages, zoom, search, text selection, links and
contents, as a PDF's. Two engines behind the PDF readers' protocol, each
its own isolated helper: DjVuLibre, linked by `skwir-djvulibre-worker`
(GPL, a package of its own where packages are split), and djvu-rs, in
Rust, whose helper every package carries. **DjVu engine** Automatic
prefers DjVuLibre and gives way to djvu-rs only where DjVuLibre is
absent. djvu-rs shrinks a page by averaging it drawn twice as large, and
is drawn at sizes it places exactly: its step through a page is
truncated to sixteenths of a pixel, which drew a page 2% too large at
two thirds. A page's words became characters and are searched by the
code a plugin from outside Skwir's pages used, moved to
`document::words`. `just djvu-check` runs both helpers on documents
DjVuLibre's tools make (a colour page, a turned one, links, an outline,
page titles) and measures where their ink lands: within 0.18% of the
page for DjVuLibre, 0.32% for djvu-rs. Not read: an indirect document,
whose pages are files the isolated helper cannot see; annotations other
than links; djvu-rs gives no page titles. DjVuLibre's helper is not in
the macOS and Windows packages. The whole screen's walk through a pane's
pictures now passes a file whose name several types claim: a `.djvu` is
a page or a book, and stepping onto a book left the arrows to its pages.


## 2026-10-16

A selection in a scanned DjVu book darkened as it grew: its text layer
placed lines and gave each word its line's box, so the words'
characters lay over one another, a translucent highlight for each.
Words lying over one another are now one run over the box around them
(`document::words`, for both DjVu engines and a plugin from outside
Skwir's pages): on a 71-page specification, 603,592 pairs of characters
covered each other, none now.

The arrows move what a preview shows once it has the keyboard, as a
document reader's do (`ui.preview_scroll`): a document's pages, a
picture, Markdown, a book's chapter, a table. Up and Down go a tenth of
the view; read a page at a time, a page turns at its edge, as under the
wheel, and a held key waits for the page it turned to. Left and Right go
sideways where what is shown is wider than the panel, else to the page
or section before or after, as they did on the whole screen, where a
picture zoomed past the screen now moves before the walk goes on. In the
panel a picture's Left and Right only move it: a step to the next
picture would give the keys back to the pane. What had arrows keeps them
(an archive's files, a map, a player, a text's rows, which also go
sideways now). Page Up, Page Down, Ctrl+Home and Ctrl+End, a text's
alone before, scroll every structured preview.

Ctrl and the wheel zoom a picture or a document's pages around the
pointer, as image and document viewers do: what is under it stays there
as far as the picture or the page can scroll. A picture and a page read
a page at a time are laid out by known rules (`preview::Lay`), so the
offset is worked out on the wheel's turn; read continuously, the page
under the pointer and the share of it there are landed by the layout at
the new zoom, which knows the rows' heights. The zoom keys keep their
place, as before.

Find in a preview marks what it finds, as a browser does: while the
field is open, every match in the text drawn, and the one it is on more
strongly (`inspector_text::Sought`, `Theme::find_*`), in a text file,
Markdown or a book. It goes from match to match, within a line too
(`text_rows::find` from a row and a byte, `Display::find` from a block
and a byte), and brings the match into view where it was drawn
(`RowScroll::reveal_spot`): along a line that does not wrap, down one
that wraps taller than the view, out from under the find field where the
view can move. In a file of one long line, each 4 KiB row it is cut
into held matches Find stepped over, and the row selected was all it
showed. Lines that do not wrap could not be scrolled past about half
their length: a row's text was shifted by a negative margin, which laid
the row out shorter by as much, so how far the rows could go shrank as
they went; the text is moved by position now (`CodeRow::shift`).

Copy Path copies the selected paths as text, one a line, from the
menu's top level, Ctrl+Shift+C or Ctrl+Alt+C (Dolphin's Copy Location
key), the folder's with nothing selected; Properties shows the whole
Path, copied by a click. Copying for a shell, quoted, stays in the
palette. The clipboard is the desktop's on Linux (ADR-062, gpui patch
0024): a copy or a cut puts the files there as both families of Linux
file managers read them (GNOME's list, MATE's, the URI list and KDE's
cut flag, the paths as text), and a paste with nothing of Skwir's reads
what another application put there, off the UI thread, as a drop from it
is. Tried on Xvfb with Thunar 4.20.7 and Dolphin 26.08 both ways (a cut
moves in each; Thunar cannot paste Dolphin's files nor Dolphin move
Thunar's cut, which Skwir does) and on a headless sway with
wl-clipboard. macOS and Windows keep Skwir's own clipboard (not yet).

## 2026-10-17

**gpui from Zed's main branch (ADR-064).** gpui 0.2.2 drew with Blade
and had no AccessKit; its release is still the latest. Skwir now takes
gpui from a commit of Zed's main branch: wgpu on Linux, Direct3D 11 on
Windows, Metal on macOS, and AccessKit in the window. It is vendored
with Zed's pins, git ones included (`deny.toml` names each), and the
patches Skwir still needs.

- **The patches.** Twelve of the 24 were ported. Ten are gone, because
  Zed's main branch has its own answer. 0014 returns as 0029
  (2026-10-18): Vulkan starts on a thread while the platform sets up.
- **Five patches are new:**
  - 0001 now carries the drag's icon;
  - 0025 takes Vulkan alone where it has a GPU;
  - 0026 hides the pointer with the fullscreen controls;
  - 0027 lets Windows be checked from Linux;
  - 0028 marks gpui unpublished.
- **Tooling.** `tools/gpui/vendor.py` makes the vendored tree from a Zed
  checkout and writes the patches back, and `just gpui-check` runs their
  tests.
- **What it prepares:**
  - accessibility: roles and labels on Skwir's elements (started
    2026-10-18, ADR-065);
  - a 3D model preview drawn by the GPU on every system.

## 2026-10-18

**What a screen reader hears (ADR-065).** The first step of
accessibility, Phase 8's audit made possible by ADR-064:

- The window is named by the active pane's location ("Documents —
  skwir").
- The file views are list boxes (the tree view a tree), named by their
  location and counting their entries. The pane keeps the keyboard focus;
  the entry under the cursor is its active descendant, so a screen reader
  says it as the keys move: its name, that it is selected, "3 of 120". An
  empty or failed pane says its placeholder.
- Nothing is built while no screen reader listens.
- `just a11y-check` reads it back as Orca would, over AT-SPI, on a
  display and session bus of its own (`tools/test/a11y.py`).
- Gaps of AccessKit 0.24 on Linux: a tree item's level and whether it is
  expanded are not passed (`docs/invariants/accessibility.md`).

The places around the files, the same day:

- The sidebar is a tree of groups and places; the trail links to its
  places.
- Text fields carry their text, caret and selection; a secret says
  bullets.
- The palette, Jump, Choose and the tag editor: the option the keys
  reach is said as focused while the field keeps the keyboard (vendored
  gpui patch 0030).
- Dialogs are modal and named, a confirmation an alert; menus, toolbars,
  popups, tabs and the settings say each control and its state.
- Toasts are announced as they appear, a failure at once (patch 0031:
  live regions and the states gpui's builder lacked).
- `just a11y-check` follows it all, 29 steps.

Found while listening: trashing the file under the cursor left no
cursor, and Down started again from the top, for the eye as for the ear.
The cursor now goes to the next entry, selected, as in Dolphin and
Nautilus, so Delete pressed again takes the next file; a folder closed in
place takes the cursor that was inside it (`docs/invariants/views.md`).

The path in the pane's row read from the folder shown back to the first,
as the row lays it out from the right; it now reads as every file
manager's and the web's breadcrumbs do, the first folder first, a "Path"
landmark. Only the tree's order changed, rebuilt only while a screen
reader listens.

**The previews and the terminal.** What is previewed is named once and
said wherever it shows: the panel a landmark, Peek and the whole screen
dialogs named by the file (Peek follows the arrows), a picture with its
size, a document with its page, text and Markdown as documents of their
lines, an archive as a tree of its members, the player's seek bar a
slider of seconds. The terminal reads as text, the cursor as the caret;
the footer is a status line read when asked. `just a11y-check`: 39 steps.
Not yet: a document's text in the reader, the map, the hex view's rows.

**F6 goes round the window** (the user's choice, as in other
applications): the sidebar, the files, the preview, a part not shown
passed; until now F6 went to the sidebar and back, and the preview's
content was reached with the pointer alone. The terminal keeps F6 for
its programs; F4 goes in and out of it.

**What was missing:** a document in the reader carries the text of the
page shown, a line a run; a map is a figure saying what it holds and its
keys, the Map view's what is placed; the hex view's rows are lines.
Checked by hand on a synthetic PDF, GPX and binary file (a PDF needs a
reader engine `a11y-check` cannot count on). Then a map's places one by
one: PageDown and PageUp go from place to place, the map centred on each
and the place ringed, read as a list and said as each is reached; in the
Map view the place's file is selected, so Enter opens it.
- Not yet: the previews, the terminal, search pills and the footer.

## 2026-10-19

**A key held down runs again only a step (ADR-066).** Space held made
peek flash open and closed at the repeat rate: gpui runs a binding for
every repeat, and Space both opened and closed peek. Every switch on a
key did the same.

- A command says when a key held down runs it again, by its arguments
  (`CommandSpec::repeats`): the cursor, lists and menus, pages,
  scrolling, seeking, the volume, zoom, the map, tabs by steps, Back,
  Forward and Up. Everything else runs once a press: switches, Enter,
  Delete, Undo.
- Space tapped opens peek as before; held until the system repeats it,
  peek shows until Space is let go, unless it was used meanwhile (a
  click, a key other than the arrows' walk). The system's repeat delay
  decides, not a constant: QuickLook for Windows does the same with a
  fixed 750 ms.
- Other file managers, measured and read: GNOME Files with Sushi flashes
  on X11 (once every 0.6 s) and opens once on Wayland, by accident of
  focus; QuickLook for Windows and Files ignore repeats on purpose.
- Vendored gpui patch 0032: an action sees the key event it came from;
  X11 reports a held key, which it never did.
- Checked on Wayland and X11 (Xwayland) on a headless sway; Windows
  compiled, macOS's window not compiled.

**3D models turned in the window (ADR-067).** A mesh's preview was one
still the helper drew on the CPU.

- Vendored gpui patch 0033: `Window::paint_mesh` draws a triangle mesh
  in 3D. wgpu draws it offscreen, multisampled, with depth, faces shaded
  flat from their screen-space derivatives, then composites it in draw
  order. Uploads run on a thread of their own, never the window's. Metal
  and Direct3D 11 drew nothing at first (see below).
- `skwir-model-worker --mesh` writes the mesh itself; Skwir checks every
  count, index and coordinate before the GPU sees it. The preview asks
  for it only where the window draws meshes on a GPU: lavapipe,
  thumbnails and agents get the still (macOS and Windows did too, until
  their passes below).
- The view opens as the still is drawn; a drag turns it, Shift or the
  middle button moves it, the wheel zooms; the arrows, Shift and the
  arrows, the zoom keys and Ctrl+0 do the same (`ModelPreview`).
- Measured (release, 1000 × 800, four samples a pixel; tori of 225,600,
  a million and four million triangles):
  - a frame takes 0.7, 1.1 and 2.5 ms on an NVIDIA GTX 1060; 2.2, 2.6
    and 5.0 ms on an Intel Iris Xe; 84 ms, 0.56 s and 2.6 s on lavapipe
    (the reason it gets the still);
  - uploading them takes 1, 6 and 31 ms on the Intel GPU, but 50 ms,
    0.23 s and 0.94 s on NVIDIA, whose driver maps upload memory
    uncached (158 MB/s): the reason uploads left the window's thread;
  - in the window, on the Intel GPU, a model shows 0.19, 0.49 and 2.0 s
    after the click, most of it the helper reading the file (0.07, 0.37
    and 1.47 s), and costs the window its copy of the mesh (5, 19 and
    72 MB). Turning the largest drew a frame per pointer move, 4.7 ms
    each (median), at 23% of a core; at rest, nothing.
- The helper then read faster: four million triangles in 0.48 s instead
  of 2.26 s (a binary STL read from its bytes, its corners made one on
  every core through a cheap hash, the closed check grouped by thread).
- Then the user's choices: the bar's 3D button switches between the model
  turned and its picture, remembered by type (a slicer's 3MF shows its
  stored picture until chosen); a mesh over four million triangles shows
  its picture and 3D turns that file alone. Perspective and Smooth shading
  are settings with bar buttons; gpui makes the normals on its upload
  thread only once smooth shading asks, drawing flat meanwhile.
- And a model's own colours: PLY's and OBJ's point colours, glTF's
  materials and `COLOR_0`, 3MF's base materials and colour groups, drawn
  by the window, the still and the thumbnails alike; not textures.
- Checked on Wayland and on X11 (Xwayland), a headless sway rendering on
  the Intel GPU; on lavapipe, the still. Not checked: the window on
  NVIDIA (the user's session), a screen reader.
- Then Windows and macOS: Direct3D 11 and Metal draw the mesh as wgpu
  does, from naga's translations of its WGSL (a test keeps them current),
  compiled when the first mesh is asked for; the frame's geometry, the
  shaders' parameters and the upload thread are gpui's, one for the three.
  Direct3D's pass ran under Wine on Mesa (`just gpui-check-windows`),
  which found the upload thread outliving its renderer (Wine killed the
  process at exit): it now ends with it. Through gpui's renderer it is for
  a Windows machine, Wine compiling none of gpui's own shaders; Metal is
  compiled only. A mesh the GPU will not hold says so instead of loading
  for good.

**Full preview (ADR-068).** A file past a bound that keeps a preview
quick was shown in part, as its text, or not at all, with no way to see
the rest.

- Each such bound has a larger one for Full preview, read through
  `limits::Bounds`: text 2 → 32 MiB, a file read whole 16 → 128 MiB,
  blocks 2,000 → 100,000, code lines 10,000 → 100,000, archive members
  4,000 → 64,000, table columns 64 → 256, pictures 64 → 128 megapixels,
  DXF and circuit drawings 32 → 128 MiB. Bounds against hostile input
  stay.
- The bar's Full preview asks for them, for the file shown, unremembered;
  it is offered exactly where it shows more (`whole_available`,
  `inspection::TooLarge`). No setting (the user's question, answered in
  the ADR).
- A DXF too large to draw now says so; an archive member is found in any
  listing kept of it; `extract` takes the member's view.
- Checked: providers' tests past each quick bound and the larger one;
  in a window on Linux, a long changelog, an HTML page, a wide CSV, JSON,
  a 67-megapixel picture and two archives, each whole and then quick
  again, their memory given back (the ADR lists what was not).

**STEP, IGES and Rhino models in 3D (ADR-069).** Nothing drew a STEP or
IGES file without f3d, and nixpkgs' f3d claims a type the database does
not know; a Rhino model showed only its stored picture.

- Engines scored on step-conformance (27 CC0 parts at four chords):
  OpenCascade the reference, ogeom 89 of 108 runs (every real part),
  Foxtrot 68 (faces of revolution dropped without a word), truck 91 on
  its unreleased branch. ogeom chosen: pure Rust, in the model helper,
  which grows from 1.5 to 3.6 MB stripped (0.6 to 1.3 MB compressed).
- Each part placed by the file's assembly, each face meshed to a
  thousandth of the model's diagonal, in its colours and the file's unit;
  faces keep their points, so edges stay sharp under smooth shading, and
  watertight and volume are measured on ogeom's welded mesh. Faces that
  will not mesh are counted and said. Properties adds the protocol, the
  program and the faces. 16 MiB quick, 128 MiB with Full preview.
- Checked: helper and plugin tests; NIST's 28 public-domain STEP files
  from several CAD programs, 27 read with no face missing in 0.04 to
  0.8 s (the 28th holds triangles only). Windows and macOS compiled.
- Rhino: 3D turns the meshes the file keeps (mesh objects, B-reps' and
  extrusions' render meshes), placed by block instances, on shown layers,
  in their materials' colours (layers' are black by default); read a chunk
  at a time from openNURBS 8's layout, nothing else parsed. Objects that
  store none (Save Small, SubD) are counted and said. On openNURBS's and
  rhino3dm's 168 example files from Rhino 1 to 8: none failed to read,
  48 drawn, the others storing no mesh shown or written by Rhino 1, each
  refusal matching an independent reader; the largest (10 MB,
  519,498 triangles) in 0.36 s and 65 MB, picture included.
- IGES, the same day, through ogeom's own reader: the global section
  gives the program and the version. Twelve SolidWorks exports read closed,
  no face missing, in 0.02 to 0.18 s; files whose trimmed surfaces sit in
  a group (two old EUCLID files) are refused by ogeom, left to it.

**Address completion (ADR-070).** A path typed in the address offered
only "Go to …"; the user asked for what Dolphin's location bar offers, or
better.

- The folder typed is read once on a worker (a pane's listing when one
  shows it), and its names matched in memory at each key: case ignored
  unless a capital is typed, whole name, start, a word's start, anywhere;
  folders then files, ranked by Jump's new `jump.scores` (zoxide within a
  folder, recent files); hidden names for a dot. Eight rows; what the
  first adds drawn muted after the text, and Enter goes there.
- Tab: the part the names share, else the row highlighted, a folder then
  listed; words leaving one name shown become its path. Right or End
  takes the rest drawn. `./` and `../` from the folder shown. A folder
  part naming nothing is abbreviated, fish-like (`~/w/sk/`). A place's
  text completes through `LocationProvider::complete` (tags answers);
  words starting a scheme offer its place.
- Enter on a file shows it selected in its folder (hidden files then
  shown, and said); Shift+Enter opens it. The rows are a list box heard
  by a screen reader.
- Checked: unit tests (matching, ranking, sharing, the rest drawn, paths
  resolved, a tag's text split); in a window on Xvfb (debug build) with a
  synthetic home: `~/Do` and Tab, a file revealed, a hidden one shown,
  `~/w/sk/` abbreviated and taken, Ctrl+Z, words to a path, `tr`, a tag
  completed, `../sk` from inside, Down then Right, zoxide's ranking, and
  `bench-data/flat100k` (100,000 names) typed into. Matching those names
  takes 1.8 to 4.4 ms a key in release, 6 to 65 ms in a debug build.
  Over AT-SPI (`just a11y-check`): the rows a list box, the row the
  arrows reach said as focused, the typed text and caret still read.
  Not checked: Shift+Enter in the window (it would open an application
  on the test display), a slow disk's "Reading…" line, a screen reader
  itself (Orca), Windows and macOS (compiled only).

**The next picture is read ahead in the panel and in peek too.** The
whole screen read the next picture ahead; the panel decoded each photo
when it was reached, about a quarter of a second each.

- A walk is two steps alike through the pane's order (`preview::Walk`):
  Down twice, or a grid's row down twice. Its next entry is decoded once
  the one shown has arrived, as the whole screen's is, and taken in the
  sync that makes the step. A click starts no walk; a step another way
  lets what was read go, as do the panel and peek closing and the pane
  showing another location (`location_lifetime`).
- A picture read ahead decodes in a turn of its own (`preview::Turn`):
  a wrong guess never keeps the step waiting, and at most two pictures
  decode at once.
- The panel decodes a JPEG twice its side and more reduced
  (`picture::decode_fitted`, as thumbnails do). Measured outside the app
  on synthetic photos, the least CPU time of seven runs on a loaded
  machine: 24 megapixels, 320–340 ms decoded whole and shrunk against
  225–255 ms reduced, peak +87 against +52 MB. A 12-megapixel phone
  photo (4032 px, half of it under 2048) gains nothing.
- `reading.read_ahead` (the Pictures settings, on) turns it off, the
  whole screen's too: one more picture held while walking, 2048 × 1365 × 4
  bytes.
- Checked: unit tests (a walk's steps, a large photo's side and size); a
  release build on Xvfb, 16 synthetic 24-megapixel photos, a timing line
  added for the check only, from the step's sync to the picture: the
  first two steps 251–262 ms, every step of the walk after 0.3–0.7 ms
  (the picture in the step's frame); a step back 255 ms twice, then read
  ahead the other way; with the setting off every step 244–319 ms. Held
  Down: the first repeats read ahead, then none while the steps outran
  the decodes. Peek, the grid (a row down) and the whole screen (Left
  turns it back; entering it reuses the panel's picture, not decoded
  again) read ahead too. A walk broken freed 10.8 MB (`Pss_Anon`); idle
  CPU 0.3 % with a picture held. Not checked: a HiDPI panel asking a
  sharper picture after a step, a slow disk, Windows and macOS
  (compiled only).

**Settings by subject, the few first (ADR-072).** The user asked to
group related settings (tags and notes) and for a short view by
default, the current one kept as Advanced. The dialog had 71 settings on
17 pages, a page per namespace: tags on four of them, two shells on two,
the engines away from Reading, four pages of one setting.

- A setting names the page of its subject (`SettingSpec::page`); the key
  stays its owner's. Ten pages of settings, then Plugins, Keyboard and
  Changed by you: eleven entries in the sidebar instead of seventeen.
- A page shows its essentials (`SettingSpec::essential`: 18 lines with
  every plugin on, a test holding them to four a page and twenty in all)
  and folds the rest under More settings (N), which stays open once
  opened (window state). The user chose the fold over a global
  Simple/Advanced switch, which would hide what links, toasts and the
  guide name. The search finds every setting; what differs from its
  default, a linked setting and a plugin's settings reached from its
  button show whether folded or not, until the page is left.
- Changed by you lists settings and plugins differing from how skwir
  comes, with Restore all. Sections made of the same settings are one
  table: the seven stores of notes are seven lines of Read and Write. A
  row says its description's first sentence (at most 30 words, tested;
  three rewritten), the whole as its tooltip and its screen reader
  description.
- Checked: the settings, palette, plugin and notes tests (82), new ones
  for the page order, tables, first sentences, subject pages and the
  budget; clippy -D warnings; in a window (debug build on Xvfb, a
  synthetic home, three values changed): every page folded and
  unfolded, the fold remembered in `state.toml`, a revert on Changed by
  you moving nothing, the search reaching folded settings, a plugin's
  Settings button revealing its settings on a folded page, a tooltip,
  the stores' table (titles wrapping, dropdowns aligned after a first
  look showed otherwise). Not checked: a screen reader on the new
  controls (`just a11y-check` not run), Windows and macOS (not
  compiled: `check-cross` not run), release-check.
- Not done: a menu item for the terminal's Follow the folder shown (the
  shell would name a plugin's key); the 3D model's bar already had its
  two switches.

## Release 0.1's steps (2026-10-07 to 10-17)

The steps of `docs/PLAN.md`'s Release 0.1, as each was done.

1. **Privacy of the repository.** Scanned 2026-10-07: every blob and
   commit message (4,482 blobs, 324 commits) for this machine's
   identifiers, addresses, home paths, keys, positions and phone
   numbers. Every hit was synthetic or public but two, fixed in the
   tree: a drive's label in a comment (`address.rs`) and a home folder
   in `vendor/patches/0001`'s `diff` lines (the patch recipe now drops
   them). Both stay in history until the one rewrite the user chose
   (2026-10-07), just before the first push, which also gives every
   commit the author address chosen for the project (2026-10-10). Done
   2026-10-11: 469 commits rewritten (the address, the two strings), the
   scan again clean, then the first push. The commits made since are
   searched before each push (2026-10-12: nine, their diffs and messages,
   for home paths, mounts and addresses; none. 2026-10-13: 27, for home
   paths and the old address; none, every one by the project's address).
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
   unknown (the session included); a renamed key can follow
   (`Settings::rename`, called by none while no one uses skwir). Fixed: a
   settings file that is not TOML or not
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
     | 2026-10-12 | `just idle-pss`, the session's display, after the settings follow their files and links ask | own share 95.1 MiB (median of 3; GPU buffers 19.6 of it); first frame 131–143 ms, median 140.0; idle CPU 0.4–0.6 % |

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
   installed, in the distributions' roots on 2026-10-10; since
   2026-10-17 made once and kept in `tools/package/notices`, checked by
   `notices-check` in `release-check`, ADR-063, after an Arch build of
   0.1.0 in a clean environment stopped at cargo-about, whose `cargo
   metadata` reads every platform's crates); `deny.toml` (licences,
   RustSec advisories, crates.io; `just deny`, a CI step).
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
    Dependency checks pass. A first gate that day missed one budget under
    load (`images10k.stream_complete_us`, 12,903 µs of 12,000) and stopped
    at a lint in work then uncommitted; the full gate passed later the
    same day (2026-10-12, all 44 budgets). No package has been published
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
    **Ubuntu's own packages** (2026-10-12): built in an Ubuntu 24.04 root
    by `try.sh ubuntu`, their revision naming the release
    (`skwir_0.1.0-1ubuntu24.04_amd64.deb`, apart from Debian's in a
    bundle), depending on Ubuntu's library packages; `skwir` installed
    alone, then the others, every helper answering `--probe`. Ubuntu's
    fontconfig gives a folder to the `staff` group, which a one-user
    namespace lacks: dpkg now runs under fakeroot in the Debian and Ubuntu
    roots (`linux/rootless-apt.sh`), as the AppImage's do. `release-build`
    builds them with the others; the manual `packages` workflow too.
    **The AUR** (2026-10-12): `skwir`, `skwir-mermaid` and `skwir-mupdf`
    are free names there; the PKGBUILD names its maintainer and
    `.SRCINFO` is current. Publication follows the pushed tag
    (`aur-prepare`, `aur-publish`), with the maintainer's AUR SSH key.
    **Before tagging** (2026-10-12, this tree): `just release-check`
    passes (lint with the Windows and macOS cross checks, deny, 1,514
    tests, smoke, both readers, all 44 budgets, the release tools; their
    19 tests in the current Nix shell, real makepkg included); every
    package built and checked as `release-build` makes them: Debian,
    Ubuntu, Fedora and Arch in their roots (`skwir` alone, then the
    others), the AppImage in its runtime-only root (both readers,
    playback, the window's commands, the sandboxed archive helper); the
    release metadata dated 2026-10-12 and agreeing (`release-metadata`),
    dated 2026-10-13 the next day.
    **The tarball** (2026-10-13, ADR-052 decision 7):
    `skwir-0.1.0-linux-x86_64.tar.gz`, the AppImage's `usr` tree under
    one folder with the program linked at its top, for a system without
    FUSE or a folder of the user's choosing. `package-appimage` builds
    and checks it with the AppImage; `release-build` expects it and its
    checksum (15 files in a bundle); the manual `packages` workflow checks
    it too. It passed the AppImage's checks in the Ubuntu 22.04, 24.04
    and Fedora 44 runtime roots, and the README's commands installed it
    as written; the release tools' 20 tests pass in the Nix shell.
    **Released** (2026-10-13): `v0.1.0`, an annotated tag on 34a7b9b
    after the review before tagging
    ([2026-10-13](#2026-10-13)). `release-build` passed (the full gate,
    then every package built and checked in its root, 25 files in the
    bundle) and `release-verify` agreed; `main` and the tag were pushed,
    `release-upload` made the draft (GitHub's SHA-256 of each of the 25
    assets equal to the bundle's), and the user published it at 23:55.
    The AUR's `skwir` base (`skwir`, `skwir-mermaid`, `skwir-mupdf`) is at
    0.1.0-1. The notes' links are relative in the changelog; they were
    made absolute in the draft by hand. Since then `release-upload` gives
    the page each relative link as its file at the tag, the changelog and
    the bundle's `notes.md` keeping them relative, and `release-metadata`
    refuses a link to no file of the source.
    **The AUR after 0.1.0** (2026-10-17): 0.1.0-2 fetches every
    platform's crates in `prepare()`, which 0.1.0's cargo-about needed
    (ADR-063). `skwir-bin` and `skwir-git` are package bases of their own
    (ADR-040 decision 7), and `aur-prepare` and `aur-publish` handle all
    three. `skwir-bin` 0.1.0-1, made from the published tarball, was
    installed by pacman in a fresh Arch root and passed 0.1.0's own
    portable check there (573 ELFs, every helper's probe, the three PDF
    engines, playback, the window's commands through `/usr/bin/skwir`,
    the sandboxed archive helper); it needed `fontconfig`, whose
    configuration the tarball's library reads. That library, Ubuntu
    22.04's, warns on every start about constants in Arch's newer
    configuration files, as the AppImage would on any newer system.
    `skwir-bin` 0.1.0-1 is on the AUR. `skwir-git`, built in the Arch root
    from this repository's `main` (0.1.0.r51.g17cf86d, an empty cargo
    home, 41 min on 8 jobs under load), passed `linux/check.sh` in a
    fresh root as `skwir` does: alone, then with its `-git` twins and the
    optional libraries, all 11 programs working and its licences in
    `skwir`'s folders. It was published once GitHub's `main` held the
    notices (ADR-063), at that version.

## Colors and font from the desktop: A, B and the fonts by role (2026-10-19)

ADR-071 and ADR-073, the steps of `docs/PLAN.md`'s "Colors and font from
the desktop" as each was built.

- **A. One derivation, guarded.** `theme.rs` derives every token from a
  few base colors; Skwir's light and dark palettes become that
  derivation over its own. The contrast guard (4.5:1 for text, 3:1 for
  the accent; 7:1 and 4.5:1 when more contrast is asked) moves a
  foreground's lightness until it passes. Tested: generated palettes
  pass after the guard; Skwir's pass, fixing the three pairs that fail
  today (light `text_muted` on `surface`, light `warning` on `bg`, dark
  `on_fill` on `accent`). Screenshots of both looks before and after.
  **Built 2026-10-19:** `color.rs` (Oklab, WCAG contrast, `readable`,
  `readable_on`), `Base`, `Steps`, `Kept` and `Theme::derive` in
  `theme.rs`; `on_fill` split into `on_accent` (guarded) and
  `block_text` (the Usage map's, kept). Skwir's looks are derived once
  (`OWN`). Tests: both looks pass in both contrasts; 400 generated
  schemes pass wherever black or white could; the steps give the
  hand-picked colors again within two units of a channel (the dark
  selection within five). Screenshots with `tools/screenshots/readme.py`,
  the commit before against this one, debug builds on Xvfb: the light
  look's muted text and the icons it tints went from #707070 to #6c6c6c,
  the selection from #d6e4ff to #d5e4ff (dark: #2c4f8a to #28508a),
  nothing else moved; in the dark look, the tags editor's tick is dark on
  the light accent (6.8:1, was white at 2.42:1). Not checked in a
  window: the warning's color in Settings, the drag's count, link
  labels; `Contrast::More` is tested but nothing asks for it until B.
- **B. The portal on Linux.** Accent, contrast and, on a desktop whose
  own settings do not name it, the font, read on a worker through
  ashpd, followed through `SettingChanged`; the last answers kept in the
  cache for the first frame; the `ui.desktop_style` switch. Checked on
  the session and on headless sway with a portal answering; `just
  idle-pss` and the first frame measured before and after.
  **Built 2026-10-19**, with deviations recorded in ADR-071's amendment:
  zbus's blocking API on a thread of its own rather than ashpd; two
  switches, `ui.desktop_colors` and `ui.desktop_font` under it (the
  user's choice; `SettingSpec::under`), the font chosen when the window
  opens. `skwir_platform::desktop_style` reads and follows,
  `desktop_settings` is shared with the icon theme,
  `skwir_ui::desktop_style` keeps what was said (the cache) and lends it
  to `Theme::of`. Tests: a portal of the tests' own over a socket pair
  (`ReadOne`, `Read` before version 2, an accent out of range, no bus, a
  change followed and one that changes nothing ignored); Pango's and
  Qt's font names; the switches. In windows (debug build, Xvfb): with a
  fake portal on a private bus, its teal on the selection, then orange
  and the stricter contrast (muted text 108 to 79) when it said so, in
  the same process; a portal taking 5 s per answer left the first frame
  at 307 ms (317 without) and its accent came once it answered; on the
  session's bus, its accent and its font (Noto Sans) from the first
  start. Release builds on headless sway (pixman, lavapipe), HEAD and
  this alternated, three runs each: own share 110.6–110.7 MiB against
  110.9–111.3 (about +0.5, zbus's code now touched at start), first
  frame 78 ms against 77, idle CPU 0. The session's own NVIDIA reading
  (92.5 MiB) was not taken again. Not checked: Xfce, Cinnamon, MATE and
  Plasma reading the font (their tests read the files and programs
  alone), the dialog's switch under the other in a window.
- **Fonts by role (ADR-073), built 2026-10-19.** The user asked to choose
  the font "among those on the system when relevant (text previews,
  EPUB/HTML rendering, etc.)", then for the interface font too, amending
  ADR-071's switch. Three choices, Automatic or a family:
  `ui.font` (Appearance), `reading.font` (Reading › Text, essential),
  `ui.code_font` (Reading › Code); `ui.desktop_font`, never released, is
  gone. Automatic is the desktop's interface, document and monospace font
  while Desktop colors is on (`DesktopStyle` reads the two new ones where
  it read the first, and `fc-match` resolves an alias), else Skwir's.
  The fonts are tokens of the theme (`fonts.rs`, `Theme::font`,
  `reading_font`, `code_font`), resolved with it: `Shell::font`,
  `Shell::mono_font` and the forty copies of them are gone, tooltips read
  the theme, so a change shows at once (ADR-071's "at the next start"
  with it). Reading mode writes Markdown, HTML, documents, books and mail
  in the reading font (`MarkdownStyle::font`); the code view, code
  blocks, bytes, the terminal, bulk rename and reviews in the code font.
  The families are the text system's that write Latin
  (`skwir_platform::fonts`: one `fc-list`, about 15 ms in a process of
  its own), the code font's the monospaced ones, listed on a worker at
  start and when the settings open, so `settings.apply` takes any of them
  from a script. A dropdown of more than 12 choices, and a font's
  however few (the code font's monospaced families were under the
  count, so its list had no field nor specimen), opens with a field
  that narrows it, its rows drawn as they scroll and fewer where the
  window leaves less room under the row, the arrows and the
  pointer moving it, a font's specimen written in the one highlighted
  (`settings::Narrowed`, `ui.list_move` let through the dialog). Tests:
  the fallbacks, the portal's three fonts read and followed, fontconfig's
  listing and aliases, the settings' choices, pages and essentials. In a
  window (debug, Xvfb, the session's portal): see ADR-073's
  consequences. Idle memory on headless sway, release builds alternated
  seven times: own share 109.1 MiB against HEAD's 109.9, first frame
  78.0 ms against 76.2 (medians, within the spread). Not checked:
  Plasma, Xfce, Cinnamon and MATE reading their fonts, a screen reader
  on the long list, the session's own display.

---

## 2026-10-20

**The plan and this log.** `docs/PLAN.md` had grown to 2,600 lines, half
of them the day-by-day record now above (from "Built beyond the plan, by
area"); it keeps the phases, their status and a "What is left" gathered
from these notes (1,248 lines). New entries go here.

**plocate's answer dated** (ADR-050's amendment, step C of "Names indexed
by skwird"). A search of `/` that plocate answers skipped the walk without
saying so: a file made or renamed since the last `updatedb`, usually a
day, was not found, and nothing told. Now a listing keeps what its
answer rests on (`location::AsOf`, sent by each run of a search as
`ListingEvent::AsOf`, `None` for a walk); plocate's time is the oldest
modification time of the databases it reads (its help's default, then
`LOCATE_PATH`'s). The footer adds "as of Yesterday 03:00", its tooltip
what answered and what that leaves out; `entries` answers `as_of`.
- Tests: the help's default (`the_default_database_is_the_one_the_help_names`),
  the oldest database (`an_answer_is_as_of_its_oldest_database`), the
  listing keeping it and a walk taking it back
  (`what_an_answer_rests_on_is_kept_until_a_run_says_otherwise`), a search
  through the app with a stand-in plocate on `PATH`, its listing and its
  `entries`, then a search of a folder saying nothing
  (`a_search_plocate_answers_is_as_of_its_database`), the footer's words
  (`an_index_s_answer_says_as_of_when_and_what_it_leaves_out`).
- plocate 1.1.25 (nixpkgs), with a database its `updatedb` made of a
  synthetic tree: its help writes `(default is /var/cache/locatedb)` on
  standard output, it reads `LOCATE_PATH`'s databases after its default,
  and a missing default fails it (exit 1, a message), so the search walks.
- In a window (debug build, Xvfb, a synthetic home, a stand-in plocate
  whose database was dated the day before at 03:00): `skwir cmd
  query.search` over `/` showed "1 result · as of Yesterday 03:00" with
  the tooltip "Found in plocate's database, last brought up to date
  2026-10-19 03:00:00: …", and `entries` answered `as_of` with that time;
  F5 kept it; a search of a folder showed "2 results" alone. Not checked:
  Orca reading the status line, a real system database.

**Plasma's scheme and GTK3 themes** (ADR-071 steps 3 and 4, its amendment
of 2026-10-20; built by a second agent in the same tree). Tests on
synthetic fixtures (references, GTK's five functions, imports within the
theme and out of it, a cycle, a compiled theme, a dark variant,
`kdeglobals` replaced and rewritten the same, the portal naming the
theme); the contrast guard on six real schemes as read, in both
contrasts: Breeze Light and Dark, adw-gtk3 and its dark, Materia and its
dark. Arc (20221218) names none. In windows (debug, Xvfb, no portal): a
synthetic `kdeglobals` worn, replaced by a dark one and followed live, its
`AccentColor` on a switch; `GTK_THEME=Materia` and `adw-gtk3-dark` worn,
pixels matching the colors read. Release builds on headless sway (pixman,
lavapipe, the session's portal naming adw-gtk3), HEAD c2c51365 and this
alternated three times: own share 107.5–107.7 MiB against 107.3–107.6,
first frame 77.8–89.0 ms against 76.9–80.3 (one later run at load 15 read
102–399 ms). Not checked: Plasma, Xfce, Cinnamon or MATE sessions, the
session's own display.

**Contents that follow the reader** (the user's request and design). A
book's contents closed whenever its next chapter came: a new section reset
the inspection's state, the drawer's flag with it (a PDF's survived page
turns, its reader's state was kept). Now the flag is kept from section to
section; a new file closes the contents unless pinned. The pin, at the
contents' top (`ui.contents_pin`, `reading.contents_pinned`, remembered
as Full width is), makes them a column beside the content, which narrows,
open until closed; unpinned they stay a drawer that choosing closes, as
before. The part shown is highlighted and kept in view: a document's by
the page its reader counts, a book's by the section shown
(`Row::section`, which the EPUB plugin sets from the spine as it does for
printed pages) and where its fragments are, else the heading at the top
of the view. A book's rows are indented by their depth (all were at 0).
- Tests: `the_contents_stay_open_from_chapter_to_chapter`,
  `the_part_shown_is_the_chapter_then_the_heading_at_the_top` (fragments
  placed and not, a section the contents do not name, none),
  `the_part_shown_is_the_last_entry_at_or_before_the_page`, the EPUB's
  rows knowing their chapters (an escaped name included).
- In a window (debug build, Xvfb, a synthetic EPUB with a cover, a
  chapter of two parts and two more, and a synthetic 8-page PDF with a
  nested outline): the next chapter kept the drawer open, its row
  highlighted; scrolling past a part's anchor moved the highlight to it;
  choosing a chapter unpinned closed the drawer; the pin made a column,
  the text reflowing narrower, and was saved; choosing kept it open; the
  PDF, selected next, opened with its outline beside it, the highlight
  following the reader's page (Results on page 7; the reader counts the
  page at the middle of the view, so page 3 at the top read as 4, Runs);
  after a restart the contents opened pinned; unpinning made a drawer
  again and was saved. Not checked: a screen reader (the contents' rows
  have no role yet), the parked view of a container, a real book.

**Archives in Windows' and macOS's builds** (ADR-041's amendment; the
user's go-ahead after an exploration of what those builds lack). The
archive helper is in both packages, libarchive built on Linux and linked
into it statically (`tools/package/libarchive.sh`): nixpkgs' sources as
nixpkgs patches them (bzip2's and lz4's CVE fixes among them; the
`windows` and `macos` shells name them, `SKWIR_LIBARCHIVE_SOURCES`), with
liblzma, zstd and lz4, and for Windows zlib and bzip2, built by Zig;
for macOS through xmac's CMake toolchain, with the system's zlib, bzip2,
iconv and Expat. The helper's build script links what the folder's `link`
names (`SKWIR_LIBARCHIVE_STATIC`), else the system's libarchive as before.
- Build time: 146 s for Windows, 84 s for macOS, from nothing, kept in
  `~/.cache/skwir-build/libarchive` by target, sources and recipe. A first
  trial's configure took 927.6 s: the dev shell's environment (225 KB,
  `NIX_CFLAGS_COMPILE` 58 KB of it) makes each CMake check about 2.4 s
  instead of 0.18 s (30 checks: 73.7 s against 5.45 s with `env -i`; Zig
  itself compiles and links one in 0.33 s). The recipe runs CMake in a
  clean environment, which also keeps Linux's flags out of these builds.
- Windows: libarchive reads a name an archive does not say the charset of
  in the OEM code page, so a GNU tar's UTF-8 names came out as
  `caf├® ÔÇö` (Wine, French locale). The helper now opens archives by
  their UTF-16 path, sets `LC_CTYPE` (2 there, it was 0: `LC_ALL`) to
  `.UTF-8`, reads tar's and cpio's names as UTF-8 and writes a ZIP's in
  UTF-8 with its flag; a ZIP's or a RAR's unflagged names keep the OEM
  code page, as Windows' archivers write them. Linux and macOS read as
  before (macOS now asks for `UTF-8` after `C.UTF-8`).
- Under Wine (`just test-windows -p skwir-archive-worker -p
  skwir-plugin-archives`, the static libarchive named): 40 tests pass, the
  helper's damaged and encrypted ZIPs (ZipCrypto, AES through CNG) and
  the plugin's making, extracting, clashes and pauses; five skip their
  trash steps, which Wine's shell lacks (`under_wine`). Four tests needed
  porting: `HOME` (now `home_dir`), the helper's `.exe` name, a folder's
  time set through a handle allowed to write attributes with backup
  semantics, `/` between a tree's names.
- `just package-windows`: under Wine the check makes each format skwir
  makes (ZIP, 7z, tar and its gzip, bzip2, xz and zstd), of names with an
  apostrophe, accents, a dash and Japanese, in a folder named `archives
  d'été`, lists them back, finds the ZIP's names flagged UTF-8 (Python's
  zipfile), and lists a GNU tar's raw UTF-8 names right. The helper is
  2,105,856 bytes; the zip 40.2 MB (39.2 before), the installer 42.6 MB
  (41.4); it imports Windows' own DLLs alone (the UCRT, bcrypt, XmlLite,
  ole32).
- `just package-macos`: the helper is 1,906,576 bytes, arm64, for macOS
  11, linked to `/usr/lib`'s libz, libbz2, libiconv, libexpat and
  libSystem; the check refuses one linked to the system's libarchive. The
  system functions it calls are all in macOS 10.13 or earlier
  (`futimens`, `utimensat` the latest). Disk image 36.7 MB (35.9), zip
  38.1 MB (37.2).
- Not checked: anything on Windows or a Mac; RAR, CAB or ISO read there
  (the formats listed are those skwir makes and a GNU tar); a link
  extracted on Windows without the right to make one; lz4 (built, its
  config found, not exercised); Skwir's own preview tests under Wine
  (`crates/skwir/tests/extended_previews.rs`, not built for Windows: the
  disk was near full).

**A trimmed GStreamer for Windows and macOS, tried** (ADR-076, proposed;
the exploration's second step). GStreamer 1.28.8's own builds, each
checked against its published SHA-256: Windows' MinGW installer (1,014.7
MB, Inno Setup 6.7, which innoextract 1.9 cannot open: installed under
nixpkgs' WoW64 Wine 11.0 on an Xvfb display of its own, 2.5 GB with the
development files), macOS's universal framework (runtime and development
packages opened with xar, pbzx and cpio, 4 GB). Kept in the session's
scratch folder; nothing of it committed.
- The media helper, unchanged, built against each in about a minute
  (pkg-config pointed at their relocatable `.pc` files); macOS's loads
  GStreamer's libraries by `@rpath`.
- Under Wine (`tools/media/check.py`'s check, its process started by Wine
  with Windows' paths, audio to a clocked fakesink), the whole Windows
  set: VP9 and Opus, AV1 and Vorbis, H.264 and AAC (gst-libav) passed;
  30.0–30.3 frames a second delivered, 0–1 dropped, lateness p95 3.0–3.5
  ms, 25–38 % CPU; one H.264 run of four stalled where subtitles are
  turned off while paused.
- The trimmed set (ADR-076's, 38 plugins, their libraries followed through
  the imports, the plugin scanner), stripped by MinGW's strip (llvm-strip
  refused libstdc++: "invalid SymbolTableIndex"): with pango 90 files,
  49.7 MB, 16.7 MB zipped (-9), 12.1 MB xz; with DirectWrite 79 files,
  43.7 MB, 14.3 MB zipped, 10.3 MB xz. Unstripped 72.5 and 66.5 MB
  (libstdc++-6.dll 26.6 MB, 4.7 stripped). Under Wine: VP9 and Opus, AV1
  and Vorbis, VP9 and MP3 (mpg123) passed with pango, though turning
  subtitles off while paused stalled in 2 runs of 3 (never with
  `GST_DEBUG` on; 5 of 5 passed on Linux with Linux's helper); AAC and
  H.264 found no decoder (Wine's `d3d11` registers no element, its
  `mediafoundation` no decoder). DirectWrite's overlay has rank none, so
  playbin's subtitle bin never takes it; ranked primary
  (`GST_PLUGIN_FEATURE_RANK`) it stalled when subtitles were turned on.
- macOS, the same set for arm64 (thinned with llvm-lipo): 85 files, 46.9
  MB, 14.6 MB zipped, 10.1 MB xz; MoltenVK 7.4 MB of it, which
  `applemedia` loads through GStreamer's Vulkan library. Not run.
- Licences: GStreamer's tree holds each library's (`share/licenses`, 88
  folders) and `versions.txt`.
- Not checked: anything on Windows or a Mac, so neither Direct3D's,
  Media Foundation's, VideoToolbox's nor AudioToolbox's decoders; the
  helper's memory there; how the installer could run without Wine.
