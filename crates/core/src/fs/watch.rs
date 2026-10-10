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
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask, Watches};
    use parking_lot::Mutex;
    use rustix::event::{PollFd, PollFlags};

    use super::{Changes, Deliver, WatchHandle};
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
    }

    impl Subscriber {
        fn gathered(&mut self, now: Instant) -> &mut Changes {
            let window = self.window;
            &mut self
                .pending
                .get_or_insert_with(|| (Changes::default(), now + window))
                .0
        }
    }

    #[derive(Default)]
    struct State {
        next: u64,
        by_wd: HashMap<WatchDescriptor, Vec<Subscriber>>,
    }

    struct Hub {
        /// Held while a watch is added or removed, and only then: adding
        /// resolves a path, which a sleeping drive can hold, and the
        /// thread reading events never waits for it. Adding and removing
        /// one after the other, a descriptor removed for its last
        /// subscriber is never one a new subscriber was just given.
        watches: Mutex<Watches>,
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
            watches: Mutex::new(inotify.watches()),
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
        let mut watches = hub.watches.lock();
        let wd = watches.add(dir, MASK)?;
        let mut state = hub.state.lock();
        let id = state.next;
        state.next += 1;
        state.by_wd.entry(wd.clone()).or_default().push(Subscriber {
            id,
            window,
            deliver,
            cancel: cancel.clone(),
            pending: None,
        });
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
            let mut watches = self.hub.watches.lock();
            let mut state = self.hub.state.lock();
            // Gone already when the folder was (its descriptor ignored).
            let Some(subscribers) = state.by_wd.get_mut(&self.wd) else {
                return;
            };
            subscribers.retain(|s| s.id != self.id);
            if subscribers.is_empty() {
                state.by_wd.remove(&self.wd);
                drop(state);
                let _ = watches.remove(self.wd.clone());
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
        /// When the first window ends.
        fn next_due(&self) -> Option<Instant> {
            self.by_wd
                .values()
                .flatten()
                .filter_map(|s| s.pending.as_ref().map(|(_, due)| *due))
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
            let Some(subscribers) = self.by_wd.get_mut(wd) else {
                return;
            };
            for s in subscribers.iter_mut() {
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
                    if s.pending.as_ref().is_some_and(|(_, at)| *at <= now)
                        && let Some((changes, _)) = s.pending.take()
                    {
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
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use crossbeam_channel::{Receiver, unbounded};
    use notify::{EventKind, RecursiveMode, Watcher};

    use super::{Changes, Deliver, WatchHandle};
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
        let dir: Arc<PathBuf> = Arc::new(dir.to_path_buf());
        std::thread::Builder::new()
            .name("skwir-watch".into())
            .spawn(move || coalesce(&raw_rx, &deliver, &dir, window, &cancel))?;
        Ok(WatchHandle {
            _inner: Box::new(watcher),
        })
    }

    fn coalesce(
        raw: &Receiver<notify::Result<notify::Event>>,
        deliver: &Deliver,
        dir: &Path,
        window: Duration,
        cancel: &CancelToken,
    ) {
        // Ends when the watcher is dropped: its sender with it.
        while let Ok(first) = raw.recv() {
            let mut changes = Changes::default();
            absorb(&mut changes, first, dir);
            let deadline = std::time::Instant::now() + window;
            while let Ok(ev) = raw.recv_deadline(deadline) {
                absorb(&mut changes, ev, dir);
            }
            if cancel.is_cancelled() {
                return;
            }
            deliver(changes);
        }
    }

    fn absorb(changes: &mut Changes, ev: notify::Result<notify::Event>, dir: &Path) {
        let Ok(ev) = ev else {
            changes.rescan = true;
            return;
        };
        if ev.need_rescan() {
            changes.rescan = true;
            return;
        }
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        for p in ev.paths {
            if p.parent() == Some(dir)
                && let Some(name) = p.file_name()
            {
                changes.names.insert(name.to_os_string());
            } else if p == dir {
                if matches!(
                    ev.kind,
                    EventKind::Modify(notify::event::ModifyKind::Metadata(_))
                ) {
                    changes.itself = true;
                } else {
                    changes.rescan = true;
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
