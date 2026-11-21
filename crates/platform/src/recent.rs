//! The desktop's recently used files (`recently-used.xbel`, the
//! freedesktop desktop bookmark specification): what the desktop's own
//! Open and Save dialogs add to as a file is chosen, which Skwir's add to
//! too (ADR-144, 7). Read by the jump plugin's Recent place.
//!
//! The file is the desktop's, written by every program and read by `GLib`'s
//! `GBookmarkFile`, which refuses the whole of a list it finds one thing
//! wrong in (a file named twice, a date or an attribute it does not know,
//! a namespace it cannot resolve): GTK's programs then start an empty list,
//! written over the old one at their next addition. So what Skwir adds is
//! edited into the list as it stands, the rest kept byte for byte (a
//! bookmark's times and Skwir's count of it, Skwir among its applications,
//! a bookmark more for a file it does not name, each file once), written
//! as `GLib` writes it, the list kept to the bookmarks GTK keeps
//! ([`KEPT`]). A list `GLib` would not read is left as it is, for its
//! programs to mend, and what Skwir would write is read again as `GLib`
//! reads it before it is (`glib_reads`): never one `GLib` refuses.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// How Skwir names itself in a bookmark's applications, and how a file is
/// opened with it (quoted as `GLib` quotes it, `g_shell_quote`).
const APPLICATION: &str = "skwir";
const EXEC: &str = "'skwir %u'";

/// The namespaces of a bookmark's applications and of its type, and the
/// owner of the metadata `GLib` reads.
const BOOKMARK_NS: &str = "http://www.freedesktop.org/standards/desktop-bookmarks";
const MIME_NS: &str = "http://www.freedesktop.org/standards/shared-mime-info";
const OWNER: &str = "http://freedesktop.org";

/// The type of a file no type is known for, as GTK's recent manager
/// writes it.
const UNKNOWN_TYPE: &str = "application/octet-stream";

/// The most bookmarks the list keeps, as GTK's recent manager keeps it
/// (its `MAX_LIST_SIZE`): those first in the list go first.
const KEPT: usize = 1000;

/// The longest list read: many times what GTK's thousand bookmarks make
/// (well under a megabyte); one longer is no list a program keeps, and is
/// left alone rather than read whole.
pub const LIMIT: u64 = 16 << 20;

/// A list with no bookmark yet, as `GLib` writes one.
const EMPTY: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\"\n      \
     xmlns:bookmark=\"http://www.freedesktop.org/standards/desktop-bookmarks\"\n      \
     xmlns:mime=\"http://www.freedesktop.org/standards/shared-mime-info\"\n></xbel>";

/// Where the list is: `$XDG_DATA_HOME/recently-used.xbel`.
pub fn path() -> Option<PathBuf> {
    crate::xdg::data_home().map(|data| data.join("recently-used.xbel"))
}

/// How the desktop remembers the files used, where it does
/// (`remembered`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Remembered {
    /// For how many days, where a setting says (GTK's and GNOME's age):
    /// one used longer ago goes, as GTK lets it go.
    pub days: Option<u32>,
}

/// Whether the desktop remembers the files used, and for how long: not
/// where one of its switches says to remember none, read as each program
/// that keeps the list reads its own, each where it is cheap to read:
/// GTK's `settings.ini` as GTK cascades them (`gtk_settings_files`; GTK
/// 4's and GTK 3's: `gtk-recent-files-enabled`,
/// `gtk-recent-files-max-age`), GNOME's privacy settings, which GTK 4
/// reads through the settings portal (`remember-recent-files`,
/// `recent-files-max-age`), Cinnamon's on Cinnamon, and KDE's
/// `kdeglobals`, which its own programs read before they add a file
/// (`[RecentDocuments]` `UseRecent`, `MaxEntries`), as `KConfig` reads
/// them (an administrator's lock over the user's word). One that says none
/// is enough; the shortest age is kept. Runs `gsettings`: on a worker.
/// Elsewhere than Linux, never: no program there keeps this list, and no
/// dialog of another program is Skwir's (ADR-141, 11).
#[cfg(target_os = "linux")]
pub fn remembered() -> Option<Remembered> {
    let gtk: Vec<Vec<String>> = ["gtk-4.0", "gtk-3.0"]
        .iter()
        .map(|version| {
            gtk_settings_files(
                version,
                crate::xdg::config_home(),
                &crate::xdg::config_dirs(),
            )
            .iter()
            .filter_map(|file| crate::file::read_regular_text(file, crate::file::DESKTOP_FILE).ok())
            .collect()
        })
        .collect();
    let schema = |schema: &str| {
        crate::desktop_settings::printed(Path::new("gsettings"), &["list-recursively", schema])
            .map_or_else(Said::default, |printed| gsettings_said(&printed))
    };
    let cinnamon = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktops| {
        desktops
            .split(':')
            .any(|d| d.eq_ignore_ascii_case("X-Cinnamon") || d.eq_ignore_ascii_case("Cinnamon"))
    });
    let mut said = gtk_said(&gtk)
        .and(schema("org.gnome.desktop.privacy"))
        .and(kde_said(&crate::desktop_settings::kdeglobals_cascade()));
    if cinnamon {
        said = said.and(schema("org.cinnamon.desktop.privacy"));
    }
    (!said.off).then_some(Remembered { days: said.days })
}

/// See the Linux one: elsewhere, never.
#[cfg(not(target_os = "linux"))]
pub fn remembered() -> Option<Remembered> {
    None
}

/// Whether Recent has something to give an Open dialog (ADR-144, 7): the
/// desktop remembers the files used (`remembered`), and its list names a
/// file that is there (one on a remote counted, unread: its server may
/// not answer, ADR-077). Runs `gsettings`, reads the list and stats: on a
/// thread of its own or a worker.
pub fn offered() -> bool {
    remembered().is_some()
        && path()
            .and_then(|list| crate::file::read_regular_text(&list, LIMIT).ok())
            .is_some_and(|text| names_one(&text))
}

/// Whether the list `text` names a file that is there, read as the jump
/// plugin's Recent reads it.
fn names_one(text: &str) -> bool {
    if crate::inspection::xml_depth(text.as_bytes()).is_err() {
        return false;
    }
    let Ok(doc) = roxmltree::Document::parse(text) else {
        return false;
    };
    doc.descendants()
        .filter(|node| node.has_tag_name("bookmark"))
        .filter_map(|node| crate::uri::path_of(node.attribute("href")?))
        .any(|path| skwir_core::fs::reach::of(&path).is_some() || path.exists())
}

/// Off as a `GLib` key file's boolean (GTK's `settings.ini`) and
/// `gsettings` say it: `false` or `0`, as written.
#[cfg(target_os = "linux")]
fn glib_off(value: &str) -> bool {
    matches!(value, "false" | "0")
}

/// Off as `KConfig`'s boolean says it (`kdeglobals`): `false`, `no`,
/// `off` or `0`, case ignored.
#[cfg(target_os = "linux")]
fn kconfig_off(value: &str) -> bool {
    ["false", "no", "off", "0"]
        .iter()
        .any(|word| value.eq_ignore_ascii_case(word))
}

/// What the desktop's switches say of remembering the files used (read
/// on Linux alone, `remembered`).
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Said {
    /// None remembered.
    off: bool,
    /// For so many days.
    days: Option<u32>,
}

