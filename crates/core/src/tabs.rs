//! Tabs (ADR-018): arrangements of panes, one shown at a time.
//!
//! The workspace groups its panes in tabs; this module is what changes them
//! with the panes' lifecycle: a tab not shown is dormant (its panes let go of
//! their listings and watchers, `Pane::suspend`), a tab shown opens its
//! panes' locations again through the one navigation path (`App::show`). It
//! also keeps the tabs closed last, so a close can be undone, and what the
//! window's tabs were when it closed, for the next start.

use std::collections::VecDeque;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::app::{App, NotifyLevel};
use crate::command::{Form, Invocation, Outcome, Topic};
use crate::entry::EntryId;
use crate::location::{Location, SortSpec};
use crate::os_path::PathArg;
use crate::settings::{STARTUP, SettingSpec};
use crate::workspace::{PaneId, Selection, StepLeft, TabId, WindowId};

/// Application data: the windows shown and their tabs, as they were
/// last (ADR-122).
pub const LAST_WINDOWS: &str = "core.last_windows";

/// Application data before there were windows: the tabs the window showed
/// when it closed. Read when `LAST_WINDOWS` is not there yet, removed once
/// it is written.
pub const LAST_TABS: &str = "core.last_tabs";

/// Whether tabs not shown let go of their listings (the default), or all
/// stay listed and current.
pub const SLEEP_KEY: &str = "core.sleep_hidden_tabs";

/// What a new tab's pane is filtered by.
enum Filtered {
    /// The lens of the pane it opens from (ADR-107).
    Lens,
    /// A place as it was kept (ADR-109): this filter alone, as its reader
    /// read it.
    Kept(Option<(Arc<str>, Option<crate::workspace::ReadFilter>)>),
}

/// What filtered a pane, read as it was left (ADR-109).
type LeftFilter = Option<(Arc<str>, Option<crate::workspace::ReadFilter>)>;

/// How many closed tabs can be reopened.
const CLOSED_KEPT: usize = 16;

/// A closed tab, as it can come back: each pane's history, sort and
/// selection, in the tab's order.
#[derive(Clone, Debug)]
struct ClosedTab {
    index: usize,
    panes: Vec<ClosedPane>,
    active: usize,
}

/// What a close took away, as it can come back: a tab, back in its window
/// (else the active one), or a window, its tabs and shape (ADR-122).
#[derive(Clone, Debug)]
enum ClosedItem {
    Tab {
        window: WindowId,
        tab: ClosedTab,
    },
    Window {
        tabs: Vec<ClosedTab>,
        shown: usize,
        shape: Option<toml::Value>,
        from: Option<WindowId>,
    },
}

/// What reopening a close brought back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reopened {
    Tab(TabId),
    Window(WindowId),
}

#[derive(Clone, Debug)]
struct ClosedPane {
    history: Vec<Location>,
    history_pos: usize,
    /// What each step had when left, the one shown's filter as it was
    /// (ADR-109).
    steps: Vec<StepLeft>,
    sort: SortSpec,
    selection: Selection,
}

impl ClosedPane {
    /// Where in its history it was, within it: a pane is recorded with a
    /// history (`close_tab`), never an empty one.
    fn pos(&self) -> usize {
        self.history_pos.min(self.history.len().saturating_sub(1))
    }
}

/// The tabs and windows closed last, newest last, each with the serial
/// that reopens it.
#[derive(Default)]
pub struct Closed {
    items: VecDeque<(u64, ClosedItem)>,
    next: u64,
}

/// The window's tabs as the settings keep them: the location each pane
/// showed and what filtered it there (ADR-109), not their histories.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedTabs {
    pub tabs: Vec<SavedTab>,
    /// Index of the tab shown.
    pub shown: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedTab {
    pub panes: Vec<Location>,
    /// Index of the active pane.
    pub active: usize,
    /// By pane, what filtered the location it showed (the `filter`
    /// command's text), empty for nothing: a search's terms say its own.
    /// Left out when nothing was filtered, as the file was before.
    #[serde(skip_serializing_if = "filters_none")]
    pub filters: Vec<String>,
    /// By pane, how it was sorted and grouped (ADR-130, 11, amended): its
    /// criterion and direction, its groups and theirs, and hidden files;
    /// folders first is the user's setting, which every pane takes. Left
    /// out when each pane sorts as a new one does. A sort this Skwir
    /// cannot read (a later one's criterion) is a new pane's: the windows
    /// still come back.
    #[serde(skip_serializing_if = "sorts_none", deserialize_with = "sorts_as_read")]
    pub sorts: Vec<SortSpec>,
}

fn filters_none(filters: &[String]) -> bool {
    filters.iter().all(String::is_empty)
}

/// Whether every pane sorts as a new one does, folders first aside: the
/// user's, never a pane's.
fn sorts_none(sorts: &[SortSpec]) -> bool {
    sorts.iter().all(|sort| {
        *sort
            == SortSpec {
                folders_first: sort.folders_first,
                ..SortSpec::default()
            }
    })
}

/// The panes' sorts as kept, each one that cannot be read a new pane's,
/// and none where what is kept is no list.
fn sorts_as_read<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<SortSpec>, D::Error> {
    Ok(match toml::Value::deserialize(d)? {
        toml::Value::Array(sorts) => sorts
            .into_iter()
            .map(|sort| sort.try_into().unwrap_or_default())
            .collect(),
        _ => Vec::new(),
    })
}

/// The windows as the state keeps them (ADR-122): each one's tabs and
/// shape, and the one in use.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedWindows {
    pub windows: Vec<SavedWindow>,
    /// Index of the window in use.
    pub active: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedWindow {
    pub tabs: Vec<SavedTab>,
    /// Index of the tab shown.
    pub shown: usize,
    /// What the UI kept of its shape (`Workspace::set_shape`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<toml::Value>,
}

impl From<SavedTabs> for SavedWindows {
    /// Tabs kept before there were windows: one window's.
    fn from(saved: SavedTabs) -> Self {
        SavedWindows {
            windows: vec![SavedWindow {
                tabs: saved.tabs,
                shown: saved.shown,
                shape: None,
            }],
            active: 0,
        }
    }
}

/// A tab as `tab.list` reports it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TabInfo {
    pub id: TabId,
    /// The window it is in.
    pub window: WindowId,
    pub title: String,
    pub panes: Vec<PaneInfo>,
    pub active: PaneId,
    /// Shown in its window.
    pub shown: bool,
}

/// A window as `window.list` reports it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WindowInfo {
    pub id: WindowId,
    /// Its tabs, in order.
    pub tabs: Vec<TabId>,
    /// The tab it shows.
    pub shown: TabId,
    /// The window commands act in when they name no pane.
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PaneInfo {
    pub id: PaneId,
    pub location: Option<Location>,
    /// Waiting for its tab to be shown to open its location.
    pub dormant: bool,
}

