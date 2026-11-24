//! The `path` scheme: stream a directory, stat lazily, watch for changes.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crossbeam_channel::Receiver;
use parking_lot::{Mutex, RwLock};

use crate::cancel::CancelToken;
use crate::entry::{Entry, EntryId, EntryKind, FileId};
use crate::hashing;
use crate::location::{
    EventSink, ListingEvent, Location, LocationProvider, Needs, OpenContext, OpenLocation,
    PATH_SCHEME,
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
            .ok_or_else(|| {
                anyhow::Error::new(crate::words!(
                    fs_not_a_path = "not a path: { $location }",
                    location = location.to_string(),
                    note = "Said to an agent or a script: what was given to open as a folder is no path (an address, as written)."
                ))
            })?;
        // Never the process's working folder: a relative path is its
        // caller's to read from the folder it was typed in
        // (`App::navigate`).
        let path = crate::os_path::absolute(&path)?.to_path_buf();
        Ok(PathLocation::open(location.clone(), path, ctx))
    }

    /// Words search a folder and its subfolders.
    fn searchable(&self, _location: &Location) -> bool {
        true
    }
}

struct Shared {
    /// Where the directory is now: it follows the directory when it moves.
    dir: RwLock<PathBuf>,
    /// Where the system resolved it as it was opened (a link on the way
    /// followed): the volume it holds, a read of it too (`Held`).
    at: PathBuf,
    /// The directory, held open or opened for each read: names are
    /// stat-ed relative to it.
    folder: RwLock<Folder>,
    /// The folder's own file: with each name, what its entries are told
    /// apart by (`EntryId::in_folder`); its device, theirs until stat-ed.
    id: FileId,
    names: Mutex<Names>,
    /// Every name of the folder is in `names`: a batch can ask them.
    listed: std::sync::atomic::AtomicBool,
    /// Never sent on directly: through [`Shared::send`], which refuses
    /// once it is lost (and by [`Shared::lose`], under the same lock).
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
    /// The names the watcher last said another program writes still
    /// (ADR-135, `keys::WRITING`): what a stat of them made since (a row
    /// hydrated) is marked with too, and what a rescan, which says nothing
    /// of it, tells again unmarked.
    writing: Mutex<std::collections::HashSet<std::ffi::OsString>>,
    /// Where its listing keeps it: let go of from there once lost
    /// ([`Shared::lose`]), so a refresh opens the folder again from its
    /// path rather than list a directory that is gone.
    slot: std::sync::Weak<Mutex<Option<Arc<Shared>>>>,
    /// How its listing opens it: what was asked of it once handed on
    /// ([`Shared::hand_over`]) is asked of the folder its listing opens
    /// after it.
    opening: std::sync::Weak<Mutex<Opening>>,
    /// Counts the stats made of its entries: each takes the next
    /// ([`Shared::ticket`]) just before it reads, so of two stats of one
    /// entry the one begun later has the higher (`Names::newest`).
    tickets: super::Tickets,
    /// How many times it was opened from its path for a read: what the
    /// tests count.
    #[cfg(test)]
    opens: std::sync::atomic::AtomicUsize,
    /// Called as a listing is about to read the folder (`None`), then with
    /// the first name it read (its first names read with it): where a test
    /// holds it while names move.
    #[cfg(test)]
    listing_at: Mutex<Option<ListingAt>>,
    /// Whether it was lost, and how: [`LISTED`] not, [`HANDED_ON`],
    /// [`LOST_MISSING`] or [`LOST_LEFT`] (its volume let go of, which a
    /// deletion told after does not say over). Changed only under the
    /// names' lock, which every event it sends is sent under
    /// ([`Shared::send`]): nothing it sends reaches its listing once lost.
    lost: std::sync::atomic::AtomicU8,
}

/// How a folder listed is held (ADR-149, amended).
enum Folder {
    /// Held open, counted among the folders held ([`wait_let_go`]):
    /// followed wherever it moves on its volume.
    Held(Arc<(Dir, Held)>),
    /// Opened for each read, nothing of it held between: on a volume the
    /// user may let go of while it is shown (`fs::volume::lets_go`: a
    /// removable drive, one mounted for the user, a remote), or one whose
    /// unmount was announced (`OpenLocation::hold_nothing`), whose
    /// unmount nothing of Skwir's refuses as busy. Followed only when
    /// renamed within its folder, found there by its identity (which its
    /// watch, holding no mount, keeps the system from forgetting).
    ByPath,
}

/// The folder as one read finds it: the one held, or opened from its path
/// for this read alone, counted among the folders held meanwhile.
enum Opened {
    Held(Arc<(Dir, Held)>),
    Now { dir: Dir, _held: Held },
}

impl std::ops::Deref for Opened {
    type Target = Dir;
    fn deref(&self) -> &Dir {
        match self {
            Opened::Held(held) => &held.0,
            Opened::Now { dir, .. } => dir,
        }
    }
}

/// Where a test holds a listing (`Shared::listing_at`).
#[cfg(test)]
type ListingAt = Box<dyn FnMut(Option<&OsStr>) + Send>;

