//! Entries: stable identity plus a metadata bag where every field records
//! where it came from.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Stable identity of a filesystem object: device and inode on Unix, volume
/// and file index on Windows. Survives renames and moves within a device.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
pub struct EntryId {
    /// Device (or volume) number.
    pub dev: u64,
    /// Inode (or file index) on that device.
    pub ino: u64,
}

impl EntryId {
    /// The device number no filesystem gives: a place's (ADR-032).
    pub const PLACE_DEV: u64 = u64::MAX;

    /// A place's identity, from what makes it the place it is (a tag's
    /// name, hashed by its provider): the same each time it is listed.
    pub const fn place(key: u64) -> EntryId {
        EntryId {
            dev: Self::PLACE_DEV,
            ino: key,
        }
    }

    /// Whether this is a place's: an entry that is no file but a location
    /// of its own, shown as a folder (a tag's in a tag location), which
    /// opens where `OpenLocation::location_of` says and has no path.
    pub const fn is_place(self) -> bool {
        self.dev == Self::PLACE_DEV
    }
}

/// Kind as reported by the directory listing (`d_type`), before any stat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    /// A named pipe: opening it to read waits for a writer.
    Fifo,
    Socket,
    CharDevice,
    BlockDevice,
    /// The filesystem did not report a type; a stat will resolve it.
    Unknown,
}

impl EntryKind {
    /// A pipe, a socket or a device: listed and acted on like any entry,
    /// never read (a pipe waits for a writer, a device may act when
    /// opened), so never previewed, thumbnailed or typed by its name.
    pub fn is_special(self) -> bool {
        matches!(
            self,
            EntryKind::Fifo | EntryKind::Socket | EntryKind::CharDevice | EntryKind::BlockDevice
        )
    }

    /// The MIME type of what is of this kind whatever its name, as the
    /// desktop's database names it; `None` for a file, typed by its name,
    /// and for a link.
    pub fn inode_type(self) -> Option<&'static str> {
        match self {
            EntryKind::Directory => Some("inode/directory"),
            EntryKind::Fifo => Some("inode/fifo"),
            EntryKind::Socket => Some("inode/socket"),
            EntryKind::CharDevice => Some("inode/chardevice"),
            EntryKind::BlockDevice => Some("inode/blockdevice"),
            EntryKind::File | EntryKind::Symlink | EntryKind::Unknown => None,
        }
    }
}

/// Where a metadata value came from. Nothing AI-derived is authoritative on its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum Provenance {
    /// Read from the filesystem (stat, xattr, etc.).
    Filesystem,
    /// Set or confirmed by the user.
    User,
    /// Computed by a plugin deterministically.
    Plugin { name: Arc<str> },
    /// Suggested by a model; a suggestion until the user confirms it.
    Model { name: Arc<str>, confidence: f32 },
}

/// A typed metadata value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Text(Arc<str>),
    Time(SystemTime),
    List(Vec<Value>),
}

impl Value {
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::UInt(v) => Some(*v),
            Value::Int(v) => u64::try_from(*v).ok(),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_time(&self) -> Option<SystemTime> {
        match self {
            Value::Time(t) => Some(*t),
            _ => None,
        }
    }
}

/// A value plus its provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Field {
    pub value: Value,
    pub provenance: Provenance,
}

