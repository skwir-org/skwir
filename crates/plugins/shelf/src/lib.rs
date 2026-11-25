//! skwir plugin: shelf. Collect entries from any location into a tray, then
//! move, copy, or trash them all at once into wherever you are. The shelf is
//! also a virtual location (`shelf:`) so any view can show it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use crossbeam_channel::Receiver;
use parking_lot::{Mutex, MutexGuard};
use skwir_core::CancelToken;
use skwir_core::app::{App, PaneArg};
use skwir_core::command::{Invocation, NoArgs, Outcome, Topic};
use skwir_core::entry::{Entry, EntryId, EntryKind};
use skwir_core::fs::stat::stat_path;
use skwir_core::fs::{Newest, Read, Reading, Tickets, read_paths_newest};
use skwir_core::hashing;
use skwir_core::location::{
    EmptyState, EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
    Tool, ToolPane, Verb, Verbs,
};
use skwir_core::metadata::MetadataProviders;
use skwir_core::op::{Change, Copy, EntryRef, Move, Operation, Plan, Trash};
use skwir_core::plugin::Plugin;
use skwir_core::registry::{KeyBinding, MenuItem, Place};
use skwir_core::scheduler::Priority;
use skwir_core::words;
use skwir_core::words::{Folder, Words};
use skwir_core::workspace::PaneId;

const ID: &str = "shelf";
/// The location scheme of the virtual shelf listing.
pub const SCHEME: &str = "shelf";
/// What the shelf is called, in the sidebar and wherever it is shown.
fn shelf_title() -> Words {
    words!(
        shelf_title = "Shelf",
        noun,
        note = "The shelf as a place: in the sidebar, its tab, its count in the footer."
    )
}
/// The count the shelf publishes in `App::tallies`.
pub const TALLY: &str = "shelf.items";

/// One shelved entry.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShelfItem {
    pub id: EntryId,
    #[serde(with = "skwir_core::os_path")]
    pub path: PathBuf,
    #[serde(with = "skwir_core::os_path")]
    pub name: OsString,
    /// As the listing it came from knew it; a stat refines it.
    #[serde(skip)]
    pub kind: EntryKind,
}

impl ShelfItem {
    fn entry_ref(&self) -> EntryRef {
        EntryRef::new(self.id.file, self.path.clone())
    }

    fn entry(&self) -> Entry {
        Entry::new(self.id, self.name.clone(), self.kind)
    }
}

/// What a hydration's answers are told at least this many at a time, as
/// its folders are read: the window applies them in slices.
const SENT_EVERY: usize = 256;

/// The shelf: its items, and the tickets of the reads of them.
#[derive(Default)]
struct Shelf {
    state: Mutex<State>,
    /// Taken by each read of an item just before it reads (a hydration's
    /// stat, a follow's), and by each change trusted as it is applied (a
    /// plan's, an item shelved): of two, the one begun later stands
    /// (`State::newest`).
    tickets: Tickets,
}

impl Shelf {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock()
    }

    /// Ask whether each item `moved` kept its identity where a plan put
    /// it (a move to another device gives it a new inode), and read it
    /// there as a hydration does, filled by the providers, since a row
    /// the plan renamed is told bare. Worker-only: this is I/O, in the
    /// lane of the paths.
    fn restat(&self, moved: Vec<(EntryId, PathBuf)>, metadata: &MetadataProviders) {
        // Dropped last, after what it found is told.
        let _reading = Reading::begin(&self.state, |s| &mut s.newest, &self.tickets);
        let mut found: Vec<(EntryId, PathBuf, Option<Entry>, u64)> = moved
            .into_iter()
            .map(|(id, path)| {
                let ticket = self.tickets.take();
                let entry = stat_path(&path);
                (id, path, entry, ticket)
            })
            .collect();
        // Each folder's filled at once, as a hydration's are.
        let mut by_dir: HashMap<PathBuf, Vec<usize>> = HashMap::new();
        for (n, (_, path, entry, _)) in found.iter().enumerate() {
            if let (Some(_), Some(dir)) = (entry, path.parent()) {
                by_dir.entry(dir.to_path_buf()).or_default().push(n);
            }
        }
        for (dir, at) in by_dir {
            let mut entries: Vec<Entry> = at.iter().filter_map(|&n| found[n].2.take()).collect();
            metadata.fill(&dir, &mut entries);
            for (n, entry) in at.into_iter().zip(entries) {
                found[n].2 = Some(entry);
            }
        }
        let mut s = self.lock();
        let s = &mut *s;
        let (mut renamed, mut updated, mut lost) = (Vec::new(), Vec::new(), Vec::new());
        for (old, path, entry, ticket) in found {
            // Only items still on the shelf, still at that path; what
            // could not be read there says nothing.
            let Some(entry) = entry.filter(|_| s.items.at(old, &path).is_some()) else {
                continue;
            };
            if entry.id == old {
                // Its file under its name still (a rename undone before
                // this was asked): told, unless a read begun later was.
                if s.newest.applies(&old, ticket) {
                    if let Some(slot) = s.items.slot_mut(old) {
                        slot.filled |= entry.metadata.is_filled();
                    }
                    updated.push(entry);
                }
                continue;
            }
            if s.items.contains(entry.id) {
                // Now the same file as another item: keep that one.
                lost.push(old);
                continue;
            }
            // What it is now, whatever was told of what it was: noted
            // under the id it takes, so a read of that begun before (a
            // hydration of the name an undo gives back) is left out.
            s.newest.applies(&entry.id, ticket);
            if let Some(slot) = s.items.rekey(old, entry.id) {
                slot.item.kind = entry.kind;
                slot.filled = entry.metadata.is_filled();
            }
            renamed.push((old, entry));
        }
        // Renamed, so a row keeps its place and its selection.
        if !renamed.is_empty() {
            s.broadcast(&ListingEvent::Renamed(renamed));
        }
        if !updated.is_empty() {
            s.broadcast(&ListingEvent::Updated(updated));
        }
        s.forget(&lost);
    }

    /// Hydrate `items`, asked by one listing of the shelf: stat-ed and
    /// filled one folder at a time, each answer told to every listing
    /// under the shelf's lock, only where the shelf holds it still where
    /// it was looked for and no read of it begun later was told
    /// (`State::newest`). Returns what is to be read again: left out
    /// behind a read that told its rows less (`Slot::filled`).
    /// Worker-only: this is I/O, in the lane of the paths.
    fn hydrate_now(
        &self,
        items: &[(EntryId, PathBuf)],
        metadata: &MetadataProviders,
        cancel: &CancelToken,
    ) -> Vec<(EntryId, PathBuf)> {
        // Dropped last, after its answers are told.
        let _reading = Reading::begin(&self.state, |s| &mut s.newest, &self.tickets);
        let asked: hashing::Map<EntryId, &Path> = items
            .iter()
            .map(|(id, path)| (*id, path.as_path()))
            .collect();
        let (mut reads, mut held, mut behind) = (Vec::new(), 0, Vec::new());
        read_paths_newest(items, metadata, cancel, &self.tickets, |read| {
            held += read.found.len() + read.gone.len() + read.unread.len();
            reads.push(read);
            if held >= SENT_EVERY {
                behind.extend(self.answer(std::mem::take(&mut reads), &asked, cancel));
                held = 0;
            }
        });
        behind.extend(self.answer(reads, &asked, cancel));
        behind
    }

    /// Tell what `reads` found, `asked` where each was looked for, to
    /// every listing of the shelf, not the asker's alone: what is decided
    /// here (`State::newest`, `Slot::filled`) is decided for all of them,
    /// and a listing never told what stood would wait for good on its own
    /// read, left out behind it. Moved or gone is decided here, under the
    /// lock, before anything is told: gone from where it was looked for,
    /// it leaves the shelf; moved meanwhile, or moved and moved back (a
    /// rename undone), the follow's read of it stands. Returns what is to
    /// be read again, as asked: left out behind a read that told its rows
    /// less than this one would (an item shelved again bare, a plan's
    /// rename), or they stay unfilled for good, their askers waiting on
    /// this answer. Each read again takes later tickets than the one it
    /// was left out behind.
    fn answer(
        &self,
        reads: Vec<Read>,
        asked: &hashing::Map<EntryId, &Path>,
        cancel: &CancelToken,
    ) -> Vec<(EntryId, PathBuf)> {
        let mut s = self.lock();
        if cancel.is_cancelled() {
            return Vec::new();
        }
        let s = &mut *s;
        let mut behind = Vec::new();
        let mut stands = |s: &mut State, id: EntryId, ticket: u64| {
            let Some(&path) = asked.get(&id) else {
                return false;
            };
            let Some(filled) = s.items.at(id, path).map(|slot| slot.filled) else {
                return false;
            };
            if s.newest.applies(&id, ticket) {
                return true;
            }
            if !filled {
                behind.push((id, path.to_path_buf()));
            }
            false
        };
        let (mut updated, mut gone, mut unread) = (Vec::new(), Vec::new(), Vec::new());
        for read in reads {
            for (e, ticket) in read.found {
                if stands(s, e.id, ticket) {
                    if let Some(slot) = s.items.slot_mut(e.id) {
                        slot.filled |= e.metadata.is_filled();
                    }
                    updated.push(e);
                }
            }
            for (id, ticket) in read.gone {
                if stands(s, id, ticket) {
                    gone.push(id);
                }
            }
            for (id, ticket) in read.unread {
                if stands(s, id, ticket) {
                    unread.push(id);
                }
            }
        }
        if !updated.is_empty() {
            s.broadcast(&ListingEvent::Updated(updated));
        }
        // Off the shelf, and out of every listing of it.
        s.forget(&gone);
        if !unread.is_empty() {
            s.broadcast(&ListingEvent::Unreadable(unread));
        }
        behind
    }
}

