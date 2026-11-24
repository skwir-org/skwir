//! The folder watches that hold their folder open (ADR-149, 16), kept by
//! volume. Windows' watch (`ReadDirectoryChangesW`, notify's) keeps a
//! handle on the folder for as long as it watches, where Linux's inotify
//! holds nothing, so a drive a pane shows a folder of could not be
//! removed. Each watch is kept here with what made it and the volume it is
//! on (`reach::volume_of`, read from the path's text alone). What hears
//! that a drive is about to be removed lets go of the watches on it,
//! waiting until the system closed them ([`Kept::let_go_on`]); if the
//! drive stays, they are watched again, each told a rescan, as what changed
//! meanwhile was not told ([`Kept::watch_again_on`]); if it goes, each is
//! told its folder went ([`Kept::gone_on`]). A watch made on a volume being
//! let go of waits, holding nothing, until then. What registers for those
//! notices is told the volume of each watch made or ended
//! ([`Kept::follow_volumes`]).
//!
//! A watch let go of tells nothing more: what it gathered and did not tell
//! yet (a name still being written) would have a worker read the drive
//! while its removal waits. Its start is given a token of its own
//! ([`Held::own`]), cancelled before it is let go of; what it missed, the
//! rescan it is told once watched again covers.
//!
//! Locks: the state's is never held while a watch's own is taken, so a
//! let-go waits for a watch being started or ended, never the reverse.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use parking_lot::Mutex;

use super::{Changes, Deliver, WatchHandle};
use crate::cancel::CancelToken;

/// A folder watched as the system watches it.
pub(super) struct Live {
    /// Dropping it ends the system's watch.
    pub(super) watch: Box<dyn std::any::Any + Send>,
    /// Disconnected once the system let go of the folder (its handle
    /// closed); nothing is sent on it.
    pub(super) ended: Receiver<()>,
}

/// How the system watches a folder: it tells nothing once the token it is
/// given is cancelled.
pub(super) type Start = fn(&Path, Duration, &CancelToken, &Deliver) -> std::io::Result<Live>;

/// A system's watch kept, and what tells it to tell nothing more.
struct Held {
    live: Live,
    /// What its start was given: the watch's own, a child of its owner's
    /// (`Slot::cancel`), cancelled as it is let go of.
    own: CancelToken,
}

impl Held {
    /// Let go of, telling nothing more: what says once the system let go
    /// of its folder.
    fn let_go(self) -> Receiver<()> {
        self.own.cancel();
        drop(self.live.watch);
        self.live.ended
    }
}

/// What is told the volume of each watch made or ended.
pub(super) type Told = Arc<dyn Fn(&Path) + Send + Sync>;

/// A watch, with what makes it again.
struct Slot {
    dir: PathBuf,
    /// The volume it is on, by the path's text; `None` where none is
    /// known.
    volume: Option<PathBuf>,
    window: Duration,
    cancel: CancelToken,
    deliver: Deliver,
    /// The system's watch, `None` while let go of. Held while it is
    /// started or ended, so a let-go begun meanwhile waits for it.
    live: Mutex<Option<Held>>,
}

impl Slot {
    fn on(&self, volume: &Path) -> bool {
        self.volume.as_deref() == Some(volume)
    }

    /// Its folder watched as the system does, with a token of its own.
    fn started(&self, start: Start) -> std::io::Result<Held> {
        let own = self.cancel.child();
        let live = start(&self.dir, self.window, &own, &self.deliver)?;
        Ok(Held { live, own })
    }
}

#[derive(Default)]
struct State {
    next: u64,
    slots: crate::hashing::Map<u64, Arc<Slot>>,
    /// The volumes being let go of: a watch made there waits.
    letting_go: Vec<PathBuf>,
    told: Option<Told>,
}

impl State {
    fn on(&self, volume: &Path) -> Vec<Arc<Slot>> {
        self.slots
            .values()
            .filter(|slot| slot.on(volume))
            .cloned()
            .collect()
    }
}

