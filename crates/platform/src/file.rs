//! Opening the user's files for reading.
//!
//! Opening a FIFO for reading blocks until a writer appears, and no
//! cancellation token can interrupt `open(2)`. Every read of a file the user
//! pointed at (content sniffing, previews, thumbnails, EXIF) goes through
//! [`open_regular`], which never blocks and refuses anything that is not a
//! regular file: a named pipe, socket or device called `x.png` is not an image.

use std::fs::File;
use std::io;
use std::path::Path;

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

    #[cfg(unix)]
    #[test]
    fn a_fifo_without_a_writer_is_refused_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("x.png");
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
    }
}
