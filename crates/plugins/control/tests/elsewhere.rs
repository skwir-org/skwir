//! An agent naming a path the system would reach another machine for is
//! refused before anything looks it up (ADR-029, ADR-044): on Windows a
//! share by its server's name, which a lookup connects to with the user's
//! credentials. Linux's and macOS's automount points are in the core's
//! tests: a table of mounts cannot be made up here. A path refused where
//! a call resolves what it reads is said why, the path left out.

use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::{Duration, Instant};

use serde_json::json;
use skwir_core::app::{App, AppConfig};
#[cfg(windows)]
use skwir_core::command::NoArgs;
use skwir_core::command::{Invocation, Outcome};
use skwir_plugin_control::PlanMode;

/// A command that reads `name` in `folder`, as a relative path is read in
/// the pane's folder: refused for an agent where what it would read is
/// (`permissions::agent_may_read`), its error naming the path resolved.
fn reads_in(app: &App, folder: PathBuf) {
    app.commands
        .register::<serde_json::Value, _>(
            "test",
            skwir_core::command::Topic::Files,
            "test.read",
            skwir_core::words::Words::as_written("Read"),
            skwir_core::words::Words::as_written("Reads a name in a folder"),
            move |args, _| {
                let name = args["name"].as_str().unwrap_or_default();
                let path = folder.join(name);
                skwir_core::permissions::agent_may_read(&path).map_err(|refused| {
                    refused.context(skwir_core::words::Words::as_written(format!(
                        "could not read {}",
                        path.display()
                    )))
                })?;
                Ok(Outcome::Value(json!("read")))
            },
        )
        .reads();
}

/// What an agent is told of `name` read by `test.read`.
fn refusal(app: &App, name: &str) -> String {
    skwir_plugin_control::run(
        app,
        &Invocation::with_args("test.read", &json!({ "name": name })),
        PlanMode::Propose,
        &|| false,
    )
    .expect_err("refused")
    .to_string()
}

/// A relative path an agent gives that is read in a private folder is
/// refused with why, never "(private)" alone, and names neither the
/// folder it was read in nor the path it was resolved to.
#[test]
fn a_path_refused_where_it_is_resolved_is_said_why_unnamed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("l'été");
    std::fs::create_dir_all(root.join("keys")).unwrap();
    std::fs::write(root.join("keys").join("id"), "secret").unwrap();
    std::fs::write(root.join("notes.txt"), "notes").unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
    keys_private(&app, &root);
    reads_in(&app, root.clone());
    let said = refusal(&app, "keys/id");
    assert!(
        said.contains("private") && said.contains("Settings, Privacy"),
        "{said}"
    );
    assert!(!said.contains("l'été") && !said.contains("keys"), "{said}");
    assert!(!said.contains(&*root.to_string_lossy()), "{said}");
    let read = skwir_plugin_control::run(
        &app,
        &Invocation::with_args("test.read", &json!({ "name": "notes.txt" })),
        PlanMode::Propose,
        &|| false,
    )
    .unwrap();
    assert_eq!(read.value, json!("read"), "what is not private is read");
}

/// The private folders are `root`'s `keys`.
fn keys_private(app: &App, root: &Path) {
    let mut record = toml::Table::new();
    record.insert(
        "path".into(),
        toml::Value::String(root.join("keys").display().to_string()),
    );
    app.settings.set(
        skwir_core::permissions::FOLDERS_KEY,
        toml::Value::Array(vec![toml::Value::Table(record)]),
    );
}

/// A relative path an agent gives that is read on a server's share (the
/// folder shown is one) is refused with why, never "(private)" alone,
/// and names neither the server nor the path it was resolved to.
#[cfg(windows)]
#[test]
fn a_path_resolved_onto_a_share_is_said_why_unnamed() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    reads_in(&app, PathBuf::from(r"\\host.invalid\share"));
    let asked = Instant::now();
    let said = refusal(&app, "a.jpg");
    assert!(said.contains("another machine"), "{said}");
    assert!(
        !said.contains("host.invalid") && !said.contains("a.jpg"),
        "{said}"
    );
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "nothing was looked up"
    );
}

#[cfg(windows)]
#[test]
fn an_agent_naming_a_server_s_share_is_refused_unread() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    app.commands
        .register::<serde_json::Value, _>(
            "test",
            skwir_core::command::Topic::Files,
            "test.root",
            skwir_core::words::Words::as_written("Root"),
            skwir_core::words::Words::as_written("Takes a root"),
            |_, _| Ok(Outcome::Value(json!("ran"))),
        )
        .reads();
    app.commands
        .register::<NoArgs, _>(
            "test",
            skwir_core::command::Topic::Files,
            "test.answer",
            skwir_core::words::Words::as_written("Answer"),
            skwir_core::words::Words::as_written("Answers 42"),
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