/// The watches kept, and how the system makes one.
pub(super) struct Kept {
    start: Start,
    volume_of: fn(&Path) -> Option<PathBuf>,
    state: Mutex<State>,
}

/// Keeps a watch while it lives: dropped, the system's watch ends.
struct Unkeep {
    kept: Arc<Kept>,
    id: u64,
}

impl Drop for Unkeep {
    fn drop(&mut self) {
        let (slot, told) = {
            let mut state = self.kept.state.lock();
            (state.slots.remove(&self.id), state.told.clone())
        };
        let Some(slot) = slot else {
            return;
        };
        // No one listens any more: it tells nothing, as Linux's.
        let held = slot.live.lock().take();
        drop(held.map(Held::let_go));
        if let (Some(told), Some(volume)) = (told, slot.volume.as_deref()) {
            told(volume);
        }
    }
}

impl Kept {
    pub(super) fn new(start: Start, volume_of: fn(&Path) -> Option<PathBuf>) -> Arc<Self> {
        Arc::new(Kept {
            start,
            volume_of,
            state: Mutex::new(State::default()),
        })
    }

    /// Watch `dir` as the system does, kept until the handle drops; on a
    /// volume being let go of, it waits, holding nothing, to be watched
    /// again.
    pub(super) fn watch(
        self: &Arc<Self>,
        dir: &Path,
        window: Duration,
        cancel: &CancelToken,
        deliver: Deliver,
    ) -> std::io::Result<WatchHandle> {
        let slot = Arc::new(Slot {
            dir: dir.to_path_buf(),
            volume: (self.volume_of)(dir),
            window,
            cancel: cancel.clone(),
            deliver,
            live: Mutex::new(None),
        });
        // Taken before it is kept: a let-go that finds it waits for its
        // start.
        let mut live = slot.live.lock();
        let (id, waits, told) = {
            let mut state = self.state.lock();
            let id = state.next;
            state.next += 1;
            state.slots.insert(id, Arc::clone(&slot));
            let waits = slot
                .volume
                .as_ref()
                .is_some_and(|volume| state.letting_go.contains(volume));
            (id, waits, state.told.clone())
        };
        let kept = Unkeep {
            kept: Arc::clone(self),
            id,
        };
        if !waits {
            match slot.started(self.start) {
                Ok(watched) => *live = Some(watched),
                Err(error) => {
                    // Its own lock let go of first: what unkeeps it takes it.
                    drop(live);
                    drop(kept);
                    return Err(error);
                }
            }
        }
        drop(live);
        if let (Some(told), Some(volume)) = (told, slot.volume.as_deref()) {
            told(volume);
        }
        Ok(WatchHandle {
            _inner: Box::new(kept),
        })
    }

    /// Let go of every watch on `volume`, the system's watches ended,
    /// waiting at most `at_most` until the system let go of their folders;
    /// those made there from now on wait. Whether every one was let go of.
    pub(super) fn let_go_on(&self, volume: &Path, at_most: Duration) -> bool {
        let deadline = Instant::now() + at_most;
        let slots = {
            let mut state = self.state.lock();
            if !state.letting_go.iter().any(|v| v == volume) {
                state.letting_go.push(volume.to_path_buf());
            }
            state.on(volume)
        };
        let mut all = true;
        let mut ending = Vec::new();
        for slot in &slots {
            match slot.live.try_lock_until(deadline) {
                Some(mut live) => {
                    if let Some(held) = live.take() {
                        ending.push(held.let_go());
                    }
                }
                // Still being started (a drive waking): it holds the folder.
                None => all = false,
            }
        }
        for ended in ending {
            all &= matches!(
                ended.recv_deadline(deadline),
                Err(RecvTimeoutError::Disconnected)
            );
        }
        all
    }

