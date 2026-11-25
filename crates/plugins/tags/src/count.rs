//! The count behind tag locations (`location.rs`): every tagged file under a folder,
//! hidden ones included, outside what a walk leaves out below it (the
//! folders of the list, a cache that says so, a trash, a network mount:
//! `skwir_core::walks`, ADR-077, ADR-125), learned by the index
//! (`index::TagIndex`), which then covers the folder as of the time the
//! count began.
//!
//! A first count reads every file's notes. A later one, the folder covered,
//! checks: it reads again only what changed since. Setting a tag changes
//! the file's change time (an attribute is part of the inode; a name tag is
//! a rename), and a store beside the file (an XMP sidecar, a Mac's `._x`,
//! `TagSpaces`' `.ts/`) changes its own, read as its file's. Two things
//! keep their old times when they arrive, so they are read whole: a folder
//! the last count did not see where it is now (new, or moved in: it is
//! known by its device, inode and path), and a folder on a filesystem
//! whose change times cannot be trusted for this (a network or FUSE one,
//! `filesystem::change_times_tell`). A tagged file the index knows that is no longer there is
//! forgotten. Symbolic links are not followed: a file is counted where it
//! is.
//!
//! Where a names index follows the folder and its journal since the last
//! count is every change, of names and of change times alike (skwird's,
//! where the folder has been live all along: ADR-150, D4), a check reads
//! only the folders it names ([`run_followed`]): those holding what was
//! made, removed or changed, alone; a folder made or moved in, what it
//! says it does not cover, whole. Elsewhere, and where the journal cannot
//! go back that far, the check walks.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::{Condvar, Mutex};

use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::{EntryId, EntryKind, FileId};
use skwir_core::fs::reach;
use skwir_core::names::{Change, Journal, Mark, Since};
use skwir_core::walks::{Below, LeftOut};

use crate::index::{Covered, TagIndex, stable_hash};

/// Taken off the time a count begins, for clocks that disagree a little
/// and a change landing in the same instant.
const MARGIN: Duration = Duration::from_secs(2);

/// How many files' notes are read before the index learns them.
const LEARN_EVERY: usize = 2000;

/// What a count found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counted {
    /// Folders walked.
    pub folders: usize,
    /// Files whose notes were read.
    pub read: usize,
    /// Folders below it left out: the list's, caches, mounts.
    pub left_out: usize,
}

/// A folder as the next count recognizes it.
fn folder_key(dev: u64, ino: u64, path: &Path) -> u64 {
    stable_hash(&[
        &dev.to_le_bytes(),
        &ino.to_le_bytes(),
        path.as_os_str().as_encoded_bytes(),
    ])
}

fn changed_since(path: &Path, meta: &std::fs::Metadata, since: Option<SystemTime>) -> bool {
    let Some(since) = since else { return true };
    skwir_core::fs::stat::changed(path, meta).is_none_or(|changed| changed >= since)
}

/// Count the tags under `root`. With `covered` (the time the last count
/// began), only what changed since is read; without, every file. Returns
/// what to cover the folder as of, with the folders seen, unless it was
/// cancelled.
///
/// # Errors
/// When `root` cannot be read.
pub fn run(
    app: &App,
    index: &Arc<TagIndex>,
    root: &Path,
    covered: Option<SystemTime>,
    cancel: &CancelToken,
) -> anyhow::Result<Option<(SystemTime, Vec<u64>, Counted)>> {
    run_ruled(app, index, root, &app.left_out(), covered, cancel)
}

/// [`run`] under `rule`, what walks leave out (`App::left_out`), read once
/// by who covers the folder with what it returns: a count is of the rule
/// it walked under.
///
/// # Errors
/// When `root` cannot be read.
pub fn run_ruled(
    app: &App,
    index: &Arc<TagIndex>,
    root: &Path,
    rule: &Arc<LeftOut>,
    covered: Option<SystemTime>,
    cancel: &CancelToken,
) -> anyhow::Result<Option<(SystemTime, Vec<u64>, Counted)>> {
    // The folders the count it relies on saw: a folder above's, when that
    // one covers it, which never walked a folder it left out below it.
    // What `root`'s own count saw is not what the index holds since.
    let dirs = covered
        .and_then(|_| index.covering(root))
        .map(|c| index.dirs(&c.root))
        .unwrap_or_default();
    run_with(
        app,
        index,
        root,
        rule,
        covered.map(|since| (since, &dirs)),
        cancel,
    )
}

/// [`run_ruled`], given the folders the last count saw.
///
/// # Errors
/// When `root` cannot be read.
pub fn run_with(
    app: &App,
    index: &Arc<TagIndex>,
    root: &Path,
    rule: &Arc<LeftOut>,
    last: Option<(SystemTime, &HashSet<u64>)>,
    cancel: &CancelToken,
) -> anyhow::Result<Option<(SystemTime, Vec<u64>, Counted)>> {
    std::fs::read_dir(root)?;
    // A remote's tree is read on this job's thread alone, in its lane, a
    // folder at a time as a listing reads one (ADR-077): threads of the
    // count's own would ask its server as many at once as this machine
    // has cores, outside the lane, and a server that stops answering
    // would hold threads no one stands in for.
    let remote = reach::of(root).is_some()
        || std::fs::metadata(root)
            .ok()
            .and_then(|meta| skwir_core::fs::stat::identity(root, &meta))
            .is_some_and(|id| reach::of_device(id.dev).is_some());
    let walkers = if remote { 1 } else { walkers() };
    // A cache's tag is never read on a remote: a lookup per folder there
    // asks its server (docs/invariants/remote.md).
    let below = rule.below(root, !remote);
    let seeds = vec![(root.to_path_buf(), true)];
    Ok(walk_with(app, index, &below, last, cancel, walkers, seeds))
}

/// A names index's journal of a folder, followed on what already runs
/// (`NameIndexes::follow`, nothing started): what changed since the mark
/// its last count kept, where that is every change, and the mark the next
/// count asks from, taken before this one reads anything.
pub struct Followed {
    pub journal: Option<Journal>,
    pub mark: Mark,
}

/// Follow `root`'s journal from `last`'s mark (none: from now). `None`
/// where no index follows it, or none answers.
pub fn follow(
    app: &App,
    root: &Path,
    last: Option<&Covered>,
    cancel: &CancelToken,
) -> Option<Followed> {
    // A mark is of the folder its count covered: one above's is not its.
    let kept = last
        .filter(|c| c.root == root)
        .and_then(|c| c.mark.as_ref());
    let (index, opened) = app
        .names
        .follow(root, kept.map(|m| m.by.as_str()), cancel)?;
    let mut session = opened.session;
    if let Some(mark) = kept.filter(|m| m.by == index.id())
        && let Some(Since::Journal(journal)) = session.changes(Some(mark))
    {
        let mark = journal.mark.clone();
        return Some(Followed {
            journal: journal.complete().then_some(journal),
            mark,
        });
    }
    // None kept, too old, or failed: a mark from now, before the walk.
    match session.changes(None)? {
        Since::Journal(journal) => Some(Followed {
            journal: None,
            mark: journal.mark,
        }),
        Since::TooOld => None,
    }
}

