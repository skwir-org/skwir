//! Locations and streaming listings.
//!
//! A location is a string: a path, or `scheme:rest` for anything a plugin
//! provides (queries, trash, recent, ...). Opening one yields an
//! [`OpenLocation`] that streams [`ListingEvent`]s over a channel. The UI
//! applies them to a [`Listing`] (or its own model) and never waits.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId};
use crate::os_path::PathArg;
use crate::scheduler::{Priority, Scheduler};

/// The scheme used for plain filesystem paths.
pub const PATH_SCHEME: &str = "path";

/// A parsed location string.
///
/// A path location keeps its exact path: a name that is not valid UTF-8 has
/// no faithful string form, so `rest` then holds a display form and the path
/// itself travels beside it (serialized as bytes, see [`crate::os_path`]).
/// Build path locations with [`Location::from_path`] and read them with
/// [`Location::path`]; never round-trip one through `to_string`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Location {
    /// `path` for filesystem paths, otherwise a plugin's scheme.
    pub scheme: Arc<str>,
    /// Everything after the scheme; the path itself for `path` (a display
    /// form when the path is not valid UTF-8).
    pub rest: String,
    /// The exact path, only when `rest` cannot hold it.
    #[serde(default, rename = "path", skip_serializing_if = "Option::is_none")]
    raw: Option<PathArg>,
}

impl Location {
    /// A location of a plugin's `scheme`.
    pub fn new(scheme: &str, rest: impl Into<String>) -> Self {
        Location {
            scheme: scheme.into(),
            rest: rest.into(),
            raw: None,
        }
    }

    /// Parse `s`. Absolute or relative paths, `~`, and `scheme:rest` are all accepted.
    /// A single letter before the colon is treated as a Windows drive, not a scheme.
    pub fn parse(s: &str) -> Self {
        if let Some((scheme, rest)) = s.split_once(':')
            && scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Location::new(scheme, rest);
        }
        Location::new(PATH_SCHEME, s)
    }

    /// [`Location::parse`] for text that arrived as an OS string (a command
    /// argument, see [`crate::os_path`]): what is not UTF-8 can only be a path.
    pub fn parse_os(s: &OsStr) -> Self {
        match s.to_str() {
            Some(text) => Location::parse(text),
            None => Location::from_path(s),
        }
    }

    /// A path location, exact whatever bytes the path holds.
    pub fn from_path(p: impl AsRef<Path>) -> Self {
        let p = p.as_ref();
        match p.to_str() {
            Some(text) => Location::new(PATH_SCHEME, text),
            None => Location {
                scheme: PATH_SCHEME.into(),
                rest: p.to_string_lossy().into_owned(),
                raw: Some(PathArg::from(p)),
            },
        }
    }

    pub fn is_path(&self) -> bool {
        &*self.scheme == PATH_SCHEME
    }

    /// Whether `other` is the same place: a folder by its path (`~` and the
    /// exact bytes resolved), anything else as written.
    pub fn same_place(&self, other: &Location) -> bool {
        match (self.path(), other.path()) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self == other,
            _ => false,
        }
    }

    /// The filesystem path, with `~` expanded, for `path` locations.
    pub fn path(&self) -> Option<PathBuf> {
        if !self.is_path() {
            return None;
        }
        if let Some(raw) = &self.raw {
            return Some(raw.to_path_buf());
        }
        let r = &self.rest;
        if r == "~" || r.starts_with("~/") {
            let home = dirs::home_dir()?;
            return Some(if r == "~" { home } else { home.join(&r[2..]) });
        }
        Some(PathBuf::from(r))
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_path() {
            f.write_str(&self.rest)
        } else {
            write!(f, "{}:{}", self.scheme, self.rest)
        }
    }
}

/// What an open location streams. Every variant is a batch.
#[derive(Clone, Debug)]
pub enum ListingEvent {
    /// New entries, or for an id already present its replacement. A
    /// replacement that carries no stat under the same name (a re-listing)
    /// keeps the metadata the listing already has.
    Added(Vec<Entry>),
    /// Entries whose name, kind, or metadata changed. Ids the listing does
    /// not hold are ignored: a late stat must not resurrect a removed entry.
    Updated(Vec<Entry>),
    /// Entries that no longer exist here.
    Removed(Vec<EntryId>),
    /// The initial listing is complete; further events come from watching.
    Complete,
    /// The listing failed or was interrupted.
    Error(String),
}

