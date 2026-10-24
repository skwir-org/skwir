# Command invariants: commands, agents, plugins from outside

Read this before adding a command, or changing the `control` plugin, the
permission layer, `skwir-plugin-api` or `skwir-extend`. It is binding,
like `AGENTS.md`. ADR-043 (agents and scripts), ADR-044 (permissions),
ADR-045 to ADR-047 (plugins from outside) hold the reasoning.

## Commands are the one surface

- Every user action is a command (`skwir_core::command`), shared by the
  keymap, the palette, the CLI, the MCP server and agents.
- The shell's own commands (`ui.*`) are declared in the registry like
  the rest (`commands.rs`, owner `ui`: a title, what it does, the schema
  of its arguments) and carried out by the shell. A plugin may bind keys
  to them.
- Every command is checked before it runs (`CommandRegistry::check`): an
  unknown name or an argument the command does not take is refused, never
  guessed. A user's shortcut is checked against the declarations when a
  save adds or changes it; one stored unchanged whose command nothing
  registers waits for its plugin, bound to nothing (ADR-024).
- **Forms (ADR-024).** Arguments that make a command one thing to do are
  declared with its title (`Declared::forms`: `sort {"key": "size"}` is
  Sort by size), and every list of actions calls it so
  (`keymap::title`). A key that runs a command with arguments has a form
  for them, where the command is declared;
  `crates/skwir/tests/keymap_titles.rs` holds it for every built-in key
  and that no place lists two actions under one title. Only a title
  naming what an argument names elsewhere (a folder, a view, a choice's
  own title) is worked out instead.
- **Repeats (ADR-066).** A command runs once for each press of its key
  unless it declares when a key held down runs it again, by its
  arguments (`Declared::repeats`; `command::always` for one that only
  steps): a step along something, never an act or a switch. The rule is
  not said to agents: only keys repeat. A plugin from outside cannot
  declare one yet.
- A new `ui.*` command is declared in `commands.rs`; the test of the
  declarations covers its keys, forms and palette entries. The palette
  offers the shell's commands as `UI_COMMANDS` lists them, each with the
  arguments that make it one thing to do; those and the keys' own
  (`KEY_FORMS`) are the commands' forms.
- **One thing, one command, one title.** The palette (`palette::offered`)
  offers each core and plugin command as it is and each of its forms
  (a sort only by what a column shows there, as the View menu offers
  it), never what is data, and a setting by its page, section and title.
  `keymap_titles.rs` holds that no two of its entries read alike. A
  shell command that only ran a core one for the pane is that core
  command instead (`toggle_hidden`, `sort`, `tab.open`): a pane's button
  names its pane, a key acts on the active one.
- A `ui.*` command dispatched from elsewhere (an agent, a script) waits
  for the window (`App::request_front`).
- Mutating commands return a `Plan` ([operations.md](operations.md)). No
  command applies a plan it makes or is given (`apply_plan` hands it
  back): the caller decides.
- **Effects (ADR-044).** A command says what it does when registered
  (`command::Effect`: `register(..).reads()`, `.shows()`, `.plans()`,
  `.user_only()`); this decides what an agent may run. A command that says
  nothing acts, and an agent's call of it asks the user first. A window
  command (`ui.*`) is the user's alone unless `commands.rs` lists it among
  those that show or act: one that answers a dialog or presses for the
  user must never be an agent's. The player's `player.request` (play,
  pause, seek, the volume) shows: sound from the window is the window's,
  so an agent plays and sets the volume unasked, as the media keys do
  (decided 2026-10-24).
