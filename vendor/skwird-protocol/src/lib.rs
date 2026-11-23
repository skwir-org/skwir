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
//! | `query {root, name, kinds, size, mtime, limit}` | `results`…, then `done` |
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
    pub fn of(path: &Path) -> WirePath {
        WirePath(path.as_os_str().as_encoded_bytes().to_vec())
    }

    /// The path, as the system takes it.
    #[cfg(unix)]
    pub fn to_path(&self) -> PathBuf {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(self.0.clone()))
    }

    /// The path, as the system takes it: elsewhere than Unix, its UTF-8.
    #[cfg(not(unix))]
    pub fn to_path(&self) -> PathBuf {
        PathBuf::from(String::from_utf8_lossy(&self.0).into_owned())
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
    Exclude {
        #[serde(default)]
        add: Vec<WirePath>,
        #[serde(default)]
        remove: Vec<WirePath>,
    },
    /// Folders the client wants live (phase 2): kept for it across
    /// restarts, as its exclusions are. A client removes only what it
    /// added.
    Watch {
        #[serde(default)]
        add: Vec<WirePath>,
        #[serde(default)]
        remove: Vec<WirePath>,
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
}

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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    /// What went wrong, for a client to tell: `bad_request`, `version`,
    /// `not_a_folder`, `bad_pattern`, `busy`, `internal`.
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
    Changes(Changes),
    /// The token is from before what the engine keeps: walk again.
    TooOld {},
    Excluded {
        folders: Vec<WirePath>,
    },
    /// The folders this client wants live, after its `watch`.
    Watching {
        folders: Vec<WirePath>,
    },
    Error(Failure),
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
        let buf = from.fill_buf()?;
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
}
