# The plugin protocol, version 1

What skwir and a plugin from outside say to each other (ADR-045,
ADR-046, ADR-047). The types are `skwir_plugin_api::protocol`'s; a Rust
plugin uses them, one in another language writes the same JSON.

The protocol is experimental (ADR-047, its sixth point): until it is
declared stable, a new version may change what an older one said, and
skwir then stops speaking the older one; the release notes say so. A
manifest says which version its plugin speaks (`api`, 1 when it says
nothing). Once it is stable, a plugin written for an older version
runs unchanged and is never asked what its version did not have.

## Transports

- **A program**: JSON-RPC 2.0, one message per line (a line at most 64
  MiB) on its standard input (skwir's messages) and standard output (the
  plugin's). Its standard error goes to skwir's log.
- **A module**: the world `skwir:plugin@0.1.0` (`crates/plugin-api/wit/plugin.wit`).
  skwir calls the export `call(method, params)` with a method's name
  and its parameters as JSON text, and reads its answer as JSON text (or
  an error's message). The plugin calls the imports `read(file, offset,
  length)` and `log(level, message)`.

One call at a time per plugin: skwir asks the next once one is
answered. While answering, the plugin may ask skwir `read` and say
`log`.

## skwir asks

### `initialize` (a program, first)

```json
{ "api": 1, "capabilities": ["read"] }
```

`api` is the newest version skwir speaks. Answer `{ "api": 1 }`, the
version the plugin speaks; one skwir does not speak is refused.

### `field`

```json
{ "key": "word-count.words",
  "file": { "handle": 1, "name": "note.txt", "path": "/home/a/note.txt",
            "mime": "text/plain", "size": 42 } }
```

Answer `{ "value": 7 }`: a string, a number, a boolean or a list of
them, or `null` when the file says nothing.

### `thumbnail`

```json
{ "file": { … }, "size": 256 }
```

Answer `{ "png": "<base64>" }`, a picture that fits the square of that
side, or `{ "png": null }`.

### `inspect`

```json
{ "file": { … }, "offset": 4096, "query": "needle", "source": false, "target": null }
```

`offset` is where the page asked starts (a byte, a row, a section: the
plugin's own `previous` and `next` say), `query` what to find from
there, `target` a row's own `target` asked for. Answer `null`, or:

```json
{ "kind": "hex",            // text, code, hex or table
  "title": "Hex · 9.8 KB",
  "text": [],               // paragraphs, or lines of code
  "columns": ["Offset", "…", "ASCII"],
  "rows": [ { "cells": ["0000000000000000", "…", "…"],
              "bytes": null, "depth": 0, "branch": false, "target": null } ],
  "previous": null, "next": 4096, "position": 0,
  "notice": "Match at 0x64",
  "details": [ { "label": "Version", "value": "1.2" } ] }
```

### `convert`

For a preview the manifest declares with `converts = true`:

```json
{ "file": { "handle": 1, "name": "sunset.ppm", … } }
```

Answer `null` (nothing to show for this one), or a file of a format
skwir shows, which it previews as it would that file: a PDF in the
reader, a picture zoomed and fitted, audio or video in the player,
Markdown, a table, a track on the map.

```json
{ "name": "sunset.png", "data": "<base64>" }
```

skwir types it by its name and bytes, as any file, and keeps it in the
plugin's folder of its cache under the file's version and the plugin's:
it is asked again only when either changes. The file's own views stay
its own: View as hex shows its bytes, not the converted file's. A link
in what a plugin made is followed only when the plugin may use the
network (`network`): otherwise a click could carry what it read away.
The file must fit one message (about 48 MB once in base64).

### `page`

For a preview the manifest declares with `pages = true`: the file shown
a page at a time, each a picture the plugin draws when it is shown, with
its text where it lies (a document of a format skwir does not read, a
file of several pictures). skwir reads it in its own reader, as it reads
a PDF: pages scrolled or turned, zoomed, their text selected, copied
and searched, their links followed, its outline listed.

```json
{ "file": { … }, "page": 2, "side": 2048, "text": true }
```

`page` counts from 1; `side` is the longest side to draw it at, in
pixels, and 0 asks no picture: how many pages there are, or a page's
text alone, which a search reads without drawing a page. `text` asks
the page's text and links too. Answer `null` (nothing to show for this
file), or:

```json
{ "pages": 12,
  "png": "<base64>",            // null past the last, or for side 0
  "width": 612, "height": 792,  // its own size, CSS pixels: what 100% shows
  "text": [ { "text": "Chapter one", "x": 0.1, "y": 0.08, "width": 0.4, "height": 0.03,
              "edges": [0.1, 0.13, 0.16, …] } ],
  "links": [ { "x": 0.1, "y": 0.9, "width": 0.2, "height": 0.03, "page": 3 },
             { "x": 0.5, "y": 0.9, "width": 0.3, "height": 0.03, "uri": "https://example.com/" } ],
  "outline": [ { "title": "Chapter one", "depth": 0, "page": 1 } ] }
```

The text is runs (a word, a line) in reading order, each with the box it
lies in as fractions of the page's width and height from its top left:
skwir puts a space between two runs on one line and a line break between
lines, and searches it whatever the case. A run's `edges`, one a
character, say where each starts across the page, so a selection follows
each glyph; without them each character is an equal part of its run. A
link lies in such a box too, and leads to a page of the file or to an
address, which is followed only when the plugin may use the network
(`network`): otherwise the panel says why. The outline (headings, how
deep, the page each starts on) is the file's, the same with any page:
skwir reads it once, with how many pages there are. Without `width` and
`height` the page is its picture's size. Every field but `pages` may be
left out.

skwir keeps what it is answered in the plugin's slot of its cache, under
the file's version and the plugin's: how many pages and the outline,
each page's picture by the side drawn, its text and links apart; nothing
is asked twice. With
`converts` too, skwir asks `convert` first and reads in pages a file it
answers `null` for: the Netpbm plugin converts one picture to a PNG, and
shows a file of several a picture a page. A preview that converts or
shows pages is not asked `inspect`.

### `command`

```json
{ "command": "word-count.count", "args": { }, "entries": [ { "handle": 1, … } ] }
```

Answer one of:

```json
{ "answer": "done" }
{ "answer": "value", "value": { "total": 9 } }
{ "answer": "text", "text": "…", "said": "Copied 3 paths" }
{ "answer": "plan", "title": "Rename by date", "note": null,
  "steps": [ { "op": "rename", "file": 1, "name": "2026-10-07.txt" },
             { "op": "trash", "file": 2 },
             { "op": "move", "file": 3, "to": "/home/a/Archive" },
             { "op": "copy", "file": 4, "to": "/home/a/Archive" },
             { "op": "mkdir", "path": "/home/a/Archive" } ] }
```

A plan needs the `write` capability.

### `$/cancel` (a program, a notification)

```json
{ "id": 7 }
```

skwir stopped waiting for call 7; reads for it are refused from then on.

## The plugin asks

### `read`

```json
{ "file": 1, "offset": 0, "length": 65536 }
```

Answered `{ "data": "<base64>" }`, at most 4 MiB, fewer at the file's
end; refused for a handle that is no file of the call being answered, or
for a plugin without the `read` capability.

### `log` (a notification)

```json
{ "level": "warn", "message": "…" }
```

`error`, `warn`, `info` or `debug`.

## Errors

A JSON-RPC error's `message` (a module: the `call`'s error) is what
skwir says: a preview shows it where the page would be; a command's
caller gets it.