- **Data.** A command that only answers whoever asks (an agent's
  question, a menu's items, Properties' rows, the timeline's bars) is
  registered `.data()`: it reads, and run by a person it would show
  nothing, so the palette and the Keyboard page leave it out. One that
  changes nothing yet shows its answer from the window (Copy paths, a
  plugin from outside's command, whose answer the window says) is
  `.reads()` and offered. `keymap_titles.rs` names those, so a new
  command that reads says which it is.

## Agents and scripts drive the window (ADR-043)

- The `control` plugin, off by default, listens on a channel only the
  user reaches (`channel::address`). `skwir mcp` carries an agent's MCP
  there, `skwir cmd` one command (`crates/skwir/src/cli.rs`).
- Every command is a tool, its dots written `__`: a new command is a new
  tool with nothing more.
- An agent's plan is proposed, and its tool answers with the verdict
  (`App::propose_awaiting`). `skwir cmd` prints a plan, or applies it with
  `--apply` as the user's own action.
- Without a window (`skwir cmd` with none open) a skwir of its own runs
  the command and ends once it answers, so a command answers once its
  work is done (saved, applied). What it hands a worker on the window's
  thread (`scheduler::on_window_thread`; `has_window` where the work may
  stop the connection asking, as `settings.apply` and
  `plugins.set_enabled` do) it does where it is asked elsewhere; anything
  left queued is cancelled with the app.
- What an agent needs to see is a command answering a value (`entries`,
  `tab.list`), never a protocol of its own. `entries` with a `location`
  lists a folder without showing it, so reading never moves the user's
  view; asked off the window's thread, it waits a moment for a listing
  still arriving.
- A command that acts on entries takes them in `entries` (`op::EntryArg`:
  an absolute path, or identity and path as the window names what it
  shows) and reads them through `App::targets`, which falls back to the
  pane's selection when none is named. Scripts and agents name files,
  never select them: the selection is the user's and changes between two
  calls. Explicit `open` and `open_with` targets remain names on the
  window thread: workers resolve and type them in their filesystem lanes.
  Capture the pane and its location lifetime before queueing; navigation
  cancels pending opening, and worker failures become notifications. No
  targets open until all named targets have resolved successfully.
- A command that makes something somewhere takes the folder too (`mkdir`'s
  `in`).
- A command whose plan copies, moves or links takes what becomes of a
  name taken there (`on_conflict`, ADR-054), carried by the plan: a script
  never answers a dialog, and without it a plan applied with no window
  up applies nothing and says what is taken.
- A path given from outside the window is absolute
  (`os_path::absolute`), never read against the window's working folder.
- What a script's or an agent's command changes shows at once: it runs off
  the window's thread, where nothing wakes the window, so the channel
  wakes it (`App::wake`) after any command that does more than read
  (`control::dispatched`). A `ui.*` command reaches the window through
  `App::request_front`, which wakes it too.

## The permission layer (ADR-044)

An agent's call goes through `permissions::for_agent`, enforced in the
control plugin's `run`:

- Nothing it names may be private.
- A command that acts runs once the user allows it in the review queue
  (`App::ask`, a `Review::Question`). A user-only command is refused and
  not listed.
- Its plans say "An agent proposes" (`Plan::by`).
- Every private path is removed from what it is answered
  (`Checker::redact`).
- Redaction removes what an answer names, not what it counted. So the
  call runs as an agent's (`permissions::as_agent`), where the selection a
  command is handed (`App::targets`, `selected_refs`) has no private file,
  and a command that answers what it counted or gathered of a pane's
  entries (how many, their tags, dates, places) leaves out
  `App::withheld(pane)`, asked before taking the workspace's lock.
- A window command that shows does nothing beyond the window for an
  agent, whatever its arguments: what one would (`ui.inspect`'s copy, a
  link to an address, a label typed) is refused where the call reaches
  the window (`commands::reaches_out`), naming the command that does it
  and asks (`ui.preview_copy`, `ui.open_url`). The window runs what it is
  handed as the user's.
- A location an agent's call puts in a dormant pane (a tab opened
  behind) is opened as that call when its tab is shown
  (`Pane::asked_by`): what asks before an agent's open (a remote's
  connection) asks then too.
- A script (`skwir cmd`) is the user's own.
- **The endpoint determines trust (ADR-085).** The agent endpoint always
  proposes and enforces the permission layer, regardless of MCP metadata.
  `skwir cmd` connects to the separate script endpoint; only there may
  `skwir/plan` choose print or apply. Never infer caller authority from
  payload fields, including `_meta`.

