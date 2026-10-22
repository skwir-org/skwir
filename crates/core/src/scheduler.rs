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

/// Where a job runs (ADR-077): the pool's own queue, or a remote's lane,
/// which runs at most [`LANE_RUNS`] of its jobs at once. A server that
/// stops answering holds those, never every worker; the rest of its jobs
/// wait their turn without holding one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lane(Option<u64>);

impl Lane {
    /// The pool's own queue: work on this machine.
    pub const SHARED: Lane = Lane(None);

    /// A remote's lane, by a key of its own (`fs::reach`).
    pub const fn keyed(key: u64) -> Lane {
        Lane(Some(key))
    }

    pub const fn is_shared(self) -> bool {
        self.0.is_none()
    }
}

/// How many jobs of one lane run at once, in a pool of three workers or
/// more; in a smaller one, one fewer than its workers, so a remote never
/// holds them all.
pub const LANE_RUNS: usize = 2;

/// A job running this long while work waits and no worker is free holds
/// its worker as if for good (a remote that stopped answering, a decoder
/// that loops): another worker is started in its place, up to the pool's
/// size again, and stops once idle for [`LINGER`].
pub const STUCK: Duration = Duration::from_secs(1);

/// How long a worker started in place of a held one waits for work
/// before it ends.
const LINGER: Duration = Duration::from_secs(2);

/// A worker's stack, as large as the window thread's: what a worker reads
/// recurses as deep as a file nests, within `limits::XML_PARSED_DEPTH`
/// for XML (the parser recurses once a level), and a debug build's frames
/// are ten times a release's. Only what is used is ever committed.
pub const WORKER_STACK: usize = 8 << 20;

struct Task {
    priority: Priority,
    seq: u64,
    label: &'static str,
    cancel: CancelToken,
    lane: Lane,
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
    /// The lanes with jobs running or waiting their turn.
    lanes: std::collections::HashMap<u64, LaneQueue>,
    /// When each running job began, and its lane, by its sequence number.
    began: std::collections::HashMap<u64, (Lane, Instant)>,
    /// Workers waiting for a job.
    waiting: usize,
    /// Workers started in place of held ones (`STUCK`).
    extra: usize,
    /// How many jobs of one lane run at once: `LANE_RUNS`, but always one
    /// worker fewer than the pool's, so no remote takes them all.
    lane_runs: usize,
    /// A look in `STUCK` at whether a worker is held is planned: work
    /// waits, no worker is free, none held long enough yet.
    looking: bool,
}

/// A lane's jobs running, and those waiting their turn.
#[derive(Default)]
struct LaneQueue {
    running: usize,
    parked: BinaryHeap<Task>,
}

impl Queue {
    /// The next job to run, its lane's turn counted; cancelled ones met on
    /// the way go to `dropped`, to be dropped outside the lock.
    fn take(&mut self, dropped: &mut Vec<Task>) -> Option<Task> {
        while let Some(task) = self.heap.pop() {
            if let Some(key) = task.lane.0 {
                let lane = self.lanes.entry(key).or_default();
                if lane.running >= self.lane_runs {
                    if task.cancel.is_cancelled() {
                        dropped.push(task);
                    } else {
                        // A lane held (a server that stopped answering)
                        // parks what is asked of it meanwhile: what was
                        // cancelled since goes now and then, outside the
                        // lock.
                        if lane.parked.len() >= 64 && lane.parked.len().is_power_of_two() {
                            let (gone, kept): (Vec<Task>, Vec<Task>) =
                                std::mem::take(&mut lane.parked)
                                    .into_iter()
                                    .partition(|t| t.cancel.is_cancelled());
                            lane.parked = kept.into();
                            dropped.extend(gone);
                        }
                        lane.parked.push(task);
                    }
                    continue;
                }
                lane.running += 1;
            }
            self.running += 1;
            self.began.insert(task.seq, (task.lane, Instant::now()));
            return Some(task);
        }
        None
    }

    /// The job `seq` of `lane` ended: the next of its lane may run.
    fn ended(&mut self, seq: u64, lane: Lane) -> bool {
        self.running -= 1;
        self.began.remove(&seq);
        let Some(key) = lane.0 else {
            return false;
        };
        let Some(queue) = self.lanes.get_mut(&key) else {
            return false;
        };
        queue.running -= 1;
        let next = queue.parked.pop();
        if queue.running == 0 && queue.parked.is_empty() {
            self.lanes.remove(&key);
        }
        match next {
            Some(task) => {
                self.heap.push(task);
                true
            }
            None => false,
        }
    }

