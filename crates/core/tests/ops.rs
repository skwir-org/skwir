//! Operation properties: inverse(apply(plan)) restores the tree, applying
//! twice is a no-op, and plans round-trip through their serialized form.

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bibli_core::app::App;
#[cfg(target_os = "linux")]
use bibli_core::op::Restore;
use bibli_core::op::{
    Copy, EntryRef, ExecEvent, Executor, Mkdir, Move, Operation, Plan, Rename, StepContext,
    StepStatus, Swap, Trash,
};
use common::*;
use proptest::prelude::*;

/// Undoing a copy moves the copy to the trash. Point the freedesktop trash
/// at a scratch directory beside the test trees (the home trash is used for
/// files on its own device), so no test fills the user's own.
fn isolate_trash() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let data = dirs::home_dir()
            .expect("home")
            .join(".cache/bibli-tests/xdg-data");
        std::fs::create_dir_all(&data).expect("create test data home");
        // SAFETY: nextest runs each test in a process of its own, and this
        // runs before the test starts an app, so no other thread reads the
        // environment concurrently.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("XDG_DATA_HOME", data);
        }
    });
}

/// A scratch tree for one test, with the trash isolated; items the test
/// left in the trash are purged when it ends.
struct Scratch(tempfile::TempDir);

impl Scratch {
    fn new() -> Self {
        isolate_trash();
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
        if let Ok(items) = trash::os_limited::list() {
            let ours = items
                .into_iter()
                .filter(|i| i.original_path().starts_with(self.0.path()));
            let _ = trash::os_limited::purge_all(ours);
        }
    }
    #[cfg(target_os = "macos")]
    fn drop(&mut self) {}
}

/// A random small tree: files f0..fN in root and in sub dirs d0..dM.
#[derive(Clone, Debug)]
struct Tree {
    dirs: usize,
    files: usize,
}

fn build(root: &Path, t: &Tree) {
    for d in 0..t.dirs {
        std::fs::create_dir(root.join(format!("d{d}"))).unwrap();
    }
    for f in 0..t.files {
        std::fs::write(root.join(format!("f{f}")), format!("content {f}")).unwrap();
        // Every other file has a companion beside it (ADR-028), which goes
        // where it goes and comes back with it.
        if f % 2 == 0 {
            std::fs::write(root.join(format!("._f{f}")), format!("aside {f}")).unwrap();
        }
        for d in 0..t.dirs {
            std::fs::write(root.join(format!("d{d}/f{f}")), format!("content {d}/{f}")).unwrap();
        }
    }
}

/// `._x` beside `x`, holding its attributes when `holds`.
struct Aside {
    holds: bool,
}

impl bibli_core::companions::Companion for Aside {
    fn id(&self) -> &'static str {
        "aside"
    }
    fn of(&self, name: &std::ffi::OsStr) -> Vec<PathBuf> {
        let mut aside = OsString::from("._");
        aside.push(name);
        vec![aside.into()]
    }
    fn owner(&self, name: &std::ffi::OsStr) -> Option<OsString> {
        let s = name.to_str()?.strip_prefix("._")?;
        (!s.is_empty()).then(|| s.into())
    }
    fn holds_attributes(&self) -> bool {
        self.holds
    }
    /// One attribute per line, `name=value`.
    fn attributes(&self, path: &Path) -> Option<Vec<(String, Vec<u8>)>> {
        let mut aside = OsString::from("._");
        aside.push(path.file_name()?);
        let text = std::fs::read_to_string(path.with_file_name(aside)).ok()?;
        Some(
            text.lines()
                .filter_map(|l| l.split_once('='))
                .map(|(n, v)| (n.to_owned(), v.as_bytes().to_vec()))
                .collect(),
        )
    }
}

#[derive(Clone, Debug)]
enum Step {
    Rename { file: usize, to: String },
    Move { file: usize, dir: usize },
    Copy { file: usize, dir: usize },
    Swap { a: usize, b: usize },
    Mkdir { name: String },
}

fn steps(t: &Tree) -> impl Strategy<Value = Vec<Step>> + use<> {
    let (files, dirs) = (t.files, t.dirs);
    let step = prop_oneof![
        (0..files, "[a-z]{3,6}").prop_map(|(file, to)| Step::Rename {
            file,
            to: format!("r_{to}")
        }),
        (0..files, 0..dirs.max(1)).prop_map(|(file, dir)| Step::Move { file, dir }),
        (0..files, 0..dirs.max(1)).prop_map(|(file, dir)| Step::Copy { file, dir }),
        (0..files, 0..files).prop_map(move |(a, b)| Step::Swap {
            a,
            b: if a == b { (a + 1) % files } else { b }
        }),
        "[a-z]{3,6}".prop_map(|name| Step::Mkdir {
            name: format!("m_{name}")
        }),
    ];
    prop::collection::vec(step, 1..6)
}

fn file_ref(root: &Path, f: usize) -> Option<EntryRef> {
    EntryRef::from_path(root.join(format!("f{f}")))
}

/// Turn abstract steps into a plan against the *current* tree. Steps whose
/// source is gone (moved by an earlier step) are dropped: that is what a UI
/// would do, since it only offers operations on entries it can see.
fn make_plan(root: &Path, t: &Tree, steps: &[Step]) -> Plan {
    let mut plan = Plan::new("prop");
    for s in steps {
        let op: Option<Box<dyn Operation>> = match s {
            Step::Rename { file, to } => file_ref(root, *file).map(|e| {
                Box::new(Rename {
                    entry: e,
                    new_name: to.into(),
                }) as Box<dyn Operation>
            }),
            Step::Move { file, dir } if t.dirs > 0 => file_ref(root, *file).map(|e| {
                Box::new(Move {
                    entry: e,
                    dest_dir: root.join(format!("d{dir}")),
                }) as Box<dyn Operation>
            }),
            Step::Copy { file, dir } if t.dirs > 0 => file_ref(root, *file).map(|e| {
                Box::new(Copy {
                    entry: e,
                    dest_dir: root.join(format!("d{dir}")),
                    new_name: Some(format!("copy_of_f{file}").into()),
                    preserve_attributes: true,
                }) as Box<dyn Operation>
            }),
            Step::Swap { a, b } if a != b => match (file_ref(root, *a), file_ref(root, *b)) {
                (Some(a), Some(b)) => Some(Box::new(Swap { a, b })),
                _ => None,
            },
            Step::Mkdir { name } => Some(Box::new(Mkdir {
                path: root.join(name),
            })),
            _ => None,
        };
        if let Some(op) = op {
            plan.steps.push(op);
        }
    }
    plan
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    #[test]
    fn apply_then_undo_restores_the_tree(t in (0usize..3, 1usize..4).prop_map(|(dirs, files)| Tree { dirs, files }), steps in (0usize..3, 1usize..4).prop_flat_map(|(dirs, files)| steps(&Tree { dirs, files }))) {
        if under_wine("trashing through the shell") {
            return Ok(());
        }
        let tmp = Scratch::new();
        let root: PathBuf = tmp.path().join("tree");
        std::fs::create_dir(&root).unwrap();
        build(&root, &t);
        let app = app(tmp.path());
        app.companions.register("test", std::sync::Arc::new(Aside { holds: false }));
        let before = snapshot(&root);

        let plan = make_plan(&root, &t, &steps);
        let plan_data = plan.to_data();
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        // A conflict (e.g. rename onto an existing name) stops the plan; what
        // was applied must still undo cleanly.
        let applied = r.applied;
        let after = snapshot(&root);
        if applied > 0 {
            // Applied operations may cancel each other (for example two swaps).
            // Verify undo/redo against snapshots even when before == after.
            let u = app.executor.undo_blocking(app.lifetime()).expect("journal has the plan");
            prop_assert!(u.error.is_none(), "undo failed: {:?}", u.error);
            prop_assert_eq!(snapshot(&root), before.clone(), "undo must restore the tree");

            // Redo takes the undo back: it replays what was applied (not the
            // steps a conflict stopped), so it succeeds; undo again restores.
            let rd = app.executor.redo_blocking(app.lifetime()).expect("redo available");
            prop_assert!(rd.error.is_none(), "redo failed: {:?}", rd.error);
            prop_assert_eq!(snapshot(&root), after, "redo must reproduce the applied state");
            let u = app.executor.undo_blocking(app.lifetime()).unwrap();
            prop_assert!(u.error.is_none(), "second undo failed: {:?}", u.error);
            prop_assert_eq!(snapshot(&root), before, "second undo must restore the tree");
        }

        // Idempotency: re-applying an applied plan changes nothing and applies
        // nothing. This holds when each entry is touched once. Chains (rename
        // A->B then move B elsewhere) carry stale path hints in later steps and
        // are reported as conflicts on re-apply, which is the honest answer;
        // they are still fully undoable and redoable, asserted above.
        if r.error.is_none() && !steps_share_an_entry(&plan_data) {
            let redo = app.executor.redo_blocking(app.lifetime());
            prop_assert!(redo.is_none() || redo.as_ref().unwrap().error.is_none());
            let again = Plan::from_data(&plan_data, &app.ops).unwrap();
            let applied_state = snapshot(&root);
            let r2 = app.executor.execute_blocking(&again, app.lifetime());
            prop_assert!(r2.error.is_none(), "re-apply failed: {:?}", r2.error);
            prop_assert_eq!(r2.applied, 0, "re-applying an applied plan must apply nothing");
            prop_assert_eq!(snapshot(&root), applied_state, "re-applying must leave the tree as it was");
        }
    }
}

