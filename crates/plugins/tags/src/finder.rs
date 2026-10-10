//! What a Mac says of a file, read with ours: its Finder tags (kept in
//! step with ours when a file carries them), its Finder comment and where
//! it was downloaded from.
//!
//! macOS keeps a file's tags in the attribute
//! `com.apple.metadata:_kMDItemUserTags`: a binary property list, an array
//! of strings, each a tag's name and, after a newline, the digit of its
//! color (`"Work\n4"`; no newline, no color). The comment Get Info shows
//! is `com.apple.metadata:kMDItemFinderComment`, a property list string,
//! and a download's addresses are `com.apple.metadata:kMDItemWhereFroms`,
//! an array: the file's, then the page's. Linux has no attribute without
//! a namespace, so an attribute reaches it under one of these names,
//! depending on how the file came:
//!
//! - `user.com.apple.metadata:_kMDItemUserTags`: copied with its
//!   attributes (`rsync -X`, `tar --xattrs`) or kept by netatalk
//!   (`ea = sys`);
//! - `osx.com.apple.metadata:_kMDItemUserTags`: on an HFS+ volume read
//!   by Linux's own driver, which puts a Mac's attributes under `osx.`;
//! - `user.DosStream.com.apple.metadata:_kMDItemUserTags:$DATA`: written
//!   by a Mac to a Samba share (`vfs_fruit` with `vfs_streams_xattr`),
//!   which keeps each of the client's attributes as a stream, the colon
//!   possibly as U+F022 (the Mac's mapping of characters Windows forbids,
//!   kept by `fruit:encoding = private`), and the value ending with a NUL
//!   byte of Samba's own;
//! - no attribute at all, on a filesystem without them (a USB stick): the
//!   Mac wrote them to the file's `._` file (`appledouble`), under the
//!   Mac's own names.
//!
//! The values' format is Apple's; the tests write it with the same
//! property-list encoder. The Samba form follows Samba's storage (the
//! trailing NUL of `vfs_streams_xattr`) and was not seen written by a Mac.

use std::path::Path;

use skwir_core::metadata::same_tag;
use skwir_core::notes::raw::RawKey;
use skwir_core::notes::{Holds, Kept, Notes, NotesStore, Probe, Tag as NoteTag, Write, Wrote};

/// The attributes' names on a Mac.
pub const TAGS: &str = "com.apple.metadata:_kMDItemUserTags";
pub const COMMENT: &str = "com.apple.metadata:kMDItemFinderComment";
pub const WHERE_FROMS: &str = "com.apple.metadata:kMDItemWhereFroms";

/// How a Finder attribute's value is stored under the name it was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    /// The property list as is.
    Plain,
    /// A Samba stream: the property list, then a NUL byte.
    Stream,
}

/// The name a Mac gives the attribute Linux calls `name`, and how its
/// value is stored; `None` when it is not a Mac's.
pub fn mac_name(name: &str) -> Option<(String, Framing)> {
    if let Some(stream) = name.strip_prefix("user.DosStream.") {
        let stream = stream.strip_suffix(":$DATA").unwrap_or(stream);
        let mac = stream.replace('\u{F022}', ":");
        return mac
            .starts_with("com.apple.")
            .then_some((mac, Framing::Stream));
    }
    let rest = name
        .strip_prefix("user.")
        .or_else(|| name.strip_prefix("osx."))?;
    rest.starts_with("com.apple.")
        .then(|| (rest.to_owned(), Framing::Plain))
}

/// Whether the attribute `name` holds Finder tags, and how.
pub fn framing(name: &str) -> Option<Framing> {
    mac_name(name)
        .filter(|(mac, _)| mac == TAGS)
        .map(|(_, framing)| framing)
}

fn plist_of(value: &[u8], framing: Framing) -> Option<plist::Value> {
    let body = match framing {
        Framing::Plain => value,
        Framing::Stream => value.strip_suffix(&[0]).unwrap_or(value),
    };
    plist::Value::from_reader(std::io::Cursor::new(body)).ok()
}

