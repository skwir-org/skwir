# ADR-043: Agents and scripts drive the running window

Status: accepted, 2026-10-07, as the user chose (Phase 6, its first
step). Phase 6 begins before Phase 5b's gate, which needs a Mac and a
Windows machine (PLAN.md records the deviation).

## Context

Phase 6 gives bibli a command line and an MCP server, both "thin
consumers of the command registry", so that an external agent can drive
bibli and finish with a plan the user reviews. The pieces exist: every
action is a command with a JSON schema (`CommandRegistry::list`,
`check`, `dispatch`); a command that changes files returns a `Plan`; a
plan from anything but the user's own gesture goes to review
(`App::propose`), shown in the window and applied with undo; a window's
own command asked from elsewhere waits for the window
(`App::request_front`). No process reaches another today: each `bibli`
builds its core and plugins and runs its window.

An agent or a script could run a bibli of its own, without a window, or
reach the window the user has open. A plan is reviewed in the window,
and applied there into the window's undo history; a bibli without a
window can show no plan, and would keep an undo history and settings
writes of its own beside the window's.

## Decision

1. **Agents and scripts drive the window.** A plugin, `control`, off by
   default, listens while it is on and the window runs, on a channel
   only the user reaches: a Unix socket in a folder of the user's alone
   (`$XDG_RUNTIME_DIR/bibli/control.sock` on Linux, under the user's
   `$TMPDIR` on macOS), a named pipe on Windows. No daemon: the channel
   ends with the window, and with the plugin turned off. A second bibli
   finds it taken and does not listen.
2. **It speaks MCP through `rmcp`**, the protocol's official Rust SDK,
   on a runtime of the plugin's own, on a thread started when the plugin
   is turned on and ended when it is turned off. Tokio is already linked
   (gpui's HTTP client, the map tiles), so `rmcp` adds itself alone. The
   core stays synchronous; a part that needs async owns its runtime, and
   one shared by several plugins would be a utility of
   `bibli-platform`, not the core's (as ADR-021 says of an HTTP client).
3. **The tools are the commands.** Each command is a tool, with its
   declared description and argument schema; its dots are written `__`
   (`tags.edit` is `tags__edit`), which MCP clients accept and which
   reads back exactly. A command of the window's own (`ui.*`) is asked
   of it (`request_front`). A command that gives a plan proposes it for
   review, and the tool answers once the user has: applied, with what
   applied and any error, or dismissed; or after a while, the plan still
   waiting. A value or a text for the clipboard is the tool's answer.
4. **`bibli mcp`** is what an agent's configuration starts: it carries
   the agent's standard input and output to the window's channel. With
   no window listening it says to open bibli and turn Control on, and
   starts nothing.
5. **`bibli cmd <name> [arguments]`** runs one command through the same
   channel and prints its result as JSON. A plan is printed, or applied
   with `--apply`: run by the user, it is the user's own action, as a
   key is, applied whole and undoable in the window. The request says
   so in its `_meta` (`bibli/plan`: `print` or `apply`; without it, a
   plan is proposed), which an agent's model, filling arguments, never
   sets. With no window listening, a command runs in a
   bibli of its own without a window: one that reads answers, a plan is
   printed, or applied with `--apply`.
6. **Off until the permission layer.** A program running as the user
   can use the channel, as it can the user's files, and an agent reads
   what the commands read. Control stays off by default until the
   permission layer (Phase 6, next) keeps folders such as `~/.ssh` from
   an agent.

## Consequences

- The window answers a proposal it is given: a plan proposed with a
  reply (`App::propose_awaiting`) learns that it was dismissed, or how
  it applied, from the executor when its run finishes.
- A script or an agent names the files a command acts on, in its
  `entries`, by absolute path; the window's surfaces name what they
  show by identity and path. The pane's selection is what is acted on
  only when nothing is named: it is the user's, and can change between
  an agent's two calls. A relative path is refused, from any surface:
  the window's working folder is not the caller's.
- No command applies a plan of its own: `apply_plan` gives the plan it
  is handed back to its caller, as every command that changes files
  does, so an agent's goes to review like any other.
- Commands that change something without a plan (`undo`, `redo`,
  `settings.apply`, `plugins.set_enabled`) run as they would from the
  palette: an agent's are not reviewed. Which of them an agent may run
  is the permission layer's to say.
- A Unix socket's path is short (108 bytes on Linux, 104 on macOS): a
  runtime folder deeper than that cannot hold the channel, and turning
  Control on says so in the window (`control.status` says why too).
- On Windows the pipe keeps the system's default security and trusts
  whoever made it: `docs/PORTING.md` lists what it needs, left for
  Phase 5b's gate.
- Turning Control on costs a thread and a runtime; off, nothing.
- The protocol's revisions come with `rmcp` updates.
- A tool's name differs from the command's by its dots; a client that
  shows tools shows `__`.
