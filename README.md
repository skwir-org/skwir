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
         alt="Skwir showing a folder as a grid of pictures and documents, the tags of each as colored dots, and a Markdown file read in the preview panel beside it">
  </picture>
</p>

Skwir is written in Rust on gpui, Zed's GPU-accelerated toolkit. Version 0.2 is
for Linux (Wayland or X11, with Vulkan). The macOS and Windows code compiles but
has never run on either system: see [how to help](docs/HELP-WANTED.md).

## Features

- **Fast on any folder**: entries stream in as they are read, however many;
  list, grid, columns, tree and board views; two panes and tabs.
- **Searches are places**: names, kinds, sizes, dates, tags, ratings and
  contents in one query, kept in the sidebar if you like.
- **Tags, ratings and comments** kept on the files, where Dolphin, darktable,
  Lightroom, a Mac's Finder and TagSpaces read them.
- **Previews of almost anything**: code, Markdown, tables, PDFs, DjVu, office
  documents, books, pictures, audio and video, fonts, 3D models, maps,
  archives ([the list](docs/previews.md)).
- **Every change undoable**: copies, moves and renames are plans with
  inverses; changes to many files are reviewed first; a name already taken is
  asked about once.
- **Copies you can trust**: pause and cancel, a USB drive's pace, room checked
  before starting.
- **Views of data**: disk usage, a timeline of changes, a map of where
  pictures were taken.
- **Properties** of anything selected: what a folder holds, the room a file
  takes, who may use it in words (changed and undone like any change), the
  application it opens with, its checksums.
- **Keys, scripts and agents**: every action is a command, from the palette,
  `skwir cmd` or an MCP client, with private folders that never leave the
  machine.
- **Light**: no daemon, no database; the desktop's thumbnails, icons and
  bookmarks; everything beyond a small core is a plugin you can turn off.

## Installing

