<p align="center">
  <img src="crates/ui/assets/org.skwir.skwir.svg" width="112" height="112" alt="">
</p>

<h1 align="center">skwir</h1>

<p align="center">
  A fast file manager that can be driven equally by humans and AI agents.
</p>

<p align="center">
  <a href="https://skwir.org"><b>Website</b></a> ·
  <a href="https://skwir.org/download"><b>Download</b></a> ·
  <a href="https://skwir.org/docs"><b>Documentation</b></a> ·
  <a href="CHANGELOG.md"><b>Release notes</b></a> ·
  <a href="CONTRIBUTING.md"><b>Contributing</b></a>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/screenshot-dark.png">
    <img src="docs/images/screenshot-light.png" width="100%"
         alt="skwir showing a folder as a grid of pictures and documents, the tags of each as colored dots, and a Markdown file read in the preview panel beside it">
  </picture>
</p>

skwir is written in Rust on gpui, Zed's GPU-accelerated toolkit. Version 0.1 is
for Linux (Wayland or X11, with Vulkan). The macOS and Windows code compiles but
has never run on either system: see [how to help](docs/HELP-WANTED.md).

## What it is like

- **A search is a place.** Type `*.pdf modified<7d size>10M` and the results are
  a location: go back to it, open it in a tab, keep it in the sidebar. Tags are
  places too: `tags:/Places/France`.
- **Changes are plans.** A bulk rename, an edit of the listing as text, or
  anything an agent proposes is shown as steps to review before it runs. Every
  operation has an inverse, so Undo and Redo cover everything.
- **Nothing runs behind your back.** No daemon, no index of its own. skwir
  reuses what the desktop has (thumbnails, icons, the type database, bookmarks,
  recent files, installed tools) and keeps one cache you can delete at any time.
- **Notes live on the files.** Tags, ratings and comments are kept where other
  applications keep them (extended attributes, XMP sidecars, a Mac's files,
  TagSpaces), and copies keep them.
- **Many views of one listing**: list, grid, columns, tree, board, two panes,
  tabs, disk usage, a timeline and a map.
- **Everything is a command** with a JSON schema. The keyboard, the palette, the
  command line and agents (over MCP) all use the same ones.

## Installing

**Packages** for Debian and Ubuntu (`.deb`) and Fedora (`.rpm`) come with each
release; Arch's are in the AUR. The `skwir` package holds the program and its
PDF, archive and 3D-model helpers. The rest is optional:

| Package         | Adds                                                  |
| --------------- | ----------------------------------------------------- |
| `skwir-media`   | Audio and video, through GStreamer                    |
| `skwir-poppler` | PDFs through Poppler (GPL), beside PDFium             |
| `skwir-mermaid` | Mermaid diagrams in Markdown                          |
| `skwir-mupdf`   | PDFs and books laid out in pages through MuPDF (AGPL) |

On Arch, `skwir` includes the player and the Poppler reader, with their
libraries as optional dependencies.

**From source**, with Rust 1.96 or later and the libraries `flake.nix` lists
(the Nix flake has everything):

```
direnv allow                # or: nix develop
just run ~/Documents        # or: cargo run --release -p skwir -- ~/Documents
```

## First steps

| Keys                     | Action                                        |
| ------------------------ | --------------------------------------------- |
| `Ctrl+Shift+P`           | Command palette: every command, with its keys |
| `F1`                     | The keys that work where you are              |
| `Ctrl+L`, `Ctrl+F`       | Type a path; search                           |
| `Ctrl+1`, `2`, `3`       | List, grid, columns                           |
| `Space`, `F11`           | Peek at a file; the preview panel             |
| `F3`, `Tab`              | Split into two panes; switch pane             |
| `Ctrl+T`, `Ctrl+W`       | New tab; close tab                            |
| `F2`, `Ctrl+Shift+N`     | Rename; new folder                            |
| `Delete`, `Shift+Delete` | Move to Trash; delete for good                |
| `Ctrl+Z`, `Ctrl+Shift+Z` | Undo; redo                                    |
| `Ctrl+D`                 | Keep this place in the sidebar                |
| `Ctrl+,`                 | Settings                                      |

The [user guide](docs/guide.md) covers the rest.

## Scripts and agents

Turn on **Agents and scripts** in Settings › Plugins. The open window then
listens on a channel only your user account can reach. It is off by default
because any program running as you can use it.

- **Scripts:** `skwir cmd [--apply] <command> [<JSON arguments>]` runs one
  command and prints its result as JSON (`skwir cmd entries` lists what the
  active pane shows). A command that changes files prints its plan; `--apply`
  applies it as your own action, undoable in the window.
- **Agents:** an MCP client starts `skwir mcp` (Claude Code:
  `claude mcp add skwir -- skwir mcp`). Every command is a tool. Changes an
  agent wants are proposed in the window for review; a command that acts without
  a plan waits until you allow it.

**Private folders** (Settings › Privacy) never leave the machine: agents never
see their names or contents, and plugins from outside skwir are never handed
them. `~/.ssh`, `~/.gnupg` and similar folders are private from the start, as is
any folder holding a file named `.private`.

## Plugins from outside skwir

A plugin can be a folder with a `plugin.toml` and either a WebAssembly module or
a program speaking JSON lines. It can provide fields, thumbnails, previews and
commands, stays off until you turn it on in Settings › Plugins (which shows what
it may do), and gets nothing it did not declare. The protocol is experimental in
0.1: [writing a plugin](docs/plugins/README.md).

## Documentation

The documentation is online at [skwir.org/docs](https://skwir.org/docs), and in
this repository ([index](docs/README.md)):

- [User guide](docs/guide.md): views, searching, tags, previews, files,
  settings, where things are kept.
- [Previews](docs/previews.md): every kind of file skwir previews, and what each
  needs installed.
- [Writing a plugin](docs/plugins/README.md) and
  [its protocol](docs/plugins/protocol.md).
- [Contributing](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md), the rules the
  code follows, for people and agents.
- [The plan](docs/PLAN.md): status and what is left; [decisions](docs/adr/) that
  closed a door.
- [Helping with macOS and Windows](docs/HELP-WANTED.md).

## License

skwir is under the Apache License 2.0 ([`LICENSE`](LICENSE)), and so are
contributions to it (section 5 of the licence). The Poppler and MuPDF readers
are separate programs that link libraries under the GPL and the AGPL;
distributed, they are under the GPL version 3 and the AGPL version 3
respectively. skwir itself links neither
([ADR-042](docs/adr/042-apache-2.0.md)). Each package carries the notices of the
crates its programs are built from (`THIRD-PARTY-NOTICES`).
