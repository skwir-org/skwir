//! Counts a plugin publishes for others to show, as core-owned data: the
//! shelf says how many items it holds, in its words and with what opens
//! it, and the window's footer shows it without knowing the shelf, and
//! without querying anything per frame.
//!
//! A reader keeps the last [`Tallies::revision`] it saw and reads again only
//! when it moved: one atomic load per frame.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::registry::OwnerGate;

/// A count and how it is shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Tally {
    /// What it counts, as the footer says it: "Shelf".
    pub label: String,
    pub count: u64,
    /// What a click on it runs: opening what it counts.
    pub open: Option<crate::command::Invocation>,
}

/// Named counts, each tagged with the plugin that published it. Names are
/// namespaced by their owner by convention (`shelf.items`).
#[derive(Default)]
pub struct Tallies {
    counts: RwLock<BTreeMap<String, (String, Tally)>>,
    revision: AtomicU64,
    /// Refuses a disabled plugin's worker.
    gate: Arc<OwnerGate>,
}

impl std::fmt::Debug for Tallies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tallies")
            .field("counts", &*self.counts.read())
            .finish_non_exhaustive()
    }
}

impl Tallies {
    pub fn gated(gate: Arc<OwnerGate>) -> Self {
        Tallies {
            gate,
            ..Tallies::default()
        }
    }

    /// Publish `name`'s tally. One that does not change leaves the
    /// revision alone, so readers do nothing. Refused while `owner` is
    /// disabled.
    pub fn set(&self, owner: &str, name: &str, tally: Tally) {
        let mut counts = self.counts.write();
        if !self.gate.is_open(owner) {
            return;
        }
        if counts
            .get(name)
            .is_some_and(|(o, t)| o == owner && *t == tally)
        {
            return;
        }
        counts.insert(name.to_owned(), (owner.to_owned(), tally));
        self.revision.fetch_add(1, Ordering::Release);
    }

    /// The published count, `None` while nobody publishes it.
    pub fn get(&self, name: &str) -> Option<u64> {
        self.counts.read().get(name).map(|(_, tally)| tally.count)
    }

    /// The tallies that count something, by name: what the footer shows.
    pub fn shown(&self) -> Vec<Tally> {
        self.counts
            .read()
            .values()
            .filter(|(_, tally)| tally.count > 0)
            .map(|(_, tally)| tally.clone())
            .collect()
    }

    /// Advances whenever a count is published, changed or withdrawn.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Withdraw what `owner` published (the plugin was disabled).
    pub fn unregister_owner(&self, owner: &str) {
        let mut counts = self.counts.write();
        let before = counts.len();
        counts.retain(|_, (o, _)| o != owner);
        if counts.len() != before {
            self.revision.fetch_add(1, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Tallies, Tally};

    fn tally(count: u64) -> Tally {
        Tally {
            label: "Shelf".into(),
            count,
            open: None,
        }
    }

    #[test]
    fn a_count_moves_the_revision_only_when_it_changes() {
        let tallies = Tallies::default();
        assert_eq!(tallies.get("shelf.items"), None);
        tallies.set("shelf", "shelf.items", tally(3));
        let seen = tallies.revision();
        tallies.set("shelf", "shelf.items", tally(3));
        assert_eq!(tallies.revision(), seen, "same count, nothing to read");
        tallies.set("shelf", "shelf.items", tally(4));
        assert_ne!(tallies.revision(), seen);
        assert_eq!(tallies.get("shelf.items"), Some(4));
        let seen = tallies.revision();
        tallies.unregister_owner("another-plugin");
        assert_eq!(tallies.revision(), seen);
        tallies.unregister_owner("shelf");
        assert_eq!(tallies.get("shelf.items"), None);
        assert_ne!(tallies.revision(), seen);
    }

    #[test]
    fn a_disabled_plugin_publishes_nothing() {
        let gate = std::sync::Arc::new(crate::registry::OwnerGate::default());
        let tallies = Tallies::gated(std::sync::Arc::clone(&gate));
        gate.set_open("shelf", false);
        // Its worker, finishing after the plugin was disabled.
        tallies.set("shelf", "shelf.items", tally(3));
        assert_eq!(tallies.get("shelf.items"), None);
        gate.set_open("shelf", true);
        tallies.set("shelf", "shelf.items", tally(3));
        assert_eq!(tallies.get("shelf.items"), Some(3));
    }
}
