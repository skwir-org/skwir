# Changes

## Unreleased

- **Code previews use far less memory.** Coloring a TypeScript file took
  over 100 MB, and every language previewed stayed in memory until Skwir
  quit. Code is now colored by Ferroni, and the grammars are released a
  minute after code was last shown. PowerShell, Sass, LiveScript, Salt,
  ARM assembly and Babel's JavaScript are colored too.
- **Two more ways to install Skwir on Arch.** `skwir-bin` installs the
  release's own build, without compiling, every reader and the player
  with it. `skwir-git` builds the latest commit.

## 0.1.0 (2026-10-13)

Skwir 0.1.0 is the first release of an experimental file manager for Linux.
Folders stream into view, searches can be saved as places, and bulk changes are
reviewed before they run. It supports Wayland and X11 and requires Vulkan.

[Installation instructions](README.md#installing)

- **Browsing**: list, grid, columns, tree and board views; two panes and tabs; a
  sidebar you arrange, with drives mounted and unlocked from it; recent folders;
  a shelf for gathering files; the Trash, with files restored to their original
  location or wherever you drop them.
- **Searching** (`Ctrl+F`): names, types, sizes, dates in your local time zone,
  tags, ratings, contents and geographic coordinates. Search terms appear as
  editable pills. Save searches in the sidebar and filter their results by
  metadata.
- **Tags, ratings and comments** kept on the files, using formats shared with
  Dolphin, darktable, Lightroom, Finder and TagSpaces. Notes from different
  stores are merged, tags can be browsed as places, and copies preserve them.
- **Changes as plans**: copies, moves, renames and edits to tags are undoable.
  Bulk renames, editing a folder as text, and changes proposed by agents or
  plugins are reviewed before they run. Irreversible operations ask for
  confirmation.
- **Copies and moves** with progress, pause and cancellation. Space is checked
  before starting, and copies to USB drives report progress at the drive's pace.
  When destination names are taken, choose once: keep both, replace into the
  Trash, replace if newer, or skip. Folders merge, and hard links are preserved.
- **Archives and drag and drop**: choose to move, copy, link or extract. Create
  ZIP, 7z and tar archives from a selection, including encrypted ZIPs.
  Extraction asks for passwords and handles destination conflicts.
- **Properties** (`Alt+Enter`): folder contents counted on demand, space used on
  disk, editable permissions, application associations, and SHA-256, MD5 and
  BLAKE3 checksums you can compare with a copied value.
- **Previews**: text and code, Markdown, tables, PDFs, office documents, books,
  comics, e-mail, pictures, audio and video, fonts, 3D models, circuit boards,
  maps, archives and raw bytes. The player keeps playing while you browse. Some
  previews require optional packages or external tools.
- **Other ways to explore**: disk usage by size, file count, age or type; a
  timeline; a map; and an integrated terminal that follows the folder.
- **Keys and settings**: editable shortcuts with conflict checks, an optional
  Vim scheme based on yazi, and settings shared across running instances and
  reloaded when their file changes.
- **Agents and scripts**, off by default: `skwir cmd` and `skwir mcp`, with
  permissions and review for agent actions. Folders marked private are withheld
  from agents and external plugins.
- **External plugins** as WebAssembly modules or isolated programs. Their
  protocol is experimental and may change.
- **Packages** for Debian 13, Ubuntu 24.04, Fedora 44 and Arch Linux; an
  AppImage and tarball for x86-64 Linux with glibc 2.35 or later; and a Nix
  flake with a Home Manager module.

Known limits: Linux only for this release. The macOS and Windows code compiles,
and Windows tests run under Wine, but neither port has been tested on its native
system ([how to help](docs/HELP-WANTED.md)). Screen readers are not supported
yet. Undo history is cleared when Skwir exits.