fn steps_share_an_entry(plan: &bibli_core::op::PlanData) -> bool {
    let mut ids: Vec<String> = plan
        .steps
        .iter()
        .flat_map(|s| ["/entry/id", "/a/id", "/b/id"].map(|p| s.args.pointer(p)))
        .flatten()
        .map(ToString::to_string)
        .collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    ids.len() != n
}

/// A sidecar's place taken is a conflict in the review, as its file's is:
/// a dry run knows the rules the plan is applied with, so the plan does
/// not stop halfway.
#[test]
fn a_companion_s_place_taken_is_a_conflict_before_anything_applies() {
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir(root.join("d")).unwrap();
    std::fs::write(root.join("it's a.txt"), b"a").unwrap();
    std::fs::write(root.join("._it's a.txt"), b"tags").unwrap();
    std::fs::write(root.join("d/._it's a.txt"), b"other").unwrap();
    let app = app(root);
    app.companions
        .register("test", std::sync::Arc::new(Aside { holds: false }));
    let mut plan = Plan::new("t");
    plan.push(Move {
        entry: EntryRef::from_path(root.join("it's a.txt")).unwrap(),
        dest_dir: root.join("d"),
    });
    let diff = plan.diff(&app.companions);
    assert!(
        matches!(diff.steps[0].1, StepStatus::Conflict { .. }),
        "{:?}",
        diff.steps[0].1
    );
    assert!(!plan.is_applicable(&app.companions));
}

#[test]
fn plan_round_trips_through_json_and_diff_reports_conflicts() {
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::write(root.join("a"), b"a").unwrap();
    std::fs::write(root.join("b"), b"b").unwrap();
    let app = app(root);
    let mut plan = Plan::new("t");
    plan.push(Rename {
        entry: EntryRef::from_path(root.join("a")).unwrap(),
        new_name: "b".into(),
    });
    plan.push(Mkdir {
        path: root.join("new"),
    });
    let json = serde_json::to_string(&plan.to_data()).unwrap();
    let back = Plan::from_data(&serde_json::from_str(&json).unwrap(), &app.ops).unwrap();
    assert_eq!(back.to_data(), plan.to_data());
    let diff = back.diff(&std::sync::Arc::default());
    assert!(
        matches!(diff.steps[0].1, StepStatus::Conflict { .. }),
        "rename onto existing must conflict"
    );
    assert_eq!(diff.steps[1].1, StepStatus::Pending { reversible: true });
    assert!(!back.is_applicable(&std::sync::Arc::default()));
}

#[test]
fn entry_ref_follows_a_rename_and_rejects_a_replacement() {
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::write(root.join("a"), b"a").unwrap();
    let r = EntryRef::from_path(root.join("a")).unwrap();
    std::fs::rename(root.join("a"), root.join("z")).unwrap();
    assert_eq!(
        r.resolve().unwrap(),
        root.join("z"),
        "same inode found under its new name"
    );
    std::fs::write(root.join("a"), b"impostor").unwrap();
    // The hint path now holds a different file, but the original still exists as z.
    assert_eq!(r.resolve().unwrap(), root.join("z"));
    std::fs::remove_file(root.join("z")).unwrap();
    assert!(matches!(
        r.resolve(),
        Err(bibli_core::op::ResolveError::Replaced { .. })
    ));
}

/// A file with several names (hard links) is never followed from the
/// name referenced to another of its names: that name gone, the
/// reference is gone too, and replaced, it says so, though another name
/// of the file is beside it.
#[cfg(unix)]
#[test]
fn entry_ref_never_follows_a_file_to_another_of_its_names() {
    use bibli_core::op::ResolveError;
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::write(root.join("a.txt"), b"one file").unwrap();
    std::fs::hard_link(root.join("a.txt"), root.join("b.txt")).unwrap();
    let b = EntryRef::from_path(root.join("b.txt")).unwrap();
    assert!(!b.only_name, "two names");
    std::fs::remove_file(root.join("b.txt")).unwrap();
    assert!(
        matches!(b.resolve(), Err(ResolveError::Gone(_))),
        "never a.txt"
    );
    std::fs::write(root.join("b.txt"), b"another file").unwrap();
    assert!(matches!(b.resolve(), Err(ResolveError::Replaced { .. })));
    // Knowing nothing of the file's names, a reference resolves at its
    // path alone.
    let id = bibli_core::fs::stat::id_of(&root.join("a.txt")).unwrap();
    let a = EntryRef::new(id, root.join("a.txt"));
    std::fs::rename(root.join("a.txt"), root.join("c.txt")).unwrap();
    assert!(matches!(a.resolve(), Err(ResolveError::Gone(_))));
}

/// A file of one name is followed to its new name in its folder while one
/// name there holds it: two, one made since, could be either. A folder
/// has one name whatever its link count says.
#[cfg(unix)]
#[test]
fn entry_ref_follows_one_name_only_while_one_name_holds_it() {
    use bibli_core::op::ResolveError;
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::write(root.join("x"), b"x").unwrap();
    let r = EntryRef::from_path(root.join("x")).unwrap();
    assert!(r.only_name);
    std::fs::rename(root.join("x"), root.join("y")).unwrap();
    assert_eq!(r.resolve().unwrap(), root.join("y"));
    std::fs::hard_link(root.join("y"), root.join("z")).unwrap();
    assert!(matches!(r.resolve(), Err(ResolveError::Gone(_))));
    std::fs::create_dir_all(root.join("dir/sub")).unwrap();
    let d = EntryRef::from_path(root.join("dir")).unwrap();
    assert!(d.only_name, "its link count counts its subfolders");
    std::fs::rename(root.join("dir"), root.join("moved")).unwrap();
    assert_eq!(d.resolve().unwrap(), root.join("moved"));
}

