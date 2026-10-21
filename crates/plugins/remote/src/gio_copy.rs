//! gio, for a batch of copies to or from an SFTP folder of GVfs
//! (ADR-078): eight `gio copy` at once, which GVfs's daemon serves
//! together, where through its FUSE bridge the kernel makes the names of
//! a folder one at a time. 100 files of 16 KiB onto a server 20 ms away:
//! 11.6 s through the bridge, eight at a time; 9.0 s by one `gio copy`;
//! 1.2 s by eight (2026-10-21).
//!
//! The same through GVfs's other network kinds (2026-10-21, the same
//! way): WebDAV 25.6–26.1 s through the bridge, 1.5 by eight `gio`; FTP
//! 139.8 s, 9.2–9.4; SMB (Samba) 28.1–28.2 s, 15.4: its backend takes a
//! request at a time, the bridge's own cost is spared still. FTPS and
//! WebDAV over TLS are the same backends. Others (NFS, AFP; a phone's MTP,
//! whose one session takes a file at a time) keep the bridge, not
//! measured.
//!
//! `gio` maps a path of the bridge to its mount's address itself. With
//! `-i` and nothing on its input it never replaces a file: without it,
//! `gio copy` and `gio move` do. Folders, which `gio copy` does not copy,
//! are made through the bridge first and given their times last
//! (`transfer::Layout`). A server that refuses some files at once (an FTP
//! server's few data ports) has what is missing copied again, by one
//! `gio`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use skwir_core::op::transfer::{Control, Layout, NotServed, Told, Transfer};

use crate::gvfs_spec::Spec;

/// How many `gio` run at once: eight copied a hundred small files in an
/// eighth of the time one did (2026-10-21).
const AT_ONCE: usize = 8;

/// The most names one `gio` is given.
const MOST: usize = 64;

/// `gio copy` and `gio move` through this session's GVfs.
#[derive(Default)]
pub struct GioCopy {
    /// Variables `gio` runs with beside skwir's own (`Gvfs::env`).
    env: Vec<(OsString, OsString)>,
    /// The runtime folder of the bridge, when not the user's (`Gvfs::runtime`).
    runtime: Option<PathBuf>,
    /// The batches it copied.
    made: AtomicUsize,
    /// The batches of moved originals it removed.
    removed: AtomicUsize,
}

impl GioCopy {
    pub fn new(env: Vec<(OsString, OsString)>, runtime: Option<PathBuf>) -> Self {
        GioCopy {
            env,
            runtime,
            made: AtomicUsize::new(0),
            removed: AtomicUsize::new(0),
        }
    }

    /// The batches of moved originals it removed (a test asks).
    pub fn removed(&self) -> usize {
        self.removed.load(Ordering::Relaxed)
    }

    /// The batches it copied (a test asks).
    pub fn made(&self) -> usize {
        self.made.load(Ordering::Relaxed)
    }

    /// Whether `path` is in a mount of this session's GVfs bridge of a kind
    /// served (SFTP, FTP, WebDAV, SMB): read from the bridge folder's name,
    /// never from a server.
    fn on_sftp(&self, path: &Path) -> bool {
        let Some(bridge) = crate::gvfs::bridge_in(self.runtime.clone()) else {
            return false;
        };
        path.strip_prefix(&bridge)
            .ok()
            .and_then(|below| below.components().next())
            .and_then(|mount| mount.as_os_str().to_str().and_then(Spec::parse))
            .is_some_and(|spec| {
                matches!(
                    spec.kind.as_str(),
                    "sftp" | "ftp" | "ftps" | "dav" | "smb-share"
                )
            })
    }

    /// `gio` with `words`, then `names`, then `into` as a folder.
    fn gio(&self, gio: &Path, words: &[&str], names: &[&Path], into: &Path) -> Command {
        let mut command = self.gio_on(gio, words, names);
        let mut folder = into.as_os_str().to_owned();
        folder.push("/");
        command.arg(folder);
        command
    }

    /// `gio` with `words`, then `names`.
    fn gio_on(&self, gio: &Path, words: &[&str], names: &[&Path]) -> Command {
        let mut command = Command::new(gio);
        crate::gvfs::english(&mut command);
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        command.args(words).arg("--").args(names);
        command
    }

