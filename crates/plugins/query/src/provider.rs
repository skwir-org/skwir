//! The `query` location: a parallel walk of the root that streams matches
//! as a virtual listing. Entries are stat-ed only when the query needs it
//! (and always once for their identity when they match).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::{Entry, EntryId, EntryKind};
use skwir_core::fs::{Dir, Statter, reach};
use skwir_core::location::{
    AsOf, EmptyState, EventSink, ListingEvent, Location, LocationProvider, OpenContext,
    OpenLocation,
};
use skwir_core::op::Change;
use skwir_core::scheduler::{Lane, Priority};

use crate::lang::{self, Candidate, Late, Query, Verdict};

pub const SCHEME: &str = "query";

/// Provider for `query:<expression>` locations. Every walk and stat it
/// starts ends when the plugin is disabled (`lifetime`), not only when its
/// pane moves on.
#[derive(Debug)]
pub struct QueryProvider {
    lifetime: CancelToken,
    open: Searches,
    /// To name the folder a search looks in as the sidebar does.
    app: Weak<App>,
}

/// The searches open now, to check again what a plan changed.
#[derive(Clone, Default)]
pub struct Searches(Arc<Mutex<Vec<(Location, std::sync::Weak<Shared>)>>>);

impl std::fmt::Debug for Searches {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Searches").finish_non_exhaustive()
    }
}

impl Searches {
    /// What a finished plan changed (`Executor::observe`): each open
    /// search checks again, on a worker, the files it changed under its
    /// root, as the walk would have. A search does not watch the folders
    /// it walked, but what skwir did it sees at once.
    pub fn changed(&self, changes: &[Change]) {
        let open: Vec<(Location, Arc<Shared>)> = {
            let mut open = self.0.lock();
            open.retain(|(_, s)| s.strong_count() > 0);
            open.iter()
                .filter_map(|(l, s)| Some((l.clone(), s.upgrade()?)))
                .collect()
        };
        for (location, shared) in open {
            if !shared.touched_by(changes) {
                continue;
            }
            let changes = changes.to_vec();
            let cancel = shared.walk.lock().clone();
            let s = Arc::clone(&shared);
            shared.ctx.scheduler.spawn_in(
                shared.lane(),
                Priority::Normal,
                "query.recheck",
                cancel.clone(),
                move || {
                    s.recheck(&location, &changes, &cancel);
                },
            );
        }
    }
}

impl QueryProvider {
    pub fn new(lifetime: CancelToken, open: Searches, app: Weak<App>) -> Self {
        QueryProvider {
            lifetime,
            open,
            app,
        }
    }
}

impl LocationProvider for QueryProvider {
    fn scheme(&self) -> &str {
        SCHEME
    }

    /// A search holds a lens in its own terms (ADR-107): one brought in is
    /// added to them (`facets::with_lens`), or, for a step shown again,
    /// takes the place of the one they held (`lens::in_place_of_lens`);
    /// what of them the listing can decide is the lens it then holds. One
    /// whose terms take nothing new is shown as it is written; one that
    /// does not read says what is wrong, and the pane keeps its lens.
    fn lensed(
        &self,
        location: &Location,
        lens: Option<&skwir_core::workspace::Lens>,
        bring: skwir_core::workspace::Bring,
    ) -> Option<(Location, Option<skwir_core::workspace::Lens>)> {
        use skwir_core::workspace::Bring;
        let expr = &location.rest;
        let Ok(as_written) = crate::facets::with_lens(expr, "") else {
            return Some((location.clone(), lens.cloned()));
        };
        let words = lens.map_or("", |lens| &*lens.words);
        let brought = match bring {
            Bring::Nothing => Ok(as_written.clone()),
            Bring::Added => crate::facets::with_lens(expr, words),
            Bring::InPlace => crate::lens::in_place_of_lens(expr, words),
        };
        let shown = match brought {
            Ok(brought) if brought != as_written => Location::new(SCHEME, brought),
            _ => location.clone(),
        };
        let own = crate::lens::lens_of(&shown.rest);
        Some((shown, own))
    }

    /// What the search looks for and where, as a saved one is named.
    fn title(&self, location: &Location) -> Option<String> {
        Some(crate::suggested_name(
            self.app.upgrade().as_deref(),
            &location.rest,
        ))
    }

    /// A search is in the folder it looks in, or the place it searches as
    /// it lists (`in:recent:`): Up goes there, and the address shows that
    /// trail, the search as pills after it.
    fn trail(&self, location: &Location) -> Option<skwir_core::location::Trail> {
        let query = lang::parse(&location.rest).ok()?;
        let app = self.app.upgrade();
        let own = skwir_core::location::Crumb::new(
            crate::suggested_name(app.as_deref(), &location.rest),
            location.clone(),
        );
        let (base, crumbs) = match query.within {
            Some(place) => {
                let title = app
                    .as_deref()
                    .map_or_else(|| place.to_string(), |app| app.location_title(&place));
                (
                    None,
                    vec![skwir_core::location::Crumb::new(title, place), own],
                )
            }
            None => (Some(query.root), vec![own]),
        };
        Some(skwir_core::location::Trail {
            base,
            crumbs,
            filters: Vec::new(),
            toggles: Vec::new(),
            holds_places: false,
            one_folder: false,
        })
    }

    fn searches(&self, _location: &Location) -> bool {
        true
    }

