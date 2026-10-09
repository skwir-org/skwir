# ADR-032: A tag is a place

Status: accepted, 2026-10-03. Amends ADR-026 (the Tags view) and ADR-027.
Amended 2026-10-04: every location that is no folder reads as a trail,
and its provider alone says how it is shown (below). Amended
2026-10-09: what is no folder never reads as one in the trail (below).

## Context

Tags had two faces and no way between them. The sidebar's All Tags
showed the Tags view, a lens of its own over the home folder; a tag
clicked there became a search, in another view, with chips in the
address instead of a trail. Nothing led back up but Back: Up asked for a
folder, and a search has none. Browsing by tag was a change of mode at
every click.

Users browse folders without thinking about it: a place, the places in
it, a trail back up, every view. Tags can be browsed the same way, and
two programs show it: Dolphin's `tags:/` (Baloo's KIO worker) lists the
tags as folders and their files in them, `tags:/Places/France` being the
tag `Places/France`; TMSU's virtual filesystem lists, inside a tag, the
other tags its files have, so `tags/a/b` is the files with both. Both
treat taking a file out of a tag folder as taking the tag off it.

A tag is not a folder in four ways, and each needs an answer:

1. A file has many tags: it is in many tag folders at once.
2. A category holds its tags' files (ADR-027's `tag_within`), where a
   folder lists only its own children.
3. "Both tags" has no folder of its own.
4. A tag has no place of its own: it is on files anywhere, and exists
   only while a file carries it (ADR-027: no database).

The shelf, a location of files kept elsewhere, had the same gap: Delete
there moved the real files to the Trash.

## Decision

### Places, trails and verbs, in the core

A **place** is an entry that is no file but a location of its own
(`EntryId::place`, a device number no filesystem gives): its provider
lists it among files, every view shows it as a folder, and opening it
goes where `OpenLocation::location_of` says. It has no path, so
operations on files leave it alone. Its provider counts what it holds
(`keys::FILES`), which its row says where a folder's size goes and which
sorting by size orders it by.

