//! skwir plugin: shell. Hands the selection to a shell: copy the paths as a
//! quoted string, or run a command in a terminal with the selection as its
//! arguments (`"$@"` in a POSIX shell, `$argv` in fish), after it unless it
//! names them itself.
//!
//! Run Command… says what its field takes and what runs as it is typed
//! (ADR-137): examples, how the paths are given (`reference`), and the
//! line with the paths as the shell is given them, quoted as it reads
//! them, and the folder it runs in (`shell.run.describe`), a command left
//! empty held back. What is said and what runs are one [`Line`]. On a
//! folder's background it runs in the folder, with no paths
//! (`shell.run_here`). Linux's terminals run a command; elsewhere both say
//! it is not available yet, and the menu has no Run Command….

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use skwir_core::app::{App, TargetsArgs};
use skwir_core::command::{Invocation, Outcome, Topic};
use skwir_core::op::EntryArg;
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::registry::MenuItem;
use skwir_core::settings::SettingSpec;
use skwir_core::words;
use skwir_core::words::Words;
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

/// Quote `bytes` (a name, as the system holds it) as fish reads it back:
/// bare when safe, else in single quotes, where fish reads `\'` and `\\`.
/// A control character, or a name that is not UTF-8, is written outside
/// quotes with each byte that is not safe as fish's `\xHH` (ASCII) or
/// `\XHH` (a byte of its own), which fish's quotes do not read.
fn quote_fish(bytes: &[u8]) -> String {
    let safe = |b: u8| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'_' | b'.' | b'/' | b':' | b'@' | b'+' | b'=' | b'-' | b','
            )
    };
    match std::str::from_utf8(bytes) {
        Ok(text) if !text.chars().any(char::is_control) => {
            if !bytes.is_empty() && bytes.iter().all(|&b| safe(b)) {
                text.to_owned()
            } else {
                format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
            }
        }
        _ => {
            let mut out = String::new();
            for &b in bytes {
                if safe(b) {
                    out.push(char::from(b));
                } else if b.is_ascii() {
                    let _ = write!(out, "\\x{b:02x}");
                } else {
                    let _ = write!(out, "\\X{b:02x}");
                }
            }
            out
        }
    }
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

/// Whether `command` names its arguments itself, as `family` reads it:
/// `$@`, `$*`, `$1` to `$9` or `${N}` (`${@}`, `${#1}`…) in a POSIX shell,
/// `$argv` in fish; outside single quotes and never escaped by a
/// backslash. `$0` is the shell's own name, no argument.
fn names_arguments(command: &str, family: Family) -> bool {
    /// Where the scan is: in a word, or in quotes of a kind.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum In {
        Word,
        Single,
        Double,
        /// `$'…'`, a POSIX shell's ANSI-C quotes, where `\` escapes.
        AnsiC,
    }
    let bytes = command.as_bytes();
    let mut state = In::Word;
    let mut at = 0;
    while at < bytes.len() {
        match (state, bytes[at]) {
            // fish reads `\'` and `\\` in single quotes, as `$'…'` does.
            (In::Single, b'\\') if family == Family::Fish => at += 1,
            (In::AnsiC | In::Word | In::Double, b'\\') => at += 1,
            (In::Single | In::AnsiC, b'\'') | (In::Double, b'"') => state = In::Word,
            (In::Word, b'\'') => state = In::Single,
            (In::Word, b'"') => state = In::Double,
            (In::Word, b'$') if family == Family::Posix && bytes.get(at + 1) == Some(&b'\'') => {
                state = In::AnsiC;
                at += 1;
            }
            // `$$` is the shell's process.
            (In::Word | In::Double, b'$') if bytes.get(at + 1) == Some(&b'$') => at += 1,
            (In::Word | In::Double, b'$') if named_after(&bytes[at + 1..], family) => {
                return true;
            }
            _ => {}
        }
        at += 1;
    }
    false
}

/// Whether what follows a `$` names the arguments (`names_arguments`).
fn named_after(after: &[u8], family: Family) -> bool {
    match family {
        Family::Posix => match after {
            [b'@' | b'*' | b'1'..=b'9', ..] => true,
            [b'{', rest @ ..] => {
                let rest = rest.strip_prefix(b"#").unwrap_or(rest);
                let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
                matches!(rest, [b'@' | b'*', ..]) || rest[..digits].iter().any(|&b| b != b'0')
            }
            _ => false,
        },
        Family::Fish => {
            after.starts_with(b"argv")
                && !after
                    .get(4)
                    .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
        }
    }
}

