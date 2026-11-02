//! A large file on a remote read in several ranges at once
//! (`builtin::copy_contents`).
//!
//! The kernel reads a FUSE mount ahead 128 KiB at a time, so a file read
//! in order waits a round trip to the server for each 128 KiB: from an
//! SFTP server 20 ms away, 11 MiB/s, where `scp` drew 74 (2026-10-21).
//! Read [`AT_ONCE`] ranges at a time, the same file came in 4.4 times
//! faster, and 4.9 times 2 ms away. The ranges are written in their order,
//! so a copy written through to a drive (`pace`) and its progress go as
//! for any other file; what arrives is measured as it comes, so its speed
//! is the server's, not the jumps of ranges written in turn (ADR-113).
//! Each reader asks for a piece of its range at a time, about a quarter
//! second's worth (`builtin::Chunk`, from 256 KiB): its bytes are counted
//! as they come, and Pause and Cancel are answered between two pieces, the
//! writer looking for Cancel while it waits for the range it writes next.
//! A range asked for whole was 4 MiB more from a server already left:
//! eight at once, 32 MiB, many seconds from a slow one, the bar still
//! until each came whole.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;

use parking_lot::{Condvar, Mutex};

use super::StepContext;
use super::builtin::Chunk;
use super::pace::Writing;

/// What each range holds.
const RANGE: u64 = 4 << 20;
/// How many ranges are read at once, and held at most: about 32 MiB, for
/// the copy's while.
const AT_ONCE: u64 = 8;
/// How long the writer waits for the range it writes next before it looks
/// whether the copy was cancelled.
const LOOK: std::time::Duration = std::time::Duration::from_millis(100);

/// Whether the file whose metadata is `meta` is read in ranges: on a FUSE
/// remote (`fs::reach`: sshfs, GVfs, rclone), at least two ranges long.
/// Asked of the mount table in memory. A kernel's network share reads
/// ahead as far as it needs (NFS, SMB).
pub(super) fn wanted(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.len() >= 2 * RANGE
        && crate::fs::reach::of_device(meta.dev()).is_some_and(|remote| !remote.by_kernel())
}

/// Which ranges the readers may take: none more than [`AT_ONCE`] ahead of
/// what is written, and none once the copy stopped.
struct Window {
    next: u64,
    written: u64,
    stop: bool,
}

/// Copy the bytes of `sf` from `from` up to `len` to `df` (at its offset,
/// written in order), reading [`AT_ONCE`] ranges at a time; the bytes
/// copied, fewer where a range came short (the file shrank meanwhile, or
/// its read was cut): what follows it is not written, so the copy holds
/// the source's start, and the caller reads on from there in order.
/// `sf`'s offset is left alone.
///
/// # Errors
/// When a range cannot be read or written, the drive refuses the pace, or
/// the copy is cancelled.
pub(super) fn copy(
    sf: &File,
    mut df: &File,
    from: u64,
    len: u64,
    writing: &mut Writing<'_>,
    ctx: &StepContext,
) -> anyhow::Result<u64> {
    let ranges = len.saturating_sub(from).div_ceil(RANGE);
    let window = Mutex::new(Window {
        next: 0,
        written: 0,
        stop: false,
    });
    let turn = Condvar::new();
    let (tx, rx) = std::sync::mpsc::channel::<(u64, std::io::Result<Vec<u8>>)>();
    let arrivals = ctx.arrivals();
    let copied = std::thread::scope(|s| {
        for _ in 0..AT_ONCE.min(ranges) {
            let (tx, window, turn) = (tx.clone(), &window, &turn);
            let control = ctx.control();
            let arrived = arrivals.clone();
            s.spawn(move || {
                let mut piece = Chunk::from_least(RANGE as usize);
                // Between two pieces: paused, it waits; cancelled, or the
                // writing ended, it stops.
                let stopped = || {
                    control.wait_while_paused();
                    window.lock().stop || control.is_cancelled()
                };
                loop {
                    let i = {
                        let mut w = window.lock();
                        while !w.stop && w.next < ranges && w.next >= w.written + AT_ONCE {
                            turn.wait(&mut w);
                        }
                        if w.stop || w.next >= ranges || control.is_cancelled() {
                            return;
                        }
                        w.next += 1;
                        w.next - 1
                    };
                    let at = from + i * RANGE;
                    let want = usize::try_from(RANGE.min(len - at)).unwrap_or(0);
                    let read = read_at(sf, (at, want), &mut piece, &arrived, &stopped);
                    if read.is_err() && stopped() {
                        return;
                    }
                    if tx.send((i, read)).is_err() {
                        return;
                    }
                }
            });
        }
        drop(tx);
        let written = (|| {
            let mut arrived = BTreeMap::new();
            let (mut next, mut copied) = (0, 0u64);
            while next < ranges {
                // A reader holds the range written next: it comes, Cancel
                // answered meanwhile.
                ctx.check_cancelled()?;
                let (i, read) = match rx.recv_timeout(LOOK) {
                    Ok(got) => got,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                };
                arrived.insert(i, read);
                while let Some(read) = arrived.remove(&next) {
                    let bytes = read?;
                    let want = RANGE.min(len - (from + next * RANGE));
                    df.write_all(&bytes)?;
                    copied += bytes.len() as u64;
                    // No longer arriving, then done: never both at once.
                    ctx.landed(bytes.len() as u64);
                    writing.wrote(bytes.len() as u64, ctx)?;
                    next += 1;
                    window.lock().written = next;
                    turn.notify_all();
                    ctx.check_cancelled()?;
                    // Short: the ranges after it would land too soon.
                    if (bytes.len() as u64) < want {
                        return Ok(copied);
                    }
                }
            }
            // Cut short: cancelled, or the readers gone without a word.
            ctx.check_cancelled()?;
            anyhow::ensure!(next == ranges, "the file stopped being read");
            anyhow::Ok(copied)
        })();
        // Whatever ended the writing, the readers stop at their piece.
        window.lock().stop = true;
        turn.notify_all();
        written
    });
    // What arrived and was never written is not on its way any more.
    ctx.landed(u64::MAX);
    copied
}

