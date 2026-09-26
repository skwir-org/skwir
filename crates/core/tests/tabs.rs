//! Tabs (ADR-018): a tab not shown lets go of what its panes list and gets
//! it back, selection included, when it is shown again; a closed tab comes
//! back with its history; the window's tabs survive a restart, dormant.

mod common;

use std::path::Path;
use std::time::Duration;

use bibli_core::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;
use bibli_core::tabs::{LAST_TABS, SavedTabs};
use bibli_core::workspace::{PaneId, TabId};
use common::{active, app, home_tempdir, navigate, select};

const WAIT: Duration = Duration::from_secs(10);

#[allow(clippy::needless_pass_by_value)]
fn run(app: &App, name: &str, args: serde_json::Value) -> Outcome {
    app.dispatch(&Invocation::with_args(name, &args))
        .unwrap_or_else(|e| panic!("{name}: {e:#}"))
}

fn tabs(app: &App) -> Vec<TabId> {
    app.workspace.read().tabs().iter().map(|t| t.id).collect()
}

fn shown(app: &App) -> TabId {
    app.workspace.read().active_tab().id
}

fn tab_of(app: &App, pane: PaneId) -> TabId {
    app.workspace.read().tab_of(pane).expect("a tab")
}

fn location_of(app: &App, pane: PaneId) -> Option<Location> {
    app.workspace.read().pane(pane)?.shown().cloned()
}

fn is_dormant(app: &App, pane: PaneId) -> bool {
    let ws = app.workspace.read();
    let p = ws.pane(pane).expect("pane");
    p.is_dormant() && p.listing.is_empty() && p.location().is_none()
}

fn selected_names(app: &App, pane: PaneId) -> Vec<String> {
    let ws = app.workspace.read();
    let p = ws.pane(pane).expect("pane");
    let mut names: Vec<String> = p
        .selection
        .iter()
        .filter_map(|id| p.listing.get(id))
        .map(|e| e.name_str().into_owned())
        .collect();
    names.sort();
    names
}

/// A tree whose names carry what breaks naive handling: an apostrophe,
/// spaces, Unicode.
fn tree(root: &Path) {
    for dir in ["it's a folder", "second folder", "tröisième"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("a file.txt"), b"x").unwrap();
    std::fs::write(root.join("it's a folder/inside.txt"), b"y").unwrap();
}

#[test]
fn a_tab_not_shown_lets_go_of_its_listing_and_gets_its_selection_back() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active(&app);
    select(&app, &["a file.txt", "second folder"]);
    let lifetime = app
        .workspace
        .read()
        .pane(first)
        .unwrap()
        .location_lifetime();

    run(&app, "tab.new", serde_json::json!({}));
    let second = active(&app);
    assert_ne!(first, second);
    assert_eq!(tabs(&app).len(), 2);
    assert_eq!(shown(&app), tab_of(&app, second));
    // The new tab shows what the pane it came from showed.
    assert_eq!(location_of(&app, second), location_of(&app, first));
    assert!(app.pump_until_complete(second, WAIT));
    // The tab left keeps where it was, and nothing else.
    assert!(is_dormant(&app, first));
    assert!(lifetime.is_cancelled(), "work tied to the pane stops");
    assert_eq!(
        location_of(&app, first),
        Some(Location::from_path(dir.path()))
    );

    run(&app, "tab.select", serde_json::json!({ "index": 0 }));
    assert_eq!(active(&app), first);
    assert!(app.pump_until_complete(first, WAIT));
    assert_eq!(
        selected_names(&app, first),
        ["a file.txt", "second folder"],
        "the selection comes back by identity once the listing is complete"
    );
    assert!(is_dormant(&app, second));
}

#[test]
fn the_selection_comes_back_once_and_then_is_the_user_s() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active(&app);
    select(&app, &["a file.txt"]);
    run(&app, "tab.new", serde_json::json!({}));
    run(&app, "tab.select", serde_json::json!({ "index": 0 }));
    assert!(app.pump_until_complete(first, WAIT));
    assert_eq!(selected_names(&app, first), ["a file.txt"]);
    select(&app, &["tröisième"]);
    std::fs::write(dir.path().join("new.txt"), b"z").unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    while app.workspace.read().pane(first).unwrap().listing.len() < 5 {
        assert!(std::time::Instant::now() < deadline, "the watcher reports");
        app.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(selected_names(&app, first), ["tröisième"]);
}