    /// The tags its `tag:` terms ask of every match.
    fn tags_shown(&self, location: &Location) -> Vec<String> {
        lang::parse(&location.rest)
            .map(|query| {
                query
                    .required_tags()
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Its root, or what the place it searches within reads (a folder
    /// written `in:path:/…` among them): what an agent's call may open.
    fn folders_read(&self, location: &Location) -> Vec<std::path::PathBuf> {
        match lang::parse(&location.rest) {
            Ok(query) => match query.within {
                Some(place) => self
                    .app
                    .upgrade()
                    .map(|app| app.folders_read(&place))
                    .unwrap_or_default(),
                None => vec![query.root],
            },
            // Refused when opened: nothing is read.
            Err(_) => Vec::new(),
        }
    }

    fn icon(&self, _location: &Location) -> Option<&'static str> {
        Some("search")
    }

    fn empty_state(&self, location: &Location) -> Option<EmptyState> {
        let detail = crate::facets::text_of(&location.rest)
            .filter(|text| !text.is_empty())
            .map(|text| {
                format!(
                    "No file matches “{text}” here. Check the spelling or search a folder higher up."
                )
            })
            .unwrap_or_default();
        Some(EmptyState::new("Nothing found", detail))
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let query = lang::parse(&location.rest).map_err(|e| anyhow::anyhow!("query: {e}"))?;
        // A search of what a place lists (`in:recent:`), not a walk.
        if let Some(place) = query.within.clone() {
            let app = self
                .app
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("the app has ended"))?;
            return crate::within::open(
                &app,
                location.clone(),
                query,
                &place,
                &ctx,
                &self.lifetime,
            );
        }
        // Whether the root is a folder is I/O (a sleeping disk can block
        // it): the walk checks, and reports a listing error.
        anyhow::ensure!(
            query.root.is_absolute(),
            "query root {} is not an absolute path",
            query.root.display()
        );
        let open = QueryLocation::open(location.clone(), query, &ctx, &self.lifetime);
        let mut searches = self.open.0.lock();
        // The searches closed since are let go of here too, not only when
        // a plan runs: each search typed is one.
        searches.retain(|(_, s)| s.strong_count() > 0);
        searches.push((location.clone(), Arc::downgrade(&open.shared)));
        drop(searches);
        Ok(open)
    }
}

pub struct Shared {
    ctx: OpenContext,
    tx: EventSink,
    root: PathBuf,
    /// A remote reached through an alias is known by the opened folder's
    /// device. Keep its lane for every later read, including hydration.
    lane: Mutex<Lane>,
    /// The root's real path, as the last walk read it on its worker:
    /// `None` until then, or when it could not be read.
    real_root: Mutex<Option<PathBuf>>,
    results: Mutex<Results>,
    /// The current walk's token. Results are published while holding it, so
    /// once a refresh has swapped it no batch of the old walk can land.
    walk: Mutex<CancelToken>,
    /// The pane's and the plugin's lifetimes together.
    lifetime: CancelToken,
    /// Rechecks run, results they looked at, and folders whose names were
    /// read for the stores: what the tests count.
    #[cfg(test)]
    counted: Counted,
}

#[cfg(test)]
struct Counted {
    rechecks: std::sync::atomic::AtomicUsize,
    considered: std::sync::atomic::AtomicUsize,
    names_read: std::sync::atomic::AtomicUsize,
    /// The clock the names kept go by: stopped when the search opened, so
    /// what a test counts does not hang on how busy the machine is (the
    /// test of `Named` moves its clock itself).
    clock: Instant,
}

#[cfg(test)]
impl Default for Counted {
    fn default() -> Self {
        Counted {
            rechecks: std::sync::atomic::AtomicUsize::default(),
            considered: std::sync::atomic::AtomicUsize::default(),
            names_read: std::sync::atomic::AtomicUsize::default(),
            clock: Instant::now(),
        }
    }
}

/// The results shown, by id and by path: `path_of` asks by id, and a
/// recheck asks which are at or below each path a plan's changes say (a
/// range of paths in order), not every result for every plan.
#[derive(Default)]
struct Results {
    by_id: HashMap<EntryId, Arc<Path>>,
    by_path: BTreeMap<Arc<Path>, Ids>,
}

/// The results at one path: one, but for a file found there again under
/// another identity before the old result was taken out.
enum Ids {
    One(EntryId),
    Many(Vec<EntryId>),
}

impl Ids {
    fn as_slice(&self) -> &[EntryId] {
        match self {
            Ids::One(id) => std::slice::from_ref(id),
            Ids::Many(ids) => ids,
        }
    }

    fn push(&mut self, id: EntryId) {
        match self {
            Ids::One(one) if *one == id => {}
            Ids::One(one) => *self = Ids::Many(vec![*one, id]),
            Ids::Many(ids) if ids.contains(&id) => {}
            Ids::Many(ids) => ids.push(id),
        }
    }

    /// Take `id` out: whether none is left.
    fn remove(&mut self, id: EntryId) -> bool {
        match self {
            Ids::One(one) => *one == id,
            Ids::Many(ids) => {
                ids.retain(|other| *other != id);
                ids.is_empty()
            }
        }
    }
}

impl Results {
    fn get(&self, id: EntryId) -> Option<&Arc<Path>> {
        self.by_id.get(&id)
    }

    fn insert(&mut self, id: EntryId, path: PathBuf) {
        self.remove(id);
        let path: Arc<Path> = Arc::from(path);
        match self.by_path.get_mut(&*path) {
            Some(ids) => ids.push(id),
            None => {
                self.by_path.insert(Arc::clone(&path), Ids::One(id));
            }
        }
        self.by_id.insert(id, path);
    }

    fn remove(&mut self, id: EntryId) -> Option<Arc<Path>> {
        let path = self.by_id.remove(&id)?;
        if self
            .by_path
            .get_mut(&*path)
            .is_some_and(|ids| ids.remove(id))
        {
            self.by_path.remove(&*path);
        }
        Some(path)
    }

    /// Every result taken out: their ids.
    fn clear(&mut self) -> Vec<EntryId> {
        self.by_path.clear();
        self.by_id.drain().map(|(id, _)| id).collect()
    }

    /// The results at `path` or below it: paths in order keep those below
    /// a folder together, from the folder on.
    fn under<'a>(&'a self, path: &'a Path) -> impl Iterator<Item = (EntryId, &'a Arc<Path>)> {
        self.by_path
            .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
            .take_while(move |(at, _)| at.starts_with(path))
            .flat_map(|(at, ids)| ids.as_slice().iter().map(move |id| (*id, at)))
    }
}

pub struct QueryLocation {
    location: Location,
    shared: Arc<Shared>,
    rx: Receiver<ListingEvent>,
}

/// Batches results and flushes by size or age so the first ones show early.
struct Batcher<'a> {
    shared: &'a Shared,
    cancel: &'a CancelToken,
    pending: Mutex<(Vec<(Entry, PathBuf)>, Instant)>,
    batch_size: usize,
}

impl Batcher<'_> {
    const MAX_AGE: Duration = Duration::from_millis(100);

    /// What the late terms read with, under this search's token.
    fn late(&self) -> Late<'_> {
        Late {
            fields: &self.shared.ctx.fields,
            cancel: self.cancel,
        }
    }

    fn push(&self, entry: Entry, path: PathBuf) {
        if self.cancel.is_cancelled() {
            return;
        }
        let flush = {
            let mut p = self.pending.lock();
            p.0.push((entry, path));
            if p.0.len() >= self.batch_size || p.1.elapsed() >= Self::MAX_AGE {
                p.1 = Instant::now();
                Some(std::mem::take(&mut p.0))
            } else {
                None
            }
        };
        if let Some(batch) = flush {
            self.shared.publish(self.cancel, batch, false);
        }
    }

    /// Publish what waits, now.
    fn flush(&self) {
        let batch = std::mem::take(&mut self.pending.lock().0);
        self.shared.publish(self.cancel, batch, false);
    }

    fn finish(&self) {
        let batch = std::mem::take(&mut self.pending.lock().0);
        self.shared.publish(self.cancel, batch, true);
    }
}

/// Matches that wait for the metadata providers (tag terms), grouped by
/// folder so each provider gets a batch, not a call per file. Shared by
/// the walk's threads, which take a folder's files from one another: a
/// group of each thread waited for the walk to end, holding every
/// candidate of the tree, and showed its matches only then. A folder's
/// group is decided once full, or once it waited `Batcher::MAX_AGE`, by
/// the thread that next passes by its shard (`Shard::take_old`), and what
/// is left when the walk ends (`finish`). The groups are in shards by
/// folder, so the threads seldom wait for one another, and the groups
/// that waited are decided by every thread, a few at a time.
struct Deferred<'a> {
    shared: &'a Shared,
    query: &'a Query,
    batcher: &'a Batcher<'a>,
    shards: Vec<Mutex<Shard>>,
    /// The names of the folders whose groups are being decided
    /// (`Deferred::names_of`): read once for every batch of a folder and
    /// handed to every provider (`fill_listed`), so the stores that look
    /// for files beside a file (an XMP sidecar, a Mac's `._` file, a
    /// `TagSpaces` folder) do not each read the folder, and stat it again
    /// for each batch.
    names: Mutex<Named>,
}

/// Folders' names kept while their groups keep coming ([`Named::IDLE`]),
/// within a bound in all. A folder is let go of once unused a moment, the
/// least recently used first past the bound: 32 folders kept, the last
/// decided, let a folder of a hundred thousand names go between two of
/// its groups while the walk's threads decided others, and read it again
/// for each. A folder is read by the first thread that needs it; those
/// asking meanwhile wait for that read (`Slot`), where two threads deciding
/// two groups of one folder at once read it twice.
#[derive(Default)]
struct Named {
    folders: HashMap<PathBuf, Kept>,
    /// Each use, oldest first, with its stamp: a folder used since has a
    /// later one (`Kept::stamp`), and its older uses are passed over.
    uses: VecDeque<(PathBuf, u64)>,
    stamp: u64,
    /// Names kept in all.
    count: usize,
}

/// A folder's names, filled once by the thread that reads them.
type Slot = Arc<std::sync::OnceLock<Arc<HashSet<OsString>>>>;

struct Kept {
    slot: Slot,
    /// Its names once read, counted against the bound.
    len: usize,
    used: Instant,
    stamp: u64,
}

impl Named {
    /// How long a folder's names are kept unused: its groups come one
    /// after another while the walk passes it, each within
    /// `Batcher::MAX_AGE` of its first file.
    const IDLE: Duration = Duration::from_millis(500);
    /// Names kept at most in all; past it, the folders least recently used
    /// are let go of, never the one just used.
    const AT_MOST: usize = 256 * 1024;

    /// `dir`'s slot, used now: the one kept, filled or being filled, or a
    /// new one for the caller to fill and say so ([`Named::read`]).
    fn slot(&mut self, dir: &Path, now: Instant) -> Slot {
        self.stamp += 1;
        let stamp = self.stamp;
        let slot = if let Some(kept) = self.folders.get_mut(dir) {
            kept.used = now;
            kept.stamp = stamp;
            Arc::clone(&kept.slot)
        } else {
            let slot = Slot::default();
            let kept = Kept {
                slot: Arc::clone(&slot),
                len: 0,
                used: now,
                stamp,
            };
            self.folders.insert(dir.to_path_buf(), kept);
            slot
        };
        self.uses.push_back((dir.to_path_buf(), stamp));
        self.let_go(now);
        slot
    }

