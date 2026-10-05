//! Tabs (ADR-018): arrangements of panes, one shown at a time.
//!
//! The workspace groups its panes in tabs; this module is what changes them
//! with the panes' lifecycle: a tab not shown is dormant (its panes let go of
//! their listings and watchers, `Pane::suspend`), a tab shown opens its
//! panes' locations again through the one navigation path (`App::show`). It
//! also keeps the tabs closed last, so a close can be undone, and what the
//! window's tabs were when it closed, for the next start.

use std::collections::VecDeque;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::app::{App, NotifyLevel};
use crate::command::{Invocation, Outcome};
use crate::entry::EntryId;
use crate::location::{Location, SortSpec};
use crate::os_path::PathArg;
use crate::settings::{STARTUP, SettingSpec};
use crate::workspace::{PaneId, Selection, TabId};

/// Application data: the tabs the window showed when it closed.
pub const LAST_TABS: &str = "core.last_tabs";

/// Whether tabs not shown let go of their listings (the default), or all
/// stay listed and current.
pub const SLEEP_KEY: &str = "core.sleep_hidden_tabs";

/// How many closed tabs can be reopened.
const CLOSED_KEPT: usize = 16;

/// A closed tab, as it can come back: each pane's history, sort and
/// selection, in the tab's order.
#[derive(Clone, Debug)]
struct ClosedTab {
    serial: u64,
    index: usize,
    panes: Vec<ClosedPane>,
    active: usize,
}

#[derive(Clone, Debug)]
struct ClosedPane {
    history: Vec<Location>,
    history_pos: usize,
    sort: SortSpec,
    selection: Selection,
}

/// The tabs closed last, newest last.
#[derive(Default)]
pub struct Closed {
    tabs: VecDeque<ClosedTab>,
    next: u64,
}

/// The window's tabs as the settings keep them: the location each pane
/// showed, not their histories.
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
}