## What leaves this machine

- Every plugin declares what it may do (`Plugin::capabilities`: read what
  files hold, write, network, run programs), truthfully for a built-in
  one; the Plugins page shows it. A new plugin declares its own.
- Private folders (`permissions::Privacy`, the Privacy page: `~/.ssh` and
  its like, and any folder holding a `.private` file) never reach an agent
  or a plugin from outside. Check with `App::privacy()` wherever such code
  is handed a file; it resolves links and reads markers, so call it off
  the window's thread. Check both the given name and its resolved target,
  including aliases of configured private folders and marked ancestors.
  For a new name resolve its existing parent; a dangling link or another
  resolution failure is withheld, never treated as evidence of a public
  path. Taking a `Privacy` snapshot itself does no filesystem work. What
  it reads of a path on a remote (by its name, or through a link on the
  way into one) it reads in the remote's lane, waiting
  `reach::NOT_ANSWERING` at most: past that the path is private and that
  server is not asked again by the same checker (`App::privacy` gives it
  the app's workers; a `Privacy` without them reads where it is asked).
- Only `map-tiles` makes requests of its own (`AGENTS.md`, "Engineering
  rules"); the `remote` plugin reaches servers through the tools it starts
  (ADR-077), and connects an address an agent opens only once the user
  allowed it (`Asker::Connection`): the server is told who the user is.

## Plugins from outside (ADR-045, ADR-046, ADR-047)

- A plugin from outside never takes one of skwir's own namespaces as its
  id (`manifest::RESERVED`, kept whole by `crates/skwir/tests/outside_ids.rs`),
  nor a command another owner registered (`register_dynamic`).
- A bundled helper's sandbox can read the package's private runtime,
  read-only (ADR-052). Find that runtime from Skwir's own executable and
  its package marker, never from a plugin's enclosing folders. Bind only
  the package's `usr` tree, after creating the private `/tmp`; an
  AppImage mounted there must remain visible. This grants no additional
  folders to programs outside that runtime.
- `skwir-plugin-api` is the contract: the manifest, the protocol, the WIT
  world, the guest's `Plugin` trait. It depends on nothing of Skwir's and
  builds for WebAssembly.
- `skwir-extend` is the host. It finds plugin folders (`folders()`,
  `SKWIR_PLUGIN_PATH`) and makes each an `External` plugin, off until
  turned on. It runs one as a program (isolated, `sandbox::plugin`) or as
  a module, which the helper `skwir-module-worker` runs and serves as such
  a program (Wasmtime, no WASI, only Skwir's two imports). Wasmtime never
  enters Skwir's own process.
- The module helper works in the plugin's cache folder and keeps the
  compiled module there (`--keep`), the only folder it writes.
- A plugin reads only the files of the call it answers, by handle
  (`call::Call`), and only with `read`. A plan needs `write` and is
  reviewed when run from the window.
- What a plugin from outside can provide is the protocol's: fields,
  thumbnails, previews, commands. A new kind is a new method and a new
  protocol version, on both transports at once, documented in
  `docs/plugins/protocol.md`.
- The protocol is experimental (ADR-047): a version may change what an
  older one said until it is declared stable, `OLDEST` following it, and
  the release notes say so. Once stable, Skwir keeps speaking older
  versions.
- A preview from outside is one of:
  - an inspection (text, code, hex, a table);
  - a file Skwir shows, made of the file (`convert`,
    `PreviewProvider::converted`, swapped in once in the panel's worker);
  - pages with their text, links and outline, drawn as read (`page`, a
    `DocumentSession` in `pages.rs` that Skwir's reader reads).

  Never Skwir's own preview model on the wire. Links in what a plugin made
  are closed unless the plugin may use the network.
- `just plugins` builds the workspace's modules
  (`crates/plugins-outside/*`) into `target/plugins`.
- A command that may wait (a plugin from outside answering): see
  `AGENTS.md`, "Background work and streamed state".
