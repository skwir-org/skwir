//! What the About dialog says of the system skwir runs on and of where
//! it is installed: the system's own name for itself, and the folder its
//! package keeps skwir's licences in. Both read files: call from a worker.

use std::path::{Path, PathBuf};

/// The system's name and version as it names itself (`Fedora Linux 42
/// (Workstation Edition)`), when it does: os-release's `PRETTY_NAME` on
/// Linux, else its `NAME` (`os_release`).
pub fn system_name() -> Option<String> {
    crate::os_release::OsRelease::read()?.pretty_name()
}

/// What shows skwir's licences where this installation keeps them: the
/// crates' third-party notices when there, else their folder, found from
/// the program's folder, its links followed
/// (`skwir_core::folders::program`). `None` for a build that was not
/// packaged.
pub fn licences() -> Option<PathBuf> {
    licences_beside(skwir_core::folders::program()?)
}

/// [`licences`] for a program in `bin`. Where each package puts them
/// (`tools/package`): Windows' beside the program, a Mac's bundle in its
/// resources, Fedora's and Arch's under `share/licenses`, Debian's and
/// Ubuntu's with the package's documentation.
pub(crate) fn licences_beside(bin: &Path) -> Option<PathBuf> {
    let prefix = bin.parent();
    let folders = [
        Some(bin.join("licenses")),
        prefix.map(|p| p.join("Resources").join("licenses")),
        prefix.map(|p| p.join("share").join("licenses").join("skwir")),
        prefix.map(|p| p.join("share").join("doc").join("skwir")),
    ];
    let folder = folders
        .into_iter()
        .flatten()
        .find(|folder| folder.is_dir())?;
    let notices = folder.join("THIRD-PARTY-NOTICES");
    Some(if notices.is_file() { notices } else { folder })
}

#[cfg(test)]
mod tests {
    use super::licences_beside;

    #[test]
    fn the_licences_are_where_each_package_puts_them() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        assert_eq!(licences_beside(&bin), None, "a build not packaged");
        let debian = tmp.path().join("share/doc/skwir");
        std::fs::create_dir_all(&debian).unwrap();
        assert_eq!(licences_beside(&bin), Some(debian.clone()));
        std::fs::write(debian.join("THIRD-PARTY-NOTICES"), "notices").unwrap();
        assert_eq!(
            licences_beside(&bin),
            Some(debian.join("THIRD-PARTY-NOTICES"))
        );
        let fedora = tmp.path().join("share/licenses/skwir");
        std::fs::create_dir_all(&fedora).unwrap();
        assert_eq!(
            licences_beside(&bin),
            Some(fedora),
            "the licences' own folder first"
        );
        let mac = tmp.path().join("Contents");
        let resources = mac.join("Resources/licenses");
        std::fs::create_dir_all(&resources).unwrap();
        assert_eq!(licences_beside(&mac.join("MacOS")), Some(resources));
    }
}
