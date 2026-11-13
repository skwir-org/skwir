//! A pane grouped and sorted by what the query plugin knows of its files
//! (ADR-130), through the commands a menu, the palette or an agent runs.

use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::location::{Location, SortKey};
use skwir_core::workspace::{Pane, PaneId};

/// Pump until `what` holds of the pane, for a few seconds at most.
fn pump_until(app: &App, pane: PaneId, what: impl Fn(&Pane) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.pump();
        if app.workspace.read().pane(pane).is_some_and(&what) {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn run(app: &App, command: &str, args: &serde_json::Value) {
    app.dispatch(&Invocation::with_args(command, args))
        .unwrap_or_else(|e| panic!("{command} {args}: {e:#}"));
}

/// What the pane shows under each heading: the label and the names.
fn shown(app: &App, pane: PaneId) -> Vec<(String, Vec<String>)> {
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    let order = p.order();
    p.groups()
        .iter()
        .map(|span| {
            let label = span
                .group
                .as_ref()
                .map_or_else(String::new, |g| g.label.to_string());
            let names = order[span.start..span.start + span.len]
                .iter()
                .map(|id| p.entry(*id).unwrap().name_str().into_owned())
                .collect();
            (label, names)
        })
        .collect()
}

fn settled(p: &Pane) -> bool {
    !p.groups().is_empty() && !p.listing.is_sorting()
}

/// Grouped by kind, each kind's files under its heading, A to Z by what
/// the type database calls it, the folders' first whichever way, and the
/// same key again turning the groups round; a criterion starting where
/// people look first; the groups gone with the query plugin, and back
/// with it.
#[test]
fn a_pane_groups_by_kind_through_the_query_plugin() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let root = tmp.path().join("field guide");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(root.join("sketches")).unwrap();
    for name in ["dunes.jpg", "aurora.png", "packing list.txt"] {
        std::fs::write(root.join(name), name).unwrap();
    }
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));

    run(&app, "group", &serde_json::json!({ "by": "kind" }));
    assert!(pump_until(&app, pane, settled), "grouped");
    let images = || {
        (
            "Images".to_owned(),
            vec!["aurora.png".into(), "dunes.jpg".into()],
        )
    };
    let folders = || ("Folders".to_owned(), vec!["sketches".to_owned()]);
    let text = || {
        (
            "Text and code".to_owned(),
            vec!["packing list.txt".to_owned()],
        )
    };
    assert_eq!(shown(&app, pane), [folders(), images(), text()]);

    // The same key again turns the groups round, the folders still first.
    run(&app, "group", &serde_json::json!({ "by": "kind" }));
    assert!(pump_until(&app, pane, |p| settled(p) && !p.sort.group_ascending));
    assert_eq!(shown(&app, pane), [folders(), text(), images()]);

    // A criterion starts in its natural direction (ADR-130, 3).
    for (key, ascending) in [
        (SortKey::Modified, false),
        (SortKey::Size, false),
        (SortKey::Type, true),
        (SortKey::Name, true),
    ] {
        run(&app, "sort", &serde_json::json!({ "key": key }));
        let ws = app.workspace.read();
        assert_eq!(ws.pane(pane).unwrap().sort.ascending, ascending, "{key:?}");
    }

    // Without the query plugin, nothing is grouped; with it, again.
    assert!(app.plugins.set_enabled("query", false, &app));
    assert!(pump_until(&app, pane, |p| p.groups().is_empty() && p.keys().is_none()));
    assert!(app.plugins.set_enabled("query", true, &app));
    assert!(pump_until(&app, pane, settled), "grouped again");
    assert_eq!(shown(&app, pane), [folders(), text(), images()]);
}

/// Sorted by type, the files are in the order of what their types are
/// called, a type's name read on a worker.
#[test]
fn a_pane_sorts_by_what_types_are_called() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let root = tmp.path().join("types");
    std::fs::create_dir(&root).unwrap();
    for name in ["a.txt", "b.png", "c.txt", "d.png"] {
        std::fs::write(root.join(name), name).unwrap();
    }
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    run(&app, "sort", &serde_json::json!({ "key": "type" }));
    let together = |p: &Pane| {
        let names: Vec<String> = p
            .order()
            .iter()
            .map(|id| p.entry(*id).unwrap().name_str().into_owned())
            .collect();
        let at = |n: &str| names.iter().position(|m| m == n).unwrap();
        p.keys().is_some()
            && !p.listing.is_sorting()
            && at("a.txt").abs_diff(at("c.txt")) == 1
            && at("b.png").abs_diff(at("d.png")) == 1
    };
    assert!(
        pump_until(&app, pane, together),
        "each type's files together"
    );
}

