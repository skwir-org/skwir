//! What a package carries beside skwir's programs, in its data folder
//! `share/skwir`: next to the program on Windows, above `bin` on Linux, a
//! Mac's `Resources/share` (`AGENTS.md`, "Build, test, lint"). Skwir's
//! own fonts (ADR-118, `fonts::bundled`) and the word saying how the copy
//! came (ADR-142, `channel`). None for a build not packaged.
//!
//! Each is looked for from the folder of the running program as
//! `skwir_core::folders::program` finds it, its links followed, as are
//! the helpers (`process::Worker::find`), the type database
//! (`mime::found_database`) and the licences (`about::licences`).

use std::path::{Path, PathBuf};

/// The data folders a package puts beside the program in `bin`, in the
/// order they are looked in: `share` next to it (Windows), above it
/// (Linux, and a Linux or Windows build's `target`), a Mac's
/// `Resources/share`.
pub(crate) fn shares(bin: &Path) -> [PathBuf; 3] {
    [
        bin.join("share"),
        bin.join("../share"),
        bin.join("../Resources/share"),
    ]
}

/// `name` in the data folder beside the program in `bin`, where `found`
/// says it is there: the first of the places a package puts it.
pub(crate) fn beside(bin: &Path, name: &str, found: fn(&Path) -> bool) -> Option<PathBuf> {
    shares(bin)
        .into_iter()
        .map(|share| share.join("skwir").join(name))
        .find(|path| found(path))
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    /// A program started through a link (a Mac's app, linked into
    /// Homebrew's `bin` by its cask) finds what its bundle carries beside
    /// the program itself, not beside the link: its helpers, its type
    /// database, its licences and what its data folder holds.
    #[test]
    fn a_program_started_through_a_link_finds_what_its_bundle_carries() {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("skwir.app/Contents");
        let macos = bundle.join("MacOS");
        let share = bundle.join("Resources/share");
        for folder in [
            macos.clone(),
            share.join("skwir"),
            share.join("mime"),
            bundle.join("Resources/licenses"),
        ] {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::write(share.join("skwir/channel"), "macos").unwrap();
        std::fs::write(share.join("mime/globs2"), "").unwrap();
        std::fs::write(macos.join("skwir"), "").unwrap();
        let helper = macos.join("skwir-pdf-worker");
        std::fs::write(&helper, "").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(macos.join("skwir"), bin.join("skwir")).unwrap();

        let worker = crate::process::Worker {
            variable: "SKWIR_TEST_UNSET_WORKER",
            executable: "skwir-pdf-worker",
        };
        let found = |folder: &Path| {
            (
                worker.found_beside(folder).is_some(),
                crate::mime::database_beside(folder).is_some(),
                crate::about::licences_beside(folder).is_some(),
                super::beside(folder, "channel", Path::is_file).is_some(),
            )
        };
        let folder = skwir_core::folders::program_of(&bin.join("skwir")).unwrap();
        assert_eq!(found(&folder), (true, true, true, true));
        assert_eq!(
            found(&bin),
            (false, false, false, false),
            "not beside the link"
        );
    }
}
