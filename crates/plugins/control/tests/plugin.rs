//! The plugin's own trace: on, the window listens where `channel::address`
//! says; off, the socket goes and nothing answers. In a runtime folder of
//! this test's own: the plugin toggle test turns Control on too, in nextest's
//! shared one.
#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use skwir_core::app::{App, AppConfig};
use skwir_core::command::{NoArgs, Outcome, Topic};
use skwir_plugin_control::{Control, ID, PlanMode, channel, client};

#[test]
fn turned_on_the_window_listens_and_turned_off_it_leaves_nothing() {
    let runtime = tempfile::tempdir().unwrap();
    // SAFETY: nextest runs each test in a process of its own, and nothing
    // reads the environment before this.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", runtime.path());
    }
    let address = channel::address().unwrap();
    assert!(address.0.starts_with(runtime.path()));

    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    app.commands.register::<NoArgs, _>(
        "test",
        Topic::Files,
        "test.answer",
        "Answer",
        "Answers 42",
        |_, _| Ok(Outcome::Value(json!(42))),
    );
    app.plugins.add(Arc::new(Control::default()), &app);
    assert!(!address.0.exists(), "off until the user turns it on");

    assert!(app.plugins.set_enabled(ID, true, &app));
    // Not before the window shows what it starts on: a script's command
    // would race the window's own start.
    std::thread::sleep(Duration::from_millis(300));
    assert!(!address.0.exists(), "listening before the window is shown");
    app.set_window_shown();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !address.0.exists() {
        assert!(Instant::now() < deadline, "listening");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        address.scripts().0.exists(),
        "the script endpoint is ready too"
    );
    let status = app
        .dispatch(&skwir_core::command::Invocation::new("control.status"))
        .unwrap();
    let Outcome::Value(status) = status else {
        panic!("{status:?}")
    };
    assert_eq!(status["listening"], json!(true));
    assert_eq!(status["address"], json!(address.0.display().to_string()));
    let answer = client::call(
        &address,
        "test.answer",
        serde_json::Map::new(),
        PlanMode::Print,
    )
    .unwrap();
    assert_eq!(answer.value, json!(42));

    assert!(app.plugins.set_enabled(ID, false, &app));
    assert!(!address.0.exists(), "the socket goes with the plugin");
    assert!(!address.scripts().0.exists(), "the script socket goes too");
    let error = client::call(
        &address,
        "test.answer",
        serde_json::Map::new(),
        PlanMode::Print,
    )
    .unwrap_err();
    assert!(client::not_listening(&error));
}

#[test]
fn turned_on_where_it_cannot_listen_it_says_why() {
    let runtime = tempfile::tempdir().unwrap();
    // SAFETY: as above, a process of this test's own.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", runtime.path());
    }
    // Its folder open to others: the plugin refuses to listen there.
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .mode(0o755)
            .create(runtime.path().join("skwir"))
            .unwrap();
    }
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    app.set_window_shown();
    app.plugins.add(Arc::new(Control::default()), &app);
    assert!(app.plugins.set_enabled(ID, true, &app));
    let deadline = Instant::now() + Duration::from_secs(10);
    let notes = loop {
        let notes = app.take_notifications();
        if !notes.is_empty() {
            break notes;
        }
        assert!(Instant::now() < deadline, "the user is told");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        notes[0]
            .text
            .starts_with("Agents and scripts cannot listen"),
        "{}",
        notes[0].text
    );
    let Outcome::Value(status) = app
        .dispatch(&skwir_core::command::Invocation::new("control.status"))
        .unwrap()
    else {
        panic!("a value")
    };
    assert_eq!(status["listening"], json!(false));
    assert!(
        status["not_listening"]
            .as_str()
            .is_some_and(|why| !why.is_empty())
    );
    assert!(!runtime.path().join("skwir").join("control.sock").exists());
}

/// A script's or an agent's command that changes what the window shows
/// wakes it, so the window shows it at once rather than at the next input;
/// one that only reads does not.
#[test]
fn a_command_that_changes_the_window_wakes_it() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    app.commands
        .register::<NoArgs, _>(
            "test",
            Topic::Files,
            "test.show",
            "Show",
            "Shows",
            |_, _| Ok(Outcome::Done),
        )
        .shows();
    app.commands
        .register::<NoArgs, _>(
            "test",
            Topic::Files,
            "test.read",
            "Read",
            "Reads",
            |_, _| Ok(Outcome::Value(json!(42))),
        )
        .reads();
    let woken = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&woken);
    app.set_waker(Some(Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    })));
    let run = |name: &str| {
        skwir_plugin_control::run(
            &app,
            &skwir_core::command::Invocation::new(name),
            PlanMode::Apply,
            &|| false,
        )
        .unwrap()
    };
    run("test.read");
    assert_eq!(woken.load(Ordering::SeqCst), 0, "a read draws nothing");
    run("test.show");
    assert_eq!(woken.load(Ordering::SeqCst), 1);
}
