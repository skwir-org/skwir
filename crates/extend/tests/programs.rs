//! Plugins that run as programs (Level 2, ADR-045): the template built as
//! one, installed in a folder of plugins, started isolated when first
//! asked, its reads answered by skwir, ended when turned off.
#![cfg(target_os = "linux")] // Isolated by bubblewrap; built by cargo.

mod common;

use skwir_core::CancelToken;
use skwir_core::command::{Invocation, Outcome};

/// The template's manifest, run as the program `word-count`.
fn manifest(capabilities: &str) -> String {
    std::fs::read_to_string(
        common::root()
            .join("crates/plugins-outside/template")
            .join("plugin.toml"),
    )
    .unwrap()
    .replace("module = \"word_count.wasm\"", "program = \"word-count\"")
    .replace("capabilities = [\"read\"]", capabilities)
}

/// Whether this machine isolates programs: a plugin never runs otherwise.
fn isolates() -> bool {
    let can = skwir_platform::sandbox::available();
    if !can {
        eprintln!("bubblewrap cannot isolate here: a program plugin does not run");
    }
    can
}

#[test]
fn a_program_answers_a_field_and_a_command_reading_through_skwir() {
    if !isolates() {
        return;
    }
    let program = common::program("skwir-outside-template");
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    common::install(
        &plugins,
        "word-count",
        &manifest("capabilities = [\"read\"]"),
        &program,
        "word-count",
    );
    let app = common::app_with(&tmp.path().join("state"), &plugins, &[]);
    let note = tmp.path().join("it's a note.txt");
    std::fs::write(&note, "one two three").unwrap();
    assert_eq!(
        app.fields
            .get("word-count.words", &note, &CancelToken::new()),
        Some(skwir_core::Value::UInt(3))
    );
    let outcome = app
        .dispatch(&Invocation::with_args(
            "word-count.count",
            &serde_json::json!({ "entries": [note] }),
        ))
        .unwrap();
    let Outcome::Value(value) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(value["total"], 3);
    assert_eq!(value["files"][0]["name"], "it's a note.txt");
    // Even a plugin granted reads cannot receive private content through
    // a public name. Exercise the command path with both forms of alias.
    let keys = tmp.path().join("keys");
    std::fs::create_dir(&keys).unwrap();
    std::fs::write(keys.join(skwir_core::permissions::MARKER), "").unwrap();
    std::fs::write(keys.join("id"), "synthetic private content").unwrap();
    let file_alias = tmp.path().join("public note");
    let dir_alias = tmp.path().join("public folder");
    std::os::unix::fs::symlink(keys.join("id"), &file_alias).unwrap();
    std::os::unix::fs::symlink(&keys, &dir_alias).unwrap();
    for alias in [file_alias, dir_alias.join("id")] {
        let error = app
            .dispatch(&Invocation::with_args(
                "word-count.count",
                &serde_json::json!({ "entries": [alias] }),
            ))
            .unwrap_err();
        assert!(format!("{error:#}").contains("private"), "{error:#}");
    }
    // Turned off, its program ends and its commands go.
    assert!(app.plugins.set_enabled("word-count", false, &app));
    assert!(app.commands.get("word-count.count").is_none());
}

#[test]
fn a_program_that_may_not_read_is_refused_its_reads() {
    if !isolates() {
        return;
    }
    let program = common::program("skwir-outside-template");
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    common::install(
        &plugins,
        "word-count",
        &manifest("capabilities = []"),
        &program,
        "word-count",
    );
    let app = common::app_with(&tmp.path().join("state"), &plugins, &[]);
    let note = tmp.path().join("note.txt");
    std::fs::write(&note, "one two three").unwrap();
    let error = app
        .dispatch(&Invocation::with_args(
            "word-count.count",
            &serde_json::json!({ "entries": [note] }),
        ))
        .unwrap_err();
    assert!(format!("{error:#}").contains("may not read"), "{error:#}");
    // A private file is never handed to it at all.
    let keys = tmp.path().join("keys");
    std::fs::create_dir(&keys).unwrap();
    std::fs::write(keys.join(skwir_core::permissions::MARKER), "").unwrap();
    std::fs::write(keys.join("id"), "secret").unwrap();
    let error = app
        .dispatch(&Invocation::with_args(
            "word-count.count",
            &serde_json::json!({ "entries": [keys.join("id")] }),
        ))
        .unwrap_err();
    assert!(format!("{error:#}").contains("private"), "{error:#}");
}

#[test]
fn a_manifest_that_does_not_hold_together_is_left_out() {
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    let folder = plugins.join("broken");
    std::fs::create_dir_all(&folder).unwrap();
    // A program outside its folder.
    std::fs::write(
        folder.join("plugin.toml"),
        "id = \"broken\"\ntitle = \"Broken\"\n[run]\nprogram = \"../../bin/sh\"\n",
    )
    .unwrap();
    let other = plugins.join("taken");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("plugin.toml"),
        "id = \"tags\"\ntitle = \"Tags again\"\n[run]\nmodule = \"x.wasm\"\n",
    )
    .unwrap();
    std::fs::write(other.join("x.wasm"), b"\0asm").unwrap();
    assert!(skwir_extend::discover(&[plugins], &["tags"]).is_empty());
}
