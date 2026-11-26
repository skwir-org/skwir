//! skwird's protocol, version 0, experimental (`docs/DESIGN.md`,
//! Protocol): what a client and an engine say to each other.
//!
//! Messages are JSON, one a line. Each request has a number of the
//! client's choosing, and every answer to it carries that number: a query
//! is answered by batches of results, then by `done`.
//!
//! | Request | Answers |
//! |---|---|
//! | `hello {versions, client}` | `hello {version, scope, engine, roots}` |
//! | `coverage {root}` | `coverage` |
//! | `sync {root}` | `coverage`, as of a time after it was asked |
//! | `query {root, name, kinds, size, mtime, limit, links}` | `results`…, then `done` |
//! | `cancel {id}` | the query's `done`, cancelled |
//! | `changes {root, since}` | `changes {events, token}`, or `too_old` |
//! | `exclude {add, remove}` | `excluded {folders}` |
//!
//! Any request may be answered by `error {what, message}` instead.
//!
//! A path is a JSON string where it is UTF-8, and `{"bytes": [...]}`
//! otherwise, byte for byte: a name that is not UTF-8 is answered as the
//! filesystem holds it. Times are instants, seconds and nanoseconds since
//! 1970: a client computes calendar boundaries in its user's time zone.
//!
//! This crate depends on `serde` and `serde_json` alone, so that a client may
//! keep a copy of it in its own tree.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The protocol's version. Experimental: it may change until declared
/// stable.
pub const VERSION: u32 = 0;

/// The longest line read, in bytes: past it, the other side is broken.
pub const MAX_LINE: usize = 1 << 20;

/// A path, as the system holds it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct WirePath(pub Vec<u8>);

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum PathForm {
    Text(String),
    Bytes { bytes: Vec<u8> },
}

impl Serialize for WirePath {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match std::str::from_utf8(&self.0) {
            Ok(text) => PathForm::Text(text.to_owned()),
            Err(_) => PathForm::Bytes {
                bytes: self.0.clone(),
            },
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for WirePath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match PathForm::deserialize(d)? {
            PathForm::Text(text) => WirePath(text.into_bytes()),
            PathForm::Bytes { bytes } => WirePath(bytes),
        })
    }
}

impl WirePath {
    /// `path` as the wire carries it: its bytes on Unix; on Windows, its
    /// WTF-8, as the engine spells the names it holds.
    pub fn of(path: &Path) -> WirePath {
        #[cfg(unix)]
        return WirePath(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()).to_vec());
        // A valid path's UTF-8 is its WTF-8, as nearly every one is: copied
        // as it is, encoded only where it holds an unpaired surrogate.
        #[cfg(windows)]
        return WirePath(path.to_str().map_or_else(
            || {
                wtf8::encode(std::os::windows::ffi::OsStrExt::encode_wide(
                    path.as_os_str(),
                ))
            },
            |text| text.as_bytes().to_vec(),
        ));
        #[cfg(not(any(unix, windows)))]
        WirePath(path.to_string_lossy().into_owned().into_bytes())
    }

    /// The path, as the system takes it.
    #[cfg(unix)]
    pub fn to_path(&self) -> PathBuf {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(self.0.clone()))
    }

    /// The path, as the system takes it: on Windows, the UTF-16 its WTF-8
    /// names, an unpaired surrogate among it, as a name may hold one.
    #[cfg(windows)]
    pub fn to_path(&self) -> PathBuf {
        use std::os::windows::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_wide(&wtf8::decode(&self.0)))
    }

    /// The path, as the system takes it: elsewhere, its UTF-8.
    #[cfg(not(any(unix, windows)))]
    pub fn to_path(&self) -> PathBuf {
        PathBuf::from(String::from_utf8_lossy(&self.0).into_owned())
    }
}

/// Names on Windows, whose names are UTF-16, as bytes: their WTF-8, UTF-8
/// for every valid name, an unpaired surrogate kept as the three bytes its
/// code point would take. The engine's own (`skwird_crawl::wtf8`), written
/// out again here so that this crate needs serde alone; never Rust's
/// encoded bytes of an `OsStr`, which it does not promise.
#[cfg(any(windows, test))]
mod wtf8 {
    /// The UTF-16 units `wide`, as WTF-8.
    pub fn encode(wide: impl IntoIterator<Item = u16>) -> Vec<u8> {
        let mut out = Vec::new();
        for unit in char::decode_utf16(wide) {
            let point = match unit {
                Ok(c) => u32::from(c),
                Err(e) => u32::from(e.unpaired_surrogate()),
            };
            push_point(point, &mut out);
        }
        out
    }

    /// A code point as UTF-8, a surrogate's as any other's of its range.
    // Each shift and mask leaves a value below 256.
    #[allow(clippy::cast_possible_truncation)]
    fn push_point(point: u32, out: &mut Vec<u8>) {
        match point {
            0..0x80 => out.push(point as u8),
            0x80..0x800 => out.extend([0xC0 | (point >> 6) as u8, 0x80 | (point & 0x3F) as u8]),
            0x800..0x1_0000 => out.extend([
                0xE0 | (point >> 12) as u8,
                0x80 | ((point >> 6) & 0x3F) as u8,
                0x80 | (point & 0x3F) as u8,
            ]),
            _ => out.extend([
                0xF0 | (point >> 18) as u8,
                0x80 | ((point >> 12) & 0x3F) as u8,
                0x80 | ((point >> 6) & 0x3F) as u8,
                0x80 | (point & 0x3F) as u8,
            ]),
        }
    }

