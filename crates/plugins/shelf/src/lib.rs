//! bibli plugin: shelf. Collect entries from any location into a tray, then
//! move, copy, or trash them all at once into wherever you are. The shelf is
//! also a virtual location (`shelf:`) so any view can show it.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use bibli_core::app::{App, PaneArg};
use bibli_core::command::{Invocation, NoArgs, Outcome};
use bibli_core::entry::{Entry, EntryId, EntryKind};
use bibli_core::fs::hydrate_paths;
use bibli_core::fs::stat::stat_path;
use bibli_core::location::{
    EventSink, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation, Verb, Verbs,
};
use bibli_core::op::{Change, Copy, EntryRef, Followed, Move, Operation, Plan, Trash};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem, Place};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;

const ID: &str = "shelf";
/// The location scheme of the virtual shelf listing.
pub const SCHEME: &str = "shelf";
/// The count the shelf publishes in `App::tallies`.
pub const TALLY: &str = "shelf.items";

/// One shelved entry.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShelfItem {
    pub id: EntryId,
    #[serde(with = "bibli_core::os_path")]
    pub path: PathBuf,
    #[serde(with = "bibli_core::os_path")]
    pub name: OsString,
    /// As the listing it came from knew it; a stat refines it.
    #[serde(skip)]
    pub kind: EntryKind,
}

impl ShelfItem {
    fn entry_ref(&self) -> EntryRef {
        EntryRef::new(self.id, self.path.clone())
    }

    fn entry(&self) -> Entry {
        Entry::new(self.id, self.name.clone(), self.kind)
    }
}

#[derive(Default)]
struct State {
    items: Vec<ShelfItem>,
    /// Open `shelf:` listings to keep live.
    sinks: Vec<EventSink>,
    /// Where the item count is published; empty while disabled.
    app: Weak<App>,
}

impl State {
    fn broadcast(&mut self, event: &ListingEvent) {
        self.sinks.retain(|s| s.send(event.clone()));
    }

    fn publish_count(&self) {
        if let Some(app) = self.app.upgrade() {
            app.tallies.set(ID, TALLY, self.items.len() as u64);
            app.wake();
        }
    }

