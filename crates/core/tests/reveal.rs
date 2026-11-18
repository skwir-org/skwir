//! What waits to be selected once listed (`Pane::reveal_names`): the
//! files another program asks shown in their folder (ADR-140).

mod common;

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use common::{active, app, home_tempdir};
use skwir_core::App;
use skwir_core::command::Invocation;
use skwir_core::workspace::PaneId;

const WAIT: Duration = Duration::from_secs(10);

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

/// Names that break naive handling: an apostrophe, spaces, Unicode, a
/// hidden file.
fn tree(root: &Path) {
    for name in [
        "it's a draft.txt",
        "second file.md",
        "tröisième.txt",
        ".hidden note",
    ] {
        std::fs::write(root.join(name), b"x").unwrap();
    }
    std::fs::create_dir(root.join("a folder")).unwrap();
}

/// Asked right after navigating, before the folder is listed: every name
/// the folder has is selected, the hidden one turning hidden files on,
/// and a name it lacks is passed over.
#[test]
fn the_names_asked_are_selected_once_listed() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    let pane = active(&app);
    app.dispatch(&Invocation::with_args(
        "navigate",
        &serde_json::json!({ "location": dir.path().to_string_lossy() }),
    ))
    .expect("navigate");
    let names: Vec<OsString> = [
        "it's a draft.txt",
        "tröisième.txt",
        ".hidden note",
        "gone.txt",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    app.reveal_names(pane, names);
    assert!(
        app.pump_until_complete(pane, WAIT),
        "listing did not complete"
    );
    assert_eq!(
        selected_names(&app, pane),
        [".hidden note", "it's a draft.txt", "tröisième.txt"]
    );
    assert!(
        app.workspace.read().pane(pane).unwrap().sort.show_hidden,
        "a hidden file asked for turns hidden files on"
    );
}

/// Asked of a folder already listed: selected at once, what was selected
/// before let go.
#[test]
fn names_asked_in_a_folder_listed_are_selected_at_once() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    common::navigate(&app, dir.path());
    let pane = active(&app);
    common::select(&app, &["a folder"]);
    app.reveal_names(
        pane,
        vec![OsString::from("second file.md"), OsString::from("a folder")],
    );
    assert_eq!(selected_names(&app, pane), ["a folder", "second file.md"]);
}

/// None of the names there: nothing selected, and nothing waits once the
/// listing is complete (a file made later is not selected).
#[test]
fn names_the_folder_lacks_select_nothing() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    common::navigate(&app, dir.path());
    let pane = active(&app);
    app.reveal_names(pane, vec![OsString::from("later.txt")]);
    std::fs::write(dir.path().join("later.txt"), b"x").unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    while std::time::Instant::now() < deadline {
        app.pump_until_complete(pane, Duration::from_millis(50));
        let ws = app.workspace.read();
        if ws
            .pane(pane)
            .unwrap()
            .listing
            .find_by_name(std::ffi::OsStr::new("later.txt"))
            .is_some()
        {
            break;
        }
    }
    assert!(selected_names(&app, pane).is_empty());
}