    /// `dir`'s `slot` was filled with `len` names: counted against the
    /// bound, unless it was let go of meanwhile.
    fn read(&mut self, dir: &Path, slot: &Slot, len: usize, now: Instant) {
        if let Some(kept) = self
            .folders
            .get_mut(dir)
            .filter(|kept| Arc::ptr_eq(&kept.slot, slot))
        {
            self.count = self.count - kept.len + len;
            kept.len = len;
            self.let_go(now);
        }
    }

    /// Let go of the folders unused a moment, and of the least recently
    /// used past the bound.
    fn let_go(&mut self, now: Instant) {
        while let Some((dir, stamp)) = self.uses.front() {
            let go = match self.folders.get(dir) {
                // Used since (a later use stands for it), or let go of.
                Some(kept) if kept.stamp != *stamp => false,
                None => false,
                Some(kept) => {
                    let idle = now.duration_since(kept.used) >= Self::IDLE;
                    let over = self.count > Self::AT_MOST && *stamp != self.stamp;
                    if !idle && !over {
                        return;
                    }
                    true
                }
            };
            let (dir, _) = self.uses.pop_front().expect("the front was there");
            if go && let Some(kept) = self.folders.remove(&dir) {
                self.count -= kept.len;
            }
        }
    }
}

#[derive(Default)]
struct Shard {
    groups: HashMap<PathBuf, (Vec<Entry>, Instant)>,
    /// The groups as they began, oldest first.
    order: std::collections::VecDeque<(PathBuf, Instant)>,
}

impl Shard {
    /// The oldest group that waited long enough, out of the shard.
    fn take_old(&mut self, now: Instant) -> Option<(PathBuf, Vec<Entry>)> {
        while let Some((_, since)) = self.order.front() {
            if now.duration_since(*since) < Batcher::MAX_AGE {
                return None;
            }
            let (dir, since) = self.order.pop_front()?;
            // Still that group: not decided full and begun again since.
            if self
                .groups
                .get(&dir)
                .is_some_and(|(_, began)| *began == since)
            {
                let (entries, _) = self.groups.remove(&dir)?;
                return Some((dir, entries));
            }
        }
        None
    }
}

impl Deferred<'_> {
    const BATCH: usize = 64;
    const SHARDS: usize = 16;

    fn new<'a>(shared: &'a Shared, query: &'a Query, batcher: &'a Batcher<'a>) -> Deferred<'a> {
        Deferred {
            shared,
            query,
            batcher,
            shards: (0..Self::SHARDS).map(|_| Mutex::default()).collect(),
            names: Mutex::default(),
        }
    }

    /// Every name in `dir`, hidden ones included, read once for the
    /// batches of its group. On a walk's thread.
    fn names_of(&self, dir: &Path) -> Arc<HashSet<OsString>> {
        let slot = self.names.lock().slot(dir, self.now());
        // Read here unless another thread does: then wait for its read.
        let mut read = None;
        let names = slot.get_or_init(|| {
            let names: Arc<HashSet<OsString>> = Arc::new(
                std::fs::read_dir(dir)
                    .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
                    .unwrap_or_default(),
            );
            read = Some(names.len());
            names
        });
        if let Some(len) = read {
            #[cfg(test)]
            self.shared
                .counted
                .names_read
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.names.lock().read(dir, &slot, len, self.now());
        }
        Arc::clone(names)
    }

    /// The time the names kept go by: stopped in a test.
    // `self` holds the clock only in tests.
    #[cfg_attr(not(test), allow(clippy::unused_self))]
    fn now(&self) -> Instant {
        #[cfg(test)]
        return self.shared.counted.clock;
        #[cfg(not(test))]
        Instant::now()
    }

    fn shard(&self, dir: &Path) -> &Mutex<Shard> {
        use std::hash::{BuildHasher, BuildHasherDefault};
        let hash = BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default()
            .hash_one(dir);
        &self.shards[usize::try_from(hash % Self::SHARDS as u64).unwrap_or(0)]
    }

    fn add(&self, dir: &Path, entry: Entry) {
        let (full, old) = {
            let mut shard = self.shard(dir).lock();
            let now = Instant::now();
            let len = if let Some((group, _)) = shard.groups.get_mut(dir) {
                group.push(entry);
                group.len()
            } else {
                shard.order.push_back((dir.to_path_buf(), now));
                shard.groups.insert(dir.to_path_buf(), (vec![entry], now));
                1
            };
            let full = if len >= Self::BATCH {
                shard.groups.remove(dir).map(|(entries, _)| entries)
            } else {
                None
            };
            (full, shard.take_old(now))
        };
        if let Some(entries) = full {
            self.decide(dir, entries);
        }
        if let Some((dir, entries)) = old {
            self.decide(&dir, entries);
        }
    }

    /// Decide the oldest group of `dir`'s shard if it waited long enough:
    /// a file that needs no metadata passing by.
    fn settle(&self, dir: &Path) {
        let old = self.shard(dir).lock().take_old(Instant::now());
        if let Some((dir, entries)) = old {
            self.decide(&dir, entries);
        }
    }

    /// Decide every group: the walk ended.
    fn finish(&self) {
        for shard in &self.shards {
            let all = std::mem::take(&mut *shard.lock()).groups;
            for (dir, (entries, _)) in all {
                self.decide(&dir, entries);
            }
        }
    }

    fn decide(&self, dir: &Path, mut entries: Vec<Entry>) {
        if self.batcher.cancel.is_cancelled() {
            return;
        }
        // Read once a store asks: only those that look for files beside a
        // file do.
        let names = std::cell::OnceCell::new();
        let listed = |name: &OsStr| names.get_or_init(|| self.names_of(dir)).contains(name);
        self.shared
            .ctx
            .metadata
            .fill_listed(dir, &listed, &mut entries);
        for entry in entries {
            let path = dir.join(&entry.name);
            if full_match(self.query, &entry, &path, self.batcher.late()) {
                self.batcher.push(entry, path);
            }
        }
    }
}

fn kind_of(ft: Option<std::fs::FileType>) -> EntryKind {
    ft.map_or(EntryKind::Unknown, EntryKind::from)
}

impl QueryLocation {
    fn open(
        location: Location,
        query: Query,
        ctx: &OpenContext,
        plugin: &CancelToken,
    ) -> Arc<Self> {
        let (tx, rx) = ctx.channel();
        let lifetime = ctx.cancel.linked(plugin);
        let shared = Arc::new(Shared {
            ctx: ctx.clone(),
            tx,
            root: query.root.clone(),
            lane: Mutex::new(reach::lane(&query.root)),
            real_root: Mutex::new(None),
            results: Mutex::default(),
            walk: Mutex::new(lifetime.child()),
            lifetime,
            #[cfg(test)]
            counted: Counted::default(),
        });
        Shared::start(&shared, &location, query);
        Arc::new(QueryLocation {
            location,
            shared,
            rx,
        })
    }
}

/// The longest a rollover waits before it looks at the clock again: the
/// monotonic clock a wait counts stops while the machine sleeps, the wall
/// clock does not, so a laptop asleep past midnight is at most this late.
const ROLLOVER_CHECK: Duration = Duration::from_secs(600);

impl Shared {
    /// Mount lookup is memory-only: callers may be on the window thread.
    fn lane(&self) -> Lane {
        let lane = reach::lane(&self.root);
        if lane.is_shared() {
            *self.lane.lock()
        } else {
            lane
        }
    }

    /// Whether a change says a path at or below the root, or a folder
    /// above it (moving it): a search elsewhere has nothing to check
    /// again. From memory, on the thread that applied the plan: the root's
    /// real path was read by the walk, and until it was, every plan is
    /// checked.
    fn touched_by(&self, changes: &[Change]) -> bool {
        let Some(real) = self.real_root.lock().clone() else {
            return true;
        };
        let near = |path: &Path, root: &Path| path.starts_with(root) || root.starts_with(path);
        changes
            .iter()
            .flat_map(Change::paths)
            .any(|path| near(path, &self.root) || near(path, &real))
    }