/// A shelved item, with what its rows were last told: its rows in every
/// listing of the shelf, each told alike (`State::broadcast`).
struct Slot {
    item: ShelfItem,
    /// Whether by a read the metadata providers filled (a hydration's, a
    /// follow's), not less (shelved bare, renamed by a plan): a hydration
    /// left out behind one that told less is read again (`Shelf::answer`).
    filled: bool,
}

/// The items shelved, in the order they were shelved, each found by its id
/// at once: taking some off costs what is taken, never the whole shelf,
/// which `path_of` waits for from the window's thread.
#[derive(Default)]
struct Items {
    /// By the order they were shelved in.
    by_order: BTreeMap<u64, Slot>,
    /// Each item's place in `by_order`, by its id: one item an id.
    by_id: hashing::Map<EntryId, u64>,
    /// The place of the next item shelved.
    next: u64,
}

impl Items {
    fn iter(&self) -> impl Iterator<Item = &ShelfItem> {
        self.by_order.values().map(|slot| &slot.item)
    }

    fn slots_mut(&mut self) -> impl Iterator<Item = &mut Slot> {
        self.by_order.values_mut()
    }

    fn len(&self) -> usize {
        self.by_order.len()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.by_order.is_empty()
    }

    fn to_vec(&self) -> Vec<ShelfItem> {
        self.iter().cloned().collect()
    }

    fn contains(&self, id: EntryId) -> bool {
        self.by_id.contains_key(&id)
    }

    fn get(&self, id: EntryId) -> Option<&ShelfItem> {
        Some(&self.by_order.get(self.by_id.get(&id)?)?.item)
    }

    fn slot_mut(&mut self, id: EntryId) -> Option<&mut Slot> {
        self.by_order.get_mut(self.by_id.get(&id)?)
    }

    /// The item `id`, if it is still at `path`.
    fn at(&self, id: EntryId, path: &Path) -> Option<&Slot> {
        self.by_order
            .get(self.by_id.get(&id)?)
            .filter(|slot| slot.item.path == path)
    }

    /// Shelve `item` last, its rows told `filled` or not; false where its
    /// id is shelved already.
    fn push(&mut self, item: ShelfItem, filled: bool) -> bool {
        if self.by_id.contains_key(&item.id) {
            return false;
        }
        let place = self.next;
        self.next += 1;
        self.by_id.insert(item.id, place);
        self.by_order.insert(place, Slot { item, filled });
        true
    }

    fn remove(&mut self, id: EntryId) -> Option<ShelfItem> {
        let place = self.by_id.remove(&id)?;
        self.by_order.remove(&place).map(|slot| slot.item)
    }

    /// The item `old` is the entry `id` now, in its place (`id` not
    /// shelved).
    fn rekey(&mut self, old: EntryId, id: EntryId) -> Option<&mut Slot> {
        let place = self.by_id.remove(&old)?;
        self.by_id.insert(id, place);
        let slot = self.by_order.get_mut(&place)?;
        slot.item.id = id;
        Some(slot)
    }

    fn clear(&mut self) {
        self.by_order.clear();
        self.by_id.clear();
    }
}

/// The shelf's one state, for every listing of it: each shows every item,
/// and what changes or reads an item is told to all of them
/// (`State::broadcast`), whichever listing asked, since what is decided
/// here (`newest`, `Slot::filled`) is decided for all of them: a read left
/// out for one listing behind a read told to another alone would leave
/// its row unread for good, the view asking once.
#[derive(Default)]
struct State {
    items: Items,
    /// Open `shelf:` listings to keep live.
    sinks: Vec<EventSink>,
    /// Where the item count is published; empty while disabled.
    app: Weak<App>,
    /// Which read of each item was told last, while another read that
    /// may hold an older one is under way (`fs::Newest`): what a read
    /// begun before it found is left out, whatever each found, gone
    /// included.
    newest: Newest<EntryId>,
}

impl State {
    /// `ids` are as a change trusted without a stat says (a plan's, an
    /// item shelved as its listing knew it), as of now: what a read of one
    /// begun before found, told after, is left out.
    fn changed(&mut self, ids: impl IntoIterator<Item = EntryId>, tickets: &Tickets) {
        // No read under way: none begun before can be told after.
        if self.newest.is_idle() {
            return;
        }
        let floor = self.newest.begin(tickets);
        let ticket = tickets.take();
        for id in ids {
            self.newest.applies(&id, ticket);
        }
        self.newest.read_ended(floor);
    }

