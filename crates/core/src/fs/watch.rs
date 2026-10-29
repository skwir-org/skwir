//! Directory watching, coalesced into named changes.
//!
//! On Linux this talks to inotify directly: one instance for the process,
//! read by one thread, each folder watched a descriptor of it (`imp`).
//! Adding a non-recursive watch is one syscall with no thread round trip.
//! Registration still belongs on a worker: resolving a watched path can
//! wait for a sleeping drive. Elsewhere the `notify` crate is used.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::cancel::CancelToken;

/// A coalesced change: the names in the watched directory that were touched.
// Independent facts gathered over one window, any number of which hold at
// once; a state machine would say less.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
pub struct Changes {
    pub names: HashSet<OsString>,
    /// Something changed that we could not attribute to a name: relist.
    pub rescan: bool,
    /// The watched directory's own attributes changed (its permissions,
    /// times, extended attributes: its tags), not what it holds.
    pub itself: bool,
    /// The watched directory itself moved elsewhere.
    pub moved: bool,
    /// The watched directory itself is gone (deleted, or its filesystem
    /// unmounted).
    pub deleted: bool,
}

impl Changes {
    /// What `later` says added to this: changes not applied yet, taken as
    /// one.
    pub fn merge(&mut self, later: Changes) {
        self.names.extend(later.names);
        self.rescan |= later.rescan;
        self.itself |= later.itself;
        self.moved |= later.moved;
        self.deleted |= later.deleted;
    }
}

/// What is told of a folder's changes. It is called on the watcher's own
/// thread, which every folder shares: it hands the changes on (to a
/// worker) and returns, never reading the disk there.
pub type Deliver = Arc<dyn Fn(Changes) + Send + Sync>;

/// How long a name only written to (a download, a log growing) waits
/// before it is told, from its first write: a file written to all along
/// is told once a second, not at each of the watcher's windows, each
/// costing a stat, a row and a frame. Any other change of it (closed after
/// writing, renamed, removed) tells it at once, with what waited.
pub const WRITTEN_WINDOW: Duration = Duration::from_secs(1);

/// Keeps the watch alive; dropping it stops watching.
pub struct WatchHandle {
    _inner: Box<dyn std::any::Any + Send>,
}

/// Watch `dir` (non-recursive). Changes are coalesced over `window` and
/// handed to `deliver` until `cancel` fires or the handle drops; once the
/// folder is gone (`Changes::deleted`), nothing more is.
///
/// # Errors
/// When the folder cannot be watched (gone, or the system's watches are
/// used up).
pub fn watch(
    dir: &Path,
    window: Duration,
    cancel: &CancelToken,
    deliver: impl Fn(Changes) + Send + Sync + 'static,
) -> std::io::Result<WatchHandle> {
    imp::watch(dir, window, cancel, Arc::new(deliver))
}

