//! gio, for a batch of copies to or from an SFTP folder of GVfs
//! (ADR-078): eight `gio copy` at once, which GVfs's daemon serves
//! together, where through its FUSE bridge the kernel makes the names of
//! a folder one at a time. 100 files of 16 KiB onto a server 20 ms away:
//! 11.6 s through the bridge, eight at a time; 9.0 s by one `gio copy`;
//! 1.2 s by eight (2026-10-21).
//!
//! `gio` maps a path of the bridge to its mount's address itself. With
//! `-i` and nothing on its input it never replaces a file: without it,
//! `gio copy` and `gio move` do. Folders, which `gio copy` does not copy,
//! are made through the bridge first and given their times last
//! (`transfer::Layout`). Only SFTP is served, as measured; GVfs's other
//! kinds keep the bridge.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use parking_lot::Mutex;
use skwir_core::op::transfer::{Control, Layout, Transfer};

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
}

impl GioCopy {
    pub fn new(env: Vec<(OsString, OsString)>, runtime: Option<PathBuf>) -> Self {
        GioCopy {
            env,
            runtime,
            made: AtomicUsize::new(0),
        }
    }

    /// The batches it copied (a test asks).
    pub fn made(&self) -> usize {
        self.made.load(Ordering::Relaxed)
    }

    /// Whether `path` is in an SFTP mount of this session's GVfs bridge:
    /// read from the bridge folder's name, never from a server.
    fn on_sftp(&self, path: &Path) -> bool {
        let Some(bridge) = crate::gvfs::bridge_in(self.runtime.clone()) else {
            return false;
        };
        path.strip_prefix(&bridge)
            .ok()
            .and_then(|below| below.components().next())
            .and_then(|mount| mount.as_os_str().to_str().and_then(Spec::parse))
            .is_some_and(|spec| spec.kind == "sftp")
    }

    /// `gio` with `words`, then `names`, then `into` as a folder.
    fn gio(&self, gio: &Path, words: &[&str], names: &[&Path], into: &Path) -> Command {
        let mut command = Command::new(gio);
        crate::gvfs::english(&mut command);
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        command.args(words).arg("--").args(names);
        let mut folder = into.as_os_str().to_owned();
        folder.push("/");
        command.arg(folder);
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
        let gio = program().ok_or_else(|| anyhow::anyhow!("gio (GLib) is not installed"))?;
        let next = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
        std::thread::scope(|s| {
            for _ in 0..AT_ONCE.min(runs.len()) {
                s.spawn(|| {
                    loop {
                        control.wait_while_paused();
                        if stop.load(Ordering::Relaxed) || control.is_cancelled() {
                            return;
                        }
                        let Some((into, names)) = runs.get(next.fetch_add(1, Ordering::Relaxed))
                        else {
                            return;
                        };
                        let paths: Vec<&Path> = names.iter().map(|(p, _)| p.as_path()).collect();
                        let mut command = self.gio(gio, words, &paths, into);
                        let ended = skwir_platform::process::run_watching(
                            &mut command,
                            None,
                            control.cancel(),
                            &|| control.is_paused(),
                            &mut |_: &[u8]| {},
                        )
                        .and_then(|finished| {
                            anyhow::ensure!(
                                finished.status.success(),
                                "gio {} failed ({}): {}",
                                words[0],
                                finished.status,
                                finished.stderr
                            );
                            Ok(())
                        });
                        match ended {
                            Ok(()) => ran(names.iter().map(|(_, bytes)| bytes).sum()),
                            Err(error) => {
                                stop.store(true, Ordering::Relaxed);
                                failed.lock().get_or_insert(error);
                            }
                        }
                    }
                });
            }
        });
        if let Some(error) = failed.into_inner() {
            return Err(error);
        }
        anyhow::ensure!(!control.is_cancelled(), "cancelled");
        Ok(())
    }
}

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
        copied: &(dyn Fn(u64) + Sync),
        control: &Control,
    ) -> anyhow::Result<()> {
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
        self.each(
            &["copy", "-i", "--preserve", "--no-dereference"],
            &runs(files),
            &ran,
            control,
        )?;
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
                    .into_iter()
                    .map(|(made, _)| (made, into.to_path_buf(), 0))
                    .collect();
                self.each(&["move", "-i"], &runs(moves), &|_| {}, control)?;
            } else {
                for (made, dst) in made {
                    match skwir_core::op::transfer::place(&made, &dst) {
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                        placed => placed?,
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
