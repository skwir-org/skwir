# ADR-114: What runs is docked at the window's foot, and the desktop is told of long actions

Status: accepted 2026-11-02 with the user, from a page of mock-ups
comparing three places and what other programs do; built the same day,
as `docs/PLAN.md` ("Activity, docked") records: checked on Linux in a
window of its own, on a session bus of its own with dunst for the
desktop's notices; Windows tried under Wine (no toasts there), Windows
and macOS otherwise compiled only.

## Context

- **The user asked**: the toasts pile up and hide the files, "we can't see
  files behind the toasts when we are at the bottom of the list"; could
  they move, fold away, with a summary of what runs when docked, "maybe
  similar to what we do when the player is docked in the bottom bar?",
  and should Skwir tell the desktop of some actions, at least long ones?
- **The toasts were one stack**, over the panes, centred, 38 px above the
  window's foot, up to 560 px wide: four plans' cards, two tasks' (a
  print), a line of what waits, and one answer. A card stayed as long as
  its plan ran: a long copy hid the last rows for minutes, and no scroll
  brought them out from under it.
- **Two things shared it**: answers, which last seconds and offer Undo,
  and work under way, which lasts minutes and offers Pause and Cancel.
- **Elsewhere**: GNOME Files puts operations behind a header-bar button
  with a pie, its popover listing them; it sends one notification when
  its last operation ends, if that ran 2 s or more and no window of its
  has the focus (`nautilus-progress-persistence-handler.c`). Dolphin
  hands its jobs to Plasma's tray. Explorer and Finder open a window of
  their own; Explorer fills its taskbar button (green, yellow paused, red
  on an error), Finder a pie beside the destination. Chrome's download
  bar along the bottom took the page's room; in 2023 it became a toolbar
  button with a ring, opening a panel. GNOME's guidelines: a notification
  for what the user is not watching, withdrawn once the window has the
  focus, the window showing everything a notification said.
- **Skwir keeps the window's state at the end of the last pane's footer**
  (`status.rs`: the player's strip while the preview is closed, what waits
  for a paste, proposals, Undo). Each pane has its own top row, already
  full; the window has no toolbar of its own.
- **Nothing kept the machine awake** during a long copy; the window only
  kept the screen on for a film (gpui patch 0023).

## Decision

