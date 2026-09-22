//! bibli plugin: shell. Hands the selection to a shell: copy the paths as a
//! quoted string, or run a command in a terminal with the selection as its
//! arguments (`"$@"` in a POSIX shell, `$argv` in fish).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bibli_core::app::{App, PaneArg};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem};
use bibli_core::settings::SettingSpec;
use bibli_core::workspace::PaneId;

const ID: &str = "shell";
const PROGRAM_KEY: &str = "shell.program";

/// Quote `s` for POSIX shells: bare when safe, single-quoted otherwise.
pub fn quote(s: &str) -> String {
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

/// Space-separated quoted paths.
pub fn quote_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| quote(&p.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
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
        "bibli".into(),
        program.into(),
        "-c".into(),
    ];
    match family(program).unwrap_or(Family::Posix) {
        Family::Posix => {
            argv.push(format!("{command} \"$@\"").into());
            argv.push("bibli".into());
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
    #[serde(default)]
    pane: Option<PaneId>,
}

fn selected_paths(app: &App, pane: PaneId) -> Vec<PathBuf> {
    app.selected_refs(pane)
        .into_iter()
        .map(|r| r.path)
        .collect()
}

fn run(
    app: &App,
    command: &str,
    pane: Option<PaneId>,
    with_selection: bool,
) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(pane);
    let dir = app.pane_dir(pane)?;
    let paths = if with_selection {
        selected_paths(app, pane)
    } else {
        Vec::new()
    };
    let program: String = app
        .settings
        .get(PROGRAM_KEY)
        .unwrap_or_else(default_program);
    let argv = run_argv(&program, command, &paths);
    // The terminal outlives the command; the platform's reaper collects it.
    bibli_platform::open::open_terminal_watched(&dir, &argv, |_| {})?;
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
    fn register(&self, app: &App) {
        app.settings.declare(SettingSpec::command(
            PROGRAM_KEY,
            "Shell",
            "Program that runs commands on the selection: a POSIX shell or fish. Defaults to $SHELL.",
            &default_program(),
        ));
        app.commands.register::<PaneArg, _>(
            ID,
            "shell.copy_paths",
            "Copy paths",
            "The selected paths, shell-quoted (the UI puts the value on the clipboard)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let paths = selected_paths(app, pane);
                if paths.is_empty() {
                    anyhow::bail!("nothing selected");
                }
                Ok(Outcome::Value(serde_json::Value::String(quote_paths(
                    &paths,
                ))))
            },
        );
        app.commands.register::<RunArgs, _>(
            ID,
            "shell.run",
            "Run command on selection…",
            "Run a shell command in a terminal with the selected paths as its arguments",
            |a, app| run(app, &a.command, a.pane, true),
        );
        app.commands.register::<RunArgs, _>(
            ID,
            "shell.run_here",
            "Run command here…",
            "Run a shell command in a terminal in the current directory",
            |a, app| run(app, &a.command, a.pane, false),
        );
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-shift-c".into(),
                command: Invocation::new("shell.copy_paths"),
                when: Some("pane selection".into()),
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Copy Paths".into(),
                command: Invocation::new("shell.copy_paths"),
                when: Some("selection".into()),
                order: 135,
                icon: Some("copy".into()),
                submenu: Some("More".into()),
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Run Command…".into(),
                command: Invocation::new("shell.run"),
                when: None,
                order: 410,
                icon: Some("command".into()),
                submenu: Some("More".into()),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use bibli_core::location::Location;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn quoting() {
        assert_eq!(quote("plain-1.txt"), "plain-1.txt");
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote("été.txt"), "'été.txt'");
        assert_eq!(quote(""), "''");
        assert_eq!(
            quote_paths(&["/x/a".into(), "/x/b c".into()]),
            "/x/a '/x/b c'"
        );
        assert_eq!(
            run_argv("/bin/sh", "ls -l", &["/x".into()]),
            [
                "/bin/sh",
                "-c",
                "\"$@\"; exec \"${SHELL:-/bin/sh}\"",
                "bibli",
                "/bin/sh",
                "-c",
                "ls -l \"$@\"",
                "bibli",
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
    fn output_of(program: &str, paths: &[PathBuf]) -> String {
        let argv = run_argv(program, "printf '<%s>'", paths);
        let out = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .env("SHELL", bibli_core::os_path::find_program("true").unwrap())
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    }

    #[test]
    fn the_command_sees_each_path_as_one_argument() {
        let paths = [PathBuf::from("/x/a b"), PathBuf::from("/x/it's $HOME")];
        assert_eq!(output_of("/bin/sh", &paths), "</x/a b></x/it's $HOME>");
        if let Some(fish) = bibli_core::os_path::find_program("fish") {
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

        let Outcome::Value(v) = app.dispatch(&Invocation::new("shell.copy_paths")).unwrap() else {
            panic!()
        };
        assert_eq!(
            v.as_str().unwrap(),
            format!("'{}'", dir.join("a b.txt").display())
        );

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
                "bibli"
            ]
        );
        assert_eq!(lines[5..9], ["/bin/sh", "-c", "ls -l \"$@\"", "bibli"]);
        assert_eq!(lines[9], dir.join("a b.txt").to_string_lossy());
        assert_eq!(
            lines[10],
            dir.to_string_lossy(),
            "runs in the pane's directory"
        );
    }
}
