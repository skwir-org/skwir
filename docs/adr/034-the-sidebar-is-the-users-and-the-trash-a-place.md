# ADR-034: The sidebar's places are the user's to arrange, and the trash is one

Status: accepted, 2026-10-04. Amends ADR-032 (verbs) for the trash.

## Context

The sidebar's groups could be folded, moved and hidden (`Sections`),
but not what was in them. Places held Home and every folder of
`user-dirs.dirs` (Templates and Public too) from the places plugin,
Recent from jump and Shelf from the shelf, in the order the plugins
were registered, none of which could be moved, renamed or left out.
Bookmarks, saved searches and pinned tags could be moved and taken out,
because their owners keep them in files of their own. Devices showed
every volume: an EFI partition mounted at `/boot` among them, while a
small FAT stick that was not mounted was left out by a guess on its
size. Nothing in the sidebar could be reached from the keyboard, and the
trash, which every file manager shows, was nowhere.

Three questions closed doors: who keeps the arrangement of a place no one
keeps (Home, a volume), what names a volume across mounts, and how much
of the trash bibli reads itself.

## Decision

### Who arranges a place

A place is one of three (`sidebar::Role`), from what its owner says of it:

- **Kept** by the user (it carries `forget` or `reorder`: a bookmark, a
  saved search, a pinned tag). Its owner orders it, renames it
  (`Place::rename`, which F2 runs) and takes it out, in the file it keeps
  it in, as before. The sidebar does not hide it: a bookmark is taken
  out, not hidden.
- **Suggested** by its owner (a tag used often): the user can hide it.
- **Offered** by its owner (Home, a folder of the desktop's, Recent, the
  Shelf, the Trash, a volume): the user moves it among the group's other
  offered places, renames it in the sidebar and hides it.

The arrangement of offered and suggested places is the shell's, kept
with the window's shape (`Sections::places`, by group: an order, what
is shown or hidden, the names given), not their owners'. One group
mixes owners (Places holds the places plugin's folders, jump's Recent,
the shelf's Shelf and the trash's Trash), and an offered place has no
file of its own to keep an order in. Nothing the desktop shares is
written: the GTK bookmarks keep only what other file managers read too.
`ui.sidebar_place` is the one command, so the menus, the keys, the
palette and agents arrange the same way. A place moves only among those
the same command moves (`sidebar::mover`): a bookmark among bookmarks,
Home among the offered places, each set keeping the slots it holds.

### What names a place

A place is named in the arrangement by `Place::key`, its location unless
its owner says otherwise. A volume is named by its filesystem (its
UUID, and the subvolume or folder mounted when that is not all of it,
since `/` and `/home` can be one filesystem's), read from sysfs and
udev's database (`places::identity`); a container's cleartext volume by
the container. So a volume is the same place locked, unlocked, mounted
or not, whatever device name the kernel gives it this time.

### udisks's word

Whether a volume is shown when the user has not said is udisks's word:
`UDISKS_IGNORE`, which its rules set for EFI, BIOS boot, recovery and
reserved partitions, read from udev's database (files in `/run`, so the
quick scan reads it without waiting for a drive, and bibli needs no
D-Bus). Such a volume is offered hidden (`Place::hidden`), listed
unchecked in its group's menu; the size guess applies only where udev
has no record. The user's choice wins either way.

### The keyboard

F6 gives the keyboard to the sidebar and back (`ui.focus_sidebar`); in
its context the arrows walk its headers and places, Enter opens, Menu
opens the menu, Alt+arrows move, F2 renames, Delete takes out or hides.
A click on a place never takes the keyboard from the files.

### The trash is a place

The trash plugin provides `trash:`: what every trash of the user's
holds, each file under the name it had, from the folder it had. It reads
the trashes itself (`bibli_platform::trash`: the home trash and those of
the volumes the sidebar shows, network and FUSE ones aside), rather than
the trash crate's listing, which asks every mount point, network ones
included, and which tests could not keep to a trash of their own. The
core keeps the operations: a file is restored by the core's `Restore`,
which now names the exact item (`item`, its `.trashinfo`) and refuses
to put it over a file that took its place; it is deleted for good by the
core's `Delete`, which takes the `.trashinfo` along, wherever the file
is deleted from; and `Trash` refuses a file in a trash already.

Where a location's files are the trash's, it says so with
`Verbs::restore` (the `restores` token): Restore is in their menu, Move
to Trash and Show in Folder are not, and Delete deletes them for good.
Deleting for good is the one step Undo cannot take back
(`Operation::undoable`), and a plan with such a step, from any gesture,
is asked about before it is applied (`Shell::apply_user_plan`): Shift+
Delete, Delete in the trash, Empty Trash.

## Consequences

- An arrangement is bibli's: Nautilus does not see Home moved below
  Downloads, nor bibli a sidebar arranged in Nautilus.
- A group whose places are all hidden keeps its header, whose menu shows
  them again.
- The trash lists no Deleted column and does not sort by it: Details says
  when a file was deleted and where from, and the Folder column says the
  folder it came from.
- Restoring over a file that took the place is refused, not renamed.
- A volume's trash is watched once read: one mounted later is read when
  the Trash is opened or refreshed.