    /// Walk for `query`, and list again when its periods move on
    /// (`modified:today` at midnight).
    fn start(this: &Arc<Self>, location: &Location, query: Query) {
        let cancel = this.walk.lock().clone();
        if let Some(when) = query.next_change() {
            Shared::roll_over_at(this, location, when, cancel.clone());
        }
        let s = Arc::clone(this);
        let lane = this.lane();
        this.ctx.scheduler.spawn_in(
            lane,
            Priority::Normal,
            "query.walk",
            cancel.clone(),
            move || Shared::begin(&s, query, cancel, lane),
        );
    }

    /// Opening the root can wait too. Start in its known lane; if its
    /// device reveals a remote alias, move the walk into that lane before
    /// reading any descendants. No filesystem call holds a shared lock.
    fn begin(this: &Arc<Self>, query: Query, cancel: CancelToken, lane: Lane) {
        let device = Dir::open(&query.root).and_then(|dir| dir.device());
        let device = match device {
            Ok(device) => device,
            Err(error) => {
                let _walk = this.walk.lock();
                if !cancel.is_cancelled() {
                    this.tx.send(ListingEvent::Error(format!(
                        "query root {} is not a folder: {error}",
                        query.root.display()
                    )));
                }
                return;
            }
        };
        if cancel.is_cancelled() {
            return;
        }
        let reached = if lane.is_shared() {
            reach::of_device(device).map_or(lane, |remote| remote.lane())
        } else {
            lane
        };
        *this.lane.lock() = reached;
        if reached == lane {
            this.run(&query, &cancel);
        } else {
            let shared = Arc::clone(this);
            this.ctx.scheduler.spawn_in(
                reached,
                Priority::Normal,
                "query.walk",
                cancel.clone(),
                move || shared.run(&query, &cancel),
            );
        }
    }

    /// Walk again from scratch: parsed again, relative dates (`modified<7d`,
    /// `today`) mean "from now".
    fn rewalk(this: &Arc<Self>, location: &Location) {
        let query = match lang::parse(&location.rest) {
            Ok(query) => query,
            Err(e) => {
                this.tx.send(ListingEvent::Error(format!("query: {e}")));
                return;
            }
        };
        let old: Vec<EntryId> = {
            let mut walk = this.walk.lock();
            walk.cancel();
            *walk = this.lifetime.child();
            this.results.lock().clear()
        };
        if !old.is_empty() {
            this.tx.send(ListingEvent::Removed(old));
        }
        Shared::start(this, location, query);
    }

    /// At `when`, walk again, unless this walk ended first (a refresh, the
    /// pane moving on, the plugin disabled). A thread of its own that
    /// sleeps, not a worker: the wait can last until next year.
    fn roll_over_at(this: &Arc<Self>, location: &Location, when: SystemTime, cancel: CancelToken) {
        let (shared, location) = (Arc::downgrade(this), location.clone());
        let spawned = std::thread::Builder::new()
            .name("skwir-query-rollover".into())
            .spawn(move || {
                let signal = cancel.subscribe();
                while let Ok(left) = when.duration_since(SystemTime::now()) {
                    match signal.receiver().recv_timeout(left.min(ROLLOVER_CHECK)) {
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                        _ => return,
                    }
                }
                if let Some(shared) = shared.upgrade()
                    && !cancel.is_cancelled()
                {
                    Shared::rewalk(&shared, &location);
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("query rollover: {e}");
        }
    }
}

/// Decide with the entry's stat and metadata, reading content and the
/// fields asked of it (a place) last.
pub(crate) fn full_match(q: &Query, entry: &Entry, path: &Path, late: Late<'_>) -> bool {
    let Some(expr) = &q.expr else {
        return true;
    };
    let c = Candidate::of_entry(entry, path).with_late(late);
    match expr.eval(&c, false) {
        Verdict::No => false,
        Verdict::Yes => true,
        Verdict::Unknown => expr.eval(&c, true) == Verdict::Yes,
    }
}

/// Evaluate one path as far as the dirent and a stat allow.
/// What is known of a file before its cheap terms: its kind, from the
/// walk's dirent, or the file stat-ed whole, one an index or `plocate`
/// named, stat-ed once for its kind and its entry.
enum Known {
    Kind(EntryKind),
    Stated(Entry),
}

impl Known {
    /// The file `path` named by an index or `plocate`, stat-ed: `None`
    /// once it is gone.
    fn stated(path: &Path, statter: &mut Statter) -> Option<Known> {
        statter.stat(path).map(Known::Stated)
    }

    fn kind(&self) -> EntryKind {
        match self {
            Known::Kind(kind) => *kind,
            Known::Stated(entry) => entry.kind,
        }
    }

    /// The file's entry: stat-ed now, unless it was.
    fn entry(self, path: &Path, statter: &mut Statter) -> Option<Entry> {
        match self {
            Known::Kind(_) => statter.stat(path),
            Known::Stated(entry) => Some(entry),
        }
    }
}

fn first(
    q: &Query,
    path: &Path,
    name: &OsStr,
    known: Known,
    late: Late<'_>,
    statter: &mut Statter,
) -> First {
    let Some(expr) = &q.expr else {
        return known.entry(path, statter).map_or(First::No, First::Yes);
    };
    // 1. Cheap terms from the dirent.
    match expr.eval(&Candidate::new(name, known.kind(), path, None), false) {
        Verdict::No => return First::No,
        Verdict::Yes => return known.entry(path, statter).map_or(First::No, First::Yes),
        Verdict::Unknown => {}
    }
    // 2. Stat; metadata comes later, in batches.
    let Some(entry) = known.entry(path, statter) else {
        return First::No;
    };
    if q.needs_metadata() {
        return First::NeedsMetadata(entry);
    }
    if full_match(q, &entry, path, late) {
        First::Yes(entry)
    } else {
        First::No
    }
}

fn consider(
    q: &Query,
    path: &Path,
    (name, known): (&OsStr, Known),
    batcher: &Batcher<'_>,
    deferred: &Deferred<'_>,
    statter: &mut Statter,
) {
    match first(q, path, name, known, batcher.late(), statter) {
        First::No => {
            if q.needs_metadata()
                && let Some(dir) = path.parent()
            {
                deferred.settle(dir);
            }
        }
        First::Yes(entry) => batcher.push(entry, path.to_path_buf()),
        First::NeedsMetadata(entry) => {
            if let Some(dir) = path.parent() {
                deferred.add(dir, entry);
            }
        }
    }
}

/// Which folders under a search's root the walk reaches (never through a
/// link), each looked at once while changes are checked.
struct Reached {
    real_root: PathBuf,
    folders: HashMap<PathBuf, bool>,
}

/// The network mounts below `root` (not `root` itself), which a walk of
/// it never enters (ADR-077), from the mount table in memory: what an
/// index names under one is left out as the walk leaves it, before
/// anything there is read.
pub(crate) fn mounts_below(root: &Path) -> Vec<PathBuf> {
    reach::all()
        .iter()
        .filter(|r| r.point != root && r.point.starts_with(root))
        .map(|r| r.point.clone())
        .collect()
}

/// Whether Windows marks `path`, or a folder of it below `root`, hidden:
/// what the walk (the `ignore` crate) leaves out there beside a name's dot
/// (`plocate::visible`). Each folder is asked once (`known`): an index
/// names many files of one. Elsewhere a dot alone says it.
#[cfg(windows)]
fn marked_hidden(path: &Path, root: &Path, known: &mut HashMap<PathBuf, bool>) -> bool {
    use std::os::windows::fs::MetadataExt;
    /// `FILE_ATTRIBUTE_HIDDEN`.
    const HIDDEN: u32 = 2;
    let marked =
        |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & HIDDEN != 0);
    marked(path)
        || path
            .ancestors()
            .skip(1)
            .take_while(|above| *above != root)
            .any(|above| {
                *known
                    .entry(above.to_path_buf())
                    .or_insert_with(|| marked(above))
            })
}

#[cfg(not(windows))]
fn marked_hidden(_: &Path, _: &Path, _: &mut HashMap<PathBuf, bool>) -> bool {
    false
}

/// Reject paths the walk cannot reach before any filesystem read: hidden
/// names, excess depth, and a network mount anywhere below the root.
fn in_walk<'a>(q: &Query, path: &'a Path) -> Option<&'a Path> {
    let relative = path.strip_prefix(&q.root).ok()?;
    if !crate::plocate::visible(relative, q.hidden, q.depth)
        || path
            .ancestors()
            .take_while(|above| *above != q.root)
            .any(reach::is_point)
    {
        return None;
    }
    Some(relative)
}

