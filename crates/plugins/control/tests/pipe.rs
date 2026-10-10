//! The window's channel on Windows (ADR-043): a named pipe, each client on
//! an instance of its own, the next made as one connects; a second skwir
//! finds the pipe taken. Tried under Wine, which keeps no account apart:
//! what the pipe lets other accounts do is not tested here.
#![cfg(windows)]

use std::time::{Duration, Instant};

use serde_json::json;
use skwir_core::App;
use skwir_core::Plan;
use skwir_core::app::AppConfig;
use skwir_core::command::{NoArgs, Outcome};
use skwir_core::op::Mkdir;
use skwir_plugin_control::channel::Address;
use skwir_plugin_control::{PlanMode, client, serve};

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Make {
    name: String,
}

#[test]
fn clients_one_after_another_reach_the_window_through_its_pipe() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
    app.commands
        .register::<NoArgs, _>("test", "test.answer", "Answer", "Answers 42", |_, _| {
            Ok(Outcome::Value(json!(42)))
        });
    let folders = tmp.path().to_owned();
    app.commands.register::<Make, _>(
        "test",
        "test.make",
        "Make a folder",
        "Makes a folder of this name",
        move |args, _| {
            let mut plan = Plan::new("Make a folder");
            plan.push(Mkdir {
                path: folders.join(args.name),
            });
            Ok(Outcome::Plan(plan))
        },
    );
    let address = Address(format!(r"\\.\pipe\skwir-test-{}", std::process::id()).into());
    let call = |name: &str, args: serde_json::Value, mode| {
        let serde_json::Value::Object(args) = args else {
            unreachable!()
        };
        client::call(&address, name, args, mode)
    };
    let error = call("test.answer", json!({}), PlanMode::Propose).unwrap_err();
    assert!(client::not_listening(&error), "{error:#}");

    let server = serve(&app, address.clone());
    let deadline = Instant::now() + Duration::from_secs(10);
    let first = loop {
        match call("test.answer", json!({}), PlanMode::Propose) {
            Ok(answer) => break answer,
            Err(_) => assert!(Instant::now() < deadline, "listening"),
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(first.value, json!(42));
    // Each call is a client of its own: the instances follow one another.
    for _ in 0..3 {
        let answer = call("test.answer", json!({}), PlanMode::Propose).unwrap();
        assert_eq!(answer.value, json!(42));
    }
    let applied = call("test.make", json!({ "name": "Made" }), PlanMode::Apply).unwrap();
    assert_eq!(applied.value["verdict"], json!("applied"));
    assert!(tmp.path().join("Made").is_dir());

    // A second skwir finds the pipe taken, and the first still answers.
    drop(serve(&app, address.clone()));
    let answer = call("test.answer", json!({}), PlanMode::Propose).unwrap();
    assert_eq!(answer.value, json!(42));

    drop(server);
    let error = call("test.answer", json!({}), PlanMode::Propose).unwrap_err();
    assert!(client::not_listening(&error), "{error:#}");
}