/// A command as its program is given it: what it runs and what is said
/// of it as it is typed come from here alone. The paths go after the
/// command (`"$@"`, `$argv`), unless it names them itself
/// ([`names_arguments`]): then they go only where it does.
struct Line<'a> {
    program: &'a str,
    family: Family,
    command: &'a str,
    /// The paths go after the command: it names none of its arguments.
    after: bool,
}

impl<'a> Line<'a> {
    /// `command` under `program`; a program of unknown family is given
    /// POSIX syntax.
    fn new(program: &'a str, command: &'a str) -> Self {
        let family = family(program).unwrap_or(Family::Posix);
        Line {
            program,
            family,
            command,
            after: !names_arguments(command, family),
        }
    }

    /// The argv that runs the command with `paths` as its arguments, then
    /// leaves the terminal open on an interactive `$SHELL` so the output
    /// can be read. The wrapper is always `/bin/sh`, whatever the program
    /// speaks.
    fn argv(&self, paths: &[PathBuf]) -> Vec<OsString> {
        let mut argv: Vec<OsString> = vec![
            "/bin/sh".into(),
            "-c".into(),
            "\"$@\"; exec \"${SHELL:-/bin/sh}\"".into(),
            "skwir".into(),
            self.program.into(),
            "-c".into(),
        ];
        let command = self.command;
        match (self.family, self.after) {
            (Family::Posix, true) => argv.push(format!("{command} \"$@\"").into()),
            (Family::Fish, true) => argv.push(format!("{command} $argv").into()),
            (_, false) => argv.push(command.into()),
        }
        // `$0`, so that the paths are `$1`… in a POSIX shell.
        if self.family == Family::Posix {
            argv.push("skwir".into());
        }
        argv.extend(paths.iter().map(|p| p.as_os_str().to_owned()));
        argv
    }

    /// What runs, said: the command with the paths it is given, whole and
    /// quoted as its program reads them (`shown` of them, and how many
    /// `more`), after it or where it names them.
    fn said(&self, shown: &[PathBuf], more: usize) -> Words {
        let command = self.command.to_owned();
        if shown.is_empty() {
            return words!(
                shell_line_no_paths = "{ $command }  with no paths",
                command = command,
                note = "What a command runs, said as it is typed: command is the command line, \
                        given no paths."
            );
        }
        // A command line, the paths as its shell reads them: not words.
        let paths = shown
            .iter()
            .map(|path| self.quoted(path))
            .collect::<Vec<_>>()
            .join(" ");
        let with_more = |line: String| {
            if more == 0 {
                return Words::as_written(line);
            }
            words!(
                shell_line_more = "{ $line } … and { $more ->
                    [one] { $more } more
                   *[other] { $more } more
                }",
                line = line,
                more = more,
                note = "line: a command line with the first paths it is given, as its shell \
                        reads them; more: how many paths it is given beyond them."
            )
        };
        if self.after {
            with_more(format!("{command} {paths}"))
        } else {
            words!(
                shell_line_named = "{ $command }  with the paths where it names them: { $paths }",
                command = command,
                paths = with_more(paths),
                note = "command: a command line that names its arguments itself (\"$@\", $1); \
                        paths: the paths it is given, as its shell reads them."
            )
        }
    }

    /// `path` as the program reads it back, typed on its line.
    fn quoted(&self, path: &Path) -> String {
        let bytes = path.as_os_str().as_encoded_bytes();
        match (self.family, path.to_str()) {
            (Family::Fish, _) => quote_fish(bytes),
            (Family::Posix, Some(text)) => quote(text),
            (Family::Posix, None) => quote_bytes(bytes),
        }
    }
}

/// The argv that runs `command` with `paths` as its arguments under
/// `program` ([`Line::argv`]).
pub fn run_argv(program: &str, command: &str, paths: &[PathBuf]) -> Vec<OsString> {
    Line::new(program, command).argv(paths)
}

