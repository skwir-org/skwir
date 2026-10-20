//! A copy's staging folder, and what a copy that ended without cleaning
//! up left.
//!
//! A copy, and a move across drives, is made in a folder in its
//! destination (`.skwir-copy-…`, one a plan's copies there share,
//! `StepContext::staging_for`) and renamed into place once whole
//! (`builtin::copy_staged`), so a partial copy never looks finished. Any
//! step that makes something in a destination does the same through
//! `StepContext::stage` (a plugin's: an archive extracted).
//! Stopped or failed, a copy removes what it made there, and the plan the
//! folder when it ends; a process that ends mid-copy
//! (a crash, a kill, a logout) cannot. So the folder holds a `lock` its
//! copy keeps locked while it runs, and a later skwir removes the folders
//! whose lock nobody holds ([`sweep`]): a copy into a folder sweeps it
//! before it stages there, and a run sweeps at its start the folders an
//! earlier one was copying into when it ended ([`UnderWay`]). A folder
//! whose copy still runs, in this skwir or another, keeps its lock; one
//! without a lock (being made) is left. Only where
//! this machine alone writes (`fs::volume::only_here`): a lock taken by
//! another machine on a shared drive is not seen here.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use super::sys;

/// What every staging folder's name begins with.
const PREFIX: &str = ".skwir-copy-";
/// The file a staging folder's copy keeps locked.
const LOCK: &str = "lock";

/// A staging folder in a destination, removed when dropped, its lock held
/// until then: what a step makes there is made in it and put in place
/// whole ([`Staging::place`]). It holds its lock, named `lock`: make what
/// is made in a folder of its own inside it.
pub struct Staging {
    /// Released before the folder goes: Windows removes no folder holding
    /// a file still open.
    lock: Option<File>,
    dir: tempfile::TempDir,
}

impl Staging {
    pub(super) fn new(dest_dir: &Path) -> io::Result<Self> {
        let mut builder = tempfile::Builder::new();
        builder.prefix(PREFIX);
        if let Some(private) = sys::owner_only() {
            builder.permissions(private);
        }
        let dir = builder.tempdir_in(dest_dir)?;
        Ok(Self {
            lock: hold(dir.path()),
            dir,
        })
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Put `made`, made in this folder, at `target`, never over what is
    /// there.
    ///
    /// # Errors
    /// When `target` is taken, or the rename fails.
    pub fn place(&self, made: &Path, target: &Path) -> io::Result<()> {
        debug_assert!(made.starts_with(self.path()), "made in the staging folder");
        sys::rename_noreplace(made, target)
    }

    /// Remove the folder now, whatever its copy left in it.
    ///
    /// # Errors
    /// When something in it cannot be removed.
    pub fn remove(self) -> io::Result<()> {
        let Self { lock, dir } = self;
        drop(lock);
        remove_tree(&dir.keep())
    }
}

/// The lock of a staging folder `dir`: locked first, named `lock` only
/// then, so a sweep never finds the lock of a running copy free. `None`
/// where the filesystem takes no lock: the folder has none, and no sweep
/// removes it.
fn hold(dir: &Path) -> Option<File> {
    let file = tempfile::Builder::new()
        .prefix(".lock-")
        .tempfile_in(dir)
        .ok()?;
    file.as_file().try_lock().ok()?;
    file.persist(dir.join(LOCK)).ok()
}

/// Remove a staging tree. Its folders may already have a copy's read-only
/// permissions: they are opened to their owner first, which is safe only
/// in a tree of our own making.
pub(super) fn remove_tree(path: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        std::fs::set_permissions(path, sys::opened_to_owner(&metadata.permissions()))?;
        for entry in std::fs::read_dir(path)? {
            remove_tree(&entry?.path())?;
        }
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Remove the staging folders in `dir` that copies which ended mid-way
/// left: those whose lock nobody holds. Only where this machine alone
/// writes. Reads `dir` and mountinfo: on a worker.
pub fn sweep(dir: &Path) {
    if !crate::fs::volume::only_here(dir) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let ours = entry
            .file_name()
            .as_encoded_bytes()
            .starts_with(PREFIX.as_bytes());
        if !ours || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let folder = entry.path();
        let Ok(lock) = crate::fs::file::open_regular(&folder.join(LOCK)) else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        // Its copy is gone. Released first, as `Staging::remove` does.
        drop(lock);
        // The folder is a destination's, and its name a private path.
        match remove_tree(&folder) {
            Ok(()) => tracing::info!("removed what a copy cut short had left"),
            Err(e) => tracing::warn!("what a copy cut short left stays: {e}"),
        }
    }
}

/// The folders copies are under way in, noted in skwir's cache for the
/// next run to [`sweep`] should this one end mid-copy: a file per folder
/// and plan, holding the folder's path, locked while the plan runs and
/// removed when it ends. The cache may go at any time: what it held is
/// then swept only by the next copy into those folders.
#[derive(Clone, Debug)]
pub struct UnderWay {
    dir: PathBuf,
}

/// A folder noted as under way, until dropped.
pub(super) type Mark = tempfile::NamedTempFile;

impl UnderWay {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Note that a plan stages copies in `dest_dir`. `None` when the cache
    /// cannot keep it.
    pub(super) fn mark(&self, dest_dir: &Path) -> Option<Mark> {
        std::fs::create_dir_all(&self.dir).ok()?;
        let mut mark = tempfile::Builder::new()
            .prefix("copy-")
            .tempfile_in(&self.dir)
            .ok()?;
        // Locked before it says anything: a sweep never reads half of it.
        mark.as_file().try_lock().ok()?;
        mark.write_all(dest_dir.as_os_str().as_encoded_bytes())
            .ok()?;
        Some(mark)
    }

