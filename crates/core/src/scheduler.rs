//! A priority work queue: viewport-visible work first, everything cancellable,
//! every task timed so slow providers can be surfaced.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use crate::cancel::CancelToken;

/// Task priority. Lower runs first; within a priority, the most recently
/// submitted task runs first (LIFO), which approximates "what the user is
/// looking at right now".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Needed to paint what is on screen.
    Visible = 0,
    /// Requested by the user but not yet visible.
    Normal = 1,
    /// Speculative or maintenance work.
    Background = 2,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

struct Task {
    priority: Priority,
    seq: u64,
    label: &'static str,
    cancel: CancelToken,
    job: Job,
}

impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.seq == other.seq
    }
}
impl Eq for Task {}
impl PartialOrd for Task {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Task {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap: lower priority value and higher seq win.
        other
            .priority
            .cmp(&self.priority)
            .then(self.seq.cmp(&other.seq))
    }
}

#[derive(Default)]
struct Queue {
    heap: BinaryHeap<Task>,
    /// Jobs taken from the heap and not yet ended, counted under the same
    /// lock, so a job that spawns another is never seen idle in between.
    running: usize,
    shutdown: bool,
}

/// Per-label timing, for surfacing slow providers.
#[derive(Clone, Debug, Default)]
pub struct LabelStats {
    pub runs: u64,
    pub total: Duration,
    pub max: Duration,
    /// Jobs that unwound; the worker survived and the caller saw no result.
    pub panics: u64,
}

struct Inner {
    queue: Mutex<Queue>,
    cv: Condvar,
    /// Told when the last job running ends with none queued.
    idle: Condvar,
    seq: AtomicU64,
    stats: Mutex<std::collections::HashMap<&'static str, LabelStats>>,
    slow_threshold: Duration,
}

/// The shared scheduler. Cheap to clone. When the last clone is dropped
/// the workers finish their current job and exit; queued jobs are dropped
/// unrun.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
    /// Held by handles only, never by workers: its drop is the shutdown.
    _owner: Arc<Owner>,
}

struct Owner(Arc<Inner>);

impl Drop for Owner {
    fn drop(&mut self) {
        let queued = {
            let mut q = self.0.queue.lock();
            q.shutdown = true;
            std::mem::take(&mut q.heap)
        };
        self.0.cv.notify_all();
        // Jobs can own handles and resources: drop them outside the lock.
        drop(queued);
    }
}

impl Scheduler {
    /// Start `workers` threads. Zero means "available parallelism minus one,
    /// clamped to 2..=8": the work is I/O-bound stat and decode, and more
    /// threads mostly cost memory.
    pub fn new(workers: usize) -> Self {
        let workers = if workers == 0 {
            std::thread::available_parallelism()
                .map_or(2, |n| n.get().saturating_sub(1).clamp(2, 8))
        } else {
            workers
        };
        let inner = Arc::new(Inner {
            queue: Mutex::new(Queue::default()),
            cv: Condvar::new(),
            idle: Condvar::new(),
            seq: AtomicU64::new(0),
            stats: Mutex::new(std::collections::HashMap::new()),
            slow_threshold: Duration::from_millis(100),
        });
        for i in 0..workers {
            let inner = Arc::clone(&inner);
            std::thread::Builder::new()
                .name(format!("bibli-worker-{i}"))
                .spawn(move || worker(&inner))
                .expect("spawn worker");
        }
        Scheduler {
            _owner: Arc::new(Owner(Arc::clone(&inner))),
            inner,
        }
    }

    /// Submit a job. It is skipped (never run) if `cancel` fires before it starts.
    pub fn spawn(
        &self,
        priority: Priority,
        label: &'static str,
        cancel: CancelToken,
        job: impl FnOnce() + Send + 'static,
    ) {
        let seq = self.inner.seq.fetch_add(1, AtomicOrdering::Relaxed);
        let mut q = self.inner.queue.lock();
        q.heap.push(Task {
            priority,
            seq,
            label,
            cancel,
            job: Box::new(job),
        });
        drop(q);
        self.inner.cv.notify_one();
    }

