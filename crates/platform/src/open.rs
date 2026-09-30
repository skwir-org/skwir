//! Open with the default application; open a terminal.

use std::ffi::OsStr;
use std::path::Path;

/// Follow a user-activated web link; document actions never launch local files
/// or executable URI schemes through this surface.
pub fn open_web_uri(uri: &str) -> anyhow::Result<()> {
    anyhow::ensure!(is_web_uri(uri), "not a web link");
    open::that_detached(uri).map_err(Into::into)
}

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

/// Show a place in the desktop's maps application: a `geo:` URI (RFC 5870)
/// and nothing else, so no other scheme's handler runs from here.
pub fn open_geo_uri(uri: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        uri.starts_with("geo:") && !uri.contains(char::is_whitespace),
        "not a map location"
    );
    open::that_detached(uri).map_err(Into::into)
}

/// Open `path` with the system default application, detached.
pub fn open_path(path: &Path) -> anyhow::Result<()> {
    open::that_detached(path).map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))
}

/// Open a terminal in `dir` running `argv` (program and arguments) instead
/// of a shell; the terminal closes when the program exits. Returns the child
/// for callers that wait for it themselves; prefer [`open_terminal_watched`],
/// which needs no waiting thread.
pub fn open_terminal_with<S: AsRef<OsStr>>(
    dir: &Path,
    argv: &[S],
) -> anyhow::Result<std::process::Child> {
    let mut command = imp::terminal_with(dir, argv)?;
    command
        .spawn()
        .map_err(|e| anyhow::anyhow!("launch {}: {e}", command.get_program().display()))
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
    crate::process::spawn_watched(&mut command, on_exit)
        .map_err(|e| anyhow::anyhow!("launch {}: {e}", command.get_program().display()))
}

/// Open a terminal emulator in `dir`, detached.
pub fn open_terminal(dir: &Path) -> anyhow::Result<()> {
    let mut command = imp::terminal(dir)?;
    crate::process::spawn_detached(&mut command)
        .map_err(|e| anyhow::anyhow!("launch {}: {e}", command.get_program().display()))
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::{OsStr, OsString};
    use std::path::Path;
    use std::process::{Command, Stdio};

    use bibli_core::os_path::find_program;

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
            .ok_or_else(|| anyhow::anyhow!("no terminal emulator found (set $TERMINAL)"))
    }

    fn detached(term: &OsStr, dir: &Path) -> Command {
        let mut c = Command::new(term);
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
        anyhow::ensure!(!argv.is_empty(), "no command to run");
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

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::Command;

    pub fn terminal(_dir: &Path) -> anyhow::Result<Command> {
        anyhow::bail!("open_terminal is not implemented on this platform yet")
    }

    pub fn terminal_with<S: AsRef<OsStr>>(_dir: &Path, _argv: &[S]) -> anyhow::Result<Command> {
        anyhow::bail!("open_terminal_with is not implemented on this platform yet")
    }
}

#[cfg(test)]
mod tests {
    use super::is_web_uri;

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
}
