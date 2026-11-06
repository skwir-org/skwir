# ADR-058: Properties

Status: accepted, 2026-10-13. Built. Amended 2026-11-06 (decision 8: the
panel is two switches, Preview and Properties; decision 9: what comes
later fills in, it never moves what is shown).

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
   the panel does; an edit carries the entry it was made on. How it opens
   and hides is decision 8's since 2026-11-06.
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
   Properties shows something else or the pane goes elsewhere (it is the
   pane's location's work). It is kept by what it counts, so selecting
   the same entries again keeps it. With several selected, the folders
   among them count as folders; what it could not list or read is said
   apart, and a count done or stopped counts again when asked.
4. **Access is changed by plans.** `set_access` (the core's, `plans`)
   takes chmod's grammar (`644`, `u+x`, `go-w`, `a=rX`; a clause naming
   no class leaves out the umask's bits, as chmod, and is refused where
   the system does not say its umask and the clause would give
   something), a group by number (as `ls -n` and Properties show it),
   and `inside`: everything a folder holds, where `X` gives folders their
   search bit and keeps files as they were. Its step changes what a link
   leads to, as chmod does, never through a link inside, a link put in
   place mid-walk, another drive or a trash; a folder whose owner keeps
   access to it is changed before what it holds, any other after, so
   every level reaches every item. A new group clears the set-ID bits
   the system clears, as chgrp. Its inverse puts back each item's mode
   and group, and says what it could not. A drive that kept nothing
   (FAT) is said after the change, read back, never guessed from its
   type. Its dry run walks what it would change: a change already made
   is done, and an item of someone else's inside is a conflict.
   Properties offers it where it can take: the user owns the file (or
   is root), the drive is not read-only, the file is not in the trash.
   Levels are said in words (Read, Read and write; a folder's List
   only, Open items, Create and delete items); a file's search bit is
   "Runs as a program"; the mode itself takes chmod's text.
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
   clipboard. What the window asks of it waits in one lane: one job at a
   time reads the file asked last for all that is asked of it, so a large
   file holds one worker, not every one while folders wait to be listed.
   A source says its group ships folded (`Property::folded`); the shell
   names no plugin's group.

7. **Compact, and the preview keeps its room.** Over a preview,
   Properties takes at most half the room above it (less when it says
   less), or half of a room wider than tall beside it, and scrolls past
   that; the edge between them is dragged, each way apart, and a double
   click puts it back to half. Each group folds from its heading, and
   stays folded for every file; the drive's group (Volume) and the
   checksums' ship folded, the drive's heading still naming it and its
   free room. A row's
   links show while the pointer is on it, over the end of its last line,
   taking no room otherwise, except those that are all the row says
   (Count, Compute, Add… to no tags or no comment) or that stop or start
   what runs.

8. **The panel is two switches** (amended 2026-11-06, the user's
   report). The panel had a state of its own (open or closed) and
   Properties one inside it, ignored where there was nothing to preview:
   on a folder Alt+Enter opened the panel and never closed it, and a
   press counted instead. Every fix (closing the panel where Properties
   was all it showed, remembering which press opened it) added a rule a
   user had to guess. Now:
   - **Preview** (F11, Ctrl+Shift+Space, the Preview button) and
     **Properties** (Alt+Enter, Ctrl+I, the Properties button beside it,
     the menu's last item, `ui.properties`) are two switches, each lit on
     its button, kept with the window's shape. The panel is on screen
     while either is on (`PreviewState::panel_open`): there is no third
     state.
   - Each key flips its own switch, always. Nothing else turns one: not
     the cursor, not what is shown, not the press before. So two presses
     leave the window as it was, from a closed panel or an open one, on
     a file, a folder or a pipe; and the menu's item says Hide Properties
     exactly while the switch is on.
   - What the panel shows follows from the two and what is shown:
     Preview alone shows the preview, or, with nothing to preview, the
     type's icon under the card and why there is none; Properties alone
     fills the panel and no preview is read, but a folder's, whose
     preview is what it holds, which Properties says; both put Properties
     over the preview (decision 7), or, with nothing to preview, as tall
     as it says, the reason under it.
   - Turning Properties on while a folder is shown counts it: that is
     the asking (decision 2). With Properties on, another folder is
     counted from its Count link.
   - Peek (Space) always shows the preview, with Properties beside it
     while the switch is on; its header keeps a Properties button, the
     toolbar being under it.
   - The card is no longer a button, nor has the panel's header a
     Properties button: one control per switch, beside the other.
   - A saved window that had the panel closed and Properties open under
     it opens with both off (`Session::upgraded`): before, Properties
     showed only with the panel.
   - Costs: a folder no longer shows its Properties by itself in a
     panel showing previews; closing a panel that shows both takes both
     switches; with Properties on, Preview's switch changes only its
     button on a folder; and with Properties alone, what only the
     preview reads (a picture's dimensions, a document's details) is not
     said.
9. **What comes later fills in; it never moves what is shown**
   (amended 2026-11-06, the user's report: Properties flickered as a
   folder's Content came a moment after the rest, and Opens with after
   a delay). What is read on a worker (the room a file takes, its drive,
   a plugin's rows, what the preview says) came about 30 ms after the
   rest, each part pushing down what was below it; the entry before
   lost its rows at once, so even between two files with the same
   answers rows went and came back. A short hold of the whole section
   until everything answered was weighed: it adds that wait to every
   step and still moves rows on a slow drive. Instead:
   - Properties' rows are decided when the entry is shown; an answer
     only fills in values. A row known to come is there at once,
     labelled and empty: what a source says it answers for a kind of
     entry (`PropertySource::places`, said when it is registered: Opens
     with for every file, each checksum), what every reading of the
     fields answers (On disk, Volume), a size not stat-ed yet.
   - The thing's own measure moves into General, under its sizes, kept
     in place while the preview that says it is on its way: a folder's
     Contains (its folders and its files in one row, replacing
     Contains and Of which), a picture's Dimensions, a document's Pages,
     a track's or a film's Duration. Which comes is known by the kind.
   - Content, what else the file says of itself, whose rows are known
     only once read, is the last group: below it nothing moves.
   - The type the preview's worker reads of the bytes is said as soon
     as it is read, so a source waiting for it (Opens with, for a name
     several types claim) fills in with the rest; with Properties alone,
     where no preview reads it, the source is asked without it at once
     and reads the head itself.
   - A row that comes only sometimes is read where everything of its kind
     is: what is beside a file (its companions, the file a companion
     belongs to) is read with its metadata, by core, for every rule and
     wherever its folder's files are read (`companions::BesideProvider`,
     [notes.md](../invariants/notes.md)); Properties reads it from the
     entry. A first fix answered it from the folder's listing and from
     what some rules kept in memory, the rest on a worker: it left
     sidecars of other forms (Adobe's `x.xmp`) and other rules late.
   - An entry shown before it is read keeps the headings reading always
     brings (Permissions, Times); the card keeps its icon's place while
     the theme's icon is read. Rows only some files have that only
     reading says (Kept in, Downloaded from, Hard links) still come in
     for such an entry: it is shown before it is read only for a moment,
     where the cursor runs ahead of a large folder's reading.
   - An empty value is unknown: the rows of the entry before are never
     kept for the next. So between two files with the same answers a
     value is empty for a frame or two; nothing moves.

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
