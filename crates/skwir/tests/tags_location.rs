//! Tags browsed as folders (ADR-032): a `tags:` location lists the tags
//! inside its tag as places and its own files, Up walks its trail back to
//! the folder it is of, a lens over it stays while it is walked, words
//! filter it at both its depths and search it as it is shown (ADR-108),
//! files dropped on a tag are given it, and Delete takes it off them.
// Tags kept in the files' extended attributes: Unix's (Windows keeps
// none, ADR-027).
#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::entry::keys;
use skwir_core::location::Location;
use skwir_core::op::EntryRef;
use skwir_core::workspace::PaneId;
use skwir_plugin_tags::location::Address;
use skwir_plugin_tags::{read_tags, write_tags};

/// A folder where extended attributes work: not `/tmp`, often a tmpfs.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
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
    tmp: tempfile::TempDir,
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
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    app.plugins.add(
        Arc::new(skwir_plugin_ops_basic::OpsBasicPlugin::default()),
        &app,
    );
    let pane = app.workspace.read().active_id();
    Setup {
        tmp,
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
                .and_then(skwir_core::Value::as_u64)
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

fn plan_of(outcome: Outcome) -> skwir_core::op::Plan {
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

/// What else narrows a tag's place than its tag and how deep it holds
/// files (another tag, a kind) is the pane's lens (ADR-107): kept while
/// its trail is walked and beyond it, never part of its address.
#[test]
fn a_lens_over_a_tag_stays_while_it_is_walked() {
    let s = setup();
    go(&s, &tag_location(&s.root, Some("Places")));
    let dispatch = |command: &str, args: serde_json::Value| {
        s.app
            .dispatch(&Invocation::with_args(command, &args))
            .unwrap();
        assert!(s.app.pump_until_complete(s.pane, Duration::from_secs(10)));
    };
    // A tag is what a file's metadata says: every file's is read, as Quick
    // filters ask for it.
    let shown_files = || -> BTreeSet<String> {
        let ids: Vec<skwir_core::EntryId> = {
            let ws = s.app.workspace.read();
            let p = ws.pane(s.pane).unwrap();
            p.listing
                .iter()
                .filter(|e| !e.is_place() && !e.is_dir() && !e.metadata.is_hydrated())
                .map(|e| e.id)
                .collect()
        };
        if !ids.is_empty() {
            s.app
                .dispatch(&Invocation::with_args(
                    "hydrate",
                    &serde_json::json!({ "ids": ids, "pane": s.pane }),
                ))
                .unwrap();
            until(&s.app, "every file's metadata", || {
                let ws = s.app.workspace.read();
                let p = ws.pane(s.pane).unwrap();
                p.listing
                    .iter()
                    .all(|e| e.is_place() || e.is_dir() || e.metadata.is_hydrated())
                    .then_some(())
            });
        }
        let ws = s.app.workspace.read();
        let p = ws.pane(s.pane).unwrap();
        p.order()
            .iter()
            .filter_map(|id| p.entry(*id))
            .filter(|e| !e.is_place() && !e.is_dir())
            .map(|e| e.name_str().into_owned())
            .collect()
    };
    let lens = || {
        let ws = s.app.workspace.read();
        ws.pane(s.pane).unwrap().lens().map(|l| l.words.to_string())
    };
    dispatch(
        "query.narrow",
        serde_json::json!({ "facet": "also_tag", "value": "family" }),
    );
    assert_eq!(lens().as_deref(), Some("tag:family"));
    enter(&s, "France");
    assert_eq!(
        shows(&s),
        tag_location(&s.root, Some("Places/France")),
        "the place is its tag alone"
    );
    assert_eq!(lens().as_deref(), Some("tag:family"), "kept going down");
    assert!(
        shown_files().is_empty(),
        "France's own files: none the family's"
    );
    dispatch("tags.narrow", serde_json::json!({ "facet": "deep" }));
    assert_eq!(
        shown_files(),
        names(&["paris.jpg"]),
        "everything within France that the family has"
    );
    assert!(
        s.app
            .dispatch(&Invocation::with_args(
                "tags.narrow",
                &serde_json::json!({ "facet": "also_tag", "value": "work" }),
            ))
            .is_err(),
        "another tag is the lens's, not the place's"
    );
    // Off its trail too: the folder it is of.
    go(&s, &Location::from_path(&s.root));
    assert_eq!(shown_files(), names(&["paris.jpg", "tokyo.jpg"]));
    // Its pill's × takes it off.
    dispatch(
        "query.narrow",
        serde_json::json!({ "facet": "also_tag", "value": "family" }),
    );
    assert_eq!(lens(), None);
    go(&s, &tag_location(&s.root, Some("Places/France")));
    assert_eq!(shown_files(), names(&["france.jpg"]));
}

/// Words over a tag's place filter it at either depth the field's switch
/// shows, the tag alone or with its sub-tags (ADR-108); what only a search
/// tells searches it as the pane shows it, as Enter runs it: what it finds
/// is what the place lists.
#[test]
fn a_tag_s_place_is_filtered_at_both_depths_and_searched_as_it_is_shown() {
    let s = setup();
    let france = tag_location(&s.root, Some("Places/France"));
    let depths = s.app.depths(&france).expect("a tag's place has depths");
    let filter = |text: &str| {
        s.app
            .dispatch(&Invocation::with_args(
                "filter",
                &serde_json::json!({ "text": text, "pane": s.pane }),
            ))
            .unwrap();
        s.app.pump();
    };
    let shown_files = || -> BTreeSet<String> {
        let ws = s.app.workspace.read();
        let p = ws.pane(s.pane).unwrap();
        p.order()
            .iter()
            .filter_map(|id| p.entry(*id))
            .filter(|e| !e.is_place() && !e.is_dir())
            .map(|e| e.name_str().into_owned())
            .collect()
    };
    go(&s, &depths.here);
    filter("lyon");
    assert!(shown_files().is_empty(), "France's own: none named so");
    filter("fran");
    assert_eq!(shown_files(), names(&["france.jpg"]));
    go(&s, &depths.below);
    filter("lyon");
    assert_eq!(shown_files(), names(&["lyon.jpg"]), "with its sub-tags");

    // Enter on words only a search tells: the place as it is shown.
    let words = "lyon";
    let ask = skwir_core::suggest::Ask {
        text: words.to_owned(),
        caret: words.len(),
        typed: skwir_core::suggest::Typed::Words,
        scope: Some(depths.below.clone()),
        shown: Some(depths.below.clone()),
        dir: None,
        pane: Some(s.pane),
        lens: None,
    };
    let run = skwir_core::suggest::gather(&s.app, &ask, &skwir_core::cancel::CancelToken::new())
        .rows
        .into_iter()
        .find(|row| row.section == skwir_core::suggest::Section::Run)
        .and_then(|row| row.run)
        .expect("Enter searches");
    filter("");
    s.app.dispatch(&run).unwrap();
    until(&s.app, "the search shown", || {
        s.app.pane_location(s.pane).filter(|l| s.app.is_search(l))
    });
    // Complete once each of the place's files was tested.
    assert!(s.app.pump_until_complete(s.pane, Duration::from_secs(10)));
    assert_eq!(shown_files(), names(&["lyon.jpg"]), "what the place lists");
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
fn a_folder_s_tags_are_one_step_from_it() {
    let s = setup();
    // The menu of what the folder holds (its trail's chevron): its tags
    // first, then its folders.
    let folder = Location::from_path(&s.root);
    let inside = s.app.places_in(&folder, None, false).unwrap();
    let labels: Vec<&str> = inside.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Tags", "trips"]);
    assert_eq!(inside[0].location, tag_location(&s.root, None));
    // Shown with sub-tags, the trail goes on into its tags so: the menu
    // has them so (marked, and kept when chosen).
    let narrowed = Address {
        tag: Some("Places/France".into()),
        scope: Some(s.root.clone()),
        deep: true,
    }
    .location();
    let trail = s.app.trail(&narrowed).unwrap();
    assert_eq!(trail.base.as_deref(), Some(s.root.as_path()));
    let along = &trail.crumbs[0].location;
    let inside = s.app.places_in(&folder, Some(along), false).unwrap();
    assert_eq!(&inside[0].location, along);
    assert_ne!(inside[0].location, tag_location(&s.root, None));
    // Going on into a folder, or another folder's tags: as they are.
    let subfolder = Location::from_path(s.root.join("trips"));
    let its_tags = tag_location(&s.root.join("trips"), None);
    for along in [&subfolder, &its_tags] {
        let inside = s.app.places_in(&folder, Some(along), false).unwrap();
        assert_eq!(inside[0].location, tag_location(&s.root, None));
    }
    // Browse tags here: the folder shown.
    go(&s, &Location::from_path(&s.root));
    s.app.dispatch(&Invocation::new("tags.browse")).unwrap();
    assert_eq!(shows(&s), tag_location(&s.root, None));
    assert!(s.app.pump_until_complete(s.pane, Duration::from_secs(10)));
    assert_eq!(
        listed(&s).0,
        set(&[("Places", 4), ("family", 2), ("work", 1)])
    );
    // All Tags with Alt, in a tag of the folder: the top of its tags.
    go(&s, &tag_location(&s.root, Some("Places/France")));
    s.app.dispatch(&Invocation::new("tags.browse")).unwrap();
    assert_eq!(shows(&s), tag_location(&s.root, None));
    // The address's Tags button and Alt+G, one command: a folder's tags,
    // and from a tag of it, back to the folder.
    let aspect = s.app.registry.aspects.find(|a| a.scheme == "tags").unwrap();
    let key = s.app.registry.keymap.find(|k| k.keys == "alt-g").unwrap();
    assert_eq!(
        key.command, aspect.command,
        "the key does what the button does"
    );
    go(&s, &Location::from_path(&s.root));
    s.app.dispatch(&aspect.command).unwrap();
    assert_eq!(shows(&s), tag_location(&s.root, None));
    go(&s, &tag_location(&s.root, Some("Places/France")));
    s.app.dispatch(&aspect.command).unwrap();
    assert_eq!(
        shows(&s),
        Location::from_path(&s.root),
        "back to the folder"
    );
    // A folder named, by an absolute path only.
    let trips = s.root.join("trips");
    s.app
        .dispatch(&Invocation::with_args(
            "tags.browse",
            &serde_json::json!({ "in": &trips }),
        ))
        .unwrap();
    assert_eq!(shows(&s), tag_location(&trips, None));
    assert!(
        s.app
            .dispatch(&Invocation::with_args(
                "tags.browse",
                &serde_json::json!({ "in": "trips" }),
            ))
            .is_err()
    );
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

#[test]
fn a_tag_copied_into_a_folder_is_a_folder_of_its_files() {
    let s = setup();
    let out = s.tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    // Copied from where it is listed, pasted in a folder.
    go(&s, &tag_location(&s.root, Some("Places")));
    let france = {
        let ws = s.app.workspace.read();
        let p = ws.pane(s.pane).unwrap();
        p.listing
            .iter()
            .find(|e| e.is_place() && e.name_str() == "France")
            .unwrap()
            .id
    };
    s.app
        .dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "ids": [france] }),
        ))
        .unwrap();
    s.app.dispatch(&Invocation::new("clipboard_copy")).unwrap();
    s.app.navigate(s.pane, &Location::from_path(&out)).unwrap();
    assert!(s.app.pump_until_complete(s.pane, Duration::from_secs(10)));
    s.app.dispatch(&Invocation::new("clipboard_paste")).unwrap();
    let plan = until(&s.app, "the copy proposed", || s.app.take_proposals().pop());
    let r = s.app.executor.execute_blocking(&plan, s.app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    let made: BTreeSet<String> = walkdir(&out);
    assert_eq!(
        made,
        names(&[
            "France",
            "France/france.jpg",
            "France/Lyon",
            "France/Lyon/lyon.jpg",
            "France/Paris",
            "France/Paris/paris.jpg",
        ])
    );
    assert_eq!(
        read_tags(&out.join("France/Paris/paris.jpg")),
        tags(&["Places/France/Paris", "family"]),
        "a copy keeps its tags"
    );
    // Again: the folder's name is taken, the second gets another.
    s.app.dispatch(&Invocation::new("clipboard_paste")).unwrap();
    let plan = until(&s.app, "the copy proposed", || s.app.take_proposals().pop());
    assert!(
        s.app
            .executor
            .execute_blocking(&plan, s.app.lifetime())
            .error
            .is_none()
    );
    assert!(out.join("France (2)/Paris/paris.jpg").exists());
}

/// Every path under `dir`, relative to it.
fn walkdir(dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            out.insert(p.strip_prefix(dir).unwrap().display().to_string());
            if p.is_dir() {
                stack.push(p);
            }
        }
    }
    out
}

