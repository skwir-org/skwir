//! The `path` scheme: stream a directory with getdents, stat lazily, watch for changes.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::Receiver;
use parking_lot::Mutex;
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
    dir: PathBuf,
    dirfd: OwnedFd,
    dev: u64,
    names: Mutex<HashMap<EntryId, OsString>>,
    tx: EventSink,
    ctx: OpenContext,
    _watcher: Mutex<Option<watch::WatchHandle>>,
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
        FileType::Unknown => EntryKind::Unknown,
        _ => EntryKind::Other,
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
                        dir,
                        dirfd,
                        dev: st.st_dev,
                        names: Mutex::new(HashMap::new()),
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
                        std::thread::Builder::new()
                            .name("bibli-watch-apply".into())
                            .spawn(move || {
                                for change in changes {
                                    if s.ctx.cancel.is_cancelled() {
                                        break;
                                    }
                                    s.apply_changes(change);
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
    /// Stream the directory in batches. `previous` (on refresh) lets us emit
    /// `Removed` for entries that disappeared.
    fn list(&self, cancel: &CancelToken, previous: Option<Vec<EntryId>>) {
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
            let id = EntryId {
                dev: self.dev,
                ino: ent.ino(),
            };
            self.names.lock().insert(id, name.clone());
            seen.push(id);
            batch.push(Entry::new(id, name, kind_of(ent.file_type())));
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
        if let Some(prev) = previous {
            let seen: std::collections::HashSet<EntryId> = seen.into_iter().collect();
            let gone: Vec<EntryId> = prev.into_iter().filter(|id| !seen.contains(id)).collect();
            if !gone.is_empty() {
                let mut names = self.names.lock();
                for id in &gone {
                    names.remove(id);
                }
                drop(names);
                self.tx.send(ListingEvent::Removed(gone));
            }
        }
        self.tx.send(ListingEvent::Complete);
    }

    fn hydrate(&self, ids: &[EntryId]) {
        let mut updated = Vec::with_capacity(ids.len());
        let mut gone = Vec::new();
        for &id in ids {
            if self.ctx.cancel.is_cancelled() {
                return;
            }
            let Some(name) = self.names.lock().get(&id).cloned() else {
                continue;
            };
            let mut e = Entry::new(id, name, EntryKind::Unknown);
            if stat_entry(&self.dirfd, &mut e) {
                // statx may report a different id for a mount point; keep the listing's.
                e.id = id;
                updated.push(e);
            } else {
                gone.push(id);
            }
        }
        if !updated.is_empty() {
            self.ctx.metadata.fill(&self.dir, &mut updated);
            self.tx.send(ListingEvent::Updated(updated));
        }
        if !gone.is_empty() {
            let mut names = self.names.lock();
            for id in &gone {
                names.remove(id);
            }
            drop(names);
            self.tx.send(ListingEvent::Removed(gone));
        }
    }

    fn apply_changes(&self, c: watch::Changes) {
        if c.rescan {
            let prev: Vec<EntryId> = self.names.lock().keys().copied().collect();
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
            if stat_entry(&self.dirfd, &mut e) {
                let mut names = self.names.lock();
                // A rename onto this name may leave a stale id behind under the old name.
                let stale: Vec<EntryId> = names
                    .iter()
                    .filter(|(id, n)| **n == name && **id != e.id)
                    .map(|(id, _)| *id)
                    .collect();
                for id in stale {
                    names.remove(&id);
                    removed.push(id);
                }
                names.insert(e.id, name);
                added.push(e);
            } else {
                let mut names = self.names.lock();
                let gone: Vec<EntryId> = names
                    .iter()
                    .filter(|(_, n)| **n == name)
                    .map(|(id, _)| *id)
                    .collect();
                for id in gone {
                    names.remove(&id);
                    removed.push(id);
                }
            }
        }
        if !removed.is_empty() {
            self.tx.send(ListingEvent::Removed(removed));
        }
        if !added.is_empty() {
            // Changed entries arrive stat-ed; give providers their turn so
            // fields such as tags survive the update.
            self.ctx.metadata.fill(&self.dir, &mut added);
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
        let name = shared.names.lock().get(&id).cloned()?;
        Some(shared.dir.join(name))
    }

    fn refresh(&self) {
        let previous = self
            .shared
            .lock()
            .as_ref()
            .map(|s| s.names.lock().keys().copied().collect());
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
