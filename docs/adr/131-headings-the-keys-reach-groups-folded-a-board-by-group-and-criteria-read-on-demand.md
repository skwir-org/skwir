# ADR-131: Headings the keys reach, groups folded, a board by group, and criteria read on demand

Status: accepted, 2026-11-11, on the user's answers to four questions
asked after ADR-130's phase 4 (headings pointer-only; phase 5 left).
Built on 2026-11-11 and run on Linux. Amends ADR-130, decisions 9 and
12, and its phase 5.

## Context

- ADR-130 kept the cursor on entries alone (9): a heading acted on the
  pointer only, and the palette's "Select the group" on the cursor's.
- Its phase 5 left collapsing, a Board with a column per group, and
  criteria read on demand ("the day a picture was taken").
- **Read on demand** are what a stat and the metadata providers do not
  read, as too costly for every listing (ADR-023, `App::fields`): the
  camera's clock (`keys::TAKEN`, the geo plugin's, from EXIF and an MP4's
  boxes) is one. A picture's size in pixels and a sound's or a film's
  length are shown in Properties from the preview (the decoded picture,
  the player), not read for a folder.

## Decision

1. **A heading is a key stop** (amends ADR-130, 9), as in Windows
   Explorer. Up from a group's first entry, and Down from the group
   before's last, stop on its heading, drawn with the cursor's ring; the
   arrows go on from it to the entries. On a heading, Space or Enter
   selects its group, Left folds it, Right unfolds it, the Menu key (and
   Shift+F10) opens its menu; a screen reader hears it focused, folded
   or not. With Shift or Ctrl the arrows go from entry to entry, as
   before. A heading is never selected nor acted on as a file: the
   selection and the commands are as ADR-130 says, and the stop is the
   window's, left as soon as the cursor moves by any other way (a press,
   a command, a name typed).
2. **A group folds** by a chevron at its heading's start (a press
   elsewhere on the heading still selects), Left and Right on its heading,
   and its menu's Fold the others and Unfold all. A folded group's
   entries leave the pane's order, as a closed folder's do in the tree:
   not drawn, passed over by the keys, left out of Select All, of a range
   and of what commands act on; its heading stays, with its count.
   Folding is a command (`group_fold`), per pane, kept while the pane is
   grouped by the same criterion, its direction changed included,
   forgotten when it is grouped by another and on restart.
3. **Grouped, the Board's columns are the groups** (amends ADR-130's
   phase 5), in their order, each column's cards its entries in the
   sort's order, in place of the folder's folders or a tag's tags; a card
   dragged to another column does nothing, a group being what a file is,
   not where. Not grouped, the Board is as before. Grouped, it closes
   once the folders it had opened in place, their files being in no
   group of the pane's. (Amended 2026-11-13: as many columns as a folder's
   folders get, `MAX_COLUMNS`; past them the last column holds the cards
   of the groups left, in order, saying how many, so a search grouped by
   Folder lays out a dozen columns, not thousands, and no card goes
   unseen.)
4. **Criteria read on demand**: Taken (the camera's clock of pictures and
   films), Dimensions (a picture's size in pixels: by its area, then its
   width) and Duration (how long a sound or a film lasts), each a
   `SortKey` with a field (`SortKey::field`), offered while a provider
   reads that field (`Fields::knows`). Sorted or grouped by one, the pane
   asks the field of every entry it holds that may have it (a file, not
   a folder: `SortKey::applies_to`), read on a worker in the entry's lane
   (`App::fields`, kept by the file's identity in memory and the cache),
   and what comes is the entry's, a value or none
   (`Listing::set_fields`); what is not read yet sorts after the rest, as
   a size before its stat, and is placed as it comes. The entries of the
   folders opened in place are asked alike, as they are in the pane's
   order; an answer not given (no provider, a read that failed) leaves
   the field unknown, asked again once the providers change (amended
   2026-11-13: a none was kept for good). What was read stays
   the entry's while its size and its modification time do
   (`keys::READ_ON_DEMAND`). Taken groups as Modified does, by the
   camera's clock, which has no zone (the user's days, read as if in
   their zone); Dimensions by classes (under 1, 1 to 4, 4 to 12, 12 to 24,
   over 24 megapixels); Duration by lengths (under a minute, 1 to 5 and 5
   to 20 minutes, 20 minutes to an hour, over an hour). What has none is
   a group of its own (No date, No dimensions, No duration). No filter
   term keeps these groups, a filter not reading files: their menus
   offer no Show only these.
5. **The readers**: Taken the geo plugin's, as now: a picture's EXIF, a
   film's Apple keys (`com.apple.quicktime.creationdate`); a movie
   header's creation time is UTC, often 0, and no camera's clock.
   Dimensions from a picture's header (`imagesize`, already in the tree
   for the print helper: no picture decoded). Duration from an MP4's or
   a QuickTime's own movie header (`skwir_platform::mp4::duration`),
   else from ffprobe where it is installed, run sandboxed as the video
   preview runs it (ADR-000, 4: the environment's tools first), given 10
   seconds; without it, a sound's duration is none. Both are the query
   plugin's provider (`groups::Facts`, `skwir_platform::facts`), offered
   while that plugin is on.

## Consequences

- A second kind of cursor, the window's alone: every way the cursor
  moves leaves it, which the shell checks at each sync (the core's cursor
  having moved).
- A folded group's span holds none of the order's entries (`len` 0) and
  keeps its count; every map from rows to positions (`RowMap`, the grid's
  lines) holds headings with no entries.
- Sorting a folder of films by duration the first time runs ffprobe once
  a film it cannot read itself; the answers are kept, and the order fills
  in as they come.
- AccessKit's AT-SPI adapter (0.19) says of no element whether it is
  expanded: on Linux a reader hears a group folded as its entries gone,
  its heading and count kept.
