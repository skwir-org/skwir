# ADR-027: Notes are read from every store and written where the user says

Status: accepted, 2026-09-30. Built the same day in seven steps
(`docs/PLAN.md`, "Notes everywhere"). Not checked: a round trip through
exFAT read by a real Mac (checked on a FUSE mount refusing attributes).
Windows's and macOS's own stores wait for Phase 5. Amended 2026-10-18
(see Amendments: a name follows an edit, when asked).

## Context

A file's notes (tags, rating, comment, download origin) live on the
file (`docs/invariants/notes.md`); no database holds them.
The tags plugin read three places in special-cased code (`attrs.rs`):
the desktop's attributes (`user.xdg.tags`, `user.baloo.rating`,
`user.xdg.comment`, shared with Dolphin and Baloo), a Mac's Finder
attributes (`finder.rs`), and the Mac's `._` file on volumes without
attributes (`appledouble.rs`). It merged by union.

The goal: the user's notes on any system and USB drive, nothing lost
going back and forth, other applications' stores read by default and
written as the user chooses. Four things block it:

1. **Every format is a branch** in reading, writing, undo and errors.
2. **Union does not round-trip.** A tag the Finder removed comes back
   from our attribute and is written back into the Finder's. Nothing
   records what two stores last agreed on.
3. **Stores hold different things.** No comma in `user.xdg.tags`; whole
   stars in most XMP tools; no spaces, brackets or rating in TagSpaces
   names. A write can silently lose what a store cannot hold.
4. **Files beside a file do not follow it** (ADR-028).

What exists (researched 2026-09-30):

| Where | What |
|---|---|
| macOS | Finder tags in `com.apple.metadata:_kMDItemUserTags`; on FAT/exFAT in `._` files (RFC 1740 AppleDouble), on SMB in streams. No rating; comments also in `.DS_Store`. Name flags (`#PS`) say what survives copy, save, sharing, sync; iCloud Drive keeps Finder tags only. |
| Windows | Tags (`System.Keywords`) only inside files with a property handler (XMP/Exif, Office). Text files, zips, folders cannot be tagged; the NTFS-stream summary ended with XP. |
| Linux | KDE's attributes; GNOME Files has no user tags (2026). |
| Standards | XMP (ISO 16684-1, -3) is the only standard for a file's metadata, embedded or as sidecar; photo apps read it. RFC 1740 is the one container an OS reads by itself without attributes. freedesktop names `user.xdg.*`, no sidecar. RO-Crate, BagIt and C2PA solve other problems. |
| Precedent | Metadata Working Group (2010), Exif+IPTC+XMP in one photo: a read order, and "a Changer MUST update all formats that were originally present", only XMP for new files. |
| Applications | TagSpaces (`.ts/x.json`, or `x[a b].ext`), filetags (`x -- a b.ext`), digiKam, darktable, Lightroom (`x.ext.xmp` or `x.xmp`). |

## Decision

### One model of notes, in the core

`skwir_core::notes`: **tags** (a name, the color a file gives it as a
palette name, and its parent where a store says it, `People/Anna`),
**rating** (1–10, half stars), **comment**, and read-only **origin** and
**referrer**.

**A color belongs to a tag**, not to the file: Mac color labels became
tags, TagSpaces colors each tag. XMP's per-file `xmp:Label` is read as
the tag of that name and written back there.

### Stores

A **store** is one place notes are kept, registered by a plugin
(`App::notes`, owner-tagged). It declares its id and title; where it
keeps notes (attributes, a file beside, the name, the content); what it
can hold, limits in words ("no comma in a tag"); its reading cost (per
file, per folder, or opening the file: then only on demand as a field,
ADR-023, never for a listing); the volumes it applies to and whether it
can create storage; and batch read and write. A write keeps what the
model does not hold (a sidecar's other properties, a `._` file's other
attributes).

| Store | Where | Read | Write (default) |
|---|---|---|---|
| XDG (KDE, Baloo) | attributes | yes | creates, where attributes exist |
| Finder | attribute, three Linux names | yes | updates; creates on macOS |
| AppleDouble | `._x` | yes | updates; **creates on volumes without attributes** |
| XMP sidecar | `x.ext.xmp` or `x.xmp` | yes | updates; creation opt-in |
| XMP in the file | content | on demand | no (it changes the content) |
| TagSpaces sidecar | `.ts/x.json` | yes | updates; creation opt-in |
| TagSpaces name | `x[a b].ext` | where a `.ts` folder shows TagSpaces is used | opt-in, as a rename (Update since 2026-10-18) |
| filetags name | `x -- a b.ext` | opt-in | opt-in, as a rename (Update since 2026-10-18) |

The core declares the stores' settings under `notes.*` (Notes page),
since several plugins share them: each read or not, written Off, Update
existing or Create. Defaults follow the platform: Linux, XDG; macOS,
Finder (XDG beside it for the rating); Windows, XMP where a format
holds it, else a store of the platform's choosing (Phase 5).

