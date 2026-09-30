//! The `query` location: a parallel walk of the root that streams matches
//! as a virtual listing. Entries are stat-ed only when the query needs it
//! (and always once for their identity when they match).

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use bibli_core::cancel::CancelToken;
use bibli_core::entry::{Entry, EntryId, EntryKind};
use bibli_core::fs::stat::stat_path;
use bibli_core::location::{
    EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
};
use bibli_core::scheduler::Priority;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;

use crate::lang::{self, Candidate, Late, Query, Verdict};

pub const SCHEME: &str = "query";

/// Provider for `query:<expression>` locations. Every walk and stat it
/// starts ends when the plugin is disabled (`lifetime`), not only when its
/// pane moves on.
#[derive(Debug)]
pub struct QueryProvider {
    lifetime: CancelToken,
}

impl QueryProvider {
    pub fn new(lifetime: CancelToken) -> Self {
        QueryProvider { lifetime }
    }
}

impl LocationProvider for QueryProvider {
    fn scheme(&self) -> &str {
        SCHEME
    }

    /// What the search looks for and where, as a saved one is named.
    fn title(&self, location: &Location) -> Option<String> {
        Some(crate::suggested_name(&location.rest))
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
        Ok(QueryLocation::open(
            location.clone(),
            query,
            &ctx,
            &self.lifetime,
        ))
    }
}

struct Shared {
    ctx: OpenContext,
    tx: EventSink,
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

    fn finish(&self) {
        let batch = std::mem::take(&mut self.pending.lock().0);
        self.shared.publish(self.cancel, batch, true);
    }
}

/// Matches that wait for the metadata providers (tag terms), grouped by
/// folder so each provider gets a batch, not a call per file. One per walker
/// thread; what is left is decided when the thread ends (on drop).
struct Deferred<'a> {
    shared: &'a Shared,
    query: &'a Query,
    batcher: &'a Batcher<'a>,
    by_dir: HashMap<PathBuf, Vec<Entry>>,
}

impl Deferred<'_> {
    const BATCH: usize = 64;

    fn add(&mut self, dir: &Path, entry: Entry) {
        let group = self.by_dir.entry(dir.to_path_buf()).or_default();
        group.push(entry);
        if group.len() >= Self::BATCH {
            let entries = std::mem::take(group);
            self.decide(dir, entries);
        }
    }

    fn decide(&self, dir: &Path, mut entries: Vec<Entry>) {
        if self.batcher.cancel.is_cancelled() {
            return;
        }
        self.shared.ctx.metadata.fill(dir, &mut entries);
        for entry in entries {
            let path = dir.join(&entry.name);
            if full_match(self.query, &entry, &path, self.batcher.late()) {
                self.batcher.push(entry, path);
            }
        }
    }
}

impl Drop for Deferred<'_> {
    fn drop(&mut self) {
        for (dir, entries) in std::mem::take(&mut self.by_dir) {
            self.decide(&dir, entries);
        }
    }
}

