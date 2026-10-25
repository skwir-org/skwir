//! `skwir cmd` with no window (ADR-043): the program itself, run as a
//! script runs it. With no selection to act on, a command is given its
//! files by absolute path; a plan is printed, or applied with `--apply`.
//! Unix only: each run gets a runtime folder of its own, so no window
//! listens there (on Windows the channel is a pipe in the user's name,
//! which a skwir the user runs could hold).
#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("cli-")
        .tempdir_in(base)
        .unwrap()
}

/// `skwir cmd <args>`, its runtime folder `runtime`: what it printed, and
/// how it ended.
fn skwir_cmd(runtime: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skwir"))
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
    let plan = printed(&skwir_cmd(runtime.path(), &["rename", &rename]));
    assert_eq!(plan["title"], json!("rename to final.txt"));
    assert_eq!(plan["steps"].as_array().map(Vec::len), Some(1));
    assert!(draft.is_file());

    // Applied: the user's own action.
    let applied = printed(&skwir_cmd(runtime.path(), &["--apply", "rename", &rename]));
    assert_eq!(applied["verdict"], json!("applied"), "{applied}");
    assert_eq!(applied["applied"], json!(1));
    assert!(dir.join("final.txt").is_file());
    assert!(!draft.exists());

    let trash = json!({ "entries": [dir.join("old.txt")] }).to_string();
    let applied = printed(&skwir_cmd(runtime.path(), &["--apply", "trash", &trash]));
    assert_eq!(applied["verdict"], json!("applied"), "{applied}");
    assert!(!dir.join("old.txt").exists());
}

