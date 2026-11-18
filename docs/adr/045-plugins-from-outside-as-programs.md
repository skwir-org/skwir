# ADR-045: Plugins from outside Skwir, and those that run as programs

Status: accepted, 2026-10-07 (Phase 6, "Level 2, external processes").
Built on Linux; macOS compiled only; Windows runs none. Amended
2026-11-18: protocol 2, the reader's language (ADR-139, 14).

## Context

All plugins were workspace crates. Phase 6's Level 2 is "a small
line-delimited protocol (LSP-style) for provider processes", first for
thumbnailers and external tools. Level 1 (WebAssembly, ADR-046) speaks
to the same plugins over another transport, so both share what a
plugin from outside is.

## Decision

1. **A plugin from outside is a folder** with a `plugin.toml`
   (`skwir_plugin_api::Manifest`): id (namespace of its commands and
   fields), title, category, capabilities (ADR-044), what it provides
   per MIME type (Details fields, thumbnails, previews, commands), and a
   `module` (ADR-046) or `program` inside the folder. Skwir looks in
   `skwir/plugins` under the user's and system's data folders, then
   `SKWIR_PLUGIN_PATH`. An inconsistent manifest or a taken id is left
   out, with the reason logged.
2. **It is a plugin like the others** (`skwir_extend::External`): on the
   Plugins page with its capabilities and folder, off until turned on,
   contributions withdrawn when off. Its commands enter the registry
   with the manifest's schema (`CommandRegistry::register_dynamic`) and
   effect, reachable from palette, scripts and agents.
3. **Protocol** (`skwir_plugin_api::protocol`,
   `docs/plugins/protocol.md`): JSON-RPC 2.0, one message per line on
   stdin/stdout. Skwir sends `initialize`, `field`, `thumbnail`,
   `inspect`, `command` (ADR-047 adds `convert`, `page`); the plugin
   sends `read` and `log`. One call at a time; `$/cancel` when Skwir
   stops waiting.
4. **Files by handle.** The plugin reads bytes through `read`, answered
   only for a file of the current call, only with `read`, never for a
   private file (never in a call). Nothing else of the user's is
   reachable.
5. **Isolated or not run.** On Linux, bubblewrap (`sandbox::plugin`):
   system programs, libraries and its own folder readable, a working
   folder writable, network only if declared, dies with Skwir. Where
   nothing isolates it, it does not run. Started on first ask; ends
   after 30 s idle, when turned off, or with Skwir.
6. **Answers.** A plan is built from the plugin's steps on its files
   (rename, trash, move, copy, make a folder), needs `write`, and is
   always proposed for review when run from the window. From the
   window, a command's answer is awaited on a worker
   (`scheduler::on_window_thread`); a value or text appears in a
   notification.

## Consequences

- Any language that reads and writes JSON lines works. The template
  (`crates/plugins-outside/template`) builds as a program from Rust
  (`guest::stdio::serve`); `crates/plugins-outside/netpbm`, the first
  outside thumbnailer, runs as a module.
- Windows runs no program plugin (no isolation: ADR-039,
  `docs/PORTING.md`); macOS is compiled only.
- Work is bounded: 30 s per answer, reads of 4 MiB, 64 MiB per message
  (`protocol::READ_LIMIT`, `MESSAGE_LIMIT`).
- Outside previews draw text, code, hex and tables
  (`protocol::Inspection`), or a file Skwir shows, whole or a page at a
  time (ADR-047). Never a UI branch.

## Amendments

### 2026-11-18: protocol 2, the reader's language (ADR-139, 14)

A plugin from outside speaks the reader's language, as Skwir does.

- **The languages.** `initialize` tells a plugin whose manifest says
  `api = 2` the languages the window speaks, most wanted first, English
  last (`InitializeParams::languages`), for what it writes in a language of
  its own. Once they change, its program is ended and started again at its
  next call, told the new ones.
- **Its words.** What it says to a person may be a message of its own
  catalogs instead of a string (`protocol::Words`, `{ "id", "args" }`): its
  manifest's title and description, a field's label, a command's title and
  description; an inspection's title, columns, notice and details' labels;
  a command's `said`, a plan's title and note; an error, whose words a
  program puts in the JSON-RPC error's `data`. Arguments are typed as
  ADR-139's where that matters: an integer a count, `{ "name" }` a name
  quoted by the host, `{ "size" }` a size, a string text as written.
- **Its catalogs** are `l10n/<language>.ftl` beside the manifest, `en.ftl`
  its English, written by its author. They are read with the manifest and
  held to bounds as anyone's input is: 256 KiB a catalog, 64 languages,
  placeables nested 16 deep at most (Fluent's parser recurses), UTF-8, a
  regular file, read whole by Fluent's parser once before any is said.
  They are registered under `outside:<id>`, a name no crate of Skwir's has,
  its id their owner, while it is on, and withdrawn with its other
  contributions. Skwir says each message through its own resolver
  (`Words::of_catalog`): in the window's language, English to agents, MCP
  and scripts; in English where a catalog lacks it; as its id where
  `en.ftl` does too.
- **Version 1 runs unchanged.** `VERSION` is 2, `OLDEST` 1, and a manifest
  that names no version speaks 1: shown as it writes, told no languages,
  its catalogs unread, a message in its manifest refused. The Netpbm
  plugin stays on 1 as the proof.
- **`skwir` is reserved** (`manifest::RESERVED`): the program's own
  catalogs are that owner's, and a plugin of that id turned off would
  withdraw them with its own.
- **Not Skwir's catalogs.** `just words` leaves `crates/plugins-outside/`
  out: a plugin's catalogs are never compiled into Skwir nor counted in a
  language's share; the catalogs' test still holds them to their `en.ftl`.

Rejected: a manifest's titles as ids only (a protocol 2 manifest may still
write text, shown as written); the languages in every call's parameters
(told once, a program started again when they change); a typed world for
errors (a module's error stays a string, the JSON of its message, ADR-046).