Each [release](https://github.com/skwir-org/skwir/releases) has packages for
x86-64 Linux. `skwir` holds the program and its PDF, DjVu, archive and
3D-model helpers; the rest is optional:

| Package           | Adds                                                  |
| ----------------- | ----------------------------------------------------- |
| `skwir-media`     | Audio and video, through GStreamer                    |
| `skwir-poppler`   | PDFs through Poppler (GPL), beside PDFium             |
| `skwir-djvulibre` | DjVu through DjVuLibre (GPL), beside djvu-rs          |
| `skwir-mermaid`   | Mermaid diagrams in Markdown                          |
| `skwir-mupdf`     | PDFs and books laid out in pages through MuPDF (AGPL) |

Some previews use programs you may already have, such as LibreOffice or
ffmpeg: [Previews](docs/previews.md) says which.

### Debian 13 and Ubuntu 24.04

Download the `.deb` files for your system from the release: Debian's end in
`-1_amd64.deb`, Ubuntu's in `-1ubuntu24.04_amd64.deb`. Then, in the folder
holding them:

```sh
sudo apt install ./skwir_0.2.0-1_amd64.deb             # Ubuntu: ./skwir_0.2.0-1ubuntu24.04_amd64.deb
sudo apt install ./skwir-media_0.2.0-1_amd64.deb       # optional, and so on
```

### Fedora 44

```sh
sudo dnf install ./skwir-0.2.0-1.x86_64.rpm
sudo dnf install ./skwir-media-0.2.0-1.x86_64.rpm      # optional, and so on
```

### Arch Linux

From the AUR, with an AUR helper or by hand:

```sh
paru -S skwir                                          # or: yay -S skwir
git clone https://aur.archlinux.org/skwir.git && cd skwir && makepkg -si
```

Arch's `skwir` holds the player and the Poppler and DjVuLibre readers;
install `gst-plugins-base`, `poppler-glib` and `djvulibre` to use them. `skwir-mermaid` and
`skwir-mupdf` are packages of their own.

Two other packages stand in for those: `skwir-bin` installs the release's
own build, the tarball below, in `/opt/skwir`, every reader and the player
with it; `skwir-git` builds the latest commit, in the same packages as
`skwir` (`skwir-mermaid-git`, `skwir-mupdf-git`).

### Any other distribution: AppImage or tarball

Both hold the same files, the optional readers and the player among them, and
need x86-64 Linux with glibc 2.35 or later. They were tried on Ubuntu 22.04 and
24.04 and on Fedora 44. The AppImage runs as it is:

```sh
chmod +x skwir-0.2.0-x86_64.AppImage
./skwir-0.2.0-x86_64.AppImage
```

The tarball needs no FUSE and goes where you like, here `~/.local/opt`:

```sh
mkdir -p ~/.local/opt ~/.local/bin
tar -xzf skwir-0.2.0-linux-x86_64.tar.gz -C ~/.local/opt
ln -sfn ~/.local/opt/skwir-0.2.0-linux-x86_64/skwir ~/.local/bin/skwir
```

To list it among your applications, add its desktop entry and icon:

```sh
cd ~/.local/opt/skwir-0.2.0-linux-x86_64/usr/share
mkdir -p ~/.local/share/applications ~/.local/share/icons/hicolor/scalable/apps
sed "s|^Exec=skwir|Exec=\"$HOME/.local/bin/skwir\"|" applications/org.skwir.skwir.desktop \
  > ~/.local/share/applications/org.skwir.skwir.desktop
cp icons/hicolor/scalable/apps/org.skwir.skwir.svg ~/.local/share/icons/hicolor/scalable/apps/
```

A later version goes beside it; run the `ln` line again with its name.

### NixOS and Nix

```sh
nix run github:skwir-org/skwir
```

The flake has a package, an overlay and a Home Manager module. The module
installs skwir and writes the settings you declare, checked by skwir while the
configuration builds; they are linked read-only, or with `mutableSettings`
merged into a writable file at each activation. Skwir keeps the window's state
and what you make in it (saved searches, pinned tags) in files of their own, so
both work
([ADR-056](docs/adr/056-the-window-s-state-is-not-a-setting.md),
[ADR-057](docs/adr/057-what-you-make-is-not-a-setting.md)):

```nix
imports = [ inputs.skwir.homeManagerModules.default ];
programs.skwir = {
  enable = true;
  settings = {
    core.startup = "last";
    plugins.control.enabled = true;
  };
};
```

`mutableSettings` needs a recent Home Manager, one with mutable settings. On
NixOS, the AppImage runs through `appimage-run`.

### From source

With Rust 1.96 or later and the libraries `flake.nix` lists (the Nix flake has
everything):

```sh
direnv allow                # or: nix develop
just run ~/Documents        # or: cargo run --release -p skwir -- ~/Documents
```

Maintainers build and check the packages of a release with
`just release-build v<version>` ([Releasing](docs/RELEASING.md)).

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
see their names or contents, and plugins from outside Skwir are never handed
them. `~/.ssh`, `~/.gnupg` and similar folders are private from the start, as is
any folder holding a file named `.private`.

## Plugins from outside Skwir

A plugin can be a folder with a `plugin.toml` and either a WebAssembly module or
a program speaking JSON lines. It can provide fields, thumbnails, previews and
commands, stays off until you turn it on in Settings › Plugins (which shows what
it may do), and gets nothing it did not declare. The protocol is experimental in
0.2: [writing a plugin](docs/plugins/README.md).

## Documentation

The documentation is online at [skwir.org/docs](https://skwir.org/docs), and in
this repository ([index](docs/README.md)):

- [User guide](docs/guide.md): views, searching, tags, previews, files,
  settings, where things are kept.
- [Previews](docs/previews.md): every kind of file Skwir previews, and what each
  needs installed.
- [Writing a plugin](docs/plugins/README.md) and
  [its protocol](docs/plugins/protocol.md).
- [Contributing](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md), the rules the
  code follows, for people and agents.
- [The plan](docs/PLAN.md): status and what is left; [decisions](docs/adr/) that
  closed a door.
- [Helping with macOS and Windows](docs/HELP-WANTED.md).

## License

Skwir is under the Apache License 2.0 ([`LICENSE`](LICENSE)), and so are
contributions to it (section 5 of the licence). The Poppler, DjVuLibre and
MuPDF readers are separate programs that link libraries under the GPL and the
AGPL; distributed, they are under the GPL version 3 (Poppler's and
DjVuLibre's) and the AGPL version 3 (MuPDF's). Skwir itself links none
([ADR-042](docs/adr/042-apache-2.0.md)). Each package carries the notices of the
crates its programs are built from (`THIRD-PARTY-NOTICES`).
