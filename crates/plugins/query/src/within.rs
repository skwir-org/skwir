//! A search of what a place lists (`in:recent:`, ADR-098): the place is
//! opened through its provider, and each entry it lists is tested as the
//! walk tests a file, what it contains read last, in the lane of the file;
//! nothing below an entry is reached. Matches stream as the place's
//! entries come; the place's own changes follow (an entry gone is gone,
//! one changed is tested again).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::{Entry, EntryId};
use skwir_core::fs::reach;
use skwir_core::location::{EventSink, ListingEvent, Location, OpenContext, OpenLocation};
use skwir_core::scheduler::Priority;

use crate::lang::{Late, Query};
use crate::provider::full_match;

/// The matches shown, with their paths, and each entry's check under way:
/// the latest asked of it, which alone may show it or take it back. An
/// entry gone, or tested again, meanwhile has none, or another.
#[derive(Default)]
struct Shown {
    paths: HashMap<EntryId, PathBuf>,
    checking: HashMap<EntryId, u64>,
    asked: u64,
}

/// A search within a place.
pub struct Within {
    location: Location,
    shared: Arc<Shared>,
    rx: Receiver<ListingEvent>,
}

struct Shared {
    ctx: OpenContext,
    tx: EventSink,
    query: Query,
    /// The place searched: set once it is opened.
    inner: Mutex<Option<Arc<dyn OpenLocation>>>,
    lifetime: CancelToken,
    /// A drain of the place's events is queued.
    draining: AtomicBool,
    /// Checks still running, and whether the place said it was complete.
    pending: AtomicUsize,
    listed: AtomicBool,
    completed: AtomicBool,
    /// The matches shown, and the checks under way.
    shown: Mutex<Shown>,
    /// Entries whose metadata was asked for, to be tested once it comes.
    asked: Mutex<HashSet<EntryId>>,
}

/// Open the search of `query` within `place`, through `app`'s provider of
/// it. A search within a search is refused: its results are a search's.
pub fn open(
    app: &App,
    location: Location,
    query: Query,
    place: &Location,
    ctx: &OpenContext,
    plugin: &CancelToken,
) -> anyhow::Result<Arc<dyn OpenLocation>> {
    anyhow::ensure!(
        &*place.scheme != crate::SCHEME,
        "a search cannot look within another search"
    );
    let provider = app.provider(&place.scheme).ok_or_else(|| {
        anyhow::anyhow!("nothing opens {place}: a folder is written as its whole path")
    })?;
    let (tx, rx) = ctx.channel();
    let lifetime = ctx.cancel.linked(plugin);
    let shared = Arc::new(Shared {
        ctx: ctx.clone(),
        tx,
        query,
        inner: Mutex::new(None),
        lifetime: lifetime.clone(),
        draining: AtomicBool::new(false),
        pending: AtomicUsize::new(0),
        listed: AtomicBool::new(false),
        completed: AtomicBool::new(false),
        shown: Mutex::new(Shown::default()),
        asked: Mutex::new(HashSet::new()),
    });
    // The place's events wake a drain of them on a worker, never the window.
    let waker = {
        let weak = Arc::downgrade(&shared);
        Arc::new(move || {
            if let Some(shared) = weak.upgrade() {
                Shared::wake(&shared);
            }
        }) as skwir_core::location::Waker
    };
    let inner_ctx = OpenContext {
        cancel: lifetime.child(),
        waker: Some(waker),
        ..ctx.clone()
    };
    let inner = provider.open(place, inner_ctx)?;
    *shared.inner.lock() = Some(inner);
    // What the place sent before its waker could reach the search.
    Shared::wake(&shared);
    Ok(Arc::new(Within {
        location,
        shared,
        rx,
    }))
}

impl Shared {
    fn inner(&self) -> Option<Arc<dyn OpenLocation>> {
        self.inner.lock().clone()
    }

    /// Queue a drain of the place's events, unless one is queued.
    fn wake(this: &Arc<Self>) {
        if this.draining.swap(true, Ordering::AcqRel) {
            return;
        }
        let s = Arc::clone(this);
        this.ctx.scheduler.spawn(
            Priority::Normal,
            "query.within",
            this.lifetime.child(),
            move || {
                s.draining.store(false, Ordering::Release);
                Shared::drain(&s);
            },
        );
    }

    /// Take what the place sent: entries to test, entries gone, its end.
    fn drain(this: &Arc<Self>) {
        let Some(inner) = this.inner() else {
            return;
        };
        while let Ok(event) = inner.events().try_recv() {
            if this.lifetime.is_cancelled() {
                return;
            }
            match event {
                ListingEvent::Added(entries) | ListingEvent::Updated(entries) => {
                    Shared::test(this, &inner, entries);
                }
                ListingEvent::Renamed(pairs) => {
                    let (gone, entries): (Vec<EntryId>, Vec<Entry>) = pairs.into_iter().unzip();
                    this.forget(gone);
                    Shared::test(this, &inner, entries);
                }
                ListingEvent::Removed(ids) => this.forget(ids),
                ListingEvent::Complete => {
                    this.listed.store(true, Ordering::Release);
                    this.complete_if_done();
                }
                ListingEvent::Error(error) => {
                    this.tx.send(ListingEvent::Error(error));
                }
                _ => {}
            }
        }
    }

