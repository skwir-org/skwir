//! A name already taken where a copy, a move or a link goes (ADR-054): what is
//! never asked, what the window is asked, what an agent says beforehand,
//! and Undo taking all of it back.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use common::*;
use skwir_core::app::{App, ClashQuestion};
use skwir_core::op::clash::{Act, Answer, Choice, FileChoice, FolderChoice, Policy};
use skwir_core::op::{Change, ExecEvent, Items, StepStatus};

/// A scratch tree for one test; what it left in the trash is purged when
/// it ends (nextest gives each test process a trash of its own).
struct Scratch(tempfile::TempDir);

impl Scratch {
    fn new() -> Self {
        Self(home_tempdir())
    }
    fn path(&self) -> &Path {
        self.0.path()
    }
}

impl Drop for Scratch {
    // macOS's trash is not listed by the trash crate (ADR-035).
    #[cfg(not(target_os = "macos"))]
    fn drop(&mut self) {
        // Linux's trashes as a plan reads them: a dead network mount's top
        // is never looked at.
        #[cfg(target_os = "linux")]
        let listed: Result<_, trash::Error> = Ok(skwir_core::op::trash_items_here());
        #[cfg(not(target_os = "linux"))]
        let listed = trash::os_limited::list();
        if let Ok(items) = listed {
            let ours = items
                .into_iter()
                .filter(|i| i.original_path().starts_with(self.0.path()));
            let _ = trash::os_limited::purge_all(ours);
        }
    }
    #[cfg(target_os = "macos")]
    fn drop(&mut self) {}
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Set when `path` last changed, `secs` after the epoch.
fn dated(path: &Path, secs: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

/// The executor's events until a plan finishes; the last is `Finished`.
fn finished(app: &App) -> ExecEvent {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let event = app
            .executor
            .events()
            .recv_deadline(deadline)
            .expect("the plan finishes");
        if matches!(event, ExecEvent::Finished { .. }) {
            return event;
        }
    }
}

fn asked(app: &App) -> ClashQuestion {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(q) = app.take_clash_questions().pop() {
            return q;
        }
        assert!(Instant::now() < deadline, "never asked");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn transfer(app: &App, verb: &str, entries: &[PathBuf], dest: &Path) -> skwir_core::op::Plan {
    plan(
        app,
        verb,
        serde_json::json!({ "entries": entries, "dest": dest }),
    )
}

fn undo(app: &App) {
    let r = app
        .executor
        .undo_blocking(app.lifetime())
        .expect("something to undo");
    assert!(r.error.is_none(), "{:?}", r.error);
}

/// A copy into its own folder is a duplicate, named as GNOME names it,
/// counting on: never asked, and nothing to ask anyone.
#[test]
fn a_copy_into_its_own_folder_keeps_both() {
    // What was replaced, or a copy undone, goes to the trash.
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path().join("it's here");
    write(&root.join("notes.txt"), "mine");
    let app = app(tmp.path());
    for _ in 0..2 {
        app.apply_whole(transfer(&app, "copy", &[root.join("notes.txt")], &root));
        assert!(matches!(
            finished(&app),
            ExecEvent::Finished {
                applied: 1,
                error: None,
                ..
            }
        ));
    }
    assert_eq!(read(&root.join("notes (copy).txt")), "mine");
    assert_eq!(read(&root.join("notes (copy 2).txt")), "mine");
    undo(&app);
    assert!(!root.join("notes (copy 2).txt").exists());
    assert!(root.join("notes (copy).txt").exists());
}

/// Two items of one name going to one folder keep both, in order; the
/// second is named after the first takes the name.
#[test]
fn two_items_of_one_name_keep_both_in_order() {
    let tmp = Scratch::new();
    let root = tmp.path();
    write(&root.join("x/a.txt"), "first");
    write(&root.join("y/a.txt"), "second");
    std::fs::create_dir_all(root.join("dest")).unwrap();
    let app = app(root);
    let entries = [root.join("x/a.txt"), root.join("y/a.txt")];
    app.apply_whole(transfer(&app, "copy", &entries, &root.join("dest")));
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished {
            applied: 2,
            error: None,
            ..
        }
    ));
    assert_eq!(read(&root.join("dest/a.txt")), "first");
    assert_eq!(read(&root.join("dest/a (copy).txt")), "second");
}

