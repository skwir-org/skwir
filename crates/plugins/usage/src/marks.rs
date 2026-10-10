//! What the map says of a block beside its size: the sorts of files that
//! fill it, by the bytes they take (`Kinds`), and whether it is room the
//! user can have back without losing anything kept (`Reclaim`).
//!
//! A file is typed by its name as the desktop's database types it, asked
//! once per extension (`Types`): asked per name, the database would add
//! about 13 µs to each file of a walk that takes 2 µs. A name without an
//! extension says nothing, and its file is counted with the others.
use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use skwir_platform::mime::{self, Kind};

/// Room the user can have back: a cache, which its program makes again
/// when it needs it, or what was put in the Trash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Reclaim {
    /// A folder holding a `CACHEDIR.TAG` (bford.net/cachedir), or the
    /// user's cache folder (`$XDG_CACHE_HOME`).
    Cache,
    /// A trash (`skwir_platform::trash`): freed when it is emptied.
    Trash,
}

/// Distinct extensions asked of the database in a worker's life; the
/// files of the rest are counted with the others.
const ASKED: usize = 4096;

/// The kind of each extension, asked once.
#[derive(Default)]
pub(crate) struct Types(HashMap<OsString, Option<Kind>>);

impl Types {
    /// The kind of a file named `name`, by its extension.
    pub(crate) fn of(&mut self, name: &OsStr) -> Option<Kind> {
        let extension = Path::new(name).extension()?;
        if let Some(kind) = self.0.get(extension) {
            return *kind;
        }
        if self.0.len() >= ASKED {
            return None;
        }
        let mut question = OsString::from("file.");
        question.push(extension);
        let kind = mime::by_name(&question).and_then(|m| {
            // Of a type of several kinds, the one that is not text: a
            // Markdown file is a document, an SVG a picture.
            let kinds = mime::kinds(&m);
            kinds
                .iter()
                .copied()
                .find(|k| *k != Kind::Text)
                .or_else(|| kinds.first().copied())
        });
        self.0.insert(extension.to_owned(), kind);
        kind
    }
}

/// Where the files whose names say nothing are counted.
const OTHER: usize = Kind::ALL.len();

/// The bytes a folder's files take, by their kind (`Kind::ALL`'s order,
/// then those of no kind). Only a folder has them: a file has its kind.
/// Sent as the kinds' names (`other` for the rest), each with its bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "BTreeMap<String, u64>", into = "BTreeMap<String, u64>")]
pub struct Kinds(Option<Box<[u64; OTHER + 1]>>);

fn slot(kind: Option<Kind>) -> usize {
    kind.and_then(|kind| Kind::ALL.iter().position(|k| *k == kind))
        .unwrap_or(OTHER)
}

impl Kinds {
    fn slots(&mut self) -> &mut [u64; OTHER + 1] {
        self.0.get_or_insert_with(|| Box::new([0; OTHER + 1]))
    }

    /// `bytes` more of `kind`.
    pub(crate) fn add(&mut self, kind: Option<Kind>, bytes: u64) {
        if bytes > 0 {
            self.slots()[slot(kind)] += bytes;
        }
    }

    /// `bytes` fewer of `kind`: a file found twice.
    pub(crate) fn take(&mut self, kind: Option<Kind>, bytes: u64) {
        if let Some(slots) = &mut self.0 {
            let at = &mut slots[slot(kind)];
            *at = at.saturating_sub(bytes);
        }
    }

    /// What `other` holds too.
    pub(crate) fn merge(&mut self, other: &Kinds) {
        if let Some(theirs) = &other.0 {
            for (mine, theirs) in self.slots().iter_mut().zip(theirs.iter()) {
                *mine += theirs;
            }
        }
    }

    /// What it costs beside the node holding it.
    pub(crate) fn heap(&self) -> usize {
        self.0
            .as_ref()
            .map_or(0, |_| std::mem::size_of::<[u64; OTHER + 1]>())
    }

    /// Each kind with its bytes, the rest under `None`, none empty.
    pub fn iter(&self) -> impl Iterator<Item = (Option<Kind>, u64)> + '_ {
        self.0.iter().flat_map(|slots| {
            slots
                .iter()
                .enumerate()
                .filter(|(_, b)| **b > 0)
                .map(|(at, b)| (Kind::ALL.get(at).copied(), *b))
        })
    }
}

