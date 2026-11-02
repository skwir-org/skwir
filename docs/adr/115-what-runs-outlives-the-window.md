# ADR-115: What runs outlives the window

Status: accepted 2026-11-02 with the user, from a comparison of what file
managers and browsers do; built the same day, as `docs/PLAN.md` ("What
runs outlives the window") records: checked on Linux in a window of its
own, on a session bus of its own; Windows tried under Wine, Windows and
macOS otherwise compiled only.

## Context

- **The user asked** what happens to actions under way when Skwir is
  closed mid-way, what other file managers and browsers do, and whether
  a taskbar icon should show the actions finishing.
- **Until now** the window's close button and Quit asked while a plan
  ran ("Copy … is still running. Quit and stop it?"); answered yes, the
  window went at once and the process cancelled everything, waiting up
  to 30 s, out of sight, for each plan to take back what it was in the
  middle of (`Executor::wind_down`), then ended. A 20 GB file copied to
  90 % was thrown away; what waited its turn never ran. The part of a
  file an error cut short, kept for Retry (ADR-080), went with the
  process. Logging out, shutting down or a kill asked nothing: the
  process ended, its staging folder left for the next start to sweep.
- **File managers never stop work because a window closed.** Nautilus
  holds the application while operations run; with no window, one
  notification says "File Operations: 2 file operations active", with
  Show Details, and its Quit only closes the windows
  (`nautilus-progress-persistence-handler.c`, `nautilus-application.c`).
  Nemo holds it too; its operations' window, closed, becomes a tray
  icon, a pie with the count and the share in its tooltip
  (`nemo-progress-ui-handler.c`). Dolphin's jobs keep the process alive
  until they end (KJob's event-loop lock), their progress, Pause and
  Cancel in Plasma's notification area; its bug reports show a question
  (a name taken) coming after the window closed crashing it. Explorer
  and Finder run copies in a progress window of their own, in a shell
  that never quits.
- **Browsers ask instead**: Chrome and Firefox warn when quitting while
  a download runs, and quitting cancels it; picking it up after a
  restart is unreliable in both.
- **Tray icons**: Windows' programs that keep working without a window
  put an icon in the notification area. On Linux, Plasma, XFCE,
  Cinnamon, MATE and waybar have a tray (the StatusNotifierItem
  protocol); GNOME has none (its Background Apps list shows sandboxed
  programs only), hence Nautilus's notification. On macOS, a program
  without windows stays in the Dock.
- **Skwir runs one process per window**: a later launch knew nothing of
  what another was doing.

## Decision

1. **Closing the window stops nothing.** The close button closes it at
   once, asking nothing. What runs, waits its turn or is paused is
   finished by the program without a window (`finishing.rs`), which ends
   by itself once nothing keeps it. Meanwhile its panes sleep
   (`App::suspend_panes`): nothing is listed or watched.
2. **Quit asks while something is under way**, with three answers:
   **Finish in the background** (Enter, and Quit again: the window goes,
   as with the close button), **Stop and quit** (as before: cancelled,
   what was in the middle of taken back), **Keep the window** (Escape).
   With nothing under way, Quit quits at once.
3. **While no window is up, the program shows that it is at work**
   (`Desktop::presence`): its icon with a badge filling as the strip's
   bar (its colour: running, paused, waiting for a server, stopped, a
   question), the strip's words in its tooltip, a click opening the
   window with Activity, a menu (Open Skwir; Pause all or Go on; Stop
   all and quit).
   - **Windows**: an icon in the notification area (the taskbar's button
     went with the window).
   - **Linux**: the same, wherever the desktop has a tray: a
     StatusNotifierItem with its menu (`com.canonical.dbusmenu`), when
     `org.kde.StatusNotifierWatcher` is on the session bus with a host,
     never by the desktop's name, registered again when the watcher
     comes back (a panel restarted). Without one (GNOME), or 3 s after
     the tray went, one notification that stays, as Nautilus's:
     "Finishing 2 actions", with Open Skwir and Stop all, sent again only
     when what it says changes (never for a percent).
   - **macOS**: nothing new: the Dock keeps the program's icon, whose
     badge shows the progress; a click on it opens the window.
   - Every action that ends meanwhile is told to the desktop, however
     short (the 5 s rule is the window's), and a question waiting for
     an answer is told once.
4. **Opening Skwir again finds it.** A program finishing without its
   window takes later launches over (`skwir_platform::handover`): a
   launch hands it its argument, its folder and the desktop's activation
   token, then ends, and the window opens again, showing that folder (or
   the tabs it had), what runs taken back with `Earlier`. On Linux a name
   on the session bus (`org.skwir.skwir`) behind a marker file in the
   runtime folder, so that a launch where no Skwir finishes costs a look
   at one file; on Windows a named pipe per user and session; on macOS,
   LaunchServices brings the running program forward and gpui's
   `on_reopen` opens the window. Only while it finishes: otherwise each
   launch is a program of its own, as before.
5. **What cannot finish alone keeps the program, and says so**: a plan
   paused, one waiting for its server to go on by itself (ADR-080, 9),
   one an error stopped since the window closed (its Retry and its kept
   partial), a question an action asks (names taken, room short, an
   archive's password, a form): it waits for the window, which takes it
   when it opens. What the window was asking when it closed is answered
   Cancel, as its own Close would; an action stopped that the window was
   showing is let go.
6. **Logging out and shutting down are held off while something runs or
   waits its turn**, with the action's words (`Desktop::hold_session`).
   - **Windows**: `ShutdownBlockReasonCreate` and `WM_QUERYENDSESSION`
     answered no: Windows names Skwir and the action, with Shut down
     anyway.
   - **Linux**: the portal's `Inhibit` (log out), which GNOME's
     end-of-session dialog lists; and logind's shutdown inhibitor, but
     only where no session manager that shows its own dialog is on the
     bus (`org.gnome.SessionManager`, `org.kde.ksmserver`), as GNOME
     mishandles logind's blocking inhibitors (power off silently does
     nothing, or polkit asks for a password without saying why). Plasma
     6.8 holds nothing off: xdg-desktop-portal-kde 6.8.2 keeps the
     portal's logout inhibitors for `org.kde.SessionState`'s
     `queryCanEndSession` (`src/inhibit.cpp`), which nothing in
     plasma-workspace or KWin 6.8.2 calls, and ksmserver asks only its
     XSMP clients (`ksmserver/logout.cpp`), which Skwir is not. logind's
     shutdown block is not asked there either: it holds off no logout,
     and plasma-shutdown asks for the power off once the session has
     ended (`startkde/plasma-shutdown/shutdown.cpp`), which systemd 262
     then refuses outright (`src/login/logind-shutdown.c`). Read in the
     sources 2026-11-02; not tried on a Plasma session. Logout there is
     held off once Plasma's session manager asks the portal.
   - **macOS**: the system asks whether Skwir may quit
     (`applicationShouldTerminate:`, gpui patch 0044); while something is
     under way it answers no, and the window asks the user as Quit does.
   - Going on anyway, the program stops what runs as Stop and quit does:
     a signal to end (`SIGTERM`, `SIGINT`, `SIGHUP`) on Linux, gpui's
     `WM_ENDSESSION` on Windows; asked twice, it ends at once.

## Rejected

- **Activity as a small window of its own when the main one closes**, as
  Nemo's and Explorer's progress windows: no tray needed, but closing
  would leave a window behind, which a tiling compositor lays out with
  the others.
- **Picking up after a restart** (a crash, a power cut, Shut down
  anyway): the rest of each plan written to disk as it runs and its
  partial kept from the sweep. The browsers' feature, which neither does
  well; it bends "stateless" (ADR-000). Its own ADR if wanted.
- **A permanent tray icon, minimising to the tray, starting at login**:
  the presence is there only while the program finishes without a
  window; Skwir stays no daemon (ADR-000).
- **One process for every window**: only a program finishing takes
  launches over; a window's crash still takes only its own.
- **logind's blocking shutdown inhibitor everywhere**: GNOME's power off
  then does nothing, or asks for a password without saying why.

## Consequences

- Closing the window during a long copy loses nothing; Quit's default
  answer does the same.
- A Skwir without a window may run for as long as its actions take, its
  presence saying so; nothing starts it but the user.
- The core's wakes are taken by one loop for the program's life
  (`finishing::listen`), the window's shell syncing while one is up.
- What runs and what was said of it is `Work` (`work.rs`), the
  program's for its life (`Finishing`), which the window's shell shares
  and never moves: what it took cannot go with a window that closed, or
  one that failed to open. A window that left (`Shell::leave`) takes
  nothing more, though gpui removes it later: what the core sends
  meanwhile is the program's, what it asks waits for the next window.
- gpui's quit mode is explicit: the program ends when nothing keeps it,
  not with its last window.
- A launch looks for a finishing Skwir first: on Linux, a marker file in
  the runtime folder; on Windows, a named pipe.
