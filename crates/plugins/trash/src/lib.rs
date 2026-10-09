//! bibli plugin: trash. The desktop's trash as a place, `trash:`: what
//! every trash of the user's holds (`bibli_platform::trash`: the home
//! trash and the volumes'), each file under the name it had, with the
//! folder it came from and when it was put there. Its files are restored
//! (`trash.restore`: the core's `Restore` of that very item) or deleted for
//! good (Delete, Empty Trash: the core's `Delete`, which takes the
//! `.trashinfo` along, once the shell has asked); files dropped on it, or
//! on its place in the sidebar, are put there. The trashes are read on a
//! worker, and again when one changes (a watch on each `info` folder, and
//! the plans bibli applies), so the listings open and the place's icon
//! follow.
//!
//! A trashed folder stays in the trash when opened (`trash:` and its
//! path, a trail of `Trash › old project › sub`): its files are restored
//! alone (the core's `RestorePart`, making its folders again) or deleted
//! for good, and files dropped or pasted there are put in the trash, as on
//! the Trash itself.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, NoArgs, Outcome};
use bibli_core::entry::{Entry, EntryId, keys};
use bibli_core::fs::stat::stat_path;
use bibli_core::location::{
    Crumb, EmptyState, EventSink, ListingEvent, Location, LocationProvider, OpenContext,
    OpenLocation, PATH_SCHEME, Tool, ToolPane, Trail, Verbs, escape, unescape,
};
use bibli_core::op::{
    Change, Delete, EntryRef, Operation, Plan, Restore, RestorePart, Trash, in_trash,
};
use bibli_core::os_path::PathArg;
use bibli_core::plugin::Plugin;
use bibli_core::registry::{MenuItem, Place};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use bibli_core::{Provenance, Value};
use bibli_platform::calendar::Calendar;
use bibli_platform::trash::Trashed;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;

const ID: &str = "trash";
/// What emptying the trash is called, in its place's menu and on its
/// button: it asks first.
const EMPTY_TRASH: &str = "Empty Trash…";
/// The location scheme of the trash.
pub const SCHEME: &str = "trash";
/// Changes this close together (a folder of files put in the trash at
/// once) are read once.
const SETTLE: Duration = Duration::from_millis(250);

/// A file in a trash, as its listing shows it.
#[derive(Clone, Debug)]
struct Item {
    /// Stat-ed (its own identity, size and times), under the name it had
    /// before, with where it came from and when (`mark`).
    entry: Entry,
    trashed: Trashed,
}

impl Item {
    fn entry_ref(&self) -> EntryRef {
        EntryRef::new(self.entry.id, self.trashed.file.clone())
    }
}

/// Where a folder of the trash leads, staying in the trash: `trash:` and
/// its path there.
fn folder_location(folder: &Path) -> Location {
    Location::new(SCHEME, escape(folder.as_os_str().as_encoded_bytes()))
}

/// The folder of the trash `location` shows; `None` for the trash itself.
fn folder_of(location: &Location) -> Option<PathBuf> {
    (!location.rest.is_empty())
        .then(|| PathBuf::from(bibli_core::os_path::from_bytes(unescape(&location.rest))))
}

/// Names a trashed file's entry as the file was named, and says when it was
/// put in the trash and where from.
fn mark(entry: &mut Entry, trashed: &Trashed) {
    if let Some(name) = trashed.original.file_name() {
        name.clone_into(&mut entry.name);
    }
    if let Some(when) = trashed.deleted {
        entry
            .metadata
            .set(keys::TRASHED, Value::Time(when), Provenance::Filesystem);
    }
    if let Some(from) = trashed.original.parent() {
        entry.metadata.set(
            keys::TRASHED_FROM,
            Value::Text(from.to_string_lossy().into()),
            Provenance::Filesystem,
        );
    }
}

/// What `trashes` hold. I/O: a worker's.
fn read(trashes: &[PathBuf], calendar: &Calendar) -> Vec<Item> {
    let mut seen = HashSet::new();
    trashes
        .iter()
        .flat_map(|trash| bibli_platform::trash::list(trash, calendar))
        .filter_map(|trashed| {
            let mut entry = stat_path(&trashed.file)?;
            mark(&mut entry, &trashed);
            seen.insert(entry.id).then_some(Item { entry, trashed })
        })
        .collect()
}

