//! Entries a view lends the pane (ADR-116): what Usage draws inside the
//! folder's own entries is shown while it is drawn, so it is selected and
//! acted on as the listing's, but it is in no order: Select all and the
//! filter are the listing's. What is no longer lent leaves the selection.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use skwir_core::command::Invocation;
use skwir_core::entry::{Entry, EntryKind, keys};
use skwir_core::workspace::{Loaned, Pane, PaneId};
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

/// The view that lends in these tests (Usage's id).
const VIEW: &str = "usage";

/// What a view lends of `path`: its identity as its folder's listing
/// names it, its kind, and nothing read yet, as Usage lends what its scan
/// found, seen as `seen`.
fn seen_as(path: &Path, kind: EntryKind, seen: u64) -> Loaned {
    let id = skwir_core::fs::stat_path(path).expect("there").id;
    let name = path.file_name().expect("named");
    Loaned {
        entry: Entry::new(id, name, kind),
        path: path.to_path_buf(),
        seen,
    }
}

fn lent(path: &Path, kind: EntryKind) -> Loaned {
    seen_as(path, kind, 0)
}

fn lend(app: &App, pane: PaneId, entries: Vec<Loaned>) -> u64 {
    app.workspace
        .write()
        .pane_mut(pane)
        .expect("pane")
        .lend(VIEW, entries)
}

fn hydrate(app: &App, ids: &[EntryId]) {
    app.dispatch(&Invocation::with_args(
        "hydrate",
        &serde_json::json!({ "ids": ids }),
    ))
    .expect("hydrate");
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

    let loan = lent(&inner, EntryKind::File);
    let id = loan.entry.id;
    let version = lend(&app, pane, vec![loan]);
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert!(p.is_shown(id) && p.is_lent(id));
        assert_eq!(p.lent_version(), version);
        assert_eq!(p.path_of(id).as_deref(), Some(inner.as_path()));
        assert!(p.entry(id).is_some_and(|e| e.kind == EntryKind::File));
        assert!(!p.order().contains(&id), "a lent entry is in no order");
        assert_eq!(
            p.entry_at(id.file, &inner).map(|e| e.id),
            Some(id),
            "found by its file and path, as a listed one"
        );
        assert_eq!(p.entry_at(id.file, &root.join("b.txt")), None);
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
    lend(&app, pane, vec![lent(&inner, EntryKind::File)]);
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

    let kept_loan = lent(&kept, EntryKind::File);
    let gone_loan = lent(&gone, EntryKind::File);
    let (kept_id, gone_id) = (kept_loan.entry.id, gone_loan.entry.id);
    lend(&app, pane, vec![kept_loan, gone_loan]);
    select_ids(&app, &[kept_id, gone_id], "replace");
    std::fs::remove_file(&gone).unwrap();

    hydrate(&app, &[kept_id, gone_id]);
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

/// Lent again as it was seen (a rescan elsewhere, another pane's scan
/// moving the plugin's answer), an entry keeps its reading and nothing
/// moves: lending what is lent already changes nothing. Seen otherwise,
/// what was read of it shows until it is read again, once selected.
#[test]
fn an_entry_lent_again_as_seen_keeps_its_reading_and_seen_otherwise_is_read_again() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let file = root.join("a folder").join("file.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"abc").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &root);

    let loan = seen_as(&file, EntryKind::File, 1);
    let id = loan.entry.id;
    let version = lend(&app, pane, vec![loan]);
    select_ids(&app, &[id], "replace");
    hydrate(&app, &[id]);
    wait(&app, pane, |p| {
        p.entry(id).is_some_and(|e| e.metadata.is_hydrated())
    });
    let selection = app
        .workspace
        .read()
        .pane(pane)
        .unwrap()
        .selection
        .revision();

    // Drawn again as it was: nothing to read, nothing moved.
    assert_eq!(
        lend(&app, pane, vec![seen_as(&file, EntryKind::File, 1)]),
        version
    );
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert!(!p.lent_to_read(id), "as it was seen, read already");
        assert_eq!(p.lent_version(), version);
        assert_eq!(p.selection.revision(), selection);
        assert!(p.entry(id).is_some_and(|e| e.metadata.is_hydrated()));
    }

    // Seen otherwise (it grew): what was read shows until it is read again.
    std::fs::write(&file, b"abcdef").unwrap();
    assert!(lend(&app, pane, vec![seen_as(&file, EntryKind::File, 2)]) > version);
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert!(p.entry(id).is_some_and(|e| e.metadata.is_hydrated()));
        assert!(p.lent_to_read(id), "seen otherwise, it is read again");
    }
    hydrate(&app, &[id]);
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

    let loan = lent(&file, EntryKind::File);
    let gone_loan = lent(&gone, EntryKind::File);
    let (id, gone_id) = (loan.entry.id, gone_loan.entry.id);
    let version = lend(&app, pane, vec![loan, gone_loan]);
    std::fs::remove_file(&gone).unwrap();
    hydrate(&app, &[id, gone_id]);
    wait(&app, pane, |p| !p.lent_to_read(id) && !p.is_lent(gone_id));
    let ws = app.workspace.read();
    let p = ws.pane(pane).unwrap();
    assert_eq!(p.lent_version(), version, "read, and one found gone");
}