/// With no window up no one is asked: the plan applies nothing and says
/// what is taken, as before.
#[test]
fn a_name_taken_with_no_one_to_ask_applies_nothing() {
    let tmp = Scratch::new();
    let root = tmp.path();
    write(&root.join("a.txt"), "mine");
    write(&root.join("b.txt"), "mine too");
    write(&root.join("dest/b.txt"), "theirs");
    let app = app(root);
    let entries = [root.join("a.txt"), root.join("b.txt")];
    app.apply_whole(transfer(&app, "copy", &entries, &root.join("dest")));
    match finished(&app) {
        ExecEvent::Finished {
            applied: 0,
            error: Some(error),
            ..
        } => assert_eq!(error, "“b.txt” is already there"),
        other => panic!("{other:?}"),
    }
    assert!(!root.join("dest/a.txt").exists(), "nothing applied");
}

/// The window is asked once, every name taken listed, a merged folder's
/// after it; its answer is the plan, and Undo takes all of it back, what
/// was replaced out of the trash.
#[test]
fn the_window_is_asked_once_and_its_answer_is_the_plan() {
    // What was replaced, or a copy undone, goes to the trash.
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path().join("tree");
    let src = root.join("src");
    let dest = root.join("dest");
    write(&src.join("a.txt"), "a, mine");
    write(&src.join("b.txt"), "b, mine");
    write(&src.join("d/x.txt"), "x, mine");
    write(&src.join("d/sub/y.txt"), "y, mine");
    write(&dest.join("a.txt"), "a, theirs");
    write(&dest.join("b.txt"), "b, theirs");
    write(&dest.join("d/x.txt"), "x, theirs");
    let before = snapshot(&root);
    let app = app(tmp.path());
    app.set_window_shown();
    let entries = ["a.txt", "b.txt", "d"].map(|n| src.join(n));
    app.apply_whole(transfer(&app, "copy", &entries, &dest));

    let question = asked(&app);
    let listed: Vec<(PathBuf, Option<usize>, bool)> = question
        .clashes
        .iter()
        .map(|c| (c.there.path.clone(), c.inside, c.folders()))
        .collect();
    assert_eq!(
        listed,
        vec![
            (dest.join("a.txt"), None, false),
            (dest.join("b.txt"), None, false),
            (dest.join("d"), None, true),
            (dest.join("d/x.txt"), Some(2), false),
        ]
    );
    let key = |i: usize| question.clashes[i].key();
    let answer = Answer {
        policy: Policy::default(),
        each: HashMap::from([(key(1), Choice::Replace), (key(3), Choice::Skip)]),
    };
    assert!(app.answer_clashes(question.id, Some(answer)));
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished { error: None, .. }
    ));
    assert_eq!(read(&dest.join("a.txt")), "a, theirs", "kept");
    assert_eq!(read(&dest.join("a (copy).txt")), "a, mine", "both");
    assert_eq!(read(&dest.join("b.txt")), "b, mine", "replaced");
    assert_eq!(read(&dest.join("d/x.txt")), "x, theirs", "skipped");
    assert_eq!(read(&dest.join("d/sub/y.txt")), "y, mine", "merged");

    undo(&app);
    assert_eq!(snapshot(&root), before, "all of it taken back");
}

/// Cancel applies nothing, and shows nothing: the plan never started.
#[test]
fn cancelled_the_plan_applies_nothing() {
    let tmp = Scratch::new();
    let root = tmp.path();
    write(&root.join("a.txt"), "mine");
    write(&root.join("dest/a.txt"), "theirs");
    write(&root.join("b.txt"), "mine too");
    let app = app(root);
    app.set_window_shown();
    let entries = [root.join("b.txt"), root.join("a.txt")];
    app.apply_whole(transfer(&app, "move", &entries, &root.join("dest")));
    let question = asked(&app);
    assert_eq!(question.clashes[0].act, Act::Move);
    app.answer_clashes(question.id, None);
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished {
            applied: 0,
            error: None,
            cancelled: true,
            ..
        }
    ));
    assert!(root.join("b.txt").exists() && !root.join("dest/b.txt").exists());
}

