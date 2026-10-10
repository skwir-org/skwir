//! What is waiting to be pasted, as core-owned data: the plugin that owns the
//! clipboard publishes it here and any view can show it (cut entries dimmed,
//! a count in the status bar) without knowing that plugin.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::entry::FileId;
use crate::registry::OwnerGate;

/// Entries held for a paste, each by its file and its name, not its path: a
/// name cut is the one entry in every pane and view that lists it, and the
/// file's other names (hard links) are not cut (ADR-049). Telling reads
/// nothing.
#[derive(Debug, Default)]
pub struct Pending {
    entries: HashMap<FileId, Vec<OsString>>,
    /// A paste will move them (cut) rather than copy them.
    pub cut: bool,
    /// Who published it, so disabling that plugin withdraws it.
    pub owner: String,
}

impl Pending {
    /// Whether the entry of `file` named `name` waits for a paste.
    pub fn holds(&self, file: FileId, name: &OsStr) -> bool {
        self.entries
            .get(&file)
            .is_some_and(|names| names.iter().any(|n| n == name))
    }

    /// How many entries wait: names, two of one file two.
    pub fn len(&self) -> usize {
        self.entries.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
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
    /// What waits: each entry by its file and its name.
    pub fn set(
        &self,
        owner: &str,
        entries: impl IntoIterator<Item = (FileId, OsString)>,
        cut: bool,
    ) {
        let mut now = self.now.write();
        if !self.gate.is_open(owner) {
            return;
        }
        let mut by_file: HashMap<FileId, Vec<OsString>> = HashMap::new();
        for (file, name) in entries {
            let names = by_file.entry(file).or_default();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        *now = Arc::new(Pending {
            entries: by_file,
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
    use crate::entry::FileId;

    #[test]
    fn a_snapshot_outlives_a_change_and_only_the_owner_withdraws_it() {
        let pending = PendingTransfer::default();
        let file = FileId { dev: 1, ino: 42 };
        pending.set("clipboard", [(file, "a.txt".into())], true);
        let seen = pending.get();
        assert!(seen.cut && seen.holds(file, "a.txt".as_ref()));
        assert!(
            !seen.holds(file, "b.txt".as_ref()),
            "the file's other name waits not"
        );
        pending.unregister_owner("another-plugin");
        assert!(!pending.get().is_empty());
        pending.unregister_owner("clipboard");
        assert!(pending.get().is_empty());
        assert!(
            seen.holds(file, "a.txt".as_ref()),
            "a frame's snapshot does not change under it"
        );
    }
}
