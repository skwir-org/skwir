//! Panes, selection, and focus: the state every view and command reads.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexSet;

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId};
use crate::location::{Listing, Location, OpenLocation, SortSpec};
use crate::orders::{Order, Orders};

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

/// Identifier of a window within the workspace (ADR-122): what a UI draws
/// its tabs in.
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
pub struct WindowId(pub u32);

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
///
/// The items were either picked (toggled, added, a range, all) or they follow
/// the cursor (an arrow or a click selects the one item reached). The
/// standard keys never ask; a key scheme's moves do (`SelectMode::Follow`,
/// `SelectMode::Pick`, ADR-024): a move carries a selection that follows the
/// cursor along and leaves a picked one where it is.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    items: IndexSet<EntryId>,
    pub anchor: Option<EntryId>,
    pub cursor: Option<EntryId>,
    picked: bool,
    sweep: Option<Sweep>,
    /// Stamped from one process-wide counter whenever the items change, so
    /// two selections never share a nonzero stamp; 0 is an untouched one.
    revision: u64,
}

/// A range held while the cursor moves (yazi's visual mode): what lies
/// between `from` and the cursor is selected, or with `unset` deselected,
/// over the items picked when it began. Any change to the selection other
/// than a move ends it; [`Selection::sweep_end`] keeps what it covers.
#[derive(Clone, Debug)]
struct Sweep {
    from: EntryId,
    unset: bool,
    base: IndexSet<EntryId>,
    /// The order it was last drawn over (held, so it is told by identity),
    /// with where `from` and the cursor were in it: a step over the same
    /// order changes only what lies between where the cursor was and where
    /// it is (`Selection::follow_at`).
    drawn: Option<(Arc<[EntryId]>, usize, usize)>,
}

/// How many times a sweep was drawn whole, for the tests that count them.
#[cfg(test)]
static SWEEPS_DRAWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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

    /// Whether the items were picked rather than following the cursor.
    pub fn picked(&self) -> bool {
        self.picked
    }

    /// `Some(unset)` while a sweep lasts: whether it deselects.
    pub fn sweeping(&self) -> Option<bool> {
        self.sweep.as_ref().map(|sweep| sweep.unset)
    }

    /// Deselect everything. The cursor stays where it was, so the keyboard
    /// carries on from there; a new location resets the whole selection.
    pub fn clear(&mut self) {
        self.touch();
        self.items.clear();
        self.anchor = None;
        self.picked = false;
        self.sweep = None;
    }

    /// Select `ids`. One item follows the cursor; several were picked.
    pub fn set(&mut self, ids: impl IntoIterator<Item = EntryId>) {
        self.touch();
        self.items = ids.into_iter().collect();
        self.anchor = self.items.first().copied();
        self.cursor = self.items.last().copied();
        self.picked = self.items.len() > 1;
        self.sweep = None;
    }

    pub fn add(&mut self, id: EntryId) {
        self.touch();
        self.items.insert(id);
        self.cursor = Some(id);
        self.anchor.get_or_insert(id);
        self.picked = true;
        self.sweep = None;
    }

    pub fn toggle(&mut self, id: EntryId) {
        self.touch();
        if !self.items.shift_remove(&id) {
            self.items.insert(id);
            self.anchor.get_or_insert(id);
        }
        self.cursor = Some(id);
        self.picked = !self.items.is_empty();
        self.sweep = None;
    }

    /// Toggle each of `ids`, in one pass over the selection: a shift per
    /// id was the whole selection's each.
    pub fn toggle_all(&mut self, ids: &[EntryId]) {
        if ids.len() <= 16 {
            for id in ids {
                self.toggle(*id);
            }
            return;
        }
        let Some(&last) = ids.last() else { return };
        self.touch();
        let toggled: std::collections::HashSet<EntryId> = ids.iter().copied().collect();
        let added: Vec<EntryId> = ids
            .iter()
            .copied()
            .filter(|id| !self.items.contains(id))
            .collect();
        self.items.retain(|id| !toggled.contains(id));
        for id in added {
            self.items.insert(id);
            self.anchor.get_or_insert(id);
        }
        self.cursor = Some(last);
        self.picked = !self.items.is_empty();
        self.sweep = None;
    }

    /// Add `added` and take out `removed`, as one change: what changed of a
    /// selection being drawn (a rubber band), where setting the whole of it
    /// again was a pass over every item for every move of the pointer. The
    /// rest stays in its order. The cursor goes to the last added; the
    /// anchor stays while it is selected, else goes to the first item. As
    /// with [`Selection::set`], several items were picked.
    pub fn change(&mut self, added: &[EntryId], removed: &[EntryId]) {
        self.touch();
        self.sweep = None;
        if removed.len() > 16 {
            let gone: crate::hashing::Set<EntryId> = removed.iter().copied().collect();
            self.items.retain(|id| !gone.contains(id));
        } else {
            for id in removed {
                self.items.shift_remove(id);
            }
        }
        self.items.extend(added.iter().copied());
        if self
            .anchor
            .is_none_or(|anchor| !self.items.contains(&anchor))
        {
            self.anchor = self.items.first().copied();
        }
        if let Some(&last) = added.last() {
            self.cursor = Some(last);
        }
        self.picked = self.items.len() > 1;
    }

    /// Pick `id`, or unpick it if it was picked. A selection that followed
    /// the cursor is not a pick, so the first pick starts from nothing;
    /// unpicking the last item gives the selection back to the cursor.
    pub fn pick(&mut self, id: EntryId) {
        self.touch();
        self.sweep = None;
        if !self.picked {
            self.items.clear();
            self.anchor = None;
        }
        if !self.items.shift_remove(&id) {
            self.items.insert(id);
            self.anchor.get_or_insert(id);
        }
        self.cursor = Some(id);
        self.picked = !self.items.is_empty();
        if !self.picked {
            self.items.insert(id);
            self.anchor = Some(id);
        }
    }

    /// Move the cursor to `id`, `order` being the pane's sorted order: a
    /// sweep covers up to it, what is in `ways` passed over ([`Ways`]), a
    /// selection that follows the cursor moves with it, a picked one stays.
    pub fn follow(&mut self, id: EntryId, order: &[EntryId], ways: Ways<'_>) {
        if self.sweep.is_some() {
            self.cursor = Some(id);
            self.redraw_sweep(order, ways);
        } else if self.picked {
            self.cursor = Some(id);
        } else {
            self.set([id]);
        }
    }

    /// Hold a range from the cursor while it moves: selecting, or with
    /// `unset` deselecting, over what was picked. Without a cursor there is
    /// nothing to start from.
    pub fn sweep_start(&mut self, unset: bool, order: &[EntryId], ways: Ways<'_>) {
        let Some(from) = self.cursor else { return };
        let base = if self.picked {
            self.items.clone()
        } else {
            IndexSet::new()
        };
        self.sweep = Some(Sweep {
            from,
            unset,
            base,
            drawn: None,
        });
        self.redraw_sweep(order, ways);
    }

    /// [`Selection::sweep_start`], the cursor at `at` in `order` (the pane's,
    /// as its view has it): the steps that follow over that order
    /// ([`Selection::follow_at`]) change only what they cross.
    pub fn sweep_start_at(
        &mut self,
        unset: bool,
        at: usize,
        order: &Arc<[EntryId]>,
        ways: Ways<'_>,
    ) {
        let Some(from) = self.cursor else { return };
        if order.get(at) != Some(&from) {
            return self.sweep_start(unset, order, ways);
        }
        self.sweep_start(unset, &order[at..=at], ways);
        if let Some(sweep) = &mut self.sweep {
            sweep.drawn = Some((Arc::clone(order), at, at));
        }
    }

    /// [`Selection::follow`], `id` being at `at` in `order`: during a sweep
    /// begun or last drawn over that same order, only what lies between
    /// where the cursor was and where it is changes. A step of a sweep over
    /// a whole folder searched the order for both ends and made the whole
    /// selection again, at each key held down. Another order, or an `at`
    /// that is not `id`'s, draws it whole, as `follow` does.
    pub fn follow_at(&mut self, id: EntryId, at: usize, order: &Arc<[EntryId]>, ways: Ways<'_>) {
        let drawn = self.sweep.as_ref().and_then(|sweep| {
            let (drawn, from_at, last) = sweep.drawn.as_ref()?;
            (Arc::ptr_eq(drawn, order) && order.get(at) == Some(&id)).then_some((*from_at, *last))
        });
        let Some((from_at, last)) = drawn else {
            self.follow(id, order, ways);
            // Drawn whole over this order: the next steps need not be.
            let cursor = order.get(at).filter(|here| **here == id).map(|_| at);
            if let (Some(sweep), Some(at)) = (&mut self.sweep, cursor) {
                sweep.drawn = order
                    .iter()
                    .position(|x| *x == sweep.from)
                    .map(|from_at| (Arc::clone(order), from_at, at));
            }
            return;
        };
        self.cursor = Some(id);
        let Some(sweep) = &mut self.sweep else { return };
        let span = |c: usize| (from_at.min(c), from_at.max(c));
        let ((lo0, hi0), (lo1, hi1)) = (span(last), span(at));
        // Both hold `from`: what one holds and not the other lies below
        // both or above both.
        let below = &order[lo0.min(lo1)..lo0.max(lo1)];
        let above = &order[hi0.min(hi1) + 1..=hi0.max(hi1)];
        let (mut entered, mut left): (Vec<EntryId>, Vec<EntryId>) = (Vec::new(), Vec::new());
        (if lo1 < lo0 { &mut entered } else { &mut left }).extend_from_slice(below);
        (if hi1 > hi0 { &mut entered } else { &mut left }).extend_from_slice(above);
        // What enters the range is selected (or deselected); what leaves it
        // is as it was before the sweep.
        let (add, take): (Vec<EntryId>, Vec<EntryId>) = if sweep.unset {
            let back = left
                .into_iter()
                .filter(|id| sweep.base.contains(id))
                .collect();
            (back, entered)
        } else {
            let back = left
                .into_iter()
                .filter(|id| !sweep.base.contains(id))
                .collect();
            entered.retain(|id| ways.is_none_or(|ways| !ways.contains(id)));
            (entered, back)
        };
        if take.len() > 16 {
            let take: crate::hashing::Set<EntryId> = take.into_iter().collect();
            self.items.retain(|id| !take.contains(id));
        } else {
            for id in &take {
                self.items.shift_remove(id);
            }
        }
        self.items.extend(add);
        sweep.drawn = Some((Arc::clone(order), from_at, at));
        self.anchor = Some(sweep.from);
        self.picked = true;
        self.touch();
    }

    /// End the sweep, keeping what it covers.
    pub fn sweep_end(&mut self) {
        if self.sweep.take().is_some() {
            self.picked = !self.items.is_empty();
        }
    }

    fn redraw_sweep(&mut self, order: &[EntryId], ways: Ways<'_>) {
        #[cfg(test)]
        SWEEPS_DRAWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(sweep) = &mut self.sweep else { return };
        sweep.drawn = None;
        let cursor = self.cursor.unwrap_or(sweep.from);
        let pos = |id: EntryId| order.iter().position(|x| *x == id);
        let range: &[EntryId] = match (pos(sweep.from), pos(cursor)) {
            (Some(a), Some(b)) => &order[a.min(b)..=a.max(b)],
            _ => std::slice::from_ref(&sweep.from),
        };
        let mut items = sweep.base.clone();
        if sweep.unset {
            // One pass: a shift per id swept was the whole selection's each.
            let swept: std::collections::HashSet<&EntryId> = range.iter().collect();
            items.retain(|id| !swept.contains(id));
        } else {
            items.extend(
                range
                    .iter()
                    .copied()
                    .filter(|id| ways.is_none_or(|ways| !ways.contains(id))),
            );
        }
        self.anchor = Some(sweep.from);
        self.items = items;
        self.picked = true;
        self.touch();
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
        self.picked &= !self.items.is_empty();
        self.sweep = None;
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

    /// Follow entries renamed (`Listing::take_renamed`, each id before and
    /// after, in the order they were): what was selected of them stays
    /// selected, in its place, the cursor and the anchor on it. Renamed
    /// twice (A to B, then B to C), it follows both.
    pub fn rekey(&mut self, renamed: &[(EntryId, EntryId)]) {
        if renamed.is_empty() {
            return;
        }
        // Where each id ends: taken from the last rename back, an id
        // renamed is where what it was renamed to ends.
        let mut ends: crate::hashing::Map<EntryId, EntryId> = crate::hashing::Map::default();
        for (before, after) in renamed.iter().rev() {
            let end = ends.get(after).copied().unwrap_or(*after);
            ends.insert(*before, end);
        }
        let now = |id: EntryId| ends.get(&id).copied().unwrap_or(id);
        if self.items.iter().any(|id| now(*id) != *id) {
            self.items = self.items.iter().map(|id| now(*id)).collect();
            self.touch();
        }
        self.anchor = self.anchor.map(now);
        self.cursor = self.cursor.map(now);
        if let Some(sweep) = &mut self.sweep {
            sweep.from = now(sweep.from);
            sweep.base = sweep.base.iter().map(|id| now(*id)).collect();
            sweep.drawn = None;
        }
    }

    /// Drop ids that no longer exist in the listing.
    pub fn retain_in(&mut self, listing: &Listing) {
        self.retain(|id| listing.get(id).is_some());
    }

    /// Drop the ids `holds` no longer knows: what the pane no longer shows.
    pub fn retain(&mut self, holds: impl Fn(EntryId) -> bool) {
        let before = self.items.len();
        self.items.retain(|id| holds(*id));
        if self.items.len() != before {
            self.touch();
        }
        if self.anchor.is_some_and(|a| !holds(a)) {
            self.anchor = self.items.first().copied();
        }
        if self.cursor.is_some_and(|c| !holds(c)) {
            self.cursor = self.items.last().copied();
        }
        if let Some(sweep) = &mut self.sweep {
            if holds(sweep.from) {
                sweep.base.retain(|id| holds(*id));
                sweep.drawn = None;
            } else {
                self.sweep = None;
            }
        }
        self.picked &= !self.items.is_empty();
    }

    /// Drop the ids `holds` no longer knows, as `retain` does; a cursor
    /// whose entry went goes to its neighbour in `order`, what was shown
    /// before: the first entry after it still shown, else the last before
    /// it (`back`: before it first, as a folder closed in place takes the
    /// cursor that was inside it). A selection the change emptied selects
    /// the neighbour, as an arrow reaching it would: Delete pressed again
    /// takes the next file, as in Dolphin and Nautilus. A folder in `ways`
    /// is no next file ([`Ways`]): it is passed over, and when the cursor
    /// has nowhere else to go (a folder closed), it is not selected. A
    /// cursor `order` does not hold falls back as `retain` has it.
    pub fn retain_near(
        &mut self,
        holds: impl Fn(EntryId) -> bool,
        order: &[EntryId],
        back: bool,
        ways: Ways<'_>,
    ) {
        let lost = self.cursor.filter(|cursor| !holds(*cursor));
        let had = !self.items.is_empty();
        self.retain(&holds);
        let Some(at) = lost.and_then(|lost| order.iter().position(|id| *id == lost)) else {
            return;
        };
        let way = |id: EntryId| ways.is_some_and(|ways| ways.contains(&id));
        let found = |id: EntryId| holds(id) && !way(id);
        let after =
            |here: &dyn Fn(EntryId) -> bool| order[at + 1..].iter().copied().find(|id| here(*id));
        let before =
            |here: &dyn Fn(EntryId) -> bool| order[..at].iter().rev().copied().find(|id| here(*id));
        let near = if back {
            before(&holds).or_else(|| after(&holds))
        } else {
            after(&found)
                .or_else(|| before(&found))
                .or_else(|| after(&holds))
                .or_else(|| before(&holds))
        };
        let Some(near) = near else { return };
        self.cursor = Some(near);
        if had && self.items.is_empty() && !way(near) {
            self.touch();
            self.items.insert(near);
            self.anchor = Some(near);
        }
    }
}

/// The folders drawn among a pane's rows only as the way to what a search
/// found (`Pane::ways`, ADR-127): only naming one selects it (a click, an
/// arrow, the end of a range). What passes over rows (Select All, Invert,
/// a range, a sweep, a rubber band: `Pane::covered`) and what picks a
/// neighbour leave them, or a Delete would take a whole folder unseen,
/// files the search never found included.
pub type Ways<'a> = Option<&'a crate::hashing::Set<EntryId>>;

/// A location a pane keeps open beside the one it shows, with its listing
/// kept current: what its view shows next to it (the columns of the folders
/// above), and what going there again adopts instead of listing it anew.
pub struct Kept {
    pub location: Location,
    /// The folder it is, for a path location: how an opened folder finds
    /// its listing.
    path: Option<PathBuf>,
    /// Closed when the kept location goes, unless it was taken to be shown.
    open: Option<Arc<dyn OpenLocation>>,
    pub listing: Listing,
    /// The order its folder remembers (ADR-132): a column beside the pane
    /// is sorted so, else as the pane's own (`Pane::sorting_of`), so no
    /// column moves as the pane goes from one to the next.
    order: Option<Order>,
    /// What the sorter knows of its entries, where its order is not the
    /// pane's and wants keys (`Pane::column_keys_wanted`), and what they
    /// were made for.
    keys: Option<crate::grouping::Keys>,
    keys_for: Option<KeysFor>,
}

impl Kept {
    fn new(open: Arc<dyn OpenLocation>, listing: Listing, order: Option<Order>) -> Self {
        let location = listing.address(open.as_ref()).clone();
        Kept {
            path: location.path(),
            location,
            open: Some(open),
            listing,
            order,
            keys: None,
            keys_for: None,
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

    /// Events wait to be applied: not those of a listing filled elsewhere,
    /// whose jobs take them, nor of one sorted elsewhere, whose order
    /// comes first; each wakes the window (ADR-120, ADR-121). What such a
    /// job left to be taken counts: its one wake may have been spent.
    fn backlog(&self) -> bool {
        self.listing.has_result()
            || (!self.listing.is_held()
                && !self.listing.is_sorting()
                && self
                    .open
                    .as_ref()
                    .is_some_and(|open| !open.events().is_empty()))
    }

    /// Follow the folder it lists where it moved (`Listing::take_moved`):
    /// where it was reached before and now.
    fn follow_moved(&mut self) -> Option<(Location, Location)> {
        let open = self.open.as_ref()?;
        let (_, now) = self.listing.take_moved()?;
        if !open.location().is_path() {
            return None;
        }
        let before = std::mem::replace(&mut self.location, now.clone());
        self.path = now.path();
        Some((before, now))
    }

    /// It is at `now`, moved with a folder above it.
    fn relocate(&mut self, now: Location) {
        self.path = now.path();
        self.listing.relocate(now.clone());
        self.location = now;
    }
}

/// Where `path` is once the folder `from` moved to `to`: itself, or what
/// is under it, moved with it; `None` when it is elsewhere.
fn moved_path(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(rest)
    })
}

impl Drop for Kept {
    fn drop(&mut self) {
        if let Some(open) = self.open.take() {
            open.close();
        }
    }
}

/// A folder, or a place (a tag's), opened in place under the one shown
/// (ADR-025): where it leads, the entry it opens at, and the one opened
/// whose listing has that entry (`None`: the shown listing's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    pub location: Location,
    pub at: EntryId,
    pub within: Option<Location>,
}

/// The folders a pane opened in place, with their listings: a walk over
/// them, shallowest first, never without their listing. Many opened among
/// many kept (a deep tree), each is found by a map made for the walk
/// rather than a search of them all.
fn opened<'a>(
    kept: &'a [Kept],
    expanded: &'a [Opened],
) -> impl Iterator<Item = (&'a Opened, &'a Kept)> + 'a {
    let by_location: Option<HashMap<&Location, &Kept>> =
        (expanded.len() * kept.len() > 64).then(|| kept.iter().map(|k| (&k.location, k)).collect());
    expanded.iter().filter_map(move |o| {
        let kept = match &by_location {
            Some(map) => map.get(&o.location).copied(),
            None => kept.iter().find(|k| k.location == o.location),
        }?;
        Some((o, kept))
    })
}

/// Each entry of the folders opened in place, by the opened folder and
/// the kept listing that hold it: indexes into `Pane::expanded` and
/// `Pane::kept`, the shallowest first where several hold one.
struct OpenedIndex(HashMap<EntryId, (usize, usize)>);

/// How many times an `OpenedIndex` was made, for the tests that count them.
#[cfg(test)]
static OPENED_INDEXES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl OpenedIndex {
    fn of(kept: &[Kept], expanded: &[Opened]) -> Self {
        #[cfg(test)]
        OPENED_INDEXES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut held = HashMap::new();
        for (e, o) in expanded.iter().enumerate() {
            let Some(k) = kept.iter().position(|k| k.location == o.location) else {
                continue;
            };
            for entry in kept[k].listing.iter() {
                held.entry(entry.id).or_insert((e, k));
            }
        }
        OpenedIndex(held)
    }
}

/// The order last computed for a pane with folders opened in place, and
/// what it was computed from: the orders of the listings it interleaves
/// (compared by identity, as `Listing::sorted` keeps one until it changes)
/// and the set of folders opened.
struct OrderCache {
    expanded: u64,
    from: Vec<Arc<[EntryId]>>,
    order: Arc<[EntryId]>,
}

/// What a pane's keys were made for: the sort's criterion, the group's,
/// and the sorters' revision.
pub(crate) type KeysFor = (
    crate::location::SortKey,
    Option<crate::location::SortKey>,
    u64,
);

/// The groups of a pane's order (`Pane::groups`), and what they were made
/// from, compared by identity.
struct GroupsCache {
    order: Arc<[EntryId]>,
    spans: Arc<[crate::grouping::Span]>,
    groups: Arc<[crate::grouping::Span]>,
}

/// What names a group folded (ADR-131, 2): where it stands among those of
/// its criterion, its rank and key, so it stays folded as keys are made
/// again; none for the entries whose group is not read yet.
type FoldKey = Option<(u64, Arc<[u8]>)>;

fn fold_key(group: Option<&crate::grouping::Group>) -> FoldKey {
    group.map(|g| (g.rank, Arc::clone(&g.key)))
}

/// The longest the window holds what it showed while what replaces it is
/// made (ADR-120, amended): the files of the folder left while the one
/// gone to is filled elsewhere (`Pane::drawn_entry`), a tab left while
/// the one shown is made (skwir-ui's `tabs::HOLD`). The user's bound.
pub const HOLD: std::time::Duration = std::time::Duration::from_millis(150);

/// The folder left, drawn while the one gone to is filled elsewhere
/// (`Pane::drawn_left`), until `until`.
#[derive(Debug)]
struct DrawnLeft {
    listing: Listing,
    until: std::time::Instant,
}

impl DrawnLeft {
    /// Whether its time to be drawn is over.
    fn over(&self) -> bool {
        std::time::Instant::now() >= self.until
    }
}

/// What a pane asks to read of its entries on demand (ADR-131, 4): for
/// one listing of its order (its own, a folder's it opened in place),
/// each entry with where it is, its identity when the listing knows it,
/// and the fields it lacks.
pub struct FieldsAsked {
    pub pane: PaneId,
    pub listing: u64,
    pub cancel: CancelToken,
    pub entries: Vec<(
        EntryId,
        PathBuf,
        Option<crate::fields::Identity>,
        Vec<&'static str>,
    )>,
}

/// What a pane asked to read on demand of one listing of its order
/// (`Pane::fields_to_read`): the fields, the providers' revision and the
/// listing's fields version it rests on, the listing's version then (what
/// came or changed since is asked next), and the token of its reading.
#[derive(Clone)]
struct FieldsAt {
    listing: u64,
    fields: Vec<&'static str>,
    revision: u64,
    fields_version: u64,
    version: u64,
    cancel: CancelToken,
}

/// What was read on demand of one entry (`Pane::take_fields`): the field,
/// the identity of the file read, and what it says (`Value::Bool(false)`
/// where nothing).
pub(crate) type FieldRead = (
    EntryId,
    &'static str,
    Option<crate::fields::Identity>,
    crate::entry::Value,
);

/// Write what `states` says into the entries of `listing`, the folder at
/// `level` below its root (ADR-134): the entries it names, found by their
/// names; the rest of a folder new or ignored whole; and, where the answer
/// is whole, a state held before by an entry it no longer names, which is
/// now as the last commit has it. What a partial answer did not reach is
/// left as it was, never said as committed; an entry nobody said anything
/// of holds no value. Only what differs reaches `set_fields`. Whether any
/// changed.
fn take_vcs(listing: &mut Listing, states: &crate::vcs::States, level: &str) -> bool {
    use crate::entry::{Value, keys};
    use crate::vcs::State;
    let word = |state: State| Value::Text(state.word().into());
    let mut values = Vec::new();
    let mut said = crate::hashing::Set::<EntryId>::default();
    for name in states.named_at(level) {
        let path = if level.is_empty() {
            name.clone()
        } else {
            format!("{level}/{name}")
        };
        for e in listing.named(OsStr::new(name.as_str())) {
            said.insert(e.id);
            let state = states.of(&path, e.is_dir());
            if State::of(e) != state {
                values.push((e.id, keys::VCS, state.map_or(Value::Bool(false), word)));
            }
        }
    }
    match states.default_at(level) {
        Some(state) => values.extend(
            listing
                .iter()
                .filter(|e| !said.contains(&e.id) && State::of(e) != Some(state))
                .map(|e| (e.id, keys::VCS, word(state))),
        ),
        None if states.is_whole() => values.extend(
            listing
                .iter()
                .filter(|e| !said.contains(&e.id) && State::of(e).is_some())
                .map(|e| (e.id, keys::VCS, Value::Bool(false))),
        ),
        None => {}
    }
    !values.is_empty() && listing.set_fields(values)
}

/// What a pane asks its folder's version control (ADR-134,
/// `Pane::states_to_read`): the folder its listing shows, to say of its
/// entries and of those of the folders it opened in place, and the folders
/// above it its view shows too (a columns view's), which the answer may
/// cover. `asking` tells this asking from the next: only the latest one's
/// answer is taken.
pub struct StatesAsked {
    pub pane: PaneId,
    pub listing: u64,
    pub asking: u64,
    pub folder: PathBuf,
    pub above: Vec<PathBuf>,
    pub cancel: CancelToken,
}

/// Where something a plan brings into a pane's folder stands in what the
/// pane shows (`Pane::stands`, ADR-135).
#[derive(Clone, Debug)]
pub struct Stand {
    /// The place in the listing's order (`Pane::listed_order`) of the
    /// first entry after it; the order's length past the last.
    pub at: usize,
    /// Its place among what is brought, as the order would have them.
    pub among: usize,
    /// The group it would be in, where the pane groups.
    pub group: Option<Arc<crate::grouping::Group>>,
}

/// What the keys of what arrives in a listing were made for
/// (`Pane::keying`, ADR-135): the sort of its order shown, whether hidden
/// names are shown, and the filter as it stood. Keys made for one hold
/// while it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keying {
    sort: crate::grouping::SortStamp,
    show_hidden: bool,
    filter: u64,
}

/// What a pane last asked of the version control: its listing; its
/// listings' names (the shown one's, the opened ones' and the columns'
/// above, summed with their ids); the folders it opened in place; the
/// providers' revision; and the generation of the answer it holds.
type StatesAt = (u64, u64, u64, u64, u64);

/// How often files written in a pane's folders ask its version control
/// again at most: a file written each second (a log, a download) would
/// read the whole folder's state each second. The first write after a
/// quiet while asks at once.
const STATES_WRITTEN_PACE: std::time::Duration = std::time::Duration::from_secs(5);

/// What to do with a pane's groups (`group_fold`).
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FoldMode {
    /// Fold it if it is open, else open it.
    #[default]
    Toggle,
    Fold,
    Unfold,
    /// Fold every group but it.
    FoldOthers,
    /// Open every group.
    UnfoldAll,
}

/// The groups a pane folded, for the criterion it groups by then: kept
/// while it groups by it, forgotten when it groups by another.
#[derive(Debug, Default)]
struct Folds {
    by: Option<crate::location::SortKey>,
    groups: crate::hashing::Set<FoldKey>,
    /// Moved at each change: what the order and the views made of the
    /// folds is made again.
    version: u64,
}

/// The order shown and its groups with folded groups' entries left out,
/// and what they were made from, compared by identity.
struct FoldCache {
    from: Arc<[EntryId]>,
    spans_from: Arc<[crate::grouping::Span]>,
    version: u64,
    order: Arc<[EntryId]>,
    spans: Arc<[crate::grouping::Span]>,
    /// The entries left out.
    folded: Arc<crate::hashing::Set<EntryId>>,
}

/// The groups of `order`, which holds entries of `top` (`is_top`) in
/// `top`'s order, some left out, and entries of folders opened in place
/// after the folder: each of `top`'s by its span of `top` (`spans`), each
/// other in the group of the entry before it. `None` when `order` is not
/// in `top`'s order. Linear in both.
fn groups_within(
    order: &[EntryId],
    top: &[EntryId],
    spans: &[crate::grouping::Span],
    is_top: impl Fn(EntryId) -> bool,
) -> Option<Vec<crate::grouping::Span>> {
    let mut at = 0;
    let mut span = 0;
    let mut lost = false;
    let groups = crate::grouping::spans_of(order, |_, id| {
        if lost || !is_top(id) {
            return None;
        }
        while top.get(at).is_some_and(|&t| t != id) {
            at += 1;
        }
        if at == top.len() {
            lost = true;
            return None;
        }
        while spans.get(span).is_some_and(|s| s.start + s.len <= at) {
            span += 1;
        }
        Some(spans.get(span).and_then(|s| s.group.clone()))
    });
    (!lost).then_some(groups)
}

/// How a name waiting to be selected is selected once listed
/// (`Pane::reveal`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Revealing {
    /// Asked for by its name (a path typed, a jump): a hidden one turns
    /// hidden files on; forgotten once the listing is complete without it.
    Asked,
    /// Listed a moment after the listing is complete: kept until it is.
    Coming,
    /// The way back down, gone up from it: selected only if shown.
    WayDown,
}

