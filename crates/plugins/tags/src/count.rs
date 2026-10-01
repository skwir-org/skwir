//! The count behind the Tags view: every tagged file under a folder,
//! outside hidden folders, learned by the index (`hints::TagIndex`), which
//! then covers the folder as of the time the count began.
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

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::entry::EntryId;

use crate::hints::{TagIndex, stable_hash};

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

fn changed_since(meta: &std::fs::Metadata, since: Option<SystemTime>) -> bool {
    let Some(since) = since else { return true };
    let Ok(ctime) = u64::try_from(meta.ctime()) else {
        return true;
    };
    let nanos = u32::try_from(meta.ctime_nsec()).unwrap_or(0);
    UNIX_EPOCH + Duration::new(ctime, nanos) >= since
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
    let began = SystemTime::now().checked_sub(MARGIN).unwrap_or(UNIX_EPOCH);
    std::fs::read_dir(root)?;
    let covered = last.map(|(since, _)| since);
    let empty = HashSet::new();
    let known_dirs = last.map_or(&empty, |(_, dirs)| dirs);
    // Asked once per device the walk meets.
    let mut devices: std::collections::HashMap<u64, bool> = std::collections::HashMap::new();
    let mut trusted = |dev: u64, dir: &Path| {
        *devices
            .entry(dev)
            .or_insert_with(|| bibli_platform::filesystem::change_times_tell(dir))
    };
    // The tagged files the index knows, by folder: those no longer there
    // are forgotten; the folders not walked again are what is gone.
    let mut known = index.by_folder(root);
    let policy = app.notes.policy();
    let mut seen_dirs: Vec<u64> = Vec::new();
    let mut learned: Vec<(PathBuf, Vec<String>)> = Vec::new();
    let mut counted = Counted::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let Ok(meta) = std::fs::symlink_metadata(&dir) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        counted.folders += 1;
        let key = folder_key(meta.dev(), meta.ino(), &dir);
        seen_dirs.push(key);
        // Read whole: on a first count, a folder not seen here before, or
        // one whose times do not tell.
        let mut whole =
            covered.is_none() || !known_dirs.contains(&key) || !trusted(meta.dev(), &dir);
        let mut present: HashSet<OsString> = HashSet::new();
        let mut candidates: Vec<(OsString, std::fs::Metadata)> = Vec::new();
        let mut owners: HashSet<OsString> = HashSet::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Ok(meta) = entry.metadata() else { continue };
            present.insert(name.clone());
            if hidden(&name) {
                if meta.is_dir() && app.companions.is_folder(&name) {
                    // A folder of companions for the whole folder: anything
                    // in it changed, the folder is read whole.
                    whole |= changed_since(&meta, covered)
                        || std::fs::read_dir(entry.path()).is_ok_and(|inside| {
                            inside
                                .flatten()
                                .any(|e| e.metadata().is_ok_and(|m| changed_since(&m, covered)))
                        });
                } else if let Some(owner) = app.companions.owner(&name)
                    && changed_since(&meta, covered)
                {
                    owners.insert(owner);
                }
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if !meta.is_file() {
                // Links are counted where their file is; a pipe or a
                // device is never read.
                continue;
            }
            // A companion that is not hidden (`x.jpg.xmp`) is its file's.
            if let Some(owner) = app.companions.owner(&name)
                && owner != name
            {
                if changed_since(&meta, covered) {
                    owners.insert(owner);
                }
                continue;
            }
            candidates.push((name, meta));
        }
        let read: Vec<(OsString, std::fs::Metadata)> = candidates
            .into_iter()
            .filter(|(name, meta)| whole || owners.contains(name) || changed_since(meta, covered))
            .collect();
        // The tagged files known here that are gone.
        if let Some(here) = known.remove(&dir) {
            for path in here {
                if path.file_name().is_none_or(|n| !present.contains(n)) {
                    learned.push((path, Vec::new()));
                }
            }
        }
        if !read.is_empty() {
            let named: Vec<bibli_core::notes::Named<'_>> = read
                .iter()
                .map(|(name, meta)| {
                    (
                        name.as_os_str(),
                        Some(EntryId {
                            dev: meta.dev(),
                            ino: meta.ino(),
                        }),
                        meta.is_dir(),
                    )
                })
                .collect();
            let listed = |name: &OsStr| present.contains(name);
            let notes = app.notes.read(&dir, &named, &policy, Some(&listed));
            counted.read += read.len();
            for ((name, _), notes) in read.iter().zip(notes) {
                learned.push((dir.join(name), notes.notes.tag_names()));
            }
        }
        if learned.len() >= LEARN_EVERY {
            index.learn(std::mem::take(&mut learned));
        }
    }
    // What the index knew in folders no longer there (or now hidden: those
    // are not counted anyway, and forgetting them costs a read later).
    for (dir, paths) in known {
        if !dir
            .strip_prefix(root)
            .is_ok_and(|rest| rest.components().any(|c| hidden(c.as_os_str())))
            && std::fs::symlink_metadata(&dir).is_err()
        {
            learned.extend(paths.into_iter().map(|p| (p, Vec::new())));
        }
    }
    index.learn(learned);
    Ok(Some((began, seen_dirs, counted)))
}

#[cfg(test)]
mod tests {
    use super::{hidden, run};
    use bibli_core::app::{App, AppConfig};
    use bibli_core::cancel::CancelToken;
    use std::ffi::OsStr;
    use std::sync::Arc;

    #[test]
    fn hidden_names_start_with_a_dot() {
        assert!(hidden(OsStr::new(".ts")) && !hidden(OsStr::new("a.ts")));
    }

    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("tags-count-")
            .tempdir_in(base)
            .unwrap()
    }

    #[test]
    fn a_check_reads_what_changed_and_what_came_in_whole() {
        let tmp = home_tempdir();
        let root = tmp.path().join("it's root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(tmp.path().join("outside/deep")).unwrap();
        let tag =
            |p: &std::path::Path, v: &str| xattr::set(p, "user.xdg.tags", v.as_bytes()).unwrap();
        for name in ["a", "b", "sub/c"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        tag(&root.join("a"), "work");
        tag(&root.join("sub/c"), "trip");
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins
            .add(Arc::new(crate::TagsPlugin::default()), &app);
        let index = crate::hints::TagIndex::new(None, &app, CancelToken::new());
        let cancel = CancelToken::new();
        let tally = |index: &Arc<crate::hints::TagIndex>| index.tally(&root).0;
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
        let check = |index: &Arc<crate::hints::TagIndex>| {
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
}
