//! Tags browsed as folders (ADR-032): a `tags:` location lists the tags
//! inside its tag as places and its own files, Up walks its trail back to
//! the folder it is of, what narrows it stays while it is walked, files
//! dropped on a tag are given it, and Delete takes it off them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bibli_core::app::{App, AppConfig};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::keys;
use bibli_core::location::Location;
use bibli_core::op::EntryRef;
use bibli_core::workspace::PaneId;
use bibli_plugin_tags::location::Address;
use bibli_plugin_tags::{read_tags, write_tags};

/// A folder where extended attributes work: not `/tmp`, often a tmpfs.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("tags-location-")
        .tempdir_in(base)
        .unwrap()
}

fn tags(t: &[&str]) -> Vec<String> {
    t.iter().map(|&s| s.to_owned()).collect()
}

/// Wait for `f` to answer, pumping the app.
fn until<T>(app: &App, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.pump();
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "waited for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct Setup {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    app: Arc<App>,
    pane: PaneId,
}

fn setup() -> Setup {
    let tmp = home_tempdir();
    let root = tmp.path().join("it's photos");
    std::fs::create_dir_all(root.join("trips/2024")).unwrap();
    let files = [
        ("paris.jpg", &["Places/France/Paris", "family"][..]),
        ("trips/lyon.jpg", &["Places/France/Lyon"][..]),
        ("trips/2024/france.jpg", &["Places/France"][..]),
        ("tokyo.jpg", &["Places/Japan", "family"][..]),
        ("notes.txt", &["work"][..]),
        ("untagged.jpg", &[][..]),
    ];
    for (name, t) in files {
        std::fs::write(root.join(name), b"x").unwrap();
        if !t.is_empty() {
            write_tags(&root.join(name), &tags(t)).unwrap();
        }
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(bibli_plugin_tags::TagsPlugin::default()), &app);
    app.plugins.add(
        Arc::new(bibli_plugin_ops_basic::OpsBasicPlugin::default()),
        &app,
    );
    let pane = app.workspace.read().active_id();
    Setup {
        _tmp: tmp,
        root,
        app,
        pane,
    }
}

/// The tag location of `tag` in `root`'s tags.
fn tag_location(root: &Path, tag: Option<&str>) -> Location {
    Address {
        tag: tag.map(str::to_owned),
        scope: Some(root.to_path_buf()),
        ..Address::default()
    }
    .location()
}

/// Show `location` and wait for its listing.
fn go(s: &Setup, location: &Location) {
    s.app.navigate(s.pane, location).unwrap();
    assert!(
        s.app.pump_until_complete(s.pane, Duration::from_secs(10)),
        "{location} listed"
    );
}

/// The places listed, with how many files each holds, and the files.
fn listed(s: &Setup) -> (BTreeSet<(String, u64)>, BTreeSet<String>) {
    let ws = s.app.workspace.read();
    let p = ws.pane(s.pane).unwrap();
    let mut places = BTreeSet::new();
    let mut files = BTreeSet::new();
    for e in p.listing.iter() {
        if e.is_place() {
            let n = e
                .metadata
                .value(keys::FILES)
                .and_then(bibli_core::Value::as_u64)
                .unwrap_or_default();
            places.insert((e.name_str().into_owned(), n));
        } else {
            files.insert(e.name_str().into_owned());
        }
    }
    (places, files)
}

fn set(items: &[(&str, u64)]) -> BTreeSet<(String, u64)> {
    items.iter().map(|(n, c)| ((*n).to_owned(), *c)).collect()
}

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|n| (*n).to_owned()).collect()
}

/// The place named `name`, opened.
fn enter(s: &Setup, name: &str) {
    let leads = {
        let ws = s.app.workspace.read();
        let p = ws.pane(s.pane).unwrap();
        let place = p
            .listing
            .iter()
            .find(|e| e.is_place() && e.name_str() == name)
            .unwrap_or_else(|| panic!("a place {name}"));
        p.location_of(place.id).expect("a place leads somewhere")
    };
    go(s, &leads);
}

