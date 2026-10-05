//! What is waiting to be pasted, as core-owned data: the plugin that owns the
//! clipboard publishes it here and any view can show it (cut entries dimmed,
//! a count in the status bar) without knowing that plugin.

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::entry::EntryId;
use crate::registry::OwnerGate;

/// Entries held for a paste. Identity, not paths: a cut file is the same
/// file in every pane and view that lists it.
#[derive(Debug, Default)]
pub struct Pending {
    pub entries: HashSet<EntryId>,
    /// A paste will move them (cut) rather than copy them.
    pub cut: bool,
    /// Who published it, so disabling that plugin withdraws it.
    pub owner: String,
}

#[derive(Default)]
pub struct PendingTransfer {
    now: RwLock<Arc<Pending>>,
    /// Refuses a disabled plugin's worker.
    gate: Arc<OwnerGate>,
}

impl std::fmt::Debug for PendingTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PendingTransfer").field(&self.get()).finish()
    }
}

impl PendingTransfer {
    pub fn gated(gate: Arc<OwnerGate>) -> Self {
        PendingTransfer {
            now: RwLock::default(),
            gate,
        }
    }

    /// One cheap snapshot per frame; rows test membership against it.
    pub fn get(&self) -> Arc<Pending> {
        Arc::clone(&self.now.read())
    }

    /// Refused while `owner` is disabled.
    pub fn set(&self, owner: &str, entries: impl IntoIterator<Item = EntryId>, cut: bool) {
        let mut now = self.now.write();
        if !self.gate.is_open(owner) {
            return;
        }
        *now = Arc::new(Pending {
            entries: entries.into_iter().collect(),
            cut,
            owner: owner.to_owned(),
        });
    }

    pub fn clear(&self) {
        *self.now.write() = Arc::default();
    }

    pub fn unregister_owner(&self, owner: &str) {
        let mut now = self.now.write();
        if now.owner == owner {
            *now = Arc::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PendingTransfer;
    use crate::entry::EntryId;

    #[test]
    fn a_snapshot_outlives_a_change_and_only_the_owner_withdraws_it() {
        let pending = PendingTransfer::default();
        let id = EntryId { dev: 1, ino: 42 };
        pending.set("clipboard", [id], true);
        let seen = pending.get();
        assert!(seen.cut && seen.entries.contains(&id));
        pending.unregister_owner("another-plugin");
        assert!(!pending.get().entries.is_empty());
        pending.unregister_owner("clipboard");
        assert!(pending.get().entries.is_empty());
        assert!(
            seen.entries.contains(&id),
            "a frame's snapshot does not change under it"
        );
    }
}
