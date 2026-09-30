//! The extended attributes the plugin reads and writes, and the provider
//! that fills their fields during hydration.
//!
//! Every attribute is read through what a link leads to, as the rest of an
//! entry is (a user attribute cannot be set on the link itself), and a file
//! is asked for its attribute names once: most files have none, so a
//! listing costs one call per entry whatever the number of keys read.
//!
//! A file's tags are its own (`user.xdg.tags`) and those a Mac gave it
//! (`finder`): read as one list, and written to both when the file has
//! both, so an edit here holds on the Mac too. Where a Mac keeps them in
//! the file's `._` file (`appledouble`: a USB stick has no attributes),
//! they are read from there and written there, and the Mac's comment and
//! download addresses show where the file has none of its own.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use bibli_core::entry::{Entry, Metadata, Provenance, Value, keys};
use bibli_core::metadata::{MetadataProvider, TAGS_XATTR, same_tag};
use parking_lot::Mutex;

use crate::appledouble::{self, AppleDouble};
use crate::finder;
use crate::vocabulary::Vocabulary;

/// Tags: comma-separated, the freedesktop convention.
pub const TAGS: &str = TAGS_XATTR;
/// A comment on the file, as Dolphin writes it.
pub const COMMENT: &str = "user.xdg.comment";
/// A rating from 1 to 10 (half stars), as Dolphin writes it.
pub const RATING: &str = "user.baloo.rating";
/// Where a download came from and the page that led to it, as Chromium,
/// wget, curl and yt-dlp record them.
pub const ORIGIN: &str = "user.xdg.origin.url";
pub const REFERRER: &str = "user.xdg.referrer.url";

/// A rating's highest value: five stars.
pub const RATING_MAX: u64 = 10;

/// Split a comma-separated tag list, trimming and dropping empties and
/// duplicates (tags ignore case: [`same_tag`]; the first spelling stays).
pub fn parse_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split(',') {
        let t = t.trim();
        if !t.is_empty() && !out.iter().any(|x| same_tag(x, t)) {
            out.push(t.to_owned());
        }
    }
    out
}

/// A rating as stored: `0` or anything unreadable is no rating, and a
/// value past five stars is five stars.
pub fn parse_rating(s: &str) -> Option<u64> {
    s.trim()
        .parse::<u64>()
        .ok()
        .filter(|r| *r > 0)
        .map(|r| r.min(RATING_MAX))
}

