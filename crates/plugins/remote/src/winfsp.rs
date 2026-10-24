//! rclone's mounts on Windows (ADR-086), through `WinFsp`: in a folder of
//! Skwir's local application data, named so its address reads back
//! (`connector::point_name`), never a drive letter (the user's choice).
//! Windows has no `--daemon`: Skwir starts `rclone mount` with no window,
//! waits until its folder is there, and leaves it running; its remote
//! control answers on the loopback behind a login made for it, kept beside
//! the folder (`.rc`, in the user's own local data) for as long as the
//! mount lives, so Skwir lets go of it by asking it to quit. Windows lists
//! no mount table: the core is told which folder holds Skwir's mounts
//! (`fs::reach::mounts_in`).
//!
//! Compiled and its parts tested on Linux; `WinFsp` is a driver Wine has
//! not, so nothing is mounted under Wine: never run on Windows here.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::rc::At;

/// Where `WinFsp` keeps its library, where installed: under Program Files
/// (x86) whichever the system, as its installer puts it.
pub fn installed() -> bool {
    std::env::var_os("ProgramFiles(x86)")
        .or_else(|| std::env::var_os("ProgramFiles"))
        .is_some_and(|programs| {
            Path::new(&programs)
                .join("WinFsp")
                .join("bin")
                .join("winfsp-x64.dll")
                .exists()
        })
}

/// The arguments of `rclone mount` for `remote` at `point`, in the
/// foreground (Windows has no `--daemon`), its remote control at `at`,
/// what it says written to `log`.
pub fn mount_arguments(remote: &str, point: &Path, at: &At, log: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = crate::rclone::mount_arguments(remote, point)
        .into_iter()
        .filter(|a| a != "--daemon")
        .collect();
    // `--daemon-wait`'s value went with it.
    if let Some(at) = args.iter().position(|a| a == "--daemon-wait") {
        args.drain(at..=at + 1);
    }
    args.push("--rc".into());
    args.extend(at.serving().0);
    args.push("--log-file".into());
    args.push(log.into());
    args
}

/// The file the login of the remote control of the mount at `point` is
/// kept in, beside the mounts: `.rc\<the folder's name>.json`.
pub fn login_file(point: &Path) -> Option<PathBuf> {
    Some(
        point
            .parent()?
            .join(".rc")
            .join(format!("{}.json", point.file_name()?.to_string_lossy())),
    )
}

/// The file what the mount at `point` says is written to.
pub fn log_file(point: &Path) -> Option<PathBuf> {
    Some(
        point
            .parent()?
            .join(".rc")
            .join(format!("{}.log", point.file_name()?.to_string_lossy())),
    )
}

/// Keep the remote control's login of the mount at `point`.
///
/// # Errors
/// When the file cannot be written.
pub fn keep_login(point: &Path, at: &At) -> anyhow::Result<()> {
    let (port, user, pass) = match at {
        At::Loopback { port, user, pass } => (port, user, pass),
        #[cfg(unix)]
        At::Socket(_) => anyhow::bail!("a mount on Windows answers on the loopback"),
    };
    let file = login_file(point).ok_or_else(|| anyhow::anyhow!("no folder for its login"))?;
    if let Some(folder) = file.parent() {
        crate::mounter::make_private(folder)?;
    }
    let text = serde_json::json!({ "port": port, "user": user, "pass": pass }).to_string();
    std::fs::write(&file, text.as_bytes())?;
    Ok(())
}

/// The remote control of the mount at `point`, as its login was kept.
pub fn rc_of(point: &Path) -> Option<At> {
    let text = skwir_core::fs::file::read_regular_text(&login_file(point)?, 4096).ok()?;
    let kept: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(At::Loopback {
        port: u16::try_from(kept["port"].as_u64()?).ok()?,
        user: kept["user"].as_str()?.to_owned(),
        pass: kept["pass"].as_str()?.to_owned(),
    })
}

/// Forget what was kept of the mount at `point`, let go of.
pub fn forget(point: &Path) {
    for file in [login_file(point), log_file(point)].into_iter().flatten() {
        let _ = std::fs::remove_file(file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rclone_mounts_in_the_foreground_its_control_on_the_loopback() {
        let at = At::loopback().unwrap();
        let point = Path::new("/local/skwir/remote/rclone rclone%3A%2F%2Fphotos%2F");
        let args: Vec<String> = mount_arguments("photos:", point, &at, Path::new("/l.log"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[..3], ["mount", "photos:", point.to_str().unwrap()]);
        assert!(!args.iter().any(|a| a.starts_with("--daemon")), "{args:?}");
        let rc = args.iter().position(|a| a == "--rc-addr").unwrap();
        assert!(args[rc + 1].starts_with("127.0.0.1:"));
        #[allow(irrefutable_let_patterns)] // a socket too, on Unix
        let At::Loopback { pass, .. } = &at else {
            panic!()
        };
        assert!(
            !args.iter().any(|a| a.contains(pass.as_str())),
            "the login off its line"
        );
        assert_eq!(
            login_file(point).unwrap(),
            Path::new("/local/skwir/remote/.rc/rclone rclone%3A%2F%2Fphotos%2F.json")
        );
    }

    #[test]
    fn a_mount_s_login_is_read_back_as_kept() {
        let folder = tempfile::tempdir().unwrap();
        let point = folder.path().join("rclone rclone%3A%2F%2Fphotos%2F");
        let at = At::loopback().unwrap();
        keep_login(&point, &at).unwrap();
        assert_eq!(rc_of(&point), Some(at));
        forget(&point);
        assert_eq!(rc_of(&point), None);
    }
}