/// A plan to trash, or to delete, a name of a file that has two leaves
/// the other name alone when the name it named is gone before it applies.
// The freedesktop trash (ADR-035).
#[cfg(target_os = "linux")]
#[test]
fn trashing_or_deleting_a_name_gone_spares_the_files_other_name() {
    let tmp = Scratch::new();
    let root = tmp.path().join("tree");
    std::fs::create_dir_all(&root).unwrap();
    let app = app(tmp.path());
    for how in ["trash", "delete"] {
        std::fs::write(root.join("a.txt"), b"kept").unwrap();
        std::fs::hard_link(root.join("a.txt"), root.join("b.txt")).unwrap();
        let entry = EntryRef::from_path(root.join("b.txt")).unwrap();
        let step: Box<dyn Operation> = if how == "trash" {
            Box::new(Trash { entry })
        } else {
            Box::new(bibli_core::op::Delete { entry })
        };
        let plan = Plan::with_steps(how, vec![step]);
        std::fs::remove_file(root.join("b.txt")).unwrap();
        let r = apply(&app, &plan);
        assert_eq!(r.applied, 0, "{how}: nothing left to do");
        assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"kept", "{how}");
        std::fs::remove_file(root.join("a.txt")).unwrap();
    }
}

/// A copy keeps the links among what it copies, as `cp -a` does
/// (ADR-049): two names of one file in a folder copied are two names of
/// one copy, two names copied as two items of a plan too, and a name
/// copied alone is a file of its own.
#[cfg(unix)]
#[test]
fn a_copy_keeps_the_links_among_what_it_copies() {
    use std::os::unix::fs::MetadataExt;
    let tmp = Scratch::new();
    let root = tmp.path().join("tree");
    std::fs::create_dir_all(root.join("album")).unwrap();
    std::fs::write(root.join("album/a.jpg"), b"one picture").unwrap();
    std::fs::hard_link(root.join("album/a.jpg"), root.join("album/b.jpg")).unwrap();
    let app = app(tmp.path());
    let copy = |path: &Path, dest: &str| -> Box<dyn Operation> {
        std::fs::create_dir_all(root.join(dest)).unwrap();
        Box::new(Copy {
            entry: EntryRef::from_path(path).unwrap(),
            dest_dir: root.join(dest),
            new_name: None,
            preserve_attributes: true,
        })
    };
    let ino = |path: &str| std::fs::metadata(root.join(path)).unwrap().ino();
    let links = |path: &str| std::fs::metadata(root.join(path)).unwrap().nlink();

    apply(
        &app,
        &Plan::with_steps("copy", vec![copy(&root.join("album"), "folder")]),
    );
    assert_eq!(ino("folder/album/a.jpg"), ino("folder/album/b.jpg"));
    assert_ne!(ino("folder/album/a.jpg"), ino("album/a.jpg"), "a copy");
    assert_eq!(links("folder/album/a.jpg"), 2);

    apply(
        &app,
        &Plan::with_steps(
            "copy",
            vec![
                copy(&root.join("album/a.jpg"), "items"),
                copy(&root.join("album/b.jpg"), "items"),
            ],
        ),
    );
    assert_eq!(ino("items/a.jpg"), ino("items/b.jpg"));
    assert_eq!(
        std::fs::read(root.join("items/b.jpg")).unwrap(),
        b"one picture"
    );

    apply(
        &app,
        &Plan::with_steps("copy", vec![copy(&root.join("album/a.jpg"), "alone")]),
    );
    assert_eq!(links("alone/a.jpg"), 1, "a copy of its own");
}

/// A copy the user asked for, short of room where it writes, asks first
/// whether to go on (ADR-049): cancelled, nothing is copied and the plan
/// says why; told to go on, it copies.
#[test]
fn a_copy_short_of_room_asks_whether_to_go_on() {
    let tmp = Scratch::new();
    let root = tmp.path().join("tree");
    std::fs::create_dir_all(root.join("dest")).unwrap();
    std::fs::write(root.join("a.bin"), [0u8; 1000]).unwrap();
    let app = app(tmp.path());
    app.executor.set_free(std::sync::Arc::new(|_| Some(10)));
    let plan = || {
        Plan::with_steps(
            "copy “a.bin”",
            vec![Box::new(Copy {
                entry: EntryRef::from_path(root.join("a.bin")).unwrap(),
                dest_dir: root.join("dest"),
                new_name: None,
                preserve_attributes: true,
            }) as Box<dyn Operation>],
        )
    };
    let question = || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(q) = app.take_questions().pop() {
                return q;
            }
            assert!(std::time::Instant::now() < deadline, "never asked");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };

    app.apply_whole(plan());
    let asked = question();
    assert_eq!(asked.asker, bibli_core::app::Asker::Room);
    assert_eq!(
        asked.title,
        "Not enough room in “dest”: 1000 B needed, 10 B free"
    );
    app.answer(asked.id, false);
    let events = events_until(&app, |e| matches!(e, ExecEvent::Finished { .. }));
    assert!(matches!(
        events.last(),
        Some(ExecEvent::Finished { applied: 0, error: Some(e), .. }) if e.starts_with("Not enough room")
    ));
    assert!(!root.join("dest/a.bin").exists(), "nothing copied");

    app.apply_whole(plan());
    app.answer(question().id, true);
    events_until(&app, |e| matches!(e, ExecEvent::Finished { .. }));
    assert_eq!(std::fs::read(root.join("dest/a.bin")).unwrap().len(), 1000);
}

/// Deleting a name of a file for good says that its file stays on the
/// disk while another of its names is not deleted with it (ADR-049): what
/// the confirmation shows, told from the listing's link counts.
#[cfg(unix)]
#[test]
fn deleting_one_name_of_a_file_says_it_stays_on_the_disk() {
    let tmp = Scratch::new();
    let root = tmp.path().join("tree");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), b"one file").unwrap();
    std::fs::hard_link(root.join("a.txt"), root.join("b.txt")).unwrap();
    std::fs::write(root.join("c.txt"), b"other").unwrap();
    let app = app(tmp.path());
    navigate(&app, &root);
    let pane = active(&app);
    let ids: Vec<_> = {
        let ws = app.workspace.read();
        ws.pane(pane).unwrap().listing.ids().collect()
    };
    app.dispatch(&bibli_core::command::Invocation::with_args(
        "hydrate",
        &serde_json::json!({ "ids": ids }),
    ))
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app
        .workspace
        .read()
        .pane(pane)
        .unwrap()
        .listing
        .iter()
        .any(|e| e.metadata.value(bibli_core::entry::keys::NLINK).is_none())
    {
        assert!(std::time::Instant::now() < deadline, "never stat-ed");
        app.pump();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    select(&app, &["a.txt", "c.txt"]);
    let one = plan(&app, "delete", serde_json::json!({}));
    assert_eq!(
        one.note.as_deref(),
        Some("1 file has names elsewhere: 8 B stays on the disk")
    );
    select(&app, &["a.txt", "b.txt", "c.txt"]);
    assert_eq!(
        plan(&app, "delete", serde_json::json!({})).note,
        None,
        "every name goes"
    );
}

/// A step that cancels or pauses its own plan while it runs, so the test
/// needs no timing.
#[derive(Debug, Clone)]
struct ActHere {
    executor: std::sync::Weak<Executor>,
    plan: std::sync::Arc<std::sync::OnceLock<bibli_core::op::PlanId>>,
    act: fn(&Executor, bibli_core::op::PlanId) -> bool,
    dir: PathBuf,
}

impl Operation for ActHere {
    fn kind(&self) -> &'static str {
        "test.act_here"
    }
    fn args(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn describe(&self) -> bibli_core::op::Change {
        Mkdir {
            path: self.dir.clone(),
        }
        .describe()
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        Mkdir {
            path: self.dir.clone(),
        }
        .check(ctx)
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let executor = self.executor.upgrade().expect("executor alive");
        assert!((self.act)(
            &executor,
            *self.plan.get().expect("plan id set")
        ));
        Mkdir {
            path: self.dir.clone(),
        }
        .apply(ctx)
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(Clone::clone(self))
    }
}

