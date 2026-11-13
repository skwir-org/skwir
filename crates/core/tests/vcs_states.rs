//! What a version control says of a folder's files reaches the pane's
//! entries (ADR-134): asked once the listing is whole, again when the
//! provider says its answer moved, kept across a file's update until said
//! again, and gone with the provider; a partial answer never says an entry
//! is as committed; the columns above the folder are told with it.
mod common;

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use skwir_core::App;
use skwir_core::vcs::{Answer, Known, State, StatesProvider};

struct Fake {
    answer: Mutex<Answer>,
    generation: AtomicU64,
    asked: AtomicUsize,
}

impl Fake {
    fn new(answer: Answer) -> Arc<Self> {
        Arc::new(Fake {
            answer: Mutex::new(answer),
            generation: AtomicU64::new(0),
            asked: AtomicUsize::new(0),
        })
    }
}

/// Its answer from the highest folder shown, as a repository holding
/// them all would give it, unless its answer names a root of its own.
impl StatesProvider for Fake {
    fn id(&self) -> &'static str {
        "fake.states"
    }
    fn states(
        &self,
        folder: &Path,
        above: &[PathBuf],
        _opened: &[PathBuf],
        _cancel: &skwir_core::CancelToken,
    ) -> anyhow::Result<Option<Answer>> {
        self.asked.fetch_add(1, Ordering::AcqRel);
        let mut answer = self.answer.lock().unwrap().clone();
        if answer.root.as_os_str().is_empty() {
            answer.root = above
                .iter()
                .min_by_key(|up| up.components().count())
                .map_or_else(|| folder.to_owned(), Clone::clone);
        }
        Ok(Some(answer))
    }
    fn generation(&self, _key: u64) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

/// What the pane's entry `name` is said to be.
fn state_of(app: &App, name: &str) -> Option<State> {
    let ws = app.workspace.read();
    let entry = ws.active().listing.find_by_name(OsStr::new(name))?;
    State::of(entry)
}

/// What the pane's listing knows of entry `name` (`State::known`).
fn known_of(app: &App, name: &str) -> Known {
    let ws = app.workspace.read();
    ws.active()
        .listing
        .find_by_name(OsStr::new(name))
        .map_or(Known::Not, State::known)
}

