//! What a plan brings into a folder stands among the folder's entries
//! before it lands (ADR-135): read from the plan, what it copies stat-ed
//! for its kind and its size, placed where the pane's order will have it,
//! left out where the pane's filter would leave it out, and gone once it
//! landed.
mod common;

use std::ffi::OsStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use parking_lot::{Condvar, Mutex};
use skwir_core::command::Invocation;
use skwir_core::op::arriving::{Arrives, Arriving};
use skwir_core::op::{Change, Copy, EntryRef, Operation, Plan, StepContext, StepStatus};
use skwir_core::{CancelToken, Entry};

/// A step that waits until it is opened, holding its plan's run as a long
/// copy before the others would.
#[derive(Debug, Clone)]
struct Held(Arc<(Mutex<bool>, Condvar)>);

impl Held {
    fn new() -> Self {
        Held(Arc::new((Mutex::new(false), Condvar::new())))
    }

    fn open(&self) {
        *self.0.0.lock() = true;
        self.0.1.notify_all();
    }
}

impl Operation for Held {
    fn kind(&self) -> &'static str {
        "held"
    }
    fn args(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn describe(&self) -> Change {
        Change::Metadata {
            path: "/example/held".into(),
            field: "held".into(),
            words: None,
        }
    }
    fn check(&self, _: &StepContext) -> StepStatus {
        StepStatus::Pending { reversible: false }
    }
    fn apply(&self, _: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let mut open = self.0.0.lock();
        while !*open {
            self.0.1.wait(&mut open);
        }
        Ok(None)
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
}

/// What the executor says arrives in `dest` once `done` holds of it, ten
/// seconds at most.
fn until(
    app: &skwir_core::App,
    dest: &std::path::Path,
    done: impl Fn(&[Arriving]) -> bool,
) -> Vec<Arriving> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let arriving = app.executor.arriving_in(dest);
        if done(&arriving) || Instant::now() > deadline {
            return arriving;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_copy_waiting_stands_where_it_will_land_and_goes_once_landed() {
    let tmp = home_tempdir();
    let from = tmp.path().join("from l'été");
    std::fs::create_dir_all(from.join("a folder")).unwrap();
    std::fs::write(from.join("a folder/inside.txt"), b"in").unwrap();
    std::fs::write(from.join("big file.bin"), vec![7u8; 3000]).unwrap();
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(dest.join("Music")).unwrap();
    std::fs::write(dest.join("a.txt"), b"a").unwrap();
    std::fs::write(dest.join("z.txt"), b"z").unwrap();
    let app = app(&tmp.path().join("state"));
    navigate(&app, &dest);

    let held = Held::new();
    let copy = |name: &str| -> Box<dyn Operation> {
        Box::new(Copy::from_settings(
            EntryRef::from_path(from.join(name)).unwrap(),
            dest.clone(),
            &app.settings,
        ))
    };
    let plan = Plan::with_steps(
        "Copy 2 items",
        vec![
            Box::new(held.clone()),
            copy("big file.bin"),
            copy("a folder"),
        ],
    );
    app.executor.execute(plan, CancelToken::new());
    // Both, as their sources are, waiting for the step before them.
    // What each is made from is read on a worker, as the run goes on.
    let read = |a: &[Arriving]| a.iter().all(|a| a.arrival.like.metadata.is_hydrated());
    let arriving = until(&app, &dest, |a| {
        a.len() == 2 && a.iter().all(|a| a.state == Arrives::Waiting) && read(a)
    });
    let names: Vec<&OsStr> = arriving
        .iter()
        .map(|a| a.arrival.name.as_os_str())
        .collect();
    assert_eq!(names, ["big file.bin", "a folder"]);
    assert!(arriving.iter().all(|a| a.state == Arrives::Waiting));
    let file = &arriving[0].arrival.like;
    assert_eq!(
        file.metadata
            .value(skwir_core::entry::keys::SIZE)
            .and_then(skwir_core::Value::as_u64),
        Some(3000)
    );
    assert!(arriving[1].arrival.like.is_dir());
    // As scripts and agents ask it.
    let said = match app
        .dispatch(&Invocation::with_args(
            "arriving",
            &serde_json::json!({ "folder": dest }),
        ))
        .unwrap()
    {
        skwir_core::command::Outcome::Value(value) => value,
        other => panic!("arriving answers a value, not {other:?}"),
    };
    assert_eq!(said[0]["name"], "big file.bin");
    assert_eq!(said[0]["state"], "waiting");
    assert_eq!(said[0]["kind"], "file");
    assert_eq!(said[0]["size"], 3000);
    assert_eq!(said[1]["kind"], "folder");

    // Where the pane would show them: folders first, by name.
    let likes: Vec<Entry> = arriving
        .iter()
        .map(|a| Entry::clone(&a.arrival.like))
        .collect();
    let stands = |app: &skwir_core::App| {
        let ws = app.workspace.read();
        let pane = ws.active();
        let order: Vec<String> = pane
            .listed_order()
            .iter()
            .map(|id| pane.listing.get(*id).unwrap().name_str().into_owned())
            .collect();
        let likes: Vec<&Entry> = likes.iter().collect();
        let at: Vec<Option<usize>> = pane
            .stands(&likes)
            .into_iter()
            .map(|stand| stand.map(|stand| stand.at))
            .collect();
        (order, at)
    };
    let (order, at) = stands(&app);
    assert_eq!(order, ["Music", "a.txt", "z.txt"]);
    assert_eq!(at, [Some(2), Some(0)], "between a.txt and z.txt; first");
    // The pane's filter leaves out what it would not show.
    app.dispatch(&Invocation::with_args(
        "filter",
        &serde_json::json!({ "text": "big" }),
    ))
    .unwrap();
    let (_, at) = stands(&app);
    assert_eq!(at, [Some(2), None]);

    // They land: nothing arrives any more.
    held.open();
    assert!(until(&app, &dest, <[Arriving]>::is_empty).is_empty());
    assert!(dest.join("big file.bin").is_file());
    assert!(dest.join("a folder/inside.txt").is_file());
}

/// What arrives is read as a listing reads its entries: its notes, which a
/// copy keeps (a provider's rating here), so a filter or a sort by them
/// places it; a copy is a file made anew, created and read when asked, a
/// move within its volume the file it was.
#[test]
fn what_arrives_is_read_with_its_notes_and_dated_as_its_file_will_be() {
    use skwir_core::entry::{Provenance, Value, keys};
    use skwir_core::metadata::MetadataProvider;
    struct Rated;
    impl MetadataProvider for Rated {
        fn id(&self) -> &'static str {
            "rated"
        }
        fn fill(&self, _: &std::path::Path, entries: &mut [Entry]) {
            for e in entries {
                e.metadata
                    .set(keys::RATING, Value::UInt(8), Provenance::Filesystem);
            }
        }
    }
    let tmp = home_tempdir();
    let from = tmp.path().join("from");
    std::fs::create_dir_all(&from).unwrap();
    std::fs::write(from.join("copied.txt"), b"c").unwrap();
    std::fs::write(from.join("moved.txt"), b"m").unwrap();
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let app = app(&tmp.path().join("state"));
    app.metadata.register("rated", Arc::new(Rated));
    let created = |entry: &Entry| entry.metadata.value(keys::CREATED).and_then(Value::as_time);
    let moved_was = skwir_core::fs::stat::stat_path(&from.join("moved.txt")).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let asked = std::time::SystemTime::now();
    let held = Held::new();
    let plan = Plan::with_steps(
        "copy and move",
        vec![
            Box::new(held.clone()),
            Box::new(Copy::from_settings(
                EntryRef::from_path(from.join("copied.txt")).unwrap(),
                dest.clone(),
                &app.settings,
            )),
            Box::new(skwir_core::op::Move::new(
                EntryRef::from_path(from.join("moved.txt")).unwrap(),
                dest.clone(),
            )),
        ],
    );
    app.executor.execute(plan, CancelToken::new());
    // Read on a worker, as the run goes on: once read, with their notes.
    let arriving = until(&app, &dest, |a| {
        a.len() == 2 && a.iter().all(|a| a.arrival.like.metadata.is_filled())
    });
    assert_eq!(arriving.len(), 2);
    let (copied, moved) = (&arriving[0].arrival.like, &arriving[1].arrival.like);
    for like in [copied, moved] {
        assert_eq!(like.metadata.value(keys::RATING), Some(&Value::UInt(8)));
        assert!(like.metadata.is_filled(), "read whole, as an entry listed");
    }
    let made = created(copied).expect("a copy is dated");
    assert!(
        made + Duration::from_secs(1) >= asked,
        "created as it was asked"
    );
    if let Some(was) = created(&moved_was) {
        assert_eq!(created(moved), Some(was), "a move keeps its file's");
    }
    held.open();
    assert!(until(&app, &dest, <[Arriving]>::is_empty).is_empty());
}
