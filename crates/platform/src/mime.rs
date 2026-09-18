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
pub fn by_name(name: &OsStr) -> Option<Mime> {
    imp::by_name(name)
}

/// Sniff the content (reads the head of the file).
pub fn by_content(path: &Path) -> Option<Mime> {
    imp::by_content(path)
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
        let mut v = db().get_mime_types_from_file_name(name);
        if v.is_empty() {
            return None;
        }
        Some(to_mime(&v.remove(0)))
    }

    pub fn by_content(path: &Path) -> Option<Mime> {
        let guess = db().guess_mime_type().path(path).guess();
        // application/octet-stream with uncertainty means "no idea".
        (!(guess.uncertain() && guess.mime_type().essence_str() == "application/octet-stream"))
            .then(|| to_mime(guess.mime_type()))
    }

    pub fn generic_icon_name(m: &Mime) -> String {
        parse(m)
            .and_then(|p| db().lookup_generic_icon_name(&p))
            .unwrap_or_else(|| format!("{}-x-generic", m.top_level()))
    }

    pub fn description(_m: &Mime) -> Option<String> {
        // xdg-mime 0.4 does not expose the <comment> element.
        None
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
    fn directory_check() {
        assert!(is_directory(&Mime("inode/directory".into())));
        assert!(!is_directory(&Mime("text/plain".into())));
    }
}
