//! The `path` scheme: stream a directory with getdents, stat lazily, watch for changes.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::Receiver;
use parking_lot::{Mutex, RwLock};
use rustix::fd::OwnedFd;
use rustix::fs::{Dir, FileType, Mode, OFlags};

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId, EntryKind};
use crate::location::{
    EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation, PATH_SCHEME,
};
use crate::scheduler::Priority;

use super::stat::stat_entry;
use super::watch;

/// Provider for filesystem paths.
#[derive(Debug, Default)]
pub struct PathProvider;

impl LocationProvider for PathProvider {
    fn scheme(&self) -> &str {
        PATH_SCHEME
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let path = location
            .path()
            .ok_or_else(|| anyhow::anyhow!("not a path: {location}"))?;
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()?.join(path)
        };
        Ok(PathLocation::open(location.clone(), path, ctx))
    }
}

struct Shared {
    /// Where the directory is now: it follows the directory when it moves.
    dir: RwLock<PathBuf>,
    dirfd: OwnedFd,
    dev: u64,
    names: Mutex<Names>,
    tx: EventSink,
    ctx: OpenContext,
    _watcher: Mutex<Option<watch::WatchHandle>>,
}

/// The names of the listed entries, both ways, so a watcher event finds its
/// entry without walking the directory.
///
/// Identity is what statx reports (`fs::stat::id_of`, which operations
/// resolve against). A listing streams before it stats, so it names an
/// entry by the directory's device and `d_ino`; the two differ only for a
/// mount point or a subvolume root. Hydration finds out, re-keys the entry
/// (removed under the provisional id, added under the real one) and keeps
/// an alias, so a later re-listing uses the real id straight away.
#[derive(Default)]
struct Names {
    by_id: HashMap<EntryId, OsString>,
    by_name: HashMap<OsString, EntryId>,
    /// Provisional (device, `d_ino`) ids to the id statx reports.
    aliases: HashMap<EntryId, EntryId>,
}

impl Names {
    /// Record `name` under `id`. Returns the id the name belonged to
    /// before, which is now stale.
    fn insert(&mut self, id: EntryId, name: OsString) -> Option<EntryId> {
        if let Some(old) = self.by_id.insert(id, name.clone())
            && old != name
            && self.by_name.get(&old) == Some(&id)
        {
            self.by_name.remove(&old);
        }
        match self.by_name.insert(name, id) {
            Some(prev) if prev != id => {
                self.by_id.remove(&prev);
                Some(prev)
            }
            _ => None,
        }
    }

    fn remove_id(&mut self, id: EntryId) -> Option<OsString> {
        let name = self.by_id.remove(&id)?;
        self.by_name.remove(&name);
        Some(name)
    }

    fn remove_name(&mut self, name: &OsStr) -> Option<EntryId> {
        let id = self.by_name.remove(name)?;
        self.by_id.remove(&id);
        Some(id)
    }

    /// The id a directory entry is listed under.
    fn listing_id(&self, provisional: EntryId) -> EntryId {
        self.aliases
            .get(&provisional)
            .copied()
            .unwrap_or(provisional)
    }

    /// `old` turned out to be `new` when stat-ed.
    fn rekey(&mut self, old: EntryId, new: EntryId) {
        if let Some(name) = self.remove_id(old) {
            self.insert(new, name);
        }
        let provisional = self
            .aliases
            .iter()
            .find(|(_, to)| **to == old)
            .map_or(old, |(from, _)| *from);
        if provisional == new {
            self.aliases.remove(&provisional);
        } else {
            self.aliases.insert(provisional, new);
        }
    }

    fn ids(&self) -> Vec<EntryId> {
        self.by_id.keys().copied().collect()
    }
}

/// An open directory.
pub struct PathLocation {
    location: Location,
    dir: PathBuf,
    ctx: OpenContext,
    tx: EventSink,
    shared: Arc<Mutex<Option<Arc<Shared>>>>,
    opening: Arc<AtomicBool>,
    rx: Receiver<ListingEvent>,
}

fn open_dirfd(path: &Path) -> rustix::io::Result<OwnedFd> {
    rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
}

