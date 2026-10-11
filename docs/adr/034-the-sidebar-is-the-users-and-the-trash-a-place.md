# ADR-034: The sidebar's places are the user's to arrange, and the trash is one

Status: accepted, 2026-10-04. Built. Amends ADR-032 (verbs) for the
trash. Amended 2026-10-04 and 2026-10-05 (see Amendments).

## Context

Sidebar groups could be folded, moved and hidden (`Sections`), but not
their places. Home, every `user-dirs.dirs` folder, Recent and Shelf sat
in plugin registration order and could not be moved, renamed or left
out. Devices showed an EFI partition at `/boot` but hid a small
unmounted FAT stick by a size guess. Nothing was reachable by keyboard,
and there was no trash.

Three questions closed doors: who keeps the arrangement of a place no
one keeps (Home, a volume), what names a volume across mounts, and how
much of the trash Skwir reads itself.

## Decision

### Who arranges a place

A place has a role (`sidebar::Role`) from what its owner says:

| Role | Examples | Arranged by |
|---|---|---|
| **Kept** (carries `forget` or `reorder`) | bookmark, saved search, pinned tag | Its owner: order, rename (`Place::rename`, F2), take out, in its own file. Never hidden. |
| **Suggested** | a frequent tag | The user can hide it. |
| **Offered** | Home, desktop folders, Recent, Shelf, Trash, volumes | The user moves it among the group's offered places, renames and hides it. |

- The arrangement of offered and suggested places is the shell's, kept
  with the window's shape (`Sections::places`: per group order,
  visibility, names), because a group mixes owners and an offered place
  has no file to keep an order in. Nothing the desktop shares is
  written: GTK bookmarks keep only what other file managers read.
- `ui.sidebar_place` is the one command, for menus, keys, palette and
  agents.
- A place moves only among those the same command moves
  (`sidebar::mover`): a bookmark among bookmarks, Home among offered
  places.

### What names a place

`Place::key`: its location unless the owner says otherwise. A volume is
named by its filesystem: UUID, plus the subvolume or folder mounted when
not the whole (`/` and `/home` can share one), from sysfs and udev's
database (`places::identity`); a cleartext volume by its container. So a
volume is the same place locked, unlocked, mounted or not, whatever its
device name.

### udisks's word

Whether a volume shows by default is udisks's `UDISKS_IGNORE` (EFI,
BIOS boot, recovery, reserved), read from udev's database in `/run` (no
waiting for a drive, no D-Bus). Such a volume is offered hidden
(`Place::hidden`), unchecked in its group's menu. The size guess
applies only without a udev record. The user's choice wins.

### The keyboard

F6 (`ui.focus_sidebar`) moves the keyboard to the sidebar and back.
Arrows walk headers and places, Enter opens, Menu opens the menu,
Alt+arrows move, F2 renames, Delete takes out or hides. A click never
takes the keyboard from the files.

### The trash is a place

- The trash plugin provides `trash:`: every trash of the user's, each
  file under its old name and folder.
- It reads trashes itself (`skwir_platform::trash`: home and the
  sidebar's volumes, not network or FUSE). Rejected: the trash crate's
  listing, which asks every mount point, network ones included, and
  cannot be confined to a test trash.
- The core keeps the operations. `Restore` names the exact item (its
  `.trashinfo`) and refuses to overwrite a file in its place; `Delete`
  takes the `.trashinfo` along, wherever it deletes from; `Trash`
  refuses a file already in a trash.
- A location whose files are the trash's says `Verbs::restore` (the
  `restores` token): Restore in their menu, no Move to Trash or Show in
  Folder, and Delete deletes for good.
- Deleting for good is the one step Undo cannot reverse
  (`Operation::undoable`); a plan with one asks first, whatever the
  gesture (`Shell::apply_user_plan`: Shift+Delete, Delete in the trash,
  Empty Trash).

## Consequences

- The arrangement is Skwir's alone: Nautilus does not see it, nor Skwir
  Nautilus's.
- A group with every place hidden keeps its header, whose menu shows
  them again.
