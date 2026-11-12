# ADR-132: A folder remembers its order when asked

Status: accepted, 2026-11-12, on the user's answers (asked in the session
that built it). Built the same day and run on Linux. Settles what
ADR-130 left "for an ADR of its own" (decision 11).

## Context

- **The order is the pane's** (ADR-130, 11): a pane keeps its sort and
  its groups as it goes from folder to folder, and its tab keeps them for
  the next start. Folders first is the user's, in every pane. A folder
  that wants an order of its own (Downloads newest first, pictures by
  month) was sorted again by hand each time.
- **What other file managers do**, researched 2026-11-12:

  | Program | Where | When it writes one |
  |---|---|---|
  | Dolphin (since 24.12) | an attribute on the folder, `user.kde.fm.viewproperties#1`, INI text (`SortRole`, `SortOrder`, `GroupedSorting`, `ViewMode`…); `.directory` files before | each folder whose view is changed, when it remembers views per folder |
  | GNOME Files | GVfs metadata, `nautilus-icon-view-sort-by`, `-sort-reversed` | every folder whose sort is changed: it has no window-wide sort |
  | Thunar | GVfs metadata, `thunar-sort-column`, `thunar-sort-order` | only with its per-folder option on |
  | Finder | `.DS_Store`, undocumented, `sortColumn` in a plist | folders it showed |
  | Explorer | the registry's bags, binary, keyed by item ID lists, no API | every folder visited |

## Decision

1. **A folder remembers its order when the user asks it to**: View's
   "Remember for this folder", under Sort by and Group by and above
   Folders first, and the command `remember_order` (`on`, toggled when
   omitted; `pane`). It remembers the sort and the groups, each with its
   direction (`orders::Order`); hidden files stay the pane's, and
   folders first the user's.
2. **Shown so wherever it is shown**: every pane that shows it, and
   every window. A change of its sort or groups made there (a header, Sort
   by, Group by) changes what it remembers, and every pane showing it
   follows. Folders opened in place and the columns above it take the
   order the pane shows.
3. **The pane's own order is set aside, never overwritten**
   (`Pane::own`): leaving the folder for one that remembers none shows
   the pane's own again, as it was. Turned off, the folder shows the
   pane's own at once (the user's choice: the view changes visibly, and
   the pane's order is not lost). A tab keeps its pane's own
   (`Pane::own_sort`), as does a tab opened from it.
4. **Any place but a search**: the trash, Recent, a tag's place, a
   remote folder. A search is what its terms say; the row is greyed
   there, and the command refuses.
5. **Kept with what the user makes** (ADR-057): the data setting
   `core.orders` in `data.toml`, one record a place (`at`, the
   location; `key`, `ascending`, `group`, `group_ascending`). Read once,
   held as a map (`orders::Orders`): a navigation looks it up in memory
   before opening (`App::open_in`), so the folder is listed and sorted
   once, by the order it remembers (`OpenContext::reads_metadata` from
   it). Saved on a worker; a change made by another skwir is taken in as
   it happens, as every data setting is. A record that does not read (a
   later skwir's criterion, a hand edit) is kept as written; a setting
   that is no list is never written over.
6. **It follows what Skwir sees move**: a plan that renames or moves a
   folder (an undo's and a redo's too) takes along what it and the
   folders below it remember (`Orders::follow`, from the executor's
   observer), two names swapped exchange theirs, a folder deleted for
   good forgets them, a folder trashed keeps them for its restore. A
   folder shown that moves, whatever moved it, takes them along too
   (`PumpReport::orders_moved`). The folders at or below one are found in
   the time of what is there (a path-ordered index), never by a walk of
   every record.
7. **Not read from other file managers** (the user's choice: a niche,
   and their stores' formats would cost the most): Dolphin's attribute
   and GVfs's metadata would be readable (a stable format, a record that
   says a choice); Finder's and Explorer's are not (undocumented; a
   record for every folder shown, a sort chosen not told apart from one
   inherited). Reading them would need, each, to be read only where its
   own program heeds it, one answer per folder in a fixed order, and the
   menu saying where an order comes from.

## Consequences

- A folder moved by another program while no pane shows it loses what
  it remembers until it is moved back, or remembers again: the record
  is keyed by place, not by the file.
- Nothing is written in the user's folders: read-only folders
  (`/nix/store`), volumes without attributes, phones and remotes
  remember alike, on every system.
- A record costs some 100 bytes in memory and in the data file, one for
  each place the user asked to remember.

## Rejected

- **An attribute on the folder** (Dolphin's way): it follows the folder
  anywhere, but needs a folder one may write and a volume that keeps
  attributes (not FAT, nor most remotes, nor Windows through
  `fs::xattr`), changes the folder for backups and sync tools, and is a
  read for every folder opened before its order is known.
- **Every folder remembering what was last done in it** (GNOME Files'
  way): with a sort that is the pane's, a change made anywhere would mark
  a folder; remembering is asked for.
- **The order shown kept as the pane's when the folder forgets**:
  nothing would move, but the folder's order would leak into every folder
  shown after.
