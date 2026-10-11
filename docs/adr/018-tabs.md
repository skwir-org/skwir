# ADR-018: Tabs

Status: accepted, 2026-09-26. Built. Amended 2026-09-26 (budget, holding
the tab left, optional dormancy) and 2026-09-29 (no window row).

## Context

A window shows one pane, or two when split; users keeping several places
at hand open windows or re-navigate. Every other desktop file manager has
tabs, in one of two models:

- **Dolphin:** a tab is a whole arrangement, one pane or two when split.
  Chosen.
- **Total Commander:** each side of a permanent split has its own tabs.
  Rejected: F5/F6, the shelf's destination, the split's arrangement
  (`layout.rs`) and pane borders read `Shell::is_split` and `other_pane`,
  and per-side tabs would give "the other pane" several answers.

The binding constraint is principle 2 (light). A pane holds a listing, a
watcher, a sorted order and view state; a search holds results and a
running query. Ten live tabs would cost ten times the idle memory and
background work, for folders nobody looks at.

## Decision

- **A tab is one pane, or two when split,** with its own active pane and
  split arrangement. F3 splits the tab shown.
- **Tabs are core data** (`Workspace::tabs`, `TabId`), so agents, scripts
  and the CLI drive them (principle 6) and the core owns dormancy:
  `tab.new`, `tab.open`, `tab.close`, `tab.reopen`, `tab.select`,
  `tab.move`, `tab.list`. The shell reconciles its per-pane UI state with
  the workspace on every sync, so a tab an agent closes is released like
  one closed by a click. The active pane is always in the active tab.
- **A tab not shown is dormant.** Its panes keep history (the location
  shown is `history[history_pos]`), sort and selection; they drop their
  open location, listing, kept columns and watchers, and their
  `location_lifetime` is cancelled, stopping plugin jobs. The shell drops
  their sorted order and resets their views. Selecting the tab reopens its
  locations through `App::show`; the selection returns once the listing is
  complete, unless the user selected something meanwhile.
- **Closing is undoable:** a bounded stack of closed tabs (histories, sort,
  selection, split); `tab.close` toasts with Undo (`tab.reopen`). The last
  tab cannot be closed.
- **The strip** sits above the panes, right of the sidebar: a tab changes
  the panes, not the sidebar. A tab is named like the place it shows, with
  its full location as tooltip. Files dropped on it move (Ctrl: copy) into
  its active pane's folder. Its × takes no focus; middle-click closes.
- **Keys** (ops-basic):

  | Key                                              | Action                      |
  | ------------------------------------------------ | --------------------------- |
  | Ctrl+T                                           | new tab on the folder shown |
  | Ctrl+W                                           | close                       |
  | Ctrl+Shift+T                                     | reopen the last closed      |
  | Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+PgDn / Ctrl+PgUp | next / previous             |
  | Ctrl+Shift+PgDn / Ctrl+Shift+PgUp                | move the tab                |
  | Alt+1…8, Alt+9                                   | by position; the last       |

  Moved for them, as in Dolphin and Nautilus: Open Terminal Here to
  Shift+F4, Tags… to Ctrl+Shift+G. Middle-click on a folder, a place or a
  breadcrumb segment, or "Open in New Tab", opens a tab behind the current
  one; successive ones line up after it.
- **The window remembers its tabs** with `core.startup = last`: at close,
  each pane's location (not its history), the tab shown and the active pane
  go to `core.last_tabs`. At startup without a folder argument the shown
  tab opens and the rest stay dormant, so startup opens one location
  however many tabs there are; a folder argument opens alone. The window's
  shape (`ui.session`) follows the tab shown; restored tabs bring their own
  splits.

## Consequences

- A dormant tab costs its history and, once shown, a `PaneUi` (budget
  below).
- Returning to a tab lists it again (usually a warm dentry cache; a search
  runs again). Keeping tabs warm would be a budgeted amendment, not a
  default.
- Scroll position returns to the pixel: the pane keeps its top entry and
  the offset into its row (`PaneView::scroll_anchor`, `restore_anchor`), so
  entries added above or a resize do not move what was shown. The columns
  view shows the cursor instead.
- No spring-loading on tabs during a drag: the source pane must stay listed
  until the drop reads its selection. Drop on the tab instead.
- Plugins' per-pane state (by `PaneId`) ends with `location_lifetime`,
  which dormancy ends too.
