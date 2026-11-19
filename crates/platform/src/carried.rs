//! What a package carries beside skwir's programs, in its data folder
//! `share/skwir`: next to the program on Windows, above `bin` on Linux, a
//! Mac's `Resources/share` (`AGENTS.md`, "Build, test, lint"). Skwir's
//! own fonts (ADR-118, `fonts::bundled`) and the word saying how the copy
//! came (ADR-142, `channel`). None for a build not packaged.

use std::path::{Path, PathBuf};

/// `name` in the data folder beside the program in `bin`, where `found`
/// says it is there: the first of the places a package puts it.
pub(crate) fn beside(bin: &Path, name: &str, found: fn(&Path) -> bool) -> Option<PathBuf> {
    [
        bin.join("share"),
        bin.join("../share"),
        bin.join("../Resources/share"),
    ]
    .into_iter()
    .map(|share| share.join("skwir").join(name))
    .find(|path| found(path))
}

/// The folder holding the running program, as `beside` takes it, the
/// one place what a package carries is looked for from. Its links
/// followed: a Mac's `current_exe` is the path the program was started
/// by, so an app started through a link in `PATH` finds its bundle's
/// `Resources` only so (Linux's is already the program's own). The path as
/// the system gave it where it cannot be followed, and on Windows, where
/// a followed path is a verbatim one (`\\?\`) and the program's own
/// already. Reads the disk: off the window's thread.
pub(crate) fn program_folder() -> Option<PathBuf> {
    folder_of(&std::env::current_exe().ok()?)
}

/// The folder holding `program`, as `program_folder` finds it.
fn folder_of(program: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    if let Ok(followed) = std::fs::canonicalize(program) {
        return followed.parent().map(Path::to_path_buf);
    }
    program.parent().map(Path::to_path_buf)
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;

    /// A program started through a link finds what its package carries
    /// beside the program itself, not beside the link.
    #[test]
    fn a_program_started_through_a_link_finds_its_own_folder() {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("skwir.app/Contents");
        std::fs::create_dir_all(bundle.join("MacOS")).unwrap();
        std::fs::create_dir_all(bundle.join("Resources/share/skwir")).unwrap();
        std::fs::write(bundle.join("Resources/share/skwir/channel"), "macos").unwrap();
        std::fs::write(bundle.join("MacOS/skwir"), "").unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(bundle.join("MacOS/skwir"), bin.join("skwir")).unwrap();
        let folder = super::folder_of(&bin.join("skwir")).unwrap();
        assert!(super::beside(&folder, "channel", Path::is_file).is_some());
        assert!(
            super::beside(&bin, "channel", Path::is_file).is_none(),
            "not beside the link"
        );
        // A path that cannot be followed is taken as given.
        assert_eq!(
            super::folder_of(&bin.join("gone/skwir")),
            Some(bin.join("gone"))
        );
    }
}