impl App {
    /// A name for `location`, as a tab, the history or a message says it:
    /// the sidebar place it is, under the name the user gave it there
    /// (`Registry::place_title`), else the folder's name, else what its
    /// provider calls it (`LocationProvider::title`), else the text after
    /// the scheme.
    pub fn location_title(&self, location: &Location) -> String {
        if let Some(title) = self.registry.place_title(location) {
            return title;
        }
        if let Some(path) = location.path() {
            return path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
        }
        if let Some(title) = self
            .provider(&location.scheme)
            .and_then(|provider| provider.title(location))
        {
            return title;
        }
        let rest = location.rest.trim();
        if rest.is_empty() {
            location.scheme.to_string()
        } else {
            rest.to_owned()
        }
    }

    /// Show tab `tab`: the panes of the tab left let go of their listings,
    /// those of `tab` open their locations again. While a UI holds tabs
    /// (`hold_tabs`), the panes left stay open until `settle_tabs`, so it
    /// can keep drawing them until the tab shown is ready.
    pub fn select_tab(&self, tab: TabId) -> anyhow::Result<()> {
        let sleep = self.tabs_sleep();
        {
            let mut ws = self.workspace.write();
            let window = ws
                .window_of_tab(tab)
                .ok_or_else(|| anyhow::anyhow!("no such tab"))?;
            let left = ws
                .window(window)
                .map(crate::workspace::WindowTabs::shown)
                .filter(|shown| shown.id != tab)
                .map(|shown| shown.panes.clone());
            if let Some(left) = left {
                ws.set_active_tab(tab);
                leave(&mut ws, window, left, sleep);
            }
        }
        self.resume_shown()
    }

    /// Let every pane sleep (`Pane::suspend`): the window closed while
    /// actions finish without it (ADR-115). Nothing is listed or watched
    /// meanwhile; a window opened again resumes the shown tab
    /// (`resume_tabs`), as at startup.
    pub fn suspend_panes(&self) {
        self.hold_tabs(false);
        let mut ws = self.workspace.write();
        let panes: Vec<PaneId> = ws.panes().iter().map(|p| p.id).collect();
        suspend(&mut ws, panes);
    }

    /// Whether the tab left stays open until `settle_tabs`: set by a UI
    /// that keeps drawing it while the tab shown lists (ADR-018 addendum).
    /// Without one, a tab left is suspended at once.
    pub fn hold_tabs(&self, on: bool) {
        self.workspace.write().holding = on;
        if !on {
            self.settle_tabs();
        }
    }

    /// The tabs left are no longer drawn: suspend their panes, those of the
    /// tab shown excepted (it may be the one left, shown again).
    pub fn settle_tabs(&self) {
        let windows: Vec<WindowId> = self
            .workspace
            .read()
            .windows()
            .iter()
            .map(|w| w.id)
            .collect();
        for window in windows {
            self.settle_window(window);
        }
    }

    /// `window`'s tab left is no longer drawn: suspend its panes, those of
    /// a tab shown excepted (it may be the one left, shown again, or moved
    /// to another window).
    pub fn settle_window(&self, window: WindowId) {
        let sleep = self.tabs_sleep();
        let mut ws = self.workspace.write();
        let Some(leaving) = ws
            .window_mut(window)
            .map(|w| std::mem::take(&mut w.leaving))
        else {
            return;
        };
        let shown = shown_panes(&ws);
        let asleep = leaving
            .into_iter()
            .filter(|id| sleep && !shown.contains(id));
        suspend(&mut ws, asleep);
    }

    /// Whether tabs not shown let go of their listings (`SLEEP_KEY`, on
    /// by default): read when a tab is left, so a change applies at once.
    pub fn tabs_sleep(&self) -> bool {
        self.settings.get::<bool>(SLEEP_KEY).unwrap_or(true)
    }

    /// Open the tabs at startup: the shown one first, and, when tabs do not
    /// sleep, every other one after it.
    pub fn resume_tabs(&self) -> anyhow::Result<()> {
        let shown = self.resume_shown();
        if !self.tabs_sleep() {
            self.apply_tabs_sleep();
        }
        shown
    }

    /// Bring the tabs in line with `SLEEP_KEY` after it changed: every tab
    /// listed again when they no longer sleep, or those not shown (nor still
    /// drawn, while a UI holds one) suspended when they do.
    pub fn apply_tabs_sleep(&self) {
        let sleep = self.tabs_sleep();
        if sleep {
            let mut ws = self.workspace.write();
            let mut keep = shown_panes(&ws);
            keep.extend(ws.windows().iter().flat_map(|w| w.leaving.iter().copied()));
            let asleep: Vec<PaneId> = ws
                .panes()
                .iter()
                .map(|p| p.id)
                .filter(|id| !keep.contains(id))
                .collect();
            suspend(&mut ws, asleep);
        } else {
            let dormant: Vec<PaneId> = self
                .workspace
                .read()
                .panes()
                .iter()
                .filter(|p| p.is_dormant())
                .map(|p| p.id)
                .collect();
            for pane in dormant {
                if let Err(error) = self.resume(pane) {
                    tracing::warn!("cannot list a tab again: {error:#}");
                }
            }
        }
        self.wake();
    }

    /// Open the locations of the dormant panes of the tab each window
    /// shows: at startup, when a tab is shown, after a shown tab closed.
    pub fn resume_shown(&self) -> anyhow::Result<()> {
        let panes = shown_panes(&self.workspace.read());
        let mut failed = None;
        for pane in panes {
            if let Err(error) = self.resume(pane) {
                failed.get_or_insert(error);
            }
        }
        self.wake();
        failed.map_or(Ok(()), Err)
    }

    /// Open what dormant `pane` shows. A location an agent's call put
    /// there is opened as that call, wherever it is opened (the window's
    /// thread, once its tab is shown): what a provider asks before it acts
    /// for an agent (`permissions::agent_call`) is asked.
    fn resume(&self, pane: PaneId) -> anyhow::Result<()> {
        let (location, restore, asked_by) = {
            let mut ws = self.workspace.write();
            let Some(p) = ws.pane_mut(pane) else {
                return Ok(());
            };
            if !p.is_dormant() {
                return Ok(());
            }
            let Some(location) = p.shown().cloned() else {
                return Ok(());
            };
            (location, p.take_restore_selection(), p.take_asked_by())
        };
        let wake = || {
            self.wake_pane(pane, &location, {
                let restore = restore.clone();
                move |p, shown| {
                    p.set_restore(restore);
                    // A search shown with the lens the tab was opened with
                    // (ADR-107) is its step as shown.
                    if p.history.get(p.history_pos) != Some(shown) {
                        p.replace_here(shown.clone());
                    }
                }
            })
        };
        let shown = match asked_by.clone() {
            Some(privacy) => crate::permissions::as_agent(privacy, wake),
            None => wake(),
        };
        if shown.is_err()
            && let Some(p) = self.workspace.write().pane_mut(pane)
        {
            p.set_restore(restore);
            p.set_asked_by(asked_by);
        }
        shown
    }

