//! sftp, for a batch of copies onto an sshfs mount whose server runs no
//! rsync (ADR-078): an SFTP-only account (chrooted, a NAS's) has none to
//! run. OpenSSH's own client in batch mode, eight sessions at once on the
//! same connection, each asking for a file's parts without waiting for
//! each answer, where through the mount the kernel makes a folder's names
//! one at a time under its lock. 100 files of 16 KiB onto an SFTP-only
//! server 20 ms away: 2.99 s, where through the mount 8.4. Not down:
//! there the names are made on this machine, and Skwir's eight copies
//! through the mount took 0.6–0.75 s where the sessions, each a file after
//! another, took 2.06.
//!
//! It reaches the server as `ssh_mount` says (through the connection
//! sshfs holds for Skwir's mount, the user's own ssh for theirs), asking
//! nothing. A path goes in double quotes, `"` and `\` escaped: sftp
//! escapes the glob characters within quotes itself. `put -p` and `get
//! -p` keep modes and times. Nothing is replaced: a file placed beside
//! what is there is written in the staging folder, then linked to its
//! name (`ln`, which the server refuses where the name is taken) and its
//! first name removed; a link is made where it goes (`symlink`, refused
//! there too). Folders are made through the mount first and finished last
//! (`transfer::Layout`): `put -R` passes links over. A name with a line
//! break cannot be said in a batch: a batch holding one is left to its
//! steps.
//!
//! Within one server (both folders on one sshfs mount), a file is copied
//! there by `cp` (the copy-data extension, OpenSSH's server from 9.0,
//! asked once a mount), its bytes never leaving the server; `cp` keeps no
//! time, so each is given its source's through the mount, once its folder
//! was read (sshfs looks up a name it does not know one at a time); a
//! link is made again by `symlink`. 100 files of 16 KiB within a server
//! 20 ms away: 4.2 s, where through the mount 8.9; a file of 64 MiB, 1.4
//! to 1.8 s (2.6 to 5.0), whatever its size.
//!
//! It removes the originals of moves from an sshfs mount whose server
//! runs no rsync too, at the plan's end (`ssh_mount::remove_beside`): `rm`
//! for each file, a folder's tree read through the mount and removed by
//! `rm` and `rmdir`, the deepest first. Only where the mount shows a link
//! as a link: one that follows links shows a folder's link as the folder,
//! and the tree below it is not the original's.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use parking_lot::Mutex;
use skwir_core::op::transfer::{Control, Layout, NotServed, Told, Transfer};

use crate::ssh_mount::Mount;

/// How many sessions run at once.
const AT_ONCE: usize = 8;

/// The most files a session of Skwir's mount is given: a session there is
/// a channel of a connection held, more of them tell the progress more
/// often. One the user made logs in for each, so it has [`AT_ONCE`].
const MOST: usize = 32;

/// sftp through an sshfs mount's server.
#[derive(Default)]
pub struct Sftp {
    /// ssh options given beside the plugin's (a test's), as sshfs has them.
    pub ssh_options: Vec<String>,
    /// An ssh configuration file in place of the user's (a test's).
    pub ssh_config: Option<PathBuf>,
    /// The mount points whose server sftp does not reach without asking.
    without: Mutex<HashSet<PathBuf>>,
    /// Whether each mount's server opens a session so, asked once.
    opens: Mutex<HashMap<PathBuf, bool>>,
    /// Whether each mount's server copies a file itself (`cp`), asked once.
    copies_there: Mutex<HashMap<PathBuf, bool>>,
    /// The batches it copied.
    made: AtomicUsize,
    /// The folders it removed moved originals in.
    removed: AtomicUsize,
}

