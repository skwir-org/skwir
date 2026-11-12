//! A folder that remembers how it is sorted and grouped (ADR-132): shown
//! so wherever it is shown, the pane's own order set aside meanwhile and
//! back once the pane leaves it, or the folder forgets.

mod common;

use std::path::Path;
use std::time::Duration;

use common::{active, app, home_tempdir, navigate};
use skwir_core::app::App;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::grouping::GroupBy;
use skwir_core::location::{Location, SortKey, SortSpec};
use skwir_core::orders::{ORDERS, Order};
use skwir_core::workspace::PaneId;

// Taken whole: each call reads as the command it runs, `json!` in place.
#[allow(clippy::needless_pass_by_value)]
fn run(app: &App, name: &str, args: serde_json::Value) -> Outcome {
    app.dispatch(&Invocation::with_args(name, &args))
        .unwrap_or_else(|e| panic!("{name}: {e:#}"))
}

fn sort_of(app: &App, pane: PaneId) -> SortSpec {
    app.workspace.read().pane(pane).unwrap().sort
}

fn remembered(app: &App, folder: &Path) -> Option<Order> {
    app.workspace.read().order_of(&Location::from_path(folder))
}

/// The pane's order: largest first. The folder's: newest first, grouped
/// by kind.
fn by_size() -> (SortKey, bool) {
    (SortKey::Size, false)
}

fn tree(root: &Path) {
    for dir in ["it's sorted", "plain folder", "it's sorted/inside"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("it's sorted/a file.txt"), b"x").unwrap();
}

#[test]
fn a_folder_that_remembers_is_shown_so_and_the_pane_s_own_order_comes_back() {
    let dir = home_tempdir();
    tree(dir.path());
    let (sorted, plain) = (
        dir.path().join("it's sorted"),
        dir.path().join("plain folder"),
    );
    let app = app(dir.path());
    navigate(&app, &sorted);
    let pane = active(&app);
    run(
        &app,
        "sort",
        serde_json::json!({ "key": "size", "ascending": false }),
    );
    let own = sort_of(&app, pane);
    assert_eq!((own.key, own.ascending), by_size());

    // Remembered as it is shown: nothing moves.
    run(&app, "remember_order", serde_json::json!({}));
    assert!(app.workspace.read().pane(pane).unwrap().keeps_order());
    assert_eq!(sort_of(&app, pane), own);
    // A change made there is the folder's.
    run(&app, "sort", serde_json::json!({ "key": "modified" }));
    run(&app, "group", serde_json::json!({ "by": "kind" }));
    let folder = Order {
        key: SortKey::Modified,
        ascending: false,
        group: GroupBy::Key(SortKey::Kind),
        group_ascending: true,
    };
    assert_eq!(remembered(&app, &sorted), Some(folder));
    assert_eq!(Order::of(sort_of(&app, pane)), folder);
    let kept = app.settings.get::<toml::Value>(ORDERS).unwrap();
    assert_eq!(kept.as_array().map(Vec::len), Some(1), "{kept:?}");

    // Elsewhere, the pane's own; back there, the folder's.
    navigate(&app, &plain);
    assert!(!app.workspace.read().pane(pane).unwrap().keeps_order());
    assert_eq!(sort_of(&app, pane), own);
    navigate(&app, &sorted);
    assert_eq!(Order::of(sort_of(&app, pane)), folder);
    // Hidden files stay the pane's wherever it is.
    run(&app, "toggle_hidden", serde_json::json!({}));
    assert!(sort_of(&app, pane).show_hidden);
    navigate(&app, &plain);
    assert!(sort_of(&app, pane).show_hidden);
    assert_eq!(
        sort_of(&app, pane),
        SortSpec {
            show_hidden: true,
            ..own
        }
    );

    // Forgotten where it is shown: the pane's own at once.
    navigate(&app, &sorted);
    run(&app, "remember_order", serde_json::json!({ "on": false }));
    assert_eq!(remembered(&app, &sorted), None);
    assert!(!app.workspace.read().pane(pane).unwrap().keeps_order());
    assert_eq!(
        sort_of(&app, pane),
        SortSpec {
            show_hidden: true,
            ..own
        }
    );
    let kept = app.settings.get::<toml::Value>(ORDERS).unwrap();
    assert_eq!(kept.as_array().map(Vec::len), Some(0));
}