/// A move merged into a folder of its name: what is not there goes whole,
/// a file there follows the files' choice, and the folders the move took
/// everything out of are removed. Said beforehand, as an agent does, no
/// one is asked. Undo makes them again and brings everything back.
#[test]
fn a_move_merged_removes_the_folders_it_emptied() {
    // What was replaced, or a copy undone, goes to the trash.
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path().join("tree");
    write(&root.join("src/d/x.txt"), "x, mine");
    write(&root.join("src/d/sub/y.txt"), "y, mine");
    write(&root.join("src/d/sub/z.txt"), "z, mine");
    write(&root.join("dest/d/x.txt"), "x, theirs");
    write(&root.join("dest/d/sub/z.txt"), "z, theirs");
    let before = snapshot(&root);
    let app = app(tmp.path());
    let plan = plan(
        &app,
        "move",
        serde_json::json!({
            "entries": [root.join("src/d")],
            "dest": root.join("dest"),
            "on_conflict": { "files": "replace" },
        }),
    );
    app.apply_whole(plan);
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished { error: None, .. }
    ));
    assert!(app.take_clash_questions().is_empty(), "never asked");
    assert!(!root.join("src/d").exists(), "emptied, removed");
    assert_eq!(read(&root.join("dest/d/x.txt")), "x, mine");
    assert_eq!(read(&root.join("dest/d/sub/y.txt")), "y, mine");
    assert_eq!(read(&root.join("dest/d/sub/z.txt")), "z, mine");

    undo(&app);
    assert_eq!(snapshot(&root), before);
}

/// A folder a merge left something in stays where it was, with what was
/// skipped.
#[test]
fn a_folder_a_move_left_something_in_stays() {
    let tmp = Scratch::new();
    let root = tmp.path();
    write(&root.join("src/d/x.txt"), "x, mine");
    write(&root.join("src/d/y.txt"), "y, mine");
    write(&root.join("dest/d/x.txt"), "x, theirs");
    let app = app(root);
    let plan = transfer(&app, "move", &[root.join("src/d")], &root.join("dest")).on_conflict(Some(
        Policy {
            files: FileChoice::Skip,
            folders: FolderChoice::Merge,
        },
    ));
    app.apply_whole(plan);
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished { error: None, .. }
    ));
    assert_eq!(read(&root.join("src/d/x.txt")), "x, mine");
    assert_eq!(read(&root.join("dest/d/x.txt")), "x, theirs");
    assert_eq!(read(&root.join("dest/d/y.txt")), "y, mine");
}

/// Replace if newer replaces what is older than what comes, and skips the
/// rest.
#[test]
fn replace_if_newer_replaces_only_what_is_older() {
    // What was replaced goes to the trash.
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path();
    for (name, mine, theirs) in [("old.txt", 2000, 1000), ("new.txt", 1000, 2000)] {
        write(&root.join(name), "mine");
        write(&root.join("dest").join(name), "theirs");
        dated(&root.join(name), mine);
        dated(&root.join("dest").join(name), theirs);
    }
    let app = app(root);
    let entries = [root.join("old.txt"), root.join("new.txt")];
    let plan = transfer(&app, "copy", &entries, &root.join("dest")).on_conflict(Some(Policy {
        files: FileChoice::ReplaceIfNewer,
        folders: FolderChoice::Merge,
    }));
    app.apply_whole(plan);
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished { error: None, .. }
    ));
    assert_eq!(read(&root.join("dest/old.txt")), "mine", "older there");
    assert_eq!(read(&root.join("dest/new.txt")), "theirs", "newer there");
}

