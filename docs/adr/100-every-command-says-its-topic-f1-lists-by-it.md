# ADR-100: Every command says its topic; F1 lists the keys here by it

Status: accepted, 2026-10-27, the user's go on the proposal "F1 that
shows everything at a glance" (a page shared in the session, the panel
before and after at 1:1) with its two choices: topics declared for every
command now, and scrolling kept over pages. Built and checked on Linux
the same day (`docs/STATUS-LOG.md`, "F1 by topic, in columns");
Windows and macOS compiled (`just check-cross`).

## Context

- F1 listed one row per binding, under the place it works in, in 300 px
  columns that wrapped two at a time. In a file list with nothing
  selected 104 bindings apply; an 860 px window showed 22, with nothing
  saying the rest were there. The file list's 35 rows made one column, a
  binding of the file list or the terminal a column of its own, and
  Anywhere's 68 wrapped under them, out of sight.
- Twenty of the file list's rows were the cursor's moves (Select up,
  Extend the selection up…); an action with two keys was two rows.
- Nothing said what a command is about: a list could group commands only
  by where their keys work.

## Decision

1. **Every command names its topic where it is registered**
   (`command::Topic`, an argument of `CommandRegistry::register`,
   `register_raw` and `register_dynamic`, so none is left out): going
   places, selecting, files, finding, tags and notes, tabs and panes,
   views and panels, previews, playing, the terminal, places and servers,
   the window, Skwir. It is part of the declaration agents read. A plugin
   from outside says it in its manifest (`topic`); one that does not has
   its category's (browsing: going places, previews: previews, else
   files).
2. **A family is declared on forms** (`Form::family`): forms of one
   command that only go elsewhere (Extend the selection up, down, to the
   first item) share a name. Declared for the cursor (one family per
   step, a page at a time apart), a tab by its number, moving a tab, zoom,
   the preview's zoom and pages; the views are one, in the view
   selector's order. Never guessed from titles.
3. **F1 lists one row per action**, its keys side by side, whichever
   place each works in (Parent directory: Backspace / Alt+↑); a family's
   actions on one row, the first key of each, the modifiers they all
   hold said once (Shift + ↑ ↓ ← → Home End).
4. **Under its topic.** The topics of the place nearest the focus come
   first, in `Topic::ALL`'s order among them; in a topic, that place's
   rows first. Places name only what a filter finds elsewhere, where the
   place is the point. (The proposal kept places as headings with topics
   inside Anywhere; every row F1 lists works here, so the place said
   nothing a person looks for.)
5. **The rows flow down columns** (`elements::columns`): as many as fit
   the panel (three at 1280 px, four from about 1400), as short as they
   can be with every topic whole; only one taller than a column carries
   on in the next, under its heading again, each part keeping four rows
   at least. A row is one line where it fits its column, else its keys go
   under its title: each is measured once on one line when the layout
   asks, and the split counts lines from there, for the layout and for
   prepaint alike.
6. **Scrolling stays, and shows**: what does not fit scrolls, under
   Skwir's overlay scrollbar, there at rest whenever there is more.

## Consequences

- A new command needs a topic to compile; a new family is a name on its
  forms. `Topic::ALL` is F1's order: a new topic takes a place in it.
- The Keyboard page keeps one row per action and key: what it lists is
  what is changed, key by key.
- In a file list with nothing selected, F1 lists its bindings in 55 rows
  under 12 topics, all of them in view at 1280 × 860.