- `Workspace::set_active` refuses a pane of another tab: that is
  `App::select_tab`, which wakes it.

## Amendments

### 2026-09-26: memory measured, and the budget

Release build, Xvfb with lavapipe, fresh XDG directories, default plugins,
ten synthetic folders of 20 000 empty files; PSS from
`/proc/<pid>/smaps_rollup` after 12 s idle, two runs each, back to back.
Lavapipe maps the LLVM rasterizer: absolute numbers are not the reference
machine's and do not bear on ADR-002; the differences do.

| Window                                    | PSS run 1 | PSS run 2 |
| ----------------------------------------- | --------: | --------: |
| One tab on a folder                       |  174.2 MB |  174.0 MB |
| Ten tabs restored at startup, one shown   |  175.7 MB |  173.4 MB |
| Ten tabs, after showing each one in turn  |  184.3 MB |  182.6 MB |
| One tab, after navigating to the same ten |  183.2 MB |  182.6 MB |

Ten dormant tabs cost nothing measurable. The ~9 MB after showing each is
what listing 200 000 entries leaves in the process whatever holds them (not
profiled further), not listings kept by dormant panes. Idle CPU returned to
0–0.5 %.

**Budget:** with default plugins, ten dormant tabs stay within 2 MB PSS of
one tab on the same folder, and showing every tab once within 2 MB of
visiting the same folders in one tab. Keeping tabs warm is measured against
this and needs an amendment here.

### 2026-09-26: the tab left stays drawn until the tab shown is ready

Showing a dormant tab drew a blank pane, rows out of order, columns filling
in, a jump to the selection, and "Loading location…" past 200 ms.

- The window keeps drawing the tab left, whole, while the tab shown gets
  ready out of sight, for at most 150 ms (`tabs::HOLD`); the strip marks
  the new tab at once. Ready: listing complete, selection back, scrolled
  into place, first rows' metadata arrived (`Shell::place_pane`). Then the
  tab left is suspended (`App::settle_tabs`). With no UI holding tabs
  (`App::hold_tabs`), it is suspended at once.
- Any other user action draws the tab shown first, so a command never acts
  on a tab not drawn. Cycling draws only the tab it stops on, the deadline
  counting from the last switch. Closing the tab shown cannot hold: the
  next one draws as it streams.
- Two tabs are open together only while one is held. Warm listings were
  considered and not needed.

Key to tab drawn (release, Xvfb with lavapipe, two rounds): 300 entries
1 ms; 5 000 entries 10–34 ms; 20 000 entries 32 ms; 10 000 images
20–25 ms; a search of 10 000 results 23–37 ms; 100 000 entries 79–89 ms.
Every switch was ready in time, with no intermediate frame. Type icons not
yet seen in the session still arrive a frame later. A list and a grid
scrolled to half a row came back identical to the pixel.

### 2026-09-26: dormancy can be turned off

`core.sleep_hidden_tabs` (General, on by default). Off, every tab stays
listed and watched, tabs opened behind list at once, and all restored tabs
list at startup; switching applies at once (`App::apply_tabs_sleep`). The
budget holds for the default only; off, memory grows as with ten windows,
and a switch between two listed tabs of 2 000 entries draws in
0.02–0.04 ms.

### 2026-09-29: no window row

The window-wide toolbar is gone; each area owns its top
(`docs/invariants/shell.md`, "The top of the window"):

- The sidebar's header holds its toggle and More; with no sidebar, they
  start the first pane's row.
- Each pane's row holds that pane's tools, views and View menu included.
- Split and Preview go in the preview panel's bar while it is open at the
  right, else at the strip's right end, else at the end of the right-hand
  pane's row (the top one when stacked).

A one-tab window has one row of bars, split or not. The strip, one row
tall, shows with two tabs or more, or with one when `ui.single_tab`
(Appearance, off by default) asks; the lone tab has no ×. Without the
strip, New tab is in More, on Ctrl+T and the middle button; a second tab
brings the strip and moves the panes down a row.

Rejected after a day's trial: tabs in a window-wide row beside the window's
tools. It cost no height for a second tab, but a lone pane's row drawn
there could not hold a split (stacked panes share no band; side by side
the row would mirror every dragged edge).

Unchanged: tabs take drops, drag to reorder, close on middle click, and the
wheel over the strip cycles them.