    fn broadcast(&mut self, event: &ListingEvent) {
        self.sinks.retain(|s| s.send(event.clone()));
    }

    fn publish_count(&self) {
        if let Some(app) = self.app.upgrade() {
            app.tallies.set(
                ID,
                TALLY,
                skwir_core::tally::Tally {
                    label: shelf_title(),
                    count: self.items.len() as u64,
                    open: Some(Invocation::new("shelf.open")),
                },
            );
            app.wake();
        }
    }

    /// Drop `ids` from the shelf and from every open listing of it, each
    /// by its id: what is taken off costs, never the whole shelf.
    fn forget(&mut self, ids: &[EntryId]) -> usize {
        let removed: Vec<EntryId> = ids
            .iter()
            .copied()
            .filter(|id| self.items.remove(*id).is_some())
            .collect();
        if !removed.is_empty() {
            self.broadcast(&ListingEvent::Removed(removed.clone()));
            self.publish_count();
        }
        removed.len()
    }
}

/// The shelf plugin.
#[derive(Default)]
pub struct ShelfPlugin {
    state: Arc<Shelf>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct AddArgs {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Entries to add; the pane's selection when omitted.
    #[serde(default)]
    ids: Vec<EntryId>,
    /// Files to add by where they are (another application's, dropped,
    /// or the clipboard's, pasted; a script's or an agent's, by absolute
    /// path), instead of a pane's.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RemoveArgs {
    /// Entries to take off; the pane's selection when omitted.
    #[serde(default)]
    ids: Vec<EntryId>,
    #[serde(default)]
    pane: Option<PaneId>,
}

impl ShelfPlugin {
    /// Current items, in insertion order.
    pub fn items(&self) -> Vec<ShelfItem> {
        self.state.lock().items.to_vec()
    }

    /// Shelve `new` with the entries their listing already holds, so
    /// nothing is stat-ed here: this runs on the dispatching thread.
    fn add(state: &Shelf, new: Vec<(ShelfItem, Option<Entry>)>) -> usize {
        let mut s = state.lock();
        // A name shelved once, however it came: two names of one file are
        // two items (ADR-049). Asked of a set of the paths shelved: a
        // folder's selection shelved compared each with every item.
        let fresh: Vec<bool> = {
            let mut shelved: HashSet<&Path> = s.items.iter().map(|i| i.path.as_path()).collect();
            new.iter()
                .map(|(item, _)| shelved.insert(item.path.as_path()))
                .collect()
        };
        let mut added = Vec::new();
        for ((item, entry), fresh) in new.into_iter().zip(fresh) {
            if !fresh {
                continue;
            }
            let entry = entry.unwrap_or_else(|| item.entry());
            if s.items.push(item, entry.metadata.is_filled()) {
                added.push(entry);
            }
        }
        let n = added.len();
        if n > 0 {
            // As its listing knew it: what a read begun before found (a
            // hydration of one shelved again) is left out.
            s.changed(added.iter().map(|e| e.id), &state.tickets);
            s.broadcast(&ListingEvent::Added(added));
            s.publish_count();
        }
        n
    }

    fn remove(state: &Shelf, ids: &[EntryId]) -> usize {
        state.lock().forget(ids)
    }

    fn clear(state: &Shelf) {
        let mut s = state.lock();
        let ids: Vec<EntryId> = s.items.iter().map(|i| i.id).collect();
        s.forget(&ids);
    }
}

/// Follow shelved paths through applied changes: a trashed or deleted item
/// leaves the shelf, a moved or renamed one follows at once (no I/O), and
/// a worker then asks whether it kept its identity: a move to another
/// device gives it a new inode.
fn follow(state: &Arc<Shelf>, changes: &[Change], app: &App) {
    let mut s = state.lock();
    let mut gone = Vec::new();
    let mut renamed = Vec::new();
    let mut moved = Vec::new();
    let follow = skwir_core::op::Follow::new(changes);
    for slot in s.items.slots_mut() {
        let item = &mut slot.item;
        let Some(path) = follow.through(&item.path).at else {
            gone.push(item.id);
            continue;
        };
        if path != item.path {
            if let Some(name) = path.file_name().filter(|n| *n != item.name) {
                item.name = name.to_owned();
                renamed.push(item.entry());
                // Told bare, under its new name.
                slot.filled = false;
            }
            slot.item.path.clone_from(&path);
            moved.push((slot.item.id, path));
        }
    }
    // Where the plan put them, as of now: what a hydration read where they
    // were, told after (once a rename is undone, where they are again), is
    // left out.
    s.changed(moved.iter().map(|(id, _)| *id), &state.tickets);
    s.forget(&gone);
    if !renamed.is_empty() {
        s.broadcast(&ListingEvent::Added(renamed));
    }
    drop(s);
    if moved.is_empty() {
        return;
    }
    // Each remote's in its lane (ADR-077): a server that stops answering
    // holds its own items' job, never the others'.
    for (lane, moved) in skwir_core::fs::reach::by_lane(moved, |(_, path)| path) {
        let state = Arc::clone(state);
        let metadata = Arc::clone(&app.metadata);
        app.scheduler.spawn_in(
            lane,
            Priority::Normal,
            "shelf.follow",
            app.lifetime().child(),
            move || state.restat(moved, &metadata),
        );
    }
}

fn selection_items(
    app: &App,
    pane: PaneId,
    ids: &[EntryId],
) -> anyhow::Result<Vec<(ShelfItem, Option<Entry>)>> {
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::Error::from(words!(shelf_no_such_pane = "no such pane")))?;
    anyhow::ensure!(
        p.open.is_some(),
        words!(shelf_pane_not_open = "pane has no open location")
    );
    let chosen: Vec<EntryId> = if ids.is_empty() {
        skwir_core::app::chosen(p).iter().collect()
    } else {
        ids.to_vec()
    };
    if chosen.is_empty() {
        return Err(skwir_core::command::NothingSelected.into());
    }
    Ok(chosen
        .into_iter()
        .filter_map(|id| {
            let path = p.path_of(id)?;
            let entry = p.entry(id).cloned();
            let name = entry
                .as_ref()
                .map(|e| e.name.as_os_str().to_owned())
                .or_else(|| path.file_name().map(ToOwned::to_owned))?;
            let kind = entry.as_ref().map_or(EntryKind::Unknown, |e| e.kind);
            Some((
                ShelfItem {
                    id,
                    path,
                    name,
                    kind,
                },
                entry,
            ))
        })
        .collect())
}

/// A file dropped or pasted on the shelf, by where it is: no listing
/// knows it, so a stat says what it is when it is hydrated.
fn dropped_item(entry: &EntryRef) -> Option<(ShelfItem, Option<Entry>)> {
    let name = entry.path.file_name()?.to_owned();
    Some((
        ShelfItem {
            id: EntryId::at_path(entry.id, &entry.path),
            path: entry.path.clone(),
            name,
            kind: EntryKind::Unknown,
        },
        None,
    ))
}

