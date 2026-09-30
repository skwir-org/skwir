//! The path provider streams, never blocks the caller, and follows changes.

mod common;

use std::time::{Duration, Instant};

use bibli_core::location::{ListingEvent, Location};
use bibli_core::scheduler::Priority;
use common::*;

const N: usize = 20_000;

#[test]
fn listing_streams_without_blocking_the_caller() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("big");
    std::fs::create_dir(&dir).unwrap();
    for i in 0..N {
        std::fs::File::create(dir.join(format!("f{i:05}"))).unwrap();
    }
    let app = app(tmp.path());

    let t0 = Instant::now();
    let open = app.open_location(&Location::from_path(&dir)).unwrap();
    let open_took = t0.elapsed();
    let first = open
        .events()
        .recv_timeout(Duration::from_secs(5))
        .expect("first event");
    let first_took = t0.elapsed();
    let ListingEvent::Added(batch) = first else {
        panic!("expected Added, got {first:?}")
    };
    assert!(!batch.is_empty() && batch.len() <= 1024);

    let mut listing = bibli_core::Listing::default();
    listing.apply(ListingEvent::Added(batch));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !listing.is_complete() {
        let ev = open
            .events()
            .recv_deadline(deadline)
            .expect("listing completes");
        listing.apply(ev);
    }
    assert_eq!(listing.len(), N);

    // Generous bounds: CI runners are slow. Locally open is ~100us and the first batch ~1ms.
    assert!(
        open_took < Duration::from_millis(50),
        "open blocked for {open_took:?}"
    );
    assert!(
        first_took < Duration::from_millis(200),
        "first batch took {first_took:?}"
    );

    // Lazy stat: nothing is hydrated until asked; asking hydrates exactly those.
    assert!(listing.iter().all(|e| !e.metadata.is_hydrated()));
    let ids: Vec<_> = listing.ids().take(3).collect();
    open.hydrate(&ids, Priority::Visible);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ev = open
            .events()
            .recv_deadline(deadline)
            .expect("hydrate result");
        listing.apply(ev);
        if ids
            .iter()
            .all(|id| listing.get(*id).unwrap().metadata.is_hydrated())
        {
            break;
        }
    }
    assert_eq!(
        listing.iter().filter(|e| e.metadata.is_hydrated()).count(),
        3
    );
}

#[test]
fn watcher_reports_create_rename_and_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("w");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("one"), b"1").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &dir);

    std::fs::write(dir.join("two"), b"2").unwrap();
    wait(&app, pane, |l| {
        l.len() == 2 && l.find_by_name("two".as_ref()).is_some()
    });

    let id_two = app
        .workspace
        .read()
        .pane(pane)
        .unwrap()
        .listing
        .find_by_name("two".as_ref())
        .unwrap()
        .id;
    std::fs::rename(dir.join("two"), dir.join("deux")).unwrap();
    wait(&app, pane, |l| {
        l.find_by_name("deux".as_ref()).is_some() && l.find_by_name("two".as_ref()).is_none()
    });
    assert_eq!(
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .listing
            .find_by_name("deux".as_ref())
            .unwrap()
            .id,
        id_two,
        "identity survives a rename"
    );

    std::fs::remove_file(dir.join("deux")).unwrap();
    wait(&app, pane, |l| l.len() == 1);
}