**On a volume without attributes, the store created is the one the
folder already uses** (`.ts`, `.xmp`, `._`), **else AppleDouble**: the
one container an OS reads there, and a container of attributes, so it
holds ours too (`user.xdg.tags` as `xdg.tags`, an attribute again on a
Mac or after `rsync -X` back to Linux). Nothing is translated or lost.

### Reading: what changed since Skwir last wrote

The cache (namespace `notes`, by file identity) keeps what Skwir last
wrote to each store. A store that differs was edited elsewhere: its
edit wins, additions and removals alike. With nothing remembered, tags
are the union; a rating or comment is the most recent store's where
stores keep a time (sidecar, XMP `MetadataDate`), else the platform's
own. **Nothing is dropped; at worst a removed tag comes back.** Details
shows where notes are kept and which stores disagree, and offers to
make them agree (a plan).

### Writing: every store the user allows, in one step

An edit (add tags, remove tags, set a rating) applies to the notes as
read and is written to every store the policy allows: stores holding
the file's notes are updated (the MWG rule); only a Create store starts
new storage. What a store cannot hold, the others keep, and the review
says so; a store that can hold none of the edit is left alone.

`set_notes` records the exact bytes it replaced in each store. Undo
puts them back, depending on no parser being perfect; a store changed
since is a conflict, never overwritten. `set_tags`, `set_rating` and
`set_comment` are gone (2026-10-13: no plan of theirs outlived them, no
one using skwir yet).

### Names that hold tags

Writing a name is a rename plan of its own (`Change::Rename`), never
part of a notes edit (amended 2026-10-18: a rename step beside the
edit, when asked). yt-dlp names downloads `title [id].ext` and
`[1080p]` ends release names, so TagSpaces names are read only under a
`.ts` folder; filetags names only on request.

### No format of our own

Nothing else would read it. What one could add (refinding a file by
fingerprint, per-field merge times, per-field survival flags) lives in
Skwir instead: the snapshot, a check for a sidecar's missing file
(ADR-028), the stores' declarations. A sidecar of our own, if ever
needed, will be an XMP profile with a Skwir namespace for the rest.

## Consequences

- The existing code became three stores (XDG, Finder, AppleDouble)
  without a change in behavior; everything else reads merged notes.
  AppleDouble also keeps the XDG attributes.
- XMP and TagSpaces stores are plugins of their own, depending only on
  the core.
- A listing reads only per-file and per-folder stores; companions come
  from the listing's names (ADR-028). `tags_fill_us` must hold.
- Limits: applications saving by rename-over drop attributes (sidecars
  survive); Windows shows `._` files as clutter and may delete them;
  Google Drive and OneDrive keep no attributes; deleting the cache can
  bring a removed tag back once.

## Amendments

### 2026-10-18: a name that holds tags follows an edit, when asked

The table above made a name's writing "opt-in, as a rename", but the
first build left the Notes page without a Write setting for a name:
only "Tags into Name" and "Tags out of Name" wrote one, and a tag added
to `IMG[vacation].jpg` reached every store but its name. Three reasons
kept it so on 2026-09-30, and what became of each:

- **A rename moves the file under whatever knew its path.** Since
  ADR-049 a listing pairs a name gone with the name come by the file
  and the pane follows (`ListingEvent::Renamed`, `Selection::rekey`);
  the shelf, the clipboard and a search's results follow
  `Change::Rename`. Other applications still lose the path: that is the
  user's choice when they turn it on, as it is in TagSpaces.
- **A name never disagrees.** Kept: a store in names is never said to
  differ, and "Make notes agree" leaves it alone.
- **A name holds less.** Kept: what a name cannot hold (spaces,
  brackets for TagSpaces, categories, a rating, a comment) stays in the
  other stores, as for any store with limits.

Decided:

- **Write: Off or Update, for a name.** Update has an edit of a file
  whose name holds tags (read so: under a `.ts` folder for TagSpaces,
  once read for filetags) rename the file to hold what its name can of
  the notes the edit leaves (`NotesStore::renamed`). Its own tags stay
  in their order and new ones follow; a name already saying the tags,
  case aside, is left as it is. A name that holds none is never started
  (no Create). Off stays the default.
- **A step of its own, decided before anything runs.** The edit reads
  nothing when it is made (it is made on the window's thread). On the
  plan's thread, before names are settled and the plan checked, and in
  the review's dry run, a step may unfold into the steps it is once
  what it reads is read (`Operation::unfolds`, `op::unfold`): the edit,
  with the name left out of its policy, then a `Rename`. So the review
  lists the rename, a name taken (before, or by an earlier step of the
  plan: `Rename::check` asks `StepContext::made`) refuses the whole plan
  before anything is done, Undo puts back the name then the notes'
  bytes, and whatever follows paths sees `Change::Rename`. The toast
  counts the plan's items as asked (`Plan::items`), not its steps.
- The tags index learns an edited file where the plan left it
  (`op::Follow`): a rename after an edit, here or in "Tags out of
  Name", left it reading the name gone.
