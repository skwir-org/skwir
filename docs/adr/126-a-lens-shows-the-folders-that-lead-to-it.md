# ADR-126: A lens shows the folders that lead to what it keeps

Status: accepted 2026-11-09 with the user, who tried each form of it in a
window before it was kept. Built on Linux; Windows and macOS compiled.
Amends ADR-107 (a lens "keeps every folder"), ADR-098 (5, "every folder
staying") and ADR-026 (the index hints, the walk answers: one exception,
below).

## Context

- **A term filtered the files and left every folder.** With `tag:config`
  over a folder, every folder inside stayed, those with no such tag
  anywhere below them too. ADR-107 kept them so that what is under a
  folder can be reached; it left nothing telling the folders that lead
  somewhere from those that do not.
- **A filter was becoming a search that keeps the tree.** `Ctrl+F` finds
  through the folders below and `Ctrl+Shift+F` shows this one; the user
  asked whether they could be one mechanism, the first flattening what
  it finds and the second keeping the pruned tree, whatever was typed.
- **What a term means decides how long it lasts (ADR-107), and so whether
  it needs a way down.** A lens stays as the pane goes on: opening a
  folder it left standing goes on filtering, so the folder has to lead
  somewhere. Words go with the location: a folder they left standing
  would open unfiltered, and the way would lead nowhere.
- **A look ends at its first hit.** A folder that leads somewhere answers
  quickly; one that leads nowhere is walked to its end. So the slow
  answers are the folders that lead nowhere.

## Decision

1. **What stays looks below; what goes stays here.** A lens's terms (a
   kind, an extension, a tag, a date, a size, a rating) look below the
   folders shown. The Name pill's words, patterns and `name:` are one
   term (`lang::Term::Name`) and hold at this level, as ADR-107 has it;
   "This folder and its subfolders" (`query.below`) searches for them
   below. The pills say which is which: there is no third kind.
2. **Whether a folder leads somewhere is a search of it**
   (`registry::LeadsBelow`, the query plugin's), ended at its first hit,
   the folder itself considered: what a search would find, the folders it
   leaves out and the pane's hidden-files switch included. One per folder
   the pane lists (`Pane::folders_to_ask`).
3. **Folders appear as they are found.** One that can be looked into
   starts unshown (`Leading::unproved`) and shows once a search found
   something in it, as a search's results come. The slow answers then
   change nothing on the screen. A folder the lens keeps by itself (its
   own tag) shows at once.
4. **What cannot be looked into stands from the start**, nothing having
   proved it leads nowhere: a link (never gone through), a remote folder
   (never walked unasked, ADR-077), a folder that cannot be read
   (`LeadsBelow` answers `None`), and any entry of a listing that is no
   folder's (a tag's place, the trash).
5. **A folder shown only as the way is dimmed** (`Pane::only_leads`, at
   `ONLY_LEADS_OPACITY`, a screen reader told so). Whether it is depends
   on the lens's test of the folder alone, so it does not flicker with
   what a look finds. Words dim nothing.
6. **A lens lands whole when it can.** Set by hand, the view stays as it
   was (`KeptView`) until the folders have answered, for 150 ms at most
   (`VIEW_KEPT`). A lens replaced again within 300 ms is being typed: its
   looks and the view wait for the keys to stop. A pane with no folder to
   look into filters at once. Going to another folder has no view to
   keep: its files show first, the folders as they come.
7. **One slow folder holds back no other.** Up to four are looked into at
   once (fewer on few workers, one worker left free), at background
   priority; what was found is told together, 150 ms after the first
   found since the last.
8. **The pane says it is looking** while answers are out and the view is
   no longer kept (`Pane::looks_below`): "Looking in folders…" in the
   footer, and "Looking in the folders here…" where it would say "Hidden
   by filters".
9. **Asked again** after a plan, about the folders above what it changed
   (`Pane::ask_again_above`), and about every folder on F5 and when the
   hidden-files switch moves: what is shown stays until the new answers
   come, and what was being looked into is looked into anew. A folder
   renamed keeps its answer: a new name is no new folder to prove.
10. **An index that covers the folder answers this one question**
    (`HintProvider::covers`, the tags index for a folder it covers): no
    walk. Searches still walk (ADR-026). A folder asked about again after
    a change or F5 walks too: the index is told of a plan's changes after
    it, and may not have them yet.

## Rejected

- **Folders that leave as they are proved empty.** Built first and tried:
  the proof comes last for the folders that show nothing, so rows went
  seconds after the user had started reading. "Disappearing for a search
  or filter is kind of counterintuitive."
- **Keeping the view until every folder has answered.** Proving a folder
  empty takes as long as the folder.
- **Dimming every folder, hiding none.** Prunes nothing.
- **Looking below for words too.** A word would have to stay as the pane
  goes on, or its way would lead nowhere; `re` is under nearly every
  folder, so it would stop narrowing as it is typed; folders arriving at
  the top would move what Enter opens; and `Ctrl+F` in the Tree view
  (ADR-127) already does it.
- **`*.md` read as `ext:md`, or patterns apart from plain words.** The
  first is an exception for one shape; the second would split one pill
  whose two halves look the same.

## Consequences

- A change made outside Skwir deep in a folder is not seen until the lens
  or the location changes or F5: only Skwir's own plans, F5 and the
  hidden-files switch ask again.
- What a covering index says is as fresh as the sidebar's counts.
- With `kind:` every folder shown is dimmed: a folder is never an image.
- A folder lit by its own tag shows what the lens keeps inside it, as any
  folder under a lens does.
- A timeline bar over a folder counts its files alone; pressing it may
  also show the folders that lead to that period (ADR-107's amendment of
  2026-11-07 said every folder stayed).
- A lens over a large tree walks it, once per folder and lens, at
  background priority, cancelled with the lens, the location or the pane.
