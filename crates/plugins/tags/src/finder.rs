//! Finder tags: the tags a Mac gives a file, read and kept in step with
//! ours when a file carries them.
//!
//! macOS keeps a file's tags in the attribute
//! `com.apple.metadata:_kMDItemUserTags`: a binary property list, an array
//! of strings, each a tag's name and, after a newline, the digit of its
//! color (`"Work\n4"`; no newline, no color). Linux has no attribute
//! without a namespace, so the attribute reaches it under one of these
//! names, depending on how the file came:
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
//!   byte of Samba's own.
//!
//! The value's format is Apple's; the tests write it with the same
//! property-list encoder. The Samba form follows Samba's storage (the
//! trailing NUL of `vfs_streams_xattr`) and was not seen written by a Mac.

use std::path::Path;

/// The attribute's name on a Mac.
const NAME: &str = "com.apple.metadata:_kMDItemUserTags";

/// How a Finder attribute's value is stored under the name it was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    /// The property list as is.
    Plain,
    /// A Samba stream: the property list, then a NUL byte.
    Stream,
}

/// Whether the attribute `name` holds Finder tags, and how.
pub fn framing(name: &str) -> Option<Framing> {
    if let Some(stream) = name.strip_prefix("user.DosStream.") {
        let stream = stream.strip_suffix(":$DATA").unwrap_or(stream);
        let mac = stream.replace('\u{F022}', ":");
        return (mac == NAME).then_some(Framing::Stream);
    }
    let rest = name
        .strip_prefix("user.")
        .or_else(|| name.strip_prefix("osx."))?;
    (rest == NAME).then_some(Framing::Plain)
}

/// A Finder tag: its name and its color digit (0 for none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub color: u8,
}

/// The tags in a Finder attribute's value; `None` when it is not one.
pub fn parse(value: &[u8], framing: Framing) -> Option<Vec<Tag>> {
    let body = match framing {
        Framing::Plain => value,
        Framing::Stream => value.strip_suffix(&[0]).unwrap_or(value),
    };
    let plist::Value::Array(items) = plist::Value::from_reader(std::io::Cursor::new(body)).ok()?
    else {
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

/// The palette index (`bibli_core::metadata::TAG_COLORS`) of a Finder
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
    bibli_core::metadata::TAG_COLORS
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
            let value = xattr::get_deref(path, name).ok().flatten()?;
            Some((name.clone(), framing, parse(&value, framing)?))
        })
        .collect()
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
            palette(6).map(|i| bibli_core::metadata::TAG_COLORS[i]),
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