    /// Run `gio` on each of `runs` (names going into a folder),
    /// [`AT_ONCE`] at a time, telling `ran` of each one ended well.
    fn each(
        &self,
        words: &[&str],
        runs: &[(PathBuf, Vec<(PathBuf, u64)>)],
        ran: &(dyn Fn(u64) + Sync),
        control: &Control,
    ) -> anyhow::Result<()> {
        self.each_at(words, runs, AT_ONCE, ran, control)
    }

    /// [`GioCopy::each`], `at_once` at a time.
    fn each_at(
        &self,
        words: &[&str],
        runs: &[(PathBuf, Vec<(PathBuf, u64)>)],
        at_once: usize,
        ran: &(dyn Fn(u64) + Sync),
        control: &Control,
    ) -> anyhow::Result<()> {
        let gio = program().ok_or_else(|| anyhow::anyhow!("gio (GLib) is not installed"))?;
        let runs = runs
            .iter()
            .map(|(into, names)| {
                let paths: Vec<&Path> = names.iter().map(|(p, _)| p.as_path()).collect();
                crate::runs::Run {
                    command: self.gio(gio, words, &paths, into),
                    input: None,
                    bytes: names.iter().map(|(_, bytes)| bytes).sum(),
                }
            })
            .collect();
        crate::runs::at_once("gio", runs, at_once, ran, control)
    }
}

/// What copies with `gio`: never over anything (`-i`, nothing on its
/// input), modes and times kept, links as links.
const COPY: &[&str] = &["copy", "-i", "--preserve", "--no-dereference"];

/// `gio`, found once.
fn program() -> Option<&'static PathBuf> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| skwir_core::os_path::find_program("gio"))
        .as_ref()
}

/// `files` (each with its bytes) grouped by the folder each goes into,
/// in runs of at most [`MOST`], about as many as [`AT_ONCE`] runs hold.
fn runs(files: Vec<(PathBuf, PathBuf, u64)>) -> Vec<(PathBuf, Vec<(PathBuf, u64)>)> {
    let size = files.len().div_ceil(AT_ONCE).clamp(1, MOST);
    let mut by_folder: BTreeMap<PathBuf, Vec<(PathBuf, u64)>> = BTreeMap::new();
    for (src, into, bytes) in files {
        by_folder.entry(into).or_default().push((src, bytes));
    }
    let mut runs = Vec::new();
    for (into, names) in by_folder {
        for run in names.chunks(size) {
            runs.push((into.clone(), run.to_vec()));
        }
    }
    runs
}