fn kind_of(ft: Option<std::fs::FileType>) -> EntryKind {
    use std::os::unix::fs::FileTypeExt;
    match ft {
        Some(t) if t.is_dir() => EntryKind::Directory,
        Some(t) if t.is_file() => EntryKind::File,
        Some(t) if t.is_symlink() => EntryKind::Symlink,
        Some(t) if t.is_fifo() => EntryKind::Fifo,
        Some(t) if t.is_socket() => EntryKind::Socket,
        Some(t) if t.is_char_device() => EntryKind::CharDevice,
        Some(t) if t.is_block_device() => EntryKind::BlockDevice,
        Some(_) | None => EntryKind::Unknown,
    }
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
    /// Walk for `query`, and list again when its periods move on
    /// (`modified:today` at midnight).
    fn start(this: &Arc<Self>, location: &Location, query: Query) {
        let cancel = this.walk.lock().clone();
        if let Some(when) = query.next_change() {
            Shared::roll_over_at(this, location, when, cancel.clone());
        }
        let s = Arc::clone(this);
        this.ctx
            .scheduler
            .spawn(Priority::Normal, "query.walk", cancel.clone(), move || {
                s.run(&query, &cancel);
            });
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
            .name("bibli-query-rollover".into())
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
fn first(q: &Query, path: &Path, name: &OsStr, kind: EntryKind, late: Late<'_>) -> First {
    let Some(expr) = &q.expr else {
        return stat_path(path).map_or(First::No, First::Yes);
    };
    // 1. Cheap terms from the dirent.
    match expr.eval(&Candidate::new(name, kind, path, None), false) {
        Verdict::No => return First::No,
        Verdict::Yes => return stat_path(path).map_or(First::No, First::Yes),
        Verdict::Unknown => {}
    }
    // 2. Stat; metadata comes later, in batches.
    let Some(entry) = stat_path(path) else {
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
    name: &OsStr,
    kind: EntryKind,
    batcher: &Batcher<'_>,
    deferred: &mut Deferred<'_>,
) {
    match first(q, path, name, kind, batcher.late()) {
        First::No => {}
        First::Yes(entry) => batcher.push(entry, path.to_path_buf()),
        First::NeedsMetadata(entry) => {
            if let Some(dir) = path.parent() {
                deferred.add(dir, entry);
            }
        }
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

    fn run(&self, q: &Query, cancel: &CancelToken) {
        if !q.root.is_dir() {
            let _walk = self.walk.lock();
            if !cancel.is_cancelled() {
                self.tx.send(ListingEvent::Error(format!(
                    "query root {} is not a folder",
                    q.root.display()
                )));
            }
            return;
        }
        let batcher = Batcher {
            shared: self,
            cancel,
            pending: Mutex::new((Vec::new(), Instant::now())),
            batch_size: self.ctx.batch_size.max(1),
        };
        let deferred = || Deferred {
            shared: self,
            query: q,
            batcher: &batcher,
            by_dir: HashMap::new(),
        };
        if let Some(paths) = crate::plocate::try_locate(q) {
            let mut deferred = deferred();
            for p in paths {
                if cancel.is_cancelled() {
                    return;
                }
                let Some(name) = p.file_name() else { continue };
                let kind = kind_of(std::fs::symlink_metadata(&p).ok().map(|m| m.file_type()));
                consider(q, &p, name, kind, &batcher, &mut deferred);
            }
        } else {
            let mut builder = ignore::WalkBuilder::new(&q.root);
            builder
                .hidden(!q.hidden)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .ignore(false)
                .parents(false)
                .follow_links(false)
                .max_depth(q.depth);
            builder.build_parallel().run(|| {
                // Dropped when this walker thread ends, which decides the rest.
                let mut deferred = deferred();
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
                    let kind = kind_of(dent.file_type());
                    consider(q, path, name, kind, batcher, &mut deferred);
                    ignore::WalkState::Continue
                })
            });
        }
        batcher.finish();
    }

    fn hydrate(&self, ids: &[EntryId], cancel: &CancelToken) {
        let items: Vec<(EntryId, PathBuf)> = {
            let paths = self.paths.lock();
            ids.iter()
                .filter_map(|id| paths.get(id).map(|p| (*id, p.clone())))
                .collect()
        };
        let gone = bibli_core::fs::hydrate_paths(&items, &self.ctx.metadata, &self.tx, cancel);
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
        let s = Arc::clone(&self.shared);
        let ids = ids.to_vec();
        let cancel = s.lifetime.child();
        self.shared
            .ctx
            .scheduler
            .spawn(priority, "query.stat", cancel.clone(), move || {
                s.hydrate(&ids, &cancel);
            });
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
            scheduler: bibli_core::scheduler::Scheduler::new(2),
            cancel: CancelToken::new(),
            batch_size: 64,
            waker: None,
            metadata: Arc::default(),
            fields: Arc::default(),
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
                ListingEvent::Updated(_) => {}
            }
        }
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
        let walk = open.shared.walk.lock().clone();
        Shared::roll_over_at(&open.shared, &location, soon, walk);
        open.refresh();
        assert_eq!(until_complete(&open), (2, 2));
        let quiet = open.rx.recv_timeout(Duration::from_millis(300));
        assert!(quiet.is_err(), "nothing after: {quiet:?}");
    }
}