    /// Watch again what was let go of on `volume`, each told a rescan once
    /// watched, or that its folder went where it cannot be watched any
    /// more. Starts each watch where it is called: off a thread that must
    /// answer at once.
    pub(super) fn watch_again_on(&self, volume: &Path) {
        let slots = {
            let mut state = self.state.lock();
            state.letting_go.retain(|v| v != volume);
            state.on(volume)
        };
        for slot in slots {
            let mut live = slot.live.lock();
            if live.is_some() || slot.cancel.is_cancelled() || self.letting_go(volume) {
                continue;
            }
            let changes = match slot.started(self.start) {
                Ok(watched) => {
                    *live = Some(watched);
                    Changes {
                        rescan: true,
                        ..Changes::default()
                    }
                }
                Err(_) => Changes {
                    deleted: true,
                    ..Changes::default()
                },
            };
            drop(live);
            (slot.deliver)(changes);
        }
    }

    /// `volume` went (a drive removed): each watch on it ends, if it did
    /// not already, and is told its folder went with its filesystem; a
    /// watch made there from now on is the system's again.
    pub(super) fn gone_on(&self, volume: &Path) {
        let slots = {
            let mut state = self.state.lock();
            state.letting_go.retain(|v| v != volume);
            state.on(volume)
        };
        for slot in slots {
            let held = slot.live.lock().take();
            drop(held.map(Held::let_go));
            if !slot.cancel.is_cancelled() {
                (slot.deliver)(Changes {
                    deleted: true,
                    unmounted: true,
                    ..Changes::default()
                });
            }
        }
    }

    /// Whether `volume` is being let go of: from [`let_go_on`](Self::let_go_on)
    /// until [`watch_again_on`](Self::watch_again_on) or
    /// [`gone_on`](Self::gone_on) begin, before they tell anything.
    pub(super) fn letting_go(&self, volume: &Path) -> bool {
        self.state.lock().letting_go.iter().any(|v| v == volume)
    }

    /// Whether a watch is kept on `volume`, watching or let go of.
    pub(super) fn watches_on(&self, volume: &Path) -> bool {
        self.state.lock().slots.values().any(|slot| slot.on(volume))
    }

