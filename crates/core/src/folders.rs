//! skwir's own folders, where each system keeps an application's: its
//! settings, its state (the window as it was left), the user's data (what
//! they made in skwir) and its throwaway cache (ADR-035). On Linux the XDG base
//! directories, read one way (`crate::xdg`); on macOS
//! `~/Library/Application Support` and `~/Library/Caches`; on Windows the
//! roaming and the local application data (`%APPDATA%`,
//! `%LOCALAPPDATA%`). What other applications share (the desktop's
//! bookmarks, its trash, its thumbnails) is not skwir's and keeps its
//! own place.
//!
//! What a running skwir keeps for the session alone (its sockets, the
//! marker of the one that takes launches, its mount points) is in
//! [`runtime`], every folder there made through [`private`].

use std::path::{Path, PathBuf};

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

/// The folder skwir keeps what the user made in it in (`…/skwir`): saved
/// searches, pinned tags, tags' colours, apart from the settings
/// (ADR-057). `$XDG_DATA_HOME` on Linux; the settings' own folder on macOS
/// and Windows, whose application data is one folder.
pub fn data() -> Option<PathBuf> {
    #[cfg(any(target_os = "macos", windows))]
    let dir = config();
    #[cfg(not(any(target_os = "macos", windows)))]
    let dir = crate::xdg::data_home().map(|dir| dir.join("skwir"));
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

/// The folder a running skwir keeps what is the session's alone in:
/// `$XDG_RUNTIME_DIR/skwir`, where the session has a runtime folder.
/// Anything there may be a socket another program of the user's connects
/// to, so what makes it, or a folder in it, makes it with [`private`]: one
/// made with the default mode is open to all, and a socket made there
/// reachable by them.
pub fn runtime() -> Option<PathBuf> {
    crate::xdg::runtime_dir().map(|dir| dir.join("skwir"))
}

/// `folder` made the user's alone: made where it is missing, with any
/// folder above it that is missing (mode 0700 on Unix), and refused unless
/// it is a folder of the user's (a link to one is refused). One of the
/// user's that is open to others, as a plain `create_dir_all` made
/// [`runtime`] before 0.4.0, is closed to them: only the user could have
/// put anything in it. On Windows, made where it is missing: the
/// folders skwir keeps there are the user's own already.
///
/// # Errors
/// When it cannot be made or closed, or is not the user's folder.
pub fn private(folder: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(folder)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let metadata = std::fs::symlink_metadata(folder)?;
        if !metadata.is_dir() || metadata.uid() != rustix::process::getuid().as_raw() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{} is not the user's alone", folder.display()),
            ));
        }
        if metadata.mode() & 0o077 != 0 {
            std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::private;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    }

    #[test]
    fn a_private_folder_is_made_the_users_alone_with_the_folders_above_it() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("skwir").join("rc");
        private(&folder).unwrap();
        assert_eq!(mode(&folder), 0o700);
        assert_eq!(mode(&root.path().join("skwir")), 0o700);
        // Made again: nothing to do.
        private(&folder).unwrap();
        assert_eq!(mode(&folder), 0o700);
    }

    #[test]
    fn a_folder_of_the_users_made_open_to_others_is_closed() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("skwir");
        // As `create_dir_all` makes it, under the usual umask.
        std::fs::create_dir(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(folder.join("running-test"), "1\n").unwrap();
        private(&folder).unwrap();
        assert_eq!(mode(&folder), 0o700);
        assert!(folder.join("running-test").exists(), "what it held stays");
    }

    #[test]
    fn a_link_or_a_file_is_refused_and_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = root.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = root.path().join("skwir");
        std::os::unix::fs::symlink(&elsewhere, &link).unwrap();
        assert!(private(&link).is_err());
        assert_eq!(mode(&elsewhere), 0o755, "the link's target is not closed");
        let file = root.path().join("file");
        std::fs::write(&file, "").unwrap();
        assert!(private(&file).is_err());
    }
}
