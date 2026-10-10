# Changes

## 0.1.0 (unreleased)

The first release, for Linux.

- **Browsing**: folders listed as they stream, however large; list, grid,
  columns, tree and board views; two panes and tabs; a sidebar of places
  you arrange, drives mounted and unlocked from it; Recent; the Trash as
  a place, restoring files where they were or where you drop them.
- **Searching**: a search is a place (`Ctrl+F`), its terms as pills;
  names, kinds, sizes, dates in your calendar, tags, ratings, contents,
  positions; kept in the sidebar; narrowed by what its files have.
- **Notes on the files**: tags, ratings and comments kept where the
  desktop, darktable, Lightroom, a Mac and TagSpaces keep them, merged
  when they disagree; categories of tags; tags as places; copies that
  keep them.
- **Changes as plans**: every operation undoable; bulk renames, an edit
  of a folder as text, and what agents and plugins propose reviewed
  before they run.
- **Previews**: text and code, Markdown, tables, structured files, PDFs
  in skwir's reader, office documents, books, comics, e-mails,
  notebooks, pictures, audio and video, fonts, 3D models, circuit boards,
  maps and tracks, archives, and any file's bytes.
- **Views of data**: disk usage, a timeline, a map.
- **A terminal** that follows the folder shown.
- **Agents and scripts** (off by default): `skwir cmd` and `skwir mcp`,
  with the permission layer between them and your files; private
  folders never leave the machine.
- **Plugins from outside skwir**, as WebAssembly modules or isolated
  programs. Their protocol is experimental: it may change before it is
  declared stable.

Known limits: macOS and Windows builds compile but have not run
([how to help](docs/HELP-WANTED.md)); there is no screen reader support
yet (the toolkit has none); Undo lasts while skwir runs.
