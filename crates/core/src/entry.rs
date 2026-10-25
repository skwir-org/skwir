//! Entries: stable identity plus a metadata bag where every field records
//! where it came from.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// A file: the device and the inode a stat reports on Unix, the volume and
/// the file index on Windows. The same under every name the file has (hard
/// links) and through every link to it; it survives renames and moves
/// within a device. What belongs to the file is kept by it: its bytes,
/// thumbnails, fields, the notes in its attributes (ADR-049).
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
pub struct FileId {
    /// Device (or volume) number.
    pub dev: u64,
    /// Inode (or file index) on that device. On a mount whose program
    /// makes inode numbers up for as long as it is mounted (FUSE: sshfs,
    /// GVfs, rclone), that number told apart by the mount's own
    /// (`fs::stat::file`): sshfs numbers files again from the start once
    /// mounted again, often on the device number the last mount had, so a
    /// number alone is another file's after a remount.
    pub ino: u64,
}

impl FileId {
    /// The file `ino` of the device `dev`, as given: a stat names a file
    /// through `fs::stat::file`, which tells a made-up number by its
    /// mount.
    pub const fn new(dev: u64, ino: u64) -> FileId {
        FileId { dev, ino }
    }

    /// Whether the identity holds beyond the mount it was read on: not on
    /// a device whose numbers a mount's program makes up
    /// (`fs::reach::mount_of`, asked of the mount table in memory). What
    /// is kept beyond it (in the cache, for a later start) knows such a
    /// file by its path, as thumbnails are known, never by this.
    pub fn lasts(self) -> bool {
        crate::fs::reach::mount_of(self.dev) == 0
    }

    /// What a cache kept beyond the mount knows the file at `path` by:
    /// its device and inode (`dev:ino`), or where they do not last
    /// ([`FileId::lasts`]) its path, as thumbnails know a file.
    pub fn kept_as(self, path: &Path) -> Vec<u8> {
        if self.lasts() {
            format!("{}:{}", self.dev, self.ino).into_bytes()
        } else {
            [b"path:", path.as_os_str().as_encoded_bytes()].concat()
        }
    }
}

/// An entry: a file under one of its names, where a location lists it.
/// Two names of one file (hard links) are two entries; one name is one
/// entry, however its folder is reached. What belongs to the name is kept
/// by the entry: its row, the selection, the cut, its sidecars (ADR-049).
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
    /// The file named.
    #[serde(flatten)]
    pub file: FileId,
    /// Which of its names: a hash of the folder holding the name (that
    /// folder's own file) and the name, so it stays while a folder above
    /// is renamed. A place's is 0.
    #[serde(default)]
    pub at: u64,
}

impl EntryId {
    /// The device number no filesystem gives: a place's (ADR-032).
    pub const PLACE_DEV: u64 = u64::MAX;

    /// An entry not stat-ed yet, whose stat says what it is
    /// (`fs::stat::stat_entry`).
    pub const UNKNOWN: EntryId = EntryId {
        file: FileId::new(0, 0),
        at: 0,
    };

    /// The entry `name` in the folder `folder` is, of the file `file`.
    pub fn in_folder(file: FileId, folder: FileId, name: &OsStr) -> EntryId {
        EntryId {
            file,
            at: fnv(&[
                &folder.dev.to_le_bytes(),
                &folder.ino.to_le_bytes(),
                name.as_encoded_bytes(),
            ]),
        }
    }

    /// The entry the name at `path` is, of the file `file`, for a location
    /// that knows its entries by their paths and may not read their
    /// folders to name them (what was dropped on the shelf).
    pub fn at_path(file: FileId, path: &Path) -> EntryId {
        EntryId {
            file,
            at: fnv(&[path.as_os_str().as_encoded_bytes()]),
        }
    }

    /// A place's identity, from what makes it the place it is (a tag's
    /// name, hashed by its provider): the same each time it is listed.
    pub const fn place(key: u64) -> EntryId {
        EntryId {
            file: FileId::new(Self::PLACE_DEV, key),
            at: 0,
        }
    }

    /// Whether this is a place's: an entry that is no file but a location
    /// of its own, shown as a folder (a tag's in a tag location), which
    /// opens where `OpenLocation::location_of` says and has no path.
    pub const fn is_place(self) -> bool {
        self.file.dev == Self::PLACE_DEV
    }
}

/// What `entries` weigh together, each file once (ADR-049): the sizes of
/// those that are no folder (`Entry::weighed`), where two names of one
/// file (hard links), or a file and a link to it, are one file.
pub fn weight<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> u64 {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter_map(Entry::weighed)
        .filter(|(file, _)| seen.insert(*file))
        .fold(0, |sum, (_, size)| sum.saturating_add(size))
}

/// What deleting some entries leaves on the disk (`kept_by_other_names`):
/// their files with names among them not all their names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    pub files: usize,
    pub bytes: u64,
}

impl Kept {
    /// What a deletion's question says of it: "2 files have names
    /// elsewhere: 1.2 GB stays on the disk". `None` when nothing stays.
    pub fn said(&self) -> Option<String> {
        (self.files > 0).then(|| {
            let (files, have) = match self.files {
                1 => ("1 file".to_owned(), "has"),
                n => (format!("{n} files"), "have"),
            };
            format!(
                "{files} {have} names elsewhere: {} stays on the disk",
                crate::fs::size_said(self.bytes)
            )
        })
    }
}