/// Pump until `done` holds, ten seconds at most.
fn pump_until(app: &App, done: impl Fn(&App) -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        app.pump();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn a_folder_s_entries_take_what_its_version_control_says() {
    let tmp = common::home_tempdir();
    let dir = tmp.path().join("l'été repo");
    std::fs::create_dir_all(dir.join("sub dir")).unwrap();
    for name in ["a.txt", "b.txt", "sub dir/c.txt"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    let app = common::app(&tmp.path().join("state"));
    let fake = Fake::new(Answer {
        states: vec![
            ("a.txt".into(), State::Changed),
            ("sub dir/c.txt".into(), State::New),
        ],
        whole: true,
        key: 7,
        ..Answer::default()
    });
    app.vcs.register("fake", Arc::clone(&fake) as _);
    common::navigate(&app, &dir);
    assert!(
        pump_until(&app, |app| state_of(app, "a.txt") == Some(State::Changed)),
        "said once the listing is whole"
    );
    assert_eq!(known_of(&app, "b.txt"), Known::Committed, "as committed");
    assert_eq!(
        state_of(&app, "sub dir"),
        Some(State::New),
        "a folder holds what is below it"
    );
    let asked = fake.asked.load(Ordering::Acquire);
    for _ in 0..5 {
        app.pump();
    }
    assert_eq!(
        fake.asked.load(Ordering::Acquire),
        asked,
        "asked once while nothing moves"
    );
    // The repository moved (a commit elsewhere): asked again.
    *fake.answer.lock().unwrap() = Answer {
        states: vec![("b.txt".into(), State::Staged)],
        whole: true,
        key: 7,
        ..Answer::default()
    };
    fake.generation.fetch_add(1, Ordering::AcqRel);
    assert!(pump_until(&app, |app| state_of(app, "b.txt") == Some(State::Staged)));
    assert_eq!(state_of(&app, "a.txt"), None, "what it no longer says goes");
    assert_eq!(state_of(&app, "sub dir"), None);
    // A file shown written (its time known, as a view asks for its rows):
    // what was said of it stays until said again, and it is asked again.
    let size = |app: &App| {
        let ws = app.workspace.read();
        ws.active()
            .listing
            .find_by_name(OsStr::new("b.txt"))
            .and_then(|e| e.metadata.value(skwir_core::entry::keys::SIZE).cloned())
    };
    {
        let ws = app.workspace.read();
        let pane = ws.active();
        let ids: Vec<_> = pane.listing.iter().map(|e| e.id).collect();
        pane.open
            .as_ref()
            .unwrap()
            .hydrate(&ids, skwir_core::scheduler::Priority::Visible);
    }
    assert!(pump_until(&app, |app| size(app).is_some()), "stat-ed");
    let asked = fake.asked.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(dir.join("b.txt"), b"written again").unwrap();
    // At its pace: just asked (the repository moved), the write waits.
    assert!(
        pump_until(&app, |_| fake.asked.load(Ordering::Acquire) > asked),
        "a file written asks again"
    );
    assert_eq!(state_of(&app, "b.txt"), Some(State::Staged));
    // The provider gone: nothing is known of any entry.
    app.vcs.unregister_owner("fake");
    assert!(pump_until(&app, |app| known_of(app, "b.txt") == Known::Not));
    assert_eq!(
        known_of(&app, "a.txt"),
        Known::Not,
        "nor of one as committed"
    );
}

/// An answer that stopped short (the provider's limits) says what it
/// found: an entry it does not name keeps what it was said to be, never
/// said as committed; a whole answer then lets it go.
#[test]
fn a_partial_answer_never_says_an_entry_is_as_committed() {
    let tmp = common::home_tempdir();
    let dir = tmp.path().join("partial");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    let app = common::app(&tmp.path().join("state"));
    let fake = Fake::new(Answer {
        states: vec![("a.txt".into(), State::Changed)],
        whole: false,
        key: 1,
        ..Answer::default()
    });
    app.vcs.register("fake", Arc::clone(&fake) as _);
    common::navigate(&app, &dir);
    assert!(pump_until(&app, |app| state_of(app, "a.txt") == Some(State::Changed)));
    assert_eq!(
        known_of(&app, "b.txt"),
        Known::Not,
        "not reached: not known"
    );
    *fake.answer.lock().unwrap() = Answer {
        states: vec![("b.txt".into(), State::Staged)],
        whole: false,
        key: 1,
        ..Answer::default()
    };
    fake.generation.fetch_add(1, Ordering::AcqRel);
    assert!(pump_until(&app, |app| state_of(app, "b.txt") == Some(State::Staged)));
    assert_eq!(
        state_of(&app, "a.txt"),
        Some(State::Changed),
        "not reached: left as it was"
    );
    *fake.answer.lock().unwrap() = Answer {
        states: vec![("b.txt".into(), State::Staged)],
        whole: true,
        key: 1,
        ..Answer::default()
    };
    fake.generation.fetch_add(1, Ordering::AcqRel);
    assert!(pump_until(&app, |app| state_of(app, "a.txt").is_none()));
}

/// The columns above a folder (a columns view's) are told with it, from
/// one answer of the highest of them: going down leaves no column marked
/// as it was, and one kept fresh is marked too.
#[test]
fn the_columns_above_a_folder_are_told_with_it() {
    let tmp = common::home_tempdir();
    let top = tmp.path().join("top");
    let inside = top.join("in side");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::write(top.join("t.txt"), b"x").unwrap();
    std::fs::write(inside.join("i.txt"), b"x").unwrap();
    let app = common::app(&tmp.path().join("state"));
    let fake = Fake::new(Answer {
        states: vec![("in side/i.txt".into(), State::Changed)],
        whole: true,
        key: 1,
        ..Answer::default()
    });
    app.vcs.register("fake", Arc::clone(&fake) as _);
    common::navigate(&app, &inside);
    let pane = common::active(&app);
    app.keep_locations(pane, &[skwir_core::location::Location::from_path(&top)]);
    let column = |app: &App, name: &str| {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        let kept = p.kept(&skwir_core::location::Location::from_path(&top))?;
        State::of(kept.find_by_name(OsStr::new(name))?)
    };
    assert!(
        pump_until(&app, |app| column(app, "in side") == Some(State::Changed)),
        "the column above holds what is below it"
    );
    assert_eq!(state_of(&app, "i.txt"), Some(State::Changed));
    // Committed elsewhere: the column above is told too.
    *fake.answer.lock().unwrap() = Answer {
        states: vec![("t.txt".into(), State::Staged)],
        whole: true,
        key: 1,
        ..Answer::default()
    };
    fake.generation.fetch_add(1, Ordering::AcqRel);
    assert!(pump_until(&app, |app| column(app, "t.txt") == Some(State::Staged)));
    assert_eq!(column(&app, "in side"), None);
    assert_eq!(state_of(&app, "i.txt"), None);
}

/// A file written again within the pace (asked of a moment ago) is asked
/// of once the pace ends by itself: the pane is woken then, though nothing
/// else comes and nobody pumps meanwhile.
#[test]
fn a_file_written_within_the_pace_is_asked_of_once_it_ends() {
    let tmp = common::home_tempdir();
    let dir = tmp.path().join("paced");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("log.txt"), b"x").unwrap();
    let app = common::app(&tmp.path().join("state"));
    let fake = Fake::new(Answer {
        whole: true,
        key: 1,
        ..Answer::default()
    });
    app.vcs.register("fake", Arc::clone(&fake) as _);
    common::navigate(&app, &dir);
    let pane = common::active(&app);
    assert!(pump_until(&app, |_| fake.asked.load(Ordering::Acquire) > 0));
    {
        let ws = app.workspace.read();
        let p = ws.active();
        let ids: Vec<_> = p.listing.iter().map(|e| e.id).collect();
        p.open
            .as_ref()
            .unwrap()
            .hydrate(&ids, skwir_core::scheduler::Priority::Visible);
    }
    let size = |app: &App| {
        let ws = app.workspace.read();
        ws.active()
            .listing
            .find_by_name(OsStr::new("log.txt"))
            .and_then(|e| e.metadata.value(skwir_core::entry::keys::SIZE).cloned())
    };
    assert!(pump_until(&app, |app| size(app).is_some()), "stat-ed");
    let asked = fake.asked.load(Ordering::Acquire);
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(dir.join("log.txt"), b"written again").unwrap();
    let due = |app: &App| app.workspace.read().pane(pane).unwrap().states_due();
    assert!(
        pump_until(&app, |app| due(app).is_some()),
        "held back by the pace"
    );
    assert_eq!(fake.asked.load(Ordering::Acquire), asked, "not asked yet");
    let due = due(&app).unwrap();
    // Nobody pumps: the pane is woken once the pace ends.
    let until = Instant::now() + Duration::from_secs(10);
    let mut woken = false;
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
        if app.take_woken().panes.contains(&pane) && std::time::SystemTime::now() >= due {
            woken = true;
            break;
        }
    }
    assert!(woken, "woken when the pace ends");
    // The pump the wake brings asks (on a worker): nothing else is needed.
    app.pump();
    let until = Instant::now() + Duration::from_secs(10);
    while fake.asked.load(Ordering::Acquire) == asked && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(fake.asked.load(Ordering::Acquire), asked + 1, "asked");
}

