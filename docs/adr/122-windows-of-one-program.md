# ADR-122: Windows of one program

Status: accepted 2026-11-07 with the user, from a comparison of what file
managers and browsers do; built in steps on Linux (see "Built" below);
macOS and Windows compiled only. Amends ADR-018 (tabs), ADR-043 (agents
reach every window), ADR-056 (each window's shape remembered) and
ADR-115 (one process for every window; launches always handed over).

## Context

- **The user asked** (2026-11-07) for several windows, as Dolphin and
  browsers have, done in a way that is consistent and robust.
- **Until now each launch was a process of its own** with its one window
  (ADR-115 kept that: only a Skwir finishing without its window took
  launches over). Two windows were two programs that knew nothing of each
  other:
  - two queues: ADR-079's one heavy action per disk held in each, not
    across them, so two copies to one USB disk ran at once;
  - two undo histories, two History lists;
  - one control channel (ADR-043): the second window was out of reach of
    `skwir cmd` and agents;
  - two players on the session bus, two tray icons when both finished;
  - the last window closed wrote `core.last_tabs` and the window's shape,
    the other's lost;
  - each window paid for a whole program: plugins, fonts, the GPU device,
    caches, about 100 MiB idle (ADR-033).
- **Dolphin** works so: its New Window starts `dolphin --new-window`, a
  tab detached is a process started with its location, and an undo is a
  process's. **Nautilus, Finder, Explorer and the browsers** run one
  program with many windows; tabs move between them; Quit remembers every
  window, a window closed alone is forgotten (Chrome and Firefox reopen it
  from their closed list).
- Every gap above is two of something that should be one. Closing them
  across processes would mean processes speaking to each other about
  queues, histories and windows: a daemon in disguise (ADR-000, 3).

## Decision

1. **One program, many windows.** Windows are core data, as tabs are
   (ADR-018): the workspace holds windows (`WindowId`, `WindowTabs`),
   each with its tabs and the tab it shows; every tab is in one window,
   every window has a tab. One window is active: the one the user last
   used, where a command that names no pane acts (`App::pane_or_active`).
   A command naming a pane acts in that pane's window: a tab opened from
   a pane opens beside it, in its window. Each window shows one tab; the
   tabs hidden in each sleep (ADR-018), so what is listed is what the
   windows show.
2. **Commands** (Topic Tabs, "Windows, tabs and the split"):
   `window.new` (a location, or what the pane shows, with its sort and
   lens; the new window made from the pane's, and active), `window.close`
   (its tabs kept to be reopened together; the last window closes only by
   its own close button or Quit, ADR-115), `window.select`, `window.list`;
   `tab.move` takes a `window`, or `new_window`; `tab.list` says each
   tab's window. A window's last tab closing (`tab.close`, Ctrl+W) closes
   the window while another is open. Closed from the window (Ctrl+W, the
   palette), the last tab of the last window closes the window, as its
   close button does, as browsers and Nautilus do; asked of the core
   alone (an agent's `tab.close`), it stays, as `window.close` keeps the
   last window. `tab.reopen` (Ctrl+Shift+T) reopens tabs and windows from one
   list, newest first, a tab in its window if it is still there, else the
   active one.
3. **One for the program**: the queue, History and Undo (Ctrl+Z in any
   window undoes the last action asked anywhere: the files are shared),
   Activity (every window's strip shows the same work), the shelf, cut
   files, settings, the agents' channel, the presence while no window is
   up, and the player: one plays at a time. A play started in a window
   pauses what another plays, where it is; started again there, it plays
   on from that point and pauses the first in turn. The desktop's
   controls steer the window that started playing last; closing a window
   stops what it plays. (Built first as one player moving between
   windows, the other's track let go of; changed the same day at the
   user's word.)
   **Each window's own**: its tabs, split, sidebar, preview, terminal
   panel, address field, palette, toasts. What answers an action (its
   toast, its question) goes to the window that asked, else to the active
   one; one Settings dialog is open at a time.
4. **Opening windows**: Ctrl+N opens a window on the folder shown, shaped
   as the window it came from; Open in New Window on a folder or a place,
   Move to New Window on a tab (a tab is not dragged between windows: see
   Rejected). Ctrl+Shift+W closes a window.
5. **Launches go to the running Skwir**, whether or not a window is up
   (ADR-115's hand-over, always on): a launch hands it its location, its
   folder and the desktop's activation token, then ends. Only to a Skwir
   of the same build, settings and state files, and display; anything
   else is a program of its own, so a window started with other folders
   (a test's) never opens in the user's. `--new-instance` is a process of
   its own. A launch opens a new window on the folder asked (the startup
   folder when none), or, with "Open folders from other programs in" set
   to a tab, a tab of the active window, a bare launch then bringing that
   window forward. With no window up, a launch opens the windows the
   program had, as before.
6. **Remembered as they change**: every window's tabs and shape go to the
   state (`core.last_windows`) as they change, written on a worker, with
   `core.startup = last`. So Quit keeps every window, a window closed by
   its × is forgotten (unless it was the last), and a crash or a kill
   loses none. At startup each window comes back with its shown tab
   listed, the rest asleep (ADR-018). The tabs kept before
   (`core.last_tabs`) come back as one window and are forgotten once
   windows are written. The shape a new window takes without a window to
   take it from stays `ui.session`, the last used window's.

## Rejected

- **A process per window with a New Window that starts one** (Dolphin):
  cheap to add, but every gap above stays, and closing them needs
  processes coordinating each other.
- **A window's crash taking only itself**, ADR-115's reason for a process
  per window: what reads files Skwir does not trust already runs in
  helpers (PDFs, media, archives, grammars, models, printing), and the
  windows are remembered as they change, so a crash reopens them where
  they were.
- **Undo per window**: the files are the program's; a History per window
  would let one window's Undo pass over what another did after it to the
  same files.
- **Tabs per side of a split, windows per side** (ADR-018's Total
  Commander model): rejected there for the other pane's sake, still.
- **Tabs dragged between windows and out of them** (browsers): built,
  then withdrawn the same day (step 4 below). gpui carries a drag out of
  a window on Wayland alone, so a tab moved between windows there, became
  a window wherever it was released on X11, and stayed in its window on
  Windows and macOS: one gesture doing three things by system. Move to
  New Window and `tab.move` do it everywhere. Carrying the drag on every
  system (the pointer's grab on X11 and Windows, a dragging session on
  macOS) would bring it back.

## Consequences

- `Workspace::active_tab`, `tabs`, `active_id` mean the active window's;
  `window_of`, `window_of_tab` and `window` reach the others. Code that
  holds a pane resolves its window from it (the pane-ownership rule).
- The UI's program (`finishing.rs`) holds every window; a wake is taken
  once and handed to each window, whose shell keeps only its own panes'
  state.
- An extra window costs its panes' listings, its surface and its atlas,
  not a program.
- A window opens at 1200 × 800, set a little apart from the one before
  where the system lets it say where it goes (not on Wayland): neither
  its size nor its position is remembered yet.

## Built

- 2026-11-07, step 1: windows in the core (`crates/core/src/workspace.rs`,
  `tabs.rs`), the commands, closed windows reopened, saved windows and
  the former tabs read once; `crates/core/tests/windows.rs`.
- 2026-11-07, step 2: the UI's program holds the windows
  (`crates/ui/src/finishing.rs`: `follow_windows`, `deliver`,
  `record_windows`), each shell its window (`Shell::window_id`, its
  `inbox`); Ctrl+N, `window.open`, Move to New Window, Ctrl+Shift+W; one
  player (`claim_player`), one Settings dialog; thumbnails stale by
  their own window's frames (`thumbs::Asker`). Questions go to the
  window in use (they say no plan); a wake redraws every window, a
  deviation `docs/PLAN.md` records. Checked on Xvfb (`docs/STATUS-LOG.md`).
- 2026-11-07, step 3: launches taken over for the program's life
  (`finishing::take_over` at start), keyed by `handover::Instance::here`
  (FNV-1a of the build, the settings and state files, `WAYLAND_DISPLAY`
  and `DISPLAY`): the bus name `org.skwir.skwir.I<key>`, the marker
  `running-<key>`, the pipe `org.skwir.skwir.running-<key>-…`;
  `--new-instance` (`handover::keep_apart`); `ui.launch_opens`;
  `App::open_window` (a launch's folder as it is); macOS's
  `on_open_urls` (compiled only). Checked on Xvfb on a bus of its own;
  Windows' hand-over tests under Wine.
- 2026-11-07, step 4: tabs dragged between windows (gpui patch 0050:
  `ExternalDragPayload::Within`, `on_drag_dropped_outside`), checked on a
  headless sway and on Xvfb, then withdrawn at the user's word, its
  behaviour differing by system (Rejected). What stays of 0050: a drag
  the platform brings in is the pointer's, so a window last used by
  keyboard takes a drop from another application.
