# ADR-018: Tabs

Status: accepted, 2026-09-26.

## Context

A window shows one pane, or two when split. People who keep several
places at hand (a project, its downloads, a search they return to) open
several windows or keep re-navigating. Every other file manager of the
desktop offers tabs.

Two models exist. In Dolphin's, a tab is a whole arrangement: one pane, or
two when that tab is split, and F3 splits the tab shown. In Total
Commander's, each side of a permanent split has its own row of tabs. The
shell is written for the first: `Shell::is_split` is "more than one pane
shown", `other_pane` is "the shown pane that is not active", and F5/F6, the
shelf's destination, the split's arrangement (`layout.rs`) and the pane
borders all read those two. Per-side tabs would make "the other pane" a
question with several answers.

The binding constraint is principle 2 (light). A pane that shows a folder
holds its listing (an entry per file), a watcher, a sorted order in the UI
and its views' state; a search holds its results and a running query.
Ten tabs that stay live would multiply idle memory and background work by
ten for folders nobody is looking at.

## Decision

- **A tab is an arrangement of panes**: one, or two when split, the order
  being the order they are laid out in. Each tab remembers its active pane
  and its own split arrangement. F3 splits the tab shown; the other tabs
  keep theirs.
- **Tabs live in the core workspace** (`Workspace::tabs`, `TabId`), beside
  the panes they group, because agents, scripts and the CLI must be able to
  list, open, select and close them (principle 6), and because dormancy
  (below) is a pane lifecycle the core owns. The commands are core
  commands (`tab.new`, `tab.open`, `tab.close`, `tab.reopen`,
  `tab.select`, `tab.move`, `tab.list`); the shell draws the strip and
  reconciles its per-pane UI state against the workspace on every sync, so
  a tab closed by an agent and one closed by a click are released the
  same way. The workspace's active pane is always in its active tab.
- **A tab not shown is dormant.** Its panes keep their history (the
  location shown is `history[history_pos]`), sort and selection; they drop
  their open location, listing, kept columns and watchers, and their
  `location_lifetime` is cancelled, so every plugin job tied to them stops
  as it does on navigation. The shell drops their sorted order and resets
  their views. Selecting the tab opens its locations again through the one
  navigation path (`App::show`); the listing streams under the loading
  banner as any navigation does, and the selection comes back once the
  listing is complete, because entries have stable identity (device +
  inode). A selection the user made in the meantime wins.
- **Closing a tab is undoable.** The core keeps the last closed tabs (a
  bounded stack in memory: histories, sort, selection, split) and
  `tab.close` answers with a toast whose Undo reopens that one
  (`tab.reopen`), as bookmark edits do through `App::notify_with_action`.
  Ctrl+Shift+T reopens the last one. The last tab cannot be closed; the
  command does nothing.
- **The strip shows only with two tabs or more**, above the panes (right of
  the sidebar), because a tab changes the panes, not the sidebar. A tab is
  named like the place it shows (the sidebar's title and icon when it shows
  a place, the folder's name otherwise, the query for a search) and
  carries its full location as a tooltip. A tab is a drop target like a
  sidebar row: files dropped on it move (copy with Ctrl) into the folder
  its active pane shows. Its × takes no focus, like a chip's; middle-click
  closes it too.
- **Keys**, bound by ops-basic like the other core commands' (Back, Up):
  Ctrl+T opens a tab on the folder shown (as a split does), Ctrl+W closes
  it, Ctrl+Shift+T reopens, Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+PgDn /
  Ctrl+PgUp cycle, Ctrl+Shift+PgDn / PgUp move the tab, Alt+1…8 select by
  position and Alt+9 the last. Two bindings move for them, as in Dolphin
  and Nautilus: Open Terminal Here goes from Ctrl+T to Shift+F4, Tags… from
  Ctrl+Shift+T to Ctrl+Shift+G. Middle-click on a folder, a sidebar place
  or a breadcrumb segment, and "Open in New Tab" in a folder's or a
  place's menu, open it in a tab behind the current one without switching;
  tabs opened one after another from the same tab line up after it.
- **The window remembers its tabs** when the startup preference is "the
  folders and tabs left open" (`core.startup = last`, until now "the last
  folder"): at close, the location each pane of
  each tab shows (not their histories), which tab was shown and which pane
  was active are written under `core.last_tabs`, beside
  `core.last_location`. At startup without a folder on the command line,
  they are restored: the shown tab opens, the others stay dormant, so
  startup opens one location whatever the number of tabs. A folder named on
  the command line opens alone, as before. The window's shape
  (`ui.session`) follows the tab shown, and restored tabs bring their own
  splits.

## Consequences

- Idle cost of a dormant tab is its history, and once it has been shown a
  `PaneUi` (an address input, focus handles, views reset): measured in the
  addendum below, and budgeted there.
- Coming back to a tab lists its folder again. For a folder this costs what
  navigating there costs (usually a warm dentry cache); for a search it
  runs the query again. Keeping the last one or two tabs warm is possible
  later and would be a budgeted change to this ADR, not a default.
- Scroll positions are not kept across dormancy; the view scrolls to the
  restored cursor.
- A drag does not switch tabs when it hovers one (spring-loading): the
  source pane of a drag must stay listed until the drop reads its
  selection. Dropping on a tab is the way to reach a tab's folder.
- Plugins keep per-pane state keyed by `PaneId`; they already release it
  when the pane's `location_lifetime` ends, which dormancy ends too.
- `Workspace::set_active` refuses a pane of another tab: making a pane of
  another tab active is selecting that tab (`App::select_tab`), which
  wakes it.

## Addendum: measurements (2026-09-26)

Release build, Xvfb with lavapipe (`AGENTS.md`, "Verifying UI changes"),
fresh XDG directories per run, the default plugins, ten synthetic folders
of 20 000 empty files each under `~/.cache/bibli-tests`. PSS from
`/proc/<pid>/smaps_rollup` after 12 s idle, two runs each, back to back.
Lavapipe maps the LLVM rasterizer, so these absolute numbers are not the
reference machine's and do not bear on ADR-002's gate; the differences do.

| Window                                          | PSS run 1 | PSS run 2 |
| ----------------------------------------------- | --------: | --------: |
| One tab on a folder                             |  174.2 MB |  174.0 MB |
| Ten tabs restored at startup, one shown         |  175.7 MB |  173.4 MB |
| Ten tabs, after showing each one in turn        |  184.3 MB |  182.6 MB |
| One tab, after navigating to the same ten       |  183.2 MB |  182.6 MB |

Ten dormant tabs cost nothing measurable over one (the runs straddle each
other within 1.5 MB). Showing each tab once costs what visiting the same
folders in one tab costs, so the growth (about 9 MB) is what listing
200 000 entries leaves behind in the process whatever holds them (its
source was not profiled further), not listings kept by dormant panes. Idle CPU returned to 0 to 0.5 % in every run.

Budget: with the default plugins, ten dormant tabs stay within 2 MB PSS
of one tab showing the same folder, and showing every tab once stays
within 2 MB of visiting the same folders in one tab. A change that keeps
tabs warm is measured against this and needs an addendum here.