impl Reached {
    /// Whether `dir`, which is `relative` under the root, is the folder
    /// the walk would find there.
    fn reaches(&mut self, dir: &Path, relative: &Path) -> bool {
        if let Some(&known) = self.folders.get(dir) {
            return known;
        }
        let real = self.real_root.join(relative);
        let reached = skwir_core::os_path::canonical(dir).is_ok_and(|c| c == real);
        self.folders.insert(dir.to_path_buf(), reached);
        reached
    }
}

/// What the cheap checks decided about a path.
enum First {
    No,
    Yes(Entry),
    /// Stat-ed; the metadata providers must run before it can be decided.
    NeedsMetadata(Entry),
}

impl Shared {
    /// Send a batch of matches (and `Complete` after the last one) unless
    /// the walk was cancelled; see `walk`.
    fn publish(&self, cancel: &CancelToken, batch: Vec<(Entry, PathBuf)>, complete: bool) {
        let _walk = self.walk.lock();
        if cancel.is_cancelled() {
            return;
        }
        if !batch.is_empty() {
            let mut results = self.results.lock();
            let entries = batch
                .into_iter()
                .map(|(entry, path)| {
                    results.insert(entry.id, path);
                    entry
                })
                .collect();
            drop(results);
            self.tx.send(ListingEvent::Added(entries));
        }
        if complete {
            self.tx.send(ListingEvent::Complete);
        }
    }

    /// Say what this run's answer rests on (`ListingEvent::AsOf`): an
    /// index as of its last update, or the files as the walk finds them.
    /// Each run says it, so a walk takes back what plocate said before.
    fn rests_on(&self, cancel: &CancelToken, as_of: Option<AsOf>) {
        let _walk = self.walk.lock();
        if !cancel.is_cancelled() {
            self.tx.send(ListingEvent::AsOf(as_of));
        }
    }

    fn run(&self, q: &Query, cancel: &CancelToken) {
        // What a plan's changes are compared with (`touched_by`), read here,
        // in the root's lane.
        *self.real_root.lock() = skwir_core::os_path::canonical(&q.root).ok();
        let batcher = Batcher {
            shared: self,
            cancel,
            pending: Mutex::new((Vec::new(), Instant::now())),
            batch_size: self.ctx.batch_size.max(1),
        };
        let deferred = Deferred::new(self, q, &batcher);
        // What an index says may match, considered first; the walk skips it.
        let hinted = {
            let hinted = self.hinted(q, cancel, &batcher, &deferred);
            deferred.finish();
            batcher.flush();
            hinted
        };
        if let Some(located) = crate::plocate::try_locate(q, cancel) {
            self.rests_on(
                cancel,
                Some(AsOf {
                    by: crate::plocate::BY.to_owned(),
                    updated: located.updated,
                }),
            );
            let mut statter = Statter::default();
            for p in located.paths {
                if cancel.is_cancelled() {
                    return;
                }
                if hinted.contains(&p) {
                    continue;
                }
                let Some(name) = p.file_name() else { continue };
                let Some(known) = Known::stated(&p, &mut statter) else {
                    continue;
                };
                consider(q, &p, (name, known), &batcher, &deferred, &mut statter);
            }
        } else {
            self.rests_on(cancel, None);
            Self::walk_below(q, &q.root, &hinted, &batcher, &deferred);
        }
        deferred.finish();
        batcher.finish();
    }

    /// Stream a subtree by the same rules as the initial walk. Depth is
    /// always relative to the search root, even for an arriving folder.
    fn walk_below(
        q: &Query,
        root: &Path,
        checked: &HashSet<PathBuf>,
        batcher: &Batcher<'_>,
        deferred: &Deferred<'_>,
    ) {
        if batcher.cancel.is_cancelled() {
            return;
        }
        let below = root.strip_prefix(&q.root).unwrap().components().count();
        let search_root = q.root.clone();
        let mut builder = ignore::WalkBuilder::new(root);
        builder
            .hidden(!q.hidden)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(false)
            .follow_links(false)
            .max_depth(q.depth.map(|depth| depth.saturating_sub(below)))
            // A network mount below the folder searched is not entered:
            // a search of one walks it from its own folder (ADR-077).
            .filter_entry(move |dent| dent.path() == search_root || !reach::is_point(dent.path()));
        let cancel = batcher.cancel;
        let visit = |result: Result<ignore::DirEntry, ignore::Error>, statter: &mut Statter| {
            if cancel.is_cancelled() {
                return ignore::WalkState::Quit;
            }
            let Ok(dent) = result else {
                return ignore::WalkState::Continue;
            };
            if dent.depth() == 0 {
                return ignore::WalkState::Continue;
            }
            let path = dent.path();
            let Some(name) = path.file_name() else {
                return ignore::WalkState::Continue;
            };
            if checked.contains(path) {
                return ignore::WalkState::Continue;
            }
            let known = Known::Kind(kind_of(dent.file_type()));
            consider(q, path, (name, known), batcher, deferred, statter);
            ignore::WalkState::Continue
        };
        // A remote's tree is walked on this job's thread, in its lane, a
        // folder at a time as a listing reads one (ADR-077): the walker's
        // own threads would ask its server as many at once as this
        // machine has cores, outside the lane, and a server that stops
        // answering would hold threads no one stands in for.
        if !batcher.shared.lane().is_shared() {
            let mut statter = Statter::default();
            for result in builder.build() {
                if matches!(visit(result, &mut statter), ignore::WalkState::Quit) {
                    return;
                }
            }
            return;
        }
        let visit = &visit;
        builder.build_parallel().run(|| {
            // The walking thread's stats, through the folder it is in.
            let mut statter = Statter::default();
            Box::new(move |result| visit(result, &mut statter))
        });
    }

    /// Consider the files the indexes name for the query's tag (ADR-026),
    /// as the walk would have (hidden files and depth as it would, never
    /// through a link, nor into a network mount below the root), and
    /// return them for the walk to skip.
    fn hinted(
        &self,
        q: &Query,
        cancel: &CancelToken,
        batcher: &Batcher<'_>,
        deferred: &Deferred<'_>,
    ) -> HashSet<PathBuf> {
        let mut hinted = HashSet::new();
        let Some(tag) = q.required_tag() else {
            return hinted;
        };
        // Indexes know real paths; the walk goes through the root as typed.
        let Ok(real_root) = skwir_core::os_path::canonical(&q.root) else {
            return hinted;
        };
        let Some(paths) =
            self.ctx
                .hints
                .candidates(skwir_core::entry::keys::TAGS, tag, &real_root, cancel)
        else {
            return hinted;
        };
        let started = Instant::now();
        let mut statter = Statter::default();
        let mut linked: HashMap<PathBuf, bool> = HashMap::new();
        let mut marked: HashMap<PathBuf, bool> = HashMap::new();
        let unreached = mounts_below(&real_root);
        for real in paths {
            if cancel.is_cancelled() {
                break;
            }
            let Ok(relative) = real.strip_prefix(&real_root) else {
                continue;
            };
            if !crate::plocate::visible(relative, q.hidden, q.depth)
                || unreached.iter().any(|point| real.starts_with(point))
            {
                continue;
            }
            // A folder on the way that is a link: the walk would not go
            // there. Asked once per folder: an index names many files of
            // one.
            let through_link = real.parent().is_none_or(|parent| {
                *linked.entry(parent.to_path_buf()).or_insert_with(|| {
                    skwir_core::os_path::canonical(parent).ok().as_deref() != Some(parent)
                })
            });
            let Some(name) = real.file_name() else {
                continue;
            };
            if through_link || (!q.hidden && marked_hidden(&real, &real_root, &mut marked)) {
                continue;
            }
            let path = q.root.join(relative);
            // Gone since the index saw it: the walk will not find it either.
            let Some(known) = Known::stated(&path, &mut statter) else {
                continue;
            };
            consider(q, &path, (name, known), batcher, deferred, &mut statter);
            hinted.insert(path);
        }
        tracing::debug!(
            hinted = hinted.len(),
            took_us = started.elapsed().as_micros() as u64,
            "search hints considered"
        );
        hinted
    }