/// A search grouped by Folder: each folder a group, called by its path,
/// in their paths' order, as the search knows where each file is; sorted
/// by Folder, its files together.
#[test]
fn a_search_groups_by_the_folder_each_file_is_in() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let root = tmp.path().join("field notes");
    std::fs::create_dir_all(root.join("maps/coast")).unwrap();
    for (dir, name) in [
        ("", "route.txt"),
        ("maps", "dunes.txt"),
        ("maps/coast", "tern.txt"),
        ("", "notes.txt"),
    ] {
        std::fs::write(root.join(dir).join(name), name).unwrap();
    }
    let pane = app.workspace.read().active_id();
    app.dispatch(
        &Invocation::try_with_args(
            "query.search",
            &serde_json::json!({
                "expr": "ext:txt",
                "root": skwir_core::os_path::PathArg::from(root.clone()),
                "pane": pane,
            }),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
    run(&app, "group", &serde_json::json!({ "by": "folder" }));
    assert!(pump_until(&app, pane, settled), "grouped");
    let folder = |dir: &str| {
        let path = if dir.is_empty() {
            root.clone()
        } else {
            root.join(dir)
        };
        skwir_core::exclusions::tilde(&path)
    };
    assert_eq!(
        shown(&app, pane),
        [
            (
                folder(""),
                vec!["notes.txt".to_owned(), "route.txt".to_owned()]
            ),
            (folder("maps"), vec!["dunes.txt".to_owned()]),
            (folder("maps/coast"), vec!["tern.txt".to_owned()]),
        ]
    );
    // Sorted by Folder, not grouped: the same order, without headings.
    run(&app, "group", &serde_json::json!({ "by": "none" }));
    run(&app, "sort", &serde_json::json!({ "key": "folder" }));
    let by_folder = |p: &Pane| {
        p.groups().is_empty()
            && !p.listing.is_sorting()
            && p.order()
                .iter()
                .map(|id| p.entry(*id).unwrap().name_str().into_owned())
                .collect::<Vec<_>>()
                == ["notes.txt", "route.txt", "dunes.txt", "tern.txt"]
    };
    assert!(pump_until(&app, pane, by_folder), "sorted by folder");
}

/// A group folded leaves what the pane shows, its heading kept with its
/// count: out of the order, of Select All and of what is selected; the
/// others folded, all unfolded again; folds forgotten once grouped by
/// another criterion.
#[test]
fn a_folded_group_leaves_what_the_pane_shows() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let root = tmp.path().join("field guide");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(root.join("sketches")).unwrap();
    for name in ["dunes.jpg", "aurora.png", "packing list.txt"] {
        std::fs::write(root.join(name), name).unwrap();
    }
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    run(&app, "group", &serde_json::json!({ "by": "kind" }));
    assert!(pump_until(&app, pane, settled), "grouped");
    let counts = |app: &App| -> Vec<(String, usize, usize)> {
        let ws = app.workspace.read();
        ws.pane(pane)
            .unwrap()
            .groups()
            .iter()
            .map(|s| (s.group.as_ref().unwrap().label.to_string(), s.len, s.count))
            .collect()
    };
    let selected = |app: &App| app.workspace.read().pane(pane).unwrap().selection.len();
    run(&app, "select_all", &serde_json::json!({}));
    assert_eq!(selected(&app), 4);
    // Images, the second group.
    run(&app, "group_fold", &serde_json::json!({ "group": 1 }));
    assert_eq!(
        counts(&app),
        [
            ("Folders".to_owned(), 1, 1),
            ("Images".to_owned(), 0, 2),
            ("Text and code".to_owned(), 1, 1)
        ]
    );
    assert_eq!(shown(&app, pane)[1], ("Images".to_owned(), vec![]));
    assert_eq!(selected(&app), 2, "what was selected in it no longer is");
    run(&app, "select_all", &serde_json::json!({}));
    assert_eq!(selected(&app), 2, "Select All leaves it out");
    // The others folded; then every one open again.
    run(
        &app,
        "group_fold",
        &serde_json::json!({ "group": 0, "mode": "fold_others" }),
    );
    let lens = |app: &App| counts(app).iter().map(|c| c.1).collect::<Vec<_>>();
    assert_eq!(lens(&app), [1, 0, 0]);
    run(
        &app,
        "group_fold",
        &serde_json::json!({ "mode": "unfold_all" }),
    );
    assert_eq!(lens(&app), [1, 2, 1]);
    // Folded, then grouped the other way round: still folded; by another
    // criterion: forgotten.
    run(
        &app,
        "group_fold",
        &serde_json::json!({ "group": 1, "mode": "fold" }),
    );
    run(&app, "group", &serde_json::json!({ "by": "kind" }));
    assert!(pump_until(&app, pane, |p| settled(p) && !p.sort.group_ascending));
    let images = |app: &App| {
        counts(app)
            .into_iter()
            .find(|c| c.0 == "Images")
            .map(|c| c.1)
    };
    assert_eq!(images(&app), Some(0), "the same criterion keeps it folded");
    run(&app, "group", &serde_json::json!({ "by": "extension" }));
    assert!(pump_until(&app, pane, |p| {
        settled(p) && p.groups().iter().all(|s| s.len > 0)
    }));
    run(&app, "group", &serde_json::json!({ "by": "kind" }));
    assert!(pump_until(&app, pane, settled));
    assert_eq!(images(&app), Some(2), "another criterion forgot it");
}

