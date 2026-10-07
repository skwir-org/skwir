//! The window's channel, end to end (ADR-043): an MCP client lists the
//! commands as tools and calls them; a plan is proposed and answered when
//! the user applies or dismisses it, printed, or applied as the user's own
//! action. Over a socket in a temporary folder, never the user's channel.
#![cfg(unix)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bibli_core::app::AppConfig;
use bibli_core::command::{NoArgs, Outcome};
use bibli_core::op::Mkdir;
use bibli_core::{App, Plan};
use bibli_plugin_control::channel::Address;
use bibli_plugin_control::client;
use bibli_plugin_control::{PlanMode, serve};
use rmcp::ServiceExt as _;
use serde_json::{Value, json};

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Make {
    name: String,
}

/// An app with a command that answers a value and one that plans a folder
/// in `root`.
fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("app"))).unwrap();
    app.commands
        .register::<NoArgs, _>("test", "test.answer", "Answer", "Answers 42", |_, _| {
            Ok(Outcome::Value(json!(42)))
        })
        .reads();
    let folders = root.to_owned();
    app.commands
        .register::<Make, _>(
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
        )
        .plans();
    // Acts without a plan: an agent asks first.
    app.commands
        .register::<NoArgs, _>("test", "test.act", "Act", "Acts at once", |_, _| {
            Ok(Outcome::Value(json!("acted")))
        });
    // Answers the window's own question: never an agent's.
    app.commands
        .register::<NoArgs, _>(
            "test",
            "test.answer_dialog",
            "Answer the dialog",
            "Presses for the user",
            |_, _| Ok(Outcome::Done),
        )
        .user_only();
    // Names a path in its answer.
    let named = root.to_owned();
    app.commands
        .register::<NoArgs, _>(
            "test",
            "test.paths",
            "Paths",
            "Answers paths",
            move |_, _| {
                Ok(Outcome::Value(json!({
                    "entries": [
                        { "name": "id", "path": named.join("keys").join("id") },
                        { "name": "notes.txt", "path": named.join("notes.txt") },
                    ],
                })))
            },
        )
        .reads();
    app
}

/// `root/keys` private, as the Privacy page would list it.
fn keys_private(app: &App, root: &Path) {
    let mut record = toml::Table::new();
    record.insert(
        "path".into(),
        toml::Value::String(root.join("keys").display().to_string()),
    );
    app.settings.set(
        bibli_core::permissions::FOLDERS_KEY,
        toml::Value::Array(vec![toml::Value::Table(record)]),
    );
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "{what} timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn arguments(value: Value) -> serde_json::Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    }
}

/// A server at a fresh address in `root`, listening.
fn listening(app: &Arc<App>, root: &Path) -> (bibli_plugin_control::Server, Address) {
    let address = Address(root.join("channel").join("control.sock"));
    let server = serve(app, address.clone());
    wait_until("listening", || address.0.exists());
    (server, address)
}

#[test]
fn an_agent_lists_the_commands_and_reads_a_value() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let (_server, address) = listening(&app, tmp.path());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let tools = runtime.block_on(async {
        let stream = bibli_plugin_control::channel::connect(&address)
            .await
            .unwrap();
        let agent = ().serve(stream).await.unwrap();
        let tools = agent.list_all_tools().await.unwrap();
        let _ = agent.cancel().await;
        tools
    });
    let make = tools
        .iter()
        .find(|t| t.name == "test__make")
        .expect("a tool per command");
    assert_eq!(make.input_schema.get("type"), Some(&json!("object")));
    assert!(make.input_schema["properties"].get("name").is_some());
    assert!(tools.iter().all(|t| {
        t.name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }));
    let answer = client::call(
        &address,
        "test.answer",
        arguments(json!({})),
        PlanMode::Propose,
    )
    .unwrap();
    assert_eq!(answer.value, json!(42));
    assert!(!answer.failed);
}

