# ADR-043: Agents and scripts drive the running window

Status: accepted, 2026-10-07, as the user chose (Phase 6, first step,
begun before Phase 5b's gate, which needs a Mac and a Windows machine;
PLAN.md records the deviation). Built (`control` plugin).
Decision 5 amended by ADR-085 (2026-10-23): separate agent and script
endpoints determine authority; request metadata never grants it.

## Context

Phase 6 adds a command line and an MCP server, thin consumers of the
command registry, so an agent can drive Skwir and end with a plan the
user reviews. The pieces exist: commands with JSON schemas
(`CommandRegistry::list`, `check`, `dispatch`), plans for file changes,
review for plans not from the user's gesture (`App::propose`), window
commands that wait for the window (`App::request_front`).

An agent could run its own windowless Skwir, or reach the user's
window. Plans are reviewed and undone in the window; a windowless Skwir
shows no plan and keeps a second undo history and settings writer.

## Decision

1. **Agents and scripts drive the window.** The `control` plugin, off by
   default, listens while on and the window runs, on a channel only the
   user reaches: a Unix socket in a user-only folder
   (`$XDG_RUNTIME_DIR/skwir/control.sock` on Linux, under `$TMPDIR` on
   macOS), a named pipe on Windows. No daemon: the channel ends with
   the window or the plugin. A second Skwir finds it taken and does not
   listen.
2. **MCP through `rmcp`** (the official Rust SDK), on the plugin's own
   runtime and thread, started and ended with the plugin. Tokio is
   already linked (gpui's HTTP client, map tiles). The core stays
   synchronous; a part needing async owns its runtime, and a shared one
   would be a `skwir-platform` utility (as ADR-021 says of HTTP).
3. **The tools are the commands**, with their declared descriptions and
   schemas; dots become `__` (`tags.edit` → `tags__edit`), which
   clients accept and which maps back exactly.
   - A window command (`ui.*`) is asked of the window (`request_front`).
   - A plan is proposed for review; the tool answers once the user
     applies (with what applied and any error) or dismisses, or after a
     while says it still waits.
   - A value, or text for the clipboard, is the answer.
4. **`skwir mcp`**, started by the agent's configuration, pipes stdio to
   the window's channel. With no window listening it says to open Skwir
   and turn Control on, and starts nothing.
5. **`skwir cmd <name> [arguments]`** runs one command over the channel
   and prints JSON. A plan is printed, or applied with `--apply`: the
   user's own action, like a key, applied whole and undoable. The
   request says which in `_meta` (`skwir/plan`: `print` or `apply`;
   absent, the plan is proposed); an agent's model fills arguments,
   never `_meta`. With no window listening, the command runs in a
   windowless Skwir: it answers, or prints or `--apply`s its plan.
6. **Off until the permission layer** (ADR-044): any program running as
   the user can reach the channel, and an agent reads what commands
   read, so folders such as `~/.ssh` need keeping from it first.

## Consequences

- A plan proposed with a reply (`App::propose_awaiting`) learns from the
  executor whether it was dismissed or how it applied.
- **Callers name files.** Scripts and agents put absolute paths in
  `entries`; window surfaces name what they show by identity and path.
  The selection is used only when nothing is named: it is the user's
  and can change between two calls. Relative paths are refused from any
  surface: the window's working folder is not the caller's.
- **No command applies a plan.** `apply_plan` hands it back to its
  caller, so an agent's plan goes to review like any other.
- Commands that change state without a plan (`undo`, `redo`,
  `settings.apply`, `plugins.set_enabled`) run as from the palette,
  unreviewed; which an agent may run is ADR-044's call.
- Socket paths are short (108 bytes Linux, 104 macOS): a deeper runtime
  folder cannot hold the channel, which turning Control on reports
  (`control.status` says why).
- On Windows the pipe keeps default security and trusts its creator;
  `docs/PORTING.md` lists what it needs (Phase 5b).
- Control costs a thread and a runtime when on, nothing when off.
- Protocol revisions arrive with `rmcp` updates.
- Clients show tool names with `__` for dots.