/// One pane: an open location, its listing, selection, and history.
pub struct Pane {
    pub id: PaneId,
    pub open: Option<Arc<dyn OpenLocation>>,
    location_lifetime: CancelToken,
    /// What the pane's switch said when a location that follows it was
    /// opened (`LocationProvider::follows_hidden`); none for any other.
    listed_hidden: Option<bool>,
    pub listing: Listing,
    /// The locations the pane's view asked to keep (`App::keep_locations`).
    kept: Vec<Kept>,
    /// The location navigation just left, until the view says whether it
    /// keeps it: entering a folder makes the one left a column beside it.
    left: Option<Kept>,
    /// The entries of the folder left, while the one gone to is filled
    /// elsewhere (ADR-120, amended), for the view to draw meanwhile
    /// (`drawn_entry`): its listing alone, its location closed, [`HOLD`]
    /// at most, let go of at the first sync after.
    drawn_left: Option<DrawnLeft>,
    pub selection: Selection,
    /// Sort and hidden-file settings, as asked; views render
    /// `listing.order_shown(sort)`, the order before while a worker makes
    /// this one (ADR-121).
    pub sort: SortSpec,
    /// The pane's own order, set aside while the place it shows remembers
    /// one of its own (ADR-132), which `sort` shows then; back once it
    /// shows a place that remembers none (`show_order`). None while it
    /// shows its own.
    own: Option<Order>,
    /// The view the place shown remembers (ADR-132), which the shell shows
    /// in place of the pane's own where both are views a folder may
    /// remember; none where it remembers none.
    place_view: Option<Arc<str>>,
    /// What the sorter knows of the entries for `sort` (ADR-130): made by
    /// `App::make_keys_again` whenever `sort` needs them; every listing the
    /// pane shows is sorted with them (`sorting`).
    keys: Option<crate::grouping::Keys>,
    /// What `keys` were made for: the criteria, and the sorters' revision
    /// then; none while the sort needs none.
    keys_for: Option<KeysFor>,
    pub history: Vec<Location>,
    /// Index into `history` of the current location.
    pub history_pos: usize,
    /// By step of `history`, what the pane had there when it left it: the
    /// entry the cursor was on, which Back and Forward select again
    /// (`go_to`), and what filtered it, which they show it with (ADR-109).
    left_at: Vec<StepLeft>,
    /// A name to select once the listing has it, and how; see
    /// [`Pane::reveal`].
    reveal: Option<(OsString, Revealing)>,
    /// The listing's names when the reveal last looked for its name: it
    /// looks again only once they change, not at every pump.
    reveal_looked: Option<u64>,
    /// Which opened folder holds each entry the pane shows beside its own
    /// listing's, made when first asked and forgotten when the folders
    /// opened, the listings kept or theirs change: asked for every row
    /// drawn, a deep tree's lookups searched every opened folder.
    opened_index: parking_lot::Mutex<Option<OpenedIndex>>,
    /// The selection a dormant pane had, given back once its listing is
    /// complete again (see [`Pane::suspend`]).
    restore: Option<Selection>,
    /// Folders (and places) opened in place under the one shown (the
    /// tree view), each after the one it is in. Their listings are kept
    /// beside the shown one (`App::open_expanded`, `keep_only`), and their
    /// entries are the pane's as much as the shown listing's: in
    /// [`Pane::order`], selectable, found by [`Pane::entry`] and
    /// [`Pane::path_of`]. Navigation closes them; dormancy keeps them,
    /// like the sort.
    expanded: Vec<Opened>,
    /// Changes with the set of folders opened.
    expanded_version: u64,
    /// Entries a view draws beside what the pane lists (Usage's blocks
    /// inside the folder's own entries, ADR-116), lent by the view while
    /// it draws them: shown, so selected and acted on as the listing's,
    /// found by [`Pane::entry`] and [`Pane::path_of`], but in no order:
    /// Select all, a range, the filter and the counts are the listing's.
    /// Navigation and dormancy let them go; the view lends them again.
    lent: Lent,
    /// Changes whenever what the opened folders show may have changed: the
    /// set, or one of their listings.
    tree_version: u64,
    /// Changes when the entries the opened folders show may have: the set,
    /// or one of their listings' names or what it shows
    /// (`Listing::names_version`, `Listing::shown_version`). Their metadata
    /// leaves it alone.
    tree_members_version: u64,
    /// The selection as last looked over: see `retain_changed`.
    retained: Option<Retained>,
    order_cache: parking_lot::Mutex<Option<OrderCache>>,
    /// The groups of the order last shown (`groups`): the order and the
    /// listing's spans they were made from, and the groups.
    groups_cache: parking_lot::Mutex<Option<GroupsCache>>,
    /// The groups folded (ADR-131, 2), and the order and groups made of
    /// them last.
    folds: Folds,
    fold_cache: parking_lot::Mutex<Option<FoldCache>>,
    /// Whether the view shown draws its groups' headings, where a folded
    /// group stays as its heading (`PaneView::draws_headings`): only then do
    /// folds take their entries out of the order. Under a view that draws
    /// none (the columns, Usage, the map, the board) every file stays
    /// shown; the folds are kept for when one is drawn again.
    folds_drawn: bool,
    /// What was asked to read on demand of each listing of its order
    /// (`fields_to_read`), with the token of its reading, cancelled when it
    /// rests on something else or the listing leaves the order.
    fields_asked: Vec<FieldsAt>,
    /// What was last asked of the version control (`states_to_read`),
    /// with the files written then and when, the token of its reading, the
    /// asking it was (`StatesAsked::asking`), and who said what its
    /// entries hold, with its answer's key (`vcs::Answer::key`): what
    /// tells it changed.
    states_asked: Option<(StatesAt, u64, std::time::Instant)>,
    states_reading: Option<CancelToken>,
    states_asking: u64,
    states_said: Option<(&'static str, u64)>,
    /// What its view shows that is read on demand (a column of when each
    /// picture was taken, `show_fields`): read as what its sort reads is.
    fields_shown: Vec<&'static str>,
    /// The order `order` last gave, what is shown: where a cursor whose
    /// entry leaves the view goes from (`Selection::retain_near`).
    shown: parking_lot::Mutex<Option<Arc<[EntryId]>>>,
    /// What is typed in the address field before Enter searches, or what
    /// Quick filters chose: only the entries it keeps are shown
    /// (`Pane::order`). Showing another location keeps its lens (ADR-107)
    /// and lets go of the rest.
    filter: Option<Arc<str>>,
    /// The filter as the filter's reader read it (ADR-098), a test of
    /// what the listing knows; `None`: names that hold the text.
    read: Option<Arc<ReadFilter>>,
    /// Changes with what the filter shows: a view keys what it drew on it.
    filter_version: u64,
    /// Changes with the test itself: what is cached of the entries
    /// (`FilterCache`, `RetainKey`) holds until then. An answer about a
    /// folder moves only `filter_version`, the entries it names applied in
    /// the cache (`Pane::take_leads`).
    filter_tested: u64,
    /// The pane's lens (ADR-107): its filter's, or the one the location
    /// holds in its own terms (a search's).
    lens: Option<Lens>,
    /// The agent's call that put its location in a dormant pane (a tab
    /// opened behind, `tab.new`): opened later, on the window's thread, it
    /// is opened as that call (`permissions::as_agent`), so a location
    /// that asks before it acts for an agent (a server connected) asks.
    asked_by: Option<crate::permissions::Privacy>,
    /// The filtered order, the order it filtered (by identity), the
    /// filter's version and, while it reads metadata, the listings' (a file's
    /// metadata arriving decides whether it is shown), and the order.
    filter_cache: parking_lot::Mutex<Option<FilterCache>>,
    /// What was found of the folders the lens leaves standing.
    leading: Leading,
    /// When the filter was last replaced by hand: one replaced again at
    /// once is being typed.
    filter_replaced_at: Option<std::time::Instant>,
    /// A search laid out as a tree.
    found: FoundTree,
}

/// A search laid out as the tree of its folders (`found_tree`), while a
/// view shows it so.
#[derive(Default)]
struct FoundTree {
    wanted: bool,
    made: Option<crate::found_tree::Made>,
    made_times: u64,
    /// The flat order the tree shown was made of, and one being made of.
    from: Option<Arc<[EntryId]>>,
    working: bool,
    answer: Arc<parking_lot::Mutex<Option<FoundAnswer>>>,
    /// The folders on the way read so far.
    known: Arc<parking_lot::Mutex<HashMap<PathBuf, Entry>>>,
    /// The folders closed in it, and how many times that changed.
    folded: crate::hashing::Set<EntryId>,
    folds: u64,
    cancel: CancelToken,
    /// The rows shown, what they were made of.
    shown: parking_lot::Mutex<Option<FoundShown>>,
}

/// The flat order a tree was made of, and the tree.
pub type FoundAnswer = (Arc<[EntryId]>, FoundMade);

/// A tree made, on its way to the pane.
#[derive(Debug)]
pub struct FoundMade(pub(crate) crate::found_tree::Made);

/// A tree's rows as shown, and what made them.
struct FoundShown {
    /// What says that a row may be gone from what was found: never a hit
    /// arriving, which waits for the next tree.
    gone: (u64, u64, u64),
    folds: u64,
    made: u64,
    order: Arc<[EntryId]>,
}

/// A pane's search to lay out as a tree, on a worker.
pub struct FoundAsked {
    pub pane: PaneId,
    pub open: Arc<dyn OpenLocation>,
    pub root: PathBuf,
    pub flat: Arc<[EntryId]>,
    pub ascending: bool,
    pub answer: Arc<parking_lot::Mutex<Option<FoundAnswer>>>,
    pub known: Arc<parking_lot::Mutex<HashMap<PathBuf, Entry>>>,
    pub cancel: CancelToken,
}

impl std::fmt::Debug for FoundAsked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FoundAsked")
            .field("pane", &self.pane)
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl FoundAsked {
    /// Make the tree and leave it for the pane's next pump: whether there
    /// is one to take.
    pub fn run(&self) -> bool {
        let hits: Vec<(EntryId, PathBuf)> = self
            .flat
            .iter()
            .filter_map(|id| Some((*id, self.open.path_of(*id)?)))
            .collect();
        let mut statter = crate::fs::Statter::default();
        let made = crate::found_tree::make(
            &self.root,
            &hits,
            self.ascending,
            &mut |path| statter.stat(path),
            &mut self.known.lock(),
            &|| self.cancel.is_cancelled(),
        );
        let Some(made) = made else { return false };
        *self.answer.lock() = Some((Arc::clone(&self.flat), FoundMade(made)));
        true
    }
}

impl FoundTree {
    fn reset(&mut self) {
        self.cancel.cancel();
        *self = FoundTree {
            wanted: self.wanted,
            ..FoundTree::default()
        };
    }

    /// The tree when it is the one shown.
    fn shown(&self) -> Option<&crate::found_tree::Made> {
        self.made.as_ref().filter(|_| self.wanted)
    }
}

/// The id a tree lends its folders on the way under.
const FOUND_TREE: &str = "found-tree";

/// How long a lens being typed waits before folders are looked into: each
/// key would read another lens.
const TYPED_LENS_WAITS: std::time::Duration = std::time::Duration::from_millis(300);

/// How long the view stays as it was when a lens comes, for the first
/// answers about its folders: most come at once, and the view then changes
/// once. What takes longer comes after, as a search's results do.
pub(crate) const VIEW_KEPT: std::time::Duration = std::time::Duration::from_millis(150);

/// What a pane found out of the folders it shows under its lens: a folder
/// shows once a search of it found something (`LeadsAsked`), as a search's
/// results come. The slow answers are the folders that lead nowhere, a
/// search of one ending only at its last file, and those change nothing
/// on the screen. A folder that cannot be looked into stands from the
/// start (`Pane::folders_to_ask`).
#[derive(Default)]
struct Leading {
    /// The folders asked about, answered or not, each with the ask that
    /// last did: an answer from any other is out of date.
    asked: crate::hashing::Map<EntryId, u64>,
    /// Those not found to lead anywhere, or not yet: not shown.
    unproved: crate::hashing::Set<EntryId>,
    /// Answers waiting for the next pump: a folder, its ask, whether it
    /// leads anywhere.
    answers: LeadsAnswers,
    /// Ends with the lens, the location, or the pane.
    cancel: CancelToken,
    /// What the folders were last looked over at.
    looked: Option<LeadingKey>,
    /// Each listing's version the rows were last looked over at.
    at: Vec<(u64, u64)>,
    /// The rows being looked over for folders to ask about.
    pass: Option<LeadsPass>,
    /// The lens was typed: what is asked of it waits a moment.
    typed: bool,
    /// The folders asked about still to be answered.
    waiting: crate::hashing::Set<EntryId>,
    /// Every folder is to be asked about, not only those new here.
    again: bool,
    /// Folders to ask about again, by name: what a plan changed is below
    /// them (`Pane::ask_again_above`).
    again_of: crate::hashing::Set<EntryId>,
    /// What is asked next is looked at, never remembered by an index: a
    /// plan's changes, F5.
    fresh: bool,
    /// The asks under way, by number.
    jobs: crate::hashing::Map<u64, LeadsJob>,
    asks: u64,
    /// Folders left out or shown since what is shown was last looked over.
    moved: Vec<EntryId>,
    /// What folders still being asked about were renamed to, while their
    /// answers come under the id they went out with.
    renamed: crate::hashing::Map<EntryId, EntryId>,
    /// The view as it was, until the first answers come.
    kept: Option<KeptView>,
}

/// An ask under way: ended when every folder it was asked about was asked
/// about again.
struct LeadsJob {
    cancel: CancelToken,
    /// How many of its folders are still its to answer.
    left: usize,
}

/// A look over the rows for the folders to ask about (`Pane::folders_to_ask`),
/// at most `PUMP_BUDGET` at a time: a folder of folders (`/nix/store`) has
/// hundreds of thousands.
struct LeadsPass {
    rows: Arc<[EntryId]>,
    /// The next row to look at.
    next: usize,
    ask: u64,
    /// Every row asked about, not only those new here.
    again: bool,
    /// The rows to ask about again by name.
    named: crate::hashing::Set<EntryId>,
    /// The folders opened in place: what is opened stands until nothing is
    /// found in it (`Pane::folders_to_ask`).
    opened: crate::hashing::Set<EntryId>,
    /// What it found to ask about.
    folders: Vec<(EntryId, PathBuf)>,
    /// Whether the folders it has not reached yet are kept from showing.
    hiding: Hiding,
    fresh: bool,
}

/// Whether folders not reached yet by a look over the rows are shown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hiding {
    /// Not a first look (`Leading::again`, what arrived): nothing moves.
    Never,
    /// A first look of a location or a lens: should it take more than one
    /// pump, the folders it has not reached yet do not show meanwhile.
    Soon,
    Now,
}

/// What a pane went on showing while a new lens's first answers came.
struct KeptView {
    filter: Option<Arc<str>>,
    read: Option<Arc<ReadFilter>>,
    unproved: crate::hashing::Set<EntryId>,
    until: std::time::Instant,
}

impl Leading {
    /// Forget the folders' answers: the work under way ends with them.
    fn reset(&mut self) {
        self.cancel.cancel();
        *self = Leading::default();
    }

    /// Ask about every folder again, answers given standing until new ones
    /// come; what was still being looked into is looked into anew.
    fn start_again(&mut self) {
        self.cancel.cancel();
        self.cancel = CancelToken::new();
        self.answers = Arc::default();
        self.waiting.clear();
        self.jobs.clear();
        self.pass = None;
        self.renamed.clear();
        self.again = true;
    }

    /// The filter entries are shown by: the one kept while it is, or
    /// `text` as `read` tests it.
    fn filter<'a>(
        &'a self,
        text: Option<&'a str>,
        read: Option<&'a ReadFilter>,
    ) -> Option<TextFilter<'a>> {
        match &self.kept {
            Some(kept) => TextFilter::of(
                kept.filter.as_deref(),
                kept.read.as_deref(),
                &kept.unproved,
                None,
            ),
            None => TextFilter::of(
                text,
                read,
                &self.unproved,
                self.pass
                    .as_ref()
                    .is_some_and(|p| p.hiding == Hiding::Now)
                    .then_some(&self.asked),
            ),
        }
    }

    /// `id` is asked about by `ask`: what asked before it no longer has
    /// to answer it, and goes once it has nothing left to answer.
    fn asked_by(&mut self, id: EntryId, ask: u64) {
        let before = self.asked.insert(id, ask);
        if let Some(before) = before.filter(|before| *before != ask)
            && let Some(job) = self.jobs.get_mut(&before)
        {
            job.left = job.left.saturating_sub(1);
            if job.left == 0 {
                job.cancel.cancel();
                self.jobs.remove(&before);
            }
        }
    }

    /// A folder renamed keeps what was found of it: a new name is no new
    /// folder to prove. Its answers, still under its id, follow it.
    fn rekey(&mut self, renamed: &[(EntryId, EntryId)]) {
        for (before, after) in renamed {
            if let Some(ask) = self.asked.remove(before) {
                self.asked.insert(*after, ask);
                self.renamed.insert(*before, *after);
                if self.waiting.remove(before) {
                    self.waiting.insert(*after);
                }
                if self.unproved.remove(before) {
                    self.unproved.insert(*after);
                }
                // Renamed again: what was to come under the first id ends
                // up here too.
                for end in self.renamed.values_mut() {
                    if *end == *before {
                        *end = *after;
                    }
                }
            }
        }
    }

    /// An answer came for `id`, by `ask`: whether it is the one awaited.
    fn answered(&mut self, id: EntryId, ask: u64) -> bool {
        if self.asked.get(&id) != Some(&ask) {
            return false;
        }
        self.waiting.remove(&id);
        if let Some(job) = self.jobs.get_mut(&ask) {
            job.left = job.left.saturating_sub(1);
            if job.left == 0 {
                self.jobs.remove(&ask);
            }
        }
        true
    }
}

/// A folder, the ask that asked (`LeadsJob`), and whether it leads anywhere.
pub type LeadsAnswers = Arc<parking_lot::Mutex<Vec<(EntryId, u64, bool)>>>;

/// The folders on the way to what a plan changed, each with the folder
/// holding it and its name, once each: what a pane's lens may have
/// something new below (`Pane::ask_again_above`). A pane shows a folder by
/// the path it reached it at: changes named through another (a link's) ask
/// nothing there, as a search's check of a plan's changes.
pub(crate) struct FoldersAbove<'a> {
    by_folder: crate::hashing::Map<&'a Path, crate::hashing::Set<&'a OsStr>>,
}

impl<'a> FoldersAbove<'a> {
    pub(crate) fn of(changes: &'a [crate::op::Change]) -> Self {
        let mut by_folder: crate::hashing::Map<&Path, crate::hashing::Set<&OsStr>> =
            crate::hashing::Map::default();
        for path in changes.iter().flat_map(crate::op::Change::paths) {
            let mut at = path;
            while let (Some(parent), Some(name)) = (at.parent(), at.file_name()) {
                // Seen: so was every folder above it.
                if !by_folder.entry(parent).or_default().insert(name) {
                    break;
                }
                at = parent;
            }
        }
        FoldersAbove { by_folder }
    }
}

/// What decides which folders a pane has to ask about.
#[derive(Clone, Copy, PartialEq, Eq)]
struct LeadingKey {
    listing: (u64, u64),
    tree: u64,
    show_hidden: bool,
}

/// Folders to look into for what a pane's lens keeps
/// (`Pane::folders_to_ask`): whether each leads to something
/// (`registry::LeadsBelow`), answered by a worker.
#[derive(Debug)]
pub struct LeadsAsked {
    pub pane: PaneId,
    pub words: Arc<str>,
    pub folders: Vec<(EntryId, PathBuf)>,
    pub show_hidden: bool,
    pub answers: LeadsAnswers,
    /// What says which ask an answer is from.
    pub ask: u64,
    pub cancel: CancelToken,
    /// How long to wait first: a lens being typed changes at each key.
    pub wait: std::time::Duration,
    /// The view is kept until the first answers come (`VIEW_KEPT`).
    pub keeps_view: bool,
    /// Walked, an index's memory set aside: what changed is looked at
    /// (a plan's changes, F5).
    pub fresh: bool,
}

/// The filtered order, and what made it: what the filter keeps of every
/// entry listed, tested again only for those that arrived or changed since
/// (`Listing::updated_since`), so a narrowing that reads metadata tests a
/// batch of it rather than the whole folder at each batch.
struct FilterCache {
    /// The version of the filter's test it tested with.
    filter: u64,
    /// Each listing tested, the shown one's then the opened folders', by
    /// its id, with the version it was tested at.
    at: Vec<(u64, u64)>,
    /// The entries listed that the filter keeps, of every listing in `at`
    /// (with some that went since, until it is made again).
    kept: crate::hashing::Set<EntryId>,
    /// The folders it keeps that the lens does not keep by themselves:
    /// shown only as the way to what is below (`Pane::only_leads`), asked
    /// of the lens once per change of each, never per row drawn.
    dimmed: crate::hashing::Set<EntryId>,
    /// The order filtered (by identity), and what it kept of it.
    from: Arc<[EntryId]>,
    order: Arc<[EntryId]>,
    /// What it kept moved since `order` was made (answers about folders).
    stale: bool,
}

/// Test again, for `cache`, what arrived in `listings` or changed there
/// since it was made: whether what it keeps moved. `None` when it is not
/// theirs (another filter, other listings), when one no longer keeps what
/// changed that far back, or when it holds many more entries than are
/// listed (those that went since): it is made again.
fn retest(
    cache: &mut FilterCache,
    filter: u64,
    listings: &[&Listing],
    keeps: impl Fn(&Entry) -> bool,
    dims: impl Fn(&Entry) -> bool,
) -> Option<bool> {
    let listed: usize = listings.iter().map(|l| l.len()).sum();
    if cache.filter != filter
        || cache.kept.len() > 2 * listed + 1024
        || cache.at.len() != listings.len()
        || cache
            .at
            .iter()
            .zip(listings)
            .any(|((id, _), l)| *id != l.id())
    {
        return None;
    }
    let mut moved = false;
    for ((_, at), listing) in cache.at.iter_mut().zip(listings) {
        if *at == listing.version() {
            continue;
        }
        for id in listing.updated_since(*at)? {
            let entry = listing.get(id);
            moved |= if entry.is_some_and(&keeps) {
                cache.kept.insert(id)
            } else {
                cache.kept.remove(&id)
            };
            if entry.is_some_and(&dims) {
                cache.dimmed.insert(id);
            } else {
                cache.dimmed.remove(&id);
            }
        }
        *at = listing.version();
    }
    Some(moved)
}

/// What decides which entries a pane shows, metadata aside: the selection
/// is looked over again after a pump only once one of these moved
/// (`Pane::retain_changed`), or, while a filter reads metadata, for what
/// changed of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RetainKey {
    /// The listing shown, and its `shown_version`.
    listing: (u64, u64),
    tree: u64,
    filter: u64,
    show_hidden: bool,
    /// The selection as it was looked over.
    selection: u64,
    /// What a view lent (`Lent::version`).
    lent: u64,
}

/// The selection as last looked over (`Pane::retain_changed`): what decided
/// it, and each listing's version then (the shown one's first).
struct Retained {
    key: RetainKey,
    at: Vec<(u64, u64)>,
}

/// A pane's lens (ADR-107): what filters it that the listing can decide
/// of a file (a kind, an extension, a tag, a date, a size, a rating),
/// kept as the pane shows other locations until it is taken off (a
/// pill's ×, Escape). A folder, the trash or a tag's place is filtered
/// by it (`Pane::carry`); a search holds it in its own terms
/// (`LocationProvider::lensed`). What it means is its reader's (the query
/// plugin's): the core keeps its words and asks its test of each file.
/// A folder is shown once a search of it found what it keeps, so what is
/// under it can be reached (`Pane::folders_to_ask`).
#[derive(Clone)]
pub struct Lens {
    /// Its words, in its reader's language (`kind:image modified:2024`).
    pub words: Arc<str>,
    /// The facets it sets (`kind`, `modified`), which Quick filters do
    /// not offer again.
    pub set: Vec<String>,
    /// Whether a file is shown. One it cannot tell yet (its metadata is
    /// still to come) is not, until it can.
    pub keeps: Arc<dyn Fn(&Entry) -> bool + Send + Sync>,
    /// Whether it asks what the files' metadata says (a tag, a size): a
    /// kind or an extension is read from the name.
    pub metadata: bool,
    /// When its words read otherwise (`modified:today` at midnight): it
    /// is read again then (`App::read_filters_again`).
    pub next_change: Option<std::time::SystemTime>,
}

impl std::fmt::Debug for Lens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lens")
            .field("words", &self.words)
            .finish_non_exhaustive()
    }
}

impl Lens {
    /// Whether `entry` is shown: a folder always is.
    pub fn shows(&self, entry: &Entry) -> bool {
        entry.is_dir() || (self.keeps)(entry)
    }
}

/// What of the pane's lens a location whose own terms hold one (a search)
/// is asked to take (`LocationProvider::lensed`, ADR-107).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bring {
    /// The pane's lens, added to its terms: a facet they set is their own.
    Added,
    /// Nothing: its terms say its lens whole (typed, kept, or a step of
    /// the history shown again as it was, ADR-109).
    Nothing,
}

/// How a location takes the lens a pane brings into it
/// (`LocationProvider::lensed`).
#[derive(Clone, Debug)]
pub enum Lensed {
    /// The pane filters what it lists by the lens (a folder, the trash, a
    /// tag's place): the lens's words are its filter.
    Filters(Option<Lens>),
    /// The location's own terms hold it (a search): the pane filters
    /// nothing, and this is its lens.
    Holds(Option<Lens>),
    /// A location that holds none, shown as it was left or kept
    /// (ADR-109): filtered by that text as its reader reads it (`None`:
    /// names), or by nothing.
    AsLeft(Option<(Arc<str>, Option<ReadFilter>)>),
}

/// What filtered a step of a pane's history when the pane left it
/// (`Pane::filter_at`, ADR-109): its text, and whether its reader read a
/// lens in it, which only that reader can tell: with no reader, a filter
/// that held one shows nothing, one of names its names (ADR-107 rule 9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepFilter {
    pub text: Arc<str>,
    pub lens: bool,
}

/// What the pane had at a step of its history when it left it.
#[derive(Clone, Debug, Default)]
pub(crate) struct StepLeft {
    /// The entry the cursor was on.
    cursor: Option<EntryId>,
    /// What filtered it: a search's own terms say the rest.
    filter: Option<StepFilter>,
}

impl StepLeft {
    pub(crate) fn filter(&self) -> Option<&StepFilter> {
        self.filter.as_ref()
    }
}

/// What words typed over a folder keep, as the filter's reader read them
/// (ADR-098, the query plugin's): a test of what the listing knows, never
/// of the file. A word finds names, of folders too; any other term holds
/// for files, and a folder shows for what a search of it finds, as a lens
/// keeps them.
#[derive(Clone)]
pub struct ReadFilter {
    /// Whether an entry is shown. A file it cannot tell yet (its metadata
    /// is still to come) is not, until it can.
    pub keeps: Arc<dyn Fn(&Entry) -> bool + Send + Sync>,
    /// What only a search can tell (what files contain, where they were
    /// taken), left out until Enter: said where the count is.
    pub waits: Vec<String>,
    /// Whether it asks what the files' metadata says: the pane asks for
    /// every file's.
    pub metadata: bool,
    /// What of it is a lens (ADR-107), kept when the pane shows another
    /// location; the rest (words that find names) goes.
    pub lens: Option<Lens>,
    /// When its words read otherwise (`modified:today` at midnight): it
    /// is read again then (`App::read_filters_again`).
    pub next_change: Option<std::time::SystemTime>,
}

impl std::fmt::Debug for ReadFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadFilter")
            .field("waits", &self.waits)
            .field("lens", &self.lens)
            .finish_non_exhaustive()
    }
}

impl ReadFilter {
    /// The filter a lens is alone: what a pane brings into a location
    /// that does not hold it.
    pub fn of_lens(lens: Lens) -> Self {
        let keeps = Arc::clone(&lens.keeps);
        ReadFilter {
            keeps: Arc::new(move |entry| entry.is_dir() || keeps(entry)),
            waits: Vec::new(),
            metadata: lens.metadata,
            next_change: lens.next_change,
            lens: Some(lens),
        }
    }
}

/// Whether a pane shows an entry, by its id: listed by `listing` or one
/// opened in place (`kept`, `expanded`), shown by its listing (hidden
/// files, companions), and kept by the filter (its lens with it).
fn shows<'a>(
    listing: &'a Listing,
    kept: &'a [Kept],
    expanded: &'a [Opened],
    lent: &'a Lent,
    filter: Option<TextFilter<'a>>,
    sort: SortSpec,
) -> impl Fn(EntryId) -> bool + 'a {
    let top = listing.shows(sort);
    let opened: Vec<_> = opened(kept, expanded)
        .map(|(_, k)| (&k.listing, k.listing.shows(sort)))
        .collect();
    move |id| {
        let found = std::iter::once((listing, &top as &dyn Fn(EntryId) -> bool))
            .chain(
                opened
                    .iter()
                    .map(|(l, shows)| (*l, shows as &dyn Fn(EntryId) -> bool)),
            )
            .find_map(|(l, shows)| l.get(id).map(|e| (e, shows(id))));
        match found {
            Some((e, true)) => filter.as_ref().is_none_or(|f| f.keeps(e)),
            Some((_, false)) => false,
            // What a view draws is shown while it lends it, whatever
            // filters the listing: the view drew it.
            None => lent.entries.contains_key(&id),
        }
    }
}

/// An entry a view lends the pane ([`Pane::lend`], ADR-116): the entry as
/// the view knows it, where it is, and what the view saw of it.
#[derive(Clone, Debug)]
pub struct Loaned {
    pub entry: Entry,
    pub path: PathBuf,
    /// What the view saw of it as it drew it (its size and its last
    /// change, say), as one number: lent again as it was seen, it keeps
    /// what was read of it; seen otherwise, it is read again once
    /// selected.
    pub seen: u64,
}

