//! All filesystem access, monitoring and cache eviction happen here.
//!
//! Coverage is decided per directory. Each directory receives its own
//! non-recursive watch immediately before it is read, and a breakdown enters
//! the cache only while every directory below it is watched. A directory that
//! cannot be read is a read error (the totals are marked incomplete); only a
//! directory that can be read but not watched, or an exhausted watch budget,
//! reduces coverage for its request. Requests without full coverage are
//! revalidated periodically at an interval proportional to their last scan.
//!
//! Requests are scanned by canonical path. Every symlink component of the
//! requested path is watched through its parent directory, so replacing an
//! alias re-resolves the request instead of falling back to polling.
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use notify::{ErrorKind, EventKind, RecursiveMode, Watcher, event::ModifyKind};
use parking_lot::Mutex;
use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::EntryKind;
use skwir_core::words;
use skwir_core::words::Words;
use skwir_core::workspace::PaneId;
use skwir_core::{EntryId, FileId};

use super::marks::{self, Places, Types};

mod indexed;
use super::{Linked, Links, Message, Node, Request, Shared, SnapshotKind, Status};

const MAX_CHILDREN: usize = 64;
const MAX_CROWDED: usize = 16;
/// Levels of folders sent inside the shown folder's own entries, at most:
/// the map draws as many of them as its zoom asks and its blocks have
/// room for (`nest`).
const NESTED_LEVELS: usize = 3;
/// What a folder must weigh, as a share of the shown folder, for what it
/// holds to be sent, and what an entry inside it must weigh to be sent on
/// its own rather than in its folder's other items: the map would draw
/// smaller ones as specks.
const OPEN_SHARE: u64 = 256;
const ENTRY_SHARE: u64 = 1024;
/// Entries sent inside folders, in all: a status is copied and read whole
/// each time it changes.
const MAX_NESTED: usize = 1024;
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIRECTORIES: usize = 4096;
const MAX_WATCH_ROOTS: usize = 4;
/// Minimum interval between revalidations of a request without full coverage.
pub(super) const REVALIDATE: Duration = Duration::from_secs(2);
/// A revalidation waits at least this many times the previous scan's duration,
/// so polling a large tree never becomes a full-duty-cycle loop; and so does
/// a change that comes this soon after a recount a change asked for, so a
/// folder that keeps changing (a copy into it) is counted again at most a
/// fifth of the time. A change after a while is counted at once.
const REVALIDATE_DUTY: u32 = 4;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
/// After the watch budget is exhausted, registrations are not retried before this.
const WATCH_BACKOFF: Duration = Duration::from_secs(30);
const MAX_WATCH_BACKOFF: Duration = Duration::from_secs(600);
/// How long the watches and the cache are kept once no view shows a
/// breakdown: what a view shown again soon finds at once, let go of after.
const RELEASE_IDLE: Duration = Duration::from_secs(120);
/// Never descended into, even on the request's own device.
const VIRTUAL_FILESYSTEMS: [&str; 2] = ["/proc", "/sys"];

/// Eviction order: unprotected before protected, deepest first, then oldest.
/// Shallow breakdowns aggregate the most work, so the deepest entries (cheap
/// to recompute) go first. Direct children of active roots are protected: a
/// change below one of them must reuse its unchanged siblings.
type Key = (bool, Reverse<usize>, u64);

/// What keeps a breakdown kept current.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kept {
    /// Usage's own watches, one on each folder in it: its scan counted it.
    Watched,
    /// The journal of the names index it was taken from (ADR-150, D3),
    /// followed at or above it while it is kept (`Cache::journals`).
    Journaled,
}

/// A names index's journal followed at a folder: what keeps the
/// breakdowns taken from it current (`Worker::journals`).
struct Followed {
    /// Where its changes are asked from next: never past one a breakdown
    /// kept may not have counted.
    mark: skwir_core::names::Mark,
    /// The folders below not covered, as it last said: one more is a
    /// folder whose changes it tells no more, counted again whole.
    holes: BTreeSet<PathBuf>,
}

#[derive(Default)]
struct Cache {
    /// Ordered by path so a folder's descendants are one range (see [`below`]).
    dirs: BTreeMap<PathBuf, (Node, Key, Kept)>,
    order: BTreeMap<Key, PathBuf>,
    /// Canonical paths of the active requests.
    roots: Vec<PathBuf>,
    clock: u64,
    bytes: usize,
    /// The journals the breakdowns taken from a names index are kept
    /// current by, by the folder each is followed at. A breakdown from an
    /// index is never kept without one at or above it
    /// ([`insert_journaled`](Self::insert_journaled)); one let go of takes
    /// what it kept with it.
    journals: BTreeMap<PathBuf, Followed>,
}
fn cost(n: &Node) -> usize {
    std::mem::size_of::<Node>()
        + n.path.as_os_str().len() * 3
        + n.name.len()
        + n.links.0.len() * 2 * std::mem::size_of::<(FileId, Linked)>()
        + n.kinds.heap()
        + n.children.iter().map(cost).sum::<usize>()
}
impl Cache {
    fn protected(roots: &[PathBuf], path: &Path) -> bool {
        roots
            .iter()
            .any(|root| path == root || path.parent() == Some(root))
    }
    fn key(&mut self, path: &Path) -> Key {
        self.clock += 1;
        (
            Self::protected(&self.roots, path),
            Reverse(path.components().count()),
            self.clock,
        )
    }
    /// A breakdown kept, to be shown as the folder measured: whatever
    /// keeps it current.
    fn get(&mut self, path: &Path) -> Option<Node> {
        self.take_up(path, |_| true)
    }
    /// A breakdown kept that a scan of the disk counts with: one it
    /// counted itself. One from an index carries no files of several names
    /// (`Node::links`), so a folder above holding another of their names
    /// would count them twice.
    fn counted(&mut self, path: &Path) -> Option<Node> {
        self.take_up(path, |kept| kept == Kept::Watched)
    }
    fn take_up(&mut self, path: &Path, accept: impl Fn(Kept) -> bool) -> Option<Node> {
        if !self
            .dirs
            .get(path)
            .is_some_and(|(_, _, kept)| accept(*kept))
        {
            return None;
        }
        let key = self.key(path);
        let (node, stamp, _) = self.dirs.get_mut(path)?;
        self.order.remove(stamp);
        *stamp = key;
        self.order.insert(key, path.to_path_buf());
        Some(node.clone())
    }
    /// A breakdown kept, read to be shown: showing it does not make it
    /// younger, only counting with it does.
    fn peek(&self, path: &Path) -> Option<&Node> {
        self.dirs.get(path).map(|(node, _, _)| node)
    }
    /// Keep a breakdown a scan counted, its folders watched.
    fn insert(&mut self, node: Node) {
        self.put(node, Kept::Watched);
    }
    /// Keep a breakdown taken from a names index, with the journal
    /// followed at `at` (at or above it) that keeps it current: `mark`,
    /// where it stood as the index first summed, and the folders it did
    /// not cover then. A journal followed at or above `at` already is kept
    /// as it stands: its mark is older, so what changed since is told
    /// whatever was summed after it, a breakdown kept from before included.
    fn insert_journaled(
        &mut self,
        node: Node,
        at: &Path,
        mark: &skwir_core::names::Mark,
        holes: &BTreeSet<PathBuf>,
    ) {
        if !at.ancestors().any(|a| self.journals.contains_key(a)) {
            self.journals.insert(
                at.to_path_buf(),
                Followed {
                    mark: mark.clone(),
                    holes: holes.clone(),
                },
            );
        }
        self.put(node, Kept::Journaled);
    }
    fn put(&mut self, node: Node, kept: Kept) {
        if let Some((old, stamp, _)) = self.dirs.remove(&node.path) {
            self.bytes -= cost(&old);
            self.order.remove(&stamp);
        }
        let key = self.key(&node.path);
        self.bytes += cost(&node);
        self.order.insert(key, node.path.clone());
        self.dirs.insert(node.path.clone(), (node, key, kept));
        while self.bytes > MAX_CACHE_BYTES || self.dirs.len() > MAX_DIRECTORIES {
            let Some((_, path)) = self.order.pop_first() else {
                break;
            };
            if let Some((node, _, _)) = self.dirs.remove(&path) {
                self.bytes -= cost(&node);
            }
        }
    }
    /// Whether a breakdown from an index is kept at or below `folder`.
    fn journaled_below(&self, folder: &Path) -> bool {
        use std::ops::Bound;
        self.dirs
            .range::<Path, _>((Bound::Included(folder), Bound::Unbounded))
            .take_while(|(p, _)| p.starts_with(folder))
            .any(|(_, (_, _, kept))| *kept == Kept::Journaled)
    }
    /// Drop `path` and its ancestors (their totals include it); with
    /// `descendants`, also everything below it. Lookups, not a scan: a
    /// deleted tree sends an event per folder.
    fn invalidate(&mut self, path: &Path, descendants: bool) {
        let mut doomed: Vec<PathBuf> = path
            .ancestors()
            .filter(|a| self.dirs.contains_key(*a))
            .map(Path::to_path_buf)
            .collect();
        if descendants {
            doomed.extend(below(&self.dirs, path).filter(|p| *p != path).cloned());
        }
        for p in doomed {
            if let Some((node, stamp, _)) = self.dirs.remove(&p) {
                self.bytes -= cost(&node);
                self.order.remove(&stamp);
            }
        }
    }
    fn set_roots(&mut self, mut roots: Vec<PathBuf>) {
        roots.sort();
        roots.dedup();
        if roots == self.roots {
            return;
        }
        self.roots = roots;
        self.order.clear();
        for (path, (_, key, _)) in &mut self.dirs {
            key.0 = Self::protected(&self.roots, path);
            self.order.insert(*key, path.clone());
        }
    }
    /// Let go of every breakdown, and of the journals that kept some.
    fn clear(&mut self) {
        self.dirs.clear();
        self.order.clear();
        self.bytes = 0;
        self.journals.clear();
    }
}

/// What an event means for cached breakdowns and registrations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Change {
    /// Permissions or timestamps: sizes are unchanged; the entry and its
    /// ancestors are re-read so a directory that became readable is counted.
    Attribute,
    /// Contents changed or an entry appeared: the subtree is recounted.
    Content,
    /// The entry was removed or renamed: registrations below it are gone.
    Structural,
}
impl Change {
    fn classify(kind: EventKind) -> Option<Self> {
        match kind {
            EventKind::Access(_) => None,
            EventKind::Modify(ModifyKind::Metadata(_)) => Some(Self::Attribute),
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Data(_)) => Some(Self::Content),
            EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Name(_) | ModifyKind::Any | ModifyKind::Other)
            | EventKind::Any
            | EventKind::Other => Some(Self::Structural),
        }
    }
}

/// Bound notifications even when a long initial scan overlaps a large copy.
#[derive(Default)]
struct Changes {
    paths: HashMap<PathBuf, Change>,
    lost: bool,
    pending: bool,
}
impl Changes {
    fn push(&mut self, event: notify::Result<notify::Event>) -> bool {
        match event {
            Ok(event) if !event.need_rescan() && !event.paths.is_empty() => {
                let Some(change) = Change::classify(event.kind) else {
                    return false;
                };
                if !self.lost {
                    for path in event.paths {
                        let known = self.paths.entry(path).or_insert(change);
                        *known = (*known).max(change);
                    }
                    if self.paths.len() > 4096 {
                        self.paths.clear();
                        self.lost = true;
                    }
                }
            }
            _ => {
                self.paths.clear();
                self.lost = true;
            }
        }
        let notify = !self.pending;
        self.pending = true;
        notify
    }
}

/// Check before advancing the OS iterator, not only after it yielded an entry.
fn entries<'a>(
    path: &Path,
    cancel: &'a CancelToken,
) -> anyhow::Result<impl Iterator<Item = std::io::Result<std::fs::DirEntry>> + use<'a>> {
    anyhow::ensure!(!cancel.is_cancelled(), "Scan cancelled");
    let mut entries = std::fs::read_dir(path)?;
    Ok(std::iter::from_fn(move || {
        if cancel.is_cancelled() {
            None
        } else {
            entries.next()
        }
    }))
}

fn virtual_filesystem(path: &Path) -> bool {
    VIRTUAL_FILESYSTEMS.iter().any(|v| path.starts_with(v))
}

/// What the walk reads of a file's metadata. Only Unix is read so far
/// (ADR-035): elsewhere `Scanner::list_root` refuses before any is asked.
#[cfg(unix)]
mod meta {
    use std::fs::Metadata;
    use std::os::unix::fs::MetadataExt;

    pub fn device(m: &Metadata) -> u64 {
        m.dev()
    }
    pub fn inode(m: &Metadata) -> u64 {
        m.ino()
    }
    /// The bytes the file takes on its volume.
    pub fn allocated(m: &Metadata) -> u64 {
        m.blocks() * 512
    }
    pub fn links(m: &Metadata) -> u64 {
        m.nlink()
    }
    /// When its content last changed, in seconds since 1970.
    pub fn modified(m: &Metadata) -> i64 {
        m.mtime()
    }
}

#[cfg(not(unix))]
mod meta {
    use std::fs::Metadata;

    pub fn device(_: &Metadata) -> u64 {
        0
    }
    pub fn inode(_: &Metadata) -> u64 {
        0
    }
    pub fn allocated(m: &Metadata) -> u64 {
        m.len()
    }
    pub fn links(_: &Metadata) -> u64 {
        1
    }
    pub fn modified(m: &Metadata) -> i64 {
        m.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    }
}

/// Mount points are shown as leaves without contents; a walk never crosses
/// onto another device or into a virtual filesystem.
fn descend(path: &Path, metadata: &std::fs::Metadata, dev: u64) -> bool {
    meta::device(metadata) == dev && !virtual_filesystem(path)
}

/// Why one registration is held: scans cover it, and/or it is the parent of
/// a symlink component of a request path. The kernel watch is released only
/// when neither use remains.
#[derive(Clone, Copy, Default)]
struct Use {
    scan: bool,
    alias: u32,
}
/// Ordered by path so a folder's registrations are one range (see [`below`]).
type Registrations = BTreeMap<PathBuf, Use>;

/// The keys of `map` that are `path` or below it. Paths order component by
/// component, so these follow `path` contiguously.
fn below<'a, V>(
    map: &'a BTreeMap<PathBuf, V>,
    path: &'a Path,
) -> impl Iterator<Item = &'a PathBuf> + 'a {
    use std::ops::Bound;
    map.range::<Path, _>((Bound::Included(path), Bound::Unbounded))
        .map(|(p, _)| p)
        .take_while(move |p| p.starts_with(path))
}

enum Registration {
    Watched,
    /// This directory cannot be watched (missing, unreadable, unsupported).
    Unwatchable,
    /// The per-user watch budget is spent; nothing can be registered for now.
    Exhausted,
}
fn register(watcher: &mut dyn Watcher, path: &Path) -> Registration {
    match watcher.watch(path, RecursiveMode::NonRecursive) {
        Ok(()) => Registration::Watched,
        Err(error) => match &error.kind {
            ErrorKind::MaxFilesWatch => Registration::Exhausted,
            ErrorKind::Io(io)
                if matches!(
                    io.kind(),
                    std::io::ErrorKind::StorageFull | std::io::ErrorKind::OutOfMemory
                ) =>
            {
                // ENOSPC: inotify budget; ENOMEM: kernel watch allocation.
                Registration::Exhausted
            }
            _ => Registration::Unwatchable,
        },
    }
}

/// Install one watch immediately before reading each directory. The native
/// backend never receives a recursive request (which hides an uncancellable walk).
struct Monitor<'a> {
    watcher: &'a mut dyn Watcher,
    registrations: &'a mut Registrations,
    exhausted: bool,
}
impl Monitor<'_> {
    /// Whether `path` is watched after this call.
    fn watch(&mut self, path: &Path) -> bool {
        if self.registrations.get(path).is_some_and(|r| r.scan) {
            return true;
        }
        if self.exhausted {
            return false;
        }
        match register(self.watcher, path) {
            Registration::Watched => {
                self.registrations
                    .entry(path.to_path_buf())
                    .or_default()
                    .scan = true;
                true
            }
            Registration::Unwatchable => false,
            Registration::Exhausted => {
                self.exhausted = true;
                false
            }
        }
    }
}

/// Newer visible requests preempt a running scan at its checkpoints.
struct Inbox<'a> {
    rx: &'a Receiver<Message>,
    deferred: &'a mut Vec<Message>,
}

struct Scanner<'a> {
    cache: &'a mut Cache,
    /// A names index summing the request's folder (ADR-150, D3), opened
    /// once a breakdown is not kept: its breakdowns are taken from it,
    /// what it does not count scanned.
    indexed: Option<indexed::Indexed>,
    monitor: Option<Monitor<'a>>,
    inbox: Option<Inbox<'a>>,
    request: &'a Request,
    /// The canonical directory being counted (`request.path` may be an alias).
    path: &'a Path,
    shared: &'a Shared,
    /// Without a monitor the caller decides whether results may be reused.
    caching: bool,
    /// The kinds of the extensions met so far, kept by the worker.
    types: Types,
    places: Places,
    dev: u64,
    dirs: u64,
    entries: u64,
    errors: u64,
    /// Readable directories without a watch; their breakdowns are never cached.
    unmonitored: u64,
    yielded: bool,
    last_progress: Instant,
    preview: Option<Progress>,
    /// When the root itself last changed.
    root_modified: i64,
}

/// Only the visible level and the branch being counted are retained. Partial
/// nodes never enter Cache; descendants still cache only completed breakdowns.
struct Progress {
    tree: Node,
    branch: Option<Node>,
}
impl Progress {
    fn snapshot(&self) -> Node {
        let mut tree = self.tree.clone();
        tree.incomplete = true;
        if let Some(branch) = &self.branch {
            // Read only: merging would copy the root's links every snapshot.
            add_totals(&mut tree, branch);
            let (size, alloc) = self.tree.links.overlap(&branch.links);
            tree.size -= size;
            tree.alloc -= alloc;
            replace_pending(&mut tree, branch.clone());
        }
        // Shown, never cached: the links stay with the tree being counted.
        tree.shared = self.tree.links.shared();
        tree.links = Arc::default();
        trim(&mut tree);
        tree
    }
}

impl Links {
    /// One more link to `id` found under the folder; whether one was found
    /// before, whose bytes the folder already counts.
    fn found(&mut self, id: FileId, linked: Linked) -> bool {
        use std::collections::hash_map::Entry;
        match self.0.entry(id) {
            Entry::Occupied(mut known) => {
                known.get_mut().found += linked.found;
                if known.get().found >= known.get().nlink {
                    known.remove();
                }
                true
            }
            Entry::Vacant(new) => {
                if linked.found < linked.nlink {
                    new.insert(linked);
                }
                false
            }
        }
    }

    /// What `other` holds that this counts too.
    fn overlap(&self, other: &Links) -> (u64, u64) {
        let (small, large) = if self.0.len() <= other.0.len() {
            (self, other)
        } else {
            (other, self)
        };
        small
            .0
            .iter()
            .filter(|(id, _)| large.0.contains_key(id))
            .fold((0, 0), |(size, alloc), (_, l)| {
                (size + l.size, alloc + l.alloc)
            })
    }
}

