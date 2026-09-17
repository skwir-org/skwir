//! The `path` scheme: stream a directory with getdents, stat lazily, watch for changes.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use rustix::fd::OwnedFd;
use rustix::fs::{Dir, FileType, Mode, OFlags};

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId, EntryKind};
use crate::location::{
    ListingEvent, Location, LocationProvider, OpenContext, OpenLocation, PATH_SCHEME,
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
        PathLocation::open(location.clone(), path, ctx)
    }
}

struct Shared {
    dir: PathBuf,
    dirfd: OwnedFd,
    dev: u64,
    names: Mutex<HashMap<EntryId, OsString>>,
    tx: Sender<ListingEvent>,
    ctx: OpenContext,
}

/// An open directory.
pub struct PathLocation {
    location: Location,
    shared: Arc<Shared>,
    rx: Receiver<ListingEvent>,
    watcher: Mutex<Option<watch::WatchHandle>>,
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
    fn open(
        location: Location,
        dir: PathBuf,
        ctx: OpenContext,
    ) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let dirfd = open_dirfd(&dir)?;
        let st = rustix::fs::fstat(&dirfd)?;
        let (tx, rx) = unbounded();
        let shared = Arc::new(Shared {
            dir,
            dirfd,
            dev: st.st_dev,
            names: Mutex::new(HashMap::new()),
            tx,
            ctx,
        });
        let watcher = match watch::watch(&shared.dir, Duration::from_millis(50), &shared.ctx.cancel)
        {
            Ok((w, changes)) => {
                let s = Arc::clone(&shared);
                std::thread::Builder::new()
                    .name("bibli-watch-apply".into())
                    .spawn(move || {
                        for c in changes {
                            if s.ctx.cancel.is_cancelled() {
                                break;
                            }
                            s.apply_changes(c);
                        }
                    })
                    .expect("spawn watch apply thread");
                Some(w)
            }
            Err(e) => {
                tracing::warn!(dir = %shared.dir.display(), "watch unavailable: {e}");
                None
            }
        };
        let this = Arc::new(PathLocation {
            location,
            shared,
            rx,
            watcher: Mutex::new(watcher),
        });
        this.start_listing(None);
        Ok(this)
    }

    fn start_listing(&self, previous: Option<Vec<EntryId>>) {
        let s = Arc::clone(&self.shared);
        let cancel = s.ctx.cancel.child();
        self.shared
            .ctx
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
        let dir = match Dir::read_from(&self.dirfd) {
            Ok(d) => d,
            Err(e) => {
                let _ = self.tx.send(ListingEvent::Error(e.to_string()));
                return;
            }
        };
        let mut batch = Vec::with_capacity(self.ctx.batch_size);
        let mut seen: Vec<EntryId> = Vec::new();
        for ent in dir {
            if cancel.is_cancelled() {
                return;
            }
            let ent = match ent {
                Ok(e) => e,
                Err(e) => {
                    let _ = self.tx.send(ListingEvent::Error(e.to_string()));
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
            if batch.len() >= self.ctx.batch_size
                && self
                    .tx
                    .send(ListingEvent::Added(std::mem::replace(
                        &mut batch,
                        Vec::with_capacity(self.ctx.batch_size),
                    )))
                    .is_err()
            {
                return;
            }
        }
        if !batch.is_empty() && self.tx.send(ListingEvent::Added(batch)).is_err() {
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
                let _ = self.tx.send(ListingEvent::Removed(gone));
            }
        }
        let _ = self.tx.send(ListingEvent::Complete);
    }

    fn hydrate(&self, ids: &[EntryId]) {
        let mut updated = Vec::with_capacity(ids.len());
        let mut gone = Vec::new();
        for &id in ids {
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
            let _ = self.tx.send(ListingEvent::Updated(updated));
        }
        if !gone.is_empty() {
            let mut names = self.names.lock();
            for id in &gone {
                names.remove(id);
            }
            drop(names);
            let _ = self.tx.send(ListingEvent::Removed(gone));
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
            let _ = self.tx.send(ListingEvent::Removed(removed));
        }
        if !added.is_empty() {
            let _ = self.tx.send(ListingEvent::Added(added));
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
        let s = Arc::clone(&self.shared);
        let ids = ids.to_vec();
        self.shared
            .ctx
            .scheduler
            .spawn(priority, "fs.stat", s.ctx.cancel.child(), move || {
                s.hydrate(&ids);
            });
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.shared
            .names
            .lock()
            .get(&id)
            .map(|n| self.shared.dir.join(n))
    }

    fn refresh(&self) {
        let prev: Vec<EntryId> = self.shared.names.lock().keys().copied().collect();
        self.start_listing(Some(prev));
    }

    fn close(&self) {
        self.shared.ctx.cancel.cancel();
        self.watcher.lock().take();
    }
}

impl Drop for PathLocation {
    fn drop(&mut self) {
        self.close();
    }
}
