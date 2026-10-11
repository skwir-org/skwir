# ADR-047: A preview from outside is a file skwir shows

Status: accepted, 2026-10-07 (Phase 6, after its three steps). Point 7
(pages) added and point 6 revised the same day, before the first
release: the protocol is experimental, with one version. Built.

## Context

Outside plugins previewed as text, code, hex or tables (ADR-045). skwir
shows far more: reading-mode Markdown and HTML, books, comics, mail,
fonts, archives, maps, fitted pictures, the PDF reader, the player, 3D
models. Could an outside plugin reach these?

Rejected: **skwir's preview model on the wire.**

- It is internal and changes (`Inspection` gained alignment, notes,
  printed pages): each change would be a protocol version, and
  `skwir-plugin-api` would depend on skwir's core.
- Some of it acts: a link runs a command or opens an address, so the
  user's click would be the plugin's act.
- Some is read later (a book's pictures, archive members), needing a
  method back into the plugin for each.
- The richest are not data: the reader, the player and the 3D view are
  native views over helper sessions (ADR-007, ADR-046).

## Decision

1. **A preview may be another file** (`convert`). A plugin with
   `converts` answers a file in a format skwir shows (PDF, PNG,
   Markdown, CSV, GPX, a 3D model…), by name and bytes; skwir previews
   it with whichever provider shows that type
   (`PreviewProvider::converted`, asked first, once, in the panel's
   worker). Every kind is reachable, reader and player included, and
   the contract is public formats.
2. **skwir types it** by name and bytes, as an archive member, never by
   the plugin's word.
3. **skwir caches it** in the plugin's cache folder, keyed by file
   version and plugin version (`made_key`), within 256 MiB (oldest
   evicted). Turning a page, reshowing or restarting asks the plugin
   nothing.
4. **The file's own views stay its own:** View as hex and View source
   show the original, not the converted file.
5. **Links in what a plugin made are followed only with `network`.**
   Otherwise a module could put what it read into an address and have
   the user's click send it. The panel says why (`closed_links`).
6. **Versions.** The protocol is experimental. `convert` and `page`
   (written as versions 2 and 3, never published) ship in skwir 0.1 as
   version 1 (`OLDEST`, `VERSION`). Until declared stable, a new
   version may change an older one's meaning, `OLDEST` follows, and the
   release notes say so. A new method is still a new version, on both
   transports. Once stable, older plugins run unchanged.
7. **A page at a time** (`page`). A plugin with `pages` draws the page
   asked within the side asked, gives the page count, page sizes and
   outline, and on request the page's text as runs with boxes (and
   character starts if known) and its links (to a page or an address).
   - skwir reads it in its own reader (`crates/extend/src/pages.rs`, a
     `DocumentSession` opened by the adapter's `open_document`):
     scrolled or turned, zoomed, text selected, copied and searched
     (side 0 asks text only, so search draws nothing), links followed,
     outline listed; addresses only with `network`
     (`DocumentSession::closed_links`).
   - Answers are cached in the plugin's slot (`CacheSlot::within`,
     256 MiB) by file and plugin version: count and outline, pictures
     per side, text and links per page.
   - Long documents are drawn as read, never converted whole.
   - With `converts` too, a file the plugin returns no file for
     (remembered per version) is read in pages.
   - A plugin that converts or pages is not asked `inspect`.

## Consequences

- The Netpbm plugin (`crates/plugins-outside/netpbm`) shows PPM and PGM
  as the PNG it makes. `crates/extend/tests/modules.rs` checks the file
  is made, cached, remade when the picture changes, never made for a
  private file.
- A plugin owns the types it declares (`docs/invariants/previews.md`):
  if it answers nothing, the panel says "“Netpbm pictures” shows
  nothing of this file", never another provider's preview. So Netpbm
  reads every kind (`P1`-`P6`, text or binary, 16-bit) and scales
  pictures over 3600 px a side down to fit a message.
- Converted files go through skwir's readers, which face any user file
  anyway: no reader meets bytes it could not meet before.
- A conversion is whole, up to one message (about 48 MB); long
  documents use pages.
- Page text is placed by runs: with glyph positions (`edges`) if the
  plugin knows them, else equal shares of the run's box.
- The portable inspection remains for text or rows the plugin pages
  itself: hex, logs, tables.
- Thumbnails are never drawn from a converted file; plugins draw their
  own (`thumbnail`).