#[cfg(target_os = "linux")]
impl Said {
    /// What both say: none where either says none, the shorter age.
    fn and(self, other: Said) -> Said {
        Said {
            off: self.off || other.off,
            days: match (self.days, other.days) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        }
    }

    /// A switch: off where it says so as its reader reads off
    /// ([`glib_off`], [`kconfig_off`]).
    fn switch(value: Option<&str>, off: fn(&str) -> bool) -> Said {
        Said {
            off: value.is_some_and(|value| off(value.trim())),
            days: None,
        }
    }

    /// An age in days, as GTK reads one: 0 remembers none, a negative one
    /// for ever.
    fn age(value: Option<&str>) -> Said {
        match value.and_then(|value| value.trim().parse::<i64>().ok()) {
            Some(0) => Said {
                off: true,
                days: None,
            },
            Some(days) if days > 0 => Said {
                off: false,
                days: u32::try_from(days).ok(),
            },
            _ => Said::default(),
        }
    }
}

/// GTK's `settings.ini`s of `version` (`gtk-3.0`), the one whose word
/// wins first: GTK loads its data prefix's (`/usr/share`, where the
/// system installs it), its `sysconfdir`'s (`/etc`), each of
/// `XDG_CONFIG_DIRS` (`config_dirs`) from the first, then the user's
/// (`config_home`), each one's keys replacing those loaded before
/// (`gtk_settings_init`, GTK 3 and 4 alike): the user's first here, then
/// `XDG_CONFIG_DIRS` from the last, `/etc`'s, the data prefix's.
#[cfg(target_os = "linux")]
fn gtk_settings_files(
    version: &str,
    config_home: Option<PathBuf>,
    config_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let file = Path::new(version).join("settings.ini");
    config_home
        .into_iter()
        .chain(config_dirs.iter().rev().cloned())
        .chain([PathBuf::from("/etc"), PathBuf::from("/usr/share")])
        .map(|folder| folder.join(&file))
        .collect()
}

/// What GTK's `settings.ini`s say, each GTK's (`versions`) in the order
/// whose word wins first (`gtk_settings_files`), the first that says a
/// key saying it.
#[cfg(target_os = "linux")]
fn gtk_said(versions: &[Vec<String>]) -> Said {
    let value = |texts: &[String], key: &str| {
        texts
            .iter()
            .find_map(|text| crate::desktop_settings::keyfile_value(text, "Settings", key))
    };
    versions.iter().fold(Said::default(), |said, texts| {
        said.and(Said::switch(
            value(texts, "gtk-recent-files-enabled").as_deref(),
            glib_off,
        ))
        .and(Said::age(
            value(texts, "gtk-recent-files-max-age").as_deref(),
        ))
    })
}

/// What a privacy schema says, as `gsettings list-recursively` prints it
/// (`schema key value` a line).
#[cfg(target_os = "linux")]
fn gsettings_said(printed: &str) -> Said {
    let value = |key: &str| {
        printed.lines().find_map(|line| {
            let mut words = line.split_whitespace();
            let (_, said, value) = (words.next()?, words.next()?, words.next()?);
            (said == key).then_some(value)
        })
    };
    Said::switch(value("remember-recent-files"), glib_off)
        .and(Said::age(value("recent-files-max-age")))
}

/// What KDE's `kdeglobals` say (`cascade`, the user's first), as
/// `KConfig` reads them: a system's lock (Kiosk's `[$i]`) over the user's
/// word, and its words for off.
#[cfg(target_os = "linux")]
fn kde_said(cascade: &[String]) -> Said {
    let value = |key: &str| crate::desktop_settings::kconfig_value(cascade, "RecentDocuments", key);
    let none = value("MaxEntries").is_some_and(|entries| entries.trim() == "0");
    Said::switch(value("UseRecent").as_deref(), kconfig_off).and(Said {
        off: none,
        days: None,
    })
}

/// Add `paths`, chosen now, to the desktop's recently used files, each
/// once: a file the list names is visited and modified now, and Skwir's
/// count of it goes up; any other is added, typed by what it is
/// (`type_of`). Nothing is written where the desktop remembers none
/// (`remembered`), nor where the list does not read as `GLib` reads one
/// (`added`); a list that is a link is written where it leads
/// (`file::rewrite_words_within`), one longer than [`LIMIT`] refused. One
/// answer's at a time in this program, the list read again for each: two
/// answers at once never write it back each with only their own. Reads
/// the files chosen and writes the disk: on a worker, in the files' lane.
///
/// # Errors
/// When the list cannot be read or written.
pub fn add(paths: &[PathBuf]) -> anyhow::Result<()> {
    static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    if paths.is_empty() {
        return Ok(());
    }
    let Some(remembering) = remembered() else {
        return Ok(());
    };
    let Some(target) = path() else {
        return Ok(());
    };
    let mut once = HashSet::new();
    let entries: Vec<(PathBuf, String)> = paths
        .iter()
        .filter(|path| once.insert(path.as_path()))
        .map(|path| (path.clone(), type_of(path)))
        .collect();
    let read_only = skwir_core::words!(
        recent_read_only =
            "The list of recently used files is read-only; change it where it is managed",
        note = "Why a file chosen through another program's dialog was not added to the desktop's recently used files: the list is a link to a file that cannot be written (a dotfile manager's)."
    );
    let _one = WRITING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let at = jiff::Timestamp::now();
    let now = date(at);
    // As GTK counts a file's age: whole days of 24 hours since it was
    // modified, one older than the age kept going.
    let before = remembering.days.and_then(|days| {
        at.checked_sub(jiff::SignedDuration::from_hours(24 * (i64::from(days) + 1)))
            .ok()
    });
    crate::file::rewrite_words_within(&target, LIMIT, read_only, |text| {
        let written = added(Some(text), &entries, &now, before);
        if written.is_none() {
            tracing::warn!(
                "the recently used files do not read as GLib reads them: left as they are"
            );
        }
        written.map(|written| (written, ()))
    })?;
    Ok(())
}

/// The type a bookmark of `path` says, as GTK's recent manager asks it of
/// the file: what it is whatever its name (a folder, a pipe), else what a
/// link leads to, by its name and, where that does not settle it, its
/// bytes; where nothing is there yet (a file about to be saved), the name
/// it is saved under. `application/octet-stream` where none says. Reads
/// the disk: on a worker.
fn type_of(path: &Path) -> String {
    let typed = match skwir_core::fs::stat::stat_path(path) {
        None => path.file_name().and_then(crate::mime::by_name),
        Some(entry) => match entry.inode_type() {
            Some(kind) => Some(crate::mime::Mime(kind.to_owned())),
            None => entry.content(path).and_then(|content| {
                let read = content.path;
                crate::mime::identify(read.file_name()?, read.as_path())
            }),
        },
    };
    typed.map_or_else(|| UNKNOWN_TYPE.to_owned(), |mime| mime.0)
}

/// `at` as `GLib` writes a date (`g_date_time_format_iso8601`): in UTC, its
/// microseconds only where there are any.
fn date(at: jiff::Timestamp) -> String {
    let seconds = at.strftime("%Y-%m-%dT%H:%M:%S");
    match at.subsec_microsecond() {
        0 => format!("{seconds}Z"),
        micros => format!("{seconds}.{micros:06}Z"),
    }
}

