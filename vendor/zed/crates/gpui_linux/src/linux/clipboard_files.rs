//! Files on the clipboard as Linux file managers put them there, in the two
//! families that do not read each other: GTK's (`x-special/gnome-copied-files`,
//! MATE's under its own name: `copy` or `cut`, then one URI a line) and KDE's
//! (`text/uri-list`, with `application/x-kde-cutselection` = `1` for a cut).
//! Both are offered, and both are read.
//!
//! A path is a URI byte for byte (`file:///lat%E9.txt`), never through text:
//! a name that is not UTF-8 stays the file it names.

use std::path::Path;

use gpui::ClipboardFiles;
use http_client::Url;

pub(crate) const GNOME_COPIED_FILES: &str = "x-special/gnome-copied-files";
pub(crate) const MATE_COPIED_FILES: &str = "x-special/mate-copied-files";
pub(crate) const URI_LIST: &str = "text/uri-list";
pub(crate) const KDE_CUT_SELECTION: &str = "application/x-kde-cutselection";

/// The `file://` URI of `path`, its bytes percent-encoded as they are.
fn uri_of(path: &Path) -> Option<String> {
    Url::from_file_path(path).ok().map(String::from)
}

fn uris(files: &ClipboardFiles) -> impl Iterator<Item = String> + '_ {
    files.paths.iter().filter_map(|path| uri_of(path))
}

/// GTK's list (and MATE's, the same bytes): `copy` or `cut`, then each URI
/// on a line of its own, with no newline after the last (Nautilus 44 refuses
/// an empty line).
pub(crate) fn copied_files(files: &ClipboardFiles) -> Vec<u8> {
    let mut out = String::from(if files.cut { "cut" } else { "copy" });
    for uri in uris(files) {
        out.push('\n');
        out.push_str(&uri);
    }
    out.into_bytes()
}

/// RFC 2483's list: each URI ended by CRLF.
pub(crate) fn uri_list(files: &ClipboardFiles) -> Vec<u8> {
    let mut out = String::new();
    for uri in uris(files) {
        out.push_str(&uri);
        out.push_str("\r\n");
    }
    out.into_bytes()
}

/// KDE's word that the files were cut.
pub(crate) const KDE_CUT: &[u8] = b"1";

/// What a type of the files offers, for one asking for `mime_type`; `None`
/// for a type that is not the files' (text is the item's own).
pub(crate) fn bytes_for(files: &ClipboardFiles, mime_type: &str) -> Option<Vec<u8>> {
    match mime_type {
        GNOME_COPIED_FILES | MATE_COPIED_FILES => Some(copied_files(files)),
        URI_LIST => Some(uri_list(files)),
        KDE_CUT_SELECTION if files.cut => Some(KDE_CUT.to_vec()),
        _ => None,
    }
}

/// The types offered for `files`, before the text's.
#[cfg_attr(not(feature = "wayland"), allow(dead_code))]
pub(crate) fn mime_types(files: &ClipboardFiles) -> Vec<&'static str> {
    let mut types = vec![GNOME_COPIED_FILES, MATE_COPIED_FILES, URI_LIST];
    if files.cut {
        types.push(KDE_CUT_SELECTION);
    }
    types
}