#[derive(Default)]
struct State {
    /// What the trashes held when last read.
    items: Vec<Item>,
    /// Where each of `items` is among them, by its entry's id.
    by_id: HashMap<EntryId, usize>,
    /// Whether they were read since the plugin started: a listing opened
    /// before waits in `waiting`.
    read: bool,
    /// Open `trash:` listings.
    sinks: Vec<EventSink>,
    waiting: Vec<EventSink>,
    /// Bumped by every reading: only the latest one publishes, so a slow
    /// one from before a change cannot overwrite what came after.
    generation: u64,
    /// The plugin's lifetime; `None` once disabled.
    lifetime: Option<CancelToken>,
    /// Watches each trash's `info`; dropped on disable, which ends the
    /// thread that waits for it.
    watcher: Option<bibli_core::fs::Settled>,
    /// The `info` folders watched.
    watched: Vec<PathBuf>,
    /// Whether the place said last that the trash holds something.
    full: Option<bool>,
}

impl State {
    fn broadcast(&mut self, event: &ListingEvent) {
        self.sinks.retain(|sink| sink.send(event.clone()));
    }

    fn entries(&self) -> Vec<Entry> {
        self.items.iter().map(|i| i.entry.clone()).collect()
    }

    fn item(&self, id: EntryId) -> Option<&Item> {
        self.by_id.get(&id).and_then(|&n| self.items.get(n))
    }

    /// The item whose file in the trash is `file`.
    fn item_at(&self, file: &Path) -> Option<&Item> {
        self.items.iter().find(|i| i.trashed.file == file)
    }
}

/// What the plugin's commands, listings and workers share.
#[derive(Clone, Default)]
struct Trashes {
    state: Arc<Mutex<State>>,
    /// The trashes read: the user's when `None`.
    only: Option<Arc<[PathBuf]>>,
}

impl Trashes {
    /// The trashes to read: asks every volume, so a worker's.
    fn trashes(&self) -> Vec<PathBuf> {
        match &self.only {
            Some(only) => only.to_vec(),
            None => bibli_platform::trash::trashes(),
        }
    }

    /// Read the trashes again, on a worker, and publish what changed.
    fn refresh(&self, app: &App) {
        let (generation, lifetime) = {
            let mut s = self.state.lock();
            s.generation += 1;
            (s.generation, s.lifetime.clone())
        };
        let Some(lifetime) = lifetime else {
            return;
        };
        let (this, weak) = (self.clone(), app.weak());
        app.scheduler.spawn(
            Priority::Normal,
            "trash.read",
            lifetime.child(),
            move || {
                let trashes = this.trashes();
                let items = read(&trashes, &Calendar::system());
                if let Some(app) = weak.upgrade() {
                    this.publish(&app, generation, items, &trashes);
                }
            },
        );
    }

    /// `items`, read by the reading numbered `generation`, become what the
    /// trash holds: the listings open are told what changed, the place
    /// says whether the trash is empty, and the trashes found are watched.
    /// On the worker that read them (watching is I/O).
    fn publish(&self, app: &App, generation: u64, items: Vec<Item>, trashes: &[PathBuf]) {
        let mut guard = self.state.lock();
        let s = &mut *guard;
        if s.generation != generation || s.lifetime.as_ref().is_none_or(CancelToken::is_cancelled) {
            return;
        }
        let before: HashMap<EntryId, &Entry> =
            s.items.iter().map(|i| (i.entry.id, &i.entry)).collect();
        let now: HashSet<EntryId> = items.iter().map(|i| i.entry.id).collect();
        let removed: Vec<EntryId> = before
            .keys()
            .copied()
            .filter(|id| !now.contains(id))
            .collect();
        let added: Vec<Entry> = items
            .iter()
            .filter(|i| before.get(&i.entry.id) != Some(&&i.entry))
            .map(|i| i.entry.clone())
            .collect();
        s.items = items;
        s.by_id = s
            .items
            .iter()
            .enumerate()
            .map(|(n, i)| (i.entry.id, n))
            .collect();
        if !removed.is_empty() {
            s.broadcast(&ListingEvent::Removed(removed));
        }
        if !added.is_empty() {
            s.broadcast(&ListingEvent::Added(added));
        }
        if !s.read {
            s.read = true;
            let entries = s.entries();
            for sink in std::mem::take(&mut s.waiting) {
                if (entries.is_empty() || sink.send(ListingEvent::Added(entries.clone())))
                    && sink.send(ListingEvent::Complete)
                {
                    s.sinks.push(sink);
                }
            }
        }
        if let Some(watcher) = s.watcher.as_mut() {
            for info in trashes.iter().map(|t| t.join("info")) {
                if !s.watched.contains(&info) && info.is_dir() && watcher.watch(&info) {
                    s.watched.push(info);
                }
            }
        }
        let full = !s.items.is_empty();
        let changed = s.full != Some(full);
        s.full = Some(full);
        drop(guard);
        if changed {
            app.registry.places.replace_owner(ID, [place(full)]);
        }
        app.wake();
    }