    /// The UTF-16 units WTF-8 `bytes` name. A byte [`encode`] never writes
    /// there is U+FFFD, as a lossy UTF-8 reading has it.
    pub fn decode(bytes: &[u8]) -> Vec<u16> {
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let Some((point, len)) = point_at(&bytes[i..]) else {
                out.push(0xFFFD);
                i += 1;
                continue;
            };
            if let Some(low) = point.checked_sub(0x1_0000) {
                // Below 0x10_0000: each half fits a unit.
                #[allow(clippy::cast_possible_truncation)]
                out.extend([0xD800 + (low >> 10) as u16, 0xDC00 + (low & 0x3FF) as u16]);
            } else {
                // Below 0x1_0000.
                #[allow(clippy::cast_possible_truncation)]
                out.push(point as u16);
            }
            i += len;
        }
        out
    }

    /// The code point the bytes start with, and how many it takes: none
    /// for a sequence cut short, overlong, or past U+10FFFF.
    fn point_at(bytes: &[u8]) -> Option<(u32, usize)> {
        let first = bytes[0];
        let (len, start) = match first {
            0x00..=0x7F => return Some((u32::from(first), 1)),
            0xC2..=0xDF => (2, u32::from(first & 0x1F)),
            0xE0..=0xEF => (3, u32::from(first & 0x0F)),
            0xF0..=0xF4 => (4, u32::from(first & 0x07)),
            _ => return None,
        };
        let rest = bytes.get(1..len)?;
        if !rest.iter().all(|b| b & 0xC0 == 0x80) {
            return None;
        }
        let point = rest
            .iter()
            .fold(start, |p, b| (p << 6) | u32::from(b & 0x3F));
        let least = [0, 0, 0x80, 0x800, 0x1_0000][len];
        (point >= least && point <= 0x10_FFFF).then_some((point, len))
    }
}

/// An instant: seconds since 1970, and nanoseconds.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct Instant {
    pub sec: i64,
    pub nsec: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    File,
    Folder,
    Link,
    /// Pipes, sockets, devices.
    Other,
}

/// Why a subtree is not covered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// Outside every root the engine covers.
    Outside,
    Excluded,
    /// It holds a `.private` file.
    Private,
    Denied,
    /// A read failed, with this error number.
    Failed(i32),
    /// A listing failed part-way, with this error number.
    CutShort(i32),
    Gone,
    /// Its path now goes through a link, or to another folder.
    Replaced,
    /// Another mount starts there.
    Mount,
    /// A network mount starts there: never crawled.
    NetworkMount,
    /// A btrfs snapshot below the root, a read-only subvolume: left out,
    /// each another copy of what it was taken of. Not private: a client
    /// that walks what is not covered may pass it over below the folder
    /// searched, as too costly, and walk it when searched in.
    Snapshot,
    /// Found, not read yet.
    Unread,
    /// The watches it needs passed the system's limit.
    WatchLimit,
}

/// How far a kind of fact can be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Proof {
    /// Every change delivered, nothing dropped, since a full sync then.
    Live(Instant),
    /// Every folder compared with the index then, or read after.
    Checked(Instant),
    /// A snapshot of then, not proven since.
    AsOf(Instant),
    NotCovered(Why),
}

/// A subtree not covered, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hole {
    pub path: WirePath,
    pub why: Why,
}

/// How far an answer below a root can be trusted, for each kind of fact,
/// and the subtrees below it that are not covered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// Names and kinds.
    pub names: Proof,
    /// Sizes and times.
    pub attrs: Proof,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Hole>,
}

/// A device and an inode: with the device of a btrfs subvolume, they name
/// one file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Id {
    pub dev: u64,
    pub ino: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attrs {
    pub size: u64,
    /// The bytes the filesystem holds for it.
    pub alloc: u64,
    pub mtime: Instant,
    pub ctime: Instant,
}

/// An entry: its path, kind, identity, its folder's identity, and its
/// attributes where the engine holds them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: WirePath,
    pub kind: Kind,
    pub id: Id,
    pub folder: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attrs: Option<Attrs>,
}

/// How a name is matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Match {
    /// As skwir's query plugin takes a term: a glob where it holds `*`,
    /// `?` or `[` and compiles as one, a substring otherwise.
    Skwir,
    /// A substring, after `str::to_lowercase` of both sides unless case
    /// counts.
    Substring,
    /// A glob, as the `globset` crate compiles it.
    Glob,
    /// The whole name.
    Exact,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameTerm {
    #[serde(rename = "match")]
    pub how: Match,
    pub text: String,
    #[serde(default)]
    pub case_sensitive: bool,
}