impl Sftp {
    /// sftp with ssh given `ssh_options` and `ssh_config` beside the
    /// plugin's, as sshfs's is.
    pub fn with(ssh_options: Vec<String>, ssh_config: Option<PathBuf>) -> Self {
        Sftp {
            ssh_options,
            ssh_config,
            ..Sftp::default()
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

    /// The sshfs mount that holds `path`, when sftp reaches its server.
    fn mount_of(&self, path: &Path) -> Option<Mount> {
        Mount::of(path).filter(|mount| !self.without.lock().contains(&mount.point))
    }

    /// A session reading its commands on its input, asking nothing.
    fn session(&self, sftp: &Path, mount: &Mount) -> Command {
        let mut command = Command::new(sftp);
        command.env("LC_ALL", "C").args(["-b", "-"]);
        if let Some(config) = &self.ssh_config {
            command.arg("-F").arg(config);
        }
        for option in mount.ssh_options(&self.ssh_options) {
            command.arg("-o").arg(option);
        }
        if let Some(port) = mount.port {
            command.arg("-P").arg(port.to_string());
        }
        command.arg(&mount.login);
        command
    }

    /// Each file's lines run in sessions at once, each told its bytes once
    /// ended: as many sessions as a mount's connection takes.
    fn run(
        &self,
        sftp: &Path,
        mount: &Mount,
        files: Vec<(Vec<String>, u64)>,
        ran: &(dyn Fn(u64) + Sync),
        control: &Control,
    ) -> anyhow::Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        // A session of Skwir's mount is a channel of its connection; the
        // user's logs in for each.
        let sessions = if mount.control.is_some() {
            files.len().div_ceil(MOST).max(AT_ONCE)
        } else {
            AT_ONCE
        };
        let runs = spread(files, sessions)
            .into_iter()
            .map(|(batch, bytes)| crate::runs::Run {
                command: self.session(sftp, mount),
                input: Some(batch.into_bytes()),
                bytes,
            })
            .collect();
        crate::runs::at_once("sftp", runs, AT_ONCE, ran, control)
    }
}

impl Sftp {
    /// Whether the server of `mount` opens a session asking nothing: one
    /// that only says where it is (`pwd`), once a mount.
    fn opens(&self, sftp: &Path, mount: &Mount, cancel: &skwir_core::cancel::CancelToken) -> bool {
        if let Some(&known) = self.opens.lock().get(&mount.point) {
            return known;
        }
        let opened = skwir_platform::process::run_to_exit(
            &mut self.session(sftp, mount),
            Some(b"@pwd\n"),
            cancel,
            std::time::Duration::from_secs(30),
        )
        .is_ok_and(|finished| finished.status.success());
        self.opens.lock().insert(mount.point.clone(), opened);
        opened
    }

    /// Whether the server of `mount` copies a file itself (`cp`: the
    /// copy-data extension, OpenSSH's server from 9.0), asked once: a copy
    /// of a name that is not there, which such a server refuses as not
    /// there, any other as unsupported.
    fn copies_there(
        &self,
        sftp: &Path,
        mount: &Mount,
        cancel: &skwir_core::cancel::CancelToken,
    ) -> bool {
        if let Some(&known) = self.copies_there.lock().get(&mount.point) {
            return known;
        }
        let nothing = mount
            .on_server(&mount.point.join(".skwir-nothing-here"))
            .as_deref()
            .and_then(quoted);
        let copies = nothing.is_some_and(|nothing| {
            skwir_platform::process::run_to_exit(
                &mut self.session(sftp, mount),
                Some(format!("-cp {nothing} {nothing}\n").as_bytes()),
                cancel,
                std::time::Duration::from_secs(30),
            )
            .is_ok_and(|finished| {
                finished.status.success() && !finished.stderr.contains("does not support")
            })
        });
        self.copies_there.lock().insert(mount.point.clone(), copies);
        copies
    }
}

/// The local sftp, found once.
fn program() -> Option<&'static PathBuf> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| skwir_core::os_path::find_program("sftp"))
        .as_ref()
}

/// `path` as a batch says it: in double quotes, `"` and `\` escaped;
/// `None` for one a line cannot hold.
fn quoted(path: &str) -> Option<String> {
    if path.contains(['\n', '\r']) {
        return None;
    }
    let mut said = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '\\') {
            said.push('\\');
        }
        said.push(c);
    }
    said.push('"');
    Some(said)
}

/// A local path as a batch says it.
fn local(path: &Path) -> Option<String> {
    quoted(path.to_str()?)
}

/// Each file's lines, with its bytes, spread over `sessions` sessions'
/// batches, a file's lines together.
fn spread(files: Vec<(Vec<String>, u64)>, sessions: usize) -> Vec<(String, u64)> {
    let mut batches = vec![(String::new(), 0u64); sessions.clamp(1, files.len().max(1))];
    let count = batches.len();
    for (n, (lines, bytes)) in files.into_iter().enumerate() {
        let (batch, total) = &mut batches[n % count];
        for line in lines {
            batch.push_str(&line);
            batch.push('\n');
        }
        *total += bytes;
    }
    batches.retain(|(batch, _)| !batch.is_empty());
    batches
}

