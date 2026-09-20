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
}