#[test]
fn a_running_plan_is_cancelled_by_id_and_its_partial_result_can_be_undone() {
    let tmp = Scratch::new();
    let app = app(tmp.path());
    let id = std::sync::Arc::new(std::sync::OnceLock::new());
    let dirs: Vec<PathBuf> = (0..4)
        .map(|i| tmp.path().join(format!("folder {i}")))
        .collect();
    let mut plan = Plan::new("create folders");
    plan.push(Mkdir {
        path: dirs[0].clone(),
    });
    plan.push(ActHere {
        executor: std::sync::Arc::downgrade(&app.executor),
        plan: std::sync::Arc::clone(&id),
        act: Executor::cancel,
        dir: dirs[1].clone(),
    });
    plan.push(Mkdir {
        path: dirs[2].clone(),
    });
    plan.push(Mkdir {
        path: dirs[3].clone(),
    });
    id.set(plan.id).unwrap();

    let result = app
        .executor
        .execute_blocking(&plan, &app.lifetime().child());
    assert!(result.cancelled);
    assert_eq!(
        result.applied, 2,
        "the running step completes, later ones do not start"
    );
    assert!(dirs[1].is_dir() && !dirs[2].exists());
    assert!(
        !app.executor.cancel(plan.id),
        "a finished plan is not running"
    );

    let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert_eq!(undone.applied, 2);
    assert!(!dirs[0].exists() && !dirs[1].exists());
}

#[test]
fn winding_down_stops_a_paused_copy_and_leaves_no_staging_folder() {
    let tmp = Scratch::new();
    let root = tmp.path();
    let app = app(root);
    let album = root.join("album");
    std::fs::create_dir(&album).unwrap();
    for i in 0..4 {
        std::fs::write(album.join(format!("photo {i}.jpg")), vec![7u8; 4096]).unwrap();
    }
    let stick = root.join("stick");
    std::fs::create_dir(&stick).unwrap();
    let plan = Plan::with_steps(
        "copy",
        vec![Box::new(Copy {
            entry: EntryRef::from_path(&album).unwrap(),
            dest_dir: stick.clone(),
            new_name: None,
            preserve_attributes: true,
        })],
    );
    // Paused once its first bytes are counted: the copy is half made, in
    // its staging folder, when it waits.
    let id = plan.id;
    let executor = std::sync::Arc::downgrade(&app.executor);
    app.executor.set_waker(Some(std::sync::Arc::new(move || {
        if let Some(executor) = executor.upgrade()
            && executor.transfer(id).is_some_and(|t| t.done > 0)
        {
            executor.pause(id);
        }
    })));
    app.executor.execute(plan, app.lifetime().child());
    events_until(&app, |e| matches!(e, ExecEvent::Paused { .. }));
    let staged = |dir: &Path| {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with(".bibli-copy-"))
    };
    assert!(staged(&stick), "the copy is under way");

    assert!(app.executor.wind_down(std::time::Duration::from_secs(10)));
    let rest = events_until(&app, |e| matches!(e, ExecEvent::Finished { .. }));
    assert!(matches!(
        rest.last(),
        Some(ExecEvent::Finished {
            applied: 0,
            cancelled: true,
            ..
        })
    ));
    assert_eq!(
        std::fs::read_dir(&stick).unwrap().count(),
        0,
        "nothing left"
    );
    assert_eq!(std::fs::read_dir(&album).unwrap().count(), 4);

    // Wound down, it starts nothing more.
    let late = Plan::with_steps(
        "make a folder",
        vec![Box::new(Mkdir {
            path: stick.join("late"),
        })],
    );
    app.executor.execute(late, app.lifetime().child());
    let late = events_until(&app, |e| matches!(e, ExecEvent::Finished { .. }));
    assert!(matches!(
        late.last(),
        Some(ExecEvent::Finished {
            applied: 0,
            cancelled: true,
            ..
        })
    ));
    assert!(!stick.join("late").exists());
}

/// A staging folder a copy left when its process ended mid-way: its lock
/// free, part of the copy in it.
#[cfg(target_os = "linux")]
fn left_by_a_copy(dest: &Path, name: &str) -> PathBuf {
    let left = dest.join(name);
    std::fs::create_dir_all(left.join("entry")).unwrap();
    std::fs::write(left.join("entry/photo 1.jpg"), b"part").unwrap();
    std::fs::write(left.join("lock"), b"").unwrap();
    left
}

#[cfg(target_os = "linux")]
#[test]
fn a_copy_and_a_new_run_sweep_what_copies_cut_short_left() {
    let tmp = Scratch::new();
    let root = tmp.path();
    let stick = root.join("stick");
    std::fs::create_dir(&stick).unwrap();
    let left = left_by_a_copy(&stick, ".bibli-copy-AbC123");
    // Another bibli still copying there: its lock held.
    let running = stick.join(".bibli-copy-XyZ789");
    std::fs::create_dir(&running).unwrap();
    let held = std::fs::File::create(running.join("lock")).unwrap();
    held.try_lock().unwrap();

    let app = app(root);
    let src = file(&root.join("it's a draft.txt"), "draft");
    let copy = Copy {
        entry: src,
        dest_dir: stick.clone(),
        new_name: None,
        preserve_attributes: true,
    };
    apply(&app, &Plan::with_steps("copy", vec![Box::new(copy)]));
    assert!(stick.join("it's a draft.txt").is_file());
    assert!(!left.exists(), "a copy into the folder sweeps it");
    assert!(running.is_dir(), "a copy that runs keeps its folder");
    drop(held);
    drop(app);

    // A run that ended mid-copy noted the folder in the cache: the next
    // one sweeps it as it starts.
    let config = bibli_core::app::AppConfig::ephemeral(root);
    let notes = bibli_core::cache::Cache::open(config.cache_root.clone())
        .namespace("staging")
        .folder("under-way");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(
        notes.join("copy-ended"),
        stick.as_os_str().as_encoded_bytes(),
    )
    .unwrap();
    let left = left_by_a_copy(&stick, ".bibli-copy-DeF456");
    let next = bibli_core::app::App::new(config).unwrap();
    assert!(next.scheduler.wait_idle(std::time::Duration::from_secs(10)));
    assert!(!left.exists() && !running.exists(), "both copies are over");
    assert!(!notes.join("copy-ended").exists());
    assert!(stick.join("it's a draft.txt").is_file());
}

/// Three folders to create, the first step pausing its own plan.
fn pausing_plan(app: &std::sync::Arc<App>, root: &Path) -> (Plan, Vec<PathBuf>) {
    let id = std::sync::Arc::new(std::sync::OnceLock::new());
    let dirs: Vec<PathBuf> = (0..3).map(|i| root.join(format!("folder {i}"))).collect();
    let mut plan = Plan::new("create folders");
    plan.push(ActHere {
        executor: std::sync::Arc::downgrade(&app.executor),
        plan: std::sync::Arc::clone(&id),
        act: Executor::pause,
        dir: dirs[0].clone(),
    });
    for dir in &dirs[1..] {
        plan.push(Mkdir { path: dir.clone() });
    }
    id.set(plan.id).unwrap();
    (plan, dirs)
}

/// The executor's events up to the first that `last` accepts.
fn events_until(app: &App, last: impl Fn(&ExecEvent) -> bool) -> Vec<ExecEvent> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut events = Vec::new();
    loop {
        let event = app
            .executor
            .events()
            .recv_deadline(deadline)
            .expect("the plan goes on");
        let done = last(&event);
        events.push(event);
        if done {
            return events;
        }
    }
}