/// A check of `root` under `rule` read from `journal` (every change since
/// `last`, its count's): the folders it names, a folder holding what
/// changed alone, one made or moved in and what it does not cover whole,
/// each as a walk reads it, and the folder holding each entry named, where
/// its own notes are read. Returns what [`run`] does: the folders seen are
/// the last count's, those read added, those removed taken off; `None`
/// only when cancelled.
///
/// # Errors
/// When `root`'s real path cannot be read.
pub fn run_followed(
    app: &App,
    index: &Arc<TagIndex>,
    root: &Path,
    rule: &Arc<LeftOut>,
    last: &Covered,
    journal: &Journal,
    cancel: &CancelToken,
) -> anyhow::Result<Option<(SystemTime, Vec<u64>, Counted)>> {
    let real_root = skwir_core::os_path::canonical(root)?;
    let below = rule.below(root, reach::of(root).is_none());
    let known = index.dirs(root);
    let (seeds, removed) = seeds_of(app, root, &real_root, &below, journal);
    let Some((began, seen, counted)) = walk_with(
        app,
        index,
        &below,
        Some((last.since(), &known)),
        cancel,
        walkers(),
        seeds,
    ) else {
        return Ok(None);
    };
    let mut dirs: HashSet<u64> = known;
    dirs.extend(seen);
    for key in removed {
        dirs.remove(&key);
    }
    Ok(Some((began, dirs.into_iter().collect(), counted)))
}

/// The folders a journal says to read, below `root` as the walk names
/// them (the journal names real paths), each with whether it is read with
/// what it holds; and the keys of the folders it says were removed.
fn seeds_of(
    app: &App,
    root: &Path,
    real_root: &Path,
    below: &Below,
    journal: &Journal,
) -> (Vec<(PathBuf, bool)>, Vec<u64>) {
    let walked = |real: &Path| {
        real.strip_prefix(real_root).ok().map(|r| {
            if r.as_os_str().is_empty() {
                root.to_path_buf()
            } else {
                root.join(r)
            }
        })
    };
    // The folder whose files a change speaks of: a companions' folder
    // (`.ts/`) is its folder's.
    let of_files = |mut dir: PathBuf| {
        while dir.file_name().is_some_and(|n| app.companions.is_folder(n)) && dir != root {
            dir.pop();
        }
        dir
    };
    let mut seeds: Vec<(PathBuf, bool)> = Vec::new();
    let mut removed = Vec::new();
    // An entry's own tags are read in the folder holding it, as the walk
    // reads them (`Walk::folder` reads its folders' with its files): every
    // entry named, a folder's too, reads the folder it is in. A tag set on
    // a folder moves its change time alone, and the journal says nothing
    // of its parent.
    let holding = |path: &Path, seeds: &mut Vec<(PathBuf, bool)>| {
        seeds.extend(path.parent().map(|p| (of_files(p.to_path_buf()), false)));
    };
    // The root itself, or a folder above it, came, went or is covered no
    // more (an index tells it since skwird 0.2): the root is read again
    // whole, which a root gone reads as holding nothing. A folder above it
    // that only changed changes nothing below it.
    let takes_the_root = |change: &Change| match change {
        Change::Added(n) | Change::Removed(n) => real_root.starts_with(&n.path),
        Change::NotCovered(hole) => real_root.starts_with(&hole.path),
        Change::Changed(_) => false,
    };
    for change in &journal.changes {
        if takes_the_root(change) {
            seeds.push((root.to_path_buf(), true));
            continue;
        }
        let (path, deep) = match change {
            Change::Added(n) | Change::Removed(n) | Change::Changed(n) => {
                let Some(path) = walked(&n.path) else {
                    continue;
                };
                match change {
                    // A folder made or moved in: what it holds, whole.
                    Change::Added(_) if n.kind == EntryKind::Directory => {
                        seeds.push((path.clone(), true));
                    }
                    Change::Removed(_) if n.kind == EntryKind::Directory => {
                        if let Some((dev, ino)) = n.id {
                            removed.push(folder_key(dev, ino, &path));
                        }
                    }
                    // A folder changed: what it holds that changed, too.
                    Change::Changed(_) if n.kind == EntryKind::Directory => {
                        seeds.push((of_files(path.clone()), false));
                    }
                    _ => {}
                }
                (path, None)
            }
            Change::NotCovered(hole) => {
                let Some(path) = walked(&hole.path) else {
                    continue;
                };
                (path, Some(true))
            }
        };
        holding(&path, &mut seeds);
        if let Some(deep) = deep {
            seeds.push((path, deep));
        }
    }
    // What it does not cover, whole, and the folder holding each.
    for path in journal.holes.iter().filter_map(|h| walked(&h.path)) {
        holding(&path, &mut seeds);
        seeds.push((path, true));
    }
    // Each once, below the root, nothing the walk leaves out; one inside a
    // folder read with what it holds read with it.
    seeds.retain(|(path, _)| {
        path.starts_with(root)
            && !below.leaves_out(path)
            && below.stops_above(path).is_none()
            && !reach::is_point(path)
    });
    (nearest_kept(seeds), removed)
}

/// `seeds` each once, none inside a folder read with what it holds: in
/// path order, which puts a folder before everything below it, a stack of
/// the folders kept above the one considered decides it, so that each is
/// pushed and popped once (a check naming fifty thousand folders compares
/// each with its own folders above, never with all those kept before).
fn nearest_kept(mut seeds: Vec<(PathBuf, bool)>) -> Vec<(PathBuf, bool)> {
    seeds.sort_by(|(a, deep_a), (b, deep_b)| a.cmp(b).then(deep_b.cmp(deep_a)));
    let mut kept: Vec<(PathBuf, bool)> = Vec::new();
    // Indexes into `kept`: the folders kept above the last one considered,
    // nearest last.
    let mut above: Vec<usize> = Vec::new();
    for (path, deep) in seeds {
        while above
            .last()
            .is_some_and(|&at| !path.starts_with(&kept[at].0))
        {
            above.pop();
        }
        let inside = above.last().is_some_and(|&at| {
            let (folder, folder_deep) = &kept[at];
            *folder_deep || *folder == path
        });
        if !inside {
            above.push(kept.len());
            kept.push((path, deep));
        }
    }
    kept
}

/// [`run_with`], read by `walkers` threads, this one among them, from
/// `seeds`: each folder with whether what it holds is read too.
fn walk_with(
    app: &App,
    index: &Arc<TagIndex>,
    below: &Below,
    last: Option<(SystemTime, &HashSet<u64>)>,
    cancel: &CancelToken,
    walkers: usize,
    seeds: Vec<(PathBuf, bool)>,
) -> Option<(SystemTime, Vec<u64>, Counted)> {
    let root = below.root();
    let began = SystemTime::now().checked_sub(MARGIN).unwrap_or(UNIX_EPOCH);
    let empty = HashSet::new();
    let walk = Walk {
        app,
        index,
        covered: last.map(|(since, _)| since),
        known_dirs: last.map_or(&empty, |(_, dirs)| dirs),
        policy: app.notes.policy(),
        cancel,
        queue: Mutex::new((seeds, 0)),
        moved: Condvar::new(),
        devices: Mutex::new(HashMap::new()),
        // The tagged files the index knows, by folder: those no longer
        // there are forgotten; the folders not walked again are what is
        // gone.
        known: Mutex::new(index.by_folder(root)),
        seen_dirs: Mutex::new(Vec::new()),
        learned: Mutex::new(Vec::new()),
        caches: Mutex::new((Vec::new(), Vec::new())),
        below,
        folders: AtomicUsize::new(0),
        read: AtomicUsize::new(0),
        left_out: AtomicUsize::new(0),
    };
    // Folders are read side by side, as a search walks: a check of a
    // million files took 1.3 s one folder after the other.
    std::thread::scope(|scope| {
        for _ in 1..walkers {
            scope.spawn(|| walk.work());
        }
        walk.work();
    });
    if cancel.is_cancelled() {
        return None;
    }
    let mut learned = walk.learned.into_inner();
    // What the index knew in folders no longer there (not those left out
    // below, which the walk did not read: asking costs a read for each).
    for (dir, paths) in walk.known.into_inner() {
        if !below.leaves_out(&dir)
            && below.stops_above(&dir).is_none()
            && std::fs::symlink_metadata(&dir).is_err()
        {
            learned.extend(paths.into_iter().map(|p| (p, Vec::new())));
        }
    }
    index.learn(learned);
    let (found, cleared) = walk.caches.into_inner();
    if !found.is_empty() || !cleared.is_empty() {
        index.mark_caches(found, &cleared);
    }
    let counted = Counted {
        folders: walk.folders.into_inner(),
        read: walk.read.into_inner(),
        left_out: walk.left_out.into_inner(),
    };
    Some((began, walk.seen_dirs.into_inner(), counted))
}

