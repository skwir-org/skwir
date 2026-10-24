//! Locations that are no folders but read as places (ADR-032): their
//! places open where they lead, Up walks their trail back to the folder
//! they are of, Delete takes out what their verbs say, places sort by how
//! many files they hold, and one that gives no trail reads as one place.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::entry::{Entry, EntryId, EntryKind, Provenance, Value, keys};
use skwir_core::location::{
    Crumb, EmptyState, EventSink, Listing, ListingEvent, Location, LocationProvider, OpenContext,
    OpenLocation, SortKey, SortSpec, Trail, Verb, Verbs,
};
use skwir_core::scheduler::Priority;
use skwir_core::workspace::PaneId;

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
    /// The folder it lists, as a trashed folder's location does.
    holds: Option<PathBuf>,
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
    fn holds(&self) -> Option<&Path> {
        self.holds.as_deref()
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
        if location.rest == "/few" {
            sink.send(ListingEvent::Added(vec![Entry::new(
                EntryId {
                    file: skwir_core::FileId::new(7, 1),
                    at: 0,
                },
                "inside.txt",
                EntryKind::File,
            )]));
        }
        sink.send(ListingEvent::Complete);
        Ok(Arc::new(Open {
            location: location.clone(),
            rx,
            _sink: sink,
            holds: (location.rest == "/few").then(|| self.base.join("few")),
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
            toggles: Vec::new(),
            holds_places: true,
            one_folder: false,
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

fn shows(app: &skwir_core::App, pane: PaneId) -> Option<Location> {
    app.pane_location(pane)
}

fn go(app: &skwir_core::App, location: &str) {
    app.dispatch(&Invocation::with_args(
        "navigate",
        &serde_json::json!({ "location": location }),
    ))
    .expect("navigate");
    assert!(app.pump_until_complete(active(app), Duration::from_secs(10)));
}

fn with_fake(base: &Path) -> Arc<skwir_core::App> {
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
fn a_volume_let_go_of_takes_what_lists_a_folder_on_it_whatever_its_scheme() {
    let tmp = tempfile::tempdir().unwrap();
    let app = with_fake(tmp.path());
    let pane = active(&app);
    go(&app, "fake:/few");
    let home = Location::from_path(tmp.path().join("elsewhere"));
    // Another volume: nothing moves.
    app.release(Path::new("/nowhere"), &home).unwrap();
    assert_eq!(shows(&app, pane), Some(Location::new(SCHEME, "/few")));
    // Its own: the pane goes, as a folder's would.
    app.release(tmp.path(), &home).unwrap();
    assert_eq!(shows(&app, pane), Some(home));
}

/// A plugin turned off takes its locations from the panes: one showing
/// it goes home, and nothing of it is kept open.
#[test]
fn a_location_no_plugin_opens_any_more_is_let_go_of() {
    let tmp = tempfile::tempdir().unwrap();
    let app = with_fake(tmp.path());
    let pane = active(&app);
    go(&app, "fake:/few");
    let open = app
        .workspace
        .read()
        .pane(pane)
        .unwrap()
        .open
        .clone()
        .unwrap();
    app.unregister_owner("test");
    let home = skwir_core::os_path::home().unwrap();
    assert_eq!(app.pane_dir(pane).unwrap(), home);
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
    app.keep_locations(pane, &[]);
    assert_eq!(
        std::sync::Arc::strong_count(&open),
        1,
        "nothing of the plugin's is held"
    );
}

/// What an agent's call counts of a pane leaves out a file whose path the
/// pane cannot say, as it does a private one; a place is no file.
#[test]
fn an_agent_counts_no_file_whose_path_is_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    let app = with_fake(tmp.path());
    let pane = active(&app);
    let withheld = || {
        skwir_core::permissions::as_agent(skwir_core::permissions::Privacy::folders([]), || {
            app.withheld(pane)
        })
    };
    go(&app, "fake:/");
    assert!(withheld().is_empty(), "places are counted");
    go(&app, "fake:/few");
    let inside = {
        let ws = app.workspace.read();
        ws.pane(pane).unwrap().listing.ids().collect::<Vec<_>>()
    };
    assert_eq!(withheld().into_iter().collect::<Vec<_>>(), inside);
    assert!(app.withheld(pane).is_empty(), "the user counts everything");
}

/// A folder shown through a link into a volume (`~/Music` on a drive) is
/// let go of with the volume: the pane goes elsewhere, and nothing holds
/// the volume once its listing closes.
#[cfg(unix)]
#[test]
fn a_folder_shown_through_a_link_is_let_go_of_with_its_volume() {
    let tmp = home_tempdir();
    let volume = tmp.path().join("volume");
    let music = volume.join("l'été");
    std::fs::create_dir_all(&music).unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let link = home.join("Music");
    std::os::unix::fs::symlink(&music, &link).unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &link);
    assert!(
        !skwir_core::fs::wait_let_go(&volume, Duration::ZERO),
        "held through the link"
    );
    app.release(&volume, &Location::from_path(&home)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    assert_eq!(app.pane_dir(pane).unwrap(), home);
    assert!(skwir_core::fs::wait_let_go(&volume, Duration::from_secs(5)));
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
    let mut file = Entry::new(
        EntryId {
            file: skwir_core::FileId::new(1, 1),
            at: 0,
        },
        "a.txt",
        EntryKind::File,
    );
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

#[test]
fn a_place_opens_in_place_as_a_folder_does() {
    let tmp = tempfile::tempdir().unwrap();
    let app = with_fake(tmp.path());
    let pane = active(&app);
    go(&app, "fake:/");
    let few = EntryId::place(3);
    app.dispatch(&Invocation::with_args(
        "expand",
        &serde_json::json!({ "ids": [few], "mode": "toggle" }),
    ))
    .unwrap();
    let inside = EntryId {
        file: skwir_core::FileId::new(7, 1),
        at: 0,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let order = loop {
        app.pump();
        let order: Vec<EntryId> = {
            let ws = app.workspace.read();
            ws.pane(pane).unwrap().order().to_vec()
        };
        if order.contains(&inside) {
            break order;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the place opened in place"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        order,
        [few, inside, EntryId::place(4)],
        "its entries right under it, before the next place"
    );
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert!(p.is_open_at(few));
        assert_eq!(p.depth(inside), 1);
    }
    app.dispatch(&Invocation::with_args(
        "expand",
        &serde_json::json!({ "ids": [few], "mode": "toggle" }),
    ))
    .unwrap();
    let ws = app.workspace.read();
    assert!(!ws.pane(pane).unwrap().is_open_at(few), "and closes again");
}

/// `flat:` is a place that holds only files (the trash, the shelf): it
/// says its name, its glyph and what it says empty, and no trail.
struct Flat;

impl LocationProvider for Flat {
    fn scheme(&self) -> &'static str {
        "flat"
    }

    fn open(&self, _: &Location, _: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        anyhow::bail!("not opened here")
    }

    fn title(&self, _: &Location) -> Option<String> {
        Some("Pile".into())
    }

    fn icon(&self, _: &Location) -> Option<&'static str> {
        Some("shelf")
    }

    fn empty_state(&self, _: &Location) -> Option<EmptyState> {
        Some(EmptyState::new("The pile is empty", "Put files on it."))
    }
}

/// A location that is no folder reads as a trail whatever its provider
/// says: one place, named as its tab is, when it gives no trail of its
/// own, with nothing above it and no places inside to list.
#[test]
fn a_place_that_gives_no_trail_reads_as_one_place_by_its_name() {
    let tmp = tempfile::tempdir().unwrap();
    let app = with_fake(tmp.path());
    app.register_provider("test", Arc::new(Flat));
    let flat = Location::parse("flat:");
    let trail = app.trail(&flat).expect("a trail");
    assert_eq!(trail.crumbs, [Crumb::new("Pile", flat.clone())]);
    assert!(trail.base.is_none() && !trail.holds_places);
    assert_eq!(app.parent(&flat), None, "nothing above it");
    assert_eq!(app.location_icon(&flat), Some("shelf"));
    assert_eq!(
        app.empty_state(&flat).map(|e| e.title),
        Some("The pile is empty".to_owned())
    );
    // A provider's own trail is kept; a folder and a scheme nobody
    // provides have none.
    assert!(
        app.trail(&Location::new(SCHEME, "/few"))
            .unwrap()
            .holds_places
    );
    assert_eq!(app.trail(&Location::from_path(tmp.path())), None);
    assert_eq!(app.trail(&Location::parse("nobody:")), None);
    assert_eq!(app.location_icon(&Location::from_path(tmp.path())), None);
}

/// `here:` is an address reached at a folder (ADR-077): opening it says
/// so once (`ListingEvent::Redirect`), as a remote connected does.
struct Here {
    folder: PathBuf,
}

struct Reaching {
    location: Location,
    rx: Receiver<ListingEvent>,
}

impl LocationProvider for Here {
    fn scheme(&self) -> &'static str {
        "here"
    }

    fn open(&self, location: &Location, ctx: OpenContext) -> anyhow::Result<Arc<dyn OpenLocation>> {
        let (tx, rx) = ctx.channel();
        let folder = self.folder.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            tx.send(ListingEvent::Redirect(Location::from_path(folder)));
        });
        Ok(Arc::new(Reaching {
            location: location.clone(),
            rx,
        }))
    }
}

impl OpenLocation for Reaching {
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
    fn refresh(&self) {}
    fn close(&self) {}
}

/// An address reached at a folder shows that folder, in its place in the
/// pane's history: Back goes where the pane was before it, and a script
/// listing the address lists the folder (ADR-077).
#[test]
fn an_address_reached_at_a_folder_shows_the_folder_in_its_place() {
    let tmp = tempfile::tempdir().unwrap();
    let (before, folder) = (tmp.path().join("before"), tmp.path().join("there"));
    std::fs::create_dir_all(&before).unwrap();
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.txt"), "a").unwrap();
    let app = app(tmp.path());
    app.register_provider(
        "test",
        Arc::new(Here {
            folder: folder.clone(),
        }),
    );
    navigate(&app, &before);
    let pane = active(&app);
    app.navigate(pane, &Location::parse("here://server/there"))
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        app.pump();
        let shown = app.pane_location(pane);
        if shown.as_ref().and_then(Location::path).as_deref() == Some(folder.as_path()) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "shown: {shown:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert_eq!(
            p.history,
            [Location::from_path(&before), Location::from_path(&folder)],
            "the address's place in the history is the folder's"
        );
        assert_eq!(p.listing.len(), 1);
    }
    let listed = app
        .dispatch(&Invocation::with_args(
            "entries",
            &serde_json::json!({ "location": "here://server/there" }),
        ))
        .unwrap();
    let Outcome::Value(listed) = listed else {
        panic!("{listed:?}")
    };
    assert_eq!(listed["total"], 1, "{listed}");
}
