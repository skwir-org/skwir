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
    shutdown: bool,
}

/// Per-label timing, for surfacing slow providers.
#[derive(Clone, Debug, Default)]
pub struct LabelStats {
    pub runs: u64,
    pub total: Duration,
    pub max: Duration,
}

struct Inner {
    queue: Mutex<Queue>,
    cv: Condvar,
    seq: AtomicU64,
    stats: Mutex<std::collections::HashMap<&'static str, LabelStats>>,
    slow_threshold: Duration,
}

/// The shared scheduler. Cheap to clone.
#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
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
        Scheduler { inner }
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

impl Drop for Inner {
    fn drop(&mut self) {
        self.queue.lock().shutdown = true;
        self.cv.notify_all();
    }
}

fn worker(inner: &Inner) {
    loop {
        let task = {
            let mut q = inner.queue.lock();
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(t) = q.heap.pop() {
                    break t;
                }
                inner.cv.wait(&mut q);
            }
        };
        if task.cancel.is_cancelled() {
            continue;
        }
        let t = Instant::now();
        (task.job)();
        let took = t.elapsed();
        let mut stats = inner.stats.lock();
        let s = stats.entry(task.label).or_default();
        s.runs += 1;
        s.total += took;
        s.max = s.max.max(took);
        if took > inner.slow_threshold {
            tracing::warn!(label = task.label, ms = took.as_millis(), "slow task");
        }
    }
}
