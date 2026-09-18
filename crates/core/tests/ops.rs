//! Operation properties: inverse(apply(plan)) restores the tree, applying
//! twice is a no-op, and plans round-trip through their serialized form.

mod common;

use std::path::{Path, PathBuf};

use bibli_core::op::{Copy, EntryRef, Mkdir, Move, Operation, Plan, Rename, StepStatus, Swap};
use common::*;
use proptest::prelude::*;

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
                    new_name: to.clone(),
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
                    new_name: Some(format!("copy_of_f{file}")),
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
        let tmp = tempfile::tempdir().unwrap();
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
            prop_assert_eq!(snapshot(&root), before, "undo must restore the tree");

            // Redo reproduces the applied state (including stopping at the same
            // conflict, if the original did); undo again restores it.
            let rd = app.executor.redo_blocking(app.lifetime()).expect("redo available");
            prop_assert_eq!(rd.error.is_none(), r.error.is_none(), "redo outcome must match the original: {:?}", rd.error);
            prop_assert_eq!(snapshot(&root), after, "redo must reproduce the applied state");
            app.executor.undo_blocking(app.lifetime()).unwrap();
        }

        // Idempotency: re-applying an applied plan changes nothing and applies
        // nothing. This holds when each entry is touched once. Chains (rename
        // A->B then move B elsewhere) carry stale path hints in later steps and
        // are reported as conflicts on re-apply, which is the honest answer;
        // they are still fully undoable and redoable, asserted above.
        if r.error.is_none() && !steps_share_an_entry(&plan_data) && !plan.steps.iter().any(|s| s.kind() == "swap") {
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
        .filter_map(|s| s.args.pointer("/entry/id").map(ToString::to_string))
        .collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    ids.len() != n
}

#[test]
fn plan_round_trips_through_json_and_diff_reports_conflicts() {
    let tmp = tempfile::tempdir().unwrap();
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
    let tmp = tempfile::tempdir().unwrap();
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
