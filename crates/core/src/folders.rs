//! bibli's own folders, where each system keeps an application's: its
//! settings and its throwaway cache (ADR-035). On Linux the XDG base
//! directories, read one way (`crate::xdg`); on macOS
//! `~/Library/Application Support` and `~/Library/Caches`; on Windows the
//! roaming and the local application data (`%APPDATA%`,
//! `%LOCALAPPDATA%`). What other applications share (the desktop's
//! bookmarks, its trash, its thumbnails) is not bibli's and keeps its
//! own place.

use std::path::PathBuf;

/// The folder bibli's settings are kept in (`…/bibli`).
pub fn config() -> Option<PathBuf> {
    #[cfg(any(target_os = "macos", windows))]
    let base = dirs::config_dir();
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = crate::xdg::config_home();
    base.map(|dir| dir.join("bibli"))
}

/// The folder of bibli's throwaway cache: `…/bibli` on Linux and macOS,
/// `%LOCALAPPDATA%\bibli\cache` on Windows, whose local application data
/// is every application's folder of its own.
pub fn cache() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let dir = dirs::cache_dir().map(|dir| dir.join("bibli"));
    #[cfg(windows)]
    let dir = dirs::cache_dir().map(|dir| dir.join("bibli").join("cache"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let dir = crate::xdg::cache_home().map(|dir| dir.join("bibli"));
    dir
}
