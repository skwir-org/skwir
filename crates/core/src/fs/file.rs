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

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// The most read of a small file another application keeps beside a file
/// (a sidecar, a `.trashinfo`, a folder's notes): one larger is not one.
pub const SIDE_FILE_LIMIT: u64 = 16 << 20;

/// Open `path` for reading if, once opened, it is a regular file.
///
/// # Errors
/// The open error, or `InvalidInput` when the path is not a regular file.
pub fn open_regular(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use rustix::fs::OFlags;
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(OFlags::NONBLOCK.bits().cast_signed())
            .open(path)?;
        ensure_regular(&file)?;
        let flags = rustix::fs::fcntl_getfl(&file)?;
        rustix::fs::fcntl_setfl(&file, flags - OFlags::NONBLOCK)?;
        Ok(file)
    }
    #[cfg(not(unix))]
    {
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
        use rustix::fs::OFlags;
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags((OFlags::NONBLOCK | OFlags::NOFOLLOW).bits().cast_signed())
            .open(path)
            .map_err(|e| {
                // A link at the end: `ELOOP`.
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
    #[cfg(not(unix))]
    {
        if std::fs::symlink_metadata(path)?.is_symlink() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a link"));
        }
        open_regular(path)
    }
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

fn ensure_regular(file: &File) -> io::Result<()> {
    if file.metadata()?.is_file() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
