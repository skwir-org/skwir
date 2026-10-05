//! `file://` URIs, written the way the desktop writes them.
//!
//! One escaping rule for every URI bibli hands to another program or uses as
//! a key: `GLib`'s `g_filename_to_uri`, which GNOME, tumbler, GTK bookmarks and
//! most thumbnailers use. The thumbnail cache is keyed by the MD5 of this
//! string, so a single byte of difference means never finding the desktop's
//! thumbnails, and the desktop never finding ours.
//!
//! Paths are bytes: both directions are lossless for names that are not
//! UTF-8.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC};

/// What `GLib` escapes in a path (`UNSAFE_PATH` in `gconvert.c`): everything
/// but letters, digits, `-._~/` and `!$&'()*+,:=@`. `;` is escaped.
const PATH: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'/')
    .remove(b'!')
    .remove(b'$')
    .remove(b'&')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b'+')
    .remove(b',')
    .remove(b':')
    .remove(b'=')
    .remove(b'@');

/// The `file://` URI of an absolute path, escaped as `g_filename_to_uri` does.
pub fn file_uri(path: &Path) -> String {
    let encoded = percent_encoding::percent_encode(os_bytes(path.as_os_str()), PATH);
    format!("file://{encoded}")
}

/// The path a `file://` URI names on this machine: with no host,
/// `localhost`, or this machine's name, as `ls --hyperlink` writes it.
/// `None` for another scheme or another host.
pub fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let (host, path) = rest.split_at(rest.find('/')?);
    (host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || hostname().is_some_and(|name| host.eq_ignore_ascii_case(name)))
    .then(|| PathBuf::from(os_string(percent_decode(path))))
}

/// This machine's name, as `file://` URIs name it: the kernel's, read once.
fn hostname() -> Option<&'static str> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .ok()
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
    })
    .as_deref()
}

/// Decode `%XX` escapes into the bytes they stand for; a `%` not followed by
/// two hex digits is kept as it is.
pub fn percent_decode(s: &str) -> Vec<u8> {
    percent_encoding::percent_decode_str(s).collect()
}

#[cfg(unix)]
fn os_bytes(s: &std::ffi::OsStr) -> &[u8] {
    std::os::unix::ffi::OsStrExt::as_bytes(s)
}

#[cfg(not(unix))]
fn os_bytes(s: &std::ffi::OsStr) -> &[u8] {
    s.as_encoded_bytes()
}

#[cfg(unix)]
fn os_string(bytes: Vec<u8>) -> OsString {
    std::os::unix::ffi::OsStringExt::from_vec(bytes)
}

#[cfg(not(unix))]
fn os_string(bytes: Vec<u8>) -> OsString {
    String::from_utf8_lossy(&bytes).into_owned().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_uri_of_this_machine_is_a_path() {
        assert_eq!(path_of("file:///etc/hosts"), Some("/etc/hosts".into()));
        assert_eq!(path_of("file://localhost/a%20b"), Some("/a b".into()));
        if let Some(host) = hostname() {
            assert_eq!(
                path_of(&format!("file://{host}/srv/x")),
                Some("/srv/x".into())
            );
        }
        assert_eq!(path_of("file://elsewhere.invalid/srv/x"), None);
        assert_eq!(path_of("https://a.b/c"), None);
    }

    #[test]
    fn punctuation_is_escaped_as_glib_does() {
        // What `gio list -u` prints for this name.
        let path = Path::new("/x/a (1) b&c'd!e+f,g;h=i@j:k$l*m~n#o?p%q[r]\"s<t>u^v`w{x}y|z é");
        let uri = file_uri(path);
        assert_eq!(
            uri,
            "file:///x/a%20(1)%20b&c'd!e+f,g%3Bh=i@j:k$l*m~n%23o%3Fp%25q%5Br%5D%22s%3Ct%3Eu%5Ev%60w%7Bx%7Dy%7Cz%20%C3%A9"
        );
        assert_eq!(path_of(&uri).as_deref(), Some(path));
    }

    #[test]
    fn only_local_file_uris_name_a_path() {
        assert_eq!(
            path_of("file://localhost/srv/a%20b").as_deref(),
            Some(Path::new("/srv/a b"))
        );
        assert_eq!(path_of("file://host/srv"), None);
        assert_eq!(path_of("sftp://host/srv"), None);
        assert_eq!(percent_decode("a%20b%zz%4"), b"a b%zz%4");
    }

    #[cfg(unix)]
    #[test]
    fn names_that_are_not_utf8_round_trip() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/x/caf\xe9"));
        let uri = file_uri(path);
        assert_eq!(uri, "file:///x/caf%E9");
        assert_eq!(path_of(&uri).as_deref(), Some(path));
    }
}
