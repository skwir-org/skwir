//! skwir plugin: shell. Hands the selection to a shell: copy the paths as a
//! quoted string, or run a command in a terminal with the selection as its
//! arguments (`"$@"` in a POSIX shell, `$argv` in fish).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use skwir_core::app::{App, TargetsArgs};
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::plugin::Plugin;
use skwir_core::registry::MenuItem;
use skwir_core::settings::SettingSpec;
use skwir_core::workspace::PaneId;

use std::fmt::Write as _;

const ID: &str = "shell";
const PROGRAM_KEY: &str = "shell.program";

/// Quote `s` for POSIX shells: bare when safe, single-quoted otherwise.
/// A control character (an escape, a carriage return) is never pasted as
/// it is: a terminal or a line editor acts on it before the shell reads
/// the quotes (an escape can end a bracketed paste), so such a name is
/// written in ANSI-C quotes, as one that is not UTF-8 is.
pub fn quote(s: &str) -> String {
    if s.chars().any(char::is_control) {
        return quote_bytes(s.as_bytes());
    }
    if !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '.' | '/' | ':' | '@' | '%' | '+' | '=' | '-' | ',')
        })
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Space-separated quoted paths. One that is not UTF-8 is written with
/// its bytes as they are (`$'caf\xe9'`, which bash, zsh and ksh read):
/// written lossily, it named another file.
pub fn quote_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| match p.to_str() {
            Some(text) => quote(text),
            None => quote_bytes(p.as_os_str().as_encoded_bytes()),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `bytes` in ANSI-C quotes: printable ASCII as it is, the rest `\xNN`.
fn quote_bytes(bytes: &[u8]) -> String {
    let mut out = String::from("$'");
    for &b in bytes {
        match b {
            b'\'' => out.push_str("\\'"),
            b'\\' => out.push_str("\\\\"),
            b' '..=b'~' => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('\'');
    out
}

/// How a shell takes a command and its arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    /// `sh -c 'cmd "$@"' $0 args…`
    Posix,
    /// `fish -c 'cmd $argv' args…`: fish rejects `$@` and has no `$0` slot.
    Fish,
}

fn family(program: &str) -> Option<Family> {
    let name = Path::new(program).file_name()?.to_str()?;
    match name {
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "mksh" | "yash" | "ash" => Some(Family::Posix),
        "fish" => Some(Family::Fish),
        _ => None,
    }
}

/// The argv that runs `command` with `paths` as its arguments under
/// `program`, then leaves the terminal open on an interactive `$SHELL` so
/// the output can be read. The wrapper is always `/bin/sh`, whatever
/// `program` speaks; a program of unknown family is given POSIX syntax.
pub fn run_argv(program: &str, command: &str, paths: &[PathBuf]) -> Vec<OsString> {
    let mut argv: Vec<OsString> = vec![
        "/bin/sh".into(),
        "-c".into(),
        "\"$@\"; exec \"${SHELL:-/bin/sh}\"".into(),
        "skwir".into(),
        program.into(),
        "-c".into(),
    ];
    match family(program).unwrap_or(Family::Posix) {
        Family::Posix => {
            argv.push(format!("{command} \"$@\"").into());
            argv.push("skwir".into());
        }
        Family::Fish => argv.push(format!("{command} $argv").into()),
    }
    argv.extend(paths.iter().map(|p| p.as_os_str().to_owned()));
    argv
}

/// `$SHELL` when it is a shell whose syntax we know, `/bin/sh` otherwise.
fn default_program() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| family(s).is_some())
        .unwrap_or_else(|| "/bin/sh".into())
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RunArgs {
    /// Shell command; the selected paths are its arguments (`"$@"`, or
    /// `$argv` in fish).
    command: String,
    /// The entries whose paths are its arguments, by absolute path; the
    /// pane's selection when omitted. It runs in the folder holding them.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct HereArgs {
    /// Shell command.
    command: String,
    #[serde(default)]
    pane: Option<PaneId>,
}

/// Run `command` in a terminal: on `on`'s entries (`App::targets`), in
/// the folder holding them, or with none in the folder the pane shows.
fn run(
    app: &App,
    command: &str,
    pane: Option<PaneId>,
    on: Option<Vec<skwir_core::op::EntryArg>>,
) -> anyhow::Result<Outcome> {
    let (dir, paths) = match on {
        Some(entries) => {
            let named = !entries.is_empty();
            let paths: Vec<PathBuf> = app
                .targets(pane, entries)?
                .into_iter()
                .map(|r| r.path)
                .collect();
            // Where the entries named are; for the selection, the folder
            // shown, or the one holding it in a search.
            let dir = if named {
                skwir_core::app::common_parent(paths.iter().map(PathBuf::as_path))
                    .ok_or_else(|| anyhow::anyhow!("the entries are in no folder"))?
            } else {
                app.work_dir(app.pane_or_active(pane))?
            };
            (dir, paths)
        }
        None => (app.pane_dir(app.pane_or_active(pane))?, Vec::new()),
    };
    let program = app
        .settings
        .get::<String>(PROGRAM_KEY)
        .filter(|program| !program.is_empty())
        .unwrap_or_else(default_program);
    let argv = run_argv(&program, command, &paths);
    // The terminal outlives the command; the platform's reaper collects it.
    // Started in a folder, it waits on its disk, a server's for a remote
    // folder: from the window, on a worker in the folder's lane, a failure
    // said.
    if skwir_core::scheduler::on_window_thread() {
        let weak = app.weak();
        app.scheduler.spawn_in(
            skwir_core::fs::reach::lane(&dir),
            skwir_core::scheduler::Priority::Visible,
            "shell.run",
            app.lifetime().child(),
            move || {
                if let Err(e) = skwir_platform::open::open_terminal_watched(&dir, &argv, |_| {})
                    && let Some(app) = weak.upgrade()
                {
                    app.notify(skwir_core::app::NotifyLevel::Error, format!("{e:#}"));
                }
            },
        );
    } else {
        skwir_platform::open::open_terminal_watched(&dir, &argv, |_| {})?;
    }
    Ok(Outcome::Done)
}

/// The shell plugin.
#[derive(Debug, Default)]
pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Shell"
    }
    fn description(&self) -> &'static str {
        "Copy selected paths for a shell; run a command on the selection in a terminal"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Desktop
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Programs])
    }
    fn register(&self, app: &App) {
        // Empty is the user's shell, found when a command runs: a default
        // read from the environment would differ from one machine to the
        // next.
        app.settings.declare(SettingSpec::command(
            PROGRAM_KEY,
            "Program for commands",
            "The shell that runs a command on the selection: a POSIX shell or fish. Leave empty for yours ($SHELL) when it is one of them, else sh.",
            "",
        )
        .page(skwir_core::settings::page::TERMINAL)
        .section("Commands"));
        app.commands
            .register::<TargetsArgs, _>(
                ID,
                Topic::Files,
                "shell.copy_paths",
                "Copy paths for a shell",
                "The selected paths, or the entries', shell-quoted, for the clipboard",
                |a, app| {
                    let paths: Vec<PathBuf> = app
                        .targets(a.pane, a.entries)?
                        .into_iter()
                        .map(|r| r.path)
                        .collect();
                    let n = paths.len();
                    Ok(Outcome::Clipboard {
                        text: quote_paths(&paths),
                        said: format!("Copied {n} path{}", if n == 1 { "" } else { "s" }),
                    })
                },
            )
            .reads();
        app.commands.register::<RunArgs, _>(
            ID, Topic::Terminal,
            "shell.run",
            "Run command on selection…",
            "Run a shell command in a terminal with the selected paths, or the entries', as its arguments",
            |a, app| run(app, &a.command, a.pane, Some(a.entries)),
        );
        app.commands.register::<HereArgs, _>(
            ID,
            Topic::Terminal,
            "shell.run_here",
            "Run command here…",
            "Run a shell command in a terminal in the current directory",
            |a, app| run(app, &a.command, a.pane, None),
        );
        // No key and no menu item: copying for a shell is rare, and the
        // menu's Copy Path, Ctrl+Shift+C and Ctrl+Alt+C copy the paths as
        // text (`copy_paths`). The palette has it, and a key can be given.
        // Beside the applications that open the selection, or the folder
        // on its background: both hand it to a program (ADR-099).
        for (when, submenu) in [
            ("selection !place", "Open With"),
            ("empty folder", "Open Folder With"),
        ] {
            app.registry.menus.add(
                ID,
                MenuItem {
                    menu: "context".into(),
                    title: "Run Command…".into(),
                    // The command is asked for: `shell.run` cannot run
                    // without one.
                    command: Invocation::with_args(
                        "ui.ask",
                        &serde_json::json!({
                            "title": "Run command on selection",
                            "accept": "Run",
                            "placeholder": "Command; the selected paths are its arguments",
                            "command": "shell.run",
                            "field": "command",
                        }),
                    ),
                    when: Some(when.into()),
                    order: 9,
                    icon: Some("command".into()),
                    submenu: Some(submenu.into()),
                    checked: None,
                    choice: None,
                    color: None,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use skwir_core::app::AppConfig;
    #[cfg(unix)]
    use skwir_core::location::Location;
    #[cfg(unix)]
    use std::sync::Arc;
    #[cfg(unix)]
    use std::time::Duration;

    #[test]
    fn quoting() {
        assert_eq!(quote("plain-1.txt"), "plain-1.txt");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote("été.txt"), "'été.txt'");
        assert_eq!(quote(""), "''");
        // An escape or a carriage return never reaches the terminal as it
        // is: `\e[201~` would end a bracketed paste.
        assert_eq!(quote("a\u{1b}[201~b"), "$'a\\x1b[201~b'");
        assert_eq!(quote("it's\r"), "$'it\\'s\\x0d'");
        assert_eq!(
            quote_paths(&["/x/a".into(), "/x/b c".into()]),
            "/x/a '/x/b c'"
        );
        // A name that is not UTF-8, byte for byte: bash reads it back so.
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let raw = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b"/x/it's caf\xe9"));
            assert_eq!(quote_paths(&[raw]), r"$'/x/it\'s caf\xe9'");
        }
        assert_eq!(
            run_argv("/bin/sh", "ls -l", &["/x".into()]),
            [
                "/bin/sh",
                "-c",
                "\"$@\"; exec \"${SHELL:-/bin/sh}\"",
                "skwir",
                "/bin/sh",
                "-c",
                "ls -l \"$@\"",
                "skwir",
                "/x"
            ]
        );
        assert_eq!(
            run_argv("/usr/bin/fish", "ls -l", &["/x".into()])[4..],
            ["/usr/bin/fish", "-c", "ls -l $argv", "/x"]
        );
    }

    /// Runs the argv without a terminal, with a `$SHELL` that exits at once,
    /// and returns what the command printed.
    #[cfg(unix)]
    fn output_of(program: &str, paths: &[PathBuf]) -> String {
        let argv = run_argv(program, "printf '<%s>'", paths);
        let out = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .env("SHELL", skwir_core::os_path::find_program("true").unwrap())
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn the_command_sees_each_path_as_one_argument() {
        let paths = [PathBuf::from("/x/a b"), PathBuf::from("/x/it's $HOME")];
        assert_eq!(output_of("/bin/sh", &paths), "</x/a b></x/it's $HOME>");
        if let Some(fish) = skwir_core::os_path::find_program("fish") {
            assert_eq!(
                output_of(&fish.to_string_lossy(), &paths),
                "</x/a b></x/it's $HOME>"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn paths_that_are_not_utf8_reach_the_command_intact() {
        use std::os::unix::ffi::OsStrExt;
        let raw = PathBuf::from(std::ffi::OsStr::from_bytes(b"/x/caf\xe9"));
        let argv = run_argv("/bin/sh", "true", std::slice::from_ref(&raw));
        assert_eq!(argv.last().unwrap(), raw.as_os_str());
    }

    #[cfg(unix)]
    #[test]
    fn run_launches_the_terminal_with_the_selection() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a b.txt"), b"").unwrap();
        let record = tmp.path().join("argv.txt");
        let fake = tmp.path().join("fake-term");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nrecord={}\n{{ printf '%s\\n' \"$@\"; pwd; }} > \"$record.tmp\"\nmv \"$record.tmp\" \"$record\"\n",
                quote(&record.to_string_lossy())
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        // One test per process under nextest; setting the environment is sound here.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("TERMINAL", &fake);
        }
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(ShellPlugin), &app);
        app.settings
            .set(PROGRAM_KEY, toml::Value::String("/bin/sh".into()));
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&dir)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["a b.txt"] }),
        ))
        .unwrap();

        let Outcome::Clipboard { text, said } =
            app.dispatch(&Invocation::new("shell.copy_paths")).unwrap()
        else {
            panic!()
        };
        assert_eq!(text, format!("'{}'", dir.join("a b.txt").display()));
        assert_eq!(said, "Copied 1 path");

        app.dispatch(&Invocation::with_args(
            "shell.run",
            &serde_json::json!({ "command": "ls -l" }),
        ))
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        // Publish the complete record atomically; existence means both argv
        // and the working directory have been written by the child.
        while !record.exists() {
            assert!(std::time::Instant::now() < deadline, "terminal never ran");
            std::thread::sleep(Duration::from_millis(10));
        }
        let lines: Vec<String> = std::fs::read_to_string(&record)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(lines[0], "-e");
        assert_eq!(
            lines[1..5],
            [
                "/bin/sh",
                "-c",
                "\"$@\"; exec \"${SHELL:-/bin/sh}\"",
                "skwir"
            ]
        );
        assert_eq!(lines[5..9], ["/bin/sh", "-c", "ls -l \"$@\"", "skwir"]);
        assert_eq!(lines[9], dir.join("a b.txt").to_string_lossy());
        assert_eq!(
            lines[10],
            dir.to_string_lossy(),
            "runs in the pane's directory"
        );
    }
}
