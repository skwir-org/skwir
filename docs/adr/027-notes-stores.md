# ADR-027: Notes are read from every store and written where the user says

Status: accepted, 2026-09-30.

## Context

A file's notes (tags, a rating, a comment, where a download came from)
live on the file (AGENTS.md, "Notes live on the file"): no database holds
them. Today the tags plugin reads them from:

- its own extended attributes, the desktop's keys (`user.xdg.tags`,
  `user.baloo.rating`, `user.xdg.comment`), which Dolphin and Baloo share;
- a Mac's Finder attributes, under the three names they reach Linux with
  (`finder.rs`);
- on a volume without attributes, the `._` file a Mac writes beside the
  file (`appledouble.rs`).

It merges them by union and writes an edit to its own attributes and to
each Mac store already present. That is special-cased code in one module
(`attrs.rs`), and it does not scale to the goal: the user's notes on any
system (Linux, macOS, Windows, USB drives), with nothing lost going back
and forth, and interoperable with other applications, which bibli reads
by default and writes as the user chooses. Four things block it:

1. **Every format is a branch.** XMP, TagSpaces and names that hold tags
   would each add one to reading, writing, undo and errors.
2. **Union does not round-trip.** When the Finder takes a tag away and
   our attribute still has it, union brings it back, and the next edit
   writes it into the Finder's attribute again: the other application's
   edit is lost. Nothing records what two stores last agreed on.
3. **Stores hold different things.** Ours cannot hold a tag with a comma
   (`user.xdg.tags` is comma-separated); XMP ratings are whole stars in
   most tools where ours has halves; TagSpaces names hold neither spaces
   nor brackets, and TagSpaces has no rating. A write can lose what the
   store cannot hold, silently.
4. **Files beside a file do not follow it.** ADR-028.

What exists (researched 2026-09-30):

