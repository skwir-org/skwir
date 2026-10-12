# ADR-054: A name already taken

Status: accepted, 2026-10-12.

## Context

- A copy or a move onto a name already taken in the destination was a
  conflict: the plan's dry run found it and the plan, asked for as one
  action, applied nothing ("“notes.txt” is already there"). Pasting a file
  into its own folder failed the same way ("it would be copied onto
  itself"), and two files of one name copied together (from a search's
  results) passed the dry run and failed halfway.
- Other file managers ask: Replace, Skip, Keep both (GNOME names the new
  one `name (copy).ext`), Merge for a folder, each for one item or all.
- What Skwir holds to: a single action applies at once and Undo takes
  all of it back; a plan is data, reviewed, that agents and scripts make
  too; nothing is lost without a question; the dry run happens before
  anything is applied.

## Decision

1. **Asked once, before anything is done.** The dry run finds every name
   taken (`op::clash::clashes`). A plan the user asked for as one action
   asks then (`Executor::set_clash_asker`, as the room is asked): one
   dialog lists them all, nothing applied until it is answered; Cancel
   applies nothing.
2. **The choices.** For a file: Keep both, Replace, Replace if newer,
   Skip. For a folder onto a folder: Merge, Keep both, Skip. A file onto
   a folder or a folder onto a file: a file's choices. Each choice is
   made for one item or for all of a kind. Enter keeps both, and merges
   folders: nothing goes to the Trash unless chosen.
3. **Replace puts what was there in the Trash**, its companions with it;
   Undo brings it back. Replace if newer replaces when the item copied or
   moved changed later than the one there, else skips it.
4. **Merge** goes into the folder: what is not there is copied or moved
   whole; a folder that is there is merged in turn; a file that is there
   follows the files' choice. A folder a merge moved everything out of is
   removed; one it left something in stays.
5. **Keep both names the new one as GNOME does**: `name (copy).ext`, then
   `name (copy 2).ext`, `(copy 3)`…: a name that already ends so counts on
   (`a (copy).txt` gives `a (copy 2).txt`); the extension is the last one,
   or a compressed tar's two (`.tar.gz`); a folder's name and a dotfile's
   have none (`.bashrc (copy)`). The words are not translated: a file's
   name does not change with the window's language.
6. **Never asked:** a copy into its own folder keeps both (a duplicate);
   two items of one name going to one folder keep both, in order; an item
   already there whole (the same kinds, sizes and times all the way down)
   is skipped by a copy, and a move puts the original in the Trash; a
   folder into itself is refused. A link is what it is, never merged as
   the folder it leads to; a move out of the trash keeps a restore's
   rules.
7. **The answers are the plan.** Resolving rewrites the plan with what
   it will do (`op::clash::resolve`): a copy or move under its new name, a
   Trash step before what replaces, the steps of a merge, an `Rmdir` for
   each folder a move-merge emptied. Its steps are checked again, with
   what each earlier step does foreseen (`StepContext::foresee`: a name
   freed, a name taken); Undo takes the plan back step by step.
8. **Agents and scripts** say beforehand: `copy` and `move` take
   `on_conflict` (`files`: `keep_both`, `replace`, `replace_if_newer`,
   `skip`; `folders`: `merge`, `keep_both`, `skip`), carried by the plan
   as data (`PlanData::on_conflict`). A plan with none, proposed for
   review, is shown as the defaults would settle it and asks the person
   who applies it; run where no one can be asked (no window), it applies
   nothing and says what is taken.

## Consequences

- Paste, drops, the split's transfers and the commands share the dialog:
  they all make `Copy` and `Move` steps.
- A merge's plan can hold many steps; the bar counts them as it counts a
  plan's items.
- What a check reads of the destination may change before the plan runs:
  a name taken in between still fails that step, as before, never
  replacing silently (`rename_noreplace`, staged copies).
- A plan that trashes a path and then puts something there is undone by
  trashing the new one and restoring the old: undoing a trash must
  restore its own item, so on Linux it names its record
  (`Restore::item`), as macOS names where the item went. Windows' trash
  does not say: there the latest item from the path comes back
  (PORTING.md).
- `link` into a folder where the name is taken still refuses: a link is
  not a copy of anything to keep beside.