#[test]
fn closing_a_tab_can_be_undone_with_its_history() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    navigate(&app, &dir.path().join("it's a folder"));
    let first = active(&app);
    let first_tab = tab_of(&app, first);
    run(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("second folder") }),
    );
    run(&app, "tab.select", serde_json::json!({ "tab": first_tab }));
    assert!(app.pump_until_complete(first, WAIT));
    app.take_notifications();

    run(&app, "tab.close", serde_json::json!({}));
    assert_eq!(tabs(&app).len(), 1);
    assert!(app.workspace.read().pane(first).is_none());
    // The tab that took its place is shown, and open.
    let other = active(&app);
    assert!(app.pump_until_complete(other, WAIT));
    let notes = app.take_notifications();
    let note = notes.last().expect("a notification");
    assert_eq!(note.text, "Closed tab “it's a folder”");
    let undo = note.action.clone().expect("Undo").command;
    assert_eq!(&*undo.name, "tab.reopen");

    app.dispatch(&undo).expect("reopen");
    assert_eq!(tabs(&app).len(), 2);
    let reopened = active(&app);
    assert_eq!(
        app.workspace.read().tab_index(tab_of(&app, reopened)),
        Some(0),
        "it comes back where it was"
    );
    assert!(app.pump_until_complete(reopened, WAIT));
    {
        let ws = app.workspace.read();
        let p = ws.pane(reopened).unwrap();
        assert_eq!(p.history.len(), 2, "with its history");
        assert_eq!(p.history_pos, 1);
    }
    assert_eq!(
        location_of(&app, reopened),
        Some(Location::from_path(dir.path().join("it's a folder")))
    );
    // Undo reopens that tab once; there is nothing more to reopen.
    app.dispatch(&undo)
        .expect("nothing to reopen is not an error");
    assert_eq!(tabs(&app).len(), 2);
}

#[test]
fn the_last_tab_stays_open() {
    let dir = home_tempdir();
    let app = app(dir.path());
    navigate(&app, dir.path());
    run(&app, "tab.close", serde_json::json!({}));
    assert_eq!(tabs(&app).len(), 1);
    assert!(app.take_notifications().is_empty(), "nothing happened");
}

#[test]
fn folders_open_behind_the_shown_tab_in_the_order_they_were_opened() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active(&app);
    select(&app, &["it's a folder", "second folder", "a file.txt"]);
    run(&app, "tab.open", serde_json::json!({}));
    run(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("tröisième"), "show": false }),
    );
    // A new tab shown at once goes last.
    let names: Vec<String> = app.tab_list().into_iter().map(|t| t.title).collect();
    let root_name = dir.path().file_name().unwrap().to_string_lossy();
    assert_eq!(
        names,
        [&*root_name, "it's a folder", "second folder", "tröisième"]
    );
    assert_eq!(active(&app), first, "the tab shown stays shown");
    for tab in app.tab_list().into_iter().skip(1) {
        assert!(tab.panes.iter().all(|p| p.dormant));
    }
    // A folder is needed: files do not open in tabs.
    select(&app, &["a file.txt"]);
    let error = app
        .dispatch(&Invocation::new("tab.open"))
        .expect_err("no folder selected");
    assert!(bibli_core::command::is_nothing_selected(&error));
}

#[test]
fn tabs_move_and_are_shown_by_position() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    for name in ["it's a folder", "second folder"] {
        run(
            &app,
            "tab.new",
            serde_json::json!({ "location": dir.path().join(name) }),
        );
    }
    let order = tabs(&app);
    assert_eq!(shown(&app), order[2]);
    run(&app, "tab.select", serde_json::json!({ "delta": 1 }));
    assert_eq!(shown(&app), order[0], "steps wrap around");
    run(&app, "tab.select", serde_json::json!({ "index": -1 }));
    assert_eq!(shown(&app), order[2]);
    run(&app, "tab.move", serde_json::json!({ "to": 0 }));
    assert_eq!(tabs(&app), [order[2], order[0], order[1]]);
    assert_eq!(shown(&app), order[2], "a move keeps the tab shown");
    run(&app, "tab.move", serde_json::json!({ "delta": -5 }));
    assert_eq!(tabs(&app)[0], order[2], "steps stop at the ends");
}

