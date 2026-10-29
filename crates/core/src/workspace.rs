//! Panes, selection, and focus: the state every view and command reads.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexSet;

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId};
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
    /// sweep covers up to it, a selection that follows the cursor moves
    /// with it, a picked one stays.
    pub fn follow(&mut self, id: EntryId, order: &[EntryId]) {
        if self.sweep.is_some() {
            self.cursor = Some(id);
            self.redraw_sweep(order);
        } else if self.picked {
            self.cursor = Some(id);
        } else {
            self.set([id]);
        }
    }

    /// Hold a range from the cursor while it moves: selecting, or with
    /// `unset` deselecting, over what was picked. Without a cursor there is
    /// nothing to start from.
    pub fn sweep_start(&mut self, unset: bool, order: &[EntryId]) {
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
        self.redraw_sweep(order);
    }

    /// [`Selection::sweep_start`], the cursor at `at` in `order` (the pane's,
    /// as its view has it): the steps that follow over that order
    /// ([`Selection::follow_at`]) change only what they cross.
    pub fn sweep_start_at(&mut self, unset: bool, at: usize, order: &Arc<[EntryId]>) {
        let Some(from) = self.cursor else { return };
        if order.get(at) != Some(&from) {
            return self.sweep_start(unset, order);
        }
        self.sweep_start(unset, &order[at..=at]);
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
    pub fn follow_at(&mut self, id: EntryId, at: usize, order: &Arc<[EntryId]>) {
        let drawn = self.sweep.as_ref().and_then(|sweep| {
            let (drawn, from_at, was) = sweep.drawn.as_ref()?;
            (Arc::ptr_eq(drawn, order) && order.get(at) == Some(&id)).then_some((*from_at, *was))
        });
        let Some((from_at, was)) = drawn else {
            self.follow(id, order);
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
        let ((lo0, hi0), (lo1, hi1)) = (span(was), span(at));
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

    fn redraw_sweep(&mut self, order: &[EntryId]) {
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
            items.extend(range.iter().copied());
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
    /// takes the next file, as in Dolphin and Nautilus. A cursor `order`
    /// does not hold falls back as `retain` has it.
    pub fn retain_near(&mut self, holds: impl Fn(EntryId) -> bool, order: &[EntryId], back: bool) {
        let lost = self.cursor.filter(|cursor| !holds(*cursor));
        let had = !self.items.is_empty();
        self.retain(&holds);
        let Some(at) = lost.and_then(|lost| order.iter().position(|id| *id == lost)) else {
            return;
        };
        let after = || order[at + 1..].iter().copied().find(|id| holds(*id));
        let before = || order[..at].iter().rev().copied().find(|id| holds(*id));
        let near = if back {
            before().or_else(after)
        } else {
            after().or_else(before)
        };
        let Some(near) = near else { return };
        self.cursor = Some(near);
        if had && self.items.is_empty() {
            self.touch();
            self.items.insert(near);
            self.anchor = Some(near);
        }
    }
}

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
}

impl Kept {
    fn new(open: Arc<dyn OpenLocation>, listing: Listing) -> Self {
        let location = listing.address(open.as_ref()).clone();
        Kept {
            path: location.path(),
            location,
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
    /// By step of `history`, the entry the cursor was on when the pane left
    /// it: Back and Forward select it again (`go_to`).
    left_at: Vec<Option<EntryId>>,
    /// A name to select once the listing has it, and whether it is still
    /// coming once the listing is complete; see [`Pane::reveal`].
    reveal: Option<(OsString, bool)>,
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
    /// The order `order` last gave, what is shown: where a cursor whose
    /// entry leaves the view goes from (`Selection::retain_near`).
    shown: parking_lot::Mutex<Option<Arc<[EntryId]>>>,
    /// What is typed in the address field before Enter searches: only
    /// the entries it keeps are shown (`Pane::order`). The pane's like its
    /// sort; showing another location clears it.
    filter: Option<Arc<str>>,
    /// The filter as the filter's reader read it (ADR-098), a test of
    /// what the listing knows; `None`: names that hold the text.
    read: Option<Arc<ReadFilter>>,
    /// Changes with the filter.
    filter_version: u64,
    /// What the folder's files are narrowed to, kept below the folder it
    /// was set in.
    narrowed: Option<Narrowed>,
    /// The agent's call that put its location in a dormant pane (a tab
    /// opened behind, `tab.new`): opened later, on the window's thread, it
    /// is opened as that call (`permissions::as_agent`), so a location
    /// that asks before it acts for an agent (a server connected) asks.
    asked_by: Option<crate::permissions::Privacy>,
    /// The filtered order, the order it filtered (by identity), the
    /// filter's version and, while narrowed, the listings' (a file's
    /// metadata arriving decides whether it is shown), and the order.
    filter_cache: parking_lot::Mutex<Option<FilterCache>>,
}

/// The filtered order, and what made it: what the filter keeps of every
/// entry listed, tested again only for those that arrived or changed since
/// (`Listing::updated_since`), so a narrowing that reads metadata tests a
/// batch of it rather than the whole folder at each batch.
struct FilterCache {
    /// The filter's version it tested with.
    filter: u64,
    /// Each listing tested, the shown one's then the opened folders', by
    /// its id, with the version it was tested at.
    at: Vec<(u64, u64)>,
    /// The entries listed that the filter keeps, of every listing in `at`
    /// (with some that went since, until it is made again).
    kept: crate::hashing::Set<EntryId>,
    /// The order filtered (by identity), and what it kept of it.
    from: Arc<[EntryId]>,
    order: Arc<[EntryId]>,
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
            moved |= if listing.get(id).is_some_and(&keeps) {
                cache.kept.insert(id)
            } else {
                cache.kept.remove(&id)
            };
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
}

/// The selection as last looked over (`Pane::retain_changed`): what decided
/// it, and each listing's version then (the shown one's first).
struct Retained {
    key: RetainKey,
    at: Vec<(u64, u64)>,
}

/// What a folder's files are narrowed to (a locked filter): kept while
/// the pane shows the folder it was set in or one under it
/// (`Pane::replace_open`), let go of anywhere else. What it means is its
/// setter's, the query plugin's `query.narrow`: the core keeps its words
/// and asks it of each file; its pills are what the setter says of its
/// words (`query.describe_filter`). Every folder is shown whatever it
/// says, so what is under it can be reached.
#[derive(Clone)]
pub struct Narrowed {
    /// The folder it was set in.
    pub root: PathBuf,
    /// Its words, in its setter's language (`kind:image modified:2024`),
    /// what a further choice changes.
    pub expr: String,
    /// The facets it narrows by already (`kind`, `modified`), which the
    /// Narrow by bar does not offer again.
    pub set: Vec<String>,
    /// Whether a file is shown. One it cannot tell yet (its metadata is
    /// still to come) is not, until it can.
    pub keeps: Arc<dyn Fn(&Entry) -> bool + Send + Sync>,
}

impl std::fmt::Debug for Narrowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Narrowed")
            .field("root", &self.root)
            .field("expr", &self.expr)
            .finish_non_exhaustive()
    }
}

impl Narrowed {
    /// Whether `entry` is shown: a folder always is.
    pub fn shows(&self, entry: &Entry) -> bool {
        entry.is_dir() || (self.keeps)(entry)
    }
}

/// What words typed over a folder keep, as the filter's reader read them
/// (ADR-098, the query plugin's): a test of what the listing knows, never
/// of the file. A word finds names, of folders too; any other term holds
/// for files, and every folder stays, as a narrowing keeps them.
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
}

impl std::fmt::Debug for ReadFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadFilter")
            .field("waits", &self.waits)
            .finish_non_exhaustive()
    }
}