fn shows(s: &Setup) -> Location {
    s.app.pane_location(s.pane).unwrap()
}

fn plan_of(outcome: Outcome) -> bibli_core::op::Plan {
    match outcome {
        Outcome::Plan(plan) => plan,
        other => panic!("a plan, not {other:?}"),
    }
}

#[test]
fn tags_are_browsed_as_folders_and_up_walks_back_to_the_folder_they_are_of() {
    let s = setup();
    go(&s, &tag_location(&s.root, None));
    assert_eq!(
        listed(&s),
        (
            set(&[("Places", 4), ("family", 2), ("work", 1)]),
            BTreeSet::new()
        ),
        "the top lists the tags, each counting its files once"
    );
    enter(&s, "Places");
    assert_eq!(
        listed(&s),
        (set(&[("France", 3), ("Japan", 1)]), BTreeSet::new()),
        "no file is tagged Places itself"
    );
    enter(&s, "France");
    assert_eq!(
        listed(&s),
        (set(&[("Paris", 1), ("Lyon", 1)]), names(&["france.jpg"])),
        "its tags, and the files tagged France exactly"
    );
    let up = Invocation::new("up");
    s.app.dispatch(&up).unwrap();
    assert_eq!(shows(&s), tag_location(&s.root, Some("Places")));
    s.app.dispatch(&up).unwrap();
    assert_eq!(shows(&s), tag_location(&s.root, None));
    s.app.dispatch(&up).unwrap();
    assert_eq!(
        shows(&s),
        Location::from_path(&s.root),
        "the top of the tags goes up to the folder they are of"
    );
}

#[test]
fn what_narrows_a_tag_stays_while_it_is_walked() {
    let s = setup();
    go(&s, &tag_location(&s.root, Some("Places")));
    let narrow = |facet: &str, value: Option<&str>| {
        s.app
            .dispatch(&Invocation::with_args(
                "tags.narrow",
                &serde_json::json!({ "facet": facet, "value": value }),
            ))
            .unwrap();
        assert!(s.app.pump_until_complete(s.pane, Duration::from_secs(10)));
    };
    narrow("also_tag", Some("family"));
    assert_eq!(
        listed(&s).0,
        set(&[("France", 1), ("Japan", 1)]),
        "only the family's files are counted"
    );
    enter(&s, "France");
    let shown = Address::parse(&shows(&s).rest);
    assert_eq!(shown.with, ["family"], "kept while going down");
    assert_eq!(listed(&s), (set(&[("Paris", 1)]), BTreeSet::new()));
    narrow("deep", None);
    assert_eq!(
        listed(&s).1,
        names(&["paris.jpg"]),
        "everything within France that the family has"
    );
    narrow("also_tag", Some("family"));
    assert_eq!(
        listed(&s).1,
        names(&["paris.jpg", "lyon.jpg", "france.jpg"]),
        "its pill's × takes it away"
    );
}

