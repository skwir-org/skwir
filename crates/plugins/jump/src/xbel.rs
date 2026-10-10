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
    skwir_platform::xdg::data_home().map(|d| d.join("recently-used.xbel"))
}

/// Parse the document. Entries are returned most recently visited first.
pub fn parse(xml: &str) -> Vec<Recent> {
    // Nested no deeper than the parser's recursion can go.
    if skwir_platform::inspection::xml_depth(xml.as_bytes()).is_err() {
        return Vec::new();
    }
    let Ok(doc) = roxmltree::Document::parse(xml) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for b in doc.descendants().filter(|n| n.has_tag_name("bookmark")) {
        let Some(href) = b.attribute("href") else {
            continue;
        };
        let Some(path) = skwir_platform::uri::path_of(href) else {
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

/// Entries whose file still exists. Stats each one: worker-only.
pub fn existing(items: Vec<Recent>) -> Vec<Recent> {
    items.into_iter().filter(|r| r.path.exists()).collect()
}

#[cfg(test)]
// Helpers of the tests that run on Linux alone (ADR-035).
#[cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]
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

    // The free desktops' recent files (step E, ADR-035).
    #[cfg(target_os = "linux")]
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

    // The free desktops' recent files (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn reads_local_hosts_and_raw_bytes() {
        let r = parse(
            r#"<xbel><bookmark href="file://localhost/x/y%20z" added="2026-03-01T08:00:00Z"/><bookmark href="file://elsewhere/x" added="2026-03-01T08:00:00Z"/><bookmark href="file:///x/caf%E9" added="2026-03-01T08:00:00Z"/></xbel>"#,
        );
        assert_eq!(r.len(), 2, "another host is not a local file");
        assert_eq!(r[0].path, PathBuf::from("/x/y z"));
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            assert_eq!(r[1].path.as_os_str().as_bytes(), b"/x/caf\xe9");
        }
    }

    #[test]
    fn garbage_yields_nothing() {
        assert!(parse("<not xml").is_empty());
        assert!(load(Path::new("/nonexistent/recently-used.xbel")).is_empty());
    }
}
