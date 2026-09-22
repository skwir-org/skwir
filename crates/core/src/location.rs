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

    fn arrival() -> Self {
        Delta {
            any: true,
            name: true,
            order: true,
            size: true,
            modified: true,
        }
    }
}

/// An in-memory view of a listing, kept up to date by applying events.
/// Insertion order is preserved so streaming order is stable; `sorted`
/// gives a cached order for a [`SortSpec`].
#[derive(Debug, Default)]
pub struct Listing {
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
    /// Incremented when a change can move an entry under every sort: an
    /// arrival, a removal, a new name or kind.
    order_version: u64,
    /// Incremented when a size or a modification time changes: only the
    /// sorts by those keys depend on them.
    size_version: u64,
    modified_version: u64,
    sort_cache: parking_lot::Mutex<Option<SortCache>>,
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
                    delta = Delta::arrival();
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
        self.names_version += u64::from(delta.name);
        self.order_version += u64::from(delta.order);
        self.size_version += u64::from(delta.size);
        self.modified_version += u64::from(delta.modified);
        self.version += u64::from(delta.any);
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
                    *delta = Delta::arrival();
                }
            }
        }
    }

    /// Changes so far; equal versions mean identical contents.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Changes to the set of names so far; see the field.
    pub fn names_version(&self) -> u64 {
        self.names_version
    }

    /// The ids in `spec` order, directories first. Cached until something
    /// the order depends on changes: a metadata batch re-sorts only a sort
    /// by size or date, so calling this every frame is cheap.
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
        if let Some(c) = cache.as_ref()
            && c.stamp == stamp
            && c.spec == spec
        {
            return Arc::clone(&c.ids);
        }
        let ids = self.compute_sorted(spec);
        *cache = Some(SortCache {
            stamp,
            spec,
            ids: Arc::clone(&ids),
        });
        ids
    }

    fn compute_sorted(&self, spec: SortSpec) -> Arc<[EntryId]> {
        use crate::entry::{Value, keys};
        let mut keyed: Vec<(bool, u64, &[u8], EntryId)> = self
            .entries
            .values()
            .filter(|s| spec.show_hidden || !s.entry.name.as_encoded_bytes().starts_with(b"."))
            .map(|Slot { entry: e, key }| {
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
                (!e.is_dir(), num, &**key, e.id)
            })
            .collect();
        keyed.sort_unstable_by(|a, b| {
            a.0.cmp(&b.0).then_with(|| {
                let o = a.1.cmp(&b.1).then_with(|| a.2.cmp(b.2));
                if spec.ascending { o } else { o.reverse() }
            })
        });
        keyed.into_iter().map(|(_, _, _, id)| id).collect()
    }

    /// Drain everything currently available without blocking. Returns how
    /// many events changed the listing.
    pub fn drain(&mut self, rx: &Receiver<ListingEvent>) -> usize {
        let mut n = 0;
        while let Ok(ev) = rx.try_recv() {
            n += usize::from(self.apply(ev));
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
        assert_eq!(listing.names_version(), names + 1);
        listing.apply(ListingEvent::Removed(vec![EntryId { dev: 1, ino: 9 }]));
        assert_eq!(listing.names_version(), names + 1, "nothing was removed");
        listing.apply(ListingEvent::Removed(vec![EntryId { dev: 1, ino: 2 }]));
        assert_eq!(listing.names_version(), names + 2);
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
}
