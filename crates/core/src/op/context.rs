//! What a step can use while it is checked and applied: the plan's
//! cancellation, a meter for work inside one step, and lookups the plan's
//! steps share instead of repeating.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::cancel::CancelToken;
use crate::location::Waker;

/// The error a step returns when it stopped because its plan was cancelled.
/// The executor reports the plan as cancelled, not failed.
#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// Bytes a running plan has moved, read by whoever shows its progress.
#[derive(Default)]
pub(super) struct Meter {
    pub(super) done: AtomicU64,
    pub(super) total: AtomicU64,
    wake: Option<Waker>,
    last_wake: Mutex<Option<Instant>>,
}

/// How often byte progress may wake the UI: often enough for a bar to move,
/// rarely enough that a fast copy does not redraw per chunk.
const WAKE_EVERY: Duration = Duration::from_millis(100);

impl Meter {
    pub(super) fn new(wake: Option<Waker>) -> Self {
        Self {
            wake,
            ..Self::default()
        }
    }

    fn advance(&self, bytes: u64) {
        self.done.fetch_add(bytes, Ordering::Relaxed);
        let Some(wake) = &self.wake else {
            return;
        };
        let now = Instant::now();
        let mut last = self.last_wake.lock();
        if last.is_none_or(|t| now.duration_since(t) >= WAKE_EVERY) {
            *last = Some(now);
            drop(last);
            wake();
        }
    }
}

/// Passed to [`super::Operation::check`] and [`super::Operation::apply`].
/// One context lives for one run of a plan (or one dry run), so what it
/// caches is shared by that plan's steps and no longer.
pub struct StepContext {
    cancel: CancelToken,
    meter: Arc<Meter>,
    /// The trash listing, read once per plan: parsing every `.trashinfo`
    /// per restored item made undoing a large trash quadratic.
    trash: RefCell<Option<Vec<trash::TrashItem>>>,
}

impl StepContext {
    pub(super) fn new(cancel: CancelToken, meter: Arc<Meter>) -> Self {
        Self {
            cancel,
            meter,
            trash: RefCell::new(None),
        }
    }

    /// A context for a dry run or a test: never cancelled, progress unread.
    pub fn detached() -> Self {
        Self::with_cancel(CancelToken::new())
    }

    /// A context outside the executor that stops when `cancel` does.
    pub fn with_cancel(cancel: CancelToken) -> Self {
        Self::new(cancel, Arc::default())
    }

    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// `Err(Cancelled)` once the plan is cancelled; call between units of work.
    ///
    /// # Errors
    /// [`Cancelled`] when the plan's token is cancelled.
    pub fn check_cancelled(&self) -> Result<(), Cancelled> {
        if self.cancel.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }

    /// Announce `bytes` of work this step is about to do.
    pub fn add_total(&self, bytes: u64) {
        self.meter.total.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Report `bytes` of work done.
    pub fn advance(&self, bytes: u64) {
        self.meter.advance(bytes);
    }

    /// Run `f` over the trash listing, reading it on first use in this plan.
    pub(super) fn with_trash<T>(
        &self,
        f: impl FnOnce(&mut Vec<trash::TrashItem>) -> T,
    ) -> Result<T, trash::Error> {
        let mut cached = self.trash.borrow_mut();
        if cached.is_none() {
            *cached = Some(trash::os_limited::list()?);
        }
        Ok(f(cached.as_mut().expect("just filled")))
    }

    /// The trash changed under the cached listing (an item was trashed).
    pub(super) fn forget_trash(&self) {
        self.trash.borrow_mut().take();
    }
}
