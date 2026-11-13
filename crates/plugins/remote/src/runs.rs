//! A tool run several times at once on a batch of copies (ADR-078): eight
//! `gio copy`, eight `sftp` sessions, which their server serves together
//! where a mount makes a folder's names one at a time under the kernel's
//! lock. Each held while the plan is paused, stopped with it.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use skwir_core::op::transfer::Control;

/// One run of a tool: its command, what it reads on its input, and the
/// bytes it copies, told once it ended well.
pub(crate) struct Run {
    pub(crate) command: Command,
    pub(crate) input: Option<Vec<u8>>,
    pub(crate) bytes: u64,
}

/// A run that ended badly: how, and what it said.
#[derive(Debug)]
pub(crate) struct Failed {
    pub(crate) tool: &'static str,
    pub(crate) status: ExitStatus,
    pub(crate) said: String,
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} failed ({}): {}", self.tool, self.status, self.said)
    }
}

impl std::error::Error for Failed {}

/// Run each of `runs`, `at_once` at a time, telling `ran` the bytes of
/// each that ended well; the first that fails stops those not begun.
///
/// # Errors
/// The first run that failed ([`Failed`]), could not start, or the plan's
/// cancel.
pub(crate) fn at_once(
    tool: &'static str,
    runs: Vec<Run>,
    at_once: usize,
    ran: &(dyn Fn(u64) + Sync),
    control: &Control,
) -> anyhow::Result<()> {
    at_once_following(tool, runs, at_once, ran, None, control)
}

/// [`at_once`], what each run's process has open followed by `following`
/// as it runs (ADR-135): how far each name it was given came.
///
/// # Errors
/// As [`at_once`].
pub(crate) fn at_once_following(
    tool: &'static str,
    runs: Vec<Run>,
    at_once: usize,
    ran: &(dyn Fn(u64) + Sync),
    following: Option<&crate::following::Following<'_>>,
    control: &Control,
) -> anyhow::Result<()> {
    let workers = at_once.min(runs.len());
    let queue = Mutex::new(runs.into_iter().collect::<std::collections::VecDeque<_>>());
    let stop = AtomicBool::new(false);
    let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    control.wait_while_paused();
                    if stop.load(Ordering::Relaxed) || control.is_cancelled() {
                        return;
                    }
                    let Some(mut run) = queue.lock().pop_front() else {
                        return;
                    };
                    let looked = std::sync::atomic::AtomicU32::new(0);
                    let look = |pid: u32| {
                        looked.store(pid, Ordering::Relaxed);
                        if let Some(following) = following {
                            following.look(pid);
                        }
                    };
                    let ran_to_end = skwir_platform::process::run_looking(
                        &mut run.command,
                        run.input.as_deref(),
                        control.cancel(),
                        &|| control.is_paused(),
                        &mut |_: &[u8]| {},
                        &look,
                    );
                    // What it had open last is done, as it ended.
                    if let Some(following) = following {
                        following.ended(looked.load(Ordering::Relaxed));
                    }
                    let ended = ran_to_end.and_then(|finished| {
                        if finished.status.success() {
                            Ok(())
                        } else {
                            Err(Failed {
                                tool,
                                status: finished.status,
                                said: finished.stderr,
                            }
                            .into())
                        }
                    });
                    match ended {
                        Ok(()) => ran(run.bytes),
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

/// What removing the tree at `path` takes: its files and links, and its
/// folders with their depth below the tree's top, deepest last. Read
/// through the mount, which shows a link as a link (never followed).
pub(crate) fn tree(
    path: &Path,
    depth: usize,
    files: &mut Vec<PathBuf>,
    folders: &mut Vec<(usize, PathBuf)>,
) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        for entry in std::fs::read_dir(path)? {
            tree(&entry?.path(), depth + 1, files, folders)?;
        }
        folders.push((depth, path.to_path_buf()));
    } else {
        files.push(path.to_path_buf());
    }
    Ok(())
}
