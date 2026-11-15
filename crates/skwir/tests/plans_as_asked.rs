//! A plan the window makes off its thread is made as it was asked at the
//! key (`skwir_core::app::as_asked`): the selection a plan command reads,
//! whatever its plugin, is the one the key found, not what an arrow or a
//! click chose since.
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use skwir_core::app::{App, AppConfig, as_asked};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::Location;
use skwir_core::workspace::PaneId;

mod common;

const ASKED: &str = "it's asked.txt";
const CHOSEN_SINCE: &str = "chosen since.txt";

fn select(app: &App, pane: PaneId, name: &str) {
    let mut ws = app.workspace.write();
    let p = ws.pane_mut(pane).expect("the pane");
    let id = p
        .listing
        .find_by_name(std::ffi::OsStr::new(name))
        .expect("listed")
        .id;
    p.selection.set(vec![id]);
}

/// What a plan's steps change, as words: the names they touch.
fn touched(outcome: anyhow::Result<Outcome>, command: &str) -> String {
    let Ok(Outcome::Plan(plan)) = outcome else {
        panic!("{command}: no plan: {outcome:?}");
    };
    plan.steps
        .iter()
        .map(|step| format!("{:?}", step.describe()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_plan_is_made_of_the_selection_the_key_found() {
    let tmp = common::home_tempdir();
    let root = tmp.path().join("the folder ‘asked’");
    std::fs::create_dir_all(&root).unwrap();
    for name in [ASKED, CHOSEN_SINCE, "left alone.txt"] {
        std::fs::write(root.join(name), b"x").unwrap();
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_trash::TrashPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_rename_bulk::RenameBulkPlugin), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(30)));

    // The key: what it finds is taken. Then another file is chosen.
    select(&app, pane, ASKED);
    let asked = app.asked(None);
    select(&app, pane, CHOSEN_SINCE);

    let dest = tmp.path().join("elsewhere");
    std::fs::create_dir_all(&dest).unwrap();
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let commands: [(&str, Value); 6] = [
        ("trash", json!({})),
        ("delete", json!({})),
        ("copy", json!({ "dest": path(&dest) })),
        ("set_access", json!({ "mode": "u+x" })),
        ("rename_bulk", json!({ "template": "{name} kept{.ext}" })),
        ("tags.add", json!({ "tags": ["asked"] })),
    ];
    for (command, args) in commands {
        let outcome = as_asked(asked.clone(), || {
            app.dispatch(&Invocation::with_args(command, &args))
        });
        let said = touched(outcome, command);
        assert!(
            said.contains(ASKED),
            "{command} acts on the asked file: {said}"
        );
        assert!(
            !said.contains(CHOSEN_SINCE),
            "{command} leaves what was chosen since: {said}"
        );
    }
    // Asked as nothing: the live selection, as before.
    let said = touched(
        app.dispatch(&Invocation::with_args("trash", &json!({}))),
        "trash",
    );
    assert!(said.contains(CHOSEN_SINCE), "{said}");
    // The folder too: a new folder goes where the key found the pane.
    let mkdir = as_asked(asked, || {
        app.dispatch(&Invocation::with_args("mkdir", &json!({ "name": "made" })))
    });
    let said = touched(mkdir, "mkdir");
    assert!(said.contains("the folder ‘asked’"), "{said}");
}
