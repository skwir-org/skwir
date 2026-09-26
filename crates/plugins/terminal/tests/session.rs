//! A real shell on a real pseudo-terminal, through the provider the UI uses:
//! what is typed runs, the shell and the pane keep to the same folder, and
//! disabling the plugin hangs up on the shell.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use bibli_core::app::{App, AppConfig};
use bibli_core::plugin::Plugin;
use bibli_core::terminal::{Input, Palette, Screen, Size, Spawn, TerminalSession};
use bibli_plugin_terminal::TerminalPlugin;

/// On the home filesystem, like every test that needs real directories.
fn home_tempdir() -> tempfile::TempDir {
    let base = PathBuf::from(std::env::var_os("HOME").expect("HOME"))
        .join(".cache")
        .join("bibli-tests");
    std::fs::create_dir_all(&base).expect("test base");
    tempfile::Builder::new()
        .prefix("t-")
        .tempdir_in(base)
        .expect("tempdir")
}

fn text(screen: &Screen) -> String {
    let mut out = String::new();
    for line in &screen.lines {
        let mut row = String::new();
        for run in line {
            while row.chars().count() < usize::from(run.col) {
                row.push(' ');
            }
            row.push_str(&run.text);
        }
        out.push_str(row.trim_end());
        out.push('\n');
    }
    out
}

struct Shell {
    session: Arc<dyn TerminalSession>,
    woken: mpsc::Receiver<()>,
}

impl Shell {
    fn open(app: &App, dir: &Path) -> Self {
        let (tx, woken) = mpsc::channel();
        let session = app
            .terminals
            .provider()
            .expect("the plugin provides terminals")
            .open(
                Spawn {
                    dir: dir.to_owned(),
                    size: Size {
                        cols: 100,
                        rows: 30,
                        cell_width: 8,
                        cell_height: 16,
                    },
                    palette: Palette {
                        ansi: [[0; 3]; 16],
                        fg: [255; 3],
                        bg: [0; 3],
                    },
                },
                Arc::new(move || {
                    let _ = tx.send(());
                }),
            )
            .expect("a shell starts");
        Self { session, woken }
    }

    /// Wait for a screen that satisfies `test`, each one announced by a wake.
    fn until(&self, what: &str, test: impl Fn(&Screen) -> bool) -> Arc<Screen> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let screen = self.session.screen();
            if test(&screen) {
                return screen;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "{what}; the screen:\n{}", text(&screen));
            let _ = self
                .woken
                .recv_timeout(left.min(Duration::from_millis(200)));
        }
    }

    fn type_line(&self, line: &str) {
        self.session.send(Input::Text(format!("{line}\r")));
    }
}

fn app(root: &Path) -> (Arc<App>, Arc<TerminalPlugin>) {
    let app = App::new(AppConfig::ephemeral(root.join("app"))).expect("app");
    // A shell whose prompt and quoting the test knows, whatever the user's.
    app.settings
        .set("terminal.program", toml::Value::String("/bin/sh".into()));
    let plugin = Arc::new(TerminalPlugin::default());
    app.plugins
        .add(Arc::clone(&plugin) as Arc<dyn Plugin>, &app);
    (app, plugin)
}

#[test]
fn what_is_typed_runs_and_the_shell_and_the_pane_follow_each_other() {
    let tmp = home_tempdir();
    let root = tmp.path().canonicalize().unwrap();
    let first = root.join("it's a dir");
    let second = root.join("an\\other");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let (app, _plugin) = app(&root);
    let shell = Shell::open(&app, &first);

    shell.type_line("echo hel''lo");
    shell.until("the command's output", |s| {
        text(s).lines().any(|line| line == "hello")
    });

    // The pane goes elsewhere: the shell follows, and that is not the user
    // moving.
    shell.session.send(Input::Follow(second.clone()));
    shell.type_line("echo done-following");
    let screen = shell.until("the shell in the pane's folder", |s| {
        s.cwd.as_deref() == Some(second.as_path())
            && text(s).lines().any(|line| line == "done-following")
    });
    assert_eq!(screen.moved, 0, "the plugin's own cd is not the user's");

    // The user changes directory: the pane is told once.
    shell.type_line("cd ..");
    let screen = shell.until("the user's cd seen", |s| {
        s.cwd.as_deref() == Some(root.as_path())
    });
    assert_eq!(screen.moved, 1);
    // The pane follows it there; nothing is typed back.
    shell.session.send(Input::Follow(root.clone()));
    shell.type_line("echo still-here");
    let screen = shell.until("the shell left alone", |s| {
        text(s).lines().any(|line| line == "still-here")
    });
    assert_eq!(
        text(&screen).matches(" cd '").count(),
        1,
        "one cd was typed for the user, the first"
    );

    drop(app);
}

#[test]
fn the_shell_ends_on_exit_and_disabling_hangs_up_on_it() {
    let tmp = home_tempdir();
    let root = tmp.path().canonicalize().unwrap();
    let (app, _plugin) = app(&root);

    let leaving = Shell::open(&app, &root);
    leaving.type_line("exit");
    leaving.until("the end of the shell", |s| s.exited);

    let staying = Shell::open(&app, &root);
    staying.type_line("echo pid=$$");
    let screen = staying.until("the shell's pid", |s| {
        text(s).lines().any(|line| line.starts_with("pid="))
    });
    let pid: u32 = text(&screen)
        .lines()
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|pid| pid.trim().parse().ok())
        .expect("a pid");
    assert!(Path::new(&format!("/proc/{pid}")).exists());
    assert!(app.plugins.set_enabled("terminal", false, &app));
    assert!(app.terminals.provider().is_none());
    // Hung up on and reaped, though the UI still holds its handle.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Path::new(&format!("/proc/{pid}")).exists() {
        assert!(Instant::now() < deadline, "the shell outlived its plugin");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(staying);
}
