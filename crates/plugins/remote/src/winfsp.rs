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
//! A logon task's rclone (ADR-087) is given nothing in its environment:
//! its login is kept beside the mounts as long as the task is, rclone
//! checking it by its line of an htpasswd file, and its port is chosen as
//! it starts (a port fixed beforehand may be taken by then), read back
//! from what its log last says.
//!
//! Compiled and its parts tested on Linux; `WinFsp` is a driver Wine has
//! not, so nothing is mounted under Wine: never run on Windows here.

use std::ffi::OsString;
use std::io::BufRead as _;
use std::path::{Path, PathBuf};

use crate::rc::{At, Login};

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

/// `rclone mount`'s arguments for `remote` at `point` as Skwir gives
/// them, in the foreground: Windows has no `--daemon`.
fn foreground(remote: &str, point: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = crate::rclone::mount_arguments(remote, point)
        .into_iter()
        .filter(|a| a != "--daemon")
        .collect();
    // `--daemon-wait`'s value went with it.
    if let Some(at) = args.iter().position(|a| a == "--daemon-wait") {
        args.drain(at..=at + 1);
    }
    args
}

/// The arguments of `rclone mount` for `remote` at `point`, in the
/// foreground (Windows has no `--daemon`), its remote control at `at`,
/// what it says written to `log`.
pub fn mount_arguments(remote: &str, point: &Path, at: &At, log: &Path) -> Vec<OsString> {
    let mut args = foreground(remote, point);
    args.push("--rc".into());
    args.extend(at.serving().0);
    args.push("--log-file".into());
    args.push(log.into());
    args
}

/// The arguments of the logon task's `rclone mount` for `remote` at
/// `point` (ADR-087): its remote control on a port of the loopback chosen
/// as it starts, behind the login kept for it ([`keep_at_login`]), said in
/// its log, which rclone keeps from growing where it can (`rotates`); no
/// console shown, nothing asked (nobody is there to answer). `None` where
/// `point` is no folder's.
pub fn at_login_arguments(remote: &str, point: &Path, rotates: bool) -> Option<Vec<OsString>> {
    let mut args = foreground(remote, point);
    args.extend(["--rc", "--rc-addr", "127.0.0.1:0", "--rc-htpasswd"].map(OsString::from));
    args.push(at_login_file(point, "htpasswd")?.into());
    args.push("--log-file".into());
    args.push(at_login_file(point, "log")?.into());
    if rotates {
        args.extend(
            ["--log-file-max-size", "1M", "--log-file-max-backups", "1"].map(OsString::from),
        );
    }
    args.extend(["--no-console", "--ask-password=false"].map(OsString::from));
    Some(args)
}

/// The file of the logon task's mount at `point` ending in `ext`, beside
/// those of Skwir's mounts: `.rc\<the folder's name>.at-login.<ext>`.
fn at_login_file(point: &Path, ext: &str) -> Option<PathBuf> {
    Some(point.parent()?.join(".rc").join(format!(
        "{}.at-login.{ext}",
        point.file_name()?.to_string_lossy()
    )))
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

/// The remote control of the mount Skwir made at `point`, as its login
/// was kept.
fn kept_rc(point: &Path) -> Option<At> {
    let text = skwir_core::fs::file::read_regular_text(&login_file(point)?, 4096).ok()?;
    let kept: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(At::Loopback {
        port: u16::try_from(kept["port"].as_u64()?).ok()?,
        user: kept["user"].as_str()?.to_owned(),
        pass: kept["pass"].as_str()?.to_owned(),
    })
}

/// Where the remote control of a mount at `point` may answer, in the
/// order asked: Skwir's own mount's, then the logon task's.
pub fn controls_of(point: &Path) -> Vec<At> {
    kept_rc(point)
        .into_iter()
        .chain(at_login_rc(point))
        .collect()
}

/// The remote control of the mount at `point`: the first of
/// [`controls_of`] something answers at.
pub fn rc_of(point: &Path) -> Option<At> {
    controls_of(point).into_iter().find(At::answers)
}

/// Forget what was kept of the mount at `point`, let go of: a logon
/// task's login stays with its task ([`forget_at_login`]).
pub fn forget(point: &Path) {
    for file in [login_file(point), log_file(point)].into_iter().flatten() {
        let _ = std::fs::remove_file(file);
    }
}

/// Keep `login`, the one the logon task's mount at `point` serves its
/// remote control behind: for Skwir, and as rclone checks it. One kept
/// already stays, which a mount the task started may be serving behind.
///
/// # Errors
/// When the files cannot be written.
pub fn keep_at_login(point: &Path, login: &Login) -> anyhow::Result<()> {
    let (Some(kept), Some(checked)) = (
        at_login_file(point, "json"),
        at_login_file(point, "htpasswd"),
    ) else {
        anyhow::bail!("no folder for its login");
    };
    if let Some(folder) = kept.parent() {
        crate::mounter::make_private(folder)?;
    }
    let login = if let Some(kept) = kept_at_login(point) {
        kept
    } else {
        let text = serde_json::json!({ "user": login.user, "pass": login.pass }).to_string();
        std::fs::write(&kept, text.as_bytes())?;
        login.clone()
    };
    std::fs::write(&checked, login.htpasswd())?;
    Ok(())
}

/// The login kept for the logon task's mount at `point`.
fn kept_at_login(point: &Path) -> Option<Login> {
    let text =
        skwir_core::fs::file::read_regular_text(&at_login_file(point, "json")?, 4096).ok()?;
    let kept: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Login {
        user: kept["user"].as_str()?.to_owned(),
        pass: kept["pass"].as_str()?.to_owned(),
    })
}