/// An item already there whole is never asked about: a copy has nothing
/// to do, and a move leaves the original in the trash.
#[test]
fn an_item_there_whole_is_never_asked_about() {
    // What was replaced, or a copy undone, goes to the trash.
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path();
    write(&root.join("a.txt"), "same");
    std::fs::create_dir_all(root.join("dest")).unwrap();
    std::fs::copy(root.join("a.txt"), root.join("dest/a.txt")).unwrap();
    let modified = std::fs::metadata(root.join("a.txt"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(root.join("dest/a.txt"))
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let app = app(root);
    app.set_window_shown();
    app.apply_whole(transfer(
        &app,
        "copy",
        &[root.join("a.txt")],
        &root.join("dest"),
    ));
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished {
            applied: 0,
            skipped: 1,
            error: None,
            ..
        }
    ));
    app.apply_whole(transfer(
        &app,
        "move",
        &[root.join("a.txt")],
        &root.join("dest"),
    ));
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished { error: None, .. }
    ));
    assert!(app.take_clash_questions().is_empty(), "never asked");
    assert!(!root.join("a.txt").exists(), "in the trash");
    assert_eq!(read(&root.join("dest/a.txt")), "same");
    undo(&app);
    assert_eq!(read(&root.join("a.txt")), "same", "back");
}

/// A plan's policy travels with it as data: an agent's `apply_plan`.
#[test]
fn a_plan_s_policy_travels_as_data() {
    let tmp = Scratch::new();
    let root = tmp.path();
    write(&root.join("a.txt"), "mine");
    let app = app(root);
    let plan = transfer(&app, "copy", &[root.join("a.txt")], root).on_conflict(Some(Policy {
        files: FileChoice::Skip,
        folders: FolderChoice::KeepBoth,
    }));
    let data = serde_json::to_value(plan.to_data()).unwrap();
    assert_eq!(
        data["on_conflict"],
        serde_json::json!({ "files": "skip", "folders": "keep_both" })
    );
    let back =
        skwir_core::op::Plan::from_data(&serde_json::from_value(data).unwrap(), &app.ops).unwrap();
    assert_eq!(back.on_conflict, plan.on_conflict);
}

/// A proposal with a name taken can be applied: its review shows it as
/// the defaults would settle it, and says how many names the user is
/// asked about; its plain dry run says what is taken.
#[test]
fn a_proposal_with_a_name_taken_is_reviewed_as_the_defaults_settle_it() {
    let tmp = Scratch::new();
    let root = tmp.path();
    write(&root.join("a.txt"), "mine");
    write(&root.join("dest/a.txt"), "theirs");
    let app = app(root);
    let plan = transfer(&app, "copy", &[root.join("a.txt")], &root.join("dest"));
    let (diff, asked) = plan.review(&app.companions);
    assert_eq!(asked, 1);
    match &diff.steps[..] {
        [(Change::Copy { to, .. }, StepStatus::Pending { .. })] => {
            assert_eq!(to, &root.join("dest/a (copy).txt"));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        &plan.diff(&app.companions).steps[0].1,
        StepStatus::Conflict { reason } if reason == "“a.txt” is already there"
    ));
}

/// `._x` beside `x`: a companion goes where its file goes (ADR-028).
struct Aside;

impl skwir_core::companions::Companion for Aside {
    fn id(&self) -> &'static str {
        "aside"
    }
    fn of(&self, name: &std::ffi::OsStr) -> Vec<PathBuf> {
        let mut aside = std::ffi::OsString::from("._");
        aside.push(name);
        vec![aside.into()]
    }
    fn owner(&self, name: &std::ffi::OsStr) -> Option<std::ffi::OsString> {
        let s = name.to_str()?.strip_prefix("._")?;
        (!s.is_empty()).then(|| s.into())
    }
}