    /// Test `entries` of the place, those whose metadata has come, a job
    /// for each lane; ask for the others' metadata, and test them when it
    /// comes (`Updated`).
    fn test(this: &Arc<Self>, inner: &Arc<dyn OpenLocation>, entries: Vec<Entry>) {
        let mut ready = Vec::new();
        let mut unknown = Vec::new();
        for entry in entries {
            let Some(path) = inner.path_of(entry.id) else {
                continue;
            };
            if entry.metadata.is_hydrated() || entry.is_place() {
                this.asked.lock().remove(&entry.id);
                let check = {
                    let mut shown = this.shown.lock();
                    shown.asked += 1;
                    let check = shown.asked;
                    shown.checking.insert(entry.id, check);
                    check
                };
                ready.push((entry, path, check));
            } else if this.asked.lock().insert(entry.id) {
                unknown.push(entry.id);
            }
        }
        if !unknown.is_empty() {
            inner.hydrate(&unknown, Priority::Normal);
        }
        for (lane, batch) in reach::by_lane(ready, |(_, path, _)| path.as_path()) {
            this.pending.fetch_add(1, Ordering::AcqRel);
            let s = Arc::clone(this);
            let cancel = this.lifetime.child();
            let job_cancel = cancel.clone();
            this.ctx.scheduler.spawn_in(
                lane,
                Priority::Normal,
                "query.within.check",
                cancel,
                move || {
                    s.check(batch, &job_cancel);
                    s.pending.fetch_sub(1, Ordering::AcqRel);
                    s.complete_if_done();
                },
            );
        }
    }

    /// Decide each entry, and show what holds, take back what no longer
    /// does: each by its latest check alone (`Shown::checking`).
    fn check(&self, batch: Vec<(Entry, PathBuf, u64)>, cancel: &CancelToken) {
        let late = Late {
            fields: &self.ctx.fields,
            cancel,
        };
        let mut added = Vec::new();
        let mut updated = Vec::new();
        let mut removed = Vec::new();
        for (entry, path, check) in batch {
            if cancel.is_cancelled() {
                return;
            }
            let holds = full_match(&self.query, &entry, &path, late);
            let mut shown = self.shown.lock();
            if shown.checking.get(&entry.id) != Some(&check) {
                continue;
            }
            shown.checking.remove(&entry.id);
            match (holds, shown.paths.contains_key(&entry.id)) {
                (true, false) => {
                    shown.paths.insert(entry.id, path);
                    added.push(entry);
                }
                (true, true) => updated.push(entry),
                (false, true) => {
                    shown.paths.remove(&entry.id);
                    removed.push(entry.id);
                }
                (false, false) => {}
            }
        }
        if !added.is_empty() {
            self.tx.send(ListingEvent::Added(added));
        }
        if !updated.is_empty() {
            self.tx.send(ListingEvent::Updated(updated));
        }
        if !removed.is_empty() {
            self.tx.send(ListingEvent::Removed(removed));
        }
    }

    /// The place's entries `ids` are gone: so are their matches, and what
    /// their checks under way would say.
    fn forget(&self, ids: Vec<EntryId>) {
        let gone: Vec<EntryId> = {
            let mut shown = self.shown.lock();
            ids.into_iter()
                .filter(|id| {
                    shown.checking.remove(id);
                    shown.paths.remove(id).is_some()
                })
                .collect()
        };
        if !gone.is_empty() {
            self.tx.send(ListingEvent::Removed(gone));
        }
    }

    /// Say the search complete once the place is listed and every check
    /// ran; what changes after still follows.
    fn complete_if_done(&self) {
        if self.listed.load(Ordering::Acquire)
            && self.pending.load(Ordering::Acquire) == 0
            && !self.completed.swap(true, Ordering::AcqRel)
        {
            self.tx.send(ListingEvent::Complete);
        }
    }
}

