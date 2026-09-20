//! bibli plugin: shelf. Collect entries from any location into a tray, then
//! move, copy, or trash them all at once into wherever you are. The shelf is
//! also a virtual location (`shelf:`) so any view can show it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bibli_core::app::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{Entry, EntryId, EntryKind};
use bibli_core::fs::stat::stat_path;
use bibli_core::location::{
    EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
};
use bibli_core::op::{Copy, EntryRef, Move, Operation, Plan, Trash};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem, Place};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;

const ID: &str = "shelf";
/// The location scheme of the virtual shelf listing.
pub const SCHEME: &str = "shelf";

/// One shelved entry.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShelfItem {
    pub id: EntryId,
    pub path: PathBuf,
    pub name: String,
}

impl ShelfItem {
    fn entry_ref(&self) -> EntryRef {
        EntryRef::new(self.id, self.path.clone())
    }
}

#[derive(Default)]
struct State {
    items: Vec<ShelfItem>,
    /// Open `shelf:` listings to keep live.
    sinks: Vec<EventSink>,
}

impl State {
    fn broadcast(&mut self, event: &ListingEvent) {
        self.sinks.retain(|s| s.send(event.clone()));
    }
}

/// The shelf plugin.
#[derive(Default)]
pub struct ShelfPlugin {
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct PaneArgs {
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct AddArgs {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Entries to add; the pane's selection when omitted.
    #[serde(default)]
    ids: Vec<EntryId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RemoveArgs {
    ids: Vec<EntryId>,
}

fn pane_of(app: &App, pane: Option<PaneId>) -> PaneId {
    pane.unwrap_or_else(|| app.workspace.read().active_id())
}

fn pane_dir(app: &App, pane: PaneId) -> anyhow::Result<PathBuf> {
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    p.location()
        .and_then(Location::path)
        .ok_or_else(|| anyhow::anyhow!("pane is not showing a directory"))
}

fn entry_for(item: &ShelfItem) -> Entry {
    stat_path(&item.path)
        .unwrap_or_else(|| Entry::new(item.id, item.name.clone(), EntryKind::Unknown))
}

impl ShelfPlugin {
    /// Current items, in insertion order.
    pub fn items(&self) -> Vec<ShelfItem> {
        self.state.lock().items.clone()
    }

    fn add(state: &Mutex<State>, new: Vec<ShelfItem>) -> usize {
        let mut s = state.lock();
        let mut added = Vec::new();
        for item in new {
            if s.items.iter().any(|i| i.id == item.id) {
                continue;
            }
            added.push(entry_for(&item));
            s.items.push(item);
        }
        let n = added.len();
        if n > 0 {
            s.broadcast(&ListingEvent::Added(added));
        }
        n
    }

    fn remove(state: &Mutex<State>, ids: &[EntryId]) -> usize {
        let mut s = state.lock();
        let before = s.items.len();
        s.items.retain(|i| !ids.contains(&i.id));
        let removed = before - s.items.len();
        if removed > 0 {
            s.broadcast(&ListingEvent::Removed(ids.to_vec()));
        }
        removed
    }

    fn clear(state: &Mutex<State>) {
        let mut s = state.lock();
        let ids: Vec<EntryId> = s.items.iter().map(|i| i.id).collect();
        s.items.clear();
        if !ids.is_empty() {
            s.broadcast(&ListingEvent::Removed(ids));
        }
    }
}

fn selection_items(app: &App, pane: PaneId, ids: &[EntryId]) -> anyhow::Result<Vec<ShelfItem>> {
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    let open = p
        .open
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("pane has no open location"))?;
    let chosen: Vec<EntryId> = if ids.is_empty() {
        p.selection.iter().collect()
    } else {
        ids.to_vec()
    };
    if chosen.is_empty() {
        anyhow::bail!("nothing selected");
    }
    Ok(chosen
        .into_iter()
        .filter_map(|id| {
            let path = open.path_of(id)?;
            let name = p
                .listing
                .get(id)
                .map(|e| e.name_str().into_owned())
                .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))?;
            Some(ShelfItem { id, path, name })
        })
        .collect())
}