/// Add what `child` holds to the folder `total`, counting a file with
/// several links once, as `du` does: a folder weighs what it holds, and a
/// folder holding two links to one file holds it once. Each folder's
/// total is its own (two folders sharing a file each hold it), so a total
/// is the same whether its folder was counted now or kept from before.
fn add_contents(total: &mut Node, child: &Node) {
    add_totals(total, child);
    let forget = |total: &mut Node, id: FileId, linked: Linked| {
        if Arc::make_mut(&mut total.links).found(id, linked) {
            total.size -= linked.size;
            total.alloc -= linked.alloc;
            total.kinds.take(linked.kind, linked.alloc);
        }
    };
    if child.is_dir {
        for (id, linked) in &child.links.0 {
            forget(total, *id, *linked);
        }
    } else if child.nlink > 1
        && let Some(id) = child.id
    {
        let linked = Linked {
            found: 1,
            nlink: child.nlink,
            size: child.size,
            alloc: child.alloc,
            kind: child.kind,
        };
        forget(total, id.file, linked);
    }
}

fn add_totals(total: &mut Node, child: &Node) {
    total.size += child.size;
    total.alloc += child.alloc;
    total.files += child.files;
    total.dirs += child.dirs;
    // A folder is as new as its newest file: one emptied, or that holds
    // only folders, has its own time (`own_time`), which says nothing of
    // what its parent holds.
    if !child.is_dir || child.files > 0 {
        total.modified = total.modified.max(child.modified);
    }
    total.reclaimable += child.reclaimable;
    if child.is_dir {
        total.kinds.merge(&child.kinds);
    } else {
        total.kinds.add(child.kind, child.alloc);
    }
}

/// A folder without files is as new as itself: its entries came or went.
fn own_time(node: &mut Node, own: i64) {
    if node.files == 0 {
        node.modified = own;
    }
}

/// A folder that is a cache or a trash holds nothing else.
fn reclaim_whole(node: &mut Node) {
    if node.reclaim.is_some() {
        node.reclaimable = node.alloc;
    }
}

fn replace_pending(tree: &mut Node, mut child: Node) {
    // Its links were added to the tree's; a row needs none.
    child.links = Arc::default();
    if let Some(index) = tree.children.iter().position(|n| n.path == child.path) {
        tree.children.swap_remove(index);
    } else {
        // Pending children have no known bytes. They may be in the bounded
        // aggregate rather than individually displayed.
        tree.other_count = tree.other_count.saturating_sub(1);
        tree.other_incomplete = tree.other_incomplete.saturating_sub(1);
    }
    child.children.clear();
    tree.children.push(child);
    trim(tree);
}

/// The file `metadata` is of: what a file of several links is counted
/// once by.
fn file_of(metadata: &std::fs::Metadata) -> FileId {
    skwir_core::fs::stat::file(meta::device(metadata), meta::inode(metadata))
}

/// The entry `name` in the folder `folder` is, as the folder's listing
/// names it (ADR-049): what the pane's selection is matched with.
fn entry_id(metadata: &std::fs::Metadata, folder: FileId, name: &std::ffi::OsStr) -> EntryId {
    EntryId::in_folder(file_of(metadata), folder, name)
}

/// The entry of the folder at `path` in its parent, `metadata` its own.
fn folder_entry(path: &Path, metadata: &std::fs::Metadata) -> EntryId {
    let parent = path
        .parent()
        .and_then(|p| std::fs::metadata(p).ok())
        .map_or(FileId::new(0, 0), |m| file_of(&m));
    entry_id(metadata, parent, path.file_name().unwrap_or_default())
}

/// One stat per entry. Returns whether the entry is a directory to descend into.
fn entry_node(
    entry: &std::fs::DirEntry,
    folder: FileId,
    dev: u64,
    types: &mut Types,
) -> std::io::Result<(Node, bool)> {
    let path = entry.path();
    let metadata = std::fs::symlink_metadata(&path)?;
    let is_dir = metadata.is_dir();
    let descend = is_dir && descend(&path, &metadata, dev);
    let name = entry.file_name();
    Ok((
        Node {
            id: Some(entry_id(&metadata, folder, &name)),
            path,
            kind: if is_dir { None } else { types.of(&name) },
            name: name.to_string_lossy().into_owned(),
            is_dir,
            incomplete: descend,
            size: if is_dir { 0 } else { metadata.len() },
            alloc: if is_dir {
                0
            } else {
                meta::allocated(&metadata)
            },
            files: u64::from(!is_dir),
            nlink: if is_dir { 0 } else { meta::links(&metadata) },
            modified: meta::modified(&metadata),
            entry_kind: (!is_dir).then(|| EntryKind::from(metadata.file_type())),
            ..Node::default()
        },
        descend,
    ))
}

impl<'a> Scanner<'a> {
    fn new(cache: &'a mut Cache, request: &'a Request, path: &'a Path, shared: &'a Shared) -> Self {
        Scanner {
            cache,
            indexed: None,
            monitor: None,
            inbox: None,
            request,
            path,
            shared,
            caching: true,
            types: Types::default(),
            places: Places::default(),
            dev: 0,
            dirs: 0,
            entries: 0,
            errors: 0,
            unmonitored: 0,
            yielded: false,
            last_progress: Instant::now(),
            preview: None,
            root_modified: 0,
        }
    }

    fn interrupted(&self) -> bool {
        self.yielded || self.request.cancel.is_cancelled()
    }
    fn ensure(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.interrupted(), "Scan interrupted");
        Ok(())
    }

    /// Whether `path` is watched. Callers count an unwatched directory only
    /// once it was read: an unreadable one is a read error, not a coverage gap.
    fn watch(&mut self, path: &Path) -> anyhow::Result<bool> {
        self.ensure()?;
        let covered = self.monitor.as_mut().is_none_or(|m| m.watch(path));
        self.ensure()?;
        Ok(covered)
    }

    /// Between entries: cancellation, preemption by newer requests, delayed progress.
    fn checkpoint(&mut self, now: Instant) -> anyhow::Result<()> {
        self.ensure()?;
        if let Some(inbox) = &mut self.inbox {
            while let Ok(message) = inbox.rx.try_recv() {
                let visible = matches!(message, Message::Scan(_));
                inbox.deferred.push(message);
                if visible {
                    self.yielded = true;
                    anyhow::bail!("Scan yielded to a newer request");
                }
            }
        }
        self.progress(now);
        Ok(())
    }

    fn progress(&mut self, now: Instant) {
        if now.duration_since(self.last_progress) < PROGRESS_INTERVAL {
            return;
        }
        self.last_progress = now;
        self.publish(true);
    }

    fn publish(&self, updating: bool) {
        let mut statuses = self.shared.statuses.lock();
        let Some(s) = statuses.get_mut(&self.request.pane).filter(|s| {
            s.generation == self.request.generation && !self.request.cancel.is_cancelled()
        }) else {
            return;
        };
        // Progressive results are for the first scan of this location only.
        // Background revalidation must not turn a finished chart into unknown
        // blocks again. Explicit refresh can report progress over the old chart.
        if s.snapshot == SnapshotKind::Finished && !self.request.force {
            return;
        }
        s.updating = updating;
        s.scanned_dirs = self.dirs;
        if s.snapshot == SnapshotKind::Progressive
            && let Some(preview) = &self.preview
        {
            let mut tree = preview.snapshot();
            // The folders counted already show what they hold.
            nest(&mut tree, self.cache);
            s.size = tree.size;
            s.alloc = tree.alloc;
            s.files = tree.files;
            s.dirs = tree.dirs;
            s.tree = Some(tree);
            s.error = None;
        }
        drop(statuses);
        self.shared.changed();
    }

    fn root(&mut self) -> anyhow::Result<Node> {
        self.ensure()?;
        if let Some(node) = self.cache.get(self.path) {
            return Ok(node);
        }
        if let Some(node) = self.indexed_root()? {
            return Ok(node);
        }
        let subdirectories = self.list_root()?;
        self.scan_children(subdirectories)?;
        self.ensure()?;
        let mut node = self.preview.take().expect("root preview").tree;
        trim(&mut node);
        reclaim_whole(&mut node);
        own_time(&mut node, self.root_modified);
        node.shared = node.links.shared();
        node.incomplete = self.errors > 0
            || node.other_incomplete > 0
            || node.children.iter().any(|child| child.incomplete);
        if !node.incomplete && self.unmonitored == 0 && self.caching {
            self.cache.insert(node.clone());
        }
        Ok(node)
    }

    /// The root's breakdown from a names index's totals, where one sums it
    /// with its sizes live (ADR-150, D3); the folders `nest` would open kept
    /// too. `None`: the scan reads the disk. Its folders are kept current
    /// by the index's journal, kept with them (`keep_indexed`), not by
    /// watches of Usage's own; only the root and what the index does not
    /// count are watched.
    fn indexed_root(&mut self) -> anyhow::Result<Option<Node>> {
        // Not where nothing is kept: no watch, no journal either.
        if !self.caching || !cfg!(unix) || virtual_filesystem(self.path) {
            return Ok(None);
        }
        let Some(app) = self.shared.app.upgrade() else {
            return Ok(None);
        };
        self.indexed = indexed::Indexed::open(&app, self.path, &self.request.cancel);
        if self.indexed.is_none() {
            return Ok(None);
        }
        let Ok(metadata) = std::fs::metadata(self.path) else {
            return Ok(None);
        };
        if !metadata.is_dir() {
            return Ok(None);
        }
        self.dev = meta::device(&metadata);
        self.root_modified = meta::modified(&metadata);
        if !self.watch(self.path)? {
            self.unmonitored += 1;
        }
        let Some((mut node, _)) = self.indexed_breakdown(self.path, 0)? else {
            return Ok(None);
        };
        let shown = &self.request.path;
        node.name = shown
            .file_name()
            .unwrap_or(shown.as_os_str())
            .to_string_lossy()
            .into_owned();
        self.indexed_nested(&node)?;
        // Kept where every folder scanned of it, those of its nested
        // breakdowns too, and the root itself are watched.
        if !node.incomplete && self.unmonitored == 0 && self.caching {
            self.keep_indexed(node.clone());
        }
        Ok(Some(node))
    }

    /// Keep a breakdown taken from the index with the journal that keeps
    /// it current: never one without the other.
    fn keep_indexed(&mut self, node: Node) {
        if let Some(indexed) = &self.indexed
            && let Some((mark, holes)) = indexed.journal()
        {
            self.cache
                .insert_journaled(node, indexed.root(), mark, holes);
        }
    }

    /// `folder`'s breakdown from the index, what it does not count below
    /// scanned and added (`indexed::add_hole`), and whether it may be kept
    /// (complete, what was scanned of it watched, as a scan's own);
    /// `None` where it does not sum it.
    fn indexed_breakdown(
        &mut self,
        folder: &Path,
        depth: usize,
    ) -> anyhow::Result<Option<(Node, bool)>> {
        let Some(indexed) = self.indexed.as_mut() else {
            return Ok(None);
        };
        let Some(indexed::Breakdown { mut node, holes }) =
            indexed.breakdown(folder, &mut self.types, &self.places)
        else {
            return Ok(None);
        };
        let (errors, unmonitored) = (self.errors, self.unmonitored);
        for hole in holes {
            self.ensure()?;
            let Ok(metadata) = std::fs::symlink_metadata(&hole) else {
                continue;
            };
            if !metadata.is_dir() || !descend(&hole, &metadata, self.dev) {
                continue;
            }
            let below = hole
                .strip_prefix(folder)
                .map_or(1, |r| r.components().count());
            let id = folder_entry(&hole, &metadata);
            match self.directory(&hole, depth + below, id, meta::modified(&metadata)) {
                Ok(scanned) => {
                    let (indexed, places) = (self.indexed.as_ref().expect("summing"), &self.places);
                    let at = scanned.path.clone();
                    indexed::add_hole(&mut node, scanned, &|level| {
                        indexed.reclaimed(level, &at, places)
                    });
                }
                Err(_) if self.interrupted() => anyhow::bail!("Scan interrupted"),
                Err(_) => self.errors += 1,
            }
        }
        trim(&mut node);
        reclaim_whole(&mut node);
        node.incomplete = self.errors != errors;
        let keep = !node.incomplete && self.unmonitored == unmonitored;
        Ok(Some((node, keep)))
    }

    /// The breakdowns `nest` opens below `tree`, from the index, kept: the
    /// folders weighing a share of the tree, down `NESTED_LEVELS`, while
    /// `MAX_NESTED` allows.
    fn indexed_nested(&mut self, tree: &Node) -> anyhow::Result<()> {
        let (total, files) = (tree.alloc, tree.files);
        let weighs = |n: &Node| {
            n.is_dir
                && (n.alloc.saturating_mul(OPEN_SHARE) >= total.max(1)
                    || n.files.saturating_mul(OPEN_SHARE) >= files.max(1))
        };
        let mut level: Vec<PathBuf> = tree
            .children
            .iter()
            .filter(|n| weighs(n))
            .map(|n| n.path.clone())
            .collect();
        let mut budget = MAX_NESTED;
        for depth in 1..=NESTED_LEVELS {
            let mut next = Vec::new();
            for folder in level {
                self.ensure()?;
                if budget == 0 {
                    return Ok(());
                }
                let node = if let Some(kept) = self.cache.peek(&folder) {
                    kept.clone()
                } else {
                    let Some((node, keep)) = self.indexed_breakdown(&folder, depth)? else {
                        continue;
                    };
                    if keep && self.caching {
                        self.keep_indexed(node.clone());
                    }
                    node
                };
                budget = budget.saturating_sub(node.children.len());
                next.extend(
                    node.children
                        .iter()
                        .filter(|n| weighs(n))
                        .map(|n| n.path.clone()),
                );
            }
            level = next;
        }
        Ok(())
    }

    /// One pass over the root: files and unknown folders are visible before
    /// any subtree is descended. Only the names of subdirectories on the
    /// root's device are retained for the descent.
    fn list_root(&mut self) -> anyhow::Result<Vec<(OsString, EntryId, i64)>> {
        if !cfg!(unix) {
            return Err(skwir_core::system::not_yet_words(words!(
                usage_not_yet = "Disk usage is not available on { $system } yet",
                system = skwir_core::system::named(),
                note =
                    "Why a folder's disk usage is not shown. system: the system's name (Windows)."
            ))
            .into());
        }
        let path = self.path;
        anyhow::ensure!(
            !virtual_filesystem(path),
            words!(
                usage_virtual = "Disk usage is not available for virtual filesystems",
                note = "Said in the Usage view of /proc or /sys, whose files are made up by the system."
            )
        );
        let covered = self.watch(path)?;
        let metadata = std::fs::metadata(path)?;
        self.ensure()?;
        anyhow::ensure!(
            metadata.is_dir(),
            words!(usage_needs_directory = "Disk usage needs a directory")
        );
        self.dev = meta::device(&metadata);
        let shown = &self.request.path;
        self.preview = Some(Progress {
            tree: Node {
                id: Some(folder_entry(path, &metadata)),
                path: path.to_path_buf(),
                name: shown
                    .file_name()
                    .unwrap_or(shown.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                is_dir: true,
                reclaim: self.places.of(path),
                ..Node::default()
            },
            branch: None,
        });
        self.root_modified = meta::modified(&metadata);
        let root = file_of(&metadata);
        let mut subdirectories = Vec::new();
        let entries = entries(path, &self.request.cancel)?;
        if !covered {
            self.unmonitored += 1;
        }
        self.dirs += 1;
        for entry in entries {
            self.ensure()?;
            let Ok(entry) = entry else {
                self.errors += 1;
                continue;
            };
            match entry_node(&entry, root, self.dev, &mut self.types) {
                Ok((child, descend)) => {
                    if descend {
                        subdirectories.push((
                            entry.file_name(),
                            child.id.expect("stat-ed"),
                            child.modified,
                        ));
                    }
                    let tags = !child.is_dir
                        && child.name == marks::CACHEDIR_TAG
                        && marks::tags_a_cache(&child.path);
                    let tree = &mut self.preview.as_mut().expect("root preview").tree;
                    if tags {
                        tree.reclaim.get_or_insert(marks::Reclaim::Cache);
                    }
                    add_contents(tree, &child);
                    tree.dirs += u64::from(child.is_dir);
                    tree.children.push(child);
                    if tree.children.len() > MAX_CHILDREN * 2 {
                        trim(tree);
                    }
                }
                Err(_) => self.errors += 1,
            }
            self.entries += 1;
            if self.entries.is_multiple_of(256) {
                self.checkpoint(Instant::now())?;
            }
        }
        // The visible level is complete: show it before the progress delay.
        self.publish(false);
        Ok(subdirectories)
    }

    fn scan_children(
        &mut self,
        subdirectories: Vec<(OsString, EntryId, i64)>,
    ) -> anyhow::Result<()> {
        for (name, id, modified) in subdirectories {
            self.ensure()?;
            let pending = Node {
                path: self.path.join(&name),
                name: name.to_string_lossy().into_owned(),
                is_dir: true,
                incomplete: true,
                modified,
                ..Node::default()
            };
            self.preview.as_mut().expect("root preview").branch = Some(pending.clone());
            let child = match self.directory(&pending.path, 1, id, modified) {
                Ok(child) => child,
                Err(_) if self.interrupted() => anyhow::bail!("Scan interrupted"),
                Err(_) => {
                    self.errors += 1;
                    pending
                }
            };
            let preview = self.preview.as_mut().expect("root preview");
            preview.branch = None;
            add_contents(&mut preview.tree, &child);
            replace_pending(&mut preview.tree, child);
            self.progress(Instant::now());
        }
        Ok(())
    }

    /// `id` and `modified` come from the parent's stat: each entry is
    /// stat-ed once.
    fn directory(
        &mut self,
        path: &Path,
        depth: usize,
        id: EntryId,
        modified: i64,
    ) -> anyhow::Result<Node> {
        let covered = self.watch(path)?;
        // Only what a scan counted: one from an index would count a file
        // of several names twice where its names meet above it.
        if let Some(node) = self.cache.counted(path) {
            if let Some(branch) = self.preview.as_mut().and_then(|p| p.branch.as_mut()) {
                if depth == 1 {
                    branch.id = node.id;
                }
                add_contents(branch, &node);
                branch.dirs += u64::from(depth > 1);
            }
            return Ok(node);
        }
        if let Some(branch) = self.preview.as_mut().and_then(|p| p.branch.as_mut()) {
            branch.dirs += u64::from(depth > 1);
            if depth == 1 {
                branch.id = Some(id);
            }
        }
        anyhow::ensure!(
            depth < 256,
            words!(usage_too_deep = "Directory nesting exceeds the scan limit")
        );
        let mut node = Node {
            id: Some(id),
            name: path.file_name().map_or_else(
                || path.to_string_lossy().into_owned(),
                |n| n.to_string_lossy().into_owned(),
            ),
            path: path.to_path_buf(),
            is_dir: true,
            reclaim: self.places.of(path),
            ..Node::default()
        };
        self.checkpoint(Instant::now())?;
        let entries = entries(path, &self.request.cancel)?;
        if !covered {
            self.unmonitored += 1;
        }
        let initial_errors = self.errors;
        let initial_unmonitored = self.unmonitored;
        self.dirs += 1;
        for entry in entries {
            self.ensure()?;
            let Ok(entry) = entry else {
                self.errors += 1;
                continue;
            };
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                self.errors += 1;
                continue;
            };
            let file_name = entry.file_name();
            let child_id = entry_id(&metadata, id.file, &file_name);
            let modified = meta::modified(&metadata);
            let name = file_name.to_string_lossy().into_owned();
            let mut child = if !metadata.is_dir() {
                if file_name == marks::CACHEDIR_TAG && marks::tags_a_cache(&path) {
                    node.reclaim.get_or_insert(marks::Reclaim::Cache);
                }
                Node {
                    path,
                    name,
                    size: metadata.len(),
                    alloc: meta::allocated(&metadata),
                    files: 1,
                    nlink: meta::links(&metadata),
                    modified,
                    kind: self.types.of(&file_name),
                    entry_kind: Some(EntryKind::from(metadata.file_type())),
                    ..Node::default()
                }
            } else if !descend(&path, &metadata, self.dev) {
                // Another filesystem: a leaf whose contents are not counted.
                Node {
                    path,
                    name,
                    is_dir: true,
                    modified,
                    ..Node::default()
                }
            } else {
                // Interior symlinks stay leaves, so a link back to an ancestor
                // never creates a cycle.
                match self.directory(&path, depth + 1, child_id, modified) {
                    Ok(node) => node,
                    Err(_) if self.interrupted() => anyhow::bail!("Scan interrupted"),
                    Err(_) => {
                        self.errors += 1;
                        Node {
                            path,
                            name,
                            is_dir: true,
                            incomplete: true,
                            modified,
                            ..Node::default()
                        }
                    }
                }
            };
            child.id = Some(child_id);
            if !child.is_dir
                && let Some(branch) = self.preview.as_mut().and_then(|p| p.branch.as_mut())
            {
                add_contents(branch, &child);
            }
            add_contents(&mut node, &child);
            node.dirs += u64::from(child.is_dir);
            // Its links were added to the folder's; a row needs none.
            child.children.clear();
            child.links = Arc::default();
            node.children.push(child);
            // Keep transient memory bounded even in directories with millions of entries.
            if node.children.len() > MAX_CHILDREN * 2 {
                trim(&mut node);
            }
            self.entries += 1;
            if self.entries.is_multiple_of(256) {
                self.checkpoint(Instant::now())?;
            }
        }
        self.ensure()?;
        trim(&mut node);
        reclaim_whole(&mut node);
        own_time(&mut node, modified);
        node.shared = node.links.shared();
        node.incomplete = self.errors != initial_errors;
        if !node.incomplete && covered && self.unmonitored == initial_unmonitored && self.caching {
            self.cache.insert(node.clone());
        }
        Ok(node)
    }
}
/// Keep the `MAX_CHILDREN` heaviest entries, and among the rest up to
/// `MAX_CROWDED` folders holding the most files, each at least a
/// `MAX_CHILDREN`th of the folder's (a map sized by files draws them
/// large); the others join the folder's other items.
fn trim(node: &mut Node) {
    let order = |a: &Node, b: &Node| b.alloc.cmp(&a.alloc).then_with(|| a.name.cmp(&b.name));
    node.children.sort_by(order);
    if node.children.len() > MAX_CHILDREN + MAX_CROWDED {
        let mut rest = node.children.split_off(MAX_CHILDREN);
        rest.sort_by_key(|c| Reverse(if c.is_dir { c.files } else { 0 }));
        let least = node.files.max(1);
        let crowded = rest
            .iter()
            .take(MAX_CROWDED)
            .take_while(|c| c.is_dir && c.files.saturating_mul(MAX_CHILDREN as u64) >= least)
            .count();
        for child in rest.split_off(crowded) {
            fold(node, &child);
        }
        node.children.append(&mut rest);
        node.children.sort_by(order);
    }
}

