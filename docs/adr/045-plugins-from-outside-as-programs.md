# ADR-045: Plugins from outside bibli, and those that run as programs

Status: accepted, 2026-10-07 (Phase 6, "Level 2, external processes").

## Context

Every plugin so far is a crate of the workspace, built into the binary.
Phase 6 opens the plugin system beyond the repository: Level 2 is "a
small line-delimited protocol (LSP-style) for provider processes", used
first for thumbnailers and external tools. Level 1, WebAssembly modules
(ADR-046), speaks to the same plugins through another transport, so the
two share what a plugin from outside is.

## Decision

1. **A plugin from outside is a folder** with a `plugin.toml`
   (`bibli_plugin_api::Manifest`): its id (a namespace for its commands
   and fields), title, category, the capabilities it needs (ADR-044),
   what it provides (fields shown in Details, thumbnails, previews,
   commands, each for the MIME types it names) and how it runs, a
   `module` (ADR-046) or a `program`, inside its folder. bibli looks in
   `bibli/plugins` of the user's and the system's data folders, then in
   `BIBLI_PLUGIN_PATH`. A plugin whose manifest does not hold together,
   or whose id another has, is left out and the log says why.
2. **It is a plugin like the others** (`bibli_extend::External`): listed
   on the Plugins page with what it may do and its folder, off until the
   user turns it on, its contributions withdrawn when turned off. Its
   commands are in the registry with the schema its manifest gives
   (`CommandRegistry::register_dynamic`) and the effect it declares, so
   the palette, scripts and agents reach them like any.
3. **The protocol** (`bibli_plugin_api::protocol`,
   `docs/plugins/protocol.md`): JSON-RPC 2.0, one message per line, on
   the program's standard input and output. bibli asks `initialize`,
   then `field`, `thumbnail`, `inspect` and `command`; the plugin asks
   back `read` and says `log`. One call at a time; `$/cancel` when bibli
   stops waiting.
4. **Files by handle.** A call names its files by handle; the plugin
   reads their bytes through `read`, which bibli answers only for a file
   of that call, only with the `read` capability, and never for a private
   file, which is never in a call. Nothing else of the user's is in
   reach.
5. **Isolated.** The program runs in bubblewrap on Linux
   (`sandbox::plugin`): the system's programs and libraries and its own
   folder readable, a working folder writable, no network unless it
   declares `network`, and it dies with bibli. Where nothing can isolate
   it, it does not run. It starts when first asked and ends once idle 30
   seconds, when turned off, or with bibli: no daemon.
6. **Answers.** A plan a command answers is built from the plugin's
   steps on its files (rename, trash, move, copy, make a folder), needs
   `write`, and is proposed for review when the command was run from the
   window: a plan from code bibli does not ship is always reviewed there.
   Asked from the window, a command's answer is waited for on a worker
   (`scheduler::on_window_thread`), a value or a text said in a
   notification.

## Consequences

- A plugin can be written in any language that reads and writes lines
  of JSON. The template (`crates/plugins-outside/template`) builds as
  one from Rust (`guest::stdio::serve`); the first thumbnailer from
  outside, `crates/plugins-outside/netpbm`, answers thumbnails as a
  module.
- Windows runs no program plugin yet: there is no isolation there
  (ADR-039, `docs/PORTING.md`); macOS is compiled only.
- A plugin's work is bounded: 30 seconds an answer, bytes read 4 MiB at
  a time, 64 MiB a message.
- Previews from outside draw text, code, hex and tables (the portable
  `protocol::Inspection`), or any file bibli shows, made of the file,
  whole or a page at a time (ADR-047): never a branch of the UI.
