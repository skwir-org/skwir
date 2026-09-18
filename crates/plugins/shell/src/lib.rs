//! bibli plugin: shell. Hands the selection to a shell: copy the paths as a
//! quoted string, or run a command in a terminal with the selection as `$@`.

use std::path::PathBuf;

use bibli_core::app::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;
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

/// The argv that runs `command` with `paths` as `$@` under `program`.
pub fn run_argv(program: &str, command: &str, paths: &[PathBuf]) -> Vec<String> {
    let mut argv = vec![
        program.to_owned(),
        "-c".to_owned(),
        format!("{command} \"$@\""),
        "bibli".to_owned(),
    ];
    argv.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
    argv
}

fn default_program() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into())
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct PaneArgs {
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RunArgs {
    /// Shell command; the selected paths are appended as `"$@"`.
    command: String,
    #[serde(default)]
    pane: Option<PaneId>,
}

fn pane_of(app: &App, pane: Option<PaneId>) -> PaneId {
    pane.unwrap_or_else(|| app.workspace.read().active_id())
}

fn pane_dir(app: &App, pane: PaneId) -> anyhow::Result<PathBuf> {
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    p.location()
        .and_then(Location::path)
        .ok_or_else(|| anyhow::anyhow!("pane is not showing a directory"))
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
    let pane = pane_of(app, pane);
    let dir = pane_dir(app, pane)?;
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
    let child = bibli_platform::open::open_terminal_with(&dir, &argv)?;
    // Detach: the terminal outlives us; reap it on a worker so it never zombies.
    app.scheduler.spawn(
        bibli_core::scheduler::Priority::Background,
        "shell.reap",
        app.lifetime().child(),
        move || {
            let mut child = child;
            let _ = child.wait();
        },
    );
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
        app.settings.declare(SettingSpec {
            constraints: bibli_core::settings::SettingConstraints::default(),
            key: PROGRAM_KEY.into(),
            description: "Shell used by shell.run ($SHELL by default)".into(),
            default: toml::Value::String(default_program()),
        });
        app.commands.register::<PaneArgs, _>(
            ID,
            "shell.copy_paths",
            "Copy paths",
            "The selected paths, shell-quoted (the UI puts the value on the clipboard)",
            |a, app| {
                let pane = pane_of(app, a.pane);
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
            "Run a shell command in a terminal with the selected paths as \"$@\"",
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
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
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
            ["/bin/sh", "-c", "ls -l \"$@\"", "bibli", "/x"]
        );
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
        assert_eq!(lines[1], "/bin/sh");
        assert_eq!(lines[2], "-c");
        assert_eq!(lines[3], "ls -l \"$@\"");
        assert_eq!(lines[4], "bibli");
        assert_eq!(lines[5], dir.join("a b.txt").to_string_lossy());
        assert_eq!(
            lines[6],
            dir.to_string_lossy(),
            "runs in the pane's directory"
        );
    }
}