/// Up to `want` bytes of `file` from `at`: fewer where it ends first.
/// Asked for a `piece` at a time, which follows how long each took,
/// `arrived` told each part as it comes; `stopped` asked before each, its
/// error said once it holds.
fn read_at(
    file: &File,
    (at, want): (u64, usize),
    piece: &mut Chunk,
    arrived: &dyn Fn(u64),
    stopped: &dyn Fn() -> bool,
) -> std::io::Result<Vec<u8>> {
    use std::os::unix::fs::FileExt;
    let mut bytes = vec![0; want];
    let mut filled = 0;
    while filled < want {
        if stopped() {
            return Err(std::io::Error::other("the copy stopped"));
        }
        let end = (filled + piece.size).min(want);
        let began = std::time::Instant::now();
        match file.read_at(&mut bytes[filled..end], at + filled as u64) {
            Ok(0) => break,
            Ok(n) => {
                piece.took(began.elapsed());
                filled += n;
                arrived(n as u64);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    bytes.truncate(filled);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancel::CancelToken;

    fn bytes(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 7 + i / 4093) as u8).collect()
    }

    #[test]
    fn ranges_read_at_once_are_written_whole_and_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        // Not a multiple of a range: the last is short.
        let len = 3 * RANGE as usize + 12_345;
        let (from, to) = (tmp.path().join("it's big.bin"), tmp.path().join("copy.bin"));
        std::fs::write(&from, bytes(len)).unwrap();
        let (sf, df) = (File::open(&from).unwrap(), File::create(&to).unwrap());
        let ctx = StepContext::with_cancel(CancelToken::new());
        let pace = super::super::pace::Pace::at_once();
        let mut writing = pace.writing(&df, 0);
        let copied = copy(&sf, &df, 0, len as u64, &mut writing, &ctx).unwrap();
        assert_eq!(copied, len as u64);
        assert_eq!(std::fs::read(&to).unwrap(), bytes(len));
    }

    #[test]
    fn a_file_that_shrank_meanwhile_is_copied_as_it_is() {
        let tmp = tempfile::tempdir().unwrap();
        let len = 2 * RANGE as usize + 10;
        let (from, to) = (tmp.path().join("shrank.bin"), tmp.path().join("copy.bin"));
        std::fs::write(&from, bytes(len)).unwrap();
        // Shorter now than the length the copy was asked for.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&from)
            .unwrap()
            .set_len(RANGE + 3)
            .unwrap();
        let (sf, df) = (File::open(&from).unwrap(), File::create(&to).unwrap());
        let ctx = StepContext::with_cancel(CancelToken::new());
        let pace = super::super::pace::Pace::at_once();
        let mut writing = pace.writing(&df, 0);
        let copied = copy(&sf, &df, 0, len as u64, &mut writing, &ctx).unwrap();
        assert_eq!(copied, RANGE + 3);
        assert_eq!(
            std::fs::read(&to).unwrap(),
            bytes(len)[..RANGE as usize + 3]
        );
    }

    #[test]
    fn a_cancelled_copy_stops_and_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let len = 4 * RANGE as usize;
        let (from, to) = (tmp.path().join("big.bin"), tmp.path().join("copy.bin"));
        std::fs::write(&from, bytes(len)).unwrap();
        let (sf, df) = (File::open(&from).unwrap(), File::create(&to).unwrap());
        let cancel = CancelToken::new();
        cancel.cancel();
        let ctx = StepContext::with_cancel(cancel);
        let pace = super::super::pace::Pace::at_once();
        let mut writing = pace.writing(&df, 0);
        assert!(copy(&sf, &df, 0, len as u64, &mut writing, &ctx).is_err());
    }

    /// A range is asked for a piece at a time: what arrived is told as
    /// each comes, and a copy stopped meanwhile reads no further piece.
    #[test]
    fn a_range_comes_a_piece_at_a_time_and_stops_between_two() {
        use std::cell::Cell;
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("big.bin");
        std::fs::write(&from, bytes(RANGE as usize)).unwrap();
        let file = File::open(&from).unwrap();
        let told = std::sync::Mutex::new(Vec::new());
        let arrived = |n: u64| told.lock().unwrap().push(n);
        let whole = (0, RANGE as usize);
        let mut piece = Chunk::from_least(RANGE as usize);
        let read = read_at(&file, whole, &mut piece, &arrived, &|| false).unwrap();
        assert_eq!(read, bytes(RANGE as usize));
        // From a quarter mebibyte, larger as each comes quickly.
        let pieces = told.lock().unwrap().clone();
        assert_eq!(pieces.first(), Some(&(256 << 10)), "{pieces:?}");
        assert!(pieces.len() > 1, "{pieces:?}");
        assert_eq!(pieces.iter().sum::<u64>(), RANGE);

        // Stopped after its first piece: the second is never asked for.
        told.lock().unwrap().clear();
        let asked = Cell::new(0);
        let stopped = || {
            asked.set(asked.get() + 1);
            asked.get() > 1
        };
        let mut piece = Chunk::from_least(RANGE as usize);
        assert!(read_at(&file, whole, &mut piece, &arrived, &stopped).is_err());
        assert_eq!(*told.lock().unwrap(), vec![256 << 10]);
    }
}
