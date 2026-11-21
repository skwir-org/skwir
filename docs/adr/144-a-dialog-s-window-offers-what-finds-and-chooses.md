# ADR-144: A dialog's window offers what finds and chooses

Status: accepted 2026-11-20, on the user's answers (asked for a simpler
window when Skwir answers another program's Open or Save dialog, and
for its own settings, the preview on in their windows but rarely in a
dialog; took the proposal whole: what a dialog offers, a layout of the
dialogs' own, and seven further ideas). Amends ADR-141 (decisions 4 and
7). Built on Linux, which alone has these dialogs (ADR-141, 11), the
same day (2f32dfaa..dd85c5b5), each step checked in a window on Xvfb;
amended as built (decisions 1, 4, 6, 7 and 10), and after review
(decision 1: one answer every way to run a command asks, questions
asked where the dialog's action asked them, Undo of its own; decision 2:
one tab). Decision 8 amended 2026-11-20, on the user's asking: the
folders are a group of the sidebar, not a dropdown of the bar; and
2026-11-21, on the user's choices: none gone, ten kept and five shown,
said with Undo, Skwir Tabs above them. Decision 4 amended by ADR-145
(the dialogs' own order and per-folder memory).

## Context

- **A dialog's window was a whole window** (ADR-141, 4, the user's
  choice over a compact dialog: Skwir's ways to find a file kept). With
  them came what has nothing to do with choosing: tabs, the split, the
  terminal, the pinned player, Activity, Settings, the palette's every
  command, Usage, Open With, compressing and printing. A rule a dialog
  needs had to be added beside each of them, or a key or the palette
  reached what the window did not show.
- **Its layout was the last window's**, copied as it opened (unsplit, a
  dialog's size at most) and never kept (ADR-141, 4): a preview turned
  off in a dialog was on again in the next one if the user's own window
  showed it, and a terminal left open below their files opened below a
  dialog's.
- **What others do.** GTK's dialog and KDE's keep one layout for every
  dialog (GTK's `org.gtk.gtk4.Settings.FileChooser`: its size, view,
  sort, hidden files; KDE's `KFileDialog Settings` in `kdeglobals`:
  preview, places, view), apart from the file managers'. macOS keeps a
  panel's size and view by application and apart for Open and Save;
  Windows by application (`IFileDialog::SetClientGuid`). GTK's dialog
  starts in Recent when nothing names a folder, adds what is chosen to
  the recently used files, and changes a name's extension with the
  filter; macOS's Save panel offers recent places under Where, and a
  file dropped on a panel goes to its folder.
- **Every command names its topic** (ADR-100): what it is about (going
  places, files, tabs, the terminal, the player, Skwir itself). Views say
  what they are for (`ViewRole`: a listing, an alternate shape, a lens
  that answers a question of its own).

## Decision

1. **A dialog's window offers what finds and chooses a file, and only
   that.** One answer says how that window takes a command run with its
   arguments (`App::in_dialogs`: offered; run only where a part of the
   window asks it, the rows' data or a setting a menu's switch saves; or
   not), and every way to run a command there asks it
   (`Shell::in_window`): keys and F1's list, the palette, menus, toolbar
   buttons and popups' rows, the address field's rows, pickers and
   prompts, Properties' links, a toast's buttons, and what the window
   asks of itself (pinning what plays, Activity opening). A command says
   it where it is registered (`Declared::in_dialogs`, `not_in_dialogs`,
   or by its arguments, `in_dialogs_by`), else its topic says it:
   going places, selecting, finding, notes, views, previews and what
   plays in them, places and the window's own parts are offered; files,
   tabs and the split, the terminal and Skwir itself are not. Whatever
   its topic, what changes files (`Effect::Plans`) and what opens
   another program (`Declared::opens_elsewhere`: an address, a file in
   its application, the desktop's maps) are offered only where they say
   so; nor are the pinned player (pinning, a menu's Play, Keep playing),
   Activity, a lens and its own commands (Usage's, the Map's), a
   slideshow, what changes the user's servers or publishes a file, a
   picture's position rewritten, a plugin from outside's command (a
   program or a module may change files or start programs, which the host
   cannot tell, until its manifest can say a command is fit for a
   dialog), nor a command that leads to one the
   window does not offer (`Declared::leads_to`: a picker's source and
   choices, a prompt's command, an item of the preview's menu: Open
   With's picker, Print…, the split's panes placed by `ui.layout`).
   Of what changes files, a dialog offers what a dialog has always
   offered: making a folder, renaming, moving to the Trash, the
   clipboard and a paste, Undo, Redo and Retry of what that window did
   alone, never of the user's actions in their own windows
   (`Executor::undo_in`, `redo_in`, `last_stopped_of`; its footer's Undo
   and More name those, and the History is not offered), the shelf (several
   files from several folders), Properties, and the notes a file is
   found by (its tags, rating and comment; never a tag renamed or taken
   from every file); of what opens another program, `open`, which
   answers the dialog there; of the window's, its own buttons, the
   palette, the list of keys and Close tab (its window's last). What it
   does not offer is not drawn nor listed, and refused if asked; a key
   bound to it does what it would with nothing bound (a binding below
   it, a field's typing). Views are offered by their role: listings and
   alternate shapes, never a lens (Usage, the Map). Ctrl+Q there is the
   dialog's Cancel, bound in its `choosing` context; Quit is not offered
   there (amended 2026-11-20). What the dialog's own action asks the
   user (a server's password or whether to trust it, a volume's
   passphrase, names a paste found taken) is asked in the dialog's
   window, a question going to the window whose action asked it
   (`app::asking_in`); only what agents, scripts and plugins' work of its
   own ask waits for a window of the user's own (ADR-141, 8, which keeps
   agents and scripts from answering a dialog, not the dialog from
   asking). Amended after review (2026-11-20): as first
   built, each surface asked a part of the rule (keys and menus one,
   the palette another, pickers a third), checks beside features (the
   terminal, the split) stood beside it, and a command about notes or
   places that rewrote pictures or opened a program was offered by its
   topic.
2. **What it does not offer is not drawn either**: no tab bar, no split,
   no terminal panel, no pinned player (a film or a song still plays in
   the preview), no Activity panel (what the user does there is said
   as anywhere else), no Settings, and the palette offers no setting.
   It holds one tab (`WindowTabs::holds_tabs`): a place asked in a tab
   there (a middle click, Ctrl+Enter, a volume mounted for a tab) opens
   in place (`App::new_tab`), a step Back returns from, its menu offers
   no tab, and no tab moves into it, so Ctrl+W, its last tab's Close,
   still cancels the dialog (amended after review, 2026-11-20). F1 there
   lists the sidebar's Ctrl+Enter nowhere: it does what Enter does.
3. **A place whose files are not files to give** (the Trash: a trashed
   file is gone from where it was) says so
   (`LocationProvider::gives_files`), and a dialog's window offers it
   nowhere (`App::places_offered`: not in the sidebar, its Places
   heading's menu nor the address field's places), and its files answer
   nothing: reached by its address typed, Enter on one says they cannot
   be chosen.
4. **Dialogs remember their own layout** (`ui.chooser_session`), read
   and written only by dialogs' windows, as a window of the user's own
   remembers its (ADR-122, `ui.session`): a panel turned off in a
   dialog stays off in the next, and the user's own windows never see
   it. The first dialog takes how the user's window shows its files (its
   view, zoom, columns) and arranges its sidebar, and no panel: no
   preview, no Properties, no timeline, no Quick filters. Its size is
   kept too, the dialog's size (1000 × 680 at 100 %) its start: the size
   it is given while it floats, never one the compositor tiles it at
   (`Placement::of_dialog`, by the tiled states Wayland's compositors
   say; X11's window managers say none, amended as reviewed,
   2026-11-20). Preferences stay the program's, one for all its
   windows: hidden files, sorting, folders first, text sizes. A folder
   that remembers its view (ADR-132) shows it in a dialog too. (Amended
   by ADR-145, 2026-11-21: the dialogs keep their own order, hidden
   files and folders first, and what a folder remembers there is theirs;
   text sizes stay one.)
5. **What is asked chooses the view.** A request whose filter in use
   asks only for pictures or films starts in the view the dialogs used
   last for such a request (the grid at first, its thumbnails what the
   files are told by); any other, in the one the dialogs used last for
   any other. The rest of the layout is one for every dialog.
6. **Saving, the filter gives the name its extension**: choosing another
   filter replaces an extension one of the program's filters takes
   (`*.png` for PNG in place of `*.jpg`, after All files too), with the
   new filter's first plain pattern's, else its first type's (the type
   database's first glob); an extension the user typed that none of them
   takes is theirs, kept, and All files changes nothing. As built and
   reviewed: a filter takes a name as it admits a file of it (a type every
   extension of its own and of what descends from it), a name the new
   filter takes already is left as it is, a compound extension the type
   database knows (`.tar.gz`) is replaced whole, and the name's bytes are
   kept but its extension's (`chooser::swapped_name`). What takes every
   extension names none: the program's own All files (`*` or `*/*`) and
   a `*.*` beside its real filters make no extension the program's.
7. **Recent files.** Opening, Recent comes first in its group of the
   places, the program's filter applied there as in any folder; a
   request naming no folder from a program that chose in none starts
   there (amends ADR-141, 7; saving still starts in the home folder).
   What is chosen is added to the desktop's recently used files
   (`recently-used.xbel`), as the desktop's own dialog adds it, unless
   the desktop is told to remember none: `gtk-recent-files-enabled` false
   or `gtk-recent-files-max-age` 0 in GTK's settings (the user's, then
   the system's), GNOME's privacy `remember-recent-files` off (which GTK
   4 reads through the settings portal on any desktop), Cinnamon's on
   Cinnamon, KDE's `[RecentDocuments]` `UseRecent` false or `MaxEntries` 0
   in `kdeglobals`; one of them is enough. A bookmark the list holds is
   visited and Skwir's count of it goes up, another is added, typed by
   what the file is, each file once, the rest kept byte for byte, the
   list kept as GTK keeps it (a file older than an age a setting says
   dropped, then the thousand bookmarks it keeps); a list GLib's reader
   would refuse is left for its programs to mend, and Skwir never writes
   one it would refuse. Where the desktop remembers none, or its list
   names no file that is there (read as the request comes, half a second
   at most), Recent is neither first nor where the window starts, which
   is home (amended as reviewed, 2026-11-20).
8. **A program's recent folders**: the folders each program last chose
   in, newest first (`ui.chooser_folders`, ten a program, for the 50
   programs that chose last, its one folder a program before read as its
   list), are the sidebar's Recent Folders, a group second as the
   sidebar ships, under Skwir Tabs, folded, moved and put away as any
   other: five at most, none gone (looked for as the request comes, half
   a second at most, kept in the record, where the program's next
   choices replace it), none that another place the sidebar shows leads
   to (in a section folded too). The dialog starts in the first of them
   there. Each is taken out alone (Remove From Sidebar) and the
   heading's menu empties the group (Clear Recent Folders), which forget
   where the program chose, each said with its Undo; with none left, the
   program's next dialog starts as one that chose nowhere would. Above
   them, Skwir Tabs lists what the tabs of the user's own windows show
   (the user's name: in another program's dialog, it says whose tabs),
   each window's shown tab first, the window used last first, then the
   others, the one left last first, five at most, none another place
   shows, Recent Folders leaving out what it lists; nothing of it is
   arranged or taken out (`Place::listed`), and it follows the tabs as
   the window is drawn again. Two folders of one name are told apart
   after it by the first folder above that differs (`skwir/…`), as any
   two places of a group shown under one name are, and a folder's place
   says its whole path in its tooltip (amended 2026-11-20: they were a
   dropdown of the bar, a second way to go places beside the sidebar's,
   two clicks away; amended 2026-11-21, on the user's choices: none
   gone, ten kept and five shown, said with Undo, Skwir Tabs above).
9. **Choosing folders, files are shown dimmed**: they say where one is,
   and none can be chosen.
10. **A file dropped on a dialog's window is gone to**, from a window of
    the user's own or another program, wherever it lands (the files, a
    folder, a place): opening, the first's folder is shown and it is
    selected, with the others dropped from that folder where the request
    takes several; saving, a folder dropped is gone into, a file's
    folder shown and its name taken. A drop never answers by itself, nor
    copies or moves anything.
11. **The answer says how many**: "Open 3 files", "Select 2 folders",
    where the label is Skwir's; a program's own label (`accept_label`)
    is said as it gave it.

## Consequences

- A command registered without a word about dialogs is offered there by
  its topic: a plugin's new command about files is not, one about
  finding is, and one that changes files or opens another program is
  not, whatever it is about (`crates/skwir/tests/dialogs_offer.rs` lists
  those a dialog offers).
- A dialog's window is a window with less: what it offers runs as in
  any window, and the window does not grow a mode beside each feature.
- `ui.session` stays the user's windows' alone; `ui.chooser_session` is
  written as a dialog's layout changes.
- Skwir writes a file of the desktop's (`recently-used.xbel`) for the
  first time: written beside and put in its place (`file::take_place`)
  after reading it again, so that what another program wrote just before
  is kept; one written in the same instant may be lost, as with GTK's own
  writer. Skwir's own answers write it one at a time.

## Rejected

- **A Settings page for dialogs** (switches for the preview, the view,
  the sidebar): what a dialog's window remembers already says it, and a
  second set of switches would say it twice.
- **A layout for each program** (macOS's, Windows'): the programs that
  ask without a sandbox give no id (ADR-141, 7), so most would share one
  anyway; what is asked (pictures or not) tells more.
- **A dialog's mode beside each feature** (a check in the toolbar, one
  in the menus, one in the keymap): ADR-000's structural rule; one
  answer, the command's.