A location that is no folder may read as a **trail** of places
(`LocationProvider::trail`, answered from its text): the folder it is of
(`base`, drawn as that folder's own trail), its places outermost first,
and what narrows it, each a pill whose × goes where it leads without
it. Up goes to the place before it, then to the folder it is of. The
chevrons of the trail list the places inside each
(`LocationProvider::places`). A search is in the folder it looks in, so
it now has an Up.

What a gesture means comes from the location (`LocationProvider::verbs`):

- **drop**: files dropped or pasted there, from a pane (its selection,
  and where it is) or by path (another application's, the clipboard's
  with where they were cut), and whether they move;
- **remove**: what the Delete key does there (`take_out`), with the
  words of its menu item;
- **create**: what New Folder's key and item do there;
- **rename**: for a place's own location, what F2 does on its entry.

A gesture a location does not give keeps a folder's meaning where it
has one, and does nothing where it has none. Keys follow the place;
commands and menu items keep their meaning: Delete takes a file out of
the tag or the shelf it is shown in, while Move to Trash, beside it in
the menu, still trashes it. The `when` vocabulary says where:
`place` (every selected entry is a place), `refs`, `drops` and
`creates` (the location's verbs), and `!` before a token.

### The `tags:` location

The tags plugin provides `tags:` locations, typed in the address as
paths are (`tags:/Places/France`):

- **The path is the category path**, as Dolphin reads it, not an AND:
  one tree, the user's own, which the tree and columns views can show.
- **A tag folder lists its own files**, those tagged exactly with it,
  and its tags as places; each place's count is every file within it,
  each once. "Include sub-tags" (the View menu) lists everything within
  instead, as digiKam's "Include Tag Sub-Tree" does. A search for the
  tag still finds everything within it, as a search of a folder finds
  the files in its subfolders.
- **Both tags is a filter kept while browsing**: "with family" narrows
  the files and the places to those that have it too, and stays as the
  trail is walked up and down, until its pill's × takes it away. So
  `Places` with family is the family's pictures browsed by place. Filters
  can also leave a tag out, or keep a kind of file or a year: the Narrow
  by bar offers them, counted.
- **It is of a folder**: `tags:/` is the home folder's, and any folder's
  tags are one step from it (`in`), drawn after its own trail
  (`Photos › Tags › Places`). That step is where the trail says it is:
  first in the menu of what the folder holds (its chevron, a place of
  it, `LocationProvider::places_of`: narrowed as the trail shown has
  it), and in its own menu (Browse tags here, Alt+G); All Tags with Alt
  shows the tags of the folder shown, as a tag's pill with Alt shows its
  files there. The tags index (ADR-026) answers: at once where a count
  covered the folder, then checked as the Tags view checked; a first
  visit counts, under the loading banner.
- **A tag folder holds references**: files dropped or pasted on it are
  given the tag; dragged from one tag to another they move (the first
  tag replaced), and Ctrl keeps both; cut from a tag and pasted in a real
  folder, the real files move, their tags with them; Delete takes the
  tag off them (taking a tag folder itself takes its tag off every file,
  a plan to review); F2 on a tag folder renames the tag everywhere;
  New Folder makes a tag inside the one shown, which exists while shown
  until a file is given it (nothing on disk can hold an empty tag).
- **A field is a board**: in a category that holds one tag at a time,
  moving a file from `Status/Draft` to `Status/Final` changes its value.

The Tags view, a lens of its own, is retired: its tree is the tree view
of `tags:/`, its menu the tag folders' own, Count again is F5 there. The
sidebar's tags open their tag folders; All Tags opens `tags:/`; keeping
a tag folder (Ctrl+D) pins its tag.

### What follows from it

- **A tag leaves as a folder**: a tag dropped or pasted in a real folder
  is copied there as a folder of its files, with a folder inside for
  each tag within it (a plan to review). That is how a tag goes onto a
  stick or to someone without bibli.
- **A tag's tile shows its pictures**: in the grid, a few of its latest,
  framed in its color, as an album does.
- **The board**: an alternate view that opens every place (or folder) of
  the one shown side by side as columns of cards; a card dragged to
  another column moves there. On a field it is the field's values.
- **Any folder can be narrowed**: the Narrow by bar offers a folder's
  kinds, years and tags too, when the View menu asks for it, and a
  choice is kept while the pane shows that folder or one under it, as
  Dolphin's locked filter bar is, a pill each after the trail. It is
  pane state like the filter typed in the field, not a location: Back
  and the address stay the folder's, and nothing is searched. It knows
  only what the listing reads of each file (its name, its stat, its
  notes); what needs the file read is a search's.

## Consequences

- A view treats a place like a folder it cannot read: no path, no
  thumbnail of its own, opened by `location_of`. What acts on files says
  `!place` in its `when`.
- Locations kept beside the one shown and folders opened in place are
  kept by location, not by path, so the columns and tree views work in
  any location that reads as a trail.
- A list of files from many folders (a tag's, a search's, the shelf's)
  says where each file is, and Show in Folder goes there: one step to
  the real place, which virtual folders that hide it make people lose
  (Windows' libraries were hidden by default after a release for it).
- Delete means two things depending on where it is pressed. The menu
  says both, with their keys, and the toast says what happened, with
  Undo.
- An empty tag is not kept across restarts unless pinned.

## Amendment: a location is shown as its provider says (2026-10-04)

A location that is no folder *may* read as a trail, the decision said,
and only the tags and the searches did: the shelf and Recent, older,
showed their text (`shelf:`, `recent:`) in the address, while the
trash, newer, read as one place. How each showed elsewhere was chosen in
the shell by its scheme, in four places that disagreed: the glyph of a
tab and of the history (a search's and the shelf's, every other a
folder's), and the words of a pane with nothing to list (the shelf's,
the trash's and a tag's there, Recent's a bare "Nothing here").

Every location that is no folder now reads as a trail: one whose
provider gives none is one place, named as its tab is
(`App::trail`, from `location_title`), with nothing above it, so Up
does nothing there, as before. Its provider says the rest
(`LocationProvider`): its name (`title`), the glyph that stands for it
(`icon`: its tab, the history, its pane when empty), what it says when
it lists nothing (`empty_state`), and whether the place shown holds
places of its own (`Trail::holds_places`), the only case the trail
ends with a chevron listing them: a tag holds tags, the trash only
files. The shell reads these once per location (`trail.rs`) and keeps
no words or glyphs of its own for a scheme, so a new kind of location
is shown like the others by what its provider answers.

A location's buttons are its own too (`Verbs::tools`): the shelf's
Move and Copy to the other pane, drawn by the shell for the `shelf`
scheme, are the shelf's tools, and the trash's Empty Trash is one. A
tool names the pane it is run with (`ToolPane`): the one showing the
location, or the other pane, whose folder its files go to. It acts on
what the location lists, so it can be pressed only while that is
something, and a tool for the other pane only while that pane shows a
folder: with the shelf on both sides, Move and Copy are not offered.

A search's "Nothing found" now quotes what that search looks for, read
from its location. The shell kept the text last typed in the pane's
field, so a search reached by Back or from a saved search quoted another
search's words, or none.

## Amendment: what is no folder never reads as one (2026-10-09)

The trail drew every place as it drew a folder, a name between
chevrons, so `Home › Tags › Places` read as a folder Tags in the home
folder, and in a narrow pane, where the trail keeps its last places,
`Places › France` could be nothing but folders. Now the first of a
location's own places carries the glyph that stands for the location
(`icon`, after the folders it is of: `Home › 🏷 Tags`, the shelf's,
Recent's, the trash's), and a place that stands for a tag
(`Crumb::tag`, its full name, as `keys::PLACE_TAG` names a listed
one's) is that tag's chip, in its color, the one shown filled as the
sidebar fills its pill. A narrowing by a tag (`Pill::tag`: a tag
location's `with`, a folder narrowed; and a search's tag chip) is a
pill in the tag's color, as the Narrow by bar offered it, and one by
the files without a tag (`not`, `metadata::TagRef::lacks`) its outline
only. Plain text in the trail is a folder. The shell colors a tag as it colors a tag's
place in a view, from the tag's name, so a color the user changes shows
at once.
