//! Counts a plugin publishes for others to show, as core-owned data: the
//! shelf says how many items it holds and the shell's status bar shows it
//! without asking the shelf, and without querying anything per frame.
//!
//! A reader keeps the last [`Tallies::revision`] it saw and reads again only
//! when it moved: one atomic load per frame.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

/// Named counts, each tagged with the plugin that published it. Names are
/// namespaced by their owner by convention (`shelf.items`).
#[derive(Debug, Default)]
pub struct Tallies {
    counts: RwLock<HashMap<String, (String, u64)>>,
    revision: AtomicU64,
}

impl Tallies {
    /// Publish `name`'s count. A count that does not change leaves the
    /// revision alone, so readers do nothing.
    pub fn set(&self, owner: &str, name: &str, count: u64) {
        let mut counts = self.counts.write();
        if counts
            .get(name)
            .is_some_and(|(o, c)| o == owner && *c == count)
        {
            return;
        }
        counts.insert(name.to_owned(), (owner.to_owned(), count));
        self.revision.fetch_add(1, Ordering::Release);
    }

    /// The published count, `None` while nobody publishes it.
    pub fn get(&self, name: &str) -> Option<u64> {
        self.counts.read().get(name).map(|(_, count)| *count)
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
    use super::Tallies;

    #[test]
    fn a_count_moves_the_revision_only_when_it_changes() {
        let tallies = Tallies::default();
        assert_eq!(tallies.get("shelf.items"), None);
        tallies.set("shelf", "shelf.items", 3);
        let seen = tallies.revision();
        tallies.set("shelf", "shelf.items", 3);
        assert_eq!(tallies.revision(), seen, "same count, nothing to read");
        tallies.set("shelf", "shelf.items", 4);
        assert_ne!(tallies.revision(), seen);
        assert_eq!(tallies.get("shelf.items"), Some(4));
        let seen = tallies.revision();
        tallies.unregister_owner("another-plugin");
        assert_eq!(tallies.revision(), seen);
        tallies.unregister_owner("shelf");
        assert_eq!(tallies.get("shelf.items"), None);
        assert_ne!(tallies.revision(), seen);
    }
}