impl Transfer for GioCopy {
    fn name(&self) -> &'static str {
        "gio"
    }

    /// Its daemon serves the bridge too: what `gio remove` removes, the
    /// bridge says gone at once.
    fn removes(&self, folder: &Path) -> bool {
        program().is_some() && self.on_sftp(folder)
    }

    fn remove(&self, folder: &Path, names: &[OsString], control: &Control) -> anyhow::Result<()> {
        let Some(gio) = program().filter(|_| self.on_sftp(folder)) else {
            return Err(NotServed.into());
        };
        let (mut files, mut folders) = (Vec::new(), Vec::new());
        for name in names {
            crate::runs::tree(&folder.join(name), 0, &mut files, &mut folders)?;
        }
        let runs = |paths: &[PathBuf]| -> Vec<crate::runs::Run> {
            paths
                .chunks(paths.len().div_ceil(AT_ONCE).clamp(1, MOST))
                .map(|chunk| {
                    let chunk: Vec<&Path> = chunk.iter().map(PathBuf::as_path).collect();
                    crate::runs::Run {
                        command: self.gio_on(gio, &["remove"], &chunk),
                        input: None,
                        bytes: 0,
                    }
                })
                .collect()
        };
        crate::runs::at_once("gio", runs(&files), AT_ONCE, &|_| {}, control)?;
        // `gio remove` takes an empty folder alone: the deepest first.
        folders.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
        for level in folders.chunk_by(|a, b| a.0 == b.0) {
            let paths: Vec<PathBuf> = level.iter().map(|(_, path)| path.clone()).collect();
            crate::runs::at_once("gio", runs(&paths), AT_ONCE, &|_| {}, control)?;
        }
        self.removed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn serves(&self, from: &Path, into: &Path) -> bool {
        program().is_some()
            && match (self.on_sftp(from), self.on_sftp(into)) {
                (true, false) => skwir_core::fs::reach::of(into).is_none(),
                (false, true) => skwir_core::fs::reach::of(from).is_none(),
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
        let layout = match temp {
            Some(_) => Layout::files(from, names, into),
            None => Layout::made(from, names, into)?,
        };
        // Beside what is there, each is copied into `temp` first.
        let files: Vec<(PathBuf, PathBuf, u64)> = layout
            .files
            .iter()
            .map(|(src, dst)| {
                let folder = temp.map_or_else(
                    || dst.parent().unwrap_or(into).to_path_buf(),
                    Path::to_path_buf,
                );
                let bytes = std::fs::symlink_metadata(src).map_or(0, |m| m.len());
                (src.clone(), folder, bytes)
            })
            .collect();
        let total = AtomicU64::new(0);
        let ran = |bytes: u64| copied(total.fetch_add(bytes, Ordering::Relaxed) + bytes);
        if let Err(error) = self.each(COPY, &runs(files.clone()), &ran, control) {
            if control.is_cancelled() || !error.is::<crate::runs::Failed>() {
                return Err(error);
            }
            // A server that refused some at once: what is missing, by one
            // `gio` a folder.
            let missing: Vec<(PathBuf, PathBuf, u64)> = files
                .into_iter()
                .filter(|(src, folder, _)| {
                    src.file_name()
                        .is_none_or(|name| std::fs::symlink_metadata(folder.join(name)).is_err())
                })
                .collect();
            tracing::debug!(
                missing = missing.len(),
                "gio copies again what was refused: {error:#}"
            );
            let mut by_folder: BTreeMap<PathBuf, Vec<(PathBuf, u64)>> = BTreeMap::new();
            for (src, folder, bytes) in missing {
                by_folder.entry(folder).or_default().push((src, bytes));
            }
            let again: Vec<(PathBuf, Vec<(PathBuf, u64)>)> = by_folder.into_iter().collect();
            self.each_at(COPY, &again, 1, &ran, control)?;
        }
        layout.finish()?;
        if let Some(temp) = temp {
            let made: Vec<(PathBuf, PathBuf)> = layout
                .files
                .iter()
                .filter_map(|(_, dst)| Some((temp.join(dst.file_name()?), dst.clone())))
                .collect();
            if self.on_sftp(into) {
                // Moved on the server, never over what is there: a name
                // taken meanwhile is left to its step.
                let moves = made
                    .iter()
                    .map(|(made, _)| (made.clone(), into.to_path_buf(), 0))
                    .collect();
                let outcome = self.each(&["move", "-i"], &runs(moves), &|_| {}, control);
                // What left the folder of its own, only `gio move` takes:
                // placed.
                let left: std::collections::HashSet<std::ffi::OsString> = std::fs::read_dir(temp)?
                    .flatten()
                    .map(|e| e.file_name())
                    .collect();
                for (made, _) in &made {
                    if let Some(name) = made.file_name().filter(|n| !left.contains(*n)) {
                        placed(name);
                    }
                }
                outcome?;
            } else {
                for (made, dst) in made {
                    match skwir_core::op::transfer::place(&made, &dst) {
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Ok(()) => placed(dst.file_name().unwrap_or_default()),
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
        self.made.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn files_go_in_runs_by_folder_about_eight_in_all() {
        let files: Vec<(PathBuf, PathBuf, u64)> = (0..100)
            .map(|i| (PathBuf::from(format!("/a/{i}")), PathBuf::from("/b"), 1))
            .chain([(PathBuf::from("/a/x/1"), PathBuf::from("/b/x"), 1)])
            .collect();
        let runs = runs(files);
        assert_eq!(
            runs.len(),
            9,
            "a hundred in eight runs, the other folder's in its own"
        );
        assert!(runs.iter().all(|(_, names)| names.len() <= 13));
        assert_eq!(runs.iter().map(|(_, n)| n.len()).sum::<usize>(), 101);
        let gio = GioCopy::default().gio(
            Path::new("gio"),
            &["copy", "-i"],
            &[Path::new("-a file")],
            Path::new("/b"),
        );
        let args: Vec<&OsStr> = gio.get_args().collect();
        assert_eq!(
            args,
            ["copy", "-i", "--", "-a file", "/b/"],
            "a name is never an option"
        );
    }
}
