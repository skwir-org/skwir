//! How a copy counts what it wrote: at once, or, onto a drive the user
//! can unplug, written through (`builtin::copy_entry`).
//!
//! Through, the drive is handed each window's worth of bytes as soon as
//! they are written, whatever files they belong to, and the copy waits for
//! a window only while the drive holds the next: the drive never waits for
//! the copy, and the copy never runs more than two windows (about half a
//! second of the drive's writing; two seconds where a program writes the
//! volume) ahead of it. What progress counts is what the drive was handed. A plan's copies onto one volume share one
//! pace (`StepContext::pace_through`), so the drive's queue runs on from
//! one item to the next.
//!
//! Folders, names and the volume's own records are written out once, at
//! the plan's end, and on the way where moves wait to remove what they
//! copied (`StepContext::write_out`, `after_write_out`): that commits the
//! whole volume (`syncfs`), which on a journaled or copy-on-write
//! filesystem on a hard drive costs about a third of a second however
//! little was written. Paid every few MiB, as it once was, it held a copy
//! onto a USB hard drive (btrfs) to 7 MiB/s; handed by ranges, the same
//! copy runs at the drive's 80.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::File;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;

use super::{StepContext, sys};
use crate::fs::Dir;

/// What the drive is handed at a time: about `STEP` of its writing, so a
/// slow drive still shows progress and answers Cancel, measured on the
/// drive as it writes, never assumed of one. The first is a guess any
/// drive writes in a moment.
const FIRST: u64 = 4 << 20;
const LEAST: u64 = 1 << 20;
const MOST: u64 = 256 << 20;
const STEP: Duration = Duration::from_millis(250);
/// The same where the wait is a write-out of the whole volume
/// ([`Handed::Volume`]), longer: each one also writes the records of the
/// files written since, a cost by the write-out (seeks to the volume's
/// records on a hard drive), which a second of writing keeps a small
/// share of. 200 files of 1 MiB onto ntfs-3g on a USB hard drive went at
/// 81 MiB/s with a quarter of a second, 86 with a second, 110 written
/// out once at the end; a run varied by a third.
const VOLUME_STEP: Duration = Duration::from_secs(1);
/// Files handed to the drive and not yet known written, at most: each
/// holds a descriptor open.
const FILES: usize = 64;

const NOT_WRITTEN_OUT: &str = "the copy could not be written out to the drive";

/// How a copy counts what it wrote: at once, or written through.
pub(super) struct Pace {
    through: Option<RefCell<Through>>,
}

/// A pace written through, as it stands.
struct Through {
    /// The destination's volume, opened before anything was written: its
    /// write-out reports the errors since.
    drive: Arc<Dir>,
    handed: Handed,
    window: Window,
}

/// How the drive is handed what a copy wrote, and how the copy learns that
/// the drive has it.
enum Handed {
    /// The kernel keeps a file's bytes in memory until they are written
    /// (its page cache): each range of them is handed to the drive as
    /// written, and waited for once the drive holds the next
    /// (`sys::write_range`).
    Ranges(Ranges),
    /// A program writes the drive (`sys::written_by_a_program`; FUSE:
    /// ntfs-3g): a file's bytes are its as soon as written, kept in the
    /// device's memory, never the file's, so only a write-out of the whole
    /// volume reaches them. Nothing is committed there, but each write-out
    /// writes the records of the files written since (`VOLUME_STEP`); it
    /// runs on a thread of its own while the copy writes the next window.
    Volume(Volume),
}

/// [`Handed::Ranges`]: what the drive was handed and is not known to have
/// written, oldest first, and its bytes.
#[derive(Default)]
struct Ranges {
    queue: VecDeque<Range>,
    queued: u64,
}

/// Bytes of a file the drive was handed.
struct Range {
    file: File,
    from: u64,
    len: u64,
}

/// [`Handed::Volume`]: bytes written since the running write-out began,
/// and that write-out with the bytes it covers.
#[derive(Default)]
struct Volume {
    pending: u64,
    running: Option<(u64, std::thread::JoinHandle<io::Result<()>>)>,
}