    /// Check again, as the walk would, the files `changes` touched under
    /// the search's root: a result changed is read again and kept or
    /// taken out, one moved is followed, one gone is taken out, and a file
    /// that matches now is added.
    fn recheck(&self, location: &Location, changes: &[Change], cancel: &CancelToken) {
        let Ok(q) = lang::parse(&location.rest) else {
            return;
        };
        let mut removed: Vec<EntryId> = Vec::new();
        // Paths to read again, with the result each was, if one.
        let mut again: Vec<(PathBuf, Option<EntryId>)> = Vec::new();
        // The results a change may move, take away or change: those at or
        // below a path it says (`Follow::through` follows a path through
        // the changes that name it or a folder above it). Followed outside
        // the lock, which `path_of` waits for under the workspace's.
        let results: Vec<(EntryId, PathBuf)> = {
            let results = self.results.lock();
            let mut picked = HashSet::new();
            changes
                .iter()
                .flat_map(Change::paths)
                .flat_map(|path| results.under(path))
                .filter(|(id, _)| picked.insert(*id))
                .map(|(id, path)| (id, path.to_path_buf()))
                .collect()
        };
        #[cfg(test)]
        {
            use std::sync::atomic::Ordering::Relaxed;
            self.counted.rechecks.fetch_add(1, Relaxed);
            self.counted.considered.fetch_add(results.len(), Relaxed);
        }
        let follow = skwir_core::op::Follow::new(changes);
        for (id, path) in results {
            let through = follow.through(&path);
            match through.at {
                None => removed.push(id),
                Some(at) if through.moved || through.changed => again.push((at, Some(id))),
                Some(_) => {}
            }
        }
        let mut seen: HashSet<PathBuf> = again.iter().map(|(p, _)| p.clone()).collect();
        let mut arrived_folders = Vec::new();
        for change in changes {
            let arrived = match change {
                Change::Metadata { path, .. }
                | Change::Create { path, .. }
                | Change::Restore { path } => vec![path.clone()],
                Change::Rename { to, .. }
                | Change::Move { to, .. }
                | Change::Copy { to, .. }
                | Change::Link { to, .. } => vec![to.clone()],
                Change::Swap { a, b } => vec![a.clone(), b.clone()],
                Change::Trash { .. } | Change::Delete { .. } | Change::Remove { .. } => Vec::new(),
            };
            for path in arrived {
                if !matches!(change, Change::Metadata { .. }) {
                    arrived_folders.push(path.clone());
                }
                if seen.insert(path.clone()) {
                    again.push((path, None));
                }
            }
        }
        let mut found: Vec<(Entry, PathBuf)> = Vec::new();
        // A result that is still one under another name (renamed, moved):
        // its row follows it, and so does its selection.
        let mut renamed: Vec<(EntryId, Entry, PathBuf)> = Vec::new();
        let mut statter = Statter::default();
        let late = Late {
            fields: &self.ctx.fields,
            cancel,
        };
        let Ok(real_root) = skwir_core::os_path::canonical(&q.root) else {
            return;
        };
        let mut reached = Reached {
            real_root,
            folders: HashMap::new(),
        };
        for (path, was) in again {
            if cancel.is_cancelled() {
                return;
            }
            let matched = self.matches_now(&q, &path, late, &mut statter, &mut reached);
            match (matched, was) {
                (Some(entry), Some(id)) if entry.id != id => renamed.push((id, entry, path)),
                (Some(entry), _) => found.push((entry, path)),
                (None, Some(id)) => removed.push(id),
                (None, None) => {}
            }
        }
        if !removed.is_empty() {
            let _walk = self.walk.lock();
            if cancel.is_cancelled() {
                return;
            }
            let mut results = self.results.lock();
            for id in &removed {
                results.remove(*id);
            }
            drop(results);
            self.tx.send(ListingEvent::Removed(removed));
        }
        if !renamed.is_empty() {
            let _walk = self.walk.lock();
            if cancel.is_cancelled() {
                return;
            }
            let mut results = self.results.lock();
            let pairs = renamed
                .into_iter()
                .map(|(was, entry, path)| {
                    results.remove(was);
                    results.insert(entry.id, path);
                    (was, entry)
                })
                .collect();
            drop(results);
            self.tx.send(ListingEvent::Renamed(pairs));
        }
        self.publish(cancel, found, false);
        self.discover(&q, arrived_folders, &seen, cancel, &mut reached);
    }

    /// A directory arriving can contain results the search never knew.
    /// Follow known rows first, then stream only the unexamined descendants
    /// without resetting the listing or announcing another completion.
    fn discover(
        &self,
        q: &Query,
        mut arrivals: Vec<PathBuf>,
        checked: &HashSet<PathBuf>,
        cancel: &CancelToken,
        reached: &mut Reached,
    ) {
        arrivals.sort();
        arrivals.dedup();
        let batcher = Batcher {
            shared: self,
            cancel,
            pending: Mutex::new((Vec::new(), Instant::now())),
            batch_size: self.ctx.batch_size.max(1),
        };
        let deferred = Deferred::new(self, q, &batcher);
        let mut statter = Statter::default();
        let mut previous: Option<PathBuf> = None;
        for path in arrivals {
            if cancel.is_cancelled() {
                return;
            }
            if previous
                .as_ref()
                .is_some_and(|above| path.starts_with(above))
            {
                continue;
            }
            if path != q.root {
                let Some(relative) = in_walk(q, &path) else {
                    continue;
                };
                // Canonical parents reject traversal through symlinks;
                // the final entry's kind below rejects a symlink root.
                if !path
                    .parent()
                    .zip(relative.parent())
                    .is_some_and(|(dir, rel)| reached.reaches(dir, rel))
                {
                    continue;
                }
            }
            if !statter.stat(&path).is_some_and(|entry| {
                entry.kind == EntryKind::Directory || (path == q.root && entry.is_dir())
            }) {
                continue;
            }
            Self::walk_below(q, &path, checked, &batcher, &deferred);
            previous = Some(path);
        }
        deferred.finish();
        batcher.flush();
    }

    /// `path` as a result of `q`, if it is one now and the walk would
    /// reach it (under the root, hidden files and depth as it would, never
    /// through a link).
    fn matches_now(
        &self,
        q: &Query,
        path: &Path,
        late: Late<'_>,
        statter: &mut Statter,
        reached: &mut Reached,
    ) -> Option<Entry> {
        let relative = in_walk(q, path)?;
        let dir = path.parent()?;
        if !reached.reaches(dir, relative.parent()?) {
            return None;
        }
        let name = path.file_name()?;
        let known = Known::stated(path, statter)?;
        match first(q, path, name, known, late, statter) {
            First::No => None,
            First::Yes(entry) => Some(entry),
            First::NeedsMetadata(entry) => {
                let mut entries = [entry];
                self.ctx.metadata.fill(dir, &mut entries);
                let [entry] = entries;
                full_match(q, &entry, path, late).then_some(entry)
            }
        }
    }

    fn hydrate(&self, ids: &[EntryId], cancel: &CancelToken) {
        let items: Vec<(EntryId, PathBuf)> = {
            let results = self.results.lock();
            ids.iter()
                .filter_map(|id| results.get(*id).map(|p| (*id, p.to_path_buf())))
                .collect()
        };
        let gone = skwir_core::fs::hydrate_paths(&items, &self.ctx.metadata, &self.tx, cancel);
        if !gone.is_empty() {
            let mut results = self.results.lock();
            for id in &gone {
                results.remove(*id);
            }
        }
    }
}

impl OpenLocation for QueryLocation {
    fn location(&self) -> &Location {
        &self.location
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }

    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        if ids.is_empty() {
            return;
        }
        // A job for each slice, side by side on the workers.
        for ids in ids.chunks(skwir_core::fs::HYDRATE_JOB) {
            let s = Arc::clone(&self.shared);
            let ids = ids.to_vec();
            let cancel = s.lifetime.child();
            self.shared.ctx.scheduler.spawn_in(
                self.shared.lane(),
                priority,
                "query.stat",
                cancel.clone(),
                move || s.hydrate(&ids, &cancel),
            );
        }
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.shared.results.lock().get(id).map(|p| p.to_path_buf())
    }

    fn refresh(&self) {
        Shared::rewalk(&self.shared, &self.location);
    }

    fn close(&self) {
        self.shared.walk.lock().cancel();
    }
}

impl Drop for QueryLocation {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(root: &Path) -> (Arc<QueryLocation>, Location) {
        open_with(root, "ext:txt", Arc::default())
    }

