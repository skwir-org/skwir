//! rsync, for a batch of copies to or from an sshfs mount Skwir made
//! (ADR-078): one stream through the ssh connection sshfs holds, where
//! the mount costs eight round trips a file. 100 files of 16 KiB from an
//! SFTP server 20 ms away: 16.6 s through the mount, 0.7 with rsync, its
//! start included (2026-10-21).
//!
//! sshfs's ssh is the master of a control socket ([`control_path`]), so
//! rsync's ssh asks nothing again and Skwir passes no credential; with
//! `BatchMode` it never asks either. A server without rsync is
//! remembered for its mount, whose copies then go through the mount.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;
use skwir_core::cancel::CancelToken;
use skwir_core::op::transfer::Transfer;

use crate::address::Address;

/// Where the ssh carrying the sshfs mount at `point` listens for others
/// to share its connection: a name of the point's own in the runtime
/// folder, short enough for a socket's path.
pub fn control_path(point: &Path) -> Option<PathBuf> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in point.as_os_str().as_encoded_bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    Some(
        skwir_core::xdg::runtime_dir()?
            .join("skwir")
            .join("ssh")
            .join(format!("{h:016x}")),
    )
}

/// What sshfs's ssh is given so others share its connection
/// ([`control_path`]): a master, never one asked to persist.
pub fn master_options(control: &Path) -> Vec<String> {
    vec![
        "ControlMaster=auto".to_owned(),
        format!("ControlPath={}", control.display()),
        "ControlPersist=no".to_owned(),
    ]
}

/// rsync through sshfs's connection.
#[derive(Default)]
pub struct Rsync {
    /// ssh options given beside the plugin's (a test's), as sshfs has them.
    pub ssh_options: Vec<String>,
    /// An ssh configuration file in place of the user's (a test's).
    pub ssh_config: Option<PathBuf>,
    /// The mount points whose server has no rsync.
    without: Mutex<HashSet<PathBuf>>,
    /// The batches it copied.
    made: AtomicUsize,
}

impl Rsync {
    /// rsync with ssh given `ssh_options` and `ssh_config` beside the
    /// plugin's, as sshfs's is.
    pub fn with(ssh_options: Vec<String>, ssh_config: Option<PathBuf>) -> Self {
        Rsync {
            ssh_options,
            ssh_config,
            ..Rsync::default()
        }
    }

    /// The batches it copied (a test asks).
    pub fn made(&self) -> usize {
        self.made.load(Ordering::Relaxed)
    }

    /// The sshfs mount Skwir made that holds `path`, its address, and the
    /// control socket of its connection, live; `None` for any other path.
    fn mount_of(&self, path: &Path) -> Option<(PathBuf, Address, PathBuf)> {
        let remote = skwir_core::fs::reach::of(path)?;
        if remote.fstype != crate::sshfs::FSTYPE || self.without.lock().contains(&remote.point) {
            return None;
        }
        let address = crate::mounter::ours("sshfs", &remote.point)?.address?;
        let control = control_path(&remote.point).filter(|c| c.exists())?;
        Some((remote.point.clone(), address, control))
    }

    /// The ssh rsync runs, as one argument rsync splits as a shell does:
    /// through the control socket, asking nothing.
    fn ssh(&self, address: &Address, control: &Path) -> String {
        let mut words = vec!["ssh".to_owned()];
        if let Some(config) = &self.ssh_config {
            words.extend(["-F".to_owned(), config.display().to_string()]);
        }
        for option in self.ssh_options.iter().cloned().chain([
            format!("ControlPath={}", control.display()),
            "ControlMaster=no".to_owned(),
            "BatchMode=yes".to_owned(),
        ]) {
            words.extend(["-o".to_owned(), option]);
        }
        if let Some(port) = address.port {
            words.extend(["-p".to_owned(), port.to_string()]);
        }
        words
            .iter()
            .map(|w| quoted(w))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// `word` in single quotes, as rsync's own splitting of `-e` reads them.
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The local rsync, found once.
fn program() -> Option<&'static PathBuf> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| skwir_core::os_path::find_program("rsync"))
        .as_ref()
}

/// `path` on the server of the mount at `point`: sshfs mounts the
/// server's top there (`host:/`).
fn on_server(point: &Path, path: &Path) -> Option<String> {
    let below = path.strip_prefix(point).ok()?;
    let mut at = String::from("/");
    at.push_str(below.to_str()?);
    Some(at)
}

/// rsync's name for `at` on the server of `address`: `user@host:/path/`
/// (an IPv6 host in the brackets its address keeps).
fn remote_spec(address: &Address, at: &str) -> String {
    let user = address
        .user
        .as_deref()
        .map(|user| format!("{user}@"))
        .unwrap_or_default();
    format!("{user}{}:{}/", address.host, at.trim_end_matches('/'))
}

/// The bytes rsync says it has copied, from a line of
/// `--info=progress2`: its first word, digits alone without
/// `--human-readable`.
fn copied_of(line: &[u8]) -> Option<u64> {
    let line = std::str::from_utf8(line).ok()?;
    let word = line.split_whitespace().next()?;
    let digits: String = word.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty()
        || !word
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
    {
        return None;
    }
    digits.parse().ok()
}

impl Transfer for Rsync {
    fn name(&self) -> &'static str {
        "rsync"
    }