/// Another pane showing the folder shows it so, and hears of a change made
/// in the first; a pane asleep in a tab not shown shows what it remembers
/// once woken; a tab keeps its pane's own order, never the folder's.
#[test]
fn every_pane_shows_the_folder_so_and_a_tab_keeps_the_pane_s_own() {
    let dir = home_tempdir();
    tree(dir.path());
    let sorted = dir.path().join("it's sorted");
    let app = app(dir.path());
    navigate(&app, &sorted);
    let first = active(&app);
    run(&app, "remember_order", serde_json::json!({ "on": true }));
    run(&app, "sort", serde_json::json!({ "key": "modified" }));

    // The other side of a split.
    let second = app.workspace.write().add_pane();
    run(
        &app,
        "navigate",
        serde_json::json!({ "location": sorted, "pane": second }),
    );
    assert!(app.pump_until_complete(second, Duration::from_secs(10)));
    assert_eq!(sort_of(&app, second).key, SortKey::Modified);
    run(
        &app,
        "sort",
        serde_json::json!({ "key": "size", "pane": second }),
    );
    assert_eq!(
        sort_of(&app, first).key,
        SortKey::Size,
        "told in every pane"
    );

    // Changed from a tab of its own while the first sleeps.
    run(
        &app,
        "tab.new",
        serde_json::json!({ "location": sorted, "show": true }),
    );
    let third = active(&app);
    assert!(app.pump_until_complete(third, Duration::from_secs(10)));
    assert_eq!(sort_of(&app, third).key, SortKey::Size);
    run(&app, "sort", serde_json::json!({ "key": "name" }));
    run(&app, "tab.select", serde_json::json!({ "index": 0 }));
    assert!(app.pump_until_complete(first, Duration::from_secs(10)));
    assert_eq!(
        sort_of(&app, first).key,
        SortKey::Name,
        "shown as remembered once woken"
    );

    let saved = app.saved_windows();
    let tabs = &saved.windows[0].tabs;
    assert!(
        tabs.iter().all(|tab| tab.sorts.is_empty()),
        "each pane's own is a new pane's: {:?}",
        tabs.iter().map(|tab| &tab.sorts).collect::<Vec<_>>()
    );
}

/// A folder renamed takes along what it and the folders it holds
/// remember; undone, it goes back.
#[test]
fn a_folder_renamed_takes_its_order_along_and_back_when_undone() {
    let dir = home_tempdir();
    tree(dir.path());
    let sorted = dir.path().join("it's sorted");
    let inside = sorted.join("inside");
    let app = app(dir.path());
    navigate(&app, &inside);
    run(&app, "remember_order", serde_json::json!({}));
    run(&app, "sort", serde_json::json!({ "key": "modified" }));
    navigate(&app, &sorted);
    run(&app, "remember_order", serde_json::json!({}));
    run(&app, "sort", serde_json::json!({ "key": "size" }));
    navigate(&app, dir.path());

    let renamed = dir.path().join("now named so");
    let plan = common::plan(
        &app,
        "rename",
        serde_json::json!({ "entries": [sorted], "new_name": "now named so" }),
    );
    common::apply(&app, &plan);
    assert_eq!(remembered(&app, &sorted), None);
    assert_eq!(remembered(&app, &inside), None);
    assert_eq!(
        remembered(&app, &renamed).map(|o| o.key),
        Some(SortKey::Size)
    );
    assert_eq!(
        remembered(&app, &renamed.join("inside")).map(|o| o.key),
        Some(SortKey::Modified)
    );
    navigate(&app, &renamed);
    assert_eq!(sort_of(&app, active(&app)).key, SortKey::Size);

    navigate(&app, dir.path());
    let undone = app
        .executor
        .undo_blocking(app.lifetime())
        .expect("something to undo");
    assert!(undone.error.is_none(), "{:?}", undone.error);
    assert_eq!(
        remembered(&app, &sorted).map(|o| o.key),
        Some(SortKey::Size)
    );
    assert_eq!(
        remembered(&app, &inside).map(|o| o.key),
        Some(SortKey::Modified)
    );
}

