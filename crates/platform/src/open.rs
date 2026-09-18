//! Open with the default application; open a terminal.

use std::path::Path;

/// Follow a user-activated web link; document actions never launch local files
/// or executable URI schemes through this surface.
pub fn open_web_uri(uri: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        uri.starts_with("https://") || uri.starts_with("http://"),
        "unsupported document link"
    );
    open::that_detached(uri).map_err(Into::into)
}

/// Open `path` with the system default application, detached.
pub fn open_path(path: &Path) -> anyhow::Result<()> {
    open::that_detached(path).map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))
}

/// Open a terminal emulator in `dir`, detached.
/// Open a terminal in `dir` running `argv` (program and arguments) instead
/// of a shell; the terminal closes when the program exits. Returns the child
/// so callers can wait for it on a worker thread.
pub fn open_terminal_with(dir: &Path, argv: &[String]) -> anyhow::Result<std::process::Child> {
    imp::open_terminal_with(dir, argv)
}

pub fn open_terminal(dir: &Path) -> anyhow::Result<()> {
    imp::open_terminal(dir)
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::Path;
    use std::process::{Command, Stdio};

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

    fn on_path(bin: &str) -> bool {
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
    }

    fn terminal() -> anyhow::Result<String> {
        std::env::var("TERMINAL")
            .ok()
            .filter(|t| !t.is_empty())
            .or_else(|| {
                CANDIDATES
                    .iter()
                    .find(|c| on_path(c))
                    .map(|c| (*c).to_owned())
            })
            .ok_or_else(|| anyhow::anyhow!("no terminal emulator found (set $TERMINAL)"))
    }

    fn detached(term: &str, dir: &Path) -> Command {
        let mut c = Command::new(term);
        c.current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        c
    }

    pub fn open_terminal(dir: &Path) -> anyhow::Result<()> {
        let term = terminal()?;
        detached(&term, dir)
            .spawn()
            .map_err(|e| anyhow::anyhow!("launch {term}: {e}"))?;
        Ok(())
    }

    /// The flag that makes each known terminal run a command: `-e` for most,
    /// `--` for gnome-terminal, `start --` for wezterm.
    pub fn open_terminal_with(dir: &Path, argv: &[String]) -> anyhow::Result<std::process::Child> {
        anyhow::ensure!(!argv.is_empty(), "no command to run");
        let term = terminal()?;
        let name = Path::new(&term)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut cmd = detached(&term, dir);
        match name.as_str() {
            "gnome-terminal" => {
                cmd.arg("--");
            }
            "wezterm" => {
                cmd.args(["start", "--"]);
            }
            _ => {
                cmd.arg("-e");
            }
        }
        cmd.args(argv);
        cmd.spawn()
            .map_err(|e| anyhow::anyhow!("launch {term}: {e}"))
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::path::Path;

    pub fn open_terminal(_dir: &Path) -> anyhow::Result<()> {
        anyhow::bail!("open_terminal is not implemented on this platform yet")
    }

    pub fn open_terminal_with(
        _dir: &Path,
        _argv: &[String],
    ) -> anyhow::Result<std::process::Child> {
        anyhow::bail!("open_terminal_with is not implemented on this platform yet")
    }
}