fn wait(
    app: &bibli_core::App,
    pane: bibli_core::workspace::PaneId,
    pred: impl Fn(&bibli_core::Listing) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.pump();
        if pred(&app.workspace.read().pane(pane).unwrap().listing) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "condition not met before timeout"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn pending_opens_report_errors_asynchronously_and_can_be_replaced() {
    use bibli_core::{App, app::AppConfig, cancel::CancelToken, command::Invocation};
    let tmp = tempfile::tempdir().unwrap();
    let config = AppConfig {
        workers: 1,
        ..AppConfig::ephemeral(tmp.path())
    };
    let app = App::new(config).unwrap();
    let pane = active(&app);
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    app.scheduler.spawn(
        Priority::Visible,
        "test.gate",
        CancelToken::new(),
        move || {
            entered.send(()).unwrap();
            let _ = blocked.recv_timeout(Duration::from_secs(10));
        },
    );
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let missing = tmp.path().join("Sleeping drive's missing folder");
    app.dispatch(&Invocation::with_args(
        "navigate",
        &serde_json::json!({"location":missing}),
    ))
    .unwrap();
    let first = app
        .workspace
        .read()
        .pane(pane)
        .unwrap()
        .open
        .clone()
        .unwrap();
    let lifetime = app.workspace.read().pane(pane).unwrap().location_lifetime();
    assert!(
        !app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .listing
            .is_complete()
    );
    assert!(
        first.events().is_empty(),
        "opening must wait on the worker, not the caller"
    );
    app.dispatch(&Invocation::with_args(
        "navigate",
        &serde_json::json!({"location":tmp.path()}),
    ))
    .unwrap();
    assert!(lifetime.is_cancelled());
    // The open left waits for the view to keep it or let it go; the UI's
    // next sync tells it, and a view that keeps nothing lets it go.
    app.keep_locations(pane, &[]);
    release.send(()).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    assert!(
        first.events().is_empty(),
        "cancelled open must not publish late events"
    );
    app.dispatch(&Invocation::new("back")).unwrap();
    wait(&app, pane, |listing| listing.error().is_some());
    assert_eq!(
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .location()
            .unwrap()
            .path(),
        Some(missing.clone())
    );
    std::fs::create_dir(&missing).unwrap();
    std::fs::write(missing.join("Awake.txt"), b"example").unwrap();
    app.dispatch(&Invocation::new("refresh")).unwrap();
    wait(&app, pane, |listing| {
        listing.is_complete() && listing.find_by_name("Awake.txt".as_ref()).is_some()
    });
    assert!(
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .listing
            .error()
            .is_none()
    );
}

#[test]
fn forward_returns_where_back_came_from_and_a_new_navigation_forgets_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (first, second, third) = (
        tmp.path().join("first folder"),
        tmp.path().join("second folder"),
        tmp.path().join("third folder"),
    );
    for dir in [&first, &second, &third] {
        std::fs::create_dir(dir).unwrap();
    }
    let app = app(tmp.path());
    let pane = app.workspace.read().active_id();
    let at = |app: &bibli_core::App| {
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .location()
            .unwrap()
            .path()
    };
    let go = |path: &std::path::Path| {
        app.dispatch(&bibli_core::command::Invocation::with_args(
            "navigate",
            &serde_json::json!({ "location": path }),
        ))
        .unwrap();
    };
    go(&first);
    go(&second);
    app.dispatch(&bibli_core::command::Invocation::new("forward"))
        .unwrap();
    assert_eq!(
        at(&app),
        Some(second.clone()),
        "nothing ahead: forward stays"
    );
    app.dispatch(&bibli_core::command::Invocation::new("back"))
        .unwrap();
    assert_eq!(at(&app), Some(first.clone()));
    app.dispatch(&bibli_core::command::Invocation::new("forward"))
        .unwrap();
    assert_eq!(at(&app), Some(second.clone()));
    app.dispatch(&bibli_core::command::Invocation::new("back"))
        .unwrap();
    go(&third);
    app.dispatch(&bibli_core::command::Invocation::new("forward"))
        .unwrap();
    assert_eq!(
        at(&app),
        Some(third),
        "navigating drops the forward history"
    );
    // A history menu goes through several at once, and never past the ends:
    // first, third, second.
    go(&second);
    let steps = |name: &str, steps: usize| {
        app.dispatch(&bibli_core::command::Invocation::with_args(
            name,
            &serde_json::json!({ "steps": steps }),
        ))
        .unwrap();
    };
    steps("back", 2);
    assert_eq!(at(&app), Some(first.clone()));
    steps("back", 5);
    assert_eq!(at(&app), Some(first), "nothing that far back");
    steps("forward", 2);
    assert_eq!(at(&app), Some(second));
}