impl Sftp {
    /// Copy what `layout` lays out within the server of `mount` (module
    /// documentation): each file by `cp` there, its bytes never leaving
    /// the server, then given its source's time through the mount, which
    /// `cp` does not keep; a link made again; beside what is there, each
    /// made in `temp`, given its time there, then linked to its name.
    fn copy_within(
        &self,
        sftp: &Path,
        mount: &Mount,
        layout: &Layout,
        temp: Option<&Path>,
        told: &Told<'_>,
        control: &Control,
    ) -> anyhow::Result<()> {
        let unsaid = |path: &Path| anyhow::anyhow!("“{}” cannot be said to sftp", path.display());
        let server = |path: &Path| {
            mount
                .on_server(path)
                .as_deref()
                .and_then(quoted)
                .ok_or_else(|| unsaid(path))
        };
        let mut copies: Vec<(Vec<String>, u64)> = Vec::new();
        let mut placing: Vec<(Vec<String>, u64)> = Vec::new();
        let mut timed: Vec<(PathBuf, PathBuf)> = Vec::new();
        for (src, dst) in &layout.files {
            let meta = std::fs::symlink_metadata(src)?;
            if meta.is_symlink() {
                let target = std::fs::read_link(src)?;
                let target = target
                    .to_str()
                    .and_then(quoted)
                    .ok_or_else(|| unsaid(&target))?;
                copies.push((vec![format!("-@symlink {target} {}", server(dst)?)], 0));
                continue;
            }
            let made = match temp {
                Some(temp) => temp.join(dst.file_name().unwrap_or_default()),
                None => dst.clone(),
            };
            copies.push((
                vec![format!("@cp {} {}", server(src)?, server(&made)?)],
                meta.len(),
            ));
            if temp.is_some() {
                placing.push((
                    vec![
                        format!("-@ln {} {}", server(&made)?, server(dst)?),
                        format!("-@rm {}", server(&made)?),
                    ],
                    0,
                ));
            }
            timed.push((src.clone(), made));
        }
        let total = AtomicU64::new(0);
        let ran = |bytes: u64| (told.copied)(total.fetch_add(bytes, Ordering::Relaxed) + bytes);
        self.run(sftp, mount, copies, &ran, control)?;
        // Its time given before it takes its name: one taken meanwhile is
        // another's, never touched. Each folder read once first: sshfs
        // looks up a name it does not know one at a time, a round trip
        // each, where a folder's listing tells it all its names.
        let folders: std::collections::BTreeSet<&Path> =
            timed.iter().filter_map(|(_, made)| made.parent()).collect();
        for folder in folders {
            let _ = std::fs::read_dir(folder).map(Iterator::count);
        }
        skwir_core::op::transfer::give_times(&timed)?;
        self.run(sftp, mount, placing, &|_| {}, control)?;
        layout.finish()?;
        self.made.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

impl Transfer for Sftp {
    fn name(&self) -> &'static str {
        "sftp"
    }

    /// An sshfs mount: it reads the folder from the server again.
    fn show(&self, folder: &Path) -> bool {
        let shown = Mount::of(folder).is_some();
        if shown {
            crate::ssh_mount::shown_again(folder);
        }
        shown
    }

    /// A mount sftp reaches whose links show as links (module
    /// documentation).
    fn removes(&self, folder: &Path) -> bool {
        program().is_some() && self.mount_of(folder).is_some_and(|mount| mount.links_shown)
    }

    fn remove(&self, folder: &Path, names: &[OsString], control: &Control) -> anyhow::Result<()> {
        let Some((sftp, mount)) =
            program().zip(self.mount_of(folder).filter(|mount| mount.links_shown))
        else {
            return Err(NotServed.into());
        };
        if !self.opens(sftp, &mount, control.cancel()) {
            anyhow::ensure!(!control.is_cancelled(), "cancelled");
            self.without.lock().insert(mount.point);
            return Err(NotServed.into());
        }
        let server = |path: &Path| {
            mount
                .on_server(path)
                .as_deref()
                .and_then(quoted)
                .ok_or_else(|| anyhow::anyhow!("“{}” cannot be said to sftp", path.display()))
        };
        let lines = |verb: &str, paths: &[PathBuf]| -> anyhow::Result<Vec<(Vec<String>, u64)>> {
            paths
                .iter()
                .map(|path| Ok((vec![format!("-@{verb} {}", server(path)?)], 0)))
                .collect()
        };
        crate::ssh_mount::remove_beside(folder, names, |beside| {
            let (mut leaves, mut folders) = (beside.files.to_vec(), Vec::new());
            for tree in beside.trees {
                crate::runs::tree(tree, 0, &mut leaves, &mut folders)?;
            }
            self.run(sftp, &mount, lines("rm", &leaves)?, &|_| {}, control)?;
            // `rmdir` takes an empty folder alone: the deepest first.
            folders.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
            for level in folders.chunk_by(|a, b| a.0 == b.0) {
                let paths: Vec<PathBuf> = level.iter().map(|(_, path)| path.clone()).collect();
                self.run(sftp, &mount, lines("rmdir", &paths)?, &|_| {}, control)?;
            }
            Ok(())
        })?;
        self.removed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Onto a mount alone (module documentation).
    fn serves(&self, from: &Path, into: &Path) -> bool {
        let Some(mount) = program().and_then(|_| self.mount_of(into)) else {
            return false;
        };
        match skwir_core::fs::reach::of(from) {
            None => true,
            // Within its server: copied there (`cp`), if it copies.
            Some(_) => Mount::of(from).is_some_and(|m| m.point == mount.point),
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
        let sftp = program().ok_or_else(|| anyhow::anyhow!("sftp is not installed"))?;
        let Some(mount) = self.mount_of(into) else {
            anyhow::bail!("sftp copies onto a server");
        };
        let within = skwir_core::fs::reach::of(from).is_some();
        anyhow::ensure!(
            !within || Mount::of(from).is_some_and(|m| m.point == mount.point),
            "sftp copies from this machine onto a server, or within one"
        );
        if !self.opens(sftp, &mount, control.cancel()) {
            anyhow::ensure!(!control.is_cancelled(), "cancelled");
            tracing::info!(point = %mount.point.display(), "sftp does not reach the server: copies go another way");
            self.without.lock().insert(mount.point);
            return Err(NotServed.into());
        }
        if within && !self.copies_there(sftp, &mount, control.cancel()) {
            anyhow::ensure!(!control.is_cancelled(), "cancelled");
            return Err(NotServed.into());
        }
        let unsaid = |path: &Path| anyhow::anyhow!("“{}” cannot be said to sftp", path.display());
        let server = |path: &Path| {
            mount
                .on_server(path)
                .as_deref()
                .and_then(quoted)
                .ok_or_else(|| unsaid(path))
        };
        let here = |path: &Path| local(path).ok_or_else(|| unsaid(path));
        let layout = match temp {
            Some(_) => Layout::files(from, names, into),
            None => Layout::made(from, names, into)?,
        };
        if within {
            return self.copy_within(sftp, &mount, &layout, temp, told, control);
        }
        // Each file's lines, kept together in one session: what was placed
        // beside is known by its size and time, which `put -p` keeps.
        let mut files: Vec<(Vec<String>, u64)> = Vec::new();
        for (src, dst) in &layout.files {
            let meta = std::fs::symlink_metadata(src)?;
            if meta.is_symlink() {
                let target = std::fs::read_link(src)?;
                let line = format!("-@symlink {} {}", here(&target)?, server(dst)?);
                files.push((vec![line], 0));
                continue;
            }
            let Some(temp) = temp else {
                files.push((
                    vec![format!("@put -p {} {}", here(src)?, server(dst)?)],
                    meta.len(),
                ));
                continue;
            };
            let made = server(&temp.join(dst.file_name().unwrap_or_default()))?;
            files.push((
                vec![
                    format!("@put -p {} {made}", here(src)?),
                    format!("-@ln {made} {}", server(dst)?),
                    format!("-@rm {made}"),
                ],
                meta.len(),
            ));
        }
        let total = AtomicU64::new(0);
        let ran = |bytes: u64| (told.copied)(total.fetch_add(bytes, Ordering::Relaxed) + bytes);
        if let Err(error) = self.run(sftp, &mount, files, &ran, control) {
            // ssh's own failure (255) for the user's ssh, which asks
            // nothing: the server is not reached so.
            let unreached = error
                .downcast_ref::<crate::runs::Failed>()
                .is_some_and(|f| {
                    (mount.control.is_none() && f.status.code() == Some(255))
                        || f.said.contains("subsystem request failed")
                });
            if unreached {
                tracing::info!(point = %mount.point.display(), "sftp does not reach the server: copies go through the mount");
                self.without.lock().insert(mount.point.clone());
            }
            return Err(error);
        }
        layout.finish()?;
        self.made.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_said_in_quotes_and_a_line_break_not_at_all() {
        assert_eq!(
            quoted(r#"it's a "b"\c*?[d]"#).unwrap(),
            r#""it's a \"b\"\\c*?[d]""#
        );
        assert_eq!(quoted("two\nlines"), None);
        let batches = spread((0..10).map(|n| (vec![format!("put {n}")], 1)).collect(), 4);
        assert_eq!(batches.len(), 4);
        assert_eq!(batches.iter().map(|(_, bytes)| bytes).sum::<u64>(), 10);
        assert!(batches[0].0.starts_with("put 0\nput 4\nput 8\n"));
        assert_eq!(spread(vec![(vec!["put a".to_owned()], 1)], 8).len(), 1);
    }
}
