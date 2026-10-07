# ADR-047: A preview from outside is a file bibli shows

Status: accepted, 2026-10-07 (Phase 6, after its three steps).

## Context

A plugin from outside previews a file as text, code, hexadecimal rows
or a table (ADR-045): the protocol's portable inspection. bibli itself
shows much more: Markdown and HTML in reading typography, books,
comics, mail, fonts, archives, maps, pictures zoomed and fitted, PDFs in
the continuous reader, audio and video in the player, 3D models. The
question was whether a plugin from outside could preview with any of
them, and how.

Putting bibli's own preview model on the wire was the obvious way and
the wrong one:

- **It is internal and keeps changing.** `Inspection` and its blocks
  gained alignment, notes and printed pages as previews grew; on the
  wire, each change would be a version of the protocol, and
  `bibli-plugin-api` would come to depend on bibli's core.
- **Some of it acts.** A detail's link runs a command, a document's link
  opens an address: from a plugin, the user's click would be the
  plugin's act.
- **Some of it is read later** (a book's pictures, an archive's
  members), which needs methods back into the plugin for each.
- **The richest are not data.** The reader, the player and the 3D view
  are bibli's views over its helpers' sessions (ADR-007, ADR-046: views
  stay native).

## Decision

1. **A preview may be another file** (protocol version 2, `convert`): a
   plugin whose manifest says `converts` answers a file of a format
   bibli shows (a PDF, a PNG, Markdown, CSV, GPX, a 3D model…) by name
   and bytes, and bibli previews it as it would that file, by whichever
   provider shows its type (`PreviewProvider::converted`, asked first,
   once, in the panel's worker). Every kind bibli shows is reached, the
   reader and the player included, and the contract is public formats,
   which do not change with bibli.
2. **bibli types it** by its name and bytes, as an archive's member,
   never by what the plugin says it is.
3. **bibli keeps it**: in the plugin's folder of the cache, under the
   file's version and the version of the plugin's module or program
   (`made_key`), within 256 MiB (the oldest go), so a page turned, the
   file shown again or bibli started again asks the plugin nothing.
4. **The file's own views stay its own**: View as hex shows its bytes
   and View source its text, not the converted file's.
5. **A link in what a plugin made is followed only if the plugin may use
   the network** (`network`): otherwise a module, which has no network,
   could write what it read into a link's address and have the user's
   click send it. The panel says why instead (`closed_links`).
6. **Versions**: bibli speaks versions 1 and 2 (`OLDEST`, `VERSION`). A
   plugin of version 1 runs unchanged; `converts` needs `api = 2`.

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
- A converted file goes through bibli's own readers, which face any
  file the user opens anyway: no reader meets bytes it could not meet
  before.
- A conversion is whole: a long file is converted at once, up to what
  one message holds (about 48 MB). A paged protocol (a page of a
  document drawn when shown) is a version to come if a plugin needs it.
- The portable inspection stays for what is naturally text or rows,
  paged by the plugin itself: the hexadecimal view, a log, a table.
- Thumbnails of a converted file are not drawn from it: a plugin draws
  its thumbnails itself (`thumbnail`).
