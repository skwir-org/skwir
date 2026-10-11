# ADR-045: Plugins from outside skwir, and those that run as programs

Status: accepted, 2026-10-07 (Phase 6, "Level 2, external processes").
Built on Linux; macOS compiled only; Windows runs none.

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
   `module` (ADR-046) or `program` inside the folder. skwir looks in
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
   stdin/stdout. skwir sends `initialize`, `field`, `thumbnail`,
   `inspect`, `command` (ADR-047 adds `convert`, `page`); the plugin
   sends `read` and `log`. One call at a time; `$/cancel` when skwir
   stops waiting.
4. **Files by handle.** The plugin reads bytes through `read`, answered
   only for a file of the current call, only with `read`, never for a
   private file (never in a call). Nothing else of the user's is
   reachable.
5. **Isolated or not run.** On Linux, bubblewrap (`sandbox::plugin`):
   system programs, libraries and its own folder readable, a working
   folder writable, network only if declared, dies with skwir. Where
   nothing isolates it, it does not run. Started on first ask; ends
   after 30 s idle, when turned off, or with skwir.
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
  (`protocol::Inspection`), or a file skwir shows, whole or a page at a
  time (ADR-047). Never a UI branch.
