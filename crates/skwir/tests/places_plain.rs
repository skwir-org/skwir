//! What Keep does where nothing keeps a filter (ADR-109 rule 7), through
//! the commands a user's keys and buttons run.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::location::Location;

const WAIT: Duration = Duration::from_secs(5);

/// A folder on a real file system, never `/tmp`.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("places-plain-")
        .tempdir_in(base)
        .unwrap()
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

/// With nothing that keeps a filter (the query plugin off, ADR-109 rule
/// 7), a folder shown filtered is kept plain, as a bookmark, and its toast
/// says the filter was not kept with it.
#[test]
fn without_a_keeper_of_filters_a_folder_is_bookmarked_plain() {
    let tmp = home_tempdir();
    let root = folder(tmp.path());
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_places::PlacesPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, WAIT));
    run(
        &app,
        "filter",
        &serde_json::json!({ "text": "beach", "pane": pane }),
    );
    let _ = app.take_notifications();
    // Off the window's thread, the bookmark is written before `keep`
    // answers.
    run(&app, "keep", &serde_json::json!({ "pane": pane }));
    let bookmarked = skwir_platform::places::bookmarks()
        .iter()
        .any(|b| b.path == root);
    let said: Vec<String> = app
        .take_notifications()
        .into_iter()
        .map(|n| n.text)
        .collect();
    run(
        &app,
        "places.unbookmark",
        &serde_json::json!({ "path": root }),
    );
    assert!(bookmarked, "{said:?}");
    assert!(
        said.iter()
            .any(|text| text.contains("Bookmarked") && text.contains("“beach”")),
        "{said:?}"
    );
}

/// A tag's place pinned plain says so too: `tags.keep` hands the filter
/// the core `keep` left out to `tags.pin`, whose toast names it.
#[test]
fn a_tag_pinned_plain_says_its_filter_was_not_kept() {
    let tmp = home_tempdir();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    let _ = app.take_notifications();
    // Off the window's thread, the pin is saved before the command answers.
    run(
        &app,
        "tags.pin",
        &serde_json::json!({ "tag": "it's family", "unkept": "beach" }),
    );
    let said: Vec<String> = app
        .take_notifications()
        .into_iter()
        .map(|n| n.text)
        .collect();
    assert!(
        said.iter()
            .any(|text| text.contains("it's family") && text.contains("“beach”")),
        "{said:?}"
    );
}
