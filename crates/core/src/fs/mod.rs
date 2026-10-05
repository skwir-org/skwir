//! The built-in filesystem provider: getdents streaming, lazy statx, watching.

pub mod file;
mod hydrate;
mod path_provider;
pub mod stat;
pub mod volume;
mod watch;

pub use hydrate::{hydrate_path, hydrate_paths};
pub use path_provider::PathProvider;
pub use stat::{Statter, content_path, stat_entry, stat_path};
pub use watch::Settled;

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
        if !hidden && entry.file_name().as_encoded_bytes().starts_with(b".") {
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