/// Whether a pane shows an entry, by its id: listed by `listing` or one
/// opened in place (`kept`, `expanded`), shown by its listing (hidden
/// files, companions), and kept by the filter and the narrowing.
fn shows<'a>(
    listing: &'a Listing,
    kept: &'a [Kept],
    expanded: &'a [Opened],
    filter: Option<&'a str>,
    read: Option<&'a ReadFilter>,
    narrowed: Option<&'a Narrowed>,
    sort: SortSpec,
) -> impl Fn(EntryId) -> bool + 'a {
    let filter = TextFilter::of(filter, read);
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
            Some((e, true)) => {
                filter.as_ref().is_none_or(|f| f.keeps(e)) && narrowed.is_none_or(|n| n.shows(e))
            }
            _ => false,
        }
    }
}

/// The pane's filter as it is asked of each entry: its reader's test, or
/// names that hold the text.
enum TextFilter<'a> {
    Names(NameFilter<'a>),
    Read(&'a ReadFilter),
}

impl<'a> TextFilter<'a> {
    fn of(text: Option<&'a str>, read: Option<&'a ReadFilter>) -> Option<Self> {
        match (text, read) {
            (Some(_), Some(read)) => Some(TextFilter::Read(read)),
            (Some(text), None) => Some(TextFilter::Names(NameFilter::new(text))),
            (None, _) => None,
        }
    }

    fn keeps(&self, entry: &Entry) -> bool {
        match self {
            TextFilter::Names(names) => names.holds(&entry.name),
            TextFilter::Read(read) => (read.keeps)(entry),
        }
    }
}

/// Whether the pane shows `entry`: its name holds the filter, and the
/// narrowing keeps it.
fn shown(filter: Option<&TextFilter>, narrowed: Option<&Narrowed>, entry: &Entry) -> bool {
    filter.is_none_or(|f| f.keeps(entry)) && narrowed.is_none_or(|n| n.shows(entry))
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
            listing: Listing::default(),
            kept: Vec::new(),
            left: None,
            selection: Selection::default(),
            sort: SortSpec::default(),
            history: Vec::new(),
            history_pos: 0,
            left_at: Vec::new(),
            reveal: None,
            reveal_looked: None,
            opened_index: parking_lot::Mutex::new(None),
            restore: None,
            expanded: Vec::new(),
            expanded_version: 0,
            tree_version: 0,
            tree_members_version: 0,
            retained: None,
            order_cache: parking_lot::Mutex::new(None),
            shown: parking_lot::Mutex::new(None),
            filter: None,
            read: None,
            filter_version: 0,
            narrowed: None,
            asked_by: None,
            filter_cache: parking_lot::Mutex::new(None),
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
        let again = self.left_at.get(before).copied().flatten().map(|id| {
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
        let again = self.left_at.get(pos).copied().flatten().map(|id| {
            let mut selection = Selection::default();
            selection.set([id]);
            selection
        });
        self.set_restore(again);
    }

    fn remember_left(&mut self, pos: usize, left: Option<EntryId>) {
        if self.left_at.len() <= pos {
            self.left_at.resize(pos + 1, None);
        }
        self.left_at[pos] = left;
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
        if !self.expanded.is_empty() {
            self.expanded.clear();
            self.expanded_changed();
        }
        self.set_filter(None, None);
        // A narrowing stays below the folder it was set in.
        let below = |n: &Narrowed| {
            let open = self.location().and_then(Location::path);
            open.is_some_and(|path| path.starts_with(&n.root))
        };
        if self.narrowed.as_ref().is_some_and(|n| !below(n)) {
            self.set_narrowed(None);
        }
    }

    /// Open the location a dormant pane shows again (`suspend`): what it
    /// kept while it slept stays (the folders opened in place, which the
    /// caller opens again, the filter, the narrowing), the selection comes
    /// back through `set_restore`.
    pub(crate) fn reopen(&mut self, open: Arc<dyn OpenLocation>, listing: Listing) {
        self.location_lifetime.cancel();
        self.location_lifetime = CancelToken::new();
        self.listing = listing;
        self.open = Some(open);
        self.left = None;
        self.selection = Selection::default();
        self.reveal = None;
        self.restore = None;
        if !self.expanded.is_empty() {
            self.expanded_changed();
        }
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
        self.filter = text.map(Arc::from);
        self.read = text.and(read).map(Arc::new);
        self.filter_changed();
    }

    /// The filter's text read again by `read`, the reader there is now
    /// (a plugin that reads filters turned on or off): `None` filters by
    /// names. Its text stays.
    pub(crate) fn read_filter_again(&mut self, read: Option<ReadFilter>) {
        if self.filter.is_none() {
            return;
        }
        self.read = read.map(Arc::new);
        self.filter_changed();
    }

    /// What the files are narrowed to, if anything.
    pub fn narrowed(&self) -> Option<&Narrowed> {
        self.narrowed.as_ref()
    }

    /// Whether some of the entries are not shown: filtered by name or
    /// narrowed.
    pub fn is_filtered(&self) -> bool {
        self.filter.is_some() || self.narrowed.is_some()
    }

    /// Narrow the files to what `narrowed` keeps (`None`: show them all
    /// again). What is selected and no longer shown is deselected, as the
    /// filter does.
    pub fn set_narrowed(&mut self, narrowed: Option<Narrowed>) {
        if narrowed.is_none() && self.narrowed.is_none() {
            return;
        }
        self.narrowed = narrowed;
        self.filter_changed();
    }

    fn filter_changed(&mut self) {
        self.filter_version += 1;
        self.retain_shown();
    }

    /// Show hidden files, or no longer: those selected leave the
    /// selection as they leave the view, so nothing acts on a file the
    /// user cannot see.
    pub(crate) fn set_show_hidden(&mut self, show: bool) {
        self.sort.show_hidden = show;
        self.retain_shown();
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
        // Each closes as it drops.
        self.kept.clear();
        self.opened_moved();
        self.left = None;
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
        if let Some(left) = self.left.take()
            && wanted.contains(&left.location)
        {
            self.kept.push(left);
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

    pub(crate) fn add_kept(&mut self, open: Arc<dyn OpenLocation>, listing: Listing) {
        self.kept.push(Kept::new(open, listing));
        self.opened_moved();
    }

    /// Whether anything is kept or waiting to be: `keep_only` has work.
    pub fn keeps_any(&self) -> bool {
        !self.kept.is_empty() || self.left.is_some() || !self.expanded.is_empty()
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
        self.reveal = Some((name, false));
        self.reveal_looked = None;
        self.take_reveal();
    }

    /// `reveal` an entry that its location lists a moment after it is
    /// complete (a tag just made, listed by a worker): the name is kept
    /// until it is listed or the pane shows another location.
    pub fn reveal_coming(&mut self, name: OsString) {
        self.reveal = Some((name, true));
        self.reveal_looked = None;
        self.take_reveal();
    }

    fn take_reveal(&mut self) {
        let Some((name, coming)) = &self.reveal else {
            return;
        };
        let names = Some(self.listing.names_version());
        let looked = std::mem::replace(&mut self.reveal_looked, names) == names;
        if looked && !self.listing.is_complete() {
            return;
        }
        if let Some(id) = (!looked)
            .then(|| self.listing.find_by_name(name).map(|e| e.id))
            .flatten()
        {
            self.select_revealed(&[id]);
            self.reveal = None;
        } else if self.listing.is_complete() && !coming {
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
        self.open
            .as_ref()
            .is_some_and(|open| !open.events().is_empty())
            || self.kept.iter().any(Kept::backlog)
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
            if let Some(n) = &mut self.narrowed
                && let Some(root) = moved_path(&n.root, &from, &to)
            {
                n.root = root;
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
        let holds = shows(
            &self.listing,
            &self.kept,
            &self.expanded,
            self.filter.as_deref(),
            self.read.as_deref(),
            self.narrowed.as_ref(),
            self.sort,
        );
        let hidden: crate::hashing::Set<EntryId> =
            ids.iter().copied().filter(|id| !holds(*id)).collect();
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
        let shown = self.shown.lock().clone();
        let holds = shows(
            &self.listing,
            &self.kept,
            &self.expanded,
            self.filter.as_deref(),
            self.read.as_deref(),
            self.narrowed.as_ref(),
            self.sort,
        );
        match shown {
            Some(shown) => self.selection.retain_near(holds, &shown, back),
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
            filter: self.filter_version,
            show_hidden: self.sort.show_hidden,
            selection: self.selection.revision(),
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
        let metadata = self.narrowed.is_some() || self.read.as_ref().is_some_and(|r| r.metadata);
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
            self.filter.as_deref(),
            self.read.as_deref(),
            self.narrowed.as_ref(),
            self.sort,
        )(id)
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

    /// The folders opened in place under the one shown.
    pub fn expanded(&self) -> &[Opened] {
        &self.expanded
    }

    pub fn is_expanded(&self, location: &Location) -> bool {
        self.expanded.iter().any(|o| &o.location == location)
    }

    /// Whether the folder (or place) that is entry `at` is open in place.
    pub fn is_open_at(&self, at: EntryId) -> bool {
        self.expanded.iter().any(|o| o.at == at)
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
        self.kept.retain(|k| !gone(&k.location));
        self.opened_moved();
    }

    /// Close every folder opened in place.
    pub fn collapse_all(&mut self) -> bool {
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
    }

    /// The entry the pane shows of the file `file` at `path`: of a file
    /// with several names, the one listed there (ADR-049). Found by its
    /// name and file in each listing (`Listing::named_file`), so a command
    /// may ask it of each entry of a selection of a whole folder.
    pub fn entry_at(&self, file: crate::FileId, path: &Path) -> Option<&Entry> {
        let name = path.file_name()?;
        std::iter::once(&self.listing)
            .chain(self.kept.iter().map(|k| &k.listing))
            .flat_map(|l| l.named_file(name, file))
            .find(|e| self.path_of(e.id).as_deref() == Some(path))
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
        self.opened_path(id)
            .or_else(|| self.open.as_ref()?.path_of(id))
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
        let order = self.filtered_order();
        *self.shown.lock() = Some(Arc::clone(&order));
        order
    }

    fn filtered_order(&self) -> Arc<[EntryId]> {
        let all = self.unfiltered_order();
        if !self.is_filtered() {
            return all;
        }
        let filter = TextFilter::of(self.filter.as_deref(), self.read.as_deref());
        let (filter, narrowed) = (filter.as_ref(), self.narrowed.as_ref());
        let keeps = |e: &Entry| shown(filter, narrowed, e);
        let listings: Vec<&Listing> = self.listings().collect();
        let mut cache = self.filter_cache.lock();
        if let Some(c) = cache.as_mut()
            && let Some(moved) = retest(c, self.filter_version, &listings, keeps)
        {
            if !moved && Arc::ptr_eq(&c.from, &all) {
                return Arc::clone(&c.order);
            }
            let order: Arc<[EntryId]> = all
                .iter()
                .copied()
                .filter(|id| c.kept.contains(id))
                .collect();
            c.from = all;
            c.order = Arc::clone(&order);
            return order;
        }
        // Made again: every entry listed tested.
        let kept: crate::hashing::Set<EntryId> = listings
            .iter()
            .flat_map(|listing| listing.iter())
            .filter(|e| keeps(e))
            .map(|e| e.id)
            .collect();
        let order: Arc<[EntryId]> = all.iter().copied().filter(|id| kept.contains(id)).collect();
        *cache = Some(FilterCache {
            filter: self.filter_version,
            at: listings.iter().map(|l| (l.id(), l.version())).collect(),
            kept,
            from: all,
            order: Arc::clone(&order),
        });
        order
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
                .is_some_and(|e| !e.is_dir() && !e.is_place() && !e.metadata.is_hydrated())
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

    /// The entries the pane shows whose metadata has not come yet,
    /// folders too (places count their files from the start): what a
    /// sort by size, date or rating places after the rest until it comes
    /// (`SortKey::needs_metadata`), so whoever sorts so asks for them.
    pub fn unknown(&self) -> Vec<EntryId> {
        self.order()
            .iter()
            .copied()
            .filter(|id| {
                self.entry(*id)
                    .is_some_and(|e| !e.is_place() && !e.metadata.is_hydrated())
            })
            .collect()
    }

    /// `order` before the name filter and the narrowing: what the pane
    /// holds. A narrowing cannot tell a file whose metadata has not come
    /// yet, and does not show it until it can; no view draws it, so
    /// whoever narrows asks for it (`hydrate`) from here.
    pub fn unfiltered_order(&self) -> Arc<[EntryId]> {
        let top = self.listing.sorted(self.sort);
        if self.expanded.is_empty() {
            return top;
        }
        let opened: Vec<(&Opened, Arc<[EntryId]>)> = opened(&self.kept, &self.expanded)
            .map(|(o, k)| (o, k.listing.sorted(self.sort)))
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
    /// they show.
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
            if let Some(to) = p.listing.take_redirect()
                && let Some(from) = p.location()
            {
                r.redirects.push(Redirect {
                    pane: p.id,
                    from: from.clone(),
                    to,
                });
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
    /// Panes whose location said it is reached at another
    /// (`ListingEvent::Redirect`): `App::pump_report` shows it there.
    pub redirects: Vec<Redirect>,
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
        sel.retain_near(without(&[3]), &order, false);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(4)), vec![4]));
        // The last one: the one before it.
        sel.set([id(5)]);
        sel.retain_near(without(&[5]), &order, false);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(4)), vec![4]));
        // Several in a row, the cursor on the last: what follows them.
        sel.set([id(2), id(3), id(4)]);
        sel.retain_near(without(&[2, 3, 4]), &order, false);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(5)), vec![5]));
        // A pick that keeps some: the cursor moves, the pick stays.
        sel.set([id(2), id(4)]);
        sel.retain_near(without(&[4]), &order, false);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(5)), vec![2]));
        // Back: before it first.
        sel.set([id(3)]);
        sel.retain_near(without(&[3]), &order, true);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(2)), vec![2]));
        // A cursor with nothing selected moves and selects nothing.
        sel.set([id(3)]);
        sel.clear();
        sel.retain_near(without(&[3]), &order, false);
        assert_eq!((sel.cursor, items(&sel)), (Some(id(4)), vec![]));
        // Nothing left: no cursor.
        sel.set([id(3)]);
        sel.retain_near(|_| false, &order, false);
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
                e.metadata
                    .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
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
        sel.follow(id(2), &order);
        assert_eq!(items(&sel), [2]);
        // The first pick starts from nothing: the item followed is not one.
        sel.pick(id(2));
        assert!(sel.picked());
        sel.follow(id(3), &order);
        sel.pick(id(3));
        sel.follow(id(4), &order);
        assert_eq!(items(&sel), [2, 3]);
        assert_eq!(sel.cursor, Some(id(4)));
        // Unpicking the last one gives the selection back to the cursor.
        sel.pick(id(3));
        sel.pick(id(2));
        assert!(!sel.picked());
        assert_eq!(items(&sel), [2]);
        sel.follow(id(5), &order);
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
        sel.follow(id(3), &order);
        sel.sweep_start(false, &order);
        assert_eq!(sel.sweeping(), Some(false));
        assert_eq!(items(&sel), [1, 3]);
        sel.follow(id(5), &order);
        assert_eq!(items(&sel), [1, 3, 4, 5]);
        // Back past its start: the range turns around.
        sel.follow(id(2), &order);
        assert_eq!(items(&sel), [1, 2, 3]);
        sel.sweep_end();
        assert_eq!(sel.sweeping(), None);
        sel.follow(id(6), &order);
        assert_eq!(items(&sel), [1, 2, 3], "what it covered stays picked");

        sel.follow(id(3), &order);
        sel.sweep_start(true, &order);
        sel.follow(id(1), &order);
        assert_eq!(items(&sel), Vec::<u64>::new());
        sel.sweep_end();
        assert!(!sel.picked(), "nothing left: the selection follows again");
        sel.follow(id(4), &order);
        assert_eq!(items(&sel), [4]);

        sel.sweep_start(false, &order);
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
        sel.sweep_start(false, &order);
        sel.follow(id(3), &order);
        sel.retain_in(&Listing::default());
        assert_eq!(sel.sweeping(), None);
        assert!(!sel.picked());
    }

    /// A pane showing `entries` through a location whose events `tx`
    /// sends, at `location`.
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

    /// A narrowing tests again only the entries that arrived or changed
    /// since it last made the order, and makes the order a fresh test of
    /// every entry makes.
    #[test]
    fn a_narrowing_tests_again_only_what_changed() {
        use crate::location::ListingEvent;
        let n = 10_000;
        let (mut p, _tx) = pane_with("/", (0..n).map(file).collect());
        p.set_narrowed(Some(Narrowed {
            root: PathBuf::from("/"),
            expr: "size:even".into(),
            set: Vec::new(),
            keeps: Arc::new(|e: &Entry| {
                TESTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                size_of(e).is_some_and(|size| size % 2 == 0)
            }),
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
            sel.sweep_start_at(unset, from, &order);
            // Down a row at a time, back up past where it began, down by
            // pages.
            let mut at = from;
            let steps = std::iter::repeat_n(1isize, 300)
                .chain(std::iter::repeat_n(-1, 600))
                .chain(std::iter::repeat_n(7, 100));
            for (i, step) in steps.enumerate() {
                at = at.checked_add_signed(step).unwrap();
                sel.follow_at(order[at], at, &order);
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
            sel.follow_at(other[at + 1], at + 1, &other);
            sel.follow_at(other[at + 2], at + 2, &other);
            assert_eq!(SWEEPS_DRAWN.load(Ordering::Relaxed), drawn + 2);
            assert_eq!(
                sel.iter().collect::<std::collections::HashSet<_>>(),
                swept(at + 2, unset)
            );
            // Told a row that is not the id's: drawn whole, rightly.
            sel.follow_at(other[at + 3], 0, &other);
            assert_eq!(SWEEPS_DRAWN.load(Ordering::Relaxed), drawn + 3);
            assert_eq!(
                sel.iter().collect::<std::collections::HashSet<_>>(),
                swept(at + 3, unset)
            );
        }
    }
}
