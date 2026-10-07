# The plugin protocol, version 1

What bibli and a plugin from outside say to each other (ADR-045,
ADR-046). The types are `bibli_plugin_api::protocol`'s; a Rust plugin
uses them, one in another language writes the same JSON.

## Transports

- **A program**: JSON-RPC 2.0, one message per line (a line at most 64
  MiB) on its standard input (bibli's messages) and standard output (the
  plugin's). Its standard error goes to bibli's log.
- **A module**: the world `bibli:plugin@0.1.0` (`crates/plugin-api/wit/plugin.wit`).
  bibli calls the export `call(method, params)` with a method's name
  and its parameters as JSON text, and reads its answer as JSON text (or
  an error's message). The plugin calls the imports `read(file, offset,
  length)` and `log(level, message)`.

One call at a time per plugin: bibli asks the next once one is
answered. While answering, the plugin may ask bibli `read` and say
`log`.

## bibli asks

### `initialize` (a program, first)

```json
{ "api": 1, "capabilities": ["read"] }
```

Answer `{ "api": 1 }`, the version the plugin speaks. A different one
is refused.

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

bibli stopped waiting for call 7; reads for it are refused from then on.

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
bibli says: a preview shows it where the page would be; a command's
caller gets it.
