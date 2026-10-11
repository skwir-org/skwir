# Status log

Dated notes from the top of `docs/PLAN.md` on 2026-09-18 and 09-19, kept
for their measurements. Newer entries go at the end; `docs/PLAN.md`
holds each phase's current status. Benchmark reruns and one-off memory
samples belong in commit messages.

Memory figures below are PSS of the whole process unless said
otherwise; they predate ADR-033's own-share budget and are
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
  reach GPUI without a copy. After 50 page turns: skwir with PDFium
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
  and corrupt PDFs. After 50 page turns, skwir and its worker: about
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
