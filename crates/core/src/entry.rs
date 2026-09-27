//! Entries: stable identity plus a metadata bag where every field records
//! where it came from.

use std::collections::BTreeMap;
use std::ffi::OsString;
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

/// Kind as reported by the directory listing (`d_type`), before any stat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
    /// The filesystem did not report a type; a stat will resolve it.
    Unknown,
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
    /// [`crate::metadata::TAGS_XATTR`]. Written by the tags plugin, read by
    /// search and the views.
    pub const TAGS: &str = "tags";
    /// Where the file says it was made: a picture's or a video's position,
    /// `Value::List` of two `Value::Float` (latitude, longitude, degrees).
    /// Read on demand (`crate::fields`), never at hydration (ADR-020).
    pub const POSITION: &str = "position";
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

    /// Whether a stat has populated this bag (size is always set by stat).
    pub fn is_hydrated(&self) -> bool {
        self.fields.contains_key(keys::SIZE)
    }
}

/// One entry of a listing.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: EntryId,
    pub name: OsString,
    pub kind: EntryKind,
    pub metadata: Metadata,
}

impl Entry {
    pub fn new(id: EntryId, name: impl Into<OsString>, kind: EntryKind) -> Self {
        Entry {
            id,
            name: name.into(),
            kind,
            metadata: Metadata::default(),
        }
    }

    pub fn name_str(&self) -> std::borrow::Cow<'_, str> {
        self.name.to_string_lossy()
    }

    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }
}
