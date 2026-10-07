# ADR-044: The permission layer: what a plugin may do, what an agent may run, what never leaves

Status: accepted, 2026-10-07 (Phase 6, its second step). Amends ADR-043:
an agent's call goes through it.

## Context

Phase 6 opens bibli to code it does not ship: agents driving the window
(ADR-043), plugins from outside as programs (ADR-045) and WebAssembly
modules (ADR-046). The plan asks for a permission layer before any model
provider exists: capabilities per plugin (read contents, write, network,
spawn processes) and per-folder policies, such as "never leaves this
machine" for `~/.ssh` or anything under a `.private` marker.

Three kinds of code act for the user here, and the layer can hold each
of them to a different degree:

- A plugin built into bibli is native code in bibli's process. Nothing
  can confine it; what it does can only be said.
- A plugin from outside reaches nothing but what its host hands it (a
  program runs isolated, a module has bibli's imports alone), so what it
  declares can be enforced.
- An agent reaches bibli only through the window's channel, one command
  at a time, so every call can be checked before it runs and every
  answer before it leaves.

## Decision

1. **Capabilities.** A plugin says what it may do beyond naming and
   listing files (`Plugin::capabilities`): `read` what files hold,
   `write` (change files, through a plan), `network`, `programs`. The
   Plugins page says it on the plugin's line, with where a plugin from
   outside comes from (`Plugin::origin`). A built-in plugin declares it
   truthfully; that is all its line can be. A plugin from outside gets
   what its manifest declares once the user turns it on (it is off
   until then), and its host enforces it: a read without `read` is
   refused, a plan without `write` is refused, a program without
   `network` has no network, a module has none at all.
2. **Private folders** (`permissions::Privacy`, the Privacy page,
   `privacy.*`): files that never leave this machine. They are the
   folders listed (by default where keys and credentials live: `~/.ssh`,
   `~/.gnupg`, `~/.password-store`, the keyrings, `~/.pki`, `~/.aws`,
   `~/.kube`, `~/.docker`, `~/.netrc`), and any folder holding a
   `.private` file, each with everything under it. An agent never names
   one (its call is refused), never sees one (what it is answered loses
   every object whose `path` is private and every string that names one,
   `Checker::redact`), and a plugin from outside is never handed one
   (its provider answers nothing, its command is refused). A built-in
   plugin is bibli: the folders are its to show the user.
3. **What a command does** (`command::Effect`, declared at registration
   with `register(..).reads()` and so on): it `reads`, `shows` (changes
   only what the window shows), `plans` (changes files only through a
   plan), `acts` (anything else: a preference, the sidebar, the
   clipboard, a program started, an undo), or is `user_only` (it answers
   what the window asked the user, or presses for the user's hand: a
   review's Apply, a menu's item, the focused button). A command that
   says nothing acts. The window's own commands (`ui.*`) are user only
   unless listed as showing or acting (`commands.rs`).
4. **An agent's call** (`permissions::for_agent`, enforced by the control
   plugin): one that reads, shows or plans runs (a plan is proposed for
   review, as before); one that acts runs once the user allows it, in
   the window, in the review's queue ("An agent asks to run …", Allow or
   Refuse: `App::ask`); one that is user only is refused and not listed
   among its tools. A script (`bibli cmd`) is the user's own: it is held
   to none of this.

## Consequences

- An agent cannot apply its own proposal, answer a question it caused,
  or press a button: those are user only. It cannot undo, change a
  setting, turn a plugin off or start a program without the user's
  Allow. The tool list says which tools ask first, and marks those that
  change nothing as read-only.
- Redaction is by what an answer names, at one place (the channel), so
  a new command needs nothing to keep private folders from agents. A
  title that names a folder found private only by its marker is not
  recognised (the marker is checked for paths, not inside text).
- A new command that forgets to say what it does asks the user first: a
  bother, never a leak. A new window command that forgets is refused to
  agents.
- Model providers (Phase 7) take the same rules: they are code from
  outside and see no private file.
- A built-in plugin's capabilities are its word; the test of the word is
  review, not the host.
- Control (ADR-043) stays off by default all the same: an agent driving
  the window is the user's choice, now made with this layer between the
  agent and their files.
