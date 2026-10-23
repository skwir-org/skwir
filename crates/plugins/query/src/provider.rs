//! The `query` location: a parallel walk of the root that streams matches
//! as a virtual listing. Entries are stat-ed only when the query needs it
//! (and always once for their identity when they match).

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
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

    /// What the search looks for and where, as a saved one is named.
    fn title(&self, location: &Location) -> Option<String> {
        Some(crate::suggested_name(
            self.app.upgrade().as_deref(),
            &location.rest,
        ))
    }

    /// A search is in the folder it looks in: Up goes there.
    fn trail(&self, location: &Location) -> Option<skwir_core::location::Trail> {
        let query = lang::parse(&location.rest).ok()?;
        Some(skwir_core::location::Trail {
            base: Some(query.root),
            crumbs: vec![skwir_core::location::Crumb::new(
                crate::suggested_name(self.app.upgrade().as_deref(), &location.rest),
                location.clone(),
            )],
            filters: Vec::new(),
            toggles: Vec::new(),
            holds_places: false,
            one_folder: false,
        })
    }

    fn searches(&self, _location: &Location) -> bool {
        true
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
    paths: Mutex<HashMap<EntryId, PathBuf>>,
    /// The current walk's token. Results are published while holding it, so
    /// once a refresh has swapped it no batch of the old walk can land.
    walk: Mutex<CancelToken>,
    /// The pane's and the plugin's lifetimes together.
    lifetime: CancelToken,
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
    /// The names of the folders whose groups were decided last
    /// (`Deferred::names_of`): read once for every batch of a folder and
    /// handed to every provider (`fill_listed`), so the stores that look
    /// for files beside a file (an XMP sidecar, a Mac's `._` file, a
    /// `TagSpaces` folder) do not each read the folder, and stat it again
    /// for each batch.
    names: Mutex<std::collections::VecDeque<(PathBuf, Arc<HashSet<OsString>>)>>,
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

    /// Folders whose names are kept at once: the walk's threads decide
    /// the groups of a few folders at a time.
    const NAMED: usize = 32;

    /// Every name in `dir`, hidden ones included, read once for the
    /// batches of its group. On a walk's thread.
    fn names_of(&self, dir: &Path) -> Arc<HashSet<OsString>> {
        if let Some((_, names)) = self.names.lock().iter().find(|(d, _)| d == dir) {
            return Arc::clone(names);
        }
        let names: Arc<HashSet<OsString>> = Arc::new(
            std::fs::read_dir(dir)
                .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
                .unwrap_or_default(),
        );
        let mut kept = self.names.lock();
        if kept.len() >= Self::NAMED {
            kept.pop_front();
        }
        kept.push_back((dir.to_path_buf(), Arc::clone(&names)));
        names
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
        let names = self.names_of(dir);
        let listed = |name: &OsStr| names.contains(name);
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
            paths: Mutex::new(HashMap::new()),
            walk: Mutex::new(lifetime.child()),
            lifetime,
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
                        "cannot search {}: {error}",
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
            this.paths.lock().drain().map(|(id, _)| id).collect()
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
fn full_match(q: &Query, entry: &Entry, path: &Path, late: Late<'_>) -> bool {
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
            let mut paths = self.paths.lock();
            let entries = batch
                .into_iter()
                .map(|(entry, path)| {
                    paths.insert(entry.id, path);
                    entry
                })
                .collect();
            drop(paths);
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
            let mut builder = ignore::WalkBuilder::new(&q.root);
            builder
                .hidden(!q.hidden)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .ignore(false)
                .parents(false)
                .follow_links(false)
                .max_depth(q.depth)
                // A network mount below the folder searched is not entered:
                // a search of one walks it from its own folder (ADR-077).
                .filter_entry(|dent| {
                    dent.depth() == 0 || !skwir_core::fs::reach::is_point(dent.path())
                });
            let (hinted, deferred) = (&hinted, &deferred);
            builder.build_parallel().run(|| {
                // The walking thread's stats, through the folder it is in.
                let mut statter = Statter::default();
                let batcher = &batcher;
                Box::new(move |result| {
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
                    if hinted.contains(path) {
                        return ignore::WalkState::Continue;
                    }
                    let known = Known::Kind(kind_of(dent.file_type()));
                    consider(q, path, (name, known), batcher, deferred, &mut statter);
                    ignore::WalkState::Continue
                })
            });
        }
        deferred.finish();
        batcher.finish();
    }

    /// Consider the files the indexes name for the query's tag (ADR-026),
    /// as the walk would have (hidden files and depth as it would, never
    /// through a link), and return them for the walk to skip.
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
        for real in paths {
            if cancel.is_cancelled() {
                break;
            }
            let Ok(relative) = real.strip_prefix(&real_root) else {
                continue;
            };
            if !crate::plocate::visible(relative, q.hidden, q.depth) {
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
            if through_link {
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
        // Followed outside the lock, which `path_of` waits for under the
        // workspace's.
        let results: Vec<(EntryId, PathBuf)> = self
            .paths
            .lock()
            .iter()
            .map(|(id, path)| (*id, path.clone()))
            .collect();
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
            let mut paths = self.paths.lock();
            for id in &removed {
                paths.remove(id);
            }
            drop(paths);
            self.tx.send(ListingEvent::Removed(removed));
        }
        if !renamed.is_empty() {
            let _walk = self.walk.lock();
            if cancel.is_cancelled() {
                return;
            }
            let mut paths = self.paths.lock();
            let pairs = renamed
                .into_iter()
                .map(|(was, entry, path)| {
                    paths.remove(&was);
                    paths.insert(entry.id, path);
                    (was, entry)
                })
                .collect();
            drop(paths);
            self.tx.send(ListingEvent::Renamed(pairs));
        }
        self.publish(cancel, found, false);
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
        let relative = path.strip_prefix(&q.root).ok()?;
        if relative.as_os_str().is_empty() || !crate::plocate::visible(relative, q.hidden, q.depth)
        {
            return None;
        }
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
            let paths = self.paths.lock();
            ids.iter()
                .filter_map(|id| paths.get(id).map(|p| (*id, p.clone())))
                .collect()
        };
        let gone = skwir_core::fs::hydrate_paths(&items, &self.ctx.metadata, &self.tx, cancel);
        if !gone.is_empty() {
            let mut paths = self.paths.lock();
            for id in &gone {
                paths.remove(id);
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
        self.shared.paths.lock().get(&id).cloned()
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
        let location = crate::location_for("ext:txt", Some(root));
        let ctx = OpenContext {
            scheduler: skwir_core::scheduler::Scheduler::new(2),
            cancel: CancelToken::new(),
            batch_size: 64,
            waker: None,
            metadata: Arc::default(),
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
                | ListingEvent::Redirect(_) => {}
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
        let id = *open.shared.paths.lock().keys().next().unwrap();
        let release = block_remote(&open);
        open.hydrate(&[id], Priority::Visible);
        let arrived = tmp.path().join("b.txt");
        std::fs::write(&arrived, "y").unwrap();
        let searches = Searches::default();
        searches
            .0
            .lock()
            .push((location, Arc::downgrade(&open.shared)));
        searches.changed(&[Change::Restore {
            path: arrived.clone(),
        }]);
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
