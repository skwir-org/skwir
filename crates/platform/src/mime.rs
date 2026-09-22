//! MIME types via the shared-mime-info database.

use std::ffi::OsStr;
use std::fmt;
use std::path::Path;

/// A MIME type such as `image/png`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Mime(pub String);

impl Mime {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `image` for `image/png`.
    pub fn top_level(&self) -> &str {
        self.0.split('/').next().unwrap_or(&self.0)
    }

    /// The icon name derived from the type: `image-png`.
    pub fn icon_name(&self) -> String {
        self.0.replace('/', "-")
    }

    /// Generic fallback icon: `image-x-generic`.
    pub fn generic_icon_name(&self) -> String {
        imp::generic_icon_name(self)
    }

    /// Human description from the database, if available.
    pub fn description(&self) -> Option<String> {
        imp::description(self)
    }
}

impl fmt::Display for Mime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Guess from the file name only. No I/O; cheap enough for every row.
///
/// `None` means the name says nothing (`settings.cfg`, `README`, `.env`):
/// not that the file is binary. Only `by_content` can say that.
pub fn by_name(name: &OsStr) -> Option<Mime> {
    imp::by_name(name)
}

/// What the bytes say, for a file whose name says nothing. Reads the head
/// of the file: call from a worker. The database's magic first (a PNG named
/// `picture.cfg` is a PNG); without a match, `text/plain` when the head is
/// UTF-8 without NUL, else `application/octet-stream`, which from here, and
/// only from here, means the bytes were looked at and are binary. `None`
/// when the file cannot be read.
pub fn by_content(path: &Path) -> Option<Mime> {
    imp::by_content(path)
}

/// Bytes read to decide what a file without a known name holds. The
/// database's deepest magic rules look further, but at that distance the
/// name usually says it first; the text rule is git's (a NUL in the first
/// 8000 bytes).
#[cfg(target_os = "linux")]
const SNIFF: usize = 8 * 1024;

/// `text/plain` or `application/octet-stream` from a file's head: the one
/// text-or-binary rule, the same one the preview applies to a declared type
/// (`crate::inspection::text_prefix`).
#[cfg(any(target_os = "linux", test))]
fn text_or_binary(head: &[u8]) -> Mime {
    Mime(
        if crate::inspection::text_prefix(head).is_some() {
            "text/plain"
        } else {
            "application/octet-stream"
        }
        .to_owned(),
    )
}