/// Two answers that come while a sort holds the listing: the latest is
/// taken once the order is back, against what the entries hold then, so
/// what the first would have written (queued while the sort ran) never
/// lands over the second.
#[test]
fn an_answer_taken_while_a_sort_holds_the_listing_waits_for_its_order() {
    let tmp = common::home_tempdir();
    let dir = tmp.path().join("sorted");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    let app = common::app(&tmp.path().join("state"));
    let answer = |states: Vec<(String, State)>| Answer {
        states,
        whole: true,
        key: 1,
        ..Answer::default()
    };
    let fake = Fake::new(answer(vec![("a.txt".into(), State::Changed)]));
    app.vcs.register("fake", Arc::clone(&fake) as _);
    common::navigate(&app, &dir);
    let pane = common::active(&app);
    assert!(pump_until(&app, |app| state_of(app, "a.txt") == Some(State::Changed)));
    // A sort on a worker holds the entries (its job keeps them).
    let job = {
        let mut ws = app.workspace.write();
        let p = ws.pane_mut(pane).unwrap();
        let mut spec = p.sort;
        spec.key = skwir_core::location::SortKey::Size;
        p.listing.sort_job(spec)
    }
    .expect("a sort on a worker");
    let answered = |app: &App, n: usize| {
        let asked = pump_until(app, |_| fake.asked.load(Ordering::Acquire) >= n);
        // The answer's job pushes it and wakes: a few pumps take it.
        let until = Instant::now() + Duration::from_millis(200);
        while Instant::now() < until {
            app.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        asked
    };
    let asked = fake.asked.load(Ordering::Acquire);
    *fake.answer.lock().unwrap() = answer(vec![("b.txt".into(), State::Staged)]);
    fake.generation.fetch_add(1, Ordering::AcqRel);
    assert!(answered(&app, asked + 1));
    *fake.answer.lock().unwrap() = answer(vec![("a.txt".into(), State::Changed)]);
    fake.generation.fetch_add(1, Ordering::AcqRel);
    assert!(answered(&app, asked + 2));
    assert_eq!(
        state_of(&app, "b.txt"),
        None,
        "nothing written while sorting"
    );
    drop(job);
    assert!(pump_until(&app, |app| {
        let ws = app.workspace.read();
        let l = &ws.pane(pane).unwrap().listing;
        !l.is_sorting() && !l.has_result()
    }));
    assert!(pump_until(&app, |app| known_of(app, "b.txt") == Known::Committed));
    assert_eq!(state_of(&app, "a.txt"), Some(State::Changed), "the latest");
}
