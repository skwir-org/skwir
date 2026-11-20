//! What the About dialog says of the system skwir runs on and of where
//! it is installed: the system's own name for itself, and the folder its
//! package keeps skwir's licences in; and what the system says it is
//! (`OsRelease`), which the updates plugin reads too. Each reads files:
//! call from a worker.

use std::path::{Path, PathBuf};

/// The system's name and version as it names itself (`Fedora Linux 42
/// (Workstation Edition)`), when it does: os-release's `PRETTY_NAME` on
/// Linux, else its `NAME`.
pub fn system_name() -> Option<String> {
    OsRelease::read()?.pretty_name()
}

/// What the system says it is, in its os-release (`/etc/os-release`, else
/// `/usr/lib/os-release`, on Linux): its values written as a shell would
/// read them, quoted or not, with backslashes escaping in double quotes,
/// read here alone.
pub struct OsRelease(String);

impl OsRelease {
    /// The system's, on Linux. Reads a file: on a worker.
    pub fn read() -> Option<OsRelease> {
        if !cfg!(target_os = "linux") {
            return None;
        }
        ["/etc/os-release", "/usr/lib/os-release"]
            .into_iter()
            .find_map(|path| crate::file::read_regular_text(Path::new(path), 64 << 10).ok())
            .map(OsRelease)
    }

    /// An os-release's text, as read.
    pub fn of_text(text: &str) -> OsRelease {
        OsRelease(text.to_owned())
    }

    /// The value of `key`, the last line naming it, unless empty.
    fn value(&self, key: &str) -> Option<String> {
        self.0
            .lines()
            .filter_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
            .next_back()
            .map(unquote)
            .filter(|value| !value.is_empty())
    }

    /// Its name for the system: `PRETTY_NAME`, else `NAME`.
    pub fn pretty_name(&self) -> Option<String> {
        self.value("PRETTY_NAME").or_else(|| self.value("NAME"))
    }

    /// Whether the system is `id` (`ID`), or says it is like it (one of
    /// `ID_LIKE`'s words): Linux Mint is like `ubuntu`, which is like
    /// `debian`.
    pub fn is_like(&self, id: &str) -> bool {
        self.value("ID").as_deref() == Some(id)
            || self
                .value("ID_LIKE")
                .is_some_and(|like| like.split_whitespace().any(|word| word == id))
    }
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if let Some(inner) = value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return inner.to_owned();
    }
    let inner = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' {
            chars.next().unwrap_or('\\')
        } else {
            c
        });
    }
    out
}

/// What shows skwir's licences where this installation keeps them: the
/// crates' third-party notices when there, else their folder. `None` for
/// a build that was not packaged.
pub fn licences() -> Option<PathBuf> {
    let program = std::env::current_exe().ok()?;
    licences_beside(program.parent()?)
}

/// [`licences`] for a program in `bin`. Where each package puts them
/// (`tools/package`): Windows' beside the program, a Mac's bundle in its
/// resources, Fedora's and Arch's under `share/licenses`, Debian's and
/// Ubuntu's with the package's documentation.
fn licences_beside(bin: &Path) -> Option<PathBuf> {
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
    use super::{OsRelease, licences_beside};

    fn pretty_name(text: &str) -> Option<String> {
        OsRelease::of_text(text).pretty_name()
    }

    #[test]
    fn the_system_is_named_as_its_os_release_says() {
        let fedora = "NAME=\"Fedora Linux\"\nVERSION_ID=42\n\
                      PRETTY_NAME=\"Fedora Linux 42 (Workstation Edition)\"\n";
        assert_eq!(
            pretty_name(fedora).as_deref(),
            Some("Fedora Linux 42 (Workstation Edition)")
        );
        assert_eq!(pretty_name("NAME=Arch\n").as_deref(), Some("Arch"));
        assert_eq!(
            pretty_name(r"PRETTY_NAME='Back\slash'").as_deref(),
            Some(r"Back\slash"),
            "single quotes escape nothing"
        );
        assert_eq!(
            pretty_name(r#"PRETTY_NAME="A \"quoted\" one""#).as_deref(),
            Some(r#"A "quoted" one"#)
        );
        assert_eq!(
            pretty_name("PRETTY_NAME=\"\"\nNAME=Plain").as_deref(),
            Some("Plain")
        );
        assert_eq!(pretty_name("ID=nixos\n"), None);
        assert_eq!(
            pretty_name("PRETTY_NAMES=No\n"),
            None,
            "a key, not a prefix"
        );
    }

    /// What a system is like: its own id, or one its `ID_LIKE` names,
    /// whole words only.
    #[test]
    fn a_system_is_what_its_ids_say() {
        let mint =
            OsRelease::of_text("NAME=\"Linux Mint\"\nID=linuxmint\nID_LIKE=\"ubuntu debian\"\n");
        assert!(mint.is_like("linuxmint") && mint.is_like("ubuntu") && mint.is_like("debian"));
        assert!(!mint.is_like("fedora"));
        let ubuntu = OsRelease::of_text("ID=ubuntu\nID_LIKE=debian\n");
        assert!(ubuntu.is_like("ubuntu") && ubuntu.is_like("debian"));
        let quoted = OsRelease::of_text("ID='debian'\n");
        assert!(quoted.is_like("debian"));
        let part = OsRelease::of_text("ID=debianish\nID_LIKE=\"ubuntu-like\"\n");
        assert!(!part.is_like("debian") && !part.is_like("ubuntu"));
        assert!(!OsRelease::of_text("").is_like("debian"));
    }

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