#[test]
fn a_script_is_told_to_name_what_it_acts_on() {
    let tmp = home_tempdir();
    let runtime = tempfile::tempdir().unwrap();
    // No window, so no selection: the files are to be named.
    let nothing = said(&skwir_cmd(runtime.path(), &["trash"]));
    assert!(nothing.contains("name the files in `entries`"), "{nothing}");
    // Named by a path whose folder would be the window's, not the script's.
    let relative = said(&skwir_cmd(
        runtime.path(),
        &["trash", r#"{"entries": ["files/old.txt"]}"#],
    ));
    assert!(relative.contains("not an absolute path"), "{relative}");
    let gone = json!({ "entries": [tmp.path().join("gone.txt")] }).to_string();
    let missing = said(&skwir_cmd(runtime.path(), &["trash", &gone]));
    assert!(missing.contains("does not exist"), "{missing}");
}

#[test]
fn skwir_says_its_version_and_how_to_use_it_without_opening_a_window() {
    let version = Command::new(env!("CARGO_BIN_EXE_skwir"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        format!("skwir {}", env!("SKWIR_BUILD"))
    );
    let help = Command::new(env!("CARGO_BIN_EXE_skwir"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(
        help.contains("skwir cmd") && help.contains("skwir mcp"),
        "{help}"
    );
}

/// The process cannot exit successfully before its settings reach disk.
/// XDG redirects the actual config path on Linux; macOS uses its own
/// application folders, so this subprocess fixture is Linux-only.
#[cfg(target_os = "linux")]
#[test]
fn settings_are_saved_before_a_headless_command_exits() {
    let tmp = home_tempdir();
    let runtime = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let file = config.join("skwir/settings.toml");
    let save = |theme: &str| {
        Command::new(env!("CARGO_BIN_EXE_skwir"))
            .args(["cmd", "settings.apply"])
            .arg(json!({"changes": {"ui.theme": theme}}).to_string())
            .env("XDG_RUNTIME_DIR", runtime.path())
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_DATA_HOME", tmp.path().join("data"))
            .env("XDG_CACHE_HOME", tmp.path().join("cache"))
            .env("XDG_STATE_HOME", tmp.path().join("state"))
            .output()
            .unwrap()
    };
    for theme in ["dark", "light"] {
        assert!(printed(&save(theme)).is_u64());
        assert_eq!(
            skwir_core::settings::Settings::load(&file)
                .get::<String>("ui.theme")
                .as_deref(),
            Some(theme),
            "success means the setting is already on disk"
        );
    }
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let failed = save("dark");
    assert!(
        !said(&failed).is_empty(),
        "persistence errors reach the script"
    );
    assert!(file.is_dir(), "the failed save overwrites nothing");
}

/// A settings file checked with no window open, as Home Manager's module
/// checks the settings it declares: the window's own settings are known
/// there too; a value a setting does not take fails, saying which; a key
/// nothing here declares is noted.
#[test]
fn settings_are_checked_with_no_window_open() {
    let tmp = home_tempdir();
    let runtime = tempfile::tempdir().unwrap();
    let file = tmp.path().join("it's settings.toml");
    std::fs::write(
        &file,
        "[ui]\ntheme = 'dark'\n[core]\nstartup = 'last'\n[elsewhere]\nthing = 1\n",
    )
    .unwrap();
    let args = json!({ "file": file }).to_string();
    let checked = printed(&skwir_cmd(runtime.path(), &["settings.check", &args]));
    assert_eq!(checked["problems"], json!([]), "{checked}");
    assert_eq!(checked["notes"][0][0], json!("elsewhere"), "{checked}");

    // Themes found where skwir runs (bat's, the installed icons) are
    // noted, not refused: none are there while a configuration builds.
    std::fs::write(
        &file,
        "[ui]\nicon_theme = 'Some Icons'\n[reading]\ncode_light = 'My Theme'\n",
    )
    .unwrap();
    let checked = printed(&skwir_cmd(runtime.path(), &["settings.check", &args]));
    assert_eq!(checked["problems"], json!([]), "{checked}");
    let noted: Vec<&str> = checked["notes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|note| note[0].as_str())
        .collect();
    assert_eq!(noted, ["reading.code_light", "ui.icon_theme"], "{checked}");

    std::fs::write(&file, "[ui]\ntheme = 'sepia'\n[core]\nstartp = 'last'\n").unwrap();
    let wrong = said(&skwir_cmd(runtime.path(), &["settings.check", &args]));
    assert!(wrong.contains("core.startp: no such setting"), "{wrong}");
    assert!(
        wrong.contains("ui.theme: choose one of auto, light, dark"),
        "{wrong}"
    );
}

/// Run as ssh runs `SSH_ASKPASS` (ADR-077), skwir asks over the socket
/// its environment names, prints the answer for ssh, and starts nothing
/// else; with no one to answer, it fails, and ssh hears no password.
#[test]
fn as_askpass_skwir_asks_the_socket_and_prints_the_answer() {
    use skwir_platform::askpass::{Kind, SOCKET_VAR, Server};
    let tmp = home_tempdir();
    let server = Server::start(tmp.path(), |question| match question.kind {
        Kind::Secret => Some(format!("secret for {}", question.prompt.trim())),
        Kind::Confirm => Some("yes".to_owned()),
        Kind::Notice => None,
    })
    .unwrap();
    let ask = |prompt: &str, socket: &Path| {
        Command::new(env!("CARGO_BIN_EXE_skwir"))
            .arg(prompt)
            .env(SOCKET_VAR, socket)
            .env_remove("SSH_ASKPASS_PROMPT")
            .env("XDG_RUNTIME_DIR", tmp.path())
            .output()
            .unwrap()
    };
    let asked = ask("me@example.com's password: ", server.path());
    assert!(asked.status.success(), "{asked:?}");
    assert_eq!(
        String::from_utf8_lossy(&asked.stdout),
        "secret for me@example.com's password:\n"
    );
    let confirmed = ask(
        "Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
        server.path(),
    );
    assert_eq!(String::from_utf8_lossy(&confirmed.stdout), "yes\n");
    let gone = server.path().to_owned();
    drop(server);
    let unanswered = ask("me@example.com's password: ", &gone);
    assert!(!unanswered.status.success());
    assert!(unanswered.stdout.is_empty());
}