/// Threads a count walks with: as many as a search's walk.
fn walkers() -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(12)
}

/// A count under way, its folders shared by the threads that read them.
struct Walk<'a> {
    app: &'a App,
    index: &'a Arc<TagIndex>,
    covered: Option<SystemTime>,
    known_dirs: &'a HashSet<u64>,
    policy: skwir_core::notes::Policy,
    cancel: &'a CancelToken,
    /// The folders to read, each with whether what it holds is read too,
    /// and how many threads are reading one: once none is left and none is
    /// read, the walk is done.
    queue: Mutex<(Vec<(PathBuf, bool)>, usize)>,
    moved: Condvar,
    /// Whether each device's change times tell, asked once per device.
    devices: Mutex<HashMap<u64, bool>>,
    known: Mutex<HashMap<PathBuf, Vec<PathBuf>>>,
    seen_dirs: Mutex<Vec<u64>>,
    learned: Mutex<Vec<(PathBuf, Vec<String>)>>,
    /// The caches found, and the folders the index held as caches that no
    /// longer say so.
    caches: Mutex<(Vec<PathBuf>, Vec<PathBuf>)>,
    /// What the walk leaves out below its root.
    below: &'a Below,
    folders: AtomicUsize,
    read: AtomicUsize,
    left_out: AtomicUsize,
}

impl Walk<'_> {
    /// Read folders until none is left, or the count is cancelled.
    fn work(&self) {
        loop {
            let (dir, deep) = {
                let mut queue = self.queue.lock();
                loop {
                    if self.cancel.is_cancelled() {
                        self.moved.notify_all();
                        return;
                    }
                    if let Some(next) = queue.0.pop() {
                        queue.1 += 1;
                        break next;
                    }
                    if queue.1 == 0 {
                        self.moved.notify_all();
                        return;
                    }
                    self.moved.wait(&mut queue);
                }
            };
            let inside = self.folder(&dir);
            let mut queue = self.queue.lock();
            if deep {
                queue.0.extend(inside.into_iter().map(|dir| (dir, true)));
            }
            queue.1 -= 1;
            self.moved.notify_all();
        }
    }

    fn trusted(&self, dev: u64, dir: &Path) -> bool {
        *self
            .devices
            .lock()
            .entry(dev)
            .or_insert_with(|| skwir_platform::filesystem::change_times_tell(dir))
    }

    /// Read `dir`'s files that need it, and say the folders it holds.
    fn folder(&self, dir: &Path) -> Vec<PathBuf> {
        let mut inside = Vec::new();
        let Ok(meta) = std::fs::symlink_metadata(dir) else {
            return inside;
        };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return inside;
        };
        let entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
        let held: HashSet<OsString> = entries.iter().map(std::fs::DirEntry::file_name).collect();
        let (app, covered) = (self.app, self.covered);
        // A folder below the root holding a marker that says so (a signed
        // `CACHEDIR.TAG`, among its names) is not read, nor seen: once it
        // no longer says so, the next check reads it whole.
        let below = self.below;
        if dir != below.root() && below.probes() {
            if below.marked_among(dir, |name| held.contains(name)) {
                self.left_out.fetch_add(1, Ordering::Relaxed);
                self.caches.lock().0.push(dir.to_owned());
                return inside;
            }
            if self.index.is_cache(dir) {
                self.caches.lock().1.push(dir.to_owned());
            }
        }
        // Whether this folder holds a sibling a beside rule asks for.
        let beside = |sibling: &skwir_core::exclusions::NameMatch| match sibling.exact() {
            Some(name) => held.contains(name),
            None => held.iter().any(|name| sibling.matches(name)),
        };
        self.folders.fetch_add(1, Ordering::Relaxed);
        // Off Unix a folder is known by its path alone (ADR-035).
        let id = skwir_core::fs::stat::identity(dir, &meta).unwrap_or(FileId::new(0, 0));
        let key = folder_key(id.dev, id.ino, dir);
        self.seen_dirs.lock().push(key);
        // Read whole: on a first count, a folder not seen here before, or
        // one whose times do not tell.
        let mut whole =
            covered.is_none() || !self.known_dirs.contains(&key) || !self.trusted(id.dev, dir);
        let mut present: HashSet<OsString> = HashSet::new();
        let mut candidates: Vec<(OsString, std::fs::Metadata)> = Vec::new();
        let mut owners: HashSet<OsString> = HashSet::new();
        // Whether a folder of companions here changed since: it may hold
        // this folder's own notes (`TagSpaces`' `.ts/tsm.json`), which its
        // folder's read did not see, this folder's change time unmoved.
        let mut own_changed = false;
        let mut learned = Vec::new();
        for entry in entries {
            let name = entry.file_name();
            let path = entry.path();
            // A network mount below the folder counted is neither read nor
            // entered (ADR-077): a count of it walks it from its own
            // folder. Told before its stat, which its server answers.
            if entry.file_type().is_ok_and(|t| t.is_dir()) && reach::is_point(&path) {
                present.insert(name);
                self.left_out.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            present.insert(name.clone());
            // A companion, hidden or not (`.ts/`, `._x`, `x.jpg.xmp`), is
            // its files' or its file's, never counted itself.
            if meta.is_dir() && app.companions.is_folder(&name) {
                // A folder of companions for the whole folder: anything in
                // it changed, the folder is read whole.
                let changed = changed_since(&path, &meta, covered)
                    || std::fs::read_dir(&path).is_ok_and(|inside| {
                        inside.flatten().any(|e| {
                            e.metadata()
                                .is_ok_and(|m| changed_since(&e.path(), &m, covered))
                        })
                    });
                whole |= changed;
                own_changed |= changed && covered.is_some();
                continue;
            }
            if let Some(owner) = app.companions.owner(&name)
                && owner != name
            {
                if changed_since(&path, &meta, covered) {
                    owners.insert(owner);
                }
                continue;
            }
            if meta.is_dir() {
                // A folder left out is not entered, but its own tags are
                // read: a tagged `node_modules` is counted. One beside the
                // file of the tool that made it (`target` beside a
                // `Cargo.toml`) is known to the index as one marked, so
                // what a pane taught it of what it holds is left out too.
                if below.named(&path) {
                    self.left_out.fetch_add(1, Ordering::Relaxed);
                } else if below.beside_among(&path, beside) {
                    self.left_out.fetch_add(1, Ordering::Relaxed);
                    self.caches.lock().0.push(path.clone());
                } else if below.passes_over(
                    &path,
                    skwir_core::fs::stat::identity(&path, &meta).map(|id| id.ino),
                ) {
                    // A snapshot (ADR-125, 14): a copy, not counted again.
                    self.left_out.fetch_add(1, Ordering::Relaxed);
                } else {
                    inside.push(path);
                }
            } else if !meta.is_file() {
                // Links are counted where their file is; a pipe or a
                // device is never read.
                continue;
            } else if below.leaves_out_file(&path) {
                // Forgotten, should the index have learned it before the
                // rule named it.
                self.left_out.fetch_add(1, Ordering::Relaxed);
                learned.push((path, Vec::new()));
                continue;
            }
            candidates.push((name, meta));
        }
        let read: Vec<(OsString, std::fs::Metadata)> = candidates
            .into_iter()
            .filter(|(name, meta)| {
                whole || owners.contains(name) || changed_since(&dir.join(name), meta, covered)
            })
            .collect();
        // The tagged files known here that are gone.
        if let Some(here) = self.known.lock().remove(dir) {
            for path in here {
                if path.file_name().is_none_or(|n| !present.contains(n)) {
                    learned.push((path, Vec::new()));
                }
            }
        }
        if !read.is_empty() {
            let named: Vec<skwir_core::notes::Named<'_>> = read
                .iter()
                .map(|(name, meta)| {
                    (
                        name.as_os_str(),
                        // As the folder's listing names the entry.
                        skwir_core::fs::stat::identity(&dir.join(name), meta)
                            .map(|file| EntryId::in_folder(file, id, name)),
                        meta.is_dir(),
                    )
                })
                .collect();
            let listed = |name: &OsStr| present.contains(name);
            let notes = app.notes.read(dir, &named, &self.policy, Some(&listed));
            self.read.fetch_add(read.len(), Ordering::Relaxed);
            for ((name, _), notes) in read.iter().zip(notes) {
                learned.push((dir.join(name), notes.notes.tag_names()));
            }
        }
        // This folder's own notes, kept inside it, read again in the folder
        // holding it: a read of that one asks only what moved there, and a
        // journal names only what changed in here. Its root's own are never
        // counted.
        if own_changed
            && dir != below.root()
            && let (Some(parent), Some(name)) = (dir.parent(), dir.file_name())
        {
            let holder = std::fs::symlink_metadata(parent)
                .ok()
                .and_then(|m| skwir_core::fs::stat::identity(parent, &m));
            let named = [(
                name,
                holder.map(|folder| EntryId::in_folder(id, folder, name)),
                true,
            )];
            if let Some(notes) = app.notes.read(parent, &named, &self.policy, None).pop() {
                self.read.fetch_add(1, Ordering::Relaxed);
                learned.push((dir.to_path_buf(), notes.notes.tag_names()));
            }
        }
        let full = {
            let mut all = self.learned.lock();
            all.append(&mut learned);
            (all.len() >= LEARN_EVERY).then(|| std::mem::take(&mut *all))
        };
        if let Some(full) = full {
            self.index.learn(full);
        }
        inside
    }
}