#[test]
fn a_paused_plan_waits_holding_no_worker_until_resumed() {
    let tmp = Scratch::new();
    let app = app(tmp.path());
    let (plan, dirs) = pausing_plan(&app, tmp.path());
    app.executor.execute(plan.clone(), app.lifetime().child());
    let first = events_until(&app, |e| matches!(e, ExecEvent::Step(_)));
    assert!(matches!(
        first.as_slice(),
        [
            ExecEvent::Started { .. },
            ExecEvent::Paused { .. },
            ExecEvent::Step(_)
        ]
    ));
    assert!(app.executor.is_paused(plan.id));
    assert!(
        app.scheduler.wait_idle(std::time::Duration::from_secs(5)),
        "a paused plan holds no worker"
    );
    // A held plan never goes on; one that did not wait would by then.
    assert!(
        app.executor
            .events()
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err(),
        "nothing happens while paused"
    );
    assert!(dirs[0].is_dir() && !dirs[1].exists());

    assert!(app.executor.resume(plan.id));
    assert!(!app.executor.is_paused(plan.id));
    let rest = events_until(&app, |e| matches!(e, ExecEvent::Finished { .. }));
    assert!(matches!(rest.first(), Some(ExecEvent::Resumed { .. })));
    assert!(matches!(
        rest.last(),
        Some(ExecEvent::Finished {
            applied: 3,
            cancelled: false,
            error: None,
            ..
        })
    ));
    assert!(dirs[2].is_dir());
}

#[test]
fn a_paused_plan_cancelled_stops_where_it_waits() {
    let tmp = Scratch::new();
    let app = app(tmp.path());
    let (plan, dirs) = pausing_plan(&app, tmp.path());
    app.executor.execute(plan.clone(), app.lifetime().child());
    events_until(&app, |e| matches!(e, ExecEvent::Step(_)));
    assert!(app.executor.cancel(plan.id));
    let rest = events_until(&app, |e| matches!(e, ExecEvent::Finished { .. }));
    assert!(matches!(
        rest.as_slice(),
        [ExecEvent::Finished {
            applied: 1,
            cancelled: true,
            ..
        }]
    ));
    assert!(dirs[0].is_dir() && !dirs[1].exists());
    assert!(
        !app.executor.resume(plan.id),
        "a finished plan is not running"
    );
}

fn file(path: &Path, content: &str) -> EntryRef {
    std::fs::write(path, content).unwrap();
    EntryRef::from_path(path).unwrap()
}

#[test]
fn undos_run_in_journal_order_after_the_plans_before_them() {
    let tmp = Scratch::new();
    let root = tmp.path();
    let app = app(root);
    let a = file(&root.join("a"), "a");
    // Queue two dependent renames and two undos without waiting: the lane
    // runs them in order, so the second undo sees the first plan's result.
    let first = Plan::with_steps(
        "rename a",
        vec![Box::new(Rename {
            entry: a.clone(),
            new_name: "b".into(),
        })],
    );
    let second = Plan::with_steps(
        "rename b",
        vec![Box::new(Rename {
            entry: EntryRef::new(a.id, root.join("b")),
            new_name: "c".into(),
        })],
    );
    app.executor.execute(first, app.lifetime().child());
    app.executor.execute(second, app.lifetime().child());
    assert!(app.executor.undo(app.lifetime().child()));
    assert!(app.executor.undo(app.lifetime().child()));
    // A blocking call queues behind them all.
    assert!(app.executor.undo_blocking(app.lifetime()).is_none());
    assert!(root.join("a").exists(), "both renames undone, newest first");
    assert!(!root.join("b").exists() && !root.join("c").exists());
    assert!(app.executor.can_redo());
}

#[test]
fn undo_of_a_copy_puts_it_in_the_trash_and_redo_brings_it_back() {
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir(root.join("dest")).unwrap();
    let app = app(root);
    let src = file(&root.join("a.txt"), "original");
    let copy = Copy {
        entry: src,
        dest_dir: root.join("dest"),
        new_name: None,
        preserve_attributes: true,
    };
    apply(&app, &Plan::with_steps("copy", vec![Box::new(copy)]));
    let copied = root.join("dest/a.txt");
    std::fs::write(&copied, "edited since").unwrap();
    let u = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(u.error.is_none(), "{:?}", u.error);
    assert!(!copied.exists());
    let r = app.executor.redo_blocking(app.lifetime()).unwrap();
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(
        std::fs::read_to_string(&copied).unwrap(),
        "edited since",
        "the copy comes back from the trash as it was"
    );
}

#[test]
fn trashing_many_and_undoing_restores_each() {
    if under_wine("trashing through the shell") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path();
    let app = app(root);
    let steps: Vec<Box<dyn Operation>> = (0..5)
        .map(|i| {
            Box::new(Trash {
                entry: file(&root.join(format!("t{i}")), "x"),
            }) as Box<dyn Operation>
        })
        .collect();
    apply(&app, &Plan::with_steps("trash", steps));
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
    let u = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(u.error.is_none(), "{:?}", u.error);
    assert_eq!(u.applied, 5);
    assert!((0..5).all(|i| root.join(format!("t{i}")).exists()));
}

#[test]
fn copying_onto_an_unrelated_folder_of_the_same_name_conflicts() {
    let tmp = Scratch::new();
    let root = tmp.path();
    for dir in ["Photos", "dest", "dest/Photos"] {
        std::fs::create_dir(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("Photos/one.jpg"), "one").unwrap();
    std::fs::write(root.join("dest/Photos/other.jpg"), "other").unwrap();
    let app = app(root);
    let copy = Copy {
        entry: EntryRef::from_path(root.join("Photos")).unwrap(),
        dest_dir: root.join("dest"),
        new_name: None,
        preserve_attributes: true,
    };
    let ctx = StepContext::detached();
    assert!(matches!(copy.check(&ctx), StepStatus::Conflict { .. }));

    // A finished copy of the same folder is recognised as done.
    std::fs::remove_dir_all(root.join("dest/Photos")).unwrap();
    let plan = Plan::with_steps("copy", vec![Box::new(copy.clone())]);
    apply(&app, &plan);
    assert_eq!(copy.check(&ctx), StepStatus::AlreadyDone);
    let again = app.executor.execute_blocking(&plan, app.lifetime());
    assert_eq!((again.applied, again.skipped), (0, 1));
}

/// A copy whose companion cannot be placed leaves nothing: the copy is
/// taken back out with the staging folder, so it is never left without
/// what goes with it, nor called done by a retry.
#[test]
fn a_copy_whose_companion_cannot_go_with_it_leaves_nothing() {
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir(root.join("dest")).unwrap();
    std::fs::write(root.join("it's a.txt"), b"a").unwrap();
    std::fs::write(root.join("._it's a.txt"), b"tags").unwrap();
    let copy = Copy {
        entry: EntryRef::from_path(root.join("it's a.txt")).unwrap(),
        dest_dir: root.join("dest"),
        new_name: None,
        preserve_attributes: true,
    };
    let companions = std::sync::Arc::new(bibli_core::companions::Companions::default());
    companions.register("test", std::sync::Arc::new(Aside { holds: false }));
    let ctx = StepContext::detached().with_companions(companions);
    assert_eq!(copy.check(&ctx), StepStatus::Pending { reversible: true });
    // Its place taken after the check, as by another application.
    std::fs::write(root.join("dest/._it's a.txt"), b"other").unwrap();
    assert!(copy.apply(&ctx).is_err());
    // The plan's staging folder goes with the plan.
    drop(ctx);
    let left: Vec<_> = std::fs::read_dir(root.join("dest"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, [std::ffi::OsString::from("._it's a.txt")]);
}

// Pipes are made with mknodat, Linux's alone here (ADR-035).
#[cfg(target_os = "linux")]
#[test]
fn a_named_pipe_is_copied_as_a_pipe_without_blocking() {
    use std::os::unix::fs::FileTypeExt;
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir(root.join("dest")).unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        root.join("src/pipe"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    let copy = Copy {
        entry: EntryRef::from_path(root.join("src")).unwrap(),
        dest_dir: root.join("dest"),
        new_name: None,
        preserve_attributes: true,
    };
    copy.apply(&StepContext::detached()).unwrap();
    assert!(
        std::fs::symlink_metadata(root.join("dest/src/pipe"))
            .unwrap()
            .file_type()
            .is_fifo()
    );
}

/// A plan's copies into one folder are staged in one folder of the plan's,
/// which goes with the plan: each copy placed out of it whole.
#[test]
fn copies_into_a_folder_share_one_staging_folder_that_goes_with_the_plan() {
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir(root.join("dest")).unwrap();
    let staging_folders = |dest: &Path| {
        std::fs::read_dir(dest)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".bibli-copy-")
            })
            .count()
    };
    let ctx = StepContext::detached();
    for name in ["a", "b's", "c d"] {
        std::fs::write(root.join(name), name).unwrap();
        Copy {
            entry: EntryRef::from_path(root.join(name)).unwrap(),
            dest_dir: root.join("dest"),
            new_name: None,
            preserve_attributes: true,
        }
        .apply(&ctx)
        .unwrap();
        assert_eq!(staging_folders(&root.join("dest")), 1, "one for the plan");
    }
    drop(ctx);
    assert_eq!(staging_folders(&root.join("dest")), 0);
    for name in ["a", "b's", "c d"] {
        assert_eq!(
            std::fs::read_to_string(root.join("dest").join(name)).unwrap(),
            name
        );
    }
}