fn kind_of(t: FileType) -> EntryKind {
    match t {
        FileType::RegularFile => EntryKind::File,
        FileType::Directory => EntryKind::Directory,
        FileType::Symlink => EntryKind::Symlink,
        FileType::Fifo => EntryKind::Fifo,
        FileType::Socket => EntryKind::Socket,
        FileType::CharacterDevice => EntryKind::CharDevice,
        FileType::BlockDevice => EntryKind::BlockDevice,
        FileType::Unknown => EntryKind::Unknown,
    }
}

impl PathLocation {
    fn open(location: Location, dir: PathBuf, ctx: OpenContext) -> Arc<Self> {
        let (tx, rx) = ctx.channel();
        let this = Arc::new(Self {
            location,
            dir,
            ctx,
            tx,
            rx,
            shared: Arc::new(Mutex::new(None)),
            opening: Arc::new(AtomicBool::new(false)),
        });
        this.start_open();
        this
    }

    fn start_open(&self) {
        if self.ctx.cancel.is_cancelled() || self.opening.swap(true, Ordering::AcqRel) {
            return;
        }
        let dir = self.dir.clone();
        let ctx = self.ctx.clone();
        let tx = self.tx.clone();
        let slot = Arc::clone(&self.shared);
        let opening = Arc::clone(&self.opening);
        self.ctx.scheduler.spawn(
            Priority::Visible,
            "fs.open",
            ctx.cancel.clone(),
            move || {
                let result = (|| -> anyhow::Result<()> {
                    // Opening/stat-ing a sleeping disk and installing a watch are I/O.
                    // Publish the handle first, then stream entries on this worker.
                    let dirfd = open_dirfd(&dir)?;
                    if ctx.cancel.is_cancelled() {
                        return Ok(());
                    }
                    let st = rustix::fs::fstat(&dirfd)?;
                    if ctx.cancel.is_cancelled() {
                        return Ok(());
                    }
                    let (watcher, changes) =
                        match watch::watch(&dir, Duration::from_millis(50), &ctx.cancel) {
                            Ok((watcher, changes)) => (Some(watcher), Some(changes)),
                            Err(error) => {
                                tracing::warn!("watch unavailable: {error}");
                                (None, None)
                            }
                        };
                    let shared = Arc::new(Shared {
                        dir: RwLock::new(dir),
                        dirfd,
                        dev: st.st_dev,
                        names: Mutex::new(Names::default()),
                        tx: tx.clone(),
                        ctx: ctx.clone(),
                        _watcher: Mutex::new(watcher),
                    });
                    {
                        let mut slot = slot.lock();
                        if ctx.cancel.is_cancelled() {
                            return Ok(());
                        }
                        *slot = Some(Arc::clone(&shared));
                    }
                    if let Some(changes) = changes {
                        let s = Arc::clone(&shared);
                        // Closing the location wakes this thread, which then
                        // lets go of the watcher: nothing here polls.
                        let closed = ctx.cancel.subscribe();
                        std::thread::Builder::new()
                            .name("bibli-watch-apply".into())
                            .spawn(move || {
                                loop {
                                    crossbeam_channel::select! {
                                        recv(changes) -> change => match change {
                                            Ok(change) => s.apply_changes(change),
                                            Err(_) => break,
                                        },
                                        recv(closed.receiver()) -> _ => break,
                                    }
                                }
                            })?;
                    }
                    shared.list(&ctx.cancel, None);
                    Ok(())
                })();
                opening.store(false, Ordering::Release);
                if !ctx.cancel.is_cancelled()
                    && let Err(error) = result
                {
                    tx.send(ListingEvent::Error(error.to_string()));
                }
            },
        );
    }

    fn start_listing(&self, previous: Option<Vec<EntryId>>) {
        let Some(s) = self.shared.lock().clone() else {
            self.start_open();
            return;
        };
        let cancel = self.ctx.cancel.child();
        self.ctx
            .scheduler
            .spawn(Priority::Visible, "fs.list", cancel.clone(), move || {
                s.list(&cancel, previous);
            });
    }
}

impl Shared {
    fn dir(&self) -> PathBuf {
        self.dir.read().clone()
    }