#[test]
fn a_card_dragged_between_two_columns_of_tags_moves_from_one_to_the_other() {
    let s = setup();
    // A board of Places shows its tags opened in place; a card dragged
    // from France to Japan says the column it leaves.
    go(&s, &tag_location(&s.root, Some("Places/France")));
    s.app
        .dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["france.jpg"] }),
        ))
        .unwrap();
    let japan = tag_location(&s.root, Some("Places/Japan"));
    let france = tag_location(&s.root, Some("Places/France"));
    let plan = plan_of(
        s.app
            .dispatch(&Invocation::with_args(
                "tags.drop",
                &serde_json::json!({
                    "location": japan.to_string(),
                    "pane": s.pane,
                    "move": true,
                    "from": france.to_string(),
                }),
            ))
            .unwrap(),
    );
    s.app.apply(plan);
    until(&s.app, "the tag moved", || {
        (read_tags(&s.root.join("trips/2024/france.jpg")) == tags(&["Places/Japan"])).then_some(())
    });
}

/// A tag opened in a tab (the middle button, Ctrl+Enter, the menu's Open
/// in New Tab: `tab.open`) shows its own tag folder, as opening it does,
/// behind the tab shown.
#[test]
fn a_tag_opens_in_a_tab_where_it_leads() {
    let s = setup();
    go(&s, &tag_location(&s.root, None));
    let (id, leads) = {
        let ws = s.app.workspace.read();
        let p = ws.pane(s.pane).unwrap();
        let place = p
            .listing
            .iter()
            .find(|e| e.is_place() && e.name_str() == "family")
            .expect("a place family");
        (place.id, p.location_of(place.id).unwrap())
    };
    s.app
        .dispatch(&Invocation::with_args(
            "tab.open",
            &serde_json::json!({ "ids": [id], "pane": s.pane }),
        ))
        .unwrap();
    let tabs = s.app.tab_list();
    assert_eq!(tabs.len(), 2);
    assert!(tabs[0].shown, "the tab shown stays shown");
    assert_eq!(tabs[1].panes[0].location.as_ref(), Some(&leads));
}

/// Files cut and pasted on a tag stay cut until the paste's plan applies:
/// one refused, failed or cancelled leaves them to paste again.
#[test]
fn files_cut_and_pasted_on_a_tag_stay_cut_until_its_plan_applies() {
    let s = setup();
    go(&s, &Location::from_path(&s.root));
    s.app
        .dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["notes.txt"] }),
        ))
        .unwrap();
    s.app.dispatch(&Invocation::new("clipboard_cut")).unwrap();
    assert_eq!(s.app.pending.get().len(), 1);
    go(&s, &tag_location(&s.root, Some("family")));
    let plan = plan_of(s.app.dispatch(&Invocation::new("clipboard_paste")).unwrap());
    // Not applied yet: still cut.
    assert_eq!(s.app.pending.get().len(), 1, "cut until it applies");
    let r = s.app.executor.execute_blocking(&plan, s.app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(read_tags(&s.root.join("notes.txt")).contains(&"family".to_owned()));
    until(&s.app, "the cut done", || {
        s.app.pending.get().is_empty().then_some(())
    });
}