/// What to look for below a root. Every part given must hold; an answer
/// may hold more than a client's own predicate would, never less under a
/// covered root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub root: WirePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<NameTerm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<Kind>>,
    /// Sizes from the first to the second, both included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[u64; 2]>,
    /// Modification times from the first to the second, both included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<[Instant; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    /// How a link meets `size` and `mtime` ([`Links`]): by its own, unless
    /// asked otherwise. Never written when so, as clients before it wrote
    /// none.
    #[serde(default, skip_serializing_if = "Links::is_own")]
    pub links: Links,
}

/// How a query's `size` and `mtime` take a link. The engine never follows
/// one (`docs/DESIGN.md`, Protocol): a link's attributes are its own, and
/// its target's it cannot prove.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Links {
    /// By its own size and times, as any entry.
    #[default]
    Own,
    /// Answered whatever `size` and `mtime` say, if the name and kind terms
    /// hold: for a client that judges a link by what it opens, and stats
    /// it itself.
    Unfiltered,
}

impl Links {
    /// The default, never written.
    pub fn is_own(&self) -> bool {
        *self == Links::Own
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub versions: Vec<u32>,
    /// The client's name: registrations are kept by it.
    pub client: String,
}

/// A request, numbered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub ask: Ask,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ask {
    Hello(Hello),
    Coverage {
        root: WirePath,
    },
    Sync {
        root: WirePath,
    },
    Query(Query),
    Cancel {
        id: u64,
    },
    Changes {
        root: WirePath,
        since: String,
    },
    /// What the folder at `root` holds, each child with its totals, from
    /// the index ([`Totals`]): [`Answer::Children`] in batches, then
    /// [`Answer::TotalsDone`]. With `largest`, only so many children are
    /// sent, and the rest summed whole.
    Totals {
        root: WirePath,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        largest: Option<Largest>,
    },
    Exclude {
        #[serde(default)]
        add: Vec<WirePath>,
        #[serde(default)]
        remove: Vec<WirePath>,
    },
    /// Folders the client wants live (phase 2): kept for it across
    /// restarts, as its exclusions are. A client removes only what it
    /// added. `remove` applies first, then `add`, in its order, after what
    /// is kept: the engine's share fills in the order folders were kept,
    /// so a client puts one ahead of others by removing them and adding
    /// them again after it.
    Watch {
        #[serde(default)]
        add: Vec<WirePath>,
        #[serde(default)]
        remove: Vec<WirePath>,
    },
    /// Removable volumes (`docs/DESIGN.md`, Protocol, "Removable
    /// volumes"), each by the identity the engine spelled
    /// ([`SeenVolume::id`]), never derived by the client: kept for this
    /// client across restarts, as its exclusions are; answered
    /// [`Answer::Volumes`]. An engine that says no [`VOLUMES`] refuses it.
    /// `forget` applies first, then `keep`, then `release` and
    /// `release_at`. A release is answered once nothing of the volume is
    /// held: every read of it ended, the reads of its first build among
    /// them, or after a few seconds, should one not end.
    Volumes {
        /// To index from now on, once its user said yes for it: a root
        /// whenever it is mounted.
        #[serde(default)]
        keep: Vec<String>,
        /// To index no more: let go of, its index removed.
        #[serde(default)]
        forget: Vec<String>,
        /// About to be ejected, asked before the client asks for its
        /// unmount: let go of, everything held on it closed, and read no
        /// more until it is mounted again, or a while has passed should
        /// the unmount have been refused.
        #[serde(default)]
        release: Vec<String>,
        /// About to be ejected, named by where it is mounted, as a client
        /// that ejects any volume knows it: every kept volume mounted at or
        /// below each is released, as `release` releases it. The engine
        /// reads nothing past a mount below its roots (a hole, `mount`): a
        /// volume mounted there, kept or not, holds nothing of the
        /// engine's.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        release_at: Vec<WirePath>,
    },
    /// Stop the user's engine as when it is idle (`docs/DESIGN.md`,
    /// Protocol, `stop`), where it runs a program in the folder of
    /// `program`, the client's own: answered [`Answer::Stopping`], the
    /// connection then ending as the engine's process does; otherwise
    /// [`Answer::OtherCopy`], and left running. Without `program`, it
    /// stops whichever it runs. The system service refuses it (`refused`),
    /// and an engine that says no [`STOP`] cannot read it (`bad_request`).
    Stop {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        program: Option<WirePath>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// One user's files, its engine theirs.
    User,
    /// Every user's, answered for each asker.
    System,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub version: u32,
    pub scope: Scope,
    pub engine: String,
    pub roots: Vec<WirePath>,
    /// What this engine does beyond the version it speaks, a word each
    /// (`docs/DESIGN.md`, Protocol, `hello`): absent or empty, nothing. A
    /// client takes a word it does not know as absent, and an engine
    /// says none it does not do: [`NAMES_BOTH_FORMS`] since 0.2.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}

/// A [`Welcome::features`] word: the engine answers [`Ask::Volumes`],
/// and indexes a removable volume a client keeps.
pub const VOLUMES: &str = "volumes";

/// A [`Welcome::features`] word: a name term is matched as skwir's
/// `name_match` matches it (DESIGN, open question 5): in its forms as
/// typed, composed and decomposed (NFC and NFD), each once; where case is
/// ignored, folded on the term and the name (`to_lowercase`, `ς` taken as
/// `σ`); a glob in its literal parts, its escapes and its classes' single
/// members, a range's ends as typed; an ASCII name against the term's one
/// ASCII form. A client that matches so asks no engine without it for a
/// term whose forms differ: the index would hold less than it matches.
pub const NAMES_BOTH_FORMS: &str = "names-both-forms";

/// A [`Welcome::features`] word: the engine answers [`Ask::Stop`], since
/// 0.3.
pub const STOP: &str = "stop";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Done {
    /// The coverage that held for the answers given.
    pub coverage: Coverage,
    pub count: u64,
    /// More answered than the limit let through.
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub cancelled: bool,
}

/// How many of a folder's children a `totals` sends: the `alloc` heaviest
/// by their alloc, and the `files` with the most files, a child among both
/// sent once. The others are summed in [`TotalsDone::rest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Largest {
    pub alloc: u64,
    pub files: u64,
}

/// What a subtree holds, the folder itself not counted, or a file alone:
/// from the index, its sizes and times as its `attrs` proof says. `size`
/// and `alloc` are its files' (every entry but folders), so that
/// `by_extension` sums to them; a folder's own bytes are not counted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub size: u64,
    pub alloc: u64,
    /// Files, links and others counted: every entry but folders, a file of
    /// several names once.
    pub files: u64,
    /// Their names: every entry but folders, each name counted, a file of
    /// several names at each, as a listing shows them (skwir's Usage counts
    /// so).
    #[serde(default)]
    pub names: u64,
    /// Folders below, the subtree's own not counted.
    pub folders: u64,
    /// The newest modification time of its files (every entry but
    /// folders): none where it holds none. A folder's own time is its
    /// [`Child::mtime`], or [`TotalsDone::mtime`] for the one asked, so that
    /// a client dates a folder holding no file by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub newest_file_mtime: Option<Instant>,
    /// Its files by their names' extensions, the largest alloc first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_extension: Vec<Extension>,
    /// The alloc of its files with more names than it holds: what deleting
    /// it would not free. At least so where a count of names is not known
    /// ([`TotalsDone::nlink`]).
    #[serde(default)]
    pub shared: u64,
}

