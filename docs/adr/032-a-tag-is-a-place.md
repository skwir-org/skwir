# ADR-032: A tag is a place

Status: accepted, 2026-10-03. Built. Amends ADR-026 (the Tags view) and
ADR-027. Amended 2026-10-04 and 2026-10-09 (see Amendments).

## Context

All Tags showed the Tags view, a lens over the home folder; a tag
clicked there became a search in another view, with chips instead of a
trail, and only Back led back. Browsing by tag changed mode at every
click.

Folders are browsed as places with a trail, in every view; tags can be
too. Dolphin's `tags:/` lists tags as folders (`tags:/Places/France` is
the tag `Places/France`); TMSU lists, inside a tag, the other tags its
files have (`tags/a/b` is files with both). Both treat taking a file
out of a tag folder as removing the tag.

A tag is not a folder in four ways:

1. A file has many tags, so it is in many tag folders.
2. A category holds its tags' files (`tag_within`); a folder lists only
   its children.
3. "Both tags" has no folder.
4. A tag exists only while a file carries it (ADR-027: no database).

The shelf had the same gap: Delete there trashed the real files.

## Decision

### Places, trails and verbs, in the core

- **A place** is an entry that is a location, not a file
  (`EntryId::place`, a device number no filesystem gives). Every view
  shows it as a folder; opening it goes to
  `OpenLocation::location_of`. No path, so file operations skip it. Its
  provider counts what it holds (`keys::FILES`: size column and sort).
- **A trail.** A location that is no folder may read as a trail
  (`LocationProvider::trail`): the folder it is of (`base`), its places,
  and what narrows it, each a pill whose × removes it. Up walks back
  the places, then to the folder. Chevrons list places inside
  (`LocationProvider::places`). A search now has an Up: its folder.
- **Verbs** (`LocationProvider::verbs`) give gestures their meaning:
  - **drop**: files dropped or pasted, from a pane or by path, and
    whether they move;
  - **remove**: what Delete does (`take_out`), with its menu words;
  - **create**: what New Folder does;
  - **rename**: what F2 does on a place.

  A gesture not given keeps a folder's meaning or does nothing. Keys
  follow the place; commands and menu items keep theirs: Delete takes a
  file out of a tag or the shelf, while Move to Trash still trashes.
  New `when` tokens: `place`, `refs`, `drops`, `creates`, and `!`.

### The `tags:` location

The tags plugin provides `tags:` locations, typed like paths
(`tags:/Places/France`):

- **The path is the category path**, not an AND: one tree, the user's,
  usable in the tree and columns views.
- **A tag folder lists its own files** (exactly tagged) and its tags as
  places, each counting every file within it once. View menu "Include
  sub-tags" lists everything within (as digiKam). A search for a tag
  still finds everything within.
- **"Both tags" is a filter kept while browsing**: "with family"
  narrows files and places and stays as the trail is walked, until its
  pill's × removes it. Filters can also exclude a tag, or keep a kind
  or a year; the Narrow by bar offers them, counted.
- **It is of a folder.** `tags:/` is home's; any folder's tags are one
  step away (`in`), drawn after its trail (`Photos › Tags › Places`):
  first in its chevron menu (`LocationProvider::places_of`), Browse tags
  here, Alt+G. All Tags with Alt shows the shown folder's tags. The
  tags index (ADR-026) answers at once where a count covered the
  folder, then checks; a first visit counts under the loading banner.
- **A tag folder holds references:**
  - dropped or pasted files get the tag; from another tag they move
    (Ctrl keeps both);
  - cut from a tag and pasted in a folder, the real files move;
  - Delete takes the tag off; deleting a tag folder untags every file
    (a plan to review);
  - F2 renames the tag everywhere;
  - New Folder makes a tag, kept while shown until a file gets it.
- **A field is a board**: moving a file from `Status/Draft` to
  `Status/Final` changes its value.

**The Tags view is retired**: its tree is the tree view of `tags:/`,
Count again is F5. Sidebar tags open tag folders; All Tags opens
`tags:/`; Ctrl+D on a tag folder pins its tag.

### What follows

- **A tag leaves as a folder**: dropped or pasted in a real folder, it
  is copied as a folder of its files, a subfolder per tag within (a
  plan to review), for a stick or someone without Skwir.
- **A tag's grid tile** shows a few of its latest pictures, framed in
  its color.
- **The board**: an alternate view opening every place or folder of the
  one shown as columns of cards; dragging a card to another column
  moves it there. On a field, the columns are its values.
- **Any folder can be narrowed** by kind, year and tag (View menu), kept
  while the pane shows that folder or one under it, a pill each after
  the trail. Pane state like the typed filter, not a location: Back and
  the address stay the folder's. It uses only what the listing reads
  (name, stat, notes).

## Consequences

- A view treats a place like an unreadable folder: no path, no own
  thumbnail, opened by `location_of`. File actions say `!place`.
- Kept locations and opened folders are keyed by location, not path,
  so columns and tree work in any trail location.
- Lists from many folders (tag, search, shelf) show each file's folder,
  and Show in Folder goes there: virtual folders that hide the real
  place lose people (Windows hid libraries by default after a release
  for it).
- Delete means two things by place; the menu shows both with keys, the
  toast says what happened, with Undo.
- An empty tag is not kept across restarts unless pinned.

## Amendments

### 2026-10-04: a location is shown as its provider says

Only tags and searches read as trails; the shelf and Recent showed
`shelf:`, `recent:`, and the shell picked glyphs and empty texts by
scheme, inconsistently.

- **Every location that is no folder reads as a trail.** Without one
  from its provider it is one place named as its tab (`App::trail`,
  `location_title`); Up does nothing there.
- **The provider says the rest**: `title`, `icon` (tab, history, empty
  pane), `empty_state`, and `Trail::holds_places` (only then does the
  trail end with a chevron: a tag holds tags, the trash only files).
  The shell reads them once per location (`trail.rs`) and keeps nothing
  per scheme.
- **A location's buttons are its own** (`Verbs::tools`): the shelf's
  Move and Copy to the other pane, the trash's Empty Trash. A tool names
  its pane (`ToolPane`: this one, or the other, whose folder receives
  the files). It is enabled only while the location lists something,
  and an other-pane tool only while that pane shows a folder (shelf on
  both sides: no Move or Copy).
- A search's "Nothing found" quotes its own query, read from the
  location, not the text last typed in the pane (wrong after Back or a
  saved search).

### 2026-10-09: what is no folder never reads as one

`Home › Tags › Places` read as folders. Now:

- The first of a location's own places carries the location's `icon`
  (`Home › 🏷 Tags`; the shelf's, Recent's, the trash's).
- A place standing for a tag (`Crumb::tag`, its full name, as
  `keys::PLACE_TAG`) is the tag's chip in its color, the one shown
  filled like the sidebar's pill.
- A narrowing by a tag (`Pill::tag`: a tag location's `with`, a
  narrowed folder, a search's tag chip) is a pill in the tag's color;
  one by its absence (`not`, `metadata::TagRef::lacks`) is outlined.
- Plain text in a trail is a folder. Tag colors come from the name at
  draw time, so a color change shows at once.