/// Entries a view draws beside what the pane lists, lent while it draws
/// them (ADR-116): each by its id, with where it is.
#[derive(Default)]
struct Lent {
    /// The view that lent them (`ViewSpec::id`): they go with it, its
    /// plugin turned off (`App::unregister_owner`).
    by: Option<Arc<str>>,
    entries: crate::hashing::Map<EntryId, Loan>,
    /// The entries lent of each file: what [`Pane::entry_at`] finds one
    /// by, with its path.
    of_file: crate::hashing::Map<crate::FileId, Vec<EntryId>>,
    /// Changes at each lending that changes what is lent and as the pane
    /// lets go of it, never as what is lent is read
    /// (`Pane::lent_version`): never back to a version a view was told.
    version: u64,
}

/// An entry lent, where it is, what the view saw of it, and whether it
/// was read since it was lent as seen: one lent again as it was seen
/// keeps its reading; one seen otherwise keeps what was read of it, read
/// again once selected.
struct Loan {
    entry: Entry,
    path: PathBuf,
    seen: u64,
    read: bool,
    /// The version (`Lent::version`) it was last to be read since: a
    /// reading asked before it is of an entry seen otherwise, and is not
    /// taken as its own.
    since: u64,
}

impl Lent {
    /// Nothing lent any more: another location shown, the pane dormant.
    fn let_go(&mut self) {
        self.by = None;
        self.entries.clear();
        self.of_file.clear();
        self.version += 1;
    }

    /// Index what is lent by file, for `Pane::entry_at`.
    fn index(&mut self) {
        self.of_file.clear();
        for id in self.entries.keys() {
            self.of_file.entry(id.file).or_default().push(*id);
        }
    }

    /// `id` no longer lent (found gone).
    fn remove(&mut self, id: EntryId) -> bool {
        if self.entries.remove(&id).is_none() {
            return false;
        }
        if let Some(ids) = self.of_file.get_mut(&id.file) {
            ids.retain(|other| *other != id);
            if ids.is_empty() {
                self.of_file.remove(&id.file);
            }
        }
        true
    }
}

/// The entries lent before that went (`gone`), each with the one lent in
/// its place among those that came, as a listing pairs them
/// (`ListingEvent::Renamed`): a file of one name gone under it and come
/// under another (renamed, or moved within what the view draws), else a
/// name another file took (an editor saving writes a new file and renames
/// it onto the name). What the pane keeps by entry follows each pair.
fn lent_renamed(gone: impl Iterator<Item = EntryId>, came: &[EntryId]) -> Vec<(EntryId, EntryId)> {
    use crate::hashing::{Map, Set};
    // Each file by how many of its names went or came, and the last.
    let count = |ids: &mut dyn Iterator<Item = EntryId>| {
        let mut files: Map<crate::FileId, (usize, EntryId)> = Map::default();
        for id in ids {
            let seen = files.entry(id.file).or_insert((0, id));
            seen.0 += 1;
            seen.1 = id;
        }
        files
    };
    let gone: Vec<EntryId> = gone.collect();
    let went = count(&mut gone.iter().copied());
    let arrived = count(&mut came.iter().copied());
    let mut renamed = Vec::new();
    let mut paired: Set<EntryId> = Set::default();
    for &new in came {
        if let (Some((1, old)), Some((1, _))) = (went.get(&new.file), arrived.get(&new.file)) {
            renamed.push((*old, new));
            paired.insert(*old);
            paired.insert(new);
        }
    }
    let mut named: Map<u64, EntryId> = gone
        .iter()
        .filter(|id| !paired.contains(id))
        .map(|id| (id.at, *id))
        .collect();
    for &new in came.iter().filter(|id| !paired.contains(id)) {
        if let Some(old) = named.remove(&new.at) {
            renamed.push((old, new));
        }
    }
    renamed
}

/// The pane's filter as it is asked of each entry: its reader's test, or
/// names that hold the text.
enum TextFilter<'a> {
    Names(NameFilter<'a>),
    /// Its reader's test, the folders not found to lead anywhere, and,
    /// while a first look over the rows takes more than a pump, those it
    /// has reached: a folder it has not does not show yet.
    Read(
        &'a ReadFilter,
        &'a crate::hashing::Set<EntryId>,
        Option<&'a crate::hashing::Map<EntryId, u64>>,
    ),
}

impl<'a> TextFilter<'a> {
    fn of(
        text: Option<&'a str>,
        read: Option<&'a ReadFilter>,
        unproved: &'a crate::hashing::Set<EntryId>,
        reached: Option<&'a crate::hashing::Map<EntryId, u64>>,
    ) -> Option<Self> {
        match (text, read) {
            (Some(_), Some(read)) => Some(TextFilter::Read(read, unproved, reached)),
            (Some(text), None) => Some(TextFilter::Names(NameFilter::new(text))),
            (None, _) => None,
        }
    }

    fn keeps(&self, entry: &Entry) -> bool {
        match self {
            TextFilter::Names(names) => names.holds(&entry.name),
            TextFilter::Read(read, unproved, reached) => {
                let waits = unproved.contains(&entry.id)
                    || reached.is_some_and(|reached| {
                        entry.kind == crate::entry::EntryKind::Directory
                            && !entry.is_place()
                            && !reached.contains_key(&entry.id)
                    });
                (read.keeps)(entry)
                    && (!waits || read.lens.as_ref().is_none_or(|lens| (lens.keeps)(entry)))
            }
        }
    }
}

/// Whether folder `entry` is shown only as the way to what `lens` keeps.
fn dimmed(lens: &Lens, entry: &Entry) -> bool {
    entry.is_dir() && !entry.is_place() && !(lens.keeps)(entry)
}

/// Whether the pane shows `entry`: the filter keeps it.
fn shown(filter: Option<&TextFilter>, entry: &Entry) -> bool {
    filter.is_none_or(|f| f.keeps(entry))
}

/// Whether `name` holds `filter`: anywhere in it, case ignored unless the
/// filter has a capital. For many names, make a [`NameFilter`] once.
pub fn name_holds(name: &OsStr, filter: &str) -> bool {
    NameFilter::new(filter).holds(name)
}

/// A name filter made once for the names it is asked about (every entry of
/// a folder at each keystroke): its lowercase form, and whether it can be
/// compared byte by byte. A name is then asked without an allocation,
/// unless it is not ASCII and case is ignored.
pub struct NameFilter<'a> {
    text: &'a str,
    /// The filter lowercased, when case is ignored (it has no capital).
    lower: Option<String>,
}

impl<'a> NameFilter<'a> {
    pub fn new(text: &'a str) -> Self {
        NameFilter {
            text,
            lower: (!text.chars().any(char::is_uppercase)).then(|| text.to_lowercase()),
        }
    }

    /// Whether `name` holds it: anywhere in it, case ignored unless it has
    /// a capital.
    pub fn holds(&self, name: &OsStr) -> bool {
        let Some(lower) = &self.lower else {
            return name.to_string_lossy().contains(self.text);
        };
        let bytes = name.as_encoded_bytes();
        // ASCII both: compared byte by byte, case folded as it goes.
        if lower.is_ascii() && bytes.is_ascii() {
            let needle = lower.as_bytes();
            return needle.is_empty()
                || bytes
                    .windows(needle.len())
                    .any(|window| window.eq_ignore_ascii_case(needle));
        }
        name.to_string_lossy()
            .to_lowercase()
            .contains(lower.as_str())
    }
}

impl Pane {
    fn new(id: PaneId) -> Self {
        Pane {
            id,
            open: None,
            location_lifetime: CancelToken::new(),
            listed_hidden: None,
            listing: Listing::default(),
            kept: Vec::new(),
            left: None,
            drawn_left: None,
            selection: Selection::default(),
            sort: SortSpec::default(),
            own: None,
            place_view: None,
            keys: None,
            keys_for: None,
            history: Vec::new(),
            history_pos: 0,
            left_at: Vec::new(),
            reveal: None,
            reveal_looked: None,
            opened_index: parking_lot::Mutex::new(None),
            restore: None,
            expanded: Vec::new(),
            expanded_version: 0,
            lent: Lent::default(),
            tree_version: 0,
            tree_members_version: 0,
            retained: None,
            order_cache: parking_lot::Mutex::new(None),
            groups_cache: parking_lot::Mutex::new(None),
            folds: Folds::default(),
            fold_cache: parking_lot::Mutex::new(None),
            folds_drawn: true,
            fields_asked: Vec::new(),
            states_asked: None,
            states_reading: None,
            states_asking: 0,
            states_said: None,
            fields_shown: Vec::new(),
            shown: parking_lot::Mutex::new(None),
            filter: None,
            read: None,
            filter_version: 0,
            filter_tested: 0,
            lens: None,
            asked_by: None,
            filter_cache: parking_lot::Mutex::new(None),
            leading: Leading::default(),
            filter_replaced_at: None,
            found: FoundTree::default(),
        }
    }

    /// A pane that will show `location` when its tab is shown: dormant
    /// until then, with a history of that one location.
    fn dormant(id: PaneId, location: Location, sort: SortSpec) -> Self {
        let mut pane = Pane::new(id);
        pane.history.push(location);
        pane.sort = sort;
        pane.asked_by = crate::permissions::agent_call();
        pane
    }

    /// The agent's call its location waits to be opened as, once: see
    /// `asked_by`.
    pub(crate) fn take_asked_by(&mut self) -> Option<crate::permissions::Privacy> {
        self.asked_by.take()
    }

    /// Put back what `take_asked_by` took, the opening having failed.
    pub(crate) fn set_asked_by(&mut self, asked_by: Option<crate::permissions::Privacy>) {
        self.asked_by = asked_by;
    }

    /// Go on from the current step of `history` to `location`: the steps
    /// ahead go, and `left`, the entry the cursor was on, is kept for Back
    /// to select again.
    pub(crate) fn go_on(&mut self, location: Location, left: Option<EntryId>) {
        if !self.history.is_empty() {
            self.remember_left(self.history_pos, left);
            self.history.truncate(self.history_pos + 1);
        }
        self.left_at.truncate(self.history.len());
        self.history.push(location);
        self.history_pos = self.history.len() - 1;
    }

    /// Put `location` in place of the current step of `history`: where
    /// the location shown said it is reached (`ListingEvent::Redirect`).
    pub(crate) fn replace_here(&mut self, location: Location) {
        if let Some(step) = self.history.get_mut(self.history_pos) {
            *step = location;
        } else {
            self.history.push(location);
            self.history_pos = self.history.len() - 1;
        }
    }

    /// Take step `from` of `history` back, as if it had never been gone
    /// to: the step before, `location`, the one now shown, is current
    /// again, and the one taken back goes with those ahead of it. The entry
    /// the cursor was on when that step was left is selected again once the
    /// listing has it. A history that changed since `from` was read (a
    /// navigation from elsewhere meanwhile, an agent's) no longer has it
    /// there: what is shown is a step of its own, `left` kept for the step
    /// left, as `go_to` does.
    pub(crate) fn take_step_back(
        &mut self,
        from: usize,
        location: &Location,
        left: Option<EntryId>,
    ) {
        let Some(before) = from.checked_sub(1) else {
            return;
        };
        if self.history_pos != from || self.history.get(before) != Some(location) {
            self.go_on(location.clone(), left);
            return;
        }
        self.history.truncate(self.history_pos);
        self.left_at.truncate(self.history.len());
        self.history_pos = before;
        let again = self.left_at.get(before).and_then(|s| s.cursor).map(|id| {
            let mut selection = Selection::default();
            selection.set([id]);
            selection
        });
        self.set_restore(again);
    }

    /// Go to step `pos` of `history` (Back, Forward), which is `location`,
    /// the one now shown: `left`, the entry the cursor was on, is kept for
    /// the step left, and the entry the cursor was on when `pos` was left
    /// is selected again once the listing has it, as a link followed and
    /// gone back from expects. A history that changed since `pos` was read
    /// (a navigation from elsewhere meanwhile, an agent's) no longer has it
    /// there: what is shown is a step of its own.
    pub(crate) fn go_to(&mut self, pos: usize, location: &Location, left: Option<EntryId>) {
        if self.history.get(pos) != Some(location) {
            self.go_on(location.clone(), left);
            return;
        }
        self.remember_left(self.history_pos, left);
        self.history_pos = pos;
        let again = self.left_at.get(pos).and_then(|s| s.cursor).map(|id| {
            let mut selection = Selection::default();
            selection.set([id]);
            selection
        });
        self.set_restore(again);
    }

    fn remember_left(&mut self, pos: usize, left: Option<EntryId>) {
        self.left_at_mut(pos).cursor = left;
    }

    fn left_at_mut(&mut self, pos: usize) -> &mut StepLeft {
        if self.left_at.len() <= pos {
            self.left_at.resize(pos + 1, StepLeft::default());
        }
        &mut self.left_at[pos]
    }

    /// What filters the step shown, as Back and Forward show it again
    /// (ADR-109): its filter's text, and whether its reader read a lens in
    /// it. A search's own terms say the rest: `None` over one.
    fn step_filter(&self) -> Option<StepFilter> {
        self.filter.as_ref().map(|text| StepFilter {
            text: Arc::clone(text),
            lens: self.read.as_ref().is_some_and(|r| r.lens.is_some()),
        })
    }

    /// What filtered step `pos` of `history` when the pane left it, or
    /// filters it now for the step shown (ADR-109).
    pub fn filter_at(&self, pos: usize) -> Option<StepFilter> {
        if pos == self.history_pos {
            return self.step_filter();
        }
        self.left_at.get(pos).and_then(|s| s.filter.clone())
    }

    /// The step shown is left for another location: what filters it is
    /// kept for Back and Forward (ADR-109). Asked as every location is
    /// shown, before it takes the lens (`App::open_in`).
    pub(crate) fn leave_step(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let filter = self.step_filter();
        self.left_at_mut(self.history_pos).filter = filter;
    }

    /// What every step had when left, the step shown's filter as it is
    /// now: what a tab closed keeps (`App::close_tab`).
    pub(crate) fn steps_left(&self) -> Vec<StepLeft> {
        let mut steps = self.left_at.clone();
        steps.resize(self.history.len(), StepLeft::default());
        if let Some(here) = steps.get_mut(self.history_pos) {
            here.filter = self.step_filter();
        }
        steps
    }

    /// Give a pane what its steps had when left (`steps_left`): a tab
    /// closed, opened again. The step shown's filter is the caller's to
    /// set, read by its reader (`filter_as_left`).
    pub(crate) fn set_steps_left(&mut self, steps: Vec<StepLeft>) {
        self.left_at = steps;
    }

    /// Work tied to this location stops immediately on navigation or pane close.
    pub fn location_lifetime(&self) -> CancelToken {
        self.location_lifetime.child()
    }

    /// Show `open` with `listing` (a new one, or a kept one going on as it
    /// was), and the lens as it takes it (`lensed`, ADR-107). The location
    /// shown before waits in `left` for the view to keep it or let it go
    /// (`App::keep_locations`); whatever waited there goes.
    pub(crate) fn replace_open(
        &mut self,
        open: Arc<dyn OpenLocation>,
        listing: Listing,
        lensed: Lensed,
    ) {
        self.location_lifetime.cancel();
        self.location_lifetime = CancelToken::new();
        let old_listing = std::mem::replace(&mut self.listing, listing);
        self.drawn_left = None;
        // The folder left keeps the order it was shown in, if it remembers
        // one: as a column, it does not move.
        let order = self.keeps_order().then(|| Order::of(self.sort));
        self.left = self
            .open
            .replace(open)
            .map(|old| Kept::new(old, old_listing, order));
        // Nothing of the old location survives, the cursor included.
        self.selection = Selection::default();
        self.reveal = None;
        self.restore = None;
        if !self.expanded.is_empty() {
            self.expanded.clear();
            self.expanded_changed();
        }
        self.lent.let_go();
        // Each folder listed is asked about anew, under the lens it takes.
        self.leading.reset();
        self.found.reset();
        // The words that found names go; the lens stays, as a filter or
        // in the location's own terms.
        self.take_lens(lensed);
    }

    /// The lens as the location shown takes it: its filter, or its own.
    fn take_lens(&mut self, lensed: Lensed) {
        match lensed {
            Lensed::Filters(lens) => self.carry(lens),
            Lensed::Holds(lens) => {
                self.set_filter(None, None);
                self.lens = lens;
            }
            Lensed::AsLeft(filter) => self.filter_as_left(filter),
        }
    }

    /// Filter the pane by what filtered a step when it was left, or a
    /// place when it was kept (ADR-109): `text` as `read` tests it, else
    /// by its names (`None`: show everything). Its test is the one given,
    /// even for the text the pane filters by already, as `carry` reads a
    /// lens: read again where it is shown, a date says the day it is
    /// read on.
    pub(crate) fn filter_as_left(&mut self, filter: Option<(Arc<str>, Option<ReadFilter>)>) {
        let Some((text, read)) = filter else {
            self.carry(None);
            return;
        };
        self.filter = Some(text);
        self.read = read.map(Arc::new);
        self.lens = self.read.as_ref().and_then(|r| r.lens.clone());
        self.filter_changed();
    }

    /// Filter the pane by `lens` alone (`None`: show everything): what a
    /// location that does not hold a lens is shown with, a tab or a split
    /// opened from a pane with that pane's. Its test is the one given,
    /// even for the words the pane filters by already: read again where a
    /// location is shown, a date says the day it is read on.
    pub(crate) fn carry(&mut self, lens: Option<Lens>) {
        let Some(lens) = lens else {
            self.set_filter(None, None);
            // One a location held (a search's) goes too.
            self.lens = None;
            return;
        };
        self.filter = Some(Arc::clone(&lens.words));
        self.read = Some(Arc::new(ReadFilter::of_lens(lens)));
        self.lens = self.read.as_ref().and_then(|r| r.lens.clone());
        self.filter_changed();
    }

    /// Open the location a dormant pane shows again (`suspend`): what it
    /// kept while it slept stays (the folders opened in place, which the
    /// caller opens again, the filter), the selection comes back through
    /// `set_restore`; a location holding its lens (a search) holds it again.
    pub(crate) fn reopen(&mut self, open: Arc<dyn OpenLocation>, listing: Listing, lensed: Lensed) {
        self.location_lifetime.cancel();
        self.location_lifetime = CancelToken::new();
        self.listing = listing;
        self.open = Some(open);
        self.left = None;
        self.drawn_left = None;
        self.selection = Selection::default();
        self.reveal = None;
        self.restore = None;
        if !self.expanded.is_empty() {
            self.expanded_changed();
        }
        self.leading.reset();
        self.found.reset();
        if let Lensed::Holds(_) = lensed {
            self.take_lens(lensed);
        }
    }

    /// What the switch said as a location that follows it was opened.
    pub(crate) fn set_listed_hidden(&mut self, listed: Option<bool>) {
        self.listed_hidden = listed;
    }

    /// The location to open again because the switch moved since it was
    /// opened: asked once, so one that cannot open is not asked for again.
    fn take_listed_for_other_hidden(&mut self) -> Option<Location> {
        let moved = self.open.is_some()
            && self
                .listed_hidden
                .is_some_and(|listed| listed != self.sort.show_hidden);
        if !moved {
            return None;
        }
        self.listed_hidden = Some(self.sort.show_hidden);
        self.location().cloned()
    }

    /// What the pane's names must hold to be shown, if anything.
    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    /// Changes with the filter: a view keys what it drew on it beside the
    /// listing's version.
    pub fn filter_version(&self) -> u64 {
        self.filter_version
    }

    /// The filter as its reader read it: its parts, what waits for a
    /// search; `None` while names are filtered by the text, or nothing is.
    pub fn read_filter(&self) -> Option<&ReadFilter> {
        self.read.as_deref()
    }

    /// Show only the entries `text` keeps (empty or `None`: all), as
    /// `read` tests them, else those whose name holds it. What is selected
    /// and no longer shown is deselected, as a hidden file is: a Delete
    /// must not take what cannot be seen.
    pub(crate) fn set_filter(&mut self, text: Option<&str>, read: Option<ReadFilter>) {
        let text = text.map(str::trim).filter(|t| !t.is_empty());
        if self.filter.as_deref() == text {
            return;
        }
        let before = (self.filter.take(), self.read.take());
        self.filter = text.map(Arc::from);
        self.read = text.and(read).map(Arc::new);
        self.lens = self.read.as_ref().and_then(|r| r.lens.clone());
        self.filter_replaced(before);
    }

    /// The filter's text read again by `read`, the reader there is now
    /// (a plugin that reads filters turned on or off): `None` filters by
    /// names, its text kept, unless the pane has a lens, which only its
    /// reader could tell: then nothing is filtered, and a lens the location
    /// held goes too.
    pub(crate) fn read_filter_again(&mut self, read: Option<ReadFilter>) {
        if read.is_none() && self.lens.is_some() {
            self.lens = None;
            if self.filter.take().is_some() {
                self.read = None;
                self.filter_changed();
            }
            return;
        }
        if self.filter.is_none() {
            return;
        }
        let before = (self.filter.clone(), self.read.take());
        self.read = read.map(Arc::new);
        self.lens = self.read.as_ref().and_then(|r| r.lens.clone());
        self.filter_replaced(before);
    }

    /// The pane's lens (ADR-107), if it has one: its filter's, or the one
    /// the location shown holds (a search's terms).
    pub fn lens(&self) -> Option<&Lens> {
        self.lens.as_ref()
    }

    /// Whether some of the entries are not shown: the pane is filtered.
    pub fn is_filtered(&self) -> bool {
        self.filter.is_some()
    }