    /// Sweep the folders an earlier run was copying into when it ended,
    /// and forget them; one on a drive not there now is kept for a later
    /// run. A note still locked is a running skwir's. On a worker.
    pub fn sweep_left(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(mut note) = crate::fs::file::open_regular(&path) else {
                continue;
            };
            if note.try_lock().is_err() {
                continue;
            }
            // A path, never more than this.
            let mut bytes = Vec::new();
            if (&mut note).take(1 << 16).read_to_end(&mut bytes).is_err() {
                continue;
            }
            drop(note);
            let folder = PathBuf::from(crate::os_path::from_bytes(bytes));
            if crate::fs::reach::of(&folder).is_some() {
                // Never swept on a network (`only_here`), nor looked at: a
                // server may not answer (ADR-077).
                let _ = std::fs::remove_file(&path);
            } else if folder.is_absolute() && folder.is_dir() {
                sweep(&folder);
                let _ = std::fs::remove_file(&path);
            } else if !folder.is_absolute() {
                // Not a note this module wrote.
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

// The sweep's tests need Linux's devices and locks.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// A folder on the disk the tests run on (never tmpfs: it has no
    /// device, so nothing there is swept).
    fn scratch() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::tempdir_in(base).unwrap()
    }

    /// What a copy leaves when its process ends mid-way: a staging folder
    /// whose lock nobody holds, part of the copy in it, read-only.
    fn cut_short(dest: &Path) -> PathBuf {
        let Staging { lock, dir } = Staging::new(dest).unwrap();
        drop(lock);
        let left = dir.keep();
        let entry = left.join("entry");
        std::fs::create_dir(&entry).unwrap();
        std::fs::write(entry.join("photo 1.jpg"), b"part").unwrap();
        let mut read_only = std::fs::metadata(&entry).unwrap().permissions();
        read_only.set_readonly(true);
        std::fs::set_permissions(&entry, read_only).unwrap();
        left
    }

    #[test]
    fn a_sweep_removes_what_a_copy_cut_short_left_and_nothing_else() {
        let tmp = scratch();
        let dest = tmp.path();
        assert!(crate::fs::volume::only_here(dest), "a disk of this machine");
        let left = cut_short(dest);
        let running = Staging::new(dest).unwrap();
        // Being made: nothing tells it is over.
        let unlocked = dest.join(".skwir-copy-being-made");
        std::fs::create_dir(&unlocked).unwrap();
        // The user's own, named alike.
        let theirs = dest.join(".skwir-copy-notes.txt");
        std::fs::write(&theirs, b"mine").unwrap();

        sweep(dest);
        assert!(!left.exists(), "its copy is gone");
        assert!(running.path().join(LOCK).is_file(), "its copy runs");
        assert!(unlocked.is_dir() && theirs.is_file());

        let path = running.path().to_path_buf();
        running.remove().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_run_sweeps_the_folders_an_earlier_one_was_copying_into() {
        let tmp = scratch();
        let (dest, gone) = (tmp.path().join("stick"), tmp.path().join("unplugged"));
        std::fs::create_dir(&dest).unwrap();
        let earlier = UnderWay::new(tmp.path().join("cache"));
        let later = UnderWay::new(tmp.path().join("cache"));

        // Still running: its note is locked, and stays.
        let running = earlier.mark(&dest).unwrap();
        let left = cut_short(&dest);
        later.sweep_left();
        assert!(left.exists() && running.path().exists());
        drop(running);
        assert_eq!(
            std::fs::read_dir(tmp.path().join("cache")).unwrap().count(),
            0
        );

        // Ended mid-copy: its notes stay unlocked.
        let (_, note) = earlier.mark(&dest).unwrap().keep().unwrap();
        let (_, away) = earlier.mark(&gone).unwrap().keep().unwrap();
        later.sweep_left();
        assert!(!left.exists(), "swept");
        assert!(!note.exists(), "and forgotten");
        assert!(away.exists(), "kept until its drive is back");
    }
}