/// A column beside the pane is in its own folder's order (ADR-132): the
/// one it remembers, else the pane's own, so none moves as the pane goes
/// from one to the next; the folder shown in its own.
#[test]
fn each_column_is_in_its_own_folder_s_order() {
    let dir = home_tempdir();
    tree(dir.path());
    let (sorted, inside) = (
        dir.path().join("it's sorted"),
        dir.path().join("it's sorted/inside"),
    );
    let app = app(dir.path());
    navigate(&app, &sorted);
    let pane = active(&app);
    run(&app, "remember_order", serde_json::json!({}));
    run(
        &app,
        "sort",
        serde_json::json!({ "key": "size", "ascending": false }),
    );
    // Into the folder below, the one above kept as a column.
    navigate(&app, &inside);
    let (root, above) = (
        Location::from_path(dir.path()),
        Location::from_path(&sorted),
    );
    app.keep_locations(pane, &[root.clone(), above.clone()]);
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    assert_eq!(p.sort.key, SortKey::Name, "the pane's own, below");
    assert_eq!(p.kept_sorting(&above).spec.key, SortKey::Size, "its own");
    assert_eq!(
        p.kept_sorting(&root).spec.key,
        SortKey::Name,
        "the pane's own"
    );
    drop(ws);

    // Back up: the column of the folder left is in the pane's own order.
    navigate(&app, &sorted);
    let inner = Location::from_path(&inside);
    app.keep_locations(pane, &[root.clone(), inner.clone()]);
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    assert_eq!(p.sort.key, SortKey::Size, "the folder's, shown");
    assert_eq!(p.kept_sorting(&inner).spec.key, SortKey::Name);
    assert_eq!(p.kept_sorting(&root).spec.key, SortKey::Name);
}

/// Remembered with the view it is shown in, where a folder may remember
/// it (the shell gives the one shown); a view of the pane's own (the
/// columns) is not remembered.
#[test]
fn a_folder_remembers_the_view_it_is_shown_in() {
    use skwir_core::registry::ViewSpec;
    let dir = home_tempdir();
    tree(dir.path());
    let sorted = dir.path().join("it's sorted");
    let app = app(dir.path());
    let revision = std::sync::Arc::default();
    app.registry.views.add(
        "test",
        ViewSpec::named("grid", "Grid", "grid", std::sync::Arc::clone(&revision)).remembered(),
    );
    app.registry.views.add(
        "test",
        ViewSpec::named("columns", "Columns", "columns", revision),
    );
    navigate(&app, &sorted);
    let pane = active(&app);
    run(
        &app,
        "remember_order",
        serde_json::json!({ "view": "columns" }),
    );
    let at = Location::from_path(&sorted);
    assert_eq!(app.workspace.read().view_of(&at), None, "the pane's");
    assert!(app.remember_view(pane, "grid"));
    assert_eq!(app.workspace.read().view_of(&at).as_deref(), Some("grid"));
    assert_eq!(
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .place_view()
            .map(|view| &**view),
        Some("grid")
    );
    assert!(!app.remember_view(pane, "columns"));
    run(&app, "remember_order", serde_json::json!({ "on": false }));
    assert!(!app.remember_view(pane, "grid"), "nothing remembered");
    assert_eq!(app.workspace.read().pane(pane).unwrap().place_view(), None);
}
