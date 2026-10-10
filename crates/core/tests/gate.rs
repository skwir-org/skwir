//! Phase 1 gate: a headless session driven only through commands.
//! List, select, rename, trash, undo, redo, all through `dispatch`.

mod common;

use std::time::Duration;

use common::*;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::op::{ExecEvent, StepStatus};

#[test]
fn list_select_rename_trash_undo_through_commands_only() {
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = home_tempdir();
    let dir = tmp.path().join("work");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
    std::fs::write(dir.join("photo.jpg"), b"jpeg").unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();

    let app = app(tmp.path());
    let pane = active(&app);

    // navigate: entries stream in; pump applies them.
    navigate(&app, &dir);
    {
        let ws = app.workspace.read();
        let listing = &ws.pane(pane).unwrap().listing;
        assert_eq!(listing.len(), 3);
        let mut names: Vec<String> = listing.iter().map(|e| e.name_str().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["notes.txt", "photo.jpg", "sub"]);
    }

    // hydrate: metadata arrives as Updated events with Filesystem provenance.
    let ids: Vec<_> = app
        .workspace
        .read()
        .pane(pane)
        .unwrap()
        .listing
        .ids()
        .collect();
    app.dispatch(&Invocation::with_args(
        "hydrate",
        &serde_json::json!({ "ids": ids }),
    ))
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        app.pump();
        let ws = app.workspace.read();
        let l = &ws.pane(pane).unwrap().listing;
        if l.iter().all(|e| e.metadata.is_hydrated()) {
            let notes = l.find_by_name(std::ffi::OsStr::new("notes.txt")).unwrap();
            assert_eq!(
                notes
                    .metadata
                    .value(skwir_core::entry::keys::SIZE)
                    .and_then(skwir_core::Value::as_u64),
                Some(5)
            );
            assert_eq!(
                notes
                    .metadata
                    .get(skwir_core::entry::keys::SIZE)
                    .unwrap()
                    .provenance,
                skwir_core::Provenance::Filesystem
            );
            break;
        }
        drop(ws);
        assert!(
            std::time::Instant::now() < deadline,
            "hydrate never completed"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    // select + rename -> a plan, not a mutation.
    select(&app, &["notes.txt"]);

    // entries: what the pane shows, as an agent reads it.
    let Outcome::Value(shown) = app
        .dispatch(&Invocation::with_args(
            "entries",
            &serde_json::json!({ "limit": 2 }),
        ))
        .unwrap()
    else {
        panic!("entries answers a value")
    };
    assert_eq!(shown["total"], 3);
    assert_eq!(shown["selected"], 1);
    assert_eq!(shown["complete"], true);
    let rows = shown["entries"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "at most the limit");
    let Outcome::Value(all) = app.dispatch(&Invocation::new("entries")).unwrap() else {
        panic!("entries answers a value")
    };
    let row = |name: &str| {
        all["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap_or_else(|| panic!("{name} listed"))
            .clone()
    };
    assert_eq!(row("notes.txt")["selected"], true);
    assert_eq!(row("notes.txt")["size"], 5);
    assert_eq!(
        row("notes.txt")["path"],
        serde_json::json!(dir.join("notes.txt"))
    );
    assert_eq!(row("sub")["folder"], true);
    assert_eq!(row("photo.jpg")["selected"], false);
    let rename_plan = plan(
        &app,
        "rename",
        serde_json::json!({ "new_name": "journal.txt" }),
    );
    assert!(
        dir.join("notes.txt").exists(),
        "rename must not mutate before apply"
    );
    let diff = rename_plan.diff(&std::sync::Arc::default());
    assert_eq!(diff.steps.len(), 1);
    assert_eq!(diff.steps[0].1, StepStatus::Pending { reversible: true });

    // apply through the command surface: apply_plan takes the serialized plan.
    let data = rename_plan.to_data();
    dispatch_and_wait(
        &app,
        &Invocation::with_args("apply_plan", &serde_json::json!({ "plan": data })),
    );
    assert!(dir.join("journal.txt").exists());
    assert!(!dir.join("notes.txt").exists());

    // The watcher reports the rename and the selection survives (same id).
    let renamed_id = wait_for_name(&app, "journal.txt");
    assert!(
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .selection
            .contains(renamed_id),
        "selection must follow identity across a rename"
    );

    // Applying the same plan again is a no-op.
    let again = app.executor.execute_blocking(
        &skwir_core::op::Plan::from_data(&data, &app.ops).unwrap(),
        app.lifetime(),
    );
    assert_eq!((again.applied, again.skipped), (0, 1));

    // trash the renamed file (selection still points at it), then undo.
    let trash_plan = plan(&app, "trash", serde_json::json!({}));
    apply(&app, &trash_plan);
    assert!(!dir.join("journal.txt").exists());

    dispatch_and_wait(&app, &Invocation::new("undo"));
    assert!(
        dir.join("journal.txt").exists(),
        "undo must restore from trash"
    );
    assert_eq!(std::fs::read(dir.join("journal.txt")).unwrap(), b"hello");

    // redo trashes it again; undo twice walks back through the rename.
    dispatch_and_wait(&app, &Invocation::new("redo"));
    assert!(!dir.join("journal.txt").exists());
    dispatch_and_wait(&app, &Invocation::new("undo"));
    dispatch_and_wait(&app, &Invocation::new("undo"));
    assert!(
        dir.join("notes.txt").exists(),
        "second undo must revert the rename"
    );

    // mkdir + move + copy round trip, through commands.
    let p = plan(&app, "mkdir", serde_json::json!({ "name": "archive" }));
    apply(&app, &p);
    wait_for_name(&app, "archive");
    select(&app, &["photo.jpg"]);
    let p = plan(
        &app,
        "copy",
        serde_json::json!({ "dest": dir.join("archive").to_string_lossy() }),
    );
    apply(&app, &p);
    assert!(dir.join("archive/photo.jpg").exists());
    let p = plan(
        &app,
        "move",
        serde_json::json!({ "dest": dir.join("sub").to_string_lossy() }),
    );
    apply(&app, &p);
    assert!(dir.join("sub/photo.jpg").exists() && !dir.join("photo.jpg").exists());

    // Every command is discoverable with a schema.
    let cmds = app.commands.list();
    assert!(
        cmds.iter()
            .any(|c| &*c.name == "rename" && c.args_schema.as_object().is_some())
    );
}

/// Drain stale executor events, dispatch, then wait for this dispatch's plan to finish.
fn dispatch_and_wait(app: &skwir_core::App, inv: &Invocation) {
    let rx = app.executor.events();
    while rx.try_recv().is_ok() {}
    // A plan is the caller's to apply, as the shell applies the user's.
    if let Outcome::Plan(plan) = app
        .dispatch(inv)
        .unwrap_or_else(|e| panic!("{}: {e:#}", inv.name))
    {
        app.apply(plan);
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while let Ok(ev) = rx.recv_deadline(deadline) {
        if let ExecEvent::Finished { error, .. } = ev {
            assert!(error.is_none(), "plan failed: {error:?}");
            return;
        }
    }
    panic!("executor never finished");
}

fn wait_for_name(app: &skwir_core::App, name: &str) -> skwir_core::EntryId {
    let pane = active(app);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        app.pump();
        if let Some(e) = app
            .workspace
            .read()
            .pane(pane)
            .unwrap()
            .listing
            .find_by_name(std::ffi::OsStr::new(name))
        {
            return e.id;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "watcher never reported {name}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
