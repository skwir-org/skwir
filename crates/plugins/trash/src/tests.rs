// Helpers of the tests that run on Linux alone (ADR-035).
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::entry::keys;
use skwir_core::location::Location;
use skwir_core::op::{EntryRef, Operation, Plan, Trash};

use super::{TrashPlugin, place};

/// On the home filesystem, as the user's files are: what is trashed
/// there goes to the home trash, which the tests' own XDG data home holds
/// (`tools/test/isolate-xdg.sh`).
fn base() -> tempfile::TempDir {
    let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
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
// The freedesktop trash.
#[cfg(target_os = "linux")]
#[test]
fn a_file_in_the_trash_is_listed_as_it_was_and_restored_where_it_was() {
    let tmp = base();
    let folder = tmp.path().join("it's a café");
    std::fs::create_dir(&folder).unwrap();
    let file = folder.join("notes [draft].txt");
    std::fs::write(&file, "x").unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
    let home = skwir_platform::trash::home_trash().unwrap();
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
    eventually("the place says the trash holds something", || {
        app.registry
            .places
            .find(|p| p.title == "Trash")
            .and_then(|p| p.icon)
            .as_deref()
            == Some("user-trash-full")
    });

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
                    .and_then(skwir_core::Value::as_str)
                    == Some(folder.to_str().unwrap())
            })
            .expect("listed in the trash");
        assert_eq!(entry.name, "notes [draft].txt");
        assert!(entry.metadata.value(keys::TRASHED).is_some());
        (entry.id, p.path_of(entry.id).unwrap())
    };
    assert!(physical.starts_with(home.join("files")), "{physical:?}");
    assert!(app.verbs(&Location::parse("trash:")).restore.is_some());
    // One place in the address, which holds the trashed folders: the
    // chevron after it lists them.
    let trail = app.trail(&Location::parse("trash:")).unwrap();
    assert_eq!(trail.crumbs[0].label, "Trash");
    assert!(trail.holds_places);

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
    // Its button in its pane's row empties it, run with that pane.
    let tools = app.verbs(&Location::parse("trash:")).tools;
    assert_eq!(
        tools
            .iter()
            .map(|t| (t.command.name.as_ref(), t.pane))
            .collect::<Vec<_>>(),
        [("trash.empty", skwir_core::ToolPane::Shown)]
    );
    let pane = app.workspace.read().active_id();
    let empty = plan(&app, "trash.empty", &serde_json::json!({ "pane": pane }));
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
    assert!(listed(&app).is_empty());
    eventually("the trash is read again", || {
        app.registry
            .places
            .find(|p| p.title == "Trash")
            .and_then(|p| p.icon)
            .as_deref()
            == Some("user-trash")
    });
    let shown = app.registry.places.find(|p| p.title == "Trash").unwrap();
    assert!(shown.menu.is_empty(), "nothing to empty");
    assert!(app.dispatch(&Invocation::new("trash.empty")).is_err());
    // Dropped on its place, files go in the trash: what its location
    // takes, which the sidebar's row takes too.
    assert_eq!(
        app.verbs(&place(true).location)
            .drop
            .map(|d| d.name.to_string())
            .as_deref(),
        Some("trash.put")
    );
}