/// `child` joins `node`'s other items.
fn fold(node: &mut Node, child: &Node) {
    node.other_alloc += child.alloc;
    node.other_size += child.size;
    node.other_count += 1;
    node.other_incomplete += u64::from(child.incomplete);
    node.other_files += child.files;
    node.other_modified = node.other_modified.max(child.modified);
}

/// What the folders of a status's `tree` hold, read from the breakdowns
/// kept, so the map draws folders inside folders: down to
/// `NESTED_LEVELS` below the tree's own entries, the heaviest folders
/// first while `MAX_NESTED` allows. A folder is opened only when it weighs
/// `1/OPEN_SHARE` of the tree, and only its entries weighing
/// `1/ENTRY_SHARE` of the tree are sent; the rest join its other items.
/// A folder weighs by its bytes, or by its files as a map sized by files
/// draws it.
/// A folder without a breakdown kept (being counted, unreadable, evicted,
/// or scanned without monitoring, which keeps none) is sent closed.
fn nest(tree: &mut Node, cache: &Cache) {
    let (total, files) = (tree.alloc, tree.files);
    if total == 0 && files == 0 {
        return;
    }
    // A folder of many small files weighs by them, as a map sized by
    // files draws it.
    let weighs = |n: &Node, share: u64| {
        n.alloc.saturating_mul(share) >= total.max(1)
            || (n.is_dir && n.files.saturating_mul(share) >= files.max(1))
    };
    let mut budget = MAX_NESTED;
    for level in 1..=NESTED_LEVELS {
        let mut folders = Vec::new();
        at_depth(tree, level, &mut folders);
        folders.retain(|n| n.is_dir && n.children.is_empty() && weighs(n, OPEN_SHARE));
        folders.sort_by_key(|n| Reverse(n.alloc));
        for folder in folders {
            let Some(breakdown) = cache.peek(&folder.path) else {
                continue;
            };
            folder.other_alloc = breakdown.other_alloc;
            folder.other_size = breakdown.other_size;
            folder.other_count = breakdown.other_count;
            folder.other_incomplete = breakdown.other_incomplete;
            folder.other_files = breakdown.other_files;
            folder.other_modified = breakdown.other_modified;
            for entry in &breakdown.children {
                if budget > 0 && weighs(entry, ENTRY_SHARE) {
                    budget -= 1;
                    folder.children.push(Node {
                        children: Vec::new(),
                        links: Arc::default(),
                        ..entry.clone()
                    });
                } else {
                    fold(folder, entry);
                }
            }
            if budget == 0 {
                return;
            }
        }
    }
}

/// The nodes `depth` levels below `node`.
fn at_depth<'a>(node: &'a mut Node, depth: usize, out: &mut Vec<&'a mut Node>) {
    if depth == 0 {
        out.push(node);
        return;
    }
    for child in &mut node.children {
        at_depth(child, depth - 1, out);
    }
}

/// How much of a request the watches observe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Coverage {
    /// Nothing is watched: results are never reused and the tree is re-walked
    /// on every revalidation.
    None,
    /// The root and most directories are watched; unwatched subtrees and
    /// aliases are revalidated periodically while cached siblings are reused.
    Partial,
    /// Every directory is watched: updates are event-driven only.
    Full,
}

/// Where a request path leads and which of its components are symlinks.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    /// Canonical directory to scan and cache.
    path: PathBuf,
    /// Symlink prefixes of the request path, longest first.
    links: Vec<PathBuf>,
}
impl Target {
    fn resolve(path: &Path) -> Self {
        let links = path
            .ancestors()
            .filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
            .map(Path::to_path_buf)
            .collect();
        let path = skwir_core::os_path::canonical(path).unwrap_or_else(|_| normalize(path));
        Target { path, links }
    }
    /// Directories whose entries decide where the aliases point.
    fn alias_parents(&self) -> BTreeSet<PathBuf> {
        self.links
            .iter()
            .filter_map(|link| link.parent().map(Path::to_path_buf))
            .collect()
    }
}

/// Lexical `.`/`..` removal for paths that cannot be canonicalized (missing).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

struct Active {
    request: Request,
    target: Target,
    alias_parents: BTreeSet<PathBuf>,
    alias_watched: bool,
    coverage: Coverage,
    last_scan: Instant,
    last_duration: Duration,
    /// The last scan yielded to a newer request and resumes at the next update.
    pending: bool,
    /// A change came that no scan counted yet: counted once
    /// `counts_changes_at` allows.
    changed: bool,
    /// When the last recount a change asked for ended, and how long it
    /// took. The first scan of a request is not one: it reads the whole
    /// tree, where a change reads its folder and those above it again.
    recounted: Option<(Instant, Duration)>,
}

impl Active {
    /// When a change may be counted: at once, unless a recount a change
    /// asked for ended less than `REVALIDATE_DUTY` times its duration ago.
    fn counts_changes_at(&self) -> Option<Instant> {
        self.recounted
            .map(|(ended, took)| ended + took * REVALIDATE_DUTY)
    }
}

#[derive(Default)]
struct Backoff {
    until: Option<Instant>,
    next: Option<Duration>,
}

struct Worker {
    shared: Arc<Shared>,
    cache: Cache,
    active: HashMap<PaneId, Active>,
    watcher: Option<Box<dyn Watcher>>,
    /// Watched request roots, least recently used first.
    roots: Vec<PathBuf>,
    registrations: Registrations,
    inbox: Option<Receiver<Message>>,
    /// Messages drained while a scan ran; applied before reading the channel again.
    deferred: Vec<Message>,
    events: Arc<Mutex<Changes>>,
    backoff: Backoff,
    /// The kinds of the extensions the scans met.
    types: Types,
    /// Read again as each request comes: a volume mounted since has a trash.
    places: Places,
    /// The plugin's lifetime, waited for with the messages.
    stop: Option<skwir_core::cancel::CancelSignal>,
    /// Since when no view has shown a breakdown.
    idle_since: Option<Instant>,
    /// When the journals the cache follows were last asked (`journals`).
    journals_asked: Instant,
    /// The panes a journal asked as a request came told of a change in:
    /// counted again with the others.
    touched: HashSet<PaneId>,
    /// The worker's own: what a journal is asked under.
    lifetime: CancelToken,
}

/// What the worker's wait ended with.
enum Received {
    Message(Message),
    Timeout,
    Closed,
}

impl Worker {
    fn new(
        shared: Arc<Shared>,
        watcher: Option<Box<dyn Watcher>>,
        inbox: Option<Receiver<Message>>,
        events: Arc<Mutex<Changes>>,
    ) -> Self {
        Worker {
            shared,
            cache: Cache::default(),
            active: HashMap::new(),
            watcher,
            roots: Vec::new(),
            registrations: Registrations::new(),
            inbox,
            deferred: Vec::new(),
            events,
            backoff: Backoff::default(),
            types: Types::default(),
            places: Places::default(),
            stop: None,
            idle_since: None,
            journals_asked: Instant::now(),
            touched: HashSet::new(),
            lifetime: CancelToken::new(),
        }
    }

    /// One loop iteration: apply deferred and new messages, then update
    /// pending, changed and due requests. Returns false once the channel closed.
    fn step(&mut self, rx: &Receiver<Message>) -> bool {
        let mut messages = std::mem::take(&mut self.deferred);
        if messages.is_empty() {
            let now = Instant::now();
            self.let_go_when_idle(now);
            match self.receive(rx, self.next_wake(now)) {
                Received::Message(message) => messages.push(message),
                Received::Timeout => {}
                Received::Closed => return false,
            }
        }
        // Coalesce a burst without delaying a new visible request.
        if matches!(messages.first(), Some(Message::Changed)) {
            while let Ok(message) = rx.recv_timeout(Duration::from_millis(40)) {
                let command = !matches!(message, Message::Changed);
                messages.push(message);
                if command || messages.len() >= 1024 {
                    break;
                }
            }
        }
        messages.extend(rx.try_iter().take(1024));
        let mut dirty = HashSet::new();
        for message in messages {
            match message {
                Message::Scan(request) => self.admit(request),
                Message::Release(pane) => self.remove_active(pane),
                Message::Changed => {
                    let batch = std::mem::take(&mut *self.events.lock());
                    dirty.extend(self.changes(batch));
                }
                // Woken to ask the journals now (`journals`).
                Message::LookedAt => {}
            }
        }
        dirty.extend(std::mem::take(&mut self.touched));
        dirty.extend(self.journals(Instant::now()));
        self.update(&dirty, Instant::now());
        true
    }

    /// Ask the journals that keep the breakdowns taken from a names index
    /// current (`Cache::journals`, ADR-150, D3) what changed, every
    /// `REVALIDATE` while a pane shows what one keeps and someone looks
    /// (`App::looked_at`): a journal is polled, which no watch wakes. What
    /// changed meanwhile is told as a pane is shown again (`admit`).
    /// Returns the panes to count again.
    fn journals(&mut self, now: Instant) -> HashSet<PaneId> {
        if self.cache.journals.is_empty()
            || self.active.is_empty()
            || now.saturating_duration_since(self.journals_asked) < REVALIDATE
        {
            return HashSet::new();
        }
        let Some(app) = self.shared.app.upgrade() else {
            return HashSet::new();
        };
        if !app.looked_at() {
            return HashSet::new();
        }
        self.journals_asked = now;
        let shown: Vec<PathBuf> = self
            .active
            .values()
            .map(|a| a.target.path.clone())
            .collect();
        self.ask_journals(&app, &|at| {
            shown
                .iter()
                .any(|target| target.starts_with(at) || at.starts_with(target))
        })
    }

    /// Ask each journal followed at a folder `wanted` accepts what changed
    /// since its mark: the breakdowns holding what changed are dropped, a
    /// folder it covers no more whole; one whose journal is not every
    /// change since, or cannot be followed, is let go of with everything
    /// it kept. One that keeps nothing any more is let go of, and one
    /// followed inside another asked after it is that one's to tell from
    /// then on. Returns the panes showing what changed.
    fn ask_journals(&mut self, app: &App, wanted: &dyn Fn(&Path) -> bool) -> HashSet<PaneId> {
        use skwir_core::names::{Change, Since};
        // Deepest first, so one inside another is asked before it.
        let followed: Vec<PathBuf> = self.cache.journals.keys().rev().cloned().collect();
        let mut changed: Vec<(PathBuf, bool)> = Vec::new();
        let mut asked: BTreeSet<PathBuf> = BTreeSet::new();
        for at in followed {
            if !self.cache.journaled_below(&at) {
                self.cache.journals.remove(&at);
                continue;
            }
            if !wanted(&at) {
                continue;
            }
            let Some(mark) = self.cache.journals.get(&at).map(|f| f.mark.clone()) else {
                continue;
            };
            let told = app
                .names
                .follow(&at, Some(&mark.by), &self.lifetime)
                .and_then(|(_, opened)| {
                    let mut session = opened.session;
                    session.changes(Some(&mark))
                });
            let Some(Since::Journal(journal)) = told.filter(|told| match told {
                Since::Journal(journal) => journal.complete(),
                Since::TooOld => false,
            }) else {
                // Too old, not every change, or not followed: whole again.
                self.cache.invalidate(&at, true);
                self.cache.journals.remove(&at);
                changed.push((at, true));
                continue;
            };
            for change in &journal.changes {
                // A folder that came, went or is covered no more takes
                // what is below it; an entry that changed only itself and
                // the folders above it.
                let (path, whole) = match change {
                    Change::Added(n) | Change::Removed(n) => {
                        (&n.path, n.kind == EntryKind::Directory)
                    }
                    Change::Changed(n) => (&n.path, false),
                    Change::NotCovered(hole) => (&hole.path, true),
                };
                if path.starts_with(&at) {
                    self.cache.invalidate(path, whole);
                    changed.push((path.clone(), whole));
                } else if at.starts_with(path) && !matches!(change, Change::Changed(_)) {
                    // A folder above the one followed came, went or is
                    // covered no more (an index tells it since skwird
                    // 0.2): what it holds goes with it, whole.
                    self.cache.invalidate(&at, true);
                    changed.push((at.clone(), true));
                }
            }
            let holes: BTreeSet<PathBuf> = journal
                .holes
                .iter()
                .filter(|h| h.path.starts_with(&at))
                .map(|h| h.path.clone())
                .collect();
            let Some(kept) = self.cache.journals.get_mut(&at) else {
                continue;
            };
            // Not covered since, though no change said so: its changes
            // are told no more.
            let uncovered: Vec<PathBuf> = holes.difference(&kept.holes).cloned().collect();
            kept.mark = journal.mark;
            kept.holes = holes;
            for hole in uncovered {
                self.cache.invalidate(&hole, true);
                changed.push((hole, true));
            }
            let inside: Vec<PathBuf> = asked
                .range::<Path, _>((
                    std::ops::Bound::Included(at.as_path()),
                    std::ops::Bound::Unbounded,
                ))
                .take_while(|p| p.starts_with(&at))
                .cloned()
                .collect();
            for folder in inside {
                self.cache.journals.remove(&folder);
                asked.remove(&folder);
            }
            asked.insert(at);
        }
        self.touched_by(&changed)
    }

    /// The panes a change at each of `paths` concerns (`true`: what is
    /// below it too): those showing a folder holding it, and those showing
    /// one inside it where it takes what is below it. Lookups by each
    /// one's folders, never a pass over the panes for each path.
    fn touched_by(&self, paths: &[(PathBuf, bool)]) -> HashSet<PaneId> {
        let mut showing: HashMap<&Path, Vec<PaneId>> = HashMap::new();
        for (pane, active) in &self.active {
            showing
                .entry(active.target.path.as_path())
                .or_default()
                .push(*pane);
        }
        let whole: HashSet<&Path> = paths
            .iter()
            .filter(|(_, whole)| *whole)
            .map(|(path, _)| path.as_path())
            .collect();
        let mut touched = HashSet::new();
        for (path, _) in paths {
            for folder in path.ancestors() {
                if let Some(panes) = showing.get(folder) {
                    touched.extend(panes.iter().copied());
                }
            }
        }
        for (target, panes) in &showing {
            if target.ancestors().skip(1).any(|a| whole.contains(a)) {
                touched.extend(panes.iter().copied());
            }
        }
        touched
    }

    /// Wait for a message, the plugin's end, or `wait` to pass (`None`:
    /// no time limit).
    fn receive(&self, rx: &Receiver<Message>, wait: Option<Duration>) -> Received {
        let never = crossbeam_channel::never::<()>();
        let stop = self
            .stop
            .as_ref()
            .map_or(&never, skwir_core::cancel::CancelSignal::receiver);
        let timeout = wait.map_or_else(crossbeam_channel::never, crossbeam_channel::after);
        crossbeam_channel::select! {
            recv(rx) -> message => message.map_or(Received::Closed, Received::Message),
            recv(stop) -> _ => Received::Closed,
            recv(timeout) -> _ => Received::Timeout,
        }
    }

