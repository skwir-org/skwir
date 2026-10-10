# ADR-047: A preview from outside is a file skwir shows

Status: accepted, 2026-10-07 (Phase 6, after its three steps); pages
(its seventh point) the same day; its sixth point revised the same day,
before the first release: the protocol is experimental and has one
version.

## Context

A plugin from outside previews a file as text, code, hexadecimal rows
or a table (ADR-045): the protocol's portable inspection. skwir itself
shows much more: Markdown and HTML in reading typography, books,
comics, mail, fonts, archives, maps, pictures zoomed and fitted, PDFs in
the continuous reader, audio and video in the player, 3D models. The
question was whether a plugin from outside could preview with any of
them, and how.

Putting skwir's own preview model on the wire was the obvious way and
the wrong one:

- **It is internal and keeps changing.** `Inspection` and its blocks
  gained alignment, notes and printed pages as previews grew; on the
  wire, each change would be a version of the protocol, and
  `skwir-plugin-api` would come to depend on skwir's core.
- **Some of it acts.** A detail's link runs a command, a document's link
  opens an address: from a plugin, the user's click would be the
  plugin's act.
- **Some of it is read later** (a book's pictures, an archive's
  members), which needs methods back into the plugin for each.
- **The richest are not data.** The reader, the player and the 3D view
  are skwir's views over its helpers' sessions (ADR-007, ADR-046: views
  stay native).

## Decision

1. **A preview may be another file** (`convert`): a
   plugin whose manifest says `converts` answers a file of a format
   skwir shows (a PDF, a PNG, Markdown, CSV, GPX, a 3D model…) by name
   and bytes, and skwir previews it as it would that file, by whichever
   provider shows its type (`PreviewProvider::converted`, asked first,
   once, in the panel's worker). Every kind skwir shows is reached, the
   reader and the player included, and the contract is public formats,
   which do not change with skwir.
2. **skwir types it** by its name and bytes, as an archive's member,
   never by what the plugin says it is.
3. **skwir keeps it**: in the plugin's folder of the cache, under the
   file's version and the version of the plugin's module or program
   (`made_key`), within 256 MiB (the oldest go), so a page turned, the
   file shown again or skwir started again asks the plugin nothing.
4. **The file's own views stay its own**: View as hex shows its bytes
   and View source its text, not the converted file's.
5. **A link in what a plugin made is followed only if the plugin may use
   the network** (`network`): otherwise a module, which has no network,
   could write what it read into a link's address and have the user's
   click send it. The panel says why instead (`closed_links`).
6. **Versions**: the protocol is experimental. `convert` and `page`
   were written as versions 2 and 3, and none was published: skwir 0.1
   publishes them as one, version 1 (`OLDEST` and `VERSION`). Until the
   protocol is declared stable, a new version may change what an older
   one said, and `OLDEST` then follows it, which the release notes say;
   a new method is still a new version, both transports at once. Once
   stable, a plugin of an older version runs unchanged.
7. **A page at a time** (`page`): a plugin whose
   manifest says `pages` draws the page asked, within the side asked,
   as a picture, says how many there are, the page's own size and the
   file's outline, and, when asked, its text as runs with the boxes
   they lie in (and where each character starts, if it says) and its
   links (to a page, or an address). skwir reads it as a document
   (`crates/extend/src/pages.rs`, a `DocumentSession` the adapter's
   `open_document` opens), in its own reader: scrolled or turned,
   zoomed, its text selected, copied and searched by skwir (side 0 asks
   a page's text alone, so a search draws nothing), its links followed,
   its outline listed; an address only when the plugin may use the
   network (`DocumentSession::closed_links`, as a converted file's).
   What it is answered is kept in the plugin's slot of the cache
   (`CacheSlot::within`, 256 MiB) under the file's version and the
   plugin's: the count and outline, each picture by its side, each
   page's text and links apart. A long document is drawn as it is read, never converted
   whole. With `converts`, a file the plugin answers no file for (an
   answer remembered for that version of it) is read in pages; a
   preview that converts or shows pages is not asked `inspect`.

## Consequences

- The Netpbm plugin (`crates/plugins-outside/netpbm`) previews PPM and
  PGM pictures as the PNG it makes, shown, zoomed and fitted as any
  picture; `crates/extend/tests/modules.rs` checks the file made, kept
  and not asked for again, made again when the picture changes, never
  for a private file.
- A plugin owns the files of the types it declares, as every preview
  provider does (`docs/invariants/previews.md`): one that answers
  nothing for a file shows "“Netpbm pictures” shows nothing of this
  file", never another provider's preview. So the Netpbm plugin reads
  every Netpbm kind (`P1` to `P6`, as text or bytes, 16-bit samples),
  and converts a picture longer than 3600 pixels a side smaller, so its
  PNG fits a message.
- A converted file goes through skwir's own readers, which face any
  file the user opens anyway: no reader meets bytes it could not meet
  before.
- A conversion is whole, up to what one message holds (about 48 MB); a
  long document is shown in pages instead.
- A page's text is placed by runs: a plugin that knows its glyphs says
  where each starts (`edges`), one that does not gives the run's box,
  each character an equal part of it.
- The portable inspection stays for what is naturally text or rows,
  paged by the plugin itself: the hexadecimal view, a log, a table.
- Thumbnails of a converted file are not drawn from it: a plugin draws
  its thumbnails itself (`thumbnail`).
