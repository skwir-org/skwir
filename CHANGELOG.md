# Changes

## Unreleased

- **Keys** are changed where they are listed: Settings › Keyboard shows
  each action once with all its keys, says what new keys would replace
  before saving them, and binds commands that need a value (a folder to
  go to) through New shortcut. Every action is called by what it does
  (no more four "Preview action" rows). A shortcut of a plugin turned
  off waits for it, where it blocked every later change to the keys.

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