/// `$SHELL` when it is a shell whose syntax we know, `/bin/sh` otherwise.
fn default_program() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| family(s).is_some())
        .unwrap_or_else(|| "/bin/sh".into())
}

/// The program a command runs under: the setting's, else the user's.
fn program(app: &App) -> String {
    app.settings
        .get::<String>(PROGRAM_KEY)
        .filter(|program| !program.is_empty())
        .unwrap_or_else(default_program)
}

/// Whether this system runs a command in a terminal: Linux's terminals
/// do; a Mac's hand it to a window and return at once
/// (`open::terminal_with`), and Windows has no POSIX shell to give it.
const RUNS: bool = !cfg!(any(target_os = "macos", windows));

/// [`RUNS`], or the error that names the system.
fn runs_here() -> anyhow::Result<()> {
    if RUNS {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            words!(
                shell_run_not_yet =
                    "Running a command in a terminal is not available on { $system } yet",
                system = skwir_core::system::NAME,
                note = "system: Linux, macOS or Windows."
            ),
        )
        .into())
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RunArgs {
    /// Shell command; the selected paths are its arguments, after it
    /// (`"$@"`, or `$argv` in fish) unless it names them itself (`"$@"`,
    /// `$1`…; `$argv`), then only there.
    command: String,
    /// The entries whose paths are its arguments, by absolute path; the
    /// pane's selection when omitted. It runs in the folder holding them.
    #[serde(default)]
    entries: Vec<EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct HereArgs {
    /// Shell command, given no paths.
    command: String,
    /// The folder it runs in, an absolute path; the folder the pane shows
    /// when omitted.
    #[serde(default, rename = "in")]
    folder: Option<PathArg>,
    #[serde(default)]
    pane: Option<PaneId>,
}

/// What a command is run on.
enum On {
    /// The entries named (by path, or by the window as it shows them), or
    /// the pane's selection when none are: their paths are its arguments,
    /// in the folder holding them.
    Entries(Vec<EntryArg>),
    /// No paths, in the folder named, else the one the pane shows.
    Folder(Option<PathArg>),
}

/// Where a command runs, the first `at_most` paths it is given, and how
/// many it is given in all: `on`'s entries (`App::targets`), in the folder
/// holding them all, or none in a folder. Only the paths returned are
/// read: what the line said shows is a few of thousands.
fn where_and_what(
    app: &App,
    pane: Option<PaneId>,
    on: On,
    at_most: usize,
) -> anyhow::Result<(PathBuf, Vec<PathBuf>, usize)> {
    let pane = app.pane_or_active(pane);
    Ok(match on {
        On::Entries(entries) if !entries.is_empty() => {
            let paths = entries
                .iter()
                .take(at_most)
                .cloned()
                .map(|entry| Ok(entry.into_ref()?.path))
                .collect::<anyhow::Result<Vec<PathBuf>>>()?;
            // Where the entries named are, all of them.
            let dir = skwir_core::app::common_parent(entries.iter().map(|entry| match entry {
                EntryArg::Path(path) | EntryArg::Named { path, .. } => path.as_ref(),
            }))
            .ok_or_else(|| {
                anyhow::anyhow!(words!(shell_no_folder = "the entries are in no folder"))
            })?;
            (dir, paths, entries.len())
        }
        // The selection: in the folder shown, or the one holding it in a
        // search.
        On::Entries(_) => {
            let selected = app.targets(Some(pane), Vec::new())?;
            let count = selected.len();
            let paths = selected.into_iter().take(at_most).map(|r| r.path).collect();
            (app.work_dir(pane)?, paths, count)
        }
        On::Folder(Some(folder)) => (
            skwir_core::os_path::absolute(&folder)?.to_owned(),
            Vec::new(),
            0,
        ),
        On::Folder(None) => (app.pane_dir(pane)?, Vec::new(), 0),
    })
}

/// The command asked, as it runs: what is around it is no part of it, and
/// none is refused (a line that is the paths alone would run the first).
fn asked<'a>(command: &'a str, on: &On) -> anyhow::Result<&'a str> {
    let command = command.trim();
    anyhow::ensure!(
        !command.is_empty(),
        match on {
            On::Entries(_) => words!(
                shell_write_command =
                    "Write the command to run: the selected paths are its arguments"
            ),
            On::Folder(_) =>
                words!(shell_write_command_here = "Write the command to run in this folder"),
        }
    );
    Ok(command)
}

