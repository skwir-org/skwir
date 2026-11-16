//! The files this process keeps open while it shows them (what plays),
//! known so that a plan lets go of them before it takes them away
//! (ADR-136).
//!
//! On Windows a file open for reading cannot be removed, moved or
//! replaced: the media helper's reader opens without the sharing that
//! allows it. Through some remotes' mounts a file removed while open
//! stays on the server, hidden, until it is closed (sshfs's
//! `.fuse_hidden…`), and its folder cannot be removed meanwhile. So what
//! keeps a file open says so here ([`keep_open`]), and the executor, before
//! a plan's first step, lets go of those its steps take
//! ([`let_go_of`]), waiting for them to be closed.
//!
//! A holder that would open the file again (a reader asked for a page)
//! waits until the plan that took it has ended ([`wait_while_taken`]):
//! by then the file is gone, moved or left. A helper's manager keeps it
//! through [`WhileOpen`].
//!
//! As [`super::wait_let_go`] does for the folders listings hold, this is
//! the process's: a file open is open whoever asks to remove it.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use crate::cancel::{CancelSignal, CancelToken};
use crate::op::PlanId;

/// What a holder does when a plan takes its file: close it, told which
/// plan (the id its observers are told, `Executor::observe_plans`).
type LetGo = Arc<dyn Fn(PlanId) + Send + Sync>;

struct Kept {
    id: u64,
    path: PathBuf,
    let_go: LetGo,
}

static KEPT: Mutex<Vec<Kept>> = Mutex::new(Vec::new());
/// How many are kept, read without the lock: most plans run while
/// nothing plays, and look no further.
static COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT: AtomicU64 = AtomicU64::new(1);
static CLOSED: Condvar = Condvar::new();
/// The files let go of for each plan still running (`let_go_of` notes
/// them, its [`Taking`] forgets them as the plan ends).
static TAKING: Mutex<Vec<(PlanId, PathBuf)>> = Mutex::new(Vec::new());
static TAKEN: Condvar = Condvar::new();
/// Counts the plans that ended having let go of something (`Taking`
/// dropped): what failed while one ran (`Taken`) is asked again once it
/// moved.
static ENDED: AtomicU64 = AtomicU64::new(0);
/// How often a wait for a plan looks at its own cancellation.
const LOOK: Duration = Duration::from_millis(50);

/// A file kept open, until this is dropped: drop it once the file is
/// closed, not when asked to close it, so a plan waits for the close.
#[must_use = "the file counts as kept open only while this lives"]
pub struct KeptOpen(u64);

/// Say that `path` is kept open until the returned guard is dropped;
/// `let_go` closes it when a plan is to take it, on the plan's thread,
/// and must not wait for the window.
pub fn keep_open(path: PathBuf, let_go: impl Fn(PlanId) + Send + Sync + 'static) -> KeptOpen {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    KEPT.lock().push(Kept {
        id,
        path,
        let_go: Arc::new(let_go),
    });
    COUNT.fetch_add(1, Ordering::Release);
    KeptOpen(id)
}

impl KeptOpen {
    /// The holder went on to another file, with the same way of letting go
    /// (a player to the track it had queued).
    pub fn now_at(&self, path: PathBuf) {
        if let Some(kept) = KEPT.lock().iter_mut().find(|kept| kept.id == self.0) {
            kept.path = path;
        }
    }

    /// Another file kept open with the same way of letting go.
    pub fn also(&self, path: PathBuf) -> Option<KeptOpen> {
        let let_go = KEPT
            .lock()
            .iter()
            .find(|kept| kept.id == self.0)
            .map(|kept| Arc::clone(&kept.let_go))?;
        Some(keep_open(path, move |plan| let_go(plan)))
    }
}

impl Drop for KeptOpen {
    fn drop(&mut self) {
        let mut kept = KEPT.lock();
        if let Some(at) = kept.iter().position(|kept| kept.id == self.0) {
            kept.swap_remove(at);
            COUNT.fetch_sub(1, Ordering::Release);
        }
        drop(kept);
        CLOSED.notify_all();
    }
}

/// Whether any file is kept open.
pub fn any() -> bool {
    COUNT.load(Ordering::Acquire) > 0
}

