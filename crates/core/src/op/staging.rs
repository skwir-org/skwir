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
//!
//! On a remote, where another machine's copies stage too, a folder is
//! swept only if this machine noted it: a staging folder made there is
//! noted itself ([`UnderWay::mark`]), the note kept when the folder cannot
//! be removed (its server stopped answering mid-copy), and what such
//! notes name is removed when this machine reaches that remote again
//! ([`UnderWay::sweep_noted`]: a copy into that folder, its mount found
//! answering), in its lane.

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
    /// `None` once removed.
    dir: Option<PathBuf>,
    /// On a remote, the note naming it ([`UnderWay::mark`]): forgotten
    /// once it is removed, kept should it stay.
    note: Option<Mark>,
}

impl Staging {
    /// A new one in `dest_dir`, noted in `under_way` where that is a
    /// remote's folder.
    pub(super) fn new(dest_dir: &Path, under_way: Option<&UnderWay>) -> io::Result<Self> {
        let mut builder = tempfile::Builder::new();
        builder.prefix(PREFIX);
        if let Some(private) = sys::owner_only() {
            builder.permissions(private);
        }
        let dir = builder.tempdir_in(dest_dir)?.keep();
        let note = under_way
            .filter(|_| crate::fs::reach::of(dest_dir).is_some())
            .and_then(|under_way| under_way.mark(&dir));
        Ok(Self {
            lock: hold(&dir),
            dir: Some(dir),
            note,
        })
    }

    pub fn path(&self) -> &Path {
        self.dir
            .as_deref()
            .expect("a staging folder is there until removed")
    }

    /// Remove it, what it holds included; its note kept where it stays.
    fn remove_now(&mut self) -> io::Result<()> {
        drop(self.lock.take());
        let Some(dir) = self.dir.take() else {
            return Ok(());
        };
        match remove_tree(&dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                // Its server gone, say: swept once reached again.
                if let Some(note) = self.note.take() {
                    let _ = note.keep();
                }
                Err(e)
            }
            _ => {
                drop(self.note.take());
                Ok(())
            }
        }
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
    pub fn remove(mut self) -> io::Result<()> {
        self.remove_now()
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Err(e) = self.remove_now() {
            tracing::warn!("a staging folder stays: {e}");
        }
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
            // One on a remote is not looked at here, where its server may
            // not answer (ADR-077): swept once reached
            // ([`Self::sweep_noted`]).
            if crate::fs::reach::of(&folder).is_none() {
                if folder.is_absolute() && folder.is_dir() {
                    sweep(&folder);
                    let _ = std::fs::remove_file(&path);
                } else if !folder.is_absolute() {
                    // Not a note this module wrote.
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    /// Remove the staging folders this machine noted on a remote, under
    /// `within`, that their copies left (their runs ended: their notes
    /// and locks are free), and forget them; one that cannot be reached
    /// is kept for later. Only what a note names: another machine's
    /// copies stage there too, their locks not seen here. Reads that
    /// remote: in its lane.
    pub fn sweep_noted(&self, within: &Path) {
        for (note, folder) in self.noted() {
            if !folder.starts_with(within)
                || crate::fs::reach::of(&folder).is_none()
                || !folder
                    .file_name()
                    .is_some_and(|n| n.as_encoded_bytes().starts_with(PREFIX.as_bytes()))
            {
                continue;
            }
            match std::fs::symlink_metadata(&folder) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(_) => continue,
                Ok(_) => {
                    // Its copy runs still, in another skwir of this machine.
                    let held = crate::fs::file::open_regular(&folder.join(LOCK))
                        .is_ok_and(|lock| lock.try_lock().is_err());
                    if held {
                        continue;
                    }
                    if let Err(e) = remove_tree(&folder) {
                        tracing::warn!("what a copy cut short left on a remote stays: {e}");
                        continue;
                    }
                    tracing::info!("removed what a copy cut short had left on a remote");
                }
            }
            let _ = std::fs::remove_file(&note);
        }
    }

    /// Whether a note names a folder under `within` (read here only).
    pub fn notes_under(&self, within: &Path) -> bool {
        self.noted()
            .iter()
            .any(|(_, folder)| folder.starts_with(within))
    }

    /// Each note no running plan holds, with the folder it names.
    fn noted(&self) -> Vec<(PathBuf, PathBuf)> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(mut note) = crate::fs::file::open_regular(&path) else {
                continue;
            };
            if note.try_lock().is_err() {
                continue;
            }
            let mut bytes = Vec::new();
            if (&mut note).take(1 << 16).read_to_end(&mut bytes).is_err() {
                continue;
            }
            out.push((path, PathBuf::from(crate::os_path::from_bytes(bytes))));
        }
        out
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
        let mut staging = Staging::new(dest, None).unwrap();
        drop(staging.lock.take());
        let left = staging.dir.take().unwrap();
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
        let running = Staging::new(dest, None).unwrap();
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

    /// On a remote, a staging folder is noted itself: one its plan could
    /// not remove (its server stopped answering; here, a folder above it
    /// refusing) keeps its note, which a run's start leaves alone and
    /// which is swept once the remote is reached. Another machine's,
    /// which no note here names, is never touched.
    #[test]
    fn what_a_copy_left_on_a_remote_is_swept_once_it_is_reached() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = scratch();
        let dest = tmp.path().join("server");
        std::fs::create_dir(&dest).unwrap();
        crate::fs::reach::pretend_remote(&dest);
        let cache = tmp.path().join("cache");
        let under_way = UnderWay::new(cache.clone());
        let theirs = {
            let mut staging = Staging::new(&dest, None).unwrap();
            drop(staging.lock.take());
            staging.dir.take().unwrap()
        };

        let staging = Staging::new(&dest, Some(&under_way)).unwrap();
        let ours = staging.path().to_path_buf();
        std::fs::write(ours.join("part"), b"half").unwrap();
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o555)).unwrap();
        drop(staging);
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(ours.exists(), "it could not be removed");
        under_way.sweep_left();
        assert!(
            ours.exists() && under_way.notes_under(&dest),
            "left for later"
        );

        under_way.sweep_noted(&dest);
        assert!(!ours.exists(), "swept once reached");
        assert!(theirs.exists(), "another machine's left");
        assert!(!under_way.notes_under(&dest), "and forgotten");

        // One its plan removes is forgotten at once.
        let staging = Staging::new(&dest, Some(&under_way)).unwrap();
        staging.remove().unwrap();
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
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
