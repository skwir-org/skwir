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
//!
//! A staging folder this process lets go of and cannot remove stays
//! noted, wherever it is: only a folder removed forgets its notes. Not
//! found is no proof it went: its drive unmounted or pulled out, it is
//! there still, out of reach. On this machine's volumes, the folder it is
//! in is noted then (a destination's note, as a run's start reads them),
//! and swept by the next run's start, or as its volume comes back
//! ([`UnderWay::sweep_left_under`], `App::volumes_changed`).
//!
//! A large file a copy failed partway through (its server stopped
//! answering, its drive was pulled out) is kept instead, for Retry to go
//! on from ([`Partial`]): its folder stays, locked, and noted where it
//! is, until the retry places it or the stopped plan is let go; a process
//! that ends meanwhile leaves it to the sweeps. As its volume is let go of
//! (an unmount, `Executor::let_go_under`), its lock is let go of too and
//! the folder kept ([`Staging::let_go`]): nothing of Skwir's holds the
//! volume, and should it stay (the unmount refused), Retry takes the lock
//! again ([`Staging::hold_again`]) and goes on from what was kept, its
//! last part read back. Meanwhile this process sweeps it not ([`KEPT`]);
//! another skwir may, as it would once this one ended: a sweep removes a
//! lock's name while it holds it, before anything else of the folder, so
//! a lock taken again is known still the folder's by its name.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use super::sys;

/// What every staging folder's name begins with.
pub(crate) const PREFIX: &str = ".skwir-copy-";
/// The file a staging folder's copy keeps locked.
const LOCK: &str = "lock";

/// The names of the staging folders kept for Retry whose lock this
/// process let go of with their volume ([`Staging::let_go`]): what a copy
/// cut short left, theirs still, which no sweep here takes
/// ([`sweep`]). By name alone, which a staging folder's random end makes
/// its own, whatever path its folder is reached by (a link): two alike in
/// two folders would leave the other's to a later sweep, nothing more.
static KEPT: parking_lot::Mutex<std::collections::BTreeSet<std::ffi::OsString>> =
    parking_lot::Mutex::new(std::collections::BTreeSet::new());

