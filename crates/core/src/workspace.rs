//! Panes, selection, and focus: the state every view and command reads.

use std::ffi::OsString;
use std::sync::Arc;

use indexmap::IndexSet;

use crate::cancel::CancelToken;
use crate::entry::EntryId;
use crate::location::{Listing, Location, OpenLocation, SortSpec};

/// Identifier of a pane within the workspace.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
pub struct PaneId(pub u32);

/// Identifier of a tab within the workspace.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
pub struct TabId(pub u32);

/// Panes shown together (ADR-018): one, or two when split, in the order
/// they are laid out. The tabs not shown are dormant (`Pane::suspend`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub panes: Vec<PaneId>,
    /// The pane commands act on while this tab is shown.
    pub active: PaneId,
}

/// Multi-selection with an anchor (for range selection) and a cursor (keyboard focus).
#[derive(Clone, Debug, Default)]
pub struct Selection {
    items: IndexSet<EntryId>,
    pub anchor: Option<EntryId>,
    pub cursor: Option<EntryId>,
    /// Stamped from one process-wide counter whenever the items change, so
    /// two selections never share a nonzero stamp; 0 is an untouched one.
    revision: u64,
}

static SELECTION_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Selection {
    /// Changes whenever the selected items may have changed: a summary of
    /// the selection (counts, sizes) keyed on it with the listing's version
    /// is recomputed only when needed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn touch(&mut self) {
        self.revision = SELECTION_REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Deselect everything. The cursor stays where it was, so the keyboard
    /// carries on from there; a new location resets the whole selection.
    pub fn clear(&mut self) {
        self.touch();
        self.items.clear();
        self.anchor = None;
    }

    pub fn set(&mut self, ids: impl IntoIterator<Item = EntryId>) {
        self.touch();
        self.items = ids.into_iter().collect();
        self.anchor = self.items.first().copied();
        self.cursor = self.items.last().copied();
    }

    pub fn add(&mut self, id: EntryId) {
        self.touch();
        self.items.insert(id);
        self.cursor = Some(id);
        self.anchor.get_or_insert(id);
    }

    pub fn toggle(&mut self, id: EntryId) {
        self.touch();
        if !self.items.shift_remove(&id) {
            self.items.insert(id);
            self.anchor.get_or_insert(id);
        }
        self.cursor = Some(id);
    }

    pub fn remove(&mut self, id: EntryId) {
        self.touch();
        self.items.shift_remove(&id);
        if self.anchor == Some(id) {
            self.anchor = self.items.first().copied();
        }
        if self.cursor == Some(id) {
            self.cursor = self.items.last().copied();
        }
    }

    pub fn contains(&self, id: EntryId) -> bool {
        self.items.contains(&id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = EntryId> + '_ {
        self.items.iter().copied()
    }

    /// Drop ids that no longer exist in the listing.
    pub fn retain_in(&mut self, listing: &Listing) {
        let before = self.items.len();
        self.items.retain(|id| listing.get(*id).is_some());
        if self.items.len() != before {
            self.touch();
        }
        if self.anchor.is_some_and(|a| listing.get(a).is_none()) {
            self.anchor = self.items.first().copied();
        }
        if self.cursor.is_some_and(|c| listing.get(c).is_none()) {
            self.cursor = self.items.last().copied();
        }
    }
}

/// A location a pane keeps open beside the one it shows, with its listing
/// kept current: what its view shows next to it (the columns of the folders
/// above), and what going there again adopts instead of listing it anew.
pub struct Kept {
    pub location: Location,
    /// Closed when the kept location goes, unless it was taken to be shown.
    open: Option<Arc<dyn OpenLocation>>,
    pub listing: Listing,
}

impl Kept {
    fn new(open: Arc<dyn OpenLocation>, listing: Listing) -> Self {
        Kept {
            location: open.location().clone(),
            open: Some(open),
            listing,
        }
    }

    /// The location and its listing, to be shown: they go on as they were.
    fn into_shown(mut self) -> (Arc<dyn OpenLocation>, Listing) {
        let open = self.open.take().expect("a kept location is open");
        (open, std::mem::take(&mut self.listing))
    }

    /// Apply its pending events, like the shown listing's.
    fn pump(&mut self, budget: std::time::Duration) -> usize {
        let Some(open) = &self.open else { return 0 };
        self.listing.drain(open.events(), budget)
    }

    fn backlog(&self) -> bool {
        self.open
            .as_ref()
            .is_some_and(|open| !open.events().is_empty())
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        if let Some(open) = self.open.take() {
            open.close();
        }
    }
}

/// What a suspended pane let go of; closed when dropped.
pub(crate) struct Released {
    open: Arc<dyn OpenLocation>,
    _kept: Vec<Kept>,
}

impl Drop for Released {
    fn drop(&mut self) {
        self.open.close();
    }
}

/// One pane: an open location, its listing, selection, and history.
pub struct Pane {
    pub id: PaneId,
    pub open: Option<Arc<dyn OpenLocation>>,
    location_lifetime: CancelToken,
    pub listing: Listing,
    /// The locations the pane's view asked to keep (`App::keep_locations`).
    kept: Vec<Kept>,
    /// The location navigation just left, until the view says whether it
    /// keeps it: entering a folder makes the one left a column beside it.
    left: Option<Kept>,
    pub selection: Selection,
    /// Sort and hidden-file settings; views render `listing.sorted(sort)`.
    pub sort: SortSpec,
    pub history: Vec<Location>,
    /// Index into `history` of the current location.
    pub history_pos: usize,
    /// A name to select once the listing has it; see [`Pane::reveal`].
    reveal: Option<OsString>,
    /// The selection a dormant pane had, given back once its listing is
    /// complete again (see [`Pane::suspend`]).
    restore: Option<Selection>,
}

impl Pane {
    fn new(id: PaneId) -> Self {
        Pane {
            id,
            open: None,
            location_lifetime: CancelToken::new(),
            listing: Listing::default(),
            kept: Vec::new(),
            left: None,
            selection: Selection::default(),
            sort: SortSpec::default(),
            history: Vec::new(),
            history_pos: 0,
            reveal: None,
            restore: None,
        }
    }

    /// A pane that will show `location` when its tab is shown: dormant
    /// until then, with a history of that one location.
    fn dormant(id: PaneId, location: Location, sort: SortSpec) -> Self {
        let mut pane = Pane::new(id);
        pane.history.push(location);
        pane.sort = sort;
        pane
    }

    /// Work tied to this location stops immediately on navigation or pane close.
    pub fn location_lifetime(&self) -> CancelToken {
        self.location_lifetime.child()
    }

    /// Show `open` with `listing` (a new one, or a kept one going on as it
    /// was). The location shown before waits in `left` for the view to keep
    /// it or let it go (`App::keep_locations`); whatever waited there goes.
    pub(crate) fn replace_open(&mut self, open: Arc<dyn OpenLocation>, listing: Listing) {
        self.location_lifetime.cancel();
        self.location_lifetime = CancelToken::new();
        let old_listing = std::mem::replace(&mut self.listing, listing);
        self.left = self
            .open
            .replace(open)
            .map(|old| Kept::new(old, old_listing));
        // Nothing of the old location survives, the cursor included.
        self.selection = Selection::default();
        self.reveal = None;
        self.restore = None;
    }

    /// Let go of what the pane shows while its tab is not shown (ADR-018):
    /// the open location, its listing and watcher, the locations kept
    /// beside it, and the work tied to it. The history, the sort and the
    /// selection stay; the selection comes back when the pane is shown
    /// again and its listing is complete. What it let go of closes when
    /// the returned value drops: after the workspace lock, since closing
    /// runs the location's code.
    pub(crate) fn suspend(&mut self) -> Option<Released> {
        let open = self.open.take()?;
        self.location_lifetime.cancel();
        self.location_lifetime = CancelToken::new();
        let selection = std::mem::take(&mut self.selection);
        if !selection.is_empty() {
            self.restore = Some(selection);
        }
        self.listing = Listing::default();
        self.reveal = None;
        let mut kept = std::mem::take(&mut self.kept);
        kept.extend(self.left.take());
        Some(Released { open, _kept: kept })
    }

    /// The selection to give back once the listing is complete.
    pub(crate) fn set_restore(&mut self, selection: Option<Selection>) {
        self.restore = selection.filter(|s| !s.is_empty());
    }

    /// Take the selection a dormant pane keeps, to open it again.
    pub(crate) fn take_restore_selection(&mut self) -> Option<Selection> {
        self.restore.take()
    }

    /// Whether the pane waits for its tab to be shown to open its location.
    pub fn is_dormant(&self) -> bool {
        self.open.is_none() && !self.history.is_empty()
    }

    /// The location the pane shows, or will show once its tab is shown.
    pub fn shown(&self) -> Option<&Location> {
        self.location()
            .or_else(|| self.history.get(self.history_pos))
    }

    /// Whether a selection is waiting for the listing to be complete.
    pub fn restoring(&self) -> bool {
        self.restore.is_some()
    }

    /// The selection it has, or will have again once it is shown.
    pub fn kept_selection(&self) -> &Selection {
        match &self.restore {
            Some(restore) if self.selection.is_empty() => restore,
            _ => &self.selection,
        }
    }

    /// Give back the selection kept while dormant, once the listing is
    /// complete: before that, the entries it names may not have arrived.
    /// A selection made in the meantime wins; a listing that failed ends
    /// the wait.
    fn take_restore(&mut self) {
        if self.restore.is_none() {
            return;
        }
        if self.listing.error().is_some() {
            self.restore = None;
            return;
        }
        if !self.listing.is_complete() {
            return;
        }
        let Some(mut restore) = self.restore.take() else {
            return;
        };
        if self.selection.is_empty() {
            restore.retain_in(&self.listing);
            restore.touch();
            self.selection = restore;
        }
    }

    /// The kept location `location`, taken out to be shown.
    pub(crate) fn take_kept(
        &mut self,
        location: &Location,
    ) -> Option<(Arc<dyn OpenLocation>, Listing)> {
        if self.left.as_ref().is_some_and(|k| &k.location == location) {
            return self.left.take().map(Kept::into_shown);
        }
        let i = self.kept.iter().position(|k| &k.location == location)?;
        Some(self.kept.remove(i).into_shown())
    }

    /// Keep exactly the locations of `wanted` (the one shown aside): the
    /// one just left stays if it is wanted, the others go. Returns the
    /// wanted ones that are not open yet, for the caller to open.
    pub(crate) fn keep_only(&mut self, wanted: &[Location]) -> Vec<Location> {
        if let Some(left) = self.left.take()
            && wanted.contains(&left.location)
        {
            self.kept.push(left);
        }
        self.kept.retain(|k| wanted.contains(&k.location));
        let shown = self.location().cloned();
        wanted
            .iter()
            .filter(|w| Some(*w) != shown.as_ref() && !self.kept.iter().any(|k| &k.location == *w))
            .cloned()
            .collect()
    }

    pub(crate) fn add_kept(&mut self, open: Arc<dyn OpenLocation>) {
        self.kept.push(Kept::new(open, Listing::default()));
    }

    /// Whether anything is kept or waiting to be: `keep_only` has work.
    pub fn keeps_any(&self) -> bool {
        !self.kept.is_empty() || self.left.is_some()
    }

    /// The listing of a location kept beside the one shown.
    pub fn kept(&self, location: &Location) -> Option<&Listing> {
        self.kept
            .iter()
            .find(|k| &k.location == location)
            .map(|k| &k.listing)
    }

    /// Select the entry called `name` as soon as the listing has it: how a
    /// jump to a file, or a script, opens its folder with the file selected
    /// without waiting for the listing. Call it right after navigating;
    /// the next navigation forgets it, and so does a listing that
    /// completes without that name.
    pub fn reveal(&mut self, name: OsString) {
        self.reveal = Some(name);
        self.take_reveal();
    }

    fn take_reveal(&mut self) {
        let Some(name) = &self.reveal else { return };
        if let Some(id) = self.listing.find_by_name(name).map(|e| e.id) {
            self.selection.set([id]);
            self.reveal = None;
        } else if self.listing.is_complete() {
            self.reveal = None;
        }
    }

    /// Whether listing events are waiting to be applied, the kept
    /// locations' included.
    pub fn backlog(&self) -> bool {
        self.open
            .as_ref()
            .is_some_and(|open| !open.events().is_empty())
            || self.kept.iter().any(Kept::backlog)
    }

    pub fn location(&self) -> Option<&Location> {
        self.open.as_ref().map(|o| o.location())
    }

    /// Apply pending listing events, for at most `budget` each, the kept
    /// locations' too. Returns how many changed the shown listing;
    /// `backlog` says whether some are still waiting.
    pub fn pump(&mut self, budget: std::time::Duration) -> usize {
        for kept in &mut self.kept {
            kept.pump(budget);
        }
        let Some(open) = &self.open else { return 0 };
        let rx = open.events().clone();
        let n = self.listing.drain(&rx, budget);
        if n > 0 {
            self.selection.retain_in(&self.listing);
            self.take_reveal();
        }
        self.take_restore();
        n
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        self.location_lifetime.cancel();
        if let Some(open) = &self.open {
            open.close();
        }
    }
}

/// All panes, the tabs that group them, and which ones are active.
pub struct Workspace {
    panes: Vec<Pane>,
    /// Never empty; every pane is in exactly one tab.
    tabs: Vec<Tab>,
    active_tab: usize,
    next_id: u32,
    next_tab: u32,
    /// The tab the last tab opened beside the shown one went to, and the
    /// shown tab then: tabs opened one after another from the same tab
    /// line up after it in the order they were opened.
    opened_beside: Option<(TabId, TabId)>,
    /// Whether a UI keeps drawing the tab left until the tab shown is ready
    /// (`App::hold_tabs`): the panes left then stay open in `leaving` until
    /// `App::settle_tabs`.
    pub(crate) holding: bool,
    /// Panes of tabs no longer shown that are not suspended yet.
    pub(crate) leaving: Vec<PaneId>,
    /// Bumped whenever panes or tabs come, go, move or change which is
    /// active, so the UI compares one number to follow them.
    revision: u64,
}

impl Default for Workspace {
    fn default() -> Self {
        let mut w = Workspace {
            panes: Vec::new(),
            tabs: Vec::new(),
            active_tab: 0,
            next_id: 0,
            next_tab: 0,
            opened_beside: None,
            holding: false,
            leaving: Vec::new(),
            revision: 0,
        };
        w.add_tab(0);
        w
    }
}

impl Workspace {
    fn new_pane_id(&mut self) -> PaneId {
        let id = PaneId(self.next_id);
        self.next_id += 1;
        id
    }

    /// A new pane in the tab shown: the other side of a split.
    pub fn add_pane(&mut self) -> PaneId {
        let id = self.new_pane_id();
        self.panes.push(Pane::new(id));
        self.tabs[self.active_tab].panes.push(id);
        self.revision += 1;
        id
    }

    /// A new tab of one new pane, at position `at` (clamped), not shown.
    pub fn add_tab(&mut self, at: usize) -> (TabId, PaneId) {
        let pane = self.new_pane_id();
        self.panes.push(Pane::new(pane));
        (self.insert_tab(at, vec![pane]), pane)
    }

    fn insert_tab(&mut self, at: usize, panes: Vec<PaneId>) -> TabId {
        let id = TabId(self.next_tab);
        self.next_tab += 1;
        let at = at.min(self.tabs.len());
        let active = panes[0];
        self.tabs.insert(at, Tab { id, panes, active });
        if at <= self.active_tab && self.tabs.len() > 1 {
            self.active_tab += 1;
        }
        self.revision += 1;
        id
    }

    /// A tab of dormant panes, one per `(location, sort)`, at `at`, not
    /// shown: they open their locations when it is.
    pub(crate) fn add_dormant_tab(
        &mut self,
        at: usize,
        panes: Vec<(Location, SortSpec)>,
    ) -> (TabId, Vec<PaneId>) {
        let ids: Vec<PaneId> = panes
            .into_iter()
            .map(|(location, sort)| {
                let id = self.new_pane_id();
                self.panes.push(Pane::dormant(id, location, sort));
                id
            })
            .collect();
        (self.insert_tab(at, ids.clone()), ids)
    }

    /// Where a tab opened beside the shown one goes: right after it, or
    /// after the tabs already opened beside it.
    pub(crate) fn beside_position(&self) -> usize {
        let shown = self.tabs[self.active_tab].id;
        match self.opened_beside {
            Some((from, last)) if from == shown => {
                self.tab_index(last).map_or(self.active_tab + 1, |i| i + 1)
            }
            _ => self.active_tab + 1,
        }
    }

    pub(crate) fn opened_beside(&mut self, tab: TabId) {
        self.opened_beside = Some((self.tabs[self.active_tab].id, tab));
    }

    pub(crate) fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.revision += 1;
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn close_pane(&mut self, id: PaneId) -> bool {
        if self.panes.len() <= 1 {
            return false;
        }
        let Some(i) = self.panes.iter().position(|p| p.id == id) else {
            return false;
        };
        let t = self
            .tabs
            .iter()
            .position(|t| t.panes.contains(&id))
            .expect("every pane is in a tab");
        self.panes.remove(i);
        self.revision += 1;
        let tab = &mut self.tabs[t];
        let at = tab.panes.iter().position(|p| *p == id).expect("in its tab");
        tab.panes.remove(at);
        if tab.panes.is_empty() {
            self.remove_tab_at(t);
        } else if tab.active == id {
            // Closing the active pane activates the one that took its place.
            tab.active = tab.panes[at.min(tab.panes.len() - 1)];
        }
        true
    }

    fn remove_tab_at(&mut self, t: usize) -> Tab {
        self.revision += 1;
        let tab = self.tabs.remove(t);
        // The shown tab stays shown when one before it closes; closing the
        // shown tab shows the one that took its place.
        if t < self.active_tab {
            self.active_tab -= 1;
        }
        self.active_tab = self.active_tab.min(self.tabs.len().saturating_sub(1));
        tab
    }

    /// Take tab `id` and its panes out, in the tab's order, with the index
    /// it had. Refused for the last tab. Dropping the panes closes what
    /// they show: do it after releasing the workspace lock.
    pub(crate) fn close_tab(&mut self, id: TabId) -> Option<(usize, Tab, Vec<Pane>)> {
        if self.tabs.len() <= 1 {
            return None;
        }
        let t = self.tab_index(id)?;
        let tab = self.remove_tab_at(t);
        let mut panes = Vec::with_capacity(tab.panes.len());
        for pane in &tab.panes {
            if let Some(i) = self.panes.iter().position(|p| p.id == *pane) {
                panes.push(self.panes.remove(i));
            }
        }
        Some((t, tab, panes))
    }

    /// Put tab `id` at position `to` (clamped). The shown tab stays shown.
    pub fn move_tab(&mut self, id: TabId, to: usize) -> bool {
        let Some(from) = self.tab_index(id) else {
            return false;
        };
        let shown = self.tabs[self.active_tab].id;
        let tab = self.tabs.remove(from);
        let to = to.min(self.tabs.len());
        self.tabs.insert(to, tab);
        self.active_tab = self.tab_index(shown).expect("still there");
        self.revision += 1;
        true
    }

    /// Changes whenever panes or tabs come, go, move, or another one is
    /// made active.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// The tab shown.
    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    /// The tab `pane` belongs to.
    pub fn tab_of(&self, pane: PaneId) -> Option<TabId> {
        self.tabs
            .iter()
            .find(|t| t.panes.contains(&pane))
            .map(|t| t.id)
    }

    /// Show tab `id`. Only the order changes: `App::select_tab` suspends
    /// the panes of the tab left and opens those of the tab shown.
    pub(crate) fn set_active_tab(&mut self, id: TabId) -> bool {
        match self.tab_index(id) {
            Some(i) => {
                self.active_tab = i;
                self.revision += 1;
                // Tabs opened beside another line up after it only until
                // another tab is shown.
                self.opened_beside = None;
                true
            }
            None => false,
        }
    }

    pub fn panes(&self) -> &[Pane] {
        &self.panes
    }

    pub fn panes_mut(&mut self) -> &mut [Pane] {
        &mut self.panes
    }

    pub fn active(&self) -> &Pane {
        let id = self.active_id();
        self.pane(id).expect("the active pane exists")
    }

    pub fn active_mut(&mut self) -> &mut Pane {
        let id = self.active_id();
        self.pane_mut(id).expect("the active pane exists")
    }

    pub fn active_id(&self) -> PaneId {
        self.tabs[self.active_tab].active
    }

    /// Make `id` the active pane. Refused for a pane of a tab not shown:
    /// showing it is `App::select_tab`, which opens its panes.
    pub fn set_active(&mut self, id: PaneId) -> bool {
        let tab = &mut self.tabs[self.active_tab];
        if tab.panes.contains(&id) {
            if tab.active != id {
                tab.active = id;
                self.revision += 1;
            }
            true
        } else {
            false
        }
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    /// Pump every pane. Returns the total number of events applied.
    pub fn pump(&mut self) -> usize {
        self.pump_report().applied
    }

    /// Pump every pane, each for at most `PUMP_BUDGET`, and say which ones
    /// changed and whether events are still waiting.
    pub fn pump_report(&mut self) -> PumpReport {
        let mut r = PumpReport::default();
        for p in &mut self.panes {
            let n = p.pump(PUMP_BUDGET);
            if n > 0 {
                r.applied += n;
                r.panes_changed.push(p.id);
            }
            r.backlog |= p.backlog();
        }
        r
    }
}

/// How long one pump may apply a pane's events: a quarter of a frame at
/// 60 Hz, so the UI thread that pumps still draws while a large folder
/// streams in. The rest waits for the next pump (`PumpReport::backlog`).
pub const PUMP_BUDGET: std::time::Duration = std::time::Duration::from_millis(4);

/// What a pump did.
#[derive(Clone, Debug, Default)]
pub struct PumpReport {
    pub applied: usize,
    pub panes_changed: Vec<PaneId>,
    /// Events are still waiting: pump again soon, without waiting for the
    /// location to wake the caller (it already has).
    pub backlog: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_a_pane_before_the_active_one_keeps_it_active() {
        let mut ws = Workspace::default();
        let first = ws.active_id();
        let second = ws.add_pane();
        let third = ws.add_pane();
        assert!(ws.set_active(second));
        assert!(ws.close_pane(first));
        assert_eq!(ws.active_id(), second);
        assert!(ws.close_pane(second));
        assert_eq!(ws.active_id(), third);
    }

    #[test]
    fn removing_the_anchor_or_cursor_moves_them_to_what_remains() {
        let id = |ino| EntryId { dev: 1, ino };
        let mut sel = Selection::default();
        sel.set([id(1), id(2), id(3)]);
        sel.remove(id(1));
        sel.remove(id(3));
        assert_eq!((sel.anchor, sel.cursor), (Some(id(2)), Some(id(2))));
    }
}
