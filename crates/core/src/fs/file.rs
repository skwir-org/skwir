//! Reading a file found where a file was expected: one the user pointed at,
//! or one another application keeps beside a file (a sidecar, a
//! `.trashinfo`).
//!
//! Opening a FIFO for reading blocks until a writer appears, and no
//! cancellation token can interrupt `open(2)`. Every such read goes through
//! [`open_regular`], which never blocks and refuses anything that is not a
//! regular file: a named pipe, socket or device called `x.png` is not an
//! image, nor one called `x.xmp` a sidecar. [`read_regular`] also refuses
//! a file larger than its caller expects, so one named like a small file
//! is never read whole into memory.
//!
//! A file replaced whole (settings, the cache, a sidecar, a thumbnail) is
//! written beside it, then put in its place by [`take_place`].

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// The most read of a small file another application keeps beside a file
/// (a sidecar, a `.trashinfo`, a folder's notes): one larger is not one.
pub const SIDE_FILE_LIMIT: u64 = 16 << 20;

/// Open `path` for reading if it is a regular file.
///
/// What is there is looked at before it is opened: opening a device runs
/// its driver (a serial port's resets the board on it), and opening a
/// pipe releases its writer.
///
/// # Errors
/// The open error, or `InvalidInput` when the path is not a regular file.
pub fn open_regular(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        open_looked_at(path, false)
    }
    #[cfg(not(unix))]
    {
        if !std::fs::metadata(path)?.is_file() {
            return Err(not_regular());
        }
        let file = File::open(path)?;
        ensure_regular(&file)?;
        Ok(file)
    }
}

/// Open `path` for reading if it is a regular file itself, never through a
/// link at its end: a link is refused as anything else that is not one.
///
/// # Errors
/// The open error (`NotFound` when nothing is there), or `InvalidInput`
/// when it is not a regular file.
pub fn open_regular_unfollowed(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        open_looked_at(path, true)
    }
    #[cfg(not(unix))]
    {
        if std::fs::symlink_metadata(path)?.is_symlink() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a link"));
        }
        open_regular(path)
    }
}

/// Open what is at `path` (itself, with `unfollowed`, a link at its end
/// refused) once it is seen to be a regular file. Linux looks at it
/// through a descriptor that opens nothing (`O_PATH`) and opens that
/// again (`/proc/self/fd`): the same file, whatever takes its name
/// meanwhile. Elsewhere, or without `/proc`, it is looked at by its path,
/// then opened without waiting (`O_NONBLOCK`) and looked at again.
#[cfg(unix)]
fn open_looked_at(path: &Path, unfollowed: bool) -> io::Result<File> {
    use rustix::fs::OFlags;
    use std::os::unix::fs::OpenOptionsExt;
    let not_followed = if unfollowed {
        OFlags::NOFOLLOW
    } else {
        OFlags::empty()
    };
    #[cfg(target_os = "linux")]
    {
        use rustix::fs::Mode;
        use std::os::fd::AsRawFd;
        let at = rustix::fs::open(
            path,
            OFlags::PATH | OFlags::CLOEXEC | not_followed,
            Mode::empty(),
        )?;
        let kind = rustix::fs::FileType::from_raw_mode(rustix::fs::fstat(&at)?.st_mode);
        if kind == rustix::fs::FileType::Symlink {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a link"));
        }
        if kind != rustix::fs::FileType::RegularFile {
            return Err(not_regular());
        }
        let again = format!("/proc/self/fd/{}", at.as_raw_fd());
        match rustix::fs::open(
            again.as_str(),
            OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => return Ok(File::from(fd)),
            Err(rustix::io::Errno::NOENT) => {}
            Err(error) => return Err(error.into()),
        }
    }
    let meta = if unfollowed {
        std::fs::symlink_metadata(path)?
    } else {
        std::fs::metadata(path)?
    };
    if meta.is_symlink() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a link"));
    }
    if !meta.is_file() {
        return Err(not_regular());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlags::NONBLOCK | not_followed).bits().cast_signed())
        .open(path)
        .map_err(|e| {
            // A link at the end since: `ELOOP`.
            if e.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) {
                io::Error::new(io::ErrorKind::InvalidInput, "a link")
            } else {
                e
            }
        })?;
    ensure_regular(&file)?;
    let flags = rustix::fs::fcntl_getfl(&file)?;
    rustix::fs::fcntl_setfl(&file, flags - OFlags::NONBLOCK)?;
    Ok(file)
}

/// The whole of the regular file at `path`, at most `limit` bytes long.
///
/// # Errors
/// As [`open_regular`], the read error, or `InvalidData` when the file is
/// longer than `limit`.
pub fn read_regular(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = open_regular(path)?;
    let mut bytes = Vec::new();
    // What it says it holds is a hint: a file can grow while it is read.
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too large"));
    }
    Ok(bytes)
}