/// Forget what the logon task's mount at `point` kept: its login, its
/// log and the log's rotated copies.
pub fn forget_at_login(point: &Path) {
    for ext in ["json", "htpasswd", "log"] {
        if let Some(file) = at_login_file(point, ext) {
            let _ = std::fs::remove_file(file);
        }
    }
    for rotated in rotated_logs(point) {
        let _ = std::fs::remove_file(rotated);
    }
}

/// The logon task's log's rotated copies, newest first: rclone names each
/// by its time, `<name>.at-login-<time>.log`.
fn rotated_logs(point: &Path) -> Vec<PathBuf> {
    let Some(log) = at_login_file(point, "log") else {
        return Vec::new();
    };
    let (Some(folder), Some(name)) = (log.parent(), point.file_name()) else {
        return Vec::new();
    };
    let prefix = format!("{}.at-login-", name.to_string_lossy());
    let mut rotated: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "log")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
        })
        .collect();
    rotated.sort();
    rotated.reverse();
    rotated
}

/// The remote control of the logon task's mount at `point`: its kept
/// login, at the port its log last says it serves on.
fn at_login_rc(point: &Path) -> Option<At> {
    let login = kept_at_login(point)?;
    let port = at_login_file(point, "log")
        .into_iter()
        .chain(rotated_logs(point))
        .find_map(|log| {
            let file = std::fs::File::open(log).ok()?;
            served_port(std::io::BufReader::new(file).lines().map_while(Result::ok))
        })?;
    Some(login.at(port))
}

