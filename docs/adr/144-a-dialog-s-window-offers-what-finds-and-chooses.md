# ADR-144: A dialog's window offers what finds and chooses

Status: accepted 2026-11-20, on the user's answers (asked for a simpler
window when Skwir answers another program's Open or Save dialog, and
for its own settings, the preview on in their windows but rarely in a
dialog; took the proposal whole: what a dialog offers, a layout of the
dialogs' own, and seven further ideas). Amends ADR-141 (decisions 4 and
7). Built on Linux, which alone has these dialogs (ADR-141, 11).

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
   that.** Each command says whether it does
   (`CommandSpec::offered_in_dialogs`), by its topic unless it says
   otherwise where it is registered (`Declared::in_dialogs`,
   `not_in_dialogs`): going places, selecting, finding, notes, views,
   previews and what plays in them, places and the window's own parts
   are offered; files, tabs and the split, the terminal and Skwir itself
   are not, nor the pinned player (pinning, a menu's Play), Activity, a
   lens's own commands, a slideshow, nor a picker or a prompt of the
   window's that leads to what it does not offer (Open With's Other
   Application…, its source about files).
   Of the files' commands, a dialog offers what a dialog has always
   offered: making a folder, renaming, moving to the Trash, the
   clipboard, Undo and Redo of these, the shelf (several files from
   several folders), Properties; of the window's, its own buttons, the
   palette, the list of keys, Close tab (its window's last) and Quit.
   What no list offers, run by the window's parts where they ask it (the
   rows' data, a setting a menu's switch saves), runs there too
   (`CommandSpec::runs_in_dialogs`). One rule answers every way to run a
   command in that window (keys, the palette, menus, toolbar buttons,
   what the window asks of itself; `Shell::offers`): what it does not
   offer is not shown, and refused if asked. Views are offered by their
   role: listings and alternate shapes, never a lens (Usage). Quit,
   asked there, cancels the dialog alone (built just before, e780d5ed).
2. **What it does not offer is not drawn either**: no tab bar, no split,
   no terminal panel, no pinned player (a film or a song still plays in
   the preview), no Activity panel (what the user does there is said
   as anywhere else), no Settings, and the palette offers no setting.
3. **A place whose files are not files to give** (the Trash: a trashed
   file is gone from where it was) says so
   (`LocationProvider::gives_files`), and a dialog's window offers it
   nowhere: not in the sidebar, and its files answer nothing.
4. **Dialogs remember their own layout** (`ui.chooser_session`), read
   and written only by dialogs' windows, as a window of the user's own
   remembers its (ADR-122, `ui.session`): a panel turned off in a
   dialog stays off in the next, and the user's own windows never see
   it. The first dialog takes the view and zoom of the user's window and
   nothing else: no preview, no Properties, no timeline, no Quick
   filters. Its size is kept too, the dialog's size (1000 × 680 at
   100 %) its start. Preferences stay the program's, one for all its
   windows: hidden files, sorting, folders first, text sizes. A folder
   that remembers its view (ADR-132) shows it in a dialog too.
5. **What is asked chooses the view.** A request whose filter in use
   asks only for pictures or films starts in the view the dialogs used
   last for such a request (the grid at first, its thumbnails what the
   files are told by); any other, in the one the dialogs used last for
   any other. The rest of the layout is one for every dialog.
6. **Saving, the filter gives the name its extension**: choosing another
   filter replaces an extension the one before took (`*.png` for PNG
   after `*.jpg`), the new filter's first plain pattern's, else its
   first type's (the type database's first glob); an extension the user
   typed that the filter before did not take is theirs, kept, and All
   files changes nothing.
7. **Recent files.** Opening, Recent is the first of the places, the
   program's filter applied there as in any folder; a request naming no
   folder from a program that chose in none starts there (amends
   ADR-141, 7; saving still starts in the home folder). What is chosen
   is added to the desktop's recently used files
   (`recently-used.xbel`), as the desktop's own dialog adds it, unless
   the desktop is told to remember none (`gtk-recent-files-enabled` false
   in GTK's settings, as GTK reads it).
8. **A program's recent folders**: the folders each program last chose
   in, five at most, newest first, are offered in the bar
   (`ui.chooser_folders`, which keeps them for the 50 programs that
   asked last, its one folder a program before read as its list).
9. **Choosing folders, files are shown dimmed**: they say where one is,
   and none can be chosen.
10. **A file dropped on a dialog's window is gone to**: opening, its
    folder is shown and it is selected (several, if the request takes
    several and they share a folder; else the shelf takes them); saving,
    its folder is shown and its name taken. A drop never answers by
    itself, nor copies or moves anything.
11. **The answer says how many**: "Open 3 files", "Select 2 folders",
    where the label is Skwir's; a program's own label (`accept_label`)
    is said as it gave it.

## Consequences

- A command registered without a word about dialogs is offered there by
  its topic: a plugin's new command about files is not, one about
  finding is.
- A dialog's window is a window with less: what it offers runs as in
  any window, and the window does not grow a mode beside each feature.
- `ui.session` stays the user's windows' alone; `ui.chooser_session` is
  written as a dialog's layout changes.
- Skwir writes a file of the desktop's (`recently-used.xbel`) for the
  first time: written beside and put in its place (`file::take_place`)
  after reading it again, so that what another program wrote just before
  is kept; one written in the same instant may be lost, as with GTK's own
  writer.

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
