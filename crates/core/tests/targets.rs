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