fn entry_id(
    app: &bibli_core::App,
    pane: bibli_core::workspace::PaneId,
    name: &str,
) -> bibli_core::EntryId {
    app.workspace
        .read()
        .pane(pane)
        .unwrap()
        .listing
        .find_by_name(name.as_ref())
        .unwrap()
        .id
}

#[test]
fn a_refresh_keeps_metadata_and_restats() {
    use bibli_core::command::Invocation;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("r");
    std::fs::create_dir(&dir).unwrap();
    for name in ["a", "b", "c"] {
        std::fs::write(dir.join(name), name).unwrap();
    }
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &dir);
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
    wait(&app, pane, |l| l.iter().all(|e| e.metadata.is_hydrated()));

    std::fs::write(dir.join("a"), b"grown").unwrap();
    app.dispatch(&Invocation::new("refresh")).unwrap();
    let a = entry_id(&app, pane, "a");
    let size = |l: &bibli_core::Listing| {
        l.get(a)
            .and_then(|e| e.metadata.value(bibli_core::entry::keys::SIZE))
            .and_then(bibli_core::Value::as_u64)
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.pump();
        let ws = app.workspace.read();
        let listing = &ws.pane(pane).unwrap().listing;
        assert!(
            listing.iter().all(|e| e.metadata.is_hydrated()),
            "a refresh never blanks what was known"
        );
        if size(listing) == Some(5) {
            break;
        }
        drop(ws);
        assert!(Instant::now() < deadline, "the refresh did not re-stat");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_listing_follows_its_folder_when_it_moves_and_says_when_it_is_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("before");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("f"), b"1").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &dir);
    let id = entry_id(&app, pane, "f");
    let path_of = || {
        let ws = app.workspace.read();
        ws.pane(pane).unwrap().open.as_ref().unwrap().path_of(id)
    };
    assert_eq!(path_of(), Some(dir.join("f")));

    let moved = tmp.path().join("after");
    std::fs::rename(&dir, &moved).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while path_of() != Some(moved.join("f")) {
        assert!(
            Instant::now() < deadline,
            "path_of still says {:?}",
            path_of()
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    std::fs::remove_dir_all(&moved).unwrap();
    wait(&app, pane, |l| l.is_empty() && l.error().is_some());
}

#[cfg(unix)]
#[test]
fn folders_whose_names_are_not_utf8_can_be_reached() {
    use bibli_core::command::Invocation;
    use bibli_core::os_path::PathArg;
    use std::os::unix::ffi::OsStrExt;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
    let inner = dir.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    let navigate = Invocation::with_args(
        "navigate",
        &serde_json::json!({ "location": PathArg::from(inner.as_path()) }),
    );
    app.dispatch(&navigate).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    app.dispatch(&Invocation::new("up")).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    assert_eq!(app.pane_dir(pane).unwrap(), dir);
    assert!(
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .listing
            .find_by_name("inner".as_ref())
            .is_some()
    );
}

#[test]
fn virtual_locations_hydrate_by_path_in_one_batch() {
    use bibli_core::location::OpenContext;
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("sub"));
    std::fs::write(&a, b"123").unwrap();
    std::fs::create_dir(&b).unwrap();
    let app = app(tmp.path());
    let ctx = OpenContext {
        scheduler: app.scheduler.clone(),
        cancel: app.lifetime().child(),
        batch_size: 64,
        waker: None,
        metadata: std::sync::Arc::clone(&app.metadata),
        fields: std::sync::Arc::clone(&app.fields),
    };
    let (sink, rx) = ctx.channel();
    let id = |ino| bibli_core::EntryId { dev: 0, ino };
    let gone = bibli_core::fs::hydrate_paths(
        &[
            (id(1), a.clone()),
            (id(2), b.clone()),
            (id(3), tmp.path().join("missing")),
        ],
        &app.metadata,
        &sink,
        &ctx.cancel,
    );
    assert_eq!(gone, [id(3)]);
    let ListingEvent::Updated(found) = rx.try_recv().unwrap() else {
        panic!("expected one Updated first")
    };
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|e| e.metadata.is_hydrated()));
    assert!(found.iter().any(|e| e.id == id(2) && e.is_dir()));
    assert!(matches!(rx.try_recv().unwrap(), ListingEvent::Removed(ids) if ids == [id(3)]));
}

