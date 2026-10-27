//! An agent naming a path the system would reach another machine for is
//! refused before anything looks it up (ADR-029, ADR-044): on Windows a
//! share by its server's name, which a lookup connects to with the user's
//! credentials. Linux's and macOS's automount points are in the core's
//! tests: a table of mounts cannot be made up here.
#![cfg(windows)]

use std::time::{Duration, Instant};

use serde_json::json;
use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Invocation, NoArgs, Outcome};
use skwir_plugin_control::PlanMode;

#[test]
fn an_agent_naming_a_server_s_share_is_refused_unread() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    app.commands
        .register::<serde_json::Value, _>(
            "test",
            skwir_core::command::Topic::Files,
            "test.root",
            "Root",
            "Takes a root",
            |_, _| Ok(Outcome::Value(json!("ran"))),
        )
        .reads();
    app.commands
        .register::<NoArgs, _>(
            "test",
            skwir_core::command::Topic::Files,
            "test.answer",
            "Answer",
            "Answers 42",
            |_, _| Ok(Outcome::Value(json!(42))),
        )
        .reads();
    let asked = Instant::now();
    let error = skwir_plugin_control::run(
        &app,
        &Invocation::with_args(
            "test.root",
            &json!({ "root": r"\\host.invalid\share\note.txt" }),
        ),
        PlanMode::Propose,
        &|| false,
    )
    .expect_err("refused");
    assert!(error.to_string().contains("another machine"), "{error:#}");
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "nothing was looked up"
    );
    let answer = skwir_plugin_control::run(
        &app,
        &Invocation::new("test.answer"),
        PlanMode::Propose,
        &|| false,
    )
    .unwrap();
    assert_eq!(answer.value, json!(42), "other calls run");
}
