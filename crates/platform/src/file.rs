//! Opening the user's files for reading, and rewriting the desktop's shared
//! text files.
//!
//! Every read of a file the user pointed at (content sniffing, previews,
//! thumbnails, EXIF) or that another application keeps beside one (a
//! sidecar, a `.trashinfo`) goes through [`open_regular`] or
//! [`read_regular`], the core's (`bibli_core::fs::file`), which never block
//! and refuse anything that is not a regular file: a named pipe, socket or
//! device called `x.png` is not an image.
//!
//! Files other programs share with us (GTK bookmarks, `mimeapps.list`) are
//! changed through [`rewrite`] only.

use std::io;
use std::path::Path;

pub use bibli_core::fs::file::{SIDE_FILE_LIMIT, open_regular, read_regular, read_regular_text};

/// Rewrite the text file at/// Rewrite the text file at `link` with `change`, which gets its current
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
    let file = match bibli_core::os_path::canonical(link) {
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
    // On the disk before it takes the file's place: renamed over it first,
    // a power cut could leave the desktop's bookmarks or its default
    // applications empty (XFS, ext4 without `auto_da_alloc`).
    staged.as_file().sync_all()?;
    staged.persist(&file)?;
    Ok(Some(value))
}

/// The most a desktop's own file read here may hold (a desktop entry, a
/// `mimeapps.list`, an icon theme's `index.theme`, a toolkit's settings):
/// far above any made by hand or by a tool, so one that is larger is no
/// such file.
pub const DESKTOP_FILE: u64 = 4 << 20;

/// A private folder for what a helper hands bibli and is read at once (a
/// video's frames, a document's pages): in memory through `/dev/shm` where
/// that exists and is writable, else the ordinary temporary folder, which
/// may be on a disk. There a file rewritten in place (the video's one
/// frame slot) stays in the page cache, its pages written out at most at
/// the system's pace, not at each frame; Windows is asked to keep the
/// video's slot in memory (`FILE_ATTRIBUTE_TEMPORARY`).
///
/// # Errors
/// When no folder can be made.
pub fn memory_dir(prefix: &str) -> std::io::Result<tempfile::TempDir> {
    #[cfg(target_os = "linux")]
    if let Ok(dir) = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in("/dev/shm")
    {
        return Ok(dir);
    }
    tempfile::Builder::new().prefix(prefix).tempdir()
}