/// The port the last of `lines`, rclone's log, says its remote control
/// serves on: `… NOTICE: Serving remote control on http://127.0.0.1:5572/`.
fn served_port(lines: impl Iterator<Item = String>) -> Option<u16> {
    lines
        .filter_map(|line| {
            let (_, after) = line.split_once("Serving remote control on http://127.0.0.1:")?;
            after.split('/').next()?.parse().ok()
        })
        .last()
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
    fn a_logon_task_s_rclone_serves_on_a_port_it_chooses_behind_a_kept_login() {
        let point = Path::new("/local/skwir/remote/rclone rclone%3A%2F%2Fphotos%2F");
        let words = |rotates| -> Vec<String> {
            at_login_arguments("photos:", point, rotates)
                .unwrap()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let args = words(false);
        assert_eq!(args[..3], ["mount", "photos:", point.to_str().unwrap()]);
        assert!(!args.iter().any(|a| a.starts_with("--daemon")), "{args:?}");
        let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("--rc-addr"), "127.0.0.1:0");
        assert_eq!(
            Path::new(&after("--rc-htpasswd")),
            Path::new("/local/skwir/remote/.rc/rclone rclone%3A%2F%2Fphotos%2F.at-login.htpasswd")
        );
        assert!(after("--log-file").ends_with(".at-login.log"));
        for flag in ["--no-console", "--ask-password=false"] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
        assert!(!args.iter().any(|a| a.starts_with("--log-file-max")));
        assert!(words(true).iter().any(|a| a == "--log-file-max-size"));
    }

    #[test]
    fn a_logon_task_s_login_is_kept_until_forgotten_and_its_port_read_from_its_log() {
        let folder = tempfile::tempdir().unwrap();
        let point = folder.path().join("rclone rclone%3A%2F%2Fphotos%2F");
        let login = Login::made().unwrap();
        keep_at_login(&point, &login).unwrap();
        let htpasswd = at_login_file(&point, "htpasswd").unwrap();
        assert_eq!(
            std::fs::read_to_string(&htpasswd).unwrap(),
            login.htpasswd()
        );
        assert!(
            !std::fs::read_to_string(&htpasswd)
                .unwrap()
                .contains(&login.pass)
        );
        // Kept again (turned on again while its mount runs): the first stays.
        keep_at_login(&point, &Login::made().unwrap()).unwrap();
        assert_eq!(kept_at_login(&point), Some(login.clone()));
        assert_eq!(
            std::fs::read_to_string(&htpasswd).unwrap(),
            login.htpasswd()
        );

        // No log yet: no port, so nothing to ask.
        assert_eq!(controls_of(&point), []);
        let log = at_login_file(&point, "log").unwrap();
        std::fs::write(
            &log,
            "2026/10/25 08:00:00 NOTICE: Serving remote control on http://127.0.0.1:41000/\n\
             2026/10/25 08:00:01 ERROR : photos: a file failed\n\
             2026/10/25 18:00:00 NOTICE: Serving remote control on http://127.0.0.1:42000/\n\
             2026/10/25 18:00:05 NOTICE: vfs cache: cleaned\n",
        )
        .unwrap();
        assert_eq!(controls_of(&point), [login.at(42000)]);
        // Rotated whole since it started: its copy says it.
        std::fs::write(&log, "2026/10/25 19:00:00 NOTICE: vfs cache: cleaned\n").unwrap();
        let rotated = log
            .with_file_name("rclone rclone%3A%2F%2Fphotos%2F.at-login-2026-10-25T19-00-00.000.log");
        std::fs::write(
            &rotated,
            "2026/10/25 18:00:00 NOTICE: Serving remote control on http://127.0.0.1:43000/\n",
        )
        .unwrap();
        assert_eq!(controls_of(&point), [login.at(43000)]);

        // Skwir's own mount's first; its forgetting leaves the task's.
        let own = At::loopback().unwrap();
        keep_login(&point, &own).unwrap();
        assert_eq!(controls_of(&point), [own, login.at(43000)]);
        forget(&point);
        assert_eq!(controls_of(&point), [login.at(43000)]);
        forget_at_login(&point);
        assert_eq!(controls_of(&point), []);
        let left: Vec<_> = std::fs::read_dir(point.parent().unwrap().join(".rc"))
            .unwrap()
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    /// rclone serving its remote control as a logon task's does: on a port
    /// it chose, behind the kept login's htpasswd line, said in its log.
    /// Found and asked to quit as Eject asks it. Under Wine with rclone's
    /// own Windows release (`just test-windows`), on Linux with the one
    /// installed; skipped, saying why, with none.
    #[test]
    fn a_logon_task_s_remote_control_is_found_by_its_log_and_quits_on_its_login() {
        let Some(rclone) = std::env::var_os("SKWIR_TEST_RCLONE")
            .map(PathBuf::from)
            .or_else(|| skwir_core::os_path::find_program("rclone"))
        else {
            eprintln!("skipped: rclone is not installed");
            return;
        };
        let folder = tempfile::tempdir().unwrap();
        let point = folder.path().join("rclone rclone%3A%2F%2Fphotos%2F");
        let login = Login::made().unwrap();
        keep_at_login(&point, &login).unwrap();
        let args = at_login_arguments("photos:", &point, false).unwrap();
        let from = args.iter().position(|a| a == "--rc").unwrap();
        let config = folder.path().join("rclone.conf");
        std::fs::write(&config, "").unwrap();
        let mut rcd = std::process::Command::new(rclone)
            .arg("rcd")
            .arg("--config")
            .arg(&config)
            .args(args[from + 1..].iter().filter(|a| *a != "--no-console"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let at = loop {
            if let Some(at) = rc_of(&point) {
                break at;
            }
            assert!(std::time::Instant::now() < deadline, "rclone never served");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let cancel = skwir_core::cancel::CancelToken::new();
        #[allow(irrefutable_let_patterns)] // a socket too, on Unix
        let At::Loopback { port, .. } = &at else {
            panic!("{at:?}")
        };
        let another = Login::made().unwrap().at(*port);
        assert!(
            crate::rc::call(&another, "core/version", &serde_json::json!({}), &cancel).is_err()
        );
        crate::rc::call(&at, "core/quit", &serde_json::json!({}), &cancel).unwrap();
        assert!(rcd.wait().unwrap().success());
        forget_at_login(&point);
    }

    #[test]
    fn a_mount_s_login_is_read_back_as_kept() {
        let folder = tempfile::tempdir().unwrap();
        let point = folder.path().join("rclone rclone%3A%2F%2Fphotos%2F");
        let at = At::loopback().unwrap();
        keep_login(&point, &at).unwrap();
        assert_eq!(controls_of(&point), [at]);
        // Nothing answers there: no remote control to ask.
        assert_eq!(rc_of(&point), None);
        forget(&point);
        assert_eq!(controls_of(&point), []);
    }
}
