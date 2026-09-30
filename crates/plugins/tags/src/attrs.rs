//! The extended attributes the plugin reads and writes, and the provider
//! that fills their fields during hydration.
//!
//! Every attribute is read through what a link leads to, as the rest of an
//! entry is (a user attribute cannot be set on the link itself), and a file
//! is asked for its attribute names once: most files have none, so a
//! listing costs one call per entry whatever the number of keys read.

use std::path::Path;
use std::sync::Arc;

use bibli_core::entry::{Entry, Metadata, Provenance, Value, keys};
use bibli_core::metadata::{MetadataProvider, TAGS_XATTR, same_tag};

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

/// Tags of a path, from its xattr.
pub fn read_tags(path: &Path) -> Vec<String> {
    get(path, TAGS).map(|t| parse_tags(&t)).unwrap_or_default()
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
    let joined = tags.join(",");
    write(path, TAGS, (!tags.is_empty()).then_some(joined.as_str()))
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
/// file's attributes, and tells the vocabulary the tags it saw.
pub struct AttributesProvider {
    pub vocabulary: Arc<Vocabulary>,
}

impl MetadataProvider for AttributesProvider {
    fn id(&self) -> &'static str {
        crate::ID
    }
    fn fill(&self, dir: &Path, entries: &mut [Entry]) {
        let mut seen: Vec<String> = Vec::new();
        for e in entries {
            let path = dir.join(&e.name);
            let Ok(names) = xattr::list_deref(&path) else {
                continue;
            };
            for name in names {
                let Some(name) = name.to_str() else { continue };
                let (key, provenance) = match name {
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
                        let tags = parse_tags(&text);
                        if tags.is_empty() {
                            continue;
                        }
                        seen.extend(tags.iter().cloned());
                        tags_field(&tags)
                    }
                    keys::RATING => match parse_rating(&text) {
                        Some(rating) => Value::UInt(rating),
                        None => continue,
                    },
                    _ => Value::Text(text.into()),
                };
                e.metadata.set(key, value, provenance);
            }
        }
        if !seen.is_empty() {
            self.vocabulary.saw(&seen);
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
