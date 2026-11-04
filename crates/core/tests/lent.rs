//! Entries a view lends the pane (ADR-116): what Usage draws inside the
//! folder's own entries is shown while it is drawn, so it is selected and
//! acted on as the listing's, but it is in no order: Select all and the
//! filter are the listing's. What is no longer lent leaves the selection.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::*;
use skwir_core::command::Invocation;
use skwir_core::entry::{Entry, EntryKind, keys};
use skwir_core::workspace::{Pane, PaneId};
use skwir_core::{App, EntryId};

fn wait(app: &App, pane: PaneId, done: impl Fn(&Pane) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.pump();
        if done(app.workspace.read().pane(pane).expect("pane")) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What a view lends of `path`: its identity as its folder's listing
/// names it, its kind, and nothing read yet, as Usage lends what its scan
/// found.
fn lent(path: &Path, kind: EntryKind) -> (Entry, PathBuf) {
    let id = skwir_core::fs::stat_path(path).expect("there").id;
    let name = path.file_name().expect("named");
    (Entry::new(id, name, kind), path.to_path_buf())
}

fn lend(app: &App, pane: PaneId, entries: Vec<(Entry, PathBuf)>) -> u64 {
    app.workspace
        .write()
        .pane_mut(pane)
        .expect("pane")
        .lend(entries)
}

fn select_ids(app: &App, ids: &[EntryId], mode: &str) {
    app.dispatch(&Invocation::with_args(
        "select",
        &serde_json::json!({ "ids": ids, "mode": mode }),
    ))
    .expect("select");
}

fn selected(app: &App, pane: PaneId) -> Vec<EntryId> {
    let ws = app.workspace.read();
    ws.pane(pane).expect("pane").selection.iter().collect()
}

#[test]
fn a_lent_entry_is_selected_and_acted_on_but_in_no_order() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("l'été 2024");
    let inner = root.join("a folder").join("deep ünïcode.txt");
    std::fs::create_dir_all(inner.parent().unwrap()).unwrap();
    std::fs::write(&inner, b"x").unwrap();
    std::fs::write(root.join("b.txt"), b"x").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &root);

    let (entry, path) = lent(&inner, EntryKind::File);
    let id = entry.id;
    let version = lend(&app, pane, vec![(entry, path)]);
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert!(p.is_shown(id) && p.is_lent(id));
        assert_eq!(p.lent_version(), version);
        assert_eq!(p.path_of(id).as_deref(), Some(inner.as_path()));
        assert!(p.entry(id).is_some_and(|e| e.kind == EntryKind::File));
        assert!(!p.order().contains(&id), "a lent entry is in no order");
    }

    // Selected, alone and beside one of the listing's.
    select_ids(&app, &[id], "replace");
    assert_eq!(selected(&app, pane), [id]);
    select(&app, &["b.txt"]);
    select_ids(&app, &[id], "toggle");
    let refs = app.targets(None, Vec::new()).expect("targets");
    let paths: Vec<&Path> = refs.iter().map(|r| r.path.as_path()).collect();
    assert_eq!(paths, [root.join("b.txt").as_path(), inner.as_path()]);
    // A range to it has no order to follow: it is added.
    select(&app, &["b.txt"]);
    select_ids(&app, &[id], "range_to");
    assert_eq!(selected(&app, pane).len(), 2);

    // Select all is the folder's own entries.
    app.dispatch(&Invocation::new("select_all"))
        .expect("select all");
    assert!(!selected(&app, pane).contains(&id));

    // No longer lent, it is no longer selected.
    select_ids(&app, &[id], "replace");
    lend(&app, pane, Vec::new());
    assert!(selected(&app, pane).is_empty());
    assert!(!app.workspace.read().pane(pane).unwrap().is_shown(id));

    // Another folder shown lets go of what was lent.
    let (entry, path) = lent(&inner, EntryKind::File);
    lend(&app, pane, vec![(entry, path)]);
    navigate(&app, inner.parent().unwrap());
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    assert!(!p.is_lent(id) && p.lent_version() > version);
}