    /// Number of tasks waiting to run.
    pub fn pending(&self) -> usize {
        self.inner.queue.lock().heap.len()
    }

    /// Wait until no job is queued or running, for at most `timeout`;
    /// whether it was reached. For headless tests and tools that read what
    /// jobs publish (the volumes a plugin measures on a worker), never the
    /// window's thread.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut q = self.inner.queue.lock();
        while q.running > 0 || !q.heap.is_empty() {
            if self.inner.idle.wait_until(&mut q, deadline).timed_out() {
                return q.running == 0 && q.heap.is_empty();
            }
        }
        true
    }

    /// Timing per label since start.
    pub fn stats(&self) -> Vec<(&'static str, LabelStats)> {
        self.inner
            .stats
            .lock()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }
}

thread_local! {
    static WINDOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a window marked its thread: else bibli runs without one (a
/// script's `bibli cmd`, a test), and nothing applies what listings send
/// unless the caller does.
static HAS_WINDOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Mark the calling thread as the window's: the one thread nothing may
/// keep waiting (ADR-000). The window marks its own at startup.
pub fn mark_window_thread() {
    WINDOW.with(|window| window.set(true));
    HAS_WINDOW.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Whether bibli has a window, whose thread applies what listings send.
pub fn has_window() -> bool {
    HAS_WINDOW.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the calling thread is the window's: a command that may wait
/// (a plugin from outside bibli answering, ADR-045) moves its waiting to
/// a worker there, and waits where it is anywhere else (an agent's call,
/// a script's, a worker's).
pub fn on_window_thread() -> bool {
    WINDOW.with(std::cell::Cell::get)
}

/// What a worker calls once idle for [`RELEASE_AFTER`] after a job: the
/// allocator's way to give back what the jobs freed, set by the binary.
static RELEASE: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// How long a worker waits idle after its last job before it releases:
/// long enough that a burst of jobs never pays for it.
const RELEASE_AFTER: Duration = Duration::from_secs(1);

/// Have each worker, once idle for a moment after running a job, call
/// `release` (`bibli_alloc::collect`): an allocator may keep what a thread
/// freed until that thread allocates again, and a worker can wait for its
/// next job for hours. Process-wide, as the allocator is; set once, by the
/// binary, before or after the scheduler starts.
pub fn release_when_idle(release: fn()) {
    let _ = RELEASE.set(release);
}

fn worker(inner: &Inner) {
    // Whether this worker ran a job since it last released.
    let mut ran = false;
    loop {
        let task = {
            let mut q = inner.queue.lock();
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(t) = q.heap.pop() {
                    q.running += 1;
                    break t;
                }
                match RELEASE.get() {
                    Some(release) if ran => {
                        if inner.cv.wait_for(&mut q, RELEASE_AFTER).timed_out()
                            && q.heap.is_empty()
                            && !q.shutdown
                        {
                            ran = false;
                            parking_lot::MutexGuard::unlocked(&mut q, release);
                        }
                    }
                    _ => inner.cv.wait(&mut q),
                }
            }
        };
        if task.cancel.is_cancelled() {
            // Before the lock: a job can own the last handle, whose drop
            // takes it.
            drop(task);
        } else {
            ran = true;
            run(inner, task);
        }
        let mut q = inner.queue.lock();
        q.running -= 1;
        if q.running == 0 && q.heap.is_empty() {
            inner.idle.notify_all();
        }
    }
}

/// Run one job, timed, its panic caught.
fn run(inner: &Inner, task: Task) {
    let t = Instant::now();
    // A job that panics (a malformed file in a third-party parser, for
    // instance) must not take the worker thread with it: the pool is
    // never respawned, and the caller would wait forever for a result.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task.job));
    let took = t.elapsed();
    if let Err(payload) = outcome {
        let message = panic_message(&*payload);
        tracing::error!(label = task.label, "job panicked: {message}");
        inner.stats.lock().entry(task.label).or_default().panics += 1;
    }
    let mut stats = inner.stats.lock();
    let s = stats.entry(task.label).or_default();
    s.runs += 1;
    s.total += took;
    s.max = s.max.max(took);
    if took > inner.slow_threshold {
        tracing::warn!(label = task.label, ms = took.as_millis(), "slow task");
    }
}

/// What a caught panic said, for the log.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_the_last_handle_stops_the_workers() {
        let scheduler = Scheduler::new(2);
        let (tx, rx) = crossbeam_channel::bounded(1);
        scheduler.spawn(Priority::Normal, "test", CancelToken::new(), move || {
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let inner = Arc::downgrade(&scheduler.inner);
        drop(scheduler);
        let deadline = Instant::now() + Duration::from_secs(5);
        while inner.strong_count() > 0 {
            assert!(Instant::now() < deadline, "workers still hold the queue");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn idle_waits_for_running_jobs_and_those_they_spawn() {
        let scheduler = Scheduler::new(2);
        assert!(scheduler.wait_idle(Duration::ZERO), "nothing was spawned");
        // A job held until told, which then spawns another.
        let (go, wait) = crossbeam_channel::bounded::<()>(1);
        let ended = Arc::new(AtomicU64::new(0));
        let (count, next) = (Arc::clone(&ended), scheduler.clone());
        scheduler.spawn(Priority::Normal, "test", CancelToken::new(), move || {
            wait.recv().unwrap();
            next.spawn(
                Priority::Background,
                "test",
                CancelToken::new(),
                move || {
                    std::thread::sleep(Duration::from_millis(20));
                    count.fetch_add(1, AtomicOrdering::SeqCst);
                },
            );
        });
        assert!(
            !scheduler.wait_idle(Duration::from_millis(50)),
            "a job runs"
        );
        go.send(()).unwrap();
        assert!(scheduler.wait_idle(Duration::from_secs(5)));
        assert_eq!(
            ended.load(AtomicOrdering::SeqCst),
            1,
            "and the one it spawned"
        );
        // A job cancelled before it starts is skipped, not waited for.
        let cancelled = CancelToken::new();
        cancelled.cancel();
        let count = Arc::clone(&ended);
        scheduler.spawn(Priority::Normal, "test", cancelled, move || {
            count.fetch_add(1, AtomicOrdering::SeqCst);
        });
        assert!(scheduler.wait_idle(Duration::from_secs(5)));
        assert_eq!(ended.load(AtomicOrdering::SeqCst), 1);
    }

    // The hook is process-wide: this test is the only one in its process
    // that sets it (nextest runs each test in a process of its own).
    #[test]
    fn a_worker_idle_after_a_job_releases_once() {
        static RELEASED: AtomicU64 = AtomicU64::new(0);
        fn count() {
            RELEASED.fetch_add(1, AtomicOrdering::SeqCst);
        }
        let released = || RELEASED.load(AtomicOrdering::SeqCst);
        let until = |n: u64| {
            let deadline = Instant::now() + RELEASE_AFTER + Duration::from_secs(5);
            while released() < n {
                assert!(Instant::now() < deadline, "released {} times", released());
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        release_when_idle(count);
        let scheduler = Scheduler::new(4);
        let run = || {
            let (tx, rx) = crossbeam_channel::bounded(1);
            scheduler.spawn(Priority::Normal, "test", CancelToken::new(), move || {
                tx.send(()).unwrap();
            });
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
        };
        run();
        until(1);
        // Only the worker that ran the job, and once while it stays idle.
        std::thread::sleep(RELEASE_AFTER * 2);
        assert_eq!(released(), 1);
        run();
        until(2);
    }
}
