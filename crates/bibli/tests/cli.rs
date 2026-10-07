//! `bibli cmd` with no window (ADR-043): the program itself, run as a
//! script runs it. With no selection to act on, a command is given its
//! files by absolute path; a plan is printed, or applied with `--apply`.
//! Unix only: each run gets a runtime folder of its own, so no window
//! listens there (on Windows the channel is a pipe in the user's name,
//! which a bibli the user runs could hold).
#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("cli-")
        .tempdir_in(base)
        .unwrap()
}

/// `bibli cmd <args>`, its runtime folder `runtime`: what it printed, and
/// how it ended.
fn bibli_cmd(runtime: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bibli"))
        .arg("cmd")
        .args(args)
        .env("XDG_RUNTIME_DIR", runtime)
        .output()
        .unwrap()
}

fn printed(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)))
}

fn said(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_script_names_files_by_path_and_prints_or_applies_the_plan() {
    let tmp = home_tempdir();
    let runtime = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("files");
    std::fs::create_dir(&dir).unwrap();
    let draft = dir.join("draft one.txt");
    std::fs::write(&draft, "draft").unwrap();
    std::fs::write(dir.join("old.txt"), "old").unwrap();
    let rename = json!({ "entries": [draft], "new_name": "final.txt" }).to_string();

    // Printed: what it would do, nothing done.
    let plan = printed(&bibli_cmd(runtime.path(), &["rename", &rename]));
    assert_eq!(plan["title"], json!("rename to final.txt"));
    assert_eq!(plan["steps"].as_array().map(Vec::len), Some(1));
    assert!(draft.is_file());

    // Applied: the user's own action.
    let applied = printed(&bibli_cmd(runtime.path(), &["--apply", "rename", &rename]));
    assert_eq!(applied["verdict"], json!("applied"), "{applied}");
    assert_eq!(applied["applied"], json!(1));
    assert!(dir.join("final.txt").is_file());
    assert!(!draft.exists());

    let trash = json!({ "entries": [dir.join("old.txt")] }).to_string();
    let applied = printed(&bibli_cmd(runtime.path(), &["--apply", "trash", &trash]));
    assert_eq!(applied["verdict"], json!("applied"), "{applied}");
    assert!(!dir.join("old.txt").exists());
}

#[test]
fn a_script_is_told_to_name_what_it_acts_on() {
    let tmp = home_tempdir();
    let runtime = tempfile::tempdir().unwrap();
    // No window, so no selection: the files are to be named.
    let nothing = said(&bibli_cmd(runtime.path(), &["trash"]));
    assert!(nothing.contains("name the files in `entries`"), "{nothing}");
    // Named by a path whose folder would be the window's, not the script's.
    let relative = said(&bibli_cmd(
        runtime.path(),
        &["trash", r#"{"entries": ["files/old.txt"]}"#],
    ));
    assert!(relative.contains("not an absolute path"), "{relative}");
    let gone = json!({ "entries": [tmp.path().join("gone.txt")] }).to_string();
    let missing = said(&bibli_cmd(runtime.path(), &["trash", &gone]));
    assert!(missing.contains("does not exist"), "{missing}");
}

#[test]
fn bibli_says_its_version_and_how_to_use_it_without_opening_a_window() {
    let version = Command::new(env!("CARGO_BIN_EXE_bibli"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("bibli {}", env!("CARGO_PKG_VERSION"))
    );
    let help = Command::new(env!("CARGO_BIN_EXE_bibli"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(
        help.contains("bibli cmd") && help.contains("bibli mcp"),
        "{help}"
    );
}
