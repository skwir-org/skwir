# ADR-009: Optional content preview plugins

Status: accepted, 2026-09-19

## Decision

Keep audio, hexadecimal, Markdown, archive, EPUB, table, structured-data, font,
email and comic previews independently registered runtime plugins. All use the
existing owner-tagged preview registry, cancellation and invalidation boundary.
Format-specific providers have priority over the hexadecimal fallback, even after
disable/re-enable. Explicit presentation selection can request the hex provider
for any regular file. Plugin disable cancels in-flight inspection work too.

Add a toolkit-independent inspection result alongside bitmap, PDF and playback
sessions: semantic text blocks and inline spans, rows with hierarchy/member keys,
embedded images, source availability and section navigation. Providers run on
scheduler workers. GPUI owns virtualized rendering, navigation commands, copy,
search, font size and expansion state; plugins never import GPUI. This is not an
HTML renderer in core, nor a replacement for the existing PDF/media session APIs.

Audio reuses the optional GStreamer worker. The pipeline publishes bounded tag
strings and 32 spectrum bins, at most one update per 50 ms of audio. The UI draws
the spectrum without a timer. Audio-only pipelines exclude attached cover streams
from video playback; cover extraction uses FFmpeg separately. Covers persist when
playback starts. No second playback engine or full-file waveform analysis is added.

Markdown uses pulldown-cmark (MIT). Markdown, EPUB and email share an inert native
reading-mode HTML adapter using scraper/html5ever (ISC/MIT/Apache-2.0). It exposes
headings, paragraphs, lists, inline emphasis, links, code and images; it does not
execute scripts, apply publisher CSS, run browser layout or fetch remote assets.
(ADR-012 amends this: the part of a document's CSS that says how text reads is
applied; layout, fonts and colours are still not.)
EPUB parsing uses rbook (Apache-2.0). Its parser and ZIP dependency require Rust
1.88, now declared by the EPUB plugin and application; the Nix shell already uses
newer Rust. Other crates retain their existing minimum. Prefer this limited reading mode over embedding
a browser before its resource and platform integration costs are measured.
litehtml remains an alternative to evaluate for CSS fidelity, not a bundled dependency.

CSV uses csv (MIT/Unlicense), JSON uses serde_json, TOML uses toml_edit source
spans (MIT/Apache-2.0, the same 0.25 release the workspace's `toml` already
pulls), YAML uses yaml-rust2 parser events (MIT/Apache-2.0; yaml-rust is
unmaintained, RUSTSEC-2024-0320),
email uses mail-parser (MIT/Apache-2.0), and font
specimens use fontdue (MIT/Apache-2.0/Zlib) with ttf-parser. Fonts are rasterized on
workers, never installed or registered permanently with the UI font system.

Archive and comic previews invoke skwir-archive-worker, the only binary linked to
libarchive (BSD). It lists structured metadata and streams a selected regular
member to a fixed private output filename. It never uses archive member names as
filesystem output paths, follows symlinks or extracts an entire archive. Archive
member and email attachment previews currently show images, text or bytes. A
persistent nested PDF/video session is not part of this initial container viewer.
No 7-Zip fallback is added without an observed compatibility need.

## Bounds and limitations

Every bound below is a named constant in `skwir_platform::inspection::limits`,
the single place plugins, helpers and the archive worker (which receives its
bounds as command-line arguments) take them from. Container member and mail
attachment byte views share the hexadecimal plugin's row formatting.

- Hexadecimal display reads 4096-byte sections; search streams bounded chunks and
  checks cancellation. It is read-only. A search reports the exact match byte as
  its position so a repeated Find resumes after it; a book search resumes at the
  next chapter and reveals the first matching block. Source mode reads 32 KiB sections.
- Parsed text/table/structured inputs are bounded to 2 MiB. Tables show 500 rows
  per section, at most 64 columns and 1024 characters per cell (longer cells are
  marked shortened). Code previews are capped at 10000 lines / 64 levels, with 4096 bytes per
  line; JSON formatting also caps nodes at 10000. Invalid or oversized input
  falls back to paginated source. Fold ranges refer to logical source lines,
  come from parser spans/events, and are nested or disjoint. JSON is formatted;
  TOML/YAML retain comments, ordering and indentation. Highlighting maintains
  parser state across lines on a worker; the UI virtualizes visible lines.
- Rich content is limited to 2000 semantic blocks and eight images decoded with
  the page; pictures a provider can read again are decoded as they come into
  view instead, within a 64 MiB cache (ADR-012).
  Local Markdown images must resolve within its directory, including symlinks.
  Email images resolve only embedded content IDs. EPUB resources remain inside
  the container. Declared sizes are advisory: a deflate stream can inflate past
  what the central directory states, so every byte decompressed from an EPUB
  passes through one bounded reader, and rbook parses only an in-memory package
  (container.xml, the OPF and its contents documents) assembled from those
  bounded reads; it never opens the file itself.
- EPUB containers are capped at 4000 resources / 128 MiB declared expansion with
  16 MiB actually decompressed per resource. Chapters load on demand. This does
  not implement DRM, scripting, fixed-layout EPUB or full CSS fidelity.
- Archive listings are capped at 4000 members / 256 KiB of member names, member
  output at 16 MiB and each helper operation at 15 seconds; the helper's captured
  output cap derives from the name bound (JSON escaping can sextuple a byte). Encrypted members
  report that an external archive application is needed. Solid archives and
  compressed tar can require sequential decompression on every member request.
- Font input and email input are bounded to 16 MiB. Fonts must declare 16–16384
  units per em and outline bounds within eight ems; each glyph bitmap is checked
  against the specimen canvas (1000 × 650 pixels) before rasterization and a
  drawing pass paints at most sixteen canvases' worth of pixels, so a malformed
  outline is skipped rather than allocated. Font collections show their first
  face. Email attachment lists show at most 100 attachments. No mail is sent.
- These bounds constrain skwir-owned work; they are not OS resource limits on
  native libraries. Optional helpers may be omitted by packagers. Disabling a
  plugin withdraws its UI contribution; compiled Rust parsers remain in the binary.

All fixtures and native-check inputs are synthetic. Current validation is recorded
in docs/PLAN.md; filesystem benchmark budgets do not measure preview UI latency.
