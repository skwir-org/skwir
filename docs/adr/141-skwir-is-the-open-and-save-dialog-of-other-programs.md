# ADR-141: Skwir is the open and save dialog of other programs

Status: accepted 2026-11-19, on the user's answers (step 2 after ADR-140:
the user chose a whole skwir window, a Settings row writing the
portal's configuration, attaching to the asking window, and a commit per
step, before an autonomous night). Built on Linux; Linux's by nature
(decision 11). Built 2026-11-19: the back end (1ae5a3bb), the window
(e85e415e), attached to the asking window (a4299e63, gpui patch 0085),
the Settings row (eaac889f), the packages; amended as built (decisions
4, 6 and 10, and the Context's reading of the configuration), on
2026-11-20 (decision 9: the windows' tags) and on 2026-11-21 (decision
3: no display reached until asked); decisions 4 and 7 amended by
ADR-144 (what a dialog's window offers, its own layout, Recent).

## Context

- **Other programs' Open and Save dialogs go through the portal**:
  sandboxed programs (Flatpak, Snap) always, Chromium and Electron
  programs, Zed (gpui's `prompt_for_paths`), Qt programs on the portal's
  platform theme and Firefox when set to, call
  `org.freedesktop.portal.FileChooser` on the session bus. The portal
  (`xdg-desktop-portal`, 1.22 on the user's machine) hands the request
  to the back end its configuration names, which answers
  `org.freedesktop.impl.portal.FileChooser`: `OpenFile`, `SaveFile`,
  `SaveFiles` (`handle`, `app_id`, `parent_window`, `title`, `options`,
  answering `response`, `results`), and exports
  `org.freedesktop.impl.portal.Request` at `handle` while the dialog is
  up, whose `Close` the portal calls when the program gives up. The
  portal waits without a limit, and lets the bus start the back end.
  `response` is 0 (chosen), 1 (cancelled) or 2 (anything else); only
  `file://` URIs are taken; a sandboxed program is given a document of
  what was chosen by the portal itself.
- **What a back end is**: a `.portal` file (`[portal]`, `DBusName`,
  `Interfaces`) the portal finds in `XDG_DESKTOP_PORTAL_DIR` alone when
  that is set (how its tests isolate it), else in
  `$XDG_DATA_HOME/xdg-desktop-portal/portals`, `$XDG_DATA_DIRS`'s and its
  own; and the bus name it names, answering at
  `/org/freedesktop/portal/desktop`. The user's portal finds KDE's in
  their Nix profile.
- **Which back end answers** is read from the configuration once, when
  the portal starts: in `$XDG_CONFIG_HOME/xdg-desktop-portal`,
  `$XDG_CONFIG_DIRS`', `/etc`'s, then the data folders', a file a folder,
  `<desktop>-portals.conf` (`XDG_CURRENT_DESKTOP`, lower case) before
  `portals.conf`, one without a `[preferred]` group passed over:
  `default=` and one key per interface
  (`org.freedesktop.impl.portal.FileChooser=kde`). Since 1.22 (#1867)
  the files cascade: an interface goes to the first file naming a back
  end installed for it, its own key before its `default`; before 1.22
  the first file found was taken whole (read in 1.22.1's source). The
  user's `hyprland-portals.conf` names KDE's for the file chooser.
- **The window identifier** says where the dialog belongs:
  `x11:<XID>`, or `wayland:<handle>` exported by the asking program
  through `xdg-foreign` (unstable v2), which the dialog imports to be
  its child. A compositor floats a child over its parent (Hyprland,
  sway); a window without one tiles.
- **On macOS and Windows** the system's panels (`NSOpenPanel`,
  `NSSavePanel`; the common item dialog, a COM object in the program's
  own process) cannot be replaced by another application.

## Decision

1. **Skwir is a FileChooser back end.** `skwir.portal` names
   `org.freedesktop.impl.portal.desktop.skwir` for
   `org.freedesktop.impl.portal.FileChooser`; `OpenFile`, `SaveFile` and
   `SaveFiles` are answered, and each request's `Close` closes its
   window and answers it cancelled.
2. **The program that takes launches over answers** (ADR-122), for its
   life: the name is skwir's own, which the portal calls only once its
   configuration names skwir, so no other program's choice is taken
   (ADR-140, 2 follows a shared name; this one is not). Asked queued,
   behind a skwir the bus started; `--new-instance` never answers. What
   is asked is handed in through the gate launches pass (ADR-140, 6),
   the answer sent when the user decides.
3. **The bus starts skwir when none runs**: the packages install the
   `.portal` file and a bus service file for the name, `skwir --portal`,
   which starts without a window as `--file-manager` does (ADR-140, 5):
   a request opens its window; nothing asked within 30 seconds, it ends
   with status 3. (Amended 2026-11-21: until it is asked, it neither
   reaches the display nor starts the window system,
   `finishing::wait_asked`; the portal starts its back ends as the
   session comes up, asking nothing, and one started then reached a
   display that was being replaced, and ended with it.) One that finds
   another skwir of its instance taking launches over takes the request
   and asks that one (`org.skwir.skwir.Finishing.Choose`, the request as
   the portal gave it), waiting for its answer, which it gives the
   portal, then ends: the name first, so that what is asked after goes
   to the skwir queued behind, then once all that reached it is
   answered.
4. **A request is a whole skwir window in a choosing mode** (the user's
   choice over a compact dialog): sidebar, address, search, tags,
   views, preview, the shelf, as any window (what else it offers:
   ADR-144), and a bar at its bottom
   holding what the request asks: the name field (saving), the
   program's filters, its choices (a dropdown each, a switch for a
   boolean), Cancel and the accept button (`accept_label`, else Open,
   Save, Select). Its title is the request's. Enter accepts, Escape
   cancels (once a name typed to find a file has gone: the first Escape
   lets go of it, the user's choice of 2026-11-19), and closing the
   window cancels. As built: a dialog's size at first (1000 × 680 at
   100 %), then the size the dialogs were left at, never a tile's, never
   maximized, its shape kept for the next dialog alone (ADR-144, 4); a
   core window marked a dialog's (`WindowTabs::chooses`), never recorded
   for the next start nor reopened; the windows asleep stay asleep while
   dialogs' windows alone are up, a launch going to a window of the
   user's own; nothing recorded in a run until a window of the user's
   own opened, so a skwir the bus started never replaces the windows the
   user left. A folder typed in the name is gone into; Replace? is read
   on a worker, in the folder's lane. A second request opens a second
   window.
5. **What is chosen.** Opening: the file under the cursor or the
   selection; several only with `multiple`, which the shelf gathers
   across folders; `directory` chooses folders only, the one selected or
   else the one shown. Saving: the folder shown and the name typed,
   started from `current_file` (its folder and name), else
   `current_folder` and `current_name`, the name selected without its
   extension; a name that exists is asked Replace? in skwir before the
   answer, as the portal expects of a back end. The name is taken as
   typed, its spaces kept, and as the bytes it was given while the
   field still says them (a name not UTF-8 is shown in its lossy form);
   a file chosen (Enter, a double click) is the answer at its own path,
   asked Replace? on it. `SaveFiles`: a folder chosen, each name of
   `files` in it, names taken asked Replace? once for all; a name a
   folder takes there is said, and nothing answered. Only local files,
   as `file://` URIs, said writable (as KDE's back end says them: the
   interface's default is read access alone, which a sandboxed program
   would be given); the choices as set; the
   filter in use. A name the program gives (`current_name`, each of
   `files`) is one name, or the request is answered 2 without a window:
   a `/`, `.`, `..` or nothing would put the file elsewhere than in the
   folder the user chooses (the portal checks `files` since 1.22, never
   `current_name`); only what the user types may be a path.
6. **The program's filters are a lens** (ADR-107): the one in use
   (`current_filter`, else the first) shows what it admits, globs by
   name (case ignored: a camera's `.JPG` is the program's `*.jpg`) and
   MIME types (a `*` subtype included, a type's descendants too) through
   the type database; folders always show; apart from the filter the
   user types (core's `Admits`, both holding); the filter's dropdown
   offers the others and all files. Filters only help: a name typed is
   taken as typed.
7. **Where it starts**: the request's folder (`current_folder`,
   `current_file`'s), else the folder that program (`app_id`) last chose
   in (the window's state, by program, ADR-056), else home (opening
   files, Recent where it has a file to give: ADR-144, 7). A program
   with no id (the host's) keeps one place for all.
8. **Only the user answers.** The chooser's commands are the window's
   (`ui.chooser_*`), the user's alone (ADR-044): never an agent's, a
   script's or a plugin's from outside, which cannot see what is asked
   either. Choosing gives a sandboxed program the files chosen. What
   the dialog's own action asks the user (a server's password, names a
   paste found taken) is asked in its window; what agents, scripts and
   plugins' own work ask waits for a window of the user's own (ADR-144,
   1, amended after review).
9. **Attached to the asking window** (the user's choice): `x11:` sets
   `WM_TRANSIENT_FOR`; `wayland:` imports the handle (`xdg-foreign`
   v2) and sets the window its child, a patch of gpui's
   (`vendor/patches`). An identifier that names nothing, or a
   compositor without the protocol, leaves a window of its own, brought
   forward. The request's `modal` is passed on, not followed: the window
   is kept over the asking one, which it does not hold. **Amended
   2026-11-20, on the user's ask:** every dialog's window is also
   tagged `file chooser`, and every window of the user's own `main
   window` (Wayland's xdg-toplevel-tag-v1, X11's `WM_WINDOW_ROLE`; gpui
   patch 0089), whether or not a parent was found, so that a rule of the
   compositor's can float the dialog where nothing keeps it over its
   asker: an asker giving no window or one of the other display server
   (an `x11:` id from a program on Xwayland, while skwir is on
   Wayland), or a compositor that leaves the child tiled (a dialog was
   seen to tile on the user's Hyprland 0.57.1, which offers xdg-foreign;
   what its asker gave was not seen). The tags are an interface
   people's configuration names: untranslated (the description beside,
   on Wayland, is the reader's language, said again when it changes),
   and renamed only as a breaking change.
10. **Turning it on: Settings › "Open and save dialogs"** (the user's
    choice), on General beside the default file manager: a choice its
    owner keeps (ADR-140, 8), the back ends installed that offer the file
    chooser (their `.portal` files, as the portal finds them: in the
    folders of the portal that runs, its environment read in `/proc` and
    its program's data folder, which a system that wraps it, NixOS, makes
    differ from skwir's; none running, skwir's own), the one the
    portal's configuration names now (read as 1.22.1 reads it: `none` is
    none, `*` the first installed, a key written twice its last), else
    Automatic. Choosing writes `org.freedesktop.impl.portal.FileChooser=`
    in the user's `<desktop>-portals.conf` (`portals.conf` where no
    desktop is named), its other lines kept; a file there that cannot be
    read is left alone, and why said. A configuration that is a link (its
    file or its folder: home-manager's, a dotfiles folder's) is another
    program's to keep: never written, the row shows its choice with no
    other offered and says where it leads, to be chosen there (the user's
    choice, 2026-11-19). Automatic takes out only a key the user's file
    holds; where the user has no file, it and a back end already in
    effect write nothing ("already"). For another, where the user has
    none, a portal from 1.22 on is given the key alone (the cascade takes
    the rest from the system's files); one before 1.22 (Ubuntu 24.04 ships
    1.18) takes the first file whole, where a lone key would drop the
    system's other choices, so the file the portal reads first is copied
    into it first. The version is that of the portal the next start runs,
    its bus service file's program asked `--version` as the choice is
    written; one not known is taken for one before, the copy being right
    for both (at the cost of freezing the system's lines where it later
    changes its own; the user's choice, 2026-11-19). It says the change
    applies when the portal starts again (the next login); skwir never
    restarts it. Undo as the default
    file manager's: the file put back as it was, or none, only while it
    holds what skwir wrote (an edit since is kept, and said), then Redo;
    its command names the choice by a serial the row keeps, never a file
    and its text. As built, an essential of General beside the default
    file manager, which it is for the same reason (it decides what other
    programs see; ADR-072); its commands `dialogs.list`, `dialogs.choose`
    (the user's) and `dialogs.put_back`, never `ui.*`, which the window
    carries out itself (a notice's Undo would reach nothing).
11. **Linux's by nature**: nothing to install, turn on or show on macOS
    and Windows, where no application can be the system's dialog
    (Context); PORTING.md lists it so.

## Consequences

- Every program that asks through the portal gets skwir's places,
  search, tags, previews and shelf to choose with, chosen once in
  Settings, or in Home Manager: `programs.skwir.fileChooser` names skwir
  in `xdg.portal.config` for the desktops given and links its `.portal`
  and bus service files under `$XDG_DATA_HOME` (where the portal and the
  bus look whatever profile they read), and
  `programs.skwir.defaultFileManager` puts skwir first for folders in
  `xdg.mimeApps` (ADR-140); the row then shows the choice as kept by
  Home Manager (decision 10). Asked by the user, 2026-11-19.
- A program that draws its own toolkit's dialog (GTK outside a sandbox,
  Qt without the portal theme) still does: the portal is the reach.
- The portal reads its configuration once: a choice applies at its next
  start.
- The bus-started skwir pays a start without a window before the dialog
  shows: the first frame's budget (ADR-033) applies.
- What the chooser shows is the user's skwir: agents and scripts never
  answer for them.

## Rejected

- **A compact dialog** (the user's choice): fewer of skwir's ways to
  find a file, and a second look to keep.
- **Restarting the portal** so a choice applies at once: it is the
  session's service.
- **Answering only while chosen** (following `portals.conf` as ADR-140
  follows the default for folders): the name is skwir's own, called only
  once chosen; following would add a reading for nothing.