/// The lines of a list: CRLF or LF, empty lines and RFC 2483's comments left
/// out.
fn lines(bytes: &[u8]) -> impl Iterator<Item = &str> {
    std::str::from_utf8(bytes)
        .unwrap_or_default()
        .split('\n')
        .map(|line| line.trim_end_matches('\r').trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

/// Files from URIs: those of `file://` with no other host, the rest said.
fn files_of<'a>(uris: impl Iterator<Item = &'a str>, cut: bool) -> ClipboardFiles {
    let mut files = ClipboardFiles {
        cut,
        ..ClipboardFiles::default()
    };
    for uri in uris {
        match Url::parse(uri)
            .ok()
            .filter(|url| url.scheme() == "file")
            .and_then(|url| url.to_file_path().ok())
        {
            Some(path) => files.paths.push(path),
            None => files.others.push(uri.to_owned()),
        }
    }
    files
}

/// GTK's (or MATE's) list; `None` when its first line is neither `copy`
/// nor `cut`.
pub(crate) fn parse_copied_files(bytes: &[u8]) -> Option<ClipboardFiles> {
    let mut lines = lines(bytes);
    let cut = match lines.next()? {
        "cut" => true,
        "copy" => false,
        _ => return None,
    };
    Some(files_of(lines, cut))
}

/// A URI list, cut when KDE's flag says so.
pub(crate) fn parse_uri_list(bytes: &[u8], cut: bool) -> ClipboardFiles {
    files_of(lines(bytes), cut)
}

/// Whether KDE's flag says cut.
pub(crate) fn kde_cut(bytes: &[u8]) -> bool {
    bytes.trim_ascii() == KDE_CUT
}

/// What the clipboard's lists say: GTK's (or MATE's), else the URI list with
/// KDE's flag. Where GTK's names what is no local file (a GVfs location),
/// the URI list, which GTK writes with the local paths of such locations,
/// takes its place when it names fewer.
pub(crate) fn files_from(
    copied: Option<&[u8]>,
    uri_list: Option<&[u8]>,
    kde_flag: Option<&[u8]>,
) -> Option<ClipboardFiles> {
    let listed = uri_list.map(|bytes| parse_uri_list(bytes, kde_flag.is_some_and(kde_cut)));
    let files = match copied.and_then(parse_copied_files) {
        Some(mut files) => {
            if let Some(listed) = listed
                && !files.others.is_empty()
                && listed.others.len() < files.others.len()
            {
                files.paths = listed.paths;
                files.others = listed.others;
            }
            files
        }
        None => listed?,
    };
    (!files.paths.is_empty() || !files.others.is_empty()).then_some(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::ffi::OsStrExt, path::PathBuf};

    fn files(paths: &[&[u8]], cut: bool) -> ClipboardFiles {
        ClipboardFiles {
            paths: paths
                .iter()
                .map(|p| PathBuf::from(std::ffi::OsStr::from_bytes(p)))
                .collect(),
            cut,
            others: Vec::new(),
        }
    }

    #[test]
    fn lists_are_written_as_each_family_reads_them() {
        let copied = files(&[b"/tmp/a b.txt", b"/tmp/it's caf\xc3\xa9.txt"], true);
        assert_eq!(
            String::from_utf8(copied_files(&copied)).unwrap(),
            "cut\nfile:///tmp/a%20b.txt\nfile:///tmp/it's%20caf%C3%A9.txt",
            "no newline after the last"
        );
        assert_eq!(
            String::from_utf8(uri_list(&copied)).unwrap(),
            "file:///tmp/a%20b.txt\r\nfile:///tmp/it's%20caf%C3%A9.txt\r\n"
        );
        assert_eq!(
            bytes_for(&copied, KDE_CUT_SELECTION).as_deref(),
            Some(KDE_CUT)
        );
        assert!(mime_types(&copied).contains(&KDE_CUT_SELECTION));
        let kept = files(&[b"/tmp/a"], false);
        assert_eq!(copied_files(&kept), b"copy\nfile:///tmp/a");
        assert_eq!(
            bytes_for(&kept, KDE_CUT_SELECTION),
            None,
            "a copy has no flag"
        );
        assert!(!mime_types(&kept).contains(&KDE_CUT_SELECTION));
        assert_eq!(
            bytes_for(&kept, "text/plain"),
            None,
            "text is the item's own"
        );
    }

    #[test]
    fn a_name_that_is_not_utf8_goes_byte_for_byte() {
        let latin1 = files(&[b"/tmp/lat\xe9.txt"], false);
        assert_eq!(copied_files(&latin1), b"copy\nfile:///tmp/lat%E9.txt");
        let back = parse_copied_files(&copied_files(&latin1)).unwrap();
        assert_eq!(back.paths, latin1.paths);
    }

    #[test]
    fn what_other_file_managers_wrote_is_read() {
        // Thunar's cut, as it wrote it on 2026-10-16.
        let thunar = b"cut\nfile:///tmp/sub%20folder\nfile:///tmp/lat%E9.txt";
        let read = files_from(Some(thunar), None, None).unwrap();
        assert!(read.cut);
        assert_eq!(
            read.paths,
            files(&[b"/tmp/sub folder", b"/tmp/lat\xe9.txt"], true).paths
        );
        // PCManFM-Qt's ends with a newline; Dolphin writes CRLF and a flag.
        let pcmanfm = b"copy\nfile:///tmp/a%20b.txt\n";
        assert_eq!(
            files_from(Some(pcmanfm), None, None).unwrap().paths.len(),
            1
        );
        let dolphin = b"file:///tmp/a%20b.txt\r\n# a comment\r\n\r\n";
        let read = files_from(None, Some(dolphin), Some(b"1")).unwrap();
        assert!(read.cut);
        assert_eq!(read.paths, [PathBuf::from("/tmp/a b.txt")]);
        assert!(!files_from(None, Some(dolphin), None).unwrap().cut);
    }

    #[test]
    fn what_is_no_local_file_is_said_apart() {
        let remote = b"copy\nsftp://host/a.txt\nfile://elsewhere/b.txt\nfile:///c.txt";
        let read = files_from(Some(remote), None, None).unwrap();
        assert_eq!(read.paths, [PathBuf::from("/c.txt")]);
        assert_eq!(read.others, ["sftp://host/a.txt", "file://elsewhere/b.txt"]);
        // GTK lists a GVfs location's local path in its URI list.
        let fuse = b"file:///run/user/1000/gvfs/sftp:host=host/a.txt\r\nfile:///c.txt\r\n";
        let read = files_from(
            Some(b"copy\nsftp://host/a.txt\nfile:///c.txt"),
            Some(fuse),
            None,
        )
        .unwrap();
        assert!(read.others.is_empty());
        assert_eq!(read.paths.len(), 2);
    }

    #[test]
    fn what_is_no_list_of_files_is_none() {
        assert_eq!(parse_copied_files(b"hello\nfile:///a"), None);
        assert_eq!(files_from(Some(b"copy"), None, None), None, "nothing named");
        assert_eq!(files_from(None, None, None), None);
        assert!(!kde_cut(b"0"));
        assert!(kde_cut(b"1\n"));
    }
}