/// What the pane keeps by entry follows a lent entry renamed, moved within
/// what the view draws, or whose name another file took (an editor
/// saving), as a listing's rows do; never to another name of a file of
/// several.
#[cfg(unix)]
#[test]
fn a_lent_entry_is_followed_through_a_rename_and_an_editor_saving() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let folder = root.join("l'été");
    let other = root.join("another folder");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let before = folder.join("draft one.txt");
    std::fs::write(&before, b"x").unwrap();
    std::fs::write(folder.join("beside.txt"), b"y").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &root);
    let selected = |app: &App| {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        (p.selection.iter().collect::<Vec<_>>(), p.selection.cursor)
    };
    let beside = lent(&folder.join("beside.txt"), EntryKind::File);
    let beside_id = beside.entry.id;
    let loan = lent(&before, EntryKind::File);
    let id = loan.entry.id;
    lend(&app, pane, vec![loan, beside.clone()]);
    select_ids(&app, &[id], "replace");

    // Renamed: the scan draws it under its new name.
    let after = folder.join("draft «two».txt");
    std::fs::rename(&before, &after).unwrap();
    let renamed = lent(&after, EntryKind::File);
    let renamed_id = renamed.entry.id;
    assert_ne!(renamed_id, id);
    lend(&app, pane, vec![renamed, beside.clone()]);
    assert_eq!(selected(&app), (vec![renamed_id], Some(renamed_id)));

    // Moved to another folder the view draws.
    let moved = other.join("draft «two».txt");
    std::fs::rename(&after, &moved).unwrap();
    let moved_loan = lent(&moved, EntryKind::File);
    let moved_id = moved_loan.entry.id;
    lend(&app, pane, vec![moved_loan, beside.clone()]);
    assert_eq!(selected(&app), (vec![moved_id], Some(moved_id)));

    // An editor saves: a new file renamed onto the name.
    let written = other.join(".draft.swp");
    std::fs::write(&written, b"saved").unwrap();
    std::fs::rename(&written, &moved).unwrap();
    let saved = lent(&moved, EntryKind::File);
    let saved_id = saved.entry.id;
    assert_ne!(saved_id.file, moved_id.file, "another file");
    lend(&app, pane, vec![saved, beside.clone()]);
    assert_eq!(selected(&app), (vec![saved_id], Some(saved_id)));

    // Two names of one file: one gone leaves nothing selected in its
    // place, though the other is lent.
    let link = other.join("same file.txt");
    std::fs::hard_link(&moved, &link).unwrap();
    let names = vec![lent(&moved, EntryKind::File), lent(&link, EntryKind::File)];
    lend(&app, pane, names);
    select_ids(&app, &[saved_id], "replace");
    std::fs::remove_file(&moved).unwrap();
    lend(&app, pane, vec![lent(&link, EntryKind::File), beside]);
    assert!(selected(&app).0.is_empty(), "never to another name");
    assert!(app.workspace.read().pane(pane).unwrap().is_lent(beside_id));
}

/// What a view lent goes when it draws it no more: another view (only
/// what it lent), or its plugin turned off (`App::unregister_owner`, the
/// view no longer registered).
#[test]
fn what_a_view_lent_goes_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let file = root.join("a folder").join("file.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"abc").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &root);
    let loan = lent(&file, EntryKind::File);
    let id = loan.entry.id;
    lend(&app, pane, vec![loan]);
    select_ids(&app, &[id], "replace");
    {
        let mut ws = app.workspace.write();
        let p = ws.pane_mut(pane).unwrap();
        assert_eq!(p.lent_by(), Some(VIEW));
        assert!(!p.let_go_of_lent("list"), "not what another view lent");
        assert!(p.is_lent(id));
        assert!(p.let_go_of_lent(VIEW));
        assert!(!p.has_lent() && p.lent_by().is_none());
        assert!(p.selection.is_empty(), "what it drew is no longer selected");
    }
    // A plugin turned off withdraws what its view lent with the rest of
    // its contributions; another plugin's leaves it be.
    let revision = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let view = || skwir_core::registry::ViewSpec::named(VIEW, "Usage", "usage", revision.clone());
    app.registry.views.add("a plugin", view());
    lend(&app, pane, vec![lent(&file, EntryKind::File)]);
    app.unregister_owner("another plugin");
    assert!(app.workspace.read().pane(pane).unwrap().has_lent());
    app.unregister_owner("a plugin");
    assert!(!app.workspace.read().pane(pane).unwrap().has_lent());
}
