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
        self.sweep = Some(Sweep { from, unset, base });
        self.redraw_sweep(order);
    }

    /// End the sweep, keeping what it covers.
    pub fn sweep_end(&mut self) {
        if self.sweep.take().is_some() {
            self.picked = !self.items.is_empty();
        }
    }

    fn redraw_sweep(&mut self, order: &[EntryId]) {
        let Some(sweep) = &self.sweep else { return };
        let cursor = self.cursor.unwrap_or(sweep.from);
        let pos = |id: EntryId| order.iter().position(|x| *x == id);
        let range: &[EntryId] = match (pos(sweep.from), pos(cursor)) {
            (Some(a), Some(b)) => &order[a.min(b)..=a.max(b)],
            _ => std::slice::from_ref(&sweep.from),
        };
        let mut items = sweep.base.clone();
        if sweep.unset {
            for id in range {
                items.shift_remove(id);
            }
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
            } else {
                self.sweep = None;
            }
        }
        self.picked &= !self.items.is_empty();
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
        let location = open.location().clone();
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
/// them, shallowest first, never without their listing.
fn opened<'a>(
    kept: &'a [Kept],
    expanded: &'a [Opened],
) -> impl Iterator<Item = (&'a Opened, &'a Kept)> + 'a {
    expanded.iter().filter_map(move |o| {
        let kept = kept.iter().find(|k| k.location == o.location)?;
        Some((o, kept))
    })
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
    /// A name to select once the listing has it, and whether it is still
    /// coming once the listing is complete; see [`Pane::reveal`].
    reveal: Option<(OsString, bool)>,
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
    order_cache: parking_lot::Mutex<Option<OrderCache>>,
    /// Only the entries whose name holds this are shown (`Pane::order`):
    /// what is typed in the address field before Enter searches. The
    /// pane's like its sort; showing another location clears it.
    filter: Option<Arc<str>>,
    /// Changes with the filter.
    filter_version: u64,
    /// What the folder's files are narrowed to, kept below the folder it
    /// was set in.
    narrowed: Option<Narrowed>,
    /// The filtered order, the order it filtered (by identity), the
    /// filter's version and, while narrowed, the listings' (a file's
    /// metadata arriving decides whether it is shown), and the order.
    filter_cache: parking_lot::Mutex<Option<FilterCache>>,
}

type FilterCache = (Arc<[EntryId]>, u64, (u64, u64), Arc<[EntryId]>);

