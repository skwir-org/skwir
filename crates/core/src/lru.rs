//! A least-recently-used map bounded by bytes, not by count: decoded
//! images differ in size by orders of magnitude, and memory is the budget.
//!
//! Recency is a queue of (key, stamp) pairs: a hit pushes a new pair and
//! leaves the old one to be recognised as stale by its stamp. The queue is
//! compacted whenever stale pairs outnumber live ones, so a cache read every
//! frame stays bounded by the number of entries, not the number of reads.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

pub struct ByteLru<K, V> {
    map: HashMap<K, (V, usize, u64)>,
    order: VecDeque<(K, u64)>,
    bytes: usize,
    budget: usize,
    tick: u64,
}

impl<K: Hash + Eq + Clone, V> ByteLru<K, V> {
    pub fn new(budget: usize) -> Self {
        ByteLru {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            budget,
            tick: 0,
        }
    }

    /// The value for `k`, now the most recently used.
    pub fn get(&mut self, k: &K) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        let (_, _, stamp) = self.map.get_mut(k)?;
        *stamp = tick;
        self.order.push_back((k.clone(), tick));
        self.compact();
        self.map.get(k).map(|(v, _, _)| v)
    }

    /// Whether `k` is cached, without making it more recent.
    pub fn contains(&self, k: &K) -> bool {
        self.map.contains_key(k)
    }

    /// Insert `v`, weighing `bytes`. Returns what was evicted to stay within
    /// the budget (and the value `k` held before), for callers that release
    /// resources beside the memory, such as GPU textures.
    pub fn insert(&mut self, k: K, v: V, bytes: usize) -> Vec<V> {
        self.tick += 1;
        let mut out = Vec::new();
        if let Some((old, b, _)) = self.map.insert(k.clone(), (v, bytes, self.tick)) {
            self.bytes -= b;
            out.push(old);
        }
        self.bytes += bytes;
        self.order.push_back((k, self.tick));
        out.extend(self.evict());
        self.compact();
        out
    }

    pub fn remove(&mut self, k: &K) -> Option<V> {
        let (v, b, _) = self.map.remove(k)?;
        self.bytes -= b;
        Some(v)
    }

    /// Change the budget; returns what no longer fits.
    pub fn set_budget(&mut self, budget: usize) -> Vec<V> {
        self.budget = budget;
        self.evict()
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Bytes currently held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn evict(&mut self) -> Vec<V> {
        let mut evicted = Vec::new();
        while self.bytes > self.budget {
            let Some((k, stamp)) = self.order.pop_front() else {
                break;
            };
            if self.map.get(&k).is_some_and(|(_, _, s)| *s == stamp)
                && let Some((v, b, _)) = self.map.remove(&k)
            {
                self.bytes -= b;
                evicted.push(v);
            }
        }
        evicted
    }

    /// Drop stale recency pairs once they outnumber the live ones.
    fn compact(&mut self) {
        if self.order.len() <= 2 * self.map.len() + 64 {
            return;
        }
        let map = &self.map;
        self.order
            .retain(|(k, stamp)| map.get(k).is_some_and(|(_, _, s)| s == stamp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_the_least_recently_used_over_budget() {
        let mut lru = ByteLru::new(30);
        assert!(lru.insert(1, "a", 10).is_empty());
        assert!(lru.insert(2, "b", 10).is_empty());
        assert!(lru.insert(3, "c", 10).is_empty());
        assert_eq!(lru.get(&1), Some(&"a"));
        assert_eq!(lru.insert(4, "d", 10), vec!["b"]);
        assert!(lru.contains(&1) && !lru.contains(&2));
        assert_eq!(lru.bytes(), 30);
        assert_eq!(lru.set_budget(10), vec!["c", "a"]);
        assert_eq!(lru.len(), 1);
    }

    #[test]
    fn replacing_returns_the_old_value() {
        let mut lru = ByteLru::new(100);
        lru.insert(1, "a", 10);
        assert_eq!(lru.insert(1, "b", 20), vec!["a"]);
        assert_eq!(lru.bytes(), 20);
        assert_eq!(lru.remove(&1), Some("b"));
        assert_eq!(lru.bytes(), 0);
    }

    #[test]
    fn reads_every_frame_do_not_grow_the_queue() {
        let mut lru = ByteLru::new(1 << 20);
        for k in 0..10 {
            lru.insert(k, k, 1);
        }
        for _ in 0..10_000 {
            for k in 0..10 {
                lru.get(&k);
            }
        }
        assert!(lru.order.len() <= 2 * lru.len() + 65, "{}", lru.order.len());
    }
}
