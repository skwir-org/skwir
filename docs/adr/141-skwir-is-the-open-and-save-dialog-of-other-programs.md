# ADR-141: Skwir is the open and save dialog of other programs

Status: accepted 2026-11-19, on the user's answers (step 2 after ADR-140:
the user chose a whole skwir window, a Settings row writing the
portal's configuration, attaching to the asking window, and a commit per
step, before an autonomous night). Built on Linux; Linux's by nature
(decision 11). Built 2026-11-19: the back end (1ae5a3bb), the window
(e85e415e), attached to the asking window (a4299e63, gpui patch 0085),
the Settings row (eaac889f), the packages; amended as built (decisions
4, 6 and 10, and the Context's reading of the configuration).

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
   with status 3. One that finds another skwir of its instance taking
   launches over takes the request and asks that one
   (`org.skwir.skwir.Finishing.Choose`, the request as the portal gave
   it), waiting for its answer, which it gives the portal, then ends.
4. **A request is a whole skwir window in a choosing mode** (the user's
   choice over a compact dialog): sidebar, address, search, tags,
   views, preview, the shelf, as any window, and a bar at its bottom
   holding what the request asks: the name field (saving), the
   program's filters, its choices (a dropdown each, a switch for a
   boolean), Cancel and the accept button (`accept_label`, else Open,
   Save, Select). Its title is the request's. Enter accepts, Escape
   cancels, and closing the window cancels. As built: a dialog's
   size at most (1000 × 680 at 100 %), never maximized, its shape never
   the next window's; a core window marked a dialog's
   (`WindowTabs::chooses`), never recorded for the next start nor
   reopened; the windows asleep stay asleep while dialogs' windows
   alone are up, a launch going to a window of the user's own; nothing
   recorded in a run until a window of the user's own opened, so a
   skwir the bus started never replaces the windows the user left. A
   folder typed in the name is gone into; Replace? is read on a worker,
   in the folder's lane. A second request opens a second window.
5. **What is chosen.** Opening: the file under the cursor or the
   selection; several only with `multiple`, which the shelf gathers
   across folders; `directory` chooses folders only, the one selected or
   else the one shown. Saving: the folder shown and the name typed,
   started from `current_file` (its folder and name), else
   `current_folder` and `current_name`, the name selected without its
   extension; a name that exists is asked Replace? in skwir before the
   answer, as the portal expects of a back end. `SaveFiles`: a folder
   chosen, each name of `files` in it, names taken asked Replace? once
   for all. Only local files, as `file://` URIs, said writable (as KDE's
   back end says them: the interface's default is read access alone,
   which a sandboxed program would be given); the choices as set; the
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
   in (the window's state, by program, ADR-056), else home. A program
   with no id (the host's) keeps one place for all.
8. **Only the user answers.** The chooser's commands are the window's
   (`ui.chooser_*`), the user's alone (ADR-044): never an agent's, a
   script's or a plugin's from outside, which cannot see what is asked
   either. Choosing gives a sandboxed program the files chosen.
9. **Attached to the asking window** (the user's choice): `x11:` sets
   `WM_TRANSIENT_FOR`; `wayland:` imports the handle (`xdg-foreign`
   v2) and sets the window its child, a patch of gpui's
   (`vendor/patches`). An identifier that names nothing, or a
   compositor without the protocol, leaves a window of its own, brought
   forward. The request's `modal` is passed on, not followed: the window
   is kept over the asking one, which it does not hold.
10. **Turning it on: Settings › "Open and save dialogs"** (the user's
    choice), on General beside the default file manager: a choice its
    owner keeps (ADR-140, 8), the back ends installed that offer the file
    chooser (their `.portal` files, as the portal finds them), the one the
    portal's configuration names now (read as 1.22.1 reads it: `none` is
    none, `*` the first installed, a key written twice its last), else
    Automatic. Choosing writes `org.freedesktop.impl.portal.FileChooser=`
    in the user's `<desktop>-portals.conf` (`portals.conf` where no
    desktop is named), its other lines kept, through a link to its target
    (its mode kept); a file there that cannot be read is left alone, and
    why said. Automatic takes out only a key the user's file holds; where
    the user has no file, it and a back end already in effect write
    nothing ("already"). For another, where the user has none, the file
    the portal reads first is copied into it first. Since 1.22 a file
    holding the key alone would do (the cascade takes the rest from the
    system's files), but skwir cannot tell which portal reads it, and one
    before 1.22 (Ubuntu 24.04 ships 1.18) takes the first file whole,
    where a lone key would drop the system's other choices: the copy works
    for both, at the cost of freezing the system's lines where it later
    changes its own. It says the change applies when the portal starts
    again (the next login); skwir never restarts it. Undo as the default
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
  Settings.
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