    /// Tell `told` the volume of each watch made or ended from now on
    /// (`None`: no one); the volumes watches are kept on now.
    pub(super) fn follow_volumes(&self, told: Option<Told>) -> Vec<PathBuf> {
        let mut state = self.state.lock();
        state.told = told;
        let mut volumes: Vec<PathBuf> = Vec::new();
        for volume in state.slots.values().filter_map(|slot| slot.volume.as_ref()) {
            if !volumes.contains(volume) {
                volumes.push(volume.clone());
            }
        }
        volumes
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use parking_lot::Mutex;

    use super::{Kept, Live};
    use crate::cancel::CancelToken;
    use crate::fs::watch::{Changes, Deliver};

    /// The folders the stand-in for Windows' watches holds now, once for
    /// each of its watches: each test its own folders.
    static WATCHING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    /// One of the stand-in's watches: counted while it lives, its end
    /// told as it drops.
    struct Watching {
        dir: PathBuf,
        _ends: crossbeam_channel::Sender<()>,
    }

    impl Drop for Watching {
        fn drop(&mut self) {
            let mut watching = WATCHING.lock();
            if let Some(at) = watching.iter().position(|d| *d == self.dir) {
                watching.swap_remove(at);
            }
        }
    }

    /// Watches a folder as Windows would, but holds nothing: a folder
    /// named `gone` cannot be watched.
    fn start(dir: &Path, _: Duration, _: &CancelToken, _: &Deliver) -> std::io::Result<Live> {
        if dir.ends_with("gone") {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        WATCHING.lock().push(dir.to_path_buf());
        let (ends, ended) = crossbeam_channel::bounded(0);
        Ok(Live {
            watch: Box::new(Watching {
                dir: dir.to_path_buf(),
                _ends: ends,
            }),
            ended,
        })
    }

    /// The volume is the path's first component, as a drive letter is.
    fn volume_of(path: &Path) -> Option<PathBuf> {
        path.components().nth(1).map(|c| Path::new("/").join(c))
    }

    fn watching(dir: &str) -> usize {
        WATCHING
            .lock()
            .iter()
            .filter(|d| d.as_path() == Path::new(dir))
            .count()
    }

    /// What a watch was told.
    fn told() -> (Deliver, Arc<Mutex<Vec<Changes>>>) {
        let told: Arc<Mutex<Vec<Changes>>> = Arc::default();
        let into = Arc::clone(&told);
        (Arc::new(move |c| into.lock().push(c)), told)
    }

    /// Let go of, the watches on a volume end, and those on another stay;
    /// watched again, each is watched and told to read its folder again.
    #[test]
    fn a_volume_let_go_of_is_watched_again_if_it_stays() {
        let kept = Kept::new(start, volume_of);
        let cancel = CancelToken::new();
        let (deliver, changes) = told();
        let a = kept
            .watch(
                Path::new("/e/a"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        let b = kept
            .watch(
                Path::new("/e/b"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        let other = kept
            .watch(
                Path::new("/f/c"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        assert_eq!((watching("/e/a"), watching("/e/b")), (1, 1));
        assert!(kept.let_go_on(Path::new("/e"), Duration::from_secs(5)));
        assert_eq!((watching("/e/a"), watching("/e/b")), (0, 0));
        assert_eq!(watching("/f/c"), 1, "another volume's stays");
        assert!(kept.watches_on(Path::new("/e")), "kept, let go of");
        assert!(changes.lock().is_empty(), "nothing told as it is let go of");
        kept.watch_again_on(Path::new("/e"));
        assert_eq!((watching("/e/a"), watching("/e/b")), (1, 1));
        let told = changes.lock();
        assert_eq!(told.len(), 2);
        assert!(told.iter().all(|c| c.rescan && !c.deleted));
        drop(told);
        drop((a, b, other));
        assert_eq!(watching("/e/a") + watching("/e/b") + watching("/f/c"), 0);
        assert!(!kept.watches_on(Path::new("/e")));
    }

    /// A watch made on a volume being let go of holds nothing until it is
    /// watched again; one whose folder went meanwhile is told so.
    #[test]
    fn a_watch_made_while_its_volume_is_let_go_of_waits() {
        let kept = Kept::new(start, volume_of);
        let cancel = CancelToken::new();
        let (deliver, changes) = told();
        assert!(kept.let_go_on(Path::new("/g"), Duration::from_secs(5)));
        let waits = kept
            .watch(
                Path::new("/g/new"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        let gone = kept
            .watch(
                Path::new("/g/gone"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        assert_eq!(watching("/g/new"), 0, "it waits");
        kept.watch_again_on(Path::new("/g"));
        assert_eq!(watching("/g/new"), 1);
        let told = changes.lock();
        assert!(told.iter().any(|c| c.rescan && !c.deleted));
        assert!(told.iter().any(|c| c.deleted), "the folder that went");
        drop(told);
        drop((waits, gone));
    }

    /// One of the stand-in's watches holding a name it gathered and did not
    /// tell yet: dropped, it tells it, as notify's thread would once its
    /// watcher drops, unless the token it was given says not to.
    struct Gathering {
        cancel: CancelToken,
        deliver: Deliver,
        _ends: crossbeam_channel::Sender<()>,
    }

    impl Drop for Gathering {
        fn drop(&mut self) {
            if !self.cancel.is_cancelled() {
                (self.deliver)(Changes {
                    names: [std::ffi::OsString::from("still being written")].into(),
                    ..Changes::default()
                });
            }
        }
    }

    // A `Start`, which may fail; this one never does.
    #[allow(clippy::unnecessary_wraps)]
    fn gathering(
        _: &Path,
        _: Duration,
        cancel: &CancelToken,
        deliver: &Deliver,
    ) -> std::io::Result<Live> {
        let (ends, ended) = crossbeam_channel::bounded(0);
        Ok(Live {
            watch: Box::new(Gathering {
                cancel: cancel.clone(),
                deliver: Arc::clone(deliver),
                _ends: ends,
            }),
            ended,
        })
    }

    /// A watch let go of tells nothing of what it gathered, which a worker
    /// would read on the drive while its removal waits; nor once its drive
    /// went, nor once its handle dropped. Watched again, it is told a
    /// rescan alone; its owner's token is never cancelled for it.
    #[test]
    fn a_watch_let_go_of_tells_nothing_more() {
        let kept = Kept::new(gathering, volume_of);
        let cancel = CancelToken::new();
        let (deliver, changes) = told();
        let watched = kept
            .watch(
                Path::new("/l/a"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        assert!(kept.let_go_on(Path::new("/l"), Duration::from_secs(5)));
        assert!(changes.lock().is_empty(), "nothing told as it is let go of");
        assert!(kept.letting_go(Path::new("/l")) && !kept.letting_go(Path::new("/n")));
        kept.watch_again_on(Path::new("/l"));
        assert!(
            !kept.letting_go(Path::new("/l")),
            "ended as the rescan is told"
        );
        assert_eq!(changes.lock().len(), 1);
        assert!(changes.lock()[0].rescan && changes.lock()[0].names.is_empty());
        assert!(!cancel.is_cancelled());
        assert!(kept.let_go_on(Path::new("/l"), Duration::from_secs(5)));
        kept.gone_on(Path::new("/l"));
        assert!(
            changes.lock()[1..]
                .iter()
                .all(|c| c.deleted && c.names.is_empty())
        );
        drop(watched);
        let ended = kept
            .watch(Path::new("/m/a"), Duration::ZERO, &cancel, deliver)
            .unwrap();
        let before = changes.lock().len();
        drop(ended);
        assert_eq!(
            changes.lock().len(),
            before,
            "a handle dropped: no one listens"
        );
    }

    /// Gone, its watches end and are told their folders went with it, the
    /// drive's filesystem; a watch made there after is the system's.
    #[test]
    fn a_volume_gone_tells_its_watches() {
        let kept = Kept::new(start, volume_of);
        let cancel = CancelToken::new();
        let (deliver, changes) = told();
        let live = kept
            .watch(
                Path::new("/h/a"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        assert!(kept.let_go_on(Path::new("/h"), Duration::from_secs(5)));
        kept.gone_on(Path::new("/h"));
        assert_eq!(watching("/h/a"), 0);
        assert!(changes.lock().iter().all(|c| c.deleted && c.unmounted));
        assert_eq!(changes.lock().len(), 1);
        let after = kept
            .watch(
                Path::new("/h/b"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        assert_eq!(watching("/h/b"), 1, "no longer let go of");
        drop((live, after));
    }

    /// What follows the volumes is told each watch made and ended, and
    /// finds those kept when it begins; a watch that could not be made is
    /// not kept.
    #[test]
    fn the_volumes_watched_are_told() {
        let kept = Kept::new(start, volume_of);
        let cancel = CancelToken::new();
        let (deliver, _) = told();
        let before = kept
            .watch(
                Path::new("/i/a"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        let volumes: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
        let into = Arc::clone(&volumes);
        let now = kept.follow_volumes(Some(Arc::new(move |volume: &Path| {
            into.lock().push(volume.to_path_buf());
        })));
        assert_eq!(now, vec![PathBuf::from("/i")]);
        let after = kept
            .watch(
                Path::new("/j/b"),
                Duration::ZERO,
                &cancel,
                Arc::clone(&deliver),
            )
            .unwrap();
        assert!(
            kept.watch(Path::new("/k/gone"), Duration::ZERO, &cancel, deliver)
                .is_err()
        );
        assert!(!kept.watches_on(Path::new("/k")), "not kept");
        drop(before);
        assert!(!kept.watches_on(Path::new("/i")));
        assert_eq!(
            *volumes.lock(),
            [
                PathBuf::from("/j"),
                PathBuf::from("/k"),
                PathBuf::from("/i")
            ],
            "each told as made and as it ends"
        );
        kept.follow_volumes(None);
        drop(after);
        assert_eq!(volumes.lock().len(), 3, "no one follows any more");
    }
}
