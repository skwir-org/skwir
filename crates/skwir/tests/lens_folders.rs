//! A pane's lens and the folders it shows: a folder shows once a search
//! of it found what the lens keeps, as a search's results come, and a
//! slow one holds back no other. Words that find names hold at this level
//! alone.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::Location;
use skwir_core::workspace::PaneId;

fn fixture() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache/skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("lens-folders-")
        .tempdir_in(base)
        .unwrap()
}

fn write(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"x").unwrap();
    path
}

struct Setup {
    tmp: tempfile::TempDir,
    root: PathBuf,
    app: Arc<App>,
    pane: PaneId,
}

fn setup() -> Setup {
    let tmp = fixture();
    let root = tmp.path().join("it's a folder ü");
    std::fs::create_dir_all(&root).unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    Setup {
        tmp,
        root,
        app,
        pane,
    }
}

fn go(s: &Setup, dir: &Path) {
    s.app.navigate(s.pane, &Location::from_path(dir)).unwrap();
    assert!(
        s.app.pump_until_complete(s.pane, Duration::from_secs(10)),
        "listed"
    );
}

fn filter(s: &Setup, text: &str) {
    s.app
        .dispatch(&Invocation::with_args(
            "filter",
            &serde_json::json!({ "text": text, "pane": s.pane }),
        ))
        .unwrap();
}