/// What deleting `entries` leaves on the disk because their files have
/// names besides those among them (ADR-049): each file once, its link
/// count saying how many names it has. A link deletes itself, not what it
/// leads to; a folder's files are not counted (what it holds is not
/// listed here). `None` while a file's count is not known (not stat-ed
/// yet): nothing is said rather than guessed.
pub fn kept_by_other_names<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Option<Kept> {
    let mut files: std::collections::HashMap<FileId, (u64, u64, u64)> =
        std::collections::HashMap::new();
    for e in entries {
        if e.is_dir() || e.is_link() || e.kind.is_special() {
            continue;
        }
        let (links, size) = (
            e.links()?,
            e.metadata.value(keys::SIZE).and_then(Value::as_u64)?,
        );
        files.entry(e.file()).or_insert((links, size, 0)).2 += 1;
    }
    let mut kept = Kept::default();
    for (links, size, here) in files.into_values() {
        if here < links {
            kept.files += 1;
            kept.bytes = kept.bytes.saturating_add(size);
        }
    }
    Some(kept)
}

/// FNV-1a of `parts`: a name's hash, the same in every process.
fn fnv(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in parts.iter().flat_map(|p| p.iter()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
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
    /// When anything about the file last changed, its bytes or what is
    /// said of it (its names, mode, owner, attributes), `Value::Time`: its
    /// inode's change time on Unix, which no program sets back, its
    /// `ChangeTime` on Windows.
    pub const CHANGED: &str = "changed";
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
    /// The file there.
    pub id: FileId,
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

    /// The file this entry names (`EntryId::file`).
    pub fn file(&self) -> FileId {
        self.id.file
    }

    /// The file this entry's size is of, and that size (`keys::SIZE`): a
    /// link's target's, as a listing sizes it. `None` for a folder, whose
    /// own size says nothing of what it holds, and before a stat.
    pub fn weighed(&self) -> Option<(FileId, u64)> {
        if self.is_dir() {
            return None;
        }
        let size = self.metadata.value(keys::SIZE).and_then(Value::as_u64)?;
        Some((self.target.as_ref().map_or(self.file(), |t| t.id), size))
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
            _ => crate::op::EntryRef::new(self.file(), own).with_only_name(self.is_only_name()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_whose_mount_made_its_number_up_is_kept_by_its_path() {
        let lasting = FileId::new(5, 7);
        assert_eq!(
            serde_json::to_value(lasting).unwrap(),
            serde_json::json!({ "dev": 5, "ino": 7 })
        );
        let made_up = FileId::new(6, 7);
        crate::fs::reach::pretend_made_up(6, 1 << 40);
        assert!(lasting.lasts() && !made_up.lasts());
        let path = Path::new("/srv/it's here.txt");
        assert_eq!(lasting.kept_as(path), b"5:7");
        assert_eq!(made_up.kept_as(path), b"path:/srv/it's here.txt");
    }

    fn sized(file: u64, at: u64, size: u64, kind: EntryKind) -> Entry {
        let mut e = Entry::new(
            EntryId {
                file: FileId::new(1, file),
                at,
            },
            format!("{file}-{at}"),
            kind,
        );
        e.metadata
            .set(keys::SIZE, Value::UInt(size), Provenance::Filesystem);
        e
    }

    /// Two names of one file, and a link to it, weigh the file once; a
    /// folder's own size is no weight (ADR-049).
    #[test]
    fn what_entries_weigh_counts_each_file_once() {
        let a = sized(1, 1, 100, EntryKind::File);
        let b = sized(1, 2, 100, EntryKind::File);
        let other = sized(2, 1, 50, EntryKind::File);
        let folder = sized(3, 1, 4096, EntryKind::Directory);
        let mut link = sized(4, 1, 100, EntryKind::Symlink);
        link.target = Some(LinkTarget {
            kind: EntryKind::File,
            id: a.file(),
            path: None,
        });
        assert_eq!(weight([&a, &b, &other, &folder, &link]), 150);
        assert_eq!(weight([&other]), 50);
    }

    /// Deleting a name of a file frees nothing while another name of it
    /// is kept: told from the link counts the listing has (ADR-049).
    #[test]
    fn what_a_deletion_keeps_is_told_by_link_counts() {
        let linked = |at: u64| {
            let mut e = sized(1, at, 100, EntryKind::File);
            e.metadata
                .set(keys::NLINK, Value::UInt(2), Provenance::Filesystem);
            e
        };
        let (a, b) = (linked(1), linked(2));
        let mut alone = sized(2, 1, 50, EntryKind::File);
        alone
            .metadata
            .set(keys::NLINK, Value::UInt(1), Provenance::Filesystem);
        let kept = kept_by_other_names([&a, &alone]).unwrap();
        assert_eq!(
            kept,
            Kept {
                files: 1,
                bytes: 100
            }
        );
        assert_eq!(
            kept.said().as_deref(),
            Some("1 file has names elsewhere: 100 B stays on the disk")
        );
        assert_eq!(
            kept_by_other_names([&a, &b, &alone]).unwrap().said(),
            None,
            "every name goes"
        );
        let unknown = sized(3, 1, 10, EntryKind::File);
        assert_eq!(
            kept_by_other_names([&a, &unknown]),
            None,
            "a count not known"
        );
    }
}
