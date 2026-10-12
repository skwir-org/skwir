//! skwir's own folders, where each system keeps an application's: its
//! settings, its state (the window as it was left) and its throwaway
//! cache (ADR-035). On Linux the XDG base
//! directories, read one way (`crate::xdg`); on macOS
//! `~/Library/Application Support` and `~/Library/Caches`; on Windows the
//! roaming and the local application data (`%APPDATA%`,
//! `%LOCALAPPDATA%`). What other applications share (the desktop's
//! bookmarks, its trash, its thumbnails) is not skwir's and keeps its
//! own place.

use std::path::PathBuf;

/// The folder skwir's settings are kept in (`…/skwir`).
pub fn config() -> Option<PathBuf> {
    #[cfg(any(target_os = "macos", windows))]
    let base = dirs::config_dir();
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = crate::xdg::config_home();
    base.map(|dir| dir.join("skwir"))
}

/// The folder skwir keeps its state in (`…/skwir`): the window as it was
/// left, apart from the settings (ADR-056). `$XDG_STATE_HOME` on Linux;
/// beside the settings on macOS, which has no folder of the kind; the
/// local application data on Windows, state being this machine's.
pub fn state() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let dir = config();
    #[cfg(windows)]
    let dir = dirs::data_local_dir().map(|dir| dir.join("skwir"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let dir = crate::xdg::state_home().map(|dir| dir.join("skwir"));
    dir
}

/// The folder of skwir's throwaway cache: `…/skwir` on Linux and macOS,
/// `%LOCALAPPDATA%\skwir\cache` on Windows, whose local application data
/// is every application's folder of its own.
pub fn cache() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let dir = dirs::cache_dir().map(|dir| dir.join("skwir"));
    #[cfg(windows)]
    let dir = dirs::cache_dir().map(|dir| dir.join("skwir").join("cache"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let dir = crate::xdg::cache_home().map(|dir| dir.join("skwir"));
    dir
}
