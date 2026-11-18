# ADR-140: Skwir shows files for other programs while it opens folders

Status: accepted 2026-11-18, on the user's answer (asked whether skwir
should take the desktop's "open in folder" requests and be the default
file manager, they chose to start there, before a file chooser for
other programs). Built on Linux; elsewhere refused (decision 7). Amends
ADR-115 and ADR-122: a launch may say what to select, and the program
may start without a window. Amended 2026-11-19, the user asking for the
row in Settings: decision 8.

## Context

- **"Show in folder" is a D-Bus call, not a folder opened.** Firefox's
  downloads, Chromium, Electron programs, editors and most IDEs call
  `org.freedesktop.FileManager1.ShowItems` (the folder opened, the file
  selected) on the session bus; a sandboxed program asks the portal's
  `OpenURI.OpenDirectory`, which calls it for them. gpui does so
  (`gpui_linux`'s `reveal_path_internal`, through the portal). Only when
  no one answers do they open the folder with the default application,
  nothing selected.
- **The bus starts whoever installed a file for the name.** Dolphin ships
  `org.kde.dolphin.FileManager1.service` (`dolphin --daemon`, which then
  holds the name for the session), Nautilus and Nemo their own. So with
  skwir the default for folders, "Show in folder" would still open
  Dolphin on the user's machine, where its daemon held the name: the
  default application answers `xdg-open`, not the bus.
- **The default for folders is already a choice skwir writes**: Open With
  on a folder, Always use, writes `[Default Applications]
  inode/directory=org.skwir.skwir.desktop;` with Undo
  (`AssociationEdit`), as GNOME's and KDE's settings and `xdg-mime
  default` do.
- **The session bus reads the user's service files first**:
  `<standard_session_servicedirs/>` lists
  `$XDG_RUNTIME_DIR/dbus-1/services`, then
  `$XDG_DATA_HOME/dbus-1/services`, then `$XDG_DATA_DIRS`; of two
  files naming one service, the first found is taken (dbus-daemon, and
  dbus-broker 37, whose launcher ignores a later duplicate). Programs
  already install theirs there (Ghostty, KeePassXC's secrets). dbus-daemon
  watches the folders; dbus-broker only those that existed when it read
  them, so a folder made since is read at the bus's `ReloadConfig`.
- **dbus-daemon waits for a program that ends with 0**, taking it for one
  gone to the background that will take the name later: the asker waits
  its 25 seconds. One that ends otherwise fails the request at once.

## Decision

1. **The desktop's default for folders is the one choice.** Skwir
   answers other programs' requests to show folders and files exactly
   while the default the lists name for `inode/directory` (the first
   installed one, as the desktop reads them, `apps::named_default`) is
   skwir's desktop entry, whoever set it: Open With, the desktop's
   settings, `xdg-mime`. No setting of skwir's own, which could disagree
   with the desktop. An application that merely says it opens folders
   (skwir's entry does) is no choice, and does not count.
2. **The program that takes launches over answers** (ADR-122: one per
   build, settings and display; `--new-instance` never does). It serves
   `/org/freedesktop/FileManager1` on the program's one connection and
   asks the name with `ReplaceExisting | AllowReplacement`, waiting in
   the queue where the holder does not let go (Dolphin's daemon holds it
   until it ends): of two skwirs answering (two builds), the one started
   last answers, the other taking it back when that one ends. Whether it
   answers is asked when the program starts and each time the user's
   lists change (`fs::Settled` on `$XDG_CONFIG_HOME`: Open With, the
   desktop, a tool), on that watcher's thread; let go at once when skwir
   is no longer the default. The system's lists (`$XDG_CONFIG_DIRS`, the
   data folders') are read, not followed: a package changes them rarely,
   and decision 5 covers the while until the next start.
3. **What another program asks is a launch.** `ShowFolders`: each folder
   a launch. `ShowItems`: the items grouped by the folder holding them,
   in the order first named, each folder a launch that selects its items
   once listed (`Opening::select`, `Pane::reveal_names`);
   `ShowItemProperties`: the same, Properties shown. Each opens as a
   launch does
   (`ui.launch_opens`: a window, or a tab of the window in use; with no
   window up, the windows come back), and the request's startup id brings
   the last one forward (Wayland's activation token, X11's startup id).
   A program the bus started is a launch naming its folder: nothing of
   the windows left is restored, the request's folder shown in one.
   Only `file:` URIs and absolute paths: an address on a server is the
   remote plugin's to open (ADR-077), never taken from another program.
   The methods answer at once, as Nautilus's and Dolphin's do.
4. **The bus starts skwir when none runs.** While skwir is the default,
   the program that answers keeps
   `$XDG_DATA_HOME/dbus-1/services/org.freedesktop.FileManager1.service`,
   read before Dolphin's or Nautilus's own. Its `Exec` is the program
   skwir's desktop entry runs, found on `PATH` as the desktop finds it (a
   profile's link, `/usr/bin/skwir`), never the path of the build that
   wrote it, which an update or a test build would leave behind; with
   `--file-manager`. When skwir is not the default the file is removed,
   only if it is skwir's (its first line says so): a file of the user's
   own of that name is left as it is; one of skwir's whose program is no
   longer found goes, as it would start nothing. A path the file's grammar
   would need quoted is not written, and the log says why. Each change
   asks the bus to read its files again.
5. **`--file-manager` starts the program without a window.** It takes
   launches and the name over as any start does, the bus hands it the
   request, and the window opens for it as for a launch handed over. A
   program asked nothing within 30 seconds ends. One that finds skwir no
   longer the default (changed while no skwir ran) removes its file and
   ends at once, with status 3 (never 0, which dbus-daemon would wait
   for): the bus then fails the request, and the asking program opens the
   folder with the default application, as it would with no one
   answering. One that finds another skwir of its instance taking
   launches over (the default changed and that one has not read it yet)
   takes the request and hands its launches there, then ends.
6. **A launch says what to select.** `handover::Opening` carries the
   names to select in its folder and whether Properties shows, on the
   bus (`Finishing.Open`) and Windows' pipe (its message's second
   version) alike: both ends are always one build.
7. **Only on Linux.** On macOS the Finder answers every "reveal"
   (`NSWorkspace.activateFileViewerSelecting` names no other); on
   Windows Explorer can be bypassed for folders by a verb of the user's,
   not for the shell's own "Open file location", and a handler that
   fails breaks opening folders everywhere. Neither is attempted, and
   `--file-manager` there says so (`system::not_yet`).
8. **Settings says it** (amended 2026-11-19): General › Startup holds
   "Default file manager", beside "Open folders from other programs in",
   a choice its owner keeps (`SettingSpec::choice_kept_by`, drawn as any
   choice's row), the open-with plugin, whose catalog and
   `open_with.set_default` (its notice offering Undo) it is: the
   applications that open folders to choose from, the default first
   (none named: "Not chosen", with what opens them meanwhile), and, under
   its description, what the choice means for Show in Folder: what choosing skwir gives; that
   skwir shows them; that another program does until it ends
   (`file_manager::shower`, the holder named as its desktop entry names
   it); that skwir cannot be chosen where its entry is not installed.
   Still the desktop's choice, kept nowhere else. Listed from memory,
   the holder asked again on a worker each time the row is listed, so
   a holder that ends while the dialog is open shows at the next
   listing. On Linux alone, where the catalog is read. An essential, as
   it decides what other programs see: in place of Ask what dropped
   files do, by the user's choice (ADR-072, amended 2026-11-19).

## Consequences

- One choice the user already knows, made wherever they make it, says
  both what opens folders and what shows files: nothing to keep in step
  by hand, nothing stored by skwir.
- Skwir writes one file outside its own folders, while the user's choice
  says so, and removes it when it does not; it never touches another
  program's.
- A Dolphin daemon started before skwir was chosen answers until it ends
  (the session's end): the bus gives the name over only when its holder
  lets go.
- Starting from the bus costs a start without a window before the
  request is answered (about 0.1 s for the request, then the window's
  first frame): the first frame's budget (ADR-033) applies.
- The bus starts skwir in the session's activation environment: one that
  lacks the display's variables (a session that never exported them to
  the bus) makes it an instance of its own, which launches from the
  desktop are not handed to.
- The request is answered whatever the asker: the methods only show, and
  a folder shown is what the user would open anyway.

## Rejected

- **A setting "Use skwir as the file manager"** beside the default: two
  answers to one question, which drift when the desktop's settings
  change the default.
- **Answering whenever skwir runs**: it would take "Show in folder" from
  the file manager the user chose.
- **Taking a running holder's name by force** (asking it to quit): it is
  another program's.
- **A small program that only forwards the request to a running skwir**:
  a second program to install and keep in step, for what the program's
  own start without a window does.