/// How many paths the line said as the command is typed shows.
const SAID: usize = 4;

/// What runs, said as the command is typed (`shell.run.describe`,
/// `shell.run_here.describe`): the [`Line`] with the first `SAID` paths it
/// is given, how many more, and the folder it runs in. An empty command is
/// refused, as no answer; so is any where nothing runs.
fn said(app: &App, command: &str, pane: Option<PaneId>, on: On) -> anyhow::Result<String> {
    runs_here()?;
    let command = asked(command, &on)?;
    let (dir, shown, count) = where_and_what(app, pane, on, SAID)?;
    let program = program(app);
    let line = Line::new(&program, command).said(&shown, count - shown.len());
    // Drawn under the window's field as the command is typed.
    Ok(words!(
        shell_runs = "Runs { $line }  in { $folder }",
        line = line,
        folder = skwir_core::os_path::from_home(&dir),
        note = "Said as a command is typed in Run Command…: line is what runs (the command \
                with its paths); folder the path of the folder it runs in."
    )
    .say())
}

/// What Run Command…'s field takes (ADR-137): on the selection, or in the
/// folder with no paths (`here`).
pub fn reference(here: bool) -> skwir_core::reference::Reference {
    use skwir_core::reference::{Group, Item, Reference};
    let for_example = words!(
        shell_examples = "For example",
        note = "The heading of commands given as examples under Run Command…'s field."
    );
    let group = if here {
        Group::new(for_example)
            .item(Item::new(
                "ls -la",
                words!(shell_example_ls = "What it holds, hidden files too"),
            ))
            .item(Item::new(
                "du -sh",
                words!(shell_example_du_here = "How much it takes"),
            ))
            .item(Item::new(
                "df -h .",
                words!(shell_example_df = "How full its disk is"),
            ))
            .item(Item::new(
                "git status",
                words!(shell_example_git = "What git says of it"),
            ))
            .note(words!(
                shell_runs_here_note = "It runs in a terminal, in this folder, with no paths after it, with the shell Settings › Terminal names, the terminal staying open after."
            ))
    } else {
        Group::new(for_example)
            .item(Item::new(
                "du -sh",
                words!(shell_example_du = "How much each takes"),
            ))
            .item(Item::new(
                "file",
                words!(shell_example_file = "What each is"),
            ))
            .item(Item::new(
                "wc -l",
                words!(shell_example_wc = "How many lines each has"),
            ))
            .item(Item::new(
                "sha256sum",
                words!(shell_example_sha = "Their checksums"),
            ))
            .item(Item::new(
                "ls -ld",
                words!(shell_example_ls_ld = "Their details"),
            ))
            .note(words!(
                shell_runs_note = "The selected paths come after it, each one argument, unless it names them itself (\"$@\" or $1… in sh, bash or zsh, $argv in fish): then they go only there. It runs in a terminal, in the folder holding them, with the shell Settings › Terminal names, the terminal staying open after."
            ))
    };
    Reference::new(words!(
        shell_reference_title = "Command",
        note = "The title of what Run Command…'s field takes."
    ))
    .group(group)
}