fn plan_into(items: &Items, dest: &Path, copy: bool, app: &App) -> anyhow::Result<Outcome> {
    let steps: Vec<Box<dyn Operation>> = items
        .iter()
        .filter(|i| copy || i.path.parent() != Some(dest))
        .map(|i| -> Box<dyn Operation> {
            if copy {
                Box::new(Copy::from_settings(
                    i.entry_ref(),
                    dest.to_path_buf(),
                    &app.settings,
                ))
            } else {
                Box::new(Move {
                    entry: i.entry_ref(),
                    dest_dir: dest.to_path_buf(),
                    new_name: None,
                })
            }
        })
        .collect();
    if steps.is_empty() {
        anyhow::bail!(words!(
            shelf_nothing_to_bring = "nothing on the shelf to bring here"
        ));
    }
    let (count, folder) = (steps.len(), Folder(skwir_core::op::folder_said(dest)));
    let title = if copy {
        words!(
            shelf_copy_here_plan = "copy { $count ->
                [one] { $count } shelved item
               *[other] { $count } shelved items
            } to { $folder }",
            count = count,
            folder = folder,
            note = "A plan's title: the folder they are copied into, by its name."
        )
    } else {
        words!(
            shelf_move_here_plan = "move { $count ->
                [one] { $count } shelved item
               *[other] { $count } shelved items
            } to { $folder }",
            count = count,
            folder = folder,
            note = "A plan's title: the folder they are moved into, by its name."
        )
    };
    Ok(Outcome::Plan(Plan::with_steps(title, steps)))
}

// ------------------------------------------------------------------ location

struct ShelfLocation {
    location: Location,
    rx: Receiver<ListingEvent>,
    ctx: OpenContext,
    state: Arc<Shelf>,
    sink: EventSink,
}

impl ShelfLocation {
    /// What the shelf holds, as its listings last knew it; hydration stats
    /// them on a worker. Told under the shelf's lock, as what changes them
    /// is: never after a rename it did not see.
    fn list(&self) {
        let s = self.state.lock();
        let entries: Vec<Entry> = s.items.iter().map(ShelfItem::entry).collect();
        if !entries.is_empty() {
            self.sink.send(ListingEvent::Added(entries));
        }
        self.sink.send(ListingEvent::Complete);
    }
}

impl OpenLocation for ShelfLocation {
    fn location(&self) -> &Location {
        &self.location
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }

    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        let items: Vec<(EntryId, PathBuf)> = {
            let s = self.state.lock();
            ids.iter()
                .filter_map(|id| Some((*id, s.items.get(*id)?.path.clone())))
                .collect()
        };
        if items.is_empty() {
            return;
        }
        // A job for each slice, side by side on the workers, each remote's
        // in its lane.
        for (lane, items) in skwir_core::fs::reach::by_lane(items, |(_, path)| path) {
            for items in items.chunks(skwir_core::fs::HYDRATE_JOB) {
                let items = items.to_vec();
                let metadata = Arc::clone(&self.ctx.metadata);
                let state = Arc::clone(&self.state);
                let cancel = self.ctx.cancel.child();
                self.ctx.scheduler.spawn_in(
                    lane,
                    priority,
                    "shelf.stat",
                    cancel.clone(),
                    move || {
                        let mut items = items;
                        // What was left out behind a read that told its
                        // row less, read again: each time with tickets
                        // later than the read it was left out behind.
                        while !items.is_empty() && !cancel.is_cancelled() {
                            items = state.hydrate_now(&items, &metadata, &cancel);
                        }
                    },
                );
            }
        }
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        let s = self.state.lock();
        s.items.get(id).map(|i| i.path.clone())
    }

    fn refresh(&self) {
        self.list();
    }

    fn close(&self) {
        self.ctx.cancel.cancel();
    }
}

struct ShelfProvider {
    state: Arc<Shelf>,
}

impl LocationProvider for ShelfProvider {
    fn scheme(&self) -> &str {
        SCHEME
    }

    fn title(&self, _location: &Location) -> Option<skwir_core::words::Words> {
        Some(shelf_title())
    }

    fn icon(&self, _location: &Location) -> Option<&'static str> {
        Some("shelf")
    }

    fn empty_state(&self, _location: &Location) -> Option<EmptyState> {
        Some(EmptyState::new(
            words!(shelf_empty = "The shelf is empty"),
            words!(
                shelf_empty_detail = "Collect files from different folders with Add to Shelf, then move or copy them together.",
                note = "Under \"The shelf is empty\", in the empty shelf. Add to Shelf is a menu item's words."
            ),
        ))
    }

    /// The shelf holds files kept where they are: Delete takes them off
    /// it, and files dropped or pasted on it are shelved (ADR-032). Its
    /// buttons move or copy them all into the folder of the other pane.
    fn verbs(&self, _location: &Location) -> Verbs {
        Verbs {
            drop: Some(Invocation::new("shelf.add")),
            remove: Some(Verb::new(
                words!(
                    shelf_remove_verb = "Remove from Shelf",
                    note = "What Delete does on the shelf, as its menu item says it."
                ),
                Invocation::new("shelf.remove"),
            )),
            tools: vec![
                Tool::new(
                    "move",
                    words!(shelf_move_tool = "Move shelf files to the other pane"),
                    Invocation::new("shelf.move_here"),
                    ToolPane::OtherFolder,
                ),
                Tool::new(
                    "copy",
                    words!(shelf_copy_tool = "Copy shelf files to the other pane"),
                    Invocation::new("shelf.copy_here"),
                    ToolPane::OtherFolder,
                ),
            ],
            ..Verbs::default()
        }
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let (sink, rx) = ctx.channel();
        self.state.lock().sinks.push(sink.clone());
        let loc = Arc::new(ShelfLocation {
            location: location.clone(),
            rx,
            ctx,
            state: Arc::clone(&self.state),
            sink,
        });
        loc.list();
        Ok(loc)
    }
}

// ------------------------------------------------------------------ plugin

const BINDINGS: &[(&str, &str, Option<&str>)] =
    &[("ctrl-shift-b", "shelf.add", Some("pane selection"))];

