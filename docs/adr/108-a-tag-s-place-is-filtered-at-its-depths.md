# ADR-108: A tag's place is filtered at its own depths

Status: accepted 2026-10-30, the user's choice of the proposal below (in
the session that built it); built on Linux the same day
(`docs/STATUS-LOG.md`, "A tag's place is filtered at its depths").
Amends ADR-098 (how deep words look) and ADR-107's rule 6 for a place
with depths.

## Context

- **A tag's place could not be searched or filtered from the field.**
  Every tag's place (`tags:/`) was no scope: no magnifier, and Ctrl+F or
  Ctrl+Shift+F opened the field as Ctrl+J does. A tag's place was one,
  but its switch read "This folder | And subfolders", and "And
  subfolders" left it for a search from the folder it is of, by its tag
  (`tags_shown`), which is not what the place lists. A lens set before
  applied there all the same (ADR-107): the place could be narrowed, not
  searched.
- **A tag's place already has two depths.** It lists the files tagged
  with it exactly and the tags within it; with Include Sub-Tags (`deep`)
  every file of every tag within it too. That is the field's "this
  folder only" and "and its subfolders", in a place's own terms, and
  both are listings the pane filters at once.

## Decision

1. **A place may say two depths** (`LocationProvider::depths`,
   `App::depths`, `location::Depths`): the place holding its own
   entries, the same place holding everything within it, each a
   location of its provider, with what the field's switch says of each.
   `None`, the default, is a folder's, below which words search.
2. **Every tag's place is a scope, the top too.** Its depths are
   `deep` off and on: "This tag | With sub-tags", at the top "These
   tags | Every tagged file". The field asks the location, never its
   scheme (the shell names no plugin).
3. **Words filter a place with depths at both.** The switch, Ctrl+F and
   Ctrl+Shift+F show the depth chosen in place of the field's step
   (`App::step_here`, `navigate_in_place`), as a search shown as typed
   is; Escape takes it back. Ctrl+F over the place opens it with
   everything within; Ctrl+Shift+F, Ctrl+L and Ctrl+J keep the depth
   it is shown at. Nothing is searched as it is typed.
4. **What only a search tells waits for Enter** (what files contain,
   when or where a picture was taken: `ReadFilter::waits`), then
   searches the place as it is shown (`in:` the place, keeping its
   trail), from where the pane was when the field began, filtered as it
   was. A place chosen as the scope from elsewhere is searched with
   everything within it.
5. **Over a search of such a place**, the switch reads the place's
   depths, at the one searched; words edited, or Ctrl+Shift+F, filter
   the place again at that depth.
6. **A search within a place is complete once each of its entries was
   tested**, those whose metadata it asked for included (a tag's place
   lists its files before stat-ing them): before, the search said it was
   complete while its matches were still to come.

## Consequences

- The top's place is offered among the scopes (`@`), its Search button
  shown; a tag's place, from elsewhere, is searched as what it lists, not
  by its tag from its folder (`tags_shown` stays for a place of tags
  without depths).
- A tag's place's depth is a step of the field's while it is typed in
  and stays a step of the pane's once Enter keeps it: Back returns to the
  other depth.
- Words that find names filter the place's tags as well as its files, as
  they filter a folder's folders.