/// [`read_regular`] as text.
///
/// # Errors
/// As [`read_regular`], or `InvalidData` when it is not UTF-8.
pub fn read_regular_text(path: &Path, limit: u64) -> io::Result<String> {
    String::from_utf8(read_regular(path, limit)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Put `staged` in `target`'s place, whole: a reader sees the old file or
/// the new one. Windows refuses to replace a file that is open (another
/// program reading it, a worker of Skwir's) as `tempfile`'s `persist` asks
/// (`MoveFileExW`). `std::fs::rename` then asks again as Unix replaces,
/// the reader keeping the old file (POSIX semantics, Windows 10 1709 on),
/// which a reader that shares no deletion still refuses, and Wine always:
/// there it is tried again while the reader lets go, for [`HELD_FOR`].
/// Call from a worker.
///
/// # Errors
/// When it cannot take the place: `staged` is removed then.
pub fn take_place(staged: tempfile::NamedTempFile, target: &Path) -> io::Result<()> {
    // Kept first: on Windows that takes off the temporary mark it was made
    // with, which the file in place must not keep.
    let staged = staged.into_temp_path().keep().map_err(|e| e.error)?;
    let mut waited = std::time::Duration::ZERO;
    let mut wait = std::time::Duration::from_millis(10);
    loop {
        match std::fs::rename(&staged, target) {
            Ok(()) => return Ok(()),
            Err(error)
                if cfg!(windows)
                    && error.kind() == io::ErrorKind::PermissionDenied
                    && waited < HELD_FOR =>
            {
                std::thread::sleep(wait);
                waited += wait;
                wait *= 2;
            }
            Err(error) => {
                let _ = std::fs::remove_file(&staged);
                return Err(error);
            }
        }
    }
}

/// How long [`take_place`] waits, on Windows, for the readers of a file to
/// let go of it: a reader of a small file holds it a moment.
pub const HELD_FOR: std::time::Duration = std::time::Duration::from_millis(600);

fn ensure_regular(file: &File) -> io::Result<()> {
    if file.metadata()?.is_file() {
        Ok(())
    } else {
        Err(not_regular())
    }
}

fn not_regular() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a regular file")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file open for reading a moment is replaced all the same: under
    /// its reader where the system can (Unix, Windows' POSIX semantics), or
    /// once the reader lets go (Wine, a reader that shares no deletion).
    /// Nothing staged is left beside it.
    #[test]
    fn a_file_open_is_replaced_whole() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("it's the notes.txt");
        std::fs::write(&target, b"old").unwrap();
        let reader = File::open(&target).unwrap();
        let reading = std::thread::spawn(move || {
            std::thread::sleep(HELD_FOR / 6);
            drop(reader);
        });
        let mut staged = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        staged.write_all(b"new").unwrap();
        take_place(staged, &target).unwrap();
        reading.join().unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// Unix replaces a file under its reader, who keeps the old bytes.
    #[cfg(unix)]
    #[test]
    fn a_reader_keeps_the_file_it_opened() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("notes");
        std::fs::write(&target, b"old").unwrap();
        let mut reader = File::open(&target).unwrap();
        let mut staged = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        staged.write_all(b"new").unwrap();
        take_place(staged, &target).unwrap();
        let mut old = Vec::new();
        reader.read_to_end(&mut old).unwrap();
        assert_eq!(old, b"old");
    }

    #[test]
    fn regular_files_open_and_directories_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(open_regular(&file).is_ok());
        assert!(open_regular(dir.path()).is_err());
    }

    /// Unfollowed, a link is refused, and nothing there is `NotFound`.
    #[cfg(unix)]
    #[test]
    fn a_link_is_not_opened_unfollowed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(open_regular_unfollowed(&file).is_ok());
        assert_eq!(
            open_regular_unfollowed(&link).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            open_regular_unfollowed(&dir.path().join("gone"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert!(open_regular_unfollowed(dir.path()).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_fifo_without_a_writer_is_refused_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("x.xmp");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        let err = open_regular(&fifo).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(read_regular(&fifo, SIDE_FILE_LIMIT).is_err());
    }

    /// A pipe is looked at, never opened, to be refused: a writer waiting
    /// for a reader still waits. Through a link too.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_pipe_refused_is_never_opened() {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("it's a pipe");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        let link = dir.path().join("x.jpg.xmp");
        std::os::unix::fs::symlink(&fifo, &link).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let writer = {
            let fifo = fifo.clone();
            std::thread::spawn(move || {
                // Waits until a reader opens it.
                let opened = std::fs::OpenOptions::new().write(true).open(&fifo);
                tx.send(opened.is_ok()).unwrap();
            })
        };
        for _ in 0..3 {
            assert!(open_regular(&link).is_err());
            assert!(open_regular(&fifo).is_err());
            assert!(open_regular_unfollowed(&fifo).is_err());
        }
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "the writer was let go"
        );
        // Let it go.
        let reader = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed())
            .open(&fifo)
            .unwrap();
        assert!(rx.recv().unwrap());
        writer.join().unwrap();
        drop(reader);
    }

    #[test]
    fn a_file_longer_than_asked_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.trashinfo");
        std::fs::write(&file, b"0123456789").unwrap();
        assert_eq!(read_regular(&file, 10).unwrap(), b"0123456789");
        let err = read_regular(&file, 9).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        std::fs::write(&file, [0xff, 0xfe]).unwrap();
        assert!(read_regular_text(&file, 10).is_err());
    }
}