/// A tab as `tab.list` reports it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TabInfo {
    pub id: TabId,
    pub title: String,
    pub panes: Vec<PaneInfo>,
    pub active: PaneId,
    pub shown: bool,
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
            if ws.active_tab().id != tab {
                let left = ws.active_tab().panes.clone();
                anyhow::ensure!(ws.set_active_tab(tab), "no such tab");
                // Without sleep every tab stays listed: nothing to let go of.
                if sleep && ws.holding {
                    ws.leaving.extend(left);
                } else if sleep {
                    suspend(&mut ws, left);
                }
            }
        }
        self.resume_shown()
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
        let sleep = self.tabs_sleep();
        let mut ws = self.workspace.write();
        let shown = ws.active_tab().panes.clone();
        let leaving = std::mem::take(&mut ws.leaving);
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
            let keep: Vec<PaneId> = ws
                .active_tab()
                .panes
                .iter()
                .chain(ws.leaving.iter())
                .copied()
                .collect();
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

    /// Open the locations of the shown tab's dormant panes: at startup, when
    /// a tab is shown, after the shown tab closed.
    pub fn resume_shown(&self) -> anyhow::Result<()> {
        let panes = self.workspace.read().active_tab().panes.clone();
        let mut failed = None;
        for pane in panes {
            if let Err(error) = self.resume(pane) {
                failed.get_or_insert(error);
            }
        }
        self.wake();
        failed.map_or(Ok(()), Err)
    }

    fn resume(&self, pane: PaneId) -> anyhow::Result<()> {
        let (location, restore) = {
            let mut ws = self.workspace.write();
            let Some(p) = ws.pane_mut(pane) else {
                return Ok(());
            };
            if !p.is_dormant() {
                return Ok(());
            }
            (p.history[p.history_pos].clone(), p.take_restore_selection())
        };
        let shown = self.show(pane, &location, {
            let restore = restore.clone();
            move |p| p.set_restore(restore)
        });
        if shown.is_err()
            && let Some(p) = self.workspace.write().pane_mut(pane)
        {
            p.set_restore(restore);
        }
        shown
    }

    /// A new tab showing `location`, or what `from` (the active pane when
    /// `None`) shows, with its sort. Shown at once and put last, or opened
    /// behind the shown tab, beside it.
    pub fn new_tab(
        &self,
        from: Option<PaneId>,
        location: Option<Location>,
        show: bool,
    ) -> anyhow::Result<(TabId, PaneId)> {
        let (tab, pane) = {
            let mut ws = self.workspace.write();
            let from = from.unwrap_or_else(|| ws.active_id());
            let source = ws.pane(from);
            let sort = source.map(|p| p.sort).unwrap_or_default();
            let location = match location {
                Some(location) => location,
                None => source
                    .and_then(|p| p.shown())
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("the pane shows nothing to open in a tab"))?,
            };
            let at = if show {
                ws.tabs().len()
            } else {
                ws.beside_position()
            };
            let (tab, panes) = ws.add_dormant_tab(at, vec![(location, sort)]);
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

    /// Close tab `tab`; the last tab stays. Returns the serial that reopens
    /// it and its title, or `None` when it was the last tab.
    pub fn close_tab(&self, tab: TabId) -> anyhow::Result<Option<(u64, String)>> {
        let (index, closed, panes, was_shown) = {
            let mut ws = self.workspace.write();
            anyhow::ensure!(ws.tab(tab).is_some(), "no such tab");
            let was_shown = ws.active_tab().id == tab;
            let Some((index, closed, panes)) = ws.close_tab(tab) else {
                return Ok(None);
            };
            (index, closed, panes, was_shown)
        };
        let active = closed
            .panes
            .iter()
            .position(|p| *p == closed.active)
            .unwrap_or(0);
        let record: Vec<ClosedPane> = panes
            .iter()
            .filter(|p| !p.history.is_empty())
            .map(|p| ClosedPane {
                history: p.history.clone(),
                history_pos: p.history_pos,
                sort: p.sort,
                selection: p.kept_selection().clone(),
            })
            .collect();
        let title = panes
            .get(active)
            .or_else(|| panes.first())
            .and_then(|p| p.shown())
            .map(|location| self.location_title(location))
            .unwrap_or_default();
        // Dropping the panes closes their locations: outside the lock.
        drop(panes);
        let serial = {
            let mut closed = self.closed_tabs.lock();
            closed.next += 1;
            let serial = closed.next;
            if !record.is_empty() {
                closed.tabs.push_back(ClosedTab {
                    serial,
                    index,
                    active: active.min(record.len() - 1),
                    panes: record,
                });
                while closed.tabs.len() > CLOSED_KEPT {
                    closed.tabs.pop_front();
                }
            }
            serial
        };
        if was_shown {
            self.resume_shown()?;
        } else {
            self.wake();
        }
        Ok(Some((serial, title)))
    }

    /// Open again the tab closed with `serial`, or the one closed last,
    /// where it was, and show it. `None` when there is none to reopen.
    pub fn reopen_tab(&self, serial: Option<u64>) -> anyhow::Result<Option<TabId>> {
        let closed = {
            let mut closed = self.closed_tabs.lock();
            let at = match serial {
                Some(serial) => closed.tabs.iter().position(|t| t.serial == serial),
                None => closed.tabs.len().checked_sub(1),
            };
            at.and_then(|at| closed.tabs.remove(at))
        };
        let Some(closed) = closed else {
            return Ok(None);
        };
        let tab = {
            let mut ws = self.workspace.write();
            let (tab, ids) = ws.add_dormant_tab(
                closed.index,
                closed
                    .panes
                    .iter()
                    .map(|p| (p.history[p.history_pos].clone(), p.sort))
                    .collect(),
            );
            for (id, record) in ids.iter().zip(closed.panes) {
                if let Some(p) = ws.pane_mut(*id) {
                    p.history = record.history;
                    p.history_pos = record.history_pos;
                    p.set_restore(Some(record.selection));
                }
            }
            if let (Some(t), Some(active)) = (ws.tab_mut(tab), ids.get(closed.active)) {
                t.active = *active;
            }
            tab
        };
        self.select_tab(tab)?;
        Ok(Some(tab))
    }

    /// Every tab, in order, for scripts and agents.
    pub fn tab_list(&self) -> Vec<TabInfo> {
        let rows: Vec<_> = {
            let ws = self.workspace.read();
            let shown = ws.active_tab().id;
            ws.tabs()
                .iter()
                .map(|tab| {
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
                    (tab.id, panes, tab.active, tab.id == shown, location)
                })
                .collect()
        };
        // Titles read the places registry: not under the workspace lock.
        rows.into_iter()
            .map(|(id, panes, active, shown, location)| TabInfo {
                id,
                title: location
                    .map(|l| self.location_title(&l))
                    .unwrap_or_default(),
                panes,
                active,
                shown,
            })
            .collect()
    }

    /// Put the tabs of `saved` in place of the workspace's one untouched
    /// tab, dormant: `resume_shown` opens the shown one once the UI is there
    /// to be woken. Returns false when there was nothing to restore.
    pub fn restore_tabs(&self, saved: &SavedTabs) -> bool {
        let mut ws = self.workspace.write();
        let Some(first) = ws.tabs().first().map(|t| t.id) else {
            return false;
        };
        let mut shown = None;
        let mut restored = 0;
        for (i, saved_tab) in saved.tabs.iter().enumerate() {
            if saved_tab.panes.is_empty() {
                continue;
            }
            let at = ws.tabs().len();
            let (tab, ids) = ws.add_dormant_tab(
                at,
                saved_tab
                    .panes
                    .iter()
                    .take(2)
                    .map(|l| (l.clone(), SortSpec::default()))
                    .collect(),
            );
            if let (Some(t), Some(active)) = (ws.tab_mut(tab), ids.get(saved_tab.active)) {
                t.active = *active;
            }
            if i == saved.shown || shown.is_none() {
                shown = Some(tab);
            }
            restored += 1;
        }
        let Some(shown) = shown else {
            return false;
        };
        ws.set_active_tab(shown);
        ws.close_tab(first);
        drop(ws);
        tracing::debug!(tabs = restored, "restored the last window's tabs");
        true
    }

    /// The window's tabs, as `LAST_TABS` keeps them.
    pub fn saved_tabs(&self) -> SavedTabs {
        let ws = self.workspace.read();
        let mut saved = SavedTabs::default();
        let shown = ws.active_tab().id;
        for tab in ws.tabs() {
            let mut panes = Vec::new();
            let mut active = 0;
            for id in &tab.panes {
                let Some(location) = ws.pane(*id).and_then(|p| p.shown()) else {
                    continue;
                };
                if *id == tab.active {
                    active = panes.len();
                }
                panes.push(location.clone());
            }
            if panes.is_empty() {
                continue;
            }
            if tab.id == shown {
                saved.shown = saved.tabs.len();
            }
            saved.tabs.push(SavedTab { panes, active });
        }
        saved
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

/// The tabs to open at startup: the last window's, when the startup
/// preference is to come back to it and it had any.
pub fn last_tabs(settings: &crate::settings::Settings) -> Option<SavedTabs> {
    if settings.get::<String>(STARTUP).as_deref() != Some("last") {
        return None;
    }
    settings
        .get::<SavedTabs>(LAST_TABS)
        .filter(|saved| !saved.tabs.is_empty())
}

/// Record where the window is, for the next start: the active pane's
/// folder and the tabs. Writes the settings file on the calling thread:
/// for the moment the application closes, not for a UI callback.
pub fn remember(app: &App) {
    let settings = &app.settings;
    if settings.get::<String>(STARTUP).as_deref() != Some("last") {
        return;
    }
    let folder = app
        .workspace
        .read()
        .active()
        .location()
        .and_then(Location::path);
    let mut changed =
        folder.is_some_and(|folder| crate::settings::remember_location(settings, &folder));
    let saved = app.saved_tabs();
    if settings.get::<SavedTabs>(LAST_TABS).as_ref() != Some(&saved)
        && let Ok(value) = toml::Value::try_from(&saved)
    {
        settings.set(LAST_TABS, value);
        changed = true;
    }
    if changed && let Err(error) = settings.save() {
        tracing::warn!("cannot remember the window's tabs: {error:#}");
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
    /// The position to put it at, from 0.
    #[serde(default)]
    pub to: Option<usize>,
    /// Steps to move it by, instead of `to`.
    #[serde(default)]
    pub delta: Option<i64>,
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
    app.settings.declare(SettingSpec::data(
        LAST_TABS,
        "The tabs shown when the window was last closed",
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
        "core",
        "tab.new",
        "New tab",
        "Open a tab showing a location, or what the pane shows; shown at once unless `show` is false",
        |a, app| {
            let location = a.location.map(|l| Location::parse_os(l.as_os_str()));
            let (tab, pane) = app.new_tab(a.pane, location, a.show)?;
            Ok(Outcome::Value(serde_json::json!({ "tab": tab, "pane": pane })))
        },
    );

    c.register::<OpenTabsArgs, _>(
        "core",
        "tab.open",
        "Open in new tab",
        "Open each selected folder in a tab of its own, behind the tab shown",
        |a, app| {
            let pane = app.pane_or_active(a.pane);
            let folders: Vec<Location> = {
                let ws = app.workspace.read();
                let Some(p) = ws.pane(pane) else {
                    anyhow::bail!("no such pane");
                };
                // In the order the pane shows them, so the tabs line up
                // the same.
                let ids: Vec<EntryId> = if a.ids.is_empty() {
                    p.order()
                        .iter()
                        .copied()
                        .filter(|id| p.selection.contains(*id))
                        .collect()
                } else {
                    a.ids
                };
                // A place (a tag's), or a folder that does not open at its
                // path (a trashed one), opens where it leads.
                ids.into_iter().filter_map(|id| p.leads_to(id)).collect()
            };
            if folders.is_empty() {
                return Err(crate::command::NothingSelected.into());
            }
            let mut tabs = Vec::new();
            for location in folders {
                tabs.push(app.new_tab(Some(pane), Some(location), false)?.0);
            }
            Ok(Outcome::Value(serde_json::json!({ "tabs": tabs })))
        },
    );

    c.register::<TabArg, _>(
        "core",
        "tab.close",
        "Close tab",
        "Close a tab (the one shown by default); the last tab stays. The notification offers Undo",
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
            }
            Ok(Outcome::Done)
        },
    );

    c.register::<ReopenTabArgs, _>(
        "core",
        "tab.reopen",
        "Reopen closed tab",
        "Open again the tab closed last, or the one a close reported, with its history",
        |a, app| match app.reopen_tab(a.closed)? {
            Some(tab) => Ok(Outcome::Value(serde_json::json!({ "tab": tab }))),
            None => Ok(Outcome::Done),
        },
    );

    c.register::<SelectTabArgs, _>(
        "core",
        "tab.select",
        "Show tab",
        "Show a tab: by id, by position (negative from the right) or by steps from the one shown",
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

    c.register::<MoveTabArgs, _>(
        "core",
        "tab.move",
        "Move tab",
        "Put a tab at another position, or move it by steps",
        |a, app| {
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

    c.register::<crate::command::NoArgs, _>(
        "core",
        "tab.list",
        "List tabs",
        "The tabs in order: each one's panes, their locations, which pane is active and which tab is shown",
        |_, app| Ok(Outcome::Value(serde_json::to_value(app.tab_list())?)),
    );
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
