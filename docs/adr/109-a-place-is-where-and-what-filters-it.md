# ADR-109: A place is where it is and what filters it there

Status: accepted 2026-10-31, the user's choices for each point below
(asked in the session that built it), before 0.3.0. Amends ADR-107:
rule 2's "Back brings back nothing", rule 4 for saved and recent
searches, and its amendment's Back and Forward.

## Context

- **The lens lived in two places.** Over a folder, a tag's place or the
  trash it was the pane's (`Pane::filter`, `Pane::lens`); over a search
  it was in the search's own terms. A pane's history kept locations
  alone, so a search's step held its lens and a folder's held none.
- **Back needed rules to reconcile them.** ADR-107's amendment showed a
  search's step with the pane's lens in place of the one its terms held
  (`Lensing::Again`, `Bring::InPlace`, `lens::in_place_of_lens`), and a
  search made of the lens alone kept its terms, else it would walk the
  whole tree: an exception, and a search shown on Back as it never was
  (`report kind:pdf` came back as `report` once the lens was taken
  off). The address field's step taken back had a rule of its own
  (`Lensing::AsItWas`).
- **Keeping ignored the filter.** Ctrl+D and the field's bookmark button
  bookmarked a folder, pinned a tag or saved a search by its location
  alone: a folder filtered to its pictures was bookmarked plain, and
  the button was filled as if what was shown were kept.
- **The desktop's bookmarks** (GTK's file, which Nautilus, Dolphin's
  GTK dialogs and the file choosers read) hold folders and addresses,
  never a filter.

## Decision

1. **A place is a location and what filters it there**: a folder, a
   tag's place at its depth, the trash, Recent, with the words and the
   lens filtering it; a search, whose terms hold them. History steps,
   tabs closed and reopened, saved places and `navigate`'s `as_kept`
   deal in places. Sort, view, zoom and the selection are not of it.
2. **Back, Forward and the field's step taken back show a step as it
   was last shown** (`Lensing::Restored`): a search as its terms say,
   anything else filtered by what filtered it when the pane left it
   (`Pane::filter_at`, kept beside the cursor it left on), read again by
   its reader (a date says the day it is read on). A lens taken off
   since comes back with a step that had it; its pills say so.
3. **Going somewhere new takes the lens along**, as ADR-107 says: a
   folder, a bookmark, a pinned tag, a place of the sidebar, Up, the
   trail, a path typed; a search made from where the pane is (a bar of
   the timeline, a tag clicked, Find Similar) adds it to its terms.
4. **A place kept with its filter opens as kept**: a saved place, a
   saved search, a recent search shows its own filter or terms alone,
   the pane's lens left out. A place kept without one (a bookmark, a
   pinned tag) takes the lens, having none to say. A place of the
   sidebar says which (`Place::opens`, `Opens::AsKept`).
5. **Keep keeps what is shown** (Ctrl+D, the field's button, the `keep`
   command): a folder filtered by nothing is a bookmark, as before; a
   tag's place filtered by nothing a pinned tag; a search a saved
   search; any location shown with a filter a **saved place**, kept by
   the query plugin beside the saved searches, known by its filter and
   where it applies (`SavedQuery::at`; a remote folder by its address,
   as its bookmark is, ADR-077: `App::kept_location`). The toast names
   it, with Undo. The button is filled only for the place as shown,
   filter included (`App::kept_place`, `Place::is_kept_as`); a folder's
   own menu still bookmarks it plain.
6. **Saved searches and saved places are one list**, the sidebar's
   "Saved" section (its group stays `searches`, so what the user
   arranged there stays), in the user's order. Bookmarks stay the
   desktop's, filters never written there.
7. **The filter goes with its reader** (ADR-107 rule 9): with the query
   plugin off, a step whose filter held a lens is shown filtered by
   nothing, one of names alone by its names; saved places go with the
   plugin.

## Consequences

- One rule for a step: it is shown as it was. `Lensing::Again`,
  `Lensing::AsItWas`, `Bring::InPlace` and `lens::in_place_of_lens` are
  gone; the pane's lens is still its filter's or its search's.
- Back past where a lens was set shows that place without it; Back to a
  step whose lens was taken off since shows it with it, as a browser
  shows a page with what its address held.
- Opening a saved search no longer adds the pane's lens to its terms
  (ADR-107 rule 4, unreleased): nothing changes for 0.2's users.
- A folder kept with a filter opens as the folder (paste, new folders,
  its trail), never as a search one level deep.
- Agents and scripts open a place as kept with `navigate`'s `as_kept`
  and `filter`, and keep what a pane shows with `keep`; a saved place is
  named by `expr` (its filter) and `at` in `query.forget`,
  `query.rename`, `query.move` and `query.edit`.
