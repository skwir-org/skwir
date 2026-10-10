//! A plan proposed for review answers whoever proposed it, once the user
//! has: how it applied, or that it was dismissed (ADR-043: an agent's
//! tool waits for this).

mod common;

use std::time::Duration;

use common::*;
use skwir_core::Plan;
use skwir_core::op::{Mkdir, Verdict};

fn a_folder_plan(path: &std::path::Path) -> Plan {
    let mut plan = Plan::new("Make a folder");
    plan.push(Mkdir {
        path: path.to_owned(),
    });
    plan
}

#[test]
fn a_proposal_applied_answers_how_it_applied() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let folder = tmp.path().join("Made by a proposal");
    let verdict = app.propose_awaiting(a_folder_plan(&folder));
    // The window takes it and the user applies it.
    let proposed = app.take_proposals();
    assert_eq!(proposed.len(), 1);
    app.apply_whole(proposed.into_iter().next().unwrap());
    assert_eq!(
        verdict.recv_timeout(Duration::from_secs(10)).unwrap(),
        Verdict::Applied {
            applied: 1,
            skipped: 0,
            error: None,
            cancelled: false,
        }
    );
    assert!(folder.is_dir());
}

#[test]
fn a_proposal_dismissed_answers_so_and_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let folder = tmp.path().join("Never made");
    let verdict = app.propose_awaiting(a_folder_plan(&folder));
    let proposed = app.take_proposals();
    app.dismissed(proposed[0].id);
    assert_eq!(
        verdict.recv_timeout(Duration::from_secs(10)).unwrap(),
        Verdict::Dismissed
    );
    assert!(!folder.exists());
    // A plan nobody waits for is dismissed quietly.
    app.dismissed(proposed[0].id);
}

#[test]
fn a_question_is_answered_once_or_withdrawn() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let (id, answer) = app.ask(
        "An agent asks to run “Undo”",
        "undo: Undo the most recent plan",
    );
    let asked = app.take_questions();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].id, id);
    assert!(app.awaited(id));
    assert!(app.answer(id, true));
    assert_eq!(answer.recv_timeout(Duration::from_secs(1)), Ok(true));
    assert!(!app.answer(id, false), "answered once");
    // Withdrawn before the window took it: never shown, never answered.
    let (gone, _answer) = app.ask("An agent asks to run “Redo”", "redo");
    app.withdraw(gone);
    assert!(app.take_questions().is_empty());
    assert!(!app.answer(gone, true));
}