/// A location the pane keeps beside the one it shows (a column of the
/// columns view) is adopted when the pane goes there: its listing, complete
/// and the same, rather than the folder listed again. The folder left
/// becomes kept when the view wants it, and goes when it does not.
#[test]
fn going_up_adopts_the_kept_listing_and_going_down_keeps_the_one_left() {
    let tmp = tempfile::tempdir().unwrap();
    let parent = tmp.path().join("parent");
    let child = parent.join("child");
    std::fs::create_dir_all(&child).unwrap();
    for i in 0..50 {
        std::fs::File::create(parent.join(format!("f{i:02}"))).unwrap();
    }
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &parent);
    let listed = app.workspace.read().pane(pane).unwrap().listing.id();

    // Down: the view keeps the folder left, as a column.
    navigate(&app, &child);
    let above = Location::from_path(&parent);
    app.keep_locations(pane, std::slice::from_ref(&above));
    {
        let ws = app.workspace.read();
        let kept = ws.pane(pane).unwrap().kept(&above).expect("kept");
        assert_eq!(kept.id(), listed, "the pane's own listing, not a new one");
        assert!(kept.is_complete());
    }

    // Up: adopted as it is.
    app.navigate(pane, &above).unwrap();
    {
        let ws = app.workspace.read();
        let shown = &ws.pane(pane).unwrap().listing;
        assert_eq!(shown.id(), listed);
        assert!(shown.is_complete());
        assert_eq!(shown.len(), 51);
    }

    // A view that keeps nothing lets the folder left go: going down again
    // lists it anew.
    app.keep_locations(pane, &[]);
    navigate(&app, &child);
    app.keep_locations(pane, &[]);
    app.navigate(pane, &above).unwrap();
    assert_ne!(
        app.workspace.read().pane(pane).unwrap().listing.id(),
        listed
    );
}