#[test]
fn an_agents_plan_waits_for_the_user_who_applies_or_dismisses_it() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let (_server, address) = listening(&app, tmp.path());
    for apply in [true, false] {
        let name = if apply {
            "Applied folder"
        } else {
            "Dismissed folder"
        };
        let call = {
            let address = address.clone();
            std::thread::spawn(move || {
                client::call(
                    &address,
                    "test.make",
                    arguments(json!({ "name": name })),
                    PlanMode::Propose,
                )
            })
        };
        // The window takes the proposal; the user answers it.
        let mut proposed = Vec::new();
        wait_until("a proposal", || {
            proposed.extend(app.take_proposals());
            !proposed.is_empty()
        });
        let plan = proposed.remove(0);
        if apply {
            app.apply_whole(plan);
        } else {
            app.dismissed(plan.id);
        }
        let answer = call.join().unwrap().unwrap();
        assert!(!answer.failed, "{answer:?}");
        assert_eq!(answer.value["plan"], json!("Make a folder"));
        if apply {
            assert_eq!(answer.value["verdict"], json!("applied"));
            assert_eq!(answer.value["applied"], json!(1));
        } else {
            assert_eq!(answer.value["verdict"], json!("dismissed"));
        }
        assert_eq!(tmp.path().join(name).is_dir(), apply);
    }
}

#[test]
fn a_script_prints_a_plan_or_applies_it_as_the_users_own_action() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let (_server, address) = listening(&app, tmp.path());
    let printed = client::call(
        &address,
        "test.make",
        arguments(json!({ "name": "Printed" })),
        PlanMode::Print,
    )
    .unwrap();
    assert_eq!(printed.value["title"], json!("Make a folder"));
    assert_eq!(printed.value["steps"].as_array().map(Vec::len), Some(1));
    assert!(!tmp.path().join("Printed").exists());
    let applied = client::call(
        &address,
        "test.make",
        arguments(json!({ "name": "Applied" })),
        PlanMode::Apply,
    )
    .unwrap();
    assert_eq!(applied.value["verdict"], json!("applied"));
    assert!(tmp.path().join("Applied").is_dir());
    assert!(app.take_proposals().is_empty(), "applied without review");
}

#[test]
fn an_agent_asks_before_a_command_that_acts_and_never_answers_for_the_user() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let (_server, address) = listening(&app, tmp.path());
    // Listed for an agent, saying it asks first; the user's own is not.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let tools = runtime.block_on(async {
        let stream = bibli_plugin_control::channel::connect(&address)
            .await
            .unwrap();
        let agent = ().serve(stream).await.unwrap();
        let tools = agent.list_all_tools().await.unwrap();
        let _ = agent.cancel().await;
        tools
    });
    let act = tools.iter().find(|t| t.name == "test__act").unwrap();
    assert!(
        act.description
            .as_deref()
            .unwrap_or_default()
            .contains("asks the user")
    );
    assert!(!tools.iter().any(|t| t.name == "test__answer_dialog"));
    let answer = tools.iter().find(|t| t.name == "test__answer").unwrap();
    assert_eq!(
        answer.annotations.as_ref().and_then(|a| a.read_only_hint),
        Some(true)
    );

    for allow in [true, false] {
        let call = {
            let address = address.clone();
            std::thread::spawn(move || {
                client::call(
                    &address,
                    "test.act",
                    arguments(json!({})),
                    PlanMode::Propose,
                )
            })
        };
        // The window shows the question; the user answers it.
        let mut asked = Vec::new();
        wait_until("a question", || {
            asked.extend(app.take_questions());
            !asked.is_empty()
        });
        assert!(asked[0].title.contains("“Act”"), "{}", asked[0].title);
        assert!(app.answer(asked[0].id, allow));
        let answer = call.join().unwrap();
        if allow {
            assert_eq!(answer.unwrap().value, json!("acted"));
        } else {
            let error = answer.unwrap();
            assert!(error.failed, "{error:?}");
            assert!(
                error.value["error"]
                    .as_str()
                    .unwrap()
                    .contains("did not allow")
            );
        }
    }
    // What answers the user's own dialogs is refused, never asked.
    let refused = client::call(
        &address,
        "test.answer_dialog",
        arguments(json!({})),
        PlanMode::Propose,
    )
    .unwrap();
    assert!(refused.failed);
    assert!(app.take_questions().is_empty());
    // A script is the user's own: it acts without asking.
    let script = client::call(&address, "test.act", arguments(json!({})), PlanMode::Print).unwrap();
    assert_eq!(script.value, json!("acted"));
}

