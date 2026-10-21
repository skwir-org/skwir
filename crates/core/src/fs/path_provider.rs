//! The `path` scheme: stream a directory, stat lazily, watch for changes.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::Receiver;
use parking_lot::{Mutex, RwLock};

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId, EntryKind, FileId};
use crate::hashing;
use crate::location::{
    EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation, PATH_SCHEME,
};
use crate::scheduler::{Lane, Priority};

use super::dir::Dir;
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
    /// The directory held open: names are stat-ed relative to it.
    folder: Dir,
    /// The folder's own file: with each name, what its entries are told
    /// apart by (`EntryId::in_folder`); its device, theirs until stat-ed.
    id: FileId,
    names: Mutex<Names>,
    /// Every name of the folder is in `names`: a batch can ask them.
    listed: std::sync::atomic::AtomicBool,
    tx: EventSink,
    ctx: OpenContext,
    watcher: Mutex<Option<watch::WatchHandle>>,
    /// Where its work runs: its remote's lane, told by the folder's path or
    /// its device once opened (ADR-077), the shared one on this machine.
    lane: Lane,
    /// Changes the watcher told and not applied yet, and whether a worker
    /// applies them: a worker at a time, changes told meanwhile taken as
    /// one when it is done with the ones before.
    changes: Mutex<(Option<watch::Changes>, bool)>,
}

