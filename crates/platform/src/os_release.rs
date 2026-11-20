//! What the system says it is, in its os-release (os-release(5), Linux's
//! alone): read here once for every reader, About's name for the system
//! and the updates plugin's link to the repository's commands. Reads a
//! file: call from a worker.

use std::path::Path;

/// The most read of the file: a larger one is not one.
const LIMIT: u64 = 64 << 10;

/// An os-release: its values written as a shell would read them, quoted
/// or not, with backslashes escaping in double quotes; comments and blank
/// lines read as nothing, the last line naming a key its value.
pub struct OsRelease(String);

impl OsRelease {
    /// The system's, on Linux: `/etc/os-release` alone where it is there,
    /// `/usr/lib/os-release` only where it is not, as os-release(5) says.
    /// `None` where neither is, or the one there cannot be read.
    pub fn read() -> Option<OsRelease> {
        if !cfg!(target_os = "linux") {
            return None;
        }
        OsRelease::read_first(&[
            Path::new("/etc/os-release"),
            Path::new("/usr/lib/os-release"),
        ])
    }

    /// The first of `paths` that is there, read; `None` where that one
    /// cannot be read (too large, not text, refused), never the next.
    fn read_first(paths: &[&Path]) -> Option<OsRelease> {
        for path in paths {
            match crate::file::read_regular_text(path, LIMIT) {
                Ok(text) => return Some(OsRelease(text)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::debug!(path = %path.display(), %error, "os-release not read");
                    return None;
                }
            }
        }
        None
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

    /// Whether the system is `id` (its `ID`), or says it is like it (one of
    /// its `ID_LIKE`'s words). Only the system's own two keys are read, and
    /// `ID_LIKE` may name the closest system alone: Linux Mint says it is
    /// like `ubuntu` and `debian`, elementary OS like `ubuntu` alone, so
    /// it is not like `debian` here.
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

#[cfg(test)]
mod tests {
    use super::OsRelease;

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

    /// The parts of the grammar a shell reads as nothing, or once: a
    /// comment naming another system, a blank line, a key repeated (the
    /// last line holds), CRLF line ends, and a one-word `ID_LIKE` naming
    /// the closest system alone.
    #[test]
    fn comments_blank_lines_and_repeats_are_read_as_a_shell_would() {
        let text = "# ID=fedora\r\n\r\nNAME=\"elementary OS\"\r\nID=ubuntu\r\n\
                    ID=elementary\r\n  # ID_LIKE=debian\r\nID_LIKE=ubuntu\r\n";
        let os = OsRelease::of_text(text);
        assert!(os.is_like("elementary"), "the last ID holds, CR left out");
        assert!(!os.is_like("fedora"), "a comment is nothing");
        assert!(os.is_like("ubuntu"), "a one-word ID_LIKE");
        assert!(!os.is_like("debian"), "not what ubuntu is like");
        assert_eq!(os.pretty_name().as_deref(), Some("elementary OS"));
    }

    /// `/etc/os-release` alone where it is there, even one that cannot be
    /// read; `/usr/lib/os-release` only where it is not.
    #[test]
    fn the_first_file_there_is_the_only_one_read() {
        let tmp = tempfile::tempdir().unwrap();
        let (etc, lib) = (tmp.path().join("etc"), tmp.path().join("lib"));
        std::fs::write(&lib, "ID=fedora\n").unwrap();
        let read = || OsRelease::read_first(&[&etc, &lib]);
        assert!(read().is_some_and(|os| os.is_like("fedora")), "none in etc");
        std::fs::write(&etc, "ID=debian\n").unwrap();
        assert!(read().is_some_and(|os| os.is_like("debian")), "etc's alone");
        std::fs::write(&etc, [0xff, 0xfe, b'\n']).unwrap();
        assert!(read().is_none(), "etc's unreadable: not lib's");
    }
}