/// `text` (the list as it is, `None` or blank where there is none) with
/// `entries` (each file and its type) used at `now`, each file once: one
/// the list names visited, another added after the others; as GTK keeps
/// the list, one last modified `before` dropped, then the first past
/// [`KEPT`]: the list's first, never one visited now, and where more
/// files are used now than it keeps, the first of those added, as GTK's
/// clamp drops them. `None` where the text does not read
/// as `GLib` reads a list, or where what would be written would not. Pure:
/// what `add` writes.
fn added(
    text: Option<&str>,
    entries: &[(PathBuf, String)],
    now: &str,
    before: Option<jiff::Timestamp>,
) -> Option<String> {
    let text = match text {
        Some(text) if !text.trim().is_empty() => text,
        _ => EMPTY,
    };
    let doc = read(text)?;
    let root = doc.root_element();
    let bookmarks: Vec<roxmltree::Node> = elements(root)
        .filter(|node| is_xbel(*node, text, "bookmark"))
        .collect();
    // Each bookmark by the file it names, its address read once: two
    // addresses of one file (escaped apart), the first.
    let mut hrefs: HashSet<&str> = HashSet::with_capacity(bookmarks.len());
    let mut named: HashMap<PathBuf, usize> = HashMap::with_capacity(bookmarks.len());
    for (at, node) in bookmarks.iter().enumerate() {
        let href = node.attribute("href")?;
        hrefs.insert(href);
        if let Some(path) = crate::uri::path_of(href) {
            named.entry(path).or_insert(at);
        }
    }
    let mut edits = Edits::default();
    let prefixes = Prefixes::of(root, text, &mut edits)?;
    let mut used: HashSet<usize> = HashSet::new();
    let mut fresh: Vec<String> = Vec::new();
    let mut once = HashSet::new();
    for (path, mime) in entries {
        if !once.insert(path.as_path()) {
            continue;
        }
        if let Some(&at) = named.get(path) {
            if used.insert(at) {
                visit(bookmarks[at], text, now, &prefixes, &mut edits)?;
            }
            continue;
        }
        // An address the list holds that does not read as a path of this
        // machine (another host's) is that bookmark's: not two.
        let href = crate::uri::file_uri(path);
        if !hrefs.contains(href.as_str()) {
            fresh.push(bookmark(&href, mime, now, &prefixes));
        }
    }
    let aged: Vec<bool> = bookmarks
        .iter()
        .enumerate()
        .map(|(at, node)| {
            !used.contains(&at)
                && before.is_some_and(|before| {
                    node.attribute("modified")
                        .and_then(|modified| modified.parse::<jiff::Timestamp>().ok())
                        .is_some_and(|modified| modified < before)
                })
        })
        .collect();
    let left = bookmarks.len() - aged.iter().filter(|aged| **aged).count();
    let mut over = (left + fresh.len()).saturating_sub(KEPT);
    for (at, node) in bookmarks.iter().enumerate() {
        let dropped = aged[at] || (over > 0 && !used.contains(&at));
        if dropped {
            if !aged[at] {
                over -= 1;
            }
            edits.put(whole_lines(*node, text), String::new());
        }
    }
    let fresh = &fresh[over.min(fresh.len())..];
    if !fresh.is_empty() {
        inside(root, text, &fresh.concat(), &mut edits)?;
    }
    let written = edits.applied(text)?;
    if read(&written).is_none() {
        tracing::warn!("what would be added to the recently used files does not read: not written");
        return None;
    }
    Some(written)
}

/// `text` as a document `GLib`'s `GBookmarkFile` reads (`glib_reads`), its
/// nesting no deeper than the parser's recursion can go. `GLib`'s parser
/// (`GMarkup`) declares no entities: a list that does is not read.
fn read(text: &str) -> Option<roxmltree::Document<'_>> {
    if text.contains("<!ENTITY") {
        return None;
    }
    crate::inspection::xml_depth(text.as_bytes()).ok()?;
    let doc = roxmltree::Document::parse(text).ok()?;
    glib_reads(&doc, text).then_some(doc)
}

/// The elements among `node`'s children.
fn elements<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    node.children().filter(roxmltree::Node::is_element)
}

/// An element's name as written, its prefix included: XBEL's own
/// elements are known to `GLib` by that alone.
fn qualified<'t>(node: roxmltree::Node, text: &'t str) -> &'t str {
    let rest = text.get(node.range().start + 1..).unwrap_or_default();
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(rest.len());
    &rest[..end]
}

/// Whether `node` is XBEL's element `name` as `GLib` tells it: by its name
/// as written, without a prefix, whatever namespace is the default.
fn is_xbel(node: roxmltree::Node, text: &str, name: &str) -> bool {
    node.is_element() && qualified(node, text) == name
}

/// Whether `node` is the element `name` of namespace `ns` as `GLib` tells
/// it: of that namespace (its prefix's, or the default one), or, with no
/// prefix and no default namespace, by its name alone.
fn is_of(node: roxmltree::Node, ns: &str, name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && node.tag_name().namespace().is_none_or(|of| of == ns)
}

/// Whether `node` declares namespaces of its own: `GLib` reads a
/// declaration as an attribute like any other.
fn declares(node: roxmltree::Node) -> bool {
    let theirs = node.parent_element().map(|parent| parent.namespaces());
    let mine = node.namespaces();
    match theirs {
        Some(theirs) => {
            mine.len() != theirs.len()
                || mine
                    .zip(theirs)
                    .any(|(a, b)| a.name() != b.name() || a.uri() != b.uri())
        }
        None => false,
    }
}

/// Whether `GLib`'s `GBookmarkFile` reads `doc` (of `text`), as its parser
/// takes a list (`gbookmarkfile.c`, checked against `GLib` 2.88's reader):
/// `xbel` of version 1.0 holding titles, descriptions and bookmarks; a
/// bookmark of an address (`href`) no other bookmark has, its dates read
/// (`added`, `modified`, `visited`) and no other attribute, holding a title,
/// a description and its `info`; that holding only metadata, freedesktop's
/// (`owner`) holding its applications (each of a `name` and an `exec`), its
/// groups, its icon, its type and whether it is private, in their
/// namespaces; another owner's holding only metadata.
fn glib_reads(doc: &roxmltree::Document, text: &str) -> bool {
    let root = doc.root_element();
    if !is_xbel(root, text, "xbel") {
        return false;
    }
    let versioned = root
        .attributes()
        .any(|a| a.namespace().is_none() && a.name() == "version" && a.value() == "1.0");
    if !versioned {
        return elements(root).next().is_none();
    }
    let mut hrefs = HashSet::new();
    elements(root).all(|child| {
        if is_xbel(child, text, "title") || is_xbel(child, text, "desc") {
            return elements(child).next().is_none();
        }
        is_xbel(child, text, "bookmark") && bookmark_reads(child, text, &mut hrefs)
    })
}

fn bookmark_reads<'a>(
    node: roxmltree::Node<'a, '_>,
    text: &str,
    hrefs: &mut HashSet<&'a str>,
) -> bool {
    if declares(node) {
        return false;
    }
    let mut href = None;
    for attribute in node.attributes() {
        if attribute.namespace().is_some() {
            return false;
        }
        match attribute.name() {
            "href" => href = Some(attribute.value()),
            "added" | "modified" | "visited" if glib_date(attribute.value()) => {}
            _ => return false,
        }
    }
    if !href.is_some_and(|href| hrefs.insert(href)) {
        return false;
    }
    elements(node).all(|child| {
        if is_xbel(child, text, "title") || is_xbel(child, text, "desc") {
            return elements(child).next().is_none();
        }
        is_xbel(child, text, "info") && info_reads(child, text)
    })
}