/// One attribute of `path` as text. Missing, unsupported or unreadable is
/// nothing: none of these attributes is worth an error when read.
fn get(path: &Path, name: &str) -> Option<String> {
    let value = xattr::get_deref(path, name).ok().flatten()?;
    let text = String::from_utf8_lossy(&value).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Where a Mac's list of a file's tags was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kept {
    /// In an attribute of the file, under this name, stored this way.
    Attribute(String, finder::Framing),
    /// In the file's `._` file.
    Aside,
}

/// A file's tags as read: its own, then those only a Mac's lists hold,
/// and those lists, to be written again with them.
#[derive(Debug, Default)]
pub struct FileTags {
    pub tags: Vec<String>,
    pub finder: Vec<(Kept, Vec<finder::Tag>)>,
    /// The file's `._` file, where a Mac keeps its attributes on this
    /// volume.
    pub aside: Option<AppleDouble>,
}

impl FileTags {
    fn merge(
        mut tags: Vec<String>,
        finder: Vec<(Kept, Vec<finder::Tag>)>,
        aside: Option<AppleDouble>,
    ) -> Self {
        for (_, theirs) in &finder {
            for t in theirs {
                if !tags.iter().any(|x| same_tag(x, &t.name)) {
                    tags.push(t.name.clone());
                }
            }
        }
        FileTags {
            tags,
            finder,
            aside,
        }
    }

    /// The Finder color digit of each tag that has one.
    pub fn finder_colors(&self) -> Vec<(String, u8)> {
        let mut out: Vec<(String, u8)> = Vec::new();
        for t in self.finder.iter().flat_map(|(_, tags)| tags) {
            if t.color != 0 && !out.iter().any(|(x, _)| same_tag(x, &t.name)) {
                out.push((t.name.clone(), t.color));
            }
        }
        out
    }
}

/// The attribute names of `path`, through a link.
fn names(path: &Path) -> Vec<String> {
    xattr::list_deref(path)
        .map(|names| names.filter_map(|n| n.into_string().ok()).collect())
        .unwrap_or_default()
}

/// The `._` file of `path`, where a Mac keeps attributes on its volume.
fn aside_of(path: &Path) -> Option<AppleDouble> {
    appledouble::applies(path.parent()?)
        .then(|| appledouble::read(path))
        .flatten()
}

/// The Finder tags a `._` file holds, as a list found there.
fn aside_tags(aside: Option<&AppleDouble>) -> Option<(Kept, Vec<finder::Tag>)> {
    let value = aside?.get(finder::TAGS)?;
    Some((Kept::Aside, finder::parse(value, finder::Framing::Plain)?))
}

/// A file's tags, its own and a Mac's.
pub fn read_file_tags(path: &Path) -> FileTags {
    let names = names(path);
    let own = if names.iter().any(|n| n == TAGS) {
        get(path, TAGS).map(|t| parse_tags(&t)).unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut finder: Vec<(Kept, Vec<finder::Tag>)> = finder::read(path, &names)
        .into_iter()
        .map(|(name, framing, tags)| (Kept::Attribute(name, framing), tags))
        .collect();
    let aside = aside_of(path);
    finder.extend(aside_tags(aside.as_ref()));
    FileTags::merge(own, finder, aside)
}

/// Tags of a path, its own and a Mac's.
pub fn read_tags(path: &Path) -> Vec<String> {
    read_file_tags(path).tags
}

pub fn read_comment(path: &Path) -> Option<String> {
    get(path, COMMENT)
}

pub fn read_rating(path: &Path) -> Option<u64> {
    get(path, RATING).as_deref().and_then(parse_rating)
}

/// Write `value` to attribute `name` of `path`; `None` removes it, and
/// removing one that is not there succeeds.
pub fn write(path: &Path, name: &str, value: Option<&str>) -> std::io::Result<()> {
    match value {
        Some(value) => xattr::set_deref(path, name, value.as_bytes()),
        None => match xattr::remove_deref(path, name) {
            // ENODATA: the attribute was not there, which is what we wanted.
            Err(e)
                if rustix::io::Errno::from_io_error(&e) == Some(rustix::io::Errno::NODATA)
                    || e.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(())
            }
            r => r,
        },
    }
}

/// Write tags; an empty list removes the attribute.
pub fn write_tags(path: &Path, tags: &[String]) -> std::io::Result<()> {
    write_file_tags(path, tags, &[])
}

fn unsupported(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::Unsupported
        || rustix::io::Errno::from_io_error(error) == Some(rustix::io::Errno::NOTSUP)
}

/// Write `tags` as the file's: its own attribute, each Finder attribute it
/// carries and its `._` file, where a tag keeps the color Finder gave it
/// (or the one `colors` gives, what an undo puts back). An empty list
/// removes them. On a volume without attributes, the `._` file holds
/// them; with neither, the file keeps no tags and this fails. Nothing is
/// written unless all of it can be.
pub fn write_file_tags(
    path: &Path,
    tags: &[String],
    colors: &[(String, u8)],
) -> std::io::Result<()> {
    let current = read_file_tags(path);
    let given = |theirs: &[finder::Tag], tag: &str| {
        colors
            .iter()
            .find(|(t, _)| same_tag(t, tag))
            .map(|(_, c)| *c)
            .or_else(|| {
                theirs
                    .iter()
                    .find(|t| same_tag(&t.name, tag))
                    .map(|t| t.color)
            })
            .unwrap_or(0)
    };
    let finder_list = |theirs: &[finder::Tag], framing| {
        let ours: Vec<finder::Tag> = tags
            .iter()
            .map(|t| finder::Tag {
                name: t.clone(),
                color: given(theirs, t),
            })
            .collect();
        finder::encode(&ours, framing)
    };
    // The `._` file first: it is the part that can be refused.
    let aside = match current.aside {
        Some(mut aside) => {
            let theirs = current
                .finder
                .iter()
                .find(|(kept, _)| *kept == Kept::Aside)
                .map_or(&[][..], |(_, t)| t.as_slice());
            let value = (!tags.is_empty()).then(|| finder_list(theirs, finder::Framing::Plain));
            let changed = aside.set(finder::TAGS, value);
            if changed && aside.encode().is_none() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "a Mac keeps its tags beside it in a form bibli does not write",
                ));
            }
            changed.then_some(aside)
        }
        None => None,
    };
    let joined = tags.join(",");
    match write(path, TAGS, (!tags.is_empty()).then_some(joined.as_str())) {
        Err(e) if unsupported(&e) && (aside.is_some() || !current.finder.is_empty()) => {}
        Err(e) if unsupported(&e) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "its volume keeps no tags",
            ));
        }
        r => r?,
    }
    for (kept, theirs) in &current.finder {
        let Kept::Attribute(name, framing) = kept else {
            continue;
        };
        if tags.is_empty() {
            write(path, name, None)?;
        } else {
            xattr::set_deref(path, name, &finder_list(theirs, *framing))?;
        }
    }
    if let Some(aside) = aside {
        appledouble::write(path, &aside)?;
    }
    Ok(())
}