    /// How long the worker may sleep: until a breakdown is due to be
    /// scanned again, at most `REVALIDATE` while views show one (a request
    /// cancelled is noticed then), until the watches and the cache are let
    /// go of once none does, then until a message comes. It woke four
    /// times a second for as long as the plugin was on.
    fn next_wake(&self, now: Instant) -> Option<Duration> {
        if self.active.is_empty() {
            let holds = !self.roots.is_empty() || self.cache.bytes > 0;
            return self
                .idle_since
                .filter(|_| holds)
                .map(|since| (since + RELEASE_IDLE).saturating_duration_since(now));
        }
        self.active
            .values()
            .map(|state| {
                if state.pending {
                    return Duration::ZERO;
                }
                // A change waiting for its recount wakes the worker then.
                let change = state.changed.then(|| {
                    state
                        .counts_changes_at()
                        .map_or(Duration::ZERO, |at| at.saturating_duration_since(now))
                });
                let covered = self
                    .roots
                    .iter()
                    .any(|root| state.target.path.starts_with(root));
                let revalidate = if covered && state.coverage == Coverage::Full {
                    REVALIDATE
                } else {
                    let interval = REVALIDATE.max(state.last_duration * REVALIDATE_DUTY);
                    (state.last_scan + interval)
                        .saturating_duration_since(now)
                        .min(REVALIDATE)
                };
                change.map_or(revalidate, |change| change.min(revalidate))
            })
            .min()
    }

    /// With no view showing a breakdown for `RELEASE_IDLE`, let go of the
    /// watches (whose events woke the worker for a build under a folder
    /// once shown) and of the cache.
    fn let_go_when_idle(&mut self, now: Instant) {
        if !self.active.is_empty() {
            self.idle_since = None;
            return;
        }
        let since = *self.idle_since.get_or_insert(now);
        if now.duration_since(since) >= RELEASE_IDLE {
            for root in std::mem::take(&mut self.roots) {
                self.release(&root);
            }
            self.cache.clear();
            self.idle_since = None;
            skwir_core::scheduler::release_now();
        }
    }

    /// A new request replaces the pane's previous one and is scanned at once.
    fn admit(&mut self, request: Request) {
        if request.cancel.is_cancelled() {
            self.forget(request.pane, request.generation);
            return;
        }
        self.remove_active(request.pane);
        self.places = Places::read();
        let mut state = Active {
            target: Target::resolve(&request.path),
            request,
            alias_parents: BTreeSet::new(),
            alias_watched: true,
            coverage: Coverage::None,
            last_scan: Instant::now(),
            last_duration: Duration::ZERO,
            pending: false,
            changed: false,
            recounted: None,
        };
        // What a breakdown kept from an index missed while no pane asked
        // its journal (nobody looked, or none showed it) is told before it
        // is shown again: it may be shown from the cache at once.
        if let Some(app) = self.shared.app.upgrade() {
            let target = state.target.path.clone();
            let touched = self.ask_journals(&app, &|at| {
                target.starts_with(at) || at.starts_with(&target)
            });
            self.touched.extend(touched);
        }
        self.acquire_aliases(&mut state);
        self.set_cache_roots(Some(&state.target.path));
        let coverage = self.cover(&state, Instant::now());
        let pane = state.request.pane;
        self.run_scan(&mut state, coverage);
        if state.request.cancel.is_cancelled() {
            self.forget(pane, state.request.generation);
            self.release_aliases(&mut state);
            self.set_cache_roots(None);
            return;
        }
        state.request.force = false;
        self.active.insert(pane, state);
    }

    /// Drop the status of a cancelled request; a newer generation is untouched.
    fn forget(&mut self, pane: PaneId, generation: u64) {
        let mut statuses = self.shared.statuses.lock();
        if statuses
            .get(&pane)
            .is_some_and(|s| s.generation == generation)
        {
            statuses.remove(&pane);
        }
    }

    fn remove_active(&mut self, pane: PaneId) {
        if let Some(mut state) = self.active.remove(&pane) {
            self.release_aliases(&mut state);
            self.set_cache_roots(None);
        }
    }

    fn set_cache_roots(&mut self, extra: Option<&Path>) {
        let roots = self
            .active
            .values()
            .map(|a| a.target.path.clone())
            .chain(extra.map(Path::to_path_buf))
            .collect();
        self.cache.set_roots(roots);
    }

    fn acquire_aliases(&mut self, state: &mut Active) {
        state.alias_parents = state.target.alias_parents();
        state.alias_watched = true;
        for parent in state.alias_parents.clone() {
            // Registration is idempotent; a replaced directory gets a fresh watch.
            let watched = self
                .watcher
                .as_mut()
                .is_some_and(|w| matches!(register(w.as_mut(), &parent), Registration::Watched));
            self.registrations.entry(parent).or_default().alias += 1;
            state.alias_watched &= watched;
        }
    }
    fn release_aliases(&mut self, state: &mut Active) {
        for parent in std::mem::take(&mut state.alias_parents) {
            let Some(r) = self.registrations.get_mut(&parent) else {
                continue;
            };
            r.alias = r.alias.saturating_sub(1);
            if r.alias == 0 && !r.scan {
                self.registrations.remove(&parent);
                if let Some(watcher) = self.watcher.as_mut() {
                    let _ = watcher.unwatch(&parent);
                }
            }
        }
    }

    /// Register the canonical root (or touch its covering root) and report
    /// the coverage a scan can start with.
    fn cover(&mut self, state: &Active, now: Instant) -> Coverage {
        if state.request.cancel.is_cancelled() || !self.watch_root(&state.target.path, now) {
            return Coverage::None;
        }
        if state.alias_watched {
            Coverage::Full
        } else {
            Coverage::Partial
        }
    }

    fn watch_root(&mut self, path: &Path, now: Instant) -> bool {
        if let Some(index) = self.roots.iter().position(|root| path.starts_with(root)) {
            let root = self.roots.remove(index);
            self.roots.push(root);
            return true;
        }
        if self.backoff.until.is_some_and(|until| now < until) {
            return false;
        }
        let Some(watcher) = self.watcher.as_mut() else {
            return false;
        };
        match register(watcher.as_mut(), path) {
            Registration::Watched => {}
            Registration::Unwatchable => {
                self.release(path);
                return false;
            }
            Registration::Exhausted => {
                self.exhaust(path, now);
                return false;
            }
        }
        self.backoff = Backoff::default();
        // A backend can share watch descriptors across overlapping requests.
        // Unwatching a covered child here would punch a hole in the new parent.
        self.registrations
            .entry(path.to_path_buf())
            .or_default()
            .scan = true;
        self.roots.retain(|root| !root.starts_with(path));
        self.roots.push(path.to_path_buf());
        if self.roots.len() > MAX_WATCH_ROOTS {
            let old = self.roots.remove(0);
            self.release(&old);
        }
        true
    }

    /// Drop coverage below `path`: roots, scan registrations and the
    /// breakdowns they validated.
    fn release(&mut self, path: &Path) {
        self.roots.retain(|root| !root.starts_with(path));
        self.unwatch(path);
        self.cache.invalidate(path, true);
    }
    fn unwatch(&mut self, prefix: &Path) {
        let paths: Vec<PathBuf> = below(&self.registrations, prefix).cloned().collect();
        for path in paths {
            let Some(r) = self.registrations.get_mut(&path) else {
                continue;
            };
            r.scan = false;
            if r.alias > 0 {
                continue;
            }
            self.registrations.remove(&path);
            if let Some(watcher) = self.watcher.as_mut() {
                let _ = watcher.unwatch(&path);
            }
        }
    }
    /// The watch budget is spent: give the registrations back to the rest of
    /// the application and stop retrying for a growing interval.
    fn exhaust(&mut self, path: &Path, now: Instant) {
        self.release(path);
        let next = self.backoff.next.unwrap_or(WATCH_BACKOFF);
        self.backoff = Backoff {
            until: Some(now + next),
            next: Some((next * 2).min(MAX_WATCH_BACKOFF)),
        };
    }

    fn run_scan(&mut self, state: &mut Active, coverage: Coverage) {
        let started = Instant::now();
        match self.scan(&state.request, &state.target, coverage) {
            None => state.pending = true,
            Some(coverage) => {
                state.pending = false;
                state.coverage = coverage;
                state.last_scan = Instant::now();
                state.last_duration = started.elapsed();
                // What the scan freed, given back before the worker
                // sleeps: it is no scheduler worker.
                skwir_core::scheduler::release_now();
            }
        }
    }

    /// Scan and publish. `None` means the scan yielded to a newer request and
    /// must be resumed; otherwise the coverage the results were collected under.
    fn scan(&mut self, request: &Request, target: &Target, coverage: Coverage) -> Option<Coverage> {
        if request.cancel.is_cancelled() {
            return Some(coverage);
        }
        if request.force || coverage == Coverage::None {
            self.cache.invalidate(&target.path, true);
        }
        {
            let mut statuses = self.shared.statuses.lock();
            if let Some(s) = statuses
                .get_mut(&request.pane)
                .filter(|s| s.generation == request.generation)
            {
                s.scanning = true;
                s.updating = false;
            }
        }
        // Do not publish a busy state for every cache hit or short update.
        // Scanner progress wakes the UI only once real work lasts long enough.
        let started = Instant::now();
        let Worker {
            cache,
            watcher,
            registrations,
            inbox,
            deferred,
            shared,
            types,
            places,
            ..
        } = self;
        let monitor = if coverage == Coverage::None {
            None
        } else {
            watcher.as_mut().map(|watcher| Monitor {
                watcher: watcher.as_mut(),
                registrations,
                exhausted: false,
            })
        };
        let mut scanner = Scanner {
            monitor,
            inbox: inbox.as_ref().map(|rx| Inbox { rx, deferred }),
            caching: coverage != Coverage::None,
            last_progress: started,
            types: std::mem::take(types),
            places: places.clone(),
            ..Scanner::new(cache, request, &target.path, shared)
        };
        let mut tree = scanner.root();
        *types = std::mem::take(&mut scanner.types);
        let scanned_dirs = scanner.dirs;
        let errors = scanner.errors;
        let unmonitored = scanner.unmonitored;
        let exhausted = scanner.monitor.as_ref().is_some_and(|m| m.exhausted);
        if scanner.yielded {
            return None;
        }
        drop(scanner);
        let coverage = if exhausted {
            self.exhaust(&target.path, Instant::now());
            Coverage::None
        } else if unmonitored > 0 {
            coverage.min(Coverage::Partial)
        } else {
            coverage
        };
        if coverage == Coverage::None {
            self.cache.invalidate(&target.path, true);
        }
        if let Ok(node) = &mut tree {
            nest(node, &self.cache);
        }
        let mut statuses = self.shared.statuses.lock();
        if !request.cancel.is_cancelled()
            && let Some(s) = statuses
                .get_mut(&request.pane)
                .filter(|s| s.generation == request.generation)
        {
            let incomplete = tree.as_ref().is_ok_and(|n| n.incomplete);
            let error = tree
                .as_ref()
                .err()
                .map(|error| {
                    if error.downcast_ref::<std::io::Error>().is_some() {
                        words!(usage_unreadable = "Unable to read this directory")
                    } else {
                        // Words said where they are drawn, in the reader's
                        // language; any other error as it is.
                        error.downcast_ref::<Words>().map_or_else(
                            || Words::as_written(error.to_string()),
                            Clone::clone,
                        )
                    }
                })
                .or_else(|| {
                    (errors > 0).then(|| {
                        words!(
                            usage_entries_unread = "{ $count ->
                                [one] { $count } entry could not be read; totals are incomplete
                               *[other] { $count } entries could not be read; totals are incomplete
                            }",
                            count = errors,
                            note = "Said in the Usage view: entries of the folder could not be read."
                        )
                    })
                })
                .or_else(|| {
                    incomplete.then(|| {
                        words!(
                            usage_changed_during = "Directory changed during scan; totals are incomplete"
                        )
                    })
                })
                .or_else(|| match coverage {
                    Coverage::Full => None,
                    Coverage::Partial => Some(
                        words!(
                            usage_partly_watched = "Some folders cannot be monitored; rechecking periodically",
                            note = "Said in the Usage view: some folders' changes cannot be followed as they happen."
                        ),
                    ),
                    Coverage::None => Some(
                        words!(
                            usage_unwatched = "Live monitoring unavailable; rechecking periodically",
                            note = "Said in the Usage view: no folder's changes can be followed as they happen."
                        ),
                    ),
                });
            let node = tree.ok();
            *s = Status {
                root: Some(request.path.clone()),
                scanning: false,
                updating: false,
                elapsed_ms: started.elapsed().as_millis() as u64,
                files: node.as_ref().map_or(0, |n| n.files),
                dirs: node.as_ref().map_or(0, |n| n.dirs),
                size: node.as_ref().map_or(0, |n| n.size),
                alloc: node.as_ref().map_or(0, |n| n.alloc),
                snapshot: if node.is_some() {
                    SnapshotKind::Finished
                } else {
                    SnapshotKind::Progressive
                },
                tree: node,
                error,
                stale: coverage != Coverage::Full,
                scanned_dirs,
                generation: request.generation,
            };
        }
        drop(statuses);
        self.shared.changed();
        Some(coverage)
    }

    fn changes(&mut self, changes: Changes) -> HashSet<PaneId> {
        let affected = self
            .active
            .iter()
            .filter_map(|(pane, active)| {
                let target = &active.target;
                (changes.lost
                    || changes.paths.keys().any(|path| {
                        path.starts_with(&target.path)
                            || target.path.starts_with(path)
                            || target.links.iter().any(|link| link.starts_with(path))
                    }))
                .then_some(*pane)
            })
            .collect();
        if changes.lost {
            self.cache.clear();
            // A lost batch can hide replacement of a root. Rebuild coverage;
            // a failed registration then falls back only for that active path.
            for root in std::mem::take(&mut self.roots) {
                self.unwatch(&root);
            }
        }
        for (path, change) in changes.paths {
            self.cache.invalidate(&path, change >= Change::Content);
            // Removal or renaming of a registered directory, or of an ancestor
            // of a watched root, takes its registrations with it.
            if change == Change::Structural
                && (self.registrations.get(&path).is_some_and(|r| r.scan)
                    || self.roots.iter().any(|root| root.starts_with(&path)))
            {
                self.release(&path);
            }
        }
        affected
    }

    fn update(&mut self, dirty: &HashSet<PaneId>, now: Instant) {
        // A change is remembered until a scan counts it.
        for pane in dirty {
            if let Some(state) = self.active.get_mut(pane) {
                state.changed = true;
            }
        }
        // Yielded scans resume first, then changed panes, then due revalidations.
        let mut panes: Vec<PaneId> = self.active.keys().copied().collect();
        panes.sort_by_key(|pane| {
            let state = &self.active[pane];
            (!state.pending, !state.changed, pane.0)
        });
        for pane in panes {
            let Some(state) = self.active.get(&pane) else {
                continue;
            };
            if state.request.cancel.is_cancelled() {
                let generation = state.request.generation;
                self.forget(pane, generation);
                self.remove_active(pane);
                continue;
            }
            let covered = self
                .roots
                .iter()
                .any(|root| state.target.path.starts_with(root));
            let coverage = if covered {
                state.coverage
            } else {
                Coverage::None
            };
            let interval = REVALIDATE.max(state.last_duration * REVALIDATE_DUTY);
            let due = coverage != Coverage::Full && now.duration_since(state.last_scan) >= interval;
            // Changes that keep coming wait for the recount the last one
            // asked for to rest (`REVALIDATE_DUTY`).
            let changed = state.changed && state.counts_changes_at().is_none_or(|at| now >= at);
            if !(state.pending || changed || due) {
                continue;
            }
            let mut state = self.active.remove(&pane).expect("active pane");
            // Aliases may point elsewhere now; resolving costs one stat per component.
            let target = Target::resolve(&state.request.path);
            if target != state.target {
                self.release_aliases(&mut state);
                state.target = target;
                self.acquire_aliases(&mut state);
                self.set_cache_roots(Some(&state.target.path));
            }
            if !covered {
                // Never reuse data collected during a gap in coverage, even
                // when re-registering succeeds on this attempt.
                self.cache.invalidate(&state.target.path, true);
            }
            let coverage = self.cover(&state, now);
            let counted = state.changed;
            self.run_scan(&mut state, coverage);
            if !state.pending {
                // What changed before it began is counted; what changes
                // while it runs comes as another change.
                state.changed = false;
                if counted {
                    state.recounted = Some((state.last_scan, state.last_duration));
                }
            }
            let yielded = state.pending;
            self.active.insert(pane, state);
            if yielded {
                // The newer request waits in `deferred`; serve it first.
                break;
            }
        }
    }
}

pub(super) fn run(
    rx: &Receiver<Message>,
    tx: Sender<Message>,
    shared: Arc<Shared>,
    cancel: &CancelToken,
) {
    let changes = Arc::new(Mutex::new(Changes::default()));
    let watcher_changes = Arc::clone(&changes);
    let watcher = notify::RecommendedWatcher::new(
        move |event| {
            if watcher_changes.lock().push(event) {
                let _ = tx.send(Message::Changed);
            }
        },
        notify::Config::default().with_follow_symlinks(false),
    )
    .ok()
    .map(|watcher| Box::new(watcher) as Box<dyn Watcher>);
    let mut worker = Worker::new(shared, watcher, Some(rx.clone()), changes);
    worker.stop = Some(cancel.subscribe());
    worker.lifetime = cancel.clone();
    while !cancel.is_cancelled() && worker.step(rx) {}
    // Watcher/cache destruction stays on this thread, including plugin disable and app shutdown.
}

// The walk reads Unix's metadata alone so far (ADR-035).
#[cfg(all(test, unix))]
mod tests {

