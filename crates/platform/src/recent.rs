//! The desktop's recently used files (`recently-used.xbel`, the
//! freedesktop desktop bookmark specification): what the desktop's own
//! Open and Save dialogs add to as a file is chosen, which Skwir's add to
//! too (ADR-144, 7). Read by the jump plugin's Recent place.
//!
//! The file is the desktop's, written by every program: what Skwir adds is
//! edited into it as it stands (a bookmark's times and its application's
//! count, or a bookmark more), the rest kept byte for byte. One that does
//! not read as a bookmark file is left as it is, for its programs to mend.

use std::ops::Range;
use std::path::{Path, PathBuf};

/// How Skwir names itself in a bookmark's applications, and how a file is
/// opened with it.
const APPLICATION: &str = "skwir";
const EXEC: &str = "'skwir %u'";

/// Where the list is: `$XDG_DATA_HOME/recently-used.xbel`.
pub fn path() -> Option<PathBuf> {
    crate::xdg::data_home().map(|data| data.join("recently-used.xbel"))
}

/// Whether the desktop remembers the files used: not where GTK is told
/// to remember none (`gtk-recent-files-enabled` false, or a
/// `gtk-recent-files-max-age` of 0, in its `settings.ini`), nor where
/// GNOME's privacy settings say so (`remember-recent-files`), as GTK's own
/// dialog reads them. Runs `gsettings` on a GNOME desktop: on a worker.
/// Elsewhere than Linux, never: no program there keeps this list, and no
/// dialog of another program is Skwir's (ADR-141, 11).
#[cfg(target_os = "linux")]
pub fn remembered() -> bool {
    let off = |value: &str| matches!(value.to_ascii_lowercase().as_str(), "false" | "0");
    if crate::desktop_settings::gtk_settings("gtk-recent-files-enabled")
        .first()
        .is_some_and(|value| off(value))
        || crate::desktop_settings::gtk_settings("gtk-recent-files-max-age")
            .first()
            .is_some_and(|value| value.trim() == "0")
    {
        return false;
    }
    let gnome = std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktops| desktops.split(':').any(|d| d.eq_ignore_ascii_case("GNOME")));
    !(gnome
        && crate::desktop_settings::gsettings("org.gnome.desktop.privacy", "remember-recent-files")
            .is_some_and(|value| value.trim() == "false"))
}

/// See the Linux one: elsewhere, never.
#[cfg(not(target_os = "linux"))]
pub fn remembered() -> bool {
    false
}

/// Add `paths`, chosen now, to the desktop's recently used files: a file
/// the list names is visited and modified now, and Skwir's count of it
/// goes up; any other is added. Nothing is written where the desktop
/// remembers none (`remembered`), nor where the list does not read as a
/// bookmark file; a list that is a link is written where it leads
/// (`file::rewrite_words`). Reads and writes the disk: on a worker.
///
/// # Errors
/// When the list cannot be read or written.
pub fn add(paths: &[PathBuf]) -> anyhow::Result<()> {
    if paths.is_empty() || !remembered() {
        return Ok(());
    }
    let Some(target) = path() else {
        return Ok(());
    };
    let now = jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%S%.6fZ")
        .to_string();
    let entries: Vec<(PathBuf, Option<String>)> = paths
        .iter()
        .map(|path| {
            let mime = path
                .file_name()
                .and_then(crate::mime::by_name)
                .map(|mime| mime.as_str().to_owned());
            (path.clone(), mime)
        })
        .collect();
    let read_only = skwir_core::words!(
        recent_read_only =
            "The list of recently used files is read-only; change it where it is managed",
        note = "Why a file chosen through another program's dialog was not added to the desktop's recently used files: the list is a link to a file that cannot be written (a dotfile manager's)."
    );
    crate::file::rewrite_words(&target, read_only, |text| {
        let written = added(Some(text), &entries, &now);
        if written.is_none() {
            tracing::warn!(
                "the recently used files do not read as a bookmark file: left as they are"
            );
        }
        written.map(|written| (written, ()))
    })?;
    Ok(())
}