/// The untranslated `<comment>` of a shared-mime-info type file, capitalized.
#[cfg(any(target_os = "linux", test))]
fn comment_of(xml: &str) -> Option<String> {
    let start = xml.find("<comment>")? + "<comment>".len();
    let end = start + xml[start..].find("</comment>")?;
    let text = xml[start..end]
        .trim()
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&apos;", "'")
        .replace("&quot;", "\"");
    let mut chars = text.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

pub fn is_directory(mime: &Mime) -> bool {
    mime.0 == "inode/directory"
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::str::FromStr;
    use std::sync::OnceLock;

    use super::Mime;

    fn db() -> &'static xdg_mime::SharedMimeInfo {
        static DB: OnceLock<xdg_mime::SharedMimeInfo> = OnceLock::new();
        DB.get_or_init(xdg_mime::SharedMimeInfo::new)
    }

    fn to_mime(m: &mime::Mime) -> Mime {
        Mime(m.essence_str().to_owned())
    }

    fn parse(m: &Mime) -> Option<mime::Mime> {
        mime::Mime::from_str(&m.0).ok()
    }

    pub fn by_name(name: &OsStr) -> Option<Mime> {
        let name = name.to_str()?;
        // The database answers application/octet-stream for a name that
        // matches no glob: its "no idea", never a type the name said.
        db().get_mime_types_from_file_name(name)
            .first()
            .filter(|m| **m != mime::APPLICATION_OCTET_STREAM)
            .map(to_mime)
    }

    pub fn by_content(path: &Path) -> Option<Mime> {
        use std::io::Read;
        let mut head = Vec::with_capacity(super::SNIFF);
        std::fs::File::open(path)
            .ok()?
            .take(super::SNIFF as u64)
            .read_to_end(&mut head)
            .ok()?;
        // Magic, which also names an empty file (application/x-zerosize).
        // There is no magic for text/plain: what it does not know is text
        // or binary by the bytes.
        Some(
            db().get_mime_type_for_data(&head)
                .map_or_else(|| super::text_or_binary(&head), |(m, _)| to_mime(&m)),
        )
    }

    pub fn generic_icon_name(m: &Mime) -> String {
        parse(m)
            .and_then(|p| db().lookup_generic_icon_name(&p))
            .unwrap_or_else(|| format!("{}-x-generic", m.top_level()))
    }

    /// xdg-mime 0.4 does not expose the `<comment>` element, so read it from
    /// the per-type XML the database installs. Reads a file: call from a
    /// worker and cache the answer.
    pub fn description(m: &Mime) -> Option<String> {
        let home = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")));
        let system = std::env::var_os("XDG_DATA_DIRS")
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        home.into_iter()
            .chain(std::env::split_paths(&system))
            .find_map(|dir| {
                let xml =
                    std::fs::read_to_string(dir.join("mime").join(format!("{}.xml", m.0))).ok()?;
                super::comment_of(&xml)
            })
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::Mime;

    pub fn by_name(_name: &OsStr) -> Option<Mime> {
        None
    }
    pub fn by_content(_path: &Path) -> Option<Mime> {
        None
    }
    pub fn generic_icon_name(m: &Mime) -> String {
        format!("{}-x-generic", m.top_level())
    }
    pub fn description(_m: &Mime) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn png_by_name() {
        let m = by_name(OsStr::new("foo.png")).expect("shared-mime-info present");
        assert_eq!(m.as_str(), "image/png");
        assert_eq!(m.top_level(), "image");
        assert_eq!(m.icon_name(), "image-png");
        assert_eq!(m.generic_icon_name(), "image-x-generic");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_name_the_database_does_not_know_says_nothing() {
        for name in ["settings.cfg", "app.ini", "README", ".env", "Cargo.lock"] {
            assert_eq!(by_name(OsStr::new(name)), None, "{name}");
        }
        assert_eq!(
            by_name(OsStr::new("notes.txt")).map(|m| m.0),
            Some("text/plain".into())
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn the_bytes_decide_for_a_name_that_says_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            by_content(&path).map(|m| m.0)
        };
        assert_eq!(
            file("settings.cfg", "[section]\nkey = valeur é\n".as_bytes()),
            Some("text/plain".into())
        );
        assert_eq!(
            file("blob.cfg", b"\x00\x01\x02text after a NUL"),
            Some("application/octet-stream".into())
        );
        assert_eq!(
            file("picture.cfg", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"),
            Some("image/png".into())
        );
        assert_eq!(
            file("script.cfg", b"#!/bin/sh\necho hi\n"),
            Some("application/x-shellscript".into())
        );
        assert_eq!(
            file("empty.cfg", b""),
            Some("application/x-zerosize".into())
        );
        assert_eq!(by_content(&dir.path().join("missing.cfg")), None);
    }

    #[test]
    fn a_head_cut_through_a_character_is_still_text() {
        let text = "caf\u{e9}".repeat(3);
        let cut = &text.as_bytes()[..text.len() - 1];
        assert_eq!(text_or_binary(cut).0, "text/plain");
        assert_eq!(text_or_binary(&[0xff, 0xfe]).0, "application/octet-stream");
    }

    #[test]
    fn the_untranslated_comment_is_the_description() {
        let xml = r#"<mime-type type="text/x-sample"><comment>plain &amp; simple document</comment>
            <comment xml:lang="fr">document</comment></mime-type>"#;
        assert_eq!(comment_of(xml).as_deref(), Some("Plain & simple document"));
        assert_eq!(comment_of("<mime-type/>"), None);
    }

    #[test]
    fn directory_check() {
        assert!(is_directory(&Mime("inode/directory".into())));
        assert!(!is_directory(&Mime("text/plain".into())));
    }
}