#[cfg(target_os = "linux")]
mod imp {
    //! One inotify instance for the process and one thread reading it. An
    //! instance per folder cost a thread and a descriptor each, and the
    //! system gives a user 128 instances (`max_user_instances`), shared
    //! with every other program: a window with many folders open (a split,
    //! columns, tabs that stay listed) ran out, and its folders stopped
    //! following their changes. Two subscribers of one folder share its
    //! descriptor (inotify answers the same one for the same inode), which
    //! is removed with the last of them.
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask, Watches};
    use parking_lot::Mutex;
    use rustix::event::{PollFd, PollFlags};

    use super::{Changes, Deliver, WRITTEN_WINDOW, WatchHandle};
    use crate::cancel::CancelToken;

    const MASK: WatchMask = WatchMask::CREATE
        .union(WatchMask::DELETE)
        .union(WatchMask::MODIFY)
        .union(WatchMask::ATTRIB)
        .union(WatchMask::CLOSE_WRITE)
        .union(WatchMask::MOVED_FROM)
        .union(WatchMask::MOVED_TO)
        .union(WatchMask::DELETE_SELF)
        .union(WatchMask::MOVE_SELF);

    struct Subscriber {
        id: u64,
        window: Duration,
        deliver: Deliver,
        cancel: CancelToken,
        /// What was gathered since the first change of its window, and
        /// when the window ends.
        pending: Option<(Changes, Instant)>,
        /// The names only written to since the first of them, and when
        /// they are told (`WRITTEN_WINDOW`): with `pending` when it is told
        /// first.
        written: Option<(HashSet<OsString>, Instant)>,
    }

    impl Subscriber {
        fn gathered(&mut self, now: Instant) -> &mut Changes {
            let window = self.window;
            &mut self
                .pending
                .get_or_insert_with(|| (Changes::default(), now + window))
                .0
        }

        /// `name` was written to, and nothing else happened to it yet: it
        /// waits, unless it is told already.
        fn written(&mut self, name: &std::ffi::OsStr, now: Instant) {
            if self
                .pending
                .as_ref()
                .is_some_and(|(changes, _)| changes.names.contains(name))
            {
                return;
            }
            self.written
                .get_or_insert_with(|| (HashSet::new(), now + WRITTEN_WINDOW))
                .0
                .insert(name.to_os_string());
        }

        /// What it tells now, if anything: what its window gathered, with
        /// the names only written to, or those alone once they waited
        /// enough.
        fn take_due(&mut self, now: Instant) -> Option<Changes> {
            if self.pending.as_ref().is_some_and(|(_, at)| *at <= now) {
                let (mut changes, _) = self.pending.take()?;
                if let Some((written, _)) = self.written.take() {
                    changes.names.extend(written);
                }
                return Some(changes);
            }
            if self.written.as_ref().is_some_and(|(_, at)| *at <= now) {
                let (names, _) = self.written.take()?;
                return Some(Changes {
                    names,
                    ..Changes::default()
                });
            }
            None
        }

        /// When it tells next.
        fn due(&self) -> Option<Instant> {
            let pending = self.pending.as_ref().map(|(_, at)| *at);
            let written = self.written.as_ref().map(|(_, at)| *at);
            pending.into_iter().chain(written).min()
        }
    }

    #[derive(Default)]
    struct State {
        next: u64,
        by_wd: HashMap<WatchDescriptor, Vec<Subscriber>>,
        /// Descriptors let go of so far (removed, or their folder gone).
        removals: u64,
        /// The watches being added, outside the lock, by the count of
        /// removals when each began: the kernel may give one a descriptor
        /// that goes before it is subscribed to.
        adding: BTreeMap<u64, usize>,
        /// The descriptors let go of while watches were added, with their
        /// removal's count: an add given one of them since it began adds
        /// again. Kept only while an add that began before runs, so a
        /// server that holds one add keeps a few numbers, never a watch.
        removed: Vec<(WatchDescriptor, u64)>,
    }

    struct Hub {
        /// The instance's watches, cloned to add one: adding resolves a
        /// path, which a sleeping drive or a server that stopped answering
        /// can hold, so no lock is held while it does (ADR-077), and no
        /// other folder waits behind it.
        watches: Watches,
        state: Mutex<State>,
    }

    fn hub() -> std::io::Result<&'static Hub> {
        static HUB: OnceLock<&'static Hub> = OnceLock::new();
        static STARTING: std::sync::Mutex<()> = std::sync::Mutex::new(());
        if let Some(hub) = HUB.get() {
            return Ok(hub);
        }
        let _starting = STARTING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(hub) = HUB.get() {
            return Ok(hub);
        }
        // Started on the first watch, tried again on the next if it could
        // not be (descriptors used up for now).
        let inotify = Inotify::init()?;
        let hub: &'static Hub = Box::leak(Box::new(Hub {
            watches: inotify.watches(),
            state: Mutex::new(State::default()),
        }));
        std::thread::Builder::new()
            .name("skwir-inotify".into())
            .spawn(move || run(inotify, hub))?;
        let _ = HUB.set(hub);
        Ok(hub)
    }

    pub fn watch(
        dir: &Path,
        window: Duration,
        cancel: &CancelToken,
        deliver: Deliver,
    ) -> std::io::Result<WatchHandle> {
        let hub = hub()?;
        let mut since = hub.state.lock().begin_add();
        let (added, mut state) = loop {
            let added = hub.watches.clone().add(dir, MASK);
            let mut state = hub.state.lock();
            if state.end_add(since, added.as_ref().ok()) {
                // Given the descriptor another watch of the folder had,
                // let go of meanwhile: the kernel's own is a new one.
                since = state.begin_add();
                continue;
            }
            break (added, state);
        };
        let added = added.map(|wd| {
            let id = state.next;
            state.next += 1;
            state.by_wd.entry(wd.clone()).or_default().push(Subscriber {
                id,
                window,
                deliver,
                cancel: cancel.clone(),
                pending: None,
                written: None,
            });
            (wd, id)
        });
        drop(state);
        let (wd, id) = added?;
        Ok(WatchHandle {
            _inner: Box::new(Unsubscribe { hub, wd, id }),
        })
    }

    struct Unsubscribe {
        hub: &'static Hub,
        wd: WatchDescriptor,
        id: u64,
    }

    impl Drop for Unsubscribe {
        fn drop(&mut self) {
            let mut state = self.hub.state.lock();
            // Gone already when the folder was (its descriptor ignored).
            let Some(subscribers) = state.by_wd.get_mut(&self.wd) else {
                return;
            };
            subscribers.retain(|s| s.id != self.id);
            if subscribers.is_empty() {
                state.by_wd.remove(&self.wd);
                state.let_go(self.wd.clone());
                // By descriptor, which reads no path.
                let _ = self.hub.watches.clone().remove(self.wd.clone());
            }
        }
    }

    /// Wait until the descriptor has events, or `timeout` passes (forever
    /// when `None`). Whether it has events.
    fn wait(inotify: &Inotify, timeout: Option<Duration>) -> bool {
        let mut fds = [PollFd::new(inotify, PollFlags::IN)];
        let ts = timeout.map(|t| rustix::time::Timespec {
            tv_sec: i64::try_from(t.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(t.subsec_nanos()),
        });
        matches!(rustix::event::poll(&mut fds, ts.as_ref()), Ok(n) if n > 0)
    }

    /// The thread: it sleeps in poll(2) until an event comes or a window
    /// ends; no timer wakes it otherwise.
    fn run(mut inotify: Inotify, hub: &'static Hub) {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let next = hub.state.lock().next_due();
            let timeout = next.map(|due| due.saturating_duration_since(Instant::now()));
            if wait(&inotify, timeout) {
                let now = Instant::now();
                match inotify.read_events(&mut buf) {
                    Ok(events) => {
                        let mut state = hub.state.lock();
                        for event in events {
                            state.absorb(&event.wd, event.mask, event.name, now);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(_) => hub.state.lock().rescan_all(now),
                }
            }
            let due = hub.state.lock().take_due(Instant::now());
            for (deliver, changes) in due {
                deliver(changes);
            }
        }
    }

    impl State {
        /// A watch begins to be added: the count it is known by.
        fn begin_add(&mut self) -> u64 {
            let since = self.removals;
            *self.adding.entry(since).or_default() += 1;
            since
        }

        /// The watch begun at `since` was added (as `wd`), or failed:
        /// whether its descriptor was let go of meanwhile, so it must be
        /// added again.
        fn end_add(&mut self, since: u64, wd: Option<&WatchDescriptor>) -> bool {
            let stale = wd.is_some_and(|wd| {
                self.removed
                    .iter()
                    .any(|(removed, at)| removed == wd && *at > since)
            });
            if let Some(count) = self.adding.get_mut(&since) {
                *count -= 1;
                if *count == 0 {
                    self.adding.remove(&since);
                }
            }
            match self.adding.keys().next() {
                Some(&oldest) => self.removed.retain(|(_, at)| *at > oldest),
                None => self.removed.clear(),
            }
            stale
        }

        /// A descriptor is let go of: removed, or its folder gone.
        fn let_go(&mut self, wd: WatchDescriptor) {
            self.removals += 1;
            if !self.adding.is_empty() {
                self.removed.push((wd, self.removals));
            }
        }

        /// When the first window ends.
        fn next_due(&self) -> Option<Instant> {
            self.by_wd
                .values()
                .flatten()
                .filter_map(Subscriber::due)
                .min()
        }

        fn rescan_all(&mut self, now: Instant) {
            for s in self.by_wd.values_mut().flatten() {
                s.gathered(now).rescan = true;
            }
        }

        fn absorb(
            &mut self,
            wd: &WatchDescriptor,
            mask: EventMask,
            name: Option<&std::ffi::OsStr>,
            now: Instant,
        ) {
            if mask.contains(EventMask::Q_OVERFLOW) {
                self.rescan_all(now);
                return;
            }
            if mask.contains(EventMask::IGNORED) {
                // The kernel's no longer, whoever had it: a folder gone
                // as a watch of it was added.
                self.let_go(wd.clone());
            }
            let Some(subscribers) = self.by_wd.get_mut(wd) else {
                return;
            };
            for s in subscribers.iter_mut() {
                // Only written to: it waits (`WRITTEN_WINDOW`).
                if let Some(name) = name
                    && mask.difference(EventMask::ISDIR) == EventMask::MODIFY
                {
                    s.written(name, now);
                    continue;
                }
                let changes = s.gathered(now);
                if mask.contains(EventMask::MOVE_SELF) {
                    changes.moved = true;
                } else if mask.intersects(EventMask::DELETE_SELF | EventMask::IGNORED) {
                    changes.deleted = true;
                } else if let Some(name) = name {
                    changes.names.insert(name.to_os_string());
                } else if mask.contains(EventMask::ATTRIB) {
                    changes.itself = true;
                }
                // Gone: told at once, as the window would end with nothing
                // more to come.
                if changes.deleted
                    && let Some((_, due)) = s.pending.as_mut()
                {
                    *due = now;
                }
            }
        }

        /// The changes whose window ended, and to whom. A folder gone is
        /// let go: its descriptor is no longer the kernel's.
        fn take_due(&mut self, now: Instant) -> Vec<(Deliver, Changes)> {
            let mut due = Vec::new();
            let mut gone = Vec::new();
            for (wd, subscribers) in &mut self.by_wd {
                for s in subscribers.iter_mut() {
                    if let Some(changes) = s.take_due(now) {
                        let deleted = changes.deleted;
                        if !s.cancel.is_cancelled() {
                            due.push((s.deliver.clone(), changes));
                        }
                        if deleted {
                            gone.push(wd.clone());
                        }
                    }
                }
            }
            for wd in gone {
                self.by_wd.remove(&wd);
            }
            due
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A watch given a descriptor let go of while it was added adds
        /// again; one let go of before it began, or with no add running,
        /// is not remembered.
        #[test]
        fn a_descriptor_let_go_of_during_an_add_is_added_again() {
            let root = tempfile::tempdir().unwrap();
            let inotify = Inotify::init().unwrap();
            let a = inotify.watches().add(root.path(), MASK).unwrap();
            let mut state = State::default();
            state.let_go(a.clone());
            assert!(state.removed.is_empty(), "no add ran");
            let since = state.begin_add();
            let later = state.begin_add();
            state.let_go(a.clone());
            assert!(state.end_add(since, Some(&a)), "let go of during the add");
            assert!(
                !state.removed.is_empty(),
                "an add that began before still runs"
            );
            let again = state.begin_add();
            assert!(!state.end_add(again, Some(&a)), "let go of before it began");
            assert!(state.end_add(later, Some(&a)));
            assert!(state.adding.is_empty());
            assert!(state.removed.is_empty(), "nothing kept once no add runs");
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::collections::HashSet;
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crossbeam_channel::{Receiver, RecvTimeoutError, unbounded};
    use notify::event::ModifyKind;
    use notify::{EventKind, RecursiveMode, Watcher};

    use super::{Changes, Deliver, WRITTEN_WINDOW, WatchHandle};
    use crate::cancel::CancelToken;

    pub fn watch(
        dir: &Path,
        window: Duration,
        cancel: &CancelToken,
        deliver: Deliver,
    ) -> std::io::Result<WatchHandle> {
        let cancel = cancel.clone();
        let (raw_tx, raw_rx) = unbounded::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = raw_tx.send(res);
        })
        .map_err(std::io::Error::other)?;
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(std::io::Error::other)?;
        // As opened, and as the system resolves it: macOS's FSEvents says
        // the resolved path of what it reports (`/private/tmp/x` for
        // `/tmp/x`, a folder reached through a link), Windows the one
        // given.
        let mut dirs = vec![dir.to_path_buf()];
        if let Ok(resolved) = crate::os_path::canonical(dir)
            && resolved != dir
        {
            dirs.push(resolved);
        }
        let dirs: Arc<[PathBuf]> = dirs.into();
        std::thread::Builder::new()
            .name("skwir-watch".into())
            .spawn(move || coalesce(&raw_rx, &deliver, &dirs, window, &cancel))?;
        Ok(WatchHandle {
            _inner: Box::new(watcher),
        })
    }

    /// What was gathered and not told yet: a window's changes, and the
    /// names only written to (`WRITTEN_WINDOW`), each with when it is told.
    struct Gathered {
        window: Duration,
        pending: Option<(Changes, Instant)>,
        written: Option<(HashSet<OsString>, Instant)>,
    }

    impl Gathered {
        fn changes(&mut self, now: Instant) -> &mut Changes {
            let window = self.window;
            &mut self
                .pending
                .get_or_insert_with(|| (Changes::default(), now + window))
                .0
        }

        /// `name` was written to, and nothing else happened to it yet: it
        /// waits, unless it is told already.
        fn written(&mut self, name: &OsStr, now: Instant) {
            if self
                .pending
                .as_ref()
                .is_some_and(|(changes, _)| changes.names.contains(name))
            {
                return;
            }
            self.written
                .get_or_insert_with(|| (HashSet::new(), now + WRITTEN_WINDOW))
                .0
                .insert(name.to_os_string());
        }

        fn due(&self) -> Option<Instant> {
            let pending = self.pending.as_ref().map(|(_, at)| *at);
            let written = self.written.as_ref().map(|(_, at)| *at);
            pending.into_iter().chain(written).min()
        }

        /// What is told by `now`: the window's changes with the names only
        /// written to, or those alone once they waited enough.
        fn take(&mut self, now: Instant) -> Option<Changes> {
            if self.pending.as_ref().is_some_and(|(_, at)| *at <= now) {
                let (mut changes, _) = self.pending.take()?;
                if let Some((written, _)) = self.written.take() {
                    changes.names.extend(written);
                }
                return Some(changes);
            }
            if self.written.as_ref().is_some_and(|(_, at)| *at <= now) {
                let (names, _) = self.written.take()?;
                return Some(Changes {
                    names,
                    ..Changes::default()
                });
            }
            None
        }
    }

    fn coalesce(
        raw: &Receiver<notify::Result<notify::Event>>,
        deliver: &Deliver,
        dirs: &[PathBuf],
        window: Duration,
        cancel: &CancelToken,
    ) {
        let mut gathered = Gathered {
            window,
            pending: None,
            written: None,
        };
        loop {
            let ev = match gathered.due() {
                Some(due) => match raw.recv_deadline(due) {
                    Ok(ev) => Some(ev),
                    Err(RecvTimeoutError::Timeout) => None,
                    // The watcher dropped, its sender with it: what was
                    // gathered is told, and it ends.
                    Err(RecvTimeoutError::Disconnected) => {
                        let far = Instant::now() + WRITTEN_WINDOW + window;
                        if let Some(changes) = gathered.take(far)
                            && !cancel.is_cancelled()
                        {
                            deliver(changes);
                        }
                        return;
                    }
                },
                None => match raw.recv() {
                    Ok(ev) => Some(ev),
                    Err(_) => return,
                },
            };
            let now = Instant::now();
            if let Some(ev) = ev {
                absorb(&mut gathered, ev, dirs, now);
            }
            if let Some(changes) = gathered.take(now) {
                if cancel.is_cancelled() {
                    return;
                }
                deliver(changes);
            }
        }
    }

    /// What `ev` says of the folder watched, known by any of `dirs`. A
    /// file only written to waits (`Gathered::written`): macOS says so
    /// (`ModifyKind::Data`), Windows says a file changed (`ModifyKind::Any`,
    /// its attributes too, which then wait as long).
    fn absorb(
        gathered: &mut Gathered,
        ev: notify::Result<notify::Event>,
        dirs: &[PathBuf],
        now: Instant,
    ) {
        let Ok(ev) = ev else {
            gathered.changes(now).rescan = true;
            return;
        };
        if ev.need_rescan() {
            gathered.changes(now).rescan = true;
            return;
        }
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        let written = matches!(
            ev.kind,
            EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Any)
        );
        let watched = |path: Option<&Path>| path.is_some_and(|p| dirs.iter().any(|d| d == p));
        for p in ev.paths {
            if watched(p.parent())
                && let Some(name) = p.file_name()
            {
                if written {
                    gathered.written(name, now);
                } else {
                    gathered.changes(now).names.insert(name.to_os_string());
                }
            } else if watched(Some(&p)) {
                if matches!(ev.kind, EventKind::Modify(ModifyKind::Metadata(_))) {
                    gathered.changes(now).itself = true;
                } else {
                    gathered.changes(now).rescan = true;
                }
            }
        }
    }
}

/// Files a plugin reads, followed: their changes (never their reads:
/// notify reports a file opened, and a watcher that rescans on that
/// rescans forever on its own reads) are let settle, then `then` runs
/// once, on a thread of the watcher's own that ends when it is dropped, so
/// no shared worker waits. A burst (a disk bringing several partitions)
/// makes one call.
pub struct Settled {
    watcher: notify::RecommendedWatcher,
}

impl Settled {
    /// Watch nothing yet: `relevant` says which of an event's paths count,
    /// and `then` answers whether to go on (false once the app is gone).
    /// `None` when the system gives no watcher.
    pub fn start(
        name: &str,
        settle: Duration,
        relevant: impl Fn(&Path) -> bool + Send + 'static,
        then: impl Fn() -> bool + Send + 'static,
    ) -> Option<Self> {
        let (tx, rx) = crossbeam_channel::unbounded::<()>();
        let handler = move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|event| {
                !matches!(event.kind, notify::EventKind::Access(_))
                    && event.paths.iter().any(|path| relevant(path))
            }) {
                let _ = tx.send(());
            }
        };
        let watcher = notify::recommended_watcher(handler).ok()?;
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                while rx.recv().is_ok() {
                    while rx.recv_timeout(settle).is_ok() {}
                    if !then() {
                        return;
                    }
                    crate::scheduler::release_now();
                }
            })
            .ok()?;
        Some(Settled { watcher })
    }

    /// Follow `path` (what is in it, not below). Whether it is followed: one
    /// not there yet is not.
    pub fn watch(&mut self, path: &Path) -> bool {
        use notify::Watcher as _;
        self.watcher
            .watch(path, notify::RecursiveMode::NonRecursive)
            .is_ok()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    const SOON: Duration = Duration::from_secs(5);

    fn watched(dir: &Path) -> (WatchHandle, mpsc::Receiver<Changes>) {
        let (tx, rx) = mpsc::channel();
        let tx = parking_lot::Mutex::new(tx);
        let handle = watch(
            dir,
            Duration::from_millis(10),
            &CancelToken::new(),
            move |c| {
                let _ = tx.lock().send(c);
            },
        )
        .unwrap();
        (handle, rx)
    }

    fn told(rx: &mpsc::Receiver<Changes>, name: &str) -> bool {
        let deadline = std::time::Instant::now() + SOON;
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(c) if c.names.contains(std::ffi::OsStr::new(name)) => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        false
    }

    /// More folders than the system gives a user inotify instances (128 by
    /// default) are all followed: they share one.
    #[test]
    fn many_folders_are_followed_at_once() {
        let root = tempfile::tempdir().unwrap();
        let watches: Vec<_> = (0..300)
            .map(|i| {
                let dir = root.path().join(format!("d{i}"));
                std::fs::create_dir(&dir).unwrap();
                let (handle, rx) = watched(&dir);
                (dir, handle, rx)
            })
            .collect();
        for (dir, _, _) in &watches {
            std::fs::write(dir.join("new"), b"").unwrap();
        }
        for (dir, _, rx) in &watches {
            assert!(told(rx, "new"), "{} was not followed", dir.display());
        }
    }

    /// Two watches of one folder share its descriptor: either let go, the
    /// other still follows it; both let go, it can be watched again.
    #[test]
    fn a_folder_watched_twice_is_followed_until_both_let_go() {
        let root = tempfile::tempdir().unwrap();
        let (first, first_rx) = watched(root.path());
        let (second, second_rx) = watched(root.path());
        std::fs::write(root.path().join("a"), b"").unwrap();
        assert!(told(&first_rx, "a"));
        assert!(told(&second_rx, "a"));
        drop(first);
        std::fs::write(root.path().join("b"), b"").unwrap();
        assert!(told(&second_rx, "b"));
        drop(second);
        let (_again, again_rx) = watched(root.path());
        std::fs::write(root.path().join("c"), b"").unwrap();
        assert!(told(&again_rx, "c"));
    }

    /// A file only written to (a download) is told once a second, not at
    /// each window; closed, it is told at once.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_only_written_to_waits_and_its_close_is_told_at_once() {
        use std::io::Write as _;
        use std::time::Instant;
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(&base).unwrap();
        let mut file = std::fs::File::create(root.path().join("growing.part")).unwrap();
        let (_handle, rx) = watched(root.path());
        let named = |c: &Changes| c.names.contains(std::ffi::OsStr::new("growing.part"));
        let started = Instant::now();
        for _ in 0..10 {
            file.write_all(b"x").unwrap();
            std::thread::sleep(Duration::from_millis(100));
        }
        let mut told = 0;
        while let Some(left) =
            (started + Duration::from_millis(1300)).checked_duration_since(Instant::now())
        {
            match rx.recv_timeout(left) {
                Ok(c) if named(&c) => told += 1,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(told <= 2, "told {told} times in a second of writes");
        let closed = Instant::now();
        drop(file);
        let deadline = closed + SOON;
        let mut seen = None;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(c) if named(&c) => {
                    seen = Some(closed.elapsed());
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let seen = seen.expect("its close is told");
        assert!(seen < WRITTEN_WINDOW, "told {seen:?} after its close");
    }

    #[test]
    fn a_folder_removed_says_so_once() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("gone");
        std::fs::create_dir(&dir).unwrap();
        let (_handle, rx) = watched(&dir);
        std::fs::remove_dir(&dir).unwrap();
        let deadline = std::time::Instant::now() + SOON;
        let mut deleted = false;
        while !deleted && std::time::Instant::now() < deadline {
            if let Ok(c) = rx.recv_timeout(Duration::from_millis(100)) {
                deleted = c.deleted;
            }
        }
        assert!(deleted);
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "nothing more is told of a folder gone"
        );
    }
}
