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
//! As [`super::wait_let_go`] does for the folders listings hold, this is
//! the process's: a file open is open whoever asks to remove it.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

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

/// Let go of every file kept open that `takes` names, telling each holder
/// `plan`, and wait until each is closed, `at_most` in all; how many were
/// let go of. A holder slower than that is left to it: the step then fails
/// as it would have.
pub fn let_go_of(plan: PlanId, takes: impl Fn(&Path) -> bool, at_most: Duration) -> usize {
    let deadline = Instant::now() + at_most;
    let closing: Vec<(u64, LetGo)> = KEPT
        .lock()
        .iter()
        .filter(|kept| takes(&kept.path))
        .map(|kept| (kept.id, Arc::clone(&kept.let_go)))
        .collect();
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
    closing.len()
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
        );
        assert_eq!(n, 1);
        assert_eq!(heard.try_recv().unwrap(), PlanId(7));
        assert!(guard.lock().is_none(), "returned once it was closed");
        assert!(started.elapsed() >= Duration::from_millis(50));
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
        );
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
        assert_eq!(let_go_of(PlanId(2), takes_one, Duration::ZERO), 0);
        let takes_three = |path: &Path| path == root.join("three.flac");
        assert_eq!(let_go_of(PlanId(3), takes_three, Duration::ZERO), 1);
        assert_eq!(
            heard.try_recv().unwrap(),
            PlanId(3),
            "the same way of letting go"
        );
        drop((kept, also));
        assert_eq!(
            let_go_of(PlanId(4), |path| path.starts_with(&root), Duration::ZERO),
            0
        );
    }
}
