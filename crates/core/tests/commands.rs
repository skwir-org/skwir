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

/// A key held down runs a command again only where its declaration says,
/// by its arguments (ADR-066): never by default, nor for a command that is
/// not there.
#[test]
fn a_held_key_repeats_only_what_its_command_declares() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    for name in ["player.volume", "player.play"] {
        app.commands
            .register::<serde_json::Value, _>("player", name, "", "", |_, _| Ok(Outcome::Done));
    }
    app.commands
        .register::<serde_json::Value, _>("player", "player.step", "", "", |_, _| Ok(Outcome::Done))
        .repeats(skwir_core::command::always);
    app.commands
        .register::<serde_json::Value, _>("player", "player.volume_by", "", "", |_, _| {
            Ok(Outcome::Done)
        })
        .repeats(|args| args.get("by").is_some());
    let none = serde_json::json!({});
    assert!(app.commands.repeats("player.step", &none));
    assert!(!app.commands.repeats("player.play", &none), "once a press");
    assert!(
        app.commands
            .repeats("player.volume_by", &serde_json::json!({"by": 0.1}))
    );
    assert!(!app.commands.repeats("player.volume_by", &none));
    assert!(!app.commands.repeats("player.gone", &none));
    let spec = app.commands.get("player.step").unwrap();
    assert!(spec.repeats_with(&none));
    // Only keys repeat: agents are not told.
    let said = serde_json::to_value(&spec).unwrap();
    assert!(said.get("repeats").is_none(), "{said}");
}

#[test]
fn an_argument_a_command_does_not_take_is_refused_never_dropped() {
    use serde_json::json;
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[serde(tag = "action", rename_all = "snake_case")]
    #[allow(dead_code)] // Read by serde only.
    enum Step {
        Go { to: String },
        Stop { now: bool },
    }
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let refused = |name: &str, args: serde_json::Value| {
        let invocation = Invocation {
            name: name.into(),
            args,
        };
        let checked = app.commands.check(&invocation).expect_err("refused");
        let dispatched = app.dispatch(&invocation).expect_err("refused");
        assert_eq!(checked.to_string(), dispatched.to_string());
        checked.to_string()
    };
    // A misspelled `entries` would leave them out, and the command would
    // act on the pane's selection instead.
    let said = refused("trash", json!({ "entry": ["/nowhere/x’ y.txt"] }));
    assert!(said.contains("unknown field `entry`"), "{said}");
    assert!(said.contains("`entries`"), "names what it takes: {said}");
    // Names a flattened struct brings are taken; others are not.
    let set_access = |args| Invocation {
        name: "set_access".into(),
        args,
    };
    assert!(
        app.commands
            .check(&set_access(
                json!({ "mode": "644", "entries": [], "inside": true })
            ))
            .is_ok()
    );
    refused("set_access", json!({ "mode": "644", "entrie": [] }));
    // A command that takes none refuses any; one taking any JSON takes all.
    app.commands
        .register::<NoArgs, _>("probe", "probe.none", "None", "", |_, _| Ok(Outcome::Done));
    app.commands
        .register::<serde_json::Value, _>("probe", "probe.any", "Any", "", |_, _| {
            Ok(Outcome::Done)
        });
    refused("probe.none", json!({ "pane": 1 }));
    assert!(
        app.dispatch(&Invocation {
            name: "probe.any".into(),
            args: json!({ "whatever": [1, 2] }),
        })
        .is_ok()
    );
    // Enum arguments: the names of every variant are taken.
    app.commands
        .register_raw::<Step, _>("probe", "probe.step", "Step", "", |_, _| Ok(Outcome::Done));
    assert!(
        app.dispatch(&Invocation {
            name: "probe.step".into(),
            args: json!({ "action": "go", "to": "x" }),
        })
        .is_ok()
    );
    refused("probe.step", json!({ "action": "go", "to": "x", "too": 1 }));
}

#[test]
fn a_plugin_from_outside_takes_no_other_owners_command() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    app.commands
        .register::<NoArgs, _>("core", "probe.kept", "Kept", "", |_, _| {
            Ok(Outcome::Value("core's".into()))
        });
    app.commands.register_dynamic(
        "outsider",
        "probe.kept",
        "Taken",
        "",
        serde_json::json!({ "type": "object" }),
        |_, _| Ok(Outcome::Value("the outsider's".into())),
    );
    let Outcome::Value(answer) = app.dispatch(&Invocation::new("probe.kept")).unwrap() else {
        panic!("a value");
    };
    assert_eq!(answer, "core's");
    assert_eq!(&*app.commands.get("probe.kept").unwrap().owner, "core");
}
