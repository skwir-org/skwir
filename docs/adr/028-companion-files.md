# ADR-028: Files beside a file go where it goes

Status: accepted, 2026-09-30. Amends ADR-006. Built the same day (notes
step C, `docs/PLAN.md`). Not built: offering an orphan back to its file
by fingerprint.

## Context

Some notes live in files beside their file (ADR-027): `._x`,
`x.ext.xmp` or `x.xmp`, `.ts/x.json` (and its thumbnail
`.ts/x.ext.jpg`). Only the name ties them. Skwir knew them in three
ad-hoc places (a second folder read in the tags plugin,
`attrs::AsideNames`; a `._` re-read rule in `path_provider.rs`), and
operations ignored them: renaming `x` orphaned `._x` and the Mac lost
its tags. Copying to a volume without attributes lost the notes, or
failed when keeping them (ADR-006). A Mac writes attributes to `._x`
there, and `dot_clean` turns them back into attributes; USB drives need
Skwir to do the same.

## Decision

### Companions, declared once, in the core

A **companion** belongs to another file. The store using it (ADR-027)
declares it in a core registry, since the listing and operations must
agree. Declarations are pure functions of names (no I/O):

- a file's companions' names (`._x`, `x.ext.xmp`, `.ts/x.json`, …);
- a name's file (`._x` → `x`);
- a companion's name for the file renamed `y`, and whether it may
  follow: Adobe's `x.xmp` follows only when no other file shares the
  stem (`IMG_1.CR3` and `IMG_1.JPG` both claim `IMG_1.xmp`;
  `photoshop:SidecarForExtension` decides when present).

### The listing knows them

- Providers get the folder's names with each batch, so stores find
  companions without reading the folder again.
- A companion whose file is listed is hidden unless hidden files
  **and** `core.show_companions` (General, off by default; also in the
  View menu under Hidden Files) are on: someone showing hidden files
  looks for configuration, not a `._` twin of every photo. The core
  hides them (`SortSpec`, `Pane::order`); with no store enabled nothing
  is hidden.
- An orphan (its file gone) is always listed, as a hidden file, and
  Details says what it belonged to. Details lists a file's companions.
- A change to a companion re-reads its file.

### Operations carry them

Rename, move, copy, swap, trash, restore and delete act on a file and
its companions in one step, staged together (ADR-006), undone together.
A companion already at the target is a conflict, like the file. Trash
trashes each companion as an item of its own (the freedesktop trash has
no groups), hidden beside its file there; restore brings them back.

### A copy converts notes to what the destination keeps

A copy or cross-device move reads notes through the stores and writes
them by the destination's policy (ADR-027):

- onto a volume without attributes: into the store created there
  (AppleDouble by default, which holds every `user.*` attribute);
- from `._x` onto a volume with attributes: as attributes, without
  copying `._x` (as `dot_clean`);
- XMP and TagSpaces sidecars: copied as companions, updated only if
  the user writes those stores.

**ADR-006 amended:** a destination refusing attributes no longer fails
a copy that keeps them while some store can hold them there; it fails
only when none can and the preference is on.

### Files another program renamed

Skwir cannot follow an outside rename; the companion becomes an orphan.
Where a store records a fingerprint (XMP's `xmpMM:DocumentID`, size and
digest), a matching orphan is offered back to its file as a plan. `._`
files record none. (Not built.)

## Consequences

- `MetadataProvider::fill_listed` gets the folder's names; the listing
  keeps a companion map. `AsideNames` and the `._` special case are
  gone.
- Operation property tests cover trees with companions (undo, redo,
  partial failures).
- A copy to a USB stick keeps notes a Mac sees; a Mac-written stick
  copied to Linux arrives with notes as attributes.
- Not covered: other tools (`cp`, `mv`, other file managers, sync
  services) move files without companions; a companion deleted
  elsewhere takes its notes.

## Amendments

### 2026-11-08: hidden as a dot hides them

Windows' Explorer shows `._x` files beside every file a Mac or Skwir
tagged on a stick, and a person tidying them away deletes the notes
(ADR-027's limits). Windows hides what wears the hidden attribute, which
FAT and exFAT keep in a file's entry, and which Linux's vfat and exFAT
drivers set (`FAT_IOCTL_SET_ATTRIBUTES`; exFAT's since Linux 6.7).

Decided:

- **What Skwir makes for a companion that a dot hides on Unix takes the
  mark** where its volume keeps one: a `._x` started (an edit, a copy
  keeping attributes beside the copy), placed whole by a copy or written
  again by an undo or a redo, and a `.ts` folder made for a companion
  (`fs::hidden::made`, `fs::hidden::create_dir_all`). On Windows on
  every volume; from Linux on FAT and exFAT; macOS makes its own `._`
  files.
- **Not a sidecar a person handles.** An XMP sidecar keeps no dot and no
  mark: photographers move `x.jpg.xmp` with their pictures by hand,
  which they could not see to do. A file another program made is never
  marked: that would be editing it.
- **A file replaced whole keeps its mark** (`file::take_place`): a
  rename puts the staged file's attributes in place, so a `._x` hidden,
  or a sidecar a person hid, would otherwise show again at the next
  edit.
- Best effort: a volume without the mark (ext4, NTFS from Linux, exFAT
  through FUSE or before Linux 6.7) leaves files as they were, and no
  edit or copy fails for it. Linux's copies carry no DOS attributes
  otherwise: only what Skwir makes is marked.