/// How much the drive is handed at a time, from how fast it wrote what it
/// was handed last: about `step` of its writing.
struct Window {
    bytes: u64,
    step: Duration,
    /// Bytes known on the drive since `since`.
    measured: u64,
    since: Instant,
}

/// One file being written at a pace ([`Pace::writing`]).
pub(super) struct Writing<'a> {
    pace: &'a Pace,
    file: &'a File,
    /// Its bytes written, and how many of them the drive was handed.
    written: u64,
    given: u64,
}

impl Pace {
    pub(super) fn at_once() -> Self {
        Self { through: None }
    }

    /// Whether what is written is left to the system, not written through.
    pub(super) fn is_at_once(&self) -> bool {
        self.through.is_none()
    }

    /// Written through to the volume `drive` is on.
    pub(super) fn through(drive: Dir) -> Self {
        let handed = if sys::written_by_a_program(&drive) {
            Handed::Volume(Volume::default())
        } else {
            Handed::Ranges(Ranges::default())
        };
        Self::handing(drive, handed, FIRST)
    }

    /// Written through to `drive`, by its volume's write-outs (as where a
    /// program writes it) or by ranges, a window as small as can be, so a
    /// test's few MiB make several.
    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn testing(drive: Dir, by_volume: bool) -> Self {
        let handed = if by_volume {
            Handed::Volume(Volume::default())
        } else {
            Handed::Ranges(Ranges::default())
        };
        Self::handing(drive, handed, LEAST)
    }

    fn handing(drive: Dir, handed: Handed, window: u64) -> Self {
        let step = match handed {
            Handed::Ranges(_) => STEP,
            Handed::Volume(_) => VOLUME_STEP,
        };
        Self {
            through: Some(RefCell::new(Through {
                drive: Arc::new(drive),
                handed,
                window: Window {
                    bytes: window,
                    step,
                    measured: 0,
                    since: Instant::now(),
                },
            })),
        }
    }

    /// `file` is about to be written, from its start.
    pub(super) fn writing<'a>(&'a self, file: &'a File) -> Writing<'a> {
        Writing {
            pace: self,
            file,
            written: 0,
            given: 0,
        }
    }

    /// Wait until the drive has every byte it was handed, then write its
    /// volume out, names, folders and the drive's own cache included.
    ///
    /// # Errors
    /// When the drive could not write it all.
    pub(super) fn write_out(&self) -> anyhow::Result<()> {
        let Some(through) = &self.through else {
            return Ok(());
        };
        let mut through = through.borrow_mut();
        let Through {
            drive,
            handed,
            window,
        } = &mut *through;
        let handed = match handed {
            Handed::Ranges(ranges) => ranges.wait(window, true),
            Handed::Volume(volume) => volume.finish(window).map(|()| volume.pending = 0),
        };
        handed
            .and_then(|()| drive.sync_volume())
            .context(NOT_WRITTEN_OUT)
    }
}

impl Writing<'_> {
    /// `n` more bytes were written to the file.
    pub(super) fn wrote(&mut self, n: u64, ctx: &StepContext) -> anyhow::Result<()> {
        self.written += n;
        let pace = self.pace;
        let Some(through) = &pace.through else {
            ctx.advance(n);
            return Ok(());
        };
        let mut through = through.borrow_mut();
        let Through {
            drive,
            handed,
            window,
        } = &mut *through;
        match handed {
            Handed::Ranges(ranges) => {
                if self.written - self.given >= window.bytes {
                    self.hand(ranges, ctx)?;
                    ranges.wait(window, false).context(NOT_WRITTEN_OUT)?;
                }
            }
            Handed::Volume(volume) => {
                // The program has them.
                ctx.advance(n);
                volume.wrote(n, drive, window).context(NOT_WRITTEN_OUT)?;
            }
        }
        Ok(())
    }

    /// The file is whole: the drive is handed what it was not of it.
    pub(super) fn close(mut self, ctx: &StepContext) -> anyhow::Result<()> {
        let pace = self.pace;
        let Some(through) = &pace.through else {
            return Ok(());
        };
        let mut through = through.borrow_mut();
        let Through { handed, window, .. } = &mut *through;
        if let Handed::Ranges(ranges) = handed {
            self.hand(ranges, ctx)?;
            ranges.wait(window, false).context(NOT_WRITTEN_OUT)?;
        }
        Ok(())
    }

    /// Hand the drive what the file wrote since it was last handed, and
    /// count it.
    fn hand(&mut self, ranges: &mut Ranges, ctx: &StepContext) -> anyhow::Result<()> {
        let len = self.written - self.given;
        if len == 0 {
            return Ok(());
        }
        sys::write_range(self.file, self.given, len, false)
            .and_then(|()| self.file.try_clone())
            .map(|file| {
                ranges.queue.push_back(Range {
                    file,
                    from: self.given,
                    len,
                });
            })
            .context(NOT_WRITTEN_OUT)?;
        ranges.queued += len;
        self.given = self.written;
        ctx.advance(len);
        Ok(())
    }
}