/// Files of one extension in a subtree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extension {
    /// The name from its last dot on, the dot left out, lowercased: none
    /// for a name with none. The client types files (rule 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<WirePath>,
    pub files: u64,
    pub alloc: u64,
}

/// A child of the folder a `totals` asked of, with its totals: a file's
/// own, a folder's subtree's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Child {
    pub path: WirePath,
    pub kind: Kind,
    pub id: Id,
    /// Its own modification time: a file's, or a folder's own, whatever it
    /// holds. None where the index holds no attributes for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<Instant>,
    pub totals: Totals,
}

/// How far the counts of names `shared` took were known.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nlink {
    /// Every file's: `shared` is what deleting would not free.
    #[default]
    Complete,
    /// Some files' are not known (Windows' listings give none): `shared`
    /// is at least what it says.
    Partial,
}

/// The children a `totals` did not send ([`Largest`]), summed as the
/// folder's own totals sum its children: a folder among them counted, with
/// what it holds; a file, as itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rest {
    pub count: u64,
    pub totals: Totals,
}

impl Totals {
    /// What `child` adds to the totals of the folder holding it: a folder
    /// with what it holds, a file as itself.
    pub fn add_child(&mut self, child: &Child) {
        self.add(&child.totals);
        if child.kind == Kind::Folder {
            self.folders += 1;
        }
    }

    /// `other`'s totals added to these, by extension too: each extension
    /// finds its match by a map.
    pub fn add(&mut self, other: &Totals) {
        self.add_counts(other);
        if other.by_extension.is_empty() {
            return;
        }
        let mut sum = Extensions::default();
        for e in std::mem::take(&mut self.by_extension) {
            sum.add(e);
        }
        for e in &other.by_extension {
            sum.add(e.clone());
        }
        self.by_extension = sum.into_sorted();
    }

    /// `other`'s totals added to these, but for their extensions.
    fn add_counts(&mut self, other: &Totals) {
        self.size += other.size;
        self.alloc += other.alloc;
        self.files += other.files;
        self.names += other.names;
        self.folders += other.folders;
        self.shared += other.shared;
        self.newest_file_mtime = match (self.newest_file_mtime, other.newest_file_mtime) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }
}

/// Files by extension as they are summed: each extension found by a map,
/// the list built and sorted once, however many are summed (a folder's
/// rest of tens of thousands of children, [`largest`]).
#[derive(Default)]
struct Extensions(std::collections::HashMap<Option<WirePath>, (u64, u64)>);

impl Extensions {
    fn add(&mut self, e: Extension) {
        let (files, alloc) = self.0.entry(e.ext).or_default();
        *files += e.files;
        *alloc += e.alloc;
    }

    /// The extensions in their order: the largest alloc first, then by
    /// extension. Each is there once, so the order is the same whatever
    /// they were added in.
    fn into_sorted(self) -> Vec<Extension> {
        let mut out: Vec<Extension> = self
            .0
            .into_iter()
            .map(|(ext, (files, alloc))| Extension { ext, files, alloc })
            .collect();
        out.sort_by(|a, b| b.alloc.cmp(&a.alloc).then_with(|| a.ext.cmp(&b.ext)));
        out
    }
}