    /// Usage names an entry as its folder's listing does (ADR-049), so the
    /// pane's selection is found among its rows: each of a file's names.
    #[test]
    fn its_entries_are_the_listings_each_name_of_a_file_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("linked");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"one file").unwrap();
        std::fs::hard_link(dir.join("a.txt"), dir.join("b.txt")).unwrap();
        let folder = super::file_of(&std::fs::metadata(&dir).unwrap());
        let ids: Vec<_> = ["a.txt", "b.txt"]
            .iter()
            .map(|name| {
                let path = dir.join(name);
                let usage = super::entry_id(
                    &std::fs::symlink_metadata(&path).unwrap(),
                    folder,
                    name.as_ref(),
                );
                let listed = skwir_core::fs::stat::stat_path(&path).unwrap().id;
                assert_eq!(usage, listed, "{name}");
                usage
            })
            .collect();
        assert_eq!(ids[0].file, ids[1].file);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(
            super::folder_entry(&dir, &std::fs::metadata(&dir).unwrap()),
            { skwir_core::fs::stat::stat_path(&dir).unwrap().id }
        );
    }

    #[test]
    fn invalidation_takes_ancestors_and_descendants_only() {
        let mut cache = Cache::default();
        for path in ["/a", "/a/b", "/a/b/c", "/a/b/c/d", "/a/b-c", "/a/bb", "/x"] {
            cache.insert(Node {
                path: PathBuf::from(path),
                ..Node::default()
            });
        }
        let bytes = cache.bytes;
        cache.invalidate(Path::new("/a/b/c"), false);
        let left: Vec<&str> = cache.dirs.keys().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(left, ["/a/b/c/d", "/a/b-c", "/a/bb", "/x"]);
        cache.invalidate(Path::new("/a/b"), true);
        let left: Vec<&str> = cache.dirs.keys().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(left, ["/a/b-c", "/a/bb", "/x"]);
        assert_eq!(cache.order.len(), 3);
        assert!(cache.bytes < bytes);
    }

    fn row(path: &str, alloc: u64, is_dir: bool) -> Node {
        Node {
            path: PathBuf::from(path),
            name: path.rsplit('/').next().unwrap().to_owned(),
            is_dir,
            size: alloc,
            alloc,
            ..Node::default()
        }
    }

    fn breakdown(path: &str, children: Vec<Node>) -> Node {
        Node {
            alloc: children.iter().map(|c| c.alloc).sum(),
            children,
            ..row(path, 0, true)
        }
    }

    fn child<'a>(node: &'a Node, name: &str) -> &'a Node {
        node.children
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("{name} in {}", node.path.display()))
    }

    #[test]
    fn nest_opens_heavy_folders_from_the_cache_and_folds_specks() {
        const MIB: u64 = 1 << 20;
        let mut cache = Cache::default();
        cache.insert(Node {
            other_alloc: 100,
            other_size: 100,
            other_count: 3,
            ..breakdown(
                "/r/big",
                vec![
                    row("/r/big/inner", 900_000, true),
                    row("/r/big/speck", 500, false),
                ],
            )
        });
        cache.insert(breakdown(
            "/r/big/inner",
            vec![row("/r/big/inner/data", 900_000, false)],
        ));
        // Too light to be opened, though its breakdown is kept.
        cache.insert(breakdown("/r/small", vec![row("/r/small/x", 2000, false)]));
        let mut tree = Node {
            alloc: MIB,
            children: vec![
                row("/r/big", 1_000_000, true),
                row("/r/small", 2000, true),
                row("/r/file", 46_576, false),
                // Its breakdown is not kept: it stays closed.
                row("/r/unknown", 40_000, true),
            ],
            ..row("/r", 0, true)
        };
        nest(&mut tree, &cache);
        let big = child(&tree, "big");
        assert_eq!(big.children.len(), 1);
        assert_eq!(
            (big.other_count, big.other_alloc, big.other_size),
            (4, 600, 600)
        );
        let inner = child(big, "inner");
        assert_eq!(child(inner, "data").alloc, 900_000);
        assert!(child(&tree, "small").children.is_empty());
        assert!(child(&tree, "unknown").children.is_empty());
    }

    #[test]
    fn nest_opens_light_folders_of_many_files() {
        let mut cache = Cache::default();
        cache.insert(breakdown(
            "/r/web",
            (0..30)
                .map(|i| Node {
                    files: 100,
                    ..row(&format!("/r/web/pkg{i:02}"), 10, true)
                })
                .collect(),
        ));
        let mut tree = Node {
            alloc: 1_000_000,
            files: 3001,
            children: vec![
                Node {
                    files: 3000,
                    ..row("/r/web", 300, true)
                },
                row("/r/film.mkv", 999_700, false),
            ],
            ..row("/r", 0, true)
        };
        nest(&mut tree, &cache);
        // Specks by their bytes, each a thirtieth of the files.
        assert_eq!(child(&tree, "web").children.len(), 30);
    }

    #[test]
    fn nest_stops_at_its_levels_and_its_budget() {
        let mut cache = Cache::default();
        for (path, inside) in [
            ("/r/a", "/r/a/b"),
            ("/r/a/b", "/r/a/b/c"),
            ("/r/a/b/c", "/r/a/b/c/d"),
            ("/r/a/b/c/d", "/r/a/b/c/d/e"),
        ] {
            cache.insert(breakdown(path, vec![row(inside, 1000, true)]));
        }
        let mut tree = Node {
            alloc: 1000,
            children: vec![row("/r/a", 1000, true)],
            ..row("/r", 0, true)
        };
        nest(&mut tree, &cache);
        let d = child(child(child(child(&tree, "a"), "b"), "c"), "d");
        assert!(
            d.children.is_empty(),
            "{NESTED_LEVELS} levels inside the entries"
        );

        let mut cache = Cache::default();
        let many = (0..MAX_NESTED + 10)
            .map(|i| row(&format!("/r/a/{i:05}"), 1000, false))
            .collect();
        cache.insert(breakdown("/r/a", many));
        let mut tree = Node {
            alloc: 1000,
            children: vec![row("/r/a", 1000, true)],
            ..row("/r", 0, true)
        };
        nest(&mut tree, &cache);
        let a = child(&tree, "a");
        assert_eq!(a.children.len(), MAX_NESTED);
        assert_eq!(a.other_count, 10);
    }
    use super::*;
    use notify::event::{CreateKind, MetadataKind, RemoveKind, RenameMode};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn request(pane: u32, path: PathBuf) -> Request {
        Request {
            pane: PaneId(pane),
            path,
            force: false,
            generation: 1,
            cancel: CancelToken::new(),
        }
    }

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            statuses: Mutex::default(),
            revision: Arc::default(),
            wake: Arc::new(|| {}),
            app: std::sync::Weak::new(),
        })
    }

    fn scanner<'a>(cache: &'a mut Cache, request: &'a Request, shared: &'a Shared) -> Scanner<'a> {
        Scanner {
            // Deterministic first publication, without depending on disk speed.
            last_progress: Instant::now().checked_sub(PROGRESS_INTERVAL).unwrap(),
            ..Scanner::new(cache, request, &request.path, shared)
        }
    }

    fn worker(shared: Arc<Shared>, watcher: Option<Box<dyn Watcher>>) -> Worker {
        Worker::new(shared, watcher, None, Arc::default())
    }

    fn observed(request: &Request) -> (Arc<Shared>, Receiver<Status>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let pane = request.pane;
        let shared = Arc::new_cyclic(|weak: &std::sync::Weak<Shared>| {
            let weak = weak.clone();
            Shared {
                statuses: Mutex::new(HashMap::from([(
                    pane,
                    Status {
                        root: Some(request.path.clone()),
                        scanning: true,
                        generation: request.generation,
                        ..Status::default()
                    },
                )])),
                revision: Arc::default(),
                wake: Arc::new(move || {
                    if let Some(shared) = weak.upgrade() {
                        tx.send(shared.statuses.lock()[&pane].clone()).unwrap();
                    }
                }),
                app: std::sync::Weak::new(),
            }
        });
        (shared, rx)
    }

    fn register_status(shared: &Shared, request: &Request) {
        shared.statuses.lock().insert(
            request.pane,
            Status {
                root: Some(request.path.clone()),
                scanning: true,
                generation: request.generation,
                ..Status::default()
            },
        );
    }

    fn revision(shared: &Shared) -> u64 {
        shared.revision.load(Ordering::Acquire)
    }

    fn tree(root: &Path, dirs: usize, subdirs: usize) {
        for d in 0..dirs {
            let dir = root.join(format!("dir-{d:03}"));
            std::fs::create_dir(&dir).unwrap();
            for s in 0..subdirs {
                std::fs::create_dir(dir.join(format!("sub-{s:03}"))).unwrap();
            }
        }
    }

    struct InterruptingWatcher {
        cancel: CancelToken,
        calls: usize,
        stop_after: usize,
    }
    impl Watcher for InterruptingWatcher {
        fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
            unreachable!("constructed with a request token")
        }
        fn watch(&mut self, _: &Path, mode: RecursiveMode) -> notify::Result<()> {
            assert_eq!(
                mode,
                RecursiveMode::NonRecursive,
                "recursive registration bypasses cancellation"
            );
            self.calls += 1;
            if self.calls == self.stop_after {
                self.cancel.cancel();
            }
            Ok(())
        }
        fn unwatch(&mut self, _: &Path) -> notify::Result<()> {
            Ok(())
        }
        fn kind() -> notify::WatcherKind {
            notify::WatcherKind::NullWatcher
        }
    }

    /// Counts registrations; optionally fails some of them.
    #[derive(Clone, Default)]
    struct Recording {
        watched: Arc<AtomicUsize>,
        unwatched: Arc<AtomicUsize>,
        /// Registration calls beyond this many report an exhausted budget.
        budget: Option<usize>,
        /// Registrations of paths containing this fail as unwatchable.
        refuse: Option<&'static str>,
    }
    struct RecordingWatcher(Recording);
    impl Watcher for RecordingWatcher {
        fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
            unreachable!("constructed directly")
        }
        fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
            assert_eq!(mode, RecursiveMode::NonRecursive);
            if self
                .0
                .refuse
                .is_some_and(|needle| path.to_string_lossy().contains(needle))
            {
                return Err(notify::Error::path_not_found());
            }
            let n = self.0.watched.fetch_add(1, Ordering::SeqCst) + 1;
            if self.0.budget.is_some_and(|budget| n > budget) {
                return Err(notify::Error::new(ErrorKind::MaxFilesWatch));
            }
            Ok(())
        }
        fn unwatch(&mut self, _: &Path) -> notify::Result<()> {
            self.0.unwatched.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn kind() -> notify::WatcherKind {
            notify::WatcherKind::NullWatcher
        }
    }
    fn recording(recording: &Recording) -> Box<dyn Watcher> {
        Box::new(RecordingWatcher(recording.clone()))
    }

    fn scan_registrations(worker: &Worker) -> BTreeSet<PathBuf> {
        worker
            .registrations
            .iter()
            .filter(|(_, r)| r.scan)
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// An entry that is no folder says what it is itself, as its stat
    /// does, at every level: what the view lends the pane it as
    /// (ADR-116), so a link or a pipe is never taken for a file.
    #[cfg(unix)]
    #[test]
    fn an_entry_says_what_it_is_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("a folder");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(tmp.path().join("notes.txt"), b"x").unwrap();
        std::os::unix::fs::symlink("notes.txt", tmp.path().join("it's a link")).unwrap();
        std::fs::write(folder.join("inner.txt"), b"x").unwrap();
        std::os::unix::fs::symlink("inner.txt", folder.join("inner link")).unwrap();
        let request = request(0, tmp.path().into());
        let shared = shared();
        let mut cache = Cache::default();
        let tree = scanner(&mut cache, &request, &shared).root().unwrap();
        let kind = |node: &Node, name: &str| {
            node.children
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.entry_kind)
        };
        assert_eq!(kind(&tree, "notes.txt"), Some(Some(EntryKind::File)));
        assert_eq!(kind(&tree, "it's a link"), Some(Some(EntryKind::Symlink)));
        assert_eq!(kind(&tree, "a folder"), Some(None));
        let inside = cache.get(&folder).expect("counted");
        assert_eq!(kind(&inside, "inner.txt"), Some(Some(EntryKind::File)));
        assert_eq!(kind(&inside, "inner link"), Some(Some(EntryKind::Symlink)));
    }

    #[test]
    fn navigation_interrupts_watch_setup_before_visiting_the_remaining_tree() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..12 {
            let dir = tmp.path().join(format!("Folder-{i}"));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("example.txt"), b"data").unwrap();
        }
        let request = request(0, tmp.path().into());
        let shared = shared();
        let mut cache = Cache::default();
        let mut registrations = Registrations::new();
        let mut watcher = InterruptingWatcher {
            cancel: request.cancel.clone(),
            calls: 0,
            stop_after: 2,
        };
        let mut scan = scanner(&mut cache, &request, &shared);
        scan.monitor = Some(Monitor {
            watcher: &mut watcher,
            registrations: &mut registrations,
            exhausted: false,
        });
        assert!(scan.root().is_err());
        drop(scan);
        assert_eq!(watcher.calls, 2);
        assert!(cache.dirs.is_empty());
        assert_eq!(registrations.len(), 2);
        // A replacement request can finish with existing watches, without
        // accepting the cancelled root as a cached result.
        let fresh = self::request(0, tmp.path().into());
        watcher.stop_after = usize::MAX;
        let mut scan = scanner(&mut cache, &fresh, &shared);
        scan.monitor = Some(Monitor {
            watcher: &mut watcher,
            registrations: &mut registrations,
            exhausted: false,
        });
        let tree = scan.root().unwrap();
        assert_eq!(tree.size, 48);
        assert_eq!(tree.dirs, 12);
        assert!(!tree.incomplete);
        assert_eq!(registrations.len(), 13);
    }

    #[test]
    fn refresh_progress_retains_finished_results_until_replacement_is_ready() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("child")).unwrap();
        let file = tmp.path().join("child/payload");
        std::fs::write(&file, b"old").unwrap();
        let mut request = request(0, tmp.path().into());
        let (shared, updates) = observed(&request);
        let mut worker = worker(Arc::clone(&shared), None);
        let target = Target::resolve(&request.path);
        worker.scan(&request, &target, Coverage::None);
        assert_eq!(
            shared.statuses.lock()[&request.pane].snapshot,
            SnapshotKind::Finished
        );
        // Repeated fallback passes, explicit refresh, and a finished result
        // with read errors all retain the displayed breakdown during work.
        for (index, (force, incomplete)) in [
            (false, false),
            (false, false),
            (true, false),
            (true, true),
            (false, true),
        ]
        .into_iter()
        .enumerate()
        {
            request.force = force;
            {
                let mut statuses = shared.statuses.lock();
                let s = statuses.get_mut(&request.pane).unwrap();
                s.scanning = true;
                s.tree.as_mut().unwrap().incomplete = incomplete;
                s.error = incomplete.then(|| Words::as_written("Some entries could not be read"));
            }
            let before = shared.statuses.lock()[&request.pane].clone();
            updates.try_iter().for_each(drop);
            std::fs::write(&file, vec![0; 10 + index]).unwrap();
            worker.cache.clear();
            // Force a progress checkpoint independently of filesystem speed.
            let fresh = scanner(&mut worker.cache, &request, &shared)
                .root()
                .unwrap();
            assert_eq!(fresh.size, (10 + index) as u64);
            let during = shared.statuses.lock()[&request.pane].clone();
            assert_eq!(
                serde_json::to_value(&during.tree).unwrap(),
                serde_json::to_value(&before.tree).unwrap()
            );
            assert_eq!(
                (during.size, during.alloc, during.files, during.dirs),
                (before.size, before.alloc, before.files, before.dirs)
            );
            assert_eq!(during.error, before.error);
            assert_eq!(during.updating, force);
            let progress: Vec<_> = updates.try_iter().collect();
            assert_eq!(
                !progress.is_empty(),
                force,
                "automatic refresh must stay silent"
            );
            worker.scan(&request, &target, Coverage::None);
            let finished = shared.statuses.lock()[&request.pane].clone();
            assert_eq!(finished.size, fresh.size);
            assert!(!finished.scanning && !finished.updating);
            assert!(!finished.tree.unwrap().incomplete);
        }
    }

    #[test]
    fn cold_roots_read_each_entry_once_and_show_the_visible_level_first() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("direct");
        std::fs::write(&file, b"old").unwrap();
        std::fs::create_dir(tmp.path().join("child")).unwrap();
        std::fs::write(tmp.path().join("child/payload"), b"data").unwrap();
        let request = request(0, tmp.path().into());
        let (shared, updates) = observed(&request);
        let mut worker = worker(Arc::clone(&shared), None);
        let target = Target::resolve(&request.path);
        worker.scan(&request, &target, Coverage::None);
        let published: Vec<_> = updates.try_iter().collect();
        let initial = published.first().unwrap();
        assert_eq!(
            initial.size, 3,
            "the visible level is shown before any subtree"
        );
        assert!(initial.scanning);
        assert!(initial.tree.as_ref().unwrap().incomplete);
        let final_status = published.last().unwrap();
        assert!(!final_status.scanning);
        assert_eq!(final_status.size, 7);
        assert_eq!(final_status.scanned_dirs, 2, "each directory is read once");
        assert!(!final_status.tree.as_ref().unwrap().incomplete);
    }

    #[test]
    fn progressive_root_shows_files_before_subtrees_and_finishes_with_exact_totals() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("summary.txt"), b"hello").unwrap();
        for index in 0..80 {
            let dir = tmp.path().join(format!("folder-{index:02}"));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("payload"), [0; 37]).unwrap();
        }
        let request = request(0, tmp.path().into());
        let (shared, updates) = observed(&request);
        let mut cache = Cache::default();
        let result = scanner(&mut cache, &request, &shared).root().unwrap();
        let first = updates.try_recv().unwrap();
        assert!(first.scanning && first.tree.as_ref().unwrap().incomplete);
        assert_eq!((first.size, first.files, first.dirs), (5, 1, 80));
        let tree = first.tree.unwrap();
        assert_eq!(tree.children.len(), MAX_CHILDREN);
        assert!(
            tree.children
                .iter()
                .any(|n| n.name == "summary.txt" && !n.incomplete)
        );
        assert!(
            tree.children
                .iter()
                .filter(|n| n.is_dir)
                .all(|n| n.incomplete && n.id.is_some())
        );
        assert!(tree.other_incomplete > 0);
        assert!(!result.incomplete);
        assert_eq!(
            (result.size, result.files, result.dirs),
            (5 + 80 * 37, 81, 80)
        );
        assert_eq!(result.other_count, 17);
        assert_eq!(result.other_incomplete, 0);
        assert_eq!(
            result.children.iter().map(|n| n.size).sum::<u64>() + result.other_size,
            result.size
        );
        assert_eq!(
            result.children.iter().map(|n| n.alloc).sum::<u64>() + result.other_alloc,
            result.alloc
        );
        assert!(cache.dirs.values().all(|(node, _, _)| !node.incomplete));
        let revision = self::revision(&shared);
        assert_eq!(
            scanner(&mut cache, &request, &shared).root().unwrap().size,
            result.size
        );
        assert_eq!(
            self::revision(&shared),
            revision,
            "a cached root publishes no partial updates"
        );
    }

    #[test]
    fn a_file_with_several_links_is_counted_once_where_they_meet() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("photos/nested")).unwrap();
        std::fs::create_dir(root.join("backup")).unwrap();
        std::fs::write(root.join("photos/a.bin"), [7; 1000]).unwrap();
        std::fs::hard_link(
            root.join("photos/a.bin"),
            root.join("photos/nested/again.bin"),
        )
        .unwrap();
        std::fs::hard_link(root.join("photos/a.bin"), root.join("backup/a copy.bin")).unwrap();
        std::fs::write(root.join("own.txt"), b"hello").unwrap();
        let alloc = |path: &str| meta::allocated(&std::fs::metadata(root.join(path)).unwrap());

        let request = request(0, root.into());
        let (shared, updates) = observed(&request);
        let mut cache = Cache::default();
        let result = scanner(&mut cache, &request, &shared).root().unwrap();
        let child = |name: &str| {
            result
                .children
                .iter()
                .find(|n| n.name == name)
                .unwrap()
                .clone()
        };
        // Each folder holds the file once, however many links it holds.
        assert_eq!(child("photos").size, 1000);
        assert_eq!(child("photos").alloc, alloc("photos/a.bin"));
        assert_eq!(child("backup").size, 1000);
        // Their parent holds it once too: the disk holds it once.
        assert_eq!(result.size, 1005);
        assert_eq!(result.alloc, alloc("photos/a.bin") + alloc("own.txt"));
        assert_eq!(result.files, 4, "every name is an item");
        // Deleting a folder frees what has no name outside it (ADR-049):
        // `photos` and `backup` each share the file, the root does not.
        assert_eq!(child("photos").shared, alloc("photos/a.bin"));
        assert_eq!(child("backup").shared, alloc("photos/a.bin"));
        assert_eq!(result.shared, 0);
        // What was shown while counting never said more than there is.
        while let Ok(update) = updates.try_recv() {
            assert!(update.size <= result.size, "{} counted so far", update.size);
        }
        // The same kept or counted again, from the root or from a folder.
        assert_eq!(
            scanner(&mut cache, &request, &shared).root().unwrap().size,
            1005
        );
        let photos = self::request(0, root.join("photos"));
        assert_eq!(
            scanner(&mut cache, &photos, &shared).root().unwrap().size,
            1000
        );
        let mut cold = Cache::default();
        assert_eq!(
            scanner(&mut cold, &photos, &shared).root().unwrap().size,
            1000
        );
    }

    #[test]
    fn a_folder_says_its_kinds_its_newest_change_and_the_room_it_gives_back() {
        use skwir_platform::mime::Kind;
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        for dir in ["film/clips", "build/out", "not a cache", "Trash", "empty"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let write = |path: &str, bytes: &[u8]| std::fs::write(root.join(path), bytes).unwrap();
        write("film/clips/a.mkv", &[1; 3000]);
        write("film/poster.jpg", &[2; 2000]);
        write("film/README", b"notes");
        write(
            "build/CACHEDIR.TAG",
            b"Signature: 8a477f597d28d172789f06886806bc55\n",
        );
        write("build/out/lib.o", &[3; 5000]);
        write("not a cache/CACHEDIR.TAG", b"Signature: none");
        write("Trash/old.iso", &[4; 4000]);
        // After what they hold, which changes a folder.
        for (path, secs) in [
            ("film/clips/a.mkv", 3000),
            ("film/clips", 3500),
            ("film/poster.jpg", 1000),
            ("film/README", 2000),
            ("film", 400),
            // Later than anything the test writes.
            ("empty", 4_000_000_000),
        ] {
            std::fs::File::open(root.join(path))
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs))
                .unwrap();
        }
        let alloc = |path: &str| meta::allocated(&std::fs::metadata(root.join(path)).unwrap());

        let request = request(0, root.clone());
        let shared = shared();
        let mut cache = Cache::default();
        let tree = Scanner {
            places: Places::new(Vec::new(), vec![root.join("Trash")]),
            ..scanner(&mut cache, &request, &shared)
        }
        .root()
        .unwrap();
        let film = child(&tree, "film");
        assert_eq!(film.modified, 3000, "as new as its newest file");
        let empty = child(&tree, "empty");
        assert_eq!(
            empty.modified, 4_000_000_000,
            "without files, as new as itself"
        );
        assert!(
            tree.modified < empty.modified,
            "which says nothing of its parent"
        );
        assert_eq!(
            film.kinds.iter().collect::<Vec<_>>(),
            [
                (Some(Kind::Image), alloc("film/poster.jpg")),
                (Some(Kind::Video), alloc("film/clips/a.mkv")),
                (None, alloc("film/README")),
            ]
        );
        let kept = cache.peek(&root.join("film")).unwrap();
        assert_eq!(child(kept, "poster.jpg").kind, Some(Kind::Image));
        assert_eq!(
            tree.kinds.iter().map(|(_, bytes)| bytes).sum::<u64>(),
            tree.alloc,
            "every file of some kind"
        );

        let build = child(&tree, "build");
        assert_eq!(build.reclaim, Some(marks::Reclaim::Cache));
        assert_eq!(build.reclaimable, build.alloc);
        let fake = child(&tree, "not a cache");
        assert_eq!((fake.reclaim, fake.reclaimable), (None, 0));
        let trash = child(&tree, "Trash");
        assert_eq!(trash.reclaim, Some(marks::Reclaim::Trash));
        assert_eq!(tree.reclaimable, build.alloc + trash.alloc);
        assert!(tree.reclaim.is_none());

        // Kept, a breakdown says the same: the folder shown is a cache.
        let build = self::request(0, root.join("build"));
        let kept = scanner(&mut cache, &build, &shared).root().unwrap();
        assert_eq!(kept.reclaim, Some(marks::Reclaim::Cache));
        let mut cold = Cache::default();
        let counted = scanner(&mut cold, &build, &shared).root().unwrap();
        assert_eq!(counted.reclaim, Some(marks::Reclaim::Cache));
        assert_eq!(
            counted.reclaimable,
            alloc("build/out/lib.o") + alloc("build/CACHEDIR.TAG")
        );
    }

    #[test]
    fn the_other_items_say_their_files_and_newest_change() {
        let mut node = breakdown(
            "/r",
            (0..MAX_CHILDREN + MAX_CROWDED + 2)
                .map(|i| Node {
                    files: 3,
                    modified: 1000 - i64::try_from(i).unwrap(),
                    ..row(&format!("/r/{i:03}"), 1000 - i as u64, true)
                })
                .collect(),
        );
        trim(&mut node);
        // The two lightest, the older of which changed last.
        assert_eq!(
            (node.other_count, node.other_files, node.other_modified),
            (
                2,
                6,
                1000 - i64::try_from(MAX_CHILDREN + MAX_CROWDED).unwrap()
            )
        );

        // Light folders of many files stay, for the map sized by files.
        let mut node = breakdown(
            "/r",
            (0..MAX_CHILDREN)
                .map(|i| row(&format!("/r/heavy {i:03}"), 1000, false))
                .chain((0..MAX_CROWDED + 4).map(|i| Node {
                    files: 10 + i as u64,
                    ..row(&format!("/r/crowded {i:03}"), 1, true)
                }))
                .collect(),
        );
        trim(&mut node);
        assert_eq!(node.children.len(), MAX_CHILDREN + MAX_CROWDED);
        assert_eq!((node.other_count, node.other_files), (4, 10 + 11 + 12 + 13));
    }

    #[test]
    fn progressive_branch_counts_nested_and_cached_contents_once() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("child");
        std::fs::create_dir_all(child.join("nested")).unwrap();
        std::fs::write(child.join("direct"), b"one").unwrap();
        std::fs::write(child.join("nested/file"), b"two!").unwrap();
        let request = request(0, tmp.path().into());
        let shared = shared();
        let mut cache = Cache::default();
        let id = folder_entry(&child, &std::fs::metadata(&child).unwrap());
        for pass in 0..2 {
            // On the second pass reuse the nested cache but recalculate its parent.
            let pending = Node {
                path: child.clone(),
                is_dir: true,
                incomplete: true,
                ..Node::default()
            };
            let mut scan = scanner(&mut cache, &request, &shared);
            scan.dev = id.file.dev;
            scan.preview = Some(Progress {
                tree: Node {
                    dirs: 1,
                    children: vec![pending.clone()],
                    ..Node::default()
                },
                branch: Some(pending),
            });
            let result = scan.directory(&child, 1, id, 0).unwrap();
            let partial = scan.preview.as_ref().unwrap().snapshot();
            assert_eq!((partial.size, partial.files, partial.dirs), (7, 2, 2));
            assert!(partial.children[0].incomplete && partial.children[0].id.is_some());
            assert_eq!(partial.children[0].alloc, result.alloc);
            assert_eq!(scan.dirs, if pass == 0 { 2 } else { 1 });
            cache.invalidate(&child, false);
        }
    }

    #[test]
    fn cancelling_a_partial_scan_never_caches_the_unfinished_root() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("child")).unwrap();
        std::fs::write(tmp.path().join("child/payload"), b"data").unwrap();
        let request = request(0, tmp.path().into());
        let cancel = request.cancel.clone();
        let shared = Shared {
            statuses: Mutex::new(HashMap::from([(
                request.pane,
                Status {
                    generation: 1,
                    ..Status::default()
                },
            )])),
            revision: Arc::default(),
            wake: Arc::new(move || cancel.cancel()),
            app: std::sync::Weak::new(),
        };
        let mut cache = Cache::default();
        assert!(scanner(&mut cache, &request, &shared).root().is_err());
        assert!(
            shared.statuses.lock()[&request.pane]
                .tree
                .as_ref()
                .unwrap()
                .incomplete
        );
        assert!(cache.get(tmp.path()).is_none());
        let fresh = self::request(1, tmp.path().into());
        let result = scanner(&mut cache, &fresh, &shared).root().unwrap();
        assert_eq!(result.size, 4);
        assert!(!result.incomplete);
    }

    #[test]
    fn updates_only_touch_intersecting_panes_and_due_fallbacks() {
        let tmp = tempfile::tempdir().unwrap();
        let left = tmp.path().join("left");
        let right = tmp.path().join("right");
        let fallback = tmp.path().join("fallback");
        for path in [&left, &right, &fallback] {
            std::fs::create_dir(path).unwrap();
            std::fs::write(path.join("file"), b"original").unwrap();
        }
        let shared = shared();
        let recording = Recording {
            refuse: Some("fallback"),
            ..Recording::default()
        };
        let mut worker = worker(Arc::clone(&shared), Some(self::recording(&recording)));
        for (pane, path) in [left.clone(), right.clone(), fallback.clone()]
            .into_iter()
            .enumerate()
        {
            let request = request(pane as u32, path);
            register_status(&shared, &request);
            worker.admit(request);
        }
        assert_eq!(worker.active[&PaneId(0)].coverage, Coverage::Full);
        assert_eq!(worker.active[&PaneId(2)].coverage, Coverage::None);
        let right_before = serde_json::to_value(&shared.statuses.lock()[&PaneId(1)]).unwrap();
        let fallback_deadline = worker.active[&PaneId(2)].last_scan + REVALIDATE;
        std::fs::write(left.join("file"), b"changed").unwrap();
        let dirty = worker.changes(Changes {
            paths: HashMap::from([(left.join("file"), Change::Content)]),
            ..Changes::default()
        });
        assert_eq!(dirty, HashSet::from([PaneId(0)]));
        worker.update(&dirty, Instant::now());
        assert_eq!(shared.statuses.lock()[&PaneId(0)].size, 7);
        // Unrelated changes cannot postpone the fallback deadline.
        std::fs::write(fallback.join("file"), b"updated fallback").unwrap();
        worker.update(&HashSet::new(), fallback_deadline);
        assert_eq!(shared.statuses.lock()[&PaneId(2)].size, 16);
        assert!(shared.statuses.lock()[&PaneId(2)].stale);
        assert_eq!(
            serde_json::to_value(&shared.statuses.lock()[&PaneId(1)]).unwrap(),
            right_before
        );

        let revision = self::revision(&shared);
        let dirty = worker.changes(Changes {
            paths: HashMap::from([(tmp.path().join("previously-visited/file"), Change::Content)]),
            ..Changes::default()
        });
        assert!(dirty.is_empty());
        worker.update(&dirty, Instant::now());
        assert_eq!(self::revision(&shared), revision);
        let dirty = worker.changes(Changes {
            paths: HashMap::from([(tmp.path().to_path_buf(), Change::Structural)]),
            ..Changes::default()
        });
        assert_eq!(
            dirty.len(),
            3,
            "an ancestor replacement affects every descendant pane"
        );
    }

    #[test]
    fn progress_is_delayed_and_cannot_outlive_its_request() {
        let shared = shared();
        let request = request(0, PathBuf::from("/synthetic"));
        shared.statuses.lock().insert(
            request.pane,
            Status {
                scanning: true,
                generation: 1,
                ..Status::default()
            },
        );
        let mut cache = Cache::default();
        cache.insert(Node {
            path: request.path.clone(),
            ..Node::default()
        });
        let started = Instant::now();
        let mut scanner = Scanner {
            dirs: 2,
            last_progress: started,
            ..Scanner::new(&mut cache, &request, &request.path, &shared)
        };
        scanner
            .directory(&request.path, 0, EntryId::UNKNOWN, 0)
            .unwrap();
        scanner.progress(started + PROGRESS_INTERVAL / 2);
        assert!(!shared.statuses.lock()[&request.pane].updating);
        assert_eq!(revision(&shared), 0);
        scanner.progress(started + PROGRESS_INTERVAL);
        assert!(shared.statuses.lock()[&request.pane].updating);
        assert_eq!(shared.statuses.lock()[&request.pane].scanned_dirs, 2);
        shared
            .statuses
            .lock()
            .get_mut(&request.pane)
            .unwrap()
            .generation = 2;
        scanner.progress(started + PROGRESS_INTERVAL * 2);
        assert_eq!(revision(&shared), 1);
        shared
            .statuses
            .lock()
            .get_mut(&request.pane)
            .unwrap()
            .generation = 1;
        request.cancel.cancel();
        scanner.progress(started + PROGRESS_INTERVAL * 3);
        assert_eq!(revision(&shared), 1);
    }

    #[test]
    fn eviction_prefers_deep_entries_and_bounds_memory() {
        let mut cache = Cache::default();
        cache.set_roots(vec![PathBuf::from("/synthetic")]);
        for index in 0..MAX_DIRECTORIES {
            cache.insert(Node {
                path: PathBuf::from(format!("/synthetic/{index}")),
                ..Node::default()
            });
        }
        assert!(cache.get(Path::new("/synthetic/0")).is_some());
        cache.insert(Node {
            path: PathBuf::from("/synthetic/1/deep"),
            ..Node::default()
        });
        assert!(
            cache.get(Path::new("/synthetic/1/deep")).is_none(),
            "a deep entry is evicted before protected direct children"
        );
        assert!(cache.get(Path::new("/synthetic/0")).is_some());
        assert!(cache.get(Path::new("/synthetic/1")).is_some());
        cache.set_roots(vec![PathBuf::from("/elsewhere")]);
        cache.insert(Node {
            path: PathBuf::from("/synthetic/new"),
            ..Node::default()
        });
        assert!(
            cache.get(Path::new("/synthetic/2")).is_none(),
            "without protection the least recently used sibling goes first"
        );
        assert!(cache.get(Path::new("/synthetic/0")).is_some());
        assert!(cache.get(Path::new("/synthetic/1")).is_some());
        cache.insert(Node {
            path: PathBuf::from("/synthetic/oversized"),
            name: "x".repeat(MAX_CACHE_BYTES),
            ..Node::default()
        });
        assert!(cache.bytes <= MAX_CACHE_BYTES);
        assert_eq!(cache.dirs.len(), cache.order.len());
    }

    #[test]
    fn unchanged_siblings_are_reused_in_trees_larger_than_the_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let (dirs, subdirs) = (80, 60);
        tree(tmp.path(), dirs, subdirs);
        assert!(dirs * subdirs + dirs > MAX_DIRECTORIES);
        let leaf = tmp.path().join("dir-037/sub-011");
        std::fs::write(leaf.join("payload"), b"first").unwrap();
        let request = request(0, tmp.path().into());
        let shared = shared();
        let mut cache = Cache::default();
        cache.set_roots(vec![request.path.clone()]);
        let mut cold = scanner(&mut cache, &request, &shared);
        let first = cold.root().unwrap();
        assert_eq!(first.dirs as usize, dirs * subdirs + dirs);
        assert_eq!(cold.dirs as usize, dirs * subdirs + dirs + 1);
        drop(cold);
        assert_eq!(cache.dirs.len(), MAX_DIRECTORIES);
        for d in 0..dirs {
            assert!(
                cache
                    .dirs
                    .contains_key(&tmp.path().join(format!("dir-{d:03}"))),
                "direct children of the root survive eviction"
            );
        }
        std::fs::write(leaf.join("payload"), b"second!").unwrap();
        cache.invalidate(&leaf.join("payload"), true);
        let mut warm = scanner(&mut cache, &request, &shared);
        let second = warm.root().unwrap();
        assert_eq!(second.size, 7);
        assert!(
            warm.dirs as usize <= subdirs + 3,
            "only the changed chain and evicted leaves are re-read: {}",
            warm.dirs
        );
    }

    #[test]
    fn lost_batches_keep_revalidating_instead_of_trusting_old_coverage() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("file");
        std::fs::write(&file, b"a").unwrap();
        let request = request(0, tmp.path().into());
        let shared = shared();
        register_status(&shared, &request);
        let mut worker = worker(Arc::clone(&shared), None);
        worker.roots = vec![tmp.path().into()];
        let target = Target::resolve(&request.path);
        worker.scan(&request, &target, Coverage::Full);
        assert!(!shared.statuses.lock()[&request.pane].stale);
        worker.changes(Changes {
            lost: true,
            ..Changes::default()
        });
        for data in [b"new".as_slice(), b"newer".as_slice()] {
            std::fs::write(&file, data).unwrap();
            assert!(worker.roots.is_empty());
            worker.scan(&request, &target, Coverage::None);
            let statuses = shared.statuses.lock();
            assert_eq!(statuses[&request.pane].size, data.len() as u64);
            assert!(statuses[&request.pane].stale);
        }
    }

    #[test]
    fn event_bursts_are_bounded_and_lost_events_require_revalidation() {
        let mut changes = Changes::default();
        for index in 0..10_000 {
            let notify = changes.push(Ok(notify::Event::new(EventKind::Create(CreateKind::File))
                .add_path(PathBuf::from(format!("/synthetic/{index}")))));
            assert_eq!(notify, index == 0, "only one wake can be queued per batch");
        }
        assert!(changes.lost);
        assert!(changes.paths.is_empty());
        let mut fresh = Changes::default();
        fresh.push(Err(notify::Error::generic("synthetic overflow")));
        assert!(fresh.lost);
        fresh.push(Ok(notify::Event::new(EventKind::Modify(ModifyKind::Any))
            .add_path(PathBuf::from("/synthetic/file"))));
        assert!(fresh.paths.is_empty());
    }

    #[test]
    fn event_kinds_are_kept_and_escalate_per_path() {
        let mut changes = Changes::default();
        let path = PathBuf::from("/synthetic/dir");
        assert!(
            !changes.push(Ok(notify::Event::new(EventKind::Access(
                notify::event::AccessKind::Any
            ))
            .add_path(path.clone())))
        );
        assert!(changes.paths.is_empty());
        changes.push(Ok(notify::Event::new(EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Any),
        ))
        .add_path(path.clone())));
        assert_eq!(changes.paths[&path], Change::Attribute);
        changes.push(Ok(notify::Event::new(EventKind::Create(
            CreateKind::Folder,
        ))
        .add_path(path.clone())));
        assert_eq!(changes.paths[&path], Change::Content);
        changes.push(Ok(notify::Event::new(EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Any),
        ))
        .add_path(path.clone())));
        assert_eq!(
            changes.paths[&path],
            Change::Content,
            "kinds never downgrade"
        );
        changes.push(Ok(notify::Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::From,
        )))
        .add_path(path.clone())));
        assert_eq!(changes.paths[&path], Change::Structural);
        assert_eq!(
            Change::classify(EventKind::Remove(RemoveKind::Folder)),
            Some(Change::Structural)
        );
    }

    #[test]
    fn attribute_events_keep_watches_and_reread_only_the_changed_chain() {
        let tmp = tempfile::tempdir().unwrap();
        for side in ["left", "right"] {
            std::fs::create_dir_all(tmp.path().join(side).join("deep")).unwrap();
            std::fs::write(tmp.path().join(side).join("deep/file"), b"data").unwrap();
        }
        let request = request(0, tmp.path().into());
        let shared = shared();
        register_status(&shared, &request);
        let recording = Recording::default();
        let mut worker = worker(Arc::clone(&shared), Some(self::recording(&recording)));
        worker.admit(request);
        assert_eq!(worker.active[&PaneId(0)].coverage, Coverage::Full);
        assert_eq!(scan_registrations(&worker).len(), 5);
        assert_eq!(shared.statuses.lock()[&PaneId(0)].size, 8);

        // chmod/touch/rsync -a on a registered directory.
        let dirty = worker.changes(Changes {
            paths: HashMap::from([(tmp.path().join("left/deep"), Change::Attribute)]),
            ..Changes::default()
        });
        assert_eq!(dirty, HashSet::from([PaneId(0)]));
        assert_eq!(scan_registrations(&worker).len(), 5);
        assert_eq!(recording.unwatched.load(Ordering::SeqCst), 0);
        assert!(
            worker
                .cache
                .dirs
                .contains_key(&tmp.path().join("right/deep")),
            "attribute events do not invalidate unrelated subtrees"
        );
        let watched = recording.watched.load(Ordering::SeqCst);
        worker.update(&dirty, Instant::now());
        let status = shared.statuses.lock()[&PaneId(0)].clone();
        assert_eq!(status.scanned_dirs, 3, "root, left and left/deep");
        assert_eq!(status.size, 8);
        assert!(!status.stale);
        assert_eq!(recording.watched.load(Ordering::SeqCst), watched);
        assert_eq!(worker.active[&PaneId(0)].coverage, Coverage::Full);

        // Removal still drops the subtree's registrations, and a rescan restores them.
        std::fs::remove_dir_all(tmp.path().join("left/deep")).unwrap();
        let dirty = worker.changes(Changes {
            paths: HashMap::from([(tmp.path().join("left/deep"), Change::Structural)]),
            ..Changes::default()
        });
        assert_eq!(scan_registrations(&worker).len(), 4);
        assert_eq!(recording.unwatched.load(Ordering::SeqCst), 1);
        // Once the first recount has rested (`REVALIDATE_DUTY`).
        worker.update(&dirty, Instant::now() + Duration::from_secs(1));
        assert_eq!(shared.statuses.lock()[&PaneId(0)].size, 4);
        assert_eq!(scan_registrations(&worker).len(), 4);
    }

    #[test]
    fn unreadable_subdirectories_are_read_errors_not_coverage_gaps() {
        use std::os::unix::fs::PermissionsExt;
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("ok")).unwrap();
        std::fs::write(root.join("ok/file"), b"data").unwrap();
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("hidden"), b"xyz").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let _restore = Restore(locked.clone());
        if std::fs::read_dir(&locked).is_ok() {
            eprintln!("skipped: permissions are not enforced for this user");
            return;
        }
        let Ok(watcher) = notify::RecommendedWatcher::new(|_| {}, notify::Config::default()) else {
            eprintln!("skipped: no native watcher");
            return;
        };
        let request = request(0, root.clone());
        let shared = shared();
        register_status(&shared, &request);
        let mut worker = worker(
            Arc::clone(&shared),
            Some(Box::new(watcher) as Box<dyn Watcher>),
        );
        worker.admit(request);
        let status = shared.statuses.lock()[&PaneId(0)].clone();
        assert_eq!(status.size, 4);
        assert!(!status.stale, "one unreadable folder is not a coverage gap");
        assert!(status.tree.unwrap().incomplete);
        assert!(
            status
                .error
                .unwrap()
                .english()
                .starts_with("1 entry could not be read")
        );
        assert_eq!(worker.active[&PaneId(0)].coverage, Coverage::Full);
        let registered = scan_registrations(&worker);
        assert!(registered.contains(&root.join("ok")));
        assert!(!registered.contains(&locked));
        assert!(worker.cache.dirs.contains_key(&root.join("ok")));
        // No filesystem change: no revalidation, however long the wait.
        let revision = self::revision(&shared);
        let started = Instant::now();
        for k in 1..=3 {
            worker.update(&HashSet::new(), started + REVALIDATE * 3 * k);
        }
        assert_eq!(self::revision(&shared), revision);
        assert_eq!(shared.statuses.lock()[&PaneId(0)].scanned_dirs, 2);
    }

    #[test]
    fn exhausted_watch_budgets_release_registrations_and_back_off() {
        let tmp = tempfile::tempdir().unwrap();
        tree(tmp.path(), 3, 2);
        let request = request(0, tmp.path().into());
        let shared = shared();
        register_status(&shared, &request);
        let recording = Recording {
            budget: Some(4),
            ..Recording::default()
        };
        let mut worker = worker(Arc::clone(&shared), Some(self::recording(&recording)));
        worker.admit(request);
        let started = worker.active[&PaneId(0)].last_scan;
        let status = shared.statuses.lock()[&PaneId(0)].clone();
        assert_eq!(status.dirs, 9);
        assert!(status.stale);
        assert_eq!(worker.active[&PaneId(0)].coverage, Coverage::None);
        assert!(
            scan_registrations(&worker).is_empty(),
            "partial registrations are given back to the rest of the app"
        );
        assert_eq!(recording.unwatched.load(Ordering::SeqCst), 4);
        assert!(worker.roots.is_empty());
        assert!(worker.cache.dirs.is_empty());
        // Revalidation continues without touching the watcher during the backoff.
        let watched = recording.watched.load(Ordering::SeqCst);
        std::fs::write(tmp.path().join("dir-000/sub-000/file"), b"late").unwrap();
        worker.update(&HashSet::new(), started + REVALIDATE);
        assert_eq!(shared.statuses.lock()[&PaneId(0)].size, 4);
        assert_eq!(recording.watched.load(Ordering::SeqCst), watched);
        // Once the backoff elapses registration is attempted again.
        worker.update(&HashSet::new(), started + WATCH_BACKOFF + REVALIDATE);
        assert!(recording.watched.load(Ordering::SeqCst) > watched);
        assert!(worker.backoff.until.unwrap() > started + WATCH_BACKOFF);
    }

    /// A change is counted at once, however long the first scan took;
    /// changes that keep coming wait until the recount the last one asked
    /// for has rested four times its own duration, remembered meanwhile,
    /// and the worker wakes for them then.
    #[test]
    fn changes_that_keep_coming_wait_for_their_recount_to_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("l'été");
        std::fs::create_dir(&folder).unwrap();
        let request = request(0, tmp.path().into());
        let shared = shared();
        register_status(&shared, &request);
        let recording = Recording::default();
        let mut worker = worker(Arc::clone(&shared), Some(self::recording(&recording)));
        worker.admit(request);
        assert_eq!(worker.active[&PaneId(0)].coverage, Coverage::Full);
        // A file of 4 bytes written, and its change told.
        let write = |worker: &mut Worker, name: &str| {
            std::fs::write(folder.join(name), b"data").unwrap();
            worker.changes(Changes {
                paths: HashMap::from([(folder.join(name), Change::Content)]),
                ..Changes::default()
            })
        };
        let size = || shared.statuses.lock()[&PaneId(0)].size;

        // A first scan of a minute does not hold the first change back.
        worker.active.get_mut(&PaneId(0)).unwrap().last_duration = Duration::from_secs(60);
        let dirty = write(&mut worker, "first");
        worker.update(&dirty, Instant::now());
        assert_eq!(size(), 4, "counted at once");

        // Its recount took a second (as a folder of 100,000 files may):
        // the next change waits four.
        let ended = Instant::now();
        worker.active.get_mut(&PaneId(0)).unwrap().recounted =
            Some((ended, Duration::from_secs(1)));
        let dirty = write(&mut worker, "second");
        worker.update(&dirty, ended + Duration::from_secs(1));
        assert_eq!(size(), 4, "held back");
        let wake = worker.next_wake(ended + Duration::from_secs(1));
        assert!(
            wake.is_some_and(|wake| wake <= Duration::from_secs(3)),
            "{wake:?}"
        );
        worker.update(&HashSet::new(), ended + Duration::from_secs(3));
        assert_eq!(size(), 4, "remembered, still held back");
        worker.update(&HashSet::new(), ended + Duration::from_secs(4));
        assert_eq!(size(), 8, "counted once rested");
        assert!(!worker.active[&PaneId(0)].changed);
    }

    #[test]
    fn fallback_intervals_scale_with_the_previous_scan_duration() {
        let shared = shared();
        let request = request(0, PathBuf::from("/synthetic/missing"));
        register_status(&shared, &request);
        let mut worker = worker(Arc::clone(&shared), None);
        let started = Instant::now();
        worker.admit(request);
        let state = worker.active.get_mut(&PaneId(0)).unwrap();
        state.last_scan = started;
        state.last_duration = Duration::from_secs(3);
        let revision = self::revision(&shared);
        worker.update(&HashSet::new(), started + REVALIDATE * 5);
        assert_eq!(
            self::revision(&shared),
            revision,
            "not due before 4x the last scan"
        );
        worker.update(&HashSet::new(), started + REVALIDATE * 6);
        assert_eq!(self::revision(&shared), revision + 1);
    }

    #[test]
    fn new_requests_preempt_a_running_fallback_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let slow = tmp.path().join("slow");
        std::fs::create_dir(&slow).unwrap();
        tree(&slow, 12, 1);
        let quick = tmp.path().join("quick");
        std::fs::create_dir(&quick).unwrap();
        std::fs::write(quick.join("file"), b"ready").unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let shared = shared();
        let mut worker = Worker::new(Arc::clone(&shared), None, Some(rx.clone()), Arc::default());
        let first = request(0, slow.clone());
        register_status(&shared, &first);
        worker.admit(first);
        assert_eq!(shared.statuses.lock()[&PaneId(0)].dirs, 24);
        let second = request(1, quick.clone());
        register_status(&shared, &second);
        tx.send(Message::Scan(second)).unwrap();
        // The due fallback rescan of the slow tree yields at its first checkpoint.
        worker.update(&HashSet::new(), Instant::now() + REVALIDATE);
        assert!(worker.active[&PaneId(0)].pending);
        assert_eq!(worker.deferred.len(), 1);
        let interrupted = shared.statuses.lock()[&PaneId(0)].clone();
        assert!(interrupted.scanning, "still counting");
        assert_eq!(interrupted.dirs, 24, "the finished chart stays visible");
        assert!(shared.statuses.lock()[&PaneId(1)].scanning);
        // The next iteration serves the new request first, then resumes.
        assert!(worker.step(&rx));
        let served = shared.statuses.lock()[&PaneId(1)].clone();
        assert!(!served.scanning);
        assert_eq!(served.size, 5);
        let resumed = shared.statuses.lock()[&PaneId(0)].clone();
        assert!(!resumed.scanning);
        assert_eq!(resumed.dirs, 24);
        assert!(!worker.active[&PaneId(0)].pending);
        assert!(worker.deferred.is_empty());
    }

    #[test]
    fn aliases_are_monitored_through_their_parent_and_re_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("file"), b"original").unwrap();
        let replacement = tmp.path().join("replacement");
        std::fs::create_dir(&replacement).unwrap();
        std::fs::write(replacement.join("file"), b"new").unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::create_dir(replacement.join("sub")).unwrap();
        let alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        // A symlink component followed by `..`: scanned by canonical path.
        let request = request(0, alias.join("sub/.."));
        let shared = shared();
        register_status(&shared, &request);
        let recording = Recording::default();
        let mut worker = worker(Arc::clone(&shared), Some(self::recording(&recording)));
        worker.admit(request);
        let state = &worker.active[&PaneId(0)];
        assert_eq!(
            state.target.path,
            skwir_core::os_path::canonical(&root).unwrap()
        );
        assert_eq!(state.target.links, vec![alias.clone()]);
        assert_eq!(state.coverage, Coverage::Full);
        let parent = tmp.path().to_path_buf();
        assert_eq!(
            state.alias_parents,
            BTreeSet::from([parent.clone()]),
            "the link's parent decides where it points"
        );
        assert_eq!(worker.registrations[&parent].alias, 1);
        let status = shared.statuses.lock()[&PaneId(0)].clone();
        assert_eq!(status.size, 8);
        assert!(!status.stale, "an alias needs no polling");
        assert_eq!(status.root.as_deref(), Some(alias.join("sub/..").as_path()));

        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&replacement, &alias).unwrap();
        let unrelated = worker.changes(Changes {
            paths: HashMap::from([(parent.join("other"), Change::Content)]),
            ..Changes::default()
        });
        assert!(
            unrelated.is_empty(),
            "sibling entries of the link are ignored"
        );
        let dirty = worker.changes(Changes {
            paths: HashMap::from([(alias.clone(), Change::Structural)]),
            ..Changes::default()
        });
        assert_eq!(dirty, HashSet::from([PaneId(0)]));
        worker.update(&dirty, Instant::now());
        assert_eq!(shared.statuses.lock()[&PaneId(0)].size, 3);
        let state = &worker.active[&PaneId(0)];
        assert_eq!(
            state.target.path,
            skwir_core::os_path::canonical(&replacement).unwrap()
        );
        assert_eq!(worker.registrations[&parent].alias, 1);
        assert!(
            worker
                .cache
                .dirs
                .contains_key(&skwir_core::os_path::canonical(&root).unwrap()),
            "the old target stays cached under its own watch"
        );
        worker.remove_active(PaneId(0));
        assert!(!worker.registrations.contains_key(&parent));
    }

    #[test]
    fn cancelled_requests_drop_their_status_but_not_a_newer_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let shared = shared();
        let mut worker = worker(Arc::clone(&shared), None);
        let first = request(0, tmp.path().into());
        register_status(&shared, &first);
        worker.admit(first);
        assert!(shared.statuses.lock().contains_key(&PaneId(0)));
        // Navigation or pane close cancel the location lifetime.
        worker.active[&PaneId(0)].request.cancel.cancel();
        let mut newer = request(0, tmp.path().into());
        newer.generation = 2;
        register_status(&shared, &newer);
        worker.update(&HashSet::new(), Instant::now());
        assert!(worker.active.is_empty());
        assert_eq!(
            shared.statuses.lock()[&PaneId(0)].generation,
            2,
            "a replacement request keeps its status"
        );
        newer.cancel.cancel();
        worker.admit(newer);
        assert!(!shared.statuses.lock().contains_key(&PaneId(0)));
    }

    #[test]
    fn other_filesystems_and_virtual_trees_are_leaves() {
        let request = request(0, PathBuf::from("/proc"));
        let shared = shared();
        register_status(&shared, &request);
        let mut worker = worker(Arc::clone(&shared), None);
        worker.admit(request);
        let status = shared.statuses.lock()[&PaneId(0)].clone();
        assert!(status.tree.is_none());
        assert_eq!(
            status.error.map(|error| error.english()).as_deref(),
            Some("Disk usage is not available for virtual filesystems")
        );
        assert!(!descend(
            Path::new("/sys/kernel"),
            &std::fs::metadata("/").unwrap(),
            meta::device(&std::fs::metadata("/").unwrap())
        ));
        assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
    }

    use skwir_core::coverage::{Hole, Why};
    use skwir_core::names::{self, Journal, Mark, Noted, Since};

    /// A stand-in names index: it sums the folders it is given, names the
    /// paths it is given, and tells the journal it is given, else that
    /// nothing changed; its totals say the holes below the folder they sum,
    /// as skwird's do.
    #[derive(Clone, Default)]
    struct Summing(Arc<Mutex<Summed>>);

    #[derive(Default)]
    struct Summed {
        root: PathBuf,
        holes: Vec<Hole>,
        totals: HashMap<PathBuf, (Vec<skwir_core::names::Child>, skwir_core::names::Totals)>,
        named: Vec<PathBuf>,
        /// What its journal tells when next asked; else that nothing
        /// changed.
        journal: Option<Since>,
        /// The tokens its journal was asked from, in order.
        since: Vec<String>,
        /// The folders whose entries were asked, in order.
        asked: Vec<PathBuf>,
        /// The journal's token its totals are as of.
        token: String,
        /// Whether some file's count of names is not known.
        links_unknown: bool,
    }

    fn live() -> skwir_core::coverage::Proof {
        skwir_core::coverage::Proof::Live {
            since: std::time::SystemTime::UNIX_EPOCH,
        }
    }

    fn mark(token: &str) -> Mark {
        Mark {
            by: "summing".into(),
            token: token.into(),
        }
    }

    /// A journal telling `changes`, every change, from `token` on.
    fn told(changes: Vec<names::Change>, token: &str, holes: Vec<Hole>) -> Since {
        Since::Journal(Journal {
            changes,
            mark: mark(token),
            names: true,
            attrs: true,
            holes,
        })
    }

    fn noted(path: PathBuf, kind: EntryKind) -> Noted {
        Noted {
            path,
            kind,
            id: None,
        }
    }

    impl skwir_core::names::NameIndex for Summing {
        fn id(&self) -> &'static str {
            "summing"
        }
        fn by(&self) -> Words {
            Words::as_written("summing")
        }
        fn syncs(&self) -> bool {
            true
        }
        fn open(&self, _: &Path, _: &CancelToken) -> Option<skwir_core::names::Opened> {
            None
        }
        fn follow(&self, root: &Path, _: &CancelToken) -> Option<skwir_core::names::Opened> {
            let summed = self.0.lock();
            root.starts_with(&summed.root)
                .then(|| skwir_core::names::Opened {
                    coverage: skwir_core::coverage::Coverage {
                        names: live(),
                        attrs: live(),
                        holes: summed.holes.clone(),
                    },
                    session: Box::new(self.clone()),
                })
        }
    }

    impl skwir_core::names::Session for Summing {
        fn sync(&mut self) -> Option<skwir_core::coverage::Coverage> {
            None
        }
        fn query(
            &mut self,
            q: &skwir_core::names::Query,
            each: &mut dyn FnMut(Vec<PathBuf>) -> bool,
        ) -> Option<skwir_core::names::Answer> {
            let named: Vec<PathBuf> = self
                .0
                .lock()
                .named
                .iter()
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().contains(q.names[0].text.as_str()))
                })
                .cloned()
                .collect();
            each(named);
            None
        }
        fn changes(&mut self, since: Option<&Mark>) -> Option<Since> {
            let mut s = self.0.lock();
            s.since
                .push(since.map_or_else(String::new, |m| m.token.clone()));
            let told = s.journal.take();
            Some(told.unwrap_or_else(|| told_nothing(&s)))
        }
        fn totals(
            &mut self,
            folder: &Path,
            largest: Option<skwir_core::names::Largest>,
            each: &mut dyn FnMut(Vec<skwir_core::names::Child>) -> bool,
        ) -> Option<skwir_core::names::Summed> {
            let mut summed = self.0.lock();
            let sent = largest.is_none_or(|l| l.alloc > 0 || l.files > 0);
            if sent {
                summed.asked.push(folder.to_path_buf());
            }
            let (children, totals) = summed.totals.get(folder)?.clone();
            if sent {
                each(children);
            }
            Some(skwir_core::names::Summed {
                totals,
                mtime: None,
                mark: mark(&summed.token),
                coverage: skwir_core::coverage::Coverage {
                    names: live(),
                    attrs: live(),
                    holes: summed
                        .holes
                        .iter()
                        .filter(|h| h.path.starts_with(folder) && h.path != folder)
                        .cloned()
                        .collect(),
                },
                links_known: !summed.links_unknown,
                count: 0,
                rest: None,
            })
        }
    }

    /// That nothing changed since, as of its token now.
    fn told_nothing(s: &Summed) -> Since {
        told(Vec::new(), &s.token, s.holes.clone())
    }

    fn totals(alloc: u64, files: u64, folders: u64) -> skwir_core::names::Totals {
        skwir_core::names::Totals {
            size: alloc,
            alloc,
            files,
            names: files,
            folders,
            newest_file: Some(std::time::SystemTime::UNIX_EPOCH + Duration::from_hours(500_000)),
            by_extension: Vec::new(),
            shared: 0,
        }
    }

    fn entry(path: PathBuf, dir: bool, t: skwir_core::names::Totals) -> skwir_core::names::Child {
        skwir_core::names::Child {
            path,
            kind: if dir {
                EntryKind::Directory
            } else {
                EntryKind::File
            },
            id: Some((1, 2)),
            mtime: None,
            totals: t,
        }
    }

    const SIGNED: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n";

    /// What a test of a measure from an index holds: the folder measured
    /// and the folder above it, outside the index; the stand-in index
    /// summing it; a window looked at, and a worker measuring.
    struct Indexing {
        _tmp: tempfile::TempDir,
        _state: tempfile::TempDir,
        root: PathBuf,
        above: PathBuf,
        index: Summing,
        app: Arc<skwir_core::app::App>,
        shared: Arc<Shared>,
        worker: Worker,
        /// The worker's time, moved on a `REVALIDATE` a round.
        clock: Instant,
    }

    impl Indexing {
        /// On disk: `big/part` (empty), `big/x.bin` (1000 bytes, a name
        /// above too), `kept out/secret` (5000), `cached` and
        /// `cached/inner` (caches, each by its tag) and
        /// `cached/private/note` (3000). The index keeps `kept out` and
        /// `cached/private` out, and says what the disk does not hold
        /// elsewhere, so what is read from where shows.
        fn new(recording: &Recording) -> Indexing {
            let tmp = tempfile::tempdir().unwrap();
            let above = std::fs::canonicalize(tmp.path()).unwrap();
            let root = above.join("measured");
            for dir in ["big/part", "kept out", "cached/inner", "cached/private"] {
                std::fs::create_dir_all(root.join(dir)).unwrap();
            }
            std::fs::write(root.join("big/x.bin"), vec![b'b'; 1000]).unwrap();
            std::fs::hard_link(root.join("big/x.bin"), above.join("x again.bin")).unwrap();
            std::fs::write(root.join("kept out/secret"), vec![b'x'; 5000]).unwrap();
            std::fs::write(root.join("cached/private/note"), vec![b'p'; 3000]).unwrap();
            for cache in ["cached", "cached/inner"] {
                std::fs::write(root.join(cache).join(marks::CACHEDIR_TAG), SIGNED).unwrap();
            }
            let index = Summing::default();
            {
                let mut s = index.0.lock();
                s.root.clone_from(&root);
                s.holes = vec![
                    Hole {
                        path: root.join("kept out"),
                        why: Why::Excluded,
                    },
                    Hole {
                        path: root.join("cached/private"),
                        why: Why::Private,
                    },
                ];
                s.totals.insert(
                    root.clone(),
                    (
                        vec![
                            entry(root.join("big"), true, totals(1_000_000, 10, 1)),
                            entry(root.join("kept out"), true, totals(0, 0, 0)),
                            entry(root.join("cached"), true, totals(50_000, 3, 2)),
                        ],
                        totals(1_050_000, 13, 6),
                    ),
                );
                let mut linked = totals(100_000, 1, 0);
                linked.shared = 100_000;
                s.totals.insert(
                    root.join("big"),
                    (
                        vec![
                            entry(root.join("big/part"), true, totals(900_000, 9, 0)),
                            entry(root.join("big/x.bin"), false, linked),
                        ],
                        totals(1_000_000, 10, 1),
                    ),
                );
                s.totals.insert(
                    root.join("cached"),
                    (
                        vec![entry(root.join("cached/inner"), true, totals(20_000, 1, 0))],
                        totals(50_000, 3, 2),
                    ),
                );
                s.totals.insert(
                    root.join("cached/inner"),
                    (Vec::new(), totals(20_000, 1, 0)),
                );
                s.named = vec![
                    root.join("cached").join(marks::CACHEDIR_TAG),
                    root.join("cached/inner").join(marks::CACHEDIR_TAG),
                ];
            }
            let state = tempfile::tempdir().unwrap();
            let app =
                skwir_core::app::App::new(skwir_core::app::AppConfig::ephemeral(state.path()))
                    .unwrap();
            app.names.register("test", Arc::new(index.clone()));
            app.set_window_focused(skwir_core::workspace::WindowId(1), true);
            let shared = Arc::new(Shared {
                statuses: Mutex::default(),
                revision: Arc::default(),
                wake: Arc::new(|| {}),
                app: app.weak(),
            });
            let worker = worker(Arc::clone(&shared), Some(self::recording(recording)));
            Indexing {
                _tmp: tmp,
                _state: state,
                root,
                above,
                index,
                app,
                shared,
                worker,
                clock: Instant::now(),
            }
        }

        /// Pane `pane` asked to show `path`, as its `generation`th request.
        fn measure(&mut self, pane: u32, path: &Path, generation: u64) -> Status {
            let mut request = request(pane, path.to_path_buf());
            request.generation = generation;
            register_status(&self.shared, &request);
            self.worker.admit(request);
            self.status(pane)
        }

        fn status(&self, pane: u32) -> Status {
            self.shared.statuses.lock()[&PaneId(pane)].clone()
        }

        /// A round of the journals a `REVALIDATE` on, and the counts it
        /// asked for: the panes counted again.
        fn round(&mut self) -> HashSet<PaneId> {
            self.clock += REVALIDATE;
            let dirty = self.worker.journals(self.clock);
            self.worker.update(&dirty, self.clock);
            dirty
        }

        /// The journal tells `changes` when next asked, from `token` on.
        fn tell(&self, changes: Vec<names::Change>, token: &str) {
            let mut s = self.index.0.lock();
            let holes = s.holes.clone();
            s.journal = Some(told(changes, token, holes));
        }

        /// The index's totals of `folder`'s entry `name` and of the
        /// folders above it to the root, `by` bytes more.
        fn grow(&self, folder: &str, name: &str, by: u64) {
            let mut s = self.index.0.lock();
            let mut at = self.root.join(folder);
            let mut child = at.join(name);
            loop {
                if let Some((children, own)) = s.totals.get_mut(&at) {
                    if let Some(c) = children.iter_mut().find(|c| c.path == child) {
                        c.totals.size += by;
                        c.totals.alloc += by;
                    }
                    own.size += by;
                    own.alloc += by;
                }
                if at == self.root {
                    break;
                }
                child.clone_from(&at);
                at = at.parent().unwrap().to_path_buf();
            }
        }
    }

    fn named<'a>(node: &'a Node, name: &str) -> &'a Node {
        node.children
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("{name} among {:?}", node.children))
    }

    /// A folder a names index sums with its sizes live is measured from it
    /// (ADR-150, D3), the disk read only where the index counts nothing: a
    /// folder it keeps out scanned and added, a cache's room asked of it by
    /// name, the folders the map opens summed too; what its journal says
    /// changed is summed again, and its changes are asked from where it
    /// stood before, never from where a breakdown summed after it stood.
    /// A folder above the one measured, gone, as an index tells it since
    /// skwird 0.2: the measure goes with it, read again whole; a folder
    /// above it that only changed keeps it.
    #[test]
    fn a_folder_above_one_measured_gone_takes_its_measure_with_it() {
        let mut t = Indexing::new(&Recording::default());
        let root = t.root.clone();
        t.measure(0, &root, 1);
        let above = root.parent().expect("a folder above").to_path_buf();
        t.tell(
            vec![names::Change::Changed(noted(
                above.clone(),
                EntryKind::Directory,
            ))],
            "m2",
        );
        assert!(t.round().is_empty(), "a folder above only changed");
        t.index.0.lock().asked.clear();
        t.tell(
            vec![names::Change::Removed(noted(above, EntryKind::Directory))],
            "m3",
        );
        assert_eq!(t.round(), HashSet::from([PaneId(0)]));
        assert!(
            t.index.0.lock().asked.contains(&root),
            "measured again whole"
        );
    }

    #[test]
    fn a_folder_an_index_sums_is_measured_from_it() {
        let mut t = Indexing::new(&Recording::default());
        let root = t.root.clone();
        let status = t.measure(0, &root, 1);
        let tree = status.tree.clone().expect("measured");
        // From the index, not the disk (big holds 1000 bytes there).
        assert_eq!(named(&tree, "big").alloc, 1_000_000);
        assert_eq!(named(&tree, "big").files, 10);
        // Kept out of the index: scanned.
        let kept_out = named(&tree, "kept out");
        assert_eq!((kept_out.size, kept_out.files), (5000, 1));
        assert_eq!(tree.size, 1_050_000 + 5000 + 3000);
        assert_eq!(tree.files, 15);
        // The cache, said so by its tag, its room asked of the index.
        assert_eq!(named(&tree, "cached").reclaim, Some(marks::Reclaim::Cache));
        // Only what the index does not count was read.
        assert_eq!(status.scanned_dirs, 2, "kept out and private alone");
        assert!(!status.stale, "kept current by the journal");
        // The map opens big: summed by the index too, and drawn inside.
        let big = named(&tree, "big");
        assert_eq!(named(big, "part").alloc, 900_000);
        assert!(t.worker.cache.journals.contains_key(&root), "its mark kept");

        // The journal says big/x.bin changed: big summed again, the rest kept.
        t.tell(
            vec![names::Change::Changed(noted(
                root.join("big/x.bin"),
                EntryKind::File,
            ))],
            "m2",
        );
        t.grow("big", "x.bin", 200_000);
        {
            let mut s = t.index.0.lock();
            s.asked.clear();
            s.token = "m3".into();
        }
        assert_eq!(t.round(), HashSet::from([PaneId(0)]));
        let tree = t.status(0).tree.unwrap();
        assert_eq!(tree.size, 1_250_000 + 8000);
        let asked = t.index.0.lock().asked.clone();
        assert!(asked.contains(&root) && asked.contains(&root.join("big")));
        assert!(
            !asked.contains(&root.join("cached")),
            "its breakdown kept: {asked:?}"
        );
        assert_eq!(
            t.worker.cache.journals[&root].mark.token, "m2",
            "asked from where it stood, not from the totals summed since: \
             what changed meanwhile in what was kept is told"
        );

        // A journal that is not every change: whole again.
        t.index.0.lock().journal = Some(Since::TooOld);
        assert_eq!(t.round(), HashSet::from([PaneId(0)]));
        assert_eq!(t.index.0.lock().since, ["", "m2"]);
        assert_eq!(
            t.worker.cache.journals[&root].mark.token, "m3",
            "followed again from the totals summed anew"
        );
    }

    /// A measure from an index says of names and caches what the scan of
    /// the disk does: a file of several names how many, a cache inside a
    /// cache what it holds, and what is scanned inside a cache room given
    /// back with it.
    #[test]
    fn an_index_measure_says_what_the_scan_says_of_names_and_caches() {
        let mut t = Indexing::new(&Recording::default());
        let root = t.root.clone();
        let tree = t.measure(0, &root, 1).tree.expect("measured");
        assert_eq!(
            named(named(&tree, "big"), "x.bin").nlink,
            2,
            "one of its two names"
        );
        let cached = named(&tree, "cached");
        let private = named(cached, "private");
        assert_eq!(private.size, 3000, "scanned");
        assert_eq!(cached.alloc, 50_000 + private.alloc);
        assert_eq!(cached.reclaimable, cached.alloc, "a cache whole");
        assert_eq!(tree.reclaimable, cached.alloc);
        let inner = named(cached, "inner");
        assert_eq!(inner.reclaim, Some(marks::Reclaim::Cache));
        assert_eq!(inner.reclaimable, 20_000);
        let kept = t.worker.cache.peek(&root.join("cached")).unwrap();
        assert_eq!(kept.reclaimable, kept.alloc);
        assert_eq!(kept.alloc, 50_000 + private.alloc);
    }

    /// A breakdown taken from an index stays current wherever it is shown
    /// again: drilled into, by a second pane, shown again after a while,
    /// told by the journal it was kept with; a scan of the disk above the
    /// index never counts with one.
    #[test]
    fn a_breakdown_from_an_index_is_kept_current_wherever_it_is_shown() {
        let mut t = Indexing::new(&Recording::default());
        let root = t.root.clone();
        t.measure(0, &root, 1);
        // Drilled into: big's breakdown, kept from the index's, shown at once.
        t.index.0.lock().asked.clear();
        let big = t.measure(0, &root.join("big"), 2);
        assert_eq!(big.alloc, 1_000_000);
        assert!(
            !t.index.0.lock().asked.contains(&root.join("big")),
            "kept, not summed again"
        );
        // A second pane shows the folder measured, kept too.
        t.measure(1, &root, 3);
        // big/part gains a file: both panes counted again, the drilled one too.
        t.tell(
            vec![names::Change::Added(noted(
                root.join("big/part/new.bin"),
                EntryKind::File,
            ))],
            "m2",
        );
        t.grow("big", "part", 50_000);
        assert_eq!(t.round(), HashSet::from([PaneId(0), PaneId(1)]));
        assert_eq!(t.status(0).alloc, 1_050_000);
        assert_eq!(t.status(1).size, 1_100_000 + 8000);

        // Let go of, then shown again: what changed meanwhile is told first.
        t.worker.remove_active(PaneId(0));
        t.worker.remove_active(PaneId(1));
        t.tell(
            vec![names::Change::Changed(noted(
                root.join("big/x.bin"),
                EntryKind::File,
            ))],
            "m4",
        );
        t.grow("big", "x.bin", 50_000);
        let again = t.measure(0, &root, 4);
        assert_eq!(again.size, 1_150_000 + 8000, "not what was kept before");

        // Measured from above, outside the index: the disk read below it,
        // a file of two names counted once where they meet.
        let above = t.above.clone();
        let status = t.measure(2, &above, 5);
        let tree = status.tree.expect("measured");
        let on_disk = 1000 + 5000 + 3000 + 2 * SIGNED.len() as u64;
        assert_eq!(named(&tree, "measured").size, on_disk);
        assert_eq!(tree.size, on_disk);
    }

    /// The journals are asked on a slow pace only while someone looks; a
    /// folder they cover no more, though no change says so, is counted
    /// again whole; an index that does not know every file's count of
    /// names is not measured from.
    #[test]
    fn journals_are_asked_while_someone_looks_and_say_what_they_cover() {
        let mut t = Indexing::new(&Recording::default());
        let root = t.root.clone();
        t.measure(0, &root, 1);
        let window = skwir_core::workspace::WindowId(1);
        t.app.set_window_focused(window, false);
        assert!(t.round().is_empty());
        assert!(t.index.0.lock().since.is_empty(), "not asked");
        t.app.set_window_focused(window, true);
        assert!(t.round().is_empty(), "nothing changed");
        assert_eq!(t.index.0.lock().since.len(), 1);

        // big/part kept out of the index now, no change said: scanned
        // (it is empty on disk), and big counted again.
        {
            let mut s = t.index.0.lock();
            s.holes.push(Hole {
                path: root.join("big/part"),
                why: Why::Private,
            });
        }
        assert_eq!(t.round(), HashSet::from([PaneId(0)]));
        let big = t
            .worker
            .cache
            .peek(&root.join("big"))
            .expect("summed again");
        assert_eq!(named(big, "part").alloc, 0);

        // Some file's count of names not known: the disk is read.
        t.index.0.lock().links_unknown = true;
        let mut force = request(0, root.clone());
        force.generation = 2;
        force.force = true;
        register_status(&t.shared, &force);
        t.worker.admit(force);
        let status = t.status(0);
        assert!(status.scanned_dirs >= 6, "{}", status.scanned_dirs);
        assert_eq!(status.size, 1000 + 5000 + 3000 + 2 * SIGNED.len() as u64);
        assert!(!t.worker.cache.journaled_below(&root));
    }

    /// A journal followed inside another is that one's to tell once both
    /// were asked: one connection a round for what they kept.
    #[test]
    fn a_journal_inside_another_is_the_outer_ones_once_both_are_asked() {
        let mut t = Indexing::new(&Recording::default());
        let root = t.root.clone();
        t.measure(0, &root.join("big"), 1);
        t.measure(1, &root, 2);
        let followed = |t: &Indexing| t.worker.cache.journals.keys().cloned().collect::<Vec<_>>();
        assert_eq!(followed(&t), [root.clone(), root.join("big")]);
        assert!(t.round().is_empty());
        assert_eq!(followed(&t), [root.as_path()]);
        // Asked as the second pane came, then both, the inner first.
        assert_eq!(t.index.0.lock().since.len(), 3);
    }

    /// A breakdown summed by the index whose scanned folders are not all
    /// watched is not kept, as a scan's own is not.
    #[test]
    fn a_breakdown_with_a_folder_scanned_unwatched_is_not_kept() {
        let recording = Recording {
            refuse: Some("private"),
            ..Recording::default()
        };
        let mut t = Indexing::new(&recording);
        let root = t.root.clone();
        let status = t.measure(0, &root, 1);
        assert!(status.stale, "partly watched");
        assert!(t.worker.cache.peek(&root.join("cached")).is_none());
        assert!(t.worker.cache.peek(&root.join("big")).is_some());
    }

    /// Without an index, or where its sizes are not live, the disk is read
    /// as before.
    #[test]
    fn without_an_index_the_disk_is_read() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        std::fs::write(root.join("file"), b"four").unwrap();
        let state = tempfile::tempdir().unwrap();
        let app =
            skwir_core::app::App::new(skwir_core::app::AppConfig::ephemeral(state.path())).unwrap();
        let shared = Arc::new(Shared {
            statuses: Mutex::default(),
            revision: Arc::default(),
            wake: Arc::new(|| {}),
            app: app.weak(),
        });
        let recording = Recording::default();
        let mut worker = worker(Arc::clone(&shared), Some(self::recording(&recording)));
        let request = request(0, root.clone());
        register_status(&shared, &request);
        worker.admit(request);
        let status = shared.statuses.lock()[&PaneId(0)].clone();
        assert_eq!(status.size, 4);
        assert_eq!(status.scanned_dirs, 1);
        assert!(worker.cache.journals.is_empty());
    }
}
