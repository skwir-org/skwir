//! Selecting from the keyboard and the pointer: what the pane shows is
//! what gets selected, and the cursor can move without selecting.

mod common;

use bibli_core::App;
use bibli_core::command::Invocation;
use common::*;

fn selected(app: &App) -> Vec<String> {
    let ws = app.workspace.read();
    let p = ws.pane(active(app)).expect("pane");
    let mut names: Vec<String> = p
        .selection
        .iter()
        .filter_map(|id| p.listing.get(id))
        .map(|e| e.name_str().into_owned())
        .collect();
    names.sort();
    names
}

fn cursor(app: &App) -> Option<String> {
    let ws = app.workspace.read();
    let p = ws.pane(active(app)).expect("pane");
    let id = p.selection.cursor?;
    p.listing.get(id).map(|e| e.name_str().into_owned())
}

fn id_of(app: &App, name: &str) -> serde_json::Value {
    let ws = app.workspace.read();
    let p = ws.pane(active(app)).expect("pane");
    let e = p
        .listing
        .find_by_name(std::ffi::OsStr::new(name))
        .expect("entry");
    serde_json::to_value(e.id).unwrap()
}

fn select_mode(app: &App, name: &str, mode: &str) {
    let id = id_of(app, name);
    app.dispatch(&Invocation::with_args(
        "select",
        &serde_json::json!({ "ids": [id], "mode": mode }),
    ))
    .expect("select");
}

/// Names that sort a, b, c, d, plus a hidden file the pane does not show.
fn tree() -> (tempfile::TempDir, std::sync::Arc<App>) {
    let tmp = tempfile::tempdir().unwrap();
    for name in ["a one", "b's", "c ü", "d", ".hidden"] {
        std::fs::write(tmp.path().join(name), b"x").unwrap();
    }
    let app = app(tmp.path());
    navigate(&app, tmp.path());
    (tmp, app)
}

#[test]
fn select_all_and_invert_take_only_what_the_pane_shows() {
    let (_tmp, app) = tree();
    app.dispatch(&Invocation::new("select_all")).unwrap();
    assert_eq!(selected(&app), ["a one", "b's", "c ü", "d"]);

    select(&app, &["b's"]);
    app.dispatch(&Invocation::new("select_invert")).unwrap();
    assert_eq!(selected(&app), ["a one", "c ü", "d"]);
    // The cursor stays where the keyboard was.
    assert_eq!(cursor(&app).as_deref(), Some("b's"));
}

#[test]
fn the_cursor_moves_without_touching_the_selection() {
    let (_tmp, app) = tree();
    select(&app, &["a one"]);
    select_mode(&app, "c ü", "cursor");
    assert_eq!(selected(&app), ["a one"]);
    assert_eq!(cursor(&app).as_deref(), Some("c ü"));
    select_mode(&app, "c ü", "toggle");
    assert_eq!(selected(&app), ["a one", "c ü"]);
}

#[test]
fn a_range_can_be_added_to_what_is_selected() {
    let (_tmp, app) = tree();
    select(&app, &["a one"]);
    select_mode(&app, "c ü", "toggle");
    // The anchor stays on the first item: the range reaches back to it.
    select_mode(&app, "d", "add_range");
    assert_eq!(selected(&app), ["a one", "b's", "c ü", "d"]);

    select(&app, &["d"]);
    select_mode(&app, "a one", "toggle");
    select_mode(&app, "b's", "range_to");
    assert_eq!(selected(&app), ["b's", "c ü", "d"]);
}

fn sweep(app: &App, unset: bool) {
    app.dispatch(&Invocation::with_args(
        "select_sweep",
        &serde_json::json!({ "unset": unset }),
    ))
    .expect("sweep");
}

/// yazi's keys through the commands: moves leave picked items alone, a
/// pick starts from nothing, a sweep covers a range in the order shown.
#[test]
fn picks_and_sweeps_through_the_commands() {
    let (_tmp, app) = tree();
    select(&app, &["a one"]);
    select_mode(&app, "b's", "follow");
    assert_eq!(selected(&app), ["b's"], "a selection that follows moves");
    select_mode(&app, "b's", "pick");
    select_mode(&app, "d", "follow");
    assert_eq!(selected(&app), ["b's"], "a picked one stays");
    assert_eq!(cursor(&app).as_deref(), Some("d"));
    select_mode(&app, "d", "pick");
    assert_eq!(selected(&app), ["b's", "d"]);

    // A sweep from "a one" to "c ü", over what was picked.
    select_mode(&app, "a one", "follow");
    sweep(&app, false);
    select_mode(&app, "c ü", "follow");
    assert_eq!(selected(&app), ["a one", "b's", "c ü", "d"]);
    app.dispatch(&Invocation::new("select_sweep_end")).unwrap();
    select_mode(&app, "a one", "follow");
    assert_eq!(selected(&app), ["a one", "b's", "c ü", "d"]);

    // An unset sweep takes a range away.
    select_mode(&app, "b's", "follow");
    sweep(&app, true);
    select_mode(&app, "c ü", "follow");
    assert_eq!(selected(&app), ["a one", "d"]);
    // Anything else ends it: a click replaces the selection.
    select(&app, &["c ü"]);
    select_mode(&app, "d", "follow");
    assert_eq!(selected(&app), ["d"]);
}

#[test]
fn the_selection_says_what_a_link_leads_to_without_reading() {
    let tmp = home_tempdir();
    let dir = tmp.path();
    std::fs::write(dir.join("a café's photo.jpg"), "not a real picture").unwrap();
    std::os::unix::fs::symlink("a café's photo.jpg", dir.join("holiday")).unwrap();
    // A folder is one whatever its name says.
    std::fs::create_dir(dir.join("an album.jpg")).unwrap();
    let app = app(dir);
    navigate(&app, dir);
    select(&app, &["holiday", "a café's photo.jpg", "an album.jpg"]);
    let mut typed: Vec<(String, Option<String>)> = app
        .selected_typed(active(&app))
        .into_iter()
        .map(|(r, name)| {
            let own = r.path.file_name().unwrap().to_string_lossy().into_owned();
            (own, name.map(|name| name.to_string_lossy().into_owned()))
        })
        .collect();
    typed.sort();
    assert_eq!(
        typed,
        [
            (
                "a café's photo.jpg".to_owned(),
                Some("a café's photo.jpg".to_owned())
            ),
            ("an album.jpg".to_owned(), None),
            // The reference is the link's, which operations act on.
            ("holiday".to_owned(), Some("a café's photo.jpg".to_owned())),
        ]
    );
}