/// The files a running plan let go of (`let_go_of`): taken until this is
/// dropped as the plan ends, what kept them waiting meanwhile.
#[must_use = "what was let go of counts as taken only while this lives"]
pub struct Taking {
    plan: PlanId,
    count: usize,
}

impl Taking {
    /// How many files were let go of.
    pub fn count(&self) -> usize {
        self.count
    }
}

impl Drop for Taking {
    fn drop(&mut self) {
        if self.count == 0 {
            return;
        }
        let mut taking = TAKING.lock();
        taking.retain(|(plan, _)| *plan != self.plan);
        // Under the lock: what `taken` found taken counted before this.
        ENDED.fetch_add(1, Ordering::Release);
        drop(taking);
        TAKEN.notify_all();
    }
}

/// Let go of every file kept open that `takes` names, telling each holder
/// `plan`, and wait until each is closed, `at_most` in all. A holder slower
/// than that is left to it: the step then fails as it would have. Taken
/// until the plan drops what this returns.
pub fn let_go_of(plan: PlanId, takes: impl Fn(&Path) -> bool, at_most: Duration) -> Taking {
    let deadline = Instant::now() + at_most;
    let closing: Vec<(u64, LetGo)> = {
        let kept = KEPT.lock();
        let closing: Vec<&Kept> = kept.iter().filter(|kept| takes(&kept.path)).collect();
        TAKING
            .lock()
            .extend(closing.iter().map(|kept| (plan, kept.path.clone())));
        closing
            .into_iter()
            .map(|kept| (kept.id, Arc::clone(&kept.let_go)))
            .collect()
    };
    // Told outside the lock: closing may take its own locks, and a holder
    // may keep another file meanwhile.
    for (_, let_go) in &closing {
        let_go(plan);
    }
    let ids: std::collections::HashSet<u64> = closing.iter().map(|(id, _)| *id).collect();
    let mut kept = KEPT.lock();
    while kept.iter().any(|kept| ids.contains(&kept.id)) {
        if CLOSED.wait_until(&mut kept, deadline).timed_out() {
            break;
        }
    }
    Taking {
        plan,
        count: closing.len(),
    }
}

/// Wait while a plan still running takes `path` (it let go of it for
/// one), for as long as `cancel` lets, `at_most` in all: what kept it
/// opens it again only once the plan has ended. Whether it is free.
pub fn wait_while_taken(path: &Path, cancel: &CancelToken, at_most: Duration) -> bool {
    let deadline = Instant::now() + at_most;
    let mut taking = TAKING.lock();
    while taking.iter().any(|(_, taken)| taken == path) {
        let now = Instant::now();
        if cancel.is_cancelled() || now >= deadline {
            return false;
        }
        let _ = TAKEN.wait_until(&mut taking, deadline.min(now + LOOK));
    }
    true
}

/// The plan still running that took `path` (it let go of it for one), if
/// one did: what would open it meanwhile lets the plan have it.
pub fn taken_by(path: &Path) -> Option<PlanId> {
    TAKING
        .lock()
        .iter()
        .find(|(_, taken)| taken == path)
        .map(|(plan, _)| *plan)
}

/// A file not opened again while a plan still running takes it (it let go
/// of it for one): what a read that cannot wait fails with (a held reader,
/// a map's tiles), never a damage of the file, and worth asking again
/// once [`ended`] has moved past `ended`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Taken {
    /// [`ended`] when it was found taken.
    pub ended: u64,
}

impl std::fmt::Display for Taken {
    /// Its words: English, or the reader's language while an error's chain
    /// is said.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(
            &crate::words!(
                fs_taken = "it is being moved or removed",
                note = "Why a file could not be read just now: an action is moving or removing it. Begins in lower case: said after other words."
            ),
            f,
        )
    }
}

impl std::error::Error for Taken {}

impl Taken {
    /// The `Taken` an error is, or holds (as an I/O error's own, or as the
    /// cause of one).
    pub fn in_error(error: &anyhow::Error) -> Option<Taken> {
        error.chain().find_map(|cause| {
            cause.downcast_ref::<Taken>().copied().or_else(|| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .and_then(std::io::Error::get_ref)
                    .and_then(|inner| inner.downcast_ref::<Taken>())
                    .copied()
            })
        })
    }

    /// Whether a plan has ended since it was found taken.
    pub fn is_over(self) -> bool {
        ended() != self.ended
    }
}

