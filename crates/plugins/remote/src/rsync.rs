//! rsync, for a batch of copies to or from an sshfs mount Skwir made
//! (ADR-078): one stream through the ssh connection sshfs holds, where
//! the mount costs eight round trips a file. 100 files of 16 KiB from an
//! SFTP server 20 ms away: 16.6 s through the mount, 0.7 with rsync, its
//! start included (2026-10-21).
//!
//! rsync reaches the server as `ssh_mount` says: through the connection
//! sshfs holds for a mount Skwir made, the user's own ssh for one they
//! made, asking nothing either way. A server without rsync, or that the
//! user's ssh does not reach so, is remembered for its mount, whose
//! copies then go by sftp (`sftp`), else through the mount.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;
use skwir_core::op::transfer::{Control, NotServed, Told, Transfer};

use crate::ssh_mount::Mount;

/// rsync through sshfs's connection.
#[derive(Default)]
pub struct Rsync {
    /// ssh options given beside the plugin's (a test's), as sshfs has them.
    pub ssh_options: Vec<String>,
    /// An ssh configuration file in place of the user's (a test's).
    pub ssh_config: Option<PathBuf>,
    /// The mount points whose server has no rsync, or that the user's ssh
    /// does not reach without asking.
    without: Mutex<HashSet<PathBuf>>,
    /// Whether each mount's server runs rsync, asked once.
    runs: Mutex<HashMap<PathBuf, bool>>,
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

    /// The sshfs mount that holds `path`, when rsync reaches its server.
    fn mount_of(&self, path: &Path) -> Option<Mount> {
        Mount::of(path).filter(|mount| !self.without.lock().contains(&mount.point))
    }

    /// Whether the server of `mount` runs rsync, asked once of it through
    /// the same ssh (`rsync --version`): an SFTP-only account answers
    /// nothing of the kind, a server without rsync that it is not found.
    fn server_runs(&self, mount: &Mount, cancel: &skwir_core::cancel::CancelToken) -> bool {
        if let Some(&known) = self.runs.lock().get(&mount.point) {
            return known;
        }
        let mut command = Command::new("ssh");
        if let Some(config) = &self.ssh_config {
            command.arg("-F").arg(config);
        }
        for option in mount.ssh_options(&self.ssh_options) {
            command.arg("-o").arg(option);
        }
        if let Some(port) = mount.port {
            command.arg("-p").arg(port.to_string());
        }
        command.arg(&mount.login).args(["rsync", "--version"]);
        let Ok(finished) = skwir_platform::process::run_to_exit(
            &mut command,
            None,
            cancel,
            std::time::Duration::from_secs(30),
        ) else {
            return false;
        };
        let said = String::from_utf8_lossy(&finished.stdout);
        let runs = finished.status.success() && said.contains("rsync") && said.contains("protocol");
        self.runs.lock().insert(mount.point.clone(), runs);
        runs
    }

    /// The ssh rsync runs, as one argument rsync splits as a shell does,
    /// asking nothing: through the control socket of Skwir's mount, the
    /// user's own otherwise.
    fn ssh(&self, mount: &Mount) -> String {
        let mut words = vec!["ssh".to_owned()];
        if let Some(config) = &self.ssh_config {
            words.extend(["-F".to_owned(), config.display().to_string()]);
        }
        for option in mount.ssh_options(&self.ssh_options) {
            words.extend(["-o".to_owned(), option]);
        }
        if let Some(port) = mount.port {
            words.extend(["-p".to_owned(), port.to_string()]);
        }
        words
            .iter()
            .map(|w| quoted(w))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What begins a line of rsync's naming a file it made (`--out-format`).
const PLACED: &str = "skwir-placed:";

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
        temp: Option<&Path>,
        told: &Told<'_>,
        control: &Control,
    ) -> anyhow::Result<()> {
        let (copied, placed) = (told.copied, told.placed);
        let rsync = program().ok_or_else(|| anyhow::anyhow!("rsync is not installed"))?;
        let (remote_side, local_side, upward) = match (self.mount_of(from), self.mount_of(into)) {
            (None, Some(mount)) => (mount, from, true),
            (Some(mount), None) => (mount, into, false),
            _ => anyhow::bail!("rsync copies between this machine and a server"),
        };
        let mount = remote_side;
        if !self.server_runs(&mount, control.cancel()) {
            anyhow::ensure!(!control.is_cancelled(), "cancelled");
            tracing::info!(point = %mount.point.display(), "the server runs no rsync: copies go another way");
            self.without.lock().insert(mount.point);
            return Err(NotServed.into());
        }
        let remote_path = if upward { into } else { from };
        let at = mount.on_server(remote_path).ok_or_else(|| {
            anyhow::anyhow!(
                "“{}” has a name rsync cannot be given",
                remote_path.display()
            )
        })?;
        let mut local = local_side.as_os_str().to_owned();
        local.push("/");
        let remote = mount.spec(&at);
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
            .arg(self.ssh(&mount));
        // Beside what is there: nothing replaced, a file written in the
        // staging folder until whole (on the destination's side).
        if let Some(temp) = temp {
            let temp = if upward {
                mount.on_server(temp).ok_or_else(|| {
                    anyhow::anyhow!("“{}” has a name rsync cannot be given", temp.display())
                })?
            } else {
                temp.to_str()
                    .ok_or_else(|| anyhow::anyhow!("“{}” is no text", temp.display()))?
                    .to_owned()
            };
            // Each file it makes said on a line of its own: those placed.
            command
                .arg("--ignore-existing")
                .arg(format!("--temp-dir={temp}"))
                .arg(format!("--out-format={PLACED}%n"));
        }
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
            control.cancel(),
            // Paused, rsync and its ssh are held where they are.
            &|| control.is_paused(),
            &mut |bytes: &[u8]| {
                for &b in bytes {
                    if b == b'\r' || b == b'\n' {
                        if let Some(name) = line.strip_prefix(PLACED.as_bytes()) {
                            placed(&skwir_core::os_path::from_bytes_ref(name));
                        } else if let Some(n) = copied_of(&line) {
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
        // The user's ssh, which asks nothing, did not reach the server.
        let unreached = mount.control.is_none() && finished.status.code() == Some(255);
        if missing || unreached {
            tracing::info!(point = %mount.point.display(), "rsync does not reach the server: copies go through the mount");
            self.without.lock().insert(mount.point);
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
    fn its_ssh_is_quoted_as_rsync_splits_it() {
        let ours = Mount {
            point: PathBuf::from("/run/user/1000/skwir/remote/x"),
            login: "me@example.com".to_owned(),
            port: Some(2222),
            root: "/".to_owned(),
            control: Some(PathBuf::from("/run/user/1000/skwir/ssh/0123")),
        };
        let rsync = Rsync::default();
        let ssh = rsync.ssh(&ours);
        assert!(ssh.starts_with("'ssh' "));
        assert!(ssh.contains("'ControlPath=/run/user/1000/skwir/ssh/0123'"));
        assert!(ssh.contains("'BatchMode=yes'"));
        assert!(ssh.ends_with("'-p' '2222'"));
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }
}
