//! A place is where it is and what filters it there (ADR-109): kept in
//! the sidebar, remembered with the window's tabs, and opened again as it
//! was kept, through the commands a user's keys and buttons run.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::location::Location;
use skwir_core::workspace::PaneId;

const WAIT: Duration = Duration::from_secs(5);

/// A folder on a real file system, never `/tmp`.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("places-kept-")
        .tempdir_in(base)
        .unwrap()
}

/// An app with the query plugin, which reads filters and keeps saved
/// places, its settings under `state`.
fn app_in(state: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(state)).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app
}

/// A folder with a picture and a text, its name quoted and spaced.
fn folder(tmp: &Path) -> PathBuf {
    let root = tmp.join("it's \"summer\" photos");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("beach day.jpg"), b"\xff\xd8\xff\xe0").unwrap();
    std::fs::write(root.join("beach notes.txt"), "sand").unwrap();
    root
}

fn run(app: &App, name: &str, args: &serde_json::Value) {
    app.dispatch(&Invocation::with_args(name, args))
        .unwrap_or_else(|error| panic!("{name}: {error:#}"));
}

fn shown(app: &App, pane: PaneId) -> (Option<Location>, Option<String>, bool) {
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    (
        p.location().cloned(),
        p.filter().map(str::to_owned),
        p.lens().is_some(),
    )
}

/// The last window's tabs come back as they were shown: each pane's
/// location filtered as it was, its lens read again (ADR-109). A tab
/// filtered by nothing is written as before, with no filters.
#[test]
fn the_last_window_s_tabs_come_back_filtered() {
    let tmp = home_tempdir();
    let root = folder(tmp.path());
    let plain = tmp.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let app = app_in(&tmp.path().join("state"));
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, WAIT));
    run(
        &app,
        "filter",
        &serde_json::json!({ "text": "kind:image beach", "pane": pane }),
    );
    let (_, other) = app
        .new_tab(Some(pane), Some(Location::from_path(&plain)), false)
        .unwrap();
    run(
        &app,
        "filter",
        &serde_json::json!({ "text": null, "pane": other }),
    );

    let saved = app.saved_tabs();
    let value = toml::Value::try_from(&saved).unwrap();
    let tabs = value["tabs"].as_array().unwrap();
    assert!(tabs[0].get("filters").is_some());
    assert!(
        tabs[1].get("filters").is_none(),
        "nothing filtered: written as before"
    );

    let next = app_in(&tmp.path().join("state again"));
    assert!(next.restore_tabs(&value.try_into().unwrap()));
    next.resume_shown().unwrap();
    let pane = next.workspace.read().active_id();
    assert!(next.pump_until_complete(pane, WAIT));
    assert_eq!(
        shown(&next, pane),
        (
            Some(Location::from_path(&root)),
            Some("kind:image beach".to_owned()),
            true
        )
    );
}
