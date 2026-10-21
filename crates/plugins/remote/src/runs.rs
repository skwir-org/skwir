//! A tool run several times at once on a batch of copies (ADR-078): eight
//! `gio copy`, eight `sftp` sessions, which their server serves together
//! where a mount makes a folder's names one at a time under the kernel's
//! lock. Each held while the plan is paused, stopped with it.

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
                    let ended = skwir_platform::process::run_watching(
                        &mut run.command,
                        run.input.as_deref(),
                        control.cancel(),
                        &|| control.is_paused(),
                        &mut |_: &[u8]| {},
                    )
                    .and_then(|finished| {
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
