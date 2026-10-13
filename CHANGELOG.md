# Changes

## Unreleased

- **Keys** are changed where they are listed: Settings › Keyboard shows
  each action once with all its keys, says what new keys would replace
  before saving them, and binds commands that need a value (a folder to
  go to) through New shortcut. Every action is called by what it does
  (no more four "Preview action" rows). A shortcut of a plugin turned
  off waits for it, where it blocked every later change to the keys.
- **One command each** for hidden files, sorting and opening in a tab:
  `toggle_hidden`, `sort` and `tab.open`, which the shell's
  `ui.toggle_hidden`, `ui.sort` and `ui.open_in_tab` only ran. Scripts
  and agents use the core's names; a shortcut saved with an old name
  keeps working. Ctrl+Enter no longer opens files: only folders and places
  open in a tab.
- **The palette** names each thing once: Copy to…, Move to… and Link
  to… beside Copy, Rename several from a template beside the dialog,
  each store's Read and Write under its name; it no longer lists what
  only answers agents (the timeline's data). It offers what a command's
  forms make of it (Sort by size, Next tab).
- **Properties** is what Details was, and the context menu's last item
  opens it (Alt+Enter, Ctrl+I as before); over a preview it takes at
  most half the panel (beside it when the panel is at the bottom), the
  line between them dragged. Its groups fold (Checksums and the drive
  start folded), and a row's links show under the pointer. It counts
  what a folder holds (at once when opened, else
  on Count), says the room a file takes on its drive, the drive, flags
  such as Immutable, and for several items their kinds, total and shared
  access. It names the application a file opens with, and changes it,
  and gives its SHA-256, MD5 and BLAKE3 when asked (the checksums
  plugin), compared with one copied.
- **Dialogs that ask for text** (Rename, New folder, Rename several, a
  comment, a passphrase) end with Cancel and their verb, as the others
  did: a click answers them as Enter and Escape do. About and F1's
  shortcuts close from a ✕, as Settings does.
- **Archives are made** from the menu of what you select: Compress to
  ZIP, at once; Compress…, whose name's ending picks the format (7z,
  tar.gz, tar.xz, tar.zst…); Compress with a Password… for an encrypted
  ZIP. Extracting asks a ZIP's password, and a name already there asks
  (keep both, replace, skip) rather than failing. With two panes,
  `Shift+F7` asks what the selection does in the other pane, as a drop
  does: extracting there among the choices.
- **Who may use a file** changes from Properties: each class's access in
  words, Allow running, the group, or the mode as chmod takes it, a
  folder's contents too, undoable. Scripts and agents use `set_access`.
  The owner never changes: skwir asks for no administrator rights.

## 0.1.0 (2026-10-13)

The first release, for Linux.

- **Browsing**: folders listed as they stream, however large; list,
  grid, columns, tree and board views; two panes and tabs; a sidebar of
  places you arrange, with drives mounted and unlocked from it; Jump and
  Recent; the shelf; the Trash as a place, restoring files where they
  were or where you drop them. Holding Back or Forward lists the history.
- **Searching**: a search is a place (`Ctrl+F`) with its terms as pills:
  names, kinds, sizes, dates in your calendar, tags, ratings, contents,
  positions. Searches can be kept in the sidebar and narrowed by what
  their files have.
- **Notes on the files**: tags, ratings and comments kept where the
  desktop, darktable, Lightroom, a Mac and TagSpaces keep them, merged
  when they disagree; tag categories; tags as places; copies keep them.
- **Changes as plans**: every operation undoable; bulk renames, editing
  a folder as text, and what agents and plugins propose are reviewed
  before they run.
- **Names already taken**: a copy, a move or a link onto names in its
  destination asks once, before anything is done: keep both (`name
  (copy).ext`), replace (into the Trash), replace if newer, skip; folders
  merge. Agents and scripts say beforehand (`on_conflict`).
- **Reliable copies**: long copies and moves pause and cancel; Quit asks
  while one runs, then undoes what it left half done. Copies onto a USB
  drive run at the drive's pace and report its progress; a copy short of
  room asks before it starts; hard links stay links, and each name of a
  file is listed on its own.
- **Drag and drop** asks whether to move, copy or link (or hold `Shift`,
  `Ctrl`, `Ctrl+Shift`), and offers to extract a dropped archive.
- **Previews**: text and code, Markdown, tables, structured data, PDFs in
  Skwir's reader, office documents, books, comics, e-mail, notebooks,
  pictures, audio and video, fonts, 3D models, circuit boards, maps and
  tracks, archives, and any file's bytes; code in bat's themes, or those
  you added to bat; a player that keeps playing while you browse.
- **Views of data**: disk usage (nested folders, sized by room or file
  count, colored by age or kind, saying what deleting frees); a
  timeline; a map.
- **A terminal** that follows the folder shown.
- **Keys**: every binding can be changed; Vim keys (yazi's, off by
  default) as a scheme.
- **Settings** shared by every Skwir and taken in as the file changes;
  checked by `skwir cmd settings.check`. The window's state and what you
  make (saved searches, pinned tags, tags' colours) are kept in files of
  their own, so the settings file may be read-only.
- **Agents and scripts** (off by default): `skwir cmd` and `skwir mcp`,
  with a permission layer between them and your files; private folders
  never leave the machine.
- **Plugins from outside Skwir**, as WebAssembly modules or isolated
  programs. Their protocol is experimental and may change before it is
  declared stable.
- **Packages** for Debian 13, Ubuntu 24.04 and Fedora 44, Arch's on the
  AUR, an AppImage and a tarball for other x86-64 Linux, and a Nix flake
  with a Home Manager module.

Known limits: the macOS and Windows builds compile but have never run
([how to help](docs/HELP-WANTED.md)); no screen reader support yet (the
toolkit has none); Undo lasts only while Skwir runs.