impl Plugin for ShelfPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(shelf_plugin_title = "Shelf")
    }

    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            shelf_plugin_description =
                "Collect entries from anywhere and move, copy, or trash them together"
        )
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Write])
    }
    fn register(&self, app: &App) {
        {
            let mut s = self.state.lock();
            s.app = app.weak();
            s.publish_count();
        }
        let st = Arc::clone(&self.state);
        let weak = app.weak();
        app.executor.observe(
            ID,
            Arc::new(move |changes| {
                if let Some(app) = weak.upgrade() {
                    follow(&st, changes, &app);
                }
            }),
        );
        app.register_provider(
            ID,
            Arc::new(ShelfProvider {
                state: Arc::clone(&self.state),
            }),
        );

        let st = Arc::clone(&self.state);
        app.commands
            .register::<AddArgs, _>(
                ID,
                Topic::Files,
                "shelf.add",
                words!(shelf_add = "Add to shelf"),
                words!(
                    shelf_add_description = "Put the selection (or the given entries) on the shelf"
                ),
                move |a, app| {
                    let items = if a.entries.is_empty() {
                        selection_items(app, app.pane_or_active(a.pane), &a.ids)?
                    } else {
                        skwir_core::op::EntryArg::refs(a.entries)?
                            .iter()
                            .filter_map(dropped_item)
                            .collect()
                    };
                    let n = ShelfPlugin::add(&st, items);
                    Ok(Outcome::Value(serde_json::json!({ "added": n })))
                },
            )
            .in_dialogs();
        let st = Arc::clone(&self.state);
        app.commands
            .register::<RemoveArgs, _>(
                ID,
                Topic::Files,
                "shelf.remove",
                words!(shelf_remove = "Remove from shelf"),
                words!(shelf_remove_description = "Take entries off the shelf"),
                move |a, app| {
                    let ids = if a.ids.is_empty() {
                        let ws = app.workspace.read();
                        let pane = ws
                            .pane(
                                a.pane
                                    .unwrap_or_else(|| skwir_core::app::asked_or_active(&ws)),
                            )
                            .ok_or_else(|| {
                                anyhow::Error::from(words!(shelf_no_such_pane = "no such pane"))
                            })?;
                        skwir_core::app::chosen(pane).iter().collect()
                    } else {
                        a.ids
                    };
                    let n = ShelfPlugin::remove(&st, &ids);
                    Ok(Outcome::Value(serde_json::json!({ "removed": n })))
                },
            )
            .in_dialogs();
        let st = Arc::clone(&self.state);
        app.commands
            .register::<NoArgs, _>(
                ID,
                Topic::Files,
                "shelf.clear",
                words!(shelf_clear = "Clear shelf"),
                words!(shelf_clear_description = "Empty the shelf"),
                move |_, _| {
                    ShelfPlugin::clear(&st);
                    Ok(Outcome::Done)
                },
            )
            .in_dialogs();
        app.commands
            .register::<PaneArg, _>(
                ID,
                Topic::Files,
                "shelf.open",
                words!(shelf_open = "Open shelf"),
                words!(shelf_open_description = "Show the shelf in the pane"),
                |a, app| {
                    app.navigate(app.pane_or_active(a.pane), &Location::parse("shelf:"))?;
                    Ok(Outcome::Done)
                },
            )
            .in_dialogs()
            .shows();
        let st = Arc::clone(&self.state);
        app.commands
            .register::<NoArgs, _>(
                ID,
                Topic::Files,
                "shelf.list",
                words!(shelf_list = "List shelf"),
                words!(shelf_list_description = "Return the shelved entries"),
                move |_, _| {
                    let items = st.lock().items.to_vec();
                    Ok(Outcome::Value(serde_json::to_value(items)?))
                },
            )
            .data();
        let st = Arc::clone(&self.state);
        app.commands
            .register::<PaneArg, _>(
                ID,
                Topic::Files,
                "shelf.move_here",
                words!(shelf_move_here = "Move shelf here"),
                words!(
                    shelf_move_here_description =
                        "Move every shelved entry into the current directory (plan)"
                ),
                move |a, app| {
                    let dest = app.pane_dir(app.pane_or_active(a.pane))?;
                    plan_into(&st.lock().items, &dest, false, app)
                },
            )
            .plans();
        let st = Arc::clone(&self.state);
        app.commands
            .register::<PaneArg, _>(
                ID,
                Topic::Files,
                "shelf.copy_here",
                words!(shelf_copy_here = "Copy shelf here"),
                words!(
                    shelf_copy_here_description =
                        "Copy every shelved entry into the current directory (plan)"
                ),
                move |a, app| {
                    let dest = app.pane_dir(app.pane_or_active(a.pane))?;
                    plan_into(&st.lock().items, &dest, true, app)
                },
            )
            .plans();
        let st = Arc::clone(&self.state);
        app.commands
            .register::<NoArgs, _>(
                ID,
                Topic::Files,
                "shelf.trash",
                words!(shelf_trash = "Trash shelf"),
                words!(shelf_trash_description = "Move every shelved entry to the trash (plan)"),
                move |_, _| {
                    let items = st.lock().items.to_vec();
                    if items.is_empty() {
                        anyhow::bail!(words!(
                            shelf_nothing_to_trash = "the shelf is empty",
                            note = "An error: the shelf's files were to be moved to the trash."
                        ));
                    }
                    let steps: Vec<Box<dyn Operation>> = items
                        .iter()
                        .map(|i| {
                            Box::new(Trash {
                                entry: i.entry_ref(),
                            }) as Box<dyn Operation>
                        })
                        .collect();
                    Ok(Outcome::Plan(Plan::with_steps(
                        words!(
                            shelf_trash_plan = "trash { $count ->
                                [one] { $count } shelved item
                               *[other] { $count } shelved items
                            }",
                            count = steps.len(),
                            note = "A plan's title: the shelf's files moved to the trash."
                        ),
                        steps,
                    )))
                },
            )
            .plans();

        app.registry.places.add(
            ID,
            Place::new(
                skwir_core::registry::groups::PLACES.id,
                shelf_title(),
                "folder-drag-accept",
                Location::parse("shelf:"),
            ),
        );
        for (keys, command, when) in BINDINGS {
            app.registry.keymap.add(
                ID,
                KeyBinding {
                    keys: (*keys).into(),
                    command: Invocation::new(*command),
                    when: when.map(str::to_owned),
                },
            );
        }
        app.registry.menus.add(
            ID,
            MenuItem::new(
                "context",
                words!(shelf_add_menu = "Add to Shelf"),
                "shelf",
                140,
                Invocation::new("shelf.add"),
            )
            .when("selection !place"),
        );
    }

    fn on_disable(&self, _app: &App) {
        let mut s = self.state.lock();
        s.items.clear();
        s.sinks.clear();
        s.app = Weak::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::app::AppConfig;
    use std::time::Duration;

    fn setup() -> (tempfile::TempDir, Arc<App>, PaneId, PathBuf) {
        // Trashed in a test: on the home filesystem, like the user's files.
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(base).unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), b"a").unwrap();
        std::fs::write(src.join("b.txt"), b"b").unwrap();
        std::fs::create_dir(tmp.path().join("dest")).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(ShelfPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&src)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        (tmp, app, pane, src)
    }

    fn select(app: &App, pane: PaneId, names: &[&str]) {
        let inv = Invocation::with_args("select", &serde_json::json!({ "names": names }));
        app.dispatch(&inv).unwrap();
        let _ = pane;
    }

    fn value(o: Outcome) -> serde_json::Value {
        match o {
            Outcome::Value(v) => v,
            other => panic!("expected value, got {other:?}"),
        }
    }

    #[test]
    fn add_list_remove_clear() {
        let (_tmp, app, pane, _src) = setup();
        assert!(
            app.dispatch(&Invocation::new("shelf.add")).is_err(),
            "empty selection is an error"
        );
        select(&app, pane, &["a.txt", "b.txt"]);
        let v = value(app.dispatch(&Invocation::new("shelf.add")).unwrap());
        assert_eq!(v["added"], 2);
        let v = value(app.dispatch(&Invocation::new("shelf.add")).unwrap());
        assert_eq!(v["added"], 0, "no duplicates");
        let list = value(app.dispatch(&Invocation::new("shelf.list")).unwrap());
        let names: Vec<&str> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["a.txt", "b.txt"]);
        let id = list[0]["id"].clone();
        let v = value(
            app.dispatch(&Invocation::with_args(
                "shelf.remove",
                &serde_json::json!({ "ids": [id] }),
            ))
            .unwrap(),
        );
        assert_eq!(v["removed"], 1);
        app.dispatch(&Invocation::new("shelf.clear")).unwrap();
        assert!(
            value(app.dispatch(&Invocation::new("shelf.list")).unwrap())
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(app.dispatch(&Invocation::new("shelf.trash")).is_err());
    }

    /// A folder's worth of entries shelved, shelved again, half taken off
    /// and the rest cleared: each asked of a set, at a size where
    /// comparing each item with every other would not end.
    #[test]
    fn a_large_selection_is_shelved_once_taken_off_and_cleared() {
        const N: u64 = 50_000;
        let state = Shelf::default();
        let items = || -> Vec<(ShelfItem, Option<Entry>)> {
            (0..N)
                .map(|i| {
                    let name = OsString::from(format!("file {i}.txt"));
                    let path = PathBuf::from("/synthetic/shelved").join(&name);
                    let id = EntryId::at_path(skwir_core::FileId::new(1, i), &path);
                    let item = ShelfItem {
                        id,
                        path,
                        name,
                        kind: EntryKind::File,
                    };
                    (item, None)
                })
                .collect()
        };
        assert_eq!(ShelfPlugin::add(&state, items()), N as usize);
        assert_eq!(ShelfPlugin::add(&state, items()), 0, "each name once");
        let half: Vec<EntryId> = state.lock().items.iter().step_by(2).map(|i| i.id).collect();
        assert_eq!(ShelfPlugin::remove(&state, &half), half.len());
        assert_eq!(state.lock().items.len(), N as usize - half.len());
        ShelfPlugin::clear(&state);
        assert!(state.lock().items.is_empty());
    }

    #[test]
    fn delete_on_the_shelf_takes_files_off_it_and_a_drop_shelves_them() {
        let (_tmp, app, pane, src) = setup();
        select(&app, pane, &["a.txt", "b.txt"]);
        app.dispatch(&Invocation::new("shelf.add")).unwrap();
        app.navigate(pane, &Location::parse("shelf:")).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        select(&app, pane, &["a.txt"]);
        // What Delete runs on the shelf: the location's own taking out.
        let v = value(app.dispatch(&Invocation::new("take_out")).unwrap());
        assert_eq!(v["removed"], 1);
        assert!(src.join("a.txt").exists(), "the file stays where it is");
        let left = value(app.dispatch(&Invocation::new("shelf.list")).unwrap());
        assert_eq!(left.as_array().unwrap().len(), 1);
        // Dropped or pasted on it, by where they are.
        let drop = app
            .verbs(&Location::parse("shelf:"))
            .drop
            .expect("takes drops");
        let mut args = drop.args.clone();
        args["entries"] = serde_json::json!([EntryRef::from_path(src.join("a.txt")).unwrap()]);
        let v = value(
            app.dispatch(&Invocation {
                name: drop.name,
                args,
            })
            .unwrap(),
        );
        assert_eq!(v["added"], 1);
    }

    #[test]
    fn move_here_plan_moves_files_and_skips_items_already_there() {
        if skwir_core::system::under_wine("trashing through the shell") {
            return;
        }
        let (tmp, app, pane, src) = setup();
        select(&app, pane, &["a.txt", "sub"]);
        app.dispatch(&Invocation::new("shelf.add")).unwrap();
        // Still in src: everything is already here.
        assert!(app.dispatch(&Invocation::new("shelf.move_here")).is_err());
        app.navigate(pane, &Location::from_path(tmp.path().join("dest")))
            .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let Outcome::Plan(plan) = app.dispatch(&Invocation::new("shelf.move_here")).unwrap() else {
            panic!("expected plan")
        };
        assert_eq!(plan.len(), 2);
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(tmp.path().join("dest/a.txt").exists() && tmp.path().join("dest/sub").is_dir());
        assert!(!src.join("a.txt").exists());
        // The shelf follows its items to where they went.
        let list = value(app.dispatch(&Invocation::new("shelf.list")).unwrap());
        for item in list.as_array().unwrap() {
            let path = PathBuf::from(item["path"].as_str().unwrap());
            assert_eq!(path.parent(), Some(tmp.path().join("dest").as_path()));
        }
        assert!(
            app.dispatch(&Invocation::new("shelf.move_here")).is_err(),
            "a second move finds everything already here"
        );
        let Outcome::Plan(copy) = app.dispatch(&Invocation::new("shelf.copy_here")).unwrap() else {
            panic!()
        };
        assert_eq!(copy.len(), 2, "copy keeps every item");
        assert!(
            !copy.is_applicable(&std::sync::Arc::default()),
            "copying onto itself conflicts"
        );

        assert_eq!(app.tallies.get(TALLY), Some(2));
        let Outcome::Plan(trash) = app.dispatch(&Invocation::new("shelf.trash")).unwrap() else {
            panic!()
        };
        let r = app.executor.execute_blocking(&trash, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(
            value(app.dispatch(&Invocation::new("shelf.list")).unwrap())
                .as_array()
                .unwrap()
                .is_empty(),
            "trashed items leave the shelf"
        );
        assert_eq!(app.tallies.get(TALLY), Some(0));
    }

    #[test]
    fn removing_broadcasts_only_shelved_ids() {
        let (_tmp, app, pane, _src) = setup();
        select(&app, pane, &["a.txt"]);
        app.dispatch(&Invocation::new("shelf.add")).unwrap();
        let open = app.open_location(&Location::parse("shelf:")).unwrap();
        let mut listing = skwir_core::Listing::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !listing.is_complete() {
            listing.apply(open.events().recv_deadline(deadline).unwrap());
        }
        let stranger = EntryId {
            file: skwir_core::FileId::new(0, 1),
            at: 0,
        };
        let v = value(
            app.dispatch(&Invocation::with_args(
                "shelf.remove",
                &serde_json::json!({ "ids": [stranger] }),
            ))
            .unwrap(),
        );
        assert_eq!(v["removed"], 0);
        assert!(open.events().is_empty(), "nothing to tell the listing");
    }

    #[test]
    fn shelf_location_streams_and_updates_live() {
        let (_tmp, app, pane, _src) = setup();
        select(&app, pane, &["a.txt"]);
        app.dispatch(&Invocation::new("shelf.add")).unwrap();
        let shelf = Location::parse("shelf:");
        // It reads as one place in the address, under its own glyph.
        let crumbs = app.trail(&shelf).unwrap().crumbs;
        assert_eq!(
            crumbs
                .iter()
                .map(|crumb| (crumb.label.english(), &crumb.location))
                .collect::<Vec<_>>(),
            [("Shelf".to_owned(), &shelf)]
        );
        assert_eq!(app.location_icon(&shelf), Some("shelf"));
        // Its buttons put its files in the other pane's folder.
        let tools: Vec<_> = app
            .verbs(&shelf)
            .tools
            .into_iter()
            .map(|t| (t.command.name.to_string(), t.pane))
            .collect();
        assert_eq!(
            tools,
            [
                (
                    "shelf.move_here".to_owned(),
                    skwir_core::ToolPane::OtherFolder
                ),
                (
                    "shelf.copy_here".to_owned(),
                    skwir_core::ToolPane::OtherFolder
                ),
            ]
        );
        let open = app.open_location(&shelf).unwrap();
        let mut listing = skwir_core::Listing::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !listing.is_complete() {
            listing.apply(open.events().recv_deadline(deadline).unwrap());
        }
        assert_eq!(listing.len(), 1);
        let ids: Vec<EntryId> = listing.ids().collect();
        assert!(open.path_of(ids[0]).unwrap().ends_with("a.txt"));
        // Live update: adding b.txt shows up without a refresh.
        select(&app, pane, &["b.txt"]);
        app.dispatch(&Invocation::new("shelf.add")).unwrap();
        let ev = open.events().recv_timeout(Duration::from_secs(2)).unwrap();
        listing.apply(ev);
        assert_eq!(listing.len(), 2);
        // Hydration fills metadata for shelved entries.
        open.hydrate(&ids, Priority::Visible);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            listing.apply(open.events().recv_deadline(deadline).unwrap());
            if listing.get(ids[0]).unwrap().metadata.is_hydrated() {
                break;
            }
        }
        app.dispatch(&Invocation::new("shelf.clear")).unwrap();
        let ev = open.events().recv_timeout(Duration::from_secs(2)).unwrap();
        listing.apply(ev);
        assert!(listing.is_empty());
    }

    /// A metadata provider that holds the first batch naming `name` until
    /// let go: a hydration held after its stats, before it is told.
    struct Hold {
        name: OsString,
        once: Mutex<Option<(crossbeam_channel::Sender<()>, Receiver<()>)>>,
    }

    impl skwir_core::metadata::MetadataProvider for Hold {
        fn id(&self) -> &'static str {
            "test.hold"
        }

        fn fill(&self, _dir: &Path, entries: &mut [Entry]) {
            if !entries.iter().any(|e| e.name == self.name) {
                return;
            }
            let Some((held, go)) = self.once.lock().take() else {
                return;
            };
            let _ = held.send(());
            // Let go, or the test gone.
            let _ = go.recv_timeout(Duration::from_secs(30));
        }
    }

    /// Hold the first batch read naming `name`: what is returned is told
    /// once it is held, and lets it go.
    fn hold(app: &App, name: &str) -> (Receiver<()>, crossbeam_channel::Sender<()>) {
        let (held, is_held) = crossbeam_channel::bounded(1);
        let (go, gone) = crossbeam_channel::bounded(1);
        app.metadata.register(
            "test",
            Arc::new(Hold {
                name: name.into(),
                once: Mutex::new(Some((held, gone))),
            }),
        );
        (is_held, go)
    }

    /// Apply what `open` sends to `listing` until `done` holds of it.
    fn until(
        open: &dyn OpenLocation,
        listing: &mut skwir_core::Listing,
        what: &str,
        done: impl Fn(&skwir_core::Listing) -> bool,
    ) {
        until_within(open, listing, what, Duration::from_secs(10), done);
    }

    /// [`until`], failing after `within`.
    fn until_within(
        open: &dyn OpenLocation,
        listing: &mut skwir_core::Listing,
        what: &str,
        within: Duration,
        done: impl Fn(&skwir_core::Listing) -> bool,
    ) {
        let deadline = std::time::Instant::now() + within;
        while !done(listing) {
            let Ok(event) = open.events().recv_deadline(deadline) else {
                panic!("never {what}");
            };
            listing.apply(event);
        }
    }

    fn home_tempdir() -> tempfile::TempDir {
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::tempdir_in(base).unwrap()
    }

    /// A plan renames a shelved file and its undo names it back while a
    /// hydration that found it gone at its name is held: told after both
    /// follows, that read is older than theirs, and the file stays on the
    /// shelf with its row.
    #[test]
    fn a_rename_undone_while_a_hydration_is_held_leaves_the_item_shelved() {
        let tmp = home_tempdir();
        let src = tmp.path().join("l'été, à l'ombre");
        std::fs::create_dir(&src).unwrap();
        let (name, kept) = ("it's ☂ here.txt", "keep me, s'il vous plaît.txt");
        let (here, there) = (src.join(name), src.join("it's ☂ there.txt"));
        std::fs::write(&here, b"shelved").unwrap();
        std::fs::write(src.join(kept), b"kept").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = Arc::new(ShelfPlugin::default());
        let shelf = Arc::clone(&plugin.state);
        app.plugins.add(plugin, &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&src)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        select(&app, pane, &[name, kept]);
        app.dispatch(&Invocation::new("shelf.add")).unwrap();
        let id_of = |path: &Path| {
            let s = shelf.lock();
            s.items.iter().find(|i| i.path == path).map(|i| i.id)
        };
        let (x, k) = (id_of(&here).unwrap(), id_of(&src.join(kept)).unwrap());
        let open = app.open_location(&Location::parse("shelf:")).unwrap();
        let mut listing = skwir_core::Listing::default();
        until(
            &*open,
            &mut listing,
            "listed",
            skwir_core::Listing::is_complete,
        );
        let (held, go) = hold(&app, kept);
        // The plan's rename is made, its observers not told yet: the
        // hydration finds the file gone at its name, and is held.
        std::fs::rename(&here, &there).unwrap();
        open.hydrate(&[x, k], Priority::Visible);
        held.recv_timeout(Duration::from_secs(10)).expect("held");
        let rename = |from: &Path, to: &Path| Change::Rename {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        };
        follow(&shelf, &[rename(&here, &there)], &app);
        until(&*open, &mut listing, "renamed", |l| {
            l.get(x).is_none() && l.iter().any(|e| e.name == "it's ☂ there.txt")
        });
        // Undone: named back, its observers told, its file found again.
        std::fs::rename(&there, &here).unwrap();
        follow(&shelf, &[rename(&there, &here)], &app);
        until(&*open, &mut listing, "named back", |l| {
            l.get(x).is_some_and(|e| e.name == name)
        });
        // The hydration let go: what it found is told after the follows'.
        go.send(()).unwrap();
        until(&*open, &mut listing, "answered", |l| {
            l.get(k).is_some_and(|e| e.metadata.is_filled())
        });
        assert!(app.scheduler.wait_idle_here(Duration::from_secs(10)));
        for event in open.events().try_iter() {
            listing.apply(event);
        }
        let items = shelf.lock().items.to_vec();
        assert!(
            items.iter().any(|i| i.id == x && i.path == here),
            "still shelved where it is: {items:?}"
        );
        assert_eq!(items.len(), 2);
        assert_eq!(listing.get(x).map(|e| e.name.clone()), Some(name.into()));
        assert_eq!(listing.len(), 2);
    }

    /// An item taken off the shelf and shelved again bare while its
    /// hydration is held: told after, that read is left out behind the
    /// shelving, which told its row less, and is read again, so the row is
    /// filled though nothing asks for it again.
    #[test]
    fn an_item_shelved_again_while_its_hydration_is_held_is_read_again() {
        let tmp = home_tempdir();
        let src = tmp.path().join("l'été, à l'ombre");
        std::fs::create_dir(&src).unwrap();
        let name = "it's ☂ shelved again.txt";
        std::fs::write(src.join(name), b"shelved").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = Arc::new(ShelfPlugin::default());
        let shelf = Arc::clone(&plugin.state);
        app.plugins.add(plugin, &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&src)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        select(&app, pane, &[name]);
        app.dispatch(&Invocation::new("shelf.add")).unwrap();
        let x = shelf.lock().items.iter().next().unwrap().id;
        let open = app.open_location(&Location::parse("shelf:")).unwrap();
        let mut listing = skwir_core::Listing::default();
        until(
            &*open,
            &mut listing,
            "listed",
            skwir_core::Listing::is_complete,
        );
        assert!(!listing.get(x).unwrap().metadata.is_filled());
        let (held, go) = hold(&app, name);
        open.hydrate(&[x], Priority::Visible);
        held.recv_timeout(Duration::from_secs(10)).expect("held");
        // Taken off and shelved again, as the pane's listing knows it: bare.
        let ids = serde_json::json!({ "ids": [x] });
        app.dispatch(&Invocation::with_args("shelf.remove", &ids))
            .unwrap();
        app.dispatch(&Invocation::with_args("shelf.add", &ids))
            .unwrap();
        until(&*open, &mut listing, "shelved again", |l| {
            l.get(x).is_some()
        });
        // The hydration let go: its answer is the older, and its row is
        // read again rather than left unfilled.
        go.send(()).unwrap();
        until(&*open, &mut listing, "filled", |l| {
            l.get(x).is_some_and(|e| e.metadata.is_filled())
        });
        assert_eq!(shelf.lock().items.len(), 1);
    }

    /// The read of an item left out behind a later one.
    #[derive(Clone, Copy, Debug)]
    enum Older {
        /// The other listing's hydration.
        Hydration,
        /// A follow's read of the item where a plan put it.
        Follow,
    }

    /// The shelf shown twice (the split, two windows): a read of an item
    /// begun first, held, is left out behind a later hydration asked by
    /// the first listing alone, and neither listing's row stays unread:
    /// what stands is told to both, the second's waiting on no answer of
    /// its own.
    #[test]
    fn a_read_left_out_behind_another_listings_leaves_no_row_unread() {
        for older in [Older::Hydration, Older::Follow] {
            left_out_behind_another_listing(older);
        }
    }

    fn left_out_behind_another_listing(older: Older) {
        let tmp = home_tempdir();
        let src = tmp.path().join("l'été, à l'ombre");
        std::fs::create_dir(&src).unwrap();
        let name = "it's ☂ shown twice.txt";
        let here = src.join(name);
        std::fs::write(&here, b"shelved").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = Arc::new(ShelfPlugin::default());
        let shelf = Arc::clone(&plugin.state);
        app.plugins.add(plugin, &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&src)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        select(&app, pane, &[name]);
        app.dispatch(&Invocation::new("shelf.add")).unwrap();
        let x = shelf.lock().items.iter().next().unwrap().id;
        let (first, second) = (
            app.open_location(&Location::parse("shelf:")).unwrap(),
            app.open_location(&Location::parse("shelf:")).unwrap(),
        );
        let (mut one, mut two) = (
            skwir_core::Listing::default(),
            skwir_core::Listing::default(),
        );
        until(
            &*first,
            &mut one,
            "listed",
            skwir_core::Listing::is_complete,
        );
        until(
            &*second,
            &mut two,
            "listed",
            skwir_core::Listing::is_complete,
        );
        let (held, go) = hold(&app, name);
        let filled = |l: &skwir_core::Listing| l.get(x).is_some_and(|e| e.metadata.is_filled());
        std::thread::scope(|scope| {
            // The older read: stat-ed first, held before it is told.
            match older {
                Older::Hydration => second.hydrate(&[x], Priority::Visible),
                Older::Follow => {
                    let (shelf, here, metadata) = (&shelf, here.clone(), &app.metadata);
                    scope.spawn(move || shelf.restat(vec![(x, here)], metadata));
                }
            }
            held.recv_timeout(Duration::from_secs(10)).expect("held");
            // The later, asked by the first listing: told first.
            first.hydrate(&[x], Priority::Visible);
            until(&*first, &mut one, "filled for the first", filled);
            go.send(()).unwrap();
        });
        until(
            &*second,
            &mut two,
            &format!("filled for the second ({older:?} left out)"),
            filled,
        );
        assert!(app.scheduler.wait_idle_here(Duration::from_secs(10)));
        assert_eq!(shelf.lock().items.len(), 1, "{older:?}");
        assert!(filled(&one) && filled(&two), "{older:?}");
    }

    /// A hydration of a shelf of a folder's worth of files, all gone (their
    /// folder never made), takes them all off: each answer asked of a map
    /// of the items, at a size where asking each of every item would not
    /// end.
    #[test]
    fn a_large_shelf_found_gone_is_taken_off_at_once() {
        const N: u64 = 50_000;
        let tmp = home_tempdir();
        let gone = tmp.path().join("never made, l'été");
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        let plugin = Arc::new(ShelfPlugin::default());
        let shelf = Arc::clone(&plugin.state);
        app.plugins.add(plugin, &app);
        let items: Vec<(ShelfItem, Option<Entry>)> = (0..N)
            .map(|i| {
                let name = OsString::from(format!("file {i} ☂.txt"));
                let path = gone.join(&name);
                let id = EntryId::at_path(skwir_core::FileId::new(1, i), &path);
                let item = ShelfItem {
                    id,
                    path,
                    name,
                    kind: EntryKind::File,
                };
                (item, None)
            })
            .collect();
        assert_eq!(ShelfPlugin::add(&shelf, items), N as usize);
        let open = app.open_location(&Location::parse("shelf:")).unwrap();
        let mut listing = skwir_core::Listing::default();
        until(&*open, &mut listing, "listed", |l| {
            l.is_complete() && l.len() == N as usize
        });
        let ids: Vec<EntryId> = listing.ids().collect();
        open.hydrate(&ids, Priority::Normal);
        // A bound on a loaded machine's debug build, not a measure: asked
        // of every item for each answer, this took minutes.
        until_within(
            &*open,
            &mut listing,
            "taken off",
            Duration::from_secs(60),
            skwir_core::Listing::is_empty,
        );
        assert!(shelf.lock().items.is_empty());
        assert_eq!(app.tallies.get(TALLY), Some(0));
    }

    #[test]
    fn disable_withdraws_everything() {
        let (_tmp, app, _pane, _src) = setup();
        assert!(app.commands.get("shelf.add").is_some());
        assert!(app.provider("shelf").is_some());
        app.plugins.set_enabled("shelf", false, &app);
        assert!(app.commands.get("shelf.add").is_none());
        assert!(app.provider("shelf").is_none());
        assert!(
            app.registry
                .places
                .all()
                .iter()
                .all(|p| p.title.english() != "Shelf")
        );
        assert!(
            app.registry
                .keymap
                .all()
                .iter()
                .all(|k| !k.command.name.starts_with("shelf."))
        );
    }
}