    /// Watch the trashes: a change to one reads them again, once the
    /// changes settle, on a thread of the plugin's own that ends with the
    /// watcher (so no shared worker waits).
    fn watch(&self, app: &App) {
        let (this, weak) = (self.clone(), app.weak());
        let watcher = bibli_core::fs::Settled::start(
            "bibli-trash",
            SETTLE,
            |_| true,
            move || {
                let Some(app) = weak.upgrade() else {
                    return false;
                };
                this.refresh(&app);
                true
            },
        );
        if let Some(watcher) = watcher {
            let mut s = self.state.lock();
            s.watcher = Some(watcher);
            s.watched.clear();
        }
    }

    /// The trash's items that are `ids`.
    fn items(&self, ids: &[EntryId]) -> Vec<Item> {
        let s = self.state.lock();
        ids.iter().filter_map(|id| s.item(*id).cloned()).collect()
    }
}

/// The trash's place in the sidebar, among the places of the home folder:
/// files dropped on it are put in the trash, and its menu empties it.
fn place(full: bool) -> Place {
    let menu = if full {
        vec![MenuItem::new(
            "place",
            EMPTY_TRASH,
            "trash",
            300,
            Invocation::new("trash.empty"),
        )]
    } else {
        Vec::new()
    };
    let icon = if full {
        "user-trash-full"
    } else {
        "user-trash"
    };
    Place {
        menu,
        ..Place::new("places", "Trash", icon, Location::new(SCHEME, ""))
    }
}

// ------------------------------------------------------------------ location

struct TrashProvider {
    trashes: Trashes,
    app: Weak<App>,
}

impl TrashProvider {
    /// What a folder of the trash is called: the item its own name had,
    /// a folder inside it its name.
    fn name_of(&self, folder: &Path) -> String {
        let item = in_trash(folder).filter(|t| t.inner.as_os_str().is_empty());
        item.and_then(|_| {
            let s = self.trashes.state.lock();
            s.item_at(folder).map(|i| i.entry.name_str().into_owned())
        })
        .or_else(|| folder.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default()
    }
}

impl LocationProvider for TrashProvider {
    fn scheme(&self) -> &'static str {
        SCHEME
    }

    fn title(&self, location: &Location) -> Option<String> {
        Some(match folder_of(location) {
            Some(folder) => self.name_of(&folder),
            None => "Trash".into(),
        })
    }

    /// The trash, then a trashed folder under its own name, then the
    /// folders inside it, each where it is in the trash. Each holds
    /// places, the trash's top its trashed folders: the chevron after the
    /// last lists them, as a folder's lists its folders.
    fn trail(&self, location: &Location) -> Option<Trail> {
        let Some(folder) = folder_of(location) else {
            return Some(Trail {
                crumbs: vec![Crumb::new("Trash", Location::new(SCHEME, ""))],
                holds_places: true,
                ..Trail::default()
            });
        };
        let inside = in_trash(&folder)?;
        let mut crumbs = vec![
            Crumb::new("Trash", Location::new(SCHEME, "")),
            Crumb::new(self.name_of(&inside.item), folder_location(&inside.item)),
        ];
        let mut at = inside.item.clone();
        for part in inside.inner.components() {
            at.push(part);
            crumbs.push(Crumb::new(
                part.as_os_str().to_string_lossy(),
                folder_location(&at),
            ));
        }
        Some(Trail {
            crumbs,
            holds_places: true,
            one_folder: true,
            ..Trail::default()
        })
    }