    /// Jobs waiting: in the queue, or for their lane's turn.
    fn waiting_jobs(&self) -> usize {
        self.heap.len() + self.lanes.values().map(|l| l.parked.len()).sum::<usize>()
    }

    /// Whether no job of this machine's (the shared lane) runs or waits:
    /// a remote's may still, held by its server.
    fn idle_here(&self) -> bool {
        !self.began.values().any(|(lane, _)| lane.is_shared())
            && !self.heap.iter().any(|task| task.lane.is_shared())
    }

    /// Whether to start a worker in place of one held: work waits, no
    /// worker is free, and fewer stand in than jobs have run past `STUCK`.
    /// Whether to plan a look, in `STUCK`, at whether a worker is held:
    /// work waits and no worker is free, but none has run long enough to be
    /// stood in for yet.
    fn wants_a_look(&mut self, workers: usize) -> bool {
        if self.looking
            || self.shutdown
            || self.waiting > 0
            || self.heap.is_empty()
            || self.extra >= workers
        {
            return false;
        }
        self.looking = true;
        true
    }

    fn wants_another(&mut self, workers: usize) -> bool {
        if self.shutdown || self.waiting > 0 || self.heap.is_empty() || self.extra >= workers {
            return false;
        }
        let held = self
            .began
            .values()
            .filter(|(_, began)| began.elapsed() >= STUCK)
            .count();
        if self.extra >= held {
            return false;
        }
        self.extra += 1;
        true
    }

    /// After a look that stood in for none: how long until a job running
    /// now will have run `STUCK`, while work still waits and no worker is
    /// free. `None` ends the looking (the next job asked plans another).
    fn look_again_in(&mut self, workers: usize) -> Option<Duration> {
        let wanted =
            !self.shutdown && self.waiting == 0 && !self.heap.is_empty() && self.extra < workers;
        let ran = self
            .began
            .values()
            .map(|(_, began)| began.elapsed())
            .filter(|ran| *ran < STUCK)
            .max()
            .filter(|_| wanted);
        if ran.is_none() {
            self.looking = false;
        }
        ran.map(|ran| STUCK.saturating_sub(ran))
    }
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
    /// The pool's own workers, besides those standing in (`STUCK`).
    workers: usize,
}

/// The shared scheduler. Cheap to clone. When the last clone is dropped
/// the workers finish their current job and exit; queued jobs are dropped
/// unrun.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
    /// Held by handles only, never by workers: its drop is the shutdown.
    _owner: Arc<Owner>,
    /// How many workers it has.
    workers: usize,
}

struct Owner(Arc<Inner>);

