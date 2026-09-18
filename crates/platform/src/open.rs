//! Open with the default application; open a terminal.

use std::path::Path;

/// Open `path` with the system default application, detached.
pub fn open_path(path: &Path) -> anyhow::Result<()> {
    open::that_detached(path).map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))
}

/// Open a terminal emulator in `dir`, detached.
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

    pub fn open_terminal(dir: &Path) -> anyhow::Result<()> {
        let term = std::env::var("TERMINAL")
            .ok()
            .filter(|t| !t.is_empty())
            .or_else(|| {
                CANDIDATES
                    .iter()
                    .find(|c| on_path(c))
                    .map(|c| (*c).to_owned())
            })
            .ok_or_else(|| anyhow::anyhow!("no terminal emulator found (set $TERMINAL)"))?;
        Command::new(&term)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("launch {term}: {e}"))?;
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::path::Path;

    pub fn open_terminal(_dir: &Path) -> anyhow::Result<()> {
        anyhow::bail!("open_terminal is not implemented on this platform yet")
    }
}