/// A bookmark's `info`, or another owner's metadata, which `GLib` reads
/// alike: only metadata in it.
fn info_reads(node: roxmltree::Node, text: &str) -> bool {
    elements(node).all(|metadata| {
        is_xbel(metadata, text, "metadata")
            && if metadata.attribute("owner") == Some(OWNER) {
                metadata_reads(metadata)
            } else {
                info_reads(metadata, text)
            }
    })
}

fn metadata_reads(node: roxmltree::Node) -> bool {
    let empty = |node: roxmltree::Node| elements(node).next().is_none();
    elements(node).all(|child| {
        if is_of(child, BOOKMARK_NS, "applications") {
            elements(child).all(|app| {
                is_of(app, BOOKMARK_NS, "application")
                    && empty(app)
                    && app.attribute("name").is_some()
                    && app.attribute("exec").is_some()
                    && app.attribute("modified").is_none_or(glib_date)
            })
        } else if is_of(child, BOOKMARK_NS, "groups") {
            elements(child).all(|group| is_of(group, BOOKMARK_NS, "group") && empty(group))
        } else if is_of(child, BOOKMARK_NS, "private") {
            // Read where it is, its own read as the metadata's.
            metadata_reads(child)
        } else {
            (is_of(child, BOOKMARK_NS, "icon") || is_of(child, MIME_NS, "mime-type"))
                && empty(child)
        }
    })
}

/// Whether `GLib` reads `text` as a date (`g_date_time_new_from_iso8601`,
/// its zone said): an instant with its offset.
fn glib_date(text: &str) -> bool {
    text.parse::<jiff::Timestamp>().is_ok()
}

/// The prefixes the bookmarks Skwir writes give their applications and
/// their type (`bookmark:`, `mime:`, as written, never empty): those the
/// list's root declares, else the usual one, its declaration added to the
/// root (where it declares none, or the namespace as its default alone);
/// none where the usual one is another namespace's.
struct Prefixes {
    bookmark: String,
    mime: String,
}

impl Prefixes {
    fn of(root: roxmltree::Node, text: &str, edits: &mut Edits) -> Option<Prefixes> {
        let mut declared = String::new();
        let mut prefix = |ns: &str, usual: &str| -> Option<String> {
            // A prefix the root declares for it, whatever else it declares
            // it as (`lookup_prefix` answers the first declaration's,
            // none where that is the default namespace).
            if let Some(prefix) = root
                .namespaces()
                .find_map(|declared| declared.name().filter(|_| declared.uri() == ns))
            {
                return Some(format!("{prefix}:"));
            }
            // Its usual name taken by another namespace: written as no
            // reader would read it.
            if root.lookup_namespace_uri(Some(usual)).is_some() {
                return None;
            }
            let _ = write!(declared, " xmlns:{usual}=\"{ns}\"");
            Some(format!("{usual}:"))
        };
        let prefixes = Prefixes {
            bookmark: prefix(BOOKMARK_NS, "bookmark")?,
            mime: prefix(MIME_NS, "mime")?,
        };
        if !declared.is_empty() {
            let after = root.attributes().next_back().map_or_else(
                || root.range().start + 1 + qualified(root, text).len(),
                |last| last.range().end,
            );
            edits.put(after..after, declared);
        }
        Some(prefixes)
    }
}

/// The changes made to a list's text: each a range of it and what takes
/// its place, none within another.
#[derive(Default)]
struct Edits(Vec<(Range<usize>, String)>);

impl Edits {
    fn put(&mut self, range: Range<usize>, with: String) {
        self.0.push((range, with));
    }

    /// `text` with the changes made, in one pass; `None` where two would
    /// change the same text (what was found was misread).
    fn applied(mut self, text: &str) -> Option<String> {
        // Stable: what is added at one place goes in the order it came.
        self.0.sort_by_key(|(range, _)| range.start);
        let more: usize = self.0.iter().map(|(_, with)| with.len()).sum();
        let mut out = String::with_capacity(text.len() + more);
        let mut at = 0;
        for (range, with) in self.0 {
            if range.start < at {
                return None;
            }
            out.push_str(text.get(at..range.start)?);
            out.push_str(&with);
            at = range.end;
        }
        out.push_str(text.get(at..)?);
        Some(out)
    }
}

/// The edits that make a bookmark visited and modified `now` by Skwir: its
/// times, Skwir's count of it up one (as `GLib` counts, `atoi`; one where
/// none is said) and its time, or Skwir added to its applications, which
/// are added to its metadata, which is added to its `info`, which is added
/// to it, where it has none.
fn visit(
    node: roxmltree::Node,
    text: &str,
    now: &str,
    prefixes: &Prefixes,
    edits: &mut Edits,
) -> Option<()> {
    for name in ["modified", "visited"] {
        set(node, name, now, edits)?;
    }
    let info = elements(node).find(|n| is_xbel(*n, text, "info"));
    let metadata = info.and_then(|info| {
        elements(info)
            .find(|n| is_xbel(*n, text, "metadata") && n.attribute("owner") == Some(OWNER))
    });
    let applications =
        metadata.and_then(|m| elements(m).find(|n| is_of(*n, BOOKMARK_NS, "applications")));
    let ours = applications.and_then(|apps| {
        elements(apps).find(|n| {
            is_of(*n, BOOKMARK_NS, "application") && n.attribute("name") == Some(APPLICATION)
        })
    });
    let b = &prefixes.bookmark;
    let app = application(b, now);
    match (info, metadata, applications, ours) {
        (_, _, _, Some(ours)) => {
            let count = ours.attribute("count").map_or(1, atoi).saturating_add(1);
            set(ours, "count", &count.to_string(), edits)?;
            set(ours, "modified", now, edits)?;
        }
        (_, _, Some(apps), None) => inside(apps, text, &app, edits)?,
        (_, Some(metadata), None, None) => inside(
            metadata,
            text,
            &format!("        <{b}applications>\n{app}        </{b}applications>\n"),
            edits,
        )?,
        (Some(info), None, None, None) => {
            inside(info, text, &metadata_text(b, None, &app), edits)?;
        }
        (None, None, None, None) => inside(
            node,
            text,
            &format!("    <info>\n{}    </info>\n", metadata_text(b, None, &app)),
            edits,
        )?,
    }
    Some(())
}

/// `element`'s attribute `name` set to `value`: its value replaced, or the
/// attribute added after its last.
fn set(element: roxmltree::Node, name: &str, value: &str, edits: &mut Edits) -> Option<()> {
    let found = element
        .attributes()
        .find(|a| a.namespace().is_none() && a.name() == name);
    if let Some(attribute) = found {
        edits.put(attribute.range_value(), escaped(value));
    } else {
        let end = element.attributes().next_back()?.range().end;
        edits.put(end..end, format!(" {name}=\"{}\"", escaped(value)));
    }
    Some(())
}

/// `inner` (whole lines) added at the end of `element`'s content: on the
/// line of its closing tag, before it; written open where it was closed
/// on itself (`<xbel/>`).
fn inside(element: roxmltree::Node, text: &str, inner: &str, edits: &mut Edits) -> Option<()> {
    let range = element.range();
    let whole = text.get(range.clone())?;
    if whole.ends_with("/>") {
        let name = qualified(element, text);
        edits.put(range.end - 2..range.end, format!(">\n{inner}</{name}>"));
        return Some(());
    }
    let closing = range.start + whole.rfind("</")?;
    let line = text[..closing].rfind('\n').map_or(0, |at| at + 1);
    let at = if text[line..closing].trim_matches([' ', '\t']).is_empty() {
        line
    } else {
        closing
    };
    let lead = if text[..at].ends_with('\n') { "" } else { "\n" };
    edits.put(at..at, format!("{lead}{inner}"));
    Some(())
}

