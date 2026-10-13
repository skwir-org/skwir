# ADR-058: Properties

Status: accepted, 2026-10-13. Built.

## Context

- Other file managers open a Properties window from a file's menu
  (Windows and Dolphin: Alt+Enter; Nautilus: Ctrl+I; a Mac's Get Info:
  Cmd+I). It says what a folder holds in all and what it takes on the
  disk, edits who may read, write and run it, names the application that
  opens its type, and in some, its checksums.
- Skwir's Details section of the preview panel already said most of the
  rest (owner, group, mode, access lists, times, notes, where a download
  came from, what the preview knows) and Alt+Enter and Ctrl+I already
  opened it. But no menu named it, it changed nothing of the file
  itself, and it said nothing of a folder's total, the room a file takes,
  the drive it is on or the flags the system keeps.

## Decision

1. **Details is Properties.** The section is renamed, not doubled: a
   dialog would list the same rows a second time. `ui.properties`
   (Alt+Enter, Ctrl+I, the context menu's last item, order 900, after the
   submenus) opens it
   for the selection, or the folder shown on the background, opening the
   panel when it is closed; over a preview it hides it again, and the
   menu's item then says Hide Properties. Opened over a preview it takes
   at most its share of the room (decision 7). It follows the cursor, as
   the panel does; an edit carries the entry it was made on.
2. **What it adds to say** (the stages below say what is built):
   - the room a file takes on its drive (`fs::stat::allocated`), beside
     its size;
   - a folder's total (size, room taken, files and folders), counted
     only when asked: a Count link, or `ui.properties` itself;
   - for several entries, how many of each kind, their total, and the
     owner, group and access they share ("Mixed" where they differ);
   - the drive (the Volume group the folder shown already had);
   - the flags the system keeps (immutable, append only, compressed,
     encrypted), which say why a delete fails.
3. **A folder is counted by the core** (`fs::count`), not by the Usage
   plugin as first planned: Usage's requests belong to a pane's Usage
   view (one per pane, its statuses by pane) and its cache lives while a
   view shows it, so sharing it would rework Usage for a cache Properties
   seldom finds. The count follows no link, stays on the folder's drive
   (a drive mounted inside is not counted, and says so), counts a file of
   several names once, streams what it has so far, and is cancelled when
   Properties shows something else.
4. **Access is changed by plans.** `set_access` (the core's, `plans`)
   takes chmod's grammar (`644`, `u+x`, `go-w`, `a=rX`), a group by name
   or number, and `inside`: everything a folder holds, where `X` gives
   folders their search bit and keeps files as they were. Its step changes
   what a link leads to, as chmod does, never through a link inside; its
   inverse puts back each item's mode and group. A drive that kept
   nothing (FAT) is said after the change, read back, never guessed from
   its type. Properties offers it where it can take: the user owns the
   file (or is root), the drive is not read-only, the file is not in the
   trash. Levels are said in words (Read, Read and write; a folder's
   List only, Open items, Create and delete items); a file's search bit
   is "Runs as a program"; the mode itself takes chmod's text.
   On Windows the step sets the read-only attribute (the owner's write
   bit); a group is not yet.
5. **No owner changes, no administrator rights.** Giving a file to
   another user needs root. Asking for it (pkexec, polkit, sudo) would
   make skwir the front of a privileged program: out until an ADR of its
   own. Access lists are shown, not edited.
6. **Plugins add rows** through a property source
   (`Registry::property_sources`): a command answering rows for the entry
   shown, asked on a worker when Properties collects it, again when its
   owner says its answers changed. Open with answers "Opens with" and
   changing it; a checksums plugin answers SHA-256, MD5 and BLAKE3, each
   computed only when asked, with Copy and a comparison with the
   clipboard.

7. **Compact, and the preview keeps its room.** Over a preview,
   Properties takes at most half the room above it (less when it says
   less), or half of a room wider than tall beside it, and scrolls past
   that; the edge between them is dragged, each way apart, and a double
   click puts it back to half. Each group folds from its heading, and
   stays folded for every file; Checksums and Volume ship folded, the
   drive's heading still naming it and its free room. A row's
   links show while the pointer is on it, over the end of its last line,
   taking no room otherwise, except those that are all the row says
   (Count, Compute, Add… to no tags or no comment) or that stop or start
   what runs.

## Consequences

- One surface: the keys, the menu, the palette, agents and scripts reach
  the same rows and the same commands.
- Properties over a preview leaves the preview at least half the room
  unless its edge is dragged, and the preview keeps its toolbar however
  far it is; hiding Properties gives the room back.
- What a row can change is not seen until the pointer is on it; the
  rows that offer nothing else keep their links in sight.
- A plugin from outside skwir adds fields to Properties, not property
  sources: its protocol has no rows with links yet.
