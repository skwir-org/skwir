//! Open with the default application; open a terminal.

use std::ffi::OsStr;
use std::path::Path;

use skwir_core::words;

/// Whether `uri` is a web link this surface follows: `http` or `https`,
/// with a host, and nothing a handler could take for another argument.
/// What a file names (a download's origin) comes here, so nothing else
/// reaches the desktop's handlers.
pub fn is_web_uri(uri: &str) -> bool {
    let lower = uri.get(..8).unwrap_or(uri).to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &uri[8..]
    } else if lower.starts_with("http://") {
        &uri[7..]
    } else {
        return false;
    };
    !rest.is_empty()
        && !rest.starts_with('/')
        && !uri.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Whether `uri` is a mail address to write to: `mailto:` and someone,
/// with nothing a handler could take for another argument.
pub fn is_mail_uri(uri: &str) -> bool {
    uri.get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
        && uri.len() > 7
        && !uri.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Follow an address a document or the terminal shows, when the user asks:
/// a web page goes to the browser, a mail address to the mail client
/// (`is_web_uri`, `is_mail_uri`), and nothing else reaches the desktop's
/// handlers from a file's text. It starts a program: call it on a worker.
pub fn open_address(uri: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        is_web_uri(uri) || is_mail_uri(uri),
        words!(
            open_not_web_or_mail = "only web and mail links open from here",
            note = "Why a link in a document or the terminal is not followed: it is neither a web page nor a mail address."
        )
    );
    open::that_detached(uri).map_err(Into::into)
}

/// Show a place in the desktop's maps application: a `geo:` URI (RFC 5870)
/// and nothing else, so no other scheme's handler runs from here.
pub fn open_geo_uri(uri: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        uri.starts_with("geo:") && !uri.contains(char::is_whitespace),
        words!(
            open_not_geo = "not a map location",
            note = "Why a position is not shown in the desktop's maps application: it is no geo: address."
        )
    );
    open::that_detached(uri).map_err(Into::into)
}

/// Open `path` with the system default application, detached.
pub fn open_path(path: &Path) -> anyhow::Result<()> {
    open::that_detached(path).map_err(|e| {
        words!(
            open_path_failed = "open { $path }: { $error }",
            path = path.display().to_string(),
            error = anyhow::Error::from(e),
            note = "The desktop's default application could not be started for a file. path: the file; error: why."
        )
        .into()
    })
}

/// Open a terminal in `dir` running `argv`, and call `on_exit` once the
/// program, and so the terminal, has ended (on the reaper thread, see
/// [`crate::process::OnExit`]). Terminals that would hand the command to an
/// already running instance and return at once are asked to wait for it.
pub fn open_terminal_watched<S: AsRef<OsStr>>(
    dir: &Path,
    argv: &[S],
    on_exit: impl FnOnce(std::io::Result<std::process::ExitStatus>) + Send + 'static,
) -> anyhow::Result<()> {
    let mut command = imp::terminal_with(dir, argv)?;
    crate::process::spawn_watched(&mut command, on_exit).map_err(|e| launch_failed(&command, e))
}

/// Open a terminal emulator in `dir`, detached.
pub fn open_terminal(dir: &Path) -> anyhow::Result<()> {
    let mut command = imp::terminal(dir)?;
    crate::process::spawn_detached(&mut command).map_err(|e| launch_failed(&command, e))
}

/// Why the terminal `command` did not start.
fn launch_failed(command: &std::process::Command, error: std::io::Error) -> anyhow::Error {
    words!(
        open_launch_failed = "launch { $program }: { $error }",
        program = command.get_program().display().to_string(),
        error = anyhow::Error::from(error),
        note = "A terminal could not be started. program: the terminal's program; error: why."
    )
    .into()
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::{OsStr, OsString};
    use std::path::Path;
    use std::process::{Command, Stdio};

    use skwir_core::os_path::find_program;

    const CANDIDATES: [&str; 8] = [
        "kitty",
        "ghostty",
        "alacritty",
        "foot",
        "wezterm",
        "gnome-terminal",
        "konsole",
        "xterm",
    ];

    fn program() -> anyhow::Result<OsString> {
        std::env::var_os("TERMINAL")
            .filter(|t| !t.is_empty())
            .or_else(|| {
                CANDIDATES
                    .iter()
                    .find(|c| find_program(c).is_some())
                    .map(OsString::from)
            })
            .ok_or_else(|| {
                skwir_core::words!(
                    open_no_terminal = "no terminal emulator found (set $TERMINAL)",
                    note = "No terminal program is installed that Skwir knows; $TERMINAL is the environment variable that names one (not translated)."
                )
                .into()
            })
    }

    fn detached(term: &OsStr, dir: &Path) -> Command {
        let mut c = crate::process::command(term);
        c.current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c
    }

    pub fn terminal(dir: &Path) -> anyhow::Result<Command> {
        Ok(detached(&program()?, dir))
    }

    /// The flag that makes each known terminal run a command, and wait for
    /// it: `-e` for most, `--wait --` for gnome-terminal (whose client
    /// otherwise returns at once), `start --always-new-process --` for
    /// wezterm.
    pub fn terminal_with<S: AsRef<OsStr>>(dir: &Path, argv: &[S]) -> anyhow::Result<Command> {
        anyhow::ensure!(
            !argv.is_empty(),
            skwir_core::words!(
                open_no_command = "no command to run",
                note = "A terminal was asked to run a program, and none was given."
            )
        );
        let term = program()?;
        let name = Path::new(&term)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut cmd = detached(&term, dir);
        match name.as_str() {
            "gnome-terminal" => {
                cmd.args(["--wait", "--"]);
            }
            "wezterm" => {
                cmd.args(["start", "--always-new-process", "--"]);
            }
            _ => {
                cmd.arg("-e");
            }
        }
        cmd.args(argv);
        Ok(cmd)
    }
}

