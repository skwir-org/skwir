# The plugin protocol, version 1

What Skwir and a plugin from outside say to each other (ADR-045, ADR-046,
ADR-047). The types are in `skwir_plugin_api::protocol`
(`crates/plugin-api/src/protocol.rs`); a Rust plugin uses them, a plugin in
another language writes the same JSON. How to write a plugin:
[`README.md`](README.md).

## Versions

The protocol is experimental (ADR-047, point 6). Until it is declared
stable, a new version may change what an older one said, and Skwir then
stops speaking the older one; the release notes say so. Once stable, a
plugin written for an older version runs unchanged and is never asked what
its version did not have.

A manifest states the version its plugin speaks (`api`, default 1). Skwir
speaks versions `OLDEST` to `VERSION`, both 1 today.

## Transports

- **Program**: JSON-RPC 2.0, one message per line on its standard input
  (Skwir's messages) and standard output (the plugin's). A line is at most
  64 MiB, newline included (`MESSAGE_LIMIT`). Standard error goes to Skwir's
  log. Skwir closes standard input when it no longer needs the program.
- **Module**: the WIT world `skwir:plugin@0.1.0`
  (`crates/plugin-api/wit/plugin.wit`). Skwir calls the export
  `call(method, params)` with the method's name and its parameters as JSON
  text, and reads the answer as JSON text, or an error message. The plugin
  calls the imports `read(file, offset, length)` and `log(level, message)`.

One call at a time per plugin: Skwir sends the next once one is answered.
While answering, the plugin may send `read` requests and `log`
notifications.

## Files

A call about files names each as a `File`:

```json
{ "handle": 1, "name": "note.txt", "path": "/home/a/note.txt",
  "mime": "text/plain", "size": 42 }
```

| Field | Type | |
| --- | --- | --- |
| `handle` | u32 | Reads its bytes (`read`); valid while the call is answered. |
| `name` | string | A name that is not UTF-8 is shown lossily. |
| `path` | string | For display only. |
| `mime` | string, optional | When known. |
| `size` | u64, optional | Bytes, when known. |

## Skwir asks

### `initialize` (program only, first)

```json
{ "api": 1, "capabilities": ["read"] }
```

`api` is the newest version Skwir speaks; `capabilities` what the plugin
was granted. Answer `{ "api": 1 }` with the version the plugin speaks; a
version Skwir does not speak is refused.

### `field`

```json
{ "key": "word-count.words", "file": { … } }
```

`key` is one of the manifest's field keys. Answer `{ "value": 7 }`: a
string, a number, a boolean or a list of them, or `null` (or no `value`)
when the file says nothing.

### `thumbnail`

```json
{ "file": { … }, "size": 256 }
```

`size` is the side, in pixels, of the square the picture must fit. Answer
`{ "png": "<base64>" }`, or `{ "png": null }` for no thumbnail.

### `inspect`

```json
{ "file": { … }, "offset": 4096, "query": "needle", "source": false, "target": null }
```

| Field | Default | |
| --- | --- | --- |
| `offset` | 0 | Where the page asked starts: a byte, a row, a section, as the plugin numbers them (its `previous` and `next`). |
| `query` | null | What to find from `offset` on; the answer's `position` says where. |
| `source` | false | The file's source asked rather than its presentation. |
| `target` | null | A row's own `target`, asked for. |

Answer `null` (nothing to show), or an inspection:

```json
{ "kind": "hex",
  "title": "Hex · 9.8 KB",
  "text": [],
  "columns": ["Offset", "…", "ASCII"],
  "rows": [ { "cells": ["0000000000000000", "…", "…"],
              "bytes": null, "depth": 0, "branch": false, "target": null } ],
  "previous": null, "next": 4096, "position": 0,
  "notice": "Match at 0x64",
  "details": [ { "label": "Version", "value": "1.2" } ] }
```

