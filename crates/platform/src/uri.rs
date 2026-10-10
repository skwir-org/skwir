//! `file://` URIs, written the way the desktop writes them.
//!
//! One escaping rule for every URI skwir hands to another program or uses as
//! a key: `GLib`'s `g_filename_to_uri`, which GNOME, tumbler, GTK bookmarks and
//! most thumbnailers use. The thumbnail cache is keyed by the MD5 of this
//! string, so a single byte of difference means never finding the desktop's
//! thumbnails, and the desktop never finding ours.
//!
//! Paths are bytes: both directions are lossless for names that are not
//! UTF-8. On Windows a path is written as `GLib` writes it there:
//! `file:///C:/…` for a drive, `file://server/share/…` for a share.

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
    #[cfg(windows)]
    if let Some(path) = path.to_str() {
        return windows_file_uri(path);
    }
    let encoded = percent_encoding::percent_encode(path.as_os_str().as_encoded_bytes(), PATH);
    format!("file://{encoded}")
}

/// [`file_uri`] of a Windows path: `C:\a b` is `file:///C:/a%20b`,
/// `\\server\share\x` is `file://server/share/x`. Tested everywhere.
#[cfg(any(windows, test))]
fn windows_file_uri(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    if let Some(share) = slashed.strip_prefix("//") {
        let (host, rest) = share.split_at(share.find('/').unwrap_or(share.len()));
        return format!(
            "file://{host}{}",
            percent_encoding::percent_encode(rest.as_bytes(), PATH)
        );
    }
    format!(
        "file:///{}",
        percent_encoding::percent_encode(slashed.as_bytes(), PATH)
    )
}

/// The path a `file://` URI names on this machine: with no host,
/// `localhost`, or this machine's name, as `ls --hyperlink` writes it.
/// `None` for another scheme or another host (on Windows, another host
/// is a share: `\\server\share`).
pub fn path_of(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let (host, path) = rest.split_at(rest.find('/')?);
    let here = host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || hostname().is_some_and(|name| host.eq_ignore_ascii_case(name));
    #[cfg(windows)]
    {
        windows_path_of(host, path, here).map(PathBuf::from)
    }
    #[cfg(not(windows))]
    here.then(|| PathBuf::from(skwir_core::os_path::from_bytes(percent_decode(path))))
}

/// [`path_of`] on Windows, for a URI's `host` and `path` (`/C:/a%20b`):
/// a drive's path here, a share's elsewhere. Tested everywhere.
#[cfg(any(windows, test))]
fn windows_path_of(host: &str, path: &str, here: bool) -> Option<String> {
    let path = String::from_utf8(percent_decode(path))
        .ok()?
        .replace('/', "\\");
    let drive = path.strip_prefix('\\').filter(|p| {
        let b = p.as_bytes();
        b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    });
    match drive {
        Some(drive) if here => Some(if drive.len() == 2 {
            format!("{drive}\\")
        } else {
            drive.to_owned()
        }),
        None if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") => {
            Some(format!(r"\\{host}{path}"))
        }
        _ => None,
    }
}

/// This machine's name, as `file://` URIs name it, read once: the
/// kernel's on Unix, `COMPUTERNAME` on Windows.
fn hostname() -> Option<&'static str> {
    static NAME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        #[cfg(target_os = "linux")]
        let name = std::fs::read_to_string("/proc/sys/kernel/hostname").ok();
        #[cfg(all(unix, not(target_os = "linux")))]
        let name = Some(
            rustix::system::uname()
                .nodename()
                .to_string_lossy()
                .into_owned(),
        );
        #[cfg(not(unix))]
        let name = std::env::var("COMPUTERNAME").ok();
        name.map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
    })
    .as_deref()
}

/// Decode `%XX` escapes into the bytes they stand for; a `%` not followed by
/// two hex digits is kept as it is.
pub fn percent_decode(s: &str) -> Vec<u8> {
    percent_encoding::percent_decode_str(s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[test]
    fn windows_paths_are_written_as_glib_writes_them_there() {
        assert_eq!(
            windows_file_uri(r"C:\Users\it's me\a b#1.txt"),
            "file:///C:/Users/it's%20me/a%20b%231.txt"
        );
        assert_eq!(
            windows_file_uri(r"\\server\share\x y"),
            "file://server/share/x%20y"
        );
        assert_eq!(
            windows_path_of("", "/C:/Users/it's%20me", true).as_deref(),
            Some(r"C:\Users\it's me")
        );
        assert_eq!(windows_path_of("", "/D:", true).as_deref(), Some(r"D:\"));
        assert_eq!(
            windows_path_of("server", "/share/x%20y", false).as_deref(),
            Some(r"\\server\share\x y")
        );
        assert_eq!(windows_path_of("", "/no-drive", true), None);
        assert_eq!(windows_path_of("server", "/C:/x", false), None);
    }
}