/// A file with holes is copied with its holes: its data where it was, its
/// length, and no more blocks than it had.
// Holes are asked of Linux's filesystems (SEEK_DATA).
#[cfg(target_os = "linux")]
#[test]
fn a_file_with_holes_keeps_them_when_copied() {
    use std::os::unix::fs::{FileExt, MetadataExt};
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir(root.join("dest")).unwrap();
    let image = root.join("disk image.raw");
    let file = std::fs::File::create(&image).unwrap();
    let len = 64u64 << 20;
    file.set_len(len).unwrap();
    file.write_all_at(b"start", 0).unwrap();
    file.write_all_at(b"middle", 32 << 20).unwrap();
    file.sync_all().unwrap();
    drop(file);
    if std::fs::metadata(&image).unwrap().blocks() * 512 >= len {
        eprintln!("this filesystem keeps no holes: nothing to compare");
        return;
    }
    Copy {
        entry: EntryRef::from_path(&image).unwrap(),
        dest_dir: root.join("dest"),
        new_name: None,
        preserve_attributes: true,
    }
    .apply(&StepContext::detached())
    .unwrap();
    let copied = root.join("dest/disk image.raw");
    let meta = std::fs::metadata(&copied).unwrap();
    assert_eq!(meta.len(), len);
    assert!(meta.blocks() * 512 < len / 2, "{} blocks", meta.blocks());
    assert_eq!(
        std::fs::read(&copied).unwrap(),
        std::fs::read(&image).unwrap()
    );
}

#[test]
fn a_cancelled_copy_stops_and_leaves_nothing_behind() {
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir(root.join("dest")).unwrap();
    std::fs::write(root.join("src/big"), vec![7u8; 1 << 20]).unwrap();
    let copy = Copy {
        entry: EntryRef::from_path(root.join("src")).unwrap(),
        dest_dir: root.join("dest"),
        new_name: None,
        preserve_attributes: true,
    };
    let cancel = bibli_core::cancel::CancelToken::new();
    cancel.cancel();
    let err = copy.apply(&StepContext::with_cancel(cancel)).unwrap_err();
    assert!(err.is::<bibli_core::op::Cancelled>(), "{err:#}");
    assert_eq!(std::fs::read_dir(root.join("dest")).unwrap().count(), 0);
}

#[test]
fn names_that_leave_the_directory_are_rejected() {
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir(root.join("dest")).unwrap();
    let a = file(&root.join("a"), "a");
    let ctx = StepContext::detached();
    for bad in ["", ".", "..", "../x", "x/y", "nul\0"] {
        let rename = Rename {
            entry: a.clone(),
            new_name: bad.into(),
        };
        assert!(
            matches!(rename.check(&ctx), StepStatus::Conflict { .. }),
            "{bad:?}"
        );
        let copy = Copy {
            entry: a.clone(),
            dest_dir: root.join("dest"),
            new_name: Some(bad.into()),
            preserve_attributes: true,
        };
        assert!(
            matches!(copy.check(&ctx), StepStatus::Conflict { .. }),
            "{bad:?}"
        );
        assert!(copy.apply(&ctx).is_err(), "{bad:?}");
    }
    for bad in [root.join("dest/../x"), PathBuf::from("/")] {
        let mkdir = Mkdir { path: bad.clone() };
        assert!(
            matches!(mkdir.check(&ctx), StepStatus::Conflict { .. }),
            "{bad:?}"
        );
    }
    assert!(!root.join("x").exists());
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_utf8_survives_rename_undo_and_json() {
    use std::os::unix::ffi::OsStringExt;
    let tmp = Scratch::new();
    let root = tmp.path();
    let raw = OsString::from_vec(b"caf\xe9.txt".to_vec());
    let a = file(&root.join(&raw), "x");
    let app = app(root);
    let plan = Plan::with_steps(
        "rename",
        vec![Box::new(Rename {
            entry: a,
            new_name: "plain.txt".into(),
        })],
    );
    apply(&app, &plan);
    let u = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(u.error.is_none(), "{:?}", u.error);
    assert!(root.join(&raw).exists(), "the original bytes come back");

    let back = Rename {
        entry: EntryRef::from_path(root.join(&raw)).unwrap(),
        new_name: raw.clone(),
    };
    let json = serde_json::to_string(&back.args()).unwrap();
    let parsed: Rename = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, back);
}

// Windows cannot swap two names in one step, and refuses.
#[cfg(unix)]
#[test]
fn an_applied_swap_is_already_done() {
    let tmp = Scratch::new();
    let root = tmp.path();
    let app = app(root);
    let swap = Swap {
        a: file(&root.join("a"), "a"),
        b: file(&root.join("b"), "b"),
    };
    let plan = Plan::with_steps("swap", vec![Box::new(swap.clone())]);
    apply(&app, &plan);
    assert_eq!(
        swap.check(&StepContext::detached()),
        StepStatus::AlreadyDone
    );
    let again = app.executor.execute_blocking(&plan, app.lifetime());
    assert_eq!(again.applied, 0);
    assert_eq!(std::fs::read_to_string(root.join("a")).unwrap(), "b");
}

#[test]
fn renaming_onto_a_hard_link_of_itself_conflicts() {
    let tmp = Scratch::new();
    let root = tmp.path();
    let a = file(&root.join("a"), "a");
    std::fs::hard_link(root.join("a"), root.join("b")).unwrap();
    let rename = Rename {
        entry: a,
        new_name: "b".into(),
    };
    assert!(matches!(
        rename.check(&StepContext::detached()),
        StepStatus::Conflict { .. }
    ));
}

/// A directory on another device than `reference`, if the machine has one.
fn other_device(reference: &Path) -> Option<tempfile::TempDir> {
    let here = bibli_core::fs::stat::id_of(reference)?.dev;
    ["/dev/shm", "/tmp"].into_iter().find_map(|candidate| {
        let there = bibli_core::fs::stat::id_of(Path::new(candidate))?.dev;
        (there != here)
            .then(|| tempfile::tempdir_in(candidate).ok())
            .flatten()
    })
}

#[test]
fn a_cross_device_move_redoes_and_is_idempotent() {
    let tmp = Scratch::new();
    let root = tmp.path();
    let Some(remote) = other_device(root) else {
        eprintln!("skipped: no second filesystem available");
        return;
    };
    let app = app(root);
    let a = file(&root.join("a"), "a");
    let plan = Plan::with_steps(
        "move",
        vec![Box::new(Move {
            entry: a,
            dest_dir: remote.path().to_path_buf(),
        })],
    );
    apply(&app, &plan);
    let again = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(again.error.is_none(), "{:?}", again.error);
    assert_eq!(again.applied, 0, "re-applying a finished move is a no-op");
    let u = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(u.error.is_none(), "{:?}", u.error);
    assert!(root.join("a").exists());
    let r = app.executor.redo_blocking(app.lifetime()).unwrap();
    assert!(
        r.error.is_none(),
        "redo of a cross-device move: {:?}",
        r.error
    );
    assert!(remote.path().join("a").exists() && !root.join("a").exists());
}

