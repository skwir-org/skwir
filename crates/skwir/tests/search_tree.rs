//! A search laid out as the tree of its folders, the Tree view's way of
//! showing it: the same hits, each under its folders, the folders on the
//! way dimmed, and only naming one selects it.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::entry::EntryId;
use skwir_core::workspace::PaneId;

fn fixture() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("search-tree-")
        .tempdir_in(base)
        .unwrap()
}

fn write(root: &Path, relative: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}

/// Each row's name and depth.
fn rows(app: &App, pane: PaneId) -> Vec<(String, usize)> {
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    p.order()
        .iter()
        .filter_map(|id| Some((p.entry(*id)?.name_str().into_owned(), p.depth(*id))))
        .collect()
}

fn shows(app: &App, pane: PaneId, expected: &[(&str, usize)], what: &str) {
    let expected: Vec<(String, usize)> = expected
        .iter()
        .map(|(n, d)| ((*n).to_owned(), *d))
        .collect();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.pump();
        let now = rows(app, pane);
        if now == expected {
            return;
        }
        assert!(Instant::now() < deadline, "{what}: {now:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn id_of(app: &App, pane: PaneId, name: &str) -> EntryId {
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    p.order()
        .iter()
        .copied()
        .find(|id| p.entry(*id).is_some_and(|e| e.name_str() == name))
        .unwrap()
}

fn selected(app: &App, pane: PaneId) -> std::collections::BTreeSet<String> {
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    p.selection
        .iter()
        .filter_map(|id| Some(p.entry(id)?.name_str().into_owned()))
        .collect()
}

fn run(app: &App, name: &str, args: &serde_json::Value) {
    app.dispatch(&Invocation::with_args(name, args)).unwrap();
}

#[test]
fn a_search_is_laid_out_under_its_folders() {
    let tmp = fixture();
    let root = tmp.path().join("it's a folder ü");
    for name in ["a/b/x.txt", "a/y.txt", "c/z.md", "top.txt"] {
        write(&root, name);
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    app.navigate(
        pane,
        &skwir_plugin_query::location_for("ext:txt", Some(&root)),
    )
    .unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
    let flat = [("top.txt", 0), ("x.txt", 0), ("y.txt", 0)];
    shows(&app, pane, &flat, "flat at first");

    app.set_found_as_tree(pane, true);
    let tree = [
        ("a", 0),
        ("b", 1),
        ("x.txt", 2),
        ("y.txt", 1),
        ("top.txt", 0),
    ];
    shows(&app, pane, &tree, "under its folders");
    let (way, hit) = (id_of(&app, pane, "a"), id_of(&app, pane, "top.txt"));
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert!(p.only_leads(way), "a way is dimmed");
        assert!(!p.only_leads(hit), "a hit is not");
        assert_eq!(p.found_count(), Some(3), "the ways are not counted");
    }

    // A range and Select All never take a folder on the way.
    run(&app, "select_all", &serde_json::json!({ "pane": pane }));
    assert_eq!(
        selected(&app, pane),
        ["top.txt", "x.txt", "y.txt"]
            .map(str::to_owned)
            .into_iter()
            .collect(),
        "Select All leaves the ways"
    );
    let (top, a) = (id_of(&app, pane, "top.txt"), id_of(&app, pane, "a"));
    run(
        &app,
        "select",
        &serde_json::json!({ "ids": [top], "mode": "replace", "pane": pane }),
    );
    run(
        &app,
        "select",
        &serde_json::json!({ "ids": [a], "mode": "range_to", "pane": pane }),
    );
    assert_eq!(
        selected(&app, pane),
        ["a", "top.txt", "x.txt", "y.txt"]
            .map(str::to_owned)
            .into_iter()
            .collect(),
        "only the way asked for"
    );

    // Nor does Invert, however it comes: a Delete would take them whole.
    run(
        &app,
        "select",
        &serde_json::json!({ "ids": [top], "mode": "replace", "pane": pane }),
    );
    run(&app, "select_invert", &serde_json::json!({ "pane": pane }));
    assert_eq!(
        selected(&app, pane),
        ["x.txt", "y.txt"].map(str::to_owned).into_iter().collect(),
        "Invert leaves the ways"
    );

    // A sweep passes over them, the row it starts on and the one it ends on.
    let (x, y) = (id_of(&app, pane, "x.txt"), id_of(&app, pane, "y.txt"));
    run(
        &app,
        "select",
        &serde_json::json!({ "ids": [top], "mode": "replace", "pane": pane }),
    );
    run(&app, "select_sweep", &serde_json::json!({ "pane": pane }));
    for id in [y, x, a] {
        run(
            &app,
            "select",
            &serde_json::json!({ "ids": [id], "mode": "follow", "pane": pane }),
        );
    }
    assert_eq!(
        selected(&app, pane),
        ["top.txt", "x.txt", "y.txt"]
            .map(str::to_owned)
            .into_iter()
            .collect(),
        "a sweep leaves the ways"
    );
    run(
        &app,
        "select_sweep_end",
        &serde_json::json!({ "pane": pane }),
    );

    // A rubber band covers rows as Select All does; naming one still works.
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        let order = p.order();
        assert_eq!(p.covered(order.iter().copied(), None).len(), 3, "no way");
    }
    run(
        &app,
        "select",
        &serde_json::json!({ "ids": [a], "mode": "replace", "pane": pane }),
    );
    assert_eq!(
        selected(&app, pane),
        ["a"].map(str::to_owned).into_iter().collect(),
        "a click names it"
    );

    // Closed, a folder keeps what is under it; the cursor comes back to it.
    run(
        &app,
        "select",
        &serde_json::json!({ "ids": [id_of(&app, pane, "x.txt")], "mode": "replace", "pane": pane }),
    );
    run(
        &app,
        "expand",
        &serde_json::json!({ "ids": [a], "mode": "collapse", "pane": pane }),
    );
    shows(&app, pane, &[("a", 0), ("top.txt", 0)], "folded");
    assert_eq!(
        app.workspace.read().pane(pane).unwrap().selection.cursor,
        Some(a),
        "the cursor inside goes back to the folder closed"
    );
    assert!(
        selected(&app, pane).is_empty(),
        "the cursor goes back to it, the selection does not"
    );
    run(
        &app,
        "expand",
        &serde_json::json!({ "ids": [a], "mode": "toggle", "pane": pane }),
    );
    shows(&app, pane, &tree, "open again");

    // Another view: as it comes again, the ways gone.
    app.set_found_as_tree(pane, false);
    shows(&app, pane, &flat, "flat again");
    assert!(app.workspace.read().pane(pane).unwrap().entry(a).is_none());
}
