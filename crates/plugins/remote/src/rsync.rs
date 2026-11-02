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
//!
//! It removes the originals of moves from such a mount too, at the plan's
//! end (`ssh_mount::remove_beside`): one rsync from an empty folder of
//! this machine's, each name a deletion asked (`--delete-missing-args`,
//! a folder whole with `--force`, a link as a link), nothing ever written
//! (`--existing --ignore-existing`). 100 files of 16 KiB moved down from a
//! server 20 ms away: their removal took 1.9 s through the mount.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;
use skwir_core::op::transfer::{Aside, Control, NotServed, Told, Transfer};

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
    /// The folders it removed moved originals in.
    removed: AtomicUsize,
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

    /// The folders it removed moved originals in (a test asks).
    pub fn removed(&self) -> usize {
        self.removed.load(Ordering::Relaxed)
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

    /// The ssh rsync runs, as one argument rsync splits with its own grammar,
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

/// An empty folder of this machine's, private, for rsync to send nothing
/// from: gone with it.
struct Empty(PathBuf);

impl Empty {
    fn new() -> std::io::Result<Empty> {
        let parent = skwir_core::xdg::runtime_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("skwir");
        std::fs::create_dir_all(&parent)?;
        let path = parent.join(crate::ssh_mount::unique("empty-"));
        std::fs::create_dir(&path)?;
        Ok(Empty(path))
    }
}

impl Drop for Empty {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// What begins a line of rsync's naming what it made, once whole
/// (`--out-format`, logged after each transfer as it names `%b`): the
/// bytes it moved, then the path.
const PLACED: &str = "skwir-placed:";

/// The path a line of rsync's names as made (`PLACED`), after its bytes.
fn made_of(line: &[u8]) -> Option<&[u8]> {
    let rest = line.strip_prefix(PLACED.as_bytes())?;
    let at = rest.iter().position(|&b| b == b':')?;
    rest[..at]
        .iter()
        .all(u8::is_ascii_digit)
        .then(|| &rest[at + 1..])
}

/// Of the paths rsync says it made in order, in a folder it copies whole
/// entries into, which entry each finished: a file of its own at once, a
/// folder once rsync goes on to another (it copies them one after
/// another, a folder's files after it).
#[derive(Default)]
struct Entries {
    open: Option<Vec<u8>>,
}

impl Entries {
    /// `path` was made: the entries finished by it.
    fn made(&mut self, path: &[u8]) -> Vec<Vec<u8>> {
        let mut finished = Vec::new();
        let (top, inside) = match path.iter().position(|&b| b == b'/') {
            Some(at) => (&path[..at], true),
            None => (path, false),
        };
        if self.open.as_deref().is_some_and(|open| open != top) {
            finished.extend(self.open.take());
        }
        if inside {
            self.open = Some(top.to_vec());
        } else {
            finished.push(top.to_vec());
        }
        finished
    }
}

/// `word` in single quotes, as rsync's own splitting of `-e` reads them:
/// a quote inside is doubled. Backslashes are literal, not shell escapes.
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', "''"))
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
/// `--human-readable`, once the percent after it says the word is whole.
/// rsync begins each such line with `\r`, which ends the one before: a
/// line is read as it comes, not when the next one begins a second later.
fn copied_of(line: &[u8]) -> Option<u64> {
    let line = std::str::from_utf8(line).ok()?;
    let mut words = line.split_whitespace();
    let word = words.next()?;
    let digits: String = word.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty()
        || !word
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
        || !words.next().is_some_and(|percent| percent.ends_with('%'))
    {
        return None;
    }
    digits.parse().ok()
}

impl Transfer for Rsync {
    fn name(&self) -> &'static str {
        "rsync"
    }

    /// An sshfs mount: it reads the folder from the server again.
    fn show(&self, folder: &Path) -> bool {
        let shown = Mount::of(folder).is_some();
        if shown {
            crate::ssh_mount::shown_again(folder);
        }
        shown
    }

    /// A mount whose server is not known to run no rsync.
    fn removes(&self, folder: &Path) -> bool {
        program().is_some()
            && self
                .mount_of(folder)
                .is_some_and(|mount| self.runs.lock().get(&mount.point) != Some(&false))
    }

    fn remove(&self, folder: &Path, names: &[OsString], control: &Control) -> anyhow::Result<()> {
        let Some((rsync, mount)) = program().zip(self.mount_of(folder)) else {
            return Err(NotServed.into());
        };
        if !self.server_runs(&mount, control.cancel()) {
            anyhow::ensure!(!control.is_cancelled(), "cancelled");
            return Err(NotServed.into());
        }
        let at = mount.on_server(folder).ok_or_else(|| {
            anyhow::anyhow!("“{}” has a name rsync cannot be given", folder.display())
        })?;
        crate::ssh_mount::remove_beside(folder, names, |beside| {
            // Each file's name, and the folder of Skwir's whole.
            let mut list = Vec::new();
            let trees = (!beside.trees.is_empty()).then_some(beside.scratch);
            for path in beside.files.iter().map(PathBuf::as_path).chain(trees) {
                list.extend_from_slice(path.file_name().unwrap_or_default().as_encoded_bytes());
                list.push(0);
            }
            let empty = Empty::new()?;
            let mut from = empty.0.as_os_str().to_owned();
            from.push("/");
            let mut command = Command::new(rsync);
            command
                .env("LC_ALL", "C")
                .args([
                    "--recursive",
                    "--existing",
                    "--ignore-existing",
                    "--delete-missing-args",
                    "--force",
                    "--protect-args",
                    "--files-from=-",
                    "--from0",
                    "-e",
                ])
                .arg(self.ssh(&mount))
                .arg(&from)
                .arg(mount.spec(&at));
            let finished = skwir_platform::process::run_watching(
                &mut command,
                Some(&list),
                control.cancel(),
                &|| control.is_paused(),
                &mut |_: &[u8]| {},
            )?;
            anyhow::ensure!(
                finished.status.success(),
                "rsync failed ({}): {}",
                finished.status,
                finished.stderr
            );
            Ok(())
        })?;
        self.removed.fetch_add(1, Ordering::Relaxed);
        Ok(())
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
        aside: &Aside<'_>,
        told: &Told<'_>,
        control: &Control,
    ) -> anyhow::Result<()> {
        let (copied, placed, made, temp) = (told.copied, told.placed, told.made, aside.temp);
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
                // Names as they are, not escaped past ASCII: what it made
                // is told by its name.
                "--8-bit-output",
                "--files-from=-",
                "--from0",
                "-e",
            ])
            .arg(self.ssh(&mount));
        // A folder of the staging folder, as the receiving side names it.
        let receiving = |path: &Path| -> anyhow::Result<String> {
            let said = if upward {
                mount.on_server(path)
            } else {
                path.to_str().map(str::to_owned)
            };
            said.ok_or_else(|| {
                anyhow::anyhow!("“{}” has a name rsync cannot be given", path.display())
            })
        };
        // A file it was writing when cut short is kept there, and gone on
        // from by its retry (ADR-080): the basis of its delta copy, whose
        // checksums say what is the same, the file made checked whole.
        // Absolute only: rsync takes a relative one as a folder in each
        // file's own (a server's path below its login's home is one).
        let partial = receiving(aside.partial)?;
        if partial.starts_with('/') {
            command.arg(format!("--partial-dir={partial}"));
        }
        // Each thing it made said on a line of its own once whole: those
        // placed, the entries made.
        command.arg(format!("--out-format={PLACED}%b:%n"));
        // Beside what is there: nothing replaced, a file written in the
        // staging folder until whole (on the destination's side).
        if let Some(temp) = temp {
            command
                .arg("--ignore-existing")
                .arg(format!("--temp-dir={}", receiving(temp)?));
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
        let mut entries = Entries::default();
        let finished = skwir_platform::process::run_watching(
            &mut command,
            Some(&list),
            control.cancel(),
            // Paused, rsync and its ssh are held where they are.
            &|| control.is_paused(),
            &mut |bytes: &[u8]| {
                for &b in bytes {
                    if b == b'\r' || b == b'\n' {
                        if let Some(path) = made_of(&line) {
                            // Beside what is there, files alone: each
                            // placed. In a folder of its own, the entries.
                            if temp.is_some() {
                                placed(&skwir_core::os_path::from_bytes_ref(path));
                            } else {
                                for entry in entries.made(path) {
                                    made(&skwir_core::os_path::from_bytes_ref(&entry));
                                }
                            }
                        } else if let Some(n) = copied_of(&line) {
                            copied(n);
                        }
                        line.clear();
                    } else {
                        line.push(b);
                    }
                }
                // A progress line ends only as the next begins: read as it
                // stands, its count whole once its percent follows (what
                // it said again at its end counts no more).
                if let Some(n) = copied_of(&line) {
                    copied(n);
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
    fn what_rsync_made_is_told_by_its_entry_once_whole() {
        assert_eq!(
            made_of(b"skwir-placed:41948199:it's a: name.bin"),
            Some(&b"it's a: name.bin"[..])
        );
        assert_eq!(made_of(b"skwir-placed:x:y"), None);
        assert_eq!(made_of(b"      1048576  45%"), None);
        // Files of their own at once; a folder once another begins.
        let mut entries = Entries::default();
        let mut made = |path: &[u8]| entries.made(path);
        assert_eq!(made(b"a.bin"), vec![b"a.bin".to_vec()]);
        assert!(made(b"tree/").is_empty());
        assert!(made(b"tree/y.bin").is_empty());
        assert!(made(b"tree/sub/").is_empty());
        assert_eq!(made(b"z.bin"), vec![b"tree".to_vec(), b"z.bin".to_vec()]);
        assert!(made(b"other/x").is_empty());
        assert_eq!(made(b"last/x"), vec![b"other".to_vec()]);
    }

    #[test]
    fn rsync_says_how_far_it_went_in_digits() {
        assert_eq!(
            copied_of(b"      1048576  45%  1.23MB/s  0:00:01 (xfr#3, to-chk=7/10)"),
            Some(1_048_576)
        );
        assert_eq!(copied_of(b"1,234,567  12%"), Some(1_234_567));
        assert_eq!(copied_of(b"sending incremental file list"), None);
        assert_eq!(copied_of(b""), None);
        // A line come in part: its count once whole, never a part of it.
        assert_eq!(copied_of(b"      10485"), None);
        assert_eq!(copied_of(b"      1048576  4"), None);
        assert_eq!(copied_of(b"      1048576  45%"), Some(1_048_576));
    }

    #[test]
    fn its_ssh_is_quoted_as_rsync_splits_it() {
        let ours = Mount {
            point: PathBuf::from("/run/user/1000/skwir/remote/x"),
            login: "me@example.com".to_owned(),
            port: Some(2222),
            root: "/".to_owned(),
            control: Some(PathBuf::from("/run/user/1000/skwir/ssh/0123")),
            links_shown: true,
        };
        let rsync = Rsync::default();
        let ssh = rsync.ssh(&ours);
        assert!(ssh.starts_with("'ssh' "));
        assert!(ssh.contains("'ControlPath=/run/user/1000/skwir/ssh/0123'"));
        assert!(ssh.contains("'BatchMode=yes'"));
        assert!(ssh.ends_with("'-p' '2222'"));
    }

    /// Exercise rsync's parser, not another implementation of its grammar.
    /// The transport only records arguments: no server is contacted.
    #[cfg(unix)]
    #[test]
    fn rsync_passes_ssh_arguments_with_quotes_and_whitespace_unchanged() {
        use std::os::unix::fs::PermissionsExt;

        let Some(rsync) = program() else {
            eprintln!("rsync is not installed: its argument parser was not exercised");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let ssh = tmp.path().join("ssh");
        std::fs::write(
            &ssh,
            "#!/bin/sh\nprintf '%s\\0' \"$@\" > \"$SKWIR_TEST_SSH_ARGS\"\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
        let captured = tmp.path().join("arguments");
        let config = tmp.path().join("it's a café \"config\"\\file");
        let control = tmp.path().join("it's a café \"socket\"\\file");
        let ours = Mount {
            point: tmp.path().join("mount"),
            login: "example.test".to_owned(),
            port: Some(2222),
            root: "/".to_owned(),
            control: Some(control.clone()),
            links_shown: true,
        };
        let transfer = Rsync::with(Vec::new(), Some(config.clone()));
        let mut paths = vec![tmp.path().to_owned()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let mut command = Command::new(rsync);
        command
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("SKWIR_TEST_SSH_ARGS", &captured)
            .arg("-e")
            .arg(transfer.ssh(&ours))
            .arg(tmp.path().join("source"))
            .arg(ours.spec("/target"));
        let finished = skwir_platform::process::run_to_exit(
            &mut command,
            None,
            &skwir_core::CancelToken::new(),
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert!(!finished.status.success(), "the transport offers no server");
        let received = std::fs::read(&captured)
            .unwrap_or_else(|error| panic!("ssh was not invoked: {error}; {}", finished.stderr));
        let received: Vec<&[u8]> = received
            .strip_suffix(&[0])
            .expect("the transport terminates each argument")
            .split(|&b| b == 0)
            .collect();
        let expected = [
            OsString::from("-F"),
            config.into_os_string(),
            OsString::from("-o"),
            OsString::from(format!("ControlPath={}", control.display())),
            OsString::from("-o"),
            OsString::from("ControlMaster=no"),
            OsString::from("-o"),
            OsString::from("BatchMode=yes"),
            OsString::from("-p"),
            OsString::from("2222"),
            OsString::from("example.test"),
        ];
        let expected: Vec<&[u8]> = expected.iter().map(|arg| arg.as_encoded_bytes()).collect();
        assert!(received.starts_with(&expected), "{received:?}");
    }
}
