//! `recently-used.xbel`: the freedesktop recent-files list most desktop
//! applications write. Read-only here.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One recently used file.
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub path: PathBuf,
    pub mime: Option<String>,
    /// Last visit (or modification, or addition) time.
    pub visited: SystemTime,
}

/// The default file location.
pub fn default_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("recently-used.xbel"))
}

/// Decode `%XX` escapes; invalid sequences are kept verbatim.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 && i + 2 <= bytes.len() - 1 + 1 {
            if let (Some(h), Some(l)) = (
                bytes.get(i + 1).and_then(|b| (*b as char).to_digit(16)),
                bytes.get(i + 2).and_then(|b| (*b as char).to_digit(16)),
            ) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `file:///a/b%20c` → `/a/b c`. Non-file URIs yield `None`.
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Strip an optional host (`file://localhost/x`).
    let rest = if rest.starts_with('/') {
        rest
    } else {
        let slash = rest.find('/')?;
        &rest[slash..]
    };
    Some(PathBuf::from(percent_decode(rest)))
}

/// Parse the document. Entries are returned most recently visited first.
pub fn parse(xml: &str) -> Vec<Recent> {
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for b in doc.descendants().filter(|n| n.has_tag_name("bookmark")) {
        let Some(href) = b.attribute("href") else {
            continue;
        };
        let Some(path) = file_uri_to_path(href) else {
            continue;
        };
        let time = ["visited", "modified", "added"]
            .iter()
            .filter_map(|a| b.attribute(*a))
            .find_map(|t| humantime::parse_rfc3339(t).ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mime = b
            .descendants()
            .find(|n| n.has_tag_name("mime-type"))
            .and_then(|n| n.attribute("type"))
            .map(str::to_owned);
        out.push(Recent {
            path,
            mime,
            visited: time,
        });
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.visited));
    out
}

/// Parse the file at `path`; missing or unreadable yields an empty list.
pub fn load(path: &Path) -> Vec<Recent> {
    std::fs::read_to_string(path)
        .map(|s| parse(&s))
        .unwrap_or_default()
}

/// Entries whose file still exists.
pub fn existing(items: Vec<Recent>) -> Vec<Recent> {
    items.into_iter().filter(|r| r.path.exists()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks" xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
  <bookmark href="file:///tmp/a%20b/photo.jpg" added="2025-12-14T12:07:47.042000Z" modified="2026-02-24T13:21:03.338000Z" visited="2026-02-24T13:21:03.338000Z">
    <info><metadata owner="http://freedesktop.org"><mime:mime-type type="image/jpeg"/></metadata></info>
  </bookmark>
  <bookmark href="file:///tmp/notes.txt" added="2026-03-01T08:00:00Z">
    <info><metadata owner="http://freedesktop.org"><mime:mime-type type="text/plain"/></metadata></info>
  </bookmark>
  <bookmark href="https://example.com/x" added="2026-03-02T08:00:00Z"/>
</xbel>"#;

    #[test]
    fn parses_paths_mimes_and_orders_by_recency() {
        let r = parse(FIXTURE);
        assert_eq!(r.len(), 2, "non-file URIs are skipped");
        assert_eq!(r[0].path, PathBuf::from("/tmp/notes.txt"));
        assert_eq!(r[0].mime.as_deref(), Some("text/plain"));
        assert_eq!(r[1].path, PathBuf::from("/tmp/a b/photo.jpg"));
        assert_eq!(r[1].mime.as_deref(), Some("image/jpeg"));
        assert!(r[0].visited > r[1].visited);
    }

    #[test]
    fn decodes_percent_escapes_and_hosts() {
        assert_eq!(percent_decode("a%20b%C3%A9%zz"), "a bé%zz");
        assert_eq!(
            file_uri_to_path("file://localhost/x/y").unwrap(),
            PathBuf::from("/x/y")
        );
        assert!(file_uri_to_path("http://x/y").is_none());
    }

    #[test]
    fn garbage_yields_nothing() {
        assert!(parse("<not xml").is_empty());
        assert!(load(Path::new("/nonexistent/recently-used.xbel")).is_empty());
    }
}
