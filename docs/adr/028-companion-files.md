# ADR-028: Files beside a file go where it goes

Status: accepted, 2026-09-30. Amends ADR-006.

## Context

Some notes are kept in files beside the file they describe (ADR-027): a
Mac's `._x`, XMP's `x.ext.xmp` or `x.xmp`, TagSpaces' `.ts/x.json` (and
its thumbnail, `.ts/x.ext.jpg`). Only the name ties them to their file.
Today bibli knows them in three unrelated places and nowhere else:

- the tags plugin reads a folder a second time to find the `._` files of
  the files it fills (`attrs::AsideNames`), though the listing has every
  name already;
- the folder listing reads `x` again when `._x` changes, a rule written
  for `._` alone (`path_provider.rs`);
- the operations (rename, move, copy, trash, delete) do not know them:
  renaming `x` in bibli leaves `._x` behind, pointing at nothing, and the
  Mac loses the file's tags.

Copying to a volume without attributes loses the notes (or, with
attributes kept, fails: ADR-006 chose "no silent metadata loss or sidecar
fallback"). A Mac does the opposite: it writes the attributes to `._x`
on such a volume, and `dot_clean` puts them back as attributes on one
that has them. Seamless notes on USB drives need bibli to do the same.

## Decision

### Companions, declared once, in the core

A **companion** is a file that belongs to another: it is described by
the store that uses it (ADR-027), in a registry of the core, because the
listing and the operations, both the core's, must agree on it. A store
declares, as pure functions of names (no I/O):

- for a file's name, the names its companions would have in its folder
  (`._x`, `x.ext.xmp`, `.ts/x.json`, `.ts/x.ext.jpg`);
- for a name, the file it would belong to (`._x` → `x`), so a changed or
  listed companion finds its file;
- how a companion is named for a file renamed `y` (`._y`, `y.ext.xmp`),
  and whether it may follow at all: `x.xmp` (Adobe's name, the extension
  replaced) follows only when no other file shares its stem, as
  `IMG_1.CR3` and `IMG_1.JPG` both claim `IMG_1.xmp`
  (`photoshop:SidecarForExtension` says which, when present).

### The listing knows them

A folder listing knows every name, so it pairs files with their
companions as it lists, and changes as they change:

- providers are given the folder's names with each batch, so a store
  finds its companions without reading the folder again (this replaces
  `AsideNames`);
- a companion whose file is there is not shown, unless the pane shows
  hidden files and `core.show_companions` (General, "Show companion
  files", off by default) asks for them too: someone showing hidden files
  looks for configuration, not a `._` twin of every photo. The core
  hides them where it hides hidden files (`SortSpec`, `Pane::order`); the
  stores only name them, so with no store enabled nothing is hidden. The
  View menu offers the same setting under Hidden Files while those are
  shown, where one looks for a file that seems missing. Details lists
  the selected file's companions either way (where its notes are kept).
  One whose file is gone (an orphan) is always listed, as a hidden file:
  it is what needs attention, and Details says what it belonged to;
- a change to a companion is a change to its file's notes: the file is
  read again (this generalizes the `._` rule of `path_provider.rs`).

### Operations carry them

Rename, move, copy, swap, trash, restore and delete act on a file and its
companions in the same step, staged together (ADR-006's staging), and
their inverse takes all of them back. A companion's name follows its
file's; a companion already at the target is a conflict, as a file
already there is. Trashing trashes them with their file, each an item of
its own (the freedesktop trash has no groups), which the trash's listing
hides beside its file like any companion; restoring the file restores
them. Deleting deletes them.

### A copy converts notes to what the destination keeps

A copy or a cross-device move reads the file's notes through the stores
and writes them at the destination by its policy (ADR-027), not only the
raw `user.*` attributes:

- onto a volume without attributes: into the store it creates there,
  AppleDouble by default, which holds every `user.*` attribute, notes and
  others alike, so nothing is dropped;
- from `._x` onto a volume with attributes: as attributes, and `._x` is
  not copied (what `dot_clean` does);
- XMP and TagSpaces sidecars are copied as companions, and updated only
  if the user writes to those stores.

ADR-006 is amended: a destination that refuses attributes no longer
fails a copy that keeps them, as long as a store can hold them there; the
copy fails, as before, only when none can, and the preference to keep
them is on.

### Files another program renamed

bibli cannot follow a rename made elsewhere; the companion becomes an
orphan. When a store records a fingerprint of its file (XMP's
`xmpMM:DocumentID`, a size and a digest), an orphan whose fingerprint
matches a file of its folder is offered back to it, as a plan. `._` files
record none and stay orphans.

## Consequences

- `MetadataProvider::fill` gains the folder's names (or a lookup of
  them); the folder listing keeps a companion map beside its entries.
- The built-in operations and their inverses carry companions; the
  operation property tests extend to trees with companions (undo restores
  them, redo applies them again), including partial failures.
- A copy to a USB stick keeps the notes, and a Mac sees them; a stick
  written on a Mac and copied to a Linux disk arrives with its notes as
  attributes.
- `attrs::AsideNames` and the `._` special case of `path_provider.rs` go
  away.
- Not covered: tools other than bibli (`cp`, `mv`, other file managers,
  sync services) still move files without their companions; a companion
  deleted elsewhere takes its notes with it.