/// A merge carries a file's companion with it: replaced, what was there
/// goes to the trash with its own; kept both, the companion takes the new
/// name too. The folder the move emptied goes, companions and all, and
/// Undo brings everything back.
#[test]
fn a_merge_carries_companions_with_their_files() {
    // What was replaced, or a copy undone, goes to the trash.
    if under_wine("trashing through the shell") {
        return;
    }
    for (files, kept) in [
        (FileChoice::Replace, "a.jpg"),
        (FileChoice::KeepBoth, "a (copy).jpg"),
    ] {
        let tmp = Scratch::new();
        let root = tmp.path().join("tree");
        write(&root.join("src/d/a.jpg"), "a, mine");
        write(&root.join("src/d/._a.jpg"), "aside, mine");
        write(&root.join("dest/d/a.jpg"), "a, theirs");
        write(&root.join("dest/d/._a.jpg"), "aside, theirs");
        let before = snapshot(&root);
        let app = app(tmp.path());
        app.companions.register("test", std::sync::Arc::new(Aside));
        let plan = transfer(&app, "move", &[root.join("src/d")], &root.join("dest")).on_conflict(
            Some(Policy {
                files,
                folders: FolderChoice::Merge,
            }),
        );
        app.apply_whole(plan);
        match finished(&app) {
            ExecEvent::Finished { error: None, .. } => {}
            other => panic!("{files:?}: {other:?}"),
        }
        assert!(!root.join("src/d").exists(), "{files:?}: emptied, removed");
        assert_eq!(read(&root.join("dest/d").join(kept)), "a, mine");
        assert_eq!(
            read(&root.join("dest/d").join(format!("._{kept}"))),
            "aside, mine"
        );
        undo(&app);
        assert_eq!(snapshot(&root), before, "{files:?}");
    }
}

/// A plan settled says what the user asked for, item by item: two files
/// replaced are two items copied, not four steps, and the first is named
/// by its own step.
#[test]
fn a_plan_settled_says_what_was_asked() {
    // What was replaced, or a copy undone, goes to the trash.
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path();
    for name in ["a.txt", "b.txt"] {
        write(&root.join(name), "mine");
        write(&root.join("dest").join(name), "theirs");
    }
    write(&root.join("c.txt"), "skipped");
    write(&root.join("dest/c.txt"), "kept");
    let app = app(root);
    let entries = ["a.txt", "b.txt", "c.txt"].map(|n| root.join(n));
    app.set_window_shown();
    app.apply_whole(transfer(&app, "copy", &entries, &root.join("dest")));
    // Replace for all files, but the third skipped.
    let question = asked(&app);
    let mut answer = Answer::all(Policy {
        files: FileChoice::Replace,
        folders: FolderChoice::Merge,
    });
    answer.each.insert(question.clashes[2].key(), Choice::Skip);
    app.answer_clashes(question.id, Some(answer));
    let deadline = Instant::now() + Duration::from_secs(10);
    let started = loop {
        if let ExecEvent::Started { total, items, .. } =
            app.executor.events().recv_deadline(deadline).unwrap()
        {
            break (total, items);
        }
    };
    assert_eq!(
        started,
        (
            4,
            Some(Items {
                first: Change::Copy {
                    from: root.join("a.txt"),
                    to: root.join("dest/a.txt"),
                },
                count: 2,
            })
        )
    );
    finished(&app);
    assert_eq!(read(&root.join("dest/c.txt")), "kept");
}