/// Of `children`, those [`Largest`] says to send, in their order, and the
/// rest summed: the `alloc` heaviest by alloc and the `files` with the most
/// files, a child among both once; ties go to the first. None for the
/// rest where every child is sent.
pub fn largest(children: Vec<Child>, largest: Largest) -> (Vec<Child>, Option<Rest>) {
    let pick = |n: u64, key: &dyn Fn(&Child) -> u64| {
        let mut order: Vec<usize> = (0..children.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(key(&children[i])));
        order.truncate(usize::try_from(n).unwrap_or(usize::MAX));
        order
    };
    let mut keep = vec![false; children.len()];
    for i in pick(largest.alloc, &|c| c.totals.alloc)
        .into_iter()
        .chain(pick(largest.files, &|c| c.totals.files))
    {
        keep[i] = true;
    }
    let mut sent = Vec::new();
    let mut rest = Rest {
        count: 0,
        totals: Totals::default(),
    };
    // Each child's extensions moved into one sum, not merged into the
    // rest's list one child at a time.
    let mut extensions = Extensions::default();
    for (mut child, kept) in children.into_iter().zip(keep) {
        if kept {
            sent.push(child);
        } else {
            rest.count += 1;
            rest.totals.add_counts(&child.totals);
            if child.kind == Kind::Folder {
                rest.totals.folders += 1;
            }
            for e in std::mem::take(&mut child.totals.by_extension) {
                extensions.add(e);
            }
        }
    }
    rest.totals.by_extension = extensions.into_sorted();
    (sent, (rest.count > 0).then_some(rest))
}

/// The end of a `totals`: the folder's own totals, and how far they hold.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TotalsDone {
    pub totals: Totals,
    /// The folder's own modification time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<Instant>,
    /// The journal's token as of the snapshot answered from: `changes`
    /// since it says what moved the totals since.
    pub token: String,
    /// Its `attrs` proof is the totals'; its holes, the folders not
    /// counted, which a client shows as unknown, never as empty.
    pub coverage: Coverage,
    #[serde(default)]
    pub nlink: Nlink,
    /// The children sent.
    pub count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<Rest>,
    #[serde(default)]
    pub cancelled: bool,
}

/// What changed of an entry, or of what is covered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Added(Entry),
    /// What the entry was. A folder removed takes what was below it.
    Removed(Entry),
    Changed(Entry),
    /// A subtree whose entries are not covered from now on.
    NotCovered {
        path: WirePath,
        why: Why,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changes {
    pub events: Vec<Event>,
    /// The token to ask from next.
    pub token: String,
    /// How far `events` hold every change below the root, from the token
    /// asked from up to `token`, outside `holes`: computed as it answers,
    /// so that a client need not judge it from a coverage asked apart.
    #[serde(default)]
    pub complete: Complete,
    /// The folders below the root not covered, whose changes are in no
    /// event: as a coverage of the root names them now.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Hole>,
}

/// How far a journal's events are every change ([`Changes::complete`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Complete {
    /// Every name made, removed or renamed: wherever the root is live now,
    /// a check having found what changed in any gap before.
    pub names: bool,
    /// Every change of size, times or anything else a stat says, a change
    /// of its change time alone among them (an extended attribute set, a
    /// mode changed): only where the root has been live since the token
    /// asked from, as no check finds an attribute changed alone.
    pub attrs: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    /// What went wrong, for a client to tell: `bad_request`, `version`,
    /// `not_a_folder`, `bad_pattern`, `busy`, `refused`, `internal`.
    pub what: String,
    pub message: String,
}

/// An answer to the request of the same number.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(flatten)]
    pub answer: Answer,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Hello(Welcome),
    Coverage(Coverage),
    Results(Vec<Entry>),
    Done(Done),
    /// A batch of a `totals`' children.
    Children(Vec<Child>),
    TotalsDone(Box<TotalsDone>),
    Changes(Changes),
    /// The token is from before what the engine keeps: walk again.
    TooOld {},
    Excluded {
        folders: Vec<WirePath>,
    },
    /// The folders this client wants live, after its `watch`.
    Watching(Watching),
    /// The removable volumes this client keeps, and those mounted, after
    /// its `volumes`.
    Volumes(Volumes),
    /// The engine stops, after its `stop`: the connection ends as its
    /// process does.
    Stopping {},
    /// The engine runs another folder's program than the one its `stop`
    /// named, this one: left running.
    OtherCopy {
        program: WirePath,
    },
    Error(Failure),
}

/// The removable volumes a client keeps, and those mounted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Volumes {
    /// This client's, in the order it kept them.
    pub kept: Vec<KeptVolume>,
    /// Every removable volume the engine sees mounted, kept or not, which
    /// a client offers its user.
    pub seen: Vec<SeenVolume>,
}

/// A removable volume kept, as the engine holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptVolume {
    pub id: String,
    pub standing: Standing,
    /// Where it is mounted, if it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<WirePath>,
    /// How far answers below it hold, where it is a root: not covered
    /// until it is checked, then checked as of the check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<Coverage>,
}