fn shown(s: &Setup) -> BTreeSet<String> {
    let ws = s.app.workspace.read();
    let p = ws.pane(s.pane).unwrap();
    p.order()
        .iter()
        .filter_map(|id| p.entry(*id))
        .map(|e| e.name_str().into_owned())
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

/// Pump until the pane shows `names`.
fn shows(s: &Setup, names: &[&str], what: &str) {
    let expected = set(names);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        s.app.pump();
        let now = shown(s);
        if now == expected {
            return;
        }
        assert!(Instant::now() < deadline, "{what}: {now:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `names` stay shown for as long as a look below the folders could take.
fn stays(s: &Setup, names: &[&str], what: &str) {
    let expected = set(names);
    let until = Instant::now() + Duration::from_millis(700);
    while Instant::now() < until {
        s.app.pump();
        assert_eq!(shown(s), expected, "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn only_leads(s: &Setup, name: &str) -> bool {
    let ws = s.app.workspace.read();
    let p = ws.pane(s.pane).unwrap();
    let id = p.listing.find_by_name(name.as_ref()).unwrap().id;
    p.only_leads(id)
}

#[test]
fn a_lens_keeps_the_folders_that_lead_to_what_it_keeps() {
    let s = setup();
    write(&s.root, "photos/2024/deep/a.jpg");
    write(&s.root, "notes/a.txt");
    write(&s.root, "empty it's/sub/b.txt");
    write(&s.root, "top.jpg");
    write(&s.root, "top.txt");
    go(&s, &s.root);
    filter(&s, "kind:image");
    shows(&s, &["photos", "top.jpg"], "only the way to a picture");
    assert!(only_leads(&s, "photos"), "no picture itself: dimmed");
    drop(s.tmp);
}

#[test]
fn words_hold_at_this_level_alone_and_never_look_below() {
    let s = setup();
    write(&s.root, "reports/inner-match.txt");
    write(&s.root, "match-here/x.txt");
    write(&s.root, "match.txt");
    go(&s, &s.root);
    filter(&s, "match");
    stays(
        &s,
        &["match-here", "match.txt"],
        "a word finds names here, not below",
    );
    assert!(!only_leads(&s, "match-here"), "words dim nothing");
    drop(s.tmp);
}

#[test]
fn a_folder_with_the_tag_stays_lit_and_one_leading_to_it_is_dimmed() {
    let s = setup();
    let tagged = s.root.join("tagged");
    std::fs::create_dir_all(&tagged).unwrap();
    let inner = write(&s.root, "leads/deep/file.txt");
    write(&s.root, "nothing/file.txt");
    let tag = |path: &Path| skwir_core::fs::xattr::set(path, "user.xdg.tags", b"config");
    if tag(&tagged).is_err() || tag(&inner).is_err() {
        eprintln!("skipped: no user extended attributes here");
        return;
    }
    go(&s, &s.root);
    filter(&s, "tag:config");
    shows(
        &s,
        &["tagged", "leads"],
        "the tag's folders and the way to one",
    );
    let id = {
        let ws = s.app.workspace.read();
        ws.pane(s.pane)
            .unwrap()
            .listing
            .find_by_name("tagged".as_ref())
            .unwrap()
            .id
    };
    s.app
        .dispatch(&Invocation::with_args(
            "hydrate",
            &serde_json::json!({ "ids": [id], "pane": s.pane }),
        ))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while only_leads(&s, "tagged") {
        assert!(Instant::now() < deadline, "its own tags are read");
        s.app.pump();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(only_leads(&s, "leads"), "only the way there");
    drop(s.tmp);
}

#[test]
fn what_a_search_would_not_find_leads_nowhere() {
    let s = setup();
    // Below a folder the searches leave out, and in a hidden folder.
    write(&s.root, "project/node_modules/pic.jpg");
    write(&s.root, "project/readme.txt");
    write(&s.root, "keeps/.dot/pic.jpg");
    go(&s, &s.root);
    filter(&s, "kind:image");
    shows(&s, &[], "neither would be found");
    // Hidden files shown: the hidden folder leads somewhere after all.
    s.app.dispatch(&Invocation::new("toggle_hidden")).unwrap();
    shows(&s, &["keeps"], "found once hidden files are shown");
    drop(s.tmp);
}

#[test]
fn a_change_made_here_is_looked_at_again() {
    let s = setup();
    write(&s.root, "later/a.txt");
    write(&s.root, "photos/a.jpg");
    go(&s, &s.root);
    filter(&s, "kind:image");
    shows(&s, &["photos"], "no picture in it yet");
    let Outcome::Plan(plan) = s
        .app
        .dispatch(&Invocation::with_args(
            "new_file",
            &serde_json::json!({ "name": "b.jpg", "in": s.root.join("later") }),
        ))
        .unwrap()
    else {
        panic!("a plan that makes the file")
    };
    let done = s.app.executor.execute_blocking(&plan, s.app.lifetime());
    assert!(done.error.is_none(), "{:?}", done.error);
    shows(&s, &["later", "photos"], "a picture came");
    drop(s.tmp);
}

#[test]
fn another_lens_starts_over() {
    let s = setup();
    write(&s.root, "pics/a.jpg");
    write(&s.root, "texts/a.txt");
    go(&s, &s.root);
    filter(&s, "kind:image");
    shows(&s, &["pics"], "pictures");
    filter(&s, "ext:txt");
    shows(
        &s,
        &["texts"],
        "the folder left out comes back for this one",
    );
    filter(&s, "");
    shows(&s, &["pics", "texts"], "no lens, every folder");
    drop(s.tmp);
}

/// A reader whose lens keeps `*.keep` files and whose folders are answered
/// by `answer`, for what the core does with answers that take long.
fn reading_with(s: &Setup, answer: impl Fn(&Path) -> Option<bool> + Send + Sync + 'static) {
    let keeps: Arc<dyn Fn(&skwir_core::entry::Entry) -> bool + Send + Sync> =
        Arc::new(|e| e.name_str().ends_with(".keep"));
    let answer: skwir_core::registry::LeadsBelow = Arc::new(move |_, dir, _| answer(dir));
    let read = move |text: &str| {
        let lens = skwir_core::workspace::Lens {
            words: Arc::from(text),
            set: Vec::new(),
            keeps: Arc::clone(&keeps),
            metadata: false,
            next_change: None,
        };
        Some(skwir_core::workspace::ReadFilter::of_lens(lens))
    };
    s.app.registry.filter_readers.add(
        "stub",
        skwir_core::registry::FilterReader {
            read: Arc::new(read),
            leads: Some(answer),
        },
    );
}

/// `setup` with no plugin that reads filters: `reading_with` is the one.
fn setup_bare() -> Setup {
    let tmp = fixture();
    let root = tmp.path().join("it's a folder ü");
    std::fs::create_dir_all(&root).unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    let pane = app.workspace.read().active_id();
    Setup {
        tmp,
        root,
        app,
        pane,
    }
}

fn looks_below(s: &Setup) -> bool {
    s.app.workspace.read().pane(s.pane).unwrap().looks_below()
}

#[test]
fn folders_appear_as_they_are_found_and_a_slow_one_holds_back_no_other() {
    let s = setup_bare();
    if s.app.scheduler.workers() < 3 {
        eprintln!("skipped: too few workers to look into two folders at once");
        return;
    }
    write(&s.root, "a-slow/x");
    write(&s.root, "found/x");
    write(&s.root, "nothing/x");
    write(&s.root, "top.keep");
    write(&s.root, "top.other");
    let (let_go, slow) = std::sync::mpsc::channel::<()>();
    let slow = std::sync::Mutex::new(slow);
    reading_with(&s, move |dir| {
        match dir.file_name().and_then(|n| n.to_str()) {
            Some("a-slow") => {
                let _ = slow.lock().unwrap().recv();
                Some(true)
            }
            Some("found") => Some(true),
            _ => Some(false),
        }
    });
    go(&s, &s.root);
    filter(&s, "lens");
    // The slow one, first of them all, keeps nothing else waiting, and
    // what leads nowhere never shows.
    shows(&s, &["found", "top.keep"], "what is known so far");
    assert!(looks_below(&s), "said to be still looking");
    assert!(only_leads(&s, "found"), "shown as the way there");
    drop(let_go);
    shows(&s, &["a-slow", "found", "top.keep"], "the slow one came");
    let deadline = Instant::now() + Duration::from_secs(10);
    while looks_below(&s) {
        assert!(Instant::now() < deadline, "the looks end");
        s.app.pump();
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(s.tmp);
}

#[test]
fn the_view_stays_as_it_was_for_a_moment_then_shows_what_is_known() {
    let s = setup_bare();
    write(&s.root, "folder/x");
    write(&s.root, "top.keep");
    write(&s.root, "top.other");
    let (let_go, held) = std::sync::mpsc::channel::<()>();
    let held = std::sync::Mutex::new(held);
    reading_with(&s, move |_| {
        let _ = held.lock().unwrap().recv();
        Some(true)
    });
    go(&s, &s.root);
    let began = Instant::now();
    filter(&s, "lens");
    s.app.pump();
    let seen = shown(&s);
    // Only what is seen within the moment says anything (a loaded machine
    // may take longer to get here).
    if began.elapsed() < Duration::from_millis(100) {
        assert_eq!(seen, set(&["folder", "top.keep", "top.other"]), "unchanged");
        assert!(!only_leads(&s, "folder"), "and not dimmed yet");
    }
    shows(&s, &["top.keep"], "no answer yet: what is known");
    drop(let_go);
    shows(&s, &["folder", "top.keep"], "then the folder");
    drop(s.tmp);
}

#[test]
fn what_shows_stays_while_the_folders_are_asked_about_again() {
    let s = setup();
    write(&s.root, "pics/a.jpg");
    go(&s, &s.root);
    filter(&s, "kind:image");
    shows(&s, &["pics"], "found");
    // The hidden files' switch asks again, as a change made here does.
    s.app.dispatch(&Invocation::new("toggle_hidden")).unwrap();
    for _ in 0..20 {
        s.app.pump();
        assert_eq!(shown(&s), set(&["pics"]), "answers given stand");
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(s.tmp);
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_read_stays_shown() {
    use std::os::unix::fs::PermissionsExt;
    let s = setup();
    write(&s.root, "pics/a.jpg");
    write(&s.root, "locked/a.txt");
    write(&s.root, "plain/a.txt");
    let locked = s.root.join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&locked).is_ok() {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: this user reads any folder");
        return;
    }
    go(&s, &s.root);
    filter(&s, "kind:image");
    shows(&s, &["locked", "pics"], "nothing proved it leads nowhere");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    drop(s.tmp);
}
