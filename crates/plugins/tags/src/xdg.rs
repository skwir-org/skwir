//! The desktop's notes in the file's extended attributes, as KDE's Dolphin
//! and Baloo keep them: tags in `user.xdg.tags` (comma-separated, the
//! freedesktop convention), a rating in `user.baloo.rating` (half stars),
//! a comment in `user.xdg.comment`, and where a download came from in
//! `user.xdg.origin.url` and `user.xdg.referrer.url` (browsers and
//! download tools write them; read only).
//!
//! Every attribute is read through what a link leads to, as the rest of an
//! entry is, and only those the batch's one `listxattr` per file named.

use std::path::Path;

use bibli_core::metadata::TAGS_XATTR;
use bibli_core::notes::raw::RawKey;
use bibli_core::notes::{
    Holds, Kept, Notes, NotesStore, Probe, RATING_MAX, Tag, Write, Wrote, parse_tags,
};

pub const TAGS: &str = TAGS_XATTR;
pub const COMMENT: &str = "user.xdg.comment";
pub const RATING: &str = "user.baloo.rating";
pub const ORIGIN: &str = "user.xdg.origin.url";
pub const REFERRER: &str = "user.xdg.referrer.url";

/// The attributes an edit writes.
const WRITTEN: [&str; 3] = [TAGS, RATING, COMMENT];

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
pub fn get(path: &Path, name: &str) -> Option<String> {
    let value = xattr::get_deref(path, name).ok().flatten()?;
    let text = String::from_utf8_lossy(&value).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn refused(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::Unsupported
        || rustix::io::Errno::from_io_error(e) == Some(rustix::io::Errno::NOTSUP)
}

fn missing(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::NotFound
        || rustix::io::Errno::from_io_error(e) == Some(rustix::io::Errno::NODATA)
}

/// Write `value` to attribute `name` of `path`; `None` removes it, and
/// removing one that is not there succeeds.
pub fn write(path: &Path, name: &str, value: Option<&str>) -> std::io::Result<()> {
    match value {
        Some(value) => xattr::set_deref(path, name, value.as_bytes()),
        None => match xattr::remove_deref(path, name) {
            Err(e) if missing(&e) => Ok(()),
            r => r,
        },
    }
}

/// The notes in the XDG attributes named `names`.
pub fn notes_of(path: &Path, names: &[String]) -> Option<Notes> {
    let has = |n: &str| names.iter().any(|x| x == n);
    let text = |n: &str| has(n).then(|| get(path, n)).flatten();
    let notes = Notes {
        tags: text(TAGS)
            .map(|t| parse_tags(&t).into_iter().map(Tag::named).collect())
            .unwrap_or_default(),
        rating: text(RATING).as_deref().and_then(parse_rating),
        comment: text(COMMENT),
        origin: text(ORIGIN),
        referrer: text(REFERRER),
    };
    (!notes.is_empty()).then_some(notes)
}

/// The values an edit writes, as text: a tag holding a comma cannot be in
/// a comma-separated list, and is kept by the stores that can hold it.
pub fn written(notes: &Notes) -> [Option<String>; 3] {
    let tags: Vec<&str> = notes
        .tags
        .iter()
        .map(|t| t.name.as_str())
        .filter(|t| !t.contains(','))
        .collect();
    [
        (!tags.is_empty()).then(|| tags.join(",")),
        notes.rating.map(|r| r.to_string()),
        notes.comment.clone(),
    ]
}

/// The desktop's attributes as a store.
pub struct XdgStore;

impl NotesStore for XdgStore {
    fn id(&self) -> &'static str {
        "xdg"
    }
    fn title(&self) -> &'static str {
        "The desktop's attributes (KDE, Dolphin, Baloo)"
    }
    fn kept(&self) -> Kept {
        Kept::Attributes
    }
    fn holds(&self) -> Holds {
        Holds {
            tags: true,
            tag_colors: false,
            rating: true,
            comment: true,
            origin: true,
            limits: "no comma in a tag, no tag colors",
        }
    }
    fn default_write(&self) -> Write {
        Write::Create
    }
    fn fit(&self, notes: &Notes) -> Notes {
        Notes {
            tags: notes
                .tags
                .iter()
                .filter(|t| !t.name.contains(','))
                .map(|t| Tag::named(t.name.clone()))
                .collect(),
            rating: notes.rating,
            comment: notes.comment.clone(),
            origin: None,
            referrer: None,
        }
    }
    fn read(&self, _dir: &Path, files: &[Probe<'_>]) -> Vec<Option<Notes>> {
        files
            .iter()
            .map(|f| {
                let ours = f
                    .attributes
                    .iter()
                    .any(|n| n.starts_with("user.xdg.") || n == RATING);
                ours.then(|| notes_of(f.path, f.attributes)).flatten()
            })
            .collect()
    }
    fn keys(&self, file: &Probe<'_>, create: bool) -> Vec<RawKey> {
        let kept = file
            .attributes
            .iter()
            .any(|n| WRITTEN.contains(&n.as_str()));
        if !kept && !create {
            return Vec::new();
        }
        WRITTEN
            .iter()
            .map(|name| RawKey::Attribute {
                name: (*name).to_owned(),
            })
            .collect()
    }
    fn write(&self, file: &Probe<'_>, notes: &Notes, create: bool) -> std::io::Result<Wrote> {
        let kept = file
            .attributes
            .iter()
            .any(|n| WRITTEN.contains(&n.as_str()));
        let values = written(notes);
        if !kept && (!create || values.iter().all(Option::is_none)) {
            return Ok(Wrote::NotHere);
        }
        let mut changed = false;
        for (name, value) in WRITTEN.iter().zip(&values) {
            let now = file
                .attributes
                .iter()
                .any(|n| n == name)
                .then(|| get(file.path, name))
                .flatten();
            if now.as_deref() == value.as_deref() {
                continue;
            }
            match write(file.path, name, value.as_deref()) {
                Ok(()) => changed = true,
                // The volume keeps no attributes: another store keeps them.
                Err(e) if refused(&e) && !changed => return Ok(Wrote::NotHere),
                Err(e) => return Err(e),
            }
        }
        Ok(if changed {
            Wrote::Written
        } else {
            Wrote::Unchanged
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rating_reads_as_dolphin_writes_it() {
        assert_eq!(parse_rating("8"), Some(8));
        assert_eq!(parse_rating(" 10\n"), Some(10));
        assert_eq!(parse_rating("12"), Some(10), "past five stars is five");
        assert_eq!(parse_rating("0"), None, "zero is unrated");
        assert_eq!(parse_rating("four"), None);
    }

    #[test]
    fn a_tag_with_a_comma_is_left_to_the_stores_that_hold_it() {
        let notes = Notes {
            tags: vec![Tag::named("a, b"), Tag::named("c")],
            rating: Some(7),
            ..Notes::default()
        };
        assert_eq!(
            written(&notes),
            [Some("c".to_owned()), Some("7".to_owned()), None]
        );
    }
}
