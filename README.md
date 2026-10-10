# skwir

A file manager that stays fast whatever the folder, light on the machine,
keeps no database of its own, and is driven the same way by people,
scripts and AI agents.

skwir is written in Rust on a GPU-accelerated toolkit (gpui, Zed's).
Version 0.1 is for Linux (Wayland or X11, with Vulkan). Its code for macOS
and Windows is written and compiles, but has not run on either yet: see
[how to help](docs/HELP-WANTED.md).

## What it is like

- **A search is a place.** Type `*.pdf modified<7d size>10M` and the
  results are a location you go to, come back to, and keep in the sidebar.
  Tags are places too: `tags:/Places/France` lists what is tagged so.
- **Changes are plans.** A batch rename, an edit of the listing as text, or
  anything an agent proposes is shown as steps to review before it runs.
  Every operation has an inverse: Undo and Redo reach everything.
- **Nothing runs behind your back.** No daemon, no index of its own. skwir
  reuses what the desktop already has (thumbnails, icons, the type
  database, bookmarks, recent files, installed tools) and keeps one cache
  you can delete at any time.
- **Notes live on the files.** Tags, ratings and comments are kept where
  other applications keep them (extended attributes, XMP sidecars, a
  Mac's files, TagSpaces), and copies keep them.
- **Views over one listing.** List, grid, columns, a tree, a board, two
  panes, tabs, disk usage, a timeline and a map are ways of showing the
  same folder.
- **Everything is a command**, with a JSON schema: the keyboard, the
  palette, the command line and agents (over MCP) all use the same ones.

## Installing

**Packages** for Debian and Ubuntu (`.deb`) and Fedora (`.rpm`) come with
each release, and Arch's are in the AUR. `skwir` is the program with its
PDF, archive and 3D-model helpers; the others are optional:

- `skwir-media`: audio and video, through GStreamer.
- `skwir-poppler`: PDFs through Poppler (under the GPL), beside PDFium.
- `skwir-mermaid`: Mermaid diagrams in Markdown.
- `skwir-mupdf`: PDFs and books laid out in pages through MuPDF (under
  the AGPL).

**From source**, with Rust 1.96 or later and the libraries `flake.nix`
lists (the Nix flake has everything):

```
direnv allow                # or: nix develop
just run ~/Documents        # or: cargo run --release -p skwir -- ~/Documents
```

## First steps

| Keys | What they do |
| --- | --- |
| `Ctrl+Shift+P` | The command palette: every command, with its keys |
| `F1` | The keys that work where you are |
| `Ctrl+L`, `Ctrl+F` | Type a path, or search |
| `Ctrl+1`, `2`, `3` | List, grid, columns |
| `Space`, `Ctrl+Shift+Space` | Peek at a file; the preview panel |
| `F3`, `Tab` | Two panes; go to the other one |
| `Ctrl+T`, `Ctrl+W` | A new tab; close it |
| `F2`, `Ctrl+Shift+N` | Rename; a new folder |
| `Delete`, `Shift+Delete` | Move to the Trash; delete for good |
| `Ctrl+Z`, `Ctrl+Shift+Z` | Undo, redo |
| `Ctrl+D` | Keep this place in the sidebar |
| `Ctrl+,` | Settings |

The [user guide](docs/guide.md) goes through the rest.

## Scripts and agents

Turn on **Agents and scripts** in Settings › Plugins and the open window
listens on a channel only your account reaches. It is off by default,
since any program running as you can then use it.

- **Scripts:** `skwir cmd <command> [<arguments as JSON>]` runs one
  command and prints its result as JSON (`skwir cmd entries` lists what
  the active pane shows). A command that changes files prints its plan;
  `--apply` applies it as your own action, undoable in the window.
- **Agents:** an MCP client starts `skwir mcp` (for Claude Code: `claude
  mcp add skwir -- skwir mcp`). Every command is a tool. What an agent
  would change is proposed in the window for your review; a command that
  acts without a plan waits for you to allow it.

**Private folders** (Settings › Privacy) never leave this machine: an
agent never sees their names nor what they hold, and a plugin from
outside skwir is never handed them. `~/.ssh`, `~/.gnupg` and the like are
private from the start, and so is any folder holding a file named
`.private`.

## Plugins from outside skwir

A plugin can live outside skwir's code: a folder with a `plugin.toml` and
a WebAssembly module, or a program speaking a small protocol of JSON
lines. It provides fields, thumbnails, previews and commands. It stays
off until you turn it on in Settings › Plugins, which says what it may
do, and it gets nothing it did not declare. The protocol is experimental
in 0.1: [writing a plugin](docs/plugins/README.md).

## Documentation

- [User guide](docs/guide.md): views, searching, tags, previews, files,
  settings.
- [Writing a plugin](docs/plugins/README.md) and [its
  protocol](docs/plugins/protocol.md).
- [Contributing](CONTRIBUTING.md), and [`AGENTS.md`](AGENTS.md), the
  rules the code follows, for people and agents.
- [The plan](docs/PLAN.md), its status and what is left;
  [decisions](docs/adr/) that closed a door.
- [Helping with macOS and Windows](docs/HELP-WANTED.md).

## License

skwir is under the Apache License 2.0 ([`LICENSE`](LICENSE)), and so is
what is contributed to it (the licence's section 5). The Poppler and
MuPDF readers are programs of their own that link libraries under the GPL
and the AGPL: distributed, they are under the GPL version 3 and the AGPL
version 3; skwir itself links neither ([ADR-042](docs/adr/042-apache-2.0.md)).
Each package carries the notices of the crates its programs are built
from (`THIRD-PARTY-NOTICES`).
