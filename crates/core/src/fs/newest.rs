//! Of two reads of an entry, the one begun later stands, whichever is told
//! last (AGENTS.md, "Watchers act on changes"): what every location that
//! reads its entries on workers keeps, the folder provider by name, a
//! location of many folders (a search, a tag's place, the shelf, recent
//! files) by id.
//!
//! Each read takes a ticket just before it reads ([`Tickets::take`]): a
//! stat, or a read of many at once (a folder's listing, a search's walk of
//! a match) whose entries share one. A read under way is counted by the
//! least ticket it may take ([`Newest::read_begins`]), from before its
//! first ticket until its last answer is applied. What a read found is
//! applied under the location's own lock, the one its events are sent
//! under, only where no read of the entry begun later was applied
//! ([`Newest::applies`]): so a late answer (a hydration's, held while a
//! plan renamed its file and an undo named it back) never stands over a
//! newer one, whatever each found, gone included.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};

/// The tickets of a location's reads: each takes the next just before it
/// reads, so of two reads the one begun later has the higher.
#[derive(Debug, Default)]
pub struct Tickets(AtomicU64);

impl Tickets {
    /// The next read's ticket, taken just before it reads: after what was
    /// changed before it, so a read with a higher one found it as it is
    /// now.
    pub fn take(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The least ticket a read beginning now may take: what it is counted
    /// by while it is under way ([`Newest::read_begins`]).
    pub fn floor(&self) -> u64 {
        self.0.load(Ordering::SeqCst) + 1
    }
}

/// Which read of each entry was applied last, while a read that may hold
/// an older one is under way: kept under the lock what a location's reads
/// change, and send their events under.
#[derive(Debug)]
pub struct Newest<K> {
    /// The reads under way, by the least ticket each may take, counted.
    reading: BTreeMap<u64, u32>,
    /// Of each entry read while another read begun no later was under way,
    /// the ticket of the newest read applied, whatever it found: one begun
    /// before it, applied after, is left out. Forgotten once no read under
    /// way may hold one begun before it: empty while one reads alone.
    applied: crate::hashing::Map<K, u64>,
    /// `applied` by ticket, oldest first: what is forgotten next. One entry
    /// a key, as in `applied` (the entries of one read share its ticket).
    by_ticket: BTreeSet<(u64, K)>,
}

impl<K> Default for Newest<K> {
    fn default() -> Self {
        Newest {
            reading: BTreeMap::new(),
            applied: crate::hashing::Map::default(),
            by_ticket: BTreeSet::new(),
        }
    }
}

impl<K: Clone + Eq + Hash + Ord> Newest<K> {
    /// A read begins, every ticket it takes `floor` at least
    /// ([`Tickets::floor`], read under the same lock as this).
    pub fn read_begins(&mut self, floor: u64) {
        *self.reading.entry(floor).or_default() += 1;
    }

    /// A read begins now: [`read_begins`](Self::read_begins) with its floor,
    /// which it ends with ([`read_ended`](Self::read_ended)).
    pub fn begin(&mut self, tickets: &Tickets) -> u64 {
        let floor = tickets.floor();
        self.read_begins(floor);
        floor
    }

    /// The read begun at `floor` ended, its last answer applied: what only
    /// a read begun before the oldest left could be overtaken by is
    /// forgotten, all of it once none is under way.
    pub fn read_ended(&mut self, floor: u64) {
        if let Some(count) = self.reading.get_mut(&floor) {
            *count -= 1;
            if *count == 0 {
                self.reading.remove(&floor);
            }
        }
        let Some(&oldest) = self.reading.keys().next() else {
            self.applied.clear();
            self.by_ticket.clear();
            return;
        };
        while self
            .by_ticket
            .first()
            .is_some_and(|(ticket, _)| *ticket <= oldest)
        {
            let Some((ticket, key)) = self.by_ticket.pop_first() else {
                break;
            };
            if self.applied.get(&key) == Some(&ticket) {
                self.applied.remove(&key);
            }
        }
    }

    /// Whether what the read of `key` that took `ticket` found is applied:
    /// no read of it begun later was. Noted as applied where another read
    /// begun no later is under way, which may hold a read of it begun
    /// before (one begun after holds none), in the place of the read of it
    /// noted before: one entry a key, however often it is read while a read
    /// stays under way (one held by a server that stopped answering).
    pub fn applies<Q>(&mut self, key: &Q, ticket: u64) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = K> + ?Sized,
    {
        if self.applied.get(key).is_some_and(|&newer| newer > ticket) {
            return false;
        }
        // Each read under way counted by its floor: its own among them.
        let before: u32 = self.reading.range(..=ticket).map(|(_, &n)| n).sum();
        if before > 1 {
            let key = key.to_owned();
            if let Some(older) = self.applied.insert(key.clone(), ticket)
                && older != ticket
            {
                self.by_ticket.remove(&(older, key.clone()));
            }
            self.by_ticket.insert((ticket, key));
        }
        true
    }

    /// How many entries are noted: what the tests count.
    pub fn noted(&self) -> usize {
        debug_assert_eq!(self.applied.len(), self.by_ticket.len());
        self.applied.len()
    }

    /// The ticket noted of `key`, if one is: what the tests look at.
    pub fn noted_of<Q>(&self, key: &Q) -> Option<u64>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.applied.get(key).copied()
    }

