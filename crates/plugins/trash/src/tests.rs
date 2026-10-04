use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bibli_core::app::{App, AppConfig};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::keys;
use bibli_core::location::Location;
use bibli_core::op::{EntryRef, Operation, Plan, Trash};

use super::{TrashPlugin, place};

/// On the home filesystem, as the user's files are: what is trashed
/// there goes to the home trash, which the tests' own XDG data home holds
/// (`tools/test/isolate-xdg.sh`).
fn base() -> tempfile::TempDir {
    let base = std::env::home_dir().unwrap().join(".cache/bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}

/// What `trash.list` says, as `(name, where it came from)`.
fn listed(app: &App) -> Vec<(String, PathBuf)> {
    let Outcome::Value(value) = app.dispatch(&Invocation::new("trash.list")).unwrap() else {
        panic!("a list")
    };
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["name"].as_str().unwrap().to_owned(),
                PathBuf::from(item["original"].as_str().unwrap()),
            )
        })
        .collect()
}

/// Waits for what a worker does, up to five seconds.
fn eventually(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn plan(app: &App, name: &str, args: &serde_json::Value) -> Plan {
    match app.dispatch(&Invocation::with_args(name, args)) {
        Ok(Outcome::Plan(plan)) => plan,
        other => panic!("{name}: expected a plan, got {other:?}"),
    }
}

/// A file put in the trash is listed there under the name it had, from
/// the folder it had, and restored there; undone, it goes back.
#[test]
fn a_file_in_the_trash_is_listed_as_it_was_and_restored_where_it_was() {
    let tmp = base();
    let folder = tmp.path().join("it's a café");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("notes [draft].txt");
    std::fs::write(&file, "x").unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
    let home = bibli_platform::trash::home_trash().unwrap();
    app.plugins.add(
        Arc::new(TrashPlugin::with_trashes(vec![home.clone()])),
        &app,
    );
    let trash: Vec<Box<dyn Operation>> = vec![Box::new(Trash {
        entry: EntryRef::from_path(&file).unwrap(),
    })];
    let r = app
        .executor
        .execute_blocking(&Plan::with_steps("move to trash", trash), app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    // Other tests share the home trash: ours is what came from here.
    let ours = |app: &App| {
        listed(app)
            .into_iter()
            .filter(|(_, from)| from.starts_with(tmp.path()))
            .collect::<Vec<_>>()
    };
    eventually("the trash is read again", || ours(&app).len() == 1);
    assert_eq!(ours(&app), [("notes [draft].txt".to_owned(), file.clone())]);
    assert_eq!(
        app.registry
            .places
            .find(|p| p.title == "Trash")
            .and_then(|p| p.icon),
        Some("user-trash-full".into())
    );

    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::parse("trash:")).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    let (id, physical) = {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        let entry = p
            .listing
            .iter()
            .find(|e| {
                e.metadata
                    .value(keys::TRASHED_FROM)
                    .and_then(bibli_core::Value::as_str)
                    == Some(folder.to_str().unwrap())
            })
            .expect("listed in the trash");
        assert_eq!(entry.name, "notes [draft].txt");
        assert!(entry.metadata.value(keys::TRASHED).is_some());
        (entry.id, p.path_of(entry.id).unwrap())
    };
    assert!(physical.starts_with(home.join("files")), "{physical:?}");
    assert!(app.verbs(&Location::parse("trash:")).restore.is_some());
    // One place in the address, which holds no places to list.
    let trail = app.trail(&Location::parse("trash:")).unwrap();
    assert_eq!(trail.crumbs[0].label, "Trash");
    assert!(!trail.holds_places);

    let restore = plan(
        &app,
        "trash.restore",
        &serde_json::json!({ "pane": pane, "ids": [id] }),
    );
    assert!(restore.undoable());
    let r = app.executor.execute_blocking(&restore, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "x");
    assert!(!physical.exists());
    eventually("restored, it leaves the trash", || ours(&app).is_empty());
    // Undone, it is in the trash again.
    let r = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(!file.exists());
    eventually("undone, it is back", || ours(&app).len() == 1);
    // Already in the trash: it does not go there twice.
    let again = app.dispatch(&Invocation::with_args(
        "trash.put",
        &serde_json::json!({ "entries": [EntryRef::from_path(&physical).unwrap()] }),
    ));
    assert!(again.is_err());
}

/// A trash on a volume (`.Trash-<uid>` at its top), as a test lays one
/// out by hand.
fn volume_trash(top: &Path) -> PathBuf {
    let trash = top.join(".Trash-1000");
    std::fs::create_dir_all(trash.join("files/folder")).unwrap();
    std::fs::create_dir_all(trash.join("info")).unwrap();
    std::fs::write(trash.join("files/a.txt"), "a").unwrap();
    std::fs::write(trash.join("files/folder/b.txt"), "b").unwrap();
    for (name, from) in [("a.txt", "docs/a.txt"), ("folder", "docs/folder")] {
        std::fs::write(
            trash.join(format!("info/{name}.trashinfo")),
            format!("[Trash Info]\nPath={from}\nDeletionDate=2026-10-04T09:30:00\n"),
        )
        .unwrap();
    }
    trash
}

/// Emptying the trash deletes every file in it for good, and what the
/// trash said of each: a plan Undo cannot take back, which the shell asks
/// about first.
#[test]
fn emptying_the_trash_deletes_its_files_and_what_it_said_of_them() {
    let tmp = base();
    let trash = volume_trash(&tmp.path().join("volume"));
    let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
    app.plugins.add(
        Arc::new(TrashPlugin::with_trashes(vec![trash.clone()])),
        &app,
    );
    eventually("the trash is read", || listed(&app).len() == 2);
    let mut names: Vec<(String, PathBuf)> = listed(&app);
    names.sort();
    assert_eq!(
        names,
        [
            ("a.txt".to_owned(), tmp.path().join("volume/docs/a.txt")),
            ("folder".to_owned(), tmp.path().join("volume/docs/folder")),
        ]
    );
    let empty = plan(&app, "trash.empty", &serde_json::json!({}));
    assert_eq!(empty.len(), 2);
    assert!(!empty.undoable(), "the shell asks before applying it");
    let r = app.executor.execute_blocking(&empty, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    for kept in ["files", "info"] {
        assert_eq!(
            std::fs::read_dir(trash.join(kept)).unwrap().count(),
            0,
            "{kept} is empty"
        );
    }
    eventually("the trash is read again", || listed(&app).is_empty());
    let shown = app.registry.places.find(|p| p.title == "Trash").unwrap();
    assert_eq!(shown.icon.as_deref(), Some("user-trash"));
    assert!(shown.menu.is_empty(), "nothing to empty");
    assert!(app.dispatch(&Invocation::new("trash.empty")).is_err());
    // Dropped on its place, files go in the trash.
    assert_eq!(
        place(true).drop.map(|d| d.name.to_string()).as_deref(),
        Some("trash.put")
    );
}
