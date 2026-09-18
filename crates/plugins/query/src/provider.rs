//! The `query` location: a parallel walk of the root that streams matches
//! as a virtual listing. Entries are stat-ed only when the query needs it
//! (and always once for their identity when they match).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bibli_core::cancel::CancelToken;
use bibli_core::entry::{Entry, EntryId, EntryKind};
use bibli_core::fs::stat::stat_path;
use bibli_core::location::{
    EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
};
use bibli_core::scheduler::Priority;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;

use crate::lang::{self, Candidate, Query, Verdict};

pub const SCHEME: &str = "query";

/// Provider for `query:<expression>` locations.
#[derive(Debug, Default)]
pub struct QueryProvider;

impl LocationProvider for QueryProvider {
    fn scheme(&self) -> &str {
        SCHEME
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let query = lang::parse(&location.rest).map_err(|e| anyhow::anyhow!("query: {e}"))?;
        anyhow::ensure!(
            query.root.is_dir(),
            "query root {} is not a directory",
            query.root.display()
        );
        Ok(QueryLocation::open(location.clone(), query, &ctx))
    }
}

struct Shared {
    query: Query,
    ctx: OpenContext,
    tx: EventSink,
    paths: Mutex<HashMap<EntryId, PathBuf>>,
    /// Cancelled to abort the current walk (refresh, close).
    walk: Mutex<CancelToken>,
}

pub struct QueryLocation {
    location: Location,
    shared: Arc<Shared>,
    rx: Receiver<ListingEvent>,
}

/// Batches results and flushes by size or age so the first ones show early.
struct Batcher<'a> {
    tx: &'a EventSink,
    paths: &'a Mutex<HashMap<EntryId, PathBuf>>,
    pending: Mutex<(Vec<Entry>, Instant)>,
    batch_size: usize,
}

impl Batcher<'_> {
    const MAX_AGE: Duration = Duration::from_millis(100);

    fn push(&self, entry: Entry, path: PathBuf) {
        self.paths.lock().insert(entry.id, path);
        let flush = {
            let mut p = self.pending.lock();
            p.0.push(entry);
            if p.0.len() >= self.batch_size || p.1.elapsed() >= Self::MAX_AGE {
                p.1 = Instant::now();
                Some(std::mem::take(&mut p.0))
            } else {
                None
            }
        };
        if let Some(batch) = flush {
            self.tx.send(ListingEvent::Added(batch));
        }
    }

    fn finish(&self) {
        let batch = std::mem::take(&mut self.pending.lock().0);
        if !batch.is_empty() {
            self.tx.send(ListingEvent::Added(batch));
        }
    }
}

fn kind_of(ft: Option<std::fs::FileType>) -> EntryKind {
    match ft {
        Some(t) if t.is_dir() => EntryKind::Directory,
        Some(t) if t.is_file() => EntryKind::File,
        Some(t) if t.is_symlink() => EntryKind::Symlink,
        Some(_) => EntryKind::Other,
        None => EntryKind::Unknown,
    }
}

impl QueryLocation {
    fn open(location: Location, query: Query, ctx: &OpenContext) -> Arc<Self> {
        let (tx, rx) = ctx.channel();
        let shared = Arc::new(Shared {
            query,
            ctx: ctx.clone(),
            tx,
            paths: Mutex::new(HashMap::new()),
            walk: Mutex::new(ctx.cancel.child()),
        });
        let this = Arc::new(QueryLocation {
            location,
            shared,
            rx,
        });
        this.start();
        this
    }

    fn start(&self) {
        let s = Arc::clone(&self.shared);
        let cancel = s.walk.lock().clone();
        self.shared.ctx.scheduler.spawn(
            Priority::Normal,
            "query.walk",
            cancel.clone(),
            move || s.run(&cancel),
        );
    }
}