/// `path` taken by a plan still running, as a [`Taken`]: what would open it
/// meanwhile fails with it.
pub fn taken(path: &Path) -> Option<Taken> {
    let taking = TAKING.lock();
    taking
        .iter()
        .any(|(_, taken)| taken == path)
        .then(|| Taken {
            ended: ENDED.load(Ordering::Acquire),
        })
}

/// How many plans that let go of something have ended.
pub fn ended() -> u64 {
    ENDED.load(Ordering::Acquire)
}

/// How long a holder waits for a plan that took its file before it opens
/// it again all the same, then finding it gone or moved.
pub const TAKEN_AT_MOST: Duration = Duration::from_secs(60);

/// What a helper's manager keeps of the file its helper has open (a
/// reader's engine, an archive's helper): kept open from the helper's
/// start ([`WhileOpen::opening`]) to its end ([`WhileOpen::closed`]). A
/// plan that takes the file cancels the request in flight (each is
/// [`WhileOpen::linked`]) and says so ([`WhileOpen::asked`]): the manager
/// ends its helper, and starts one for the next request once the plan
/// has ended.
pub struct WhileOpen {
    path: PathBuf,
    open: Option<(CancelToken, CancelSignal, KeptOpen)>,
}

impl WhileOpen {
    /// For a helper that opens `path`.
    pub fn new(path: PathBuf) -> Self {
        WhileOpen { path, open: None }
    }

    /// A helper is about to open the file: once no plan takes it (or
    /// `cancel` ends the wait), it counts as kept open.
    pub fn opening(&mut self, cancel: &CancelToken) {
        self.open = None;
        wait_while_taken(&self.path, cancel, TAKEN_AT_MOST);
        let asked = CancelToken::new();
        let signal = asked.subscribe();
        let told = asked.clone();
        let kept = keep_open(self.path.clone(), move |_| told.cancel());
        self.open = Some((asked, signal, kept));
    }

    /// `cancel`, cancelled too when a plan takes the file.
    pub fn linked(&self, cancel: &CancelToken) -> CancelToken {
        match &self.open {
            Some((asked, ..)) => cancel.linked(asked),
            None => cancel.clone(),
        }
    }

    /// Ready once a plan takes the file, for a manager's `select!`.
    pub fn asked(&self) -> crossbeam_channel::Receiver<()> {
        self.open
            .as_ref()
            .map_or_else(crossbeam_channel::never, |(_, signal, _)| {
                signal.receiver().clone()
            })
    }

    /// Whether a plan takes the file.
    pub fn is_asked(&self) -> bool {
        self.open
            .as_ref()
            .is_some_and(|(asked, ..)| asked.is_cancelled())
    }