/// The text of `node` with the line it stands on, where it stands alone
/// on it: what goes when it is dropped.
fn whole_lines(node: roxmltree::Node, text: &str) -> Range<usize> {
    let Range { mut start, mut end } = node.range();
    let line = text[..start].rfind('\n').map_or(0, |at| at + 1);
    if text[line..start].trim_matches([' ', '\t']).is_empty() {
        start = line;
        if text[end..].starts_with("\r\n") {
            end += 2;
        } else if text[end..].starts_with('\n') {
            end += 1;
        }
    }
    start..end
}

/// A count as `GLib` reads it (`atoi`): the digits it starts with, 0 for
/// none.
fn atoi(text: &str) -> u32 {
    let digits: String = text
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        0
    } else {
        digits.parse().unwrap_or(u32::MAX)
    }
}

/// A bookmark of the file at `href`, of type `mime`, added `now` by
/// Skwir, as `GLib` writes one.
fn bookmark(href: &str, mime: &str, now: &str, prefixes: &Prefixes) -> String {
    let app = application(&prefixes.bookmark, now);
    format!(
        "  <bookmark href=\"{}\" added=\"{now}\" modified=\"{now}\" visited=\"{now}\">\n    \
         <info>\n{}    </info>\n  </bookmark>\n",
        escaped(href),
        metadata_text(&prefixes.bookmark, Some((&prefixes.mime, mime)), &app),
    )
}