impl Shared {
    /// Evaluate one path. Returns the entry to emit when it matches.
    fn consider(&self, path: &Path, name: &std::ffi::OsStr, kind: EntryKind) -> Option<Entry> {
        let q = &self.query;
        let Some(expr) = &q.expr else {
            return stat_path(path);
        };
        // 1. Cheap terms from the dirent.
        let c = Candidate {
            name,
            kind,
            path,
            metadata: None,
        };
        match expr.eval(&c, false) {
            Verdict::No => return None,
            Verdict::Yes => return stat_path(path),
            Verdict::Unknown => {}
        }
        // 2. Stat and metadata.
        let mut entry = stat_path(path)?;
        if q.needs_metadata()
            && let Some(parent) = path.parent()
        {
            self.ctx
                .metadata
                .fill(parent, std::slice::from_mut(&mut entry));
        }
        let c = Candidate {
            name,
            kind: entry.kind,
            path,
            metadata: Some(&entry.metadata),
        };
        match expr.eval(&c, false) {
            Verdict::No => return None,
            Verdict::Yes => return Some(entry),
            Verdict::Unknown => {}
        }
        // 3. Content, last.
        match expr.eval(&c, true) {
            Verdict::Yes => Some(entry),
            _ => None,
        }
    }

    fn run(&self, cancel: &CancelToken) {
        let batcher = Batcher {
            tx: &self.tx,
            paths: &self.paths,
            pending: Mutex::new((Vec::new(), Instant::now())),
            batch_size: self.ctx.batch_size.max(1),
        };
        if let Some(paths) = crate::plocate::try_locate(&self.query) {
            for p in paths {
                if cancel.is_cancelled() {
                    return;
                }
                let Some(name) = p.file_name() else { continue };
                let kind = kind_of(std::fs::symlink_metadata(&p).ok().map(|m| m.file_type()));
                if let Some(e) = self.consider(&p, name, kind) {
                    batcher.push(e, p);
                }
            }
        } else {
            let q = &self.query;
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
                Box::new(|result| {
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
                    if let Some(e) = self.consider(path, name, kind) {
                        batcher.push(e, path.to_path_buf());
                    }
                    ignore::WalkState::Continue
                })
            });
        }
        if cancel.is_cancelled() {
            return;
        }
        batcher.finish();
        self.tx.send(ListingEvent::Complete);
    }

    fn hydrate(&self, ids: &[EntryId]) {
        let mut updated = Vec::new();
        let mut gone = Vec::new();
        let mut by_dir: HashMap<PathBuf, Vec<Entry>> = HashMap::new();
        for &id in ids {
            let Some(path) = self.paths.lock().get(&id).cloned() else {
                continue;
            };
            match stat_path(&path) {
                Some(mut e) => {
                    e.id = id;
                    by_dir
                        .entry(path.parent().map(Path::to_path_buf).unwrap_or_default())
                        .or_default()
                        .push(e);
                }
                None => gone.push(id),
            }
        }
        for (dir, mut entries) in by_dir {
            self.ctx.metadata.fill(&dir, &mut entries);
            updated.extend(entries);
        }
        if !updated.is_empty() {
            self.tx.send(ListingEvent::Updated(updated));
        }
        if !gone.is_empty() {
            let mut paths = self.paths.lock();
            for id in &gone {
                paths.remove(id);
            }
            drop(paths);
            self.tx.send(ListingEvent::Removed(gone));
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
        self.shared
            .ctx
            .scheduler
            .spawn(priority, "query.stat", s.ctx.cancel.child(), move || {
                s.hydrate(&ids);
            });
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.shared.paths.lock().get(&id).cloned()
    }

    fn refresh(&self) {
        let old: Vec<EntryId> = {
            let mut walk = self.shared.walk.lock();
            walk.cancel();
            *walk = self.shared.ctx.cancel.child();
            self.shared.paths.lock().drain().map(|(id, _)| id).collect()
        };
        if !old.is_empty() {
            self.shared.tx.send(ListingEvent::Removed(old));
        }
        self.start();
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