    /// The helper has ended, its file closed: a plan waiting for it goes on.
    pub fn closed(&mut self) {
        self.open = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// Tests share the process's files kept open: each names its own.
    fn under(test: &str) -> PathBuf {
        PathBuf::from("/kept-open-test").join(test)
    }

    #[test]
    fn a_plan_lets_go_of_what_it_takes_and_waits_for_the_close() {
        let root = under("waits");
        let (told, heard) = mpsc::channel();
        let guard = Arc::new(Mutex::new(None));
        let taken = Arc::clone(&guard);
        *guard.lock() = Some(keep_open(root.join("song.ogg"), move |plan| {
            told.send(plan).unwrap();
            // Closed a moment later, on another thread, as a helper's
            // process ends.
            let taken = Arc::clone(&taken);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                taken.lock().take();
            });
        }));
        let _other = keep_open(root.join("other.ogg"), |_| panic!("not taken"));
        let started = Instant::now();
        let n = let_go_of(
            PlanId(7),
            |path| path.starts_with(root.join("song.ogg")),
            Duration::from_secs(5),
        )
        .count();
        assert_eq!(n, 1);
        assert_eq!(heard.try_recv().unwrap(), PlanId(7));
        assert!(guard.lock().is_none(), "returned once it was closed");
        assert!(started.elapsed() >= Duration::from_millis(50));
    }

    /// What a plan let go of is taken until the plan ends: what kept it
    /// waits for that before opening it again, a wait its cancellation
    /// ends.
    #[test]
    fn what_a_plan_took_is_opened_again_only_once_it_ended() {
        let root = under("taken");
        let song = root.join("it's a song.flac");
        let kept = Arc::new(Mutex::new(None));
        let closes = Arc::clone(&kept);
        *kept.lock() = Some(keep_open(song.clone(), move |_| {
            closes.lock().take();
        }));
        let taking = let_go_of(PlanId(11), |path| path == song, Duration::from_secs(5));
        assert_eq!(taking.count(), 1);
        let never = CancelToken::new();
        assert!(!wait_while_taken(&song, &never, Duration::from_millis(30)));
        assert_eq!(taken_by(&song), Some(PlanId(11)));
        assert_eq!(taken_by(&root.join("other")), None);
        assert!(wait_while_taken(
            &root.join("other"),
            &never,
            Duration::ZERO
        ));
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(!wait_while_taken(&song, &cancelled, Duration::from_secs(5)));
        let (done, ended) = std::sync::mpsc::channel();
        let waiting = song.clone();
        std::thread::spawn(move || {
            let free = wait_while_taken(&waiting, &CancelToken::new(), Duration::from_secs(5));
            done.send(free).unwrap();
        });
        std::thread::sleep(Duration::from_millis(20));
        drop(taking);
        assert_eq!(ended.recv_timeout(Duration::from_secs(5)), Ok(true));
    }

    /// A helper's manager: its request in flight is cancelled and it is told
    /// when a plan takes the file; ended, the plan waits no more.
    #[test]
    fn a_helper_s_manager_is_told_and_its_request_cancelled() {
        let root = under("helper");
        let book = root.join("a book's pages.pdf");
        let mut open = WhileOpen::new(book.clone());
        let request = open.linked(&CancelToken::new());
        assert!(!request.is_cancelled() && !open.is_asked());
        open.opening(&CancelToken::new());
        let request = open.linked(&CancelToken::new());
        let asked = open.asked();
        let started = Instant::now();
        // Not closed: the plan waits as long as it is told to, no longer.
        let taking = let_go_of(PlanId(12), |path| path == book, Duration::from_millis(40));
        assert_eq!(taking.count(), 1);
        assert!(started.elapsed() >= Duration::from_millis(40));
        assert!(request.is_cancelled() && open.is_asked());
        assert!(asked.try_recv().is_ok());
        open.closed();
        drop(taking);
        let started = Instant::now();
        assert_eq!(
            let_go_of(PlanId(13), |path| path == book, Duration::from_secs(5)).count(),
            0
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        // Opened again: kept open again.
        open.opening(&CancelToken::new());
        assert!(!open.is_asked());
        let taking = let_go_of(PlanId(14), |path| path == book, Duration::ZERO);
        assert_eq!(taking.count(), 1);
        assert!(open.is_asked());
    }

    #[test]
    fn a_holder_that_does_not_close_is_waited_for_no_longer_than_asked() {
        let root = under("slow");
        let _kept = keep_open(root.join("film.mkv"), |_| {});
        let started = Instant::now();
        let n = let_go_of(
            PlanId(1),
            |path| path.starts_with(&root),
            Duration::from_millis(30),
        )
        .count();
        assert_eq!(n, 1);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_holder_gone_on_to_another_file_is_let_go_of_there() {
        let root = under("moved");
        let (told, heard) = mpsc::channel();
        let kept = keep_open(root.join("one.flac"), move |plan| {
            told.send(plan).unwrap();
        });
        kept.now_at(root.join("two.flac"));
        let also = kept.also(root.join("three.flac")).unwrap();
        let takes_one = |path: &Path| path == root.join("one.flac");
        assert_eq!(let_go_of(PlanId(2), takes_one, Duration::ZERO).count(), 0);
        let takes_three = |path: &Path| path == root.join("three.flac");
        assert_eq!(let_go_of(PlanId(3), takes_three, Duration::ZERO).count(), 1);
        assert_eq!(
            heard.try_recv().unwrap(),
            PlanId(3),
            "the same way of letting go"
        );
        drop((kept, also));
        assert_eq!(
            let_go_of(PlanId(4), |path| path.starts_with(&root), Duration::ZERO).count(),
            0
        );
    }
}
