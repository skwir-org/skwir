//! Work asked of a worker whose answer counts only while nothing newer was
//! asked: a note from another chapter, a picture of a page, a search.
//!
//! One is pending at a time. Asking again, forgetting it or dropping the
//! asker cancels it, and an answer to an older ask is dropped when it
//! arrives: cancellation alone does not keep a stale result out (AGENTS.md,
//! "Background work"). The worker leaves its answer and wakes the window,
//! which takes it as the core wakes it (`take`).
//!
//! Each ask names its lane, as `Shell::off_thread` does: the lane of the
//! path the work reads (`fs::reach::lane`, ADR-077), the shared one for
//! work that reads none a remote holds.

use std::sync::Arc;

use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::scheduler::{Lane, Priority};

pub struct Latest<T> {
    /// Advanced by each ask and each forget: an answer carries the one it
    /// was asked at.
    asked: u64,
    pending: Option<CancelToken>,
    tx: flume::Sender<(u64, T)>,
    rx: flume::Receiver<(u64, T)>,
}

impl<T> Default for Latest<T> {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        Latest {
            asked: 0,
            pending: None,
            tx,
            rx,
        }
    }
}

impl<T> Drop for Latest<T> {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancel();
        }
    }
}

impl<T: Send + 'static> Latest<T> {
    /// Ask `work` of a worker in `lane`, in place of what was asked before.
    /// It runs with a token of `lifetime`'s that the next ask, `forget` or
    /// the drop cancels; its answer is kept unless cancelled, and the
    /// window woken.
    pub fn ask(
        &mut self,
        app: &Arc<App>,
        lane: Lane,
        lifetime: &CancelToken,
        priority: Priority,
        name: &'static str,
        work: impl FnOnce(&CancelToken) -> T + Send + 'static,
    ) {
        self.forget();
        let cancel = lifetime.child();
        self.pending = Some(cancel.clone());
        let asked = self.asked;
        let tx = self.tx.clone();
        let wake = app.waker_handle();
        app.scheduler
            .spawn_in(lane, priority, name, cancel.clone(), move || {
                let answer = work(&cancel);
                if !cancel.is_cancelled() {
                    let _ = tx.send((asked, answer));
                    wake();
                }
            });
    }

    /// Forget what was asked: cancelled, and its answer dropped if it comes.
    pub fn forget(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancel();
        }
        self.asked = self.asked.wrapping_add(1);
    }

    /// Whether an answer is still awaited.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The answer to what is asked now, once it arrived; older ones are
    /// dropped.
    pub fn take(&mut self) -> Option<T> {
        let mut answer = None;
        while let Ok((asked, arrived)) = self.rx.try_recv() {
            if asked == self.asked {
                self.pending = None;
                answer = Some(arrived);
            }
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::Latest;

    #[test]
    fn only_the_answer_to_the_last_ask_is_taken() {
        let mut latest = Latest::<u32>::default();
        latest.asked = 1;
        latest.pending = Some(skwir_core::cancel::CancelToken::new());
        latest.tx.send((0, 7)).unwrap();
        latest.tx.send((1, 8)).unwrap();
        assert_eq!(latest.take(), Some(8));
        assert!(!latest.is_pending());
        latest.forget();
        latest.tx.send((2 - 1, 9)).unwrap();
        assert_eq!(latest.take(), None, "forgotten");
    }
}
