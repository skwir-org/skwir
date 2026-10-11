# ADR-044: The permission layer: what a plugin may do, what an agent may run, what never leaves

Status: accepted, 2026-10-07 (Phase 6, second step); point 5 added the
same day. Amends ADR-043: agent calls go through it. Built.

## Context

Phase 6 opens Skwir to code it does not ship: agents (ADR-043), plugin
programs (ADR-045) and modules (ADR-046). The plan asks for a permission
layer before any model provider: capabilities per plugin (read
contents, write, network, spawn processes) and per-folder policies
("never leaves this machine" for `~/.ssh` or a `.private` marker).
Each kind of code can be held to a different degree:

| Code | Reaches | So the layer can |
|---|---|---|
| Built-in plugin | everything (native, in-process) | only state what it does |
| Plugin from outside | only what its host hands it | enforce its declaration |
| Agent | the window's channel, one command at a time | check each call and each answer |

## Decision

1. **Capabilities** (`Plugin::capabilities`): `read` file contents,
   `write` (through a plan), `network`, `programs`, shown on the Plugins
   page with an outside plugin's origin (`Plugin::origin`). A built-in
   plugin declares truthfully. An outside plugin, off until turned on,
   gets what its manifest declares, enforced: a read without `read` or
   a plan without `write` is refused; a program without `network` has
   none; a module never has it.
2. **Private folders** (`permissions::Privacy`, Privacy page,
   `privacy.*`) never leave this machine: the listed folders (default
   `permissions::DEFAULT_FOLDERS`: `~/.ssh`, `~/.gnupg`,
   `~/.password-store`, the keyrings, `~/.pki`, `~/.aws`, `~/.kube`,
   `~/.docker`, `~/.netrc`) and any folder holding a `.private` file,
   with everything under them.
   - An agent's call naming one is refused; its answers lose every
     object whose `path` is private and every string naming one
     (`Checker::redact`).
   - An outside plugin is never handed one (provider answers nothing,
     command refused).
   - Built-in plugins are Skwir: they show these folders to the user.
3. **Each command declares its effect** at registration
   (`command::Effect`, `register(..).reads()`…): `reads`; `shows`
   (changes only what the window shows); `plans` (changes files only by
   plan); `acts` (anything else: a preference, the sidebar, the
   clipboard, a program, an undo); `user_only` (answers what the window
   asked the user, or presses for them: a review's Apply, a menu item,
   the focused button). Undeclared means `acts`. `ui.*` commands are
   user only unless `commands.rs` lists them as showing or acting.
4. **An agent's call** (`permissions::for_agent`, enforced by the
   control plugin): reads, shows and plans run (plans go to review);
   acts runs once the user allows it in the review queue ("An agent
   asks to run …", Allow or Refuse: `App::ask`); user only is refused
   and not listed. A script (`skwir cmd`) is the user's own and exempt.
5. **What an agent counts** (2026-10-07, after the first MCP trial).
   Redaction removed names, not counts: a content search answered
   `total: 1` with the private match removed, so an agent could probe a
   private file word by word. Now an agent's call runs as an agent's
   (`permissions::as_agent`, a scope on its thread): its selection
   (`App::targets`, `selected_refs`) holds no private file, and commands
   that count or gather a pane's entries (`entries`, `tags.list`, the
   timeline's histogram, the map's points, bulk rename's preview) leave
   out `App::withheld(pane)`, as if the window had no private files.

## Consequences

- An agent cannot apply its own proposal, answer a question it caused
  or press a button. Without the user's Allow it cannot undo, change a
  setting, turn a plugin off or start a program. Its tool list marks
  which tools ask first and which are read-only.
- Redaction happens in one place (the channel), by what an answer
  names, so new commands need nothing to hide private names. Gaps:
  - a title naming a folder private only by marker is not caught
    (markers are checked on paths, not in text);
  - a new command that counts a pane's entries must ask
    `App::withheld`; forgetting leaks how many private files exist, not
    their names.
- Background totals over a folder (the timeline's subtree histogram,
  Usage's sizes) include private folders for whoever asks: an agent can
  learn how much they hold and when it changed, never names or
  contents.
- A command that forgets to declare its effect makes agents ask first
  (a bother, not a leak); a window command that forgets is refused to
  agents.
- Model providers (Phase 7) are code from outside: same rules, no
  private file.
- A built-in plugin's capabilities are its word, tested by review, not
  the host.
- Control (ADR-043) stays off by default: an agent driving the window
  is the user's choice, now made with this layer in between.
