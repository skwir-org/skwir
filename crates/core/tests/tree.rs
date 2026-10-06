//! Folders opened in place (the tree view): their entries are the pane's
//! like the shown folder's, in the order drawn, selectable, and resolved
//! to where they are for every command.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use bibli_core::command::Invocation;
use bibli_core::workspace::{Pane, PaneId};
use bibli_core::{App, EntryId, Location};
use common::*;

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

/// The names in the order the pane shows them, indented by depth.
fn drawn(app: &App, pane: PaneId) -> Vec<String> {
    let ws = app.workspace.read();
    let p = ws.pane(pane).expect("pane");
    p.order()
        .iter()
        .map(|id| {
            let e = p.entry(*id).expect("every id in the order is an entry");
            format!("{}{}", "  ".repeat(p.depth(*id)), e.name_str())
        })
        .collect()
}

fn id(app: &App, pane: PaneId, dir: &Path, name: &str) -> EntryId {
    let ws = app.workspace.read();
    let p = ws.pane(pane).expect("pane");
    let listing = if Some(dir) == p.location().and_then(Location::path).as_deref() {
        &p.listing
    } else {
        p.kept(&Location::from_path(dir)).expect("opened")
    };
    listing.find_by_name(name.as_ref()).expect("listed").id
}

fn expand(app: &App, ids: &[EntryId], mode: &str) {
    app.dispatch(&Invocation::with_args(
        "expand",
        &serde_json::json!({ "ids": ids, "mode": mode }),
    ))
    .expect("expand");
}

fn opened_listing_complete(dir: &Path) -> impl Fn(&Pane) -> bool + '_ {
    move |p| {
        p.kept(&Location::from_path(dir))
            .is_some_and(bibli_core::Listing::is_complete)
    }
}

#[test]
fn opened_folders_are_drawn_selected_and_resolved_like_the_shown_one() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("l'été 2024");
    let a = root.join("a folder");
    let inner = a.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(a.join("a file.txt"), b"x").unwrap();
    std::fs::write(inner.join("deep.txt"), b"x").unwrap();
    std::fs::create_dir(root.join("b")).unwrap();
    std::fs::write(root.join("c.txt"), b"x").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &root);
    assert_eq!(drawn(&app, pane), ["a folder", "b", "c.txt"]);

    expand(&app, &[id(&app, pane, &root, "a folder")], "toggle");
    wait(&app, pane, opened_listing_complete(&a));
    assert_eq!(
        drawn(&app, pane),
        ["a folder", "  inner", "  a file.txt", "b", "c.txt"]
    );
    // A folder in an opened one opens in place too.
    expand(&app, &[id(&app, pane, &a, "inner")], "expand");
    wait(&app, pane, opened_listing_complete(&inner));
    assert_eq!(
        drawn(&app, pane),
        [
            "a folder",
            "  inner",
            "    deep.txt",
            "  a file.txt",
            "b",
            "c.txt"
        ]
    );

    // A range crosses the levels as drawn, and every entry resolves.
    let deep = id(&app, pane, &inner, "deep.txt");
    let b = id(&app, pane, &root, "b");
    app.dispatch(&Invocation::with_args(
        "select",
        &serde_json::json!({ "ids": [deep], "mode": "replace" }),
    ))
    .unwrap();
    app.dispatch(&Invocation::with_args(
        "select",
        &serde_json::json!({ "ids": [b], "mode": "range_to" }),
    ))
    .unwrap();
    let mut paths: Vec<_> = app
        .selected_refs(pane)
        .into_iter()
        .map(|r| r.path)
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        [a.join("a file.txt"), inner.join("deep.txt"), root.join("b")]
    );

    // A file that appears in an opened folder takes its place there.
    std::fs::write(a.join("0 new.txt"), b"x").unwrap();
    wait(&app, pane, |p| p.order().len() == 7);
    assert_eq!(drawn(&app, pane)[1], "  inner");
    assert_eq!(drawn(&app, pane)[3], "  0 new.txt");

    // Closing a folder closes what was opened inside it, and the selection
    // lets go of what it no longer shows.
    expand(&app, &[id(&app, pane, &root, "a folder")], "collapse");
    assert_eq!(drawn(&app, pane), ["a folder", "b", "c.txt"]);
    assert!(
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .expanded()
            .is_empty()
    );
    let refs = app.selected_refs(pane);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, root.join("b"));

    // Navigating forgets them.
    expand(&app, &[id(&app, pane, &root, "a folder")], "expand");
    navigate(&app, &a);
    assert!(
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .expanded()
            .is_empty()
    );
}

#[test]
fn a_folder_opens_in_place_only_under_the_one_shown() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("file.txt"), b"x").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &root);
    // A file is not a folder: nothing opens.
    let file = id(&app, pane, &root, "file.txt");
    assert!(
        app.dispatch(&Invocation::with_args(
            "expand",
            &serde_json::json!({ "ids": [file] }),
        ))
        .is_err()
    );
    expand(&app, &[id(&app, pane, &root, "sub")], "expand");
    wait(&app, pane, opened_listing_complete(&root.join("sub")));
    app.dispatch(&Invocation::with_args(
        "expand",
        &serde_json::json!({ "mode": "collapse_all" }),
    ))
    .unwrap();
    assert_eq!(drawn(&app, pane), ["sub", "file.txt"]);
}

/// What the process holds open under `dir`: what makes the kernel refuse
/// to unmount a volume. Read in `/proc`: Linux's.
#[cfg(target_os = "linux")]
fn held_under(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|fd| std::fs::read_link(fd.ok()?.path()).ok())
        .filter(|target| target.starts_with(dir))
        .collect()
}

/// Before a volume is unmounted, bibli lets go of it: a pane showing a
/// folder there goes elsewhere, and the folders it opened in place or kept
/// there close, until nothing of the process holds the volume.
#[cfg(target_os = "linux")]
#[test]
fn released_folders_hold_nothing_open() {
    let tmp = tempfile::tempdir().unwrap();
    let volume = tmp.path().join("USB stick");
    let inner = volume.join("it's inside");
    let elsewhere = tmp.path().join("home");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::create_dir(&elsewhere).unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &volume);
    expand(&app, &[id(&app, pane, &volume, "it's inside")], "expand");
    wait(&app, pane, opened_listing_complete(&inner));
    assert!(
        !held_under(&volume).is_empty(),
        "a listing holds its folder"
    );

    app.release(&volume, &Location::from_path(&elsewhere))
        .unwrap();
    wait(&app, pane, |p| {
        p.listing.is_complete() && p.location().and_then(Location::path) == Some(elsewhere.clone())
    });
    {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert!(p.expanded().is_empty());
        assert!(!p.keeps_any(), "the folder left is not kept");
    }
    // Listings close their folders on a worker.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !held_under(&volume).is_empty() {
        assert!(
            Instant::now() < deadline,
            "still held: {:?}",
            held_under(&volume)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
