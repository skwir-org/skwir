//! Work under way that is no plan, shown while it runs: a print being made
//! and sent (ADR-111), which nothing undoes. The window draws each beside
//! a plan's progress, with its title, how far it went and Cancel. What
//! starts the work holds a [`Task`] while it runs: dropped, the task is
//! gone from the window. Its progress wakes the window at most every
//! [`WAKE_EVERY`], as a plan's bytes do.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::cancel::CancelToken;
use crate::location::Waker;

/// How often progress may wake the window.
const WAKE_EVERY: Duration = Duration::from_millis(100);

/// How far a task went, in thousandths; this when nothing was said yet.
const UNKNOWN: u32 = u32::MAX;

/// The tasks running.
#[derive(Default)]
pub struct Tasks {
    list: Mutex<Vec<Arc<Shared>>>,
    next: AtomicU64,
    waker: Mutex<Option<Waker>>,
    last_wake: Mutex<Option<Instant>>,
}

struct Shared {
    id: u64,
    title: String,
    went: AtomicU32,
    cancel: CancelToken,
}

/// A task as the window shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub id: u64,
    /// What it does, as a sentence starts: "Print “notes.md”".
    pub title: String,
    /// How far it went, from 0 to 1; `None` until it says.
    pub went: Option<f64>,
    /// It was asked to stop and has not yet.
    pub cancelling: bool,
}

/// A task held by the work it is: it says how far it went, and is gone
/// from the window when dropped.
pub struct Task {
    shared: Arc<Shared>,
    tasks: Arc<Tasks>,
}

impl Tasks {
    /// What wakes the window when a task starts, moves or ends.
    pub fn set_waker(&self, waker: Option<Waker>) {
        *self.waker.lock() = waker;
    }

    /// A task titled `title` starts, stopped by `cancel` (which Cancel
    /// cancels).
    pub fn start(self: &Arc<Self>, title: impl Into<String>, cancel: CancelToken) -> Task {
        let shared = Arc::new(Shared {
            id: self.next.fetch_add(1, Ordering::Relaxed),
            title: title.into(),
            went: AtomicU32::new(UNKNOWN),
            cancel,
        });
        self.list.lock().push(Arc::clone(&shared));
        self.wake(true);
        Task {
            shared,
            tasks: Arc::clone(self),
        }
    }

    /// The tasks running, oldest first.
    pub fn running(&self) -> Vec<Running> {
        self.list
            .lock()
            .iter()
            .map(|shared| {
                let went = shared.went.load(Ordering::Relaxed);
                Running {
                    id: shared.id,
                    title: shared.title.clone(),
                    went: (went != UNKNOWN).then(|| f64::from(went) / 1000.0),
                    cancelling: shared.cancel.is_cancelled(),
                }
            })
            .collect()
    }

    /// Cancel the task `id`: whether it was running.
    pub fn cancel(&self, id: u64) -> bool {
        let found = self
            .list
            .lock()
            .iter()
            .find(|shared| shared.id == id)
            .map(|shared| shared.cancel.clone());
        match found {
            Some(cancel) => {
                cancel.cancel();
                self.wake(true);
                true
            }
            None => false,
        }
    }

    /// Wake the window: always when `now`, else at most every
    /// [`WAKE_EVERY`].
    fn wake(&self, now: bool) {
        let at = Instant::now();
        {
            let mut last = self.last_wake.lock();
            if !now && last.is_some_and(|t| at.duration_since(t) < WAKE_EVERY) {
                return;
            }
            *last = Some(at);
        }
        let waker = self.waker.lock().clone();
        if let Some(waker) = waker {
            waker();
        }
    }
}

impl Task {
    /// It went `fraction` (0 to 1) of the way.
    pub fn went(&self, fraction: f64) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let thousandths = (fraction.clamp(0.0, 1.0) * 1000.0).round() as u32;
        if self.shared.went.swap(thousandths, Ordering::Relaxed) != thousandths {
            self.tasks.wake(false);
        }
    }

    /// What stops it.
    pub fn cancel_token(&self) -> &CancelToken {
        &self.shared.cancel
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        let id = self.shared.id;
        self.tasks.list.lock().retain(|shared| shared.id != id);
        self.tasks.wake(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn a_task_is_shown_while_held_and_cancelled_when_asked() {
        let tasks = Arc::new(Tasks::default());
        let woken = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&woken);
        tasks.set_waker(Some(Arc::new(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        })));
        let task = tasks.start("Print “notes.md”", CancelToken::new());
        let shown = tasks.running();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].went, None);
        task.went(0.25);
        assert_eq!(tasks.running()[0].went, Some(0.25));
        assert!(!task.cancel_token().is_cancelled());
        assert!(tasks.cancel(shown[0].id));
        assert!(task.cancel_token().is_cancelled());
        assert!(tasks.running()[0].cancelling);
        drop(task);
        assert!(tasks.running().is_empty());
        assert!(!tasks.cancel(shown[0].id), "gone");
        assert!(
            woken.load(Ordering::Relaxed) >= 3,
            "started, cancelled, ended"
        );
    }
}