/// A Finder comment's text; `None` when it is empty or not one.
pub fn parse_comment(value: &[u8], framing: Framing) -> Option<String> {
    let text = plist_of(value, framing)?.into_string()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// A download's addresses: the file's, then the page it was found on.
pub fn parse_where_froms(value: &[u8], framing: Framing) -> Vec<String> {
    let Some(plist::Value::Array(items)) = plist_of(value, framing) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(plist::Value::into_string)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

/// A Finder tag: its name and its color digit (0 for none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub color: u8,
}

/// The tags in a Finder attribute's value; `None` when it is not one.
pub fn parse(value: &[u8], framing: Framing) -> Option<Vec<Tag>> {
    let plist::Value::Array(items) = plist_of(value, framing)? else {
        return None;
    };
    Some(
        items
            .iter()
            .filter_map(plist::Value::as_string)
            .filter_map(|item| {
                let (name, color) = match item.rsplit_once('\n') {
                    Some((name, digit)) => (name, digit.trim().parse().unwrap_or(0)),
                    None => (item, 0),
                };
                let name = name.trim();
                (!name.is_empty()).then(|| Tag {
                    name: name.to_owned(),
                    color: if color <= 7 { color } else { 0 },
                })
            })
            .collect(),
    )
}

/// A Finder attribute's value for `tags`, framed as it was found.
pub fn encode(tags: &[Tag], framing: Framing) -> Vec<u8> {
    let items = tags
        .iter()
        .map(|t| {
            plist::Value::String(if t.color == 0 {
                t.name.clone()
            } else {
                format!("{}\n{}", t.name, t.color)
            })
        })
        .collect();
    let mut out = Vec::new();
    plist::Value::Array(items)
        .to_writer_binary(&mut out)
        .expect("writing to memory");
    if framing == Framing::Stream {
        out.push(0);
    }
    out
}

/// The palette index (`skwir_core::metadata::TAG_COLORS`) of a Finder
/// color digit: 1 gray, 2 green, 3 purple, 4 blue, 5 yellow, 6 red,
/// 7 orange. Gray has no counterpart and draws as the name's color.
pub fn palette(digit: u8) -> Option<usize> {
    let name = match digit {
        2 => "green",
        3 => "purple",
        4 => "blue",
        5 => "yellow",
        6 => "red",
        7 => "orange",
        _ => return None,
    };
    skwir_core::metadata::TAG_COLORS
        .iter()
        .position(|c| *c == name)
}

/// The Finder attributes of `path` among its attribute `names`, each with
/// its tags. Unreadable or malformed ones are left out, never rewritten.
pub fn read(path: &Path, names: &[String]) -> Vec<(String, Framing, Vec<Tag>)> {
    names
        .iter()
        .filter_map(|name| {
            let framing = framing(name)?;
            let value = skwir_core::fs::xattr::get_deref(path, name)
                .ok()
                .flatten()?;
            Some((name.clone(), framing, parse(&value, framing)?))
        })
        .collect()
}

/// The Finder digit of a palette color: the one [`palette`] maps to it.
pub fn digit(color: &str) -> Option<u8> {
    (2..=7).find(|d| palette(*d).map(|i| skwir_core::metadata::TAG_COLORS[i]) == Some(color))
}

/// A Finder list of tags as the model's: a digit becomes a palette color.
pub fn to_notes(tags: Vec<Tag>) -> Vec<NoteTag> {
    tags.into_iter()
        .map(|t| NoteTag {
            color: palette(t.color).map(|i| skwir_core::metadata::TAG_COLORS[i].to_owned()),
            name: t.name,
        })
        .collect()
}

/// `notes`' tags as a Finder list: a tag keeps the digit `theirs` gave it
/// (gray, which has no color of ours, included) unless the notes give it
/// another color.
pub fn from_notes(notes: &Notes, theirs: &[Tag]) -> Vec<Tag> {
    notes
        .tags
        .iter()
        .map(|t| {
            let had = theirs
                .iter()
                .find(|x| same_tag(&x.name, &t.name))
                .map(|x| x.color);
            let given = t.color.as_deref().and_then(digit);
            let color = match (had, given) {
                (Some(had), Some(given)) if palette(had).is_some() && had != given => given,
                (Some(had), _) => had,
                (None, given) => given.unwrap_or(0),
            };
            Tag {
                name: t.name.clone(),
                color,
            }
        })
        .collect()
}

/// A Finder comment as the Mac writes it: a property list string.
pub fn encode_comment(text: &str, framing: Framing) -> Vec<u8> {
    let mut out = Vec::new();
    plist::Value::String(text.to_owned())
        .to_writer_binary(&mut out)
        .expect("writing to memory");
    if framing == Framing::Stream {
        out.push(0);
    }
    out
}

/// What a Mac's attributes say, each given by its Mac name, value and
/// framing; `None` when they say nothing.
pub fn notes_from<'a>(
    attributes: impl IntoIterator<Item = (&'a str, &'a [u8], Framing)>,
) -> Option<Notes> {
    let mut notes = Notes::default();
    for (name, value, framing) in attributes {
        match name {
            TAGS => {
                if let Some(tags) = parse(value, framing) {
                    notes.merge(Notes {
                        tags: to_notes(tags),
                        ..Notes::default()
                    });
                }
            }
            COMMENT if notes.comment.is_none() => notes.comment = parse_comment(value, framing),
            WHERE_FROMS if notes.origin.is_none() => {
                let mut froms = parse_where_froms(value, framing).into_iter();
                notes.origin = froms.next();
                notes.referrer = froms.next();
            }
            _ => {}
        }
    }
    (!notes.is_empty()).then_some(notes)
}