/// `text` (the list as it is, `None` where there is none) with `entries`
/// (each file and its type) used at `now`: `None` where the text does not
/// read as a bookmark file. Pure: what `add` writes.
fn added(text: Option<&str>, entries: &[(PathBuf, Option<String>)], now: &str) -> Option<String> {
    let text = match text {
        Some(text) if !text.trim().is_empty() => text,
        _ => {
            let mut fresh = String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\"\n      \
                 xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\"\n      \
                 xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\"\n>\n",
            );
            for (path, mime) in entries {
                fresh.push_str(&bookmark(path, mime.as_deref(), now));
            }
            fresh.push_str("</xbel>\n");
            return Some(fresh);
        }
    };
    crate::inspection::xml_depth(text.as_bytes()).ok()?;
    let doc = roxmltree::Document::parse(text).ok()?;
    let root = doc.root_element();
    if !root.has_tag_name("xbel") {
        return None;
    }
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut appended = String::new();
    for (path, mime) in entries {
        let named = root.children().find(|node| {
            node.has_tag_name("bookmark")
                && node
                    .attribute("href")
                    .and_then(crate::uri::path_of)
                    .is_some_and(|href| href == *path)
        });
        match named {
            Some(node) => edits.extend(visited(node, text, now)),
            None => appended.push_str(&bookmark(path, mime.as_deref(), now)),
        }
    }
    if !appended.is_empty() {
        let end = closing_tag_start(root, text)?;
        edits.push((end..end, appended));
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut written = text.to_owned();
    for (range, with) in edits {
        written.replace_range(range, &with);
    }
    Some(written)
}

/// The edits that make a bookmark visited and modified `now` by Skwir:
/// its times, and its application's (Skwir's count up one, or Skwir added
/// to its applications).
fn visited(node: roxmltree::Node, text: &str, now: &str) -> Vec<(Range<usize>, String)> {
    let mut edits = Vec::new();
    let mut times = |element: roxmltree::Node, names: &[&str], after: &str| {
        for name in names {
            match element.attributes().find(|a| a.name() == *name) {
                Some(attribute) => edits.push((attribute.range_value(), now.to_owned())),
                None => {
                    if let Some(at) = element.attributes().find(|a| a.name() == after) {
                        let end = at.range().end;
                        edits.push((end..end, format!(" {name}=\"{now}\"")));
                    }
                }
            }
        }
    };
    times(node, &["modified", "visited"], "href");
    let applications = node
        .descendants()
        .find(|n| n.tag_name().name() == "applications");
    let ours = applications.and_then(|apps| {
        apps.children().find(|n| {
            n.tag_name().name() == "application" && n.attribute("name") == Some(APPLICATION)
        })
    });
    match (applications, ours) {
        (_, Some(ours)) => {
            match ours.attributes().find(|a| a.name() == "count") {
                Some(count) => {
                    let more = count.value().trim().parse::<u64>().unwrap_or(0) + 1;
                    edits.push((count.range_value(), more.to_string()));
                }
                None => {
                    if let Some(name) = ours.attributes().find(|a| a.name() == "name") {
                        let end = name.range().end;
                        edits.push((end..end, " count=\"1\"".to_owned()));
                    }
                }
            }
            match ours.attributes().find(|a| a.name() == "modified") {
                Some(modified) => edits.push((modified.range_value(), now.to_owned())),
                None => {
                    if let Some(name) = ours.attributes().find(|a| a.name() == "name") {
                        let end = name.range().end;
                        edits.push((end..end, format!(" modified=\"{now}\"")));
                    }
                }
            }
        }
        (Some(apps), None) => {
            if let Some(end) = closing_tag_start(apps, text) {
                // Written with the prefix the file gives the namespace.
                let prefix = apps
                    .tag_name()
                    .namespace()
                    .and_then(|ns| apps.lookup_prefix(ns))
                    .map_or_else(String::new, |p| format!("{p}:"));
                edits.push((end..end, application(&prefix, now)));
            }
        }
        (None, None) => {}
    }
    edits
}

/// Where `element`'s closing tag starts: its end tag's `</`, in `text`.
fn closing_tag_start(element: roxmltree::Node, text: &str) -> Option<usize> {
    let range = element.range();
    let inside = text.get(range.clone())?;
    if inside.ends_with("/>") {
        return None;
    }
    inside.rfind("</").map(|at| range.start + at)
}

/// A bookmark of `path`, of type `mime`, added now by Skwir.
fn bookmark(path: &Path, mime: Option<&str>, now: &str) -> String {
    let href = escaped(&crate::uri::file_uri(path));
    let mime = mime.map_or_else(String::new, |mime| {
        format!("        <mime:mime-type type=\"{}\"/>\n", escaped(mime))
    });
    format!(
        "  <bookmark href=\"{href}\" added=\"{now}\" modified=\"{now}\" visited=\"{now}\">\n    \
         <info>\n      <metadata owner=\"http://freedesktop.org\">\n{mime}        \
         <bookmark:applications>\n{}        </bookmark:applications>\n      </metadata>\n    \
         </info>\n  </bookmark>\n",
        application("bookmark:", now)
    )
}

/// Skwir as a bookmark's application, used once `now`.
fn application(prefix: &str, now: &str) -> String {
    format!(
        "          <{prefix}application name=\"{}\" exec=\"{}\" modified=\"{now}\" count=\"1\"/>\n",
        escaped(APPLICATION),
        escaped(EXEC)
    )
}

/// `text` as an attribute's value says it.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