#[test]
fn the_window_tabs_come_back_dormant_but_the_one_shown() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    run(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("it's a folder") }),
    );
    // A location no provider here can open is kept as it is, dormant.
    run(
        &app,
        "tab.new",
        serde_json::json!({ "location": "query:kind:image", "show": false }),
    );
    let saved = app.saved_tabs();
    assert_eq!(saved.tabs.len(), 3);
    assert_eq!(saved.shown, 1);

    // What the settings file keeps reads back the same.
    let value = toml::Value::try_from(&saved).unwrap();
    assert_eq!(value.clone().try_into::<SavedTabs>().unwrap(), saved);

    // Only a window that comes back to where it was restores its tabs.
    let settings = &app.settings;
    settings.set(LAST_TABS, value);
    assert!(bibli_core::tabs::last_tabs(settings).is_none());
    settings.set(
        bibli_core::settings::STARTUP,
        toml::Value::String("last".into()),
    );
    let saved = bibli_core::tabs::last_tabs(settings).expect("tabs to restore");

    let next = common::app(dir.path());
    assert!(next.restore_tabs(&saved));
    assert_eq!(tabs(&next).len(), 3);
    let shown_pane = active(&next);
    assert_eq!(
        next.workspace.read().tab_index(shown(&next)),
        Some(1),
        "the tab shown comes back shown"
    );
    assert!(
        next.workspace
            .read()
            .panes()
            .iter()
            .all(bibli_core::workspace::Pane::is_dormant)
    );
    next.resume_shown().unwrap();
    assert!(next.pump_until_complete(shown_pane, WAIT));
    let dormant = next
        .workspace
        .read()
        .panes()
        .iter()
        .filter(|p| p.is_dormant())
        .count();
    assert_eq!(dormant, 2, "startup opens one location");
}

#[test]
fn a_split_tab_keeps_its_panes_and_its_active_one() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let left = active(&app);
    let right = app.workspace.write().add_pane();
    app.navigate(
        right,
        &Location::from_path(dir.path().join("second folder")),
    )
    .unwrap();
    assert!(app.workspace.write().set_active(right));
    let split = shown(&app);
    run(&app, "tab.new", serde_json::json!({}));
    assert!(
        !app.workspace.write().set_active(left),
        "a pane of a tab not shown is not made active behind its back"
    );
    assert!(is_dormant(&app, left) && is_dormant(&app, right));
    let saved = app.saved_tabs();
    assert_eq!(saved.tabs[0].panes.len(), 2);
    assert_eq!(saved.tabs[0].active, 1);

    run(&app, "tab.select", serde_json::json!({ "tab": split }));
    assert_eq!(active(&app), right, "the tab's active pane is active again");
    assert!(app.pump_until_complete(left, WAIT));
    assert!(app.pump_until_complete(right, WAIT));
}

#[test]
fn a_held_tab_stays_open_until_settled() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active(&app);
    select(&app, &["a file.txt"]);
    app.hold_tabs(true);

    run(&app, "tab.new", serde_json::json!({}));
    let second = active(&app);
    // Still open and listed while a UI draws it.
    assert!(!is_dormant(&app, first));
    assert_eq!(selected_names(&app, first), ["a file.txt"]);

    // Shown again before settling: it was never let go.
    run(&app, "tab.select", serde_json::json!({ "index": 0 }));
    app.settle_tabs();
    assert!(!is_dormant(&app, first), "the tab shown is not suspended");
    assert_eq!(selected_names(&app, first), ["a file.txt"]);
    assert!(is_dormant(&app, second));

    run(&app, "tab.select", serde_json::json!({ "index": 1 }));
    assert!(!is_dormant(&app, first));
    app.settle_tabs();
    assert!(is_dormant(&app, first));

    // Without a UI holding them, tabs left are suspended at once.
    app.hold_tabs(false);
    run(&app, "tab.select", serde_json::json!({ "index": 0 }));
    assert!(is_dormant(&app, second));
}