| Field | Default | |
| --- | --- | --- |
| `kind` | `text` | `text` (paragraphs), `code` (lines, monospaced), `hex` (offset, bytes, characters), `table` (rows and columns). |
| `title` | required | |
| `text` | `[]` | Paragraphs, or lines of code. |
| `columns` | `[]` | |
| `rows` | `[]` | Each: `cells`; `bytes` (a container member's size, optional); `depth` (0); `branch` (false); `target` (what `target` asks for this row, optional). |
| `previous`, `next` | null | Where the page before and after starts, if any. |
| `position` | 0 | Where what was asked (found) is. |
| `notice` | `""` | A line over the page: what a search found, what was left out. |
| `details` | `[]` | `{ label, value }` facts the format knows, for Properties. |

A preview that converts or shows pages is not asked `inspect`.

### `convert`

For a preview declared with `converts = true`:

```json
{ "file": { "handle": 1, "name": "sunset.ppm", … } }
```

Answer `null` (nothing to show for this file), or a file in a format Skwir
shows, which it previews as it would that file (a PDF in the reader, a
picture zoomed and fitted, audio or video in the player, Markdown, a table,
a track on the map):

```json
{ "name": "sunset.png", "data": "<base64>" }
```

- Skwir types it by its name (the extension) and bytes, as any file.
- It must fit one message: about 48 MB once in base64.
- Skwir caches it in the plugin's cache folder under the file's version and
  the plugin's, and asks again only when either changes.
- The file's own views stay its own: View as hex shows the original's bytes,
  not the converted file's.
- A link in it is followed only when the plugin has `network`; otherwise a
  click could carry away what it read.

### `page`

For a preview declared with `pages = true`: the file shown a page at a time,
each page a picture the plugin draws when shown, with its text where it
lies (a document format Skwir does not read, a file of several pictures).
Skwir reads it in its own reader, as a PDF: pages scrolled or turned,
zoomed, text selected, copied and searched, links followed, outline listed.

```json
{ "file": { … }, "page": 2, "side": 2048, "text": true }
```

| Field | Default | |
| --- | --- | --- |
| `page` | required | From 1. |
| `side` | required | Longest side to draw at, in pixels. 0 asks no picture: only the page count, or a page's text (a search reads text without drawing). |
| `text` | false | Asks the page's text and links too. |

Answer `null` (nothing to show for this file), or:

```json
{ "pages": 12,
  "png": "<base64>",
  "width": 612, "height": 792,
  "text": [ { "text": "Chapter one", "x": 0.1, "y": 0.08, "width": 0.4, "height": 0.03,
              "edges": [0.1, 0.13, 0.16, …] } ],
  "links": [ { "x": 0.1, "y": 0.9, "width": 0.2, "height": 0.03, "page": 3 },
             { "x": 0.5, "y": 0.9, "width": 0.3, "height": 0.03, "uri": "https://example.com/" } ],
  "outline": [ { "title": "Chapter one", "depth": 0, "page": 1 } ] }
```

Every field but `pages` may be left out.

| Field | |
| --- | --- |
| `pages` | How many pages the file has. |
| `png` | The page drawn; null past the last page, or when `side` is 0. |
| `width`, `height` | The page's own size in CSS pixels (1/96 inch): what 100% shows. Default: the picture's size. |
| `text` | Runs (a word, a line) in reading order. `x`, `y`, `width`, `height` are the run's box as fractions of the page's width and height, from its top left. Optional `edges`: where each character starts, as a fraction of the page's width, one per character, so a selection follows each glyph; without them each character is an equal part of its run. |
| `links` | A box (as for runs) and where it leads: `page` (from 1) of this file, or `uri`. |
| `outline` | The file's headings: `title`, `depth` (0 at the top, default 0), `page` it starts on (optional). |

- Skwir joins two runs on one line with a space and lines with a line
  break, and searches the text case-insensitively.
- An address link is followed only when the plugin has `network`;
  otherwise the panel says why.
- The outline is the file's, the same for any page: Skwir reads it once,
  with the page count.
- Skwir caches what it is answered in the plugin's cache folder, under the
  file's version and the plugin's: the page count and outline, each page's
  picture by side, and its text and links separately. Nothing is asked
  twice.
- With `converts` too, Skwir asks `convert` first and reads in pages a file
  `convert` answers `null` for. The Netpbm plugin converts a single picture
  to PNG and shows a file of several one picture a page.

### `command`

```json
{ "command": "word-count.count", "args": { }, "entries": [ { "handle": 1, … } ] }
```

`command` is one of the manifest's command names; `args` follows its
schema (default null); `entries` are the files it acts on, those its caller
named, else a pane's selection (default `[]`). Answer one of:

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

| Answer | Meaning |
| --- | --- |
| `done` | Nothing to report. |
| `value` | A value for the caller (a script, an agent). |
| `text` | Text for the clipboard; `said` is what Skwir says once it is there. |
| `plan` | A plan the caller applies or proposes. Needs the `write` capability. `note` is optional. |

A step's `file` is a handle of the call's files: `rename` gives it `name`
in its folder, `trash` trashes it, `move` and `copy` put it into the folder
`to`. `mkdir` makes the folder `path`. `to` and `path` are absolute.

### `$/cancel` (program only, notification)

```json
{ "id": 7 }
```

Skwir stopped waiting for call 7; its `read` requests are refused from then
on.

## The plugin asks

### `read`

```json
{ "file": 1, "offset": 0, "length": 65536 }
```

Answered `{ "data": "<base64>" }`: at most 4 MiB (`READ_LIMIT`; a longer
`length` is cut to it), fewer at the file's end. Refused for a handle that
is not a file of the call being answered, after `$/cancel`, and for a
plugin without the `read` capability.

### `log` (notification)

```json
{ "level": "warn", "message": "…" }
```

`level` is `error`, `warn`, `info` or `debug`.

## Errors

A JSON-RPC error's `message` (for a module, the error `call` returns) is
what Skwir reports: a preview shows it where the page would be; a command's
caller receives it. Skwir does not read the `code`; its own errors and the
Rust guest's use `-32000`. A request to Skwir for a method other than
`read` is answered with an error.
