//! Cooperative cancellation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;

/// A cheap, cloneable cancellation flag. Children observe their parent.
#[derive(Clone, Debug, Default)]
pub struct CancelToken {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    cancelled: AtomicBool,
    parent: Option<CancelToken>,
    other_parent: Option<CancelToken>,
    /// Subscribers waiting for this token or a descendant. Weak: a dropped
    /// [`CancelSignal`] leaves nothing to notify and is pruned on the next
    /// subscription, so long-lived tokens do not accumulate dead entries.
    signals: Mutex<Vec<Weak<Sender<()>>>>,
}

/// A wakeable cancellation notification for threads that block in a
/// channel `select!`. The receiver yields one message when the token or any
/// ancestor is cancelled, immediately if it already was; it never blocks
/// for a poll interval. Dropping the signal unsubscribes.
#[derive(Debug)]
pub struct CancelSignal {
    receiver: Receiver<()>,
    _sender: Arc<Sender<()>>,
}
impl CancelSignal {
    /// The channel end to select on. Receiving `Ok(())` means cancelled.
    pub fn receiver(&self) -> &Receiver<()> {
        &self.receiver
    }
}

impl CancelToken {
    /// A fresh, uncancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// A token that is cancelled when either it or `self` is cancelled.
    #[must_use]
    pub fn child(&self) -> Self {
        CancelToken {
            inner: Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                parent: Some(self.clone()),
                other_parent: None,
                signals: Mutex::default(),
            }),
        }
    }

    /// A child cancelled by either parent, without cancelling either parent.
    /// Used for work tied to both a request and a plugin's enabled lifetime.
    pub fn linked(&self, other: &Self) -> Self {
        Self {
            inner: Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                parent: Some(self.clone()),
                other_parent: Some(other.clone()),
                signals: Mutex::default(),
            }),
        }
    }

    /// Cancel this token and everything derived from it.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
        // Notify after the flag is visible so a woken subscriber that checks
        // `is_cancelled` observes the cancellation.
        let signals = std::mem::take(&mut *self.inner.signals.lock());
        for signal in signals.iter().filter_map(Weak::upgrade) {
            let _ = signal.try_send(());
        }
    }

    /// Whether this token or any ancestor has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
            || self
                .inner
                .parent
                .as_ref()
                .is_some_and(CancelToken::is_cancelled)
            || self
                .inner
                .other_parent
                .as_ref()
                .is_some_and(CancelToken::is_cancelled)
    }

    /// Subscribe to cancellation of this token or any ancestor. Prefer this
    /// over polling `is_cancelled` on a timer in a blocked thread.
    pub fn subscribe(&self) -> CancelSignal {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let sender = Arc::new(sender);
        self.register(&Arc::downgrade(&sender));
        if self.is_cancelled() {
            let _ = sender.try_send(());
        }
        CancelSignal {
            receiver,
            _sender: sender,
        }
    }

    /// Register on this token and every ancestor, pruning dropped subscribers.
    fn register(&self, signal: &Weak<Sender<()>>) {
        let mut signals = self.inner.signals.lock();
        signals.retain(|s| s.strong_count() > 0);
        signals.push(signal.clone());
        drop(signals);
        for parent in [&self.inner.parent, &self.inner.other_parent]
            .into_iter()
            .flatten()
        {
            parent.register(signal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn linked_tokens_observe_either_parent_without_cancelling_siblings() {
        for cancel_request in [false, true] {
            let request = CancelToken::new();
            let plugin = CancelToken::new();
            let linked = request.linked(&plugin);
            let child = linked.child();
            if cancel_request {
                request.cancel();
            } else {
                plugin.cancel();
            }
            assert!(child.is_cancelled());
            assert_eq!(request.is_cancelled(), cancel_request);
            assert_ne!(plugin.is_cancelled(), cancel_request);
        }
        let parent = CancelToken::new();
        parent.linked(&CancelToken::new()).cancel();
        assert!(!parent.is_cancelled());
    }

    #[test]
    fn subscriptions_wake_on_ancestor_cancellation_without_polling() {
        let request = CancelToken::new();
        let plugin = CancelToken::new();
        let child = request.linked(&plugin).child();
        let signal = child.subscribe();
        assert!(signal.receiver().try_recv().is_err(), "not cancelled yet");
        let waiter = std::thread::spawn({
            let receiver = signal.receiver().clone();
            move || receiver.recv_timeout(Duration::from_secs(5)).is_ok()
        });
        plugin.cancel();
        assert!(waiter.join().unwrap(), "woken by an ancestor");
        assert!(child.is_cancelled());
        // Already-cancelled tokens signal immediately; a sibling stays quiet.
        assert!(child.subscribe().receiver().try_recv().is_ok());
        assert!(request.subscribe().receiver().try_recv().is_err());
    }

    #[test]
    fn dropped_subscriptions_are_pruned_from_long_lived_tokens() {
        let app = CancelToken::new();
        for _ in 0..64 {
            drop(app.child().subscribe());
        }
        let kept = app.subscribe();
        assert_eq!(app.inner.signals.lock().len(), 1);
        drop(kept);
        app.cancel();
        assert!(app.subscribe().receiver().try_recv().is_ok());
    }
}
