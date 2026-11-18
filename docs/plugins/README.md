# Writing a plugin for Skwir

A plugin from outside Skwir is a folder holding a `plugin.toml` and what
runs it: a **WebAssembly module** (ADR-046) or a **program** (ADR-045).
Skwir finds it in a plugin folder, lists it on Settings › Plugins with what
it may do, and runs it once the user turns it on. It can provide:

- **fields** shown in Properties (a text's word count, a format's version);
- **thumbnails** for files of some types;
- **previews**, one of:
  - an inspection: text, code, hexadecimal rows or a table, a page at a
    time;
  - a converted file Skwir already shows (a PDF, a picture, Markdown,
    audio, video…), displayed by Skwir's own preview of it;
  - pages: each a picture drawn on demand, with its text and links where
    they lie, read in Skwir's reader (select, copy, search, follow links,
    outline);
- **commands**, in the palette and for scripts and agents, answering a
  value, a text for the clipboard, or a plan of changes to files.

Skwir draws everything: a plugin provides data, never a view.

What it says to a person can be in the reader's language (protocol 2): a
message of its own catalogs, which Skwir says in the window's language and
in English to agents ([Its words](#its-words-in-every-language)).

The protocol is experimental (ADR-047): until declared stable, a new
version may change what an older one said. The spec is
[`protocol.md`](protocol.md).

## Start from the template

`crates/plugins-outside/template` is a working plugin (a word count, as a
field and two commands) that builds both as a module and as a program, and
speaks English, French and Simplified Chinese (protocol 2).

1. Copy it to `crates/plugins-outside/<name>`. `skwir-plugin-api` is not
   on crates.io, so build inside the Skwir workspace, which includes every
   folder there.
2. Rename the package in `Cargo.toml` and the `id` in `plugin.toml`, and
   write your messages in `l10n/en.ftl` (and a file a language).
3. Write `src/lib.rs`:

   ```rust
   use serde_json::{Value, json};
   use skwir_plugin_api::guest::{Host, Plugin};
   use skwir_plugin_api::protocol::FieldParams;

   #[derive(Default)]
   pub struct Mine;

   impl Plugin for Mine {
       fn field(&self, host: &dyn Host, params: FieldParams) -> Result<Option<Value>, String> {
           let head = host.read(params.file.handle, 0, 4096)?;
           Ok(Some(json!(head.len())))
       }
   }

   skwir_plugin_api::export!(Mine);
   ```

4. Build with `just plugins` (needs the `wasm32-unknown-unknown` target:
   the Nix dev shell has it; with rustup,
   `rustup target add wasm32-unknown-unknown`). Each plugin lands in
   `target/plugins/<id>`, laid out as an installed one: its manifest, its
   module and its `l10n/`.
5. Run Skwir with it, then turn it on in Settings › Plugins:

   ```
   SKWIR_PLUGIN_PATH=$PWD/target/plugins just run
   ```

`Plugin` has one method per kind of contribution. Each defaults to having
nothing to say (`command` defaults to an error), so implement only what the
manifest declares:

| Method | Declared by | Returns |
| --- | --- | --- |
| `field` | `[[fields]]` | `Option<Value>` |
| `thumbnail` | `[[thumbnails]]` | PNG bytes, or `None` |
| `inspect` | `[[previews]]` | `Option<Inspection>` |
| `convert` | `[[previews]]` with `converts = true` | `(name, bytes)`, or `None` |
| `page` | `[[previews]]` with `pages = true` | `Option<Page>` |
| `command` | `[[commands]]` | `CommandAnswer` |

A file arrives as a `protocol::File`: its `name`, its `path` (for display
only), its `mime` type and `size` when known, and a `handle`. Read its bytes
with `host.read(handle, offset, length)`, at most 4 MiB per read; the
handle is valid only while the call is being answered.
`host.languages()` says the languages the window speaks, most wanted first
(protocol 2), for text the plugin writes in a language of its own.

More examples:

- `crates/plugins-outside/netpbm`: thumbnails, and previews converted to
  PNG (a file of several pictures shown one picture a page). It stays on
  protocol 1 (`api = 1`), the proof that a plugin written for it still runs
  as it did: its words are shown as it writes them.
- `crates/plugins-outside/hex-view`: the built-in hexadecimal preview,
  again as a module, in every language the built-in one speaks.

## The manifest

```toml
id = "word-count"            # lowercase, digits, dashes: names its commands and fields
title = { id = "plugin-title" }  # a message of l10n/ (protocol 2), or text: "Word count"
description = "How many words a text holds, in Properties"
category = "files"           # browsing, files (default), previews or desktop
capabilities = ["read"]      # read, write, network, programs
api = 2                      # the protocol version it speaks (1 when left out)

[run]
module = "word_count.wasm"   # or: program = "bin/word-count", args = [...]

[[fields]]
key = "word-count.words"     # under the id
label = "Words"              # what Properties calls it (text or a message); none keeps it out of Properties
types = ["text/plain"]       # MIME types, matched with their aliases

[[thumbnails]]
types = ["image/x-portable-anymap"]

[[previews]]
types = ["application/octet-stream"]
priority = -100              # higher is tried first (default 0); a fallback is negative
shows_bytes = false          # true: the view every file offers (View as hex)
converts = false             # true: answers `convert`, a file skwir shows
pages = false                # true: answers `page`, a picture and its text a page

[[commands]]
name = "word-count.count"    # under the id
title = "Count words"        # text or a message, as its description
description = "How many words the files hold"
effect = "reads"             # reads, shows, plans or acts (default): what an agent may run
topic = "files"              # where F1 lists its keys: go, select, files, find, notes, tabs, view,
                             # preview, player, terminal, places, window or skwir; the category's
                             # (browsing: go, previews: preview, else files) when left out
arguments = { type = "object", properties = { minimum = { type = "integer" } } }
```

Skwir refuses a manifest (and logs why) unless:

- `id` is 2 to 64 characters: lowercase letters, digits and dashes,
  starting with a letter, and none of the namespaces Skwir names its own
  commands and settings under (`core`, `ui`, `settings`, `plugins`, `tab`…:
  `skwir_plugin_api::manifest::RESERVED`);
- every field `key` and command `name` is `<id>.<something>`;
- `[run]` names exactly one of `module` and `program` (paths relative to
  the plugin's folder);
- `api` is a version Skwir speaks (1 or 2 today), and a message stands in
  it only on 2; a message's id is a letter, then letters, digits, `_` and
  `-`;
- a module does not ask for `network`;
- a command with `effect = "plans"` comes with the `write` capability;
- `arguments`, when given, is a JSON schema of type `object`;
- every field, thumbnail and preview lists its `types`;
- a `shows_bytes` preview neither `converts` nor shows `pages`;
- it has no unknown keys.

A command also takes `entries` (absolute paths) and `pane`. Skwir resolves
the files and hands them to the plugin as `File`s: the ones named, else the
pane's selection.

## Its words, in every language

On protocol 2 (`api = 2`), what a plugin says to a person can be a message
of its own catalogs instead of text: its title and description, a field's
label, a command's title and description, and in what it answers, an
inspection's title, columns, notice and labels, a command's `said`, a
plan's title and note, and its errors. Skwir says each in the window's
language, and in English to agents, MCP and scripts. Text it writes as a
string is shown as written to everyone.

```rust
use skwir_plugin_api::protocol::{Arg, CommandAnswer, Words};

// A message, its arguments typed: a count chooses the plural, a size is
// written as the reader's size setting says, a name is quoted.
let said = Words::message("count-said").with("count", total);
let title = Words::message("title").with("size", Arg::Size(len));
// An error as a message: a `String` that Skwir reads back as one.
return Err(Words::message("no-command").with("command", name).into());
```

Its catalogs lie beside the manifest, in `l10n/`: `en.ftl` its English,
written by you, and a file a language, named by its tag (`fr.ftl`,
`zh-Hans.ftl`), Fluent as Skwir's own catalogs are:

```ftl
# Said with the number the command gives for the clipboard.
count-said = { $count ->
        [one] One word in all
       *[other] { $count } words in all
    }
```

What a language's catalog lacks is said in English; what `en.ftl` lacks, as
its id. Skwir reads the catalogs as it finds the plugin, and leaves out one
over 256 KiB, not UTF-8, or nesting deeper than 16 (the log says why).
Skwir's own catalogs are made from its code and compiled in (`just words`);
a plugin's are its own, never compiled into Skwir. The catalogs' test
(`crates/core/src/words/tests.rs`) still holds those of the plugins here to
their `en.ftl`. The full rules: [`protocol.md`](protocol.md), "Words" and
"Catalogs".

## What a plugin may do

Only what it declares (ADR-044):

| Capability | Allows |
| --- | --- |
| `read` | Reading the bytes of the files a call names, through `Host::read`. |
| `write` | Answering a command with a plan: steps on the call's files by handle (`rename`, `trash`, `move` or `copy` into an absolute folder) or `mkdir` of an absolute folder. Skwir shows the plan for review before anything changes; a plan from a plugin run from the window is always reviewed. |
| `network` | A program may use the network. A module cannot. |
| `programs` | A program may start other programs, isolated as it is. |

Whatever the capabilities, nothing else of the user's is in reach: a module
has no files at all, and a program runs isolated. A link in a converted
file or a page is followed only when the plugin has `network`.

A **private folder** (Settings › Privacy: `~/.ssh` and the like, and any
folder holding a `.private` file) is never handed to a plugin from outside:
its fields say nothing there, its previews say the file is private, and its
commands are refused.

## A module or a program

A **module** (`[run] module`) is the safest and simplest to ship. Build it
for `wasm32-unknown-unknown` with `skwir_plugin_api::export!`. It reaches
only Skwir's two imports, `read` and `log`: no files, no clock, no network,
on every system.

- It runs in `skwir-module-worker`, a helper installed with Skwir and
  isolated where the system can.
- Each call is bounded: 20 billion units of Wasmtime fuel (about one
  instruction each, a few seconds of work) and 256 MiB of memory. A module
  that traps is instantiated again for the next call.
- The helper compiles the module on first start and keeps it compiled in
  the plugin's cache folder (`~/.cache/skwir/1/<id>/module` on Linux); a new
  version of the module is compiled again.

A **program** (`[run] program`) can be written in any language that reads
and writes JSON lines on standard input and output (see `protocol.md`). A
Rust program serves its `Plugin` with `skwir_plugin_api::guest::stdio::serve`
(the template's `main.rs`).

- Skwir starts it isolated when first asked: bubblewrap on Linux,
  `sandbox-exec` on macOS (compiled, never run). Where nothing can isolate
  it, as on Windows today, it does not run.
- Inside: the system's programs and libraries (read-only), the plugin's own
  folder (read-only), and a fresh writable `/work` (its `HOME` is
  `/work/home`, `XDG_CACHE_HOME` is `/work/cache`), deleted when the program
  ends; a private `/tmp`; no `DISPLAY`, `WAYLAND_DISPLAY` or D-Bus session;
  no network without `network`.

Both run one call at a time, under the same limits (a module's helper is
started as a program):

| Limit | Value |
| --- | --- |
| Ready after start (`initialize`) | 10 s |
| Answer to a call | 30 s |
| Idle before it is ended | 30 s (also ended when turned off, and with Skwir) |
| Restart after a failed start | not before 10 s |
| Breaks in a row before the plugin is turned off | 3 (no answer in time, stopped, or protocol broken; the user is told) |

## Testing and debugging

- **Unit tests.** Implement `Host` over bytes in memory and call your
  methods directly: the template's `tests` module does this with `Text`.
  Run them with `cargo nextest run -p <package>`.
- **The JSON.** `guest::answer(&plugin, &host, method, params)` returns
  exactly what Skwir would receive (`plugin-api`'s own tests use it).
- **A program by hand.** Build it (`cargo build -p skwir-outside-template`)
  and send it one JSON-RPC message per line:

  ```
  echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"api":2,"capabilities":["read"],"languages":["fr","en"]}}' \
    | target/debug/skwir-outside-template
  ```

  It answers `{"jsonrpc":"2.0","id":1,"result":{"api":2}}`.
- **End to end.** `crates/extend/tests/` (`modules.rs`, `programs.rs`)
  installs workspace plugins in a plugin folder, as a user would, and drives
  them through Skwir.
- **Logs.** A plugin's `log` lines go to Skwir's log (the terminal Skwir was
  started from; level set by `RUST_LOG`, default `info`) as `plugin: …`, cut
  to 500 characters: `error` and `warn` at warn, `info` at info, `debug` at
  debug. A program's standard error is logged line by line at debug, with
  the plugin's id. To see debug lines:
  `RUST_LOG=info,skwir_extend=debug just run`.

## Installing

Put the plugin's folder in one of these, searched in order:

1. `~/.local/share/skwir/plugins/` (the user's data folder);
2. `skwir/plugins/` in each of the system's data folders
   (`$XDG_DATA_DIRS`);
3. the absolute folders in `SKWIR_PLUGIN_PATH` (separated as in `PATH`;
   relative ones are ignored).

Every subfolder holding a `plugin.toml` is a plugin (its `l10n/` beside
it). Then restart Skwir
(plugins are found at startup) and turn the plugin on in Settings ›
Plugins, where its line says what it may do and where it is. A plugin whose
id is taken (by a built-in plugin, or one found earlier) or whose manifest
does not hold together is left out, with a warning in the log saying why.