#[test]
fn observers_hear_what_holds_after_each_run_until_withdrawn() {
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir(root.join("d")).unwrap();
    std::fs::write(root.join("a"), b"a").unwrap();
    let app = bibli_core::app::App::new(bibli_core::app::AppConfig::ephemeral(root)).unwrap();
    let heard = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&heard);
    app.executor.observe(
        "test",
        std::sync::Arc::new(move |changes: &[bibli_core::op::Change]| {
            sink.lock().push(changes.to_vec());
        }),
    );
    let plan = Plan::with_steps(
        "move",
        vec![Box::new(Move {
            entry: EntryRef::from_path(root.join("a")).unwrap(),
            dest_dir: root.join("d"),
        })],
    );
    assert!(
        app.executor
            .execute_blocking(&plan, app.lifetime())
            .error
            .is_none()
    );
    assert!(app.executor.undo_blocking(app.lifetime()).is_some());
    let expected = [
        bibli_core::op::Change::Move {
            from: root.join("a"),
            to: root.join("d/a"),
        },
        bibli_core::op::Change::Move {
            from: root.join("d/a"),
            to: root.join("a"),
        },
    ];
    assert_eq!(
        heard.lock().iter().flatten().cloned().collect::<Vec<_>>(),
        expected,
        "the move and its undo"
    );
    app.unregister_owner("test");
    assert!(app.executor.redo_blocking(app.lifetime()).is_some());
    assert_eq!(heard.lock().len(), 2, "withdrawn with its owner");
}

/// `new_file`: the user-facing command makes an empty file with an odd
/// name, undo takes it away, redo brings it back, and an undo never takes a
/// file that was written into since.
#[test]
fn a_new_file_is_created_undone_and_never_undone_once_written_into() {
    let tmp = Scratch::new();
    let root = tmp.path();
    let app = app(root);
    navigate(&app, root);
    let name = "l’été d'un « fichier ».txt";
    let plan = match app
        .dispatch(&bibli_core::command::Invocation::with_args(
            "new_file",
            &serde_json::json!({ "name": name }),
        ))
        .unwrap()
    {
        bibli_core::command::Outcome::Plan(plan) => plan,
        other => panic!("expected a plan, got {other:?}"),
    };
    assert!(apply(&app, &plan).error.is_none());
    let path = root.join(name);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    // Applying again is a no-op: the empty file is what the step makes.
    let again = apply(&app, &plan);
    assert!(again.error.is_none());
    assert_eq!(again.applied, 0);

    let undone = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(undone.error.is_none());
    assert!(!path.exists());
    let redone = app.executor.redo_blocking(app.lifetime()).unwrap();
    assert!(redone.error.is_none());
    assert!(path.is_file());

    std::fs::write(&path, "written since").unwrap();
    let refused = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(
        refused.error.is_some(),
        "an undo must not take written content"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "written since");
}

