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
use super::stat::{Stated, stat_entry_as};
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
        // Never the process's working folder: a relative path is its
        // caller's to read from the folder it was typed in
        // (`App::navigate`).
        let path = crate::os_path::absolute(&path)?.to_path_buf();
        Ok(PathLocation::open(location.clone(), path, ctx))
    }
}

struct Shared {
    /// Counted among the folders held open until it goes ([`wait_let_go`]).
    _held: Held,
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
/// entry (renamed from the provisional id to the real one, so its row
/// keeps its selection) and keeps an alias, so a later re-listing uses the
/// real id straight away.
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

/// What a listing keeps of `previous` (the ids noted when it was asked
/// for, on a refresh) and what is gone: those it did not see, and every id
/// a name it listed belonged to before (`replaced`), which no other name
/// holds now. One name, one row: an id the watcher added after `previous`
/// was noted, its file numbered anew by its FUSE program since, would
/// otherwise stay beside the new one.
fn gone_after(
    previous: Option<Vec<EntryId>>,
    seen: &[EntryId],
    replaced: Vec<EntryId>,
) -> (Vec<EntryId>, Vec<EntryId>) {
    let seen: hashing::Set<EntryId> = seen.iter().copied().collect();
    let (kept, mut gone): (Vec<EntryId>, Vec<EntryId>) = previous
        .unwrap_or_default()
        .into_iter()
        .partition(|id| seen.contains(id));
    for id in replaced {
        if !seen.contains(&id) && !gone.contains(&id) {
            gone.push(id);
        }
    }
    (kept, gone)
}

/// `event` as `names` holds it now: an entry no longer the one its name
/// holds is not added (nor updated), an id that is again is not removed,
/// and a rename whose ends changed since is what is left of it. Sent under
/// the names' lock (`Shared::send_as_now`), each worker's events leave in
/// the order of what it did to the names, whatever order the workers end
/// in.
fn as_now(names: &Names, event: ListingEvent) -> Vec<ListingEvent> {
    let current = |id: &EntryId| names.by_id.contains_key(id);
    let some = |event: ListingEvent, empty: bool| if empty { None } else { Some(event) };
    match event {
        ListingEvent::Added(mut batch) => {
            batch.retain(|e| current(&e.id));
            let empty = batch.is_empty();
            some(ListingEvent::Added(batch), empty)
                .into_iter()
                .collect()
        }
        ListingEvent::Updated(mut batch) => {
            batch.retain(|e| current(&e.id));
            let empty = batch.is_empty();
            some(ListingEvent::Updated(batch), empty)
                .into_iter()
                .collect()
        }
        ListingEvent::Removed(mut ids) => {
            ids.retain(|id| !current(id));
            let empty = ids.is_empty();
            some(ListingEvent::Removed(ids), empty)
                .into_iter()
                .collect()
        }
        ListingEvent::Renamed(pairs) => {
            let (mut renamed, mut added, mut removed) = (Vec::new(), Vec::new(), Vec::new());
            for (old, new) in pairs {
                match (current(&old), current(&new.id)) {
                    (false, true) => renamed.push((old, new)),
                    (true, true) => added.push(new),
                    (false, false) => removed.push(old),
                    (true, false) => {}
                }
            }
            let (no_removed, no_renamed, no_added) =
                (removed.is_empty(), renamed.is_empty(), added.is_empty());
            [
                some(ListingEvent::Removed(removed), no_removed),
                some(ListingEvent::Renamed(renamed), no_renamed),
                some(ListingEvent::Added(added), no_added),
            ]
            .into_iter()
            .flatten()
            .collect()
        }
        other => vec![other],
    }
}

/// An open directory.
pub struct PathLocation {
    location: Location,
    dir: PathBuf,
    /// The folder held open as the system resolved it once opened (a link
    /// on the way followed): what is compared with a volume's mount point
    /// (`OpenLocation::holds`).
    held: Arc<std::sync::OnceLock<PathBuf>>,
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
            held: Arc::default(),
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
        let held = Arc::clone(&self.held);
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
                    // Where it is held, by the folder's path the system
                    // knows: `~/Music` a link into a drive is held there.
                    let at = held.get_or_init(|| folder.path_now().unwrap_or_else(|| dir.clone()));
                    let shared = Arc::new(Shared {
                        _held: Held::new(&dir, at),
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

/// The folders held open by their listings, each by where it was opened
/// and where the system says it is (a link on the way followed: `~/Music`
/// into a drive): a listing closed lets go of its folder a moment later,
/// on a worker (`OpenLocation::close`), and what unmounts a volume waits
/// for that ([`wait_let_go`]).
static HELD: parking_lot::Mutex<Vec<[PathBuf; 2]>> = parking_lot::Mutex::new(Vec::new());
static LET_GO: parking_lot::Condvar = parking_lot::Condvar::new();

/// A folder counted in [`HELD`] while it lives.
struct Held([PathBuf; 2]);

impl Held {
    fn new(opened: &Path, at: &Path) -> Self {
        let paths = [opened.to_path_buf(), at.to_path_buf()];
        HELD.lock().push(paths.clone());
        Held(paths)
    }
}

/// Whether a folder held by `paths` is at or below `path`.
fn held_under(paths: &[PathBuf; 2], path: &Path) -> bool {
    paths.iter().any(|p| p.starts_with(path))
}

impl Drop for Held {
    fn drop(&mut self) {
        let mut held = HELD.lock();
        if let Some(at) = held.iter().position(|p| *p == self.0) {
            held.swap_remove(at);
        }
        LET_GO.notify_all();
    }
}

/// Wait until no listing holds a folder at or below `path` open, for at
/// most `at_most`; whether none does. What lets go of a mount asks it
/// after leaving the folders shown there (`App::release`): until their
/// descriptors are closed, the system refuses to unmount it as busy.
pub fn wait_let_go(path: &Path, at_most: Duration) -> bool {
    let deadline = std::time::Instant::now() + at_most;
    let mut held = HELD.lock();
    while held.iter().any(|paths| held_under(paths, path)) {
        if LET_GO.wait_until(&mut held, deadline).timed_out() {
            return !held.iter().any(|paths| held_under(paths, path));
        }
    }
    true
}

impl Shared {
    /// Send what an entry event says, as the names hold it now, under
    /// their lock (`as_now`). The listing, the watcher's changes and
    /// hydration run at once (two at a time in a remote's lane): each
    /// changes the names under the lock and sends after, so without it one
    /// worker's events could reach the pane after another's that changed
    /// the names later, and a name keep two rows where a FUSE program
    /// numbers its file anew (sshfs). False when no one listens.
    fn send_as_now(&self, event: ListingEvent) -> bool {
        let names = self.names.lock();
        as_now(&names, event)
            .into_iter()
            .all(|event| self.tx.send(event))
    }

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
        // Ids a name listed belonged to before: a file its FUSE program
        // numbered anew since (sshfs, once the kernel forgot it).
        let mut replaced: Vec<EntryId> = Vec::new();
        // What tells the folder's made-up numbers apart, as its stats do.
        let mount = self.folder.mount().unwrap_or(0);
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
                let file = FileId::new(self.id.dev, super::stat::on_mount(ent.ino, mount));
                let id = names.listing_id(EntryId::in_folder(file, self.id, &ent.name));
                replaced.extend(names.insert(id, ent.name.clone()));
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
                if !self.send_as_now(ListingEvent::Added(full)) {
                    return;
                }
            }
        }
        if cancel.is_cancelled() {
            return;
        }
        if !batch.is_empty() && !self.send_as_now(ListingEvent::Added(batch)) {
            return;
        }
        let (kept, gone) = gone_after(previous, &seen, replaced);
        // What was stat-ed is stat-ed again; the rest still waits to be
        // asked for.
        let names = self.names.lock();
        let stayed: Vec<EntryId> = kept
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
            self.send_as_now(ListingEvent::Removed(gone));
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
            match stat_entry_as(&self.folder, &mut e) {
                Stated::Found => stated.push((id, e)),
                Stated::Gone => gone.push(id),
                // There, unread: its row stays as listed.
                Stated::Unread => {}
            }
        }
        let (mut updated, mut rekeyed): (Vec<Entry>, Vec<Entry>) = (Vec::new(), Vec::new());
        // Ids a stat found another file under: a provisional one, or a
        // file another took the place of (an editor saving).
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
            self.send_as_now(ListingEvent::Updated(updated));
        }
        if !gone.is_empty() {
            self.send_as_now(ListingEvent::Removed(gone));
        }
        // The row under the name is the file found there, in its place
        // and its selection.
        if !rekeyed.is_empty() {
            self.fill(&dir, &mut rekeyed);
            self.send_as_now(ListingEvent::Renamed(
                stale.into_iter().zip(rekeyed).collect(),
            ));
        }
    }

    /// The directory itself moved: find where through its descriptor, which
    /// followed it, and tell its holder, which shows it there
    /// (`ListingEvent::Moved`). A directory that is gone empties the
    /// listing.
    fn follow_self(&self) -> bool {
        let Some(now) = self.folder.path_now() else {
            return false;
        };
        let before = self.dir();
        let now = spelled_as(&before, now);
        if now != before {
            self.dir.write().clone_from(&now);
            self.tx.send(ListingEvent::Moved(now));
        }
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
            self.send_as_now(ListingEvent::Removed(ids));
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
        // Names another file took (a rename onto them), each with the id
        // it held.
        let mut replaced = Vec::new();
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
            let stated = stat_entry_as(&self.folder, &mut e);
            if stated == Stated::Unread {
                // There, unread: what is listed of it stays.
                continue;
            }
            let mut names = self.names.lock();
            if stated == Stated::Found {
                // A rename onto this name leaves the id it replaced stale.
                if let Some(stale) = names.insert(e.id, name) {
                    removed.push(stale);
                    replaced.push((stale, e.id));
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
        // A name another file took is its row still, unless its file was
        // renamed: an editor saves by writing a new file and renaming it
        // onto the name (gedit, Kate, VS Code), and what was selected there
        // stays so, as in Dolphin and Nautilus. A file renamed onto it from
        // beside it is both.
        for (stale, now) in replaced {
            let Some(at) = removed.iter().position(|id| *id == stale) else {
                continue;
            };
            removed.swap_remove(at);
            let entry = match added.iter().position(|e| e.id == now) {
                Some(at) => added.swap_remove(at),
                None => match renamed.iter().find(|(_, e)| e.id == now) {
                    Some((_, e)) => e.clone(),
                    None => continue,
                },
            };
            renamed.push((stale, entry));
        }
        if !removed.is_empty() {
            self.send_as_now(ListingEvent::Removed(removed));
        }
        // Changed entries arrive stat-ed; give providers their turn so
        // fields such as tags survive the update.
        if !renamed.is_empty() {
            let (olds, mut news): (Vec<EntryId>, Vec<Entry>) = renamed.drain(..).unzip();
            self.fill(&self.dir(), &mut news);
            self.send_as_now(ListingEvent::Renamed(olds.into_iter().zip(news).collect()));
        }
        if !added.is_empty() {
            self.fill(&self.dir(), &mut added);
            self.send_as_now(ListingEvent::Added(added));
        }
    }
}

/// Where a folder that was at `before` is `now` (a path the system
/// resolved), spelled as `before` was while it stays in the same folder:
/// renamed, it keeps the address it was reached by (through a link above
/// it). Stats both folders above it: on the listing's worker.
fn spelled_as(before: &Path, now: PathBuf) -> PathBuf {
    let (Some(was_in), Some(is_in), Some(name)) = (before.parent(), now.parent(), now.file_name())
    else {
        return now;
    };
    if was_in == is_in {
        return now;
    }
    let id = |p: &Path| {
        std::fs::metadata(p)
            .ok()
            .and_then(|meta| super::stat::identity(p, &meta))
    };
    match id(was_in) {
        Some(folder) if id(is_in) == Some(folder) => was_in.join(name),
        _ => now,
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
        Some(self.held.get().unwrap_or(&self.dir))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_listed_anew_leaves_no_row_beside_it() {
        let folder = FileId::new(1, 2);
        let id =
            |ino, name: &str| EntryId::in_folder(FileId::new(1, ino), folder, OsStr::new(name));
        let (kept_file, gone_file) = (id(10, "kept.txt"), id(11, "gone.txt"));
        // "a folder" noted under 20; the watcher added it under 21 after
        // the refresh noted what it starts from; the listing finds it as
        // 22, sshfs having numbered it anew meanwhile.
        let (noted, added_since, listed) =
            (id(20, "a folder"), id(21, "a folder"), id(22, "a folder"));
        let (kept, gone) = gone_after(
            Some(vec![kept_file, gone_file, noted]),
            &[kept_file, listed],
            vec![added_since],
        );
        assert_eq!(kept, [kept_file]);
        assert_eq!(gone, [gone_file, noted, added_since], "one name, one row");
        // A first listing notes nothing: what a name held before still goes.
        let (kept, gone) = gone_after(None, &[listed], vec![added_since]);
        assert!(kept.is_empty());
        assert_eq!(gone, [added_since]);
        // An id seen under another name stays.
        let (_, gone) = gone_after(Some(vec![]), &[added_since], vec![added_since]);
        assert!(gone.is_empty());
    }

    /// The pane's rows after `events`, from `rows`.
    fn shown(mut rows: Vec<EntryId>, events: Vec<ListingEvent>) -> Vec<EntryId> {
        for event in events {
            match event {
                ListingEvent::Added(batch) => {
                    for e in batch {
                        if !rows.contains(&e.id) {
                            rows.push(e.id);
                        }
                    }
                }
                ListingEvent::Removed(ids) => rows.retain(|id| !ids.contains(id)),
                ListingEvent::Renamed(pairs) => {
                    for (old, new) in pairs {
                        rows.retain(|id| *id != old);
                        rows.push(new.id);
                    }
                }
                _ => {}
            }
        }
        rows
    }

    #[test]
    fn workers_ending_out_of_order_leave_a_name_one_row() {
        let folder = FileId::new(1, 2);
        let name = OsStr::new("a folder");
        let id = |ino| EntryId::in_folder(FileId::new(1, ino), folder, name);
        let entry = |id| Entry::new(id, name, EntryKind::Directory);
        let (shown_first, by_watcher, by_listing) = (id(20), id(21), id(22));
        let mut names = Names::default();
        names.insert(shown_first, name.into());
        // The watcher stats it anew (sshfs numbered it 21) and notes so;
        // before it sends, a refresh's listing finds it as 22, sends that
        // and what the name held before; then the watcher sends.
        let stale = names.insert(by_watcher, name.into());
        assert_eq!(stale, Some(shown_first));
        let replaced = names.insert(by_listing, name.into());
        assert_eq!(replaced, Some(by_watcher));
        let mut events = as_now(&names, ListingEvent::Added(vec![entry(by_listing)]));
        events.extend(as_now(&names, ListingEvent::Removed(vec![by_watcher])));
        events.extend(as_now(&names, ListingEvent::Removed(vec![shown_first])));
        events.extend(as_now(&names, ListingEvent::Added(vec![entry(by_watcher)])));
        assert_eq!(
            shown(vec![shown_first], events),
            [by_listing],
            "one name, one row"
        );

        // A rename whose new end went since is its old end's removal; one
        // whose old end came back is the new end's arrival.
        let (old, new) = (id(30), id(31));
        let pairs = || ListingEvent::Renamed(vec![(old, entry(new))]);
        let events = as_now(&names, pairs());
        assert!(matches!(&events[..], [ListingEvent::Removed(ids)] if *ids == [old]));
        names.insert(old, "old".into());
        names.insert(new, "new".into());
        let events = as_now(&names, pairs());
        assert!(matches!(&events[..], [ListingEvent::Added(batch)] if batch[0].id == new));
        names.remove_id(old);
        let events = as_now(&names, pairs());
        assert!(matches!(&events[..], [ListingEvent::Renamed(pairs)] if pairs[0].0 == old));
    }
}