#[test]
fn an_agent_never_names_nor_sees_a_private_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    keys_private(&app, tmp.path());
    let (_server, address) = listening(&app, tmp.path());
    let named = client::call(
        &address,
        "test.make",
        arguments(json!({ "name": tmp.path().join("keys").join("x") })),
        PlanMode::Propose,
    )
    .unwrap();
    assert!(named.failed, "{named:?}");
    assert!(named.value["error"].as_str().unwrap().contains("private"));
    assert!(app.take_proposals().is_empty(), "nothing proposed");
    // What a command answers loses what names a private file.
    let answer = client::call(
        &address,
        "test.paths",
        arguments(json!({})),
        PlanMode::Propose,
    )
    .unwrap();
    let seen: Vec<&str> = answer.value["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(seen, ["notes.txt"]);
    // A script is the user's own: it sees what the user sees.
    let script = client::call(
        &address,
        "test.paths",
        arguments(json!({})),
        PlanMode::Print,
    )
    .unwrap();
    assert_eq!(script.value["entries"].as_array().map(Vec::len), Some(2));
}

#[test]
fn a_command_refused_says_why_and_no_window_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let (server, address) = listening(&app, tmp.path());
    let unknown = client::call(
        &address,
        "test.nothing",
        arguments(json!({})),
        PlanMode::Propose,
    )
    .unwrap();
    assert!(unknown.failed);
    assert!(
        unknown.value["error"]
            .as_str()
            .is_some_and(|e| !e.is_empty())
    );
    let wrong = client::call(
        &address,
        "test.make",
        arguments(json!({ "nme": 1 })),
        PlanMode::Propose,
    )
    .unwrap();
    assert!(wrong.failed, "{wrong:?}");
    // Turned off: the socket goes, and a call says no window listens.
    drop(server);
    assert!(!address.0.exists());
    let error = client::call(
        &address,
        "test.answer",
        arguments(json!({})),
        PlanMode::Propose,
    )
    .unwrap_err();
    assert!(client::not_listening(&error), "{error:#}");
}

#[test]
fn a_folder_open_to_others_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let folder = tmp.path().join("open");
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .mode(0o755)
            .create(&folder)
            .unwrap();
    }
    let address = Address(folder.join("control.sock"));
    let server = serve(&app, address.clone());
    std::thread::sleep(Duration::from_millis(300));
    assert!(!address.0.exists(), "no socket where others could reach it");
    drop(server);
}

#[test]
fn a_socket_left_by_a_crash_is_replaced_and_one_in_use_is_not_taken() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let folder = tmp.path().join("channel");
    {
        use std::os::unix::fs::DirBuilderExt as _;
        // As the plugin makes it: the user's alone, or it refuses to listen.
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&folder)
            .unwrap();
    }
    let address = Address(folder.join("control.sock"));
    // A bibli that ended without removing its socket.
    drop(std::os::unix::net::UnixListener::bind(&address.0).unwrap());
    assert!(address.0.exists());
    let first = serve(&app, address.clone());
    wait_until("the first answers", || {
        client::call(
            &address,
            "test.answer",
            arguments(json!({})),
            PlanMode::Propose,
        )
        .is_ok()
    });
    // A second bibli finds it taken and leaves it to the first.
    let second = serve(&app, address.clone());
    drop(second);
    let answer = client::call(
        &address,
        "test.answer",
        arguments(json!({})),
        PlanMode::Propose,
    )
    .unwrap();
    assert_eq!(answer.value, json!(42));
    drop(first);
}
