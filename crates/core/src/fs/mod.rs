//! The built-in filesystem provider: streamed listing, lazy stat, watching.

pub mod dir;
pub mod file;
mod hydrate;
mod path_provider;
pub mod stat;
pub mod volume;
mod watch;
#[cfg(windows)]
pub(crate) mod win;
pub mod xattr;

pub use dir::Dir;
pub use hydrate::{HYDRATE_JOB, hydrate_path, hydrate_paths};
pub use path_provider::PathProvider;
pub use stat::{Statter, content_path, stat_entry, stat_path};
pub use watch::Settled;

/// A size the way the file list shows one: `512 B`, `1.5 KB`, `3.2 GB`, in
/// binary multiples (the platform's `inspection::size`, which skwir's
/// words of sizes all are).
pub fn size_said(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Whether a folder's entry `entry`, read with `std::fs::read_dir`, is
/// hidden: its name starts with a dot, or Windows marks it hidden (which
/// its listing says, free). A Mac's `UF_HIDDEN` would cost a stat per
/// entry here; a listing's `Entry::is_hidden` has it.
pub fn is_hidden(entry: &std::fs::DirEntry) -> bool {
    if entry.file_name().as_encoded_bytes().starts_with(b".") {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        /// `FILE_ATTRIBUTE_HIDDEN`.
        const HIDDEN: u32 = 2;
        entry
            .metadata()
            .is_ok_and(|meta| meta.file_attributes() & HIDDEN != 0)
    }
    #[cfg(not(windows))]
    false
}

/// The folders inside `dir`, in the panes' name order, links to folders
/// included; names starting with a dot only when `hidden`. What a folder's
/// trail menus list (`App::places_in`), and a trashed folder's. Reads the
/// folder: on a worker.
///
/// # Errors
/// When the folder cannot be read.
pub fn subfolders(dir: &std::path::Path, hidden: bool) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        if !hidden && is_hidden(&entry) {
            continue;
        }
        let is_dir = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
            Ok(kind) => kind.is_dir(),
            Err(_) => false,
        };
        if is_dir {
            dirs.push(entry.path());
        }
    }
    dirs.sort_by_cached_key(|dir| crate::collate::sort_key(dir.file_name().unwrap_or_default()));
    Ok(dirs)
}
