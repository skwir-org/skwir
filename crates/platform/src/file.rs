//! Opening the user's files for reading, and rewriting the desktop's shared
//! text files.
//!
//! Opening a FIFO for reading blocks until a writer appears, and no
//! cancellation token can interrupt `open(2)`. Every read of a file the user
//! pointed at (content sniffing, previews, thumbnails, EXIF) goes through
//! [`open_regular`], which never blocks and refuses anything that is not a
//! regular file: a named pipe, socket or device called `x.png` is not an image.
//!
//! Files other programs share with us (GTK bookmarks, `mimeapps.list`) are
//! changed through [`rewrite`] only.

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

/// Rewrite the text file at `link` with `change`, which gets its current
/// text (empty when there is no file) and answers the new text with a value
/// for the caller, or `None` to leave the file alone. Writes a file: call
/// from a worker.
///
/// The file is replaced whole (a reader never sees half of it), keeping its
/// mode. A file that is a link (a dotfile manager's) is updated where it
/// points; if that is read-only, as a declarative configuration is, it is
/// refused, naming the file as `what`, rather than replaced by a copy that
/// silently parts from the declaration.
///
/// # Errors
/// When the file cannot be read or written.
pub fn rewrite<T>(
    link: &Path,
    what: &str,
    change: impl FnOnce(&str) -> Option<(String, T)>,
) -> anyhow::Result<Option<T>> {
    use std::io::Write as _;
    let file = match std::fs::canonicalize(link) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => link.to_path_buf(),
        Err(error) => return Err(error.into()),
    };
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let Some((updated, value)) = change(&text) else {
        return Ok(None);
    };
    let parent = file
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{what} has no folder"))?;
    std::fs::create_dir_all(parent)?;
    let read_only = |error: &io::Error| {
        matches!(
            error.kind(),
            io::ErrorKind::ReadOnlyFilesystem | io::ErrorKind::PermissionDenied
        )
    };
    let mut staged = match tempfile::NamedTempFile::new_in(parent) {
        Ok(staged) => staged,
        Err(error) if read_only(&error) => {
            anyhow::bail!("{what} is read-only; change it where it is managed")
        }
        Err(error) => return Err(error.into()),
    };
    if let Ok(metadata) = std::fs::metadata(&file) {
        std::fs::set_permissions(staged.path(), metadata.permissions())?;
    }
    staged.write_all(updated.as_bytes())?;
    staged.persist(&file)?;
    Ok(Some(value))
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