/// The staging folders this test's process made.
#[cfg(test)]
pub(super) static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
thread_local! {
    /// What a test has happen as a lock let go of is about to be taken
    /// again, opened and not locked yet ([`take_lock`]): a sweep, here or
    /// in another skwir.
    static AS_TAKEN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// How long a lock taken again waits for this process's own, let go of
/// with its volume, to be closed ([`Released`]): as long as a plan waits
/// for what this process keeps open to be let go of (`executor`'s).
const CLOSED_WITHIN: std::time::Duration = super::executor::LET_GO_WITHIN;

/// A staging folder in a destination, removed when dropped, its lock held
/// until then: what a step makes there is made in it and put in place
/// whole ([`Staging::place`]). It holds its lock, named `lock`: make what
/// is made in a folder of its own inside it.
pub struct Staging {
    /// Released before the folder goes: Windows removes no folder holding
    /// a file still open.
    lock: Option<File>,
    /// Its lock let go of with its volume, the folder kept ([`Self::let_go`]),
    /// until it is taken again ([`Self::hold_again`]).
    let_go: bool,
    /// The lock let go of, being closed off the caller's thread: told
    /// closed as it is ([`Released`]).
    closing: Option<crossbeam_channel::Receiver<()>>,
    /// `None` once removed.
    dir: Option<PathBuf>,
    /// On a remote, the note naming it ([`UnderWay::mark`]): forgotten
    /// once it is removed, kept should it stay.
    note: Option<Mark>,
    /// On this machine's volumes, where the folder it is in is noted
    /// should it stay as it goes ([`Self::stays`]), for the sweeps.
    noted_in: Option<UnderWay>,
}

/// A staging folder's lock let go of with its volume ([`Staging::let_go`]),
/// closed as it is dropped, off the caller's thread; its folder's
/// [`Staging::hold_again`] waits for it to be, or it would meet this
/// process's own lock.
pub(super) struct Released {
    /// Closed first: a struct's fields are dropped in order.
    _lock: File,
    /// Dropped once the lock is closed, which tells its folder so.
    _closed: crossbeam_channel::Sender<()>,
}

/// What came of taking a staging folder's lock ([`take_lock`]).
enum Taken {
    Locked(File),
    /// Another holds it, or swept the folder (its lock's name gone): left
    /// to it.
    Theirs,
    /// Not reached: the folder gone, its volume away, its lock unreadable.
    Unreached,
}

impl Staging {
    /// A new one in `dest_dir`, noted in `under_way`: itself where that is
    /// a remote's folder, `dest_dir` should it stay elsewhere.
    pub(super) fn new(dest_dir: &Path, under_way: Option<&UnderWay>) -> io::Result<Self> {
        #[cfg(test)]
        MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut builder = tempfile::Builder::new();
        builder.prefix(PREFIX);
        if let Some(private) = sys::owner_only() {
            builder.permissions(private);
        }
        let dir = builder.tempdir_in(dest_dir)?.keep();
        let remote = crate::fs::reach::of(dest_dir).is_some();
        let note = under_way
            .filter(|_| remote)
            .and_then(|under_way| under_way.mark(&dir));
        Ok(Self {
            lock: hold(&dir),
            let_go: false,
            closing: None,
            dir: Some(dir),
            note,
            noted_in: under_way.filter(|_| !remote).cloned(),
        })
    }

    /// Let go of the lock, which holds its volume busy, the folder and what
    /// it holds kept: what a copy cut short kept for Retry, as its volume
    /// is let go of (`Executor::let_go_under`). Noted as this process's
    /// first ([`KEPT`]), so no sweep here takes it once its lock is free.
    /// The lock is handed back, to be closed off the caller's thread (on
    /// FAT, mounted `flush`, a close waits a tenth of a second); `None`
    /// where none is held.
    pub(super) fn let_go(&mut self) -> Option<Released> {
        self.lock.as_ref()?;
        if let Some(name) = self.dir.as_deref().and_then(Path::file_name) {
            KEPT.lock().insert(name.to_os_string());
        }
        self.let_go = true;
        let (closed, closing) = crossbeam_channel::bounded(0);
        self.closing = Some(closing);
        Some(Released {
            _lock: self.lock.take()?,
            _closed: closed,
        })
    }

    /// This process's own lock let go of ([`Self::let_go`]) closed, or
    /// [`CLOSED_WITHIN`] gone by: whether it is.
    fn closed(&mut self) -> bool {
        self.closing.take().is_none_or(|closing| {
            closing.recv_timeout(CLOSED_WITHIN) != Err(crossbeam_channel::RecvTimeoutError::Timeout)
        })
    }

    /// Take again the lock let go of with its volume ([`Self::let_go`]),
    /// before a Retry goes on in the folder: false when the folder or its
    /// lock is not reached (gone, its volume not there now), or another
    /// holds it or swept it, the folder then forgotten. True for one never
    /// let go of. This process's own lock is closed first (on its thread),
    /// and the folder kept from this process's sweeps ([`KEPT`]) until it
    /// is locked again: never in between, or a sweep here would take it.
    /// I/O: the Retry's worker.
    pub(super) fn hold_again(&mut self) -> bool {
        if !self.let_go {
            return true;
        }
        if !self.closed() {
            // Its drive holds the close: not taken again (it starts that
            // file again), removed or noted as it is let go of.
            self.kept_no_more();
            return false;
        }
        let taken = self.dir.as_deref().map_or(Taken::Unreached, take_lock);
        self.kept_no_more();
        match taken {
            Taken::Locked(lock) => {
                self.lock = Some(lock);
                true
            }
            Taken::Theirs => {
                // Not this process's any more: left to whoever holds it.
                self.dir = None;
                false
            }
            Taken::Unreached => false,
        }
    }

    /// Where the folder is now as the system knows it from the lock it
    /// holds (a link on the way resolved, `fs::dir::held_at`), asking
    /// nothing of its volume; `None` where it holds none.
    pub(super) fn held_at(&self) -> Option<PathBuf> {
        crate::fs::dir::held_at(self.lock.as_ref()?)?
            .parent()
            .map(Path::to_path_buf)
    }

    /// No longer kept unlocked by this process ([`KEPT`]).
    fn kept_no_more(&mut self) {
        if std::mem::take(&mut self.let_go)
            && let Some(name) = self.dir.as_deref().and_then(Path::file_name)
        {
            KEPT.lock().remove(name);
        }
    }

    pub fn path(&self) -> &Path {
        self.dir
            .as_deref()
            .expect("a staging folder is there until removed")
    }

    /// Remove it, what it holds included; noted where it stays
    /// ([`Self::stays`]). Not found, it is `Ok`, and noted all the same.
    fn remove_now(&mut self) -> io::Result<()> {
        drop(self.lock.take());
        let Some(dir) = self.dir.take() else {
            return Ok(());
        };
        // Its lock let go of closed first: Windows removes no folder
        // holding a file still open.
        self.closed();
        // Kept from this process's sweeps no more.
        if std::mem::take(&mut self.let_go)
            && let Some(name) = dir.file_name()
        {
            KEPT.lock().remove(name);
        }
        match remove_tree(&dir) {
            Ok(()) => {
                drop(self.note.take());
                Ok(())
            }
            Err(e) => {
                self.stays(&dir);
                if e.kind() == io::ErrorKind::NotFound {
                    tracing::info!("a staging folder out of reach is noted, to be swept");
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    /// The folder `dir` stays, or may, as it is let go of: not removed
    /// (its server stopped answering), or not found (its drive unmounted
    /// or pulled out, it is there still, out of reach). Noted, for a sweep
    /// to remove once it is reached again: on a remote its own note kept
    /// ([`UnderWay::sweep_noted`]), elsewhere the folder it is in noted, as
    /// a copy under way notes it ([`UnderWay::sweep_left`],
    /// [`UnderWay::sweep_left_under`]).
    fn stays(&mut self, dir: &Path) {
        if let Some(note) = self.note.take() {
            let _ = note.keep();
        }
        if let (Some(under_way), Some(in_dir)) = (self.noted_in.take(), dir.parent())
            && let Some(note) = under_way.mark(in_dir)
        {
            let _ = note.keep();
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

    /// Put `made`, anywhere in what this folder holds, at `target`, never
    /// over what is there: an item of a folder made here (an archive's
    /// member, merged into a folder there). Taking a name out of a folder
    /// needs that folder open to its owner, and moving a folder to another
    /// needs the folder itself open (Unix: its `..` is written): what an
    /// archive made read-only is opened for the move (`opened`), the folder
    /// it leaves staying so (this folder goes whole), the one it moves
    /// getting its own permissions back once placed. Placed, it is `Ok`
    /// whatever becomes of those permissions (a warning says they could not
    /// be given back), so that what placed it notes it placed.
    ///
    /// # Errors
    /// When `target` is taken, or the move fails.
    pub fn place_within(&self, made: &Path, target: &Path) -> io::Result<()> {
        debug_assert!(made.starts_with(self.path()), "made in the staging folder");
        if let Some(parent) = made.parent().filter(|parent| *parent != self.path()) {
            opened(parent)?;
        }
        moved_opened(made, target)
    }

    /// Take `placed`, put out of this folder by [`Self::place_within`]
    /// (a merge placing the next item failed), back to `made` where it was
    /// made: opened for the move as it was placed, left so (this folder
    /// goes whole). On failure, given its permissions back where it stays.
    ///
    /// # Errors
    /// When `made` is taken, or the move fails.
    pub fn take_back(&self, placed: &Path, made: &Path) -> io::Result<()> {
        debug_assert!(
            made.starts_with(self.path()),
            "taken back into the staging folder"
        );
        let own = opened(placed)?;
        let back = sys::rename_noreplace(placed, made);
        if back.is_err()
            && let Some(own) = own
            && let Err(e) = std::fs::set_permissions(placed, own)
        {
            tracing::warn!("{} stays opened: {e}", placed.display());
        }
        back
    }

    /// Prepare this empty directory itself and publish it beside its
    /// staging name. A read-only directory cannot move between parents on
    /// Unix (updating `..` needs write access), but can be renamed within
    /// its parent with its final permissions already in place.
    #[cfg(unix)]
    pub(super) fn place_empty_directory(
        mut self,
        target: &Path,
        prepare: impl FnOnce(&Path) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        debug_assert_eq!(self.path().parent(), target.parent());
        // Unlink while still holding the lock, including through publication:
        // a sweep may already have opened it before its name disappears.
        match std::fs::remove_file(self.path().join(LOCK)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        prepare(self.path())?;
        sys::rename_noreplace(self.path(), target)?;
        self.dir = None;
        drop(self.note.take());
        Ok(())
    }

    /// Remove the folder now, whatever its copy left in it.
    ///
    /// # Errors
    /// When something in it cannot be removed.
    pub fn remove(mut self) -> io::Result<()> {
        self.remove_now()
    }
}

/// What a copy cut short left in its staging folder, kept for Retry
/// (`Executor::retry`) to go on from: the folder, its lock held and its
/// notes kept, so no sweep takes it; removed with what it holds when let
/// go, as any staging folder. Its volume let go of, its lock is let go of
/// and the folder kept ([`Staging::let_go`]).
pub struct Partial {
    pub(super) staging: Staging,
    /// The destination's note as under way, on this machine's volumes.
    pub(super) mark: Option<Mark>,
    pub(super) dest_dir: PathBuf,
    /// Where its staging folder is as the system resolved it once kept
    /// ([`Staging::held_at`]): a destination reached through a link is on
    /// the volume the link leads to, which is what is let go of.
    pub(super) held_at: Option<PathBuf>,
    pub(super) kept: Kept,
}

impl Partial {
    /// Whether it is on the volume whose mount point is `tree`: by the
    /// path its plan named, or by where its folder is (a link on the way
    /// followed), as a listing's `holds` is told.
    pub(super) fn is_under(&self, tree: &Path) -> bool {
        self.dest_dir.starts_with(tree)
            || self
                .held_at
                .as_deref()
                .is_some_and(|held| held.starts_with(tree))
    }

    /// Whether it is on a remote, by either path (`fs::reach`, the table in
    /// memory).
    pub(super) fn on_remote(&self) -> bool {
        crate::fs::reach::of(&self.dest_dir).is_some()
            || self
                .held_at
                .as_deref()
                .is_some_and(|held| crate::fs::reach::of(held).is_some())
    }
}

/// What of a copy cut short is kept in its staging folder.
#[derive(Debug)]
pub(super) enum Kept {
    /// A step's copy, a large file or a folder's tree, of `src`.
    Copy {
        at: PathBuf,
        src: PathBuf,
        /// The file it was writing, the start of its source.
        cut: Option<Cut>,
    },
    /// A batch's (`transfer::batch`) of entries of the folder `from`.
    Batch(KeptBatch),
}

/// What a batch cut short made (`transfer::batch`): the folder it copied
/// into, where it did, the folder a tool keeps a file it was writing in
/// (rsync's `--partial-dir`), maybe never made, and the sources of the
/// files it laid out there as they were then (`transfer::Layout`): a file
/// it was writing is gone on from while its source is still so.
#[derive(Debug, Clone)]
pub(super) struct KeptBatch {
    pub(super) from: PathBuf,
    pub(super) into: Option<PathBuf>,
    pub(super) partial: PathBuf,
    pub(super) sources: std::collections::HashMap<PathBuf, Was>,
}

/// A source as a copy found it: its length, and when it was modified.
pub(super) type Was = (u64, Option<std::time::SystemTime>);

/// A file a copy writes byte by byte from its start: where, and its
/// source as it was then, its length and when it was modified.
#[derive(Debug)]
pub(super) struct Cut {
    pub(super) at: PathBuf,
    pub(super) len: u64,
    pub(super) modified: Option<std::time::SystemTime>,
}

impl std::fmt::Debug for Partial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Partial")
            .field("dest_dir", &self.dest_dir)
            .field("kept", &self.kept)
            .finish_non_exhaustive()
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

/// The lock of the staging folder `dir`, taken again: locked by this
/// process, still the one the folder names (a sweep removes the name of
/// the lock it took before it lets go of it, [`sweep`]: one locked after
/// is the lock of a folder being removed).
fn take_lock(dir: &Path) -> Taken {
    let path = dir.join(LOCK);
    let Ok(lock) = crate::fs::file::open_regular(&path) else {
        return Taken::Unreached;
    };
    #[cfg(test)]
    if let Some(meanwhile) = AS_TAKEN.with_borrow_mut(Option::take) {
        meanwhile();
    }
    if lock.try_lock().is_err() || !still_named(&lock, &path) {
        return Taken::Theirs;
    }
    Taken::Locked(lock)
}

/// Whether `file`, opened from `path`, is what `path` names still.
fn still_named(file: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (file.metadata(), std::fs::symlink_metadata(path)) {
            (Ok(held), Ok(named)) => held.dev() == named.dev() && held.ino() == named.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        // No sweep runs there (`fs::volume::only_here`): what was opened
        // is what is named.
        let _ = (file, path);
        true
    }
}

/// Remove a staging tree. Its folders may already have a copy's read-only
/// permissions: they are opened to their owner first, which is safe only
/// in a tree of our own making; one open already is left as it is, on a
/// volume that keeps no modes (a phone's MTP through GVfs refuses any).
pub(super) fn remove_tree(path: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        let mode = metadata.permissions();
        if !sys::open_to_owner(&mode) {
            sys::mode_kept(std::fs::set_permissions(path, sys::opened_to_owner(&mode)))?;
        }
        for entry in std::fs::read_dir(path)? {
            remove_tree(&entry?.path())?;
        }
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// `path` opened to its owner where it is a folder that is not (an
/// archive's read-only one), so that it can move between folders and what
/// it holds can be taken out: what it was, to give back.
fn opened(path: &Path) -> io::Result<Option<std::fs::Permissions>> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir() || sys::open_to_owner(&meta.permissions()) {
        return Ok(None);
    }
    std::fs::set_permissions(path, sys::opened_to_owner(&meta.permissions()))?;
    Ok(Some(meta.permissions()))
}

/// `from` moved to `to`, never over what is there, opened for the move
/// ([`opened`]) and given its permissions back where it ends up: placed,
/// `Ok` all the same where they cannot be (said in a warning).
fn moved_opened(from: &Path, to: &Path) -> io::Result<()> {
    let own = opened(from)?;
    let moved = sys::rename_noreplace(from, to);
    if let Some(own) = own {
        let now = if moved.is_ok() { to } else { from };
        if let Err(e) = std::fs::set_permissions(now, own) {
            tracing::warn!("{} stays opened: {e}", now.display());
        }
    }
    moved
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
        // Kept for Retry by this process, its lock let go of with its
        // volume (`Staging::let_go`).
        if KEPT.lock().contains(&entry.file_name()) {
            continue;
        }
        let folder = entry.path();
        let lock_at = folder.join(LOCK);
        let Ok(lock) = crate::fs::file::open_regular(&lock_at) else {
            continue;
        };
        if lock.try_lock().is_err() {
            continue;
        }
        // Its copy is gone. Its lock's name goes first, while it is held:
        // whoever opened it meanwhile (a Retry taking again what it let go
        // of, `Staging::hold_again`) and locks it once let go of finds it
        // named no more. Then released, as `Staging::remove` does.
        if std::fs::remove_file(&lock_at).is_err() {
            continue;
        }
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
    /// and those staging folders were in that stayed out of reach as they
    /// were let go of (`Staging::stays`), and forget them; one on a drive
    /// not there now is kept for later. A note still locked is a running
    /// plan's. On a worker.
    pub fn sweep_left(&self) {
        self.sweep_here(|_| true);
    }

    /// [`Self::sweep_left`] of the folders under the mount points `points`,
    /// mounted just now (a drive come back, `App::volumes_changed`). Reads
    /// them: on a worker.
    pub fn sweep_left_under(&self, points: &[PathBuf]) {
        self.sweep_here(|folder| points.iter().any(|point| folder.starts_with(point)));
    }

    /// Sweep the folders on this machine's volumes the notes name that
    /// `wanted` takes, those there now, and forget them.
    fn sweep_here(&self, wanted: impl Fn(&Path) -> bool) {
        for (note, folder) in self.noted() {
            // One on a remote is not looked at here, where its server may
            // not answer (ADR-077): swept once reached
            // ([`Self::sweep_noted`]).
            if !wanted(&folder) || crate::fs::reach::of(&folder).is_some() {
                continue;
            }
            if folder.is_absolute() && folder.is_dir() {
                sweep(&folder);
                let _ = std::fs::remove_file(&note);
            } else if !folder.is_absolute() {
                // Not a note this module wrote.
                let _ = std::fs::remove_file(&note);
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

    /// A read-only folder an archive made, placed out of the staging
    /// folder with its own permissions, is taken back into it all the same
    /// (a merge that failed after it), and goes with it.
    #[test]
    fn a_read_only_folder_placed_is_taken_back() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = scratch();
        let staging = Staging::new(tmp.path(), None).unwrap();
        let made = staging.path().join("archive/sealed");
        std::fs::create_dir_all(&made).unwrap();
        std::fs::write(made.join("inside.txt"), b"inside").unwrap();
        std::fs::set_permissions(&made, std::fs::Permissions::from_mode(0o555)).unwrap();
        std::fs::set_permissions(
            made.parent().unwrap(),
            std::fs::Permissions::from_mode(0o555),
        )
        .unwrap();

        let placed = tmp.path().join("it's sealed");
        staging.place_within(&made, &placed).unwrap();
        let bits = std::fs::metadata(&placed).unwrap().permissions().mode() & 0o777;
        assert_eq!(bits, 0o555, "its own permissions back");
        staging.take_back(&placed, &made).unwrap();
        assert!(!placed.exists());
        assert!(made.join("inside.txt").is_file());
        staging.remove().unwrap();
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_directory_restore_keeps_its_lock_through_read_only_publication() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = scratch();
        let staging = Staging::new(tmp.path(), None).unwrap();
        // A sweep may have opened the marker just before it is unlinked.
        let opened = crate::fs::file::open_regular(&staging.path().join(LOCK)).unwrap();
        let target = tmp.path().join("restored");
        staging
            .place_empty_directory(&target, |path| {
                assert!(opened.try_lock().is_err(), "the old marker stays locked");
                assert!(!path.join(LOCK).exists());
                sweep(tmp.path());
                assert!(path.is_dir(), "a running publication is not swept");
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500))?;
                Ok(())
            })
            .unwrap();
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o500
        );
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
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

    /// A staging folder let go of with its volume, out of reach as it goes
    /// (its drive unmounted: here, its folder moved away), is noted: a
    /// run's start keeps the note while the drive is away, and the folder
    /// is swept once its drive is back. One removed leaves no note.
    #[test]
    fn a_staging_folder_its_drive_took_away_is_swept_once_it_is_back() {
        let tmp = scratch();
        let (stick, away) = (tmp.path().join("stick"), tmp.path().join("unplugged"));
        let dest = stick.join("l'été");
        std::fs::create_dir_all(&dest).unwrap();
        let cache = tmp.path().join("cache");
        let under_way = UnderWay::new(cache.clone());
        let mut staging = Staging::new(&dest, Some(&under_way)).unwrap();
        let kept = staging.path().to_path_buf();
        std::fs::write(kept.join("0"), b"part").unwrap();
        drop(staging.let_go());
        std::fs::rename(&stick, &away).unwrap();
        drop(staging);
        assert!(under_way.notes_under(&dest), "noted as it went");
        under_way.sweep_left();
        assert!(under_way.notes_under(&dest), "kept while its drive is away");

        std::fs::rename(&away, &stick).unwrap();
        assert!(kept.is_dir());
        under_way.sweep_left_under(&[tmp.path().join("elsewhere")]);
        assert!(kept.is_dir(), "another drive's coming leaves it");
        under_way.sweep_left_under(std::slice::from_ref(&stick));
        assert!(!kept.exists(), "swept as its drive is back");
        assert!(!under_way.notes_under(&dest), "and forgotten");

        let staging = Staging::new(&dest, Some(&under_way)).unwrap();
        drop(staging);
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    }

    /// A lock let go of with its volume is taken again before the folder
    /// is kept from this process's sweeps no more: a sweep meanwhile (a
    /// copy into the same folder) leaves it.
    #[test]
    fn a_lock_taken_again_is_kept_from_a_sweep_meanwhile() {
        let tmp = scratch();
        let mut staging = Staging::new(tmp.path(), None).unwrap();
        drop(staging.let_go());
        let dest = tmp.path().to_path_buf();
        AS_TAKEN.set(Some(Box::new(move || sweep(&dest))));
        assert!(staging.hold_again(), "held again");
        assert!(staging.path().join(LOCK).is_file(), "not swept meanwhile");
        sweep(tmp.path());
        assert!(staging.path().is_dir(), "held");
        let name = staging.path().file_name().unwrap().to_os_string();
        assert!(!KEPT.lock().contains(&name), "kept no more");
        staging.remove().unwrap();
    }

    /// A Retry taking the lock again before this process's own let go of
    /// is closed (off the caller's thread) waits for it, rather than take
    /// it for another's.
    #[test]
    fn a_lock_taken_again_waits_for_its_own_to_be_closed() {
        let tmp = scratch();
        let mut staging = Staging::new(tmp.path(), None).unwrap();
        let released = staging.let_go().unwrap();
        let closing = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            drop(released);
        });
        assert!(staging.hold_again(), "its own, closed meanwhile");
        closing.join().unwrap();
        let path = staging.path().to_path_buf();
        staging.remove().unwrap();
        assert!(!path.exists());
    }

    /// A lock taken again just as another skwir sweeps the folder (it
    /// took the lock, removed its name, let go of it) is not held: the
    /// folder is left to that sweep.
    #[test]
    fn a_lock_another_sweep_took_is_not_held_again() {
        let tmp = scratch();
        let mut staging = Staging::new(tmp.path(), None).unwrap();
        drop(staging.let_go());
        let folder = staging.path().to_path_buf();
        let theirs = folder.clone();
        AS_TAKEN.set(Some(Box::new(move || {
            let at = theirs.join(LOCK);
            let lock = crate::fs::file::open_regular(&at).unwrap();
            lock.try_lock().unwrap();
            std::fs::remove_file(&at).unwrap();
            drop(lock);
        })));
        assert!(!staging.hold_again(), "theirs");
        drop(staging);
        assert!(folder.is_dir(), "left to their sweep");
        remove_tree(&folder).unwrap();
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