impl From<Kinds> for BTreeMap<String, u64> {
    fn from(kinds: Kinds) -> Self {
        kinds
            .iter()
            .map(|(kind, bytes)| (kind.map_or("other", Kind::name).to_owned(), bytes))
            .collect()
    }
}

impl From<BTreeMap<String, u64>> for Kinds {
    fn from(map: BTreeMap<String, u64>) -> Self {
        let mut kinds = Kinds::default();
        for (name, bytes) in map {
            kinds.add(Kind::from_name(&name), bytes);
        }
        kinds
    }
}

/// The folders whose room can be had back by what they are, not by
/// what they hold: the user's cache folder and trash, canonical as the
/// walk sees them, and a volume's trash, known by its name. A folder
/// holding a `CACHEDIR.TAG` says so itself.
#[derive(Clone, Default)]
pub(crate) struct Places {
    caches: Vec<PathBuf>,
    trashes: Vec<PathBuf>,
}

impl Places {
    /// The user's: a few stats, never the volumes (`trash::trashes` reads
    /// each one's device).
    pub(crate) fn read() -> Self {
        let canonical = |path: PathBuf| std::fs::canonicalize(&path).ok();
        Places {
            caches: skwir_core::xdg::cache_home()
                .and_then(canonical)
                .into_iter()
                .collect(),
            trashes: skwir_platform::trash::home_trash()
                .and_then(canonical)
                .into_iter()
                .collect(),
        }
    }

    /// The worker's tests, Unix's alone, name them.
    #[cfg(all(test, unix))]
    pub(crate) fn new(caches: Vec<PathBuf>, trashes: Vec<PathBuf>) -> Self {
        Places { caches, trashes }
    }

    pub(crate) fn of(&self, path: &Path) -> Option<Reclaim> {
        if self.trashes.iter().any(|t| t == path) || skwir_platform::trash::named_as_trash(path) {
            Some(Reclaim::Trash)
        } else if self.caches.iter().any(|c| c == path) {
            Some(Reclaim::Cache)
        } else {
            None
        }
    }
}

/// The name of the file that says its folder is a cache.
pub(crate) const CACHEDIR_TAG: &str = "CACHEDIR.TAG";
const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// Whether the `CACHEDIR.TAG` at `path` begins as the convention says: a
/// file of that name and nothing more is not one.
pub(crate) fn tags_a_cache(path: &Path) -> bool {
    let mut head = [0; SIGNATURE.len()];
    skwir_core::fs::file::open_regular(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok_and(|()| head == SIGNATURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_sent_by_name_and_read_back() {
        let mut kinds = Kinds::default();
        assert_eq!(kinds.heap(), 0, "nothing kept for a folder of nothing");
        kinds.add(Some(Kind::Image), 300);
        kinds.add(None, 20);
        kinds.add(Some(Kind::Image), 100);
        let mut other = Kinds::default();
        other.add(Some(Kind::Video), 50);
        kinds.merge(&other);
        kinds.take(Some(Kind::Video), 80);
        let sent = serde_json::to_value(&kinds).unwrap();
        assert_eq!(sent, serde_json::json!({"image": 400, "other": 20}));
        let read: Kinds = serde_json::from_value(sent).unwrap();
        assert_eq!(read, kinds);
    }

    // Elsewhere the system's registry types a name first (ADR-037).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_is_typed_by_its_extension_once() {
        let mut types = Types::default();
        assert_eq!(types.of(OsStr::new("trip.MKV")), Some(Kind::Video));
        assert_eq!(types.of(OsStr::new("notes.md")), Some(Kind::Document));
        assert_eq!(types.of(OsStr::new("main.rs")), Some(Kind::Text));
        assert_eq!(types.of(OsStr::new("Makefile")), None);
        assert_eq!(types.of(OsStr::new(".bashrc")), None, "a hidden file's dot");
        assert_eq!(types.0.len(), 3);
        assert_eq!(types.of(OsStr::new("other.mkv")), Some(Kind::Video));
    }

    #[test]
    fn only_a_signed_tag_makes_a_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let tag = tmp.path().join(CACHEDIR_TAG);
        std::fs::write(
            &tag,
            b"Signature: 8a477f597d28d172789f06886806bc55\n# a tool\n",
        )
        .unwrap();
        assert!(tags_a_cache(&tag));
        std::fs::write(&tag, b"Signature: none").unwrap();
        assert!(!tags_a_cache(&tag));
        assert!(!tags_a_cache(tmp.path()), "a folder of that name");
    }
}