/// Called after events are queued so a sleeping UI can wake and pump.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// What a provider gets when a location is opened.
#[derive(Clone)]
pub struct OpenContext {
    pub scheduler: Scheduler,
    pub cancel: CancelToken,
    /// Preferred batch size for `Added` events.
    pub batch_size: usize,
    /// Wake hook; providers should send through [`OpenContext::channel`] which calls it.
    pub waker: Option<Waker>,
    /// Metadata providers to run after stat (see [`crate::metadata`]).
    pub metadata: Arc<crate::metadata::MetadataProviders>,
}

impl OpenContext {
    /// An event channel whose sender wakes the UI after each send.
    pub fn channel(&self) -> (EventSink, Receiver<ListingEvent>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        (
            EventSink {
                tx,
                waker: self.waker.clone(),
            },
            rx,
        )
    }
}

/// Sender half of a listing channel. Never blocks; wakes the UI.
#[derive(Clone)]
pub struct EventSink {
    tx: Sender<ListingEvent>,
    waker: Option<Waker>,
}

impl EventSink {
    /// Queue an event. Returns false if the receiver is gone.
    pub fn send(&self, event: ListingEvent) -> bool {
        let ok = self.tx.send(event).is_ok();
        if let Some(w) = &self.waker {
            w();
        }
        ok
    }
}

/// An opened location. Dropping it (or `close`) stops all work for it.
pub trait OpenLocation: Send + Sync {
    fn location(&self) -> &Location;

    /// The event stream. Never blocks the producer: the channel is unbounded.
    fn events(&self) -> &Receiver<ListingEvent>;

    /// Ask for metadata on these entries (lazy stat). Results arrive as `Updated`.
    fn hydrate(&self, ids: &[EntryId], priority: Priority);

    /// Where this entry lives on disk, if anywhere. Operations need this.
    fn path_of(&self, id: EntryId) -> Option<PathBuf>;

    /// Re-list from scratch (emits `Removed` for gone entries, `Added` for new).
    fn refresh(&self);

    /// Stop listing, stat, and watching.
    fn close(&self);
}

/// Something that can open locations of one scheme.
pub trait LocationProvider: Send + Sync {
    fn scheme(&self) -> &str;
    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>>;
}

/// What to sort a listing by. Directories always come first.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Modified,
}

/// A pane's sort and filter settings; the sorted order is cached in the listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SortSpec {
    pub key: SortKey,
    pub ascending: bool,
    /// Include dot-prefixed entries.
    pub show_hidden: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        SortSpec {
            key: SortKey::Name,
            ascending: true,
            show_hidden: false,
        }
    }
}

#[derive(Debug)]
struct SortCache {
    /// The versions the order depends on: see [`Listing::sorted`].
    stamp: (u64, u64),
    spec: SortSpec,
    ids: Arc<[EntryId]>,
    /// How many entries the listing held when `ids` was made: the ones
    /// after, in insertion order, are arrivals it has not placed yet.
    seen: usize,
    /// `ids`' sort keys in brief, side by side, kept while the listing
    /// streams in so that arrivals find their place without looking every
    /// entry up (`Listing::place_arrivals`); empty once it is complete.
    ranks: Vec<Rank>,
}

/// An entry's sort key in brief: what `Listing::compare` compares, with
/// the name's collation key cut to its first sixteen bytes (eight tied a
/// quarter of the time: a run of digits takes two bytes of marker). Two ranks that
/// differ order their entries as the full keys do; equal ones are compared
/// in full.
#[derive(Clone, Copy, Debug)]
struct Rank {
    file: bool,
    num: u64,
    prefix: u128,
    id: EntryId,
}