impl Drop for Owner {
    fn drop(&mut self) {
        let queued = {
            let mut q = self.0.queue.lock();
            q.shutdown = true;
            let mut queued: Vec<Task> = std::mem::take(&mut q.heap).into_vec();
            for lane in q.lanes.values_mut() {
                queued.extend(std::mem::take(&mut lane.parked));
            }
            queued
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
            queue: Mutex::new(Queue {
                lane_runs: LANE_RUNS.min(workers.saturating_sub(1)).max(1),
                ..Queue::default()
            }),
            cv: Condvar::new(),
            idle: Condvar::new(),
            seq: AtomicU64::new(0),
            stats: Mutex::new(std::collections::HashMap::new()),
            slow_threshold: Duration::from_millis(100),
            workers,
        });
        {
            let mut pools = POOLS.lock();
            pools.retain(|pool| pool.strong_count() > 0);
            pools.push(Arc::downgrade(&inner));
        }
        for i in 0..workers {
            let inner = Arc::clone(&inner);
            std::thread::Builder::new()
                .name(format!("skwir-worker-{i}"))
                .stack_size(WORKER_STACK)
                .spawn(move || worker(&inner, false))
                .expect("spawn worker");
        }
        Scheduler {
            _owner: Arc::new(Owner(Arc::clone(&inner))),
            inner,
            workers,
        }
    }

    /// How many workers it runs jobs on: what work that may hold one long
    /// (a thumbnailer) keeps some of free.
    pub fn workers(&self) -> usize {
        self.workers
    }

    /// Submit a job. It is skipped (never run) if `cancel` fires before it starts.
    pub fn spawn(
        &self,
        priority: Priority,
        label: &'static str,
        cancel: CancelToken,
        job: impl FnOnce() + Send + 'static,
    ) {
        self.spawn_in(Lane::SHARED, priority, label, cancel, job);
    }

    /// Submit a job to `lane`: work that reads a path runs in the lane of
    /// its remote (`fs::reach::lane`), the shared one elsewhere. Skipped
    /// (never run) if `cancel` fires before it starts.
    pub fn spawn_in(
        &self,
        lane: Lane,
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
            lane,
            job: Box::new(job),
        });
        let another = q.wants_another(self.inner.workers);
        let look = !another && q.wants_a_look(self.inner.workers);
        drop(q);
        self.inner.cv.notify_one();
        if another {
            start_standing_in(&self.inner);
        } else if look {
            look_later(&self.inner);
        }
    }

    /// Number of tasks waiting to run, their lane's turn included.
    pub fn pending(&self) -> usize {
        self.inner.queue.lock().waiting_jobs()
    }

    /// How long the longest job of `lane` running has run: a remote that
    /// stopped answering says so, a moment after (`None` while none runs).
    pub fn stalled(&self, lane: Lane) -> Option<Duration> {
        self.inner
            .queue
            .lock()
            .began
            .values()
            .filter(|(l, _)| *l == lane)
            .map(|(_, began)| began.elapsed())
            .max()
    }

    /// Wait until no job is queued or running, for at most `timeout`;
    /// whether it was reached. For headless tests and tools that read what
    /// jobs publish (the volumes a plugin measures on a worker), never the
    /// window's thread.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut q = self.inner.queue.lock();
        while q.running > 0 || q.waiting_jobs() > 0 {
            if self.inner.idle.wait_until(&mut q, deadline).timed_out() {
                return q.running == 0 && q.waiting_jobs() == 0;
            }
        }
        true
    }

    /// [`Scheduler::wait_idle`] for the jobs of this machine's alone: a
    /// remote's lane may wait for good on a server that stopped answering
    /// (ADR-077), which nothing a test reads here waits on.
    pub fn wait_idle_here(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut q = self.inner.queue.lock();
        while !q.idle_here() {
            if self.inner.idle.wait_until(&mut q, deadline).timed_out() {
                return q.idle_here();
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
    /// The workers the window's thread hands what it lets go of
    /// ([`let_go`]).
    static LET_GO: std::cell::RefCell<Option<std::sync::Weak<Inner>>> =
        const { std::cell::RefCell::new(None) };
    /// The token of the job a worker runs now ([`job_cancelled`]).
    static JOB: std::cell::RefCell<Option<CancelToken>> = const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// The job the calling worker runs now: its pool and sequence number,
    /// for [`progress`].
    static RUNNING: std::cell::RefCell<Option<(std::sync::Weak<Inner>, u64)>> =
        const { std::cell::RefCell::new(None) };
}

/// Say that the job the calling thread runs goes on: a job that serves
/// several requests in turn (a remote's thumbnails) is not held, however
/// long it has run (`Scheduler::stalled`). Nothing outside a job.
pub fn progress() {
    let _ = RUNNING.try_with(|running| {
        if let Some((pool, seq)) = &*running.borrow()
            && let Some(inner) = pool.upgrade()
            && let Some(began) = inner.queue.lock().began.get_mut(seq)
        {
            began.1 = Instant::now();
        }
    });
}

/// Whether the job the calling thread runs was cancelled since it began:
/// code a job calls without a token of its own (a command dispatched on a
/// worker, a preview superseded as the user types) stops early. False
/// outside a job.
pub fn job_cancelled() -> bool {
    JOB.try_with(|job| job.borrow().as_ref().is_some_and(CancelToken::is_cancelled))
        .unwrap_or(false)
}

/// Whether a window marked its thread: else skwir runs without one (a
/// script's `skwir cmd`, a test), and nothing applies what listings send
/// unless the caller does.
static HAS_WINDOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Mark the calling thread as the window's: the one thread nothing may
/// keep waiting (ADR-000), which hands `scheduler` what it lets go of.
/// The window marks its own at startup.
pub fn mark_window_thread(scheduler: &Scheduler) {
    WINDOW.with(|window| window.set(true));
    LET_GO.with(|workers| *workers.borrow_mut() = Some(Arc::downgrade(&scheduler.inner)));
    HAS_WINDOW.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Free `value` on a worker when the calling thread is the window's, here
/// anywhere else: a listing of a hundred thousand entries takes
/// milliseconds to free, and the window's thread lets go of one under the
/// workspace's lock each time a pane shows another folder.
pub fn let_go<T: Send + 'static>(value: T) {
    let workers = LET_GO
        .try_with(|workers| workers.borrow().as_ref().and_then(std::sync::Weak::upgrade))
        .ok()
        .flatten();
    let Some(inner) = workers else {
        return drop(value);
    };
    let mut q = inner.queue.lock();
    if q.shutdown {
        drop(q);
        return drop(value);
    }
    let seq = inner.seq.fetch_add(1, AtomicOrdering::Relaxed);
    q.heap.push(Task {
        priority: Priority::Background,
        seq,
        label: "let_go",
        cancel: CancelToken::new(),
        lane: Lane::SHARED,
        job: Box::new(move || drop(value)),
    });
    drop(q);
    inner.cv.notify_one();
}

/// Whether skwir has a window, whose thread applies what listings send.
pub fn has_window() -> bool {
    HAS_WINDOW.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the calling thread is the window's: a command that may wait
/// (a plugin from outside skwir answering, ADR-045) moves its waiting to
/// a worker there, and waits where it is anywhere else (an agent's call,
/// a script's, a worker's).
pub fn on_window_thread() -> bool {
    WINDOW.with(std::cell::Cell::get)
}

/// What a worker calls once idle for [`RELEASE_AFTER`] after a job: the
/// allocator's way to give back what the jobs freed, set by the binary.
static RELEASE: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// How many times every worker was asked to release (`release_on_workers`).
static RELEASE_ASKED: AtomicU64 = AtomicU64::new(0);

/// The pools running, whose idle workers `release_on_workers` wakes.
static POOLS: Mutex<Vec<std::sync::Weak<Inner>>> = Mutex::new(Vec::new());

/// How long a worker waits idle after its last job before it releases:
/// long enough that a burst of jobs never pays for it.
const RELEASE_AFTER: Duration = Duration::from_secs(1);

/// Have each worker, once idle for a moment after running a job, call
/// `release` (`skwir_alloc::collect`): an allocator may keep what a thread
/// freed until that thread allocates again, and a worker can wait for its
/// next job for hours. Process-wide, as the allocator is; set once, by the
/// binary, before or after the scheduler starts.
pub fn release_when_idle(release: fn()) {
    let _ = RELEASE.set(release);
}

/// Give back now what the calling thread freed (the hook
/// `release_when_idle` set): a thread of a plugin's own, about to wait
/// long after a burst of work (a scan, a write), which no worker's idle
/// release covers.
pub fn release_now() {
    if let Some(release) = RELEASE.get() {
        release();
    }
}

/// Have every worker release once idle, as after a job. What another
/// thread frees of what workers allocated stays with the workers until
/// they release, and an idle worker may wait for its next job for hours:
/// call this after letting go of something large that workers built (the
/// grammars code was colored with, `code_preview`).
pub fn release_on_workers() {
    RELEASE_ASKED.fetch_add(1, AtomicOrdering::Relaxed);
    let pools: Vec<_> = POOLS
        .lock()
        .iter()
        .filter_map(std::sync::Weak::upgrade)
        .collect();
    for inner in pools {
        // Under the queue's lock: a worker sees the request before it
        // waits, or is waiting and is woken.
        let _queue = inner.queue.lock();
        inner.cv.notify_all();
    }
}

/// Look again in `STUCK` at whether a worker is held while work waits, and
/// stand in for it: work queued while every worker had only just begun
/// would otherwise wait for the next job asked. A look planned before the
/// job it finds began (a worker took it once free) finds it too young:
/// it looks again when that job will have run `STUCK`.
fn look_later(inner: &Arc<Inner>) {
    let held = Arc::downgrade(inner);
    let started = std::thread::Builder::new()
        .name("skwir-workers-look".into())
        .spawn(move || {
            let mut wait = STUCK;
            loop {
                std::thread::sleep(wait);
                let Some(inner) = held.upgrade() else { return };
                let mut q = inner.queue.lock();
                if q.wants_another(inner.workers) {
                    q.looking = false;
                    drop(q);
                    start_standing_in(&inner);
                    return;
                }
                match q.look_again_in(inner.workers) {
                    Some(after) => wait = after,
                    None => return,
                }
            }
        });
    if started.is_err() {
        inner.queue.lock().looking = false;
    }
}

/// Start a worker in place of one a job holds (`STUCK`).
fn start_standing_in(inner: &Arc<Inner>) {
    let inner_for = Arc::clone(inner);
    let started = std::thread::Builder::new()
        .name("skwir-worker-more".into())
        .stack_size(WORKER_STACK)
        .spawn(move || worker(&inner_for, true));
    if let Err(error) = started {
        tracing::warn!("cannot start a worker in place of a held one: {error}");
        inner.queue.lock().extra -= 1;
    }
}

/// A worker: the pool's own (`standing_in` false) runs until the pool is
/// dropped; one started in place of a held one ends once idle for
/// [`LINGER`].
fn worker(inner: &Arc<Inner>, standing_in: bool) {
    // Whether this worker ran a job, or was asked to release, since it
    // last released.
    let mut ran = false;
    // The requests to release it has seen (`release_on_workers`): one made
    // before it started is seen too, and costs one release.
    let mut asked = 0;
    let mut dropped = Vec::new();
    loop {
        let task = {
            let mut q = inner.queue.lock();
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(t) = q.take(&mut dropped) {
                    break t;
                }
                if !dropped.is_empty() {
                    // Cancelled while waiting their lane's turn: their
                    // drops may take handles, outside the lock.
                    parking_lot::MutexGuard::unlocked(&mut q, || dropped.clear());
                    continue;
                }
                if standing_in {
                    q.waiting += 1;
                    let timed_out = inner.cv.wait_for(&mut q, LINGER).timed_out();
                    q.waiting -= 1;
                    if timed_out && q.heap.is_empty() {
                        q.extra -= 1;
                        drop(q);
                        if ran {
                            release_now();
                        }
                        return;
                    }
                    continue;
                }
                let requests = RELEASE_ASKED.load(AtomicOrdering::Relaxed);
                if requests != asked {
                    asked = requests;
                    ran = true;
                }
                q.waiting += 1;
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
                q.waiting -= 1;
            }
        };
        let (seq, lane) = (task.seq, task.lane);
        if task.cancel.is_cancelled() {
            // Before the lock: a job can own the last handle, whose drop
            // takes it.
            drop(task);
        } else {
            ran = true;
            run(inner, task);
        }
        let mut q = inner.queue.lock();
        if q.ended(seq, lane) {
            inner.cv.notify_one();
        }
        if (q.running == 0 && q.waiting_jobs() == 0) || q.idle_here() {
            inner.idle.notify_all();
        }
    }
}

/// Run one job, timed, its panic caught.
fn run(inner: &Arc<Inner>, task: Task) {
    let t = Instant::now();
    JOB.with(|job| *job.borrow_mut() = Some(task.cancel.clone()));
    RUNNING.with(|running| *running.borrow_mut() = Some((Arc::downgrade(inner), task.seq)));
    // A job that panics (a malformed file in a third-party parser, for
    // instance) must not take the worker thread with it: the pool is
    // never respawned, and the caller would wait forever for a result.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task.job));
    JOB.with(|job| job.borrow_mut().take());
    RUNNING.with(|running| running.borrow_mut().take());
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

    /// What a job calls sees its job cancelled, and stops early: a
    /// command dispatched on a worker has no token of its own.
    #[test]
    fn a_job_sees_its_own_cancellation() {
        assert!(!job_cancelled(), "outside a job");
        let scheduler = Scheduler::new(2);
        let cancel = CancelToken::new();
        let (started, began) = crossbeam_channel::bounded(1);
        let (tx, rx) = crossbeam_channel::bounded(1);
        scheduler.spawn(Priority::Normal, "test", cancel.clone(), move || {
            let before = job_cancelled();
            started.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !job_cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            tx.send((before, job_cancelled())).unwrap();
        });
        began.recv_timeout(Duration::from_secs(5)).unwrap();
        cancel.cancel();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            (false, true)
        );
        // The next job on that worker begins uncancelled.
        let (tx, rx) = crossbeam_channel::bounded(2);
        for _ in 0..2 {
            let tx = tx.clone();
            scheduler.spawn(Priority::Normal, "test", CancelToken::new(), move || {
                tx.send(job_cancelled()).unwrap();
            });
        }
        assert!(!rx.recv_timeout(Duration::from_secs(5)).unwrap());
        assert!(!rx.recv_timeout(Duration::from_secs(5)).unwrap());
    }

    /// Waiting for this machine's jobs leaves out a remote's lane held by
    /// its server: idle here once the shared jobs ended, not idle at all.
    #[test]
    fn idle_here_leaves_out_a_held_remote() {
        let scheduler = Scheduler::new(2);
        let (release, held) = crossbeam_channel::bounded::<()>(0);
        let (started, begun) = crossbeam_channel::bounded(1);
        scheduler.spawn_in(
            Lane::keyed(9),
            Priority::Background,
            "held",
            CancelToken::new(),
            move || {
                started.send(()).unwrap();
                let _ = held.recv();
            },
        );
        begun.recv_timeout(Duration::from_secs(5)).unwrap();
        let (tx, rx) = crossbeam_channel::bounded(1);
        scheduler.spawn(Priority::Normal, "here", CancelToken::new(), move || {
            std::thread::sleep(Duration::from_millis(50));
            tx.send(()).unwrap();
        });
        assert!(scheduler.wait_idle_here(Duration::from_secs(5)));
        assert!(rx.try_recv().is_ok(), "the shared job ended first");
        assert!(!scheduler.wait_idle(Duration::from_millis(100)));
        drop(release);
        assert!(scheduler.wait_idle(Duration::from_secs(5)));
    }

    /// A remote that stops answering holds its own lane's jobs, never the
    /// others': at most `LANE_RUNS` of a lane run at once, while the
    /// shared queue and another remote's lane go on (ADR-077).
    #[test]
    fn a_held_lane_holds_only_its_own_jobs() {
        let scheduler = Scheduler::new(4);
        let lane = Lane::keyed(7);
        let (release, held) = crossbeam_channel::bounded::<()>(0);
        let ran = Arc::new(AtomicU64::new(0));
        for _ in 0..5 {
            let (held, ran) = (held.clone(), Arc::clone(&ran));
            scheduler.spawn_in(
                lane,
                Priority::Visible,
                "held",
                CancelToken::new(),
                move || {
                    ran.fetch_add(1, AtomicOrdering::SeqCst);
                    let _ = held.recv();
                },
            );
        }
        let (tx, rx) = crossbeam_channel::bounded(2);
        let here = tx.clone();
        scheduler.spawn(
            Priority::Background,
            "here",
            CancelToken::new(),
            move || {
                here.send("here").unwrap();
            },
        );
        scheduler.spawn_in(
            Lane::keyed(8),
            Priority::Background,
            "elsewhere",
            CancelToken::new(),
            move || tx.send("elsewhere").unwrap(),
        );
        let mut said = [
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        ];
        said.sort_unstable();
        assert_eq!(said, ["elsewhere", "here"]);
        assert_eq!(ran.load(AtomicOrdering::SeqCst), LANE_RUNS as u64);
        assert_eq!(scheduler.pending(), 5 - LANE_RUNS);
        assert!(scheduler.stalled(lane).is_some());
        assert!(scheduler.stalled(Lane::keyed(9)).is_none());
        // The server answers again: the lane's jobs end, the rest run.
        drop(release);
        assert!(scheduler.wait_idle(Duration::from_secs(5)));
        assert_eq!(ran.load(AtomicOrdering::SeqCst), 5);
        assert!(scheduler.stalled(lane).is_none());
    }

    /// Work queued while every worker had only just begun is stood in for
    /// all the same, a moment later: no further job need be asked.
    #[test]
    fn work_queued_behind_new_jobs_is_stood_in_for_later() {
        let scheduler = Scheduler::new(1);
        let (release, held) = crossbeam_channel::bounded::<()>(0);
        let (began, has_begun) = crossbeam_channel::bounded(1);
        scheduler.spawn(Priority::Normal, "held", CancelToken::new(), move || {
            began.send(()).unwrap();
            let _ = held.recv();
        });
        has_begun.recv_timeout(Duration::from_secs(5)).unwrap();
        let (tx, rx) = crossbeam_channel::bounded(1);
        scheduler.spawn(Priority::Normal, "next", CancelToken::new(), move || {
            tx.send(()).unwrap();
        });
        rx.recv_timeout(STUCK + Duration::from_secs(5))
            .expect("a worker stood in, though nothing more was asked");
        drop(release);
        assert!(scheduler.wait_idle(Duration::from_secs(5)));
    }

    /// A look planned before the job it finds began (asked while another
    /// still ran) looks again once that job has run `STUCK`: under load
    /// it found it too young, planned nothing more, and the work queued
    /// meanwhile waited for good.
    #[test]
    fn a_look_too_early_looks_again() {
        let scheduler = Scheduler::new(1);
        let (end_first, first_ends) = crossbeam_channel::bounded::<()>(0);
        let (began, has_begun) = crossbeam_channel::bounded(1);
        scheduler.spawn(Priority::Normal, "first", CancelToken::new(), move || {
            began.send(()).unwrap();
            let _ = first_ends.recv();
        });
        has_begun.recv_timeout(Duration::from_secs(5)).unwrap();
        // Queued behind the first: a look is planned in `STUCK`.
        let (release, held) = crossbeam_channel::bounded::<()>(0);
        let (began, has_begun) = crossbeam_channel::bounded(1);
        scheduler.spawn(Priority::Normal, "held", CancelToken::new(), move || {
            began.send(()).unwrap();
            let _ = held.recv();
        });
        // It begins half of `STUCK` later, younger than `STUCK` at the look.
        std::thread::sleep(STUCK / 2);
        drop(end_first);
        has_begun.recv_timeout(Duration::from_secs(5)).unwrap();
        let (tx, rx) = crossbeam_channel::bounded(1);
        scheduler.spawn(Priority::Normal, "next", CancelToken::new(), move || {
            tx.send(()).unwrap();
        });
        rx.recv_timeout(STUCK * 2 + Duration::from_secs(5))
            .expect("a worker stood in once the held job had run long enough");
        drop(release);
        assert!(scheduler.wait_idle(Duration::from_secs(5)));
    }

    /// A pool of two keeps a worker from a remote's lane: a server that
    /// stops answering never holds all of a small pool, even before a
    /// worker is stood in for.
    #[test]
    fn a_lane_never_takes_every_worker() {
        let scheduler = Scheduler::new(2);
        let lane = Lane::keyed(7);
        let (release, held) = crossbeam_channel::bounded::<()>(0);
        let ran = Arc::new(AtomicU64::new(0));
        for _ in 0..3 {
            let (held, ran) = (held.clone(), Arc::clone(&ran));
            scheduler.spawn_in(
                lane,
                Priority::Visible,
                "held",
                CancelToken::new(),
                move || {
                    ran.fetch_add(1, AtomicOrdering::SeqCst);
                    let _ = held.recv();
                },
            );
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        scheduler.spawn(
            Priority::Background,
            "here",
            CancelToken::new(),
            move || {
                tx.send(()).unwrap();
            },
        );
        rx.recv_timeout(STUCK / 2)
            .expect("a worker was left for this machine's work");
        assert_eq!(ran.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(scheduler.pending(), 2);
        drop(release);
        assert!(scheduler.wait_idle(Duration::from_secs(5)));
        assert_eq!(ran.load(AtomicOrdering::SeqCst), 3);
    }

    /// A job that says it goes on is not taken as held: a remote's
    /// thumbnails, made one after another in one job, do not make its
    /// folder say its server stopped answering.
    #[test]
    fn a_job_that_goes_on_is_not_stalled() {
        // Outside a job, it says nothing.
        progress();
        let scheduler = Scheduler::new(2);
        let lane = Lane::keyed(7);
        let (step, steps) = crossbeam_channel::bounded::<()>(0);
        let (done, stepped) = crossbeam_channel::bounded::<()>(0);
        scheduler.spawn_in(
            lane,
            Priority::Visible,
            "steps",
            CancelToken::new(),
            move || {
                while steps.recv().is_ok() {
                    progress();
                    done.send(()).unwrap();
                }
            },
        );
        for _ in 0..3 {
            std::thread::sleep(Duration::from_millis(400));
            step.send(()).unwrap();
            stepped.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let stalled = scheduler.stalled(lane).expect("it runs");
        assert!(stalled < Duration::from_millis(600), "{stalled:?}");
        drop(step);
        assert!(scheduler.wait_idle(Duration::from_secs(5)));
    }

    /// The jobs waiting their lane's turn are let go of with the pool.
    #[test]
    fn a_dropped_pool_lets_go_of_parked_jobs() {
        let token = Arc::new(());
        let (release, held) = crossbeam_channel::bounded::<()>(0);
        let ran = Arc::new(AtomicU64::new(0));
        let scheduler = Scheduler::new(3);
        let lane = Lane::keyed(7);
        for _ in 0..4 {
            let (held, ran, token) = (held.clone(), Arc::clone(&ran), Arc::clone(&token));
            scheduler.spawn_in(
                lane,
                Priority::Visible,
                "held",
                CancelToken::new(),
                move || {
                    ran.fetch_add(1, AtomicOrdering::SeqCst);
                    let _ = held.recv();
                    drop(token);
                },
            );
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while ran.load(AtomicOrdering::SeqCst) < LANE_RUNS as u64 {
            assert!(Instant::now() < deadline, "the lane's jobs began");
            std::thread::yield_now();
        }
        drop(scheduler);
        assert_eq!(
            Arc::strong_count(&token),
            1 + LANE_RUNS,
            "only the running jobs hold it"
        );
        drop(release);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Arc::strong_count(&token) > 1 {
            assert!(Instant::now() < deadline, "the running jobs ended");
            std::thread::yield_now();
        }
        assert_eq!(ran.load(AtomicOrdering::SeqCst), LANE_RUNS as u64);
    }

    /// A job that holds its worker past `STUCK` while work waits is stood
    /// in for: the pool keeps answering, and the worker started for it
    /// ends once idle.
    #[test]
    fn a_held_worker_is_stood_in_for() {
        let scheduler = Scheduler::new(2);
        let (release, held) = crossbeam_channel::bounded::<()>(0);
        for _ in 0..2 {
            let held = held.clone();
            scheduler.spawn(Priority::Normal, "held", CancelToken::new(), move || {
                let _ = held.recv();
            });
        }
        std::thread::sleep(STUCK + Duration::from_millis(200));
        let (tx, rx) = crossbeam_channel::bounded(1);
        scheduler.spawn(Priority::Normal, "next", CancelToken::new(), move || {
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(5))
            .expect("a worker stood in for a held one");
        drop(release);
        assert!(scheduler.wait_idle(Duration::from_secs(5)));
        let deadline = Instant::now() + LINGER + Duration::from_secs(5);
        while scheduler.inner.queue.lock().extra > 0 {
            assert!(Instant::now() < deadline, "the stand-in never ended");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

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

    // Sets the process-wide hook, alone in its process like the test above.
    #[test]
    fn workers_asked_to_release_each_release_once_idle() {
        static RELEASED: AtomicU64 = AtomicU64::new(0);
        fn count() {
            RELEASED.fetch_add(1, AtomicOrdering::SeqCst);
        }
        let released = || RELEASED.load(AtomicOrdering::SeqCst);
        release_when_idle(count);
        let scheduler = Scheduler::new(3);
        // No job ran and nothing was asked: nothing to give back.
        std::thread::sleep(RELEASE_AFTER * 2);
        assert_eq!(released(), 0);
        release_on_workers();
        let deadline = Instant::now() + RELEASE_AFTER + Duration::from_secs(5);
        while released() < 3 {
            assert!(Instant::now() < deadline, "released {} times", released());
            std::thread::sleep(Duration::from_millis(10));
        }
        // Each worker once, then idle again.
        std::thread::sleep(RELEASE_AFTER * 2);
        assert_eq!(released(), 3);
        drop(scheduler);
    }

    #[test]
    fn the_window_lets_go_on_a_worker() {
        struct Noted(crossbeam_channel::Sender<Option<String>>);
        impl Drop for Noted {
            fn drop(&mut self) {
                let name = std::thread::current().name().map(str::to_owned);
                self.0.send(name).unwrap();
            }
        }
        let scheduler = Scheduler::new(1);
        let (tx, rx) = crossbeam_channel::unbounded();
        let_go(Noted(tx.clone()));
        let here = std::thread::current().name().map(str::to_owned);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), here);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                mark_window_thread(&scheduler);
                let_go(Noted(tx));
            });
        });
        let dropped_on = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(dropped_on.as_deref(), Some("skwir-worker-0"));
    }
}