impl Ranges {
    /// Wait for the oldest ranges while the drive holds a window besides
    /// them, or too many files; `all`, for every one.
    fn wait(&mut self, window: &mut Window, all: bool) -> io::Result<()> {
        while let Some(oldest) = self.queue.front() {
            let over = self.queued - oldest.len >= window.bytes || self.queue.len() > FILES;
            if !all && !over {
                break;
            }
            sys::write_range(&oldest.file, oldest.from, oldest.len, true)?;
            let len = oldest.len;
            self.queue.pop_front();
            self.queued -= len;
            window.written(len);
        }
        Ok(())
    }
}

impl Volume {
    /// `n` more bytes were written: once a window is, its write-out
    /// starts, and the copy waits for the one before.
    fn wrote(&mut self, n: u64, drive: &Arc<Dir>, window: &mut Window) -> io::Result<()> {
        self.pending += n;
        if self.pending >= window.bytes {
            self.write_out(drive, window)?;
        }
        Ok(())
    }

    /// Wait for the write-out running, then start one of what was written
    /// since, on a thread of its own (on this one where none can be had).
    fn write_out(&mut self, drive: &Arc<Dir>, window: &mut Window) -> io::Result<()> {
        self.finish(window)?;
        let n = std::mem::take(&mut self.pending);
        let volume = Arc::clone(drive);
        let thread = std::thread::Builder::new()
            .name("skwir-write-out".into())
            .spawn(move || volume.sync_volume());
        if let Ok(running) = thread {
            self.running = Some((n, running));
        } else {
            drive.sync_volume()?;
            window.written(n);
        }
        Ok(())
    }

    /// Wait for the write-out running, if one is. What was written since
    /// is the next write-out's.
    fn finish(&mut self, window: &mut Window) -> io::Result<()> {
        if let Some((n, running)) = self.running.take() {
            running
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("the write-out stopped")))?;
            window.written(n);
        }
        Ok(())
    }
}

impl Window {
    /// `n` bytes are known on the drive: size the next window to what the
    /// drive wrote over the last `STEP` or so.
    fn written(&mut self, n: u64) {
        self.measured += n;
        let took = self.since.elapsed();
        if took >= self.step / 2 {
            self.bytes = next_window(self.measured, took, self.step);
            self.measured = 0;
            self.since = Instant::now();
        }
    }
}

/// The window that takes about `step` on a drive that wrote `n` bytes in
/// `took`.
fn next_window(n: u64, took: Duration, step: Duration) -> u64 {
    let next = u128::from(n) * step.as_micros() / took.as_micros().max(1);
    u64::try_from(next).unwrap_or(u64::MAX).clamp(LEAST, MOST)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_takes_about_a_step_of_the_drive() {
        // 4 MiB in a second: a quarter of it next.
        assert_eq!(next_window(4 << 20, Duration::from_secs(1), STEP), 1 << 20);
        // A very slow drive is still handed the least window at a time.
        assert_eq!(next_window(1 << 20, Duration::from_secs(60), STEP), LEAST);
        // A fast one, no more than the most.
        assert_eq!(next_window(1 << 20, Duration::ZERO, STEP), MOST);
    }
}
