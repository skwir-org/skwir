//! Phase 1 gate: a headless session driven only through commands.
//! List, select, rename, trash, undo, redo, all through `dispatch`.

mod common;

use std::time::Duration;

use bibli_core::command::Invocation;
use bibli_core::op::{ExecEvent, StepStatus};
use common::*;

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
                    .value(bibli_core::entry::keys::SIZE)
                    .and_then(bibli_core::Value::as_u64),
                Some(5)
            );
            assert_eq!(
                notes
                    .metadata
                    .get(bibli_core::entry::keys::SIZE)
                    .unwrap()
                    .provenance,
                bibli_core::Provenance::Filesystem
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
    let rename_plan = plan(
        &app,
        "rename",
        serde_json::json!({ "new_name": "journal.txt" }),
    );
    assert!(
        dir.join("notes.txt").exists(),
        "rename must not mutate before apply"
    );
    let diff = rename_plan.diff();
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
        &bibli_core::op::Plan::from_data(&data, &app.ops).unwrap(),
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
fn dispatch_and_wait(app: &bibli_core::App, inv: &Invocation) {
    let rx = app.executor.events();
    while rx.try_recv().is_ok() {}
    app.dispatch(inv)
        .unwrap_or_else(|e| panic!("{}: {e:#}", inv.name));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while let Ok(ev) = rx.recv_deadline(deadline) {
        if let ExecEvent::Finished { error, .. } = ev {
            assert!(error.is_none(), "plan failed: {error:?}");
            return;
        }
    }
    panic!("executor never finished");
}

fn wait_for_name(app: &bibli_core::App, name: &str) -> bibli_core::EntryId {
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
