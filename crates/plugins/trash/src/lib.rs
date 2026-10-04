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
    Crumb, EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation, Trail,
    Verbs,
};
use bibli_core::op::{Change, Delete, EntryRef, Operation, Plan, Restore, Trash, trash_info_of};
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
    watcher: Option<notify::RecommendedWatcher>,
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
            use notify::Watcher as _;
            for info in trashes.iter().map(|t| t.join("info")) {
                if !s.watched.contains(&info)
                    && info.is_dir()
                    && watcher
                        .watch(&info, notify::RecursiveMode::NonRecursive)
                        .is_ok()
                {
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
        let (tx, rx) = crossbeam_channel::unbounded::<()>();
        // Reading a `.trashinfo` is no change to the trash.
        let handler = move |event: notify::Result<notify::Event>| {
            if event.is_ok_and(|event| !matches!(event.kind, notify::EventKind::Access(_))) {
                let _ = tx.send(());
            }
        };
        let Ok(watcher) = notify::recommended_watcher(handler) else {
            return;
        };
        let (this, weak) = (self.clone(), app.weak());
        let settle = std::thread::Builder::new()
            .name("bibli-trash".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    while rx.recv_timeout(SETTLE).is_ok() {}
                    let Some(app) = weak.upgrade() else { return };
                    this.refresh(&app);
                }
            });
        if settle.is_ok() {
            let mut s = self.state.lock();
            s.watcher = Some(watcher);
            s.watched.clear();
        }
    }

    /// The trash's items that are `ids`.
    fn items(&self, ids: &[EntryId]) -> Vec<Item> {
        self.state
            .lock()
            .items
            .iter()
            .filter(|i| ids.contains(&i.entry.id))
            .cloned()
            .collect()
    }
}

/// The trash's place in the sidebar, among the places of the home folder:
/// files dropped on it are put in the trash, and its menu empties it.
fn place(full: bool) -> Place {
    let menu = if full {
        vec![MenuItem {
            menu: "place".into(),
            title: "Empty Trash…".into(),
            command: Invocation::new("trash.empty"),
            when: None,
            order: 300,
            icon: Some("trash".into()),
            submenu: None,
        }]
    } else {
        Vec::new()
    };
    Place {
        title: "Trash".into(),
        location: Location::new(SCHEME, ""),
        icon: Some(
            if full {
                "user-trash-full"
            } else {
                "user-trash"
            }
            .into(),
        ),
        group: "places".into(),
        space: None,
        eject: None,
        activate: None,
        unavailable: None,
        busy: None,
        menu,
        forget: None,
        reorder: None,
        color: None,
        drop: Some(Invocation::new("trash.put")),
        suggested: false,
        key: None,
        hidden: false,
        rename: None,
    }
}

// ------------------------------------------------------------------ location

struct TrashProvider {
    trashes: Trashes,
    app: Weak<App>,
}

impl LocationProvider for TrashProvider {
    fn scheme(&self) -> &'static str {
        SCHEME
    }

    fn title(&self, _location: &Location) -> Option<String> {
        Some("Trash".into())
    }

    fn trail(&self, location: &Location) -> Option<Trail> {
        Some(Trail {
            crumbs: vec![Crumb::new("Trash", location.clone())],
            ..Trail::default()
        })
    }

    /// Files dropped or pasted here are put in the trash; its own are
    /// restored (`Verbs::restore`), and deleted for good by Delete.
    fn verbs(&self, _location: &Location) -> Verbs {
        Verbs {
            drop: Some(Invocation::new("trash.put")),
            restore: Some(Invocation::new("trash.restore")),
            ..Verbs::default()
        }
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
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
            .items
            .iter()
            .find(|i| i.entry.id == id)
            .map(|i| i.trashed.file.clone())
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
    /// Files by where they are (another application's, dropped), instead
    /// of a pane's selection.
    #[serde(default)]
    entries: Vec<EntryRef>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct RestoreArgs {
    /// The pane showing the trash, whose selection is restored.
    #[serde(default)]
    pane: Option<PaneId>,
    /// The trash's entries to restore, instead of the pane's selection.
    #[serde(default)]
    ids: Vec<EntryId>,
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

/// The selection of `pane`, by id.
fn selected(app: &App, pane: Option<PaneId>) -> Vec<EntryId> {
    let ws = app.workspace.read();
    ws.pane(app.pane_or_active(pane))
        .map(|p| p.selection.iter().collect())
        .unwrap_or_default()
}

fn register_commands(app: &App, trashes: &Trashes) {
    app.commands.register::<PutArgs, _>(
        ID,
        "trash.put",
        "Put in the trash",
        "Move files to the trash (plan): a pane's selection, or the entries given (files dropped on the Trash)",
        |a, app| {
            let refs = if a.entries.is_empty() {
                app.selected_refs(app.pane_or_active(a.pane))
            } else {
                a.entries
            };
            let steps: Vec<Box<dyn Operation>> = refs
                .into_iter()
                .filter(|r| trash_info_of(&r.path).is_none())
                .map(|entry| Box::new(Trash { entry }) as Box<dyn Operation>)
                .collect();
            anyhow::ensure!(!steps.is_empty(), "These are in the trash already");
            Ok(Outcome::Plan(Plan::with_steps("move to trash", steps)))
        },
    );
    let t = trashes.clone();
    app.commands.register::<RestoreArgs, _>(
        ID,
        "trash.restore",
        "Restore from the trash",
        "Put files of the trash back where they came from (plan): the trash pane's selection, or the entries given",
        move |a, app| {
            let ids = if a.ids.is_empty() {
                selected(app, a.pane)
            } else {
                a.ids
            };
            let steps: Vec<Box<dyn Operation>> = t
                .items(&ids)
                .into_iter()
                .map(|item| {
                    Box::new(Restore {
                        path: item.trashed.original,
                        item: Some(PathArg(item.trashed.info)),
                    }) as Box<dyn Operation>
                })
                .collect();
            if steps.is_empty() {
                return Err(bibli_core::command::NothingSelected.into());
            }
            Ok(Outcome::Plan(Plan::with_steps("restore from the trash", steps)))
        },
    );
    let t = trashes.clone();
    app.commands.register::<NoArgs, _>(
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
    );
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
    );
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
