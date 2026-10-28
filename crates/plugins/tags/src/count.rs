//! The count behind tag locations (`location.rs`): every tagged file under a folder,
//! outside hidden folders and the network mounts below it (ADR-077),
//! learned by the index (`index::TagIndex`), which then covers the folder
//! as of the time the count began.
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

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::{Condvar, Mutex};

use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::entry::{EntryId, FileId};
use skwir_core::fs::reach;

use crate::index::{TagIndex, stable_hash};

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

fn hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
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
    let dirs = covered.map(|_| index.dirs(root)).unwrap_or_default();
    run_with(
        app,
        index,
        root,
        covered.map(|since| (since, &dirs)),
        cancel,
    )
}

/// [`run`], given the folders the last count saw.
///
/// # Errors
/// When `root` cannot be read.
pub fn run_with(
    app: &App,
    index: &Arc<TagIndex>,
    root: &Path,
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
    Ok(walk_with(app, index, root, last, cancel, walkers))
}

/// [`run_with`], read by `walkers` threads, this one among them.
fn walk_with(
    app: &App,
    index: &Arc<TagIndex>,
    root: &Path,
    last: Option<(SystemTime, &HashSet<u64>)>,
    cancel: &CancelToken,
    walkers: usize,
) -> Option<(SystemTime, Vec<u64>, Counted)> {
    let began = SystemTime::now().checked_sub(MARGIN).unwrap_or(UNIX_EPOCH);
    let empty = HashSet::new();
    let walk = Walk {
        app,
        index,
        covered: last.map(|(since, _)| since),
        known_dirs: last.map_or(&empty, |(_, dirs)| dirs),
        policy: app.notes.policy(),
        cancel,
        queue: Mutex::new((vec![root.to_path_buf()], 0)),
        moved: Condvar::new(),
        devices: Mutex::new(HashMap::new()),
        // The tagged files the index knows, by folder: those no longer
        // there are forgotten; the folders not walked again are what is
        // gone.
        known: Mutex::new(index.by_folder(root)),
        seen_dirs: Mutex::new(Vec::new()),
        learned: Mutex::new(Vec::new()),
        folders: AtomicUsize::new(0),
        read: AtomicUsize::new(0),
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
    // What the index knew in folders no longer there (or now hidden: those
    // are not counted anyway, and forgetting them costs a read later).
    for (dir, paths) in walk.known.into_inner() {
        if !dir
            .strip_prefix(root)
            .is_ok_and(|rest| rest.components().any(|c| hidden(c.as_os_str())))
            && std::fs::symlink_metadata(&dir).is_err()
        {
            learned.extend(paths.into_iter().map(|p| (p, Vec::new())));
        }
    }
    index.learn(learned);
    let counted = Counted {
        folders: walk.folders.into_inner(),
        read: walk.read.into_inner(),
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
    /// The folders to read, and how many threads are reading one: once
    /// none is left and none is read, the walk is done.
    queue: Mutex<(Vec<PathBuf>, usize)>,
    moved: Condvar,
    /// Whether each device's change times tell, asked once per device.
    devices: Mutex<HashMap<u64, bool>>,
    known: Mutex<HashMap<PathBuf, Vec<PathBuf>>>,
    seen_dirs: Mutex<Vec<u64>>,
    learned: Mutex<Vec<(PathBuf, Vec<String>)>>,
    folders: AtomicUsize,
    read: AtomicUsize,
}

impl Walk<'_> {
    /// Read folders until none is left, or the count is cancelled.
    fn work(&self) {
        loop {
            let dir = {
                let mut queue = self.queue.lock();
                loop {
                    if self.cancel.is_cancelled() {
                        self.moved.notify_all();
                        return;
                    }
                    if let Some(dir) = queue.0.pop() {
                        queue.1 += 1;
                        break dir;
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
            queue.0.extend(inside);
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
        let (app, covered) = (self.app, self.covered);
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
        for entry in entries.flatten() {
            let name = entry.file_name();
            // A network mount below the folder counted is neither read nor
            // entered (ADR-077): a count of it walks it from its own
            // folder. Told before its stat, which its server answers.
            if entry.file_type().is_ok_and(|t| t.is_dir()) && reach::is_point(&entry.path()) {
                present.insert(name);
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            present.insert(name.clone());
            // Hidden as a search's walk has it: a dot, or Windows' mark.
            if skwir_core::fs::is_hidden(&entry) {
                if meta.is_dir() && app.companions.is_folder(&name) {
                    // A folder of companions for the whole folder: anything
                    // in it changed, the folder is read whole.
                    whole |= changed_since(&entry.path(), &meta, covered)
                        || std::fs::read_dir(entry.path()).is_ok_and(|inside| {
                            inside.flatten().any(|e| {
                                e.metadata()
                                    .is_ok_and(|m| changed_since(&e.path(), &m, covered))
                            })
                        });
                } else if let Some(owner) = app.companions.owner(&name) {
                    if changed_since(&entry.path(), &meta, covered) {
                        owners.insert(owner);
                    }
                } else if !hidden(&name) {
                    // Hidden by Windows' mark alone, which the index does
                    // not see (it lists all but dot files): what it knows
                    // of the entry is forgotten, as of one gone.
                    present.remove(&name);
                }
                continue;
            }
            if meta.is_dir() {
                inside.push(entry.path());
            } else if !meta.is_file() {
                // Links are counted where their file is; a pipe or a
                // device is never read.
                continue;
            }
            // A companion that is not hidden (`x.jpg.xmp`) is its file's.
            if let Some(owner) = app.companions.owner(&name)
                && owner != name
            {
                if changed_since(&entry.path(), &meta, covered) {
                    owners.insert(owner);
                }
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
        let mut learned = Vec::new();
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
    use super::{hidden, run};
    use skwir_core::app::{App, AppConfig};
    use skwir_core::cancel::CancelToken;
    use std::ffi::OsStr;
    use std::sync::Arc;

    #[test]
    fn hidden_names_start_with_a_dot() {
        assert!(hidden(OsStr::new(".ts")) && !hidden(OsStr::new("a.ts")));
    }

    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("tags-count-")
            .tempdir_in(base)
            .unwrap()
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
        index.cover(&root, since, &dirs);
        // Nothing stored without a cache: the folders seen are handed back.
        let known: std::collections::HashSet<u64> = dirs.iter().copied().collect();

        // Nothing changed since: nothing is read.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let check = |index: &Arc<crate::index::TagIndex>| {
            super::run_with(&app, index, &root, Some((later, &known)), &cancel)
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
        let counted = super::run_with(&app, &index, &root, Some((mark, &known)), &cancel)
            .unwrap()
            .unwrap()
            .2;
        assert_eq!(counted.read, 2, "b, and deep/d, still a folder not seen");
        assert_eq!(
            tally(&index),
            owned(&[("deep", 1), ("new", 1), ("work", 1)])
        );
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
        let counted = |walkers: usize| {
            let index = crate::index::TagIndex::new(None, &app, CancelToken::new());
            let (_, mut dirs, counted) =
                super::walk_with(&app, &index, &root, None, &cancel, walkers).unwrap();
            dirs.sort_unstable();
            (index.tally(&root).0, dirs, counted.folders, counted.read)
        };
        let alone = counted(1);
        assert_eq!(alone.0, vec![("work".to_owned(), tagged)]);
        assert_eq!(alone.2, 25, "the root, its folders and theirs, each once");
        assert_eq!(alone, counted(8));
    }
}