fn plan_into(
    items: &[ShelfItem],
    dest: &std::path::Path,
    copy: bool,
    app: &App,
) -> anyhow::Result<Outcome> {
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
                })
            }
        })
        .collect();
    if steps.is_empty() {
        anyhow::bail!("nothing on the shelf to bring here");
    }
    let verb = if copy { "copy" } else { "move" };
    Ok(Outcome::Plan(Plan::with_steps(
        format!("{verb} {} shelved items to {}", steps.len(), dest.display()),
        steps,
    )))
}

// ------------------------------------------------------------------ location

struct ShelfLocation {
    location: Location,
    rx: Receiver<ListingEvent>,
    ctx: OpenContext,
    state: Arc<Mutex<State>>,
    sink: EventSink,
}

impl ShelfLocation {
    fn list(&self) {
        let items = self.state.lock().items.clone();
        let entries: Vec<Entry> = items.iter().map(entry_for).collect();
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
        let items: Vec<ShelfItem> = {
            let s = self.state.lock();
            s.items
                .iter()
                .filter(|i| ids.contains(&i.id))
                .cloned()
                .collect()
        };
        if items.is_empty() {
            return;
        }
        let sink = self.sink.clone();
        let metadata = Arc::clone(&self.ctx.metadata);
        self.ctx
            .scheduler
            .spawn(priority, "shelf.stat", self.ctx.cancel.child(), move || {
                // Group by parent so metadata providers get a directory.
                let mut by_dir: HashMap<PathBuf, Vec<Entry>> = HashMap::new();
                let mut gone = Vec::new();
                for item in &items {
                    match stat_path(&item.path) {
                        Some(mut e) => {
                            e.id = item.id;
                            let dir = item.path.parent().map(PathBuf::from).unwrap_or_default();
                            by_dir.entry(dir).or_default().push(e);
                        }
                        None => gone.push(item.id),
                    }
                }
                let mut updated = Vec::new();
                for (dir, mut entries) in by_dir {
                    metadata.fill(&dir, &mut entries);
                    updated.extend(entries);
                }
                if !updated.is_empty() {
                    sink.send(ListingEvent::Updated(updated));
                }
                if !gone.is_empty() {
                    sink.send(ListingEvent::Removed(gone));
                }
            });
    }

    fn path_of(&self, id: EntryId) -> Option<PathBuf> {
        self.state
            .lock()
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.path.clone())
    }

    fn refresh(&self) {
        self.list();
    }

    fn close(&self) {
        self.ctx.cancel.cancel();
    }
}

struct ShelfProvider {
    state: Arc<Mutex<State>>,
}

impl LocationProvider for ShelfProvider {
    fn scheme(&self) -> &str {
        SCHEME
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

const MENU: &[(&str, &str, Option<&str>, i32, &str)] =
    &[("Add to Shelf", "shelf.add", Some("selection"), 140, "shelf")];

impl Plugin for ShelfPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Shelf"
    }