/// A folder listed, not lost ([`Shared::lost`]).
const LISTED: u8 = 0;
/// A folder handed on to the listing that opened its path again
/// ([`Shared::hand_on`]): its rows are that one's, and it says nothing more.
const HANDED_ON: u8 = 1;
/// A folder lost as deleted ([`Shared::lose`]).
const LOST_MISSING: u8 = 2;
/// A folder lost with its volume ([`Shared::lose`], ADR-149).
const LOST_LEFT: u8 = 3;

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
    /// `aliases` the other way, the latest provisional id of each: where a
    /// folder's stats never agree with its listing, every entry is re-keyed,
    /// and a walk of `aliases` for each was the folder's size squared.
    aliased: hashing::Map<EntryId, EntryId>,
    /// The entries stat-ed since they were listed: what a listing again
    /// (F5, a watcher's rescan) stats again, its metadata kept meanwhile.
    /// The others were never asked for, and a refresh does not read the
    /// whole folder for them. Each with what was last told of it
    /// (`Told`, `Names::newest`): what a stat begun before found is not
    /// told after it.
    stated: hashing::Map<EntryId, u64>,
    /// Answered unread unasked of the folder (`hydrate`: a drive Windows
    /// lets go of, a folder not there to read), with as much as each was
    /// asked to be read: what a listing again (the rescan once a drive is
    /// watched again, its removal refused) asks for again, as it does what
    /// was stat-ed. A row answered unread counts as read: nothing else
    /// asks for it again.
    unread: hashing::Map<EntryId, Needs>,
    /// Asked for before this listing named them (rows of the one it was
    /// opened again in place of, `reopen`), with how soon and how much:
    /// read once it is listed, every id a hydration is given being
    /// answered.
    early: Vec<(Vec<EntryId>, Priority, Needs)>,
    /// Which read of each name was applied last, while a read that may
    /// hold an older one is under way (`fs::Newest`): one begun before it,
    /// applied after, is left out, so of two reads of a name (a stat, or a
    /// listing's, whose names share its ticket) the one begun later
    /// stands, gone or unread too.
    newest: super::Newest<OsString>,
    /// Ids a hydration re-keyed to (a provisional id, a file another took
    /// the place of) whose rename is not told yet, with the id their row
    /// is still told under: whatever tells one of them first tells it
    /// renamed from that (`Names::newest`), so a row keeps its selection
    /// whichever stat of it is told, and in whatever order.
    untold: hashing::Map<EntryId, EntryId>,
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
                self.stated.remove(&prev);
                self.unread.remove(&prev);
                self.untold.remove(&prev);
                Some(prev)
            }
            _ => None,
        }
    }

    fn remove_id(&mut self, id: EntryId) -> Option<OsString> {
        self.stated.remove(&id);
        self.unread.remove(&id);
        self.untold.remove(&id);
        let name = self.by_id.remove(&id)?;
        self.by_name.remove(&name);
        Some(name)
    }

    fn remove_name(&mut self, name: &OsStr) -> Option<EntryId> {
        let id = self.by_name.remove(name)?;
        self.by_id.remove(&id);
        self.stated.remove(&id);
        self.unread.remove(&id);
        self.untold.remove(&id);
        Some(id)
    }

    /// The id a directory entry is listed under.
    fn listing_id(&self, provisional: EntryId) -> EntryId {
        self.aliases
            .get(&provisional)
            .copied()
            .unwrap_or(provisional)
    }

    /// `old` turned out to be `new` when stat-ed: its row, told under
    /// `old` (or what `old` was re-keyed from, not told yet), is told
    /// renamed by whatever tells `new` first (`untold`).
    fn rekey(&mut self, old: EntryId, new: EntryId) {
        let told_as = self.untold.get(&old).copied().unwrap_or(old);
        if let Some(name) = self.remove_id(old) {
            self.insert(new, name);
            if told_as != new {
                self.untold.insert(new, told_as);
            }
        }
        let provisional = self
            .aliased
            .remove(&old)
            .filter(|from| self.aliases.get(from) == Some(&old))
            .unwrap_or(old);
        if provisional == new {
            self.aliases.remove(&provisional);
        } else {
            if let Some(before) = self.aliases.insert(provisional, new)
                && self.aliased.get(&before) == Some(&provisional)
            {
                self.aliased.remove(&before);
            }
            self.aliased.insert(new, provisional);
        }
    }

    fn ids(&self) -> Vec<EntryId> {
        self.by_id.keys().copied().collect()
    }

    /// `event`, what stats found (`Added`, `Updated`, `Renamed`; `tickets`
    /// the ticket each entry's stat took, by its id), without what a stat
    /// begun later told already: of two stats of an entry, the one begun
    /// later stands, whichever is told last (a hydration's, its providers
    /// slow, told after the watcher's of a file written meanwhile, would
    /// leave its row as it was then, marked written for good). Each entry
    /// kept is noted as told (`Told`). An entry kept whose rename is not
    /// told yet (`untold`) is told renamed, whatever told it: the row keeps
    /// its place and its selection. A rename whose entry is left out takes
    /// out the row it renames, where one is left: the entry it would become
    /// was told already, renamed so. With what is to be read again: an
    /// entry the providers filled, left out for a stat alone begun later
    /// (an order's reading) that told less than it would have.
    fn newest(
        &mut self,
        event: ListingEvent,
        tickets: &hashing::Map<EntryId, u64>,
    ) -> (Vec<ListingEvent>, Vec<EntryId>) {
        let mut again = Vec::new();
        let mut newest = |e: &Entry| {
            let Some(&ticket) = tickets.get(&e.id) else {
                return true;
            };
            // Not its now: `as_now` leaves it out, and nothing is noted.
            if !self.by_id.contains_key(&e.id) {
                return true;
            }
            let filled = e.metadata.is_filled();
            let told = self.stated.get(&e.id).copied().map(Told);
            match told {
                Some(told) if told.ticket() > ticket => {
                    if filled && !told.filled() {
                        again.push(e.id);
                    }
                    false
                }
                // The providers' fields a row holds stay under a stat alone
                // (`Listing::upsert`): filled still.
                _ => {
                    let filled = filled || told.is_some_and(Told::filled);
                    self.stated.insert(e.id, Told::of(ticket, filled).0);
                    true
                }
            }
        };
        let mut events = match event {
            ListingEvent::Added(mut batch) => {
                batch.retain(|e| newest(e));
                vec![ListingEvent::Added(batch)]
            }
            ListingEvent::Updated(mut batch) => {
                batch.retain(|e| newest(e));
                vec![ListingEvent::Updated(batch)]
            }
            ListingEvent::Renamed(pairs) => {
                let (kept, left): (Vec<_>, Vec<_>) =
                    pairs.into_iter().partition(|(_, e)| newest(e));
                let gone = left.into_iter().map(|(old, _)| old).collect();
                vec![ListingEvent::Renamed(kept), ListingEvent::Removed(gone)]
            }
            other => vec![other],
        };
        if !self.untold.is_empty() {
            let carried = self.carry_renames(&mut events);
            if !carried.is_empty() {
                events.push(ListingEvent::Renamed(carried));
            }
        }
        (events, again)
    }

    /// The entries `events` tell whose rename is not told yet (`untold`),
    /// taken out of an `Added` or `Updated` and returned as renamed from
    /// the id their row is told under; a rename from another id told too.
    /// Each told so is no longer untold.
    fn carry_renames(&mut self, events: &mut [ListingEvent]) -> Vec<(EntryId, Entry)> {
        let mut carried = Vec::new();
        for event in events {
            match event {
                ListingEvent::Added(batch) | ListingEvent::Updated(batch) => {
                    for e in std::mem::take(batch) {
                        match self.untold_of(e.id) {
                            Some(old) => carried.push((old, e)),
                            None => batch.push(e),
                        }
                    }
                }
                ListingEvent::Renamed(pairs) => {
                    for (from, e) in pairs.iter() {
                        if let Some(old) = self.untold_of(e.id)
                            && old != *from
                        {
                            carried.push((old, e.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
        carried
    }

    /// The id the row of `id`, listed now, is still told under (`untold`),
    /// told renamed from now on.
    fn untold_of(&mut self, id: EntryId) -> Option<EntryId> {
        if !self.by_id.contains_key(&id) {
            return None;
        }
        self.untold.remove(&id)
    }
}

/// What was last told of an entry (`Names::stated`): the ticket of the
/// stat it came from, and whether its row is filled by the metadata
/// providers since, in one number (the ticket, then a bit).
#[derive(Clone, Copy)]
struct Told(u64);

impl Told {
    fn of(ticket: u64, filled: bool) -> Self {
        Told(ticket << 1 | u64::from(filled))
    }

    fn ticket(self) -> u64 {
        self.0 >> 1
    }

    fn filled(self) -> bool {
        self.0 & 1 == 1
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
    // Asked of a set: a folder its FUSE program numbered anew has every id
    // replaced and every one gone, and a look along `gone` for each was the
    // folder's size squared.
    let mut going: hashing::Set<EntryId> = gone.iter().copied().collect();
    for id in replaced {
        if !seen.contains(&id) && going.insert(id) {
            gone.push(id);
        }
    }
    (kept, gone)
}

/// `event` as `names` holds it now: an entry no longer the one its name
/// holds is not added (nor updated), an id that is again is not removed,
/// and a rename whose ends changed since is what is left of it. Sent under
/// the names' lock (`Shared::send`), each worker's events leave in
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
        ListingEvent::Unreadable(mut ids) => {
            ids.retain(current);
            let empty = ids.is_empty();
            some(ListingEvent::Unreadable(ids), empty)
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
    /// The folder held open as the system resolved it at its latest open
    /// (a link on the way followed, which may lead elsewhere once it is
    /// opened again): what is compared with a volume's mount point
    /// (`OpenLocation::holds`).
    held: Arc<RwLock<Option<PathBuf>>>,
    ctx: OpenContext,
    tx: EventSink,
    shared: Arc<Mutex<Option<Arc<Shared>>>>,
    opening: Arc<Mutex<Opening>>,
    rx: Receiver<ListingEvent>,
}

/// What a folder's listing does about opening it: whether an open is
/// under way, and whether it looks again once done whether its path leads
/// to the folder it holds (`OpenLocation::reopen` asked meanwhile); what
/// was asked to be read while it held none; and what was decided of its
/// volume, which an open under way is held to as it ends, as the folder it
/// opened is (ADR-149).
#[derive(Default)]
struct Opening {
    under_way: bool,
    again: bool,
    /// What was asked to be read (`OpenLocation::hydrate`, `read`) while
    /// no folder was held, an open under way: asked of the folder it opens
    /// once it holds it, never dropped while its rows show.
    wanted: Vec<(Vec<EntryId>, Priority, Needs)>,
    /// Its volume is about to be let go of (`OpenLocation::hold_nothing`):
    /// every folder it opens from then on is read by its path, one an open
    /// under way finds too, until a volume is mounted there again
    /// (`reopen`).
    hold_nothing: bool,
    /// Its volume was let go of (`OpenLocation::volume_gone`): what an
    /// open under way finds is lost at once, saying this, and what it
    /// fails with says this instead. Forgotten as it is asked to open again
    /// (a refresh, a volume mounted there).
    gone: Option<crate::ListingError>,
}

/// Open the folder at `dir` and hold it in `slot`, for its listing to
/// list: on a worker, in `lane`. Held to what `opening` decided of its
/// volume meanwhile (`hold_nothing`, `gone`), looked at once it is in
/// `slot`, as what decides looks at `slot` once it decided: one of the two
/// sees the other, and an open under way never holds a volume let go of
/// nor shows a folder of one gone. `None` when cancelled.
fn open_folder(
    dir: &Path,
    ctx: &OpenContext,
    tx: &EventSink,
    slot: &Arc<Mutex<Option<Arc<Shared>>>>,
    opening: &Arc<Mutex<Opening>>,
    held: &RwLock<Option<PathBuf>>,
    lane: Lane,
) -> anyhow::Result<Option<Arc<Shared>>> {
    // Opening/stat-ing a sleeping disk and installing a watch are I/O.
    // Publish the handle first, then stream entries on this worker.
    let folder = Dir::open(dir)?;
    if ctx.cancel.is_cancelled() {
        return Ok(None);
    }
    let id = folder.identity()?;
    if ctx.cancel.is_cancelled() {
        return Ok(None);
    }
    let lane = if lane.is_shared() {
        super::reach::of_device(id.dev).map_or(lane, |remote| remote.lane())
    } else {
        lane
    };
    // Where it is held, by the folder's path the system
    // knows: `~/Music` a link into a drive is held there.
    let at = folder.path_now().unwrap_or_else(|| dir.to_path_buf());
    // A volume the user may let go of is never held between reads
    // (ADR-149, amended): an unmount by any program finds it free. Nor one
    // about to be let go of.
    let hold_nothing = opening.lock().hold_nothing;
    let folder = if lane.is_shared() && !hold_nothing && !super::volume::lets_go(&at) {
        Folder::Held(Arc::new((folder, Held::new(dir, &at))))
    } else {
        Folder::ByPath
    };
    let shared = Arc::new(Shared {
        dir: RwLock::new(dir.to_path_buf()),
        at,
        folder: RwLock::new(folder),
        id,
        names: Mutex::new(Names::default()),
        listed: std::sync::atomic::AtomicBool::new(false),
        tx: tx.clone(),
        ctx: ctx.clone(),
        watcher: Mutex::new(None),
        lane,
        changes: Mutex::new((None, false)),
        writing: Mutex::new(std::collections::HashSet::new()),
        slot: Arc::downgrade(slot),
        opening: Arc::downgrade(opening),
        tickets: super::Tickets::default(),
        #[cfg(test)]
        opens: std::sync::atomic::AtomicUsize::new(0),
        #[cfg(test)]
        listing_at: Mutex::new(None),
        lost: std::sync::atomic::AtomicU8::new(LISTED),
    });
    // Watched before it is listed, so nothing changed
    // meanwhile is missed. The watcher holds the listing
    // weakly: closing it lets go of both.
    let weak = Arc::downgrade(&shared);
    match watch::watch(dir, Duration::from_millis(50), &ctx.cancel, move |c| {
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
            return Ok(None);
        }
        *slot = Some(Arc::clone(&shared));
        // What its listing says it holds, the folder held now.
        *held.write() = Some(shared.at.clone());
    }
    // Decided while it opened, and asked of it: held to it now.
    let (hold_nothing, gone, wanted) = {
        let mut opening = opening.lock();
        (
            opening.hold_nothing,
            opening.gone.clone(),
            std::mem::take(&mut opening.wanted),
        )
    };
    if hold_nothing {
        shared.hold_nothing();
    }
    if let Some(gone) = gone {
        // What was asked goes with its rows.
        shared.lose(gone);
        return Ok(Some(shared));
    }
    for (ids, priority, needs) in wanted {
        shared.ask(&ids, priority, needs);
    }
    Ok(Some(shared))
}

/// Ask what the listing holds now (`slot`) to read `needs` of `ids`, or,
/// while it opens one, the one it opens once it holds it
/// (`Opening::wanted`): every id a hydration is given is answered
/// (AGENTS.md, "Entries arrive before metadata") by the listing whose rows
/// they are. Nothing while it holds none and opens none: lost, its rows
/// went with it.
fn hydrate_held(
    slot: &Mutex<Option<Arc<Shared>>>,
    opening: &Mutex<Opening>,
    ids: Vec<EntryId>,
    priority: Priority,
    needs: Needs,
) {
    let held = {
        let mut opening = opening.lock();
        let held = slot.lock().clone().filter(|held| !held.is_lost());
        if held.is_none() && opening.under_way {
            opening.wanted.push((ids, priority, needs));
            return;
        }
        held
    };
    if let Some(held) = held {
        held.ask(&ids, priority, needs);
    }
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
            opening: Arc::default(),
        });
        this.start_open();
        this
    }

    fn start_open(&self) {
        {
            let mut opening = self.opening.lock();
            if self.ctx.cancel.is_cancelled() || opening.under_way {
                return;
            }
            opening.under_way = true;
            // Asked anew: what is at its path now is what it shows.
            opening.gone = None;
        }
        self.spawn_open(false);
    }

    /// Open the folder from its path and list it on a worker, in its
    /// lane. `again` (`reopen`): only where its path leads to another
    /// folder than the one it holds now (`Shared::is_at`), the entries
    /// that one listed handed on to be taken out once the new one is
    /// listed, those it lists no more; a folder opened since the mount that
    /// asks it was made is left as it is, never listed twice. Asked again
    /// meanwhile (`Opening::again`), it looks again once done.
    fn spawn_open(&self, again: bool) {
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
                let mut again = again;
                loop {
                    // Opened again (a volume mounted at or above it): only
                    // where its path leads elsewhere than the folder held.
                    let mut previous = None;
                    let opens = !again || {
                        let held_now = slot.lock().clone();
                        match held_now {
                            Some(held_now) if held_now.is_at(&held_now.dir()) => false,
                            Some(held_now) => {
                                let taken = {
                                    let mut slot = slot.lock();
                                    slot.take_if(|it| Arc::ptr_eq(it, &held_now))
                                };
                                if let Some(taken) = taken {
                                    previous = taken.hand_on();
                                    let_go(&ctx, taken);
                                }
                                true
                            }
                            None => true,
                        }
                    };
                    let result = if opens {
                        open_folder(&dir, &ctx, &tx, &slot, &opening, &held, lane)
                    } else {
                        Ok(None)
                    };
                    match result {
                        Ok(Some(shared)) if !shared.is_lost() => {
                            shared.list(&ctx.cancel, previous.take(), true);
                        }
                        // Lost as it opened (its volume gone meanwhile):
                        // what was listed before goes with it.
                        Ok(Some(_)) => {
                            if let Some(ids) = previous.take() {
                                tx.send(ListingEvent::Removed(ids));
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            if !ctx.cancel.is_cancelled() {
                                // What was listed before is not there any
                                // more: never left beside the failure.
                                if let Some(ids) = previous.take() {
                                    tx.send(ListingEvent::Removed(ids));
                                }
                                // Its volume gone says over what the open
                                // met after.
                                let gone = opening.lock().gone.clone();
                                tx.send(ListingEvent::Error(
                                    gone.unwrap_or_else(|| crate::ListingError::of(error)),
                                ));
                            }
                        }
                    }
                    let mut state = opening.lock();
                    if state.again && !ctx.cancel.is_cancelled() {
                        // Asked again meanwhile: its path may lead
                        // elsewhere since it was opened.
                        state.again = false;
                        again = true;
                        continue;
                    }
                    // What was decided of its volume stays; what was asked
                    // of a folder that did not open went with its rows.
                    state.under_way = false;
                    state.wanted = Vec::new();
                    return;
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
                s.list(&cancel, previous, false);
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
    /// Send what an event says, as the names hold it now, under their
    /// lock (`as_now`), unless it was lost: every event of a folder listed
    /// goes through here, and what loses it ([`Shared::lose`],
    /// [`Shared::hand_on`]) does so under the same lock, so nothing it
    /// sends (a `Complete` after the stats a listing makes first, an entry
    /// a change found) reaches its listing after it was lost, nor says
    /// over why. The listing, the watcher's changes and hydration run at
    /// once (two at a time in a remote's lane): each changes the names
    /// under the lock and sends after, so without it one worker's events
    /// could reach the pane after another's that changed the names later,
    /// and a name keep two rows where a FUSE program numbers its file anew
    /// (sshfs). False when no one listens, or once it is lost.
    fn send(&self, event: ListingEvent) -> bool {
        let names = self.names.lock();
        if self.is_lost() {
            return false;
        }
        as_now(&names, event)
            .into_iter()
            .all(|event| self.tx.send(event))
    }

    /// [`Shared::send`] of what stats found, `tickets` the ticket each
    /// entry's stat took, by its id: what a stat begun later told already
    /// is left out (`Names::newest`), under the lock it is decided by.
    /// What is to be read again, the providers' fields of it not told.
    #[must_use]
    fn send_stated(
        &self,
        event: ListingEvent,
        tickets: &hashing::Map<EntryId, u64>,
    ) -> Vec<EntryId> {
        let mut names = self.names.lock();
        if self.is_lost() {
            return Vec::new();
        }
        let (newest, again) = names.newest(event, tickets);
        for event in newest {
            for event in as_now(&names, event) {
                if !self.tx.send(event) {
                    return Vec::new();
                }
            }
        }
        again
    }

    /// Whether its folder is on a drive whose watches are let go of now
    /// (`watch::letting_go`: Windows' Eject asked, never on Linux), by its
    /// path or where the system resolved it: nothing of it is read
    /// meanwhile. From memory.
    fn letting_go(&self) -> bool {
        watch::letting_go(&self.dir()) || watch::letting_go(&self.at)
    }

    /// The next stat's ticket, taken just before it reads: after what was
    /// changed before it (the names the watcher says are written), so a
    /// stat with a higher one read them as they are now.
    fn ticket(&self) -> u64 {
        self.tickets.take()
    }

    /// A read of stats begins (a hydration's batch, the watcher's
    /// changes): counted until the guard is dropped, so what a stat found
    /// is noted applied while another read may hold one begun before it
    /// (`Names::applies`). Taken before its first ticket, dropped after
    /// its last event is sent, no names' guard held then.
    fn reading(&self) -> super::Reading<'_, Names, OsString> {
        super::Reading::begin(&self.names, |names| &mut names.newest, &self.tickets)
    }

    fn dir(&self) -> PathBuf {
        self.dir.read().clone()
    }

    /// What a test does where a listing reads (`listing_at`), taken out
    /// while it runs: it may wait for what the watcher applies.
    #[cfg(test)]
    fn at_listing(&self, name: Option<&OsStr>) {
        let hook = self.listing_at.lock().take();
        if let Some(mut hook) = hook {
            hook(name);
            *self.listing_at.lock() = Some(hook);
        }
    }

    /// Its listing failed with `error`. What was handed on to it to take
    /// out (`taken`: rows of the folder it was opened again in place of,
    /// `reopen`) goes first, those it did not list again, never shown
    /// beside the failure: sent through the gate, as the names hold them
    /// now (`as_now`), a `Removed` takes out no id this listing named
    /// before it failed, those of a batch not sent yet included. A
    /// refresh's own rows stay, the files it read still shown under it.
    fn fail(&self, error: &std::io::Error, taken: Option<Vec<EntryId>>) {
        if let Some(ids) = taken {
            self.send(ListingEvent::Removed(ids));
        }
        self.send(ListingEvent::Error(crate::ListingError::of_io(error)));
    }

    /// Stream the directory in batches. `previous` (on refresh) lets us emit
    /// `Removed` for entries that disappeared; the entries that stayed are
    /// stat-ed again afterwards, since the listing keeps their old metadata.
    /// `handed_on`: `previous` are another folder's rows, handed on by
    /// the listing it was opened again in place of (`reopen`), taken out
    /// with a failure too; a refresh's are its own, kept beside it.
    fn list(
        self: &Arc<Self>,
        cancel: &CancelToken,
        previous: Option<Vec<EntryId>>,
        handed_on: bool,
    ) {
        if cancel.is_cancelled() || self.is_lost() {
            return;
        }
        let first = previous.is_none();
        let folder = match self.opened() {
            Ok(folder) => folder,
            Err(e) => {
                self.fail(&e, previous.filter(|_| handed_on));
                return;
            }
        };
        // A read of every name, as a stat is of one (`Names::applies`): its
        // one ticket taken before the folder is read, so a stat of a name
        // begun after it, the watcher's or a hydration's, told already,
        // stands over what this read finds of the name, and this read over
        // a stat begun before it, told after. Dropped once what it found is
        // applied.
        let reading = self.reading();
        let ticket = self.ticket();
        #[cfg(test)]
        self.at_listing(None);
        let mut dir = match folder.read() {
            Ok(d) => d,
            Err(e) => {
                self.fail(&e, previous.filter(|_| handed_on));
                return;
            }
        };
        let mut batch = Vec::with_capacity(self.ctx.batch_size);
        let mut seen: Vec<EntryId> = Vec::new();
        // Ids a name listed belonged to before: a file its FUSE program
        // numbered anew since (sshfs, once the kernel forgot it).
        let mut replaced: Vec<EntryId> = Vec::new();
        // What tells the folder's made-up numbers apart, as its stats do.
        let mount = folder.mount().unwrap_or(0);
        #[cfg(test)]
        let mut first_read = true;
        while !cancel.is_cancelled() {
            let Some(ent) = dir.next() else {
                break;
            };
            // Lost meanwhile (`Shared::lose`, `reopen`): what it would
            // send is no listing's any more.
            if cancel.is_cancelled() || self.is_lost() {
                return;
            }
            let ent = match ent {
                Ok(e) => e,
                Err(e) => {
                    self.fail(&e, previous.filter(|_| handed_on));
                    return;
                }
            };
            #[cfg(test)]
            if std::mem::take(&mut first_read) {
                self.at_listing(Some(&ent.name));
            }
            let id = {
                let mut names = self.names.lock();
                // A read of the name begun after this one told already (a
                // rename, `mv f g; mv g f`, told while a long listing ran):
                // what it found stands, the file it found there kept.
                if !names.newest.applies(&ent.name, ticket) {
                    seen.extend(names.by_name.get(&ent.name).copied());
                    continue;
                }
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
                entry.target = crate::fs::stat::follow(&folder, entry.name.as_os_str());
            }
            batch.push(entry);
            if batch.len() >= self.ctx.batch_size {
                let full = std::mem::replace(&mut batch, Vec::with_capacity(self.ctx.batch_size));
                if !self.send(ListingEvent::Added(full)) {
                    return;
                }
            }
        }
        if cancel.is_cancelled() || self.is_lost() {
            return;
        }
        if !batch.is_empty() && !self.send(ListingEvent::Added(batch)) {
            return;
        }
        let (kept, gone) = gone_after(previous, &seen, replaced);
        // What was stat-ed is stat-ed again, as far as it was read: what
        // the providers filled (a row drawn) read by them again, what only
        // a stat read (an order's reading, ADR-121) stat-ed alone, never
        // the whole folder's providers at each refresh or rescan. So is
        // what was answered unread unasked of the folder, as far as it was
        // asked. The rest still waits to be asked for.
        let (mut filled, mut stat_only) = (Vec::new(), Vec::new());
        {
            let mut names = self.names.lock();
            for id in kept {
                let told = names.stated.get(&id).copied().map(Told);
                let unread = names.unread.remove(&id);
                if told.is_some_and(Told::filled) || unread >= Some(Needs::Providers) {
                    filled.push(id);
                } else if told.is_some() || unread.is_some() {
                    stat_only.push(id);
                }
            }
        }
        if !gone.is_empty() {
            let mut names = self.names.lock();
            // A row this read did not see, whose name a read begun after it
            // found there since: it stays, as found.
            let gone: Vec<EntryId> = gone
                .into_iter()
                .filter(|id| match names.by_id.get(id).cloned() {
                    Some(name) => names.newest.applies(&name, ticket),
                    None => true,
                })
                .collect();
            for id in &gone {
                names.remove_id(*id);
            }
            drop(names);
            if !gone.is_empty() {
                self.send(ListingEvent::Removed(gone));
            }
        }
        drop(reading);
        // Every name known now: what was asked before it was is read.
        let early = {
            let mut names = self.names.lock();
            self.listed.store(true, Ordering::Release);
            std::mem::take(&mut names.early)
        };
        // The pane's order reads what a stat finds (a date, a size, an
        // owner) or what the providers say (tags, a rating): every entry
        // is read that far before the listing completes, so the folder
        // appears whole in that order, in its groups, rather than moving
        // as rows are read (ADR-120, amended). No further: for an order by
        // a stat, what the providers say is read of the rows drawn, as in
        // any listing (ADR-121, amended 2026-11-24). Its names are all
        // known now, which the metadata providers read beside it.
        if first && self.ctx.reads > Needs::Nothing {
            for chunk in seen.chunks(self.ctx.batch_size.max(1)) {
                if cancel.is_cancelled() {
                    return;
                }
                // Handed on meanwhile: the next listing reads what its
                // order wants. In the folder this listing opened: read by
                // its path, it is opened once.
                let _ = self.hydrate_in(&folder, chunk, Priority::Visible, self.ctx.reads);
            }
        }
        self.send(ListingEvent::Complete);
        for (ids, priority, needs) in early {
            self.ask(&ids, priority, needs);
        }
        if !filled.is_empty() {
            self.ask(&filled, Priority::Background, Needs::Providers);
        }
        if !stat_only.is_empty() {
            let needs = if self.ctx.reads >= Needs::Providers {
                Needs::Providers
            } else {
                Needs::Stat
            };
            self.ask(&stat_only, Priority::Background, needs);
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

    /// Read `needs` of `ids` in batches, a job each in its lane: what a
    /// job finds handed on is asked of the folder its listing holds after
    /// it ([`Shared::hand_over`]).
    fn ask(self: &Arc<Self>, ids: &[EntryId], priority: Priority, needs: Needs) {
        for chunk in ids.chunks(self.ctx.batch_size.max(1)) {
            let (s, ids) = (Arc::clone(self), chunk.to_vec());
            self.ctx.scheduler.spawn_in(
                self.lane,
                priority,
                "fs.stat",
                self.ctx.cancel.child(),
                move || {
                    if let Some(ids) = s.hydrate(&ids, priority, needs) {
                        s.hand_over(ids, priority, needs);
                    }
                },
            );
        }
    }

    /// What was asked of it once it was handed on, asked of the folder its
    /// listing holds or opens now.
    fn hand_over(&self, ids: Vec<EntryId>, priority: Priority, needs: Needs) {
        if let (Some(slot), Some(opening)) = (self.slot.upgrade(), self.opening.upgrade()) {
            hydrate_held(&slot, &opening, ids, priority, needs);
        }
    }

    /// Answer `ids` (`Updated`, `Removed`, `Unreadable`), as far as
    /// `needs` reads (`hydrate_in`), those it does not name yet read once
    /// it is listed. What it could not answer as it was handed on to the
    /// folder its listing opened again (`reopen`), for that one to answer:
    /// never dropped while their rows show. Nothing once lost: its rows
    /// went with it.
    fn hydrate(&self, ids: &[EntryId], priority: Priority, needs: Needs) -> Option<Vec<EntryId>> {
        match self.lost.load(Ordering::Acquire) {
            LISTED => {}
            HANDED_ON => return Some(ids.to_vec()),
            _ => return None,
        }
        // Not there to read now (its volume going), or not to be read (a
        // drive Windows lets go of, `letting_go`): answered, its rows as
        // listed.
        let folder = if self.letting_go() {
            None
        } else {
            self.opened().ok()
        };
        let Some(folder) = folder else {
            // Asked for again by the listing again that follows (`unread`).
            let unread: Vec<EntryId> = {
                let mut names = self.names.lock();
                let unread: Vec<EntryId> = ids
                    .iter()
                    .copied()
                    .filter(|id| names.by_id.contains_key(id))
                    .collect();
                for id in &unread {
                    let asked = names.unread.entry(*id).or_insert(needs);
                    *asked = (*asked).max(needs);
                }
                unread
            };
            if !unread.is_empty() {
                self.send(ListingEvent::Unreadable(unread));
            }
            return self.handed_on(ids);
        };
        self.hydrate_in(&folder, ids, priority, needs)
    }

    /// [`Shared::hydrate`] in `folder` as a read already found it (the
    /// listing's own, which stats every entry before it completes): a
    /// folder read by its path is opened once for that read, never once
    /// for each batch of it. Each entry is stat-ed, and filled by the
    /// metadata providers where `needs` reads what they say: an order by
    /// a stat alone waits for no more (ADR-121, amended 2026-11-24), what
    /// they say read when its row is drawn (`hydrate`).
    fn hydrate_in(
        &self,
        folder: &Dir,
        ids: &[EntryId],
        priority: Priority,
        needs: Needs,
    ) -> Option<Vec<EntryId>> {
        if self.is_lost() {
            return self.handed_on(ids);
        }
        // Dropped last, after what this read sends.
        let _reading = self.reading();
        let mut stated = Vec::with_capacity(ids.len());
        // What a stat found gone or could not read: its id, its name, its
        // stat's ticket.
        let (mut gone, mut unread) = (Vec::new(), Vec::new());
        let mut early = Vec::new();
        for &id in ids {
            if self.ctx.cancel.is_cancelled() {
                return None;
            }
            let name = {
                let names = self.names.lock();
                let name = names.by_id.get(&id).cloned();
                // Not listed yet (a row of the folder it opened again in
                // place of): read once it is.
                if name.is_none() && !self.listed.load(Ordering::Acquire) {
                    early.push(id);
                }
                name
            };
            let Some(name) = name else {
                continue;
            };
            let mut e = Entry::new(id, name, EntryKind::Unknown);
            let ticket = self.ticket();
            match stat_entry_as(folder, &mut e) {
                Stated::Found => {
                    // Written still as the watcher last said: a stat of it
                    // says so too, rather than drop its mark until told.
                    if !e.is_dir() && self.writing.lock().contains(&e.name) {
                        e.metadata.set(
                            crate::entry::keys::WRITING,
                            crate::entry::Value::Bool(true),
                            crate::entry::Provenance::Filesystem,
                        );
                    }
                    stated.push((id, e, ticket));
                }
                Stated::Gone => gone.push((id, e.name, ticket)),
                // There, unread: answered, its row as listed.
                Stated::Unread => unread.push((id, e.name, ticket)),
            }
        }
        let (mut updated, mut rekeyed): (Vec<Entry>, Vec<Entry>) = (Vec::new(), Vec::new());
        // Ids a stat found another file under: a provisional one, or a
        // file another took the place of (an editor saving).
        let mut stale = Vec::new();
        // Each entry's stat's ticket, noted as told once it is
        // (`send_stated`), and what is to be read again.
        let mut tickets = hashing::Map::default();
        let mut again = Vec::new();
        // Of two stats of a name, the one begun later stands, whatever
        // each found (`Names::applies`): a file the watcher found back
        // since is not taken out by a stat that found it gone before.
        let (gone, unread) = {
            let mut names = self.names.lock();
            for (id, e, ticket) in stated {
                if !names.newest.applies(&e.name, ticket) {
                    continue;
                }
                if e.id == id {
                    tickets.insert(id, ticket);
                    updated.push(e);
                } else if names.by_id.contains_key(&id) {
                    names.rekey(id, e.id);
                    tickets.insert(e.id, ticket);
                    stale.push(id);
                    rekeyed.push(e);
                }
            }
            let gone: Vec<EntryId> = gone
                .into_iter()
                .filter(|(_, name, ticket)| names.newest.applies(name, *ticket))
                .map(|(id, _, _)| id)
                .collect();
            for id in &gone {
                names.remove_id(*id);
            }
            let unread: Vec<EntryId> = unread
                .into_iter()
                .filter(|(_, name, ticket)| names.newest.applies(name, *ticket))
                .map(|(id, _, _)| id)
                .collect();
            (gone, unread)
        };
        let dir = self.dir();
        let fills = needs >= Needs::Providers;
        if !updated.is_empty() {
            if fills {
                self.fill(&dir, &mut updated);
            }
            again.extend(self.send_stated(ListingEvent::Updated(updated), &tickets));
        }
        if !gone.is_empty() {
            self.send(ListingEvent::Removed(gone));
        }
        if !unread.is_empty() {
            self.send(ListingEvent::Unreadable(unread));
        }
        // The row under the name is the file found there, in its place
        // and its selection.
        if !rekeyed.is_empty() {
            if fills {
                self.fill(&dir, &mut rekeyed);
            }
            again.extend(self.send_stated(
                ListingEvent::Renamed(stale.into_iter().zip(rekeyed).collect()),
                &tickets,
            ));
        }
        // Filled, and left out for a stat alone begun later: read again,
        // its row filled as asked.
        if !again.is_empty() {
            let _ = self.hydrate(&again, priority, Needs::Providers);
        }
        if !early.is_empty() {
            let mut names = self.names.lock();
            if self.listed.load(Ordering::Acquire) {
                // Listed meanwhile: read now (handed on, all of `ids` are
                // asked again below).
                drop(names);
                let _ = self.hydrate(&early, priority, needs);
            } else {
                names.early.push((early, priority, needs));
            }
        }
        self.handed_on(ids)
    }

    /// `ids` where it was handed on (`hand_on`) as it answered them: what
    /// it sent of them since may have been refused, so all are asked again
    /// of the next folder.
    fn handed_on(&self, ids: &[EntryId]) -> Option<Vec<EntryId>> {
        (self.lost.load(Ordering::Acquire) == HANDED_ON).then(|| ids.to_vec())
    }

    /// The directory itself moved: find where through its descriptor, which
    /// followed it, and tell its holder, which shows it there
    /// (`ListingEvent::Moved`). A directory that is gone empties the
    /// listing.
    fn follow_self(&self) -> bool {
        let held = match &*self.folder.read() {
            Folder::Held(held) => Some(Arc::clone(held)),
            Folder::ByPath => None,
        };
        let now = match held {
            Some(held) => held.0.path_now(),
            None => self.renamed_to(),
        };
        let Some(now) = now else {
            return false;
        };
        let before = self.dir();
        let now = spelled_as(&before, now);
        if now != before {
            self.dir.write().clone_from(&now);
            self.send(ListingEvent::Moved(now));
        }
        true
    }

    /// Whether the directory was removed. inotify cannot say so while we
    /// hold it open (the inode outlives its name until we close it), but a
    /// removed directory has no links left; its children's removal is the
    /// event that brings us here.
    fn is_unlinked(&self, folder: &Opened) -> bool {
        match folder {
            Opened::Held(held) => held.0.is_unlinked(),
            // Read by its path: another folder there now is this one gone.
            Opened::Now { dir, .. } => dir.identity().is_ok_and(|id| id != self.id),
        }
    }

    /// The folder as a read finds it: the one held, or opened from its
    /// path now.
    fn opened(&self) -> std::io::Result<Opened> {
        if let Folder::Held(held) = &*self.folder.read() {
            return Ok(Opened::Held(Arc::clone(held)));
        }
        let dir = self.dir();
        #[cfg(test)]
        self.opens.fetch_add(1, Ordering::Relaxed);
        let opened = Dir::open(&dir)?;
        // Counted by where the system resolved it too: a read through a
        // link holds the volume the link leads to.
        let held = Held::new(&dir, &self.at);
        Ok(Opened::Now {
            dir: opened,
            _held: held,
        })
    }

    /// Where a folder read by its path went when it moved: the folder of
    /// its folder of the same identity, renamed within it; `None` where
    /// none is (moved elsewhere, removed).
    fn renamed_to(&self) -> Option<PathBuf> {
        let before = self.dir();
        let parent = Dir::open(before.parent()?).ok()?;
        let mut names = parent.read().ok()?;
        names.find_map(|ent| {
            let ent = ent.ok()?;
            if !matches!(ent.kind, EntryKind::Directory | EntryKind::Unknown) {
                return None;
            }
            let mut e = Entry::new(EntryId::UNKNOWN, ent.name.clone(), EntryKind::Unknown);
            (stat_entry_as(&parent, &mut e) == Stated::Found && e.file() == self.id)
                .then(|| before.with_file_name(&ent.name))
        })
    }

    /// Hold nothing of it from now on: its volume is about to be let go
    /// of (ADR-149, amended). What it held closes in the shared lane.
    fn hold_nothing(&self) {
        let before = std::mem::replace(&mut *self.folder.write(), Folder::ByPath);
        if let Folder::Held(held) = before {
            self.ctx.scheduler.spawn_in(
                Lane::SHARED,
                Priority::Background,
                "fs.close",
                CancelToken::new(),
                move || drop(held),
            );
        }
    }

    fn lose_self(self: &Arc<Self>) {
        self.lose(crate::ListingError::missing(crate::words!(
            fs_folder_gone = "This folder was deleted or is no longer available.",
            note = "Shown in place of a folder's files: the folder shown went away (deleted, or its drive or server let go)."
        )));
    }

    /// Whether it was lost ([`Shared::lose`]) or handed on
    /// ([`Shared::hand_on`]): nothing of it is sent any more.
    fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire) != LISTED
    }

    /// The folder is gone, or its volume was let go of: its entries go,
    /// its listing fails with `error`, and its listing lets go of it, its
    /// directory and its watch closed in the shared lane, so a refresh
    /// opens the folder again from its path. Its volume gone is said over
    /// a deletion told before, never the other way; one handed on says
    /// nothing. Said under the names' lock, as every event it sends
    /// ([`Shared::send`]): what another worker sends after is refused, and
    /// two losses told at once leave the stronger last.
    fn lose(self: &Arc<Self>, error: crate::ListingError) {
        let rank = if error.kind == crate::location::Failure::Left {
            LOST_LEFT
        } else {
            LOST_MISSING
        };
        let first = {
            let mut names = self.names.lock();
            let before = self.lost.load(Ordering::Acquire);
            if before == HANDED_ON || before >= rank {
                return;
            }
            self.lost.store(rank, Ordering::Release);
            if before == LISTED {
                let ids = std::mem::take(&mut *names).ids();
                if !ids.is_empty() {
                    self.tx.send(ListingEvent::Removed(ids));
                }
            }
            self.tx.send(ListingEvent::Error(error));
            before == LISTED
        };
        if first {
            let taken = self.slot.upgrade().and_then(|slot| {
                let mut slot = slot.lock();
                slot.as_ref()
                    .is_some_and(|held| Arc::ptr_eq(held, self))
                    .then(|| slot.take())
                    .flatten()
            });
            if let Some(taken) = taken {
                let_go(&self.ctx, taken);
            }
        }
    }

    /// Hand it on to the listing that opens its path again
    /// (`OpenLocation::reopen`): nothing more of it is sent, under the
    /// lock every event it sends is sent under. The entries it listed, for
    /// the next listing to take out those it lists no more; `None` when it
    /// was lost before, its entries gone with it.
    fn hand_on(&self) -> Option<Vec<EntryId>> {
        let early = {
            let mut names = self.names.lock();
            if self.is_lost() {
                return None;
            }
            self.lost.store(HANDED_ON, Ordering::Release);
            (names.ids(), std::mem::take(&mut names.early))
        };
        // What was asked of it and not read yet, asked of the next one.
        let (ids, early) = early;
        for (asked, priority, needs) in early {
            self.hand_over(asked, priority, needs);
        }
        Some(ids)
    }

    /// Whether `dir` leads to this folder still, and its watch goes on:
    /// what a volume mounted at or above it (`reopen`) leaves as it is.
    /// Opens `dir`: on a worker, in its lane.
    fn is_at(&self, dir: &Path) -> bool {
        !self.is_lost()
            && self.watcher.lock().is_some()
            && Dir::open(dir)
                .and_then(|folder| folder.identity())
                .is_ok_and(|id| id == self.id)
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
        if self.is_lost() {
            return;
        }
        // A drive Windows lets go of (its Eject asked, ADR-149, 16): what
        // its watch told before it was let go of is not read there, which
        // would hold the drive. Should the drive stay, the rescan it is told
        // once watched again covers it; gone, it is told its folder went.
        if self.letting_go() {
            return;
        }
        // Its filesystem let go of by its automounter, unused a while
        // (ADR-149): no folder going, as it is mounted again as it is
        // looked up. Nothing of its path is read now (that would mount it);
        // it stays shown, read by its path, its watch ended with the
        // filesystem, and a mount made there again opens it anew (`is_at`).
        if c.unmounted && super::reach::on_demand(&self.dir()) {
            drop(self.watcher.lock().take());
            return;
        }
        if c.deleted || (c.moved && !self.follow_self()) {
            self.lose_self();
            return;
        }
        // Opened once for what follows: read by its path, the folder there
        // now tells whether this one went.
        let folder = match self.opened() {
            Ok(folder) => folder,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.lose_self();
                return;
            }
            // Not there to read now: its watch or the mounts say what
            // comes next.
            Err(_) => return,
        };
        if self.is_unlinked(&folder) {
            self.lose_self();
            return;
        }
        if c.itself {
            self.send(ListingEvent::Changed);
        }
        if c.rescan {
            let prev = self.names.lock().ids();
            // A rescan says nothing of what is written still, and a name
            // re-listed keeps what its row held: those marked are told
            // again, unmarked, as any change of them is.
            let marked = std::mem::take(&mut *self.writing.lock());
            self.list(&self.ctx.cancel.child(), Some(prev), false);
            if !marked.is_empty() {
                self.apply_changes(watch::Changes {
                    names: marked,
                    ..watch::Changes::default()
                });
            }
            return;
        }
        let mut added = Vec::new();
        let mut removed = Vec::new();
        // Names another file took (a rename onto them), each with the id
        // it held.
        let mut replaced = Vec::new();
        // A companion keeps its file's notes (ADR-028: a Mac's `._x`, a
        // sidecar): when it changes, the file's notes did.
        let writing = c.writing;
        {
            let mut marked = self.writing.lock();
            for name in &c.names {
                if writing.contains(name) {
                    marked.insert(name.clone());
                } else {
                    marked.remove(name);
                }
            }
        }
        let mut names = c.names;
        let owners: Vec<std::ffi::OsString> = names
            .iter()
            .filter_map(|n| self.ctx.companions.owner(n))
            .collect();
        names.extend(owners);
        // Dropped last, after what this read sends.
        let _reading = self.reading();
        // Each entry's stat's ticket (`send_stated`): taken after what is
        // written was noted above, as a hydration's after it reads it.
        let mut tickets = hashing::Map::default();
        for name in names {
            if self.ctx.cancel.is_cancelled() {
                return;
            }
            let mut e = Entry::new(EntryId::UNKNOWN, name.clone(), EntryKind::Unknown);
            let ticket = self.ticket();
            let stated = stat_entry_as(&folder, &mut e);
            if stated == Stated::Unread {
                // There, unread: what is listed of it stays.
                continue;
            }
            // Another program writes it still (ADR-135).
            if writing.contains(&name) && !e.is_dir() {
                e.metadata.set(
                    crate::entry::keys::WRITING,
                    crate::entry::Value::Bool(true),
                    crate::entry::Provenance::Filesystem,
                );
            }
            let mut names = self.names.lock();
            // A stat of it begun later applied already (a hydration's that
            // found it gone, or another file there): it stands.
            if !names.newest.applies(&name, ticket) {
                continue;
            }
            if stated == Stated::Found {
                // A rename onto this name leaves the id it replaced stale.
                if let Some(stale) = names.insert(e.id, name) {
                    removed.push(stale);
                    replaced.push((stale, e.id));
                }
                tickets.insert(e.id, ticket);
                added.push(e);
            } else if let Some(id) = names.remove_name(&name) {
                removed.push(id);
            }
        }
        let (removed, mut renamed, mut added) = pair_changes(removed, added, replaced);
        if !removed.is_empty() {
            self.send(ListingEvent::Removed(removed));
        }
        // Changed entries arrive stat-ed; give providers their turn so
        // fields such as tags survive the update.
        if !renamed.is_empty() {
            let (olds, mut news): (Vec<EntryId>, Vec<Entry>) = renamed.drain(..).unzip();
            self.fill(&self.dir(), &mut news);
            let again = self.send_stated(
                ListingEvent::Renamed(olds.into_iter().zip(news).collect()),
                &tickets,
            );
            self.ask(&again, Priority::Visible, Needs::Providers);
        }
        if !added.is_empty() {
            self.fill(&self.dir(), &mut added);
            let again = self.send_stated(ListingEvent::Added(added), &tickets);
            self.ask(&again, Priority::Visible, Needs::Providers);
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

/// What a batch of a watcher's changes sends, from the ids that went
/// (`removed`), the entries that came (`added`) and the names another file
/// took (`replaced`: the id each held, and the one it holds now): what
/// still went, the renames, and what still came. Each id and entry is
/// found by a map, never by a walk of the others: a bulk rename lands
/// thousands of changes in one batch.
fn pair_changes(
    mut removed: Vec<EntryId>,
    mut added: Vec<Entry>,
    replaced: Vec<(EntryId, EntryId)>,
) -> (Vec<EntryId>, Vec<(EntryId, Entry)>, Vec<Entry>) {
    let arrived: hashing::Set<EntryId> = added.iter().map(|e| e.id).collect();
    removed.retain(|id| !arrived.contains(id));
    // A file renamed within the folder is gone under its old name and come
    // under its new one: a rename, so its row keeps its place and its
    // selection.
    let mut renamed = renames(&mut removed, &mut added);
    // A name another file took is its row still, unless its file was
    // renamed: an editor saves by writing a new file and renaming it onto
    // the name (gedit, Kate, VS Code), and what was selected there stays
    // so, as in Dolphin and Nautilus. A file renamed onto it from beside it
    // is both.
    if !replaced.is_empty() {
        let mut removed_at: hashing::Map<EntryId, usize> = hashing::Map::default();
        for (i, id) in removed.iter().enumerate() {
            removed_at.entry(*id).or_insert(i);
        }
        let mut added_at: hashing::Map<EntryId, usize> = hashing::Map::default();
        for (i, e) in added.iter().enumerate() {
            added_at.entry(e.id).or_insert(i);
        }
        let mut renamed_at: hashing::Map<EntryId, usize> = hashing::Map::default();
        for (i, (_, e)) in renamed.iter().enumerate() {
            renamed_at.entry(e.id).or_insert(i);
        }
        let mut went = vec![false; removed.len()];
        let mut came: Vec<Option<Entry>> = added.into_iter().map(Some).collect();
        for (stale, now) in replaced {
            let Some(r) = removed_at.remove(&stale) else {
                continue;
            };
            went[r] = true;
            let entry = match added_at.remove(&now).and_then(|a| came[a].take()) {
                Some(entry) => entry,
                None => match renamed_at.get(&now) {
                    Some(&at) => renamed[at].1.clone(),
                    None => continue,
                },
            };
            renamed_at.entry(now).or_insert(renamed.len());
            renamed.push((stale, entry));
        }
        let mut went = went.into_iter();
        removed.retain(|_| !went.next().unwrap_or(false));
        added = came.into_iter().flatten().collect();
    }
    (removed, renamed, added)
}

/// The renames among a batch's changes, taken out of `removed` and
/// `added`: a file one of whose names went and one came. A file that lost
/// or gained several names at once is left as it is: which name became
/// which cannot be told. What is left keeps its order.
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
    // Where each file gone under one name went: found by its file, not by a
    // walk of `removed` for each one that came.
    let mut gone_at: hashing::Map<FileId, usize> = removed
        .iter()
        .enumerate()
        .filter(|(_, id)| one(&gone, id.file))
        .map(|(i, id)| (id.file, i))
        .collect();
    let mut went = vec![false; removed.len()];
    let mut renamed = Vec::new();
    let mut left = Vec::with_capacity(added.len());
    for e in std::mem::take(added) {
        let file = e.file();
        match gone_at.remove(&file).filter(|_| one(&came, file)) {
            Some(at) => {
                went[at] = true;
                renamed.push((removed[at], e));
            }
            None => left.push(e),
        }
    }
    *added = left;
    let mut went = went.into_iter();
    removed.retain(|_| !went.next().unwrap_or(false));
    renamed
}

impl OpenLocation for PathLocation {
    fn location(&self) -> &Location {
        &self.location
    }

    fn holds(&self) -> Option<PathBuf> {
        Some(self.held.read().clone().unwrap_or_else(|| self.dir.clone()))
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }

    /// A job per batch: a request for a whole folder (a sort by size asks
    /// for every entry) is stat-ed by several workers at once, and its
    /// answers arrive a batch at a time, each an event the UI applies
    /// within a frame, not one holding them all.
    /// Asked while it opens its folder again (`reopen`), of the one it
    /// opens: every id is answered (`hydrate_held`).
    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        self.read(ids, Needs::Providers, priority);
    }

    /// As `hydrate`, its entries filled by the metadata providers only
    /// where `needs` reads what they say: an order by a date is made once
    /// its entries are stat-ed (ADR-121, amended 2026-11-24).
    fn read(&self, ids: &[EntryId], needs: Needs, priority: Priority) {
        if ids.is_empty() {
            return;
        }
        hydrate_held(&self.shared, &self.opening, ids.to_vec(), priority, needs);
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

    /// Lost with its volume, an open under way too: told in `Opening`
    /// first, then the folder held looked at, as an open looks at `Opening`
    /// once it holds its folder (`open_folder`).
    fn volume_gone(&self, error: crate::ListingError) {
        self.opening.lock().gone = Some(error.clone());
        let shared = self.shared.lock().clone();
        match shared {
            Some(shared) => shared.lose(error),
            // Lost already, never opened, or an open under way, which is
            // lost as it ends: said now all the same, which says more than
            // a deletion told before.
            None => {
                self.tx.send(ListingEvent::Error(error));
            }
        }
    }

    /// Holds nothing from now on, what an open under way finds too: told
    /// in `Opening` first, then the folder held looked at (`volume_gone`).
    fn hold_nothing(&self) -> bool {
        self.opening.lock().hold_nothing = true;
        if let Some(shared) = self.shared.lock().clone() {
            shared.hold_nothing();
        }
        true
    }

    /// A volume was mounted at or above it (ADR-149): where its path
    /// leads to another folder now than the one it holds (the mount's
    /// own, an empty mount point's no more), or its watch ended with a
    /// filesystem, what it held is handed on and the folder opened again
    /// from its path, the entries it listed taken out as the new ones are
    /// listed (no empty frame between); one opened since the mount was made
    /// (by the sidebar's Mount, which shows it) is left as it is. Looked
    /// at on a worker, in its lane.
    fn reopen(&self) {
        {
            let mut opening = self.opening.lock();
            if self.ctx.cancel.is_cancelled() {
                return;
            }
            // A volume mounted there: what it is now decides.
            opening.gone = None;
            opening.hold_nothing = false;
            // The open under way may have found the folder before: it
            // looks again once done.
            if opening.under_way {
                opening.again = true;
                return;
            }
            opening.under_way = true;
        }
        self.spawn_open(true);
    }

    fn close(&self) {
        self.ctx.cancel.cancel();
        if let Some(shared) = self.shared.lock().take() {
            let_go(&self.ctx, shared);
        }
    }
}

/// Let go of a folder held open: its watcher and its directory closed off
/// the UI, and out of the remote's lane: a server that stopped answering
/// holds its lane, and closing reads nothing of it.
fn let_go(ctx: &OpenContext, shared: Arc<Shared>) {
    ctx.scheduler.spawn_in(
        Lane::SHARED,
        Priority::Background,
        "fs.close",
        CancelToken::new(),
        move || drop(shared),
    );
}

impl Drop for PathLocation {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The events of `open` until one `done` says ends them, 20 s at most.
    #[cfg(target_os = "linux")]
    fn until(
        open: &Arc<dyn crate::location::OpenLocation>,
        done: impl Fn(&ListingEvent) -> bool,
    ) -> Vec<ListingEvent> {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let event = open.events().recv_timeout(left).expect("told in time");
            let end = done(&event);
            seen.push(event);
            if end {
                return seen;
            }
        }
    }

    /// `dir` opened as a pane opens it, the listing itself at hand.
    fn opened_here(app: &crate::App, dir: &Path, reads: Needs) -> Arc<PathLocation> {
        opened_on(app, app.scheduler.clone(), dir, reads)
    }

    /// `dir` opened as a pane opens it, its work on `scheduler`.
    fn opened_on(
        app: &crate::App,
        scheduler: crate::scheduler::Scheduler,
        dir: &Path,
        reads: Needs,
    ) -> Arc<PathLocation> {
        let ctx = OpenContext {
            scheduler,
            cancel: CancelToken::new(),
            batch_size: 64,
            waker: None,
            metadata: Arc::clone(&app.metadata),
            fields: Arc::clone(&app.fields),
            hints: Arc::clone(&app.hints),
            names: Arc::clone(&app.names),
            companions: Arc::clone(&app.companions),
            hidden_shown: false,
            reads,
        };
        PathLocation::open(Location::from_path(dir), dir.to_path_buf(), ctx)
    }

    /// The events `rx` holds until one `done` says ends them, 20 s at most.
    fn events_until(
        rx: &Receiver<ListingEvent>,
        done: impl Fn(&ListingEvent) -> bool,
    ) -> Vec<ListingEvent> {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let event = rx.recv_timeout(left).expect("told in time");
            let end = done(&event);
            seen.push(event);
            if end {
                return seen;
            }
        }
    }

    /// A folder lost says nothing more (ADR-149): what a worker of it
    /// still sends after (the `Complete` a first listing sends once it
    /// stat-ed every entry, an entry a change found) is refused, never
    /// saying over why it was lost; its volume gone says over a deletion
    /// told before, never the other way; one handed on to the listing that
    /// opened its path again says nothing at all.
    #[test]
    fn a_folder_lost_says_nothing_more() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("it's lost, été");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let open = opened_here(&app, &dir, Needs::Providers);
        events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        let shared = open.shared.lock().clone().expect("listed");

        shared.lose(crate::ListingError::missing(
            crate::words::Words::as_written("deleted"),
        ));
        shared.lose(crate::ListingError::left(crate::words::Words::as_written(
            "unmounted",
        )));
        shared.lose(crate::ListingError::missing(
            crate::words::Words::as_written("deleted, told after"),
        ));
        assert!(!shared.send(ListingEvent::Complete), "refused once lost");
        assert!(!shared.send(ListingEvent::Added(vec![Entry::new(
            EntryId::UNKNOWN,
            "late.txt",
            EntryKind::File,
        )])));
        let told: Vec<ListingEvent> = open.rx.try_iter().collect();
        let kinds: Vec<_> = told
            .iter()
            .filter_map(|e| match e {
                ListingEvent::Error(error) => Some(error.kind),
                ListingEvent::Complete | ListingEvent::Added(_) => panic!("told after: {e:?}"),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            [
                crate::location::Failure::Missing,
                crate::location::Failure::Left
            ]
        );
        assert!(matches!(told.first(), Some(ListingEvent::Removed(ids)) if ids.len() == 1));
        assert!(open.shared.lock().is_none(), "let go of by its listing");

        // Handed on: nothing more, not even a loss.
        let again = opened_here(&app, &dir, Needs::Nothing);
        events_until(&again.rx, |e| matches!(e, ListingEvent::Complete));
        let shared = again.shared.lock().clone().expect("listed");
        assert_eq!(shared.hand_on().map(|ids| ids.len()), Some(1));
        shared.lose(crate::ListingError::left(crate::words::Words::as_written(
            "unmounted",
        )));
        assert!(!shared.send(ListingEvent::Complete));
        assert!(again.rx.try_iter().next().is_none(), "nothing said");
        open.close();
        again.close();
    }

    /// A scheduler of one worker, held by a job until the sender it gives
    /// is dropped or sent to: what is asked of it meanwhile waits.
    fn held_scheduler() -> (crate::scheduler::Scheduler, std::sync::mpsc::Sender<()>) {
        let scheduler = crate::scheduler::Scheduler::new(1);
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (taken, started) = std::sync::mpsc::channel::<()>();
        scheduler.spawn_in(
            Lane::SHARED,
            Priority::Visible,
            "test.hold",
            CancelToken::new(),
            move || {
                let _ = taken.send(());
                let _ = wait.recv_timeout(Duration::from_secs(20));
            },
        );
        started
            .recv_timeout(Duration::from_secs(20))
            .expect("the worker holds");
        (scheduler, release)
    }

    /// A folder whose volume goes while it is opened (ADR-149): what the
    /// open finds is lost at once, saying its volume went, never listed
    /// after; one whose volume is about to be let go of holds nothing of
    /// it once opened.
    #[test]
    fn an_open_under_way_is_held_to_what_was_decided_of_its_volume() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("it's opening, été");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();

        let (scheduler, release) = held_scheduler();
        let open = opened_on(&app, scheduler, &dir, Needs::Nothing);
        open.volume_gone(crate::ListingError::left(crate::words::Words::as_written(
            "unmounted",
        )));
        drop(release);
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Error(_)));
        assert!(
            matches!(told.last(), Some(ListingEvent::Error(e)) if e.kind == crate::location::Failure::Left)
        );
        // The open ends lost: nothing listed after, its folder let go of.
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while open.opening.lock().under_way {
            assert!(std::time::Instant::now() < deadline, "never opened");
            std::thread::sleep(Duration::from_millis(5));
        }
        for event in open.rx.try_iter() {
            assert!(
                matches!(&event, ListingEvent::Error(e) if e.kind == crate::location::Failure::Left),
                "told after: {event:?}"
            );
        }
        assert!(open.shared.lock().is_none());
        open.close();

        let (scheduler, release) = held_scheduler();
        let open = opened_on(&app, scheduler, &dir, Needs::Nothing);
        assert!(open.hold_nothing());
        drop(release);
        events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        let shared = open.shared.lock().clone().expect("listed");
        assert!(matches!(*shared.folder.read(), Folder::ByPath), "held");
        open.close();
    }

    /// A folder opened again as a volume is mounted at or above it
    /// (ADR-149) is listed again only where its path leads elsewhere now:
    /// one opened since the mount was made, which a pane shows once its
    /// Mount is done, is left as it is, never listed twice.
    #[test]
    fn a_folder_opened_again_where_its_path_leads_to_it_is_left_as_it_is() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("it's mounted, été");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let open = opened_here(&app, &dir, Needs::Nothing);
        events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        let before = open.shared.lock().clone().expect("listed");
        open.reopen();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while open.opening.lock().under_way {
            assert!(std::time::Instant::now() < deadline, "never looked");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(open.rx.try_iter().next().is_none(), "listed again");
        let after = open.shared.lock().clone().expect("held still");
        assert!(Arc::ptr_eq(&before, &after));
        open.close();
    }

    /// What is asked to be read of a folder as it is opened again in place
    /// (its watch ended, `reopen`) is answered by the folder opened, never
    /// dropped with the one handed on: its rows show still, and what asked
    /// asks no more (AGENTS.md, "a hydration answers every id it is
    /// given").
    #[test]
    fn a_hydration_asked_as_a_folder_is_opened_again_is_answered() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("it's read, été");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let scheduler = crate::scheduler::Scheduler::new(1);
        let open = opened_on(&app, scheduler.clone(), &dir, Needs::Nothing);
        let listed = events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        let ids: Vec<EntryId> = listed
            .iter()
            .filter_map(|e| match e {
                ListingEvent::Added(entries) => Some(entries.iter().map(|e| e.id)),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(ids.len(), 1);
        // Its watch ended (its filesystem let go of on demand, VC-4): a
        // volume mounted there opens it again, the same folder.
        drop(open.shared.lock().as_ref().unwrap().watcher.lock().take());
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (taken, started) = std::sync::mpsc::channel::<()>();
        scheduler.spawn_in(
            Lane::SHARED,
            Priority::Visible,
            "test.hold",
            CancelToken::new(),
            move || {
                let _ = taken.send(());
                let _ = wait.recv_timeout(Duration::from_secs(20));
            },
        );
        started.recv_timeout(Duration::from_secs(20)).unwrap();
        // Opened again first, then read: the folder asked is handed on by
        // then.
        open.reopen();
        open.hydrate(&ids, Priority::Background);
        drop(release);
        let told = events_until(
            &open.rx,
            |e| matches!(e, ListingEvent::Updated(entries) if entries.iter().any(|e| e.id == ids[0])),
        );
        assert!(
            !told.iter().any(|e| matches!(e, ListingEvent::Removed(_))),
            "{told:?}"
        );
        open.close();
    }

    /// A folder opened again whose listing fails once it is open (read by
    /// its path, it cannot be read again) takes out the rows it was handed
    /// on, another folder's, never left beside its failure; a refresh that
    /// fails keeps its own, the files it read still shown under the error.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_listing_that_fails_takes_out_only_what_was_handed_on() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let volume = tmp.path().join("a server");
        let shown = volume.join("it's shown, été");
        std::fs::create_dir_all(&shown).unwrap();
        std::fs::write(shown.join("a.txt"), "a").unwrap();
        crate::fs::reach::pretend_remote(&volume);
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let open = opened_here(&app, &shown, Needs::Nothing);
        events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        let shared = open.shared.lock().clone().expect("listed");
        assert!(matches!(*shared.folder.read(), Folder::ByPath));
        // Read by its path, it cannot be opened for its listing now.
        std::fs::set_permissions(&shown, std::fs::Permissions::from_mode(0o000)).unwrap();
        if Dir::open(&shown).is_ok() {
            // As root, whom no mode refuses: nothing to try here.
            std::fs::set_permissions(&shown, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        let another = EntryId::in_folder(FileId::new(1, 2), FileId::new(1, 3), OsStr::new("b"));

        shared.list(&CancelToken::new(), Some(vec![another]), true);
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Error(_)));
        assert!(
            told.iter()
                .any(|e| matches!(e, ListingEvent::Removed(ids) if *ids == [another])),
            "{told:?}"
        );

        shared.list(&CancelToken::new(), Some(vec![another]), false);
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Error(_)));
        assert!(
            !told.iter().any(|e| matches!(e, ListingEvent::Removed(_))),
            "{told:?}"
        );
        std::fs::set_permissions(&shown, std::fs::Permissions::from_mode(0o755)).unwrap();
        open.close();
    }

    /// A listing that fails part way takes out only the rows handed on it
    /// did not list again: one it named before it failed (the same folder
    /// opened again, its ids the same) stays, whether sent or not.
    #[test]
    fn a_listing_that_fails_part_way_keeps_what_it_listed_again() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("it's listed, été");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let open = opened_here(&app, &dir, Needs::Nothing);
        let listed: Vec<EntryId> = events_until(&open.rx, |e| matches!(e, ListingEvent::Complete))
            .iter()
            .filter_map(|e| match e {
                ListingEvent::Added(entries) => Some(entries.iter().map(|e| e.id)),
                _ => None,
            })
            .flatten()
            .collect();
        let shared = open.shared.lock().clone().expect("listed");
        let another = EntryId::in_folder(FileId::new(1, 2), FileId::new(1, 3), OsStr::new("b"));
        let mut taken = listed.clone();
        taken.push(another);
        shared.fail(&std::io::Error::other("broken"), Some(taken));
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Error(_)));
        assert!(
            told.iter()
                .any(|e| matches!(e, ListingEvent::Removed(ids) if *ids == [another])),
            "{told:?}"
        );
        open.close();
    }

    /// A folder opened again whose path leads nowhere now takes out what
    /// it listed before, never left beside its failure (ADR-149).
    #[cfg(unix)]
    #[test]
    fn a_folder_opened_again_that_fails_takes_out_what_it_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first");
        std::fs::create_dir(&first).unwrap();
        std::fs::write(first.join("it's one.txt"), "1").unwrap();
        let link = tmp.path().join("it leads, été");
        std::os::unix::fs::symlink(&first, &link).unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let open = opened_here(&app, &link, Needs::Nothing);
        events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("nowhere"), &link).unwrap();
        open.reopen();
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Error(_)));
        assert!(
            told.iter()
                .any(|e| matches!(e, ListingEvent::Removed(ids) if ids.len() == 1)),
            "{told:?}"
        );
        open.close();
    }

    /// Run in a user and mount namespace of the test's own, where it
    /// mounts as root (`a_folder_mounted_on_demand_stays_as_its_mount_expires`).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_folder_follows_its_mount_expiring() {
        if !crate::testing::in_namespace() {
            crate::testing::as_root_in_a_namespace(
                "fs::path_provider::tests::a_folder_mounted_on_demand_stays_as_its_mount_expires",
            );
        }
    }

    /// A folder whose filesystem an automounter lets go of, unused a while
    /// (autofs, systemd's idle timeout: ADR-149), stays shown, saying
    /// nothing went, its watch ended with it; one on any other mount let go
    /// of is gone. Does nothing outside its namespace.
    #[cfg(target_os = "linux")]
    #[test]
    // Tests' own, on Linux: no console window to keep away.
    #[allow(clippy::disallowed_methods)]
    fn a_folder_mounted_on_demand_stays_as_its_mount_expires() {
        use std::process::Command;
        if !crate::testing::in_namespace() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let shown_on = |name: &str, on_demand: bool| {
            let point = tmp.path().join(name);
            std::fs::create_dir(&point).unwrap();
            assert!(
                Command::new("mount")
                    .args(["-t", "tmpfs", "skwir-test"])
                    .arg(&point)
                    .status()
                    .unwrap()
                    .success()
            );
            let shown = point.join("it's shown, été");
            std::fs::create_dir(&shown).unwrap();
            std::fs::write(shown.join("a.txt"), "a").unwrap();
            // Read by its path, as a remote's folder is: nothing of it held.
            crate::fs::reach::pretend_remote(&point);
            if on_demand {
                crate::fs::reach::pretend_on_demand(&point);
            }
            let open = opened_here(&app, &shown, Needs::Nothing);
            events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
            assert!(
                open.shared
                    .lock()
                    .as_ref()
                    .unwrap()
                    .watcher
                    .lock()
                    .is_some()
            );
            (point, open)
        };
        let umount = |point: &Path| {
            assert!(
                Command::new("umount")
                    .arg(point)
                    .status()
                    .unwrap()
                    .success()
            );
        };

        let (point, open) = shown_on("an automounted share", true);
        umount(&point);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while open
            .shared
            .lock()
            .as_ref()
            .unwrap()
            .watcher
            .lock()
            .is_some()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "its watch never ended"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let told: Vec<ListingEvent> = open.rx.try_iter().collect();
        assert!(
            !told
                .iter()
                .any(|e| matches!(e, ListingEvent::Error(_) | ListingEvent::Removed(_))),
            "{told:?}"
        );
        open.close();

        let (point, open) = shown_on("a drive", false);
        umount(&point);
        events_until(&open.rx, |e| matches!(e, ListingEvent::Error(_)));
        open.close();
    }

    /// What stats found is told only if no stat begun later was: an older
    /// stat after a newer is left out, read again where only it was
    /// filled by the providers (the newer a stat alone, for an order); a
    /// rename left out takes out the row it renames; what no stat found
    /// here (no ticket) and what is no entry's now pass as they are.
    #[test]
    fn of_two_stats_the_later_begun_stands() {
        let id = |ino| EntryId {
            file: FileId::new(1, ino),
            at: 0,
        };
        let entry = |ino, filled: bool| {
            let mut e = Entry::new(id(ino), format!("it's {ino}, été"), EntryKind::File);
            e.metadata.set(
                crate::entry::keys::SIZE,
                crate::entry::Value::UInt(ino),
                crate::entry::Provenance::Filesystem,
            );
            if filled {
                e.metadata.mark_filled();
            }
            e
        };
        let mut names = Names::default();
        for ino in 1..=3 {
            names.insert(id(ino), format!("it's {ino}, été").into());
        }
        let tickets = |of: &[(u64, u64)]| -> hashing::Map<EntryId, u64> {
            of.iter().map(|&(ino, ticket)| (id(ino), ticket)).collect()
        };
        let told = |(events, again): (Vec<ListingEvent>, Vec<EntryId>)| {
            let ids: Vec<EntryId> = events
                .iter()
                .flat_map(|e| match e {
                    ListingEvent::Updated(batch) | ListingEvent::Added(batch) => {
                        batch.iter().map(|e| e.id).collect::<Vec<_>>()
                    }
                    _ => Vec::new(),
                })
                .collect();
            (ids, again)
        };
        // A stat alone begun later told first: the filled one after it is
        // left out, and read again.
        let newer = names.newest(
            ListingEvent::Updated(vec![entry(1, false)]),
            &tickets(&[(1, 7)]),
        );
        assert_eq!(told(newer), (vec![id(1)], vec![]));
        let older = names.newest(
            ListingEvent::Updated(vec![entry(1, true), entry(2, true)]),
            &tickets(&[(1, 5), (2, 6)]),
        );
        assert_eq!(told(older), (vec![id(2)], vec![id(1)]), "1 read again");
        // Filled told, a stat alone after it from before: left out, as
        // the row holds more.
        let older = names.newest(
            ListingEvent::Updated(vec![entry(2, false)]),
            &tickets(&[(2, 4)]),
        );
        assert_eq!(told(older), (vec![], vec![]));
        // Later still: told, and filled still (the row keeps its fields).
        let later = names.newest(
            ListingEvent::Added(vec![entry(2, false)]),
            &tickets(&[(2, 9)]),
        );
        assert_eq!(told(later), (vec![id(2)], vec![]));
        assert!(Told(names.stated[&id(2)]).filled());
        // No ticket, or no entry's now: as they are, nothing noted.
        let bare = names.newest(
            ListingEvent::Added(vec![entry(3, false), entry(4, false)]),
            &tickets(&[(4, 1)]),
        );
        assert_eq!(told(bare).0, [id(3), id(4)]);
        assert!(!names.stated.contains_key(&id(3)) && !names.stated.contains_key(&id(4)));
        // A rename from a stat begun before the one told: its row goes.
        let (events, _) = names.newest(
            ListingEvent::Renamed(vec![(id(9), entry(2, true))]),
            &tickets(&[(2, 8)]),
        );
        assert!(
            matches!(&events[..], [ListingEvent::Renamed(kept), ListingEvent::Removed(gone)]
                if kept.is_empty() && gone == &[id(9)]),
            "{events:?}"
        );
    }

    /// An entry a hydration re-keyed whose rename is not told yet is told
    /// renamed by whatever tells it first: the watcher's stat of its name,
    /// begun later, told before the hydration's rename, renames the row
    /// (its place and its selection kept), and the rename left out after
    /// it takes out nothing left.
    #[test]
    fn a_rename_left_out_is_told_by_the_stat_that_left_it_out() {
        let id = |ino| EntryId {
            file: FileId::new(1, ino),
            at: 0,
        };
        let name = "it's saved, été";
        let entry = |ino| Entry::new(id(ino), name, EntryKind::File);
        let (old, new) = (id(1), id(2));
        let mut names = Names::default();
        names.insert(old, name.into());
        let mut listing = crate::Listing::default();
        listing.apply(ListingEvent::Added(vec![entry(1)]));
        // The hydration's stat (5) found another file under the name: its
        // row re-keyed, under the names' lock, then filled.
        names.rekey(old, new);
        // The watcher's stat of the name (6) is told first.
        let tickets = |ticket| std::iter::once((new, ticket)).collect::<hashing::Map<_, _>>();
        let (events, _) = names.newest(ListingEvent::Added(vec![entry(2)]), &tickets(6));
        for event in events.into_iter().flat_map(|e| as_now(&names, e)) {
            listing.apply(event);
        }
        assert_eq!(listing.take_renamed(), vec![(old, new)], "renamed");
        // The hydration's rename, from a stat begun before, told after.
        let (events, _) = names.newest(ListingEvent::Renamed(vec![(old, entry(2))]), &tickets(5));
        for event in events.into_iter().flat_map(|e| as_now(&names, e)) {
            listing.apply(event);
        }
        let rows: Vec<EntryId> = listing.iter().map(|e| e.id).collect();
        assert_eq!(rows, vec![new], "one row, renamed");
        assert!(listing.take_renamed().is_empty());
        assert!(names.untold.is_empty());
    }

    /// While Windows lets go of a drive (its Eject asked, ADR-149, 16), a
    /// folder listed there reads nothing of it: a row asked for is answered
    /// unread, as where it cannot be opened. The removal refused, the
    /// rescan told once it is watched again shows what changed meanwhile,
    /// and reads the row answered unread.
    #[cfg(windows)]
    #[test]
    fn a_drive_being_let_go_of_is_read_no_more_until_it_stays() {
        let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let dir = tmp.path().join("on a stick, l'été");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("it's there.txt"), "x").unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let open = opened_here(&app, &dir, Needs::Nothing);
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        let id = told
            .iter()
            .find_map(|e| match e {
                ListingEvent::Added(batch) => batch.first().map(|e| e.id),
                _ => None,
            })
            .expect("listed");
        let volume = crate::fs::reach::volume_of(&dir).expect("a drive");
        assert!(watch::let_go_on(&volume, Duration::from_secs(5)));
        assert!(watch::letting_go(&dir));
        open.hydrate(&[id], Priority::Visible);
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Unreadable(_)));
        assert!(
            !told.iter().any(|e| matches!(e, ListingEvent::Updated(_))),
            "nothing read: {told:?}"
        );
        // Its removal refused: watched again, and read again.
        std::fs::write(dir.join("it's new.txt"), "y").unwrap();
        watch::watch_again_on(&volume);
        assert!(!watch::letting_go(&dir));
        events_until(
            &open.rx,
            |e| matches!(e, ListingEvent::Added(batch) if batch.iter().any(|e| e.name == "it's new.txt")),
        );
        let told = events_until(
            &open.rx,
            |e| matches!(e, ListingEvent::Updated(batch) if batch.iter().any(|e| e.id == id)),
        );
        let read = told.iter().rev().find_map(|e| match e {
            ListingEvent::Updated(batch) => batch.iter().find(|e| e.id == id),
            _ => None,
        });
        assert!(
            read.is_some_and(|e| e.metadata.is_hydrated()
                && e.metadata.get(crate::entry::keys::UNREAD).is_none()),
            "read again: {read:?}"
        );
        open.close();
    }

    /// A metadata provider that, once armed, holds the next fill it is
    /// asked until let go: a hydration whose providers are slow.
    #[cfg(target_os = "linux")]
    struct SlowFill {
        armed: std::sync::atomic::AtomicBool,
        entered: Mutex<std::sync::mpsc::Sender<()>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    #[cfg(target_os = "linux")]
    impl crate::metadata::MetadataProvider for SlowFill {
        fn id(&self) -> &'static str {
            "held"
        }
        fn fill(&self, _: &Path, _: &mut [Entry]) {
            if self.armed.swap(false, Ordering::SeqCst) {
                let _ = self.entered.lock().send(());
                let _ = self.release.lock().recv_timeout(Duration::from_secs(20));
            }
        }
    }

    /// Of two stats of an entry, the one begun later stands, whichever is
    /// told last: a row hydrated while its file is written, its providers
    /// slow, is told after the watcher's stat of the file closed, and its
    /// mark and its size as they were then are left out; the row is marked
    /// no more, at its last size (ADR-135).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_stat_told_after_a_later_one_is_left_out() {
        use crate::entry::{Value, keys};
        use std::io::Write as _;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("l'été, written");
        std::fs::create_dir(&dir).unwrap();
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let (entered, on_entered) = std::sync::mpsc::channel();
        let (release, on_release) = std::sync::mpsc::channel();
        let held = Arc::new(SlowFill {
            armed: std::sync::atomic::AtomicBool::new(false),
            entered: Mutex::new(entered),
            release: Mutex::new(on_release),
        });
        app.metadata.register("held", held.clone());
        let scheduler = crate::scheduler::Scheduler::new(4);
        let open = opened_on(&app, scheduler.clone(), &dir, Needs::Nothing);
        let mut listing = crate::Listing::default();
        let name = "it's coming.part";
        // Applies what comes until `done` holds of the row.
        let until = |listing: &mut crate::Listing, done: &dyn Fn(&Entry) -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while !listing.iter().find(|e| e.name == name).is_some_and(done) {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                listing.apply(open.rx.recv_timeout(left).expect("told"));
            }
        };
        let marked = |e: &Entry| e.metadata.value(keys::WRITING) == Some(&Value::Bool(true));
        let size = |e: &Entry| e.metadata.value(keys::SIZE).cloned();

        for event in events_until(&open.rx, |e| matches!(e, ListingEvent::Complete)) {
            listing.apply(event);
        }
        let mut file = std::fs::File::create(dir.join(name)).unwrap();
        file.write_all(b"the first part").unwrap();
        until(&mut listing, &marked);
        let id = listing.iter().find(|e| e.name == name).unwrap().id;
        // Its row hydrated while it is written: stat-ed, marked, then held
        // in its providers.
        held.armed.store(true, Ordering::SeqCst);
        open.hydrate(&[id], Priority::Visible);
        on_entered
            .recv_timeout(Duration::from_secs(20))
            .expect("the hydration reached its providers");
        // Written on and closed meanwhile: the watcher's stat says so.
        file.write_all(b", and the rest").unwrap();
        drop(file);
        let last = Some(Value::UInt(28));
        until(&mut listing, &|e| !marked(e) && size(e) == last);
        // The hydration let go: told last, from a stat begun before.
        release.send(()).unwrap();
        assert!(scheduler.wait_idle(Duration::from_secs(20)));
        for event in open.rx.try_iter() {
            listing.apply(event);
        }
        let row = listing.iter().find(|e| e.name == name).unwrap();
        assert!(!marked(row), "marked no more");
        assert_eq!(size(row), last, "at its last size");
        open.close();
    }

    /// A refresh read while a name moves away and back (F5 during `mv b
    /// b2; mv b2 b`, told by the watcher meanwhile) shows what the watcher
    /// found since: the name it read, gone since, is not shown, and the row
    /// it did not see, back since, stays.
    #[test]
    fn a_refresh_keeps_what_was_found_of_a_name_since_it_began() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("l'été, moving");
        std::fs::create_dir(&dir).unwrap();
        let (a, b, c, moved) = ("it's a", "it's b, été", "it's c", "it's b, moved");
        for name in [a, b, c] {
            std::fs::write(dir.join(name), name).unwrap();
        }
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let scheduler = crate::scheduler::Scheduler::new(4);
        let open = opened_on(&app, scheduler.clone(), &dir, Needs::Nothing);
        let mut listing = crate::Listing::default();
        for event in events_until(&open.rx, |e| matches!(e, ListingEvent::Complete)) {
            listing.apply(event);
        }
        let shown = |listing: &crate::Listing| -> Vec<String> {
            let mut names: Vec<String> = listing
                .iter()
                .map(|e| e.name.to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        // Applies what comes until the names shown are `want`.
        let until = |listing: &mut crate::Listing, want: &[&str]| {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while shown(listing) != want {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                listing.apply(open.rx.recv_timeout(left).expect("told"));
            }
        };
        until(&mut listing, &[a, b, c]);
        // The refresh held where it reads, the watcher applying meanwhile.
        let (at, on_at) = std::sync::mpsc::channel::<Option<OsString>>();
        let (go, on_go) = std::sync::mpsc::channel::<()>();
        let shared = open.shared.lock().clone().expect("listed");
        *shared.listing_at.lock() = Some(Box::new(move |name| {
            at.send(name.map(OsStr::to_os_string)).unwrap();
            on_go.recv_timeout(Duration::from_secs(20)).expect("let go");
        }));
        open.refresh();
        // About to read the folder: the name moves away, told.
        assert_eq!(on_at.recv_timeout(Duration::from_secs(20)).unwrap(), None);
        std::fs::rename(dir.join(b), dir.join(moved)).unwrap();
        until(&mut listing, &[a, moved, c]);
        go.send(()).unwrap();
        // Read as it was then (the name moved), then moved back, told.
        assert!(
            on_at
                .recv_timeout(Duration::from_secs(20))
                .unwrap()
                .is_some()
        );
        std::fs::rename(dir.join(moved), dir.join(b)).unwrap();
        until(&mut listing, &[a, b, c]);
        go.send(()).unwrap();
        for event in events_until(&open.rx, |e| matches!(e, ListingEvent::Complete)) {
            listing.apply(event);
        }
        assert!(scheduler.wait_idle(Duration::from_secs(20)));
        for event in open.rx.try_iter() {
            listing.apply(event);
        }
        assert_eq!(shown(&listing), [a, b, c], "as the watcher found them");
        open.close();
    }

    /// A folder opened for an order by a stat (a date) is complete once
    /// every entry is stat-ed, none filled by the metadata providers, which
    /// are not asked: an order that reads no more waits for no more
    /// (ADR-121, amended 2026-11-24). For an order by what they say (a
    /// rating), every entry is filled before it completes; asked for a row,
    /// an entry is filled whatever the order.
    #[test]
    fn a_folder_is_read_as_far_as_its_order_reads_before_it_completes() {
        use std::sync::atomic::AtomicUsize;
        /// Counts the entries it is asked to fill.
        struct Count(Arc<AtomicUsize>);
        impl crate::metadata::MetadataProvider for Count {
            fn id(&self) -> &'static str {
                "count"
            }
            fn fill(&self, _: &Path, entries: &mut [Entry]) {
                self.0.fetch_add(entries.len(), Ordering::SeqCst);
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("many, l'été");
        std::fs::create_dir(&dir).unwrap();
        for i in 0..150 {
            std::fs::write(dir.join(format!("it's {i}.txt")), "x").unwrap();
        }
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let asked = Arc::new(AtomicUsize::new(0));
        app.metadata
            .register("count", Arc::new(Count(Arc::clone(&asked))));
        let read = |told: &[ListingEvent]| -> (usize, usize) {
            let updated = told.iter().filter_map(|e| match e {
                ListingEvent::Updated(entries) => Some(entries),
                _ => None,
            });
            let all: Vec<&Entry> = updated.flatten().collect();
            assert!(all.iter().all(|e| e.metadata.is_hydrated()), "stat-ed");
            let filled = all.iter().filter(|e| e.metadata.is_filled()).count();
            (all.len(), filled)
        };

        let open = opened_here(&app, &dir, Needs::Stat);
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        assert_eq!(read(&told), (150, 0), "stat-ed, none filled");
        assert_eq!(asked.load(Ordering::SeqCst), 0, "no provider asked");
        // A row drawn asks for the rest.
        let ids: Vec<EntryId> = told
            .iter()
            .filter_map(|e| match e {
                ListingEvent::Updated(entries) => Some(entries.iter().map(|e| e.id)),
                _ => None,
            })
            .flatten()
            .take(2)
            .collect();
        open.hydrate(&ids, Priority::Visible);
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Updated(_)));
        assert_eq!(read(&told), (2, 2), "filled once asked for");
        // A refresh reads again each entry as far as it was read: the two
        // filled by the providers again, the rest stat-ed alone.
        assert!(app.scheduler.wait_idle(Duration::from_secs(20)));
        let before = asked.load(Ordering::SeqCst);
        open.refresh();
        let mut told = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while read(&told).0 < 150 {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            told.push(open.rx.recv_timeout(left).expect("read again in time"));
        }
        assert!(app.scheduler.wait_idle(Duration::from_secs(20)));
        told.extend(open.rx.try_iter());
        assert_eq!(read(&told), (150, 2), "as far as each was read");
        assert_eq!(asked.load(Ordering::SeqCst) - before, 2, "two filled again");
        open.close();

        let open = opened_here(&app, &dir, Needs::Providers);
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        assert_eq!(read(&told), (150, 150), "every entry filled first");
        open.close();
    }

    /// A folder read by its path (a remote's, as pretended) is opened once
    /// for a read: the listing's own stats of every entry, made before it
    /// completes for a pane whose order reads them, read in the folder the
    /// listing opened, never once more for each batch (ADR-149).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_folder_read_by_its_path_is_opened_once_a_read() {
        let tmp = tempfile::tempdir().unwrap();
        let volume = tmp.path().join("a server");
        let shown = volume.join("many, été");
        std::fs::create_dir_all(&shown).unwrap();
        for i in 0..300 {
            std::fs::write(shown.join(format!("it's {i}.txt")), "x").unwrap();
        }
        crate::fs::reach::pretend_remote(&volume);
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let open = opened_here(&app, &shown, Needs::Providers);
        let told = events_until(&open.rx, |e| matches!(e, ListingEvent::Complete));
        let updated: usize = told
            .iter()
            .filter_map(|e| match e {
                ListingEvent::Updated(entries) => Some(entries.len()),
                _ => None,
            })
            .sum();
        assert_eq!(updated, 300, "every entry read before it completes");
        let shared = open.shared.lock().clone().expect("listed");
        assert!(matches!(*shared.folder.read(), Folder::ByPath));
        assert_eq!(shared.opens.load(Ordering::Relaxed), 1, "opened once");
        open.close();
    }

    /// A folder on a volume the user may let go of (here a remote, as
    /// pretended) is read by its path, held between reads by nothing
    /// that would refuse its unmount (ADR-149, amended); renamed within
    /// its folder it is found there by its identity and followed, what is
    /// made in it after told all the same; moved elsewhere, it is gone.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_folder_the_user_may_let_go_of_is_held_by_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let volume = tmp.path().join("a server");
        let shown = volume.join("it's shown, été");
        std::fs::create_dir_all(&shown).unwrap();
        std::fs::write(shown.join("a.txt"), "a").unwrap();
        crate::fs::reach::pretend_remote(&volume);
        let app = crate::App::new(crate::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let open = app.open_location(&Location::from_path(&shown)).unwrap();
        until(&open, |e| matches!(e, ListingEvent::Complete));
        assert!(
            wait_let_go(&volume, Duration::from_secs(20)),
            "nothing of it held between reads"
        );
        // Renamed within its folder: followed.
        let renamed = volume.join("renamed, l'été");
        std::fs::rename(&shown, &renamed).unwrap();
        let moved = until(&open, |e| matches!(e, ListingEvent::Moved(_)));
        assert!(
            matches!(moved.last(), Some(ListingEvent::Moved(to)) if *to == renamed),
            "{moved:?}"
        );
        std::fs::write(renamed.join("b.txt"), "b").unwrap();
        until(
            &open,
            |e| matches!(e, ListingEvent::Added(added) if added.iter().any(|e| e.name == "b.txt")),
        );
        // Moved elsewhere: not followed, gone.
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::rename(&renamed, elsewhere.join("it")).unwrap();
        let gone = until(&open, |e| matches!(e, ListingEvent::Error(_)));
        assert!(
            matches!(gone.last(), Some(ListingEvent::Error(e)) if e.kind == crate::location::Failure::Missing),
            "{gone:?}"
        );
        open.close();
    }

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

    /// What may take the square of a folder's size takes far longer than
    /// this; what takes its size, a fraction of it, unoptimized.
    const LINEAR_WITHIN: Duration = Duration::from_secs(5);

    /// A folder its FUSE program numbered anew: every name's id replaced,
    /// and every id noted before gone, each once.
    #[test]
    fn a_folder_numbered_anew_goes_in_one_pass() {
        let folder = FileId::new(1, 2);
        let n = 100_000;
        let id = |ino| EntryId::in_folder(FileId::new(1, ino), folder, OsStr::new("a"));
        let before: Vec<EntryId> = (0..n).map(id).collect();
        let now: Vec<EntryId> = (n..2 * n).map(id).collect();
        let started = std::time::Instant::now();
        let (kept, gone) = gone_after(Some(before.clone()), &now, before.clone());
        assert!(started.elapsed() < LINEAR_WITHIN, "{:?}", started.elapsed());
        assert!(kept.is_empty());
        assert_eq!(gone, before, "each once, in order");
    }

    fn named(ino: u64, name: &str) -> Entry {
        let id = EntryId::in_folder(FileId::new(1, ino), FileId::new(1, 2), OsStr::new(name));
        Entry::new(id, name, EntryKind::File)
    }

    /// A bulk rename and as many editors saving, in one batch: each paired
    /// by a map, nothing left over.
    #[test]
    fn a_batch_of_renames_pairs_in_one_pass() {
        let n = 50_000;
        let (mut removed, mut added, mut replaced) = (Vec::new(), Vec::new(), Vec::new());
        let mut expected = std::collections::HashMap::new();
        for i in 0..n {
            // Renamed: one name went, one came.
            let (old, new) = (named(i, &format!("old {i}")), named(i, &format!("new {i}")));
            expected.insert(old.id, new.id);
            removed.push(old.id);
            added.push(new);
            // Saved over: the name held one file, now another.
            let (stale, saved) = (
                named(n + i, &format!("saved {i}")),
                named(2 * n + i, &format!("saved {i}")),
            );
            expected.insert(stale.id, saved.id);
            removed.push(stale.id);
            replaced.push((stale.id, saved.id));
            added.push(saved);
        }
        let started = std::time::Instant::now();
        let (removed, renamed, added) = pair_changes(removed, added, replaced);
        assert!(started.elapsed() < LINEAR_WITHIN, "{:?}", started.elapsed());
        assert!(removed.is_empty() && added.is_empty());
        assert_eq!(renamed.len(), expected.len());
        for (old, new) in &renamed {
            assert_eq!(expected.get(old), Some(&new.id));
        }
    }

    /// Each file takes the name of the next: each row follows its file,
    /// and the last name's row follows the file that took it.
    #[test]
    fn a_name_taken_by_its_neighbour_follows_both() {
        let (n0, n1, n2) = (named(0, "n0"), named(1, "n1"), named(2, "n2"));
        let (to_n1, to_n2) = (named(0, "n1"), named(1, "n2"));
        let (removed, renamed, added) = pair_changes(
            vec![n0.id, n1.id, n2.id],
            vec![to_n1.clone(), to_n2.clone()],
            vec![(n1.id, to_n1.id), (n2.id, to_n2.id)],
        );
        assert!(removed.is_empty() && added.is_empty());
        let pairs: Vec<(EntryId, EntryId)> = renamed.iter().map(|(old, e)| (*old, e.id)).collect();
        assert_eq!(
            pairs,
            [(n0.id, to_n1.id), (n1.id, to_n2.id), (n2.id, to_n2.id)]
        );
    }

    /// A folder whose stats never agree with its listing: every entry
    /// re-keyed, each alias found by its target, followed when re-keyed
    /// again, gone once the stat agrees.
    #[test]
    fn every_entry_re_keyed_finds_its_alias_by_its_target() {
        let folder = FileId::new(1, 2);
        let n = 50_000;
        let id =
            |ino: u64, name: &str| EntryId::in_folder(FileId::new(1, ino), folder, name.as_ref());
        let mut names = Names::default();
        let listed: Vec<(EntryId, String)> = (0..n)
            .map(|i| {
                let name = i.to_string();
                (id(i, &name), name)
            })
            .collect();
        for (provisional, name) in &listed {
            names.insert(*provisional, name.into());
        }
        let started = std::time::Instant::now();
        for (i, (provisional, name)) in listed.iter().enumerate() {
            names.rekey(*provisional, id(n + i as u64, name));
        }
        for (i, (_, name)) in listed.iter().enumerate() {
            names.rekey(id(n + i as u64, name), id(2 * n + i as u64, name));
        }
        assert!(started.elapsed() < LINEAR_WITHIN, "{:?}", started.elapsed());
        for (i, (provisional, name)) in listed.iter().enumerate() {
            assert_eq!(names.listing_id(*provisional), id(2 * n + i as u64, name));
        }
        // The stat agrees with the listing again: no alias left for it.
        let (first, name) = &listed[0];
        names.rekey(id(2 * n, name), *first);
        assert_eq!(names.listing_id(*first), *first);
        assert_eq!(names.aliases.len(), listed.len() - 1);
        assert_eq!(names.aliased.len(), listed.len() - 1);
        assert_eq!(names.by_name.get(OsStr::new(name.as_str())), Some(first));
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
