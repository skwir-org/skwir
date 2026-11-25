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
//!
//! What a package carries beside skwir's programs (its helpers, the type
//! database, the licences, the fonts; a helper's own libraries) is looked
//! for from the folder of the running program, [`program`], its links
//! followed: never from `current_exe`'s parent.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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

/// The folder holding the running program, its links followed: the one
/// place what its package carries is looked for from (the module's
/// header), by skwir and by its helpers alike. On macOS `current_exe` is
/// the path the program was started by, so an app started through a link
/// on `PATH` (Homebrew's cask links `skwir` into its `bin`) finds its
/// bundle only so; Linux's is the program's own already. The path as the
/// system gave it where it cannot be followed, and on Windows, where a
/// followed path is a verbatim one (`\\?\`) and the program's own
/// already. Read once, from the disk: the first call is off the window's
/// thread.
pub fn program() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| program_of(&std::env::current_exe().ok()?))
        .as_deref()
}

/// The folder holding `program`, as [`program`] finds the running one's:
/// for a test, given the path a program was started by.
pub fn program_of(program: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    if let Ok(followed) = std::fs::canonicalize(program) {
        return followed.parent().map(Path::to_path_buf);
    }
    program.parent().map(Path::to_path_buf)
}

/// The folder a running skwir keeps what is the session's alone in:
/// `$XDG_RUNTIME_DIR/skwir`, where the session has a runtime folder; else,
/// on Unix, one of the user's in the temporary folder: macOS's own
/// `$TMPDIR/skwir` (the user's alone), elsewhere `/tmp/skwir-<uid>`, a
/// name no other user's skwir takes. None on Windows, which has none.
/// Anything there may be a socket another program of the user's connects
/// to, so what makes it, or a folder in it, makes it with [`private`]: one
/// made with the default mode is open to all, and a socket made there
/// reachable by them. Every part of skwir names it through here, so all
/// agree on one folder.
pub fn runtime() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        Some(runtime_of(crate::xdg::runtime_dir()))
    }
    #[cfg(not(unix))]
    {
        crate::xdg::runtime_dir().map(|dir| dir.join("skwir"))
    }
}

/// [`runtime`] on Unix, the session's runtime folder being `session`.
#[cfg(unix)]
fn runtime_of(session: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = session {
        return dir.join("skwir");
    }
    #[cfg(target_os = "macos")]
    {
        std::env::temp_dir().join("skwir")
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::temp_dir().join(format!("skwir-{}", rustix::process::getuid().as_raw()))
    }
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
                crate::words!(
                    folders_not_private = "{ $path } is not the user's alone",
                    path = folder.display().to_string(),
                    note = "Why Skwir will not keep its session's files (its sockets, its mounts) in a folder: someone else owns it. The folder's path."
                ),
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

    use super::{private, program_of, runtime_of};

    /// A program started through a link is in the folder of the file the
    /// link names, not the link's: a Mac's app linked into Homebrew's
    /// `bin` is in its bundle's `MacOS`. A path that cannot be followed is
    /// taken as given.
    #[test]
    fn a_program_started_through_a_link_is_in_its_own_folder() {
        let root = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(root.path()).unwrap();
        let macos = root.join("skwir.app/Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(macos.join("skwir"), "").unwrap();
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(macos.join("skwir"), bin.join("skwir")).unwrap();
        assert_eq!(program_of(&bin.join("skwir")), Some(macos.clone()));
        assert_eq!(program_of(&macos.join("skwir")), Some(macos));
        assert_eq!(program_of(&bin.join("gone/skwir")), Some(bin.join("gone")));
    }

    /// The session's runtime folder holds skwir's; where it has none, the
    /// user's own in the temporary folder, named for the user on Linux.
    #[test]
    fn the_runtime_folder_is_the_session_s_else_the_user_s_own() {
        let session = std::path::Path::new("/run/user/1234");
        assert_eq!(
            runtime_of(Some(session.to_path_buf())),
            session.join("skwir")
        );
        let fallback = runtime_of(None);
        assert_eq!(fallback.parent(), Some(std::env::temp_dir().as_path()));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            fallback.file_name().and_then(|n| n.to_str()),
            Some(format!("skwir-{}", rustix::process::getuid().as_raw()).as_str())
        );
    }

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
