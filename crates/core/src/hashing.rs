//! Hash maps of what a listing holds, entry by entry: by its id, by its
//! name. foldhash is a few multiplies a word where std's `SipHash` makes
//! rounds of its own, a listing hashing each entry several times. Each
//! map is seeded anew from a secret of the process, so names a folder
//! holds, which anyone may choose, cannot be made to collide: no one sees
//! what they hash to. Never for what is kept beyond the process (its
//! output changes with the seed and the version).

/// What a map of entries hashes with: foldhash, seeded per map.
pub type State = foldhash::fast::RandomState;

/// A hash map of entries ([`State`]).
pub type Map<K, V> = std::collections::HashMap<K, V, State>;

/// A hash set of entries ([`State`]).
pub type Set<K> = std::collections::HashSet<K, State>;

/// An insertion-ordered map of entries ([`State`]).
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, State>;