    fn description(&self) -> &'static str {
        "Collect entries from anywhere and move, copy, or trash them together"
    }

    fn register(&self, app: &App) {
        app.register_provider(
            ID,
            Arc::new(ShelfProvider {
                state: Arc::clone(&self.state),
            }),
        );

        let st = Arc::clone(&self.state);
        app.commands.register::<AddArgs, _>(
            ID,
            "shelf.add",
            "Add to shelf",
            "Put the selection (or the given entries) on the shelf",
            move |a, app| {
                let items = selection_items(app, pane_of(app, a.pane), &a.ids)?;
                let n = ShelfPlugin::add(&st, items);
                Ok(Outcome::Value(serde_json::json!({ "added": n })))
            },
        );
        let st = Arc::clone(&self.state);
        app.commands.register::<RemoveArgs, _>(
            ID,
            "shelf.remove",
            "Remove from shelf",
            "Take entries off the shelf",
            move |a, _| {
                let n = ShelfPlugin::remove(&st, &a.ids);
                Ok(Outcome::Value(serde_json::json!({ "removed": n })))
            },
        );
        let st = Arc::clone(&self.state);
        app.commands.register::<PaneArgs, _>(
            ID,
            "shelf.clear",
            "Clear shelf",
            "Empty the shelf",
            move |_, _| {
                ShelfPlugin::clear(&st);
                Ok(Outcome::Done)
            },
        );
        let st = Arc::clone(&self.state);
        app.commands.register::<PaneArgs, _>(
            ID,
            "shelf.list",
            "List shelf",
            "Return the shelved entries",
            move |_, _| {
                let items = st.lock().items.clone();
                Ok(Outcome::Value(serde_json::to_value(items)?))
            },
        );
        let st = Arc::clone(&self.state);
        app.commands.register::<PaneArgs, _>(
            ID,
            "shelf.move_here",
            "Move shelf here",
            "Move every shelved entry into the current directory (plan)",
            move |a, app| {
                let dest = pane_dir(app, pane_of(app, a.pane))?;
                plan_into(&st.lock().items, &dest, false, app)
            },
        );
        let st = Arc::clone(&self.state);
        app.commands.register::<PaneArgs, _>(
            ID,
            "shelf.copy_here",
            "Copy shelf here",
            "Copy every shelved entry into the current directory (plan)",
            move |a, app| {
                let dest = pane_dir(app, pane_of(app, a.pane))?;
                plan_into(&st.lock().items, &dest, true, app)
            },
        );
        let st = Arc::clone(&self.state);
        app.commands.register::<PaneArgs, _>(
            ID,
            "shelf.trash",
            "Trash shelf",
            "Move every shelved entry to the trash (plan)",
            move |_, _| {
                let items = st.lock().items.clone();
                if items.is_empty() {
                    anyhow::bail!("the shelf is empty");
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
                    format!("trash {} shelved items", steps.len()),
                    steps,
                )))
            },
        );

        app.registry.places.add(
            ID,
            Place {
                title: "Shelf".into(),
                location: Location::parse("shelf:"),
                icon: Some("folder-drag-accept".into()),
                group: "places".into(),
            },
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
        for (title, command, when, order, icon) in MENU {
            app.registry.menus.add(
                ID,
                MenuItem {
                    menu: "context".into(),
                    title: (*title).into(),
                    command: Invocation::new(*command),
                    when: when.map(str::to_owned),
                    order: *order,
                    icon: Some((*icon).into()),
                    submenu: None,
                },
            );
        }
    }

    fn on_disable(&self, _app: &App) {
        let mut s = self.state.lock();
        s.items.clear();
        s.sinks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use std::time::Duration;

    fn setup() -> (tempfile::TempDir, Arc<App>, PaneId, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
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

    #[test]
    fn move_here_plan_moves_files_and_skips_items_already_there() {
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
        // The shelf keeps its items; copy_here of the (moved) items into dest is a no-op set.
        let Outcome::Plan(copy) = app.dispatch(&Invocation::new("shelf.copy_here")).unwrap() else {
            panic!()
        };
        assert_eq!(copy.len(), 2, "copy keeps every item");
        assert!(!copy.is_applicable(), "copying onto itself conflicts");
    }

    #[test]
    fn shelf_location_streams_and_updates_live() {
        let (_tmp, app, pane, _src) = setup();
        select(&app, pane, &["a.txt"]);
        app.dispatch(&Invocation::new("shelf.add")).unwrap();
        let open = app.open_location(&Location::parse("shelf:")).unwrap();
        let mut listing = bibli_core::Listing::default();
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

    #[test]
    fn disable_withdraws_everything() {
        let (_tmp, app, _pane, _src) = setup();
        assert!(app.commands.get("shelf.add").is_some());
        assert!(app.provider("shelf").is_some());
        app.plugins.set_enabled("shelf", false, &app);
        assert!(app.commands.get("shelf.add").is_none());
        assert!(app.provider("shelf").is_none());
        assert!(app.registry.places.all().iter().all(|p| p.title != "Shelf"));
        assert!(
            app.registry
                .keymap
                .all()
                .iter()
                .all(|k| !k.command.name.starts_with("shelf."))
        );
    }
}