    /// Drop `ids` from the shelf and from every open listing of it.
    fn forget(&mut self, ids: &[EntryId]) -> usize {
        let mut removed = Vec::new();
        self.items.retain(|i| {
            let gone = ids.contains(&i.id);
            if gone {
                removed.push(i.id);
            }
            !gone
        });
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
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct AddArgs {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Entries to add; the pane's selection when omitted.
    #[serde(default)]
    ids: Vec<EntryId>,
    /// Files to add by where they are (another application's, dropped,
    /// or the clipboard's, pasted), instead of a pane's.
    #[serde(default)]
    entries: Vec<EntryRef>,
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
        self.state.lock().items.clone()
    }

    /// Shelve `new` with the entries their listing already holds, so
    /// nothing is stat-ed here: this runs on the dispatching thread.
    fn add(state: &Mutex<State>, new: Vec<(ShelfItem, Option<Entry>)>) -> usize {
        let mut s = state.lock();
        let mut added = Vec::new();
        for (item, entry) in new {
            if s.items.iter().any(|i| i.id == item.id) {
                continue;
            }
            added.push(entry.unwrap_or_else(|| item.entry()));
            s.items.push(item);
        }
        let n = added.len();
        if n > 0 {
            s.broadcast(&ListingEvent::Added(added));
            s.publish_count();
        }
        n
    }

    fn remove(state: &Mutex<State>, ids: &[EntryId]) -> usize {
        state.lock().forget(ids)
    }

    fn clear(state: &Mutex<State>) {
        let mut s = state.lock();
        let ids: Vec<EntryId> = s.items.iter().map(|i| i.id).collect();
        s.forget(&ids);
    }
}

/// Follow shelved paths through applied changes: a trashed or deleted item
/// leaves the shelf, a moved or renamed one follows at once (no I/O), and
/// a worker then asks whether it kept its identity: a move to another
/// device gives it a new inode.
fn follow(state: &Arc<Mutex<State>>, changes: &[Change], app: &App) {
    let mut s = state.lock();
    let mut gone = Vec::new();
    let mut renamed = Vec::new();
    let mut moved = Vec::new();
    for item in &mut s.items {
        let mut path = item.path.clone();
        for change in changes {
            match change.follow(&path) {
                Followed::Same => {}
                Followed::Moved(to) => path = to,
                Followed::Gone => {
                    gone.push(item.id);
                    break;
                }
            }
        }
        if path != item.path && !gone.contains(&item.id) {
            if let Some(name) = path.file_name().filter(|n| *n != item.name) {
                item.name = name.to_owned();
                renamed.push(item.entry());
            }
            item.path.clone_from(&path);
            moved.push((item.id, path));
        }
    }
    s.forget(&gone);
    if !renamed.is_empty() {
        s.broadcast(&ListingEvent::Added(renamed));
    }
    drop(s);
    if moved.is_empty() {
        return;
    }
    let state = Arc::clone(state);
    app.scheduler.spawn(
        Priority::Normal,
        "shelf.follow",
        app.lifetime().child(),
        move || {
            let found: Vec<(EntryId, PathBuf, Option<Entry>)> = moved
                .into_iter()
                .map(|(id, path)| {
                    let entry = stat_path(&path);
                    (id, path, entry)
                })
                .collect();
            let mut s = state.lock();
            let mut removed = Vec::new();
            let mut added = Vec::new();
            let mut lost = Vec::new();
            for (old, path, entry) in found {
                // Only items still on the shelf, still at that path.
                if !s.items.iter().any(|i| i.id == old && i.path == path) {
                    continue;
                }
                let Some(entry) = entry.filter(|e| e.id != old) else {
                    continue;
                };
                if s.items.iter().any(|i| i.id == entry.id) {
                    // Now the same file as another item: keep that one.
                    lost.push(old);
                    continue;
                }
                let item = s
                    .items
                    .iter_mut()
                    .find(|i| i.id == old)
                    .expect("checked above");
                item.id = entry.id;
                item.kind = entry.kind;
                removed.push(old);
                added.push(entry);
            }
            if !removed.is_empty() {
                s.broadcast(&ListingEvent::Removed(removed));
            }
            if !added.is_empty() {
                s.broadcast(&ListingEvent::Added(added));
            }
            s.forget(&lost);
        },
    );
}

fn selection_items(
    app: &App,
    pane: PaneId,
    ids: &[EntryId],
) -> anyhow::Result<Vec<(ShelfItem, Option<Entry>)>> {
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    anyhow::ensure!(p.open.is_some(), "pane has no open location");
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
            id: entry.id,
            path: entry.path.clone(),
            name,
            kind: EntryKind::Unknown,
        },
        None,
    ))
}