    /// A new tab showing `location`, or what `from` (the active pane when
    /// `None`) shows, with its sort and its lens (ADR-107). Shown at once
    /// and put last, or opened behind the shown tab, beside it.
    pub fn new_tab(
        &self,
        from: Option<PaneId>,
        location: Option<Location>,
        show: bool,
    ) -> anyhow::Result<(TabId, PaneId)> {
        self.new_tab_as(from, location, Filtered::Lens, show)
    }

    /// A new tab showing `location` as it was kept (ADR-109: a saved place,
    /// a saved search), filtered by `filter` alone, with `from`'s sort: as
    /// `new_tab`, but the pane's lens is not brought in.
    pub fn new_tab_as_kept(
        &self,
        from: Option<PaneId>,
        location: Location,
        filter: Option<&str>,
        show: bool,
    ) -> anyhow::Result<(TabId, PaneId)> {
        // Read before the workspace's lock: the reader is a plugin's.
        let filter = self
            .kept_filter(filter)
            .and_then(|filter| self.read_as_left(&filter));
        self.new_tab_as(from, Some(location), Filtered::Kept(filter), show)
    }

    /// `new_tab`, its pane filtered as `filtered` says.
    fn new_tab_as(
        &self,
        from: Option<PaneId>,
        location: Option<Location>,
        filtered: Filtered,
        show: bool,
    ) -> anyhow::Result<(TabId, PaneId)> {
        let (tab, pane) = {
            let mut ws = self.workspace.write();
            let from = from.unwrap_or_else(|| ws.active_id());
            // In the window of the pane it opens from.
            let window = ws.window_of(from).unwrap_or_else(|| ws.active_window_id());
            let source = ws.pane(from);
            let sort = source.map(|p| p.sort).unwrap_or_default();
            let lens = source.and_then(|p| p.lens().cloned());
            let location = match location {
                Some(location) => location,
                None => source
                    .and_then(|p| p.shown())
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("the pane shows nothing to open in a tab"))?,
            };
            let at = if show {
                ws.window(window).map_or(0, |w| w.tabs().len())
            } else {
                ws.beside_position(window)
            };
            let (tab, panes) = ws.add_dormant_tab_in(window, at, vec![(location, sort)]);
            if let Some(p) = ws.pane_mut(panes[0]) {
                match filtered {
                    Filtered::Kept(filter) => p.filter_as_left(filter),
                    Filtered::Lens => p.carry(lens),
                }
            }
            if !show {
                ws.opened_beside(tab);
            }
            (tab, panes[0])
        };
        if show {
            self.select_tab(tab)?;
        } else if self.tabs_sleep() {
            self.wake();
        } else {
            self.resume(pane)?;
            self.wake();
        }
        Ok((tab, pane))
    }

    /// Close tab `tab`; the last tab of a window stays. Returns the serial
    /// that reopens it and its title, or `None` when it was its window's
    /// last tab.
    pub fn close_tab(&self, tab: TabId) -> anyhow::Result<Option<(u64, String)>> {
        let (window, index, closed, panes, was_shown) = {
            let mut ws = self.workspace.write();
            let window = ws
                .window_of_tab(tab)
                .ok_or_else(|| anyhow::anyhow!("no such tab"))?;
            let was_shown = ws.window(window).is_some_and(|w| w.shown().id == tab);
            let Some((index, closed, panes)) = ws.close_tab(tab) else {
                return Ok(None);
            };
            (window, index, closed, panes, was_shown)
        };
        let (record, title) = self.closed_record(index, &closed, &by_id(&panes));
        // Dropping the panes closes their locations: outside the lock.
        drop(panes);
        let serial = self.keep_closed(record.map(|tab| ClosedItem::Tab { window, tab }));
        if was_shown {
            self.resume_shown()?;
        } else {
            self.wake();
        }
        Ok(Some((serial, title)))
    }

    /// Close window `window`, its tabs kept to be reopened together
    /// (`reopen_tab`). Returns the serial that reopens it, or `None` for
    /// the last window, which only the UI closes (ADR-115).
    pub fn close_window(&self, window: WindowId) -> anyhow::Result<Option<u64>> {
        let (closed, panes) = {
            let mut ws = self.workspace.write();
            anyhow::ensure!(ws.window(window).is_some(), "no such window");
            let Some((_, closed, panes)) = ws.close_window(window) else {
                return Ok(None);
            };
            (closed, panes)
        };
        let panes_by_id = by_id(&panes);
        let mut tabs = Vec::new();
        let mut shown = 0;
        for (index, tab) in closed.tabs().iter().enumerate() {
            if let (Some(record), _) = self.closed_record(index, tab, &panes_by_id) {
                if tab.id == closed.shown().id {
                    shown = tabs.len();
                }
                tabs.push(record);
            }
        }
        drop(panes_by_id);
        drop(panes);
        let item = (!tabs.is_empty()).then(|| ClosedItem::Window {
            tabs,
            shown,
            shape: closed.shape().cloned(),
            from: closed.made_from(),
        });
        let serial = self.keep_closed(item);
        self.wake();
        Ok(Some(serial))
    }

    /// What of `tab`, closed from position `index`, can come back, and its
    /// title: each pane with a history, from `panes`.
    fn closed_record(
        &self,
        index: usize,
        tab: &crate::workspace::Tab,
        panes: &std::collections::HashMap<PaneId, &crate::workspace::Pane>,
    ) -> (Option<ClosedTab>, String) {
        let in_order: Vec<&crate::workspace::Pane> = tab
            .panes
            .iter()
            .filter_map(|id| panes.get(id).copied())
            .collect();
        let active = in_order
            .iter()
            .position(|p| p.id == tab.active)
            .unwrap_or(0);
        let record: Vec<ClosedPane> = in_order
            .iter()
            .filter(|p| !p.history.is_empty())
            .map(|p| ClosedPane {
                history: p.history.clone(),
                history_pos: p.history_pos,
                steps: p.steps_left(),
                sort: p.sort,
                selection: p.kept_selection().clone(),
            })
            .collect();
        let title = in_order
            .get(active)
            .or_else(|| in_order.first())
            .and_then(|p| p.shown())
            .map(|location| self.location_title(location))
            .unwrap_or_default();
        let record = (!record.is_empty()).then(|| ClosedTab {
            index,
            active: active.min(record.len() - 1),
            panes: record,
        });
        (record, title)
    }

    /// Keep `item` to be reopened, if there is one: the serial that
    /// reopens it.
    fn keep_closed(&self, item: Option<ClosedItem>) -> u64 {
        let mut closed = self.closed_tabs.lock();
        closed.next += 1;
        let serial = closed.next;
        if let Some(item) = item {
            closed.items.push_back((serial, item));
            while closed.items.len() > CLOSED_KEPT {
                closed.items.pop_front();
            }
        }
        serial
    }

    /// Open again the tab or window closed with `serial`, or the one closed
    /// last, where it was, and show it: a tab in its window (else the
    /// active one), a window made active. `None` when there is none to
    /// reopen.
    pub fn reopen_tab(&self, serial: Option<u64>) -> anyhow::Result<Option<Reopened>> {
        let closed = {
            let mut closed = self.closed_tabs.lock();
            let at = match serial {
                Some(serial) => closed.items.iter().position(|(s, _)| *s == serial),
                None => closed.items.len().checked_sub(1),
            };
            at.and_then(|at| closed.items.remove(at))
        };
        let Some((_, closed)) = closed else {
            return Ok(None);
        };
        match closed {
            ClosedItem::Tab { window, tab } => {
                // What filtered each pane's step shown, read before the
                // workspace's lock: the reader is a plugin's.
                let filters = self.closed_filters(&tab);
                let id = {
                    let mut ws = self.workspace.write();
                    let window = ws
                        .window(window)
                        .map_or_else(|| ws.active_window_id(), |w| w.id);
                    let (id, ids) = ws.add_dormant_tab_in(window, tab.index, closed_panes(&tab));
                    fill_closed(&mut ws, id, &ids, tab, filters);
                    id
                };
                self.select_tab(id)?;
                Ok(Some(Reopened::Tab(id)))
            }
            ClosedItem::Window {
                tabs,
                shown,
                shape,
                from,
            } => {
                let filters: Vec<_> = tabs.iter().map(|tab| self.closed_filters(tab)).collect();
                let window = {
                    let mut ws = self.workspace.write();
                    let mut window = None;
                    let mut shown_tab = None;
                    for (i, (tab, filters)) in tabs.into_iter().zip(filters).enumerate() {
                        let panes = closed_panes(&tab);
                        let (id, ids) = match window {
                            None => {
                                let (w, id, ids) = ws.add_window(from, panes);
                                window = Some(w);
                                (id, ids)
                            }
                            Some(w) => ws.add_dormant_tab_in(w, usize::MAX, panes),
                        };
                        fill_closed(&mut ws, id, &ids, tab, filters);
                        if i == shown || shown_tab.is_none() {
                            shown_tab = Some(id);
                        }
                    }
                    let window = window.expect("a window is kept with a tab");
                    if let Some(tab) = shown_tab {
                        ws.set_active_tab(tab);
                    }
                    ws.set_shape(window, shape);
                    ws.set_active_window(window);
                    window
                };
                self.resume_shown()?;
                Ok(Some(Reopened::Window(window)))
            }
        }
    }

    /// What filtered each pane of `tab` at the step it showed, read as it
    /// was left (ADR-109). Not under the workspace's lock: the reader is a
    /// plugin's.
    fn closed_filters(&self, tab: &ClosedTab) -> Vec<LeftFilter> {
        tab.panes
            .iter()
            .map(|p| {
                p.steps
                    .get(p.pos())
                    .and_then(StepLeft::filter)
                    .and_then(|filter| self.read_as_left(filter))
            })
            .collect()
    }

    /// A new window showing `location`, or what `from` (the active pane
    /// when `None`) shows, with its sort and its lens, made from `from`'s
    /// window and made active (ADR-122).
    pub fn new_window(
        &self,
        from: Option<PaneId>,
        location: Option<Location>,
    ) -> anyhow::Result<(WindowId, TabId, PaneId)> {
        let (window, tab, pane) = {
            let mut ws = self.workspace.write();
            let from = from.unwrap_or_else(|| ws.active_id());
            let made_from = ws.window_of(from);
            let source = ws.pane(from);
            let sort = source.map(|p| p.sort).unwrap_or_default();
            let lens = source.and_then(|p| p.lens().cloned());
            let location = match location {
                Some(location) => location,
                None => source
                    .and_then(|p| p.shown())
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("the pane shows nothing to open in a window"))?,
            };
            let (window, tab, panes) = ws.add_window(made_from, vec![(location, sort)]);
            if let Some(p) = ws.pane_mut(panes[0]) {
                p.carry(lens);
            }
            ws.set_active_window(window);
            (window, tab, panes[0])
        };
        self.resume_shown()?;
        Ok((window, tab, pane))
    }

    /// A new window showing `location` as it is, asked from outside (a
    /// launch, ADR-122): no pane's sort or lens; made from the active
    /// window, whose shape it takes, and made active.
    pub fn open_window(&self, location: Location) -> anyhow::Result<(WindowId, TabId, PaneId)> {
        let (window, tab, pane) = {
            let mut ws = self.workspace.write();
            let from = ws.active_window_id();
            let (window, tab, panes) =
                ws.add_window(Some(from), vec![(location, SortSpec::default())]);
            ws.set_active_window(window);
            (window, tab, panes[0])
        };
        self.resume_shown()?;
        Ok((window, tab, pane))
    }

    /// Move tab `tab` to `window` (a new window made from its own when
    /// `None`), at `to` (last when `None`), shown there, the window made
    /// active. The tab shown there before is left; a tab alone in its
    /// window takes the window with it. What it moved to.
    pub fn move_tab_to_window(
        &self,
        tab: TabId,
        window: Option<WindowId>,
        to: Option<usize>,
    ) -> anyhow::Result<WindowId> {
        let sleep = self.tabs_sleep();
        let window = {
            let mut ws = self.workspace.write();
            match window {
                Some(window) => {
                    let left = ws.window(window).map(|w| w.shown().panes.clone());
                    ws.move_tab_to_window(tab, window, to)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    leave(&mut ws, window, left.unwrap_or_default(), sleep);
                    window
                }
                None => ws.tab_to_new_window(tab).map_err(|e| anyhow::anyhow!(e))?,
            }
        };
        self.resume_shown()?;
        Ok(window)
    }

    /// What `ids` (else `pane`'s selection) lead to that opens in a tab or
    /// a window, in the order the pane shows them: folders and places (a
    /// tag's), a folder that does not open at its path (a trashed one)
    /// where it leads. Refused when what was asked holds none.
    fn selected_folders(&self, pane: PaneId, ids: Vec<EntryId>) -> anyhow::Result<Vec<Location>> {
        let (asked, folders): (bool, Vec<Location>) = {
            let ws = self.workspace.read();
            let Some(p) = ws.pane(pane) else {
                anyhow::bail!("no such pane");
            };
            // In the order the pane shows them, so the tabs line up the
            // same.
            let ids: Vec<EntryId> = if ids.is_empty() {
                p.order()
                    .iter()
                    .copied()
                    .filter(|id| p.selection.contains(*id))
                    .collect()
            } else {
                ids
            };
            (
                !ids.is_empty(),
                ids.into_iter().filter_map(|id| p.leads_to(id)).collect(),
            )
        };
        if folders.is_empty() {
            anyhow::ensure!(!asked, "Only folders and places open in a tab or a window");
            return Err(crate::command::NothingSelected.into());
        }
        Ok(folders)
    }

    /// Every window, for scripts and agents.
    pub fn window_list(&self) -> Vec<WindowInfo> {
        let ws = self.workspace.read();
        let active = ws.active_window_id();
        ws.windows()
            .iter()
            .map(|w| WindowInfo {
                id: w.id,
                tabs: w.tabs().iter().map(|t| t.id).collect(),
                shown: w.shown().id,
                active: w.id == active,
            })
            .collect()
    }

    /// Every tab, window after window, in order, for scripts and agents.
    pub fn tab_list(&self) -> Vec<TabInfo> {
        let rows: Vec<_> = {
            let ws = self.workspace.read();
            ws.windows()
                .iter()
                .flat_map(|w| {
                    let shown = w.shown().id;
                    let ws = &ws;
                    w.tabs().iter().map(move |tab| {
                        let panes: Vec<PaneInfo> = tab
                            .panes
                            .iter()
                            .filter_map(|id| ws.pane(*id))
                            .map(|p| PaneInfo {
                                id: p.id,
                                location: p.shown().cloned(),
                                dormant: p.is_dormant(),
                            })
                            .collect();
                        let location = ws.pane(tab.active).and_then(|p| p.shown()).cloned();
                        (tab.id, w.id, panes, tab.active, tab.id == shown, location)
                    })
                })
                .collect()
        };
        // Titles read the places registry: not under the workspace lock.
        rows.into_iter()
            .map(|(id, window, panes, active, shown, location)| TabInfo {
                id,
                window,
                title: location
                    .map(|l| self.location_title(&l))
                    .unwrap_or_default(),
                panes,
                active,
                shown,
            })
            .collect()
    }

    /// Put the windows of `saved` in place of the workspace's one untouched
    /// window, their tabs dormant: `resume_shown` opens the shown ones once
    /// the UI is there to be woken. Returns false when there was nothing to
    /// restore.
    pub fn restore_windows(&self, saved: &SavedWindows) -> bool {
        // What filtered each pane, read before the workspace's lock: the
        // reader is a plugin's (ADR-109).
        let filters: Vec<Vec<Vec<LeftFilter>>> = saved
            .windows
            .iter()
            .map(|window| {
                window
                    .tabs
                    .iter()
                    .map(|tab| {
                        tab.filters
                            .iter()
                            .take(2)
                            .map(|text| {
                                self.kept_filter(Some(text))
                                    .and_then(|filter| self.read_as_left(&filter))
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
        let mut ws = self.workspace.write();
        let first_window = ws.active_window_id();
        let Some(first_tab) = ws.tabs().first().map(|t| t.id) else {
            return false;
        };
        let mut first_used = false;
        let mut restored = Vec::new();
        for (i, (saved_window, filters)) in saved.windows.iter().zip(filters).enumerate() {
            let into = (!first_used).then_some(first_window);
            if let Some(window) = restore_window(&mut ws, into, saved_window, filters) {
                first_used |= window == first_window;
                restored.push((i, window));
            }
        }
        let Some(&(_, first)) = restored.first() else {
            return false;
        };
        let active = restored
            .iter()
            .find(|(i, _)| *i == saved.active)
            .map_or(first, |(_, window)| *window);
        ws.set_active_window(active);
        if first_used {
            ws.close_tab(first_tab);
        } else {
            ws.close_window(first_window);
        }
        drop(ws);
        tracing::debug!(windows = restored.len(), "restored the last windows");
        true
    }

    /// Every window, as `LAST_WINDOWS` keeps them: those that show nothing
    /// left out.
    pub fn saved_windows(&self) -> SavedWindows {
        let ws = self.workspace.read();
        let active = ws.active_window_id();
        let mut saved = SavedWindows::default();
        for window in ws.windows() {
            let kept = self.saved_window(&ws, window);
            if kept.tabs.is_empty() {
                continue;
            }
            if window.id == active {
                saved.active = saved.windows.len();
            }
            saved.windows.push(kept);
        }
        saved
    }

    /// `window`'s tabs and shape, as the state keeps them.
    fn saved_window(
        &self,
        ws: &crate::workspace::Workspace,
        window: &crate::workspace::WindowTabs,
    ) -> SavedWindow {
        let mut saved = SavedWindow {
            shape: window.shape().cloned(),
            ..SavedWindow::default()
        };
        let shown = window.shown().id;
        for tab in window.tabs() {
            let mut panes = Vec::new();
            let mut filters = Vec::new();
            let mut sorts = Vec::new();
            let mut active = 0;
            for id in &tab.panes {
                let pane = ws.pane(*id);
                let Some(location) = pane.and_then(|p| p.shown()) else {
                    continue;
                };
                filters.push(pane.and_then(|p| p.filter()).unwrap_or_default().to_owned());
                sorts.push(pane.map(|p| p.sort).unwrap_or_default());
                if *id == tab.active {
                    active = panes.len();
                }
                // A remote folder is kept as its address: a later start
                // connects again (ADR-077).
                panes.push(
                    self.address_of(location)
                        .unwrap_or_else(|| location.clone()),
                );
            }
            if panes.is_empty() {
                continue;
            }
            if tab.id == shown {
                saved.shown = saved.tabs.len();
            }
            // Nothing filtered, nothing sorted otherwise: written, and read
            // back, as before.
            if filters_none(&filters) {
                filters.clear();
            }
            if sorts_none(&sorts) {
                sorts.clear();
            }
            saved.tabs.push(SavedTab {
                panes,
                active,
                filters,
                sorts,
            });
        }
        saved
    }
}

/// Put the tabs of `saved` into `into`, or a new window when `None`,
/// dormant, with what filtered each pane and how it was sorted, its shown
/// tab and its shape: the
/// window they went to, `None` when it had no tab to restore.
fn restore_window(
    ws: &mut crate::workspace::Workspace,
    mut into: Option<WindowId>,
    saved: &SavedWindow,
    filters: Vec<Vec<LeftFilter>>,
) -> Option<WindowId> {
    let mut shown = None;
    for (i, (saved_tab, filters)) in saved.tabs.iter().zip(filters).enumerate() {
        if saved_tab.panes.is_empty() {
            continue;
        }
        // Each pane sorted as it was; folders first the user's
        // (`Workspace::push_pane`).
        let panes: Vec<(Location, SortSpec)> = saved_tab
            .panes
            .iter()
            .take(2)
            .enumerate()
            .map(|(at, l)| {
                let sort = saved_tab.sorts.get(at).copied().unwrap_or_default();
                (l.clone(), sort)
            })
            .collect();
        let (tab, ids) = if let Some(window) = into {
            ws.add_dormant_tab_in(window, usize::MAX, panes)
        } else {
            let (window, tab, ids) = ws.add_window(None, panes);
            into = Some(window);
            (tab, ids)
        };
        for (id, filter) in ids.iter().zip(filters) {
            if let Some(p) = ws.pane_mut(*id) {
                p.filter_as_left(filter);
            }
        }
        if let (Some(t), Some(active)) = (ws.tab_mut(tab), ids.get(saved_tab.active)) {
            t.active = *active;
        }
        if i == saved.shown || shown.is_none() {
            shown = Some(tab);
        }
    }
    let shown = shown?;
    ws.set_active_tab(shown);
    let window = into?;
    ws.set_shape(window, saved.shape.clone());
    Some(window)
}

/// `panes` by their id.
fn by_id(
    panes: &[crate::workspace::Pane],
) -> std::collections::HashMap<PaneId, &crate::workspace::Pane> {
    panes.iter().map(|p| (p.id, p)).collect()
}

/// Where each pane of closed `tab` was, and its sort: what its pane opens
/// again.
fn closed_panes(tab: &ClosedTab) -> Vec<(Location, SortSpec)> {
    tab.panes
        .iter()
        .map(|p| (p.history[p.pos()].clone(), p.sort))
        .collect()
}

/// Give the panes `ids` of reopened tab `id` what closed `tab` kept: their
/// histories, filters and selections, and its active pane.
fn fill_closed(
    ws: &mut crate::workspace::Workspace,
    id: TabId,
    ids: &[PaneId],
    tab: ClosedTab,
    filters: Vec<LeftFilter>,
) {
    for ((pane, record), filter) in ids.iter().zip(tab.panes).zip(filters) {
        if let Some(p) = ws.pane_mut(*pane) {
            p.history_pos = record.pos();
            p.history = record.history;
            p.set_steps_left(record.steps);
            p.filter_as_left(filter);
            p.set_restore(Some(record.selection));
        }
    }
    if let (Some(t), Some(active)) = (ws.tab_mut(id), ids.get(tab.active)) {
        t.active = *active;
    }
}

/// The panes of the tab each window shows.
fn shown_panes(ws: &crate::workspace::Workspace) -> Vec<PaneId> {
    ws.windows()
        .iter()
        .flat_map(|w| w.shown().panes.iter().copied())
        .collect()
}

/// `window`'s tab of `left` is no longer shown: its panes sleep, now or,
/// while a UI still draws it (`hold_tabs`), once it settles. Without sleep
/// every tab stays listed: nothing to let go of.
fn leave(ws: &mut crate::workspace::Workspace, window: WindowId, left: Vec<PaneId>, sleep: bool) {
    if !sleep {
        return;
    }
    if ws.holding
        && let Some(w) = ws.window_mut(window)
    {
        w.leaving.extend(left);
    } else {
        suspend(ws, left);
    }
}

/// Let `panes` go to sleep (`Pane::suspend`), under the lock.
fn suspend(ws: &mut crate::workspace::Workspace, panes: impl IntoIterator<Item = PaneId>) {
    for id in panes {
        if let Some(pane) = ws.pane_mut(id) {
            pane.suspend();
        }
    }
}

/// The windows to open at startup: those left, when the startup
/// preference is to come back to them and they had tabs. Tabs kept before
/// there were windows are one window's.
pub fn last_windows(settings: &crate::settings::Settings) -> Option<SavedWindows> {
    if settings.get::<String>(STARTUP).as_deref() != Some("last") {
        return None;
    }
    settings
        .get::<SavedWindows>(LAST_WINDOWS)
        .filter(|saved| saved.windows.iter().any(|w| !w.tabs.is_empty()))
        .or_else(|| {
            settings
                .get::<SavedTabs>(LAST_TABS)
                .filter(|saved| !saved.tabs.is_empty())
                .map(SavedWindows::from)
        })
}

/// Record where the windows are, for the next start: the active pane's
/// folder and every window's tabs and shape. Only sets the values: whether
/// any changed, for the caller to save (`remember` saves at once).
pub fn record(app: &App) -> bool {
    let settings = &app.settings;
    if settings.get::<String>(STARTUP).as_deref() != Some("last") {
        return false;
    }
    // A remote folder is kept by its tab's address, which connects
    // again (ADR-077); its mount's path may name nothing next time.
    let folder = app
        .workspace
        .read()
        .active()
        .location()
        .and_then(Location::path)
        .filter(|folder| crate::fs::reach::of(folder).is_none());
    let mut changed =
        folder.is_some_and(|folder| crate::settings::remember_location(settings, &folder));
    let saved = app.saved_windows();
    if settings.get::<SavedWindows>(LAST_WINDOWS).as_ref() != Some(&saved)
        && let Ok(value) = toml::Value::try_from(&saved)
    {
        settings.set(LAST_WINDOWS, value);
        changed = true;
    }
    // Kept before there were windows: what replaced it is written.
    if settings
        .get::<toml::Value>(LAST_TABS)
        .is_some_and(|v| v.as_table().is_none_or(|t| !t.is_empty()))
    {
        settings.unset(LAST_TABS);
        changed = true;
    }
    changed
}

/// Record where the windows are (`record`) and save it on the calling
/// thread: for the moment the application closes, not for a UI callback.
pub fn remember(app: &App) {
    if record(app)
        && let Err(error) = app.settings.save()
    {
        tracing::warn!("cannot remember the windows: {error:#}");
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct NewTabArgs {
    /// What the tab shows: a path or `scheme:rest`; what `pane` shows when
    /// omitted.
    #[serde(default)]
    pub location: Option<PathArg>,
    /// The pane the tab opens from: its location and sort. The active pane
    /// when omitted.
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// Show the tab at once, last (the default); `false` opens it behind
    /// the tab shown, beside it.
    #[serde(default = "shown_by_default")]
    pub show: bool,
    /// Show `location` as it was kept (ADR-109: a saved place, a saved
    /// search), filtered by `filter` alone, not with the pane's lens.
    #[serde(default)]
    pub as_kept: bool,
    /// With `as_kept`: what filters it, as the `filter` command's `text`.
    #[serde(default)]
    pub filter: Option<String>,
}

fn shown_by_default() -> bool {
    true
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct OpenTabsArgs {
    /// The entries to open, folders or places; the pane's selection when
    /// empty.
    #[serde(default)]
    pub ids: Vec<EntryId>,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct TabArg {
    /// The tab; the one shown when omitted.
    #[serde(default)]
    pub tab: Option<TabId>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SelectTabArgs {
    #[serde(default)]
    pub tab: Option<TabId>,
    /// Position from the left, from 0; negative counts from the right (-1
    /// is the last tab).
    #[serde(default)]
    pub index: Option<i64>,
    /// Steps from the shown tab, wrapping around.
    #[serde(default)]
    pub delta: Option<i64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct MoveTabArgs {
    /// The tab; the one shown when omitted.
    #[serde(default)]
    pub tab: Option<TabId>,
    /// The position to put it at, from 0 (in `window`, last there when
    /// omitted).
    #[serde(default)]
    pub to: Option<usize>,
    /// Steps to move it by, instead of `to`.
    #[serde(default)]
    pub delta: Option<i64>,
    /// Another window to move it to, shown there.
    #[serde(default)]
    pub window: Option<WindowId>,
    /// Move it to a new window of its own.
    #[serde(default)]
    pub new_window: bool,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct NewWindowArgs {
    /// What the window shows: a path or `scheme:rest`; what `pane` shows
    /// when omitted.
    #[serde(default)]
    pub location: Option<PathArg>,
    /// The pane it opens from: its location, sort and lens, and the shape
    /// of its window. The active pane when omitted.
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WindowArg {
    /// The window; the active one when omitted.
    #[serde(default)]
    pub window: Option<WindowId>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ReopenTabArgs {
    /// The closed tab, as its close reported it; the one closed last when
    /// omitted.
    #[serde(default)]
    pub closed: Option<u64>,
}

/// Position `index` (negative from the end) or `delta` steps from `from`,
/// among `len` tabs.
fn tab_position(len: usize, from: usize, index: Option<i64>, delta: Option<i64>) -> usize {
    let len_i = i64::try_from(len).unwrap_or(i64::MAX).max(1);
    let wanted = match (index, delta) {
        (Some(i), _) if i < 0 => (len_i + i).max(0),
        (Some(i), _) => i.min(len_i - 1),
        (None, Some(d)) => (i64::try_from(from).unwrap_or(0) + d).rem_euclid(len_i),
        (None, None) => i64::try_from(from).unwrap_or(0),
    };
    usize::try_from(wanted).unwrap_or(0)
}

pub(crate) fn register(app: &App) {
    let c = &app.commands;
    app.settings.declare(SettingSpec::state(
        LAST_WINDOWS,
        "The windows shown last and their tabs",
        toml::Value::Table(toml::map::Map::new()),
    ));
    app.settings.declare(SettingSpec::state(
        LAST_TABS,
        "The tabs shown when the window was last closed, before there were windows",
        toml::Value::Table(toml::map::Map::new()),
    ));

    app.settings.declare(
        SettingSpec::toggle(
            SLEEP_KEY,
            "Free the memory of hidden tabs",
            "A tab you are not looking at lets go of its list of files and lists it again when shown. Turn this off to keep every tab listed and up to date, which uses more memory and background work.",
            true,
        )
        .section("Tabs"),
    );

    c.register::<NewTabArgs, _>(
        "core", Topic::Tabs,
        "tab.new",
        "New tab",
        "Open a tab showing a location, or what the pane shows; shown at once unless `show` is false",
        |a, app| {
            let location = a.location.map(|l| Location::parse_os(l.as_os_str()));
            let (tab, pane) = match (a.as_kept, location) {
                (true, Some(location)) => {
                    app.new_tab_as_kept(a.pane, location, a.filter.as_deref(), a.show)?
                }
                (_, location) => app.new_tab(a.pane, location, a.show)?,
            };
            Ok(Outcome::Value(serde_json::json!({ "tab": tab, "pane": pane })))
        },
    )
        .shows();

    c.register::<OpenTabsArgs, _>(
        "core", Topic::Tabs,
        "tab.open",
        "Open in new tab",
        "Open each selected folder in a tab of its own, behind the tab shown; files do not open in tabs",
        |a, app| {
            let pane = app.pane_or_active(a.pane);
            let folders = app.selected_folders(pane, a.ids)?;
            let mut tabs = Vec::new();
            for location in folders {
                tabs.push(app.new_tab(Some(pane), Some(location), false)?.0);
            }
            Ok(Outcome::Value(serde_json::json!({ "tabs": tabs })))
        },
    )
    .shows();

    c.register::<OpenTabsArgs, _>(
        "core",
        Topic::Tabs,
        "window.open",
        "Open in new window",
        "Open each selected folder in a window of its own, shaped as the pane's; files do not open in windows",
        |a, app| {
            let pane = app.pane_or_active(a.pane);
            let folders = app.selected_folders(pane, a.ids)?;
            let mut windows = Vec::new();
            for location in folders {
                windows.push(app.new_window(Some(pane), Some(location))?.0);
            }
            Ok(Outcome::Value(serde_json::json!({ "windows": windows })))
        },
    )
    .shows();

    c.register::<TabArg, _>(
        "core",
        Topic::Tabs,
        "tab.close",
        "Close tab",
        "Close a tab (the one shown by default); the notification offers Undo. A window's last tab closes the window while another is open; the last tab of the last window stays (its window's own Ctrl+W closes the window)",
        |a, app| {
            let tab = a
                .tab
                .unwrap_or_else(|| app.workspace.read().active_tab().id);
            if let Some((serial, title)) = app.close_tab(tab)? {
                app.notify_with_action(
                    NotifyLevel::Info,
                    format!("Closed tab “{title}”"),
                    "Undo",
                    Invocation::with_args("tab.reopen", &serde_json::json!({ "closed": serial })),
                );
            } else {
                // Its window's last: the window goes, as in browsers;
                // Reopen closed tab brings it back whole. (The lock is let
                // go first: closing takes it.)
                let window = app.workspace.read().window_of_tab(tab);
                if let Some(window) = window {
                    app.close_window(window)?;
                }
            }
            Ok(Outcome::Done)
        },
    )
    .shows();

    c.register::<ReopenTabArgs, _>(
        "core",
        Topic::Tabs,
        "tab.reopen",
        "Reopen closed tab",
        "Open again the tab or window closed last, or the one a close reported, with its history",
        |a, app| match app.reopen_tab(a.closed)? {
            Some(Reopened::Tab(tab)) => Ok(Outcome::Value(serde_json::json!({ "tab": tab }))),
            Some(Reopened::Window(window)) => {
                Ok(Outcome::Value(serde_json::json!({ "window": window })))
            }
            None => Ok(Outcome::Done),
        },
    )
    .shows();

    let select = c.register::<SelectTabArgs, _>(
        "core",
        Topic::Tabs,
        "tab.select",
        "Show tab",
        "Show a tab: by id, or among the active window's tabs by position (negative from the right) or by steps from the one shown",
        |a, app| {
            let tab = if let Some(tab) = a.tab {
                tab
            } else {
                let ws = app.workspace.read();
                let from = ws.tab_index(ws.active_tab().id).unwrap_or(0);
                let at = tab_position(ws.tabs().len(), from, a.index, a.delta);
                ws.tabs()[at].id
            };
            app.select_tab(tab)?;
            Ok(Outcome::Done)
        },
    );
    select.forms(
        [
            (serde_json::json!({ "delta": 1 }), "Next tab".to_owned()),
            (
                serde_json::json!({ "delta": -1 }),
                "Previous tab".to_owned(),
            ),
        ]
        .into_iter()
        .chain((0..9).map(|index| {
            (
                serde_json::json!({ "index": index }),
                format!("Tab {}", index + 1),
            )
        }))
        .chain([(serde_json::json!({ "index": -1 }), "Last tab".to_owned())])
        .map(|(args, title)| {
            // A tab by its number, the last included, is one row of keys.
            let form = Form::new(args, title);
            if form.args.get("index").is_some() {
                form.in_family("Go to a tab")
            } else {
                form
            }
        }),
    );
    // A key held down goes on from tab to tab, as browsers do.
    select.repeats(|args| args.get("delta").is_some());
    select.shows();

    let moving = c.register::<MoveTabArgs, _>(
        "core",
        Topic::Tabs,
        "tab.move",
        "Move tab",
        "Put a tab at another position, or move it by steps; or move it to another window, or a new one",
        |a, app| {
            if a.window.is_some() || a.new_window {
                let tab = a
                    .tab
                    .unwrap_or_else(|| app.workspace.read().active_tab().id);
                let window = app.move_tab_to_window(tab, a.window, a.to)?;
                return Ok(Outcome::Value(serde_json::json!({ "window": window })));
            }
            let mut ws = app.workspace.write();
            let tab = a.tab.unwrap_or_else(|| ws.active_tab().id);
            let from = ws
                .tab_index(tab)
                .ok_or_else(|| anyhow::anyhow!("no such tab"))?;
            let len = ws.tabs().len();
            let to = match (a.to, a.delta) {
                (Some(to), _) => to,
                (None, Some(delta)) => {
                    // Moving by steps stops at the ends rather than wrapping.
                    let to = i64::try_from(from).unwrap_or(0) + delta;
                    usize::try_from(to.clamp(0, i64::try_from(len).unwrap_or(1) - 1)).unwrap_or(0)
                }
                (None, None) => from,
            };
            ws.move_tab(tab, to);
            drop(ws);
            app.wake();
            Ok(Outcome::Done)
        },
    );
    moving.forms([
        Form::new(serde_json::json!({ "delta": -1 }), "Move the tab left")
            .in_family("Move the tab"),
        Form::new(serde_json::json!({ "delta": 1 }), "Move the tab right")
            .in_family("Move the tab"),
        Form::new(
            serde_json::json!({ "new_window": true }),
            "Move the tab to a new window",
        ),
    ]);
    moving.repeats(|args| args.get("delta").is_some());
    moving.shows();

    c.register::<crate::command::NoArgs, _>(
        "core", Topic::Tabs,
        "tab.list",
        "List tabs",
        "The tabs in order, window after window: each one's window, panes, their locations, which pane is active and whether its window shows it",
        |_, app| Ok(Outcome::Value(serde_json::to_value(app.tab_list())?)),
    )
        .data();

    c.register::<NewWindowArgs, _>(
        "core",
        Topic::Tabs,
        "window.new",
        "New window",
        "Open a window showing a location, or what the pane shows, shaped as the pane's window; it becomes the active window",
        |a, app| {
            let location = a.location.map(|l| Location::parse_os(l.as_os_str()));
            let (window, tab, pane) = app.new_window(a.pane, location)?;
            Ok(Outcome::Value(
                serde_json::json!({ "window": window, "tab": tab, "pane": pane }),
            ))
        },
    )
    .shows();

    c.register::<WindowArg, _>(
        "core",
        Topic::Tabs,
        "window.close",
        "Close window",
        "Close a window (the active one by default) and its tabs, which Reopen closed tab brings back; the last window closes only by its own close button or Quit",
        |a, app| {
            let window = a
                .window
                .unwrap_or_else(|| app.workspace.read().active_window_id());
            match app.close_window(window)? {
                Some(serial) => Ok(Outcome::Value(serde_json::json!({ "closed": serial }))),
                None => anyhow::bail!("The last window closes with its close button or Quit"),
            }
        },
    )
    .shows();

    c.register::<WindowArg, _>(
        "core",
        Topic::Tabs,
        "window.select",
        "Go to window",
        "Make a window the active one, brought forward",
        |a, app| {
            let window = a
                .window
                .ok_or_else(|| anyhow::anyhow!("which window: `window` is needed"))?;
            anyhow::ensure!(
                app.workspace.write().set_active_window(window),
                "no such window"
            );
            app.wake();
            Ok(Outcome::Done)
        },
    )
    .shows();

    c.register::<crate::command::NoArgs, _>(
        "core",
        Topic::Tabs,
        "window.list",
        "List windows",
        "The windows: each one's tabs, the tab it shows, and which is active",
        |_, app| Ok(Outcome::Value(serde_json::to_value(app.window_list())?)),
    )
    .data();
}

#[cfg(test)]
mod tests {
    use super::tab_position;

    #[test]
    fn positions_count_from_either_end_and_steps_wrap() {
        assert_eq!(tab_position(4, 1, Some(0), None), 0);
        assert_eq!(tab_position(4, 1, Some(9), None), 3);
        assert_eq!(tab_position(4, 1, Some(-1), None), 3);
        assert_eq!(tab_position(4, 1, Some(-9), None), 0);
        assert_eq!(tab_position(4, 3, None, Some(1)), 0);
        assert_eq!(tab_position(4, 0, None, Some(-1)), 3);
        assert_eq!(tab_position(1, 0, None, Some(1)), 0);
    }
}
