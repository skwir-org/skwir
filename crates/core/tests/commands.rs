//! The command surface: registration, shadowing and arguments.

mod common;

use common::*;
use skwir_core::command::{Invocation, NoArgs, Outcome};

#[test]
fn a_command_that_panics_fails_without_taking_its_caller() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    app.commands
        .register::<NoArgs, _>("faulty", "faulty.run", "Run", "", |_, _| {
            panic!("a fault of its own")
        });
    let error = app
        .dispatch(&Invocation::new("faulty.run"))
        .expect_err("it failed");
    assert_eq!(
        error.to_string(),
        "“faulty.run” stopped on a fault of its own (the log says where)"
    );
    app.commands
        .register::<NoArgs, _>("faulty", "faulty.fine", "Fine", "", |_, _| {
            Ok(Outcome::Value(1.into()))
        });
    assert!(
        app.dispatch(&Invocation::new("faulty.fine")).is_ok(),
        "the registry still answers"
    );
}

#[test]
fn a_shadowed_command_returns_when_its_shadow_is_withdrawn() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let value = |app: &skwir_core::App| match app.dispatch(&Invocation::new("probe")).unwrap() {
        Outcome::Value(v) => v,
        other => panic!("{other:?}"),
    };
    app.commands
        .register::<NoArgs, _>("first", "probe", "Probe", "", |_, _| {
            Ok(Outcome::Value(1.into()))
        });
    app.commands
        .register::<NoArgs, _>("second", "probe", "Probe", "", |_, _| {
            Ok(Outcome::Value(2.into()))
        });
    assert_eq!(value(&app), 2);
    assert_eq!(
        app.commands
            .list()
            .iter()
            .filter(|c| &*c.name == "probe")
            .count(),
        1
    );
    app.unregister_owner("second");
    assert_eq!(value(&app), 1);
    app.unregister_owner("first");
    assert!(app.dispatch(&Invocation::new("probe")).is_err());
}

#[test]
fn arguments_that_cannot_be_serialized_fail_at_dispatch_not_at_construction() {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        #[derive(serde::Serialize)]
        struct Raw {
            location: std::path::PathBuf,
        }
        let tmp = tempfile::tempdir().unwrap();
        let app = app(tmp.path());
        let raw = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b"/example/caf\xe9"));
        let inv = Invocation::with_args("navigate", &Raw { location: raw });
        assert!(inv.args.is_null());
        assert!(app.dispatch(&inv).is_err());
    }
}

#[test]
fn a_new_folder_name_cannot_leave_the_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("d");
    std::fs::create_dir(&dir).unwrap();
    let app = app(tmp.path());
    navigate(&app, &dir);
    for name in ["../x", "/abs", "..", ""] {
        let inv = Invocation::with_args("mkdir", &serde_json::json!({ "name": name }));
        assert!(app.dispatch(&inv).is_err(), "{name:?} accepted");
    }
}

/// A command run with arguments is called by their form, else by its own
/// title with the arguments said after it: two ways to run one command
/// never read alike (ADR-024).
#[test]
fn a_command_is_called_by_the_form_of_its_arguments() {
    use skwir_core::command::Form;
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let declared = app.commands.register::<serde_json::Value, _>(
        "sorter",
        "sorter.sort",
        "Sort",
        "",
        |_, _| Ok(Outcome::Done),
    );
    declared.forms([Form::new(
        serde_json::json!({"key": "size"}),
        "Sort by size",
    )]);
    declared.keeps_a_key();
    declared.reads();
    let spec = app.commands.get("sorter.sort").unwrap();
    assert!(spec.keeps_a_key);
    assert_eq!(
        spec.title_for(&serde_json::json!({"key": "size"})),
        "Sort by size"
    );
    assert_eq!(spec.title_for(&serde_json::json!({})), "Sort");
    assert_eq!(spec.title_for(&serde_json::Value::Null), "Sort");
    assert_eq!(
        spec.title_for(&serde_json::json!({"key": "name"})),
        "Sort (key name)"
    );
}