/// Well-known field keys. Plugins add their own, namespaced `plugin.key`.
pub mod keys {
    pub const SIZE: &str = "size";
    pub const MODIFIED: &str = "modified";
    pub const CREATED: &str = "created";
    pub const ACCESSED: &str = "accessed";
    pub const MODE: &str = "mode";
    pub const NLINK: &str = "nlink";
    pub const UID: &str = "uid";
    pub const GID: &str = "gid";
    /// Symlink target, present for symlinks after stat.
    pub const LINK_TARGET: &str = "link_target";
    /// The user's tags, a `Value::List` of `Value::Text`, stored on disk in
    /// [`crate::metadata::TAGS_XATTR`] (and in a Mac's Finder attribute
    /// when the file has one). Written by the tags plugin, read by
    /// search and the views.
    pub const TAGS: &str = "tags";
    /// The colors the file itself gives some of its tags (a Mac's Finder
    /// tags carry one), a `Value::List` of `[tag, color]` pairs of
    /// `Value::Text`, the color one of [`crate::metadata::TAG_COLORS`].
    /// A color the user chose for the tag (`tags.colors`) wins over it.
    pub const TAG_COLORS: &str = "tag_colors";
    /// The user's rating, a `Value::UInt` from 1 to 10 (half stars: 10 is
    /// five stars), stored in `user.baloo.rating` as KDE does; absent when
    /// unrated. Written by the tags plugin, read by search, sort and views.
    pub const RATING: &str = "rating";
    /// The user's comment on a file, `Value::Text`, from `user.xdg.comment`.
    pub const COMMENT: &str = "comment";
    /// Where a downloaded file came from, `Value::Text` (a URL), from
    /// `user.xdg.origin.url` as browsers, wget and curl record it.
    pub const ORIGIN: &str = "origin";
    /// The page that led to the download, `Value::Text` (a URL), from
    /// `user.xdg.referrer.url`.
    pub const REFERRER: &str = "referrer";
    /// Where a file's notes are kept: the titles of the stores holding
    /// them (ADR-027), a list of text.
    pub const NOTES_KEPT: &str = "notes_kept";
    /// The stores whose notes differ from the rest, a list of text: they
    /// can be made to agree.
    pub const NOTES_DIFFER: &str = "notes_differ";
    /// Where the file says it was made: a picture's or a video's position,
    /// `Value::List` of two `Value::Float` (latitude, longitude, degrees).
    /// Read on demand (`crate::fields`), never at hydration (ADR-020).
    pub const POSITION: &str = "position";
    /// How many files a place holds (`EntryId::is_place`), `Value::UInt`:
    /// what its row says where a folder's size would be, and what sorting
    /// by size orders places by. A place's is known when it is listed.
    pub const FILES: &str = "files";
    /// The tag a place of a tag location stands for, `Value::Text`, its
    /// full name (`Places/France`): what its color and its menu are of.
    pub const PLACE_TAG: &str = "place_tag";
    /// A few of the pictures a place holds, what the grid draws on its
    /// tile: a `Value::List` of one `Value::List` each, its path
    /// (`Value::Text`), device and inode (`Value::UInt`) and modification
    /// time (`Value::Time`), what a thumbnail is asked with.
    pub const PLACE_PICTURES: &str = "place_pictures";
    /// When a file of the trash was put there, `Value::Time`, as its
    /// `.trashinfo` says.
    pub const TRASHED: &str = "trashed";
    /// The folder a file of the trash was put there from, where restoring
    /// it puts it back: `Value::Text`, its path, for showing.
    pub const TRASHED_FROM: &str = "trashed_from";
}

/// The metadata bag. Keys are sorted for deterministic serialization.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Metadata {
    fields: BTreeMap<Arc<str>, Field>,
}

impl Metadata {
    pub fn get(&self, key: &str) -> Option<&Field> {
        self.fields.get(key)
    }

    pub fn value(&self, key: &str) -> Option<&Value> {
        self.fields.get(key).map(|f| &f.value)
    }

    pub fn set(&mut self, key: impl Into<Arc<str>>, value: Value, provenance: Provenance) {
        self.fields.insert(key.into(), Field { value, provenance });
    }

    pub fn remove(&mut self, key: &str) -> Option<Field> {
        self.fields.remove(key)
    }

