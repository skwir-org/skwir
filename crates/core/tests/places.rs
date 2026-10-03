//! Locations that are no folders but read as places (ADR-032): their
//! places open where they lead, Up walks their trail back to the folder
//! they are of, Delete takes out what their verbs say, and places sort
//! by how many files they hold.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{Entry, EntryId, EntryKind, Provenance, Value, keys};
use bibli_core::location::{
    Crumb, EventSink, Listing, ListingEvent, Location, LocationProvider, OpenContext, OpenLocation,
    SortKey, SortSpec, Trail, Verb, Verbs,
};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use common::*;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;

const SCHEME: &str = "fake";

/// `fake:/` holds two places, `few` and `many`, and `fake:/few` none.
struct Fake {
    base: PathBuf,
}

fn place(name: &str, files: u64) -> Entry {
    let mut e = Entry::new(
        EntryId::place(name.len() as u64),
        name,
        EntryKind::Directory,
    );
    e.metadata
        .set(keys::FILES, Value::UInt(files), Provenance::Filesystem);
    e
}

struct Open {
    location: Location,
    rx: Receiver<ListingEvent>,
    _sink: EventSink,
}

impl OpenLocation for Open {
    fn location(&self) -> &Location {
        &self.location
    }
    fn events(&self) -> &Receiver<ListingEvent> {
        &self.rx
    }
    fn hydrate(&self, _: &[EntryId], _: Priority) {}
    fn path_of(&self, _: EntryId) -> Option<PathBuf> {
        None
    }
    fn location_of(&self, id: EntryId) -> Option<Location> {
        ["few", "many"]
            .into_iter()
            .find(|name| id == EntryId::place(name.len() as u64))
            .map(|name| Location::new(SCHEME, format!("/{name}")))
    }
    fn refresh(&self) {}
    fn close(&self) {}
}

impl LocationProvider for Fake {
    fn scheme(&self) -> &str {
        SCHEME
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let (sink, rx) = ctx.channel();
        if location.rest == "/" {
            sink.send(ListingEvent::Added(vec![place("few", 2), place("many", 9)]));
        }
        sink.send(ListingEvent::Complete);
        Ok(Arc::new(Open {
            location: location.clone(),
            rx,
            _sink: sink,
        }))
    }

    fn trail(&self, location: &Location) -> Option<Trail> {
        let mut crumbs = vec![Crumb::new("Fake", Location::new(SCHEME, "/"))];
        if let Some(name) = location.rest.strip_prefix('/').filter(|n| !n.is_empty()) {
            crumbs.push(Crumb::new(name, location.clone()));
        }
        Some(Trail {
            base: Some(self.base.clone()),
            crumbs,
            filters: Vec::new(),
        })
    }

    fn verbs(&self, _: &Location) -> Verbs {
        Verbs {
            remove: Some(Verb::new(
                "Remove from Fake",
                Invocation::with_args("fake.out", &serde_json::json!({ "why": "test" })),
            )),
            ..Verbs::default()
        }
    }
}

fn shows(app: &bibli_core::App, pane: PaneId) -> Option<Location> {
    app.pane_location(pane)
}

fn go(app: &bibli_core::App, location: &str) {
    app.dispatch(&Invocation::with_args(
        "navigate",
        &serde_json::json!({ "location": location }),
    ))
    .expect("navigate");
    assert!(app.pump_until_complete(active(app), Duration::from_secs(10)));
}

fn with_fake(base: &Path) -> Arc<bibli_core::App> {
    let app = app(base);
    app.register_provider(
        "test",
        Arc::new(Fake {
            base: base.to_path_buf(),
        }),
    );
    app
}

#[test]
fn a_place_leads_somewhere_and_up_walks_its_trail_back_to_its_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let app = with_fake(tmp.path());
    let pane = active(&app);
    go(&app, "fake:/");
    let (place, leads) = {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        let id = EntryId::place(3);
        assert!(p.entry(id).is_some_and(Entry::is_place));
        assert!(p.path_of(id).is_none(), "a place has no path");
        (id, p.location_of(id))
    };
    assert!(place.is_place());
    assert_eq!(leads, Some(Location::new(SCHEME, "/few")));

    go(&app, "fake:/few");
    let up = Invocation::new("up");
    app.dispatch(&up).unwrap();
    assert_eq!(shows(&app, pane), Some(Location::new(SCHEME, "/")));
    app.dispatch(&up).unwrap();
    assert_eq!(
        shows(&app, pane),
        Some(Location::from_path(tmp.path())),
        "the top of the trail goes up to the folder it is of"
    );
    app.dispatch(&up).unwrap();
    assert_eq!(
        shows(&app, pane),
        tmp.path().parent().map(Location::from_path),
        "a folder goes up to its parent"
    );
}

#[test]
fn delete_takes_out_what_the_location_says_and_nothing_where_it_says_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let app = with_fake(tmp.path());
    let asked: Arc<Mutex<Option<serde_json::Value>>> = Arc::default();
    {
        let asked = Arc::clone(&asked);
        app.commands.register::<serde_json::Value, _>(
            "test",
            "fake.out",
            "Out",
            "",
            move |a, _| {
                *asked.lock() = Some(a);
                Ok(Outcome::Done)
            },
        );
    }
    go(&app, "fake:/");
    let pane = active(&app);
    app.dispatch(&Invocation::new("take_out")).unwrap();
    let args = asked.lock().take().expect("the location's command ran");
    assert_eq!(args["why"], "test", "with the arguments the verb gave");
    assert_eq!(args["pane"], serde_json::json!(pane), "and the pane");

    navigate(&app, tmp.path());
    assert!(
        app.dispatch(&Invocation::new("take_out")).is_err(),
        "a folder takes nothing out: Move to Trash deletes"
    );
    assert!(asked.lock().is_none());
}

#[test]
fn places_sort_first_and_by_how_many_files_they_hold() {
    let mut listing = Listing::default();
    let mut file = Entry::new(EntryId { dev: 1, ino: 1 }, "a.txt", EntryKind::File);
    file.metadata
        .set(keys::SIZE, Value::UInt(1_000_000), Provenance::Filesystem);
    listing.apply(ListingEvent::Added(vec![
        place("many", 9),
        file,
        place("few", 2),
    ]));
    let by_size = SortSpec {
        key: SortKey::Size,
        ..SortSpec::default()
    };
    let names: Vec<String> = listing
        .sorted(by_size)
        .iter()
        .map(|id| listing.get(*id).unwrap().name_str().into_owned())
        .collect();
    assert_eq!(names, ["few", "many", "a.txt"]);
    assert!(
        listing
            .get(EntryId::place(3))
            .unwrap()
            .metadata
            .is_hydrated(),
        "a place counted is known, never asked to be stat-ed"
    );
}