/// The name a Finder list of tags is created under on this system.
#[cfg(target_os = "macos")]
const CREATED: &str = TAGS;
#[cfg(not(target_os = "macos"))]
const CREATED: &str = "user.com.apple.metadata:_kMDItemUserTags";

/// A Mac's Finder tags, comment and download addresses in the file's
/// attributes, under whichever name they reached this system.
pub struct FinderStore;

impl FinderStore {
    /// The Finder attributes among `names`: Linux's name, the Mac's, the
    /// framing.
    fn present(names: &[String]) -> Vec<(String, String, Framing)> {
        names
            .iter()
            .filter_map(|n| {
                let (mac, framing) = mac_name(n)?;
                matches!(mac.as_str(), TAGS | COMMENT | WHERE_FROMS)
                    .then(|| (n.clone(), mac, framing))
            })
            .collect()
    }
}

impl NotesStore for FinderStore {
    fn id(&self) -> &'static str {
        "finder"
    }
    fn title(&self) -> &'static str {
        "A Mac's Finder tags and comments"
    }
    fn kept(&self) -> Kept {
        Kept::Attributes
    }
    fn holds(&self) -> Holds {
        Holds {
            tags: true,
            tag_colors: true,
            rating: false,
            comment: true,
            origin: true,
            limits: "no rating; seven colors",
        }
    }
    fn default_write(&self) -> Write {
        if cfg!(target_os = "macos") {
            Write::Create
        } else {
            Write::Update
        }
    }
    fn read(
        &self,
        _dir: &Path,
        files: &[Probe<'_>],
        _: Option<&skwir_core::metadata::Listed<'_>>,
    ) -> Vec<Option<Notes>> {
        files
            .iter()
            .map(|f| {
                let present = Self::present(f.attributes);
                if present.is_empty() {
                    return None;
                }
                let values: Vec<(String, Vec<u8>, Framing)> = present
                    .into_iter()
                    .filter_map(|(linux, mac, framing)| {
                        Some((
                            mac,
                            skwir_core::fs::xattr::get_deref(f.path, &linux).ok()??,
                            framing,
                        ))
                    })
                    .collect();
                notes_from(
                    values
                        .iter()
                        .map(|(m, v, fr)| (m.as_str(), v.as_slice(), *fr)),
                )
            })
            .collect()
    }
    fn keys(&self, file: &Probe<'_>, create: bool) -> Vec<RawKey> {
        let present = Self::present(file.attributes);
        let has_tags = present.iter().any(|(_, mac, _)| mac == TAGS);
        let mut keys: Vec<RawKey> = present
            .into_iter()
            .filter(|(_, mac, _)| mac != WHERE_FROMS)
            .map(|(name, _, _)| RawKey::Attribute { name })
            .collect();
        if create && !has_tags {
            keys.push(RawKey::Attribute {
                name: CREATED.to_owned(),
            });
        }
        keys
    }
    fn write(&self, file: &Probe<'_>, notes: &Notes, create: bool) -> std::io::Result<Wrote> {
        let present = Self::present(file.attributes);
        let mut changed = false;
        let mut kept = false;
        let mut set = |name: &str, value: Option<Vec<u8>>| -> std::io::Result<()> {
            let now = skwir_core::fs::xattr::get_deref(file.path, name)
                .ok()
                .flatten();
            if now == value {
                return Ok(());
            }
            match value {
                Some(value) => skwir_core::fs::xattr::set_deref(file.path, name, &value)?,
                None => match skwir_core::fs::xattr::remove_deref(file.path, name) {
                    Err(e) if skwir_core::fs::xattr::missing(&e) => {}
                    r => r?,
                },
            }
            changed = true;
            Ok(())
        };
        let mut has_tags = false;
        for (linux, mac, framing) in &present {
            kept = true;
            match mac.as_str() {
                TAGS => {
                    has_tags = true;
                    let theirs = skwir_core::fs::xattr::get_deref(file.path, linux)
                        .ok()
                        .flatten()
                        .and_then(|v| parse(&v, *framing))
                        .unwrap_or_default();
                    let value = (!notes.tags.is_empty())
                        .then(|| encode(&from_notes(notes, &theirs), *framing));
                    set(linux, value)?;
                }
                COMMENT => {
                    let value = notes
                        .comment
                        .as_deref()
                        .map(|c| encode_comment(c, *framing));
                    set(linux, value)?;
                }
                _ => {}
            }
        }
        if !has_tags && create && !notes.tags.is_empty() {
            match set(
                CREATED,
                Some(encode(&from_notes(notes, &[]), Framing::Plain)),
            ) {
                Ok(()) => kept = true,
                Err(e) if skwir_core::fs::xattr::refused(&e) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(match (changed, kept) {
            (true, _) => Wrote::Written,
            (false, true) => Wrote::Unchanged,
            (false, false) => Wrote::NotHere,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_a_finder_attribute_reaches_linux_under() {
        assert_eq!(
            framing("user.com.apple.metadata:_kMDItemUserTags"),
            Some(Framing::Plain)
        );
        assert_eq!(
            framing("osx.com.apple.metadata:_kMDItemUserTags"),
            Some(Framing::Plain)
        );
        assert_eq!(
            framing("user.DosStream.com.apple.metadata:_kMDItemUserTags:$DATA"),
            Some(Framing::Stream)
        );
        assert_eq!(
            framing("user.DosStream.com.apple.metadata\u{F022}_kMDItemUserTags:$DATA"),
            Some(Framing::Stream)
        );
        for other in [
            "user.xdg.tags",
            "user.com.apple.FinderInfo",
            "com.apple.metadata:_kMDItemUserTags",
            "trusted.com.apple.metadata:_kMDItemUserTags",
            "user.DosStream.AFP_AfpInfo:$DATA",
        ] {
            assert_eq!(framing(other), None, "{other}");
        }
    }

    #[test]
    fn a_comment_and_where_a_download_came_from_read_as_a_mac_writes_them() {
        let mut comment = Vec::new();
        plist::Value::String(" For the report \n".into())
            .to_writer_binary(&mut comment)
            .unwrap();
        assert_eq!(
            parse_comment(&comment, Framing::Plain).as_deref(),
            Some("For the report")
        );
        comment.push(0);
        assert_eq!(
            parse_comment(&comment, Framing::Stream).as_deref(),
            Some("For the report")
        );
        let mut froms = Vec::new();
        plist::Value::Array(vec![
            plist::Value::String("https://example.org/a.pdf".into()),
            plist::Value::String("https://example.org/".into()),
        ])
        .to_writer_binary(&mut froms)
        .unwrap();
        assert_eq!(
            parse_where_froms(&froms, Framing::Plain),
            ["https://example.org/a.pdf", "https://example.org/"]
        );
        assert_eq!(parse_comment(&froms, Framing::Plain), None);
        assert!(parse_where_froms(b"x", Framing::Plain).is_empty());
        assert_eq!(
            mac_name("osx.com.apple.metadata:kMDItemWhereFroms"),
            Some((WHERE_FROMS.to_owned(), Framing::Plain))
        );
        assert_eq!(
            mac_name("user.DosStream.com.apple.metadata\u{F022}kMDItemFinderComment:$DATA"),
            Some((COMMENT.to_owned(), Framing::Stream))
        );
        assert_eq!(mac_name("user.xdg.comment"), None);
    }

    #[test]
    fn tags_and_their_colors_survive_a_round_trip() {
        let tags = vec![
            Tag {
                name: "Red".into(),
                color: 6,
            },
            Tag {
                name: "it's work".into(),
                color: 0,
            },
            Tag {
                name: "Été".into(),
                color: 4,
            },
        ];
        for framing in [Framing::Plain, Framing::Stream] {
            let bytes = encode(&tags, framing);
            assert!(bytes.starts_with(b"bplist00"));
            assert_eq!(bytes.last() == Some(&0), framing == Framing::Stream);
            assert_eq!(parse(&bytes, framing).as_ref(), Some(&tags));
        }
        assert_eq!(
            palette(6).map(|i| skwir_core::metadata::TAG_COLORS[i]),
            Some("red")
        );
        assert_eq!(palette(1), None, "gray has no counterpart");
        assert_eq!(palette(0), None);
    }

    #[test]
    fn what_is_not_a_list_of_tags_is_nothing() {
        assert_eq!(parse(b"work,home", Framing::Plain), None);
        assert_eq!(parse(b"", Framing::Plain), None);
        let mut dict = Vec::new();
        plist::Value::Dictionary(plist::Dictionary::new())
            .to_writer_binary(&mut dict)
            .unwrap();
        assert_eq!(parse(&dict, Framing::Plain), None);
        // A color past the palette, blanks and a bare name.
        let mut odd = Vec::new();
        plist::Value::Array(vec![
            plist::Value::String("A\n9".into()),
            plist::Value::String("  \n2".into()),
            plist::Value::Integer(3.into()),
            plist::Value::String("B".into()),
        ])
        .to_writer_binary(&mut odd)
        .unwrap();
        assert_eq!(
            parse(&odd, Framing::Plain).unwrap(),
            [
                Tag {
                    name: "A".into(),
                    color: 0
                },
                Tag {
                    name: "B".into(),
                    color: 0
                }
            ]
        );
    }
}
