//! Cooperative cancellation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
            }),
        }
    }

    /// Cancel this token and everything derived from it.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