fn plan_into(items: &[ShelfItem], dest: &Path, copy: bool, app: &App) -> anyhow::Result<Outcome> {
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
    /// What the shelf holds, as its listings last knew it; hydration stats
    /// them on a worker.
    fn list(&self) {
        let entries: Vec<Entry> = self
            .state
            .lock()
            .items
            .iter()
            .map(ShelfItem::entry)
            .collect();
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
        let wanted: HashSet<EntryId> = ids.iter().copied().collect();
        let items: Vec<(EntryId, PathBuf)> = self
            .state
            .lock()
            .items
            .iter()
            .filter(|i| wanted.contains(&i.id))
            .map(|i| (i.id, i.path.clone()))
            .collect();
        if items.is_empty() {
            return;
        }
        let sink = self.sink.clone();
        let metadata = Arc::clone(&self.ctx.metadata);
        let state = Arc::clone(&self.state);
        let cancel = self.ctx.cancel.child();
        self.ctx
            .scheduler
            .spawn(priority, "shelf.stat", cancel.clone(), move || {
                let gone = hydrate_paths(&items, &metadata, &sink, &cancel);
                if gone.is_empty() {
                    return;
                }
                // Gone from where it was looked for, unless a change moved
                // it meanwhile: then the next hydration looks there.
                let mut s = state.lock();
                let vanished: Vec<EntryId> = gone
                    .into_iter()
                    .filter(|id| {
                        let asked = items.iter().find(|(i, _)| i == id).map(|(_, p)| p);
                        s.items
                            .iter()
                            .any(|i| i.id == *id && Some(&i.path) == asked)
                    })
                    .collect();
                s.forget(&vanished);
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

    /// The shelf holds files kept where they are: Delete takes them off
    /// it, and files dropped or pasted on it are shelved (ADR-032).
    fn verbs(&self, _location: &Location) -> Verbs {
        Verbs {
            drop: Some(Invocation::new("shelf.add")),
            remove: Some(Verb::new(
                "Remove from Shelf",
                Invocation::new("shelf.remove"),
            )),
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

const MENU: &[(&str, &str, Option<&str>, i32, &str)] = &[(
    "Add to Shelf",
    "shelf.add",
    Some("selection !place"),
    140,
    "shelf",
)];

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

    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Files
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
        app.commands.register::<AddArgs, _>(
            ID,
            "shelf.add",
            "Add to shelf",
            "Put the selection (or the given entries) on the shelf",
            move |a, app| {
                let items = if a.entries.is_empty() {
                    selection_items(app, app.pane_or_active(a.pane), &a.ids)?
                } else {
                    a.entries.iter().filter_map(dropped_item).collect()
                };
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
            move |a, app| {
                let ids = if a.ids.is_empty() {
                    let ws = app.workspace.read();
                    let pane = ws
                        .pane(app.pane_or_active(a.pane))
                        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                    pane.selection.iter().collect()
                } else {
                    a.ids
                };
                let n = ShelfPlugin::remove(&st, &ids);
                Ok(Outcome::Value(serde_json::json!({ "removed": n })))
            },
        );
        let st = Arc::clone(&self.state);
        app.commands.register::<NoArgs, _>(
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
        app.commands.register::<NoArgs, _>(
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
        app.commands.register::<PaneArg, _>(
            ID,
            "shelf.move_here",
            "Move shelf here",
            "Move every shelved entry into the current directory (plan)",
            move |a, app| {
                let dest = app.pane_dir(app.pane_or_active(a.pane))?;
                plan_into(&st.lock().items, &dest, false, app)
            },
        );
        let st = Arc::clone(&self.state);
        app.commands.register::<PaneArg, _>(
            ID,
            "shelf.copy_here",
            "Copy shelf here",
            "Copy every shelved entry into the current directory (plan)",
            move |a, app| {
                let dest = app.pane_dir(app.pane_or_active(a.pane))?;
                plan_into(&st.lock().items, &dest, true, app)
            },
        );
        let st = Arc::clone(&self.state);
        app.commands.register::<NoArgs, _>(
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
                space: None,
                eject: None,
                activate: None,
                unavailable: None,
                busy: None,
                menu: Vec::new(),
                forget: None,
                reorder: None,
                color: None,
                drop: None,
                suggested: false,
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
        s.app = Weak::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use std::time::Duration;

    fn setup() -> (tempfile::TempDir, Arc<App>, PaneId, PathBuf) {
        // Trashed in a test: on the home filesystem, like the user's files.
        let base = std::env::home_dir().unwrap().join(".cache/bibli-tests");
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
        assert!(!copy.is_applicable(), "copying onto itself conflicts");

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
        let mut listing = bibli_core::Listing::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !listing.is_complete() {
            listing.apply(open.events().recv_deadline(deadline).unwrap());
        }
        let stranger = EntryId { dev: 0, ino: 1 };
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