    fn serves(&self, from: &Path, into: &Path) -> bool {
        program().is_some()
            && match (self.mount_of(from), self.mount_of(into)) {
                (Some(_), None) => skwir_core::fs::reach::of(into).is_none(),
                (None, Some(_)) => skwir_core::fs::reach::of(from).is_none(),
                _ => false,
            }
    }

    fn copy(
        &self,
        from: &Path,
        names: &[OsString],
        into: &Path,
        copied: &(dyn Fn(u64) + Sync),
        cancel: &CancelToken,
    ) -> anyhow::Result<()> {
        let rsync = program().ok_or_else(|| anyhow::anyhow!("rsync is not installed"))?;
        let (remote_side, local_side, upward) = match (self.mount_of(from), self.mount_of(into)) {
            (None, Some(mount)) => (mount, from, true),
            (Some(mount), None) => (mount, into, false),
            _ => anyhow::bail!("rsync copies between this machine and a server"),
        };
        let (point, address, control) = remote_side;
        let remote_path = if upward { into } else { from };
        let at = on_server(&point, remote_path).ok_or_else(|| {
            anyhow::anyhow!(
                "“{}” has a name rsync cannot be given",
                remote_path.display()
            )
        })?;
        let mut local = local_side.as_os_str().to_owned();
        local.push("/");
        let remote = remote_spec(&address, &at);
        let mut command = Command::new(rsync);
        command
            .env("LC_ALL", "C")
            .args([
                "--recursive",
                "--links",
                "--perms",
                "--times",
                "--protect-args",
                "--no-human-readable",
                "--info=progress2",
                "--files-from=-",
                "--from0",
                "-e",
            ])
            .arg(self.ssh(&address, &control));
        if upward {
            command.arg(&local).arg(&remote);
        } else {
            command.arg(&remote).arg(&local);
        }
        let mut list = Vec::new();
        for name in names {
            list.extend_from_slice(name.as_encoded_bytes());
            list.push(0);
        }
        let mut line = Vec::new();
        let finished = skwir_platform::process::run_watching(
            &mut command,
            Some(&list),
            cancel,
            &mut |bytes: &[u8]| {
                for &b in bytes {
                    if b == b'\r' || b == b'\n' {
                        if let Some(n) = copied_of(&line) {
                            copied(n);
                        }
                        line.clear();
                    } else {
                        line.push(b);
                    }
                }
            },
        )?;
        if finished.status.success() {
            self.made.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        // rsync's "remote command not found", or the shell's.
        let missing = finished.status.code() == Some(127)
            || finished.stderr.contains("command not found")
            || finished.stderr.contains("rsync: not found");
        if missing {
            tracing::info!(point = %point.display(), "the server has no rsync: copies go through the mount");
            self.without.lock().insert(point);
        }
        anyhow::bail!("rsync failed ({}): {}", finished.status, finished.stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsync_says_how_far_it_went_in_digits() {
        assert_eq!(
            copied_of(b"      1048576  45%  1.23MB/s  0:00:01 (xfr#3, to-chk=7/10)"),
            Some(1_048_576)
        );
        assert_eq!(copied_of(b"1,234,567  12%"), Some(1_234_567));
        assert_eq!(copied_of(b"sending incremental file list"), None);
        assert_eq!(copied_of(b""), None);
    }

    #[test]
    fn a_server_path_is_given_whole_and_quoted_for_ssh() {
        let point = Path::new("/run/user/1000/skwir/remote/x");
        assert_eq!(
            on_server(point, &point.join("srv/it's here")).as_deref(),
            Some("/srv/it's here")
        );
        assert_eq!(on_server(point, Path::new("/elsewhere")), None);
        let address = Address::parse("sftp://me@example.com:2222/").unwrap();
        assert_eq!(
            remote_spec(&address, "/srv/a b"),
            "me@example.com:/srv/a b/"
        );
        let six = Address::parse("sftp://[::1]/").unwrap();
        assert_eq!(remote_spec(&six, "/"), "[::1]:/");
        let rsync = Rsync::default();
        let ssh = rsync.ssh(&address, Path::new("/run/user/1000/skwir/ssh/0123"));
        assert!(ssh.starts_with("'ssh' "));
        assert!(ssh.contains("'ControlPath=/run/user/1000/skwir/ssh/0123'"));
        assert!(ssh.contains("'BatchMode=yes'"));
        assert!(ssh.ends_with("'-p' '2222'"));
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }
}