- **macOS**: Finder tags in the attribute
  `com.apple.metadata:_kMDItemUserTags`; on FAT and exFAT in the `._`
  file (RFC 1740's AppleDouble, as `copyfile` writes it), on SMB in
  streams. The Finder has no rating; its comments are also kept in
  `.DS_Store`. An attribute's name may end with flags (`#PS`) that say
  whether it survives a copy, a save by rewriting the file, sharing, and
  sync; iCloud Drive keeps Finder tags and strips most other attributes.
- **Windows**: tags (`System.Keywords`) only through a format's property
  handler, stored inside the file (XMP or Exif for images, document
  properties for Office); a text file, a zip, a folder cannot be tagged.
  The per-file summary kept in an NTFS stream ended with XP.
- **Linux**: KDE's attributes; GNOME Files has no user tags (2026).
- **Standards**: XMP (ISO 16684-1, and a JSON-LD form, ISO 16684-3:2021)
  is the only standard for a file's metadata embedded or in a sidecar
  (`x.xmp`, beside it); photo applications read it, no file manager's
  system does. RFC 1740 is the one container an operating system writes
  and reads by itself on volumes without attributes. The freedesktop
  guidelines name attributes (`user.xdg.*`) and define no sidecar.
  RO-Crate and BagIt (RFC 8493) describe collections, not a file's
  everyday notes; C2PA says where a file came from, not how it is tagged.
- **Precedent**: the Metadata Working Group's guidelines (2010) solve the
  same problem for one photo carrying Exif, IPTC and XMP: an order to
  read them in when they disagree, "a Changer MUST update all formats
  that were originally present", and only XMP for new files.
- **Applications**: TagSpaces (`.ts/x.json` sidecars, or tags in the
  name, `x[a b].ext`), filetags (`x -- a b.ext`), digiKam, darktable and
  Lightroom (XMP sidecars, `x.ext.xmp` or `x.xmp`).

## Decision

### One model of notes, in the core

`bibli_core::notes` holds what every store and every surface agree on:

- **tags**: each a name, the color a file gives it (a palette name, as a
  Finder tag's digit maps today) and, where a store says it, its parent
  (`People/Anna`, digiKam's and Lightroom's nesting);
- **rating**: 1 to 10, half stars, as now;
- **comment**: text;
- **origin** and **referrer**: read only, the system's.

**A color belongs to a tag**, not to the file: a Mac's color labels
became tags, TagSpaces colors each tag, and a tag named after a color
already wears it. XMP's one per-file label (`xmp:Label`, Lightroom's
"color label") is read as the tag of that name; its store remembers the
tag came from the label and writes it back there, not as a keyword.

### Stores

A **store** is one place notes are kept, registered by a plugin
(`App::notes`, owner-tagged, withdrawn on disable, like hints). It
declares:

- its id and title, for the settings and the review;
- where it keeps them: attributes of the file, a file beside it
  (ADR-028), its name, or its content;
- what it can hold, with limits in words ("no comma in a tag", "whole
  stars", "no rating");
- what reading costs: a call per file, one look at the folder, or opening
  the file. A store that opens the file is never read for a listing, only
  on demand as a field (ADR-023), for Details;
- the volumes it applies to (`bibli_platform::filesystem`), and whether it
  can create its storage where none exists;
- read and write of one batch of a folder's files; a write gets the
  file's notes before and after, and keeps what it holds that the model
  does not (a sidecar's other properties, a `._` file's other
  attributes).

The stores, with their defaults:

| Store | Where | Read | Write |
|---|---|---|---|
| XDG (KDE, Baloo) | attributes | yes | creates, where attributes exist |
| Finder | attribute, three Linux names | yes | updates; creates on macOS |
| AppleDouble | `._x`, beside | yes | updates; **creates on volumes without attributes** |
| XMP sidecar | `x.ext.xmp` or `x.xmp`, beside | yes | updates; creation opt-in |
| XMP inside the file | content | on demand | no (it changes the content) |
| TagSpaces sidecar | `.ts/x.json`, beside | yes | updates; creation opt-in |
| TagSpaces name | `x[a b].ext` | where a `.ts` folder shows TagSpaces is used | opt-in, as a rename |
| filetags name | `x -- a b.ext` | opt-in | opt-in, as a rename |

The settings list the stores: each read or not, and written Off, Update
existing, or Create. They are shared by the stores of several plugins, so
none of them owns them: the core declares them with the registry, under
`notes.*`, a page of their own (Notes), as `reading.*` is shared by the
content of several plugins. The defaults follow the
platform's own: on Linux the XDG attributes, on macOS the Finder's (the
XDG attributes beside it, as a Mac has no rating), on Windows XMP where
a format holds it and a store of the platform's choosing elsewhere
(Phase 5). **On a volume without attributes, the store created is the
one the folder already uses** (a `.ts` folder, `.xmp` files, `._` files),
**else AppleDouble**: the one container an operating system reads there,
and a container of attributes, not of one format's fields, so it holds
ours as well as the Mac's (`user.xdg.tags` as `xdg.tags`: a Mac copying
the file home makes it an attribute again, and `rsync -X` back to Linux
gives `user.xdg.tags`), with nothing translated and nothing lost.

### Reading: what changed since bibli last wrote

The cache (namespace `notes`, by the file's identity) keeps what bibli
last wrote for a file in every store. Reading compares each store with
it:

- a store that differs from it was edited by another application: its
  edit wins, additions and removals alike;
- stores that agree with it say nothing new;
- with nothing remembered (a file bibli never wrote, a cache deleted),
  tags are the union and a rating or a comment is the most recent store's
  where stores keep a time (a sidecar's, XMP's `MetadataDate`), else the
  platform's own store's.

The principle: **nothing is dropped; at worst a removed tag comes back.**
When stores still disagree, Details says which says what, and offers to
make them agree (a plan).

### Writing: every store the user allows, in one step

An edit (add these tags, take those, set a rating) applies to the notes
as read, then is written to every store its policy allows: those already
holding the file's notes are updated (the Metadata Working Group's rule),
and only a Create store starts storage where there was none. What a store
cannot hold is kept by the others, and the review says so ("TagSpaces
names hold no rating"); a store that can hold none of the edit is left
alone.

The step (`set_notes`) records, for every store it touched, the exact
bytes it replaced (or that there were none). Undo puts those bytes back:
it depends on no store parsing and writing its format perfectly. A store
that changed since the step wrote it is a conflict the review shows, not
something undo overwrites. `set_tags`, `set_rating` and `set_comment` stay
readable, so a journal written before keeps its meaning.

### Names that hold tags

A name store's write is a rename: a separate command that proposes a plan
of renames (`Change::Rename`, with its own undo), never part of a notes
edit. Reading a name is guarded: `title [id].ext` is how yt-dlp names
every download and `[1080p]` ends many release names, so TagSpaces names
are read only in folders a TagSpaces location covers (a `.ts` folder in
it or above it), and filetags names on request.

### Not a format of our own

No format bibli invents would be read by anything else. What a new one
could improve (finding its file again after a rename by a fingerprint,
merging edits made on two machines by keeping a time per field and the
tags taken away, saying per field what survives a copy or a sync, as the
macOS flags do) lives in bibli instead: the snapshot above, a check when
a sidecar's file is missing (ADR-028), and the stores' declarations. If a
sidecar of our own is ever needed (a Windows volume, a user who refuses
`._` files), it will be an XMP profile: XMP any photo application reads
for tags, rating and description, with a bibli namespace for the rest.

## Consequences

- The present code moves into three stores (XDG, Finder, AppleDouble)
  with no change in behavior first; the provider, the index (ADR-026),
  the vocabulary, the sidebar, the editor and the commands stay, and read
  the merged notes. AppleDouble then also keeps the XDG attributes.
- The XMP and TagSpaces stores can be plugins of their own, depending
  only on the core; `plugins_toggle` lists them.
- A listing reads only stores that cost a call per file or one look at
  the folder, and a folder's files beside files come from the names the
  listing already has (ADR-028): `tags_fill_us` in `bench-check` must
  hold.
- Limits stay: an application that saves by writing a new file and
  renaming it over the old one drops attributes (a sidecar survives);
  Windows shows `._` files as clutter and may delete them; Google Drive
  and OneDrive keep no attributes; the snapshot is in the deletable
  cache, so after deleting it a removed tag can come back once.
- Details lists where a file's notes are kept and when stores disagree.
