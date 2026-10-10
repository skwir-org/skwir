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

impl From<std::fs::FileType> for EntryKind {
    /// What a listing or a stat through `std::fs` says an entry is: pipes,
    /// sockets and devices on Unix, where there are some.
    fn from(t: std::fs::FileType) -> Self {
        if t.is_dir() {
            return EntryKind::Directory;
        }
        if t.is_file() {
            return EntryKind::File;
        }
        if t.is_symlink() {
            return EntryKind::Symlink;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            if t.is_fifo() {
                return EntryKind::Fifo;
            }
            if t.is_socket() {
                return EntryKind::Socket;
            }
            if t.is_char_device() {
                return EntryKind::CharDevice;
            }
            if t.is_block_device() {
                return EntryKind::BlockDevice;
            }
        }
        EntryKind::Unknown
    }
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
    /// What a place is, in its provider's words, `Value::Text`: the Type
    /// column's ("Tag", "Field" for a category holding one tag at a time).
    /// A place without one is a "Place".
    pub const PLACE_KIND: &str = "place_kind";
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

/// `key` as one shared `Arc`: the same few keys are set on every entry
/// hydrated, and allocating each anew cost a few hundred bytes an entry.
/// Read-locked once a key was seen.
fn interned(key: &str) -> Arc<str> {
    static KEYS: std::sync::LazyLock<parking_lot::RwLock<std::collections::HashSet<Arc<str>>>> =
        std::sync::LazyLock::new(Default::default);
    if let Some(known) = KEYS.read().get(key) {
        return Arc::clone(known);
    }
    let mut keys = KEYS.write();
    if let Some(known) = keys.get(key) {
        return Arc::clone(known);
    }
    let key: Arc<str> = Arc::from(key);
    keys.insert(Arc::clone(&key));
    key
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

    /// Set `key`'s field. The key is interned (`interned`): every entry of
    /// a folder of 100 000 holds its keys without a copy of each name.
    pub fn set(&mut self, key: impl AsRef<str>, value: Value, provenance: Provenance) {
        self.fields
            .insert(interned(key.as_ref()), Field { value, provenance });
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
    /// Hidden by the system's own mark: Windows' hidden attribute, a
    /// Mac's `UF_HIDDEN` flag. Read where the listing or a stat says it;
    /// a name starting with a dot hides too ([`Entry::is_hidden`]).
    pub hidden: bool,
    /// Its name's order key, worked out as its provider sends it.
    pub(crate) name_key: NameKey,
}

/// The order key of an entry's name (`collate::sort_key`), worked out on
/// the provider's thread as it sends a batch (`EventSink::send`), so the
/// window's thread does not as the listing takes it: half a microsecond a
/// name, 50 ms a hundred thousand. A cache of the name, never part of what
/// an entry is: equal whatever it holds, and taken by the listing.
#[derive(Clone, Default)]
pub(crate) struct NameKey(pub(crate) Option<Box<[u8]>>);

impl PartialEq for NameKey {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for NameKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NameKey")
    }
}

impl Entry {
    pub fn new(id: EntryId, name: impl Into<OsString>, kind: EntryKind) -> Self {
        Entry {
            id,
            name: name.into(),
            kind,
            target: None,
            metadata: Metadata::default(),
            hidden: false,
            name_key: NameKey::default(),
        }
    }

    /// Whether the entry is left out while hidden files are: its name
    /// starts with a dot, or the system marks it hidden.
    pub fn is_hidden(&self) -> bool {
        self.hidden || self.name.as_encoded_bytes().starts_with(b".")
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
            // A link's link count is its target's, as stat-ed.
            Some(LinkTarget {
                id,
                kind,
                path: Some(path),
            }) => crate::op::EntryRef::new(*id, path.clone())
                .with_only_name(*kind == EntryKind::Directory || self.links() == Some(1)),
            _ => crate::op::EntryRef::new(self.id, own).with_only_name(self.is_only_name()),
        })
    }

    /// Whether this entry is its file's one name, as far as is known: a
    /// folder always is (no folder has two), a file once stat-ed when its
    /// link count is one. A link's own count is not known (what is read
    /// is its target's), nor one not stat-ed yet: no.
    pub fn is_only_name(&self) -> bool {
        match self.kind {
            EntryKind::Directory => true,
            EntryKind::Symlink | EntryKind::Unknown => false,
            _ => self.links() == Some(1),
        }
    }

    /// The link count a stat read (of a link, its target's).
    fn links(&self) -> Option<u64> {
        match self.metadata.value(keys::NLINK) {
            Some(Value::UInt(n)) => Some(*n),
            _ => None,
        }
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