- Restoring over a file in the place is refused, not renamed; a folder
  merges (2026-10-04 amendment).
- A volume's trash is watched once read; one mounted later is read when
  the Trash is opened or refreshed.
- The Deleted column came with the 2026-10-05 amendment; before, only
  Details said when a file was deleted.

## Amendments

### 2026-10-04: a trashed folder stays in the trash

Opening a trashed folder went to its real path, an ordinary folder:
New Folder, paste, drop and Move to Trash were offered, and a file of it
was trashed again.

- **A location may say where a folder it lists leads**
  (`OpenLocation::location_of`, now asked of folders too). A trashed
  folder leads to `trash:` plus its path, listed by the core's folder
  listing and read as `Trash › old project › sub`. Opening, opening in
  a tab or in place, and dropping go there; the path stays for file
  actions.
- **Its files are the trash's:** Delete deletes for good; a drop or
  paste trashes (files moved into a trashed folder would be restored
  where they never were); no New Folder or Rename. The core knows any
  file under a trash's `files` as trashed (`op::in_trash`), and Trash
  refuses it.
- **A file of a trashed folder is restored alone** (`RestorePart`), to
  the folder's old place plus its path inside. Missing folders on the
  way are recreated like their trashed counterparts (mode, `user.*`
  attributes, times); companions follow. The folder stays trashed with
  its still-correct `.trashinfo`.
- **A folder restored later merges** into the recreated folders
  (`Restore`): contents move in where free; a file in the way stops it.
  Undo puts back only what each moved (`ReturnPart`, `Unmerge`);
  recreated folders go while empty.
- The freedesktop.org trash keeps one `.trashinfo` per item, which is
  why file managers restore only items; nothing forbids taking a file
  out of one.

### 2026-10-04: a folder's trail starts at the sidebar's place

A folder's trail starts at the deepest folder the sidebar shows that
holds it, under the sidebar's name (`sidebar::roots`), as Dolphin does:
Documents renamed Papers reads `Papers › 2026`, a drive
`stick › Photos`. Home stands for what is above it even when hidden.

- Of two places on one folder, the first in sidebar order names it.
- A hidden place, one in a hidden group, or one not mounted starts no
  trail.
- The location stays a path: Up goes to the real parent, Ctrl+L edits
  the real path. Only the trail, tabs and history use the sidebar's
  name; listings, toasts and Details use the folder's.
- The core did not see these names then; it does since 2026-10-05.

### 2026-10-05: out of the trash wherever a file goes

**Moving a trashed file takes it out there.** Before, only Restore could
(a drop said "restore it first", Cut was not offered); Finder and
Dolphin take a file out where it is dropped.

- Any move of a trashed file (drop, paste after Cut, Shift+F5/F6,
  `move`, an agent's plan) is a `RestorePart` to its place in the
  destination, so no path branches on the trash.
- An item goes under its original name (`draft.txt.2` back to
  `draft.txt`, from its `.trashinfo`), takes its `.trashinfo`, and the
  companions trashed with it (`._x`, `x.xmp`, `.ts/x.json`, each an
  item) go beside it where free, found as `Restore` finds them: by
  origin and date. Across volumes: copy, then remove.
- Undo puts all of it back as the trash had it, same names and
  `.trashinfo` text (`ReturnPart`, `Untrashed`), so a later restore
  still finds the companions.
- Within the trash an item does not move: the trash would hold a file
  it says nothing of.
- A merging folder restore takes its trashed companions too
  (`Unmerge::companions` undoes it). Recreated folders may be
  read-only: undo opens them to remove them, and one something was put
  in since keeps its mode.

**The Deleted column.** Where files are the trash's (`Verbs::restore`),
the list has a Deleted column (`keys::TRASHED`) and sorts by it
(`SortKey::Deleted`, newest first). Not inside a trashed folder: its
contents were trashed with it. The trash still opens in the pane's
sort.

**Names.** The sidebar's names now reach the core
(`Registry::set_place_titles`): the query plugin names a search's folder
as the sidebar does (`place_name`), in its pill ("In Papers") and title
("report in Papers").