pub fn tags_field(tags: &[String]) -> Value {
    Value::List(
        tags.iter()
            .map(|t| Value::Text(Arc::from(t.as_str())))
            .collect(),
    )
}

/// Tags recorded in an entry's metadata, if the provider filled them.
pub fn tags_of(metadata: &Metadata) -> Option<Vec<String>> {
    match metadata.value(keys::TAGS)? {
        Value::List(items) => Some(
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
        ),
        _ => None,
    }
}

/// The names of the `._` files in the folders a batch was in (their
/// files' names), on volumes where a Mac writes them: a folder is read
/// once for its whole listing, not once per file, and again when it
/// changes.
#[derive(Default)]
pub struct AsideNames {
    folders: Mutex<Vec<Seen>>,
}

struct Seen {
    dir: PathBuf,
    modified: Option<SystemTime>,
    at: Instant,
    names: Arc<HashSet<OsString>>,
}

/// Folders remembered at once: a search walks several in parallel.
const FOLDERS: usize = 16;
/// How long a folder's names are trusted while its time says it did not
/// change: FAT stores times to two seconds.
const FRESH: Duration = Duration::from_secs(5);

impl AsideNames {
    /// The files of `dir` that have a `._` file; `None` for none, or on a
    /// volume where a Mac would not write one.
    fn of(&self, dir: &Path) -> Option<Arc<HashSet<OsString>>> {
        if !appledouble::applies(dir) {
            return None;
        }
        let modified = std::fs::metadata(dir).ok()?.modified().ok();
        let known = self
            .folders
            .lock()
            .iter()
            .find(|s| s.dir == dir && s.modified == modified && s.at.elapsed() < FRESH)
            .map(|s| Arc::clone(&s.names));
        if let Some(names) = known {
            return (!names.is_empty()).then_some(names);
        }
        let names: HashSet<OsString> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| {
                let name = e.ok()?.file_name();
                let file = name
                    .as_encoded_bytes()
                    .strip_prefix(appledouble::PREFIX.as_bytes())?;
                Some(os_from_bytes(file))
            })
            .collect();
        let names = Arc::new(names);
        let mut folders = self.folders.lock();
        folders.retain(|s| s.dir != dir);
        if folders.len() >= FOLDERS {
            folders.remove(0);
        }
        folders.push(Seen {
            dir: dir.to_path_buf(),
            modified,
            at: Instant::now(),
            names: Arc::clone(&names),
        });
        drop(folders);
        (!names.is_empty()).then_some(names)
    }
}

#[cfg(unix)]
fn os_from_bytes(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes).to_owned()
}

#[cfg(not(unix))]
fn os_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

/// A Mac's comment and download addresses: a comment, then the file's
/// address and the page's.
#[derive(Default)]
struct MacNotes {
    comment: Option<String>,
    where_froms: Vec<String>,
}

impl MacNotes {
    fn read(&mut self, mac: &str, value: &[u8], framing: finder::Framing) {
        match mac {
            finder::COMMENT if self.comment.is_none() => {
                self.comment = finder::parse_comment(value, framing);
            }
            finder::WHERE_FROMS if self.where_froms.is_empty() => {
                self.where_froms = finder::parse_where_froms(value, framing);
            }
            _ => {}
        }
    }

    /// Set what the file does not say itself.
    fn fill(self, metadata: &mut Metadata) {
        let mut set = |key: &str, text: Option<String>, provenance| {
            if let Some(text) = text
                && metadata.value(key).is_none()
            {
                metadata.set(key, Value::Text(text.into()), provenance);
            }
        };
        set(keys::COMMENT, self.comment, Provenance::User);
        let mut froms = self.where_froms.into_iter();
        set(keys::ORIGIN, froms.next(), Provenance::Filesystem);
        set(keys::REFERRER, froms.next(), Provenance::Filesystem);
    }
}

/// Fills the tags, rating, comment, origin and referrer fields from the
/// file's attributes (and a Mac's, in them or in its `._` file), and
/// tells the vocabulary the tags it saw and the index which files have
/// tags (and which no longer have).
pub struct AttributesProvider {
    pub vocabulary: Arc<Vocabulary>,
    pub index: Option<Arc<crate::hints::TagIndex>>,
    pub aside: AsideNames,
}