/// Where a kept volume stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// Not mounted: its index kept, by its identity. A path on it is in no
    /// root.
    Away,
    /// Mounted, a root.
    Indexed,
    /// Mounted, let go of before an eject: read no more until it is
    /// mounted again, or a while has passed. A path on it is in no root.
    Released,
    /// Mounted while another volume of its identity is, and indexed (a
    /// stick copied whole, two FAT serials alike): this one is refused. A
    /// path on it is in no root.
    Twin,
}

/// A removable volume mounted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenVolume {
    /// Its identity, in the engine's spelling, the one a client keeps:
    /// none where it has none, and it is never indexed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub at: WirePath,
    /// Its filesystem's type, as the system names it.
    pub filesystem: String,
    /// Its label, as its filesystem holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Whether any client keeps it.
    pub kept: bool,
}

/// What a client keeps live, after its `watch`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watching {
    /// This client's folders, in the order it kept them: the order the
    /// share fills in. At the system service, which keeps none, the
    /// folders asked.
    pub folders: Vec<Kept>,
    /// The engine's share of watches, every client's folders in it: none
    /// at the system service, which hears every folder with none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<Share>,
}

/// A folder kept live, as the engine holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// As the client asked it.
    pub path: WirePath,
    pub state: Keeping,
    /// The folders below it, itself among them, as the index holds them:
    /// the watches it takes. None until the index holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folders: Option<u64>,
    /// The folder kept live, by any client, that holds it: it takes no
    /// watch of its own, and is live as that one is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<WirePath>,
}

/// Whether a folder kept live is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keeping {
    /// Every change below it heard since then.
    Live(Instant),
    /// Checked instead, for this reason.
    NotLive(NotLive),
}

/// Why a folder kept live is not: it is checked instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotLive {
    /// The engine's share of watches was spent by the folders kept before
    /// it. It is tried again when a folder kept is let go of.
    Share,
    /// The system's limit of watches is reached: other programs hold the
    /// rest.
    WatchLimit,
    /// A watch failed, with this error number.
    Failed(i32),
    /// It is not covered, for this reason: outside every root, excluded,
    /// a mount, a network mount.
    NotCovered(Why),
    /// No folder of the index is there: tried again at each check.
    Missing,
    /// Not live yet: its root is being opened, or an overflow lost what it
    /// heard until the check after.
    Pending,
}

/// A share of watches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Share {
    /// The watches every client's folders hold.
    pub used: u64,
    /// The most the engine holds: its share of the user's limit.
    pub size: u64,
}

/// Write `message` as one line.
pub fn write<W: Write, T: Serialize>(to: &mut W, message: &T) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(message).map_err(std::io::Error::other)?;
    line.push(b'\n');
    to.write_all(&line)?;
    to.flush()
}