// The list is written on Linux alone (`remembered`), of paths as Linux
// writes them.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-20T10:00:00.000000Z";

    fn read(text: &str) -> Vec<(PathBuf, String, Vec<(String, String)>)> {
        let doc = roxmltree::Document::parse(text).expect("it reads");
        doc.root_element()
            .children()
            .filter(|n| n.has_tag_name("bookmark"))
            .map(|b| {
                let apps = b
                    .descendants()
                    .filter(|n| n.tag_name().name() == "application")
                    .map(|a| {
                        (
                            a.attribute("name").unwrap_or_default().to_owned(),
                            a.attribute("count").unwrap_or_default().to_owned(),
                        )
                    })
                    .collect();
                (
                    crate::uri::path_of(b.attribute("href").unwrap()).unwrap(),
                    b.attribute("visited").unwrap_or_default().to_owned(),
                    apps,
                )
            })
            .collect()
    }

    /// A list made anew holds the files chosen, Skwir's once, a name's
    /// apostrophe and accents kept.
    #[test]
    fn a_new_list_holds_what_was_chosen() {
        let path = PathBuf::from("/tmp/l'été/notes & plans.txt");
        let written = added(None, &[(path.clone(), Some("text/plain".into()))], NOW).unwrap();
        assert_eq!(
            read(&written),
            [(path, NOW.to_owned(), vec![("skwir".into(), "1".into())])]
        );
        assert!(written.contains("type=\"text/plain\""));
    }

    /// A file the list names is visited again, Skwir's count up one, or
    /// Skwir added beside the program that used it; another file is
    /// added; the rest is kept byte for byte.
    #[test]
    fn a_list_keeps_what_others_wrote() {
        let before = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<xbel version=\"1.0\" xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\" xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\">\n\
  <bookmark href=\"file:///home/a/r%C3%A9sum%C3%A9.odt\" added=\"2026-01-01T00:00:00Z\" modified=\"2026-01-01T00:00:00Z\" visited=\"2026-01-01T00:00:00Z\">\n\
    <info>\n      <metadata owner=\"http://freedesktop.org\">\n\
        <mime:mime-type type=\"application/vnd.oasis.opendocument.text\"/>\n\
        <bookmark:applications>\n\
          <bookmark:application name=\"LibreOffice\" exec=\"&apos;soffice %u&apos;\" modified=\"2026-01-01T00:00:00Z\" count=\"4\"/>\n\
        </bookmark:applications>\n      </metadata>\n    </info>\n  </bookmark>\n\
  <bookmark href=\"file:///home/a/photo.png\" added=\"2026-01-02T00:00:00Z\" visited=\"2026-01-02T00:00:00Z\">\n\
    <info>\n      <metadata owner=\"http://freedesktop.org\">\n\
        <bookmark:applications>\n\
          <bookmark:application name=\"skwir\" exec=\"&apos;skwir %u&apos;\" modified=\"2026-01-02T00:00:00Z\" count=\"2\"/>\n\
        </bookmark:applications>\n      </metadata>\n    </info>\n  </bookmark>\n\
</xbel>\n";
        let entries = [
            (PathBuf::from("/home/a/résumé.odt"), None),
            (PathBuf::from("/home/a/photo.png"), None),
            (
                PathBuf::from("/home/a/new.txt"),
                Some("text/plain".to_owned()),
            ),
        ];
        let written = added(Some(before), &entries, NOW).unwrap();
        let after = read(&written);
        assert_eq!(after.len(), 3);
        assert_eq!(after[0].1, NOW);
        assert_eq!(
            after[0].2,
            [
                ("LibreOffice".into(), "4".into()),
                ("skwir".into(), "1".into())
            ]
        );
        assert_eq!(after[1].1, NOW);
        assert_eq!(after[1].2, [("skwir".into(), "3".into())]);
        assert!(written.contains("modified=\"2026-11-20T10:00:00.000000Z\" visited"));
        assert_eq!(after[2].0, PathBuf::from("/home/a/new.txt"));
        // What another program wrote stays as it was.
        assert!(written.contains(
            "<bookmark:application name=\"LibreOffice\" exec=\"&apos;soffice %u&apos;\" \
             modified=\"2026-01-01T00:00:00Z\" count=\"4\"/>"
        ));
        assert!(written.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel"));
    }

    /// A list that does not read as bookmarks is left alone.
    #[test]
    fn a_list_that_does_not_read_is_left_alone() {
        let entries = [(PathBuf::from("/home/a/x.txt"), None)];
        assert_eq!(added(Some("<xbel><bookmark"), &entries, NOW), None);
        assert_eq!(added(Some("<html></html>"), &entries, NOW), None);
        assert!(
            added(Some("   "), &entries, NOW).is_some(),
            "an empty one is made anew"
        );
    }
}