/// Sorted by dimensions, each picture's size is read from its header on
/// demand and the order follows it, largest first, what says none (a text)
/// the lowest; grouped by them, in classes of megapixels.
#[test]
fn pictures_sort_and_group_by_their_size_read_on_demand() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let root = tmp.path().join("plates");
    std::fs::create_dir(&root).unwrap();
    for (name, w, h) in [
        ("wide.png", 1200, 1000),
        ("small.png", 40, 30),
        ("tall.png", 300, 2000),
    ] {
        image::RgbImage::new(w, h).save(root.join(name)).unwrap();
    }
    std::fs::write(root.join("caption.txt"), "plates").unwrap();
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    run(&app, "sort", &serde_json::json!({ "key": "dimensions" }));
    let names = |p: &Pane| -> Vec<String> {
        p.order()
            .iter()
            .map(|id| p.entry(*id).unwrap().name_str().into_owned())
            .collect()
    };
    // 1.2 MP, 0.6 MP, 1 200 px, nothing.
    let largest = ["wide.png", "tall.png", "small.png", "caption.txt"];
    assert!(
        pump_until(&app, pane, |p| !p.listing.is_sorting()
            && names(p) == largest),
        "read and sorted, largest first"
    );
    run(&app, "group", &serde_json::json!({ "by": "dimensions" }));
    assert!(pump_until(&app, pane, settled));
    let labels: Vec<String> = shown(&app, pane)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(labels, ["1 to 4 MP", "Under 1 MP", "No dimensions"]);
}

/// Sorted by dimensions while no provider reads them, a file says none
/// for good no more: it is unknown, after the rest, and read once one
/// comes (the query plugin turned on).
#[test]
fn what_no_provider_read_is_read_once_one_comes() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    let root = tmp.path().join("plates");
    std::fs::create_dir(&root).unwrap();
    for (name, w, h) in [("b.png", 40, 30), ("a.png", 1200, 1000)] {
        image::RgbImage::new(w, h).save(root.join(name)).unwrap();
    }
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    run(&app, "sort", &serde_json::json!({ "key": "dimensions" }));
    for _ in 0..20 {
        app.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    let dimensions = |p: &Pane| -> Vec<Option<skwir_core::entry::Value>> {
        p.order()
            .iter()
            .map(|id| {
                p.entry(*id)
                    .unwrap()
                    .metadata
                    .value(skwir_core::entry::keys::DIMENSIONS)
                    .cloned()
            })
            .collect()
    };
    assert_eq!(
        dimensions(app.workspace.read().pane(pane).unwrap()),
        [None, None],
        "no provider: unknown, not none"
    );
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let names = |p: &Pane| -> Vec<String> {
        p.order()
            .iter()
            .map(|id| p.entry(*id).unwrap().name_str().into_owned())
            .collect()
    };
    assert!(
        pump_until(&app, pane, |p| !p.listing.is_sorting()
            && names(p) == ["a.png", "b.png"]
            && dimensions(p).iter().all(Option::is_some)),
        "read once a provider came, largest first"
    );
}

/// A search remembers no order (ADR-132): its terms are what it is. The
/// folder it searched does, and the search shows the pane's own.
#[test]
fn a_search_remembers_no_order_and_shows_the_pane_s_own() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let root = tmp.path().join("field notes");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("route.txt"), "route").unwrap();
    let pane = app.workspace.read().active_id();
    run(
        &app,
        "navigate",
        &serde_json::json!({ "location": skwir_core::os_path::PathArg::from(root.clone()) }),
    );
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
    run(&app, "remember_order", &serde_json::json!({}));
    run(&app, "sort", &serde_json::json!({ "key": "size" }));
    app.dispatch(
        &Invocation::try_with_args(
            "query.search",
            &serde_json::json!({
                "expr": "ext:txt",
                "root": skwir_core::os_path::PathArg::from(root.clone()),
                "pane": pane,
            }),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
    let p = |app: &App| {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        (p.keeps_order(), p.sort.key)
    };
    assert_eq!(p(&app), (false, SortKey::Name), "the pane's own");
    let refused = app
        .dispatch(&Invocation::with_args(
            "remember_order",
            &serde_json::json!({ "on": true }),
        ))
        .unwrap_err();
    assert!(format!("{refused:#}").contains("search"), "{refused:#}");
    assert!(
        app.workspace
            .read()
            .order_of(&Location::from_path(&root))
            .is_some_and(|order| order.key == SortKey::Size)
    );
}