impl OpenLocation for Within {
    fn location(&self) -> &Location {
        &self.location
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }

    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        // Its matches were tested with their metadata: the place keeps it.
        if let Some(inner) = self.shared.inner() {
            inner.hydrate(ids, priority);
        }
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.shared.shown.lock().paths.get(&id).cloned()
    }

    fn refresh(&self) {
        if let Some(inner) = self.shared.inner() {
            inner.refresh();
        }
    }

    fn close(&self) {
        self.shared.lifetime.cancel();
        if let Some(inner) = self.shared.inner() {
            inner.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use crossbeam_channel::Receiver;
    use skwir_core::app::{App, AppConfig};
    use skwir_core::entry::{Entry, EntryId, EntryKind, FileId, Provenance, Value, keys};
    use skwir_core::location::{
        ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
    };
    use skwir_core::scheduler::Priority;

    use crate::{QueryPlugin, lang};

    /// A place that lists files from anywhere, as Recent does.
    struct Listed(Vec<PathBuf>);

    struct ListedOpen {
        location: Location,
        rx: Receiver<ListingEvent>,
        paths: HashMap<EntryId, PathBuf>,
    }

    impl LocationProvider for Listed {
        fn scheme(&self) -> &'static str {
            "listed"
        }

        fn open(
            &self,
            location: &Location,
            ctx: OpenContext,
        ) -> anyhow::Result<Arc<dyn OpenLocation>> {
            let (tx, rx) = ctx.channel();
            let mut paths = HashMap::new();
            let entries: Vec<Entry> = self
                .0
                .iter()
                .enumerate()
                .map(|(i, path)| {
                    let id = EntryId::at_path(FileId::new(9, i as u64 + 1), path);
                    let mut e = Entry::new(id, path.file_name().unwrap(), EntryKind::File);
                    let size = std::fs::metadata(path).unwrap().len();
                    e.metadata
                        .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
                    paths.insert(id, path.clone());
                    e
                })
                .collect();
            tx.send(ListingEvent::Added(entries));
            tx.send(ListingEvent::Complete);
            Ok(Arc::new(ListedOpen {
                location: location.clone(),
                rx,
                paths,
            }))
        }
    }

    impl OpenLocation for ListedOpen {
        fn location(&self) -> &Location {
            &self.location
        }
        fn events(&self) -> &Receiver<ListingEvent> {
            &self.rx
        }
        fn hydrate(&self, _: &[EntryId], _: Priority) {}
        fn path_of(&self, id: EntryId) -> Option<PathBuf> {
            self.paths.get(&id).cloned()
        }
        fn refresh(&self) {}
        fn close(&self) {}
    }

    fn found(app: &App, expr: &str, dir: &Path) -> BTreeSet<String> {
        let open = app
            .open_location(&Location::new(crate::SCHEME, expr))
            .unwrap_or_else(|e| panic!("{expr}: {e:#}"));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut names = BTreeSet::new();
        loop {
            match open.events().recv_deadline(deadline).expect("completes") {
                ListingEvent::Added(batch) | ListingEvent::Updated(batch) => {
                    for e in batch {
                        let path = open.path_of(e.id).expect("path_of");
                        names.insert(
                            path.strip_prefix(dir)
                                .unwrap()
                                .to_string_lossy()
                                .into_owned(),
                        );
                    }
                }
                ListingEvent::Complete => break,
                ListingEvent::Error(e) => panic!("{expr}: {e}"),
                _ => {}
            }
        }
        names
    }

    #[test]
    fn a_place_is_searched_as_it_lists_contents_and_all() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("files");
        std::fs::create_dir_all(dir.join("deep")).unwrap();
        std::fs::write(dir.join("filter.txt"), "a water filter").unwrap();
        std::fs::write(dir.join("notes.md"), "nothing here").unwrap();
        std::fs::write(dir.join("deep/well.txt"), "water").unwrap();
        // Not listed: never reached, though it holds the words.
        std::fs::write(dir.join("deep/unlisted.txt"), "water").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let listed = ["filter.txt", "notes.md", "deep/well.txt"].map(|n| dir.join(n));
        app.register_provider("test", Arc::new(Listed(listed.to_vec())));

        let set = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<BTreeSet<_>>();
        assert_eq!(
            found(&app, "content:water in:listed:", &dir),
            set(&["deep/well.txt", "filter.txt"])
        );
        assert_eq!(found(&app, "notes in:listed:", &dir), set(&["notes.md"]));
        assert_eq!(found(&app, "ext:md,txt in:listed:", &dir).len(), 3);
        // A search within a search is refused.
        assert!(
            app.open_location(&Location::new(crate::SCHEME, r#"x in:"query:y""#))
                .is_err()
        );
    }

    #[test]
    fn in_names_a_place_or_a_folder() {
        let q = lang::parse("report in:recent:").unwrap();
        assert_eq!(q.within, Some(Location::new("recent", "")));
        // Printed back as it reads, the place kept.
        assert_eq!(q.to_string(), "name:report in:recent:");
        assert_eq!(lang::parse(&q.to_string()).unwrap().within, q.within);
        let q = lang::parse(r#"in:"tags:/Places/France" kind:image"#).unwrap();
        assert_eq!(q.within, Some(Location::new("tags", "/Places/France")));
        let q = lang::parse("in:/data report").unwrap();
        assert!(q.within.is_none() && q.root_given);
        // One letter before a colon is a drive.
        let q = lang::parse(r"in:C:\data").unwrap();
        assert!(q.within.is_none());
    }
}