    /// The trashed folders at its top, the folders in a trashed folder:
    /// what the trail's chevrons list.
    fn places(&self, location: &Location) -> Vec<Crumb> {
        let mut places: Vec<Crumb> = match folder_of(location) {
            None => self
                .trashes
                .state
                .lock()
                .items
                .iter()
                .filter(|i| i.entry.is_dir())
                .map(|i| Crumb::new(i.entry.name_str(), folder_location(&i.trashed.file)))
                .collect(),
            // As a folder's: hidden ones too, a trash holds what it was given.
            Some(folder) => bibli_core::fs::subfolders(&folder, true)
                .unwrap_or_default()
                .into_iter()
                .map(|dir| {
                    let name = dir
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    Crumb::new(name, folder_location(&dir))
                })
                .collect(),
        };
        places
            .sort_by_cached_key(|c| bibli_core::collate::sort_key(std::ffi::OsStr::new(&c.label)));
        places
    }

    fn icon(&self, _location: &Location) -> Option<&'static str> {
        Some("trash")
    }

    fn empty_state(&self, location: &Location) -> Option<EmptyState> {
        Some(match folder_of(location) {
            None => EmptyState::new(
                "The trash is empty",
                "Files moved to the trash wait here until it is emptied: restore them from their menu.",
            ),
            Some(_) => EmptyState::new(
                "This folder is empty",
                "Nothing it held is left in the trash.",
            ),
        })
    }

    /// Files dropped or pasted here are put in the trash, in one of its
    /// folders as on the trash itself; its own are restored
    /// (`Verbs::restore`), and deleted for good by Delete, or all at once
    /// by the trash's button.
    fn verbs(&self, location: &Location) -> Verbs {
        Verbs {
            drop: Some(Invocation::new("trash.put")),
            restore: Some(Invocation::new("trash.restore")),
            tools: if folder_of(location).is_none() {
                vec![Tool::new(
                    "trash",
                    EMPTY_TRASH,
                    Invocation::new("trash.empty"),
                    ToolPane::Shown,
                )]
            } else {
                Vec::new()
            },
            ..Verbs::default()
        }
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        if let Some(folder) = folder_of(location) {
            return self.open_folder(location, &folder, ctx);
        }
        let (sink, rx) = ctx.channel();
        {
            let mut s = self.trashes.state.lock();
            if s.read {
                let entries = s.entries();
                if !entries.is_empty() {
                    sink.send(ListingEvent::Added(entries));
                }
                sink.send(ListingEvent::Complete);
                s.sinks.push(sink.clone());
            } else {
                s.waiting.push(sink.clone());
            }
        }
        // Shown at once as last read, then read again: a volume mounted
        // since brings its trash.
        if let Some(app) = self.app.upgrade() {
            self.trashes.refresh(&app);
        }
        Ok(Arc::new(TrashLocation {
            location: location.clone(),
            rx,
            ctx,
            sink,
            trashes: self.trashes.clone(),
            app: self.app.clone(),
        }))
    }
}

impl TrashProvider {
    /// A folder of the trash, listed as the folder it is (the core's own
    /// listing, watched), kept in the trash: its folders lead into it.
    fn open_folder(
        &self,
        location: &Location,
        folder: &Path,
        ctx: OpenContext,
    ) -> anyhow::Result<Arc<dyn OpenLocation>> {
        anyhow::ensure!(
            in_trash(folder).is_some(),
            "{} is no folder of the trash",
            folder.display()
        );
        let app = self
            .app
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("bibli is closing"))?;
        let folders = app
            .provider(PATH_SCHEME)
            .ok_or_else(|| anyhow::anyhow!("no provider for folders"))?;
        Ok(Arc::new(TrashFolder {
            location: location.clone(),
            inner: folders.open(&Location::from_path(folder), ctx)?,
        }))
    }
}