/// Freedesktop's metadata of a bookmark: its type (`mime`, with its
/// namespace's prefix), and its applications, `app`.
fn metadata_text(b: &str, mime: Option<(&str, &str)>, app: &str) -> String {
    let typed = mime.map_or_else(String::new, |(m, mime)| {
        format!("        <{m}mime-type type=\"{}\"/>\n", escaped(mime))
    });
    format!(
        "      <metadata owner=\"{OWNER}\">\n{typed}        <{b}applications>\n{app}        \
         </{b}applications>\n      </metadata>\n"
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

/// `text` as an attribute's value says it, as `GLib` escapes it.
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

    const NOW: &str = "2026-11-20T10:00:00.123456Z";

    /// Each bookmark: its path, when it was visited, its type, and its
    /// applications with their counts.
    type Read = (PathBuf, String, String, Vec<(String, String)>);

    fn bookmarks(text: &str) -> Vec<Read> {
        let doc = roxmltree::Document::parse(text).expect("it reads");
        assert!(glib_reads(&doc, text), "as GLib reads it:\n{text}");
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
                let mime = b
                    .descendants()
                    .find(|n| n.tag_name().name() == "mime-type")
                    .and_then(|n| n.attribute("type"))
                    .unwrap_or_default()
                    .to_owned();
                (
                    crate::uri::path_of(b.attribute("href").unwrap()).unwrap(),
                    b.attribute("visited").unwrap_or_default().to_owned(),
                    mime,
                    apps,
                )
            })
            .collect()
    }

    fn entry(path: &str, mime: &str) -> (PathBuf, String) {
        (PathBuf::from(path), mime.to_owned())
    }

    /// A list as `GLib` writes one, holding `body`.
    fn glib_list(body: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\"\n      \
             xmlns:bookmark=\"{BOOKMARK_NS}\"\n      xmlns:mime=\"{MIME_NS}\"\n>\n{body}</xbel>"
        )
    }

    /// One of another program's bookmarks, as `GLib` writes them.
    fn theirs(href: &str, app: &str, count: u32) -> String {
        format!(
            "  <bookmark href=\"{href}\" added=\"2026-01-01T00:00:00Z\" modified=\"2026-01-01T00:00:00Z\" visited=\"2026-01-01T00:00:00Z\">\n    <info>\n      <metadata owner=\"http://freedesktop.org\">\n        <mime:mime-type type=\"text/plain\"/>\n        <bookmark:applications>\n          <bookmark:application name=\"{app}\" exec=\"&apos;{app} %u&apos;\" modified=\"2026-01-01T00:00:00Z\" count=\"{count}\"/>\n        </bookmark:applications>\n      </metadata>\n    </info>\n  </bookmark>\n"
        )
    }

    /// A list made anew holds the files chosen, Skwir's once, typed, a
    /// name's apostrophe and accents kept, as `GLib` writes one.
    #[test]
    fn a_new_list_holds_what_was_chosen() {
        let path = "/tmp/l'été/notes & plans.txt";
        let written = added(None, &[entry(path, "text/plain")], NOW, None).unwrap();
        assert_eq!(
            bookmarks(&written),
            [(
                PathBuf::from(path),
                NOW.to_owned(),
                "text/plain".to_owned(),
                vec![("skwir".into(), "1".into())]
            )]
        );
        assert!(
            written.starts_with(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel version=\"1.0\"\n      xmlns:bookmark="
            ),
            "{written}"
        );
        assert!(written.ends_with("  </bookmark>\n</xbel>"), "{written}");
        assert!(written.contains("exec=\"&apos;skwir %u&apos;\""));
    }

    /// A file the list names is visited again, Skwir's count up one, or
    /// Skwir added beside the program that used it; another file is
    /// added; the rest is kept byte for byte.
    #[test]
    fn a_list_keeps_what_others_wrote() {
        let libre = theirs("file:///home/a/r%C3%A9sum%C3%A9.odt", "LibreOffice", 4);
        let before = glib_list(&format!(
            "{libre}  <bookmark href=\"file:///home/a/photo.png\" added=\"2026-01-02T00:00:00Z\" visited=\"2026-01-02T00:00:00Z\">\n    <info>\n      <metadata owner=\"http://freedesktop.org\">\n        <bookmark:applications>\n          <bookmark:application name=\"skwir\" exec=\"&apos;skwir %u&apos;\" modified=\"2026-01-02T00:00:00Z\" count=\"2\"/>\n        </bookmark:applications>\n      </metadata>\n    </info>\n  </bookmark>\n"
        ));
        let entries = [
            entry(
                "/home/a/résumé.odt",
                "application/vnd.oasis.opendocument.text",
            ),
            entry("/home/a/photo.png", "image/png"),
            entry("/home/a/new.txt", "text/plain"),
        ];
        let written = added(Some(&before), &entries, NOW, None).unwrap();
        let after = bookmarks(&written);
        assert_eq!(after.len(), 3);
        assert_eq!(after[0].1, NOW);
        assert_eq!(
            after[0].3,
            [
                ("LibreOffice".into(), "4".into()),
                ("skwir".into(), "1".into())
            ]
        );
        assert_eq!(after[1].1, NOW);
        assert_eq!(after[1].3, [("skwir".into(), "3".into())]);
        assert!(written.contains(&format!("visited=\"{NOW}\" modified=\"{NOW}\">")));
        assert_eq!(after[2].0, PathBuf::from("/home/a/new.txt"));
        // What another program wrote stays as it was.
        assert!(written.contains(
            "<bookmark:application name=\"LibreOffice\" exec=\"&apos;LibreOffice %u&apos;\" \
             modified=\"2026-01-01T00:00:00Z\" count=\"4\"/>\n          <bookmark:application name=\"skwir\""
        ));
        assert!(written.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbel"));
    }

    /// A file an answer names twice (a program saving two files of one
    /// name) is one bookmark, visited once: never two of one address, a
    /// date written over itself or an attribute twice, which `GLib` refuses
    /// whole (CH-1).
    #[test]
    fn a_file_named_twice_is_one_bookmark() {
        let before = glib_list(&theirs("file:///home/a/listed.jpg", "skwir", 9));
        let entries = [
            entry("/home/a/listed.jpg", "image/jpeg"),
            entry("/home/a/new.jpg", "image/jpeg"),
            entry("/home/a/listed.jpg", "image/jpeg"),
            entry("/home/a/new.jpg", "image/jpeg"),
        ];
        let written = added(Some(&before), &entries, NOW, None).unwrap();
        let after = bookmarks(&written);
        assert_eq!(after.len(), 2, "{written}");
        assert_eq!(after[0].3, [("skwir".into(), "10".into())]);
        assert_eq!(after[0].1, NOW);
        assert_eq!(after[1].0, PathBuf::from("/home/a/new.jpg"));
        // And a bookmark with no count or time of Skwir's gets each once.
        let bare = glib_list(
            "  <bookmark href=\"file:///home/a/bare.txt\">\n    <info>\n      <metadata owner=\"http://freedesktop.org\">\n        <bookmark:applications>\n          <bookmark:application name=\"skwir\" exec=\"x\"/>\n        </bookmark:applications>\n      </metadata>\n    </info>\n  </bookmark>\n",
        );
        let twice = [
            entry("/home/a/bare.txt", "text/plain"),
            entry("/home/a/bare.txt", "text/plain"),
        ];
        let written = added(Some(&bare), &twice, NOW, None).unwrap();
        assert_eq!(bookmarks(&written)[0].3, [("skwir".into(), "2".into())]);
        assert_eq!(written.matches("modified=").count(), 2, "{written}");
        assert_eq!(written.matches("visited=").count(), 1, "{written}");
    }

    /// A list that does not read as `GLib` reads one is left alone: not
    /// XML, another document, an address twice, an attribute `GLib` does
    /// not know, bookmarks without the version, an entity declared.
    #[test]
    fn a_list_that_does_not_read_is_left_alone() {
        let entries = [entry("/home/a/x.txt", "text/plain")];
        let listed = theirs("file:///home/a/y.txt", "gedit", 1);
        for broken in [
            "<xbel><bookmark".to_owned(),
            "<html></html>".to_owned(),
            glib_list(&format!("{listed}{listed}")),
            glib_list(&listed.replace(" added=", " starred=\"1\" added=")),
            glib_list(&listed.replace("2026-01-01T00:00:00Z\" visited", "yesterday\" visited")),
            glib_list(&listed).replace(" version=\"1.0\"", ""),
            glib_list(&listed).replace("</bookmark:applications>", "</bookmark:applications><x/>"),
            format!(
                "<?xml version=\"1.0\"?><!DOCTYPE xbel [<!ENTITY e \"x\">]>{}",
                glib_list("").trim_start_matches("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")
            ),
        ] {
            assert_eq!(added(Some(&broken), &entries, NOW, None), None, "{broken}");
        }
        assert!(
            added(Some("   "), &entries, NOW, None).is_some(),
            "an empty one is made anew"
        );
    }

    /// The bookmarks Skwir writes take the prefixes the list's root gives
    /// the namespaces, declared there where it gives none; a list closed
    /// on itself is opened (CH-5).
    #[test]
    fn a_bookmark_is_written_in_the_list_s_own_prefixes() {
        let entries = [entry("/home/a/x.txt", "text/plain")];
        let own = format!(
            "<?xml version=\"1.0\"?>\n<xbel version=\"1.0\" xmlns:b=\"{BOOKMARK_NS}\" xmlns:t=\"{MIME_NS}\">\n</xbel>\n"
        );
        let written = added(Some(&own), &entries, NOW, None).unwrap();
        assert!(
            written.contains("<t:mime-type type=\"text/plain\"/>"),
            "{written}"
        );
        assert!(
            written.contains("<b:application name=\"skwir\""),
            "{written}"
        );
        assert_eq!(bookmarks(&written).len(), 1);
        for none in [
            "<xbel version=\"1.0\"></xbel>",
            "<xbel version=\"1.0\"/>",
            "<?xml version=\"1.0\"?>\n<xbel version=\"1.0\"\n/>\n",
        ] {
            let written = added(Some(none), &entries, NOW, None).unwrap();
            assert!(
                written.contains(&format!("xmlns:bookmark=\"{BOOKMARK_NS}\"")),
                "{written}"
            );
            assert!(written.contains("<bookmark:applications>"), "{written}");
            assert_eq!(bookmarks(&written).len(), 1, "{written}");
        }
        // Declared as the default namespace, and as a prefix too: the
        // prefix; as the default alone: the usual one, declared.
        let both = format!(
            "<xbel version=\"1.0\" xmlns=\"{BOOKMARK_NS}\" xmlns:bookmark=\"{BOOKMARK_NS}\"></xbel>"
        );
        let written = added(Some(&both), &entries, NOW, None).unwrap();
        assert!(written.contains("<bookmark:applications>"), "{written}");
        let default = format!("<xbel version=\"1.0\" xmlns=\"{BOOKMARK_NS}\"></xbel>");
        let written = added(Some(&default), &entries, NOW, None).unwrap();
        assert!(
            written.contains(&format!("xmlns:bookmark=\"{BOOKMARK_NS}\"")),
            "{written}"
        );
        // A prefix of the usual name taken by another namespace: left
        // alone.
        let taken = "<xbel version=\"1.0\" xmlns:bookmark=\"urn:other\"></xbel>";
        assert_eq!(added(Some(taken), &entries, NOW, None), None);
    }

    /// A bookmark another program wrote without applications, metadata or
    /// `info` gets what it lacks, Skwir among them: `GLib` writes no
    /// bookmark without an application.
    #[test]
    fn a_bare_bookmark_gets_skwir_s_application() {
        let entries = [entry("/home/a/x.txt", "text/plain")];
        for bare in [
            "  <bookmark href=\"file:///home/a/x.txt\"/>\n",
            "  <bookmark href=\"file:///home/a/x.txt\" added=\"2026-01-01T00:00:00Z\"></bookmark>\n",
            "  <bookmark href=\"file:///home/a/x.txt\">\n    <info/>\n  </bookmark>\n",
            "  <bookmark href=\"file:///home/a/x.txt\">\n    <info>\n      <metadata owner=\"http://www.kde.org\"/>\n    </info>\n  </bookmark>\n",
            "  <bookmark href=\"file:///home/a/x.txt\">\n    <info>\n      <metadata owner=\"http://freedesktop.org\">\n        <mime:mime-type type=\"text/plain\"/>\n      </metadata>\n    </info>\n  </bookmark>\n",
            "  <bookmark href=\"file:///home/a/x.txt\">\n    <info>\n      <metadata owner=\"http://freedesktop.org\">\n        <bookmark:applications/>\n      </metadata>\n    </info>\n  </bookmark>\n",
        ] {
            let written = added(Some(&glib_list(bare)), &entries, NOW, None).unwrap();
            let after = bookmarks(&written);
            assert_eq!(after.len(), 1, "{written}");
            assert_eq!(after[0].1, NOW, "{written}");
            assert_eq!(after[0].3, [("skwir".into(), "1".into())], "{written}");
        }
    }

    /// The list keeps the thousand bookmarks GTK keeps, those first in it
    /// going first, never one used now (CH-2).
    #[test]
    fn the_list_keeps_what_gtk_keeps() {
        let body: String = (0..KEPT)
            .map(|at| theirs(&format!("file:///home/a/{at}.txt"), "gedit", 1))
            .collect();
        let before = glib_list(&body);
        let entries = [
            entry("/home/a/0.txt", "text/plain"),
            entry("/home/a/new1.txt", "text/plain"),
            entry("/home/a/new2.txt", "text/plain"),
        ];
        let written = added(Some(&before), &entries, NOW, None).unwrap();
        let after = bookmarks(&written);
        assert_eq!(after.len(), KEPT);
        let names: Vec<String> = after
            .iter()
            .map(|b| b.0.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names[..3], ["0.txt", "3.txt", "4.txt"], "1 and 2 went");
        assert_eq!(names[KEPT - 2..], ["new1.txt", "new2.txt"]);
        assert!(!written.contains("\n\n"), "no line left where one went");
    }

    /// A bookmark last modified longer ago than the desktop keeps files is
    /// dropped as GTK drops it, never one used now; one of another age, or
    /// of a date not said, stays (CH-3).
    #[test]
    fn the_list_forgets_what_the_desktop_forgets() {
        let body = [
            theirs("file:///home/a/old.txt", "gedit", 1),
            theirs("file:///home/a/old-used.txt", "gedit", 1),
            theirs("file:///home/a/new.txt", "gedit", 1).replace(
                "modified=\"2026-01-01T00:00:00Z\"",
                "modified=\"2026-11-19T00:00:00Z\"",
            ),
            theirs("file:///home/a/undated.txt", "gedit", 1)
                .replace(" modified=\"2026-01-01T00:00:00Z\" visited", " visited"),
        ]
        .concat();
        let before: jiff::Timestamp = "2026-11-01T00:00:00Z".parse().unwrap();
        let entries = [entry("/home/a/old-used.txt", "text/plain")];
        let written = added(Some(&glib_list(&body)), &entries, NOW, Some(before)).unwrap();
        let names: Vec<String> = bookmarks(&written)
            .iter()
            .map(|b| b.0.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["old-used.txt", "new.txt", "undated.txt"]);
    }

    /// GTK's `settings.ini`s are asked in the order whose word wins
    /// first, as GTK loads them the other way round: the user's, the last
    /// of `XDG_CONFIG_DIRS` before the first, `/etc`'s, the data prefix's.
    #[test]
    fn gtk_s_settings_are_asked_as_gtk_cascades_them() {
        let dirs = [PathBuf::from("/etc/xdg"), PathBuf::from("/opt/look/xdg")];
        let files = gtk_settings_files("gtk-3.0", Some(PathBuf::from("/u/.config")), &dirs);
        let expected: Vec<PathBuf> = [
            "/u/.config",
            "/opt/look/xdg",
            "/etc/xdg",
            "/etc",
            "/usr/share",
        ]
        .iter()
        .map(|folder| Path::new(folder).join("gtk-3.0/settings.ini"))
        .collect();
        assert_eq!(files, expected);
    }

    /// Each desktop's switch is read where it keeps it: GTK's
    /// `settings.ini`s, the first saying a key for each GTK, an age of 0
    /// remembering none; GNOME's or Cinnamon's privacy schema as
    /// `gsettings` prints it; KDE's `kdeglobals`, the user's first. Any
    /// that says none is enough; the shortest age is kept (CH-3).
    #[test]
    fn the_desktop_s_switches_are_read_where_each_keeps_its_own() {
        let ini = |body: &str| format!("[Settings]\n{body}\n");
        let on = Said::default();
        let off = Said {
            off: true,
            days: None,
        };
        let days = |days| Said {
            off: false,
            days: Some(days),
        };
        assert_eq!(gtk_said(&[]), on);
        assert_eq!(
            gtk_said(&[vec![ini("gtk-recent-files-enabled=false")], vec![]]),
            off
        );
        assert_eq!(
            gtk_said(&[
                vec![
                    ini("gtk-recent-files-enabled=true"),
                    ini("gtk-recent-files-enabled=false")
                ],
                vec![],
            ]),
            on,
            "the user's said first"
        );
        assert_eq!(
            gtk_said(&[
                vec![],
                vec![String::new(), ini("gtk-recent-files-max-age=0")]
            ]),
            off,
            "the system's, where the user's says nothing; GTK 3's too"
        );
        assert_eq!(
            gtk_said(&[
                vec![ini("gtk-recent-files-max-age=14")],
                vec![ini("gtk-recent-files-max-age=7")],
            ]),
            days(7)
        );
        assert_eq!(gtk_said(&[vec![ini("gtk-recent-files-max-age=-1")]]), on);
        let printed = |remember: &str, age: &str| {
            format!(
                "org.gnome.desktop.privacy disable-camera false\n\
                 org.gnome.desktop.privacy recent-files-max-age {age}\n\
                 org.gnome.desktop.privacy remember-recent-files {remember}\n"
            )
        };
        assert_eq!(gsettings_said(&printed("true", "-1")), on);
        assert_eq!(gsettings_said(&printed("false", "-1")), off);
        assert_eq!(gsettings_said(&printed("true", "30")), days(30));
        assert_eq!(gsettings_said(""), on, "no such schema");
        let kde = |body: &str| format!("[General]\nUseRecent=false\n[RecentDocuments]\n{body}\n");
        assert_eq!(kde_said(&[kde("UseRecent=false")]), off);
        assert_eq!(
            kde_said(&[kde("UseRecent=true"), kde("UseRecent=false")]),
            on
        );
        assert_eq!(kde_said(&[kde(""), kde("MaxEntries=0")]), off);
        assert_eq!(kde_said(&[kde("MaxEntries=300")]), on);
        // KConfig's words for off, case ignored.
        for word in ["off", "No", "0", "FALSE"] {
            assert_eq!(
                kde_said(&[kde(&format!("UseRecent={word}"))]),
                off,
                "{word}"
            );
        }
        assert_eq!(kde_said(&[kde("UseRecent=yes")]), on);
        assert_eq!(
            gtk_said(&[vec![ini("gtk-recent-files-enabled=no")], vec![]]),
            on,
            "GLib's booleans are true, false, 1 and 0 alone"
        );
        // An administrator's lock, on the group or the key, over the user's.
        let locked = "[RecentDocuments][$i]\nUseRecent=false\n".to_owned();
        assert_eq!(kde_said(&[kde("UseRecent=true"), locked]), off);
        let key_locked = "[RecentDocuments]\nUseRecent[$i]=off\n".to_owned();
        assert_eq!(kde_said(&[kde("UseRecent=true"), key_locked]), off);
        let unlocked = "[RecentDocuments]\nUseRecent=false\n".to_owned();
        assert_eq!(kde_said(&[kde("UseRecent=true"), unlocked]), on);
        assert_eq!(
            days(7).and(off),
            Said {
                off: true,
                days: Some(7)
            }
        );
        assert_eq!(days(7).and(days(30)).and(on), days(7));
    }

    /// Recent has something to give where its list names a file that is
    /// there; a list of none, of files gone or of another host's, or that
    /// does not read, has none (CH-13).
    #[test]
    fn recent_has_something_where_a_file_it_names_is_there() {
        let tmp = tempfile::tempdir().unwrap();
        let here = tmp.path().join("l'été.txt");
        std::fs::write(&here, "x").unwrap();
        let href = crate::uri::file_uri(&here);
        assert!(names_one(&glib_list(&theirs(&href, "gedit", 1))));
        let gone = crate::uri::file_uri(&tmp.path().join("gone.txt"));
        assert!(!names_one(&glib_list(&theirs(&gone, "gedit", 1))));
        assert!(!names_one(&glib_list(&theirs(
            "file://elsewhere/x",
            "gedit",
            1
        ))));
        assert!(!names_one(&glib_list("")));
        assert!(!names_one("<xbel><bookmark"));
    }

    /// A date is written as `GLib` writes one: its microseconds only where
    /// there are any.
    #[test]
    fn a_date_is_written_as_glib_writes_it() {
        let whole: jiff::Timestamp = "2026-11-20T10:00:00Z".parse().unwrap();
        assert_eq!(date(whole), "2026-11-20T10:00:00Z");
        let micros: jiff::Timestamp = "2026-11-20T10:00:00.0405Z".parse().unwrap();
        assert_eq!(date(micros), "2026-11-20T10:00:00.040500Z");
        assert!(glib_date(&date(micros)));
        assert_eq!(atoi(" 12abc"), 12);
        assert_eq!(atoi("abc"), 0);
    }

    /// A bookmark's type is what the file is: a link's, its target's; a
    /// name that says none, its bytes'; a file not there yet, its name's;
    /// none known, GTK's `application/octet-stream` (CH-4).
    #[test]
    fn a_bookmark_is_typed_by_what_the_file_is() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("report.pdf"), b"%PDF-1.4\n").unwrap();
        std::os::unix::fs::symlink(root.join("report.pdf"), root.join("report")).unwrap();
        std::fs::write(root.join("picture"), b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        std::fs::write(root.join("blob"), [0u8, 159, 146, 150, 1, 2, 3]).unwrap();
        assert_eq!(type_of(&root.join("report")), "application/pdf");
        assert_eq!(type_of(&root.join("picture")), "image/png");
        assert_eq!(type_of(&root.join("to be saved.txt")), "text/plain");
        assert_eq!(type_of(&root.join("blob")), UNKNOWN_TYPE);
        assert_eq!(type_of(root), "inode/directory");
    }

    /// `GLib`'s own reader, built from a few lines of C where this machine
    /// can (the dev shell: a C compiler and `GLib`'s headers through
    /// `pkg-config`), reads every list Skwir writes, as Skwir meant it.
    #[test]
    // A test's own helpers, started as they are: no console on Linux.
    #[allow(clippy::disallowed_methods)]
    fn glib_reads_what_skwir_writes() {
        use std::process::Command;
        let tmp = tempfile::tempdir().unwrap();
        let flags = Command::new("pkg-config")
            .args(["--cflags", "--libs", "glib-2.0"])
            .output()
            .ok()
            .filter(|out| out.status.success());
        let libdir = Command::new("pkg-config")
            .args(["--variable=libdir", "glib-2.0"])
            .output()
            .ok()
            .filter(|out| out.status.success());
        let (Some(flags), Some(libdir)) = (flags, libdir) else {
            eprintln!("skipped: no GLib headers through pkg-config");
            return;
        };
        let reader = tmp.path().join("reads");
        std::fs::write(tmp.path().join("reads.c"), READER).unwrap();
        let flags = String::from_utf8_lossy(&flags.stdout).into_owned();
        let libdir = String::from_utf8_lossy(&libdir.stdout).trim().to_owned();
        // The dev shell's environment is too large for the compiler's own
        // programs: only its path.
        let built = Command::new("cc")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .arg("-o")
            .arg(&reader)
            .arg(tmp.path().join("reads.c"))
            .args(flags.split_whitespace())
            .arg(format!("-Wl,-rpath,{libdir}"))
            .status();
        if !built.is_ok_and(|status| status.success()) {
            eprintln!("skipped: no C compiler to build GLib's reader with");
            return;
        }
        let listed = theirs("file:///home/a/listed.jpg", "eog", 3);
        let cases: Vec<(String, Vec<(PathBuf, String)>)> = vec![
            (
                String::new(),
                vec![entry("/home/a/l'été & co.txt", "text/plain")],
            ),
            (
                glib_list(&listed),
                vec![
                    entry("/home/a/listed.jpg", "image/jpeg"),
                    entry("/home/a/listed.jpg", "image/jpeg"),
                    entry("/home/a/new.jpg", "image/jpeg"),
                    entry("/home/a/new.jpg", "image/jpeg"),
                ],
            ),
            (
                "<xbel version=\"1.0\"/>".to_owned(),
                vec![entry("/home/a/x.txt", "text/plain")],
            ),
            (
                glib_list("  <bookmark href=\"file:///home/a/x.txt\"/>\n"),
                vec![entry("/home/a/x.txt", "text/plain")],
            ),
        ];
        let mut files = Vec::new();
        for (at, (before, entries)) in cases.iter().enumerate() {
            let once = added(Some(before), entries, NOW, None).unwrap();
            // Written again, by a later answer.
            let twice = added(Some(&once), entries, "2026-11-21T08:00:00Z", None).unwrap();
            for (round, text) in [once, twice].into_iter().enumerate() {
                let file = tmp.path().join(format!("{at}-{round}.xbel"));
                std::fs::write(&file, text).unwrap();
                files.push(file);
            }
        }
        let out = Command::new(&reader).args(&files).output().unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{said}");
        assert!(!said.contains("refused"), "{said}");
        // What `GLib` reads of them: each file once, Skwir's count.
        assert!(
            said.contains("file:///home/a/l'%C3%A9t%C3%A9%20&%20co.txt text/plain [skwir 1]"),
            "{said}"
        );
        let listed_line = said
            .lines()
            .rfind(|line| line.starts_with("file:///home/a/listed.jpg"))
            .unwrap_or_default();
        assert!(
            listed_line.contains("[eog 3]") && listed_line.contains("[skwir 2]"),
            "{said}"
        );
        assert!(
            said.contains("file:///home/a/new.jpg image/jpeg [skwir 2]"),
            "{said}"
        );
        assert_eq!(said.matches("listed.jpg").count(), 2, "{said}");
    }

    /// Reads each list given as `GLib`'s `GBookmarkFile` does, saying each
    /// bookmark's address, type, and applications with their counts.
    const READER: &str = r#"
#include <glib.h>
#include <stdio.h>
int main(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    GBookmarkFile *b = g_bookmark_file_new();
    GError *e = NULL;
    if (!g_bookmark_file_load_from_file(b, argv[i], &e)) {
      printf("%s: refused: %s\n", argv[i], e->message);
      continue;
    }
    gsize n = 0;
    gchar **uris = g_bookmark_file_get_uris(b, &n);
    printf("%s: %zu\n", argv[i], n);
    for (gsize u = 0; u < n; u++) {
      gchar *mime = g_bookmark_file_get_mime_type(b, uris[u], NULL);
      gsize na = 0;
      gchar **apps = g_bookmark_file_get_applications(b, uris[u], &na, NULL);
      printf("%s %s", uris[u], mime ? mime : "-");
      for (gsize a = 0; a < na; a++) {
        guint count = 0;
        g_bookmark_file_get_application_info(b, uris[u], apps[a], NULL, &count, NULL, NULL);
        printf(" [%s %u]", apps[a], count);
      }
      printf("\n");
    }
  }
  return 0;
}
"#;
}