impl MetadataProvider for AttributesProvider {
    fn id(&self) -> &'static str {
        crate::ID
    }
    fn fill(&self, dir: &Path, entries: &mut [Entry]) {
        let mut seen: Vec<String> = Vec::new();
        let mut given: Vec<(String, &str)> = Vec::new();
        let mut read: Vec<(std::path::PathBuf, Vec<String>)> = Vec::new();
        let aside_names = self.aside.of(dir);
        for e in entries {
            let path = dir.join(&e.name);
            let names = names(&path);
            let aside = aside_names
                .as_ref()
                .filter(|n| n.contains(&e.name) && !appledouble::is_sidecar(&e.name))
                .and_then(|_| appledouble::read(&path));
            if names.is_empty() && aside.is_none() {
                if self.index.is_some() {
                    read.push((path, Vec::new()));
                }
                continue;
            }
            let mut own = Vec::new();
            let mut mac = MacNotes::default();
            for name in &names {
                let (key, provenance) = match name.as_str() {
                    TAGS => (keys::TAGS, Provenance::User),
                    COMMENT => (keys::COMMENT, Provenance::User),
                    RATING => (keys::RATING, Provenance::User),
                    ORIGIN => (keys::ORIGIN, Provenance::Filesystem),
                    REFERRER => (keys::REFERRER, Provenance::Filesystem),
                    other => {
                        if let Some((name_on_mac, framing)) = finder::mac_name(other)
                            && name_on_mac != finder::TAGS
                            && let Ok(Some(value)) = xattr::get_deref(&path, other)
                        {
                            mac.read(&name_on_mac, &value, framing);
                        }
                        continue;
                    }
                };
                let Some(text) = get(&path, name) else {
                    continue;
                };
                let value = match key {
                    keys::TAGS => {
                        own = parse_tags(&text);
                        continue;
                    }
                    keys::RATING => match parse_rating(&text) {
                        Some(rating) => Value::UInt(rating),
                        None => continue,
                    },
                    _ => Value::Text(text.into()),
                };
                e.metadata.set(key, value, provenance);
            }
            if let Some(aside) = &aside {
                for a in &aside.attributes {
                    mac.read(&a.name, &a.value, finder::Framing::Plain);
                }
            }
            mac.fill(&mut e.metadata);
            let mut finder: Vec<(Kept, Vec<finder::Tag>)> = finder::read(&path, &names)
                .into_iter()
                .map(|(name, framing, tags)| (Kept::Attribute(name, framing), tags))
                .collect();
            finder.extend(aside_tags(aside.as_ref()));
            let file = FileTags::merge(own, finder, None);
            if self.index.is_some() {
                read.push((path, file.tags.clone()));
            }
            if file.tags.is_empty() {
                continue;
            }
            e.metadata
                .set(keys::TAGS, tags_field(&file.tags), Provenance::User);
            let colors: Vec<Value> = file
                .finder_colors()
                .into_iter()
                .filter_map(|(tag, digit)| {
                    let color = bibli_core::metadata::TAG_COLORS[finder::palette(digit)?];
                    Some(Value::List(vec![
                        Value::Text(tag.into()),
                        Value::Text(color.into()),
                    ]))
                })
                .collect();
            if !colors.is_empty() {
                e.metadata
                    .set(keys::TAG_COLORS, Value::List(colors), Provenance::User);
            }
            given.extend(file.finder_colors().into_iter().filter_map(|(tag, digit)| {
                Some((
                    tag,
                    bibli_core::metadata::TAG_COLORS[finder::palette(digit)?],
                ))
            }));
            seen.extend(file.tags);
        }
        if !seen.is_empty() {
            self.vocabulary.saw(&seen);
        }
        if !given.is_empty() {
            self.vocabulary.given(&given);
        }
        if let Some(index) = &self.index {
            index.learn(read);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_ratings_parse_as_the_desktop_writes_them() {
        assert_eq!(parse_tags(" a, b ,a,,c "), ["a", "b", "c"]);
        assert_eq!(parse_tags("Work, work,WORK, Été, été"), ["Work", "Été"]);
        assert_eq!(parse_rating("8"), Some(8));
        assert_eq!(parse_rating(" 10\n"), Some(10));
        assert_eq!(parse_rating("12"), Some(10), "past five stars is five");
        assert_eq!(parse_rating("0"), None, "zero is unrated");
        assert_eq!(parse_rating("four"), None);
    }
}