/// Read one message, or none at the end. A line that is not one, or
/// longer than [`MAX_LINE`], is an error.
pub fn read<R: BufRead, T: DeserializeOwned>(from: &mut R) -> std::io::Result<Option<T>> {
    let mut line = Vec::new();
    loop {
        // A read interrupted by a signal is made again: what it would
        // leave of the line is held here alone.
        let buf = match from.fill_buf() {
            Ok(buf) => buf,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if buf.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "a message cut short",
                ))
            };
        }
        if let Some(end) = buf.iter().position(|&b| b == b'\n') {
            line.extend_from_slice(&buf[..end]);
            from.consume(end + 1);
            break;
        }
        let n = buf.len();
        line.extend_from_slice(buf);
        from.consume(n);
        if line.len() > MAX_LINE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "a message longer than a line may be",
            ));
        }
    }
    if line.len() > MAX_LINE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "a message longer than a line may be",
        ));
    }
    serde_json::from_slice(&line)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An engine says what it does beyond its version in its hello, a word
    /// each: an older engine's hello says nothing, read as none; none is
    /// sent as nothing; a word a client does not know is kept, for it to
    /// pass over.
    #[test]
    fn a_hello_says_its_features_or_none() {
        let plain =
            r#"{"version":0,"scope":"user","engine":"skwird 0.1.0","roots":["/home/user"]}"#;
        let welcome: Welcome = serde_json::from_str(plain).unwrap();
        assert_eq!(welcome.features, Vec::<String>::new());
        assert_eq!(serde_json::to_string(&welcome).unwrap(), plain);
        let more = r#"{"version":0,"scope":"user","engine":"skwird 0.2.0","roots":[],"features":["names-both-forms","unknown"]}"#;
        let welcome: Welcome = serde_json::from_str(more).unwrap();
        assert_eq!(welcome.features, ["names-both-forms", "unknown"]);
    }

    /// Every line of the transcripts reads as a request (`>`) or an
    /// answer (`<`), and writes back as the same JSON: a client writing
    /// its own speaks what they show.
    #[test]
    fn the_transcripts_read_and_write_back_alike() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("transcripts");
        let mut lines = 0;
        for file in std::fs::read_dir(&dir).unwrap() {
            let text = std::fs::read_to_string(file.unwrap().path()).unwrap();
            for line in text
                .lines()
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
            {
                let (side, json) = line.split_at(2);
                let value: serde_json::Value = serde_json::from_str(json).unwrap();
                let back = match side {
                    "> " => serde_json::to_value(
                        serde_json::from_str::<Request>(json)
                            .unwrap_or_else(|e| panic!("{e}: {json}")),
                    ),
                    "< " => serde_json::to_value(
                        serde_json::from_str::<Response>(json)
                            .unwrap_or_else(|e| panic!("{e}: {json}")),
                    ),
                    _ => panic!("a line neither > nor <: {line}"),
                }
                .unwrap();
                assert_eq!(back, value, "{json}");
                lines += 1;
            }
        }
        assert!(lines > 10, "{lines} lines");
    }

    fn child(name: &str, kind: Kind, alloc: u64, files: u64, ext: &str) -> Child {
        Child {
            path: WirePath(format!("/home/user/{name}").into_bytes()),
            kind,
            id: Id::default(),
            mtime: None,
            totals: Totals {
                size: alloc,
                alloc,
                files,
                names: files,
                folders: u64::from(kind == Kind::Folder),
                newest_file_mtime: Some(Instant {
                    sec: i64::try_from(alloc).unwrap(),
                    nsec: 0,
                }),
                by_extension: vec![Extension {
                    ext: Some(WirePath(ext.as_bytes().to_vec())),
                    files,
                    alloc,
                }],
                shared: 0,
            },
        }
    }

    /// `largest` sends the heaviest and the fullest, a child among both
    /// once, in their order, and sums the rest as the folder's totals sum
    /// its children: what is sent and the rest add up to the whole.
    #[test]
    fn largest_sends_the_heaviest_and_the_fullest_and_sums_the_rest() {
        let children = vec![
            child("a.txt", Kind::File, 10, 1, "txt"),
            child("big", Kind::Folder, 900, 3, "mkv"),
            child("full", Kind::Folder, 50, 400, "rs"),
            child("b.txt", Kind::File, 20, 1, "txt"),
            child("small", Kind::Folder, 5, 2, "md"),
        ];
        let mut whole = Totals::default();
        for c in &children {
            whole.add_child(c);
        }
        assert_eq!((whole.files, whole.folders, whole.alloc), (407, 6, 985));
        assert_eq!(whole.by_extension[0].ext, Some(WirePath(b"mkv".to_vec())));
        let (sent, rest) = largest(children.clone(), Largest { alloc: 1, files: 1 });
        let names: Vec<_> = sent.iter().map(|c| c.path.to_path()).collect();
        assert_eq!(
            names,
            [
                PathBuf::from("/home/user/big"),
                PathBuf::from("/home/user/full")
            ]
        );
        let rest = rest.unwrap();
        assert_eq!(rest.count, 3);
        let txt = rest
            .totals
            .by_extension
            .iter()
            .find(|e| e.ext == Some(WirePath(b"txt".to_vec())))
            .unwrap();
        assert_eq!((txt.files, txt.alloc), (2, 30));
        let mut again = rest.totals.clone();
        for c in &sent {
            again.add_child(c);
        }
        assert_eq!(again, whole);
        let (all, none) = largest(
            children.clone(),
            Largest {
                alloc: 10,
                files: 0,
            },
        );
        assert_eq!((all, none), (children, None));
    }

    /// A query's `links` is written only when asked otherwise than by a
    /// link's own attributes, so that a query as clients before it wrote one
    /// is the same; read as its own where absent.
    #[test]
    fn a_query_says_how_it_takes_links_only_when_asked() {
        let mut q = Query {
            root: WirePath(b"/home/user".to_vec()),
            name: None,
            kinds: None,
            size: Some([1, 2]),
            mtime: None,
            limit: None,
            links: Links::Own,
        };
        let json = serde_json::to_string(&q).unwrap();
        assert!(!json.contains("links"), "{json}");
        assert_eq!(serde_json::from_str::<Query>(&json).unwrap(), q);
        q.links = Links::Unfiltered;
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains(r#""links":"unfiltered""#), "{json}");
        assert_eq!(serde_json::from_str::<Query>(&json).unwrap(), q);
    }

    /// A path that is not UTF-8 travels as its bytes, and comes back so.
    #[test]
    fn a_path_not_utf8_travels_byte_for_byte() {
        let text = WirePath(b"notes/caf\xc3\xa9".to_vec());
        assert_eq!(serde_json::to_string(&text).unwrap(), "\"notes/café\"");
        let bytes = WirePath(b"latin1-\xe9t\xe9".to_vec());
        let json = serde_json::to_string(&bytes).unwrap();
        assert!(json.starts_with("{\"bytes\":["), "{json}");
        assert_eq!(serde_json::from_str::<WirePath>(&json).unwrap(), bytes);
    }

    /// Lines are read one at a time, the end said, a broken one refused.
    #[test]
    fn messages_are_read_a_line_each() {
        let mut out = Vec::new();
        let ask = Request {
            id: 7,
            ask: Ask::Cancel { id: 3 },
        };
        write(&mut out, &ask).unwrap();
        write(&mut out, &ask).unwrap();
        let mut r = std::io::Cursor::new(out);
        assert_eq!(read::<_, Request>(&mut r).unwrap(), Some(ask.clone()));
        assert_eq!(read::<_, Request>(&mut r).unwrap(), Some(ask));
        assert_eq!(read::<_, Request>(&mut r).unwrap(), None);
        let mut cut = std::io::Cursor::new(b"{\"id\":1".to_vec());
        assert!(read::<_, Request>(&mut cut).is_err());
        let mut long = std::io::Cursor::new(vec![b'x'; MAX_LINE + 10]);
        assert!(read::<_, Request>(&mut long).is_err());
    }

    /// What a read gives, a part at a time, a signal between two parts.
    struct Parts(Vec<Option<Vec<u8>>>);

    impl std::io::Read for Parts {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            match self.0.remove(0) {
                Some(part) => {
                    buf[..part.len()].copy_from_slice(&part);
                    Ok(part.len())
                }
                None => Err(std::io::ErrorKind::Interrupted.into()),
            }
        }
    }

    /// A read interrupted by a signal in the middle of a line is made
    /// again: the line is read whole, what came before the signal with it.
    #[test]
    fn a_read_interrupted_keeps_its_line() {
        let ask = Request {
            id: 7,
            ask: Ask::Cancel { id: 3 },
        };
        let mut line = Vec::new();
        write(&mut line, &ask).unwrap();
        let second = line.split_off(line.len() / 2);
        let mut r = std::io::BufReader::new(Parts(vec![Some(line), None, Some(second)]));
        assert_eq!(read::<_, Request>(&mut r).unwrap(), Some(ask));
        assert_eq!(read::<_, Request>(&mut r).unwrap(), None);
    }

    /// Totals added merge their extensions, each once, the largest alloc
    /// first, then by extension, whatever order they came in; the rest of
    /// a `largest`, summed at once, as the same children added one by one.
    #[test]
    fn totals_added_merge_their_extensions() {
        let ext = |name: &str, files, alloc| Extension {
            ext: (!name.is_empty()).then(|| WirePath(name.as_bytes().to_vec())),
            files,
            alloc,
        };
        let mut a = Totals {
            by_extension: vec![ext("rs", 2, 40), ext("md", 1, 10), ext("", 1, 5)],
            ..Totals::default()
        };
        let b = Totals {
            by_extension: vec![ext("txt", 3, 30), ext("", 2, 6), ext("rs", 1, 1)],
            ..Totals::default()
        };
        a.add(&b);
        assert_eq!(
            a.by_extension,
            [
                ext("rs", 3, 41),
                ext("txt", 3, 30),
                ext("", 3, 11),
                ext("md", 1, 10)
            ]
        );
        let children: Vec<Child> = (0..200u64)
            .map(|i| {
                child(
                    &format!("c{i}"),
                    Kind::File,
                    i % 7,
                    1,
                    &format!("e{}", i % 13),
                )
            })
            .collect();
        let (_, rest) = largest(children.clone(), Largest { alloc: 0, files: 0 });
        let mut summed = Totals::default();
        for c in &children {
            summed.add_child(c);
        }
        assert_eq!(rest.unwrap().totals, summed);
        assert_eq!(summed.by_extension.len(), 13);
        // Many children of many extensions each: their rest sums each
        // extension once, as the same children added one by one.
        let many: Vec<Child> = (0..400u64)
            .map(|i| {
                let mut c = child(&format!("m{i}"), Kind::Folder, i, 1, "");
                c.totals.by_extension = (0..20u64)
                    .map(|j| ext(&format!("x{}", (i * 7 + j) % 1_000), 1, j))
                    .collect();
                c
            })
            .collect();
        let (sent, rest) = largest(many.clone(), Largest { alloc: 1, files: 0 });
        let mut summed = Totals::default();
        for c in many.iter().filter(|c| c.path != sent[0].path) {
            summed.add_child(c);
        }
        let rest = rest.unwrap();
        assert_eq!(rest.count, 399);
        assert_eq!(rest.totals, summed);
        assert_eq!(rest.totals.by_extension.len(), 1_000);
    }

    /// A name on Windows travels as its WTF-8: a valid one as its UTF-8,
    /// an unpaired surrogate as the three bytes of its code point, read
    /// back as it was; what no encoding gives reads as U+FFFD.
    #[test]
    fn a_windows_name_is_its_wtf8() {
        for name in ["notes.txt", "été", "日本語", "a 🙂 b", ""] {
            let wide: Vec<u16> = name.encode_utf16().collect();
            assert_eq!(wtf8::encode(wide.iter().copied()), name.as_bytes());
            assert_eq!(wtf8::decode(name.as_bytes()), wide, "{name}");
        }
        assert_eq!(wtf8::encode([0x61, 0xD800, 0x62]), b"a\xED\xA0\x80b");
        for wide in [&[0x61, 0xD800, 0x62][..], &[0xDC00], &[0xD83D, 0x61]] {
            let bytes = wtf8::encode(wide.iter().copied());
            assert!(std::str::from_utf8(&bytes).is_err(), "{bytes:x?}");
            assert_eq!(wtf8::decode(&bytes), wide, "{bytes:x?}");
        }
        assert_eq!(wtf8::decode(b"a\xFFb"), [0x61, 0xFFFD, 0x62]);
        assert_eq!(wtf8::decode(b"\xC0\x80"), [0xFFFD, 0xFFFD], "overlong");
    }
}
