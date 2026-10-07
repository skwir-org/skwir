//! A plan proposed for review answers whoever proposed it, once the user
//! has: how it applied, or that it was dismissed (ADR-043: an agent's
//! tool waits for this).

mod common;

use std::time::Duration;

use bibli_core::Plan;
use bibli_core::op::{Mkdir, Verdict};
use common::*;

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