    /// Stream the directory in batches. `previous` (on refresh) lets us emit
    /// `Removed` for entries that disappeared; the entries that stayed are
    /// stat-ed again afterwards, since the listing keeps their old metadata.
    fn list(self: &Arc<Self>, cancel: &CancelToken, previous: Option<Vec<EntryId>>) {
        if cancel.is_cancelled() {
            return;
        }
        let mut dir = match Dir::read_from(&self.dirfd) {
            Ok(d) => d,
            Err(e) => {
                self.tx.send(ListingEvent::Error(e.to_string()));
                return;
            }
        };
        let mut batch = Vec::with_capacity(self.ctx.batch_size);
        let mut seen: Vec<EntryId> = Vec::new();
        while !cancel.is_cancelled() {
            let Some(ent) = dir.next() else {
                break;
            };
            if cancel.is_cancelled() {
                return;
            }
            let ent = match ent {
                Ok(e) => e,
                Err(e) => {
                    self.tx.send(ListingEvent::Error(e.to_string()));
                    return;
                }
            };
            let name = ent.file_name();
            if name == c"." || name == c".." {
                continue;
            }
            let name: OsString =
                std::os::unix::ffi::OsStringExt::from_vec(name.to_bytes().to_vec());
            let id = {
                let mut names = self.names.lock();
                let id = names.listing_id(EntryId {
                    dev: self.dev,
                    ino: ent.ino(),
                });
                names.insert(id, name.clone());
                id
            };
            seen.push(id);
            let mut entry = Entry::new(id, name, kind_of(ent.file_type()));
            // A link is followed now, not at hydration: whether it leads to
            // a folder decides where it sorts, and rows off screen are not
            // hydrated.
            if entry.is_link() {
                entry.target = crate::fs::stat::follow(&self.dirfd, entry.name.as_os_str());
            }
            batch.push(entry);
            if batch.len() >= self.ctx.batch_size {
                let full = std::mem::replace(&mut batch, Vec::with_capacity(self.ctx.batch_size));
                if !self.tx.send(ListingEvent::Added(full)) {
                    return;
                }
            }
        }
        if cancel.is_cancelled() {
            return;
        }
        if !batch.is_empty() && !self.tx.send(ListingEvent::Added(batch)) {
            return;
        }
        let mut stayed = Vec::new();
        if let Some(prev) = previous {
            let seen: HashSet<EntryId> = seen.into_iter().collect();
            let (kept, gone): (Vec<EntryId>, Vec<EntryId>) =
                prev.into_iter().partition(|id| seen.contains(id));
            stayed = kept;
            if !gone.is_empty() {
                let mut names = self.names.lock();
                for id in &gone {
                    names.remove_id(*id);
                }
                drop(names);
                self.tx.send(ListingEvent::Removed(gone));
            }
        }
        self.tx.send(ListingEvent::Complete);
        if !stayed.is_empty() {
            let s = Arc::clone(self);
            self.ctx.scheduler.spawn(
                Priority::Background,
                "fs.restat",
                cancel.clone(),
                move || {
                    for chunk in stayed.chunks(s.ctx.batch_size.max(1)) {
                        if s.ctx.cancel.is_cancelled() {
                            return;
                        }
                        s.hydrate(chunk);
                    }
                },
            );
        }
    }

    fn hydrate(&self, ids: &[EntryId]) {
        let mut stated = Vec::with_capacity(ids.len());
        let mut gone = Vec::new();
        for &id in ids {
            if self.ctx.cancel.is_cancelled() {
                return;
            }
            let Some(name) = self.names.lock().by_id.get(&id).cloned() else {
                continue;
            };
            let mut e = Entry::new(id, name, EntryKind::Unknown);
            if stat_entry(&self.dirfd, &mut e) {
                stated.push((id, e));
            } else {
                gone.push(id);
            }
        }
        let (mut updated, mut rekeyed): (Vec<Entry>, Vec<Entry>) = (Vec::new(), Vec::new());
        let mut stale = Vec::new();
        {
            let mut names = self.names.lock();
            for (id, e) in stated {
                if e.id == id {
                    updated.push(e);
                } else if names.by_id.contains_key(&id) {
                    names.rekey(id, e.id);
                    stale.push(id);
                    rekeyed.push(e);
                }
            }
            for id in &gone {
                names.remove_id(*id);
            }
        }
        let dir = self.dir();
        if !updated.is_empty() {
            self.ctx.metadata.fill(&dir, &mut updated);
            self.tx.send(ListingEvent::Updated(updated));
        }
        gone.extend(stale);
        if !gone.is_empty() {
            self.tx.send(ListingEvent::Removed(gone));
        }
        if !rekeyed.is_empty() {
            self.ctx.metadata.fill(&dir, &mut rekeyed);
            self.tx.send(ListingEvent::Added(rekeyed));
        }
    }

