//! Operation properties: inverse(apply(plan)) restores the tree, applying
//! twice is a no-op, and plans round-trip through their serialized form.

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bibli_core::op::{
    Copy, EntryRef, Mkdir, Move, Operation, Plan, Rename, StepContext, StepStatus, Swap, Trash,
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
    fn drop(&mut self) {
        if let Ok(items) = trash::os_limited::list() {
            let ours = items
                .into_iter()
                .filter(|i| i.original_path().starts_with(self.0.path()));
            let _ = trash::os_limited::purge_all(ours);
        }
    }
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
        for d in 0..t.dirs {
            std::fs::write(root.join(format!("d{d}/f{f}")), format!("content {d}/{f}")).unwrap();
        }
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
                    preserve_tags: true,
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
        let tmp = Scratch::new();
        let root: PathBuf = tmp.path().join("tree");
        std::fs::create_dir(&root).unwrap();
        build(&root, &t);
        let app = app(tmp.path());
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
    let diff = back.diff();
    assert!(
        matches!(diff.steps[0].1, StepStatus::Conflict { .. }),
        "rename onto existing must conflict"
    );
    assert_eq!(diff.steps[1].1, StepStatus::Pending { reversible: true });
    assert!(!back.is_applicable());
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

/// A step that cancels its own plan while it runs, so the test needs no timing.
#[derive(Debug, Clone)]
struct CancelHere {
    executor: std::sync::Weak<bibli_core::op::Executor>,
    plan: std::sync::Arc<std::sync::OnceLock<bibli_core::op::PlanId>>,
    dir: PathBuf,
}

impl Operation for CancelHere {
    fn kind(&self) -> &'static str {
        "test.cancel_here"
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
        assert!(executor.cancel(*self.plan.get().expect("plan id set")));
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
    plan.push(CancelHere {
        executor: std::sync::Arc::downgrade(&app.executor),
        plan: std::sync::Arc::clone(&id),
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
    let tmp = Scratch::new();
    let root = tmp.path();
    std::fs::create_dir(root.join("dest")).unwrap();
    let app = app(root);
    let src = file(&root.join("a.txt"), "original");
    let copy = Copy {
        entry: src,
        dest_dir: root.join("dest"),
        new_name: None,
        preserve_tags: true,
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
        preserve_tags: true,
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
        preserve_tags: true,
    };
    copy.apply(&StepContext::detached()).unwrap();
    assert!(
        std::fs::symlink_metadata(root.join("dest/src/pipe"))
            .unwrap()
            .file_type()
            .is_fifo()
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
        preserve_tags: true,
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
            preserve_tags: true,
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
    use std::os::unix::fs::MetadataExt;
    let here = std::fs::metadata(reference).ok()?.dev();
    ["/dev/shm", "/tmp"].into_iter().find_map(|candidate| {
        let meta = std::fs::metadata(candidate).ok()?;
        (meta.dev() != here)
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
        plan.diff().steps[0].1,
        StepStatus::Conflict { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(root.join("taken")).unwrap(),
        "keep me"
    );
}