/// A folder of the trash: the folder's own listing, under its location in
/// the trash.
struct TrashFolder {
    location: Location,
    inner: Arc<dyn OpenLocation>,
}

impl OpenLocation for TrashFolder {
    fn location(&self) -> &Location {
        &self.location
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        self.inner.events()
    }

    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        self.inner.hydrate(ids, priority);
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.inner.path_of(id)
    }

    /// A folder inside it stays in the trash too.
    fn location_of(&self, id: EntryId) -> Option<Location> {
        self.inner.path_of(id).map(|path| folder_location(&path))
    }

    fn holds(&self) -> Option<&std::path::Path> {
        self.inner.holds()
    }

    fn refresh(&self) {
        self.inner.refresh();
    }

    fn close(&self) {
        self.inner.close();
    }
}

struct TrashLocation {
    location: Location,
    rx: Receiver<ListingEvent>,
    ctx: OpenContext,
    sink: EventSink,
    trashes: Trashes,
    app: Weak<App>,
}

impl OpenLocation for TrashLocation {
    fn location(&self) -> &Location {
        &self.location
    }

    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }

    /// Stat-ed again and filled by the metadata providers (tags), each in
    /// the trash's `files` under the name it has there, then named as it
    /// was.
    fn hydrate(&self, ids: &[EntryId], priority: Priority) {
        let items = self.trashes.items(ids);
        if items.is_empty() {
            return;
        }
        let (sink, metadata, cancel) = (
            self.sink.clone(),
            Arc::clone(&self.ctx.metadata),
            self.ctx.cancel.child(),
        );
        self.ctx
            .scheduler
            .spawn(priority, "trash.stat", cancel.clone(), move || {
                let mut by_folder: HashMap<&Path, Vec<&Item>> = HashMap::new();
                for item in &items {
                    if let Some(folder) = item.trashed.file.parent() {
                        by_folder.entry(folder).or_default().push(item);
                    }
                }
                let mut updated = Vec::with_capacity(items.len());
                for (folder, items) in by_folder {
                    if cancel.is_cancelled() {
                        return;
                    }
                    let mut entries: Vec<Entry> = items
                        .iter()
                        .filter_map(|item| {
                            let mut entry = stat_path(&item.trashed.file)?;
                            entry.id = item.entry.id;
                            Some(entry)
                        })
                        .collect();
                    metadata.fill(folder, &mut entries);
                    for (entry, item) in entries.iter_mut().zip(&items) {
                        mark(entry, &item.trashed);
                    }
                    updated.append(&mut entries);
                }
                if !updated.is_empty() && !cancel.is_cancelled() {
                    sink.send(ListingEvent::Updated(updated));
                }
            });
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.trashes
            .state
            .lock()
            .item(id)
            .map(|i| i.trashed.file.clone())
    }

    /// A trashed folder stays in the trash when opened.
    fn location_of(&self, id: EntryId) -> Option<Location> {
        let s = self.trashes.state.lock();
        let item = s.item(id).filter(|i| i.entry.is_dir())?;
        Some(folder_location(&item.trashed.file))
    }

    fn refresh(&self) {
        if let Some(app) = self.app.upgrade() {
            self.trashes.refresh(&app);
        }
    }

    fn close(&self) {}
}