impl Rank {
    fn of(key: SortKeyOf<'_>) -> Self {
        let mut prefix = [0u8; 16];
        let n = key.2.len().min(16);
        prefix[..n].copy_from_slice(&key.2[..n]);
        Rank {
            file: key.0,
            num: key.1,
            prefix: u128::from_be_bytes(prefix),
            id: key.3,
        }
    }
}

#[derive(Debug)]
struct Slot {
    entry: Entry,
    /// [`crate::collate::sort_key`] of the name, computed once per name.
    key: Box<[u8]>,
}

impl Slot {
    fn new(entry: Entry) -> Self {
        let key = crate::collate::sort_key(&entry.name);
        Slot { entry, key }
    }
}

/// What one replacement changed, for the versions that depend on it.
// One flag per version it bumps; a state machine would say less.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct Delta {
    any: bool,
    name: bool,
    /// Something already listed may have moved. An arrival alone does not
    /// move anything: it is placed among the rest (`Listing::sorted`).
    order: bool,
    size: bool,
    modified: bool,
}

impl Delta {
    fn between(old: &Entry, new: &Entry) -> Self {
        use crate::entry::keys;
        let name = old.name != new.name;
        let size = old.metadata.value(keys::SIZE) != new.metadata.value(keys::SIZE);
        let modified = old.metadata.value(keys::MODIFIED) != new.metadata.value(keys::MODIFIED);
        Delta {
            any: old != new,
            name,
            order: name || old.kind != new.kind,
            size,
            modified,
        }
    }

    /// Entries that went: whatever was placed after them moves.
    fn removal() -> Self {
        Delta {
            any: true,
            name: true,
            order: true,
            size: true,
            modified: true,
        }
    }
}

/// What an entry sorts by: see `Listing::sort_key`.
type SortKeyOf<'a> = (bool, u64, &'a [u8], EntryId);