    /// Merge `other` into `self`, overwriting fields present in both.
    pub fn merge(&mut self, other: Metadata) {
        self.fields.extend(other.fields);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Field)> {
        self.fields.iter().map(|(k, v)| (k.as_ref(), v))
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Whether a stat has populated this bag (size is always set by stat),
    /// or, for a place, its provider (which counts its files).
    pub fn is_hydrated(&self) -> bool {
        self.fields.contains_key(keys::SIZE) || self.fields.contains_key(keys::FILES)
    }
}

/// Where a link leads, followed to the end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkTarget {
    /// What is there.
    pub kind: EntryKind,
    /// Its own identity.
    pub id: EntryId,
    /// Where it is, absolute and through every link of a chain: what is
    /// read to show the link (`Entry::content`), and whose name says its
    /// type (`Entry::type_name`). `None` in the rare case the path could
    /// not be resolved though what is there could be reached.
    pub path: Option<PathBuf>,
}

/// One entry of a listing.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: EntryId,
    pub name: OsString,
    /// What the entry is itself: a link is `Symlink`, whatever it leads to.
    pub kind: EntryKind,
    /// What a link leads to, followed to the end: `None` for a link that
    /// leads nowhere (its target missing, a loop, a folder we may not
    /// enter) and for anything that is not a link. Read when the entry is
    /// listed or stat-ed, so a link to a folder sorts, shows and opens as
    /// one from its first row.
    pub target: Option<LinkTarget>,
    pub metadata: Metadata,
}

impl Entry {
    pub fn new(id: EntryId, name: impl Into<OsString>, kind: EntryKind) -> Self {
        Entry {
            id,
            name: name.into(),
            kind,
            target: None,
            metadata: Metadata::default(),
        }
    }

    pub fn name_str(&self) -> std::borrow::Cow<'_, str> {
        self.name.to_string_lossy()
    }

    /// Whether the entry is a place, no file: see `EntryId::is_place`.
    pub fn is_place(&self) -> bool {
        self.id.is_place()
    }

    /// Whether the entry opens as a folder: a directory, or a link that
    /// leads to one. Everything shown and navigated asks this; what the
    /// entry is on disk (what an operation acts on) is `kind`.
    pub fn is_dir(&self) -> bool {
        self.opens() == EntryKind::Directory
    }

    /// What the entry opens as: a link's target's kind (`Symlink` for a
    /// link that leads nowhere), the entry's own otherwise.
    pub fn opens(&self) -> EntryKind {
        self.target.as_ref().map_or(self.kind, |target| target.kind)
    }

    /// The MIME type the entry has whatever its name: a folder, a pipe,
    /// a device (or a link to one), a link that leads nowhere
    /// (`inode/symlink`). `None` for a file, whose name says its type
    /// (`type_name`).
    pub fn inode_type(&self) -> Option<&'static str> {
        match self.opens() {
            EntryKind::Symlink => Some("inode/symlink"),
            kind => kind.inode_type(),
        }
    }

    /// The name that says what type of file this is: a link's target's,
    /// since a link opens what it leads to; the entry's own otherwise.
    /// Never type an entry by `name`.
    pub fn type_name(&self) -> &OsStr {
        self.target
            .as_ref()
            .and_then(|target| target.path.as_deref()?.file_name())
            .unwrap_or(&self.name)
    }

    /// What is read to show this entry, which is at `own`: what a link
    /// leads to, with its own identity (a reference resolves only to the
    /// file it names), the entry itself otherwise. A preview or a
    /// thumbnail is asked of it; an operation acts on the entry at `own`,
    /// the link. `None` when there is nothing to read: a link that leads
    /// nowhere, a pipe, a socket or a device (`EntryKind::is_special`).
    pub fn content(&self, own: &Path) -> Option<crate::op::EntryRef> {
        let opens = self.opens();
        if opens == EntryKind::Symlink || opens.is_special() {
            return None;
        }
        Some(match &self.target {
            Some(LinkTarget {
                id,
                path: Some(path),
                ..
            }) => crate::op::EntryRef::new(*id, path.clone()),
            _ => crate::op::EntryRef::new(self.id, own),
        })
    }

    /// Whether the entry is a link, resolved or not.
    pub fn is_link(&self) -> bool {
        self.kind == EntryKind::Symlink
    }

    /// A link whose target is missing (or cannot be reached).
    pub fn is_broken_link(&self) -> bool {
        self.is_link() && self.target.is_none()
    }
}