    /// Whether no read is under way.
    pub fn is_idle(&self) -> bool {
        self.reading.is_empty()
    }
}

/// A read under way, counted from before its first ticket until dropped,
/// after its last answer is applied ([`Newest::begin`]): a read that ends
/// early (cancelled, failed) is not counted for good. `of` finds the
/// [`Newest`] in what `lock` holds, the location's own state. Dropped with
/// `lock` free: it takes it.
pub struct Reading<'a, S, K: Clone + Eq + Hash + Ord> {
    lock: &'a parking_lot::Mutex<S>,
    of: fn(&mut S) -> &mut Newest<K>,
    floor: u64,
}

impl<'a, S, K: Clone + Eq + Hash + Ord> Reading<'a, S, K> {
    /// A read begins now, under `lock`, its tickets taken of `tickets`.
    pub fn begin(
        lock: &'a parking_lot::Mutex<S>,
        of: fn(&mut S) -> &mut Newest<K>,
        tickets: &Tickets,
    ) -> Self {
        let floor = of(&mut lock.lock()).begin(tickets);
        Reading { lock, of, floor }
    }
}

impl<S, K: Clone + Eq + Hash + Ord> Drop for Reading<'_, S, K> {
    fn drop(&mut self) {
        (self.of)(&mut self.lock.lock()).read_ended(self.floor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};

    /// Of two reads of a key, the one begun later stands whatever each
    /// found, applied in either order. Noted only while another read begun
    /// no later is under way, and forgotten once no read left may hold one
    /// begun before.
    #[test]
    fn of_two_reads_of_a_key_the_later_begun_stands_whatever_it_found() {
        let name = OsStr::new("it's back, été");
        let mut newest = Newest::<OsString>::default();
        // A hydration's batch begins (its reads from ticket 1), then the
        // watcher's changes (from 3).
        newest.read_begins(1);
        newest.read_begins(3);
        // The watcher's read, begun later (4), applied first: the
        // hydration's (2) is left out.
        assert!(newest.applies(name, 4));
        assert!(!newest.applies(name, 2), "from before: left out");
        // The other way: the hydration's (5) applied first, then the
        // watcher's begun before (3).
        let other = OsStr::new("it's gone, été");
        assert!(newest.applies(other, 5));
        assert!(!newest.applies(other, 3), "from before: left out");
        assert!(newest.applies(other, 6), "a later one stands");
        // The hydration ends: what only it could be older than is
        // forgotten, the watcher's own kept while it reads on.
        newest.read_ended(1);
        assert_eq!(newest.noted_of(name), Some(4));
        newest.read_ended(3);
        assert_eq!(newest.noted(), 0);
        assert!(newest.is_idle());
        // A read alone notes nothing: no read begun before it is under way.
        newest.read_begins(7);
        assert!(newest.applies(name, 7));
        assert_eq!(newest.noted(), 0);
        newest.read_ended(7);
    }

    /// While a read stays under way (held by a server that stopped
    /// answering), each key read again and again is noted once, by its
    /// latest read: what is noted grows with the keys, never the reads.
    #[test]
    fn a_key_read_again_while_a_read_stays_is_noted_once() {
        let name = OsStr::new("it's written often, été");
        let mut newest = Newest::<OsString>::default();
        newest.read_begins(1);
        for ticket in 2..1_000 {
            newest.read_begins(ticket);
            assert!(newest.applies(name, ticket));
            newest.read_ended(ticket);
        }
        assert_eq!(newest.noted_of(name), Some(999));
        assert_eq!(newest.noted(), 1, "one a key");
        newest.read_begins(1_000);
        assert!(!newest.applies(name, 998));
        newest.read_ended(1_000);
        newest.read_ended(1);
        assert_eq!(newest.noted(), 0);
    }

    /// The entries of one read share its ticket (a listing's names): each
    /// is noted while a read begun before it is under way, never for reads
    /// begun after it, and all are forgotten once none older is left.
    #[test]
    fn the_entries_of_one_read_are_noted_by_its_one_ticket() {
        let tickets = Tickets::default();
        let mut newest = Newest::<u64>::default();
        // A hydration under way, then a read of three at once.
        let hydration = newest.begin(&tickets);
        let older = tickets.take();
        let listing = newest.begin(&tickets);
        let ticket = tickets.take();
        assert!(older < ticket && listing <= ticket);
        for key in [1, 2, 3] {
            assert!(newest.applies(&key, ticket));
        }
        assert_eq!(newest.noted(), 3, "one entry a key");
        assert!(!newest.applies(&2, older), "the hydration's, from before");
        newest.read_ended(hydration);
        assert_eq!(newest.noted(), 0, "nothing older left");
        // A read begun after: what the earlier one applies now is noted
        // by nobody, what the later finds is.
        let later = newest.begin(&tickets);
        assert!(newest.applies(&4, ticket));
        assert_eq!(newest.noted(), 0);
        let newer = tickets.take();
        assert!(newest.applies(&1, newer));
        assert!(!newest.applies(&1, ticket), "the earlier read's");
        newest.read_ended(later);
        newest.read_ended(listing);
        assert!(newest.is_idle() && newest.noted() == 0);
    }
}