    /// The filter as each entry is asked of it.
    fn text_filter(&self) -> Option<TextFilter<'_>> {
        self.leading
            .filter(self.filter.as_deref(), self.read.as_deref())
    }

    fn filter_changed(&mut self) {
        self.leading.reset();
        self.filter_test_moved();
        self.retain_shown();
    }

    /// What decides whether an entry is shown moved: what was kept of
    /// the entries is made again.
    fn filter_test_moved(&mut self) {
        self.filter_version += 1;
        self.filter_tested += 1;
    }

    /// The filter of the location shown replaced `before`: what the pane
    /// shows stays as it was for a moment when folders are to be looked
    /// into, so a lens lands whole. A lens changing again at once is
    /// being typed: it waits for the keys to stop, and the view with it.
    fn filter_replaced(&mut self, before: (Option<Arc<str>>, Option<Arc<ReadFilter>>)) {
        let now = std::time::Instant::now();
        let typed = self
            .filter_replaced_at
            .replace(now)
            .is_some_and(|at| now.duration_since(at) < TYPED_LENS_WAITS);
        let was_kept = self.leading.kept.take();
        let unproved = std::mem::take(&mut self.leading.unproved);
        let kept = self.may_look_below().then(|| {
            let wait = if typed {
                TYPED_LENS_WAITS
            } else {
                std::time::Duration::ZERO
            };
            let until = now + wait + VIEW_KEPT;
            match was_kept {
                Some(kept) => KeptView { until, ..kept },
                None => KeptView {
                    filter: before.0,
                    read: before.1,
                    unproved,
                    until,
                },
            }
        });
        self.leading.reset();
        self.leading.kept = kept;
        self.leading.typed = typed;
        self.filter_test_moved();
        self.retain_shown();
    }

    /// Whether the lens may have folders here to look into. Quick to ask:
    /// a pane with none filters at once.
    fn may_look_below(&self) -> bool {
        self.read.as_ref().is_some_and(|r| r.lens.is_some())
            && self.open.is_some()
            && !self.listing.is_held()
            && self.location().is_some_and(Location::is_path)
            && self.listings().any(|listing| {
                listing
                    .iter()
                    .any(|e| e.kind == crate::entry::EntryKind::Directory)
            })
    }

    /// The folders of the pane's lens still to be asked about: those it
    /// lists, listed by a folder, each once, and not shown until a search
    /// found something in them; one opened in place stands until a search
    /// finds nothing in it, or what is opened under it would stand where
    /// it is not. A place (a tag's), the trash or a search has no folders to
    /// look into; a link is never gone through; a remote folder is never
    /// walked unasked (ADR-077): each of those stands. Looked over at most
    /// `PUMP_BUDGET` at a time. Asked again after a change
    /// (`ask_again_above`, `ask_about_folders_again`, hidden files shown),
    /// what is shown staying until the answers come; what a name filters
    /// out is not asked about at all.
    fn folders_to_ask(&mut self) -> Option<LeadsAsked> {
        let read = Arc::clone(self.read.as_ref()?);
        let lens = read.lens.clone()?;
        if self.open.is_none() || self.listing.is_held() {
            return None;
        }
        // A search, the trash, a tag's place: none of its rows is a folder
        // to look into.
        if self.expanded.is_empty() && !self.location().is_some_and(Location::is_path) {
            return None;
        }
        if self.leading.pass.is_none() {
            self.leading.pass = self.look_over_rows();
        }
        let mut pass = self.leading.pass.take()?;
        let deadline = std::time::Instant::now() + PUMP_BUDGET;
        let mut looked = 0_usize;
        while let Some(&id) = pass.rows.get(pass.next) {
            // Asking the clock costs as much as a row.
            looked += 1;
            if looked.is_multiple_of(64) && std::time::Instant::now() >= deadline {
                break;
            }
            pass.next += 1;
            self.ask_of_row(id, &read, &mut pass);
        }
        if pass.next < pass.rows.len() {
            if pass.hiding == Hiding::Soon {
                pass.hiding = Hiding::Now;
                self.filter_test_moved();
            }
            self.leading.pass = Some(pass);
            return None;
        }
        // What it kept from showing shows: a row it passed over without a
        // question (a folder no folder lists) included.
        if pass.hiding == Hiding::Now {
            self.filter_test_moved();
        }
        if pass.folders.is_empty() {
            return None;
        }
        let cancel = self.leading.cancel.child();
        self.leading.jobs.insert(
            pass.ask,
            LeadsJob {
                cancel: cancel.clone(),
                left: pass.folders.len(),
            },
        );
        let wait = if std::mem::take(&mut self.leading.typed) {
            TYPED_LENS_WAITS
        } else {
            std::time::Duration::ZERO
        };
        Some(LeadsAsked {
            pane: self.id,
            words: lens.words,
            folders: pass.folders,
            show_hidden: self.sort.show_hidden,
            answers: Arc::clone(&self.leading.answers),
            ask: pass.ask,
            cancel,
            wait,
            keeps_view: self.leading.kept.is_some(),
            fresh: pass.fresh,
        })
    }

    /// What of the rows is to be looked over, if anything moved since they
    /// last were: what came since, unless every folder is asked again or
    /// the listings are others than those looked at.
    fn look_over_rows(&mut self) -> Option<LeadsPass> {
        let key = LeadingKey {
            listing: (self.listing.id(), self.listing.names_version()),
            tree: self.tree_members_version,
            show_hidden: self.sort.show_hidden,
        };
        let looked = self.leading.looked.replace(key);
        if looked.is_some_and(|before| before.show_hidden != key.show_hidden) {
            self.leading.start_again();
        }
        let again = std::mem::take(&mut self.leading.again);
        let named = std::mem::take(&mut self.leading.again_of);
        if looked == Some(key) && !again && named.is_empty() {
            return None;
        }
        let fresh = std::mem::take(&mut self.leading.fresh);
        let at = self.listings_at();
        let before = std::mem::replace(&mut self.leading.at, at.clone());
        let same_listings = !again
            && looked.is_some()
            && before.len() == at.len()
            && before.iter().zip(&at).all(|(a, b)| a.0 == b.0);
        let arrived = || -> Option<Vec<EntryId>> {
            let mut rows = Vec::new();
            for (listing, (_, then)) in self.listings().zip(&before) {
                rows.extend(listing.updated_since(*then)?);
            }
            Some(rows)
        };
        let rows: Arc<[EntryId]> = match same_listings.then(arrived).flatten() {
            Some(mut rows) => {
                rows.extend(named.iter().copied());
                rows.into()
            }
            None => self.unfiltered_order(),
        };
        self.leading.asks += 1;
        let opened = self.expanded.iter().map(|o| o.at).collect();
        Some(LeadsPass {
            rows,
            next: 0,
            ask: self.leading.asks,
            again,
            named,
            opened,
            folders: Vec::new(),
            hiding: if looked.is_none() {
                Hiding::Soon
            } else {
                Hiding::Never
            },
            fresh,
        })
    }

    /// Whether row `id` is a folder to ask about, in `pass`.
    fn ask_of_row(&mut self, id: EntryId, read: &ReadFilter, pass: &mut LeadsPass) {
        let Some(entry) = self.entry(id) else {
            return;
        };
        let first = !self.leading.asked.contains_key(&id);
        if entry.kind != crate::entry::EntryKind::Directory
            || entry.is_place()
            || !(first || pass.again || pass.named.contains(&id))
            || !(read.keeps)(entry)
            || !self.listed_by_a_folder(id)
        {
            return;
        }
        let Some(path) = self.path_of(id) else {
            return;
        };
        self.leading.asked_by(id, pass.ask);
        let stands = !crate::fs::reach::lane(&path).is_shared();
        if !stands {
            if first && !pass.opened.contains(&id) {
                self.leading.unproved.insert(id);
            }
            self.leading.waiting.insert(id);
            pass.folders.push((id, path));
        }
        if first {
            self.leading.moved.push(id);
        }
    }

    /// Whether the listing holding `id` is a folder's, on disk.
    fn listed_by_a_folder(&self, id: EntryId) -> bool {
        self.holder(id).is_some_and(Location::is_path)
    }

    /// Take the answers that came about the folders: whether what the
    /// pane shows changed. What moved is applied in what the filter kept,
    /// so nothing but the folders named is tested again.
    fn take_leads(&mut self) -> bool {
        let answers = std::mem::take(&mut *self.leading.answers.lock());
        let mut moved = std::mem::take(&mut self.leading.moved);
        for (id, ask, leads) in answers {
            let id = self.leading.renamed.get(&id).copied().unwrap_or(id);
            if !self.leading.answered(id, ask) {
                continue;
            }
            let changed = if leads {
                self.leading.unproved.remove(&id)
            } else {
                self.leading.unproved.insert(id)
            };
            if changed {
                moved.push(id);
            }
        }
        if self.leading.waiting.is_empty() {
            self.leading.renamed.clear();
        }
        if let Some(kept) = &self.leading.kept {
            let answered = self.leading.waiting.is_empty()
                && self.leading.pass.is_none()
                && self.leading.looked.is_some();
            if !answered && std::time::Instant::now() < kept.until {
                return false;
            }
            self.leading.kept = None;
            self.filter_test_moved();
            self.retain_shown();
            return true;
        }
        if moved.is_empty() {
            return false;
        }
        self.folders_moved(&moved);
        true
    }

    /// `moved`, folders, show or no longer do: what the filter kept is told
    /// so, and nothing else is tested again; what is selected of them and
    /// no longer shown leaves the selection.
    fn folders_moved(&mut self, moved: &[EntryId]) {
        self.filter_version += 1;
        let filter = self.text_filter();
        let filter = filter.as_ref();
        if let Some(cache) = self
            .filter_cache
            .lock()
            .as_mut()
            .filter(|cache| cache.filter == self.filter_tested)
        {
            for id in moved {
                if self.entry(*id).is_some_and(|e| shown(filter, e)) {
                    cache.kept.insert(*id);
                } else {
                    cache.kept.remove(id);
                }
            }
            cache.stale = true;
        }
        if moved
            .iter()
            .any(|id| self.selection.contains(*id) && self.leading.unproved.contains(id))
        {
            self.retain_shown();
        }
    }

    /// Something changed files, or the pane was asked to look again (F5):
    /// every folder is asked about again, from the files themselves, what
    /// is shown staying as it is until the answers come.
    pub(crate) fn ask_about_folders_again(&mut self) {
        self.leading.start_again();
        self.leading.fresh = true;
    }

    /// A plan changed what is below `above`: the folders this pane lists on
    /// the way there are asked about again, from the files themselves,
    /// the others' looks going on. Whether any is.
    pub(crate) fn ask_again_above(&mut self, above: &FoldersAbove<'_>) -> bool {
        if self.read.as_ref().is_none_or(|r| r.lens.is_none()) || self.open.is_none() {
            return false;
        }
        let mut named = Vec::new();
        let top = self
            .location()
            .and_then(Location::path)
            .map(|at| (at, &self.listing));
        let kept = opened(&self.kept, &self.expanded)
            .filter_map(|(_, k)| Some((k.path.clone()?, &k.listing)));
        for (folder, listing) in top.into_iter().chain(kept) {
            let Some(below) = above.by_folder.get(folder.as_path()) else {
                continue;
            };
            named.extend(
                below
                    .iter()
                    .filter_map(|name| listing.find_by_name(name))
                    .filter(|e| e.kind == crate::entry::EntryKind::Directory)
                    .map(|e| e.id),
            );
        }
        if named.is_empty() {
            return false;
        }
        self.leading.again_of.extend(named);
        self.leading.fresh = true;
        true
    }

    /// Whether folders are still being looked into for the pane's lens:
    /// more may yet show.
    pub fn looks_below(&self) -> bool {
        (!self.leading.waiting.is_empty() || self.leading.pass.is_some())
            && self.leading.kept.is_none()
    }

    /// Show a search as the tree of its folders, or as it comes: what the
    /// view shown does (`PaneView::lays_found_out_as_tree`).
    pub(crate) fn set_found_as_tree(&mut self, wanted: bool) -> bool {
        if self.found.wanted == wanted {
            return false;
        }
        self.found.reset();
        self.found.wanted = wanted;
        if !wanted {
            self.lent.let_go();
        }
        self.tree_version += 1;
        self.tree_members_version += 1;
        true
    }

    /// Whether the view shown lays a search out as a tree.
    pub fn wants_found_tree(&self) -> bool {
        self.found.wanted
    }

    /// What its view shows that is read on demand (`keys::READ_ON_DEMAND`):
    /// a column, or a caption under the grid's names. Read for its files
    /// as what the sort reads is (`fields_to_read`). Whether it changed.
    pub fn show_fields(&mut self, fields: &[&'static str]) -> bool {
        if self.fields_shown == fields {
            return false;
        }
        self.fields_shown = fields.to_vec();
        true
    }

    /// What `show_fields` last said its view shows.
    pub fn fields_shown(&self) -> &[&'static str] {
        &self.fields_shown
    }

    /// How many it found, when a tree shows them among the folders on the
    /// way: what says how many counts those alone.
    pub fn found_count(&self) -> Option<usize> {
        self.found.shown().map(|_| self.flat_order().len())
    }

    /// Whether the pane shows a tree of what it found.
    pub fn is_found_tree(&self) -> bool {
        self.found.shown().is_some()
    }

    /// The tree to make of the order now, one at a time: each one made
    /// says when it is done, so a search streaming is laid out as fast as
    /// the worker goes, and never behind by more than one.
    fn found_tree_asked(&mut self) -> Option<FoundAsked> {
        if !self.found.wanted || self.found.working || self.open.is_none() {
            return None;
        }
        let open = Arc::clone(self.open.as_ref()?);
        let root = open.found_under()?.to_path_buf();
        let flat = self.flat_order();
        if self
            .found
            .from
            .as_ref()
            .is_some_and(|f| Arc::ptr_eq(f, &flat))
            || (flat.is_empty() && self.found.made.is_none())
        {
            return None;
        }
        self.found.working = true;
        Some(FoundAsked {
            pane: self.id,
            open,
            root,
            flat,
            ascending: self.sort.ascending,
            answer: Arc::clone(&self.found.answer),
            known: Arc::clone(&self.found.known),
            cancel: self.found.cancel.child(),
        })
    }

    /// Take the tree made: whether what the pane shows changed.
    fn take_found_tree(&mut self) -> bool {
        let Some((flat, FoundMade(mut made))) = self.found.answer.lock().take() else {
            return false;
        };
        self.found.working = false;
        self.found.from = Some(flat);
        if !self.found.wanted {
            return false;
        }
        let ways = std::mem::take(&mut made.ways);
        self.lend(FOUND_TREE, ways);
        self.found.made = Some(made);
        self.found.made_times += 1;
        self.tree_version += 1;
        self.tree_members_version += 1;
        true
    }

    /// `flat` as the tree shows it: what went since it was made left out,
    /// what is under a closed folder too. New hits wait for the next tree,
    /// so a search streaming costs nothing here: the rows are made again
    /// when a row may be gone (`gone`), the folds change, or a tree comes.
    fn found_order(&self, flat: Arc<[EntryId]>) -> Arc<[EntryId]> {
        let Some(tree) = self.found.shown() else {
            return flat;
        };
        let gone = (
            self.listing.shown_version(),
            self.filter_version,
            if self.read.as_ref().is_some_and(|read| read.metadata) {
                self.listing.version()
            } else {
                0
            },
        );
        let mut shown = self.found.shown.lock();
        if let Some(s) = shown.as_ref()
            && s.gone == gone
            && s.folds == self.found.folds
            && s.made == self.found.made_times
        {
            return Arc::clone(&s.order);
        }
        let hit: crate::hashing::Set<EntryId> = flat.iter().copied().collect();
        let mut order = Vec::with_capacity(tree.order.len());
        let mut folded: Option<u32> = None;
        for &id in tree.order.iter() {
            let depth = tree.depth.get(&id).copied().unwrap_or(0);
            if folded.is_some_and(|at| depth > at) {
                continue;
            }
            folded = None;
            if hit.contains(&id) || tree.way_ids.contains(&id) {
                order.push(id);
                if self.found.folded.contains(&id) {
                    folded = Some(depth);
                }
            }
        }
        let order: Arc<[EntryId]> = order.into();
        *shown = Some(FoundShown {
            gone,
            folds: self.found.folds,
            made: self.found.made_times,
            order: Arc::clone(&order),
        });
        order
    }

    /// Open or close folder `at` of the tree of what was found; whether
    /// it moved. `end_folding` once all were.
    pub(crate) fn fold(&mut self, at: EntryId, folded: bool) -> bool {
        let Some(tree) = self.found.shown() else {
            return false;
        };
        if !tree.parents.contains(&at) {
            return false;
        }
        if folded {
            self.found.folded.insert(at)
        } else {
            self.found.folded.remove(&at)
        }
    }

    /// Folders of the tree opened or closed: the cursor inside a folder
    /// closed goes back to it.
    pub(crate) fn end_folding(&mut self) {
        let before = self.shown.lock().clone();
        self.found.folds += 1;
        self.tree_version += 1;
        self.tree_members_version += 1;
        let order = self.order();
        let shown: crate::hashing::Set<EntryId> = order.iter().copied().collect();
        let holds = |id: EntryId| shown.contains(&id);
        let ways = self.found.shown().map(|tree| &tree.way_ids);
        match before {
            Some(before) => self.selection.retain_near(holds, &before, true, ways),
            None => self.selection.retain(holds),
        }
    }

    /// Close every folder of the tree of what was found.
    fn fold_all(&mut self) -> bool {
        let Some(parents) = self.found.shown().map(|tree| tree.parents.clone()) else {
            return false;
        };
        self.found.folded = parents;
        self.end_folding();
        true
    }

    /// Whether `id` is a folder shown only as the way to what a search
    /// found: only naming it selects it ([`Ways`]).
    pub fn is_way(&self, id: EntryId) -> bool {
        self.found
            .shown()
            .is_some_and(|tree| tree.way_ids.contains(&id))
    }

    /// The folders shown only as the way to what a search found.
    pub fn ways(&self) -> Ways<'_> {
        self.found.shown().map(|tree| &tree.way_ids)
    }

    /// The selection to change, with what it takes only by name.
    pub(crate) fn selecting(&mut self) -> (&mut Selection, Ways<'_>) {
        let ways = self.found.shown().map(|tree| &tree.way_ids);
        (&mut self.selection, ways)
    }

    /// What a gesture passing over `rows` takes of them (Select All,
    /// Invert, a range, a rubber band): the one place that leaves out what
    /// only naming selects ([`Ways`]). `named`: the one a gesture ends on
    /// (a range), taken as asked.
    pub fn covered(
        &self,
        rows: impl IntoIterator<Item = EntryId>,
        named: Option<EntryId>,
    ) -> Vec<EntryId> {
        let ways = self.ways();
        rows.into_iter()
            .filter(|id| Some(*id) == named || ways.is_none_or(|ways| !ways.contains(id)))
            .collect()
    }

    /// Whether folder `id` can be opened in place. In the tree of what
    /// was found, only one with something found under it.
    pub fn opens_here(&self, id: EntryId) -> bool {
        self.found
            .shown()
            .is_none_or(|tree| tree.parents.contains(&id))
    }

    /// Whether folder `id` is shown only as the way to what the pane's
    /// filter keeps below it: drawn dimmed. Whether it is, is the lens's
    /// to say of it, known at once and not moved by what a look below it
    /// finds, read from the order made (`FilterCache`): no plugin is asked
    /// per row drawn.
    pub fn only_leads(&self, id: EntryId) -> bool {
        if self.is_way(id) {
            return true;
        }
        let Some(lens) = self.lens_shown() else {
            return false;
        };
        if let Some(cache) = self
            .filter_cache
            .lock()
            .as_ref()
            .filter(|cache| cache.filter == self.filter_tested)
        {
            return cache.dimmed.contains(&id);
        }
        self.entry(id).is_some_and(|e| dimmed(lens, e))
    }

    /// Show hidden files, or no longer: those selected leave the
    /// selection as they leave the view, so nothing acts on a file the
    /// user cannot see.
    pub(crate) fn set_show_hidden(&mut self, show: bool) {
        self.sort.show_hidden = show;
        self.retain_shown();
    }

    /// Whether the place shown remembers its order (ADR-132): `sort` is
    /// that place's, the pane's own set aside.
    pub fn keeps_order(&self) -> bool {
        self.own.is_some()
    }

    /// The pane's own sort: what it shows where no place remembers an
    /// order, what its tab keeps and a tab opened from it starts with.
    pub fn own_sort(&self) -> SortSpec {
        self.sort_with(None)
    }

    /// What it shows with `kept`, the order of a place that remembers
    /// one, else its own.
    pub(crate) fn sort_with(&self, kept: Option<Order>) -> SortSpec {
        kept.or(self.own)
            .map_or(self.sort, |order| order.applied(self.sort))
    }

    /// Show the order of the place it shows: `kept`, the one that place
    /// remembers, its own set aside meanwhile, and `view`, the view it
    /// remembers; none, its own again.
    pub(crate) fn show_order(&mut self, kept: Option<Order>, view: Option<Arc<str>>) {
        let sort = self.sort_with(kept);
        self.own = kept.map(|_| self.own.unwrap_or_else(|| Order::of(self.sort)));
        self.place_view = view.filter(|_| kept.is_some());
        self.sort = sort;
    }

    /// The view the place shown remembers (ADR-132), if it remembers one.
    pub fn place_view(&self) -> Option<&Arc<str>> {
        self.place_view.as_ref()
    }

    /// The sort of a column beside the pane (ADR-132): its folder's, the
    /// one it remembers, else the pane's own; a folder opened in place is
    /// in the pane's order, as what it holds is shown among the pane's.
    fn column_spec(&self, kept: &Kept) -> SortSpec {
        if self.expanded.iter().any(|o| o.location == kept.location) {
            return self.sort;
        }
        kept.order
            .map_or_else(|| self.own_sort(), |order| order.applied(self.sort))
    }

    /// How a location kept beside the pane is sorted (`column_spec`), with
    /// the keys made for it: the pane's where its sort is the pane's.
    fn sorting_of(&self, kept: &Kept) -> crate::grouping::Sort {
        let spec = self.column_spec(kept);
        let wanted = (spec.key, spec.group_key());
        let keys = if !spec.needs_keys() {
            None
        } else if self
            .keys_for
            .is_some_and(|(key, group, _)| (key, group) == wanted)
        {
            self.keys.clone()
        } else {
            kept.keys.clone().filter(|_| {
                kept.keys_for
                    .is_some_and(|(key, group, _)| (key, group) == wanted)
            })
        };
        let mut sort = crate::grouping::Sort {
            spec,
            keys,
            folders: None,
        };
        if sort.wants_folders()
            && let Some(open) = &kept.open
        {
            let open = Arc::clone(open);
            sort.folders = Some(crate::grouping::Folders(Arc::new(move |id| {
                open.path_of(id)?.parent().map(Path::to_path_buf)
            })));
        }
        sort
    }

    /// How the location `at`, kept beside the pane (a column), is sorted:
    /// as its folder remembers, else as the pane's own (ADR-132); the
    /// pane's sort where nothing is kept there.
    pub fn kept_sorting(&self, at: &Location) -> crate::grouping::Sort {
        self.kept
            .iter()
            .find(|k| &k.location == at)
            .map_or_else(|| self.sorting(), |kept| self.sorting_of(kept))
    }

    /// The columns whose order wants keys the pane's do not answer, and
    /// what for: those whose keys are not that yet, or all of them
    /// (`all`, the keys made again everywhere).
    pub(crate) fn column_keys_wanted(&self, revision: u64, all: bool) -> Vec<(Location, KeysFor)> {
        self.kept
            .iter()
            .filter_map(|kept| {
                let spec = self.column_spec(kept);
                let wanted = (spec.key, spec.group_key(), revision);
                (spec.needs_keys()
                    && self.keys_wanted_for(revision) != Some(wanted)
                    && (all || kept.keys_for != Some(wanted)))
                .then(|| (kept.location.clone(), wanted))
            })
            .collect()
    }

    /// Sort the column at `at` with `keys`, made for `made_for`, while it
    /// still wants them.
    pub(crate) fn set_column_keys(
        &mut self,
        at: &Location,
        keys: Option<crate::grouping::Keys>,
        made_for: KeysFor,
    ) {
        let revision = made_for.2;
        let still = self
            .column_keys_wanted(revision, true)
            .into_iter()
            .any(|(there, wanted)| &there == at && wanted == made_for);
        if let Some(kept) = self.kept.iter_mut().find(|k| &k.location == at)
            && still
        {
            kept.keys = keys;
            kept.keys_for = Some(made_for);
        }
    }

    /// When the first of its keys, the columns' too, answers otherwise
    /// (`Keys::next_change`: the date groups at midnight).
    pub(crate) fn keys_next_change(&self) -> Option<std::time::SystemTime> {
        self.keys
            .iter()
            .chain(self.kept.iter().filter_map(|k| k.keys.as_ref()))
            .filter_map(|keys| keys.next_change)
            .min()
    }

    /// What its columns remember of their order, as `orders` says now.
    fn columns_remember(&mut self, orders: &Orders) {
        for kept in self.kept.iter_mut().chain(self.left.as_mut()) {
            kept.order = orders.of(&kept.location);
        }
    }

    /// Let go of what the pane shows while its tab is not shown (ADR-018):
    /// the open location, its listing and watcher, the locations kept
    /// beside it, and the work tied to it. The history, the sort and the
    /// selection stay; the selection comes back when the pane is shown
    /// again and its listing is complete. What it let go of closes here
    /// (`OpenLocation::close`, under the lock).
    pub(crate) fn suspend(&mut self) {
        let Some(open) = self.open.take() else {
            return;
        };
        open.close();
        self.location_lifetime.cancel();
        self.location_lifetime = CancelToken::new();
        let selection = std::mem::take(&mut self.selection);
        if !selection.is_empty() {
            self.restore = Some(selection);
        }
        self.listing = Listing::default();
        self.shown.lock().take();
        self.reveal = None;
        self.leading.reset();
        self.found.reset();
        // Each closes as it drops.
        self.kept.clear();
        self.opened_moved();
        self.left = None;
        self.drawn_left = None;
        self.lent.let_go();
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
        let kept = self.kept.remove(i);
        self.opened_moved();
        Some(kept.into_shown())
    }

    /// Keep exactly the locations of `wanted` (the one shown aside): the
    /// one just left stays if it is wanted, the others go. Returns the
    /// wanted ones that are not open yet, for the caller to open.
    /// The folders opened in place are always wanted.
    pub(crate) fn keep_only(&mut self, wanted: &[Location]) -> Vec<Location> {
        let wanted: Vec<Location> = wanted
            .iter()
            .cloned()
            .chain(self.expanded.iter().map(|o| o.location.clone()))
            .collect();
        let before = self.kept.len();
        match self.left.take() {
            Some(left) if wanted.contains(&left.location) => self.kept.push(left),
            // While the folder gone to is filled elsewhere (ADR-120), the
            // view may draw the files of the one left until it is whole
            // (`Pane::drawn_entry`): its listing stays, its location is
            // closed, so nothing of it is read or sent any more.
            Some(left) if self.listing.is_held() => {
                let (open, listing) = left.into_shown();
                open.close();
                self.drawn_left = Some(DrawnLeft {
                    listing,
                    until: std::time::Instant::now() + HOLD,
                });
            }
            _ => {}
        }
        if !self.listing.is_held() || self.drawn_left.as_ref().is_some_and(DrawnLeft::over) {
            self.drawn_left = None;
        }
        let pushed = self.kept.len() > before;
        self.kept.retain(|k| wanted.contains(&k.location));
        // Asked at each sync of the window: which opened folder holds which
        // entry is made again only when what is kept changed.
        if pushed || self.kept.len() != before {
            self.opened_moved();
        }
        let shown = self.location().cloned();
        wanted
            .into_iter()
            .filter(|w| Some(w) != shown.as_ref() && !self.kept.iter().any(|k| &k.location == w))
            .collect()
    }

    /// The folders opened in place whose listing is not kept yet, for the
    /// caller to open.
    pub(crate) fn unopened(&self) -> Vec<Location> {
        self.expanded
            .iter()
            .filter(|o| !self.kept.iter().any(|k| k.location == o.location))
            .map(|o| o.location.clone())
            .collect()
    }

    /// Every listing the pane holds: whether companions are shown changed.
    pub(crate) fn companions_changed(&mut self) {
        self.listing.companions_changed();
        for kept in &mut self.kept {
            kept.listing.companions_changed();
        }
        self.tree_version += 1;
        self.tree_members_version += 1;
        self.retain_shown();
    }

    pub(crate) fn add_kept(
        &mut self,
        open: Arc<dyn OpenLocation>,
        listing: Listing,
        order: Option<Order>,
    ) {
        self.kept.push(Kept::new(open, listing, order));
        self.opened_moved();
    }

    /// Whether anything is kept or waiting to be: `keep_only` has work.
    pub fn keeps_any(&self) -> bool {
        !self.kept.is_empty()
            || self.left.is_some()
            || self.drawn_left.is_some()
            || !self.expanded.is_empty()
    }

    /// The listing of a location kept beside the one shown.
    pub fn kept(&self, location: &Location) -> Option<&Listing> {
        self.kept
            .iter()
            .find(|k| &k.location == location)
            .map(|k| &k.listing)
    }

    /// Where a place listed by the kept location `location` leads (a tag
    /// in a column of the tags above): plugin code, called under the
    /// caller's lock, like `path_of`.
    pub fn kept_location_of(&self, location: &Location, id: EntryId) -> Option<Location> {
        self.kept
            .iter()
            .find(|k| &k.location == location)?
            .open
            .as_ref()?
            .location_of(id)
    }

    /// Select the entry called `name` as soon as the listing has it: how a
    /// jump to a file, or a script, opens its folder with the file selected
    /// without waiting for the listing. Call it right after navigating;
    /// the next navigation forgets it, and so does a listing that
    /// completes without that name.
    pub fn reveal(&mut self, name: OsString) {
        self.reveal_as(name, Revealing::Asked);
    }

    /// `reveal` an entry that its location lists a moment after it is
    /// complete (a tag just made, listed by a worker): the name is kept
    /// until it is listed or the pane shows another location.
    pub fn reveal_coming(&mut self, name: OsString) {
        self.reveal_as(name, Revealing::Coming);
    }

    /// `reveal` the way back down, gone up from it (`App::way_down`):
    /// selected only if shown, so a hidden folder left turns no hidden
    /// files on and a folder the filter leaves out is not selected.
    pub fn reveal_way_down(&mut self, name: OsString) {
        self.reveal_as(name, Revealing::WayDown);
    }

    fn reveal_as(&mut self, name: OsString, how: Revealing) {
        self.reveal = Some((name, how));
        self.reveal_looked = None;
        self.take_reveal();
    }

    fn take_reveal(&mut self) {
        let Some((name, how)) = &self.reveal else {
            return;
        };
        let how = *how;
        let names = Some(self.listing.names_version());
        let looked = std::mem::replace(&mut self.reveal_looked, names) == names;
        if looked && !self.listing.is_complete() {
            return;
        }
        if let Some(id) = (!looked)
            .then(|| self.listing.find_by_name(name).map(|e| e.id))
            .flatten()
        {
            if how != Revealing::WayDown || self.is_shown(id) {
                self.select_revealed(&[id]);
            }
            self.reveal = None;
        } else if self.listing.is_complete() && how != Revealing::Coming {
            self.reveal = None;
        }
    }

    /// Select `ids` alone, entries asked for by their names (a path typed,
    /// a jump, a link followed) or just put there (a new folder, what was
    /// pasted): a hidden one turns hidden files on in the pane, or it would
    /// be selected unseen and let go of at once (`retain_shown`). Whether
    /// it turned them on.
    pub fn select_revealed(&mut self, ids: &[EntryId]) -> bool {
        let unhide = !self.sort.show_hidden
            && ids
                .iter()
                .any(|id| self.listing.get(*id).is_some_and(crate::Entry::is_hidden));
        if unhide {
            self.set_show_hidden(true);
        }
        self.selection.set(ids.iter().copied());
        unhide
    }

    /// Whether listing events are waiting to be applied, the kept
    /// locations' included.
    pub fn backlog(&self) -> bool {
        self.listing.has_result()
            || !self.listing.is_held()
                && !self.listing.is_sorting()
                && self
                    .open
                    .as_ref()
                    .is_some_and(|open| !open.events().is_empty())
            || self.kept.iter().any(Kept::backlog)
            || self.leading.pass.is_some()
    }

    /// The jobs for workers that what the pane shows wants: its
    /// listing's and the folders' it keeps beside it. A folder filled
    /// elsewhere while events wait for it (ADR-120), sorted in the pane's
    /// order once whole; an order asked anew of a listing shown (ADR-121).
    /// The window's thread builds no folder it opens, nor sorts one anew.
    pub fn jobs_wanted(&mut self) -> Vec<crate::location::ListingJob> {
        use crate::location::{ListingJob, ReadJob};
        // Each column in its own order (ADR-132), the listing shown and the
        // folders opened in place in the pane's.
        let sorts: Vec<crate::grouping::Sort> = std::iter::once(self.sorting())
            .chain(self.kept.iter().map(|kept| self.sorting_of(kept)))
            .collect();
        let mut jobs = Vec::new();
        let expanded = &self.expanded;
        // What the order reads is read of what the pane's order holds: the
        // listing shown and the folders opened in place, not a column
        // beside it.
        let listings = std::iter::once((self.open.as_ref(), &mut self.listing, true)).chain(
            self.kept.iter_mut().map(|kept| {
                let in_order = expanded.iter().any(|o| o.location == kept.location);
                (kept.open.as_ref(), &mut kept.listing, in_order)
            }),
        );
        for ((open, listing, in_order), sort) in listings.zip(&sorts) {
            let Some(open) = open else { continue };
            jobs.extend(listing.fill_job(open.events(), sort).map(ListingJob::Fill));
            if in_order {
                let ids = listing.reading_wanted(sort.spec);
                if !ids.is_empty() {
                    tracing::debug!(n = ids.len(), "read for the order");
                    jobs.push(ListingJob::Read(ReadJob {
                        open: Arc::clone(open),
                        ids,
                    }));
                }
            } else {
                // A folder no longer opened in place: its order waits for
                // nothing.
                listing.let_go_of_reading();
            }
            jobs.extend(listing.sort_job(sort).map(ListingJob::Sort));
        }
        jobs
    }

    /// The location the pane shows: where it was opened, or where its
    /// folder moved since (`Listing::address`).
    pub fn location(&self) -> Option<&Location> {
        self.open.as_ref().map(|o| self.listing.address(o.as_ref()))
    }

    /// Apply pending listing events, the kept locations' too, for at most
    /// `budget` in all: the listing shown first, the kept ones with what
    /// it left. Returns how many changed the shown listing; `backlog`
    /// says whether some are still waiting.
    pub fn pump(&mut self, budget: std::time::Duration) -> usize {
        let deadline = std::time::Instant::now() + budget;
        let n = match &self.open {
            Some(open) => {
                let rx = open.events().clone();
                self.listing.drain(&rx, budget)
            }
            None => 0,
        };
        let mut moves: Vec<(Location, Location)> = self.follow_moved().into_iter().collect();
        let mut opened = 0;
        let mut members = false;
        for kept in &mut self.kept {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break;
            }
            let before = (kept.listing.names_version(), kept.listing.shown_version());
            let applied = kept.pump(left);
            if self.expanded.iter().any(|o| o.location == kept.location) {
                opened += applied;
                members |= before != (kept.listing.names_version(), kept.listing.shown_version());
            }
            moves.extend(kept.follow_moved());
        }
        if opened > 0 {
            self.tree_version += 1;
        }
        // Which opened folder holds which entry is made again only when
        // their entries came, went or were renamed: a batch of metadata,
        // each row scrolled to in a big folder opened in place, made it
        // again from every entry.
        if members {
            self.tree_members_version += 1;
            self.opened_moved();
        }
        if !moves.is_empty() {
            self.follow_moves(&moves);
        }
        self.follow_renamed();
        if self.open.is_none() {
            return 0;
        }
        if n + opened > 0 {
            self.retain_changed();
        }
        if n > 0 {
            self.take_reveal();
        }
        self.take_restore();
        n + opened
    }

    /// Follow the folder shown where it moved (`Listing::take_moved`): it is
    /// shown there, in its place in the history. Where it was reached
    /// before and now.
    fn follow_moved(&mut self) -> Option<(Location, Location)> {
        let open = self.open.as_ref()?;
        let (before, now) = self.listing.take_moved()?;
        if !open.location().is_path() {
            return None;
        }
        let before = before.unwrap_or_else(|| open.location().clone());
        self.replace_here(now.clone());
        Some((before, now))
    }

    /// Follow folders that moved, each where it was reached before and
    /// now: what the pane keeps of them or under them goes with them, the
    /// folders kept and opened in place (whose own listings, each watching
    /// its folder alone, see nothing move) and the narrowing's folder. The
    /// folder shown moved with one above it is not followed: its listing
    /// would still name its files where they were.
    fn follow_moves(&mut self, moves: &[(Location, Location)]) {
        for (before, now) in moves {
            let (Some(from), Some(to)) = (before.path(), now.path()) else {
                continue;
            };
            let there = |location: &Location| {
                let path = moved_path(&location.path()?, &from, &to)?;
                Some(if path == to {
                    now.clone()
                } else {
                    Location::from_path(path)
                })
            };
            for kept in self.kept.iter_mut().chain(self.left.as_mut()) {
                if let Some(at) = there(&kept.location)
                    && at != kept.location
                {
                    kept.relocate(at);
                }
            }
            for o in &mut self.expanded {
                if let Some(at) = there(&o.location) {
                    o.location = at;
                }
                if let Some(at) = o.within.as_ref().and_then(there) {
                    o.within = Some(at);
                }
            }
        }
        self.expanded_changed();
    }

    /// What the pane keeps by entry follows the entries its listings say
    /// were renamed: the selection (and one waiting to come back), the
    /// folders opened in place at them.
    fn follow_renamed(&mut self) {
        let mut renamed = self.listing.take_renamed();
        for kept in &mut self.kept {
            renamed.extend(kept.listing.take_renamed());
        }
        if renamed.is_empty() {
            return;
        }
        self.selection.rekey(&renamed);
        if let Some(restore) = &mut self.restore {
            restore.rekey(&renamed);
        }
        self.leading.rekey(&renamed);
        let mut moved = false;
        for o in &mut self.expanded {
            if let Some((_, after)) = renamed.iter().find(|(before, _)| *before == o.at) {
                o.at = *after;
                moved = true;
            }
        }
        if moved {
            self.expanded_changed();
        }
    }

    /// Drop from the selection what the pane no longer shows: an entry
    /// gone, a hidden file while hidden files are not shown (one renamed
    /// to a dot name too), a companion hidden beside its file, what the
    /// filter or the narrowing leaves out, a file the narrowing cannot tell
    /// yet included: it waits hidden, and nothing acts on what is hidden.
    pub(crate) fn retain_shown(&mut self) {
        self.keep_shown(false);
    }

    /// `retain_shown` asked only of `ids`, those a change just selected
    /// (`Selection::change`): the rest was selected, and shown, already. A
    /// pass over the whole selection happens only when one of them goes.
    pub(crate) fn retain_shown_of(&mut self, ids: &[EntryId]) {
        let folded = self.folded_entries();
        let holds = shows(
            &self.listing,
            &self.kept,
            &self.expanded,
            &self.lent,
            self.leading
                .filter(self.filter.as_deref(), self.read.as_deref()),
            self.sort,
        );
        let hidden: crate::hashing::Set<EntryId> = ids
            .iter()
            .copied()
            .filter(|id| !holds(*id) || folded.contains(id))
            .collect();
        if !hidden.is_empty() {
            self.selection.retain(|id| !hidden.contains(&id));
        }
    }

    /// Add `added` to the selection and take out `removed`
    /// (`Selection::change`), asking only of what it added whether the
    /// pane shows it: a selection looked over before the change is still
    /// looked over after it.
    pub(crate) fn change_selection(&mut self, added: &[EntryId], removed: &[EntryId]) {
        let before = self.selection.revision();
        self.selection.change(added, removed);
        self.retain_shown_of(added);
        let now = self.retain_key();
        if let Some(retained) = &mut self.retained
            && retained.key
                == (RetainKey {
                    selection: before,
                    ..now
                })
        {
            retained.key.selection = now.selection;
        }
    }

    /// `retain_shown`, the cursor going back to what is before it when its
    /// entry leaves the view (`Selection::retain_near`).
    fn keep_shown(&mut self, back: bool) {
        // The order as shown now: a fold may have just left entries out.
        let folded = self.folded_entries();
        let shown = if folded.is_empty() {
            self.shown.lock().clone()
        } else {
            Some(self.order())
        };
        let listed = shows(
            &self.listing,
            &self.kept,
            &self.expanded,
            &self.lent,
            self.leading
                .filter(self.filter.as_deref(), self.read.as_deref()),
            self.sort,
        );
        let holds = move |id: EntryId| listed(id) && !folded.contains(&id);
        match shown {
            Some(shown) => {
                let ways = self.found.shown().map(|tree| &tree.way_ids);
                self.selection.retain_near(holds, &shown, back, ways);
            }
            None => self.selection.retain(holds),
        }
        self.retained = Some(Retained {
            key: self.retain_key(),
            at: self.listings_at(),
        });
    }

    /// What decides which entries the pane shows, metadata aside, now.
    fn retain_key(&self) -> RetainKey {
        RetainKey {
            listing: (self.listing.id(), self.listing.shown_version()),
            tree: self.tree_members_version,
            filter: self.filter_tested,
            show_hidden: self.sort.show_hidden,
            selection: self.selection.revision(),
            lent: self.lent.version,
        }
    }

    /// The listings the pane shows entries of, the shown one's first, each
    /// by its id with its version.
    fn listings(&self) -> impl Iterator<Item = &Listing> {
        std::iter::once(&self.listing)
            .chain(opened(&self.kept, &self.expanded).map(|(_, k)| &k.listing))
    }

    fn listings_at(&self) -> Vec<(u64, u64)> {
        self.listings().map(|l| (l.id(), l.version())).collect()
    }

    /// `retain_shown` after a pump, only for what may have changed: nothing
    /// when only metadata did and no filter reads it, the entries it
    /// changed when one does (a narrowing), the whole selection only once
    /// what decides what is shown moved (an entry gone, hidden, the
    /// filter). A pass over a selection of a whole folder for each batch of
    /// its metadata was the folder's size each time.
    fn retain_changed(&mut self) {
        let Some(retained) = self.retained.take() else {
            return self.retain_shown();
        };
        if retained.key != self.retain_key() {
            return self.retain_shown();
        }
        let at = self.listings_at();
        let metadata = self.read.as_ref().is_some_and(|r| r.metadata);
        if at == retained.at || !metadata {
            self.retained = Some(Retained {
                key: retained.key,
                at,
            });
            return;
        }
        // A filter reads metadata: the selected entries whose metadata
        // changed are asked again, as long as each listing is the one
        // looked over and keeps what changed since.
        let same = at.len() == retained.at.len()
            && at
                .iter()
                .zip(&retained.at)
                .all(|(now, then)| now.0 == then.0);
        let asked = || -> Option<Vec<EntryId>> {
            let mut asked = Vec::new();
            for (listing, (_, then)) in self.listings().zip(&retained.at) {
                let changed = listing.updated_since(*then)?;
                asked.extend(changed.filter(|id| self.selection.contains(*id)));
            }
            Some(asked)
        };
        let Some(asked) = same.then(asked).flatten() else {
            return self.retain_shown();
        };
        self.retain_shown_of(&asked);
        self.retained = Some(Retained {
            key: self.retain_key(),
            at,
        });
    }

    /// Whether the pane shows entry `id`: listed, here or in a folder
    /// opened in place, not hidden (a dot file, a companion) and kept by
    /// the filter and the narrowing. What a command selects must be.
    pub fn is_shown(&self, id: EntryId) -> bool {
        shows(
            &self.listing,
            &self.kept,
            &self.expanded,
            &self.lent,
            self.text_filter(),
            self.sort,
        )(id)
            && !self.folded_entries().contains(&id)
    }

    /// The entries a folded group holds: not shown (ADR-131, 2).
    fn folded_entries(&self) -> Arc<crate::hashing::Set<EntryId>> {
        if self.any_folded() {
            self.shown_groups().2
        } else {
            Arc::default()
        }
    }

    fn expanded_changed(&mut self) {
        self.expanded_version += 1;
        self.tree_version += 1;
        self.tree_members_version += 1;
        self.opened_moved();
    }

    /// The folders opened, the listings kept or theirs changed: which
    /// holds which entry is made again when next asked.
    fn opened_moved(&self) {
        self.opened_index.lock().take();
    }

    /// The opened folder, and its kept listing, holding entry `id`.
    fn opened_of(&self, id: EntryId) -> Option<(&Opened, &Kept)> {
        if self.expanded.is_empty() {
            return None;
        }
        let (e, k) = *self
            .opened_index
            .lock()
            .get_or_insert_with(|| OpenedIndex::of(&self.kept, &self.expanded))
            .0
            .get(&id)?;
        Some((self.expanded.get(e)?, self.kept.get(k)?))
    }

    /// Lend the pane `entries`, from the view `view` (`ViewSpec::id`):
    /// what it draws beside what the pane lists (Usage's blocks inside the
    /// folder's own entries, ADR-116), in place of what it lent before.
    /// Shown while lent, so selected and acted on as the listing's, but in
    /// no order. An entry lent again at its path as it was seen keeps its
    /// reading; one seen otherwise keeps what was read of it, read again
    /// once selected ([`Pane::lent_to_read`]). What the pane keeps by
    /// entry follows one renamed, or a name another file took, as a
    /// listing's (`lent_renamed`); what is no longer lent leaves the
    /// selection. Lending what is lent already changes nothing. Says the
    /// version lent, which [`Pane::lent_version`] gives until what is lent
    /// changes or the pane lets go of it (another location, dormancy).
    pub fn lend(&mut self, view: &str, entries: impl IntoIterator<Item = Loaned>) -> u64 {
        let next = self.lent.version + 1;
        let mut before = std::mem::take(&mut self.lent.entries);
        let mut same = true;
        let mut came = Vec::new();
        self.lent.entries = entries
            .into_iter()
            .map(|Loaned { entry, path, seen }| {
                let id = entry.id;
                let loan = match before.remove(&id) {
                    // As it was seen: its reading stands.
                    Some(known) if known.path == path && known.seen == seen => known,
                    // Seen otherwise: what was read of it shows until it
                    // is read again.
                    Some(known) if known.path == path => {
                        same = false;
                        Loan {
                            entry: known.entry,
                            path,
                            seen,
                            read: false,
                            since: next,
                        }
                    }
                    known => {
                        same = false;
                        if known.is_none() {
                            came.push(id);
                        }
                        Loan {
                            entry,
                            path,
                            seen,
                            read: false,
                            since: next,
                        }
                    }
                };
                (id, loan)
            })
            .collect();
        let lender = (!self.lent.entries.is_empty()).then(|| Arc::<str>::from(view));
        if same && before.is_empty() && (lender.is_none() || self.lent.by == lender) {
            return self.lent.version;
        }
        self.lent.by = lender;
        self.lent.version = next;
        self.lent.index();
        let renamed = lent_renamed(before.into_keys(), &came);
        self.selection.rekey(&renamed);
        self.retain_shown();
        self.lent.version
    }

    /// Let go of what `view` lent, if it lent what the pane holds: it
    /// draws it no more (another view shown, its plugin turned off).
    /// Whether anything was let go of.
    pub fn let_go_of_lent(&mut self, view: &str) -> bool {
        if self.lent.by.as_deref() != Some(view) {
            return false;
        }
        self.lend(view, std::iter::empty());
        true
    }

    /// The view that lent what the pane holds (`Pane::lend`).
    pub fn lent_by(&self) -> Option<&str> {
        self.lent.by.as_deref()
    }

    /// Changes as what a view lends changes and as the pane lets go of
    /// it, never as what is lent is read or found gone: a view lends again
    /// when it is not the version its lending said, and lending again
    /// asks for a reading (`lent_to_read`), so a reading that moved it
    /// read the selection again and again, forever.
    pub fn lent_version(&self) -> u64 {
        self.lent.version
    }

    /// Whether a view lent the pane anything.
    pub fn has_lent(&self) -> bool {
        !self.lent.entries.is_empty()
    }

    /// The entry a view lent of the file `file` at `path`
    /// ([`Pane::entry_at`]).
    fn lent_at(&self, file: crate::FileId, path: &Path) -> Option<&Entry> {
        self.lent
            .of_file
            .get(&file)?
            .iter()
            .filter_map(|id| self.lent.entries.get(id))
            .find(|loan| loan.path == path)
            .map(|loan| &loan.entry)
    }

    /// Whether `id` is an entry a view lent, which no listing holds.
    pub fn is_lent(&self, id: EntryId) -> bool {
        self.lent.entries.contains_key(&id)
            && self.listing.get(id).is_none()
            && self.opened_of(id).is_none()
    }

    /// Whether `id` is lent and not read since it was last lent: what a
    /// view asks to be read once it is selected.
    pub fn lent_to_read(&self, id: EntryId) -> bool {
        self.is_lent(id) && self.lent.entries.get(&id).is_some_and(|loan| !loan.read)
    }

    /// Those of `ids` lent and not read since, with where each is: what
    /// a hydration reads by path (`fs::read_paths`); and the version they
    /// are asked at, which their reading gives back (`lent_read`).
    pub(crate) fn lent_paths(&self, ids: &[EntryId]) -> (Vec<(EntryId, PathBuf)>, u64) {
        let paths = ids
            .iter()
            .filter(|id| self.lent_to_read(**id))
            .filter_map(|id| Some((*id, self.lent.entries.get(id)?.path.clone())))
            .collect();
        (paths, self.lent.version)
    }

    /// What was read of lent entries (`lent_paths`, at version `asked`):
    /// `found` read, `gone` no longer there, `unread` there and unreadable
    /// (marked `keys::UNREAD`, so nothing waits for them). An entry no
    /// longer lent is left alone, and so is one lent again since, seen
    /// otherwise: the reading is of what it was. Whether anything changed.
    pub(crate) fn lent_read(
        &mut self,
        found: Vec<Entry>,
        gone: &[EntryId],
        unread: &[EntryId],
        asked: u64,
    ) -> bool {
        use crate::entry::{Provenance, Value, keys};
        let mut changed = false;
        for entry in found {
            if let Some(loan) = self.lent.entries.get_mut(&entry.id)
                && loan.since <= asked
            {
                loan.entry = entry;
                loan.read = true;
                changed = true;
            }
        }
        for id in unread {
            if let Some(loan) = self.lent.entries.get_mut(id)
                && loan.since <= asked
            {
                // What was known of it stands.
                if !loan.entry.metadata.is_hydrated() {
                    loan.entry.metadata.set(
                        keys::UNREAD,
                        Value::Bool(true),
                        Provenance::Filesystem,
                    );
                }
                loan.read = true;
                changed = true;
            }
        }
        let went: Vec<EntryId> = gone
            .iter()
            .copied()
            .filter(|id| {
                self.lent
                    .entries
                    .get(id)
                    .is_some_and(|loan| loan.since <= asked)
            })
            .collect();
        for id in &went {
            self.lent.remove(*id);
        }
        let went = went.len();
        // The version stays: what the view lent is what it drew, and it
        // draws again once what drew it (a scan) sees the change.
        if went > 0 {
            self.retain_shown();
        }
        changed || went > 0
    }

    /// The folders opened in place under the one shown.
    pub fn expanded(&self) -> &[Opened] {
        &self.expanded
    }

    pub fn is_expanded(&self, location: &Location) -> bool {
        self.expanded.iter().any(|o| &o.location == location)
    }

    /// Whether the folder (or place) that is entry `at` is open in place.
    pub fn is_open_at(&self, at: EntryId) -> bool {
        if let Some(tree) = self.found.shown() {
            return tree.parents.contains(&at) && !self.found.folded.contains(&at);
        }
        self.expanded.iter().any(|o| o.at == at)
    }

    /// Changes whenever a folder is opened in place or closed, not as
    /// their listings fill: what a view that asks for folders to be opened
    /// keys its asking on, so it asks again once they were closed.
    pub fn expanded_version(&self) -> u64 {
        self.expanded_version
    }

    /// Changes whenever the entries or the order the opened folders add may
    /// have: a view keys what it drew on it beside the listing's version.
    pub fn tree_version(&self) -> u64 {
        self.tree_version
    }

    /// Changes whenever the entries the opened folders show may have (the
    /// set, their names, which are shown), not their metadata: what a view
    /// keys on beside the listing's `names_version`.
    pub fn tree_members_version(&self) -> u64 {
        self.tree_members_version
    }

    /// Open `location` in place, the folder (or place) that is entry `at`
    /// of the one shown or of one already opened. Returns whether it was
    /// not open. Its listing is opened by the caller (`App::open_expanded`).
    pub(crate) fn expand(&mut self, location: Location, at: EntryId) -> bool {
        if self.location().is_none() || self.is_expanded(&location) || self.is_open_at(at) {
            return false;
        }
        let within = if self.listing.get(at).is_some() {
            None
        } else {
            match self.opened_of(at) {
                Some((o, _)) => Some(o.location.clone()),
                None => return false,
            }
        };
        self.expanded.push(Opened {
            location,
            at,
            within,
        });
        self.expanded_changed();
        true
    }

    /// Close the opened folders `closes` holds, and every one opened inside
    /// them; what the selection held there goes. Returns whether some were
    /// open.
    fn close_where(&mut self, closes: impl Fn(&Opened) -> bool) -> bool {
        let mut gone: Vec<Location> = self
            .expanded
            .iter()
            .filter(|o| closes(o))
            .map(|o| o.location.clone())
            .collect();
        if gone.is_empty() {
            return false;
        }
        loop {
            let more: Vec<Location> = self
                .expanded
                .iter()
                .filter(|o| !gone.contains(&o.location))
                .filter(|o| o.within.as_ref().is_some_and(|w| gone.contains(w)))
                .map(|o| o.location.clone())
                .collect();
            if more.is_empty() {
                break;
            }
            gone.extend(more);
        }
        self.expanded.retain(|o| !gone.contains(&o.location));
        self.expanded_changed();
        // A cursor inside goes back to the folder that held it.
        self.keep_shown(true);
        true
    }

    /// Close `location` and every folder opened inside it; what the
    /// selection held there goes. Returns whether it was open.
    pub(crate) fn collapse(&mut self, location: &Location) -> bool {
        self.close_where(|o| &o.location == location)
    }

    /// What it holds open of `folders`: the folder it shows, those it
    /// keeps beside it and the one just left.
    pub(crate) fn open_in(&self, folders: &[PathBuf]) -> Vec<Arc<dyn OpenLocation>> {
        let of = |path: Option<&Path>| path.is_some_and(|p| folders.iter().any(|f| f == p));
        let shown = self
            .open
            .iter()
            .filter(|_| of(self.location().and_then(Location::path).as_deref()))
            .cloned();
        let kept = self
            .kept
            .iter()
            .chain(&self.left)
            .filter(|k| of(k.path.as_deref()))
            .filter_map(|k| k.open.clone());
        shown.chain(kept).collect()
    }

    /// Let go of the folders under `path` it keeps beside the one shown,
    /// has opened in place or has just left: their volume is about to be
    /// unmounted. Each closes as it drops.
    pub(crate) fn release_under(&mut self, path: &Path) {
        // What each holds open (`OpenLocation::holds`), whatever its
        // scheme: a trashed folder opened in place is listed by its own.
        let under = |k: &Kept| {
            k.open
                .as_ref()
                .and_then(|open| open.holds())
                .or(k.path.as_deref())
                .is_some_and(|p| p.starts_with(path))
        };
        let held: Vec<Location> = self
            .kept
            .iter()
            .filter(|k| under(k))
            .map(|k| k.location.clone())
            .collect();
        self.close_where(|o| {
            held.contains(&o.location) || o.location.path().is_some_and(|p| p.starts_with(path))
        });
        if self.left.as_ref().is_some_and(under) {
            self.left = None;
        }
        self.drawn_left = None;
        self.kept.retain(|k| !under(k));
        self.opened_moved();
    }

    /// Let go of the locations `gone` names (a scheme no plugin opens any
    /// more) that it keeps beside the one shown, has opened in place (with
    /// what is open inside them) or has just left. Each closes as it drops.
    pub(crate) fn release_where(&mut self, gone: impl Fn(&Location) -> bool) {
        self.close_where(|o| gone(&o.location));
        if self.left.as_ref().is_some_and(|k| gone(&k.location)) {
            self.left = None;
        }
        self.drawn_left = None;
        self.kept.retain(|k| !gone(&k.location));
        self.opened_moved();
    }

    /// Close every folder opened in place.
    pub fn collapse_all(&mut self) -> bool {
        if self.fold_all() {
            return true;
        }
        if self.expanded.is_empty() {
            return false;
        }
        self.expanded.clear();
        self.expanded_changed();
        // A cursor inside goes back to the folder of the shown listing
        // that held it.
        self.keep_shown(true);
        true
    }

    /// An entry the pane shows: the shown listing's, or an opened folder's.
    pub fn entry(&self, id: EntryId) -> Option<&Entry> {
        self.listing
            .get(id)
            .or_else(|| self.opened_of(id).and_then(|(_, k)| k.listing.get(id)))
            .or_else(|| self.lent.entries.get(&id).map(|loan| &loan.entry))
    }

    /// An entry as a view draws it: one the pane holds (`entry`), or, while
    /// the folder gone to is filled elsewhere (ADR-120), one of the folder
    /// left, whose files the view may still draw until the new one is
    /// whole (ADR-120, amended). Never what a command acts on: the pane's
    /// order and selection are the folder gone to's.
    pub fn drawn_entry(&self, id: EntryId) -> Option<&Entry> {
        self.entry(id).or_else(|| {
            if !self.listing.is_held() {
                return None;
            }
            self.left
                .as_ref()
                .map(|left| &left.listing)
                .or(self
                    .drawn_left
                    .as_ref()
                    .filter(|drawn| !drawn.over())
                    .map(|drawn| &drawn.listing))?
                .get(id)
        })
    }

    /// The entry the pane shows of the file `file` at `path`: of a file
    /// with several names, the one listed there (ADR-049). Found by its
    /// name and file in each listing (`Listing::named_file`), so a command
    /// may ask it of each entry of a selection of a whole folder; one a
    /// view lent (ADR-116) by its file and its path.
    pub fn entry_at(&self, file: crate::FileId, path: &Path) -> Option<&Entry> {
        let name = path.file_name()?;
        std::iter::once(&self.listing)
            .chain(self.kept.iter().map(|k| &k.listing))
            .flat_map(|l| l.named_file(name, file))
            .find(|e| self.path_of(e.id).as_deref() == Some(path))
            .or_else(|| self.lent_at(file, path))
    }

    /// The listing (`Listing::id`) that holds entry `id`: the shown one's,
    /// or an opened folder's. An entry that moves from one to another (a
    /// file dragged between two tags opened in place) arrives bare in the
    /// other, to be hydrated again.
    /// The location whose listing holds entry `id`: the one shown, or a
    /// folder or a place opened in place (the tree's, the board's columns).
    pub fn holder(&self, id: EntryId) -> Option<&Location> {
        if self.listing.get(id).is_some() {
            return self.location();
        }
        self.opened_of(id).map(|(o, _)| &o.location)
    }

    pub fn listing_of(&self, id: EntryId) -> Option<u64> {
        if self.listing.get(id).is_some() {
            return Some(self.listing.id());
        }
        self.opened_of(id).map(|(_, k)| k.listing.id())
    }

    /// Where an entry of an opened folder lives. `None` for the shown
    /// listing's own entries, which only its location knows. An opened
    /// place's (a tag's) are asked of it: plugin code, like `path_of`.
    pub fn opened_path(&self, id: EntryId) -> Option<PathBuf> {
        if self.expanded.is_empty() || self.listing.get(id).is_some() {
            return None;
        }
        let (o, k) = self.opened_of(id)?;
        match o.location.path() {
            Some(dir) => k.listing.get(id).map(|e| dir.join(&e.name)),
            None => k.open.as_ref()?.path_of(id),
        }
    }

    /// Where an entry the pane shows lives on disk. Asks the location for
    /// its own entries: plugin code, called under the caller's lock.
    pub fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.opened_path(id).or_else(|| {
            // A lent entry is no listing's: the location never knew it.
            if self.listing.get(id).is_none()
                && let Some(loan) = self.lent.entries.get(&id)
            {
                return Some(loan.path.clone());
            }
            self.open.as_ref()?.path_of(id)
        })
    }

    /// Where a place the pane shows leads, or a folder that does not open
    /// at its path (a trashed folder: `OpenLocation::location_of`): plugin
    /// code, called under the caller's lock, like `path_of`. Opening it,
    /// in a tab, in place, and dropping on it, all go there.
    pub fn location_of(&self, id: EntryId) -> Option<Location> {
        if !id.is_place() && !self.entry(id).is_some_and(crate::Entry::is_dir) {
            return None;
        }
        if self.listing.get(id).is_some() {
            return self.open.as_ref()?.location_of(id);
        }
        self.opened_of(id)
            .and_then(|(_, k)| k.open.as_ref()?.location_of(id))
    }

    /// The location opening entry `id` shows: where a place leads, or a
    /// folder that does not open at its path (`location_of`), else a
    /// folder's own path; `None` for a file. Plugin code, called under the
    /// caller's lock, like `path_of`.
    pub fn leads_to(&self, id: EntryId) -> Option<Location> {
        self.location_of(id).or_else(|| {
            self.entry(id).filter(|e| e.is_dir())?;
            self.path_of(id).map(Location::from_path)
        })
    }

    /// How many opened folders deep entry `id` is: 0 for the shown
    /// listing's own entries.
    pub fn depth(&self, id: EntryId) -> usize {
        if let Some(tree) = self.found.shown() {
            return tree.depth.get(&id).map_or(0, |depth| *depth as usize);
        }
        if self.expanded.is_empty() || self.listing.get(id).is_some() {
            return 0;
        }
        let Some((o, _)) = self.opened_of(id) else {
            return 0;
        };
        let mut depth = 1;
        let mut within = o.within.as_ref();
        while let Some(w) = within {
            depth += 1;
            within = self
                .expanded
                .iter()
                .find(|o| &o.location == w)
                .and_then(|o| o.within.as_ref());
            if depth > self.expanded.len() {
                break;
            }
        }
        depth
    }

    /// The open locations of the folders opened in place.
    pub(crate) fn opened_locations(&self) -> Vec<Arc<dyn OpenLocation>> {
        opened(&self.kept, &self.expanded)
            .filter_map(|(_, k)| k.open.clone())
            .collect()
    }

    /// `ids` by the location that lists them: the shown one, an opened
    /// folder's, or one kept beside it that its view also shows (a column
    /// of the columns view, `PaneView::kept_locations`). For hydration,
    /// called outside the lock.
    pub(crate) fn by_location(
        &self,
        ids: &[EntryId],
    ) -> Vec<(Arc<dyn OpenLocation>, Vec<EntryId>)> {
        let mut out: Vec<(Arc<dyn OpenLocation>, Vec<EntryId>)> = Vec::new();
        for &id in ids {
            // No location knows a lent entry: it is read by its path
            // (`lent_paths`).
            if self.is_lent(id) {
                continue;
            }
            let open = if self.listing.get(id).is_some() {
                self.open.as_ref()
            } else {
                self.opened_of(id)
                    .and_then(|(_, k)| k.open.as_ref())
                    .or_else(|| {
                        self.kept
                            .iter()
                            .find(|k| k.listing.get(id).is_some())
                            .and_then(|k| k.open.as_ref())
                    })
                    .or(self.open.as_ref())
            };
            let Some(open) = open else { continue };
            match out.iter_mut().find(|(o, _)| Arc::ptr_eq(o, open)) {
                Some((_, ids)) => ids.push(id),
                None => out.push((Arc::clone(open), vec![id])),
            }
        }
        out
    }

    /// The entries the pane shows, in the order shown: the listing sorted,
    /// each opened folder's entries after it, sorted the same way, those
    /// the filter and the narrowing keep. What range selection, select
    /// all and the views follow.
    pub fn order(&self) -> Arc<[EntryId]> {
        let order = self.shown_groups().0;
        *self.shown.lock() = Some(Arc::clone(&order));
        order
    }

    /// The order shown and its groups' spans, made together: a folded
    /// group's entries left out of the order (ADR-131, 2), its span kept
    /// with none of them (`len` 0) and its count; and the entries left out.
    fn shown_groups(
        &self,
    ) -> (
        Arc<[EntryId]>,
        Arc<[crate::grouping::Span]>,
        Arc<crate::hashing::Set<EntryId>>,
    ) {
        let unfolded = self.found_order(self.flat_order());
        let spans = self.groups_of(&unfolded);
        if spans.is_empty() || !self.any_folded() {
            return (unfolded, spans, Arc::default());
        }
        let mut cache = self.fold_cache.lock();
        if let Some(c) = cache.as_ref()
            && Arc::ptr_eq(&c.from, &unfolded)
            && Arc::ptr_eq(&c.spans_from, &spans)
            && c.version == self.folds.version
        {
            return (
                Arc::clone(&c.order),
                Arc::clone(&c.spans),
                Arc::clone(&c.folded),
            );
        }
        let mut order = Vec::with_capacity(unfolded.len());
        let mut folded = crate::hashing::Set::default();
        let shown: Arc<[crate::grouping::Span]> = spans
            .iter()
            .map(|span| {
                let start = order.len();
                let entries = &unfolded[span.start..span.start + span.len];
                let fold = self.folds.groups.contains(&fold_key(span.group.as_deref()));
                if fold {
                    folded.extend(entries.iter().copied());
                } else {
                    order.extend_from_slice(entries);
                }
                crate::grouping::Span {
                    start,
                    len: if fold { 0 } else { span.len },
                    count: span.count,
                    group: span.group.clone(),
                }
            })
            .collect();
        let order: Arc<[EntryId]> = order.into();
        let folded = Arc::new(folded);
        *cache = Some(FoldCache {
            from: unfolded,
            spans_from: spans,
            version: self.folds.version,
            order: Arc::clone(&order),
            spans: Arc::clone(&shown),
            folded: Arc::clone(&folded),
        });
        (order, shown, folded)
    }

    /// Whether any group of the criterion the pane groups by is folded,
    /// under a view that draws the folded group's heading.
    fn any_folded(&self) -> bool {
        self.folds_drawn && !self.folds.groups.is_empty() && self.folds.by == self.sort.group_key()
    }

    /// Whether the view shown draws its groups' headings, so that a fold
    /// takes a group's entries out (`folds_drawn`): what the shell's view
    /// says (`PaneView::draws_headings`). Whether what is shown changed:
    /// the folded groups' entries come back, or leave (what was selected of
    /// them let go).
    pub(crate) fn set_folds_drawn(&mut self, drawn: bool) -> bool {
        if self.folds_drawn == drawn {
            return false;
        }
        let folded = !self.folds.groups.is_empty();
        self.folds_drawn = drawn;
        if !folded {
            return false;
        }
        self.folds.version += 1;
        if drawn {
            self.retain_shown();
        }
        true
    }

    /// Whether a fold takes a group's entries out here (`set_folds_drawn`).
    pub fn folds_drawn(&self) -> bool {
        self.folds_drawn
    }

    /// The order shown with the entries of its folded groups in it: what
    /// the pane lists, as its footer counts and weighs it; the order itself
    /// (`order`, the same) when nothing is folded.
    pub fn order_unfolded(&self) -> Arc<[EntryId]> {
        self.found_order(self.flat_order())
    }

    /// Fold or open the pane's groups as `mode` says, group `group` of
    /// those it shows (`groups`) for those that name one: whether anything
    /// changed. What was selected in a group folded is no longer.
    pub fn fold_groups(&mut self, group: Option<usize>, mode: FoldMode) -> bool {
        let by = self.sort.group_key();
        if by.is_none() {
            return false;
        }
        if self.folds.by != by {
            self.folds.by = by;
            self.folds.groups.clear();
        }
        let spans = self.groups();
        // Without one, the cursor's: the group whose span holds it.
        let group = group.or_else(|| {
            let cursor = self.selection.cursor?;
            let at = self.order().iter().position(|id| *id == cursor)?;
            spans
                .partition_point(|span| span.start <= at)
                .checked_sub(1)
        });
        let key = group
            .and_then(|n| spans.get(n))
            .map(|span| fold_key(span.group.as_deref()));
        let before = self.folds.groups.clone();
        let groups = &mut self.folds.groups;
        match (mode, key) {
            (FoldMode::UnfoldAll, _) => groups.clear(),
            (FoldMode::FoldOthers, Some(key)) => {
                groups.clear();
                groups.extend(
                    spans
                        .iter()
                        .map(|span| fold_key(span.group.as_deref()))
                        .filter(|other| *other != key),
                );
            }
            (FoldMode::Fold, Some(key)) => {
                groups.insert(key);
            }
            (FoldMode::Unfold, Some(key)) => {
                groups.remove(&key);
            }
            (FoldMode::Toggle, Some(key)) => {
                if !groups.remove(&key) {
                    groups.insert(key);
                }
            }
            (_, None) => {}
        }
        if self.folds.groups == before {
            return false;
        }
        self.folds.version += 1;
        self.retain_shown();
        true
    }

    /// What its sort, and its view (`show_fields`), read on demand and the
    /// entries of its order lack (ADR-131, 4), by listing: its own and
    /// those of the folders it opened in place, each once whole. Every
    /// entry is asked the first time, and again when the fields, the
    /// providers (`revision`) or a file (`Listing::fields_version`)
    /// changed, the reading before given up; else only what came or
    /// changed since (`Listing::updated_since`), the reading before going
    /// on: a folder whose names change does not walk all its entries
    /// again. A folder is never asked, nor a place. What was read is kept
    /// (`App::fields`) and the listing's; an answer not given (no
    /// provider, a read that failed) leaves the field unknown, asked again
    /// once the providers change.
    pub(crate) fn fields_to_read(&mut self, revision: u64) -> Vec<FieldsAsked> {
        let mut fields = self.sort.fields();
        for field in &self.fields_shown {
            if !fields.contains(field) {
                fields.push(field);
            }
        }
        let before = std::mem::take(&mut self.fields_asked);
        if fields.is_empty() || self.open.is_none() {
            for at in before {
                at.cancel.cancel();
            }
            return Vec::new();
        }
        let own = std::iter::once((&self.listing, self.open.as_ref()));
        let in_place =
            opened(&self.kept, &self.expanded).map(|(_, k)| (&k.listing, k.open.as_ref()));
        let (mut now, mut asked, mut carried) = (Vec::new(), Vec::new(), Vec::new());
        for (listing, open) in own.chain(in_place) {
            let id = listing.id();
            let was = before.iter().find(|at| at.listing == id);
            let Some(open) = open.filter(|_| listing.is_complete()) else {
                // Asked once whole; what was asked of it goes on.
                if let Some(was) = was {
                    now.push(was.clone());
                    carried.push(id);
                }
                continue;
            };
            let same = was.filter(|at| {
                at.fields == fields
                    && at.revision == revision
                    && at.fields_version == listing.fields_version()
            });
            let lacking = |e: &Entry| {
                if e.is_dir() || e.is_place() {
                    return None;
                }
                let lacks: Vec<&'static str> = fields
                    .iter()
                    .copied()
                    .filter(|field| e.metadata.value(field).is_none())
                    .collect();
                if lacks.is_empty() {
                    return None;
                }
                Some((
                    e.id,
                    open.path_of(e.id)?,
                    crate::fields::identity_of_entry(e),
                    lacks,
                ))
            };
            let entries: Vec<_> = match same.map(|at| listing.updated_since(at.version)) {
                Some(Some(since)) => {
                    let mut seen = crate::hashing::Set::<EntryId>::default();
                    since
                        .filter(|id| seen.insert(*id))
                        .filter_map(|id| listing.get(id))
                        .filter_map(lacking)
                        .collect()
                }
                // Asked anew, or what changed since is no longer told:
                // every entry.
                _ => listing.iter().filter_map(lacking).collect(),
            };
            let cancel = match same {
                Some(at) => {
                    carried.push(id);
                    at.cancel.clone()
                }
                None => self.location_lifetime.child(),
            };
            if !entries.is_empty() {
                asked.push(FieldsAsked {
                    pane: self.id,
                    listing: id,
                    cancel: cancel.clone(),
                    entries,
                });
            }
            now.push(FieldsAt {
                listing: id,
                fields: fields.clone(),
                revision,
                fields_version: listing.fields_version(),
                version: listing.version(),
                cancel,
            });
        }
        // Asked anew, or of a listing no longer in the order: given up.
        for at in before {
            if !carried.contains(&at.listing) {
                at.cancel.cancel();
            }
        }
        self.fields_asked = now;
        asked
    }

    /// Take what was read on demand (`App::read_wanted_fields`) into the
    /// listing it was read for, its own or a folder's it opened in place,
    /// while it holds it: each value of an entry whose file is still the
    /// one read (its identity, `fields::identity_of_entry`). An answer
    /// read before the file changed is not taken: the change asks it
    /// again (`fields_to_read`). Whether what the pane shows changed.
    pub(crate) fn take_fields(&mut self, listing: u64, values: Vec<FieldRead>) -> bool {
        let (target, opened) = if self.listing.id() == listing {
            (&mut self.listing, false)
        } else if let Some(kept) = self.kept.iter_mut().find(|k| k.listing.id() == listing) {
            (&mut kept.listing, true)
        } else {
            return false;
        };
        let values: Vec<_> = values
            .into_iter()
            .filter(|(id, _, read, _)| {
                target
                    .get(*id)
                    .is_some_and(|e| crate::fields::identity_of_entry(e) == *read)
            })
            .map(|(id, key, _, value)| (id, key, value))
            .collect();
        let changed = target.set_fields(values);
        if changed && opened {
            self.tree_version += 1;
        }
        changed
    }

    /// What its folder's version control is to say of its entries (ADR-134):
    /// asked once its listing is whole, again when its names change, it
    /// opens a folder in place, the providers change, or the provider says
    /// the answer it gave may have (`StatesProvider::generation`: its
    /// repository moved, a window came to the front); when a file is
    /// written in it, in a folder it opened or in a column above it, at
    /// once after a quiet while, else every `STATES_WRITTEN_PACE`. The
    /// columns above it its view shows are asked of with it (`above`). A
    /// place, a search or a remote mount (where each file's stat is a
    /// request) is not asked, and what was said of its entries goes; so
    /// does it once no provider is left.
    pub(crate) fn states_to_read(
        &mut self,
        providers: &crate::vcs::StatesProviders,
    ) -> Option<StatesAsked> {
        let folder = self.location().and_then(Location::path);
        let askable = folder
            .as_ref()
            .is_some_and(|folder| crate::fs::reach::of(folder).is_none())
            && !providers.is_empty();
        if !askable {
            if let Some(reading) = self.states_reading.take() {
                reading.cancel();
            }
            self.states_asked = None;
            if self.states_said.is_some() {
                self.forget_states();
            }
            return None;
        }
        if !self.listing.is_complete() || self.open.is_none() {
            return None;
        }
        let folder = folder?;
        let above: Vec<&Kept> = self
            .kept
            .iter()
            .filter(|kept| {
                kept.path
                    .as_deref()
                    .is_some_and(|path| path != folder && folder.starts_with(path))
            })
            .collect();
        let (mut names, mut written) = (0u64, 0u64);
        for listing in self
            .listings()
            .chain(above.iter().map(|kept| &kept.listing))
        {
            names = names
                .wrapping_add(listing.id())
                .wrapping_add(listing.names_version());
            written = written.wrapping_add(listing.rewritten_version());
        }
        let above: Vec<PathBuf> = above.iter().filter_map(|kept| kept.path.clone()).collect();
        let generation = self
            .states_said
            .map_or(0, |(id, key)| providers.generation(id, key));
        let at = (
            self.listing.id(),
            names,
            self.tree_members_version,
            providers.revision(),
            generation,
        );
        let now = std::time::Instant::now();
        if let Some((asked, was_written, when)) = &self.states_asked
            && *asked == at
            && (*was_written == written || now.duration_since(*when) < STATES_WRITTEN_PACE)
        {
            return None;
        }
        self.states_asked = Some((at, written, now));
        if let Some(reading) = self.states_reading.take() {
            reading.cancel();
        }
        self.states_asking += 1;
        let cancel = self.location_lifetime.child();
        self.states_reading = Some(cancel.clone());
        Some(StatesAsked {
            pane: self.id,
            listing: self.listing.id(),
            asking: self.states_asking,
            folder,
            above,
            cancel,
        })
    }

    /// Take what the version control said (`said`: who, and what, made
    /// ready on the worker; `None` where none knows the folder) into the
    /// entries of the listing it was asked for, if it is still the one
    /// shown and this the latest asking, of the folders opened in place
    /// below it and of the columns above it under the answer's root, each
    /// by its path from that root (`take_vcs`). Whether any changed.
    pub(crate) fn take_states(
        &mut self,
        listing: u64,
        asking: u64,
        said: Option<(&'static str, crate::vcs::States)>,
    ) -> bool {
        if self.listing.id() != listing || self.states_asking != asking {
            return false;
        }
        let Some(folder) = self.location().and_then(Location::path) else {
            return false;
        };
        let Some((id, states)) = said else {
            return self.forget_states();
        };
        self.states_said = Some((id, states.key()));
        let level_of = |path: &Path| -> Option<String> {
            let parts: Option<Vec<&str>> = path
                .strip_prefix(states.root())
                .ok()?
                .components()
                .map(|part| part.as_os_str().to_str())
                .collect();
            Some(parts?.join("/"))
        };
        let changed =
            level_of(&folder).is_some_and(|level| take_vcs(&mut self.listing, &states, &level));
        let mut kept_changed = false;
        for kept in &mut self.kept {
            if let Some(level) = kept.path.as_deref().and_then(level_of) {
                kept_changed |= take_vcs(&mut kept.listing, &states, &level);
            }
        }
        if kept_changed {
            self.tree_version += 1;
        }
        changed || kept_changed
    }

    /// Let go of what the version control said of the entries: each is
    /// said nothing of (`Value::Bool(false)`), which a listing keeps as any
    /// answer. Whether any changed.
    fn forget_states(&mut self) -> bool {
        self.states_said = None;
        let forget = |listing: &mut Listing| {
            let said: Vec<_> = listing
                .iter()
                .filter(|e| crate::vcs::State::of(e).is_some())
                .map(|e| {
                    (
                        e.id,
                        crate::entry::keys::VCS,
                        crate::entry::Value::Bool(false),
                    )
                })
                .collect();
            listing.set_fields(said)
        };
        let changed = forget(&mut self.listing);
        let mut kept_changed = false;
        for kept in &mut self.kept {
            kept_changed |= forget(&mut kept.listing);
        }
        if kept_changed {
            self.tree_version += 1;
        }
        changed || kept_changed
    }

    /// Moves whenever the groups folded change: what is shown with them.
    pub fn folds_version(&self) -> u64 {
        self.folds.version
    }

    /// Whether group `group` of those the pane shows is folded.
    pub fn is_folded(&self, group: Option<&crate::grouping::Group>) -> bool {
        self.any_folded() && self.folds.groups.contains(&fold_key(group))
    }

    /// The entries the filter keeps, in the order sorted.
    fn flat_order(&self) -> Arc<[EntryId]> {
        let all = self.unfiltered_order();
        if !self.is_filtered() {
            return all;
        }
        let filter = self.text_filter();
        let filter = filter.as_ref();
        let lens = self.lens_shown();
        let keeps = |e: &Entry| shown(filter, e);
        let dims = |e: &Entry| lens.is_some_and(|lens| dimmed(lens, e));
        let listings: Vec<&Listing> = self.listings().collect();
        let mut cache = self.filter_cache.lock();
        if let Some(c) = cache.as_mut()
            && let Some(moved) = retest(c, self.filter_tested, &listings, keeps, dims)
        {
            if !moved && !c.stale && Arc::ptr_eq(&c.from, &all) {
                return Arc::clone(&c.order);
            }
            let order: Arc<[EntryId]> = all
                .iter()
                .copied()
                .filter(|id| c.kept.contains(id))
                .collect();
            c.from = all;
            c.order = Arc::clone(&order);
            c.stale = false;
            return order;
        }
        // Made again: every entry listed tested.
        let entries = || listings.iter().flat_map(|listing| listing.iter());
        let kept: crate::hashing::Set<EntryId> =
            entries().filter(|e| keeps(e)).map(|e| e.id).collect();
        let dimmed: crate::hashing::Set<EntryId> =
            entries().filter(|e| dims(e)).map(|e| e.id).collect();
        let order: Arc<[EntryId]> = all.iter().copied().filter(|id| kept.contains(id)).collect();
        *cache = Some(FilterCache {
            filter: self.filter_tested,
            at: listings.iter().map(|l| (l.id(), l.version())).collect(),
            kept,
            dimmed,
            from: all,
            order: Arc::clone(&order),
            stale: false,
        });
        order
    }

    /// The lens shown: the one kept while the view is.
    fn lens_shown(&self) -> Option<&Lens> {
        self.leading
            .kept
            .as_ref()
            .map_or(self.read.as_ref(), |kept| kept.read.as_ref())
            .and_then(|r| r.lens.as_ref())
    }

    /// Where the first file whose metadata has not come yet is in the
    /// pane's order before the filter ([`Pane::unfiltered_order`]: a
    /// narrowing hides a file it cannot tell yet), looking from
    /// `from` on and then from the start. Whoever asks again after each
    /// batch of metadata starts where the last one was: hydration goes
    /// through the order, so the look is about a batch long, not the
    /// files known before it.
    pub fn file_unknown_from(&self, from: usize) -> Option<usize> {
        let order = self.unfiltered_order();
        let from = from.min(order.len());
        let unknown = |id: &EntryId| {
            self.entry(*id)
                .is_some_and(|e| !e.is_dir() && !e.is_place() && !e.metadata.is_filled())
        };
        order[from..]
            .iter()
            .position(unknown)
            .map(|i| from + i)
            .or_else(|| order[..from].iter().position(unknown))
    }

    /// Where the first selected file whose size is not known yet is in
    /// the selection, looking from `from` on and then from the start: a
    /// summary of the selection asks after each batch of metadata,
    /// starting where the last one was, and the selection is hydrated in
    /// its order, so the look is about a batch long.
    pub fn selected_unknown_from(&self, from: usize) -> Option<usize> {
        let items = &self.selection.items;
        let from = from.min(items.len());
        let unknown = |id: &EntryId| {
            self.entry(*id).is_some_and(|e| {
                !e.is_dir()
                    && e.metadata
                        .value(crate::entry::keys::SIZE)
                        .and_then(crate::entry::Value::as_u64)
                        .is_none()
            })
        };
        let (before, after) = (items.get_range(..from), items.get_range(from..));
        after
            .and_then(|after| after.iter().position(unknown).map(|i| from + i))
            .or_else(|| before.and_then(|before| before.iter().position(unknown)))
    }

    /// The sort every listing the pane shows is sorted with: its spec, and
    /// the keys made for it where it needs them (`SortSpec::needs_keys`).
    pub fn sorting(&self) -> crate::grouping::Sort {
        let mut sort = crate::grouping::Sort {
            spec: self.sort,
            keys: self.keys.clone().filter(|_| self.sort.needs_keys()),
            folders: None,
        };
        // Sorted or grouped by Folder: each entry's, as the location has it.
        if sort.wants_folders()
            && let Some(open) = &self.open
        {
            let open = Arc::clone(open);
            sort.folders = Some(crate::grouping::Folders(Arc::new(move |id| {
                open.path_of(id)?.parent().map(Path::to_path_buf)
            })));
        }
        sort
    }

    /// The keys made for the pane's sort, if any.
    pub fn keys(&self) -> Option<&crate::grouping::Keys> {
        self.keys.as_ref()
    }

    /// Sort with `keys`, made for `made_for`, from now on
    /// (`App::make_keys_again`).
    pub(crate) fn set_keys(
        &mut self,
        keys: Option<crate::grouping::Keys>,
        made_for: Option<KeysFor>,
    ) {
        // Grouped by another criterion, or no longer grouped: the groups
        // folded are forgotten (ADR-131, 2).
        let by = made_for.and_then(|(_, group, _)| group);
        if self.folds.by.is_some() && self.folds.by != by {
            self.folds = Folds {
                version: self.folds.version + 1,
                ..Folds::default()
            };
        }
        self.keys = keys;
        self.keys_for = made_for;
    }

    /// What the pane's keys are to be made for with the sorters at
    /// `revision`: the criteria, while its sort needs keys.
    pub(crate) fn keys_wanted_for(&self, revision: u64) -> Option<KeysFor> {
        self.sort
            .needs_keys()
            .then(|| (self.sort.key, self.sort.group_key(), revision))
    }

    /// Whether its keys are not those its sort wants (a tab opened with
    /// another's sort, a sorter turned on or off): made again at the next
    /// pump, whatever path changed the sort.
    pub(crate) fn keys_wanted(&self, revision: u64) -> bool {
        self.keys_wanted_for(revision) != self.keys_for
    }

    /// Where each group starts in what the pane shows (`order`), its label
    /// and its count (ADR-130); empty while it is not grouped. An entry of
    /// a folder opened in place is in the group of the folder; a pane
    /// laid out otherwise than its listing's order (a search as a tree)
    /// is not grouped.
    pub fn groups(&self) -> Arc<[crate::grouping::Span]> {
        self.shown_groups().1
    }

    /// The folders the pane opened in place (ADR-025) with their listings,
    /// shallowest first: what it shows of each beside its own.
    pub fn opened_listings(&self) -> impl Iterator<Item = (&Opened, &Listing)> + '_ {
        opened(&self.kept, &self.expanded).map(|(o, k)| (o, &k.listing))
    }

    /// `listing`'s order as the pane sorts it: the order `stands_in` places
    /// among, the pane's own listing's being `listed_order`.
    pub fn sorted_of(&self, listing: &Listing) -> Arc<[EntryId]> {
        listing.order_shown(self.sorting())
    }

    /// The listing's order as the pane sorts it (`Listing::order_shown`):
    /// what `stands` places in, before the folders opened in place, the
    /// filter and the folds make what the pane shows of it (`order`).
    pub fn listed_order(&self) -> Arc<[EntryId]> {
        self.listing.order_shown(self.sorting())
    }

    /// Where each of `likes`, what plans bring into the pane's folder as it
    /// will be (ADR-135), stands, if the pane would show it: not a hidden
    /// name while hidden files are not shown, nor one its filter would not
    /// keep (a term it cannot tell yet, as for an entry not read); placed
    /// in the listing's order (`listed_order`, `Listing::standing`), among
    /// the others as the order would have them, with the group it would be
    /// in where the pane groups.
    pub fn stands(&self, likes: &[&Entry]) -> Vec<Option<Stand>> {
        self.stands_in(&self.listing, likes)
    }

    /// `stands`, among the entries of `listing`, the pane's own or that of
    /// a folder it opened in place (`opened_listings`): placed in its order
    /// as the pane sorts it (`Listing::order_shown`), each keyed once
    /// (`keyed`).
    pub fn stands_in(&self, listing: &Listing, likes: &[&Entry]) -> Vec<Option<Stand>> {
        let (sort, _) = self.keying(listing);
        let keyed = self.keyed(&sort, likes);
        let shown: Vec<(usize, &crate::location::Keyed)> = keyed
            .iter()
            .enumerate()
            .filter_map(|(i, keyed)| Some((i, keyed.as_ref()?)))
            .collect();
        // In their order among themselves: each searched from where the
        // one before stands.
        let mut order: Vec<usize> = (0..shown.len()).collect();
        order.sort_by(|a, b| shown[*a].1.against(shown[*b].1));
        let them: Vec<&crate::location::Keyed> = order.iter().map(|&j| shown[j].1).collect();
        let at = listing
            .stand(&sort, &them)
            .expect("keyed for the sort it is placed by");
        let mut stands = vec![None; likes.len()];
        for (among, (j, at)) in order.into_iter().zip(at).enumerate() {
            let (i, keyed) = shown[j];
            stands[i] = Some(Stand {
                at,
                among,
                group: keyed.group().cloned(),
            });
        }
        stands
    }

    /// The sort what arrives in `listing` (the pane's own, or that of a
    /// folder it opened in place) is keyed for (ADR-135): that of its
    /// order shown (`Listing::sort_shown`); and what keys made for it hold
    /// for (`Keying`): that sort, whether hidden names are shown, and the
    /// filter as it stands.
    pub fn keying(&self, listing: &Listing) -> (crate::grouping::Sort, Keying) {
        let sort = listing.sort_shown(self.sorting());
        let keying = Keying {
            sort: sort.stamp(),
            show_hidden: self.sort.show_hidden,
            filter: self.filter_version,
        };
        (sort, keying)
    }

    /// Each of `likes` (what plans bring, as it will be) keyed for `sort`
    /// (`keying`) where the pane would show it: not a hidden name while
    /// hidden files are not shown, nor one its filter would not keep (a
    /// term it cannot tell yet, as for an entry not read). Placed by
    /// `Listing::stand` as the order changes, never keyed again while the
    /// keying holds.
    pub fn keyed(
        &self,
        sort: &crate::grouping::Sort,
        likes: &[&Entry],
    ) -> Vec<Option<crate::location::Keyed>> {
        let filter = self.text_filter();
        likes
            .iter()
            .map(|like| {
                let hidden = !self.sort.show_hidden && like.is_hidden();
                (!hidden && shown(filter.as_ref(), like))
                    .then(|| crate::location::Keyed::new(sort, like))
            })
            .collect()
    }

    /// Where `group`, which none of `spans` (the pane's groups, `groups`)
    /// is, would stand among them (ADR-135): before the first that comes
    /// after it, `spans.len()` after them all. What a heading of what
    /// arrives in a group no file is in yet is placed by.
    pub fn fresh_before(
        &self,
        group: &crate::grouping::Group,
        spans: &[crate::grouping::Span],
    ) -> usize {
        let spec = self.sorting().spec;
        spans.partition_point(|span| {
            crate::location::group_order(spec, span.group.as_deref(), Some(group)).is_lt()
        })
    }

    /// The groups of `order` (what the pane shows, folded groups' entries
    /// in it), its spans of the listing's order mapped onto it.
    fn groups_of(&self, order: &Arc<[EntryId]>) -> Arc<[crate::grouping::Span]> {
        let sort = self.sorting();
        if sort.grouping().is_none() {
            return Arc::from([]);
        }
        let spans = self.listing.spans_shown(&sort);
        let order = Arc::clone(order);
        let top = self.listing.order_shown(&sort);
        let mut cache = self.groups_cache.lock();
        if let Some(c) = cache.as_ref()
            && Arc::ptr_eq(&c.order, &order)
            && Arc::ptr_eq(&c.spans, &spans)
        {
            return Arc::clone(&c.groups);
        }
        let groups: Arc<[crate::grouping::Span]> = if Arc::ptr_eq(&order, &top) {
            Arc::clone(&spans)
        } else {
            groups_within(&order, &top, &spans, |id| self.listing.get(id).is_some())
                .map_or_else(|| Arc::from([]), Arc::from)
        };
        *cache = Some(GroupsCache {
            order,
            spans,
            groups: Arc::clone(&groups),
        });
        groups
    }

    /// `order` before the name filter and the narrowing: what the pane
    /// holds. A narrowing cannot tell a file whose metadata has not come
    /// yet, and does not show it until it can; no view draws it, so
    /// whoever narrows asks for it (`hydrate`) from here.
    pub fn unfiltered_order(&self) -> Arc<[EntryId]> {
        let sort = self.sorting();
        let top = self.listing.order_shown(&sort);
        if self.expanded.is_empty() {
            return top;
        }
        let opened: Vec<(&Opened, Arc<[EntryId]>)> = opened(&self.kept, &self.expanded)
            .map(|(o, k)| (o, k.listing.order_shown(&sort)))
            .collect();
        let from: Vec<Arc<[EntryId]>> = std::iter::once(Arc::clone(&top))
            .chain(opened.iter().map(|(_, ids)| Arc::clone(ids)))
            .collect();
        let mut cache = self.order_cache.lock();
        if let Some(c) = cache.as_ref()
            && c.expanded == self.expanded_version
            && c.from.len() == from.len()
            && c.from.iter().zip(&from).all(|(a, b)| Arc::ptr_eq(a, b))
        {
            return Arc::clone(&c.order);
        }
        // Each opened one by the entry it opens at: a walk looks up only
        // those, and allocates nothing for the rest.
        let at: HashMap<EntryId, usize> = opened
            .iter()
            .enumerate()
            .map(|(i, (o, _))| (o.at, i))
            .collect();
        let mut order = Vec::with_capacity(from.iter().map(|ids| ids.len()).sum());
        let mut stack: Vec<&[EntryId]> = vec![&top[..]];
        while let Some(ids) = stack.pop() {
            for (k, &id) in ids.iter().enumerate() {
                order.push(id);
                if let Some(&i) = at.get(&id) {
                    // The rest of this one after the opened one.
                    stack.push(&ids[k + 1..]);
                    stack.push(&opened[i].1[..]);
                    break;
                }
            }
        }
        let order: Arc<[EntryId]> = order.into();
        *cache = Some(OrderCache {
            expanded: self.expanded_version,
            from,
            order: Arc::clone(&order),
        });
        order
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        self.location_lifetime.cancel();
        self.leading.cancel.cancel();
        self.found.cancel.cancel();
        if let Some(open) = &self.open {
            open.close();
        }
    }
}

