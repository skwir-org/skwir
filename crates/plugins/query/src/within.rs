//! A search of what a place lists (`in:recent:`, ADR-098): the place is
//! opened through its provider, and each entry it lists is tested as the
//! walk tests a file, what it contains read last, in the lane of the file;
//! nothing below an entry is reached. An entry listed without its metadata
//! (a tag's place lists its files before stat-ing them) is read by the
//! search itself, as the walk reads a file, never waited for from the
//! place: a place's hydration answers what it can, and one that cannot be
//! read is tested with what the place listed. Matches stream as the
//! place's entries come; the search is complete once the place is and
//! every entry was tested; the place's own changes follow (an entry gone is
//! gone, one changed is tested again).

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
    /// A drain of the place's events is queued or running: one at a time,
    /// so the place's events are taken in the order it sent them.
    draining: AtomicBool,
    /// Checks still running, and whether the place said it was complete.
    pending: AtomicUsize,
    listed: AtomicBool,
    completed: AtomicBool,
    /// The matches shown, and the checks under way.
    shown: Mutex<Shown>,
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

    /// Queue a drain of the place's events, unless one is queued or runs.
    /// The drain lets go only once nothing waits: an event sent as it let
    /// go, whose wake found it still running, is drained by it.
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
                loop {
                    Shared::drain(&s);
                    s.draining.store(false, Ordering::Release);
                    if s.lifetime.is_cancelled() {
                        return;
                    }
                    let waiting = s.inner().is_some_and(|inner| !inner.events().is_empty());
                    if !waiting || s.draining.swap(true, Ordering::AcqRel) {
                        return;
                    }
                }
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

    /// Test `entries` of the place, a job for each slice of each lane,
    /// counted in `pending` before the drain takes the place's next event:
    /// its end is never taken before what it listed is under way.
    fn test(this: &Arc<Self>, inner: &Arc<dyn OpenLocation>, entries: Vec<Entry>) {
        let mut ready = Vec::new();
        for entry in entries {
            let Some(path) = inner.path_of(entry.id) else {
                continue;
            };
            let check = {
                let mut shown = this.shown.lock();
                shown.asked += 1;
                let check = shown.asked;
                shown.checking.insert(entry.id, check);
                check
            };
            ready.push((entry, path, check));
        }
        for (lane, batch) in reach::by_lane(ready, |(_, path, _)| path.as_path()) {
            let mut batch = batch.into_iter().peekable();
            while batch.peek().is_some() {
                let slice: Vec<_> = batch.by_ref().take(skwir_core::fs::HYDRATE_JOB).collect();
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
                        s.check(slice, &job_cancel);
                        s.pending.fetch_sub(1, Ordering::AcqRel);
                        s.complete_if_done();
                    },
                );
            }
        }
    }

    /// Read what the place listed without its metadata, then decide each
    /// entry, and show what holds, take back what no longer does: each by
    /// its latest check alone (`Shown::checking`). An entry gone is no
    /// match; one that cannot be read is tested with what was listed.
    fn check(&self, mut batch: Vec<(Entry, PathBuf, u64)>, cancel: &CancelToken) {
        let unread: Vec<(EntryId, PathBuf)> = batch
            .iter()
            .filter(|(entry, ..)| !entry.metadata.is_hydrated() && !entry.is_place())
            .map(|(entry, path, _)| (entry.id, path.clone()))
            .collect();
        let mut read: HashMap<EntryId, Entry> = HashMap::new();
        let mut gone: HashSet<EntryId> = HashSet::new();
        if !unread.is_empty() {
            let finished =
                skwir_core::fs::read_paths(&unread, &self.ctx.metadata, cancel, |found, went| {
                    read.extend(found.into_iter().map(|e| (e.id, e)));
                    gone.extend(went);
                });
            if !finished {
                return;
            }
            for (entry, ..) in &mut batch {
                // Its identity and name are the place's: the rest is read.
                if let Some(e) = read.remove(&entry.id) {
                    entry.kind = e.kind;
                    entry.target = e.target;
                    entry.hidden = e.hidden;
                    entry.metadata = e.metadata;
                }
            }
        }
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
            let holds = !gone.contains(&entry.id) && full_match(&self.query, &entry, &path, late);
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

    /// Say the search complete once the place is listed and every entry it
    /// listed was tested (read first when it came without its metadata);
    /// what changes after still follows.
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
        // Its matches were tested with their metadata, read with them;
        // asked again (a field read on demand), the place answers.
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
        EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
    };
    use skwir_core::scheduler::Priority;

    use crate::{QueryPlugin, lang};

    /// A place that lists files from anywhere, as Recent does; `late`,
    /// without their metadata, read when it is asked for as a tag's place
    /// reads them (`hydrate_paths`).
    struct Listed {
        paths: Vec<PathBuf>,
        late: bool,
    }

    struct ListedOpen {
        location: Location,
        rx: Receiver<ListingEvent>,
        paths: HashMap<EntryId, PathBuf>,
        /// Where what is read when asked for is sent, and what reads it.
        late: Option<(EventSink, OpenContext)>,
    }

    impl LocationProvider for Listed {
        fn scheme(&self) -> &'static str {
            if self.late { "late" } else { "listed" }
        }

        fn open(
            &self,
            location: &Location,
            ctx: OpenContext,
        ) -> anyhow::Result<Arc<dyn OpenLocation>> {
            let (tx, rx) = ctx.channel();
            let mut paths = HashMap::new();
            let entries: Vec<Entry> = self
                .paths
                .iter()
                .enumerate()
                .map(|(i, path)| {
                    let id = EntryId::at_path(FileId::new(9, i as u64 + 1), path);
                    let mut e = Entry::new(id, path.file_name().unwrap(), EntryKind::File);
                    paths.insert(id, path.clone());
                    if !self.late {
                        let size = std::fs::metadata(path).unwrap().len();
                        e.metadata
                            .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
                    }
                    e
                })
                .collect();
            tx.send(ListingEvent::Added(entries));
            tx.send(ListingEvent::Complete);
            Ok(Arc::new(ListedOpen {
                location: location.clone(),
                rx,
                paths,
                late: self.late.then_some((tx, ctx)),
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
        fn hydrate(&self, ids: &[EntryId], _: Priority) {
            let Some((tx, ctx)) = &self.late else {
                return;
            };
            let items: Vec<(EntryId, PathBuf)> = ids
                .iter()
                .filter_map(|id| Some((*id, self.paths.get(id)?.clone())))
                .collect();
            let (tx, metadata, cancel) =
                (tx.clone(), Arc::clone(&ctx.metadata), ctx.cancel.child());
            std::thread::spawn(move || {
                skwir_core::fs::hydrate_paths(&items, &metadata, &tx, &cancel);
            });
        }
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
        app.register_provider(
            "test",
            Arc::new(Listed {
                paths: listed.to_vec(),
                late: false,
            }),
        );

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

    /// A place whose files come without their metadata (a tag's) is
    /// searched complete once each was tested with what it was asked for,
    /// never before: what is found comes before the end.
    #[test]
    fn a_place_s_files_are_tested_with_their_metadata_before_it_completes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("l'été.jpg"), "x").unwrap();
        std::fs::write(dir.join("notes.md"), "nothing here").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let paths = ["l'été.jpg", "notes.md"].map(|n| dir.join(n)).to_vec();
        app.register_provider("test", Arc::new(Listed { paths, late: true }));

        let set = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<BTreeSet<_>>();
        assert_eq!(found(&app, "été in:late:", &dir), set(&["l'été.jpg"]));
        assert_eq!(found(&app, "size<1k in:late:", &dir).len(), 2);
        assert_eq!(found(&app, "size>1k in:late:", &dir), set(&[]));
    }

    /// An entry whose folder cannot be read (one that may be gone through
    /// but not listed) is tested with what the place listed of it, and the
    /// search completes: a place's hydration does not answer it.
    #[cfg(unix)]
    #[test]
    fn a_place_s_entry_that_cannot_be_read_is_tested_with_what_it_listed() {
        use std::os::unix::fs::PermissionsExt;

        /// The folder made readable again, so the fixture can be removed.
        struct Opened(PathBuf);
        impl Drop for Opened {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
            }
        }

        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let dir = tmp.path().join("files");
        std::fs::create_dir_all(dir.join("open")).unwrap();
        std::fs::create_dir_all(dir.join("shut")).unwrap();
        std::fs::write(dir.join("open/l'été.jpg"), "x").unwrap();
        std::fs::write(dir.join("shut/notes d'été.md"), "nothing").unwrap();
        let shut = Opened(dir.join("shut"));
        std::fs::set_permissions(&shut.0, std::fs::Permissions::from_mode(0o300)).unwrap();
        // Read all the same (as root): the case cannot be made.
        if std::fs::read_dir(&shut.0).is_ok() {
            return;
        }
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let paths = ["open/l'été.jpg", "shut/notes d'été.md"]
            .map(|n| dir.join(n))
            .to_vec();
        app.register_provider("test", Arc::new(Listed { paths, late: true }));

        let set = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<BTreeSet<_>>();
        assert_eq!(
            found(&app, "été in:late:", &dir),
            set(&["open/l'été.jpg", "shut/notes d'été.md"]),
            "its name, as listed"
        );
        assert_eq!(
            found(&app, "size<1k in:late:", &dir),
            set(&["open/l'été.jpg"]),
            "its size is not known"
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
