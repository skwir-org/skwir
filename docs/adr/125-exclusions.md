# ADR-125: Exclusions: one list of files and folders, each excluded from roles

Status: accepted 2026-11-08 with the user, from issue #6 (a tagged
`~/.config` never showed in its tag's place); built on Linux, macOS and
Windows compiled only. Replaces the private folders' list of ADR-044
(`privacy.folders`, `privacy.marker`, read on as a 0.3.1 file's) and
ADR-092's `extract.exclude` (the Index role); amends ADR-026 (what a
search's walk does not reach).

## Context

- **The dot stood for "too costly to walk", and failed both ways.** Tag
  places, tag counts and the search walk left out every hidden entry (a
  dot, or Windows' mark): a tagged `~/.config` never showed, and Show
  hidden files did nothing in a tag place; yet `node_modules` and cargo's
  `target` were walked. What is costly is what tools manage: on one
  developer's home, 3.66 M of 7.6 M entries sat in dot folders, 2.3 M of
  them in tool stores no cache marker names (the research behind this
  record, 2026-11-08: Go's modules, rustup, Bun, npm, opam, pnpm, coding
  agents' session stores).
- **Privacy was a second list of folders** (ADR-044: `~/.ssh` and its
  like, `.private`), shipped whole into a user's file once edited, as
  Baloo's and Tracker's exclusion lists are, so later defaults never
  reached them. Both lists say "this folder is not for that reader", and
  overlap (`~/.aws`, `~/.docker`, keyrings).
- **What mature lists do** (VS Code's `files.exclude`, GNOME's search
  providers, AdGuard's filters, Gmail's labels, nixpkgs' `attrsOf`): ids
  for what ships, the user's state as what differs from it, a table keyed
  by id that merges key by key (Home Manager's jq `*`, an agent's dotted
  key), a category's own state so what a release adds to it follows the
  user's choice.

## Decision

1. **One list, `exclusions`, of entries in categories; each category
   says which roles its folders are kept from:**

   | Role | Kept from it | Readers |
   |---|---|---|
   | `searches` | never walked below the folder looked in | searches, tag counts, the timeline (`walks::Below`) |
   | `agents` | never named, listed or read for anyone but the user; the path, under it, its target, fail closed | agents, plugins from outside, models (`permissions::Privacy`) |
   | `index` | never harvested | ADR-092's index, not built |

   An entry's folders are kept from a role when its category's switch for
   it (the heading's master switch) and its own tick are both on. Secrets
   are kept from agents and the index, not from the user's own searches.
2. **skwir's own rules ship in code** (`builtin.toml`, ids `skwir.…`, one
   per tool store, from the research: names, store subfolders where files
   people write sit beside them, XDG bases followed), in categories:
   Secrets, Version control, Project environments, Build output,
   Toolchains and packages (by language), Editors and IDEs, Containers and
   sandboxes, Games, AI models and agents, Caches, The system's
   (`~/Library`, `~/AppData`), Yours. Secrets holds, per system, keys and
   key files wherever they are (SSH and PEM private keys, PKCS #12, Java
   and KeePass stores, `.env` and its variants but not `.env.example`,
   Terraform state), certificate stores, tools' sign-ins (cloud, registry,
   AI and database clients), password managers, browsers' and mail
   clients' profiles, wallets, messaging apps and shell histories. What
   people commit and patch beside their project (`vendor`, `Pods`,
   `bower_components`) is in no category: a search of the project finds
   their changes there.
3. **Stored as what changed,** never a copy:
   `exclusions.categories.<id> = { name?, searches?, agents?, index? }` and
   `exclusions.entries.<id> = { pattern | holding, names?, beside?, what?, category?,
   searches?, agents?, index? }`. A built-in's table holds only its
   switches and category (moving one is allowed; Reset takes both back);
   the user's holds their entry. Contributions are `owner.name`; the user's
   ids hold no dot (made from "What it is"); a `skwir.` id naming a folder
   is refused; an override of a built-in a release drops does nothing.
   What a launch cannot take in (another system's, a base unset here, what
   cannot be read) is written back as it stood by any change of the page;
   what cannot be read applies nothing, and is said on the page and in a
   notice leading to it.
4. **An entry is** a pattern: a name (anything so named), a path (`~` for home), a
   glob (`cmake-build-*`, `~/Projects/*/out`; `*` within a name, `**`
   across), `regex:` over the folder's path written from `~`, a name
   `beside` the file that says which tool made it (`target` beside
   `Cargo.toml`: an ordinary word is never left out everywhere), or
   `holding` a marker (a signed `CACHEDIR.TAG`, `.private`). Names and
   paths are hash lookups; globs and regexes one compiled set each.
5. **An exclusion says what it names: files, folders or both** (`names`:
   a shipped pattern names folders unless it says `files`; one of the
   user's both unless it says otherwise). `*.kdbx` and `.env` are files:
   a Python environment named `.env` stays readable. A rule keeps a side
   for each: a walk knows which it found; privacy takes what is above a
   path as a folder and looks at the path itself only where just one side
   names it, and what cannot be looked at stays private. A search leaves
   out the files a rule names too, and says so.
6. **Always, whatever the list says:** a trash and a network mount are
   never walked (code, not entries). Only what is below a walk's root is
   left out: a search or a count of a listed folder walks it, and
   `excluded:yes` (the pill's "Excluded files and folders") walks them
   from anywhere. A search says how many folders it walked past in its
   footer (`ListingEvent::LeftOut`), never silently.
7. **Hidden entries are walked**; whether they show is the pane's switch.
   A search that does not say follows that switch, searched again as it
   moves (`hidden:true` and `hidden:false` say otherwise, and hold).
   A place listing files of many folders marks one inside a hidden folder
   below its root (`Entry::in_hidden_folder`), so a tag's place shows with
   hidden files off what a `tag:` search without `hidden:true` finds; a
   pane with only hidden entries says "Only hidden files here".
8. **Only the user lowers privacy:** a change taking the agents role from
   any folder (an unticked entry, a category switched, a move, a removal,
   a narrower pattern) asks first in the dialog, and is refused outright to
   an agent (`settings.apply`, `exclusions::refuse_exposure`).
9. **One page, Exclusions,** with a role picker (Excluded from: Searches
   and tags | Agents): categories folded when it opens, each heading
   counting what it holds and folded by a click, its switch the master
   switch; rows ticked, dimmed and saying so while their category is off;
   built-ins switch, move and duplicate, the user's are edited and
   removed; the user's own categories; filters (text, Only on this
   computer, Changed). A row's menu is the shared context menu (Move to
   a real submenu). No Undo: a tick undoes itself, and no other settings
   page has one. Privacy opens it on Agents.
10. **A path may start at a variable** (`$CARGO_HOME/registry`,
   `${CARGO_HOME:-~/.cargo}/registry`, the shell's syntax, a default
   nesting another; a list's first folder, where tools write; each one of
   a list a tool reads whole, `KUBECONFIG`), as it may start at `~`. What
   a variable or the home folder holds is never read as a pattern's
   syntax. skwir's
   own entries are written under bases (`under`), each such a path: the
   XDG folders and the tools' homes. A variable is skwir's own
   environment's, else the login shell's (`skwir_core::environment`): a
   skwir started from the desktop does not see what `.zshrc` exports, so
   the shell is asked on a worker (`skwir_platform::shell_environment`,
   with a timeout, its process group stopped whole), again only once its
   files change (their stamps kept with the answer in the cache), and only
   the names the rules use are kept, never the rest of an environment that
   holds tokens. A variable unset with no default names nothing, and says
   so. Where a variable puts a path is part of the rule's fingerprint.
   Off with `exclusions.shell_variables`. Windows has no login shell.
11. **A secret moved or copied is told by what it holds:** a GnuPG
   keyring by its `trustdb.gpg` or `pubring.kbx`, a password store by
   its `.gpg-id`, a browser's profile by its `key4.db` or `Login Data`,
   wherever it is (a backup on a drive), with no variable to read.
12. **A 0.3.1 file's private folders are read among the rules**, under the
   new tables, against 0.3.1's list kept as it was: what it listed that
   skwir does not becomes the user's, in Secrets; what 0.3.1 shipped that
   it no longer listed stays unticked for agents; `.private` off unticks
   its rule. Home Manager's declared old keys keep working.

## Consequences

- A tag's place lists a tagged `~/.config`; Show hidden files decides.
- Tag counts and searches of a home leave out the tool stores of every
  ecosystem researched; what a release adds reaches users who changed the
  list. The index's coverage records the rules' fingerprint: another
  list, and the home counts are made again and every folder counted again.
- The rules are one place to extend: an index, models, thumbnails get a
  role, not a list; a plugin could ship rules under its own id.
- The page is the largest settings editor after the keymap's; its rows
  are drawn as they come into view.
- The rules are made once per settings revision and environment
  generation, for searches and for privacy alike (`App::left_out`,
  `App::privacy`): a catalog costs about 5 ms to make (release build),
  which a check per path paid before. A listed path is found by the
  ancestors of what is asked, and what the listed paths resolve to and the
  mounts are shared by every check for a second (`permissions::Seen`).
- Asking the login shell runs the user's shell files, which may do more
  than export (start an agent, print): rarely, on a worker, with
  `SKWIR_RESOLVING_ENVIRONMENT=1` set for a file that wants to skip
  what only a terminal needs.
- Not built: the Index role's reader, marker files other tools honour for
  agents (`.aiexclude`, `.cursorignore`), rules a plugin ships.