/// Run `command` in a terminal: on `on`'s entries (`App::targets`), in
/// the folder holding them, or with none in a folder.
fn run(app: &App, command: &str, pane: Option<PaneId>, on: On) -> anyhow::Result<Outcome> {
    runs_here()?;
    let command = asked(command, &on)?;
    let (dir, paths, _) = where_and_what(app, pane, on, usize::MAX)?;
    let program = program(app);
    let argv = Line::new(&program, command).argv(&paths);
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
                    app.notify(skwir_core::app::NotifyLevel::Error, Words::of_error(e));
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
    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(shell_plugin_title = "Shell")
    }
    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            shell_plugin_description =
                "Copy selected paths for a shell; run a command on the selection in a terminal"
        )
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
            words!(shell_program = "Program for commands"),
            words!(
                shell_program_description = "The shell that runs a command on the selection: a POSIX shell or fish. Leave empty for yours ($SHELL) when it is one of them, else sh."
            ),
            "",
        )
        .page(skwir_core::settings::page::TERMINAL)
        .section(skwir_core::settings::section::COMMANDS));
        app.commands
            .register::<TargetsArgs, _>(
                ID,
                Topic::Files,
                "shell.copy_paths",
                words!(shell_copy_paths = "Copy paths for a shell"),
                words!(
                    shell_copy_paths_description =
                        "The selected paths, or the entries', shell-quoted, for the clipboard"
                ),
                |a, app| {
                    let paths: Vec<PathBuf> = app
                        .targets(a.pane, a.entries)?
                        .into_iter()
                        .map(|r| r.path)
                        .collect();
                    let n = paths.len();
                    Ok(Outcome::Clipboard {
                        text: quote_paths(&paths),
                        said: words!(
                            shell_copied_paths = "Copied { $count ->
                                [one] { $count } path
                               *[other] { $count } paths
                            }",
                            count = n
                        ),
                    })
                },
            )
            .reads();
        app.commands.register::<RunArgs, _>(
            ID, Topic::Terminal,
            "shell.run",
            words!(shell_run = "Run command on selection…"),
            words!(
                shell_run_description = "Run a shell command in a terminal with the selected paths, or the entries', as its arguments"
            ),
            |a, app| run(app, &a.command, a.pane, On::Entries(a.entries)),
        );
        app.commands
            .register::<RunArgs, _>(
                ID,
                Topic::Terminal,
                "shell.run.describe",
                words!(shell_run_describe = "Say what a command runs"),
                words!(
                    shell_run_describe_description = "Say what `shell.run` would run with `command` on the selected paths, or the entries': the line with the first paths as its shell is given them (after it, or where it names its arguments), how many more, and the folder; an error for an empty command, or where nothing runs"
                ),
                |a, app| {
                    Ok(Outcome::Value(serde_json::json!({
                        "says": said(app, &a.command, a.pane, On::Entries(a.entries))?
                    })))
                },
            )
            .data();
        app.commands.register::<HereArgs, _>(
            ID,
            Topic::Terminal,
            "shell.run_here",
            words!(shell_run_here = "Run command here…"),
            words!(
                shell_run_here_description = "Run a shell command in a terminal in a folder (`in`, else the one the pane shows), with no paths"
            ),
            |a, app| run(app, &a.command, a.pane, On::Folder(a.folder)),
        );
        app.commands
            .register::<HereArgs, _>(
                ID,
                Topic::Terminal,
                "shell.run_here.describe",
                words!(shell_run_here_describe = "Say what a command runs here"),
                words!(
                    shell_run_here_describe_description = "Say what `shell.run_here` would run with `command`: the line and the folder; an error for an empty command, or where nothing runs"
                ),
                |a, app| {
                    Ok(Outcome::Value(serde_json::json!({
                        "says": said(app, &a.command, a.pane, On::Folder(a.folder))?
                    })))
                },
            )
            .data();
        // No key and no menu item: copying for a shell is rare, and the
        // menu's Copy Path, Ctrl+Shift+C and Ctrl+Alt+C copy the paths as
        // text (`copy_paths`). The palette has it, and a key can be given.
        // Beside the applications that open the selection, or the folder
        // on its background (in the folder, with no paths): both hand it
        // to a program (ADR-099). Where no terminal runs a command, none.
        let asks = [
            (
                "selection !place",
                skwir_core::registry::submenus::OPEN_WITH,
                words!(shell_ask_run = "Run command on selection"),
                words!(shell_ask_run_placeholder = "Command; the selected paths are its arguments"),
                "shell.run",
                false,
            ),
            (
                "empty folder",
                skwir_core::registry::submenus::OPEN_FOLDER_WITH,
                words!(shell_ask_run_here = "Run command in this folder"),
                words!(shell_ask_run_here_placeholder = "Command; it runs in this folder"),
                "shell.run_here",
                true,
            ),
        ];
        for (when, submenu, title, placeholder, command, here) in asks.into_iter().filter(|_| RUNS)
        {
            app.registry.menus.add(
                ID,
                MenuItem {
                    menu: "context".into(),
                    title: words!(shell_run_menu = "Run Command…"),
                    // The command is asked for: neither runs without one.
                    command: Invocation::with_args(
                        "ui.ask",
                        &serde_json::json!({
                            // Said when the menu is made: the question's
                            // arguments are text.
                            "title": title.say(),
                            "accept": words!(
                                shell_ask_accept = "Run",
                                note = "The button that runs the command typed."
                            )
                            .say(),
                            "placeholder": placeholder.say(),
                            "command": command,
                            "field": "command",
                            "reference": reference(here),
                            // Asked about what the question is (ADR-137).
                            "describe": { "command": format!("{command}.describe") },
                        }),
                    ),
                    when: Some(when.into()),
                    order: 9,
                    icon: Some("command".into()),
                    submenu: Some(submenu),
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
    use skwir_core::app::AppConfig;
    // What the tests run in a terminal read (not on a Mac, where none runs).
    #[cfg(all(unix, not(target_os = "macos")))]
    use skwir_core::location::Location;
    use std::sync::Arc;
    #[cfg(all(unix, not(target_os = "macos")))]
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
        // fish's own: `\'` and `\\` in its single quotes, bytes outside.
        assert_eq!(quote_fish(b"plain-1.txt"), "plain-1.txt");
        assert_eq!(quote_fish(br"it's a\b"), r"'it\'s a\\b'");
        assert_eq!(quote_fish(b""), "''");
        assert_eq!(quote_fish(b"a\x1bb c"), r"a\x1bb\x20c");
        assert_eq!(quote_fish(b"caf\xe9"), r"caf\Xe9");
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

    /// A command that names its arguments itself is given them there
    /// alone, as its shell reads it: never in single quotes, nor escaped;
    /// `$0` is no argument. Its line says so.
    #[test]
    fn a_command_naming_its_arguments_is_given_them_there_alone() {
        for named in [
            r#"cp -r "$@" /backup"#,
            "echo $@",
            "echo $*",
            r#"mv "$1" "$1.bak""#,
            "echo ${1}",
            "echo ${10}",
            "echo ${@}",
            "echo ${#1}",
            r"echo \\$1",
            r#"echo "$(basename "$1")""#,
            "echo 'a'$1",
        ] {
            assert!(names_arguments(named, Family::Posix), "{named}");
        }
        for not in [
            "ls -l",
            "echo '$@'",
            r#"echo "it's" '$1'"#,
            r"echo \$@",
            r#"echo "\$1""#,
            "echo $0 ${0}",
            "echo $$ $HOME ${HOME}",
            r"echo $'\'$1'",
            "echo ${#}",
        ] {
            assert!(!names_arguments(not, Family::Posix), "{not}");
        }
        assert!(names_arguments("cp $argv /backup", Family::Fish));
        assert!(names_arguments(r#"echo "$argv[1]""#, Family::Fish));
        assert!(!names_arguments("echo '$argv' $argvs", Family::Fish));
        assert!(!names_arguments(r"echo 'it\'s $argv'", Family::Fish));
        assert!(!names_arguments(r#"echo "$@""#, Family::Fish));

        // Given them there, never after it as well.
        let argv = run_argv("/bin/sh", r#"cp -r "$@" /backup"#, &["/x/a".into()]);
        assert_eq!(argv[6..], [r#"cp -r "$@" /backup"#, "skwir", "/x/a"]);
        let argv = run_argv("/bin/sh", "echo '$@'", &["/x/a".into()]);
        assert_eq!(argv[6..], [r#"echo '$@' "$@""#, "skwir", "/x/a"]);
        let argv = run_argv("/usr/bin/fish", "cp $argv /backup", &["/x/a".into()]);
        assert_eq!(argv[6..], ["cp $argv /backup", "/x/a"]);

        let line = Line::new("/bin/sh", r#"cp -r "$@" /backup"#);
        assert_eq!(
            line.said(&["/x/a b".into()], 2).english(),
            r#"cp -r "$@" /backup  with the paths where it names them: '/x/a b' … and 2 more"#
        );
        let line = Line::new("/bin/sh", "echo '$@'");
        assert_eq!(
            line.said(&["/x/a b".into()], 0).english(),
            "echo '$@' '/x/a b'"
        );
        assert_eq!(line.said(&[], 0).english(), "echo '$@'  with no paths");
    }

    /// Runs `script` in `program` (`-c`) with `args` as its arguments, and
    /// returns what it printed.
    #[cfg(unix)]
    fn printed(program: &Path, script: &str, args: &[PathBuf]) -> Vec<u8> {
        let mut command = std::process::Command::new(program);
        if family(&program.to_string_lossy()) == Some(Family::Fish) {
            command.arg("--no-config");
        }
        let out = command.arg("-c").arg(script).args(args).output().unwrap();
        assert!(out.status.success(), "{script}: {out:?}");
        out.stdout
    }

    /// The line said, typed in its shell, gives the command the arguments
    /// the argv gives it, byte for byte: paths whole, quoted as that shell
    /// reads them (a name starting with `-`, quotes, a tab, an escape, a
    /// name that is not UTF-8), fish's own way in fish.
    #[cfg(unix)]
    #[test]
    fn the_line_said_gives_what_the_argv_gives() {
        use std::os::unix::ffi::OsStrExt;
        let paths: Vec<PathBuf> = [
            &b"/x/a b"[..],
            b"/x/it's $HOME",
            b"/x/-n",
            b"/x/tab\there",
            b"/x/back\\slash \"q\"",
            b"/x/esc\x1b[201~",
            "/x/été".as_bytes(),
            b"/x/caf\xe9",
        ]
        .into_iter()
        .map(|bytes| PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
        .collect();
        let shells = ["bash", "fish"]
            .into_iter()
            .filter_map(skwir_core::os_path::find_program);
        for shell in shells {
            let program = shell.to_string_lossy();
            let line = Line::new(&program, "printf '<%s>'");
            let typed = printed(&shell, &line.said(&paths, 0).english(), &[]);
            // What runs: the argv's script, given the paths.
            let argv = line.argv(&paths);
            let ran = printed(
                &shell,
                &argv[6].to_string_lossy(),
                &argv[7..].iter().map(PathBuf::from).collect::<Vec<_>>(),
            );
            let mut wanted = Vec::new();
            for path in &paths {
                wanted.extend_from_slice(b"<");
                wanted.extend_from_slice(path.as_os_str().as_bytes());
                wanted.extend_from_slice(b">");
            }
            assert_eq!(
                typed,
                wanted,
                "{program}: typed {}",
                String::from_utf8_lossy(&typed)
            );
            assert_eq!(ran, wanted, "{program}: ran");
        }
    }

    /// Runs the argv without a terminal, with a `$SHELL` that exits at once,
    /// and returns what the command printed.
    #[cfg(unix)]
    fn output_of(program: &str, command: &str, paths: &[PathBuf]) -> String {
        let argv = run_argv(program, command, paths);
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
        assert_eq!(
            output_of("/bin/sh", "printf '<%s>'", &paths),
            "</x/a b></x/it's $HOME>"
        );
        // Named by the command, given there once.
        assert_eq!(
            output_of("/bin/sh", r#"printf '<%s>' "$2" "$@""#, &paths),
            "</x/it's $HOME></x/a b></x/it's $HOME>"
        );
        if let Some(fish) = skwir_core::os_path::find_program("fish") {
            let fish = fish.to_string_lossy();
            assert_eq!(
                output_of(&fish, "printf '<%s>'", &paths),
                "</x/a b></x/it's $HOME>"
            );
            assert_eq!(
                output_of(&fish, "printf '<%s>' $argv[2]", &paths),
                "</x/it's $HOME>"
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

    /// Run Command…'s line, as the command is typed: the paths as the
    /// shell is given them, whole and quoted, the first few and how many
    /// more, after it or where it names them, and the folder; on a
    /// folder's background, no paths. An empty command is no answer
    /// (ADR-137).
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn what_runs_is_said_as_the_command_is_typed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        for name in ["a b.txt", "c.txt", "d.txt", "e.txt", "f.txt", "g.txt"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(ShellPlugin), &app);
        app.settings
            .set(PROGRAM_KEY, toml::Value::String("/bin/sh".into()));
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&dir)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::new("select_all")).unwrap();
        let said = |name: &str, args: serde_json::Value| {
            app.dispatch(&Invocation::with_args(name, &args))
                .map(|outcome| match outcome {
                    Outcome::Value(v) => v["says"].as_str().unwrap().to_owned(),
                    other => panic!("{other:?}"),
                })
        };
        let path = |name: &str| quote(&dir.join(name).to_string_lossy());
        let folder = skwir_core::os_path::from_home(&dir);
        let says = said(
            "shell.run.describe",
            serde_json::json!({ "command": " du -sh " }),
        )
        .unwrap();
        assert_eq!(
            says,
            format!(
                "Runs du -sh {} {} {} {} … and 2 more  in {folder}",
                path("a b.txt"),
                path("c.txt"),
                path("d.txt"),
                path("e.txt")
            )
        );
        // What the window asks: the entries it was opened for, named.
        let named = serde_json::json!({
            "command": r#"cp "$@" /backup"#,
            "entries": [dir.join("c.txt"), dir.join("a b.txt")],
        });
        assert_eq!(
            said("shell.run.describe", named).unwrap(),
            format!(
                r#"Runs cp "$@" /backup  with the paths where it names them: {} {}  in {folder}"#,
                path("c.txt"),
                path("a b.txt")
            )
        );
        assert!(said("shell.run.describe", serde_json::json!({ "command": "  " })).is_err());
        // On the folder's background: in the folder asked about, no paths.
        let here = serde_json::json!({ "command": "ls -la", "in": tmp.path() });
        assert_eq!(
            said("shell.run_here.describe", here).unwrap(),
            format!(
                "Runs ls -la  with no paths  in {}",
                skwir_core::os_path::from_home(tmp.path())
            )
        );
        assert!(
            said(
                "shell.run_here.describe",
                serde_json::json!({ "command": "" })
            )
            .is_err()
        );
        assert!(!reference(false).is_empty() && !reference(true).is_empty());
        // Both are in the menu, each asking its own.
        let asked: Vec<String> = app
            .registry
            .menus
            .all()
            .into_iter()
            .filter(|m| m.title.english() == "Run Command…")
            .map(|m| m.command.args["command"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(asked, ["shell.run", "shell.run_here"]);
    }

    /// Where no terminal runs a command yet, neither is run nor said, and
    /// the menu offers neither.
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn elsewhere_no_command_runs_yet() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("a.txt");
        std::fs::write(&file, b"").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(ShellPlugin), &app);
        for name in ["shell.run", "shell.run.describe"] {
            let error = app
                .dispatch(&Invocation::with_args(
                    name,
                    &serde_json::json!({ "command": "ls", "entries": [&file] }),
                ))
                .unwrap_err();
            assert!(
                format!("{error:#}").contains("not available"),
                "{name}: {error:#}"
            );
        }
        assert!(
            app.registry
                .menus
                .all()
                .iter()
                .all(|m| m.title.english() != "Run Command…")
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
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
        assert_eq!(said.english(), "Copied 1 path");

        // What the terminal was handed, once it has: its argv, then the
        // folder it ran in.
        let ran = |name: &str, command: &str, mut args: serde_json::Value| -> Vec<String> {
            let _ = std::fs::remove_file(&record);
            args["command"] = command.into();
            app.dispatch(&Invocation::with_args(name, &args)).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            // Publish the complete record atomically; existence means both
            // argv and the working directory have been written by the child.
            while !record.exists() {
                assert!(std::time::Instant::now() < deadline, "terminal never ran");
                std::thread::sleep(Duration::from_millis(10));
            }
            std::fs::read_to_string(&record)
                .unwrap()
                .lines()
                .map(str::to_owned)
                .collect()
        };
        let lines = ran("shell.run", "ls -l", serde_json::json!({}));
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
        // Named by the command, the paths go there alone.
        let lines = ran("shell.run", r#"wc -l "$1""#, serde_json::json!({}));
        assert_eq!(lines[5..9], ["/bin/sh", "-c", r#"wc -l "$1""#, "skwir"]);
        assert_eq!(lines[9], dir.join("a b.txt").to_string_lossy());
        // Here, in the folder asked about, with no paths.
        let lines = ran(
            "shell.run_here",
            "ls -la",
            serde_json::json!({ "in": tmp.path() }),
        );
        assert_eq!(lines[5..9], ["/bin/sh", "-c", "ls -la \"$@\"", "skwir"]);
        assert_eq!(
            lines[9],
            tmp.path().to_string_lossy(),
            "no paths, in the folder"
        );
        assert_eq!(lines.len(), 10);
    }
}