/// Two records whose files are one (names of one file, hard links,
/// trashed apart) are two items, and emptying the trash deletes both;
/// one whose file has a name outside the trash stays on the disk, which
/// emptying says, its items read while the trash is listed (ADR-049).
#[cfg(unix)]
#[test]
fn emptying_the_trash_deletes_every_name_of_a_file_trashed_twice() {
    let tmp = base();
    let trash = tmp.path().join("volume/.Trash-1000");
    std::fs::create_dir_all(trash.join("files")).unwrap();
    std::fs::create_dir_all(trash.join("info")).unwrap();
    std::fs::write(trash.join("files/a.txt"), "one file").unwrap();
    std::fs::hard_link(trash.join("files/a.txt"), trash.join("files/b.txt")).unwrap();
    let outside = tmp.path().join("volume/kept.txt");
    std::fs::write(&outside, "kept").unwrap();
    std::fs::hard_link(&outside, trash.join("files/kept.txt")).unwrap();
    for name in ["a.txt", "b.txt", "kept.txt"] {
        std::fs::write(
            trash.join(format!("info/{name}.trashinfo")),
            format!("[Trash Info]\nPath=docs/{name}\nDeletionDate=2026-10-04T09:30:00\n"),
        )
        .unwrap();
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
    app.plugins.add(
        Arc::new(TrashPlugin::with_trashes(vec![trash.clone()])),
        &app,
    );
    eventually("all are listed", || listed(&app).len() == 3);
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::parse("trash:")).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    let empty = plan(&app, "trash.empty", &serde_json::json!({}));
    assert_eq!(empty.len(), 3);
    assert_eq!(
        empty.note.as_deref(),
        Some("1 file has names elsewhere: 4 B stays on the disk")
    );
    let r = app.executor.execute_blocking(&empty, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    for kept in ["files", "info"] {
        assert_eq!(
            std::fs::read_dir(trash.join(kept)).unwrap().count(),
            0,
            "{kept} is empty"
        );
    }
    assert_eq!(
        std::fs::read(&outside).unwrap(),
        b"kept",
        "its other name stays"
    );
}

/// What the trash holds is known by its files alone (the place's icon,
/// Empty Trash) until a listing shows it: its items are read whole then,
/// and let go of once nothing shows them.
#[test]
fn the_items_are_read_whole_only_while_a_listing_shows_them() {
    let tmp = base();
    let trash = volume_trash(&tmp.path().join("volume"));
    let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
    let plugin = Arc::new(TrashPlugin::with_trashes(vec![trash.clone()]));
    app.plugins.add(plugin.clone(), &app);
    let state = Arc::clone(&plugin.trashes.state);
    eventually("what it holds is read", || {
        state
            .lock()
            .held
            .as_ref()
            .is_some_and(|held| held.len() == 2)
    });
    assert!(state.lock().items.is_empty(), "no listing: no record read");
    assert_eq!(
        app.registry
            .places
            .find(|p| p.title == "Trash")
            .and_then(|p| p.icon),
        Some("user-trash-full".into())
    );
    assert_eq!(plan(&app, "trash.empty", &serde_json::json!({})).len(), 2);

    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::parse("trash:")).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    assert_eq!(app.workspace.read().pane(pane).unwrap().listing.len(), 2);
    assert_eq!(state.lock().items.len(), 2);
    // A file trashed meanwhile is read, the others kept as read.
    std::fs::write(trash.join("files/c.txt"), "c").unwrap();
    std::fs::write(
        trash.join("info/c.txt.trashinfo"),
        "[Trash Info]\nPath=docs/c.txt\nDeletionDate=2026-10-04T09:31:00\n",
    )
    .unwrap();
    let kept = Arc::clone(&state.lock().items[0]);
    eventually("the new file is listed", || {
        app.pump();
        app.workspace.read().pane(pane).unwrap().listing.len() == 3
    });
    assert!(
        state
            .lock()
            .items
            .iter()
            .any(|item| Arc::ptr_eq(item, &kept)),
        "not read again"
    );

    app.navigate(pane, &Location::from_path(tmp.path()))
        .unwrap();
    // The view keeps nothing of the location left.
    app.keep_locations(pane, &[]);
    eventually("let go of once nothing shows them", || {
        app.pump();
        state.lock().items.is_empty()
    });
    assert_eq!(state.lock().held.as_ref().map(Vec::len), Some(3));
}

/// A trashed folder opens in the trash, `Trash › folder`, where a file is
/// restored alone, its folders made again, and undone goes back; the rest
/// of the folder, restored after, merges into them.
// The freedesktop trash.
#[cfg(target_os = "linux")]
#[test]
fn a_trashed_folder_is_browsed_in_the_trash_and_restored_a_file_at_a_time() {
    let tmp = base();
    let volume = tmp.path().join("volume");
    let trash = volume_trash(&volume);
    std::fs::write(trash.join("files/folder/c.txt"), "c").unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
    app.plugins.add(
        Arc::new(TrashPlugin::with_trashes(vec![trash.clone()])),
        &app,
    );
    eventually("the trash is read", || listed(&app).len() == 2);
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::parse("trash:")).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    let inside = {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        let folder = p.listing.iter().find(|e| e.name == "folder").unwrap().id;
        p.location_of(folder)
            .expect("a trashed folder stays in the trash")
    };
    let trail = app.trail(&inside).unwrap();
    let labels: Vec<&str> = trail.crumbs.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Trash", "folder"]);
    assert!(trail.one_folder);
    assert!(app.verbs(&inside).restore.is_some());

    app.navigate(pane, &inside).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    let b = {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        let mut names: Vec<String> = p.listing.iter().map(|e| e.name_str().into()).collect();
        names.sort();
        assert_eq!(names, ["b.txt", "c.txt"]);
        p.listing.iter().find(|e| e.name == "b.txt").unwrap().id
    };
    // In the trash already: not put there again.
    let again = app.dispatch(&Invocation::with_args(
        "trash.put",
        &serde_json::json!({ "pane": pane }),
    ));
    assert!(again.is_err(), "nothing selected, or in the trash");

    let restore = plan(
        &app,
        "trash.restore",
        &serde_json::json!({ "pane": pane, "ids": [b] }),
    );
    let r = app.executor.execute_blocking(&restore, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    let back = volume.join("docs/folder");
    assert_eq!(std::fs::read_to_string(back.join("b.txt")).unwrap(), "b");
    assert!(trash.join("files/folder/c.txt").exists(), "the rest stays");
    assert!(trash.join("info/folder.trashinfo").exists());
    // Undone, it is in the trashed folder again, and the folders made go.
    let r = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(trash.join("files/folder/b.txt").exists());
    assert!(!volume.join("docs").exists());
    let r = app.executor.redo_blocking(app.lifetime()).unwrap();
    assert!(r.error.is_none(), "{:?}", r.error);

    // The folder itself, restored after, merges into what came back.
    eventually("the trash is read again", || listed(&app).len() == 2);
    app.navigate(pane, &Location::parse("trash:")).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    let folder = {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        p.listing.iter().find(|e| e.name == "folder").unwrap().id
    };
    let whole = plan(
        &app,
        "trash.restore",
        &serde_json::json!({ "pane": pane, "ids": [folder] }),
    );
    let r = app.executor.execute_blocking(&whole, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(back.join("b.txt").exists() && back.join("c.txt").exists());
    assert!(!trash.join("files/folder").exists());
    assert!(!trash.join("info/folder.trashinfo").exists());
}