#[cfg(test)]
// Helpers of the tests that run on Unix alone (ADR-035).
#[cfg_attr(not(unix), allow(dead_code, unused_imports))]
mod tests {
    use super::run;
    use skwir_core::app::{App, AppConfig};
    use skwir_core::cancel::CancelToken;
    use std::sync::Arc;

    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("tags-count-")
            .tempdir_in(base)
            .unwrap()
    }

    /// Hidden entries are counted like any other (whether a place shows
    /// them is its pane's); what a walk leaves out below the root is not
    /// entered, a folder of the list and a cache that says so, nor a trash
    /// whatever the list says, their own tags read all the same. A cache
    /// no longer saying so is read whole by the next check.
    #[cfg(unix)]
    #[test]
    fn hidden_entries_are_counted_and_what_a_walk_leaves_out_is_not() {
        let tmp = home_tempdir();
        let root = tmp.path().join("it's home ü");
        let tag = |p: &std::path::Path, v: &str| {
            skwir_core::fs::xattr::set(p, "user.xdg.tags", v.as_bytes()).unwrap();
        };
        for dir in [
            ".config/app's",
            "node_modules/pkg",
            "built",
            ".Trash-1000/files",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            ".config/app's/x.conf",
            ".dot file",
            "node_modules/pkg/y.js",
            "built/z.o",
            ".Trash-1000/files/t",
        ] {
            std::fs::write(root.join(file), b"x").unwrap();
            tag(&root.join(file), "work");
        }
        tag(&root.join(".config"), "work");
        tag(&root.join("node_modules"), "work");
        std::fs::write(
            root.join("built/CACHEDIR.TAG"),
            b"Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(crate::TagsPlugin::default()), &app);
        let index = crate::index::TagIndex::new(None, &app, CancelToken::new());
        let cancel = CancelToken::new();
        let (since, dirs, counted) = run(&app, &index, &root, None, &cancel).unwrap().unwrap();
        let mut found: Vec<String> = index
            .files_under(&root, None)
            .into_iter()
            .map(|(path, _)| path.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                ".config",
                ".config/app's/x.conf",
                ".dot file",
                "node_modules"
            ]
        );
        assert_eq!(
            counted.left_out, 3,
            "node_modules, the trash, the cache: {counted:?}"
        );
        assert!(index.is_cache(&root.join("built")));
        index.cover(&root, since, &dirs, None, index.under());

        // The cache no longer says so: read whole at the next check.
        std::fs::remove_file(root.join("built/CACHEDIR.TAG")).unwrap();
        let known: std::collections::HashSet<u64> = dirs.iter().copied().collect();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let counted = super::run_with(
            &app,
            &index,
            &root,
            &app.left_out(),
            Some((later, &known)),
            &cancel,
        )
        .unwrap()
        .unwrap()
        .2;
        assert_eq!(counted.read, 1, "built/z.o: {counted:?}");
        assert!(!index.is_cache(&root.join("built")));
        assert_eq!(index.files_under(&root, None).len(), 5);

        // A count of a folder left out walks it.
        run(&app, &index, &root.join("node_modules"), None, &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(index.files_under(&root.join("node_modules"), None).len(), 1);
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn a_check_reads_what_changed_and_what_came_in_whole() {
        let tmp = home_tempdir();
        let root = tmp.path().join("it's root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(tmp.path().join("outside/deep")).unwrap();
        let tag = |p: &std::path::Path, v: &str| {
            skwir_core::fs::xattr::set(p, "user.xdg.tags", v.as_bytes()).unwrap();
        };
        for name in ["a", "b", "sub/c"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        tag(&root.join("a"), "work");
        tag(&root.join("sub/c"), "trip");
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(crate::TagsPlugin::default()), &app);
        let index = crate::index::TagIndex::new(None, &app, CancelToken::new());
        let cancel = CancelToken::new();
        let tally = |index: &Arc<crate::index::TagIndex>| index.tally(&root).0;
        let owned = |v: &[(&str, usize)]| {
            v.iter()
                .map(|(t, n)| ((*t).to_owned(), *n))
                .collect::<Vec<_>>()
        };

        // First count: every file is read.
        let (since, dirs, counted) = run(&app, &index, &root, None, &cancel).unwrap().unwrap();
        assert_eq!(
            (counted.folders, counted.read),
            (2, 4),
            "a, b, sub and sub/c"
        );
        assert_eq!(tally(&index), owned(&[("trip", 1), ("work", 1)]));
        index.cover(&root, since, &dirs, None, index.under());
        // Nothing stored without a cache: the folders seen are handed back.
        let known: std::collections::HashSet<u64> = dirs.iter().copied().collect();

        // Nothing changed since: nothing is read.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let check = |index: &Arc<crate::index::TagIndex>| {
            super::run_with(
                &app,
                index,
                &root,
                &app.left_out(),
                Some((later, &known)),
                &cancel,
            )
            .unwrap()
            .unwrap()
            .2
        };
        assert_eq!(check(&index).read, 0);

        // A folder moved in keeps its times: it is read whole. A file
        // removed is forgotten.
        std::fs::write(tmp.path().join("outside/deep/d"), b"x").unwrap();
        tag(&tmp.path().join("outside/deep/d"), "deep");
        std::fs::rename(tmp.path().join("outside/deep"), root.join("deep")).unwrap();
        std::fs::remove_file(root.join("sub/c")).unwrap();
        let counted = check(&index);
        assert_eq!(counted.read, 1, "deep/d only: {counted:?}");
        assert_eq!(tally(&index), owned(&[("deep", 1), ("work", 1)]));

        // A file tagged by another application after the count began is
        // read again, by its change time.
        let mark = std::time::SystemTime::now();
        std::thread::sleep(std::time::Duration::from_millis(20));
        tag(&root.join("b"), "new");
        let counted = super::run_with(
            &app,
            &index,
            &root,
            &app.left_out(),
            Some((mark, &known)),
            &cancel,
        )
        .unwrap()
        .unwrap()
        .2;
        assert_eq!(counted.read, 2, "b, and deep/d, still a folder not seen");
        assert_eq!(
            tally(&index),
            owned(&[("deep", 1), ("new", 1), ("work", 1)])
        );
    }

    /// A folder in one a folder covered leaves out is checked against what
    /// that folder's count saw, which never walked it: read whole, so what
    /// was tagged there since its own count is found.
    #[cfg(unix)]
    #[test]
    fn a_folder_left_out_below_a_covered_one_is_checked_whole() {
        let tmp = home_tempdir();
        let above = tmp.path().join("it's above");
        let root = above.join("node_modules/inner ü");
        std::fs::create_dir_all(&root).unwrap();
        let tag = |p: &std::path::Path, v: &str| {
            skwir_core::fs::xattr::set(p, "user.xdg.tags", v.as_bytes()).unwrap();
        };
        for name in ["a", "b"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        tag(&root.join("a"), "work");
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(crate::TagsPlugin::default()), &app);
        let cache = skwir_core::cache::Cache::new(tmp.path().join("cache"));
        let index =
            crate::index::TagIndex::new(Some(cache.namespace("tags")), &app, CancelToken::new());
        let cancel = CancelToken::new();
        let (since, dirs, _) = run(&app, &index, &root, None, &cancel).unwrap().unwrap();
        index.cover(&root, since, &dirs, None, index.under());
        // Tagged by another application, then the folder above counted
        // (later still: its count covers what changed before).
        tag(&root.join("b"), "trip");
        let (_, dirs, _) = run(&app, &index, &above, None, &cancel).unwrap().unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        index.cover(&above, later, &dirs, None, index.under());
        // Checked as its tag location checks it.
        let covered = index.covering(&root).map(|c| c.since());
        assert_eq!(covered, Some(later));
        run(&app, &index, &root, covered, &cancel).unwrap().unwrap();
        assert_eq!(
            index.tally(&root).0,
            [("trip".to_owned(), 1), ("work".to_owned(), 1)]
        );
    }

    /// What Windows marks hidden is counted like any other entry: the
    /// folder so marked is walked, its files read, and what a pane taught
    /// the index of them and of the files shown is what their notes say
    /// (none here: all forgotten).
    #[cfg(windows)]
    #[test]
    fn what_windows_marks_hidden_is_counted_like_any_other() {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        /// `FILE_ATTRIBUTE_HIDDEN`.
        const HIDDEN: u32 = 2;
        let tmp = home_tempdir();
        let root = tmp.path().join("it's root");
        let marked = root.join("marked ü");
        std::fs::create_dir_all(marked.join("deeper")).unwrap();
        let taught = [
            marked.join("a.txt"),
            marked.join("deeper").join("b.txt"),
            root.join("desktop.ini"),
            root.join("shown.txt"),
        ];
        for path in &taught[..2] {
            std::fs::write(path, b"x").unwrap();
        }
        std::fs::write(&taught[3], b"x").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .attributes(HIDDEN)
            .open(&taught[2])
            .unwrap();
        // A folder is marked by the system's own command: std makes none so.
        let status = std::process::Command::new("attrib")
            .arg("+h")
            .arg(&marked)
            .status()
            .unwrap();
        assert!(status.success());
        assert_ne!(
            std::fs::metadata(&marked).unwrap().file_attributes() & HIDDEN,
            0
        );
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(crate::TagsPlugin::default()), &app);
        let index = crate::index::TagIndex::new(None, &app, CancelToken::new());
        index.learn(
            taught
                .iter()
                .map(|p| (p.clone(), vec!["work".to_owned()]))
                .collect(),
        );
        assert_eq!(index.files_under(&root, None).len(), 4);
        let (_, _, counted) = run(&app, &index, &root, None, &CancelToken::new())
            .unwrap()
            .unwrap();
        // Every entry read: the marked folder, the files in it and in the
        // folder it holds, the marked file and the one shown.
        assert_eq!((counted.folders, counted.read), (3, 6), "{counted:?}");
        assert!(index.files_under(&root, None).is_empty());
        assert!(index.by_folder(&root).is_empty(), "all forgotten");
    }

    /// A remote's tree is read by one walker, the job's own thread: it
    /// counts what several count, every folder read once.
    #[cfg(unix)]
    #[test]
    fn one_walker_counts_what_several_count() {
        let tmp = home_tempdir();
        let root = tmp.path().join("it's root");
        let mut tagged = 0;
        for folder in 0..12 {
            let dir = root.join(format!("folder {folder}")).join("inner é");
            std::fs::create_dir_all(&dir).unwrap();
            for file in 0..3 {
                let path = dir.join(format!("file {file}"));
                std::fs::write(&path, b"x").unwrap();
                if (folder + file) % 2 == 0 {
                    skwir_core::fs::xattr::set(&path, "user.xdg.tags", b"work").unwrap();
                    tagged += 1;
                }
            }
        }
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(crate::TagsPlugin::default()), &app);
        let cancel = CancelToken::new();
        let below = app.left_out().below(&root, true);
        let counted = |walkers: usize| {
            let index = crate::index::TagIndex::new(None, &app, CancelToken::new());
            let (_, mut dirs, counted) = super::walk_with(
                &app,
                &index,
                &below,
                None,
                &cancel,
                walkers,
                vec![(root.clone(), true)],
            )
            .unwrap();
            dirs.sort_unstable();
            (index.tally(&root).0, dirs, counted.folders, counted.read)
        };
        let alone = counted(1);
        assert_eq!(alone.0, vec![("work".to_owned(), tagged)]);
        assert_eq!(alone.2, 25, "the root, its folders and theirs, each once");
        assert_eq!(alone, counted(8));
    }

    /// A check read from a journal that is every change (ADR-150, D4)
    /// reads the folders it names and nothing else: one holding a file
    /// tagged since, alone; one moved in, whole; one removed, its tagged
    /// files forgotten, its folder taken off those seen.
    #[cfg(unix)]
    #[test]
    fn a_check_from_a_journal_reads_only_the_folders_it_names() {
        use skwir_core::names::{Change, Journal, Mark, Noted};
        use std::os::unix::fs::MetadataExt;
        let tmp = home_tempdir();
        let root = tmp.path().join("it's followed");
        for dir in ["quiet/deep", "busy", "sub"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for name in ["quiet/deep/q", "quiet/r", "busy/x", "sub/c"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        let tag = |p: &std::path::Path, v: &str| {
            skwir_core::fs::xattr::set(p, "user.xdg.tags", v.as_bytes()).unwrap();
        };
        tag(&root.join("sub/c"), "trip");
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(crate::TagsPlugin::default()), &app);
        // With a cache: the folders a count saw are kept there.
        let slot = skwir_core::cache::Cache::new(tmp.path().join("cache")).namespace("tags");
        let index = crate::index::TagIndex::new(Some(slot), &app, CancelToken::new());
        let cancel = CancelToken::new();
        let (since, dirs, _) = run(&app, &index, &root, None, &cancel).unwrap().unwrap();
        index.cover(&root, since, &dirs, None, index.under());
        let last = index.covering(&root).unwrap();

        // Since: a file tagged, a folder moved in, a folder removed.
        tag(&root.join("busy/x"), "work");
        std::fs::create_dir_all(root.join("came/in")).unwrap();
        std::fs::write(root.join("came/in/y"), b"x").unwrap();
        tag(&root.join("came/in/y"), "new");
        let sub = std::fs::metadata(root.join("sub")).unwrap();
        std::fs::remove_dir_all(root.join("sub")).unwrap();
        let real = std::fs::canonicalize(&root).unwrap();
        let noted = |at: &str, kind, id| Noted {
            path: real.join(at),
            kind,
            id,
        };
        let journal = Journal {
            changes: vec![
                Change::Changed(noted("busy/x", skwir_core::entry::EntryKind::File, None)),
                Change::Added(noted("came", skwir_core::entry::EntryKind::Directory, None)),
                Change::Removed(noted(
                    "sub",
                    skwir_core::entry::EntryKind::Directory,
                    Some((sub.dev(), sub.ino())),
                )),
            ],
            mark: Mark {
                by: "test".into(),
                token: "t2".into(),
            },
            names: true,
            attrs: true,
            holes: Vec::new(),
        };
        let (_, after, counted) = super::run_followed(
            &app,
            &index,
            &root,
            &app.left_out(),
            &last,
            &journal,
            &cancel,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            counted.folders, 4,
            "busy, came, came/in, and the root, which held sub: {counted:?}"
        );
        let mut found: Vec<(String, Vec<String>)> = index
            .files_under(&root, None)
            .into_iter()
            .map(|(path, tags)| {
                (
                    path.strip_prefix(&root).unwrap().display().to_string(),
                    tags,
                )
            })
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                ("busy/x".to_owned(), vec!["work".to_owned()]),
                ("came/in/y".to_owned(), vec!["new".to_owned()]),
            ]
        );
        let removed = super::folder_key(sub.dev(), sub.ino(), &root.join("sub"));
        assert!(dirs.contains(&removed) && !after.contains(&removed));
        assert_eq!(after.len(), dirs.len() + 1, "came and came/in in, sub out");
    }

    /// A folder tagged since, the journal saying only that it changed, and
    /// one moved in, saying only that it came: each one's own tags are read
    /// in the folder holding it, as a walk reads them (ADR-150, D4).
    #[cfg(unix)]
    #[test]
    fn a_check_from_a_journal_reads_a_folders_own_tags_where_it_is_held() {
        use skwir_core::entry::EntryKind;
        use skwir_core::names::{Change, Journal, Mark, Noted};
        let tmp = home_tempdir();
        let root = tmp.path().join("it's held");
        std::fs::create_dir_all(root.join("trips/rome")).unwrap();
        std::fs::write(root.join("trips/rome/f"), b"x").unwrap();
        std::fs::create_dir_all(tmp.path().join("away/came")).unwrap();
        let tag = |p: &std::path::Path, v: &str| {
            skwir_core::fs::xattr::set(p, "user.xdg.tags", v.as_bytes()).unwrap();
        };
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(crate::TagsPlugin::default()), &app);
        let slot = skwir_core::cache::Cache::new(tmp.path().join("cache")).namespace("tags");
        let index = crate::index::TagIndex::new(Some(slot), &app, CancelToken::new());
        let cancel = CancelToken::new();
        let (since, dirs, _) = run(&app, &index, &root, None, &cancel).unwrap().unwrap();
        index.cover(&root, since, &dirs, None, index.under());
        let last = index.covering(&root).unwrap();

        tag(&root.join("trips/rome"), "trip");
        tag(&tmp.path().join("away/came"), "came");
        std::fs::rename(tmp.path().join("away/came"), root.join("came")).unwrap();
        let real = std::fs::canonicalize(&root).unwrap();
        let noted = |at: &str| Noted {
            path: real.join(at),
            kind: EntryKind::Directory,
            id: None,
        };
        let journal = Journal {
            changes: vec![
                Change::Changed(noted("trips/rome")),
                Change::Added(noted("came")),
            ],
            mark: Mark {
                by: "test".into(),
                token: "t2".into(),
            },
            names: true,
            attrs: true,
            holes: Vec::new(),
        };
        super::run_followed(
            &app,
            &index,
            &root,
            &app.left_out(),
            &last,
            &journal,
            &cancel,
        )
        .unwrap()
        .unwrap();
        let mut found: Vec<(String, Vec<String>)> = index
            .files_under(&root, None)
            .into_iter()
            .map(|(path, tags)| {
                (
                    path.strip_prefix(&root).unwrap().display().to_string(),
                    tags,
                )
            })
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                ("came".to_owned(), vec!["came".to_owned()]),
                ("trips/rome".to_owned(), vec!["trip".to_owned()]),
            ]
        );
    }

    /// A folder's own notes kept inside it (`TagSpaces`' `.ts/tsm.json`),
    /// written since, the folder's change time unmoved: read again, by a
    /// check that walks and by one read from a journal alike.
    #[cfg(unix)]
    #[test]
    fn a_folders_own_notes_kept_inside_it_are_read_again() {
        use skwir_core::names::{Change, Journal, Mark, Noted};
        let tmp = home_tempdir();
        let root = tmp.path().join("it's inside");
        std::fs::create_dir_all(root.join("trip/.ts")).unwrap();
        std::fs::write(root.join("trip/f"), b"x").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.companions.register("test", Arc::new(Beside));
        app.notes.register("test", Arc::new(Inside));
        let slot = skwir_core::cache::Cache::new(tmp.path().join("cache")).namespace("tags");
        let index = crate::index::TagIndex::new(Some(slot), &app, CancelToken::new());
        let cancel = CancelToken::new();
        let (since, dirs, _) = run(&app, &index, &root, None, &cancel).unwrap().unwrap();
        index.cover(&root, since, &dirs, None, index.under());
        assert!(index.files_under(&root, None).is_empty());
        let known = index.dirs(&root);
        let changed = |p: &std::path::Path| {
            let meta = std::fs::symlink_metadata(p).unwrap();
            skwir_core::fs::stat::changed(p, &meta).unwrap()
        };
        // Checked since just after the folders were made; the note written
        // once the clock has moved past that, as its change time says.
        let after = changed(&root.join("trip")).max(changed(&root.join("trip/.ts")))
            + std::time::Duration::from_nanos(1);
        let write = |tag: &str| {
            let own = root.join("trip/.ts/own");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                std::fs::write(&own, tag).unwrap();
                if changed(&own) >= after || std::time::Instant::now() > deadline {
                    break;
                }
                std::thread::yield_now();
            }
        };
        let found = |index: &Arc<crate::index::TagIndex>| {
            index
                .files_under(&root, None)
                .into_iter()
                .map(|(path, tags)| {
                    (
                        path.strip_prefix(&root).unwrap().display().to_string(),
                        tags,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert!(changed(&root.join("trip")) < after);

        // A check that walks.
        write("away");
        super::run_with(
            &app,
            &index,
            &root,
            &app.left_out(),
            Some((after, &known)),
            &cancel,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            found(&index),
            [("trip".to_owned(), vec!["away".to_owned()])]
        );

        // A check read from a journal naming the note alone.
        write("home");
        let at = after.duration_since(std::time::UNIX_EPOCH).unwrap();
        let last = crate::index::Covered {
            root: root.clone(),
            since: (at.as_secs(), at.subsec_nanos()),
            mark: None,
        };
        let journal = Journal {
            changes: vec![Change::Changed(Noted {
                path: std::fs::canonicalize(&root).unwrap().join("trip/.ts/own"),
                kind: skwir_core::entry::EntryKind::File,
                id: None,
            })],
            mark: Mark {
                by: "test".into(),
                token: "t2".into(),
            },
            names: true,
            attrs: true,
            holes: Vec::new(),
        };
        super::run_followed(
            &app,
            &index,
            &root,
            &app.left_out(),
            &last,
            &journal,
            &cancel,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            found(&index),
            [("trip".to_owned(), vec!["home".to_owned()])]
        );
    }

    /// A store keeping a folder's own tag inside it, in `.ts/own`, as
    /// `TagSpaces` keeps one in `.ts/tsm.json`.
    struct Inside;

    impl skwir_core::notes::NotesStore for Inside {
        fn id(&self) -> &'static str {
            "inside"
        }
        fn title(&self) -> skwir_core::words::Words {
            skwir_core::words::Words::as_written("inside")
        }
        fn kept(&self) -> skwir_core::notes::Kept {
            skwir_core::notes::Kept::Beside
        }
        fn holds(&self) -> skwir_core::notes::Holds {
            skwir_core::notes::Holds {
                tags: true,
                ..skwir_core::notes::Holds::default()
            }
        }
        fn default_write(&self) -> skwir_core::notes::Write {
            skwir_core::notes::Write::Off
        }
        fn read(
            &self,
            _: &std::path::Path,
            files: &[skwir_core::notes::Probe<'_>],
            _: Option<&skwir_core::metadata::Listed<'_>>,
        ) -> Vec<Option<skwir_core::notes::Notes>> {
            files
                .iter()
                .map(|f| {
                    let name = std::fs::read_to_string(f.path.join(".ts/own"))
                        .ok()
                        .filter(|_| f.folder)?;
                    Some(skwir_core::notes::Notes {
                        tags: vec![skwir_core::notes::Tag { name, color: None }],
                        ..skwir_core::notes::Notes::default()
                    })
                })
                .collect()
        }
        fn keys(
            &self,
            _: &skwir_core::notes::Probe<'_>,
            _: bool,
        ) -> Vec<skwir_core::notes::raw::RawKey> {
            Vec::new()
        }
        fn write(
            &self,
            _: &skwir_core::notes::Probe<'_>,
            _: &skwir_core::notes::Notes,
            _: bool,
        ) -> std::io::Result<skwir_core::notes::Wrote> {
            Ok(skwir_core::notes::Wrote::NotHere)
        }
    }

    /// What a journal names becomes the folders a check reads: a change at
    /// the root's own level reads the root, one in a companions' folder
    /// (`.ts/`) its folder, one inside a folder read whole nothing more,
    /// what the walk leaves out nothing.
    #[test]
    fn a_journal_names_the_folders_a_check_reads() {
        use skwir_core::coverage::{Hole, Why};
        use skwir_core::entry::EntryKind;
        use skwir_core::names::{Change, Journal, Mark, Noted};
        use std::path::{Path, PathBuf};
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        // A folder of companions, as TagSpaces' `.ts/`.
        app.companions.register("test", Arc::new(Beside));
        let root = Path::new("/followed");
        let below = app.left_out().below(root, false);
        let noted = |at: &str, kind| Noted {
            path: root.join(at),
            kind,
            id: None,
        };
        let journal = Journal {
            changes: vec![
                Change::Changed(noted("top.txt", EntryKind::File)),
                Change::Added(noted("a/.ts/b.json", EntryKind::File)),
                Change::Added(noted("new", EntryKind::Directory)),
                Change::Changed(noted("new/inside/f", EntryKind::File)),
                Change::Changed(noted("x/node_modules/p/f", EntryKind::File)),
                // A folder tagged: read in the folder holding it.
                Change::Changed(noted("trips/rome", EntryKind::Directory)),
                Change::NotCovered(Hole {
                    path: root.join("gone dark"),
                    why: Why::Denied,
                }),
            ],
            mark: Mark {
                by: "test".into(),
                token: String::new(),
            },
            names: true,
            attrs: true,
            holes: vec![Hole {
                path: root.join("held/out"),
                why: Why::Excluded,
            }],
        };
        let (seeds, removed) = super::seeds_of(&app, root, root, &below, &journal);
        let seeds: Vec<(PathBuf, bool)> = seeds;
        assert_eq!(
            seeds,
            [
                (root.to_path_buf(), false),
                (root.join("a"), false),
                (root.join("gone dark"), true),
                (root.join("held"), false),
                (root.join("held/out"), true),
                (root.join("new"), true),
                (root.join("trips"), false),
                (root.join("trips/rome"), false),
            ]
        );
        assert!(removed.is_empty());

        // The root itself, or a folder above it, gone or come again: the
        // root read again whole; a folder above it only changed, nothing.
        for (above, said) in [
            (Path::new("/"), "a folder above it removed"),
            (root, "the root itself removed"),
        ] {
            let journal = Journal {
                changes: vec![
                    Change::Changed(Noted {
                        path: PathBuf::from("/"),
                        kind: EntryKind::Directory,
                        id: None,
                    }),
                    Change::Removed(Noted {
                        path: above.to_path_buf(),
                        kind: EntryKind::Directory,
                        id: Some((1, 2)),
                    }),
                ],
                holes: Vec::new(),
                ..journal.clone()
            };
            let (seeds, _) = super::seeds_of(&app, root, root, &below, &journal);
            assert_eq!(seeds, [(root.to_path_buf(), true)], "{said}");
        }

        // A root gone since: an error, which ends the count, never a
        // cancel, which leaves it running.
        let index = crate::index::TagIndex::new(None, &app, CancelToken::new());
        let last = crate::index::Covered {
            root: root.to_path_buf(),
            since: (1_800_000_000, 0),
            mark: None,
        };
        let followed = super::run_followed(
            &app,
            &index,
            root,
            &app.left_out(),
            &last,
            &journal,
            &CancelToken::new(),
        );
        assert!(followed.is_err());
    }

    /// Seeds are kept each once, none inside a folder read with what it
    /// holds, whatever their order, among many siblings.
    #[test]
    fn seeds_are_kept_once_outside_folders_read_whole() {
        use std::path::PathBuf;
        let at = |p: &str| PathBuf::from(p);
        let mut seeds = vec![
            (at("/r/b/x"), false),
            (at("/r/a"), true),
            (at("/r/a/deep"), false),
            (at("/r/b"), false),
            (at("/r/b"), false),
            (at("/r/a b"), false),
            (at("/r/a/deep/er"), true),
            (at("/r/b"), true),
            (at("/r/b/y/z"), false),
            (at("/r/c"), false),
            (at("/r/c/d"), false),
        ];
        // Many siblings below a folder read with what it holds, and one
        // shallow above them all: each compared with its own folders above.
        seeds.extend((0..50_000).map(|i| (at(&format!("/r/many/{i:05}")), false)));
        seeds.push((at("/r/many"), true));
        seeds.extend((0..50_000).map(|i| (at(&format!("/r/wide/{i:05}")), false)));
        seeds.push((at("/r/wide"), false));
        let kept = super::nearest_kept(seeds);
        assert_eq!(kept.len(), 7 + 50_000, "{:?}", &kept[..8]);
        assert_eq!(
            kept[..7],
            [
                (at("/r/a"), true),
                (at("/r/a b"), false),
                (at("/r/b"), true),
                (at("/r/c"), false),
                (at("/r/c/d"), false),
                (at("/r/many"), true),
                (at("/r/wide"), false),
            ]
        );
    }

    /// What a count is of follows the stores and the companions
    /// registered, the stores' read choices too, however often it is
    /// asked (once a folder): a store turned on is never left unread.
    #[test]
    fn what_a_count_is_of_follows_the_stores_and_companions() {
        let tmp = home_tempdir();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        let index = crate::index::TagIndex::new(None, &app, CancelToken::new());
        let before = index.under();
        assert_eq!(index.under(), before, "the same, asked again");
        app.companions.register("test", Arc::new(Beside));
        let with = index.under();
        assert_ne!(with.reads, before.reads, "a companion registered");
        app.notes.register("test", Arc::new(Inside));
        let stored = index.under();
        assert_ne!(stored.reads, with.reads, "a store registered");
        let mut policy = app.notes.policy();
        policy.read.retain(|s| s != "inside");
        app.notes.choose(Some(policy));
        assert_eq!(index.under().reads, with.reads, "its reading turned off");
    }

    /// Companions kept in a folder `.ts` beside their files, as `TagSpaces`
    /// keeps them.
    struct Beside;

    impl skwir_core::companions::Companion for Beside {
        fn id(&self) -> &'static str {
            "beside"
        }
        fn of(&self, _: &std::ffi::OsStr) -> Vec<std::path::PathBuf> {
            Vec::new()
        }
        fn owner(&self, _: &std::ffi::OsStr) -> Option<std::ffi::OsString> {
            None
        }
        fn is_folder(&self, name: &std::ffi::OsStr) -> bool {
            name == ".ts"
        }
    }

    /// A stand-in names index whose journal answers what it is given, and
    /// says which mark it was asked from.
    struct Following {
        answers: parking_lot::Mutex<Vec<skwir_core::names::Since>>,
        asked: Arc<parking_lot::Mutex<Vec<Option<String>>>>,
    }

    struct Session(
        Arc<parking_lot::Mutex<Vec<skwir_core::names::Since>>>,
        Arc<parking_lot::Mutex<Vec<Option<String>>>>,
    );

    impl skwir_core::names::Session for Session {
        fn sync(&mut self) -> Option<skwir_core::coverage::Coverage> {
            None
        }
        fn query(
            &mut self,
            _: &skwir_core::names::Query,
            _: &mut dyn FnMut(Vec<std::path::PathBuf>) -> bool,
        ) -> Option<skwir_core::names::Answer> {
            None
        }
        fn changes(
            &mut self,
            since: Option<&skwir_core::names::Mark>,
        ) -> Option<skwir_core::names::Since> {
            self.1.lock().push(since.map(|m| m.token.clone()));
            let mut answers = self.0.lock();
            (!answers.is_empty()).then(|| answers.remove(0))
        }
    }

    impl skwir_core::names::NameIndex for Following {
        fn id(&self) -> &'static str {
            "following"
        }
        fn by(&self) -> skwir_core::words::Words {
            skwir_core::words::Words::as_written("following")
        }
        fn syncs(&self) -> bool {
            true
        }
        fn open(&self, _: &std::path::Path, _: &CancelToken) -> Option<skwir_core::names::Opened> {
            None
        }
        fn follow(
            &self,
            _: &std::path::Path,
            _: &CancelToken,
        ) -> Option<skwir_core::names::Opened> {
            let answers = Arc::new(parking_lot::Mutex::new(std::mem::take(
                &mut *self.answers.lock(),
            )));
            Some(skwir_core::names::Opened {
                coverage: skwir_core::coverage::Coverage::as_of(None),
                session: Box::new(Session(answers, Arc::clone(&self.asked))),
            })
        }
    }

    /// A count follows the journal from its last count's mark: every change
    /// is read from it; one not every change, or too old, is walked, and
    /// the next count asks from the mark taken before the walk.
    #[test]
    fn a_count_follows_the_journal_from_the_last_mark() {
        use skwir_core::names::{Journal, Mark, Since};
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        let root = std::path::Path::new("/followed");
        let journal = |token: &str, complete: bool| {
            Since::Journal(Journal {
                changes: Vec::new(),
                mark: Mark {
                    by: "following".into(),
                    token: token.into(),
                },
                names: complete,
                attrs: complete,
                holes: Vec::new(),
            })
        };
        let asked = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let follow = |answers: Vec<Since>, last: Option<&crate::index::Covered>| {
            app.names.unregister_owner("test");
            app.names.register(
                "test",
                Arc::new(Following {
                    answers: parking_lot::Mutex::new(answers),
                    asked: Arc::clone(&asked),
                }),
            );
            super::follow(&app, root, last, &CancelToken::new())
        };
        let covered = |token: Option<&str>| crate::index::Covered {
            root: root.to_path_buf(),
            since: (1_800_000_000, 0),
            mark: token.map(|token| Mark {
                by: "following".into(),
                token: token.into(),
            }),
        };
        // A first count: a mark from now.
        let first = follow(vec![journal("t1", false)], None).unwrap();
        assert!(first.journal.is_none());
        assert_eq!(first.mark.token, "t1");
        // From its mark, every change: read from the journal.
        let second = follow(vec![journal("t2", true)], Some(&covered(Some("t1")))).unwrap();
        assert!(second.journal.is_some());
        assert_eq!(second.mark.token, "t2");
        // Not every change: walked, its mark the next count's.
        let third = follow(vec![journal("t3", false)], Some(&covered(Some("t2")))).unwrap();
        assert!(third.journal.is_none());
        assert_eq!(third.mark.token, "t3");
        // Too old: walked from a mark taken now.
        let fourth = follow(
            vec![Since::TooOld, journal("t4", false)],
            Some(&covered(Some("t3"))),
        )
        .unwrap();
        assert!(fourth.journal.is_none());
        assert_eq!(fourth.mark.token, "t4");
        // Another folder's mark is not this one's.
        let above = crate::index::Covered {
            root: std::path::PathBuf::from("/"),
            ..covered(Some("t4"))
        };
        follow(vec![journal("t5", false)], Some(&above)).unwrap();
        assert_eq!(
            *asked.lock(),
            [
                None,
                Some("t1".to_owned()),
                Some("t2".to_owned()),
                Some("t3".to_owned()),
                None,
                None
            ]
        );
    }
}