#[test]
fn a_filter_shows_only_names_holding_it_until_the_pane_shows_another_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("it's a folder");
    std::fs::create_dir(&dir).unwrap();
    for name in [
        "Report 2026.pdf",
        "it's a report.txt",
        "notes.md",
        "REPORTS",
    ] {
        std::fs::write(dir.join(name), b"example").unwrap();
    }
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &dir);
    let shown = |app: &bibli_core::App| -> Vec<String> {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        let mut names: Vec<String> = p
            .order()
            .iter()
            .map(|id| p.entry(*id).unwrap().name.to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let filter = |text: Option<&str>| {
        app.dispatch(&bibli_core::command::Invocation::with_args(
            "filter",
            &serde_json::json!({ "text": text }),
        ))
        .unwrap();
    };
    select(&app, &["notes.md"]);
    filter(Some("rep"));
    // Case ignored for a filter in lower case; what is no longer shown is
    // no longer selected.
    assert_eq!(
        shown(&app),
        ["REPORTS", "Report 2026.pdf", "it's a report.txt"]
    );
    assert!(
        app.workspace
            .read()
            .pane(pane)
            .unwrap()
            .selection
            .is_empty()
    );
    // Select all takes what is shown, nothing hidden by the filter.
    app.dispatch(&bibli_core::command::Invocation::new("select_all"))
        .unwrap();
    assert_eq!(app.workspace.read().pane(pane).unwrap().selection.len(), 3);
    // A capital asks for that case.
    filter(Some("Rep"));
    assert_eq!(shown(&app), ["Report 2026.pdf"]);
    filter(None);
    assert_eq!(shown(&app).len(), 4);
    // Another folder shows everything again.
    filter(Some("rep"));
    navigate(&app, tmp.path());
    let ws = app.workspace.read();
    assert_eq!(ws.pane(pane).unwrap().filter(), None);
}

#[test]
fn a_link_is_listed_as_what_it_leads_to() {
    use bibli_core::entry::{EntryKind, Value, keys};
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("links");
    std::fs::create_dir_all(dir.join("real folder")).unwrap();
    std::fs::write(dir.join("a note's text.txt"), "hello, some words").unwrap();
    let link = |target: &str, name: &str| std::os::unix::fs::symlink(target, dir.join(name));
    link("real folder", "a folder link").unwrap();
    link("a note's text.txt", "a file link").unwrap();
    // A chain: the type is the last link's target's.
    link("a file link", "a link to a link").unwrap();
    link("nowhere", "a broken link").unwrap();
    let app = app(tmp.path());

    let open = app.open_location(&Location::from_path(&dir)).unwrap();
    let mut listing = bibli_core::Listing::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !listing.is_complete() {
        listing.apply(open.events().recv_deadline(deadline).expect("listing"));
    }
    let find = |listing: &bibli_core::Listing, name: &str| {
        listing
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} listed"))
            .clone()
    };
    let kind = |e: &bibli_core::Entry| e.target.as_ref().map(|target| target.kind);
    let folder = find(&listing, "a folder link");
    assert_eq!(
        folder.kind,
        EntryKind::Symlink,
        "the entry is the link itself"
    );
    assert_eq!(kind(&folder), Some(EntryKind::Directory));
    assert!(folder.is_dir() && !folder.is_broken_link());
    let file = find(&listing, "a file link");
    assert_eq!(kind(&file), Some(EntryKind::File));
    assert!(!file.is_dir());
    assert_eq!(file.type_name(), "a note's text.txt");
    // What is read to show it is the file itself, named by its own
    // identity: a reference to it resolves.
    let note = dir.join("a note's text.txt").canonicalize().unwrap();
    let content = file.content(&dir.join("a file link")).unwrap();
    assert_eq!(content.path, note);
    assert_eq!(content.resolve().unwrap(), note);
    assert_eq!(
        find(&listing, "a link to a link").type_name(),
        "a note's text.txt"
    );
    let broken = find(&listing, "a broken link");
    assert!(broken.is_broken_link());
    assert_eq!(broken.type_name(), "a broken link");
    assert_eq!(broken.inode_type(), Some("inode/symlink"));
    assert_eq!(
        broken.content(&dir.join("a broken link")),
        None,
        "nothing is read at a missing target"
    );

    // Folders sort first, and a link to one with them, from the first
    // listing: rows off screen are never hydrated.
    let order: Vec<_> = listing
        .sorted(bibli_core::location::SortSpec::default())
        .iter()
        .map(|id| {
            listing
                .get(*id)
                .unwrap()
                .name
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        order[..2],
        ["a folder link".to_owned(), "real folder".to_owned()]
    );

    // A stat (hydration, a watcher) says the same, and what a link says of
    // size is what it leads to; a broken link says its own.
    let ids = [folder.id, file.id, broken.id];
    open.hydrate(&ids, Priority::Visible);
    while !ids
        .iter()
        .all(|id| listing.get(*id).unwrap().metadata.is_hydrated())
    {
        listing.apply(open.events().recv_deadline(deadline).expect("hydrated"));
    }
    let folder = listing.get(folder.id).unwrap();
    assert_eq!(folder.kind, EntryKind::Symlink);
    assert_eq!(kind(folder), Some(EntryKind::Directory));
    let size = |e: &bibli_core::Entry| e.metadata.value(keys::SIZE).and_then(Value::as_u64);
    let file = listing.get(file.id).unwrap();
    assert_eq!(size(file), Some(17), "the text's size, not the link's");
    assert_eq!(file.type_name(), "a note's text.txt");
    let broken = listing.get(broken.id).unwrap();
    assert_eq!(size(broken), Some("nowhere".len() as u64));
    assert!(broken.is_broken_link());
}

#[test]
fn a_pipe_or_a_socket_is_listed_as_what_it_is_and_never_read() {
    use bibli_core::entry::EntryKind;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("special");
    std::fs::create_dir_all(&dir).unwrap();
    // Named as pictures: the name says nothing of what they are.
    rustix::fs::mknodat(
        rustix::fs::CWD,
        dir.join("a pipe.png"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    let _socket = std::os::unix::net::UnixListener::bind(dir.join("a socket.jpg")).unwrap();
    std::os::unix::fs::symlink("a pipe.png", dir.join("a link to a pipe")).unwrap();
    let app = app(tmp.path());

    let open = app.open_location(&Location::from_path(&dir)).unwrap();
    let mut listing = bibli_core::Listing::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !listing.is_complete() {
        listing.apply(open.events().recv_deadline(deadline).expect("listing"));
    }
    let find = |name: &str| {
        listing
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} listed"))
            .clone()
    };
    let pipe = find("a pipe.png");
    assert_eq!(pipe.kind, EntryKind::Fifo);
    assert_eq!(pipe.inode_type(), Some("inode/fifo"));
    assert_eq!(pipe.content(&dir.join("a pipe.png")), None);
    let socket = find("a socket.jpg");
    assert_eq!(socket.kind, EntryKind::Socket);
    assert_eq!(socket.inode_type(), Some("inode/socket"));
    assert_eq!(socket.content(&dir.join("a socket.jpg")), None);
    // A link to a pipe is shown as one, and nothing is read through it.
    let link = find("a link to a pipe");
    assert_eq!(link.kind, EntryKind::Symlink);
    assert_eq!(link.opens(), EntryKind::Fifo);
    assert_eq!(link.inode_type(), Some("inode/fifo"));
    assert_eq!(link.content(&dir.join("a link to a pipe")), None);
    assert!(!link.is_dir() && !link.is_broken_link());

    // A device, as a stat reads it.
    let null = bibli_core::fs::stat_path(std::path::Path::new("/dev/null")).unwrap();
    assert_eq!(null.kind, EntryKind::CharDevice);
    assert_eq!(null.inode_type(), Some("inode/chardevice"));
    assert_eq!(null.content(std::path::Path::new("/dev/null")), None);
}

/// A folder's own attributes changing (its tags, its permissions) is a
/// change of the location, not of what it lists: the listing says so
/// (`itself_version`) and its entries stay as they are.
#[test]
fn a_folder_whose_own_attributes_change_says_so() {
    let tmp = home_tempdir();
    let dir = tmp.path().join("Trip d'été");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("one"), b"1").unwrap();
    let app = app(tmp.path());
    let pane = active(&app);
    navigate(&app, &dir);
    let before = app
        .workspace
        .read()
        .pane(pane)
        .unwrap()
        .listing
        .itself_version();
    xattr::set(&dir, "user.xdg.tags", b"holiday").unwrap();
    wait(&app, pane, |l| l.itself_version() != before);
    let ws = app.workspace.read();
    let listing = &ws.pane(pane).unwrap().listing;
    assert_eq!(listing.len(), 1, "what it lists is unchanged");
}

/// A path read alone is read as a listing reads its entries: stat-ed,
/// then filled by the metadata providers, the root included.
#[test]
fn a_path_read_alone_is_filled_by_the_providers() {
    use bibli_core::entry::{Entry, Provenance, Value};
    use bibli_core::metadata::MetadataProvider;
    struct Probe;
    impl MetadataProvider for Probe {
        fn id(&self) -> &'static str {
            "probe"
        }
        fn fill(&self, dir: &std::path::Path, entries: &mut [Entry]) {
            for e in entries {
                let seen = dir.join(&e.name).exists();
                e.metadata
                    .set("probe.seen", Value::Bool(seen), Provenance::Filesystem);
            }
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("it's a folder");
    std::fs::create_dir(&dir).unwrap();
    let app = app(tmp.path());
    app.metadata.register("probe", std::sync::Arc::new(Probe));
    for path in [dir.as_path(), std::path::Path::new("/")] {
        let entry = bibli_core::fs::hydrate_path(path, &app.metadata).unwrap();
        assert!(entry.is_dir() && entry.metadata.is_hydrated());
        assert_eq!(
            entry.metadata.value("probe.seen"),
            Some(&Value::Bool(true)),
            "{} filled at its own path",
            path.display()
        );
    }
    assert!(bibli_core::fs::hydrate_path(&dir.join("missing"), &app.metadata).is_none());
}