#[test]
fn a_new_file_never_replaces_what_is_there() {
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::write(root.join("taken"), "keep me").unwrap();
    let plan = Plan::with_steps(
        "new file",
        vec![Box::new(bibli_core::op::NewFile {
            path: root.join("taken"),
        })],
    );
    assert!(matches!(
        plan.diff(&std::sync::Arc::default()).steps[0].1,
        StepStatus::Conflict { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(root.join("taken")).unwrap(),
        "keep me"
    );
}

// Extended attributes: Unix's.

// Extended attributes: Unix's.
#[cfg(unix)]
#[test]
fn a_file_goes_with_its_companion_and_a_copy_takes_its_attributes_in() {
    let tmp = Scratch::new();
    let root = tmp.path().join("tree");
    std::fs::create_dir_all(root.join("into")).unwrap();
    let f = root.join("it's a.txt");
    std::fs::write(&f, b"x").unwrap();
    std::fs::write(root.join("._it's a.txt"), "user.xdg.tags=from the mac\n").unwrap();
    let app = app(tmp.path());
    app.companions
        .register("test", std::sync::Arc::new(Aside { holds: true }));
    let before = snapshot(&root);

    // A copy onto a volume that keeps attributes takes them in: the copy
    // has the tag, and no `._` file beside it.
    let mut plan = Plan::new("copy");
    plan.steps.push(Box::new(Copy {
        entry: EntryRef::from_path(&f).unwrap(),
        dest_dir: root.join("into"),
        new_name: None,
        preserve_attributes: true,
    }));
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    let copy = root.join("into/it's a.txt");
    assert_eq!(
        bibli_core::fs::xattr::get(&copy, "user.xdg.tags")
            .unwrap()
            .as_deref(),
        Some(b"from the mac".as_slice())
    );
    assert!(!root.join("into/._it's a.txt").exists());
    // Re-applied, it is already done, the companion that became attributes
    // not counted against it.
    let again = Plan::from_data(&plan.to_data(), &app.ops).unwrap();
    assert_eq!(
        app.executor
            .execute_blocking(&again, app.lifetime())
            .applied,
        0
    );
    app.executor.undo_blocking(app.lifetime()).unwrap();
    assert_eq!(snapshot(&root), before);

    // Renamed, moved and trashed with its file, and back with each undo.
    let mut plan = Plan::new("rename, move");
    plan.steps.push(Box::new(Rename {
        entry: EntryRef::from_path(&f).unwrap(),
        new_name: "b.txt".into(),
    }));
    let r = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(r.error.is_none(), "{:?}", r.error);
    assert!(root.join("._b.txt").exists() && !root.join("._it's a.txt").exists());
    let mut plan = Plan::new("move");
    plan.steps.push(Box::new(Move {
        entry: EntryRef::from_path(root.join("b.txt")).unwrap(),
        dest_dir: root.join("into"),
    }));
    assert!(
        app.executor
            .execute_blocking(&plan, app.lifetime())
            .error
            .is_none()
    );
    assert!(root.join("into/._b.txt").exists());
    let mut plan = Plan::new("trash");
    plan.steps.push(Box::new(Trash {
        entry: EntryRef::from_path(root.join("into/b.txt")).unwrap(),
    }));
    assert!(
        app.executor
            .execute_blocking(&plan, app.lifetime())
            .error
            .is_none()
    );
    assert!(!root.join("into/._b.txt").exists());
    for _ in 0..3 {
        let u = app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(u.error.is_none(), "{:?}", u.error);
    }
    assert_eq!(snapshot(&root), before);

    // A companion's place taken: the rename conflicts, touching nothing.
    std::fs::write(root.join("._c.txt"), b"someone else's").unwrap();
    let rename = Rename {
        entry: EntryRef::from_path(&f).unwrap(),
        new_name: "c.txt".into(),
    };
    assert!(matches!(
        rename.check(
            &StepContext::detached().with_companions(std::sync::Arc::clone(&app.companions))
        ),
        StepStatus::Conflict { .. }
    ));
}

/// The `.trashinfo` of the item the trash holds from `original`, and where
/// the item is.
#[cfg(target_os = "linux")]
fn trashed(original: &Path) -> Option<(PathBuf, PathBuf)> {
    let info = trash::os_limited::list()
        .ok()?
        .into_iter()
        .find(|i| i.original_path() == original)
        .map(|i| PathBuf::from(i.id))?;
    let item = info
        .parent()?
        .parent()?
        .join("files")
        .join(info.file_stem()?);
    Some((info, item))
}

/// What leaves the trash takes the companions trashed with it, and undone
/// puts them back as the trash had them: an item moved elsewhere, a
/// folder restored into one there again.
// The freedesktop trash's records (ADR-035: the others' are step D).
#[cfg(target_os = "linux")]
#[test]
fn companions_trashed_with_a_file_leave_the_trash_with_it() {
    let tmp = Scratch::new();
    let root = tmp.path().join("tree");
    std::fs::create_dir_all(root.join("desk")).unwrap();
    let app = app(tmp.path());
    app.companions
        .register("test", std::sync::Arc::new(Aside { holds: false }));
    let undo = || {
        let u = app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(u.error.is_none(), "{:?}", u.error);
    };
    let redo = || {
        let r = app.executor.redo_blocking(app.lifetime()).unwrap();
        assert!(r.error.is_none(), "{:?}", r.error);
    };

    // An item moved out of the trash, its companion beside it.
    let f = file(&root.join("it's a.txt"), "a");
    std::fs::write(root.join("._it's a.txt"), "aside").unwrap();
    apply(
        &app,
        &Plan::with_steps("trash", vec![Box::new(Trash { entry: f })]),
    );
    let (info, item) = trashed(&root.join("it's a.txt")).expect("in the trash");
    let (aside_info, _) = trashed(&root.join("._it's a.txt")).expect("its companion too");
    let out = Move {
        entry: EntryRef::from_path(&item).unwrap(),
        dest_dir: root.join("desk"),
    };
    apply(&app, &Plan::with_steps("move", vec![Box::new(out)]));
    assert_eq!(
        std::fs::read_to_string(root.join("desk/it's a.txt")).unwrap(),
        "a"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("desk/._it's a.txt")).unwrap(),
        "aside"
    );
    assert!(
        !info.exists() && !aside_info.exists(),
        "both out of the trash"
    );
    undo();
    assert!(item.exists() && info.exists() && aside_info.exists());
    assert!(!root.join("desk/._it's a.txt").exists());
    redo();
    assert!(root.join("desk/._it's a.txt").exists() && !aside_info.exists());

    // A folder restored into one made there again.
    std::fs::create_dir(root.join("old")).unwrap();
    std::fs::write(root.join("old/b.txt"), "b").unwrap();
    std::fs::write(root.join("._old"), "aside").unwrap();
    let old = EntryRef::from_path(root.join("old")).unwrap();
    apply(
        &app,
        &Plan::with_steps("trash", vec![Box::new(Trash { entry: old })]),
    );
    let (info, item) = trashed(&root.join("old")).expect("in the trash");
    let (aside_info, _) = trashed(&root.join("._old")).expect("its companion too");
    std::fs::create_dir(root.join("old")).unwrap();
    let restore = Restore {
        path: root.join("old"),
        item: Some(info.as_path().into()),
        landed: Vec::new(),
    };
    apply(&app, &Plan::with_steps("restore", vec![Box::new(restore)]));
    assert!(root.join("old/b.txt").exists() && root.join("._old").exists());
    assert!(!info.exists() && !aside_info.exists());
    undo();
    assert!(item.join("b.txt").exists() && info.exists() && aside_info.exists());
    assert!(!root.join("._old").exists(), "back in the trash");
    redo();
    assert!(root.join("old/b.txt").exists() && root.join("._old").exists());
    assert!(!aside_info.exists(), "found again by when it was trashed");
}

/// Copied on Windows by the system (`CopyFileExW`), a file keeps what a
/// copy of its bytes would lose there: its alternate streams, the mark a
/// downloaded file carries among them.
#[cfg(windows)]
#[test]
fn a_copy_keeps_the_mark_a_download_carries() {
    // Wine takes `name:stream` for a name of its own: it keeps no streams.
    if under_wine("alternate data streams") {
        return;
    }
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir(root.join("into")).unwrap();
    let file = root.join("it's downloaded.zip");
    std::fs::write(&file, b"the bytes").unwrap();
    let mark = "[ZoneTransfer]\r\nZoneId=3\r\n";
    let stream = |path: &Path| PathBuf::from(format!("{}:Zone.Identifier", path.display()));
    std::fs::write(stream(&file), mark).unwrap();
    let copy = Copy {
        entry: EntryRef::from_path(&file).unwrap(),
        dest_dir: root.join("into"),
        new_name: None,
        preserve_attributes: true,
    };
    copy.apply(&StepContext::detached()).unwrap();
    let copied = root.join("into/it's downloaded.zip");
    assert_eq!(std::fs::read(&copied).unwrap(), b"the bytes");
    assert_eq!(std::fs::read_to_string(stream(&copied)).unwrap(), mark);
}

/// A plan that makes a folder and copies into it (a tag exported as a
/// folder of its files) is checked as applying it will meet it: the folder
/// its first step makes is there for the steps after.
#[test]
fn a_plan_that_makes_a_folder_and_fills_it_is_no_conflict() {
    let tmp = Scratch::new();
    let root = tmp.path();
    let a = file(&root.join("a"), "a");
    let app = app(root);
    let made = root.join("made");
    let plan = Plan::with_steps(
        "export",
        vec![
            Box::new(Mkdir { path: made.clone() }),
            Box::new(Mkdir {
                path: made.join("inner"),
            }),
            Box::new(Copy {
                entry: a,
                dest_dir: made.clone(),
                new_name: None,
                preserve_attributes: true,
            }),
        ],
    );
    let diff = plan.diff(&std::sync::Arc::default());
    assert!(
        diff.steps
            .iter()
            .all(|(_, status)| !matches!(status, StepStatus::Conflict { .. })),
        "{:?}",
        diff.steps
    );
    assert!(plan.is_applicable(&std::sync::Arc::default()));
    apply(&app, &plan);
    assert!(made.join("inner").is_dir() && made.join("a").is_file());
}

/// A file of a btrfs subvolume of the filesystem the home trash is on
/// goes to the home trash across filesystems, as the trash crate sent it,
/// moved as any move across volumes is, and its undo brings it back the
/// same way: the crate's restore only renamed, and left an empty file
/// where it failed. Skipped where no subvolume can be made (another
/// filesystem, no `btrfs`).
#[cfg(target_os = "linux")]
#[test]
fn a_subvolume_s_files_go_to_the_home_trash_and_back_across_filesystems() {
    use std::os::unix::fs::MetadataExt;
    let tmp = home_tempdir();
    let sub = tmp.path().join("it's a subvolume");
    let made = std::process::Command::new("btrfs")
        .args(["subvolume", "create"])
        .arg(&sub)
        .output()
        .is_ok_and(|out| out.status.success());
    let data = std::path::PathBuf::from(std::env::var_os("XDG_DATA_HOME").unwrap());
    std::fs::create_dir_all(&data).unwrap();
    if !made || std::fs::metadata(&sub).unwrap().dev() == std::fs::metadata(&data).unwrap().dev() {
        eprintln!("skipped: no btrfs subvolume beside the test's data folder");
        return;
    }
    let file = sub.join("café notes.txt");
    std::fs::write(&file, "words").unwrap();
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let folder = sub.join("a folder");
    std::fs::create_dir_all(folder.join("inner")).unwrap();
    std::fs::write(folder.join("inner/x.txt"), "x").unwrap();
    std::os::unix::fs::symlink("inner/x.txt", folder.join("link")).unwrap();
    let app = app(&tmp.path().join("state"));
    apply(
        &app,
        &Plan::with_steps(
            "trash",
            vec![
                Box::new(Trash {
                    entry: EntryRef::from_path(&file).unwrap(),
                }),
                Box::new(Trash {
                    entry: EntryRef::from_path(&folder).unwrap(),
                }),
            ],
        ),
    );
    assert!(!file.exists() && !folder.exists(), "the originals went");
    let (_, item) = trashed(&file).expect("in the home trash");
    assert!(item.starts_with(&data), "{}", item.display());
    let r = app.executor.undo_blocking(app.lifetime()).unwrap();
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "words");
    assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), old);
    assert_eq!(
        std::fs::read_link(folder.join("link")).unwrap(),
        Path::new("inner/x.txt")
    );
    assert!(trashed(&file).is_none(), "its record went with it");
    std::fs::remove_file(&file).unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    // An empty subvolume is removed as a folder by its owner.
    let _ = std::fs::remove_dir(&sub);
}
