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
    /// What runs when this token or a descendant is cancelled
    /// ([`CancelToken::on_cancel`]), weak as the signals are.
    hooks: Mutex<Vec<Weak<Hook>>>,
}

/// What [`CancelToken::on_cancel`] runs, once.
struct Hook {
    ran: AtomicBool,
    run: Box<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for Hook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hook")
            .field("ran", &self.ran)
            .finish_non_exhaustive()
    }
}

impl Hook {
    fn fire(&self) {
        if !self.ran.swap(true, Ordering::AcqRel) {
            (self.run)();
        }
    }
}

/// A hook registered with [`CancelToken::on_cancel`]: it runs while this is
/// held; dropped, it is let go of and never runs.
#[derive(Debug)]
#[must_use = "a hook dropped is let go of at once"]
pub struct OnCancel {
    _hook: Arc<Hook>,
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
                hooks: Mutex::default(),
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
                hooks: Mutex::default(),
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
        // Run outside the lock: a hook may cancel or subscribe in turn.
        let hooks = std::mem::take(&mut *self.inner.hooks.lock());
        for hook in hooks.iter().filter_map(Weak::upgrade) {
            hook.fire();
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

    /// Run `hook` once when this token or an ancestor is cancelled, at once
    /// if it already was, on the thread that cancels it: it must not block
    /// (a socket shut down, a flag set). For a call blocked where no token
    /// is looked at, a read on a socket: the hook makes it return. Runs
    /// while the [`OnCancel`] is held.
    pub fn on_cancel(&self, hook: impl Fn() + Send + Sync + 'static) -> OnCancel {
        let hook = Arc::new(Hook {
            ran: AtomicBool::new(false),
            run: Box::new(hook),
        });
        self.hook(&Arc::downgrade(&hook));
        if self.is_cancelled() {
            hook.fire();
        }
        OnCancel { _hook: hook }
    }

    /// Register a hook on this token and every ancestor, pruning those let
    /// go of.
    fn hook(&self, hook: &Weak<Hook>) {
        let mut hooks = self.inner.hooks.lock();
        hooks.retain(|h| h.strong_count() > 0);
        hooks.push(hook.clone());
        drop(hooks);
        for parent in [&self.inner.parent, &self.inner.other_parent]
            .into_iter()
            .flatten()
        {
            parent.hook(hook);
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
    fn a_hook_runs_once_where_the_token_is_cancelled_while_it_is_held() {
        use std::sync::atomic::AtomicUsize;
        let ran = Arc::new(AtomicUsize::new(0));
        let counting = || {
            let ran = Arc::clone(&ran);
            move || {
                ran.fetch_add(1, Ordering::SeqCst);
            }
        };
        let request = CancelToken::new();
        let plugin = CancelToken::new();
        let child = request.linked(&plugin).child();
        let held = child.on_cancel(counting());
        assert_eq!(ran.load(Ordering::SeqCst), 0, "not cancelled yet");
        plugin.cancel();
        assert_eq!(ran.load(Ordering::SeqCst), 1, "by an ancestor");
        child.cancel();
        request.cancel();
        assert_eq!(ran.load(Ordering::SeqCst), 1, "once");
        drop(held);
        // Already cancelled: at once.
        let at_once = child.on_cancel(counting());
        assert_eq!(ran.load(Ordering::SeqCst), 2);
        drop(at_once);
        // Let go of: never, and pruned from a long-lived token.
        let app = CancelToken::new();
        for _ in 0..64 {
            drop(app.child().on_cancel(counting()));
        }
        let kept = app.on_cancel(counting());
        assert_eq!(app.inner.hooks.lock().len(), 1);
        drop(kept);
        app.cancel();
        assert_eq!(ran.load(Ordering::SeqCst), 2, "a hook let go of never runs");
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
