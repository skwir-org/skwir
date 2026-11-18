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
    let installed = common::install(
        &plugins,
        "word-count",
        &manifest("capabilities = [\"read\"]"),
        &program,
        "word-count",
    );
    common::catalogs(&installed, "crates/plugins-outside/template");
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
    // What it says of the number for the clipboard, from its catalogs: in
    // English to an agent, in the window's language there.
    let outcome = app
        .dispatch(&Invocation::with_args(
            "word-count.copy",
            &serde_json::json!({ "entries": [note] }),
        ))
        .unwrap();
    let Outcome::Clipboard { text, said } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        (text.as_str(), said.english()),
        ("3", "3 words in all".to_owned())
    );
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

/// A program that says back what skwir told it as it started
/// (`<id>.told`), or fails with the words of its catalogs (`<id>.fails`):
/// a probe of the protocol itself, in a shell's builtins.
const PROBE: &str = r#"#!/bin/sh
read -r hello
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"api":2}}'
while read -r line; do
    id=${line#*\"id\":}
    id=${id%%,*}
    case "$line" in
    *.fails*)
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"gone","data":{"id":"gone","args":{"name":{"name":"café [1].txt"}}}}}\n' "$id" ;;
    *)
        printf '{"jsonrpc":"2.0","id":%s,"result":{"answer":"value","value":%s}}\n' "$id" "$hello" ;;
    esac
done
"#;

/// The probe installed as `id`, speaking protocol `api`, with an English
/// and a French of this test's own.
fn probe(plugins: &std::path::Path, id: &str, api: u32) {
    let folder = plugins.join(id);
    std::fs::create_dir_all(folder.join("l10n")).unwrap();
    std::fs::write(
        folder.join("plugin.toml"),
        format!(
            "id = \"{id}\"\ntitle = \"Probe\"\napi = {api}\n[run]\nprogram = \"probe\"\n\
             [[commands]]\nname = \"{id}.told\"\ntitle = \"Told\"\neffect = \"reads\"\n\
             [[commands]]\nname = \"{id}.fails\"\ntitle = \"Fails\"\neffect = \"reads\"\n"
        ),
    )
    .unwrap();
    let program = folder.join("probe");
    std::fs::write(&program, PROBE).unwrap();
    std::fs::set_permissions(
        &program,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(folder.join("l10n/en.ftl"), "gone = { $name } is gone\n").unwrap();
    std::fs::write(
        folder.join("l10n/fr.ftl"),
        "gone = { $name } a disparu (essai)\n",
    )
    .unwrap();
}

/// Protocol 2 tells a program the window's languages as it starts, and
/// starts it again once they change; its error is said from its catalogs,
/// in French for the window and in English for an agent. Protocol 1, side
/// by side, is told none and shown as it writes.
#[test]
fn a_program_on_protocol_2_is_told_the_languages_and_fails_in_its_words() {
    if !isolates() {
        return;
    }
    let tmp = common::home_tempdir();
    let plugins = tmp.path().join("plugins");
    probe(&plugins, "probe", 2);
    probe(&plugins, "older-probe", 1);
    let app = common::app_with(&tmp.path().join("state"), &plugins, &[]);
    let told = |id: &str| -> serde_json::Value {
        let outcome = app
            .dispatch(&Invocation::with_args(
                format!("{id}.told"),
                &serde_json::json!({}),
            ))
            .unwrap();
        let Outcome::Value(hello) = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(hello["method"], "initialize", "{hello}");
        hello["params"].clone()
    };
    skwir_core::locale::set_chosen(Some("fr".into()));
    assert_eq!(told("probe")["languages"], serde_json::json!(["fr", "en"]));
    assert_eq!(told("probe")["api"], 2);
    assert!(
        told("older-probe").get("languages").is_none(),
        "protocol 1 is told none"
    );
    // The window's language changed: started again, told the new ones.
    skwir_core::locale::set_chosen(Some("zh-Hans".into()));
    assert_eq!(
        told("probe")["languages"],
        serde_json::json!(["zh-Hans", "en"])
    );
    // The pseudo-language's words are English's: none told of it.
    skwir_core::locale::set_chosen(Some(skwir_core::words::PSEUDO.into()));
    assert_eq!(told("probe")["languages"], serde_json::json!(["en"]));
    skwir_core::locale::set_chosen(Some("fr".into()));
    let failed = |id: &str| {
        skwir_core::words::Words::of_error(
            app.dispatch(&Invocation::with_args(
                format!("{id}.fails"),
                &serde_json::json!({}),
            ))
            .unwrap_err(),
        )
    };
    let error = failed("probe");
    assert_eq!(error.english(), "“café [1].txt” is gone", "an agent's");
    assert_eq!(
        error.say_plain(),
        "“café [1].txt” a disparu (essai)",
        "the window's"
    );
    // Protocol 1: its message, as written.
    let error = failed("older-probe");
    assert_eq!(
        (error.english(), error.say_plain()),
        ("gone".into(), "gone".into())
    );
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
    // Named as skwir's own are named: its `plugins.set_enabled` would be
    // the plugin's, the Plugins page's switch reaching it.
    let own = plugins.join("own");
    std::fs::create_dir_all(&own).unwrap();
    std::fs::write(
        own.join("plugin.toml"),
        "id = \"plugins\"\ntitle = \"Plugins\"\n[run]\nmodule = \"x.wasm\"\n\
         [[commands]]\nname = \"plugins.set_enabled\"\ntitle = \"Turn on or off\"\n",
    )
    .unwrap();
    std::fs::write(own.join("x.wasm"), b"\0asm").unwrap();
    assert!(skwir_extend::discover(&[plugins], &["tags"]).is_empty());
}
