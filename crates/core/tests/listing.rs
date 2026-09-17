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