/// What a folder's files are narrowed to (a locked filter): kept while
/// the pane shows the folder it was set in or one under it
/// (`Pane::replace_open`), let go of anywhere else. What it means is its
/// setter's, the query plugin's `query.narrow`: the core keeps its words
/// and pills and asks it of each file. Every folder is shown whatever it
/// says, so what is under it can be reached.
#[derive(Clone)]
pub struct Narrowed {
    /// The folder it was set in.
    pub root: PathBuf,
    /// Its words, in its setter's language (`kind:image modified:2024`),
    /// what a further choice changes.
    pub expr: String,
    /// A pill each after the trail, each with what takes it off.
    pub pills: Vec<Pill>,
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

/// A pill: its words, and what takes it off.
#[derive(Clone, Debug, PartialEq)]
pub struct Pill {
    pub label: String,
    pub remove: crate::command::Invocation,
    /// The tag it narrows by (“with family”, “not family”): the pill
    /// wears its color, hollow for the files without it.
    pub tag: Option<crate::metadata::TagRef>,
}

/// Whether the pane shows `entry`: its name holds the filter, and the
/// narrowing keeps it.
fn shown(filter: Option<&str>, narrowed: Option<&Narrowed>, entry: &Entry) -> bool {
    filter.is_none_or(|f| name_holds(&entry.name, f)) && narrowed.is_none_or(|n| n.shows(entry))
}

/// Whether `name` holds `filter`: anywhere in it, case ignored unless the
/// filter has a capital.
pub fn name_holds(name: &OsStr, filter: &str) -> bool {
    let name = name.to_string_lossy();
    if filter.chars().any(char::is_uppercase) {
        name.contains(filter)
    } else {
        name.to_lowercase().contains(&filter.to_lowercase())
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
            reveal: None,
            restore: None,
            expanded: Vec::new(),
            expanded_version: 0,
            tree_version: 0,
            order_cache: parking_lot::Mutex::new(None),
            filter: None,
            filter_version: 0,
            narrowed: None,
            filter_cache: parking_lot::Mutex::new(None),
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
        if !self.expanded.is_empty() {
            self.expanded.clear();
            self.expanded_changed();
        }
        self.set_filter(None);
        // A narrowing stays below the folder it was set in.
        let below = |n: &Narrowed| {
            let open = self.open.as_ref().map(|o| o.location().path());
            open.flatten().is_some_and(|path| path.starts_with(&n.root))
        };
        if self.narrowed.as_ref().is_some_and(|n| !below(n)) {
            self.set_narrowed(None);
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

    /// Show only the entries whose name holds `text` (empty or `None`:
    /// all). What is selected and no longer shown is deselected, as a
    /// hidden file is: a Delete must not take what cannot be seen.
    pub(crate) fn set_filter(&mut self, text: Option<&str>) {
        let text = text.map(str::trim).filter(|t| !t.is_empty());
        if self.filter.as_deref() == text {
            return;
        }
        self.filter = text.map(Arc::from);
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
        self.reveal = None;
        // Each closes as it drops.
        self.kept.clear();
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
        Some(self.kept.remove(i).into_shown())
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
        if let Some(left) = self.left.take()
            && wanted.contains(&left.location)
        {
            self.kept.push(left);
        }
        self.kept.retain(|k| wanted.contains(&k.location));
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
        self.retain_shown();
    }

    pub(crate) fn add_kept(&mut self, open: Arc<dyn OpenLocation>, listing: Listing) {
        self.kept.push(Kept::new(open, listing));
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
        self.take_reveal();
    }

    /// `reveal` an entry that its location lists a moment after it is
    /// complete (a tag just made, listed by a worker): the name is kept
    /// until it is listed or the pane shows another location.
    pub fn reveal_coming(&mut self, name: OsString) {
        self.reveal = Some((name, true));
        self.take_reveal();
    }

    fn take_reveal(&mut self) {
        let Some((name, coming)) = &self.reveal else {
            return;
        };
        if let Some(id) = self.listing.find_by_name(name).map(|e| e.id) {
            self.selection.set([id]);
            self.reveal = None;
        } else if self.listing.is_complete() && !coming {
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
        let mut opened = 0;
        for kept in &mut self.kept {
            let n = kept.pump(budget);
            if self.expanded.iter().any(|o| o.location == kept.location) {
                opened += n;
            }
        }
        if opened > 0 {
            self.tree_version += 1;
        }
        let Some(open) = &self.open else { return 0 };
        let rx = open.events().clone();
        let n = self.listing.drain(&rx, budget);
        if n + opened > 0 {
            self.retain_shown();
        }
        if n > 0 {
            self.take_reveal();
        }
        self.take_restore();
        n + opened
    }

    /// Drop from the selection what the pane no longer shows: an entry
    /// gone, a hidden file while hidden files are not shown (one renamed
    /// to a dot name too), a companion hidden beside its file, what the
    /// filter or the narrowing leaves out. A file a narrowing cannot tell
    /// yet (its metadata still coming) stays: it waits, it is not left out.
    fn retain_shown(&mut self) {
        let (listing, kept, expanded) = (&self.listing, &self.kept, &self.expanded);
        let (filter, narrowed, sort) = (self.filter.as_deref(), self.narrowed.as_ref(), self.sort);
        let top = listing.shows(sort);
        let opened: Vec<_> = opened(kept, expanded)
            .map(|(_, k)| (&k.listing, k.listing.shows(sort)))
            .collect();
        self.selection.retain(|id| {
            let found = std::iter::once((listing, &top))
                .chain(opened.iter().map(|(l, shows)| (*l, shows)))
                .find_map(|(l, shows)| l.get(id).map(|e| (e, shows(id))));
            match found {
                Some((e, true)) => {
                    filter.is_none_or(|f| name_holds(&e.name, f))
                        && narrowed.is_none_or(|n| !e.metadata.is_hydrated() || n.shows(e))
                }
                _ => false,
            }
        });
    }

    fn expanded_changed(&mut self) {
        self.expanded_version += 1;
        self.tree_version += 1;
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
            match opened(&self.kept, &self.expanded).find(|(_, k)| k.listing.get(at).is_some()) {
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
        self.retain_shown();
        true
    }

    /// Close `location` and every folder opened inside it; what the
    /// selection held there goes. Returns whether it was open.
    pub(crate) fn collapse(&mut self, location: &Location) -> bool {
        self.close_where(|o| &o.location == location)
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
    }

    /// Close every folder opened in place.
    pub fn collapse_all(&mut self) -> bool {
        if self.expanded.is_empty() {
            return false;
        }
        self.expanded.clear();
        self.expanded_changed();
        self.retain_shown();
        true
    }

    /// An entry the pane shows: the shown listing's, or an opened folder's.
    pub fn entry(&self, id: EntryId) -> Option<&Entry> {
        self.listing
            .get(id)
            .or_else(|| opened(&self.kept, &self.expanded).find_map(|(_, k)| k.listing.get(id)))
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
        opened(&self.kept, &self.expanded)
            .find(|(_, k)| k.listing.get(id).is_some())
            .map(|(o, _)| &o.location)
    }

    pub fn listing_of(&self, id: EntryId) -> Option<u64> {
        if self.listing.get(id).is_some() {
            return Some(self.listing.id());
        }
        opened(&self.kept, &self.expanded)
            .find(|(_, k)| k.listing.get(id).is_some())
            .map(|(_, k)| k.listing.id())
    }

    /// Where an entry of an opened folder lives. `None` for the shown
    /// listing's own entries, which only its location knows. An opened
    /// place's (a tag's) are asked of it: plugin code, like `path_of`.
    pub fn opened_path(&self, id: EntryId) -> Option<PathBuf> {
        if self.expanded.is_empty() || self.listing.get(id).is_some() {
            return None;
        }
        let (o, k) =
            opened(&self.kept, &self.expanded).find(|(_, k)| k.listing.get(id).is_some())?;
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
        opened(&self.kept, &self.expanded)
            .find(|(_, k)| k.listing.get(id).is_some())
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
        let Some((o, _)) =
            opened(&self.kept, &self.expanded).find(|(_, k)| k.listing.get(id).is_some())
        else {
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

    /// `ids` by the location that lists them: the shown one, or an opened
    /// folder's. For hydration, called outside the lock.
    pub(crate) fn by_location(
        &self,
        ids: &[EntryId],
    ) -> Vec<(Arc<dyn OpenLocation>, Vec<EntryId>)> {
        let mut out: Vec<(Arc<dyn OpenLocation>, Vec<EntryId>)> = Vec::new();
        for &id in ids {
            let open = if self.listing.get(id).is_some() {
                self.open.as_ref()
            } else {
                opened(&self.kept, &self.expanded)
                    .find(|(_, k)| k.listing.get(id).is_some())
                    .and_then(|(_, k)| k.open.as_ref())
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
        let all = self.unfiltered_order();
        if !self.is_filtered() {
            return all;
        }
        // A narrowing reads metadata, which changes the listings' versions
        // and not always their order.
        let listings = if self.narrowed.is_some() {
            (self.listing.version(), self.tree_version)
        } else {
            (0, 0)
        };
        let mut cache = self.filter_cache.lock();
        if let Some((from, version, at, order)) = cache.as_ref()
            && Arc::ptr_eq(from, &all)
            && *version == self.filter_version
            && *at == listings
        {
            return Arc::clone(order);
        }
        let (filter, narrowed) = (self.filter.as_deref(), self.narrowed.as_ref());
        let order: Arc<[EntryId]> = all
            .iter()
            .copied()
            .filter(|id| self.entry(*id).is_some_and(|e| shown(filter, narrowed, e)))
            .collect();
        *cache = Some((all, self.filter_version, listings, Arc::clone(&order)));
        order
    }

    /// The files the pane holds whose metadata has not come yet: what a
    /// narrowing cannot tell yet, and does not show until it can. No view
    /// draws them, so whoever narrows asks for them (`hydrate`).
    pub fn files_unknown(&self) -> Vec<EntryId> {
        self.unfiltered_order()
            .iter()
            .copied()
            .filter(|id| {
                self.entry(*id)
                    .is_some_and(|e| !e.is_dir() && !e.is_place() && !e.metadata.is_hydrated())
            })
            .collect()
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

    /// `order` before the filter.
    fn unfiltered_order(&self) -> Arc<[EntryId]> {
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

    /// What the pane stops showing leaves the selection: a dot file once
    /// hidden files are hidden, one renamed to a dot name, one the filter
    /// leaves out. Delete must never act on a file the user cannot see.
    #[test]
    fn what_is_no_longer_shown_leaves_the_selection() {
        use crate::entry::EntryKind;
        use crate::location::ListingEvent;
        let file = |ino, name: &str| Entry::new(EntryId { dev: 1, ino }, name, EntryKind::File);
        let mut p = Pane::new(PaneId(1));
        p.listing.apply(ListingEvent::Added(vec![
            file(1, ".profile"),
            file(2, "it's here.txt"),
            file(3, "Été.md"),
        ]));
        p.set_show_hidden(true);
        p.selection
            .set([1, 2, 3].map(|ino| EntryId { dev: 1, ino }));
        p.set_show_hidden(false);
        assert_eq!(items(&p.selection), [2, 3]);
        p.listing
            .apply(ListingEvent::Updated(vec![file(3, ".Été.md")]));
        p.retain_shown();
        assert_eq!(items(&p.selection), [2]);
        p.selection.set([2].map(|ino| EntryId { dev: 1, ino }));
        p.set_filter(Some("ÉTÉ"));
        assert!(p.selection.is_empty());
    }

    fn items(sel: &Selection) -> Vec<u64> {
        sel.iter().map(|id| id.ino).collect()
    }

    #[test]
    fn a_move_carries_a_selection_that_follows_and_leaves_a_picked_one() {
        let id = |ino| EntryId { dev: 1, ino };
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
        let id = |ino| EntryId { dev: 1, ino };
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
        let id = |ino| EntryId { dev: 1, ino };
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
        let id = |ino| EntryId { dev: 1, ino };
        let order: Vec<EntryId> = (1..=3).map(id).collect();
        let mut sel = Selection::default();
        sel.set([id(2)]);
        sel.sweep_start(false, &order);
        sel.follow(id(3), &order);
        sel.retain_in(&Listing::default());
        assert_eq!(sel.sweeping(), None);
        assert!(!sel.picked());
    }
}