#[test]
fn a_tag_gives_itself_to_what_is_dropped_and_delete_takes_it_off() {
    let s = setup();
    let france = tag_location(&s.root, Some("Places/France"));
    let japan = tag_location(&s.root, Some("Places/Japan"));
    go(&s, &france);
    let entry = |name: &str| EntryRef::from_path(s.root.join(name)).unwrap();
    // Dropped from another application, or pasted: given the tag.
    let plan = plan_of(
        s.app
            .dispatch(&Invocation::with_args(
                "tags.drop",
                &serde_json::json!({
                    "location": france.to_string(),
                    "entries": [entry("untagged.jpg")],
                }),
            ))
            .unwrap(),
    );
    s.app.apply(plan);
    until(&s.app, "the file to have the tag", || {
        read_tags(&s.root.join("untagged.jpg"))
            .contains(&"Places/France".to_owned())
            .then_some(())
    });
    until(&s.app, "the file listed", || {
        listed(&s).1.contains("untagged.jpg").then_some(())
    });
    // Moved from another tag: out of that one.
    let plan = plan_of(
        s.app
            .dispatch(&Invocation::with_args(
                "tags.drop",
                &serde_json::json!({
                    "location": france.to_string(),
                    "entries": [entry("tokyo.jpg")],
                    "move": true,
                    "from": japan.to_string(),
                }),
            ))
            .unwrap(),
    );
    s.app.apply(plan);
    until(&s.app, "Japan replaced by France", || {
        (read_tags(&s.root.join("tokyo.jpg")) == tags(&["family", "Places/France"])).then_some(())
    });
    // Delete takes the tag off, the file staying where it is.
    until(&s.app, "the file listed", || {
        listed(&s).1.contains("tokyo.jpg").then_some(())
    });
    s.app
        .dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["tokyo.jpg"] }),
        ))
        .unwrap();
    let plan = plan_of(s.app.dispatch(&Invocation::new("take_out")).unwrap());
    s.app.apply(plan);
    until(&s.app, "the tag taken off", || {
        (read_tags(&s.root.join("tokyo.jpg")) == tags(&["family"])).then_some(())
    });
    until(&s.app, "the file gone from the tag", || {
        (!listed(&s).1.contains("tokyo.jpg")).then_some(())
    });
    assert!(s.root.join("tokyo.jpg").exists());
}

#[test]
fn a_tag_made_in_a_tag_is_there_until_a_file_has_it() {
    let s = setup();
    let places = tag_location(&s.root, Some("Places"));
    go(&s, &places);
    s.app
        .dispatch(&Invocation::with_args(
            "tags.new",
            &serde_json::json!({ "location": places.to_string(), "name": "Spain" }),
        ))
        .unwrap();
    until(&s.app, "the new tag", || {
        listed(&s)
            .0
            .contains(&("Spain".to_owned(), 0))
            .then_some(())
    });
    // A file dropped on it makes it a tag like any other.
    let spain = tag_location(&s.root, Some("Places/Spain"));
    let plan = plan_of(
        s.app
            .dispatch(&Invocation::with_args(
                "tags.drop",
                &serde_json::json!({
                    "location": spain.to_string(),
                    "entries": [EntryRef::from_path(s.root.join("untagged.jpg")).unwrap()],
                }),
            ))
            .unwrap(),
    );
    s.app.apply(plan);
    until(&s.app, "it to hold the file", || {
        listed(&s)
            .0
            .contains(&("Spain".to_owned(), 1))
            .then_some(())
    });
}

#[test]
fn a_pill_alt_clicked_shows_its_tag_in_the_folder_shown() {
    let s = setup();
    s.app
        .navigate(s.pane, &Location::from_path(&s.root))
        .unwrap();
    assert!(s.app.pump_until_complete(s.pane, Duration::from_secs(10)));
    s.app
        .dispatch(&Invocation::with_args(
            "tags.pill",
            &serde_json::json!({ "tag": "family", "how": "here" }),
        ))
        .unwrap();
    assert_eq!(shows(&s), tag_location(&s.root, Some("family")));
    assert!(s.app.pump_until_complete(s.pane, Duration::from_secs(10)));
    assert_eq!(listed(&s).1, names(&["paris.jpg", "tokyo.jpg"]));
}

#[test]
fn a_file_of_a_tag_is_shown_in_its_folder() {
    let s = setup();
    go(&s, &tag_location(&s.root, Some("Places/France/Lyon")));
    s.app
        .dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["lyon.jpg"] }),
        ))
        .unwrap();
    s.app.dispatch(&Invocation::new("show_in_folder")).unwrap();
    assert_eq!(shows(&s), Location::from_path(s.root.join("trips")));
    until(&s.app, "the file selected in its folder", || {
        let ws = s.app.workspace.read();
        let p = ws.pane(s.pane).unwrap();
        let selected: Vec<String> = p
            .selection
            .iter()
            .filter_map(|id| p.entry(id).map(|e| e.name_str().into_owned()))
            .collect();
        (selected == ["lyon.jpg"]).then_some(())
    });
}
