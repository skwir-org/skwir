# ADR-034: The sidebar's places are the user's to arrange, and the trash is one

Status: accepted, 2026-10-04. Amends ADR-032 (verbs) for the trash.
Amended 2026-10-04: a trashed folder stays in the trash, and a folder's
trail starts at the sidebar's place (below).

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
- Restoring over a file that took the place is refused, not renamed; a
  folder merges into one there again (amendment below).
- A volume's trash is watched once read: one mounted later is read when
  the Trash is opened or refreshed.

## Amendment: in the trash, and from the sidebar's places (2026-10-04)

### A trashed folder stays in the trash

Opening a trashed folder went to its path, so the address read
`Home › .local › share › Trash › files › old project`, and the pane was
a folder like any other: New Folder, paste, drop and Move to Trash were
offered in the trash, and the core, which knew only the items of a
trash's `files` as trashed, put a file of a trashed folder in the trash
again, as an item that came from the trash.

A location may now say where a folder it lists leads, as it says where
a place does (`OpenLocation::location_of`, asked of folders too): a
trashed folder leads to `trash:` and its path, which the trash plugin
lists with the core's own folder listing and reads as
`Trash › old project › sub`. Opening it, opening it in a tab or in
place, and dropping on it all go there; it keeps its path for what acts
on files. Its files are the trash's: Delete deletes for good, a drop or
a paste trashes, as on the Trash itself (moving files into a trashed
folder would make restoring it bring them where they never were), and
there is no New Folder or Rename. The core knows a file anywhere under a
trash's `files` as trashed (`op::in_trash`), and Trash refuses it.

A file of a trashed folder is restored alone (`RestorePart`): where it
was is where the folder was, then its path in the folder; the folders
missing on the way are made again, as their counterparts in the trash
are (mode, `user.*` attributes, times), and it moves back with its
companions. The folder stays in the trash with the rest and its
`.trashinfo`, which still speaks of it rightly. Restored later, the
folder merges into the folders made again (`Restore`): what it holds
moves in where its place is free, and a file where one of its files
would go stops it, as a file in the folder's own place always did.
Undone, each puts back only what it moved (`ReturnPart`, `Unmerge`), and
the folders a restore made go while empty. The freedesktop.org trash
keeps one `.trashinfo` per item, which is why file managers usually
restore only items; nothing in it forbids taking a file out of one.

### A folder's trail starts at the sidebar's place

A place renamed in the sidebar kept its folder's name everywhere else.
A folder's trail now starts at the deepest folder the sidebar shows
that holds it, under the sidebar's name (`sidebar::roots`): the rule
"the home folder stands for everything above it" made general, as
Dolphin's breadcrumb does with its places. Documents named Papers reads
`Papers › 2026`, a drive `stick › Photos`; the home folder stands for
what is above it even when the sidebar hides it. Of two places on one
folder (Music, and a bookmark of it), the first in the sidebar's order
names it. A place hidden, in a group put away, or not mounted starts no
trail. The location stays a path: Up goes to the real parent, Ctrl+L
edits the real path, and only where a place is named (the trail, a
tab, the history) does the sidebar's name replace the folder's; a
listing, a toast and Details say the folder's own. The core does not see
the user's names (they are the window's, ADR-034), so the CLI and
agents name a folder as its owner does, and the search's pill ("In
Documents") is the query plugin's words.