/// All panes, the tabs that group them, the windows that show the tabs,
/// and which ones are active (ADR-122).
pub struct Workspace {
    panes: Vec<Pane>,
    /// Never empty; every tab is in exactly one window, every pane in
    /// exactly one tab.
    windows: Vec<WindowTabs>,
    /// The window commands act in when they name no pane: the one the user
    /// last used.
    active_window: usize,
    /// The windows, the one used last at the end: who comes next when the
    /// active window closes (`remove_window_at`).
    used: Vec<WindowId>,
    next_id: u32,
    next_tab: u32,
    next_window: u32,
    /// Whether a UI keeps drawing the tab left until the tab shown is ready
    /// (`App::hold_tabs`): the panes left then stay open in their window's
    /// `leaving` until `App::settle_tabs`.
    pub(crate) holding: bool,
    /// Bumped whenever panes, tabs or windows come, go, move or change
    /// which is active, so the UI compares one number to follow them.
    revision: u64,
    /// Whether folders come before files (`location::FOLDERS_FIRST`),
    /// in every pane's sort: given each pane as it is made (`push_pane`)
    /// and to all of them when it changes (`set_folders_first`).
    folders_first: bool,
    /// The orders places remember (ADR-132), as the data file holds them:
    /// a pane showing one of them shows it so (`Pane::show_order`).
    orders: Orders,
}

