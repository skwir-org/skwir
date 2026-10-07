# Writing a plugin for bibli

A plugin from outside bibli is a folder: a `plugin.toml` that says what
the plugin is, and what runs it, a **WebAssembly module** (ADR-046) or a
**program** (ADR-045). bibli finds it in a folder of plugins, lists it on
Settings › Plugins with what it may do, and runs it once the user turns
it on. It provides any of:

- **fields** shown in Details (a text's word count, a format's version);
- **thumbnails** of the files of some types;
- **previews**: text, code, hexadecimal rows or a table, a page at a time;
- **commands**, in the palette and for scripts and agents, answering a
  value, a text for the clipboard, or a plan of changes to files.

Views stay bibli's: a plugin provides data, bibli draws it.

## Start from the template

`crates/plugins-outside/template` is a working plugin (a word count, as a
field and a command) that builds both ways. Copy it, rename the package
and the `id`, and change `src/lib.rs`:

```rust
use bibli_plugin_api::guest::{Host, Plugin};
use bibli_plugin_api::protocol::FieldParams;
use serde_json::{Value, json};

#[derive(Default)]
pub struct Mine;

impl Plugin for Mine {
    fn field(&self, host: &dyn Host, params: FieldParams) -> Result<Option<Value>, String> {
        let head = host.read(params.file.handle, 0, 4096)?;
        Ok(Some(json!(head.len())))
    }
}

bibli_plugin_api::export!(Mine);
```

`Plugin` has one method per kind of thing a plugin provides (`field`,
`thumbnail`, `inspect`, `command`), each with a default that has nothing
to say: implement those the manifest declares. A file comes as a
`protocol::File`: its name, its path (to show), its type and size, and a
`handle` to read its bytes with, valid while the call is answered.

`crates/plugins-outside/netpbm` is a thumbnailer, and
`crates/plugins-outside/preview-hex` a preview (the built-in
hexadecimal one, again as a module).

Build the workspace's plugins with `just plugins`: each lands in
`target/plugins/<id>` as an installed plugin is laid out. Try them with

```
BIBLI_PLUGIN_PATH=$PWD/target/plugins just run
```

then Settings › Plugins, and turn it on.

## The manifest

```toml
id = "word-count"            # lowercase, digits, dashes: names its commands and fields
title = "Word count"
description = "How many words a text holds, in Details"
category = "files"           # browsing, files, previews or desktop
capabilities = ["read"]      # read, write, network, programs

[run]
module = "word_count.wasm"   # or: program = "bin/word-count", args = [...]

[[fields]]
key = "word-count.words"     # under the id
label = "Words"              # what Details calls it; none keeps it out of Details
types = ["text/plain"]       # MIME types, matched with their aliases

[[thumbnails]]
types = ["image/x-portable-anymap"]

[[previews]]
types = ["application/octet-stream"]
priority = -100              # higher is tried first
shows_bytes = false          # true: the view every file offers (View as hex)

[[commands]]
name = "word-count.count"    # under the id
title = "Count words"
description = "How many words the files hold"
effect = "reads"             # reads, shows, plans or acts (what an agent may run)
arguments = { type = "object", properties = { minimum = { type = "integer" } } }
```

A command also takes `entries` (absolute paths) and `pane`: the files it
acts on, which bibli resolves and hands it as `File`s, the pane's
selection when none is named.

## What a plugin may do

Nothing it does not declare (ADR-044):

- **`read`**: read the bytes of the files a call names, through
  `Host::read`. Nothing else of the user's is in reach, whichever the
  capabilities: a module has no files at all, a program runs isolated.
- **`write`**: answer a command with a plan. A plan is steps on the
  call's files by handle (`rename`, `trash`, `move` and `copy` into an
  absolute folder) or `mkdir` an absolute folder. bibli shows it for
  review before anything changes; a plan from a plugin run from the
  window is always reviewed.
- **`network`**: a program may use the network. A module cannot.
- **`programs`**: a program may start others, isolated as it is.

A **private folder** (Settings › Privacy: `~/.ssh` and its like, and any
folder holding a `.private` file) is never handed to a plugin from
outside: its fields say nothing there, its previews say the file is
private, its commands are refused.

## A module or a program

A **module** (`[run] module`) is the safest and the simplest to ship:
built for `wasm32-unknown-unknown` with `bibli_plugin_api::export!`, it
reaches nothing but `read` and `log`, on every system. bibli runs it in
a helper of its own (`bibli-module-worker`, installed with bibli), and
each call has a bound on its work and memory. The helper compiles the
module the first time and keeps it compiled in bibli's cache
(`~/.cache/bibli/<id>/module`), so a new version of the module is
compiled again on its own.

A **program** (`[run] program`) can be written in any language that
reads and writes lines of JSON on its standard input and output: the
protocol is in `protocol.md`. A Rust program serves its `Plugin` with
`bibli_plugin_api::guest::stdio::serve` (the template's `main.rs`).
bibli starts it isolated (bubblewrap on Linux; it does not run where
nothing can isolate it, as on Windows today), one call at a time, and
ends it once idle.

## Installing

Put the folder in `~/.local/share/bibli/plugins/` (or the system's data
folder's `bibli/plugins/`), restart bibli, and turn the plugin on in
Settings › Plugins, where its line says what it may do and where it is.
An id another plugin has, or a manifest that does not hold together, is
left out; the log says why.