    fn open_with(
        root: &Path,
        expr: &str,
        metadata: Arc<skwir_core::metadata::MetadataProviders>,
    ) -> (Arc<QueryLocation>, Location) {
        let location = crate::location_for(expr, Some(root));
        let ctx = OpenContext {
            scheduler: skwir_core::scheduler::Scheduler::new(2),
            cancel: CancelToken::new(),
            batch_size: 64,
            waker: None,
            metadata,
            fields: Arc::default(),
            hints: Arc::default(),
            companions: Arc::default(),
        };
        let query = lang::parse(&location.rest).unwrap();
        let open = QueryLocation::open(location.clone(), query, &ctx, &CancelToken::new());
        (open, location)
    }

    /// The events until the listing is complete, as (added, removed).
    fn until_complete(open: &QueryLocation) -> (usize, usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut added, mut removed) = (0, 0);
        loop {
            match open.rx.recv_deadline(deadline).expect("the walk completes") {
                ListingEvent::Added(batch) => added += batch.len(),
                ListingEvent::Removed(ids) => removed += ids.len(),
                ListingEvent::Complete => return (added, removed),
                ListingEvent::Error(e) => panic!("{e}"),
                ListingEvent::Updated(_)
                | ListingEvent::Renamed(_)
                | ListingEvent::Changed
                | ListingEvent::AsOf(_)
                | ListingEvent::Redirect(_)
                | ListingEvent::Moved(_) => {}
            }
        }
    }

    /// Stand in for a root whose opened device identified a remote alias.
    /// With two workers only one job may enter this lane; the sender keeps
    /// that job blocked, and dropping it also releases a failing test.
    fn block_remote(open: &QueryLocation) -> crossbeam_channel::Sender<()> {
        let scheduler = &open.shared.ctx.scheduler;
        assert!(scheduler.wait_idle(Duration::from_secs(10)));
        let lane = Lane::keyed(1);
        *open.shared.lane.lock() = lane;
        let (started, running) = crossbeam_channel::bounded(1);
        let (release, waiting) = crossbeam_channel::bounded(1);
        scheduler.spawn_in(
            lane,
            Priority::Normal,
            "test.remote",
            CancelToken::new(),
            move || {
                started.send(()).unwrap();
                let _ = waiting.recv();
            },
        );
        running.recv_timeout(Duration::from_secs(10)).unwrap();
        release
    }

    fn local_work_completes(open: &QueryLocation) {
        let scheduler = &open.shared.ctx.scheduler;
        let (sent, received) = crossbeam_channel::bounded(1);
        scheduler.spawn(
            Priority::Normal,
            "test.local",
            CancelToken::new(),
            move || {
                sent.send(()).unwrap();
            },
        );
        received.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(scheduler.wait_idle_here(Duration::from_secs(10)));
    }

    /// A remote's tree is walked in its lane, on the job's own thread: it
    /// finds what the walkers' threads find of a tree on this machine,
    /// below its folders, its hidden files left out and none counted twice.
    #[test]
    fn a_remote_tree_walked_in_its_lane_finds_what_a_local_walk_finds() {
        let tmp = tempfile::tempdir().unwrap();
        for folder in ["one", "one/two", "it's three", ".hidden"] {
            std::fs::create_dir(tmp.path().join(folder)).unwrap();
        }
        for file in [
            "a.txt",
            "one/b.txt",
            "one/two/c.txt",
            "it's three/d é.txt",
            ".hidden/e.txt",
            "one/f.md",
        ] {
            std::fs::write(tmp.path().join(file), "x").unwrap();
        }
        let (local, _) = open(tmp.path());
        assert_eq!(until_complete(&local), (4, 0));
        let (remote, _) = open(tmp.path());
        // Its first walk was in the shared lane: walk again in a remote's.
        assert_eq!(until_complete(&remote), (4, 0));
        assert!(
            remote
                .shared
                .ctx
                .scheduler
                .wait_idle(Duration::from_secs(10))
        );
        *remote.shared.lane.lock() = Lane::keyed(1);
        remote.refresh();
        assert_eq!(until_complete(&remote), (4, 4));
        let mut found: Vec<PathBuf> = remote
            .shared
            .results
            .lock()
            .by_id
            .values()
            .map(|p| p.to_path_buf())
            .collect();
        found.sort();
        let mut expected: Vec<PathBuf> = local
            .shared
            .results
            .lock()
            .by_id
            .values()
            .map(|p| p.to_path_buf())
            .collect();
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn remote_refresh_waits_in_its_lane_and_superseded_walk_is_cancelled() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let (open, _) = open(tmp.path());
        assert_eq!(until_complete(&open), (1, 0));
        let release = block_remote(&open);
        open.refresh();
        open.refresh();
        local_work_completes(&open);
        assert!(matches!(open.rx.try_recv(), Ok(ListingEvent::Removed(ids)) if ids.len() == 1));
        assert!(open.rx.is_empty(), "a remote walk must wait for its lane");

        release.send(()).unwrap();
        assert_eq!(until_complete(&open), (1, 0));
        assert!(open.shared.ctx.scheduler.wait_idle(Duration::from_secs(10)));
        assert!(open.rx.is_empty(), "only the current walk publishes");
    }

    #[test]
    fn remote_hydration_and_rechecks_wait_in_the_roots_lane() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "x").unwrap();
        let (open, location) = open(tmp.path());
        assert_eq!(until_complete(&open), (1, 0));
        let id = *open.shared.results.lock().by_id.keys().next().unwrap();
        let release = block_remote(&open);
        open.hydrate(&[id], Priority::Visible);
        let folder = tmp.path().join("arrived");
        std::fs::create_dir(&folder).unwrap();
        let arrived = folder.join("b.txt");
        std::fs::write(&arrived, "y").unwrap();
        let searches = Searches::default();
        searches
            .0
            .lock()
            .push((location, Arc::downgrade(&open.shared)));
        searches.changed(&[Change::Restore { path: folder }]);
        local_work_completes(&open);
        assert!(open.rx.is_empty(), "remote reads must wait for their lane");

        release.send(()).unwrap();
        assert!(open.shared.ctx.scheduler.wait_idle(Duration::from_secs(10)));
        let mut updated = false;
        let mut added = false;
        for event in open.rx.try_iter() {
            match event {
                ListingEvent::Updated(entries) => {
                    updated |= entries.iter().any(|entry| entry.id == id);
                }
                ListingEvent::Added(entries) => {
                    added |= entries
                        .iter()
                        .any(|entry| open.path_of(entry.id) == Some(arrived.clone()));
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert!(updated, "hydration resumes when the lane is free");
        assert!(added, "the operation's new result is rechecked");
    }

    /// A folder of its own for a test that writes files, on the home
    /// filesystem as the user's are.
    fn home_tempdir() -> tempfile::TempDir {
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::tempdir_in(base).unwrap()
    }

    fn count(n: &std::sync::atomic::AtomicUsize) -> usize {
        n.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// A plan elsewhere checks nothing again; one under the root checks
    /// again the results at or below the paths it says, not every result.
    #[test]
    fn a_plan_checks_again_only_the_results_it_touched() {
        const MANY: usize = 2_000;
        let tmp = home_tempdir();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("many")).unwrap();
        std::fs::create_dir_all(root.join("one")).unwrap();
        for i in 0..MANY {
            std::fs::write(root.join("many").join(format!("f{i}.txt")), "x").unwrap();
        }
        std::fs::write(root.join("one/a.txt"), "x").unwrap();
        let elsewhere = tmp.path().join("elsewhere");
        let (open, location) = open(&root);
        assert_eq!(until_complete(&open), (MANY + 1, 0));
        let searches = Searches::default();
        searches
            .0
            .lock()
            .push((location, Arc::downgrade(&open.shared)));
        let idle = || assert!(open.shared.ctx.scheduler.wait_idle(Duration::from_secs(10)));
        for i in 0..100 {
            let path = elsewhere.join(format!("x{i}.txt"));
            searches.changed(&[Change::Trash { path }]);
        }
        idle();
        assert_eq!(
            count(&open.shared.counted.rechecks),
            0,
            "nothing under the root"
        );

        let (from, to) = (root.join("one/a.txt"), root.join("one/b.txt"));
        std::fs::rename(&from, &to).unwrap();
        searches.changed(&[Change::Rename {
            from,
            to: to.clone(),
        }]);
        idle();
        assert_eq!(count(&open.shared.counted.rechecks), 1);
        assert_eq!(
            count(&open.shared.counted.considered),
            1,
            "the file renamed"
        );
        let renamed = open.rx.try_iter().any(|event| {
            matches!(event, ListingEvent::Renamed(pairs)
                if pairs.iter().any(|(_, entry)| open.path_of(entry.id) == Some(to.clone())))
        });
        assert!(renamed, "the result follows its file");

        // A folder taken away takes its results along, those alone looked at.
        searches.changed(&[Change::Trash {
            path: root.join("many"),
        }]);
        idle();
        assert_eq!(count(&open.shared.counted.considered), 1 + MANY);
        let removed: usize = open
            .rx
            .try_iter()
            .map(|event| match event {
                ListingEvent::Removed(ids) => ids.len(),
                _ => 0,
            })
            .sum();
        assert_eq!(removed, MANY);
        assert_eq!(open.shared.results.lock().by_id.len(), 1);
        assert_eq!(open.shared.results.lock().by_path.len(), 1);
    }

    /// A store that looks for files beside a file has the folder's names,
    /// read once for its groups; when none looks, the folder is not read
    /// again.
    #[test]
    fn a_folders_names_are_read_once_and_only_when_a_store_asks() {
        use skwir_core::entry::{Provenance, Value, keys};
        use skwir_core::metadata::{Listed, MetadataProvider};
        /// Tags every `*.tagged` file; asks for each file's sidecar when
        /// `asks`, as an XMP store does.
        struct Sidecars {
            asks: bool,
        }
        impl MetadataProvider for Sidecars {
            fn id(&self) -> &'static str {
                "sidecars"
            }
            fn fill(&self, _: &Path, entries: &mut [Entry]) {
                for e in entries {
                    if e.name_str().ends_with(".tagged") {
                        let tags = Value::List(vec![Value::Text("Work".into())]);
                        e.metadata.set(keys::TAGS, tags, Provenance::User);
                    }
                }
            }
            fn fill_listed(&self, dir: &Path, listed: &Listed<'_>, entries: &mut [Entry]) {
                if self.asks {
                    for e in entries.iter() {
                        let mut sidecar = e.name.clone();
                        sidecar.push(".xmp");
                        listed(&sidecar);
                    }
                }
                self.fill(dir, entries);
            }
        }
        let tmp = home_tempdir();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("big")).unwrap();
        for i in 0..300 {
            let ext = if i % 10 == 0 { "tagged" } else { "plain" };
            std::fs::write(root.join("big").join(format!("f{i}.{ext}")), "x").unwrap();
        }
        for i in 0..20 {
            let small = root.join(format!("small{i}"));
            std::fs::create_dir(&small).unwrap();
            std::fs::write(small.join("a.tagged"), "x").unwrap();
            std::fs::write(small.join("b.plain"), "x").unwrap();
        }
        for asks in [false, true] {
            let metadata = Arc::new(skwir_core::metadata::MetadataProviders::default());
            metadata.register("sidecars", Arc::new(Sidecars { asks }));
            let (open, _) = open_with(&root, "tag:work", metadata);
            assert_eq!(until_complete(&open).0, 30 + 20);
            let read = count(&open.shared.counted.names_read);
            if asks {
                // The root (its folders are candidates too), `big` and the
                // small ones: each once, though the walk's threads decide
                // groups of one folder at once.
                assert_eq!(read, 22, "folders read");
            } else {
                assert_eq!(read, 0, "no store asked");
            }
        }
    }

    /// A folder's names stay while its groups keep coming, others decided
    /// meanwhile, and go once unused a moment; past the bound, the least
    /// recently used go first, never the one just used.
    #[test]
    fn a_folders_names_stay_while_its_groups_come() {
        let set = |n: usize| -> Arc<HashSet<OsString>> {
            Arc::new((0..n).map(|i| OsString::from(format!("f{i}"))).collect())
        };
        // A folder read: its slot made and filled, as `names_of` does.
        let keep = |named: &mut Named, dir: &Path, n: usize, at: Instant| {
            let slot = named.slot(dir, at);
            assert!(slot.set(set(n)).is_ok(), "a slot of its own");
            named.read(dir, &slot, n, at);
        };
        // A folder used again: whether its names were kept.
        let used = |named: &mut Named, dir: &Path, at: Instant| {
            let kept = named
                .folders
                .get(dir)
                .is_some_and(|kept| kept.slot.get().is_some());
            named.slot(dir, at);
            kept
        };
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut named = Named::default();
        let big = Path::new("/synthetic/big");
        keep(&mut named, big, 1_000, t0);
        for i in 0..100 {
            let small = format!("/synthetic/small{i}");
            keep(&mut named, Path::new(&small), 3, ms(i));
            assert!(used(&mut named, big, ms(i)), "kept while its groups come");
        }
        assert!(used(&mut named, big, ms(400)));
        let late = Path::new("/synthetic/late");
        keep(&mut named, late, 1, ms(700));
        let mut kept: Vec<&Path> = named.folders.keys().map(PathBuf::as_path).collect();
        kept.sort();
        assert_eq!(kept, [big, late], "the others unused a moment");
        keep(&mut named, Path::new("/synthetic/later"), 1, ms(1_000));
        assert!(!named.folders.contains_key(big), "unused since 400 ms");
        assert_eq!(named.count, 2);

        // Asked again while it is read, a folder is read once.
        let mut named = Named::default();
        let (first, again) = (named.slot(big, t0), named.slot(big, t0));
        assert!(Arc::ptr_eq(&first, &again), "one read for both");

        let mut named = Named::default();
        keep(&mut named, Path::new("/a"), Named::AT_MOST / 2, t0);
        keep(&mut named, Path::new("/b"), Named::AT_MOST / 2, t0);
        keep(&mut named, Path::new("/c"), 10, t0);
        let mut kept: Vec<&Path> = named.folders.keys().map(PathBuf::as_path).collect();
        kept.sort();
        assert_eq!(kept, [Path::new("/b"), Path::new("/c")], "the oldest first");
        keep(&mut named, Path::new("/huge"), Named::AT_MOST + 1, t0);
        let kept: Vec<&Path> = named.folders.keys().map(PathBuf::as_path).collect();
        assert_eq!(kept, [Path::new("/huge")], "the one just used stays");
        assert_eq!(named.count, Named::AT_MOST + 1);
    }

    /// A search that finds nothing says what it looked for, not where:
    /// the folder is its pill.
    #[test]
    fn a_search_that_finds_nothing_says_what_it_looked_for() {
        let provider = QueryProvider::new(CancelToken::new(), Searches::default(), Weak::new());
        let location = crate::location_for("it's a draft", Some(Path::new("/x/a folder")));
        let empty = provider.empty_state(&location).unwrap();
        assert_eq!(empty.title, "Nothing found");
        assert!(
            empty
                .detail
                .starts_with("No file matches “it's a draft” here."),
            "{}",
            empty.detail
        );
        assert_eq!(provider.icon(&location), Some("search"));
    }

    /// A search whose period moves on (`today` at midnight) lists again
    /// then, by itself; one whose walk ended first does not.
    #[test]
    fn a_search_lists_again_when_its_period_moves_on() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["a.txt", "b.txt", "c.png"] {
            std::fs::write(tmp.path().join(name), "x").unwrap();
        }
        let (open, location) = open(tmp.path());
        assert_eq!(until_complete(&open), (2, 0));
        let soon = SystemTime::now() + Duration::from_millis(50);
        let walk = open.shared.walk.lock().clone();
        Shared::roll_over_at(&open.shared, &location, soon, walk);
        assert_eq!(until_complete(&open), (2, 2), "removed, then found again");

        // A refresh ends the walk the rollover belonged to: it never fires.
        // Due ahead still, whatever the first wait took: one already due
        // would list again before the refresh began.
        let later = SystemTime::now() + Duration::from_millis(150);
        let walk = open.shared.walk.lock().clone();
        Shared::roll_over_at(&open.shared, &location, later, walk);
        open.refresh();
        assert_eq!(until_complete(&open), (2, 2));
        let quiet = open.rx.recv_timeout(Duration::from_millis(400));
        assert!(quiet.is_err(), "nothing after: {quiet:?}");
    }
}