/// A window's tabs, one shown at a time (ADR-122).
#[derive(Debug)]
pub struct WindowTabs {
    pub id: WindowId,
    /// Never empty.
    tabs: Vec<Tab>,
    /// Index in `tabs` of the tab shown.
    shown: usize,
    /// The tab the last tab opened beside the shown one went to, and the
    /// shown tab then: tabs opened one after another from the same tab
    /// line up after it in the order they were opened.
    opened_beside: Option<(TabId, TabId)>,
    /// Panes of this window's tabs no longer shown that are not suspended
    /// yet.
    pub(crate) leaving: Vec<PaneId>,
    /// What the UI keeps of the window's shape (its view, panels, sizes):
    /// opaque to the core, remembered with its tabs (`App::saved_windows`).
    shape: Option<toml::Value>,
    /// The window this one was made from: a UI starts it as that one is.
    from: Option<WindowId>,
}

impl WindowTabs {
    fn new(id: WindowId, tab: Tab, from: Option<WindowId>) -> Self {
        WindowTabs {
            id,
            tabs: vec![tab],
            shown: 0,
            opened_beside: None,
            leaving: Vec::new(),
            shape: None,
            from,
        }
    }

    /// Its tabs, in order.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// The tab it shows.
    pub fn shown(&self) -> &Tab {
        &self.tabs[self.shown]
    }

    /// The pane commands act on in this window.
    pub fn active_pane(&self) -> PaneId {
        self.shown().active
    }

    /// What the UI kept of its shape (`Workspace::set_shape`).
    pub fn shape(&self) -> Option<&toml::Value> {
        self.shape.as_ref()
    }

    /// The window it was made from, if it was.
    pub fn made_from(&self) -> Option<WindowId> {
        self.from
    }

    fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    /// Remove the tab at `t`, the window keeping a tab shown: the shown tab
    /// stays shown when one before it goes; the shown tab gone, the one that
    /// took its place is. Its last tab is never removed here.
    fn remove_tab_at(&mut self, t: usize) -> Tab {
        let tab = self.tabs.remove(t);
        if t < self.shown {
            self.shown -= 1;
        }
        self.shown = self.shown.min(self.tabs.len().saturating_sub(1));
        tab
    }

    fn insert_tab(&mut self, at: usize, tab: Tab) {
        let at = at.min(self.tabs.len());
        self.tabs.insert(at, tab);
        if at <= self.shown && self.tabs.len() > 1 {
            self.shown += 1;
        }
    }
}

impl Default for Workspace {
    fn default() -> Self {
        let mut w = Workspace {
            panes: Vec::new(),
            windows: Vec::new(),
            active_window: 0,
            used: Vec::new(),
            next_id: 0,
            next_tab: 0,
            next_window: 0,
            holding: false,
            revision: 0,
            folders_first: SortSpec::default().folders_first,
            orders: Orders::default(),
        };
        w.add_window(None, Vec::new());
        w
    }
}

impl Workspace {
    fn new_pane_id(&mut self) -> PaneId {
        let id = PaneId(self.next_id);
        self.next_id += 1;
        id
    }

    fn new_tab(&mut self, panes: Vec<PaneId>) -> Tab {
        let id = TabId(self.next_tab);
        self.next_tab += 1;
        let active = panes[0];
        Tab { id, panes, active }
    }

    /// Panes for a new tab: dormant ones, one per `(location, sort)`, or a
    /// new empty pane when there are none.
    fn new_panes(&mut self, panes: Vec<(Location, SortSpec)>) -> Vec<PaneId> {
        if panes.is_empty() {
            let id = self.new_pane_id();
            self.push_pane(Pane::new(id));
            return vec![id];
        }
        panes
            .into_iter()
            .map(|(location, sort)| {
                let id = self.new_pane_id();
                self.push_pane(Pane::dormant(id, location, sort));
                id
            })
            .collect()
    }

    /// Take `pane` among the workspace's, sorting as every pane does: the
    /// way in of every pane made, so none keeps a sort of folders that
    /// is no longer the user's (a closed tab's, reopened).
    fn push_pane(&mut self, mut pane: Pane) {
        pane.sort.folders_first = self.folders_first;
        self.panes.push(pane);
    }