/// The names of the listed entries, both ways, so a watcher event finds its
/// entry without walking the directory.
///
/// An entry is its file under its name in this folder
/// (`EntryId::in_folder`, ADR-049): two names of one file are two. Its
/// file is what a stat reports (`fs::stat::id_of`, which operations
/// resolve against). A listing streams before it stats, so it names an
/// entry's file by the directory's device and `d_ino`; the two differ only
/// for a mount point or a subvolume root. Hydration finds out, re-keys the
/// entry (removed under the provisional id, added under the real one) and
/// keeps an alias, so a later re-listing uses the real id straight away.
#[derive(Default)]
struct Names {
    by_id: hashing::Map<EntryId, OsString>,
    by_name: hashing::Map<OsString, EntryId>,
    /// Provisional (device, `d_ino`) ids to the id a stat reports.
    aliases: hashing::Map<EntryId, EntryId>,
    /// The entries stat-ed since they were listed: what a listing again
    /// (F5, a watcher's rescan) stats again, its metadata kept meanwhile.
    /// The others were never asked for, and a refresh does not read the
    /// whole folder for them.
    stated: hashing::Set<EntryId>,
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
        self.stated.remove(&id);
        let name = self.by_id.remove(&id)?;
        self.by_name.remove(&name);
        Some(name)
    }

    fn remove_name(&mut self, name: &OsStr) -> Option<EntryId> {
        let id = self.by_name.remove(name)?;
        self.by_id.remove(&id);
        self.stated.remove(&id);
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
        // A remote that stopped answering holds its own lane (ADR-077):
        // told by the path here, by the folder's device once opened.
        let lane = super::reach::lane(&dir);
        self.ctx.scheduler.spawn_in(
            lane,
            Priority::Visible,
            "fs.open",
            ctx.cancel.clone(),
            move || {
                let result = (|| -> anyhow::Result<()> {
                    // Opening/stat-ing a sleeping disk and installing a watch are I/O.
                    // Publish the handle first, then stream entries on this worker.
                    let folder = Dir::open(&dir)?;
                    if ctx.cancel.is_cancelled() {
                        return Ok(());
                    }
                    let id = folder.identity()?;
                    if ctx.cancel.is_cancelled() {
                        return Ok(());
                    }
                    let lane = if lane.is_shared() {
                        super::reach::of_device(id.dev).map_or(lane, |remote| remote.lane())
                    } else {
                        lane
                    };
                    let shared = Arc::new(Shared {
                        dir: RwLock::new(dir.clone()),
                        folder,
                        id,
                        names: Mutex::new(Names::default()),
                        listed: std::sync::atomic::AtomicBool::new(false),
                        tx: tx.clone(),
                        ctx: ctx.clone(),
                        watcher: Mutex::new(None),
                        lane,
                        changes: Mutex::new((None, false)),
                    });
                    // Watched before it is listed, so nothing changed
                    // meanwhile is missed. The watcher holds the listing
                    // weakly: closing it lets go of both.
                    let weak = Arc::downgrade(&shared);
                    match watch::watch(&dir, Duration::from_millis(50), &ctx.cancel, move |c| {
                        if let Some(s) = weak.upgrade() {
                            s.changed(c);
                        }
                    }) {
                        Ok(watcher) => *shared.watcher.lock() = Some(watcher),
                        Err(error) => tracing::warn!("watch unavailable: {error}"),
                    }
                    {
                        let mut slot = slot.lock();
                        if ctx.cancel.is_cancelled() {
                            return Ok(());
                        }
                        *slot = Some(Arc::clone(&shared));
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
        self.ctx.scheduler.spawn_in(
            s.lane,
            Priority::Visible,
            "fs.list",
            cancel.clone(),
            move || {
                s.list(&cancel, previous);
            },
        );
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
        let mut dir = match self.folder.read() {
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
            let id = {
                let mut names = self.names.lock();
                // Provisional: the stat names it (`stat_entry`).
                let file = FileId {
                    ino: ent.ino,
                    ..self.id
                };
                let id = names.listing_id(EntryId::in_folder(file, self.id, &ent.name));
                names.insert(id, ent.name.clone());
                id
            };
            seen.push(id);
            let mut entry = Entry::new(id, ent.name, ent.kind);
            entry.hidden = ent.hidden;
            // A link is followed now, not at hydration: whether it leads to
            // a folder decides where it sorts, and rows off screen are not
            // hydrated.
            if entry.is_link() {
                entry.target = crate::fs::stat::follow(&self.folder, entry.name.as_os_str());
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
            let seen: hashing::Set<EntryId> = seen.into_iter().collect();
            let (kept, gone): (Vec<EntryId>, Vec<EntryId>) =
                prev.into_iter().partition(|id| seen.contains(id));
            // What was stat-ed is stat-ed again; the rest still waits to be
            // asked for.
            let names = self.names.lock();
            stayed = kept
                .into_iter()
                .filter(|id| names.stated.contains(id))
                .collect();
            drop(names);
            if !gone.is_empty() {
                let mut names = self.names.lock();
                for id in &gone {
                    names.remove_id(*id);
                }
                drop(names);
                self.tx.send(ListingEvent::Removed(gone));
            }
        }
        self.listed
            .store(true, std::sync::atomic::Ordering::Release);
        self.tx.send(ListingEvent::Complete);
        if !stayed.is_empty() {
            let s = Arc::clone(self);
            self.ctx.scheduler.spawn_in(
                self.lane,
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

    /// Run the metadata providers on a batch, with the folder's names once
    /// they are all known.
    fn fill(&self, dir: &Path, entries: &mut [Entry]) {
        if self.listed.load(std::sync::atomic::Ordering::Acquire) {
            let listed = |name: &std::ffi::OsStr| self.names.lock().by_name.contains_key(name);
            self.ctx.metadata.fill_listed(dir, &listed, entries);
        } else {
            self.ctx.metadata.fill(dir, entries);
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
            if stat_entry(&self.folder, &mut e) {
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
                    names.stated.insert(id);
                    updated.push(e);
                } else if names.by_id.contains_key(&id) {
                    names.rekey(id, e.id);
                    names.stated.insert(e.id);
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
            self.fill(&dir, &mut updated);
            self.tx.send(ListingEvent::Updated(updated));
        }
        gone.extend(stale);
        if !gone.is_empty() {
            self.tx.send(ListingEvent::Removed(gone));
        }
        if !rekeyed.is_empty() {
            self.fill(&dir, &mut rekeyed);
            self.tx.send(ListingEvent::Added(rekeyed));
        }
    }

    /// The directory itself moved: find where through its descriptor, which
    /// followed it. A directory that is gone empties the listing.
    fn follow_self(&self) -> bool {
        let Some(now) = self.folder.path_now() else {
            return false;
        };
        *self.dir.write() = now;
        true
    }

    /// Whether the directory was removed. inotify cannot say so while we
    /// hold it open (the inode outlives its name until we close it), but a
    /// removed directory has no links left; its children's removal is the
    /// event that brings us here.
    fn is_unlinked(&self) -> bool {
        self.folder.is_unlinked()
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

    /// Changes the watcher told, on its thread: handed to a worker.
    fn changed(self: &Arc<Self>, c: watch::Changes) {
        {
            let mut changes = self.changes.lock();
            match &mut changes.0 {
                Some(before) => before.merge(c),
                None => changes.0 = Some(c),
            }
            if std::mem::replace(&mut changes.1, true) {
                return;
            }
        }
        let s = Arc::clone(self);
        self.ctx.scheduler.spawn_in(
            self.lane,
            Priority::Visible,
            "fs.changes",
            self.ctx.cancel.clone(),
            move || {
                loop {
                    let c = {
                        let mut changes = s.changes.lock();
                        let Some(c) = changes.0.take() else {
                            changes.1 = false;
                            return;
                        };
                        c
                    };
                    s.apply_changes(c);
                }
            },
        );
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
        // A companion keeps its file's notes (ADR-028: a Mac's `._x`, a
        // sidecar): when it changes, the file's notes did.
        let mut names = c.names;
        let owners: Vec<std::ffi::OsString> = names
            .iter()
            .filter_map(|n| self.ctx.companions.owner(n))
            .collect();
        names.extend(owners);
        for name in names {
            if self.ctx.cancel.is_cancelled() {
                return;
            }
            let mut e = Entry::new(EntryId::UNKNOWN, name.clone(), EntryKind::Unknown);
            let exists = stat_entry(&self.folder, &mut e);
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
        let arrived: hashing::Set<EntryId> = added.iter().map(|e| e.id).collect();
        removed.retain(|id| !arrived.contains(id));
        // A file renamed within the folder is gone under its old name and
        // come under its new one: a rename, so its row keeps its place and
        // its selection.
        let mut renamed = renames(&mut removed, &mut added);
        if !removed.is_empty() {
            self.tx.send(ListingEvent::Removed(removed));
        }
        // Changed entries arrive stat-ed; give providers their turn so
        // fields such as tags survive the update.
        if !renamed.is_empty() {
            let (olds, mut news): (Vec<EntryId>, Vec<Entry>) = renamed.drain(..).unzip();
            self.fill(&self.dir(), &mut news);
            self.tx
                .send(ListingEvent::Renamed(olds.into_iter().zip(news).collect()));
        }
        if !added.is_empty() {
            self.fill(&self.dir(), &mut added);
            self.tx.send(ListingEvent::Added(added));
        }
    }
}

/// The renames among a batch's changes, taken out of `removed` and
/// `added`: a file one of whose names went and one came. A file that lost
/// or gained several names at once is left as it is: which name became
/// which cannot be told.
fn renames(removed: &mut Vec<EntryId>, added: &mut Vec<Entry>) -> Vec<(EntryId, Entry)> {
    let count =
        |files: &mut hashing::Map<FileId, usize>, file| *files.entry(file).or_default() += 1;
    let (mut gone, mut came) = (hashing::Map::default(), hashing::Map::default());
    for id in removed.iter() {
        count(&mut gone, id.file);
    }
    for e in added.iter() {
        count(&mut came, e.file());
    }
    let one = |files: &hashing::Map<FileId, usize>, file| files.get(&file) == Some(&1);
    let mut renamed = Vec::new();
    let mut i = 0;
    while i < added.len() {
        let file = added[i].file();
        if one(&came, file)
            && one(&gone, file)
            && let Some(at) = removed.iter().position(|id| id.file == file)
        {
            renamed.push((removed.swap_remove(at), added.swap_remove(i)));
        } else {
            i += 1;
        }
    }
    renamed
}

impl OpenLocation for PathLocation {
    fn location(&self) -> &Location {
        &self.location
    }

    fn holds(&self) -> Option<&Path> {
        Some(&self.dir)
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }

    /// A job per batch: a request for a whole folder (a sort by size asks
    /// for every entry) is stat-ed by several workers at once, and its
    /// answers arrive a batch at a time, each an event the UI applies
    /// within a frame, not one holding them all.
    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        if ids.is_empty() {
            return;
        }
        let Some(s) = self.shared.lock().clone() else {
            return;
        };
        for chunk in ids.chunks(self.ctx.batch_size.max(1)) {
            let (s, ids) = (Arc::clone(&s), chunk.to_vec());
            self.ctx.scheduler.spawn_in(
                s.lane,
                priority,
                "fs.stat",
                self.ctx.cancel.child(),
                move || {
                    s.hydrate(&ids);
                },
            );
        }
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
            // Native watcher and directory descriptor cleanup stays off the
            // UI, and out of the remote's lane: a server that stopped
            // answering holds its lane, and closing reads nothing of it.
            self.ctx.scheduler.spawn_in(
                Lane::SHARED,
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
