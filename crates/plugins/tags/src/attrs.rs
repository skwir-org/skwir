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
//! both, so an edit here holds on the Mac too.

use std::path::Path;
use std::sync::Arc;

use bibli_core::entry::{Entry, Metadata, Provenance, Value, keys};
use bibli_core::metadata::{MetadataProvider, TAGS_XATTR, same_tag};

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

/// A file's tags as read: its own, then those only its Finder attributes
/// hold, and the Finder attributes found, to be written again with them.
#[derive(Debug, Default)]
pub struct FileTags {
    pub tags: Vec<String>,
    pub finder: Vec<(String, finder::Framing, Vec<finder::Tag>)>,
}

impl FileTags {
    fn merge(
        mut tags: Vec<String>,
        finder: Vec<(String, finder::Framing, Vec<finder::Tag>)>,
    ) -> Self {
        for (_, _, theirs) in &finder {
            for t in theirs {
                if !tags.iter().any(|x| same_tag(x, &t.name)) {
                    tags.push(t.name.clone());
                }
            }
        }
        FileTags { tags, finder }
    }

    /// The Finder color digit of each tag that has one.
    pub fn finder_colors(&self) -> Vec<(String, u8)> {
        let mut out: Vec<(String, u8)> = Vec::new();
        for t in self.finder.iter().flat_map(|(_, _, tags)| tags) {
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

/// A file's tags, its own and a Mac's.
pub fn read_file_tags(path: &Path) -> FileTags {
    let names = names(path);
    let own = if names.iter().any(|n| n == TAGS) {
        get(path, TAGS).map(|t| parse_tags(&t)).unwrap_or_default()
    } else {
        Vec::new()
    };
    FileTags::merge(own, finder::read(path, &names))
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

/// Write `tags` as the file's: its own attribute, and each Finder attribute
/// it carries, where a tag keeps the color Finder gave it (or the one
/// `colors` gives, what an undo puts back). An empty list removes them.
pub fn write_file_tags(
    path: &Path,
    tags: &[String],
    colors: &[(String, u8)],
) -> std::io::Result<()> {
    let current = read_file_tags(path);
    let joined = tags.join(",");
    write(path, TAGS, (!tags.is_empty()).then_some(joined.as_str()))?;
    for (name, framing, theirs) in &current.finder {
        if tags.is_empty() {
            write(path, name, None)?;
            continue;
        }
        let given = |tag: &str| {
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
        let ours: Vec<finder::Tag> = tags
            .iter()
            .map(|t| finder::Tag {
                name: t.clone(),
                color: given(t),
            })
            .collect();
        xattr::set_deref(path, name, &finder::encode(&ours, *framing))?;
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

/// Fills the tags, rating, comment, origin and referrer fields from the
/// file's attributes, and tells the vocabulary the tags it saw and the
/// index which files have tags (and which no longer have).
pub struct AttributesProvider {
    pub vocabulary: Arc<Vocabulary>,
    pub index: Option<Arc<crate::hints::TagIndex>>,
}

impl MetadataProvider for AttributesProvider {
    fn id(&self) -> &'static str {
        crate::ID
    }
    fn fill(&self, dir: &Path, entries: &mut [Entry]) {
        let mut seen: Vec<String> = Vec::new();
        let mut read: Vec<(std::path::PathBuf, Vec<String>)> = Vec::new();
        for e in entries {
            let path = dir.join(&e.name);
            let names = names(&path);
            if names.is_empty() {
                if self.index.is_some() {
                    read.push((path, Vec::new()));
                }
                continue;
            }
            let mut own = Vec::new();
            for name in &names {
                let (key, provenance) = match name.as_str() {
                    TAGS => (keys::TAGS, Provenance::User),
                    COMMENT => (keys::COMMENT, Provenance::User),
                    RATING => (keys::RATING, Provenance::User),
                    ORIGIN => (keys::ORIGIN, Provenance::Filesystem),
                    REFERRER => (keys::REFERRER, Provenance::Filesystem),
                    _ => continue,
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
            let file = FileTags::merge(own, finder::read(&path, &names));
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
            seen.extend(file.tags);
        }
        if !seen.is_empty() {
            self.vocabulary.saw(&seen);
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