    /// Whether folders come before files in every pane's sort.
    pub fn folders_first(&self) -> bool {
        self.folders_first
    }

    /// Put folders before files, or sort them together, in every pane
    /// (`location::FOLDERS_FIRST` changed): each one's order is made again
    /// on a worker (ADR-121), the one before shown until it is back.
    pub(crate) fn set_folders_first(&mut self, first: bool) {
        self.folders_first = first;
        for pane in &mut self.panes {
            pane.sort.folders_first = first;
        }
    }

    /// The orders places remember (ADR-132).
    pub fn orders(&self) -> &Orders {
        &self.orders
    }

    /// The order `at` remembers, if it remembers one.
    pub fn order_of(&self, at: &Location) -> Option<Order> {
        self.orders.of(at)
    }

    /// Remember `order` for `at`, or forget its own, and show it so in
    /// every pane that shows it: whether that changed anything.
    pub(crate) fn remember_order(&mut self, at: &Location, order: Option<Order>) -> bool {
        if !self.orders.set(at, order) {
            return false;
        }
        self.tell_panes(Some(at));
        true
    }

    /// Remember `view` as the one `at` is shown in (ADR-132), where it
    /// remembers its order, and show it so in every pane that shows it:
    /// whether that changed anything.
    pub(crate) fn remember_view(&mut self, at: &Location, view: Option<Arc<str>>) -> bool {
        if !self.orders.set_view(at, view) {
            return false;
        }
        self.tell_panes(Some(at));
        true
    }

    /// The view `at` remembers, if it remembers one.
    pub fn view_of(&self, at: &Location) -> Option<Arc<str>> {
        self.orders.view_of(at)
    }

    /// Take the orders as the data file says now (changed by another
    /// skwir, or read at the start): each pane shows its place's.
    pub(crate) fn set_orders(&mut self, orders: Orders) {
        self.orders = orders;
        self.tell_panes(None);
    }

    /// Show what `at` remembers (every place's, for none) in every pane
    /// showing it, and in the columns beside them.
    fn tell_panes(&mut self, at: Option<&Location>) {
        for pane in &mut self.panes {
            if let Some(shown) = pane.location().cloned()
                && at.is_none_or(|at| at == &shown)
            {
                pane.show_order(self.orders.of(&shown), self.orders.view_of(&shown));
            } else if at.is_none() {
                pane.show_order(None, None);
            }
            pane.columns_remember(&self.orders);
        }
    }

    /// Follow what a plan changed (`Orders::follow`): whether what the
    /// places remember changed.
    pub(crate) fn follow_orders(&mut self, changes: &[crate::op::Change]) -> bool {
        self.orders.follow(changes)
    }

    /// Take in what a look at the trash found (`Orders::forget_emptied`):
    /// whether what the places remember changed. A folder forgotten is
    /// shown by no pane: it is nowhere.
    pub(crate) fn forget_emptied(&mut self, found: &crate::orders::Emptied) -> bool {
        self.orders.forget_emptied(found)
    }

    /// A new pane in the tab shown: the other side of a split.
    pub fn add_pane(&mut self) -> PaneId {
        let tab = self.active_tab().id;
        self.add_pane_to(tab)
    }

    /// A new pane in tab `tab` (the tab shown when there is no such tab).
    fn add_pane_to(&mut self, tab: TabId) -> PaneId {
        let id = self.new_pane_id();
        self.push_pane(Pane::new(id));
        let (w, t) = self
            .tab_place(tab)
            .unwrap_or((self.active_window, self.windows[self.active_window].shown));
        self.windows[w].tabs[t].panes.push(id);
        self.revision += 1;
        id
    }

    /// A new pane in `from`'s tab with `from`'s lens (ADR-107): the other
    /// side of a split made from it, which then shows what it shows.
    pub fn add_pane_from(&mut self, from: PaneId) -> PaneId {
        let lens = self.pane(from).and_then(|p| p.lens().cloned());
        let tab = self.tab_of(from).unwrap_or_else(|| self.active_tab().id);
        let id = self.add_pane_to(tab);
        if let Some(p) = self.pane_mut(id) {
            p.carry(lens);
        }
        id
    }

    /// A new tab of one new pane, at position `at` (clamped) of the active
    /// window, not shown.
    pub fn add_tab(&mut self, at: usize) -> (TabId, PaneId) {
        let window = self.active_window_id();
        let (tab, panes) = self.add_dormant_tab_in(window, at, Vec::new());
        (tab, panes[0])
    }

    /// A tab of dormant panes at `at` of `window` (the active one when there
    /// is no such window), not shown.
    pub(crate) fn add_dormant_tab_in(
        &mut self,
        window: WindowId,
        at: usize,
        panes: Vec<(Location, SortSpec)>,
    ) -> (TabId, Vec<PaneId>) {
        let ids = self.new_panes(panes);
        let tab = self.new_tab(ids.clone());
        let id = tab.id;
        let w = self.window_index(window).unwrap_or(self.active_window);
        self.windows[w].insert_tab(at, tab);
        self.revision += 1;
        (id, ids)
    }

    /// A new window, last, of one tab of dormant panes (one new pane when
    /// `panes` is empty), made from `from`: not made active.
    pub(crate) fn add_window(
        &mut self,
        from: Option<WindowId>,
        panes: Vec<(Location, SortSpec)>,
    ) -> (WindowId, TabId, Vec<PaneId>) {
        let ids = self.new_panes(panes);
        let tab = self.new_tab(ids.clone());
        let tab_id = tab.id;
        let id = WindowId(self.next_window);
        self.next_window += 1;
        self.windows.push(WindowTabs::new(id, tab, from));
        // Not used yet: the first to wait, whoever is active stays.
        self.used.insert(0, id);
        self.revision += 1;
        (id, tab_id, ids)
    }

    /// Where a tab opened beside `window`'s shown one goes: right after it,
    /// or after the tabs already opened beside it.
    pub(crate) fn beside_position(&self, window: WindowId) -> usize {
        let Some(w) = self.window(window) else {
            return 0;
        };
        let shown = w.shown().id;
        match w.opened_beside {
            Some((from, last)) if from == shown => w.tab_index(last).map_or(w.shown + 1, |i| i + 1),
            _ => w.shown + 1,
        }
    }

    /// `tab` was opened beside the shown tab of its window.
    pub(crate) fn opened_beside(&mut self, tab: TabId) {
        if let Some(w) = self.window_of_tab(tab).and_then(|w| self.window_index(w)) {
            let window = &mut self.windows[w];
            window.opened_beside = Some((window.shown().id, tab));
        }
    }

