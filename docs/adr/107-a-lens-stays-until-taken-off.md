# ADR-107: A lens stays until it is taken off

Status: accepted 2026-10-30, the user's choices for each point below
(asked in the session that built it); built the night of 2026-10-30 on
Linux (`docs/STATUS-LOG.md`, "A lens stays until it is taken off").
Amends ADR-098 (words typed over a folder, the pills after the trail)
and retires the folder narrowing of 2026-10-03 (`workspace::Narrowed`).

## Context

- **Two filters, one look.** Words typed over a folder (ADR-098) were
  let go of when the pane showed another location; a folder narrowed by
  the Narrow by bar (`query.narrow`) stayed below the folder it was set
  in. ADR-098 made both one pill family, the same pills and the same
  funnel, and let typed words hold terms such as `tag:` and `kind:`:
  `tag:test` typed went at the first folder opened, the same `tag:test`
  chosen from the bar stayed.
- **What a term means decides how long it should last.** Words that find
  names, of folders too (`report`, `name:`), are a find: kept, they
  filter the folder opened by the word that found it, often to nothing,
  folders included. A term the listing decides of a file (a kind, a tag,
  a date, a size, a rating) keeps every folder: browsing a subtree
  through it is the point, and every folder stays reachable.
- **"Narrow by"** in the View menu turned on a bar that hid itself when it
  had nothing to offer (every file of one kind and one year, no file):
  turning it on often looked like nothing happened.
- Dolphin's filter bar keeps its filter across folders when locked;
  Finder's and Explorer's searches do not.

## Decision

1. **A lens is the part of what filters a pane the listing can decide of
   a file**: its terms with no word that finds names and nothing read
   from the file (contents, when a picture was taken, where), each term
   of the AND on its own (`lens::is_lens`). `ext:` is a lens's and keeps
   folders, as `kind:` does (a word and `name:` hold for folders too).
2. **It stays until taken off**, wherever the pane goes: a folder below,
   above, another place, a search, the trash, a tag's place. A pill's ×,
   its menu, Quick filters' choices and `query.narrow` without a facet
   take it off; Escape over a filtered folder shows everything. Back
   brings back nothing: the lens is the pane's, never a step's.
3. **The words that find names go** with the location, and so does what
   only a search can tell. The pane's filter is then the lens alone
   (`Pane::carry`, `ReadFilter::of_lens`).
4. **A search holds the lens in its own terms**
   (`LocationProvider::lensed`, the query plugin's): a lens brought in
   is added to them (`facets::with_lens`: a facet the search sets is its
   own, a date the timeline chose replacing the lens's; tags add up), so
   the walk itself narrows (the tags index answers a tag); the search's
   facets are the pane's lens then, and a folder opened from its results
   keeps them. Every search opened from anywhere (a saved or recent one,
   the timeline, a tag clicked, an agent) takes the lens; one whose text
   says its lens whole does not (`query.search`'s `as_is`: the address
   field's words, a pill's choice, `query.below`).
5. **A change of a search's lens alone is made in place of it** (no step
   of its own), as a folder's filter changes; a change of its words is a
   step Back undoes.
6. **The address field's text holds the lens** where it edits the filter
   or the search (Ctrl+F, Ctrl+Shift+F, Ctrl+L over a search): removing
   a term there removes it. Begun on a path or on nothing (Ctrl+L over a
   folder, Ctrl+J), the lens is kept before the words typed
   (`suggest::Ask::lens`): words add to it, never take its place.
7. **A tab or a split opened from a pane takes its lens**, as it takes
   its view (`App::new_tab`, `Workspace::add_pane_from`).
8. **A tag's place is its tag and how deep it holds files** (`deep`):
   the other tags, kinds and years that narrowed it are the pane's lens;
   its address no longer holds them (`with`, `without`, `kind`, `year`
   are read as nothing). The counts on its places are the index's, not
   the lens's, as a folder's folders are counted.
9. **The lens goes with its reader**: the query plugin turned off leaves
   no lens in any pane (`App::read_filters_again`).
10. **Every location shows it**: the pills after the trail over a folder,
    the trash and a tag's place as over a search, each wearing its
    facet's icon (the funnel only on the field's token and the depth
    pill); a lens pill's tooltip says it stays. The footer says "n of m"
    and "Reading N files…" while what the lens needs of the files' metadata
    comes; a location filtered to nothing says "Hidden by filters", with
    Show everything.
11. **Narrow by becomes Quick filters** (`ui.quick_filters`, its setting
    the same key; `quick_filters.rs`): a choice changes the lens
    (`query.narrow`), over any location. Asked for, the bar always shows
    and says why it offers nothing (the details still read, no file,
    only folders, every file alike); kinds are offered once the listing
    is complete, years once every file's metadata is in. Over a search
    with a tag and a tag's place it shows by itself, as before. Its count
    asks for the metadata a lens needs of the files no view draws, bar
    or not.

## Consequences

- One filter state in the core (`Pane::lens`, the filter's or the
  search's); `workspace::Narrowed`, `Pane::set_narrowed` and
  `location::Narrowing` are gone (`QuickFilters` holds a place's tags).
- A lens follows the user where it may surprise them (the trash, a
  search opened by the timeline); the pills, the footer's count and the
  empty state say it there.
- On a remote, a lens of tags, dates or sizes reads every file's metadata
  of each folder shown, as a sort by date does; a kind or an extension
  reads nothing. Whether a mount answers that from what it listed is to
  be counted (PLAN.md).
- The place counts of a tag's place no longer follow what narrows it.
