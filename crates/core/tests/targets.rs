//! Commands that act on entries take them named as well as act on the
//! selection (`App::targets`, ADR-043): a script or an agent names files
//! by their absolute paths, with no pane showing them.

mod common;

use bibli_core::command::{Invocation, is_nothing_selected};
use bibli_core::op::{EntryArg, EntryRef};
use common::*;
use serde_json::json;

fn refused(app: &bibli_core::App, name: &str, args: &serde_json::Value) -> anyhow::Error {
    match app.dispatch(&Invocation::with_args(name, args)) {
        Ok(outcome) => panic!("{name} {args}: expected a refusal, got {outcome:?}"),
        Err(error) => error,
    }
}

#[test]
fn file_commands_act_on_entries_named_by_path() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("work");
    let sub = dir.join("sub folder");
    std::fs::create_dir_all(&sub).unwrap();
    for name in ["notes.txt", "it's here.txt", "café.md", "draft.txt"] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let app = app(tmp.path());
    // No pane shows the folder: what the commands act on is named.

    // As a script names a file: its path alone, or with no identity.
    let rename = plan(
        &app,
        "rename",
        json!({ "entries": [dir.join("notes.txt")], "new_name": "journal.txt" }),
    );
    apply(&app, &rename);
    assert!(dir.join("journal.txt").is_file());
    assert!(!dir.join("notes.txt").exists());

    let moved = plan(
        &app,
        "move",
        json!({
            "entries": [dir.join("it's here.txt"), { "path": dir.join("café.md") }],
            "dest": sub,
        }),
    );
    assert_eq!(moved.diff().steps.len(), 2);
    apply(&app, &moved);
    assert!(sub.join("it's here.txt").is_file());
    assert!(sub.join("café.md").is_file());

    let copied = plan(
        &app,
        "copy",
        json!({ "entries": [dir.join("journal.txt")], "dest": sub }),
    );
    apply(&app, &copied);
    assert!(sub.join("journal.txt").is_file());
    assert!(dir.join("journal.txt").is_file());

    // As the window names what it shows: identity and path.
    let shown = EntryRef::from_path(dir.join("draft.txt")).unwrap();
    let trashed = plan(&app, "trash", json!({ "entries": [shown] }));
    assert_eq!(trashed.diff().steps.len(), 1);
    if !under_wine("trashing through the shell") {
        apply(&app, &trashed);
        assert!(!dir.join("draft.txt").exists());
    }
}

#[test]
fn a_path_named_must_be_whole_and_lead_somewhere() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    std::fs::write(dir.join("b.txt"), "b").unwrap();
    let app = app(tmp.path());

    // Read against whose working folder? Refused, never guessed.
    let relative = refused(&app, "trash", &json!({ "entries": ["work/a.txt"] }));
    assert!(
        format!("{relative:#}").contains("not an absolute path"),
        "{relative:#}"
    );
    let dest = refused(
        &app,
        "move",
        &json!({ "entries": [dir.join("a.txt")], "dest": "elsewhere" }),
    );
    assert!(
        format!("{dest:#}").contains("not an absolute path"),
        "{dest:#}"
    );
    let missing = refused(&app, "trash", &json!({ "entries": [dir.join("gone.txt")] }));
    assert!(
        format!("{missing:#}").contains("does not exist"),
        "{missing:#}"
    );
    let two = refused(
        &app,
        "rename",
        &json!({ "entries": [dir.join("a.txt"), dir.join("b.txt")], "new_name": "c.txt" }),
    );
    assert!(format!("{two:#}").contains("exactly one"), "{two:#}");
    // Nothing named and nothing selected: the hint a surface shows.
    assert!(is_nothing_selected(&refused(&app, "trash", &json!({}))));
    assert!(dir.join("a.txt").is_file() && dir.join("b.txt").is_file());
}

#[test]
fn an_entry_is_named_by_its_path_or_as_the_window_names_it() {
    let tmp = home_tempdir();
    let file = tmp.path().join("named.txt");
    std::fs::write(&file, "x").unwrap();
    let known = EntryRef::from_path(&file).unwrap();
    let named = |value: serde_json::Value| -> EntryRef {
        serde_json::from_value::<EntryArg>(value.clone())
            .unwrap_or_else(|e| panic!("{value}: {e}"))
            .into_ref()
            .unwrap()
    };
    assert_eq!(named(json!(file)), known);
    assert_eq!(named(json!({ "path": file })), known);
    assert_eq!(named(serde_json::to_value(&known).unwrap()), known);
    // A path that is no UTF-8 is written as its bytes.
    #[cfg(unix)]
    {
        let bytes = file.as_os_str().as_encoded_bytes().to_vec();
        assert_eq!(named(json!({ "bytes": bytes })), known);
    }
    // As the window names it, its identity is taken as given.
    let elsewhere = EntryRef::new(known.id, tmp.path().join("moved.txt"));
    assert_eq!(named(serde_json::to_value(&elsewhere).unwrap()), elsewhere);
}