    pub(crate) fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.revision += 1;
        self.windows
            .iter_mut()
            .flat_map(|w| w.tabs.iter_mut())
            .find(|t| t.id == id)
    }

    /// Close pane `id`. Refused for the last pane of its window: a window
    /// keeps a tab, and a tab a pane.
    pub fn close_pane(&mut self, id: PaneId) -> bool {
        let Some(i) = self.panes.iter().position(|p| p.id == id) else {
            return false;
        };
        let Some((w, t)) = self.place_of(id) else {
            return false;
        };
        let window = &self.windows[w];
        if window.tabs.len() == 1 && window.tabs[t].panes.len() == 1 {
            return false;
        }
        self.panes.remove(i);
        self.revision += 1;
        let window = &mut self.windows[w];
        let tab = &mut window.tabs[t];
        let at = tab.panes.iter().position(|p| *p == id).expect("in its tab");
        tab.panes.remove(at);
        if tab.panes.is_empty() {
            window.remove_tab_at(t);
        } else if tab.active == id {
            // Closing the active pane activates the one that took its place.
            tab.active = tab.panes[at.min(tab.panes.len() - 1)];
        }
        true
    }

    /// The window and tab indices `pane` is in.
    fn place_of(&self, pane: PaneId) -> Option<(usize, usize)> {
        self.windows.iter().enumerate().find_map(|(w, window)| {
            window
                .tabs
                .iter()
                .position(|t| t.panes.contains(&pane))
                .map(|t| (w, t))
        })
    }

    /// The window and tab indices of `tab`.
    fn tab_place(&self, tab: TabId) -> Option<(usize, usize)> {
        self.windows
            .iter()
            .enumerate()
            .find_map(|(w, window)| window.tab_index(tab).map(|t| (w, t)))
    }

    /// Take `panes` out of the workspace, in their order.
    fn take_panes(&mut self, panes: &[PaneId]) -> Vec<Pane> {
        let mut taken = Vec::with_capacity(panes.len());
        for pane in panes {
            if let Some(i) = self.panes.iter().position(|p| p.id == *pane) {
                taken.push(self.panes.remove(i));
            }
        }
        taken
    }

    /// Take tab `id` and its panes out, in the tab's order, with the index
    /// it had in its window. Refused for the last tab of a window. Dropping
    /// the panes closes what they show.
    pub(crate) fn close_tab(&mut self, id: TabId) -> Option<(usize, Tab, Vec<Pane>)> {
        let (w, t) = self.tab_place(id)?;
        if self.windows[w].tabs.len() <= 1 {
            return None;
        }
        let tab = self.windows[w].remove_tab_at(t);
        self.revision += 1;
        let panes = self.take_panes(&tab.panes);
        Some((t, tab, panes))
    }

    /// Put tab `id` at position `to` (clamped) of its window. The shown tab
    /// stays shown.
    pub fn move_tab(&mut self, id: TabId, to: usize) -> bool {
        let Some((w, from)) = self.tab_place(id) else {
            return false;
        };
        let window = &mut self.windows[w];
        let shown = window.shown().id;
        let tab = window.tabs.remove(from);
        let to = to.min(window.tabs.len());
        window.tabs.insert(to, tab);
        window.shown = window.tab_index(shown).expect("still there");
        self.revision += 1;
        true
    }

    /// Move tab `id` to `window`, at `to` (clamped; last when `None`),
    /// shown there, and make that window active. A tab alone in its window
    /// takes the window with it: its id is returned. The tab shown there
    /// before is left (`App::move_tab_to_window` lets it sleep). Refused
    /// for a window or a tab that is not there, or a tab already in
    /// `window`.
    pub(crate) fn move_tab_to_window(
        &mut self,
        id: TabId,
        window: WindowId,
        to: Option<usize>,
    ) -> Result<Option<WindowId>, &'static str> {
        let (w, t) = self.tab_place(id).ok_or("no such tab")?;
        let dest = self.window_index(window).ok_or("no such window")?;
        if dest == w {
            return Err("the tab is in that window already");
        }
        let source = &mut self.windows[w];
        let emptied = (source.tabs.len() == 1).then_some(source.id);
        let tab = if emptied.is_some() {
            source.tabs.remove(0)
        } else {
            source.remove_tab_at(t)
        };
        // Shown where it goes: none of its panes is left any more.
        source.leaving.retain(|p| !tab.panes.contains(p));
        {
            let dest = &mut self.windows[dest];
            let at = to.unwrap_or(dest.tabs.len()).min(dest.tabs.len());
            dest.tabs.insert(at, tab);
            dest.shown = at;
            dest.opened_beside = None;
        }
        if let Some(gone) = emptied
            && let Some(i) = self.window_index(gone)
        {
            self.remove_window_at(i);
        }
        let dest = self.window_index(window).expect("still there");
        self.make_active(dest);
        self.revision += 1;
        Ok(emptied)
    }

    /// Take tab `id` out of its window into a new one, made from it, shown
    /// there and active. Refused for a tab alone in its window.
    pub(crate) fn tab_to_new_window(&mut self, id: TabId) -> Result<WindowId, &'static str> {
        let (w, t) = self.tab_place(id).ok_or("no such tab")?;
        let source = &mut self.windows[w];
        if source.tabs.len() == 1 {
            return Err("the tab is alone in its window");
        }
        let tab = source.remove_tab_at(t);
        source.leaving.retain(|p| !tab.panes.contains(p));
        let from = source.id;
        let window = WindowId(self.next_window);
        self.next_window += 1;
        self.windows.push(WindowTabs::new(window, tab, Some(from)));
        self.make_active(self.windows.len() - 1);
        self.revision += 1;
        Ok(window)
    }

    /// Window `i` is the one in use now.
    fn make_active(&mut self, i: usize) {
        self.active_window = i;
        let id = self.windows[i].id;
        self.used.retain(|used| *used != id);
        self.used.push(id);
    }

    /// Take window `i` out. The active window gone, the one used last before
    /// it is active (ADR-122: "the one the user last used"), not whichever
    /// took its place.
    fn remove_window_at(&mut self, i: usize) -> WindowTabs {
        let was_active = i == self.active_window;
        let window = self.windows.remove(i);
        self.used.retain(|used| *used != window.id);
        if i < self.active_window {
            self.active_window -= 1;
        }
        if was_active {
            let next = self.used.last().and_then(|id| self.window_index(*id));
            self.active_window = next.unwrap_or(0);
        }
        self.revision += 1;
        window
    }

    /// Take window `id` out, its tabs and their panes, with the index it
    /// had. Refused for the last window. Dropping the panes closes what
    /// they show.
    pub(crate) fn close_window(&mut self, id: WindowId) -> Option<(usize, WindowTabs, Vec<Pane>)> {
        if self.windows.len() <= 1 {
            return None;
        }
        let i = self.window_index(id)?;
        let window = self.remove_window_at(i);
        let panes: Vec<PaneId> = window
            .tabs
            .iter()
            .flat_map(|t| t.panes.iter().copied())
            .collect();
        let panes = self.take_panes(&panes);
        Some((i, window, panes))
    }

    /// Changes whenever panes, tabs or windows come, go, move, or another
    /// one is made active.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The active window's tabs, in order.
    pub fn tabs(&self) -> &[Tab] {
        &self.windows[self.active_window].tabs
    }

    /// Every window's tabs, window after window.
    pub fn all_tabs(&self) -> impl Iterator<Item = &Tab> {
        self.windows.iter().flat_map(|w| w.tabs.iter())
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.all_tabs().find(|t| t.id == id)
    }

    /// Where tab `id` is among its window's tabs.
    pub fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tab_place(id).map(|(_, t)| t)
    }

    /// The tab the active window shows.
    pub fn active_tab(&self) -> &Tab {
        self.windows[self.active_window].shown()
    }

    /// The tab `pane` belongs to.
    pub fn tab_of(&self, pane: PaneId) -> Option<TabId> {
        self.place_of(pane).map(|(w, t)| self.windows[w].tabs[t].id)
    }

    /// The window tab `tab` is in.
    pub fn window_of_tab(&self, tab: TabId) -> Option<WindowId> {
        self.tab_place(tab).map(|(w, _)| self.windows[w].id)
    }

    /// The window `pane` is in.
    pub fn window_of(&self, pane: PaneId) -> Option<WindowId> {
        self.place_of(pane).map(|(w, _)| self.windows[w].id)
    }

    /// Show tab `id` in its window. Only the order changes:
    /// `App::select_tab` suspends the panes of the tab left and opens those
    /// of the tab shown.
    pub(crate) fn set_active_tab(&mut self, id: TabId) -> bool {
        match self.tab_place(id) {
            Some((w, t)) => {
                let window = &mut self.windows[w];
                window.shown = t;
                // Tabs opened beside another line up after it only until
                // another tab is shown.
                window.opened_beside = None;
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    /// The windows, in the order they were made.
    pub fn windows(&self) -> &[WindowTabs] {
        &self.windows
    }

    pub fn window(&self, id: WindowId) -> Option<&WindowTabs> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub(crate) fn window_mut(&mut self, id: WindowId) -> Option<&mut WindowTabs> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    fn window_index(&self, id: WindowId) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    /// The window commands act in when they name no pane.
    pub fn active_window(&self) -> &WindowTabs {
        &self.windows[self.active_window]
    }

    pub fn active_window_id(&self) -> WindowId {
        self.windows[self.active_window].id
    }

    /// Make `id` the active window: the user went to it.
    pub fn set_active_window(&mut self, id: WindowId) -> bool {
        match self.window_index(id) {
            Some(i) => {
                if i != self.active_window {
                    self.make_active(i);
                    self.revision += 1;
                }
                true
            }
            None => false,
        }
    }

    /// Keep what the UI says of window `id`'s shape, remembered with its
    /// tabs. Not a change of the workspace: the revision stays.
    pub fn set_shape(&mut self, id: WindowId, shape: Option<toml::Value>) {
        if let Some(i) = self.window_index(id) {
            self.windows[i].shape = shape;
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

    /// The active window's active pane.
    pub fn active_id(&self) -> PaneId {
        self.active_tab().active
    }

    /// Make `id` the active pane, and its window the active window.
    /// Refused for a pane of a tab not shown: showing it is
    /// `App::select_tab`, which opens its panes.
    pub fn set_active(&mut self, id: PaneId) -> bool {
        let Some((w, t)) = self.place_of(id) else {
            return false;
        };
        if self.windows[w].shown != t {
            return false;
        }
        let tab = &mut self.windows[w].tabs[t];
        if tab.active != id || self.active_window != w {
            tab.active = id;
            self.make_active(w);
            self.revision += 1;
        }
        true
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.iter().find(|p| p.id == id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    /// Pump every pane, each for at most `PUMP_BUDGET`, and say which ones
    /// changed and whether events are still waiting.
    pub fn pump_report(&mut self) -> PumpReport {
        let mut r = PumpReport::default();
        // A folder shown that moved, whatever moved it, takes along what it
        // and the folders it holds remember (ADR-132): looked for only
        // while any place remembers an order.
        let remembers = !self.orders.is_empty();
        let mut moved = Vec::new();
        for p in &mut self.panes {
            let before = if remembers {
                p.location().and_then(Location::path)
            } else {
                None
            };
            let pumped = p.pump(PUMP_BUDGET);
            if let Some(before) = before
                && let Some(now) = p.location().and_then(Location::path)
                && now != before
            {
                moved.push((before, now));
            }
            r.leads.extend(p.folders_to_ask());
            let n = pumped + usize::from(p.take_leads()) + usize::from(p.take_found_tree());
            if n > 0 {
                r.applied += n;
                r.panes_changed.push(p.id);
            }
            if let Some(to) = p.listing.take_redirect()
                && let Some(from) = p.location()
            {
                r.redirects.push(Redirect {
                    pane: p.id,
                    from: from.clone(),
                    to,
                });
            }
            if let Some(location) = p.take_listed_for_other_hidden() {
                r.hidden_moved.push((p.id, location));
            }
            r.backlog |= p.backlog();
            r.jobs
                .extend(p.jobs_wanted().into_iter().map(|job| (p.id, job)));
            r.found.extend(p.found_tree_asked());
        }
        for (from, to) in moved {
            r.orders_moved |= self.orders.moved(&from, &to);
        }
        r
    }
}

/// How long one pump may apply a pane's events: a quarter of a frame at
/// 60 Hz, so the UI thread that pumps still draws while a large folder
/// streams in. The rest waits for the next pump (`PumpReport::backlog`).
pub const PUMP_BUDGET: std::time::Duration = std::time::Duration::from_millis(4);

/// What a pump did.
#[derive(Debug, Default)]
pub struct PumpReport {
    pub applied: usize,
    pub panes_changed: Vec<PaneId>,
    /// Events are still waiting: pump again soon, without waiting for the
    /// location to wake the caller (it already has).
    pub backlog: bool,
    /// Panes whose location said it is reached at another
    /// (`ListingEvent::Redirect`): `App::pump_report` shows it there.
    pub redirects: Vec<Redirect>,
    /// Panes showing a location that follows their hidden-files switch,
    /// which moved since it was opened, however it did: `App::pump_report`
    /// opens it again.
    pub hidden_moved: Vec<(PaneId, Location)>,
    /// The jobs for workers that what the panes show wants
    /// ([`Pane::jobs_wanted`]): `App::pump_report` runs them. A pump that
    /// leaves them shows no folder it opened, nor an order asked anew.
    /// Each with the pane it is for: its wake is that pane's.
    pub jobs: Vec<(PaneId, crate::location::ListingJob)>,
    /// The folders whose way to what a lens keeps is still to be found
    /// out: `App::pump_report` hands them to a worker.
    pub leads: Vec<LeadsAsked>,
    /// What places remember of their order moved with a folder shown
    /// (ADR-132): `App::pump_report` writes it down.
    pub orders_moved: bool,
    /// The searches to lay out as trees: `App::pump_report` hands them to
    /// a worker.
    pub found: Vec<FoundAsked>,
}

/// A pane's location said it is reached at another.
#[derive(Clone, Debug)]
pub struct Redirect {
    pub pane: PaneId,
    /// What the pane showed: replaced only while it still shows it.
    pub from: Location,
    pub to: Location,
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
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let mut sel = Selection::default();
        sel.set([id(1), id(2), id(3)]);
        sel.remove(id(1));
        sel.remove(id(3));
        assert_eq!((sel.anchor, sel.cursor), (Some(id(2)), Some(id(2))));
    }

    /// Back or Forward whose step went from the history meanwhile (a
    /// navigation from elsewhere between reading it and showing it) makes
    /// what it shows a step of its own: the position stays in the history.
    #[test]
    fn a_step_gone_meanwhile_is_shown_as_a_new_one() {
        let at = |name: &str| Location::from_path(format!("/example/{name}"));
        let mut p = Pane::new(PaneId(1));
        for name in ["a", "b", "c"] {
            p.go_on(at(name), None);
        }
        // Back read step 1 (b); meanwhile the pane went from a to d.
        p.history_pos = 0;
        p.go_on(at("d"), None);
        p.go_to(1, &at("b"), None);
        assert_eq!(p.history, [at("a"), at("d"), at("b")]);
        assert_eq!(p.history_pos, 2);
        p.go_to(7, &at("c"), None);
        assert_eq!(p.history[p.history_pos], at("c"));
        // Still there, it is gone to.
        p.go_to(0, &at("a"), None);
        assert_eq!(p.history_pos, 0);
    }

    /// Renames that arrive together are followed in their order: a chain
    /// to its end, a swap back to where it was.
    #[test]
    fn a_selection_follows_renames_in_their_order() {
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let mut sel = Selection::default();
        sel.set([id(1), id(5)]);
        sel.rekey(&[
            (id(1), id(2)),
            (id(2), id(3)),
            (id(5), id(6)),
            (id(6), id(5)),
        ]);
        assert_eq!(sel.iter().collect::<Vec<_>>(), [id(3), id(5)]);
        assert_eq!((sel.anchor, sel.cursor), (Some(id(3)), Some(id(5))));
    }

    /// A folder drawn only as the way (`Ways`) is no next file, and what
    /// a sweep passes over is what it finds, however the steps are drawn.
    #[test]
    fn a_way_is_neither_the_next_file_nor_swept() {
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let order: Arc<[EntryId]> = (1..=6).map(id).collect();
        let ways: crate::hashing::Set<EntryId> = [id(3), id(4)].into_iter().collect();
        let ways = Some(&ways);
        let without = |gone: &'static [u64]| move |e: EntryId| !gone.contains(&e.file.ino);

        // The file after it is past the folders; with none after, the one
        // before.
        let mut sel = Selection::default();
        sel.set([id(2)]);
        sel.retain_near(without(&[2]), &order, false, ways);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(5)), vec![5]));
        // A folder closed takes the cursor, and is not selected.
        sel.set([id(5)]);
        sel.retain_near(without(&[5]), &order, true, ways);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(4)), vec![]));
        // Nothing but ways left: the cursor is on one, nothing selected.
        sel.set([id(1)]);
        sel.retain_near(without(&[1, 2, 5, 6]), &order, false, ways);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(3)), vec![]));

        // A sweep over them, step by step and drawn whole.
        for whole in [false, true] {
            let mut sel = Selection::default();
            sel.set([id(2)]);
            sel.sweep_start_at(false, 1, &order, ways);
            for at in 2..=4 {
                if whole {
                    sel.follow(order[at], &order, ways);
                } else {
                    sel.follow_at(order[at], at, &order, ways);
                }
            }
            assert_eq!(items(&sel), vec![2, 5], "drawn whole: {whole}");
            assert_eq!(sel.cursor, Some(id(5)));
        }
    }

    /// A cursor whose entry leaves the view goes to the entry after it, or
    /// before it at the end (or before it first, `back`, as a folder closed
    /// in place takes it); an emptied selection follows it there, a picked
    /// one stays.
    #[test]
    fn a_cursor_whose_entry_goes_takes_its_neighbour() {
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let order: Vec<EntryId> = (1..=5).map(id).collect();
        let without = |gone: &'static [u64]| move |e: EntryId| !gone.contains(&e.file.ino);
        let mut sel = Selection::default();

        sel.set([id(3)]);
        sel.retain_near(without(&[3]), &order, false, None);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(4)), vec![4]));
        // The last one: the one before it.
        sel.set([id(5)]);
        sel.retain_near(without(&[5]), &order, false, None);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(4)), vec![4]));
        // Several in a row, the cursor on the last: what follows them.
        sel.set([id(2), id(3), id(4)]);
        sel.retain_near(without(&[2, 3, 4]), &order, false, None);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(5)), vec![5]));
        // A pick that keeps some: the cursor moves, the pick stays.
        sel.set([id(2), id(4)]);
        sel.retain_near(without(&[4]), &order, false, None);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(5)), vec![2]));
        // Back: before it first.
        sel.set([id(3)]);
        sel.retain_near(without(&[3]), &order, true, None);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(2)), vec![2]));
        // A cursor with nothing selected moves and selects nothing.
        sel.set([id(3)]);
        sel.clear();
        sel.retain_near(without(&[3]), &order, false, None);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(4)), vec![]));
        // Nothing left: no cursor.
        sel.set([id(3)]);
        sel.retain_near(|_| false, &order, false, None);
        assert_eq!((sel.cursor, items(&sel)), (None, vec![]));
    }

    /// The pane moves the cursor along the order it last showed: a file
    /// trashed under the cursor gives it to the next one.
    #[test]
    fn a_file_gone_under_the_cursor_gives_it_to_the_next() {
        use crate::entry::EntryKind;
        use crate::location::ListingEvent;
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let mut p = Pane::new(PaneId(1));
        p.listing.apply(ListingEvent::Added(
            [(1, "a.txt"), (2, "b.txt"), (3, "c.txt")]
                .into_iter()
                .map(|(ino, name)| Entry::new(id(ino), name, EntryKind::File))
                .collect(),
        ));
        // What a view shows, read as it draws.
        assert_eq!(p.order().len(), 3);
        p.selection.set([id(2)]);
        p.listing.apply(ListingEvent::Removed(vec![id(2)]));
        p.retain_shown();
        assert_eq!(
            (p.selection.cursor, items(&p.selection)),
            (Some(id(3)), vec![3])
        );
        p.order();
        p.listing.apply(ListingEvent::Removed(vec![id(3)]));
        p.retain_shown();
        assert_eq!(
            (p.selection.cursor, items(&p.selection)),
            (Some(id(1)), vec![1])
        );
    }

    /// What the pane stops showing leaves the selection: a dot file once
    /// hidden files are hidden, one renamed to a dot name, one the filter
    /// leaves out. Delete must never act on a file the user cannot see.
    #[test]
    fn what_is_no_longer_shown_leaves_the_selection() {
        use crate::entry::EntryKind;
        use crate::location::ListingEvent;
        let file = |ino, name: &str| {
            Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                name,
                EntryKind::File,
            )
        };
        let mut p = Pane::new(PaneId(1));
        p.listing.apply(ListingEvent::Added(vec![
            file(1, ".profile"),
            file(2, "it's here.txt"),
            file(3, "Été.md"),
        ]));
        p.set_show_hidden(true);
        p.selection.set([1, 2, 3].map(|ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        }));
        p.set_show_hidden(false);
        assert_eq!(items(&p.selection), [2, 3]);
        p.listing
            .apply(ListingEvent::Updated(vec![file(3, ".Été.md")]));
        p.retain_shown();
        assert_eq!(items(&p.selection), [2]);
        p.selection.set([2].map(|ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        }));
        p.set_filter(Some("ÉTÉ"), None);
        assert!(p.selection.is_empty());
    }

    /// The weight asks after each batch of metadata whether a file is
    /// still unknown, from where the last one was: it finds one wherever
    /// it is, before or after, and none once every file is known.
    #[test]
    fn a_file_unknown_is_found_from_where_the_last_one_was() {
        use crate::entry::{EntryKind, Provenance, Value, keys};
        use crate::location::ListingEvent;
        let entry = |ino, name: &str, kind, size: Option<u64>| {
            let mut e = Entry::new(
                EntryId {
                    file: crate::FileId::new(1, ino),
                    at: 0,
                },
                name,
                kind,
            );
            if let Some(size) = size {
                // Read whole, as a hydration answers: stat-ed and filled.
                e.metadata
                    .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
                e.metadata.mark_filled();
            }
            e
        };
        let mut p = Pane::new(PaneId(1));
        p.listing.apply(ListingEvent::Added(vec![
            entry(1, "a folder", EntryKind::Directory, None),
            entry(2, "b known.txt", EntryKind::File, Some(3)),
            entry(3, "c it's unknown.txt", EntryKind::File, None),
            entry(4, "d known.txt", EntryKind::File, Some(5)),
            entry(5, "é unknown.md", EntryKind::File, None),
        ]));
        // Folders are not waited for; the order is by name.
        assert_eq!(p.file_unknown_from(0), Some(2));
        assert_eq!(p.file_unknown_from(3), Some(4));
        // Past the last one, from the start again; past the end too.
        p.listing.apply(ListingEvent::Updated(vec![entry(
            5,
            "é unknown.md",
            EntryKind::File,
            Some(8),
        )]));
        assert_eq!(p.file_unknown_from(3), Some(2));
        assert_eq!(p.file_unknown_from(99), Some(2));
        // The selection's, in its own order: a folder has no size to wait
        // for, nor a file known.
        p.selection.set([5, 1, 3, 2].map(|ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        }));
        assert_eq!(p.selected_unknown_from(0), Some(2));
        assert_eq!(p.selected_unknown_from(3), Some(2));
        p.selection.set([1, 2, 5].map(|ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        }));
        assert_eq!(p.selected_unknown_from(1), None);
        p.listing.apply(ListingEvent::Updated(vec![entry(
            3,
            "c it's unknown.txt",
            EntryKind::File,
            Some(1),
        )]));
        assert_eq!(p.file_unknown_from(2), None);
    }

    #[test]
    fn a_name_filter_ignores_case_unless_it_has_a_capital() {
        let holds = |name: &str, filter: &str| name_holds(OsStr::new(name), filter);
        assert!(holds("README.md", "read"));
        assert!(holds("it's a Report.txt", "s a rep"));
        assert!(!holds("README.md", "Read"), "a capital: case counts");
        assert!(holds("Été 2024.jpg", "été"), "beyond ASCII, folded too");
        assert!(holds("KELVIN", "kel"));
        assert!(holds("anything", ""));
        assert!(!holds("short", "much longer"));
    }

    fn items(sel: &Selection) -> Vec<u64> {
        sel.iter().map(|id| id.file.ino).collect()
    }

    /// A location of entries from many folders, whose paths it knows.
    struct Searched {
        location: Location,
        events: crossbeam_channel::Receiver<crate::location::ListingEvent>,
        paths: crate::hashing::Map<EntryId, PathBuf>,
    }

    impl OpenLocation for Searched {
        fn location(&self) -> &Location {
            &self.location
        }
        fn events(&self) -> &crossbeam_channel::Receiver<crate::location::ListingEvent> {
            &self.events
        }
        fn hydrate(&self, _: &[EntryId], _: crate::scheduler::Priority) {}
        fn path_of(&self, id: EntryId) -> Option<PathBuf> {
            self.paths.get(&id).cloned()
        }
        fn refresh(&self) {}
        fn close(&self) {}
    }

    /// Each entry of a selection of a whole listing is found at its path by
    /// its name and file, half of them under one name as a search lists
    /// `README.md` from every folder: never by a walk of the listing for
    /// each, which was the square of its size.
    #[test]
    fn every_entry_of_a_large_listing_is_found_at_its_path() {
        use crate::entry::EntryKind;
        use crate::location::ListingEvent;
        let n = 50_000u64;
        let (mut entries, mut paths) = (Vec::new(), crate::hashing::Map::default());
        for i in 0..n {
            let id = EntryId {
                file: crate::FileId::new(1, i),
                at: i,
            };
            let (name, path) = if i % 2 == 0 {
                ("README.md".to_owned(), format!("/s/{i}/README.md"))
            } else {
                (format!("f{i}"), format!("/s/f{i}"))
            };
            entries.push(Entry::new(id, name, EntryKind::File));
            paths.insert(id, PathBuf::from(path));
        }
        let mut p = Pane::new(PaneId(1));
        p.listing.apply(ListingEvent::Added(entries));
        let (_sink, events) = crossbeam_channel::unbounded();
        let asked: Vec<(EntryId, PathBuf)> = paths.iter().map(|(id, p)| (*id, p.clone())).collect();
        p.open = Some(Arc::new(Searched {
            location: Location::new("search", "/"),
            events,
            paths,
        }));
        let started = std::time::Instant::now();
        for (id, path) in &asked {
            assert_eq!(p.entry_at(id.file, path).map(|e| e.id), Some(*id));
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        // Another file at a listed path is not that entry.
        assert!(
            p.entry_at(crate::FileId::new(1, n + 1), Path::new("/s/0/README.md"))
                .is_none()
        );
    }

    /// A change adds and takes out, few or many, in one revision: the rest
    /// keeps its order, the cursor goes to the last added, the anchor to
    /// the first item once its own goes.
    #[test]
    fn a_change_adds_and_takes_out_leaving_the_rest_in_order() {
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let mut sel = Selection::default();
        sel.set((1..=100).map(id));
        let revision = sel.revision();
        // Many taken out (one pass), two added.
        let removed: Vec<EntryId> = (1..=60).map(id).collect();
        sel.change(&[id(200), id(201)], &removed);
        assert_ne!(sel.revision(), revision);
        let mut expected: Vec<u64> = (61..=100).collect();
        expected.extend([200, 201]);
        assert_eq!(items(&sel), expected);
        assert_eq!((sel.anchor, sel.cursor), (Some(id(61)), Some(id(201))));
        assert!(sel.picked());
        // Few taken out, none added: the cursor stays.
        sel.change(&[], &[id(200), id(201), id(61)]);
        assert_eq!(items(&sel), (62..=100).collect::<Vec<_>>());
        assert_eq!((sel.anchor, sel.cursor), (Some(id(62)), Some(id(201))));
        // Down to one, it follows the cursor again, as `set` has it.
        let rest: Vec<EntryId> = (63..=100).map(id).collect();
        sel.change(&[], &rest);
        assert_eq!(items(&sel), [62]);
        assert!(!sel.picked());
    }

    #[test]
    fn a_move_carries_a_selection_that_follows_and_leaves_a_picked_one() {
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let order: Vec<EntryId> = (1..=5).map(id).collect();
        let mut sel = Selection::default();
        sel.set([id(1)]);
        assert!(!sel.picked());
        sel.follow(id(2), &order, None);
        assert_eq!(items(&sel), [2]);
        // The first pick starts from nothing: the item followed is not one.
        sel.pick(id(2));
        assert!(sel.picked());
        sel.follow(id(3), &order, None);
        sel.pick(id(3));
        sel.follow(id(4), &order, None);
        assert_eq!(items(&sel), [2, 3]);
        assert_eq!(sel.cursor, Some(id(4)));
        // Unpicking the last one gives the selection back to the cursor.
        sel.pick(id(3));
        sel.pick(id(2));
        assert!(!sel.picked());
        assert_eq!(items(&sel), [2]);
        sel.follow(id(5), &order, None);
        assert_eq!(items(&sel), [5]);
    }

    #[test]
    fn the_standard_keys_are_unchanged() {
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let mut sel = Selection::default();
        sel.set([id(1)]);
        // Ctrl+click on the one selected item deselects it, as it did.
        sel.toggle(id(1));
        assert!(sel.is_empty());
        sel.set([id(1)]);
        sel.toggle(id(2));
        assert_eq!(items(&sel), [1, 2]);
        assert!(sel.picked());
        sel.set([id(3)]);
        assert!(!sel.picked());
    }

    #[test]
    fn a_sweep_covers_the_range_over_what_was_picked_and_ends_on_any_other_change() {
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let order: Vec<EntryId> = (1..=6).map(id).collect();
        let mut sel = Selection::default();
        sel.set([id(1)]);
        sel.pick(id(1));
        sel.follow(id(3), &order, None);
        sel.sweep_start(false, &order, None);
        assert_eq!(sel.sweeping(), Some(false));
        assert_eq!(items(&sel), [1, 3]);
        sel.follow(id(5), &order, None);
        assert_eq!(items(&sel), [1, 3, 4, 5]);
        // Back past its start: the range turns around.
        sel.follow(id(2), &order, None);
        assert_eq!(items(&sel), [1, 2, 3]);
        sel.sweep_end();
        assert_eq!(sel.sweeping(), None);
        sel.follow(id(6), &order, None);
        assert_eq!(items(&sel), [1, 2, 3], "what it covered stays picked");

        sel.follow(id(3), &order, None);
        sel.sweep_start(true, &order, None);
        sel.follow(id(1), &order, None);
        assert_eq!(items(&sel), Vec::<u64>::new());
        sel.sweep_end();
        assert!(!sel.picked(), "nothing left: the selection follows again");
        sel.follow(id(4), &order, None);
        assert_eq!(items(&sel), [4]);

        sel.sweep_start(false, &order, None);
        sel.toggle(id(6));
        assert_eq!(sel.sweeping(), None);
    }

    #[test]
    fn a_sweep_whose_start_is_gone_ends() {
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let order: Vec<EntryId> = (1..=3).map(id).collect();
        let mut sel = Selection::default();
        sel.set([id(2)]);
        sel.sweep_start(false, &order, None);
        sel.follow(id(3), &order, None);
        sel.retain_in(&Listing::default());
        assert_eq!(sel.sweeping(), None);
        assert!(!sel.picked());
    }

    /// A pane showing `entries` through a location whose events `tx`
    /// sends, at `location`.
    /// A pane on a path whose rows are every folder of `n`.
    fn folders_pane(
        n: u64,
    ) -> (
        Pane,
        crossbeam_channel::Sender<crate::location::ListingEvent>,
    ) {
        use crate::entry::EntryKind;
        // One more known of the location than listed yet: one arriving.
        let (mut entries, paths): (Vec<Entry>, crate::hashing::Map<EntryId, PathBuf>) = (0..=n)
            .map(|i| {
                let id = EntryId {
                    file: crate::FileId::new(1, i),
                    at: i,
                };
                let name = format!("d{i}");
                (
                    Entry::new(id, name.clone(), EntryKind::Directory),
                    (id, PathBuf::from("/r").join(name)),
                )
            })
            .unzip();
        entries.pop();
        let (tx, events) = crossbeam_channel::unbounded();
        let mut p = Pane::new(PaneId(1));
        p.listing
            .apply(crate::location::ListingEvent::Added(entries));
        p.open = Some(Arc::new(Searched {
            location: Location::from_path(Path::new("/r")),
            events,
            paths,
        }));
        p.carry(Some(Lens {
            words: "lens".into(),
            set: Vec::new(),
            keeps: Arc::new(|e: &Entry| !e.is_dir()),
            metadata: false,
            next_change: None,
        }));
        (p, tx)
    }

    /// Sorted by what is read on demand, a pane asks its files once its
    /// listing is whole, never its folders, and those of a folder it
    /// opened in place alike; what comes since is asked alone, the reading
    /// going on; asked anew (other fields), the reading before is given
    /// up, and so it is once the sort reads nothing. What its view shows
    /// is asked alike (`show_fields`). An answer for a file that changed
    /// since it was read is not taken.
    #[test]
    fn fields_are_asked_of_what_came_since_and_anew_when_they_change() {
        use crate::entry::{EntryKind, Value, keys};
        use crate::location::{ListingEvent, SortKey};
        let entry = |i: u64, kind| {
            let id = EntryId {
                file: crate::FileId::new(1, i),
                at: i,
            };
            Entry::new(id, format!("e{i}"), kind)
        };
        let (entries, mut paths): (Vec<Entry>, crate::hashing::Map<EntryId, PathBuf>) = (0..4)
            .map(|i| {
                let kind = if i == 0 {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                };
                let e = entry(i, kind);
                let at = (e.id, PathBuf::from(format!("/r/e{i}")));
                (e, at)
            })
            .unzip();
        let late = entry(9, EntryKind::File);
        paths.insert(late.id, PathBuf::from("/r/e9"));
        let (_tx, events) = crossbeam_channel::unbounded();
        let mut p = Pane::new(PaneId(1));
        let folder = entries[0].id;
        p.listing.apply(ListingEvent::Added(entries));
        p.listing.apply(ListingEvent::Complete);
        p.open = Some(Arc::new(Searched {
            location: Location::from_path(Path::new("/r")),
            events,
            paths,
        }));
        assert!(
            p.fields_to_read(0).is_empty(),
            "sorted by name: nothing to read"
        );
        p.sort.key = SortKey::Duration;
        let first = p.fields_to_read(0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].entries.len(), 3, "the files, not the folder");
        assert!(p.fields_to_read(0).is_empty(), "asked once a change");
        p.listing.apply(ListingEvent::Added(vec![late]));
        let second = p.fields_to_read(0);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].entries.len(), 1, "what came, alone");
        assert!(!first[0].cancel.is_cancelled(), "the reading goes on");

        // A folder opened in place: its files are asked too, and what is
        // read of them is its listing's.
        let inner = entry(20, EntryKind::File);
        let inner_id = inner.id;
        let (_itx, inner_events) = crossbeam_channel::unbounded();
        let mut inner_paths = crate::hashing::Map::default();
        inner_paths.insert(inner_id, PathBuf::from("/r/e0/e20"));
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![inner]));
        listing.apply(ListingEvent::Complete);
        let at = Location::from_path(Path::new("/r/e0"));
        p.kept.push(Kept::new(
            Arc::new(Searched {
                location: at.clone(),
                events: inner_events,
                paths: inner_paths,
            }),
            listing,
            None,
        ));
        assert!(p.expand(at, folder));
        let opened = p.fields_to_read(0);
        assert_eq!(opened.len(), 1, "the pane's own asked already");
        assert_ne!(opened[0].listing, p.listing.id());
        assert_eq!(opened[0].entries.len(), 1);
        let tree = p.tree_version;
        assert!(p.take_fields(
            opened[0].listing,
            vec![(inner_id, keys::DURATION, None, Value::UInt(1_000))]
        ));
        assert_ne!(p.tree_version, tree, "what the pane shows changed");

        // What was read: one file as it was asked, another changed since
        // (another identity): that one is not taken.
        let (e1, e2) = (first[0].entries[0].0, first[0].entries[1].0);
        assert!(p.take_fields(
            p.listing.id(),
            vec![
                (e1, keys::DURATION, None, Value::UInt(5)),
                (e2, keys::DURATION, Some((e2.file, 9, 9)), Value::UInt(6)),
            ]
        ));
        let duration = |p: &Pane, id| {
            p.listing
                .get(id)
                .unwrap()
                .metadata
                .value(keys::DURATION)
                .cloned()
        };
        assert_eq!(duration(&p, e1), Some(Value::UInt(5)));
        assert_eq!(duration(&p, e2), None, "read of another file");

        p.sort.key = SortKey::Name;
        assert!(p.fields_to_read(0).is_empty());
        assert!(
            first[0].cancel.is_cancelled(),
            "a sort reading nothing gives it up"
        );
        // A view showing what is read on demand (a column of when each
        // picture was taken) has it read as a sort by it would.
        assert!(p.show_fields(&[keys::TAKEN]));
        assert!(!p.show_fields(&[keys::TAKEN]), "said again: no change");
        let shown = p.fields_to_read(0);
        assert_eq!(shown.len(), 2, "its own listing and the folder opened");
        assert_eq!(shown[0].entries.len(), 4);
        assert!(
            shown[0]
                .entries
                .iter()
                .all(|(.., lacks)| lacks == &[keys::TAKEN])
        );
        // Sorted by it as well, it is the same reading: not asked again.
        p.sort.key = SortKey::Taken;
        assert!(
            p.fields_to_read(0).is_empty(),
            "the same field, already asked"
        );
        p.sort.key = SortKey::Name;
        assert!(p.show_fields(&[]));
        assert!(
            p.fields_to_read(0).is_empty(),
            "shown no longer, nothing read"
        );
        assert!(shown[0].cancel.is_cancelled(), "its reading given up");
        assert!(shown[1].cancel.is_cancelled());
    }

    /// A folder of folders is looked over a slice at a time, the window
    /// waiting at most a pump's budget for any one, and what arrives after
    /// is looked at alone.
    #[test]
    fn folders_to_ask_are_found_a_slice_at_a_time() {
        let (mut p, _tx) = folders_pane(100_000);
        assert!(p.folders_to_ask().is_none(), "more than one pump's worth");
        assert!(p.backlog(), "so the pump goes on");
        assert!(p.looks_below());
        let asked = std::iter::repeat_with(|| p.folders_to_ask())
            .take(100_000)
            .find_map(|asked| asked)
            .unwrap();
        assert_eq!(asked.folders.len(), 100_000, "every one, once");
        assert!(!p.backlog());
        assert!(p.folders_to_ask().is_none(), "nothing moved since");

        let id = EntryId {
            file: crate::FileId::new(1, 100_000),
            at: 100_000,
        };
        let new = Entry::new(id, "d100000".to_owned(), crate::entry::EntryKind::Directory);
        p.listing
            .apply(crate::location::ListingEvent::Added(vec![new]));
        let asked = p.folders_to_ask().expect("what arrived is asked about");
        assert_eq!(asked.folders.len(), 1, "it alone");
        assert_eq!(asked.folders[0].0, id);
    }

    /// An answer about a folder is applied where it is: not a test of every
    /// entry listed at each batch of answers.
    #[test]
    fn an_answer_about_a_folder_tests_no_entry_again() {
        let n = 1_000;
        let (mut p, _tx) = pane_with("/", (0..n).map(file).collect());
        let folder = Entry::new(
            EntryId {
                file: crate::FileId::new(1, n + 1),
                at: 0,
            },
            "d".to_owned(),
            crate::entry::EntryKind::Directory,
        );
        let id = folder.id;
        p.listing
            .apply(crate::location::ListingEvent::Added(vec![folder]));
        p.carry(Some(Lens {
            words: "size:even".into(),
            set: Vec::new(),
            keeps: Arc::new(|e: &Entry| {
                TESTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                size_of(e).is_some_and(|size| size % 2 == 0)
            }),
            metadata: true,
            next_change: None,
        }));
        p.leading.asked.insert(id, 1);
        p.leading.waiting.insert(id);
        p.leading.unproved.insert(id);
        p.filter_tested += 1;
        tested();
        assert!(p.order().is_empty(), "nothing proved yet");
        assert!(tested() <= n as usize + 2, "made: each tested once");

        p.leading.answers.lock().push((id, 1, true));
        assert!(p.take_leads(), "the folder shows");
        let order = p.order();
        assert_eq!(&*order, [id]);
        assert!(tested() <= 1, "only the folder");
        assert!(!p.looks_below());
    }

    fn pane_with(
        location: &str,
        entries: Vec<Entry>,
    ) -> (
        Pane,
        crossbeam_channel::Sender<crate::location::ListingEvent>,
    ) {
        let (tx, events) = crossbeam_channel::unbounded();
        let mut p = Pane::new(PaneId(1));
        p.listing
            .apply(crate::location::ListingEvent::Added(entries));
        p.open = Some(Arc::new(Searched {
            location: Location::new("search", location),
            events,
            paths: crate::hashing::Map::default(),
        }));
        (p, tx)
    }

    fn file(ino: u64) -> Entry {
        Entry::new(
            EntryId {
                file: crate::FileId::new(1, ino),
                at: 0,
            },
            format!("f{ino}"),
            crate::entry::EntryKind::File,
        )
    }

    fn sized(ino: u64, size: u64) -> Entry {
        use crate::entry::{Provenance, Value, keys};
        let mut e = file(ino);
        e.metadata
            .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
        e
    }

    fn size_of(e: &Entry) -> Option<u64> {
        e.metadata
            .value(crate::entry::keys::SIZE)
            .and_then(crate::entry::Value::as_u64)
    }

    /// Calls of a filter's test, counted.
    static TESTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn tested() -> usize {
        TESTED.swap(0, std::sync::atomic::Ordering::Relaxed)
    }

    /// A filter that reads what the listing knows, counted: a file of size
    /// 7 it leaves out.
    fn counted(metadata: bool) -> ReadFilter {
        ReadFilter {
            keeps: Arc::new(|e: &Entry| {
                TESTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                size_of(e) != Some(7)
            }),
            waits: Vec::new(),
            metadata,
            lens: None,
            next_change: None,
        }
    }

    /// After a pump, a selection of the whole folder is looked over only
    /// once what decides what is shown moved (an entry gone), never for a
    /// batch of metadata no filter reads; while one reads it, only the
    /// entries it changed are asked again.
    #[test]
    fn a_pump_looks_the_selection_over_only_for_what_may_have_changed() {
        use crate::location::ListingEvent;
        let n = 20_000;
        let (mut p, tx) = pane_with("/", (0..n).map(file).collect());
        p.selection.set((0..n).map(|ino| file(ino).id));
        let pump = |p: &mut Pane, event| {
            tx.send(event).unwrap();
            p.pump(std::time::Duration::from_secs(10));
        };
        p.set_filter(Some("f"), Some(counted(false)));
        assert!(tested() >= n as usize, "set, the filter asks every one");

        pump(
            &mut p,
            ListingEvent::Updated((0..10).map(|ino| sized(ino, 1)).collect()),
        );
        assert_eq!(tested(), 0, "metadata no filter reads");
        assert_eq!(p.selection.len(), n as usize);

        pump(&mut p, ListingEvent::Removed(vec![file(5).id]));
        assert!(tested() > 0, "an entry gone: the selection looked over");
        assert_eq!(p.selection.len(), n as usize - 1);

        // A filter that reads metadata: the entries changed, asked again.
        p.read_filter_again(Some(counted(true)));
        tested();
        pump(
            &mut p,
            ListingEvent::Updated((10..20).map(|ino| sized(ino, 1)).collect()),
        );
        assert_eq!(tested(), 10);
        pump(&mut p, ListingEvent::Updated(vec![sized(30, 7)]));
        assert_eq!(tested(), 1);
        assert!(
            !p.selection.contains(file(30).id),
            "left out by the filter now"
        );
        assert_eq!(p.selection.len(), n as usize - 2);
    }

    /// A lens tests again only the entries that arrived or changed since
    /// it last made the order, and makes the order a fresh test of every
    /// entry makes.
    #[test]
    fn a_lens_tests_again_only_what_changed() {
        use crate::location::ListingEvent;
        let n = 10_000;
        let (mut p, _tx) = pane_with("/", (0..n).map(file).collect());
        p.carry(Some(Lens {
            words: "size:even".into(),
            set: Vec::new(),
            keeps: Arc::new(|e: &Entry| {
                TESTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                size_of(e).is_some_and(|size| size % 2 == 0)
            }),
            metadata: true,
            next_change: None,
        }));
        tested();
        assert!(p.order().is_empty(), "none known yet");
        assert_eq!(tested(), n as usize, "made: each tested once");

        p.listing.apply(ListingEvent::Updated(
            (0..100).map(|ino| sized(ino, ino)).collect(),
        ));
        let order = p.order();
        assert_eq!(tested(), 100, "only what changed");
        assert_eq!(order.len(), 50);
        assert!(Arc::ptr_eq(&order, &p.order()), "nothing changed since");
        assert_eq!(tested(), 0);

        p.listing.apply(ListingEvent::Added(vec![sized(n + 1, 2)]));
        let order = p.order();
        assert_eq!(tested(), 1, "an arrival");
        *p.filter_cache.lock() = None;
        assert_eq!(order, p.order(), "as a fresh test makes it");
    }

    /// Which opened folder holds which entry is made again when their
    /// entries change, not for a batch of their metadata, nor for each
    /// sync of what the view keeps.
    #[test]
    fn a_folder_opened_in_place_is_indexed_again_only_when_its_entries_change() {
        use crate::location::ListingEvent;
        use std::sync::atomic::Ordering;
        let mut folder = file(0);
        folder.kind = crate::entry::EntryKind::Directory;
        let at = folder.id;
        let (mut p, _tx) = pane_with("/", vec![folder]);
        let inside = Location::new("search", "/f0");
        assert!(p.expand(inside.clone(), at));
        let (tx, events) = crossbeam_channel::unbounded();
        p.add_kept(
            Arc::new(Searched {
                location: inside,
                events,
                paths: crate::hashing::Map::default(),
            }),
            Listing::default(),
            None,
        );
        let pump = |p: &mut Pane, event| {
            tx.send(event).unwrap();
            p.pump(std::time::Duration::from_secs(10));
        };
        pump(
            &mut p,
            ListingEvent::Added((1..=10_000).map(file).collect()),
        );
        let built = || OPENED_INDEXES.load(Ordering::Relaxed);
        let before = built();
        assert!(p.entry(file(5).id).is_some());
        assert_eq!(built(), before + 1);

        let (tree, members) = (p.tree_version(), p.tree_members_version());
        pump(
            &mut p,
            ListingEvent::Updated((1..=100).map(|ino| sized(ino, 3)).collect()),
        );
        assert_ne!(p.tree_version(), tree, "what it shows changed");
        assert_eq!(p.tree_members_version(), members, "not which entries");
        assert!(p.entry(file(5).id).is_some());
        assert!(p.keep_only(&[]).is_empty());
        assert!(p.entry(file(6).id).is_some());
        assert_eq!(built(), before + 1, "neither made it again");

        pump(&mut p, ListingEvent::Added(vec![file(20_000)]));
        assert_ne!(p.tree_members_version(), members);
        assert!(p.entry(file(20_000).id).is_some());
        assert_eq!(built(), before + 2);
    }

    /// What is open in place moves `expanded_version` when a folder opens
    /// or closes, not as its listing fills: the board asks for its columns
    /// once for each, and again once another view closed them.
    /// A reading asked before an entry was lent again seen otherwise is of
    /// what it was: it is not taken for it, nor its finding it gone, and
    /// the entry is still to be read; asked after, it is.
    #[test]
    fn a_reading_asked_before_an_entry_was_seen_otherwise_is_not_its() {
        let mut lent = file(7);
        lent.id.at = 9;
        let id = lent.id;
        let loaned = |seen| Loaned {
            entry: lent.clone(),
            path: PathBuf::from("/s/a folder/f7"),
            seen,
        };
        let read = || {
            let mut e = sized(7, 3);
            e.id = id;
            e
        };
        let (mut p, _tx) = pane_with("/s", vec![file(1)]);
        p.lend("usage", [loaned(1)]);
        let (asked, before) = p.lent_paths(&[id]);
        assert_eq!(asked.len(), 1);
        p.lend("usage", [loaned(2)]);
        assert!(!p.lent_read(vec![read()], &[], &[], before));
        assert!(!p.lent_read(Vec::new(), &[id], &[], before));
        assert!(!p.lent_read(Vec::new(), &[], &[id], before));
        assert!(p.lent_to_read(id) && p.is_lent(id), "still to be read");
        let (_, now) = p.lent_paths(&[id]);
        assert!(p.lent_read(vec![read()], &[], &[], now));
        assert!(!p.lent_to_read(id));
        assert_eq!(p.entry(id).and_then(size_of), Some(3));
    }

    #[test]
    fn what_is_open_in_place_changes_when_folders_open_or_close() {
        use crate::location::ListingEvent;
        let mut folder = file(0);
        folder.kind = crate::entry::EntryKind::Directory;
        let at = folder.id;
        let (mut p, _tx) = pane_with("/", vec![folder]);
        let inside = Location::new("search", "/f0");
        let opened = p.expanded_version();
        assert!(p.expand(inside.clone(), at));
        assert_ne!(p.expanded_version(), opened, "opened");
        let (tx, events) = crossbeam_channel::unbounded();
        p.add_kept(
            Arc::new(Searched {
                location: inside.clone(),
                events,
                paths: crate::hashing::Map::default(),
            }),
            Listing::default(),
            None,
        );
        let open = p.expanded_version();
        tx.send(ListingEvent::Added((1..=10).map(file).collect()))
            .unwrap();
        let tree = p.tree_version();
        p.pump(std::time::Duration::from_secs(10));
        assert_ne!(p.tree_version(), tree, "its entries came");
        assert_eq!(p.expanded_version(), open, "nothing opened or closed");
        assert!(!p.expand(inside.clone(), at), "open already");
        assert_eq!(p.expanded_version(), open);

        assert!(p.collapse_all());
        assert_ne!(p.expanded_version(), open, "closed");
        let closed = p.expanded_version();
        assert!(!p.collapse_all(), "nothing open");
        assert_eq!(p.expanded_version(), closed);
        assert!(p.expand(inside, at), "opened again");
        assert_ne!(p.expanded_version(), closed);
    }

    /// A sweep over the order it was drawn over changes only what each step
    /// crosses: drawn whole once, it holds what a whole drawing holds at
    /// every step, back over where it began too, selecting or deselecting.
    #[test]
    fn a_sweep_steps_without_being_drawn_whole_again() {
        use std::sync::atomic::Ordering;
        let id = |ino| EntryId {
            file: crate::FileId::new(1, ino),
            at: 0,
        };
        let n = 50_000;
        let order: Arc<[EntryId]> = (0..n).map(id).collect();
        let base: Vec<EntryId> = (0..n).step_by(10).map(id).collect();
        let from = 25_000;
        let swept = |to: usize, unset: bool| -> std::collections::HashSet<EntryId> {
            let range: std::collections::HashSet<EntryId> =
                order[from.min(to)..=from.max(to)].iter().copied().collect();
            if unset {
                base.iter()
                    .copied()
                    .filter(|id| !range.contains(id))
                    .collect()
            } else {
                base.iter().copied().chain(range).collect()
            }
        };
        for unset in [false, true] {
            let mut sel = Selection::default();
            sel.set(base.iter().copied());
            sel.cursor = Some(order[from]);
            let drawn = SWEEPS_DRAWN.load(Ordering::Relaxed);
            sel.sweep_start_at(unset, from, &order, None);
            // Down a row at a time, back up past where it began, down by
            // pages.
            let mut at = from;
            let steps = std::iter::repeat_n(1isize, 300)
                .chain(std::iter::repeat_n(-1, 600))
                .chain(std::iter::repeat_n(7, 100));
            for (i, step) in steps.enumerate() {
                at = at.checked_add_signed(step).unwrap();
                sel.follow_at(order[at], at, &order, None);
                if i % 97 == 0 || at == from {
                    assert_eq!(
                        sel.iter().collect::<std::collections::HashSet<_>>(),
                        swept(at, unset),
                        "step {i}, unset {unset}"
                    );
                    assert_eq!(
                        (sel.anchor, sel.cursor),
                        (Some(order[from]), Some(order[at]))
                    );
                }
            }
            assert_eq!(
                SWEEPS_DRAWN.load(Ordering::Relaxed),
                drawn + 1,
                "drawn once"
            );
            assert_eq!(
                sel.iter().collect::<std::collections::HashSet<_>>(),
                swept(at, unset)
            );
            // Another order: drawn whole once, then stepped over again.
            let other: Arc<[EntryId]> = order.iter().copied().collect();
            sel.follow_at(other[at + 1], at + 1, &other, None);
            sel.follow_at(other[at + 2], at + 2, &other, None);
            assert_eq!(SWEEPS_DRAWN.load(Ordering::Relaxed), drawn + 2);
            assert_eq!(
                sel.iter().collect::<std::collections::HashSet<_>>(),
                swept(at + 2, unset)
            );
            // Told a row that is not the id's: drawn whole, rightly.
            sel.follow_at(other[at + 3], 0, &other, None);
            assert_eq!(SWEEPS_DRAWN.load(Ordering::Relaxed), drawn + 3);
            assert_eq!(
                sel.iter().collect::<std::collections::HashSet<_>>(),
                swept(at + 3, unset)
            );
        }
    }
}