// ------------------------------------------------------------------ commands

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct PutArgs {
    /// The pane whose selection goes in the trash: one dragged from.
    #[serde(default)]
    pane: Option<PaneId>,
    /// Files by where they are (another application's, dropped; a
    /// script's or an agent's, by absolute path), instead of a pane's
    /// selection.
    #[serde(default)]
    entries: Vec<bibli_core::op::EntryArg>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct RestoreArgs {
    /// The pane showing the trash, or a folder of it, whose selection is
    /// restored.
    #[serde(default)]
    pane: Option<PaneId>,
    /// The entries of that pane to restore, instead of its selection.
    #[serde(default)]
    ids: Vec<EntryId>,
    /// Files of the trash to restore, by absolute path (in a trash's
    /// `files`, or inside a folder there), instead of a pane's.
    #[serde(default)]
    entries: Vec<bibli_core::op::EntryArg>,
}

/// What `trash.list` says of a file in the trash.
#[derive(serde::Serialize)]
struct Listed {
    #[serde(with = "bibli_core::os_path")]
    name: std::ffi::OsString,
    #[serde(with = "bibli_core::os_path")]
    original: PathBuf,
    #[serde(with = "bibli_core::os_path")]
    file: PathBuf,
    /// Seconds since 1970, when the trash says.
    deleted: Option<u64>,
}

/// The entries `ids` of `pane`, or its selection, where they are.
fn chosen(app: &App, pane: Option<PaneId>, ids: Vec<EntryId>) -> Vec<EntryRef> {
    let ws = app.workspace.read();
    let Some(p) = ws.pane(pane.unwrap_or_else(|| ws.active_id())) else {
        return Vec::new();
    };
    let ids = if ids.is_empty() {
        p.selection.iter().collect()
    } else {
        ids
    };
    ids.into_iter()
        .filter_map(|id| Some(EntryRef::new(id, p.path_of(id)?)))
        .collect()
}

/// The step that puts `entry`, a file of the trash, back where it was: a
/// whole item by what the trash says of it, a file inside a trashed folder
/// alone, where that folder was.
/// `trashed` is what the trash says of the item `entry` is in: what the
/// plugin read (`State::item_at`), or for an entry named by path, what
/// its trash says now (`read_item`).
fn restore_step(
    entry: EntryRef,
    trashed: impl FnOnce(&Path) -> Option<Trashed>,
) -> Option<Box<dyn Operation>> {
    let inside = in_trash(&entry.path)?;
    let trashed = trashed(&inside.item)?;
    Some(if inside.inner.as_os_str().is_empty() {
        Box::new(Restore {
            path: trashed.original,
            item: Some(PathArg(trashed.info)),
            landed: Vec::new(),
        })
    } else {
        Box::new(RestorePart {
            path: trashed.original.join(&inside.inner),
            entry,
        })
    })
}

/// What its trash says of the item `file` (in the trash's `files`, or a
/// Recycle Bin's own item), read from that trash. Reads it whole.
fn read_item(file: &Path) -> Option<Trashed> {
    let parent = file.parent()?;
    let trash = if parent.file_name() == Some("files".as_ref()) {
        parent.parent()?
    } else {
        parent
    };
    bibli_platform::trash::list(trash, &Calendar::system())
        .into_iter()
        .find(|trashed| trashed.file == file)
}

fn register_commands(app: &App, trashes: &Trashes) {
    app.commands.register::<PutArgs, _>(
        ID,
        "trash.put",
        "Put in the trash",
        "Move files to the trash (plan): a pane's selection, or the entries given (files dropped on the Trash)",
        |a, app| {
            let refs = app.targets(a.pane, a.entries)?;
            let steps: Vec<Box<dyn Operation>> = refs
                .into_iter()
                .filter(|r| in_trash(&r.path).is_none())
                .map(|entry| Box::new(Trash { entry }) as Box<dyn Operation>)
                .collect();
            anyhow::ensure!(!steps.is_empty(), "These are in the trash already");
            Ok(Outcome::Plan(Plan::with_steps("move to trash", steps)))
        },
    )
        .plans();
    let t = trashes.clone();
    app.commands.register::<RestoreArgs, _>(
        ID,
        "trash.restore",
        "Restore from the trash",
        "Put files of the trash back where they came from (plan): the trash pane's selection, or the entries given",
        move |a, app| {
            let named = !a.entries.is_empty();
            let steps: Vec<Box<dyn Operation>> = if named {
                // Named by whoever asked, on their thread: the trash is
                // read, which the plugin may not have done yet (`bibli
                // cmd` runs a bibli just started).
                bibli_core::op::EntryArg::refs(a.entries)?
                    .into_iter()
                    .filter_map(|entry| restore_step(entry, read_item))
                    .collect()
            } else {
                let refs = chosen(app, a.pane, a.ids);
                let s = t.state.lock();
                refs.into_iter()
                    .filter_map(|entry| {
                        restore_step(entry, |item| Some(s.item_at(item)?.trashed.clone()))
                    })
                    .collect()
            };
            if steps.is_empty() {
                anyhow::ensure!(!named, "These are not in the trash");
                return Err(bibli_core::command::NothingSelected.into());
            }
            Ok(Outcome::Plan(Plan::with_steps("restore from the trash", steps)))
        },
    )
        .plans();
    let t = trashes.clone();
    app.commands
        .register::<NoArgs, _>(
            ID,
            "trash.empty",
            "Empty the trash",
            "Delete every file in the trash for good (plan, irreversible)",
            move |_, _| {
                let s = t.state.lock();
                anyhow::ensure!(
                    s.read,
                    "The trash is still being read: try again in a moment"
                );
                anyhow::ensure!(!s.items.is_empty(), "The trash is empty");
                let steps: Vec<Box<dyn Operation>> = s
                    .items
                    .iter()
                    .map(|item| {
                        Box::new(Delete {
                            entry: item.entry_ref(),
                        }) as Box<dyn Operation>
                    })
                    .collect();
                Ok(Outcome::Plan(Plan::with_steps("empty the trash", steps)))
            },
        )
        .plans();
    let t = trashes.clone();
    app.commands.register::<NoArgs, _>(
        ID,
        "trash.list",
        "List the trash",
        "Return the files in the trash: each one's name, where it came from, where it is, and when it was put there",
        move |_, _| {
            let listed: Vec<Listed> = t
                .state
                .lock()
                .items
                .iter()
                .map(|item| Listed {
                    name: item.entry.name.clone(),
                    original: item.trashed.original.clone(),
                    file: item.trashed.file.clone(),
                    deleted: item
                        .trashed
                        .deleted
                        .and_then(|d| d.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                })
                .collect();
            Ok(Outcome::Value(serde_json::to_value(listed)?))
        },
    )
        .reads();
}

// ------------------------------------------------------------------ plugin

/// The trash plugin.
#[derive(Default)]
pub struct TrashPlugin {
    trashes: Trashes,
}

impl TrashPlugin {
    /// One that reads only `trashes`, never the user's: what a test that
    /// keeps a trash of its own needs.
    pub fn with_trashes(trashes: Vec<PathBuf>) -> Self {
        TrashPlugin {
            trashes: Trashes {
                state: Arc::default(),
                only: Some(trashes.into()),
            },
        }
    }
}

impl Plugin for TrashPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Trash"
    }

    fn description(&self) -> &'static str {
        "The trash as a place: restore its files, delete them for good, empty it"
    }

    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Files
    }

    fn capabilities(&self) -> bibli_core::permissions::Capabilities {
        use bibli_core::permissions::Capability;
        bibli_core::permissions::Capabilities::of(&[Capability::Write])
    }
    fn register(&self, app: &App) {
        {
            let mut s = self.trashes.state.lock();
            if let Some(previous) = s.lifetime.replace(app.lifetime().child()) {
                previous.cancel();
            }
        }
        app.registry.places.add(ID, place(false));
        app.register_provider(
            ID,
            Arc::new(TrashProvider {
                trashes: self.trashes.clone(),
                app: app.weak(),
            }),
        );
        register_commands(app, &self.trashes);
        // What bibli puts in the trash, restores or deletes reads it again,
        // even before there is a trash to watch.
        let (t, weak) = (self.trashes.clone(), app.weak());
        app.executor.observe(
            ID,
            Arc::new(move |changes| {
                let touched = changes.iter().any(|c| {
                    matches!(
                        c,
                        Change::Trash { .. } | Change::Restore { .. } | Change::Delete { .. }
                    )
                });
                if touched && let Some(app) = weak.upgrade() {
                    t.refresh(&app);
                }
            }),
        );
        self.trashes.watch(app);
        self.trashes.refresh(app);
    }

    fn on_disable(&self, _app: &App) {
        let mut s = self.trashes.state.lock();
        if let Some(lifetime) = s.lifetime.take() {
            lifetime.cancel();
        }
        *s = State::default();
    }
}

#[cfg(test)]
mod tests;