#[test]
fn a_folder_or_a_file_is_made_in_the_folder_named() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("work it's");
    std::fs::create_dir_all(&dir).unwrap();
    let app = app(tmp.path());
    // No pane shows the folder: where they are made is named.
    apply(
        &app,
        &plan(&app, "mkdir", json!({ "name": "new folder", "in": dir })),
    );
    assert!(dir.join("new folder").is_dir());
    apply(
        &app,
        &plan(&app, "new_file", json!({ "name": "café.txt", "in": dir })),
    );
    assert!(dir.join("café.txt").is_file());
    apply(
        &app,
        &plan(&app, "new_file", json!({ "name": "made/", "in": dir })),
    );
    assert!(dir.join("made").is_dir());
    // A folder is named whole, and a name is one name.
    assert!(
        refused(&app, "mkdir", &json!({ "name": "x", "in": "relative" }))
            .to_string()
            .contains("absolute")
    );
    refused(
        &app,
        "mkdir",
        &json!({ "name": dir.join("elsewhere"), "in": dir }),
    );
}

#[test]
fn an_undo_asked_from_outside_the_window_says_what_it_took_back() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    let app = app(tmp.path());
    apply(
        &app,
        &plan(&app, "mkdir", json!({ "name": "kept", "in": dir })),
    );
    let undone = match app.dispatch(&Invocation::new("undo")).unwrap() {
        bibli_core::command::Outcome::Value(value) => value,
        other => panic!("expected what it undid, got {other:?}"),
    };
    assert_eq!(undone["undone"], "new folder kept");
    assert_eq!(undone["applied"], 1);
    assert!(!dir.join("kept").exists());
    let redone = match app.dispatch(&Invocation::new("redo")).unwrap() {
        bibli_core::command::Outcome::Value(value) => value,
        other => panic!("expected what it redid, got {other:?}"),
    };
    assert_eq!(redone["redone"], "new folder kept");
    assert!(dir.join("kept").is_dir());
    assert!(
        app.dispatch(&Invocation::new("redo"))
            .unwrap_err()
            .to_string()
            .contains("nothing to redo")
    );
}

#[test]
fn a_location_is_listed_without_showing_it() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("listed");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("b.txt"), "bb").unwrap();
    std::fs::write(dir.join("a it's.txt"), "a").unwrap();
    std::fs::write(dir.join(".hidden"), "").unwrap();
    let app = app(tmp.path());
    let shown = app
        .workspace
        .read()
        .pane(active(&app))
        .unwrap()
        .shown()
        .cloned();
    let listed = match app
        .dispatch(&Invocation::with_args(
            "entries",
            &json!({ "location": dir }),
        ))
        .unwrap()
    {
        bibli_core::command::Outcome::Value(value) => value,
        other => panic!("expected a listing, got {other:?}"),
    };
    assert_eq!(listed["complete"], true);
    assert_eq!(listed["total"], 3);
    let entries = listed["entries"].as_array().unwrap();
    let names: Vec<&str> = entries
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"a it's.txt") && names.contains(&"sub"),
        "{names:?}"
    );
    assert!(!names.contains(&".hidden"));
    let b = entries.iter().find(|e| e["name"] == "b.txt").unwrap();
    assert_eq!(b["size"], 2, "sizes are read for the page: {b}");
    assert!(b["modified"].is_u64());
    // Hidden ones when asked; the pane shows what it showed.
    let all = app
        .dispatch(&Invocation::with_args(
            "entries",
            &json!({ "location": dir, "hidden": true }),
        ))
        .unwrap();
    let bibli_core::command::Outcome::Value(all) = all else {
        panic!("expected a listing")
    };
    assert_eq!(all["total"], 4);
    assert_eq!(
        app.workspace
            .read()
            .pane(active(&app))
            .unwrap()
            .shown()
            .cloned(),
        shown
    );
}
