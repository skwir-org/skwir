# ADR-145: A dialog lists files its own way

Status: accepted 2026-11-21, on the user's answers (asked whether the
windows answering other programs' Open and Save dialogs should keep
their own sorting and grouping, a folder's remembered view included,
"we might want a different view from the Download folder from the file
chooser and the main window"; chose them all apart, folders first too,
after the research below). Amends ADR-144 (decision 4) and ADR-132.

## Context

- **What a dialog kept** (ADR-144, 4): its own layout and size, and a
  view apart for pictures and films. Its order was not kept: each
  dialog's pane started sorted by name, hidden files off; and a folder
  that remembered its order or view (ADR-132) showed it in a dialog too,
  so that a choice made for the user's window decided a dialog's.
  Folders first was the user's setting in every pane, a dialog's too.
- **What others do**, researched 2026-11-21:

  | Program | The dialog's view settings | A folder's own in dialogs |
  |---|---|---|
  | GTK (GNOME's, Xfce's, Cinnamon's dialogs) | its own, every program's dialogs one (`org.gtk.gtk4.Settings.FileChooser`: sort column and order, folders first, view, hidden files), apart from the file manager | none; GNOME took per-folder views out of its file manager after testing them with users |
  | KDE | its own (`kdeglobals`, `[KFileDialog Settings]`: sort, reversed, view style, hidden files), apart from Dolphin | none; Dolphin's per-folder views are not followed (an open wishlist; the maintainers: two different programs) |
  | COSMIC Files, a file manager that is its desktop's dialog | a separate `dialog` configuration (view, icon sizes, folders first, hidden files) and the dialog's own sort | none; per-folder sorts are the main window's alone |
  | Windows | per program | yes, apart from Explorer's (the registry's bags: `ComDlg` beside `Shell`, per folder) |
  | macOS | not confirmed (reports: one view mode for every panel, not Finder's) | — |

## Decision

1. **The dialogs keep their own order**: the sort and its direction,
   the groups and theirs, hidden files and folders first, one for every
   dialog, the one the last dialog was left in (`core.chooser_order`,
   the state file). Until one was, a dialog starts in the order of the
   pane in use in the user's window used last, folders first theirs
   (`Workspace::dialog_order`). A dialog's window is marked so as it is
   made (`App::open_dialog_window`), so that it is sorted as the dialogs
   list from its first listing on.
2. **What a folder remembers in a dialog is the dialogs'**: Remember for
   this folder there keeps its order and view in a list of their own
   (`core.chooser_orders`, the data file, records as `core.orders`
   holds), and what the folder remembers in the user's windows is not
   shown there, nor the dialogs' in the user's windows. Both lists follow
   what a plan renames, moves, trashes or deletes, as ADR-132 says.
3. **Folders first is never a folder's own** (ADR-132, reworded: what
   was said as "the user's, in every pane" meant that no folder keeps
   one): the user's setting in their windows, the dialogs' own in a
   dialog's. The View menu's row runs `sort` with `folders_first`, which
   sets the one of the window it is run in; the user's setting changed
   so, an agent's call asks the user first, as one of `settings.apply`
   does (`Declared::effect_by`, ADR-044; amended after review,
   2026-11-21).
4. **Text sizes stay one**: how large the user reads, not how a folder is
   listed.

## Consequences

- Every lookup of what a place remembers names the pane it is for
  (`Workspace::order_of(pane, at)`, `view_of`, `orders_for`), the kind of
  its window choosing the list.
- A dialog's order is kept as it changes (`sort`, `group`,
  `toggle_hidden`, `show_companions`: a folder of companions only), from
  the pane's own order, never a folder's. Show companion files stays the
  user's one setting, every window's, a dialog's too (ADR-028).
- The Settings page's Folders first is the user's windows': dialogs have
  no Settings (ADR-144), and change theirs from their View menu.

## Rejected

- **Following the user's window's per-folder memory, a dialog's own on
  top** (one list, a dialog's records winning): one folder's two records
  would be told apart only by where they were made, and a record made in
  the user's window would still decide a dialog's first look.
- **Folders first shared**: every dialog the research found keeps its
  own.