/// macOS: the terminal application is `$TERMINAL` (an application's
/// name, `iTerm`) or Terminal, opened in the folder by `open -a`. Compiled,
/// never run (ADR-035).
#[cfg(target_os = "macos")]
// One signature on every system: Linux's can fail to find a terminal.
#[allow(clippy::unnecessary_wraps)]
mod imp {
    use std::ffi::{OsStr, OsString};
    use std::path::Path;
    use std::process::{Command, Stdio};

    pub fn terminal(dir: &Path) -> anyhow::Result<Command> {
        let app = std::env::var_os("TERMINAL")
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| OsString::from("Terminal"));
        let mut c = crate::process::command("/usr/bin/open");
        c.arg("-a")
            .arg(app)
            .arg(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Ok(c)
    }

    /// A Mac's terminal applications hand a command to a window and return
    /// at once: nothing waits for the program yet.
    pub fn terminal_with<S: AsRef<OsStr>>(_dir: &Path, _argv: &[S]) -> anyhow::Result<Command> {
        Err(skwir_core::system::not_yet_words(skwir_core::words!(
            open_terminal_program_not_yet =
                "A terminal running a program is not available on { $system } yet",
            system = skwir_core::system::named(),
            note =
                "Why a program is not run in a terminal window. system: the system's name (macOS)."
        ))
        .into())
    }
}

/// Windows: Windows Terminal (`wt`) where it is installed, else a console
/// of its own; a program to run gets a console of its own, which is the
/// terminal it runs in and ends with it, so waiting for the program waits
/// for the terminal. Run under Wine only as far as Wine has consoles.
#[cfg(windows)]
// One signature on every system: Linux's can fail to find a terminal.
#[allow(clippy::unnecessary_wraps)]
mod imp {
    use std::ffi::OsStr;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    /// `CREATE_NEW_CONSOLE`: the program gets a console window of its own,
    /// in place of the none `process::command` gives.
    const NEW_CONSOLE: u32 = 0x10;

    pub fn terminal(dir: &Path) -> anyhow::Result<Command> {
        if let Some(wt) = skwir_core::os_path::find_program("wt") {
            let mut c = crate::process::command(wt);
            c.arg("-d")
                .arg(dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            return Ok(c);
        }
        let shell = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
        let mut c = crate::process::command(shell);
        c.current_dir(dir).creation_flags(NEW_CONSOLE);
        Ok(c)
    }

    pub fn terminal_with<S: AsRef<OsStr>>(dir: &Path, argv: &[S]) -> anyhow::Result<Command> {
        let (program, rest) = argv.split_first().ok_or_else(|| {
            anyhow::Error::from(skwir_core::words!(
                open_no_command = "no command to run",
                note = "A terminal was asked to run a program, and none was given."
            ))
        })?;
        let mut c = crate::process::command(program);
        c.args(rest).current_dir(dir).creation_flags(NEW_CONSOLE);
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_mail_uri, is_web_uri};

    #[test]
    fn only_web_links_reach_the_desktop() {
        for ok in [
            "https://example.org/pic.jpg",
            "http://example.org",
            "HTTPS://example.org/a?b=c#d",
            "https://example.org/it%27s%20here",
        ] {
            assert!(is_web_uri(ok), "{ok}");
        }
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://example.org",
            "smb://server/share",
            "https://",
            "https:///local",
            "https://example.org/a b",
            "https://example.org/\n--new-window",
            "--help",
            "",
            "héhé",
        ] {
            assert!(!is_web_uri(refused), "{refused}");
        }
    }

    #[test]
    fn a_mail_address_reaches_the_mail_client_alone() {
        assert!(is_mail_uri("mailto:someone@example.org?subject=Hi"));
        assert!(is_mail_uri("MAILTO:someone@example.org"));
        for refused in ["mailto:", "mailto:a b@c", "mailto:a@b\n-x", "mail:a@b", ""] {
            assert!(!is_mail_uri(refused), "{refused}");
        }
    }
}