    /// The directory itself moved: find where through its descriptor, which
    /// followed it. A directory that is gone empties the listing.
    fn follow_self(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let link = format!("/proc/self/fd/{}", self.dirfd.as_raw_fd());
            if let Ok(now) = std::fs::read_link(link)
                && now.is_absolute()
                && !now.as_os_str().as_encoded_bytes().ends_with(b" (deleted)")
            {
                *self.dir.write() = now;
                return true;
            }
            false
        }
        #[cfg(not(target_os = "linux"))]
        false
    }

    /// Whether the directory was removed. inotify cannot say so while we
    /// hold it open (the inode outlives its name until we close it), but a
    /// removed directory has no links left; its children's removal is the
    /// event that brings us here.
    fn is_unlinked(&self) -> bool {
        rustix::fs::fstat(&self.dirfd).is_ok_and(|st| st.st_nlink == 0)
    }

    fn lose_self(&self) {
        let ids = std::mem::take(&mut *self.names.lock()).ids();
        if !ids.is_empty() {
            self.tx.send(ListingEvent::Removed(ids));
        }
        self.tx.send(ListingEvent::Error(
            "This folder was deleted or is no longer available.".into(),
        ));
    }

    fn apply_changes(self: &Arc<Self>, c: watch::Changes) {
        if c.deleted || (c.moved && !self.follow_self()) || self.is_unlinked() {
            self.lose_self();
            return;
        }
        if c.itself {
            self.tx.send(ListingEvent::Changed);
        }
        if c.rescan {
            let prev = self.names.lock().ids();
            self.list(&self.ctx.cancel.child(), Some(prev));
            return;
        }
        let mut added = Vec::new();
        let mut removed = Vec::new();
        for name in c.names {
            if self.ctx.cancel.is_cancelled() {
                return;
            }
            let mut e = Entry::new(EntryId { dev: 0, ino: 0 }, name.clone(), EntryKind::Unknown);
            let exists = stat_entry(&self.dirfd, &mut e);
            let mut names = self.names.lock();
            if exists {
                // A rename onto this name leaves the id it replaced stale.
                if let Some(stale) = names.insert(e.id, name) {
                    removed.push(stale);
                }
                added.push(e);
            } else if let Some(id) = names.remove_name(&name) {
                removed.push(id);
            }
        }
        // An entry renamed within the folder is gone under its old name and
        // back under its new one: it is an update, not a removal, so it keeps
        // its place and its selection.
        let arrived: HashSet<EntryId> = added.iter().map(|e| e.id).collect();
        removed.retain(|id| !arrived.contains(id));
        if !removed.is_empty() {
            self.tx.send(ListingEvent::Removed(removed));
        }
        if !added.is_empty() {
            // Changed entries arrive stat-ed; give providers their turn so
            // fields such as tags survive the update.
            self.ctx.metadata.fill(&self.dir(), &mut added);
            self.tx.send(ListingEvent::Added(added));
        }
    }
}

impl OpenLocation for PathLocation {
    fn location(&self) -> &Location {
        &self.location
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }

    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        if ids.is_empty() {
            return;
        }
        let Some(s) = self.shared.lock().clone() else {
            return;
        };
        let ids = ids.to_vec();
        self.ctx
            .scheduler
            .spawn(priority, "fs.stat", self.ctx.cancel.child(), move || {
                s.hydrate(&ids);
            });
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        let shared = self.shared.lock().clone()?;
        let name = shared.names.lock().by_id.get(&id).cloned()?;
        Some(shared.dir().join(name))
    }

    fn refresh(&self) {
        let previous = self.shared.lock().as_ref().map(|s| s.names.lock().ids());
        self.start_listing(previous);
    }

    fn close(&self) {
        self.ctx.cancel.cancel();
        if let Some(shared) = self.shared.lock().take() {
            // Native watcher and directory descriptor cleanup stays off the UI.
            self.ctx.scheduler.spawn(
                Priority::Background,
                "fs.close",
                CancelToken::new(),
                move || drop(shared),
            );
        }
    }
}

impl Drop for PathLocation {
    fn drop(&mut self) {
        self.close();
    }
}