/// Every version of every listing is read from this one clock, so a value
/// names one state of one listing. A pane that reopens its folder gets a
/// new listing whose versions cannot repeat the old one's: work keyed on a
/// version (a sorted order, requests already sent, measured names) sees the
/// replacement even though the location reads the same.
fn stamp() -> u64 {
    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// An in-memory view of a listing, kept up to date by applying events.
/// Insertion order is preserved so streaming order is stable; `sorted`
/// gives a cached order for a [`SortSpec`].
#[derive(Debug)]
pub struct Listing {
    /// Which listing this is; see [`stamp`].
    id: u64,
    entries: IndexMap<EntryId, Slot>,
    complete: bool,
    error: Option<String>,
    /// Incremented on every change, so views know when to recompute. An
    /// event that changes nothing (a stat that found what the listing
    /// already had) leaves it alone, so work keyed on it cannot loop.
    version: u64,
    /// Incremented when a name appears, disappears or changes. Hydration
    /// leaves it alone, so work that depends only on names (a grid counting
    /// the lines each one wraps to) is not redone for every metadata batch.
    names_version: u64,
    /// Incremented when a change can move an entry already listed under
    /// every sort: a removal, a new name or kind. Arrivals leave it alone:
    /// they are appended to `entries`, and `sorted` places them.
    order_version: u64,
    /// Incremented when a size or a modification time changes: only the
    /// sorts by those keys depend on them.
    size_version: u64,
    modified_version: u64,
    sort_cache: parking_lot::Mutex<Option<SortCache>>,
}

impl Default for Listing {
    fn default() -> Self {
        let id = stamp();
        Listing {
            id,
            entries: IndexMap::new(),
            complete: false,
            error: None,
            version: id,
            names_version: id,
            order_version: id,
            size_version: id,
            modified_version: id,
            sort_cache: parking_lot::Mutex::new(None),
        }
    }
}

impl Listing {
    /// Apply one event. Returns whether the listing changed.
    pub fn apply(&mut self, event: ListingEvent) -> bool {
        let mut delta = Delta::default();
        match event {
            ListingEvent::Added(batch) => {
                for e in batch {
                    self.upsert(e, true, &mut delta);
                }
            }
            ListingEvent::Updated(batch) => {
                for e in batch {
                    self.upsert(e, false, &mut delta);
                }
            }
            ListingEvent::Removed(ids) => {
                let before = self.entries.len();
                if ids.len() > 16 {
                    // One pass instead of an O(n) shift per id.
                    let gone: std::collections::HashSet<EntryId> = ids.into_iter().collect();
                    self.entries.retain(|id, _| !gone.contains(id));
                } else {
                    for id in ids {
                        self.entries.shift_remove(&id);
                    }
                }
                if self.entries.len() != before {
                    delta = Delta::removal();
                }
            }
            ListingEvent::Complete => {
                delta.any = !self.complete || self.error.is_some();
                self.complete = true;
                self.error = None;
            }
            ListingEvent::Error(e) => {
                delta.any = self.error.as_deref() != Some(e.as_str());
                self.error = Some(e);
            }
        }
        for (moved, version) in [
            (delta.name, &mut self.names_version),
            (delta.order, &mut self.order_version),
            (delta.size, &mut self.size_version),
            (delta.modified, &mut self.modified_version),
            (delta.any, &mut self.version),
        ] {
            if moved {
                *version = stamp();
            }
        }
        delta.any
    }

    fn upsert(&mut self, mut e: Entry, insert: bool, delta: &mut Delta) {
        match self.entries.entry(e.id) {
            indexmap::map::Entry::Occupied(mut slot) => {
                let old = &slot.get().entry;
                // A re-listing (refresh, a watcher's rescan) sends bare
                // entries: keep what hydration found rather than blanking
                // every column until the rows are stat-ed again.
                if !e.metadata.is_hydrated() && old.metadata.is_hydrated() && old.name == e.name {
                    e.metadata = old.metadata.clone();
                    e.kind = old.kind;
                }
                let d = Delta::between(old, &e);
                if !d.any {
                    return;
                }
                if d.name {
                    *slot.get_mut() = Slot::new(e);
                } else {
                    slot.get_mut().entry = e;
                }
                delta.any = true;
                delta.name |= d.name;
                delta.order |= d.order;
                delta.size |= d.size;
                delta.modified |= d.modified;
            }
            indexmap::map::Entry::Vacant(slot) => {
                if insert {
                    slot.insert(Slot::new(e));
                    // A new name; nothing listed moved (see `order`).
                    delta.any = true;
                    delta.name = true;
                }
            }
        }
    }

    /// Changes so far; equal versions mean identical contents.
    /// Which listing this is: a pane that reopens its location, even the
    /// same one, holds a listing with another id.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Changes, unique across listings: see [`stamp`].
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Changes to the set of names so far; see the field.
    pub fn names_version(&self) -> u64 {
        self.names_version
    }

    /// The ids in `spec` order, directories first. Cached until something
    /// the order depends on changes: a metadata batch re-sorts only a sort
    /// by size or date, so calling this every frame is cheap. Entries that
    /// arrived since are placed in the cached order rather than sorting
    /// again: a folder of 366 000 entries streams in hundreds of batches,
    /// and sorting it whole after each one cost a tenth of a second a time.
    /// Until the listing is complete, the order may leave out the latest
    /// arrivals (see the placement rule below); once it is, it holds every
    /// shown entry.
    pub fn sorted(&self, spec: SortSpec) -> Arc<[EntryId]> {
        let stamp = (
            self.order_version,
            match spec.key {
                SortKey::Name => 0,
                SortKey::Size => self.size_version,
                SortKey::Modified => self.modified_version,
            },
        );
        let mut cache = self.sort_cache.lock();
        if let Some(c) = cache.as_mut()
            && c.stamp == stamp
            && c.spec == spec
            && (c.seen == self.entries.len()
                || (c.seen < self.entries.len() && !c.ranks.is_empty()))
        {
            // While the listing streams in, arrivals are placed once they
            // are a quarter of what is placed: each placement copies the
            // order, so placing every batch of a folder of 366 000 entries
            // copied it hundreds of times. Growing by quarters, the copies
            // add up to a few times the final order; the order shown lags by
            // at most that much, and is whole once the listing completes.
            let arrivals = self.entries.len() - c.seen;
            if arrivals > 0 && (self.complete || arrivals * 4 >= c.ranks.len()) {
                self.place_arrivals(c, spec);
            }
            if self.complete {
                c.ranks = Vec::new();
            }
            return Arc::clone(&c.ids);
        }
        let keyed = self.keyed(spec);
        let ids: Arc<[EntryId]> = keyed.iter().map(|key| key.3).collect();
        // Arrivals after completion are the watcher's, one at a time: a
        // full sort for those, and no ranks kept once nothing streams in.
        let ranks = if self.complete {
            Vec::new()
        } else {
            keyed.into_iter().map(Rank::of).collect()
        };
        *cache = Some(SortCache {
            stamp,
            spec,
            ids: Arc::clone(&ids),
            seen: self.entries.len(),
            ranks,
        });
        ids
    }

    /// What an entry is sorted by under `spec`: files after directories,
    /// then the size or date (0 for a name sort, or when unknown), then the
    /// name's collation key, then the id, so that no two entries tie: the
    /// order is one order, whether made whole or by placing arrivals, and
    /// equal names do not trade places from one sort to the next.
    fn sort_key(spec: SortSpec, slot: &Slot) -> SortKeyOf<'_> {
        use crate::entry::{Value, keys};
        let e = &slot.entry;
        let num = match spec.key {
            SortKey::Name => 0,
            SortKey::Size => e
                .metadata
                .value(keys::SIZE)
                .and_then(Value::as_u64)
                .unwrap_or(0),
            SortKey::Modified => e
                .metadata
                .value(keys::MODIFIED)
                .and_then(Value::as_time)
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
        };
        (!e.is_dir(), num, &*slot.key, e.id)
    }

    fn compare(spec: SortSpec, a: SortKeyOf<'_>, b: SortKeyOf<'_>) -> std::cmp::Ordering {
        a.0.cmp(&b.0)
            .then_with(|| {
                let o = a.1.cmp(&b.1).then_with(|| a.2.cmp(b.2));
                if spec.ascending { o } else { o.reverse() }
            })
            .then_with(|| a.3.cmp(&b.3))
    }

    fn shown(spec: SortSpec, slot: &Slot) -> bool {
        spec.show_hidden || !slot.entry.name.as_encoded_bytes().starts_with(b".")
    }

    /// The shown entries' sort keys, in order.
    fn keyed(&self, spec: SortSpec) -> Vec<SortKeyOf<'_>> {
        let mut keyed: Vec<SortKeyOf<'_>> = self
            .entries
            .values()
            .filter(|slot| Self::shown(spec, slot))
            .map(|slot| Self::sort_key(spec, slot))
            .collect();
        keyed.sort_unstable_by(|a, b| Self::compare(spec, *a, *b));
        keyed
    }

    #[cfg(test)]
    fn compute_sorted(&self, spec: SortSpec) -> Arc<[EntryId]> {
        self.keyed(spec).iter().map(|key| key.3).collect()
    }

    /// Where `placed` (a rank of the cached order) stands against `key`,
    /// as `compare` says: by the ranks, and by the full keys when they tie.
    fn compare_rank(
        &self,
        spec: SortSpec,
        placed: &Rank,
        key: SortKeyOf<'_>,
        rank: &Rank,
    ) -> std::cmp::Ordering {
        placed
            .file
            .cmp(&rank.file)
            .then_with(|| {
                let o = (placed.num, placed.prefix)
                    .cmp(&(rank.num, rank.prefix))
                    .then_with(|| {
                        let slot = self.entries.get(&placed.id).expect("a ranked id is listed");
                        (*slot.key).cmp(key.2)
                    });
                if spec.ascending { o } else { o.reverse() }
            })
            .then_with(|| placed.id.cmp(&rank.id))
    }

    /// The cached order with the entries after its first `seen` put in their
    /// places: the arrivals are sorted among themselves, then merged with
    /// the ranks in one pass, which the copy of the order costs anyway. Removals and moves never come here; they change
    /// `order_version`, and the order is made again.
    fn place_arrivals(&self, cache: &mut SortCache, spec: SortSpec) {
        let mut arrivals: Vec<SortKeyOf<'_>> = self.entries.as_slice()[cache.seen..]
            .values()
            .filter(|slot| Self::shown(spec, slot))
            .map(|slot| Self::sort_key(spec, slot))
            .collect();
        arrivals.sort_unstable_by(|a, b| Self::compare(spec, *a, *b));
        let placed = &cache.ranks;
        let mut ranks = Vec::with_capacity(placed.len() + arrivals.len());
        let mut old = placed.iter().peekable();
        for key in arrivals {
            let rank = Rank::of(key);
            while let Some(p) = old
                .next_if(|p| self.compare_rank(spec, p, key, &rank) != std::cmp::Ordering::Greater)
            {
                ranks.push(*p);
            }
            ranks.push(rank);
        }
        ranks.extend(old.copied());
        let ids: Vec<EntryId> = ranks.iter().map(|r| r.id).collect();
        cache.ids = ids.into();
        cache.ranks = ranks;
        cache.seen = self.entries.len();
    }

    /// Apply what was queued when called, until `budget` has passed, without
    /// blocking; at least one event. Returns how many events changed the
    /// listing. What is left waits for the next call: a location still
    /// streaming never keeps the caller (the UI thread) here, and a backlog
    /// is applied a slice at a time, with frames drawn in between.
    pub fn drain(&mut self, rx: &Receiver<ListingEvent>, budget: std::time::Duration) -> usize {
        let started = std::time::Instant::now();
        let mut n = 0;
        for _ in 0..rx.len() {
            let Ok(ev) = rx.try_recv() else { break };
            n += usize::from(self.apply(ev));
            if started.elapsed() >= budget {
                break;
            }
        }
        n
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.entries.get(&id).map(|s| &s.entry)
    }

    pub fn get_index(&self, i: usize) -> Option<&Entry> {
        self.entries.get_index(i).map(|(_, s)| &s.entry)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values().map(|s| &s.entry)
    }

    pub fn ids(&self) -> impl Iterator<Item = EntryId> + '_ {
        self.entries.keys().copied()
    }

    pub fn find_by_name(&self, name: &OsStr) -> Option<&Entry> {
        self.iter().find(|e| e.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::EntryKind;

    #[test]
    fn names_version_ignores_hydration() {
        let file = |ino, name: &str| Entry::new(EntryId { dev: 1, ino }, name, EntryKind::File);
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![
            file(1, "a.txt"),
            file(2, "b.txt"),
        ]));
        let (names, version) = (listing.names_version(), listing.version());

        // The same entries again, as a metadata batch sends them.
        listing.apply(ListingEvent::Updated(vec![file(1, "a.txt")]));
        listing.apply(ListingEvent::Complete);
        assert_eq!(listing.names_version(), names);
        assert!(listing.version() > version);

        // A rename keeps the inode, so the id: only the name tells.
        listing.apply(ListingEvent::Updated(vec![file(1, "renamed.txt")]));
        let renamed = listing.names_version();
        assert!(renamed > names);
        listing.apply(ListingEvent::Removed(vec![EntryId { dev: 1, ino: 9 }]));
        assert_eq!(listing.names_version(), renamed, "nothing was removed");
        listing.apply(ListingEvent::Removed(vec![EntryId { dev: 1, ino: 2 }]));
        assert!(listing.names_version() > renamed);
    }

    #[test]
    fn a_reopened_location_never_repeats_a_version() {
        // A pane that reopens the folder it shows gets a fresh listing of the
        // same entries. Work keyed on the old one's versions (the hydration
        // it asked for, its sorted order) must see the change.
        let file = |ino, name: &str| Entry::new(EntryId { dev: 1, ino }, name, EntryKind::File);
        let mut old = Listing::default();
        old.apply(ListingEvent::Added(vec![file(1, "a"), file(2, "b")]));
        let mut new = Listing::default();
        assert_ne!(new.id(), old.id());
        new.apply(ListingEvent::Added(vec![file(1, "a"), file(2, "b")]));
        for (a, b) in [
            (old.version(), new.version()),
            (old.names_version(), new.names_version()),
        ] {
            assert_ne!(a, b);
        }
        assert_ne!(Listing::default().version(), Listing::default().version());
    }

    fn stat(mut e: Entry, size: u64) -> Entry {
        use crate::entry::{Provenance, Value, keys};
        e.metadata
            .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
        e
    }

    #[test]
    fn a_relisting_keeps_what_hydration_found() {
        let file = |ino, name: &str| Entry::new(EntryId { dev: 1, ino }, name, EntryKind::File);
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![file(1, "a"), file(2, "b")]));
        listing.apply(ListingEvent::Updated(vec![stat(file(1, "a"), 7)]));
        let version = listing.version();

        // A refresh sends the same entries bare: nothing changes.
        assert!(!listing.apply(ListingEvent::Added(vec![file(1, "a"), file(2, "b")])));
        assert_eq!(listing.version(), version);
        assert!(
            listing
                .get(EntryId { dev: 1, ino: 1 })
                .unwrap()
                .metadata
                .is_hydrated()
        );

        // A stat that finds the same thing again changes nothing either, so
        // work keyed on the version cannot loop on it.
        assert!(!listing.apply(ListingEvent::Updated(vec![stat(file(1, "a"), 7)])));
        assert_eq!(listing.version(), version);

        // An inode reused under another name is another file.
        listing.apply(ListingEvent::Added(vec![file(1, "c")]));
        assert!(
            !listing
                .get(EntryId { dev: 1, ino: 1 })
                .unwrap()
                .metadata
                .is_hydrated()
        );
    }

    #[test]
    fn a_late_update_does_not_resurrect_a_removed_entry() {
        let file = |ino, name: &str| Entry::new(EntryId { dev: 1, ino }, name, EntryKind::File);
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![file(1, "a")]));
        listing.apply(ListingEvent::Removed(vec![EntryId { dev: 1, ino: 1 }]));
        assert!(!listing.apply(ListingEvent::Updated(vec![stat(file(1, "a"), 1)])));
        assert!(listing.is_empty());
    }

    #[test]
    fn hydration_re_sorts_only_the_sorts_that_depend_on_it() {
        let file = |ino, name: &str| Entry::new(EntryId { dev: 1, ino }, name, EntryKind::File);
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added(vec![
            file(1, "f10"),
            file(2, "f2"),
            file(3, "É"),
            file(4, "z"),
        ]));
        let by_name = SortSpec::default();
        let by_size = SortSpec {
            key: SortKey::Size,
            ..SortSpec::default()
        };
        let names = listing.sorted(by_name);
        let ino: Vec<u64> = names.iter().map(|id| id.ino).collect();
        assert_eq!(ino, [3, 2, 1, 4], "natural and case-folded");
        listing.apply(ListingEvent::Updated(vec![stat(file(4, "z"), 1)]));
        assert!(
            Arc::ptr_eq(&names, &listing.sorted(by_name)),
            "not sorted again"
        );
        let sizes: Vec<u64> = listing.sorted(by_size).iter().map(|id| id.ino).collect();
        assert_eq!(sizes, [3, 2, 1, 4]);
        listing.apply(ListingEvent::Updated(vec![stat(file(2, "f2"), 9)]));
        let sizes: Vec<u64> = listing.sorted(by_size).iter().map(|id| id.ino).collect();
        assert_eq!(sizes, [3, 1, 4, 2]);
    }

    #[test]
    fn large_removals_keep_the_order_of_what_remains() {
        let file = |ino| Entry::new(EntryId { dev: 1, ino }, format!("n{ino}"), EntryKind::File);
        let mut listing = Listing::default();
        listing.apply(ListingEvent::Added((0..100).map(file).collect()));
        listing.apply(ListingEvent::Removed(
            (0..100)
                .filter(|i| i % 3 != 0)
                .map(|ino| EntryId { dev: 1, ino })
                .collect(),
        ));
        let left: Vec<u64> = listing.ids().map(|id| id.ino).collect();
        assert_eq!(left, (0..100).filter(|i| i % 3 == 0).collect::<Vec<_>>());
    }

    #[cfg(unix)]
    #[test]
    fn path_locations_keep_names_that_are_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let raw = Path::new(OsStr::from_bytes(b"/example/caf\xe9"));
        let loc = Location::from_path(raw);
        assert_eq!(loc.path().as_deref(), Some(raw));
        let json = serde_json::to_value(&loc).unwrap();
        let back: Location = serde_json::from_value(json).unwrap();
        assert_eq!(back, loc);
        assert_eq!(Location::parse_os(raw.as_os_str()), loc);
        assert_eq!(
            Location::from_path("/example/a"),
            Location::parse("/example/a")
        );
    }

    /// Arrivals placed in the cached order give the order a full sort
    /// gives, under every sort, hidden or not, however the batches fall;
    /// a removal or a rename in between is sorted again, and the complete
    /// listing's order holds everything.
    #[test]
    fn arrivals_are_placed_where_a_full_sort_puts_them() {
        use crate::entry::{Provenance, Value, keys};
        let entry = |ino: u64| {
            // Names that repeat and differ in case, some hidden, some
            // folders, sizes that tie: the orders' tie-breaks are exercised.
            let name = match ino % 5 {
                0 => format!(".hidden-{}", ino % 7),
                1 => format!("Name {}", ino % 11),
                2 => format!("name {}", ino % 11),
                _ => format!("é{:03}", ino.wrapping_mul(37) % 101),
            };
            let kind = if ino.is_multiple_of(3) {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            let mut e = Entry::new(EntryId { dev: 1, ino }, &name, kind);
            e.metadata
                .set(keys::SIZE, Value::UInt(ino % 4), Provenance::Filesystem);
            e
        };
        for key in [SortKey::Name, SortKey::Size, SortKey::Modified] {
            for (ascending, show_hidden) in [(true, false), (false, true), (true, true)] {
                let spec = SortSpec {
                    key,
                    ascending,
                    show_hidden,
                };
                let mut listing = Listing::default();
                let mut next = 0;
                for batch in [1, 7, 0, 30, 2, 64, 5] {
                    let arrivals = (next..next + batch).map(entry).collect();
                    next += batch;
                    listing.apply(ListingEvent::Added(arrivals));
                    // While it streams, the order may leave arrivals out
                    // but never misplaces what it holds.
                    let shown = listing.sorted(spec);
                    let held: std::collections::HashSet<_> = shown.iter().collect();
                    let whole: Vec<_> = listing
                        .compute_sorted(spec)
                        .iter()
                        .filter(|id| held.contains(id))
                        .copied()
                        .collect();
                    assert_eq!(*shown, *whole, "{spec:?}");
                    if batch == 30 {
                        listing.apply(ListingEvent::Removed(vec![EntryId { dev: 1, ino: 3 }]));
                        listing.apply(ListingEvent::Updated(vec![Entry::new(
                            EntryId { dev: 1, ino: 4 },
                            "aaa renamed",
                            EntryKind::File,
                        )]));
                        assert_eq!(*listing.sorted(spec), *listing.compute_sorted(spec));
                    }
                }
                listing.apply(ListingEvent::Complete);
                assert_eq!(
                    *listing.sorted(spec),
                    *listing.compute_sorted(spec),
                    "{spec:?}"
                );
            }
        }
    }

    /// A location that keeps sending never keeps the drain: it applies
    /// what was queued when called, and leaves the rest for the next call.
    #[test]
    fn a_drain_leaves_what_arrives_during_it_for_the_next() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let file = |ino: u64| Entry::new(EntryId { dev: 1, ino }, ino.to_string(), EntryKind::File);
        for ino in 0..3 {
            tx.send(ListingEvent::Added(vec![file(ino)])).unwrap();
        }
        let mut listing = Listing::default();
        let long = std::time::Duration::from_secs(60);
        assert_eq!(listing.drain(&rx, long), 3);
        tx.send(ListingEvent::Added(vec![file(9)])).unwrap();
        tx.send(ListingEvent::Added(vec![file(10)])).unwrap();
        // No time at all: one event, the other waits.
        assert_eq!(listing.drain(&rx, std::time::Duration::ZERO), 1);
        assert_eq!(rx.len(), 1);
        assert_eq!(listing.drain(&rx, long), 1);
        assert_eq!(listing.len(), 5);
    }
}
