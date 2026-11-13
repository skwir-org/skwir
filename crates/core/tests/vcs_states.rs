//! What a version control says of a folder's files reaches the pane's
//! entries (ADR-134): asked once the listing is whole, again when the
//! provider says its answer moved, kept across a file's update until said
//! again, and gone with the provider.
mod common;

use std::ffi::OsStr;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use skwir_core::App;
use skwir_core::vcs::{Answer, State, StatesProvider};

struct Fake {
    answer: Mutex<Answer>,
    generation: AtomicU64,
    asked: AtomicUsize,
}

impl StatesProvider for Fake {
    fn id(&self) -> &'static str {
        "fake.states"
    }
    fn states(
        &self,
        _folder: &Path,
        _cancel: &skwir_core::CancelToken,
    ) -> anyhow::Result<Option<Answer>> {
        self.asked.fetch_add(1, Ordering::AcqRel);
        Ok(Some(self.answer.lock().unwrap().clone()))
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
    let fake = Arc::new(Fake {
        answer: Mutex::new(Answer {
            folder: None,
            states: vec![
                ("a.txt".into(), State::Changed),
                ("sub dir/c.txt".into(), State::New),
            ],
            key: 7,
        }),
        generation: AtomicU64::new(0),
        asked: AtomicUsize::new(0),
    });
    app.vcs.register("fake", Arc::clone(&fake) as _);
    common::navigate(&app, &dir);
    assert!(
        pump_until(&app, |app| state_of(app, "a.txt") == Some(State::Changed)),
        "said once the listing is whole"
    );
    assert_eq!(state_of(&app, "b.txt"), None, "as committed");
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
        folder: None,
        states: vec![("b.txt".into(), State::Staged)],
        key: 7,
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
    assert!(
        pump_until(&app, |_| fake.asked.load(Ordering::Acquire) > asked),
        "a file written asks again"
    );
    assert_eq!(state_of(&app, "b.txt"), Some(State::Staged));
    // The provider gone: nothing is said of any entry.
    app.vcs.unregister_owner("fake");
    assert!(pump_until(&app, |app| state_of(app, "b.txt").is_none()));
}