/// A link onto a name taken is asked about as a copy is, with a file's
/// choices even for a folder: a link is what it is, never merged. Kept
/// both, it is named `name (link).ext`.
#[cfg(unix)]
#[test]
fn a_link_onto_a_name_taken_is_asked_about() {
    // What was replaced goes to the trash.
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path().join("tree");
    let (src, dest) = (root.join("src"), root.join("dest"));
    write(&src.join("a.txt"), "a, mine");
    write(&src.join("b.txt"), "b, mine");
    write(&src.join("d/x.txt"), "x, mine");
    write(&dest.join("a.txt"), "a, theirs");
    write(&dest.join("b.txt"), "b, theirs");
    write(&dest.join("d/x.txt"), "x, theirs");
    let before = snapshot(&root);
    let app = app(tmp.path());
    app.set_window_shown();
    let entries = ["a.txt", "b.txt", "d"].map(|n| src.join(n));
    app.apply_whole(transfer(&app, "link", &entries, &dest));

    let question = asked(&app);
    let listed: Vec<(PathBuf, bool, bool)> = question
        .clashes
        .iter()
        .map(|c| (c.there.path.clone(), c.act == Act::Link, c.folders()))
        .collect();
    assert_eq!(
        listed,
        vec![
            (dest.join("a.txt"), true, false),
            (dest.join("b.txt"), true, false),
            (dest.join("d"), true, false),
        ]
    );
    let key = |i: usize| question.clashes[i].key();
    let answer = Answer {
        policy: Policy::default(),
        each: HashMap::from([(key(1), Choice::Replace), (key(2), Choice::Skip)]),
    };
    assert!(app.answer_clashes(question.id, Some(answer)));
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished { error: None, .. }
    ));
    assert_eq!(read(&dest.join("a.txt")), "a, theirs", "kept");
    assert_eq!(
        std::fs::read_link(dest.join("a (link).txt")).unwrap(),
        src.join("a.txt"),
        "both"
    );
    assert_eq!(
        std::fs::read_link(dest.join("b.txt")).unwrap(),
        src.join("b.txt"),
        "replaced"
    );
    assert!(
        dest.join("d").symlink_metadata().unwrap().is_dir(),
        "skipped"
    );
    assert_eq!(read(&dest.join("d/x.txt")), "x, theirs");

    undo(&app);
    assert_eq!(snapshot(&root), before, "all of it taken back");
}

/// A link into its own folder is kept beside what it leads to, never
/// asked; a link already made is not made again, nor asked about.
#[cfg(unix)]
#[test]
fn a_link_beside_itself_or_made_already_is_never_asked_about() {
    let tmp = Scratch::new();
    let root = tmp.path().join("it's here");
    write(&root.join("notes.txt"), "mine");
    write(&root.join("dest/other.txt"), "theirs");
    let app = app(tmp.path());
    app.set_window_shown();
    app.apply_whole(transfer(&app, "link", &[root.join("notes.txt")], &root));
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished { error: None, .. }
    ));
    assert_eq!(
        std::fs::read_link(root.join("notes (link).txt")).unwrap(),
        root.join("notes.txt")
    );

    let entries = [root.join("dest/other.txt")];
    for applied in [1, 0] {
        app.apply_whole(transfer(&app, "link", &entries, &root));
        assert!(
            matches!(finished(&app), ExecEvent::Finished { applied: a, error: None, .. } if a == applied)
        );
    }
    assert!(app.take_clash_questions().is_empty(), "nothing asked");
    assert_eq!(
        std::fs::read_link(root.join("other.txt")).unwrap(),
        root.join("dest/other.txt")
    );
}

/// An agent says beforehand what becomes of a name a link would take.
#[cfg(unix)]
#[test]
fn a_link_s_policy_is_said_beforehand() {
    let tmp = Scratch::new();
    let root = tmp.path();
    write(&root.join("a.txt"), "mine");
    write(&root.join("dest/a.txt"), "theirs");
    let app = app(root);
    let plan = plan(
        &app,
        "link",
        serde_json::json!({
            "entries": [root.join("a.txt")],
            "dest": root.join("dest"),
            "on_conflict": { "files": "keep_both" },
        }),
    );
    let diff = plan.diff(&std::sync::Arc::default());
    assert_eq!(diff.steps.len(), 1);
    assert!(
        matches!(&diff.steps[0], (Change::Link { to, .. }, StepStatus::Pending { .. })
            if *to == root.join("dest/a (link).txt")),
        "{:?}",
        diff.steps
    );
    // With no window and no policy, nothing is applied.
    app.apply_whole(transfer(
        &app,
        "link",
        &[root.join("a.txt")],
        &root.join("dest"),
    ));
    assert!(matches!(
        finished(&app),
        ExecEvent::Finished {
            applied: 0,
            error: Some(_),
            ..
        }
    ));
    assert_eq!(read(&root.join("dest/a.txt")), "theirs");
}
