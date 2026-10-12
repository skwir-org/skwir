# ADR-009: Optional content preview plugins

Status: accepted, 2026-09-19. Amended by ADR-012 (reading mode applies the
CSS that says how text reads) and ADR-016 (audio joins `preview-media`).
Archive extraction, a plan step, is ADR-048. Area rules:
`docs/invariants/previews.md`.

## Context

Beyond images, text, PDFs and video, users preview audio, raw bytes,
Markdown, archives, EPUB, tables, structured data, fonts, email and comics,
each a parser of untrusted input. Each preview must stay optional, keep GPUI
out of plugins, bound Skwir's work, and not embed a browser before its costs
are measured.

## Decision

### Plugins and the inspection result

- Audio, hex, Markdown, archive, EPUB, table, structured-data, font, email
  and comic previews are independently registered plugins on the owner-tagged
  preview registry, with its cancellation and invalidation. Disabling one
  cancels its inspections in flight.
- Format-specific providers outrank the hex fallback, even after
  disable/re-enable; an explicit presentation choice can ask hex for any
  regular file.
- A toolkit-independent **inspection** result sits beside bitmaps and the PDF
  and playback sessions: semantic text blocks with inline spans, rows with
  hierarchy and member keys, embedded images, source availability, section
  navigation. Providers run on scheduler workers.
- GPUI owns virtualized rendering, navigation commands, copy, search, font
  size and expansion; plugins never import GPUI. Not an HTML renderer in
  core, and not a replacement for the PDF and media sessions.

### Formats

- **Audio** reuses the GStreamer worker: bounded tag strings and 32 spectrum
  bins, at most one update per 50 ms of audio, drawn without a timer.
  Audio-only pipelines keep cover streams out of video playback; FFmpeg
  extracts covers, which stay once playback starts. No second engine, no
  whole-file waveform.
- **Reading mode.** Markdown: pulldown-cmark (MIT). Markdown, EPUB and email
  share an inert native HTML adapter on scraper/html5ever
  (ISC/MIT/Apache-2.0): headings, paragraphs, lists, emphasis, links, code,
  images. No scripts, publisher CSS, browser layout or remote fetches.
  (ADR-012: CSS that says how text reads is applied; layout, fonts and
  colours are not.)
- **EPUB**: rbook (Apache-2.0); its parser and ZIP dependency need Rust 1.88,
  which `preview-epub` declares.
- **Data.** CSV: csv (MIT/Unlicense). JSON: serde_json. TOML: toml_edit spans
  (MIT/Apache-2.0, the 0.25 release `toml` already pulls). YAML: yaml-rust2
  events (MIT/Apache-2.0; yaml-rust is unmaintained, RUSTSEC-2024-0320).
  Email: mail-parser (MIT/Apache-2.0).
- **Fonts**: fontdue (MIT/Apache-2.0/Zlib) with ttf-parser, rasterized on
  workers, never installed or registered permanently with the UI.
- **Archives and comics** go through `skwir-archive-worker`, the only binary
  linked to libarchive (BSD). It lists metadata and streams one regular member
  to a fixed private file. It never uses member names as output paths,
  follows symlinks or extracts a whole archive.

Alternatives: a browser engine (not before its resource and platform costs
are measured); litehtml (to evaluate for CSS fidelity, not bundled); a 7-Zip
fallback (not without an observed compatibility need).

### Bounds

Every bound is a constant in `skwir_platform::inspection::limits`, the one
source for plugins, helpers and the archive worker (given them as
arguments).

| Area | Bound |
|------|-------|
| Hex view | Rows of 16 bytes, the whole file in one view (ADR-051), asked 4096 bytes at a time; search streams bounded chunks, checking cancellation |
| Text and source mode | Read where shown (ADR-051, which replaced 32 KiB sections): rows of 4 KiB, an index of every 256th, chunks of 256 rows; Copy 16 MiB |
| Parsed text, table, structured input | 2 MiB |
| Tables | 500 rows per section, 64 columns, 1024 characters per cell (longer marked shortened) |
| Code | 10000 lines, 64 levels, 4096 bytes per line; JSON 10000 nodes |
| Rich content | 2000 blocks; 8 images decoded with the page (ADR-012: re-readable pictures decode on view, 64 MiB cache) |
| EPUB | 4000 resources, 128 MiB declared expansion, 16 MiB decompressed per resource |
| Archives | 4000 members, 256 KiB of names, 16 MiB per member, 15 s per helper call; helper output derived from the name bound (JSON escaping can sextuple a byte) |
| Font, email input | 16 MiB |
| Fonts | 16–16384 units per em; outlines within eight ems; each glyph checked against the 1000 × 650 canvas; at most sixteen canvases of pixels per pass |
| Email attachments listed | 100 |

- The hex view is read-only; container members and attachments shown as
  bytes use its rows. A hex search reports the exact match byte, so Find
  again resumes after it; a book search resumes at the next chapter and
  reveals the first matching block.
- Invalid or oversized structured input falls back to the source, read
  where it is shown (ADR-051; paginated before). Fold
  ranges are logical source lines from parser spans or events, nested or
  disjoint. JSON is formatted; TOML and YAML keep comments, order and
  indentation. Highlighting keeps parser state across lines on a worker; the
  UI virtualizes lines.
- Local Markdown images must resolve inside its directory, symlinks included;
  email images only from embedded content IDs; EPUB resources inside the
  container.
- Declared sizes are advisory (deflate can inflate past the central
  directory). Every byte decompressed from an EPUB passes one bounded reader,
  and rbook parses only an in-memory package (container.xml, the OPF, its
  contents documents) built from those reads, never the file itself. Chapters
  load on demand.
- A malformed font outline is skipped, not allocated. A font collection shows
  its first face. No mail is ever sent.

## Consequences

- The bounds limit Skwir's own work, not native libraries' resources.
- Packagers may omit the helpers. Disabling a plugin withdraws its UI; its
  Rust parsers stay compiled in.
- Archive members and attachments preview as images, text or bytes, with no
  nested PDF or video session. Encrypted members point to an external archive
  application. Solid archives and compressed tar may decompress sequentially
  for every member request.
- EPUB: no DRM, scripting, fixed layout (later: ADR-013) or full CSS
  fidelity.
- Fixtures and native-check inputs are synthetic. Validation is in
  `docs/PLAN.md`; the filesystem budgets do not measure preview latency.