1. **Work under way is docked in the footer**: a strip at the end of the
   last pane's footer, before Undo, says how far what runs has gone, in
   as many words as the footer has room for (`Fit`): "Copy 3 items to
   Backup · 42 % · about 3 min left", then "42 % · about 3 min left",
   then "42 %"; several, "2 actions · 36 % · about 6 min left", their
   bytes together where each was sized, else each one's share alike, and
   the longest time left. One action keeps its Pause and Cancel there.
   Paused or stopping, its bar is muted and it says so.

   Amended 2026-11-03 (the user's choice of the recommended options):
   the strip counts the **spell of work**, from an action asked with
   nothing under way until nothing runs, waits or goes on by itself.
   Each action of it counts one, whatever its bytes: those ended whole,
   those running by their own share, those waiting nothing ("Copy “a” ·
   2 of 3 · 47 % · about 3 min left · 1 waiting", then "2 of 3 · 47 % ·
   … · 1 waiting", then "47 %"). Corrected the same day, after review:
   - One test, the core's, says what counts as an action, for what
     waits, the card and what ended (`Waiting::counts`,
     `ExecEvent::Started::counts`): several steps, or a heavy one (bytes
     moved, a tree walked: one file copied, a folder deleted for good),
     an undo or a redo by its entry's steps; never a single rename. A
     plan is counted by its steps until what it touches is worked out
     (a single heavy step joins a few milliseconds after it is asked, a
     single rename never flashes "1 waiting"); an undo or a redo counts
     until its entry is known. (The first build had three tests, which
     disagreed: an undo of one rename counted while it waited, a folder
     deleted for good counted waiting and never after.)
   - The bar never goes back as one ends. A task (a print) that ends
     counts whole, as a plan does. An action cancelled before it did
     anything (its Cancel or Cancel all while it waits) leaves the count,
     its share nothing; one that did something ended, and counts whole.
     One admitted and not yet told started counts as waiting
     (`Executor::waiting_count`), so the strip stays through each
     hand-over: "1 of 2 · 50 % · 1 waiting", or "1 waiting" before
     anything of the spell started.
   - It goes back only when more is asked, which "2 of 4" says, or as an
     action stopped goes on, retried or by itself once its server
     answers (`ExecEvent::Retried`, `GoesOn`): the same action (ADR-080,
     9), the run that stopped leaving what ended as its continuation is
     asked, counted again by what it has left.
   Nothing is read ahead to size what waits: the time left is what runs,
   and the strip says how many wait that count; Activity's Waiting
   heading says how many quick ones it lists beside them. Activity's
   header says the same over a bar of one segment each (one bar past
   twelve); the taskbar's progress and the presence follow it, with no
   share while only something waits and nothing ended, and no Pause with
   nothing to pause. Counted where the cards are decided (`Work`'s
   spell, settled after each sync's events), what waits a count the
   executor keeps for it, with no footprint compared and no title built,
   not in the core as the proposal said: the agents' `running` is
   unchanged.
2. **Activity opens above the strip** on a click or `ui.activity`
   (Ctrl+Shift+J, Chrome's key for its downloads; Ctrl+J is Jump's): each
   action's card as it was (its bar, items and bytes, speed and time
   left, Pause and Cancel, "Stopping…"), what waits its turn, and
   **Earlier**: the answers of this session, newest first, twenty at most,
   with Undo, and Retry while it can be, kept in memory only: every answer
   a toast says, a plan's or another's (a print's, a message). One popup at
   a time, closed by Escape or a press elsewhere (`Shell::toggle_popup`).
3. **Pinned** (`ui.activity_pin`, its pin button; remembered with the
   window's shape), Activity stands above the last pane's footer while
   something is under way, waits or stopped, taking room rather than
   covering the files, at most half the pane's height; the strip steps
   aside meanwhile, unless Activity is open as a popup, which stays where
   it opened until closed. Not pinned, nothing under way covers the files.
   Pinned, it is a place of the keyboard's: `ui.activity` gives it the
   keyboard and Escape gives it back to the files. A click on the strip or
   on Activity pinned leaves the keyboard where it was, as a click on the
   cards in the toasts did.
4. **Answers stay toasts**, one at a time, at the last pane's lower right,
   above the strip (or Activity pinned): there the list shows sizes and
   dates rather than names. On the whole screen, over the preview, as
   before. While Activity is open, no toast is drawn: Earlier holds every
   answer, a plan's or not, so none that came meanwhile is lost.
5. **An action an error stopped stays in the strip**, in the error's
   colour, "Copy …: stopped" with Retry and Dismiss, until it is retried
   (from anywhere), Activity is opened, or it is dismissed; only one that
   was shown as a card there (a quick rename that fails has its answer).
6. **The desktop is told of long actions it is not watched for**: an
   action that ran 5 s or more (the same 5 s after which its answer says
   what it moved in how long) and finished or stopped, or one waiting for
   a server that stopped answering, while Skwir's window does not have
   the focus, is a notification: one at a time, at most one every 2 s
   (the first at once, then the last, which sums what ended:
   `desktop::NOTICE_PACE`), replacing the last ("3
   actions finished, 1 stopped"), filed as the specification's
   `transfer.complete`, `transfer.error` or `transfer`, withdrawn once the
   window has the focus again, or once a waiting one goes on. A click
   brings the window forward (with the token the desktop gave, on
   Wayland) and opens Activity. No Undo from a notification: it would
   undo what cannot be seen. A cancelled action tells nothing. A setting
   turns it off (`ui.notify_desktop`, on by default).
7. **Progress on the taskbar entry** while something runs: Plasma's task
   manager through the launcher API (`com.canonical.Unity.LauncherEntry`,
   keyed on the desktop file), Windows' taskbar button (green, yellow
   paused, red stopped), the macOS Dock's badge. Silent, no setting.
8. **The machine is kept awake** while something runs and is not paused:
   on Linux by the portal's `Inhibit` (suspend) where a session manager
   is on the bus, else by logind's sleep inhibitor (`block`); a
   system-required power request on Windows, a user-initiated activity on
   macOS. A closed lid still sleeps. Let go a second after the last action
   ends, so a queue never lapses. A setting turns it off (`ui.keep_awake`,
   on by default).
   - logind's `block` was asked everywhere at first. Read in the sources
     (2026-11-02): systemd 262 refuses any suspend while a `block` is
     held, the user's own too, only `block-weak` yielding to its own user
     (`src/login/logind-shutdown.c`, `logind-inhibit.c`);
     gnome-settings-daemon 50.1 arms its idle suspend by gnome-session's
     `InhibitedActions`, reading no logind lock
     (`plugins/power/gsd-power-manager.c`); gnome-session 50.1 turns a
     program's suspend inhibitor, which xdg-desktop-portal-gtk 1.15.3
     hands it as asked (`src/inhibit.c`), into logind's `block-weak`
     (`gnome-session/gsm-systemd.c`); xdg-desktop-portal-kde 6.8.2 makes
     it PowerDevil's `InterruptSession` (`src/inhibit.cpp`), and
     PowerDevil 6.8.2 honours other programs' logind `block` locks too.
     Under GNOME the `block` left the idle suspend armed (its warning
     shown, the suspend then refused) and refused the user's own Suspend;
     through the portal, idle does not suspend and the user's Suspend
     does. Not tried on a GNOME or Plasma session.
9. **One seam for the desktop** (`skwir_platform::desktop::Desktop`):
   every call returns at once, a thread of its own talking to the
   desktop, started on the first; the session bus is Skwir's one
   connection (`bus::session`), logind's system bus opened for the call
   alone. Made by the window's shell, never in a headless run.

## Rejected

- **A button at the top**, as GNOME Files and the browsers: Skwir has no
  window toolbar; each pane's row is full and folds into one View button
  when short; with two panes, which row? Far from Undo and the answers.
- **A tray that takes room by default**, as Double Commander's panel: the
  list loses rows for the whole of the action, Chrome's old trade, and
  the panes jump as actions start and end. Kept as the pinned choice.
- **The card shown for a few seconds before folding into the strip**: the
  action was just asked for, and the strip shows that it began.
- **Progress in a notification**: GNOME draws none; the taskbar shows it.
- **KDE's job tracker** (`org.kde.JobViewServerV2`), which would list
  Skwir's actions in Plasma's tray with Pause and Cancel as Dolphin's: one
  desktop's, and a second place to keep in step with the window. Not now.

## Consequences

- The files are covered only by an answer, for its few seconds, at the
  pane's lower right.
- What a toast said is found again in Activity's Earlier until the window
  closes.
- The window keeps whether it has the focus (`Shell::window_active`).
- A sleep inhibitor appears in `systemd-inhibit --list` while a plan runs:
  Skwir's where no session manager is on the bus, gnome-session's
  (`block-weak`) under GNOME; under Plasma, PowerDevil keeps it in its own
  list.
- Toasts show on Windows only for an installed Skwir: its installer
  registers the program's AppUserModelID (`org.skwir.skwir`) under
  `HKCU\Software\Classes\AppUserModelId`, as unpackaged programs do;
  wixl, which makes the installer, cannot give the Start menu's shortcut
  one, and the process keeps the id Windows gives it, so a pinned
  shortcut and the window share their taskbar button.
- GNOME lists Skwir in its notification settings (the desktop file's
  `X-GNOME-UsesNotifications`).