#[test]
fn a_lent_entry_is_read_by_its_path_and_goes_when_its_file_does() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let folder = root.join("a folder");
    std::fs::create_dir_all(&folder).unwrap();
    let kept = folder.join("kept.txt");
    let gone = folder.join("gone.txt");
    std::fs::write(&kept, b"twelve bytes").unwrap();
    std::fs::write(&gone, b"x").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &root);

    let (kept_entry, kept_path) = lent(&kept, EntryKind::File);
    let (gone_entry, gone_path) = lent(&gone, EntryKind::File);
    let (kept_id, gone_id) = (kept_entry.id, gone_entry.id);
    lend(
        &app,
        pane,
        vec![(kept_entry, kept_path), (gone_entry, gone_path)],
    );
    select_ids(&app, &[kept_id, gone_id], "replace");
    std::fs::remove_file(&gone).unwrap();

    app.dispatch(&Invocation::with_args(
        "hydrate",
        &serde_json::json!({ "ids": [kept_id, gone_id] }),
    ))
    .expect("hydrate");
    wait(&app, pane, |p| {
        p.entry(kept_id).is_some_and(|e| e.metadata.is_hydrated()) && !p.is_lent(gone_id)
    });
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    let size = p
        .entry(kept_id)
        .and_then(|e| e.metadata.value(keys::SIZE))
        .and_then(skwir_core::entry::Value::as_u64);
    assert_eq!(size, Some(12));
    assert_eq!(
        p.selection.iter().collect::<Vec<_>>(),
        [kept_id],
        "what went is no longer selected"
    );
}

#[test]
fn an_entry_lent_again_keeps_what_was_read_of_it_and_is_read_again() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let file = root.join("a folder").join("file.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"abc").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &root);

    let (entry, path) = lent(&file, EntryKind::File);
    let id = entry.id;
    lend(&app, pane, vec![(entry, path)]);
    app.dispatch(&Invocation::with_args(
        "hydrate",
        &serde_json::json!({ "ids": [id] }),
    ))
    .expect("hydrate");
    wait(&app, pane, |p| {
        p.entry(id).is_some_and(|e| e.metadata.is_hydrated())
    });
    // The view draws again (another level, a rescan): the same entry,
    // what was read of it shown until it is read again.
    assert!(!app.workspace.read().pane(pane).unwrap().lent_to_read(id));
    lend(&app, pane, vec![lent(&file, EntryKind::File)]);
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert!(p.entry(id).is_some_and(|e| e.metadata.is_hydrated()));
        assert!(p.lent_to_read(id), "drawn again, it may have changed");
    }
    std::fs::write(&file, b"abcdef").unwrap();
    app.dispatch(&Invocation::with_args(
        "hydrate",
        &serde_json::json!({ "ids": [id] }),
    ))
    .expect("hydrate");
    wait(&app, pane, |p| !p.lent_to_read(id));
    let ws = app.workspace.read();
    let size = ws
        .pane(pane)
        .unwrap()
        .entry(id)
        .and_then(|e| e.metadata.value(keys::SIZE))
        .and_then(skwir_core::entry::Value::as_u64);
    assert_eq!(size, Some(6), "read again as it is now");
}

/// Reading what was lent never moves the version a view lends again by:
/// a view lends again when it moves, and lending again asks for a
/// reading, so a reading that moved it read the selection forever (nine
/// cores busy while an entry inside a folder was selected in Usage).
#[test]
fn reading_a_lent_entry_never_asks_the_view_to_lend_again() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let folder = root.join("a folder");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("file.txt");
    let gone = folder.join("gone.txt");
    std::fs::write(&file, b"abc").unwrap();
    std::fs::write(&gone, b"x").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &root);

    let (entry, path) = lent(&file, EntryKind::File);
    let (gone_entry, gone_path) = lent(&gone, EntryKind::File);
    let (id, gone_id) = (entry.id, gone_entry.id);
    let version = lend(&app, pane, vec![(entry, path), (gone_entry, gone_path)]);
    std::fs::remove_file(&gone).unwrap();
    app.dispatch(&Invocation::with_args(
        "hydrate",
        &serde_json::json!({ "ids": [id, gone_id] }),
    ))
    .expect("hydrate");
    wait(&app, pane, |p| !p.lent_to_read(id) && !p.is_lent(gone_id));
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    assert_eq!(p.lent_version(), version, "read, and one found gone");
}
