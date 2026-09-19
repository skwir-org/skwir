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
EPUB parsing uses rbook (Apache-2.0). Its parser and ZIP dependency require Rust
1.88, now declared by the EPUB plugin and application; the Nix shell already uses
newer Rust. Other crates retain their existing minimum. Prefer this limited reading mode over embedding
a browser before its resource and platform integration costs are measured.
litehtml remains an alternative to evaluate for CSS fidelity, not a bundled dependency.

CSV uses csv (MIT/Unlicense), JSON uses serde_json, TOML uses the existing parser,
YAML uses serde_yaml_ng (MIT), email uses mail-parser (MIT/Apache-2.0), and font
specimens use fontdue (MIT/Apache-2.0/Zlib) with ttf-parser. Fonts are rasterized on
workers, never installed or registered permanently with the UI font system.

Archive and comic previews invoke bibli-archive-worker, the only binary linked to
libarchive (BSD). It lists structured metadata and streams a selected regular
member to a fixed private output filename. It never uses archive member names as
filesystem output paths, follows symlinks or extracts an entire archive. Archive
member and email attachment previews currently show images, text or bytes. A
persistent nested PDF/video session is not part of this initial container viewer.
No 7-Zip fallback is added without an observed compatibility need.

## Bounds and limitations

- Hexadecimal display reads 4096-byte sections; search streams bounded chunks and
  checks cancellation. It is read-only. Source mode reads 32 KiB sections.
- Parsed text/table/structured inputs are bounded to 2 MiB. Tables show 500 rows
  per section and at most 64 columns. Trees are capped at 10000 nodes / 64 levels.
- Rich content is limited to 2000 semantic blocks and eight embedded images.
  Local Markdown images must resolve within its directory, including symlinks.
  Email images resolve only embedded content IDs. EPUB resources remain inside
  the container and declared expansion is checked before parsing its package.
- EPUB containers are capped at 4000 resources / 128 MiB declared expansion with
  16 MiB per resource. Chapters load on demand. This does not implement DRM,
  scripting, fixed-layout EPUB or full CSS fidelity.
- Archive listings are capped at 4000 members / 256 KiB of member names, member
  output at 16 MiB and each helper operation at 15 seconds. Encrypted members
  report that an external archive application is needed. Solid archives and
  compressed tar can require sequential decompression on every member request.
- Font input and email input are bounded to 16 MiB. Font collections show their
  first face. Email attachment lists show at most 100 attachments. No mail is sent.
- These bounds constrain bibli-owned work; they are not OS resource limits on
  native libraries. Optional helpers may be omitted by packagers. Disabling a
  plugin withdraws its UI contribution; compiled Rust parsers remain in the binary.

All fixtures and native-check inputs are synthetic. Current validation is recorded
in docs/PLAN.md; filesystem benchmark budgets do not measure preview UI latency.
