//! The tagged files skwir has seen (ADR-026): every file the provider
//! reads tells it whether the file has tags (a folder shown, a search's
//! walk, a file changed and read again), and a count of a tag location's
//! folder teaches it the whole folder (`count.rs`), which it then covers.
//!
//! Kept small so it can hold a large library: each tag's name once, by
//! number; files by folder, each folder's path once, with how many of its
//! files carry each tag, so counts sum folders, not files. It lives in the
//! deletable cache in 256 pieces by folder, each read the first time a
//! folder of it is needed and written alone when it changed, and let go
//! of after a few minutes unused; the folders it holds and the tags of the
//! files under the home folder (outside hidden folders, as its tag places
//! count them) stay known in between, for the sidebar and for knowing
//! which pieces a question needs. A category counts the files with any
//! tag within it once each (`Places` those tagged `Places/France/Paris`),
//! as a search for it finds them: each tag knows its category, which is a
//! tag of the index too. How much room it may take is the user's
//! (`tags.index_mb`): past it, it learns no new file (those known stay
//! current) and covers no folder, and skwir says the tags leave some out.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::{Mutex, MutexGuard};
use skwir_core::app::App;
use skwir_core::cache::CacheSlot;
use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach;
use skwir_core::metadata::{tag_parent, tag_within};

/// The setting: how much room the index may take, in megabytes.
pub const LIMIT_SETTING: &str = "tags.index_mb";
pub const LIMIT_DEFAULT: i64 = 64;

/// What a file and a folder are counted as taking, past their names: the
/// entry, its allocations and the map's share (measured, `scale_probe`).
const FILE_BYTES: u64 = 40;
const FOLDER_BYTES: u64 = 160;

const BUCKETS: usize = 256;
/// What it knows between pieces: tag names, folders covered, room taken.
const META: &[u8] = b"index.4";
/// The folders it holds files of.
const FOLDERS: &[u8] = b"index.4.folders";
const PIECE_MAGIC: &[u8; 4] = b"BTI1";

/// How long a piece stays read unused.
const IDLE: Duration = Duration::from_secs(180);
/// How long the first write after the thread starts waits.
const FIRST_WRITE: Duration = Duration::from_secs(1);
/// What changed is written at most this often: a count teaches the index
/// in batches, and each written alone would write every piece again.
const WRITE_EVERY: Duration = Duration::from_secs(2);

/// A folder whose every tagged file, outside hidden folders, the index
/// holds as of a time: a count of it ended then (`count.rs`), and what was
/// learned since keeps it so.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Covered {
    pub root: PathBuf,
    /// Seconds and nanoseconds since the epoch.
    pub since: (u64, u32),
}

impl Covered {
    pub fn since(&self) -> SystemTime {
        UNIX_EPOCH + Duration::new(self.since.0, self.since.1)
    }
}

/// A path's 64-bit FNV-1a hash: the same in every build, so what was
/// stored with one is read by the next.
pub fn stable_hash(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Where the folders seen by a count of `root` are kept.
fn dirs_key(root: &Path) -> Vec<u8> {
    format!(
        "dirs.{:016x}",
        stable_hash(&[root.as_os_str().as_encoded_bytes()])
    )
    .into_bytes()
}

fn piece_key(piece: usize) -> Vec<u8> {
    format!("index.4.{piece:02x}").into_bytes()
}

fn piece_of(folder: &Path) -> usize {
    (stable_hash(&[folder.as_os_str().as_encoded_bytes()]) % BUCKETS as u64) as usize
}

fn hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

/// Whether `path`, under `root`, is inside a hidden folder or hidden
/// itself: left out of the counts as a search leaves it out.
fn hidden_under(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .is_ok_and(|rest| rest.components().any(|c| hidden(c.as_os_str())))
}

/// The network mounts below `root`, as the table in memory has them: what
/// a walk of it never enters (ADR-077), nor its counts count.
fn mounts_below(root: &Path) -> Vec<PathBuf> {
    reach::all()
        .iter()
        .filter(|r| r.point != root && r.point.starts_with(root))
        .map(|r| r.point.clone())
        .collect()
}

/// A tagged file: its tags, by number, and its name in its folder, in one
/// allocation (how many tags, each four bytes, then the name), so a large
/// library takes little room.
struct File(Box<[u8]>);

impl File {
    fn new(name: &OsStr, tags: &[u32]) -> File {
        let n = tags.len().min(255);
        let mut data = Vec::with_capacity(1 + 4 * n + name.len());
        data.push(u8::try_from(n).unwrap_or(u8::MAX));
        for tag in &tags[..n] {
            data.extend_from_slice(&tag.to_le_bytes());
        }
        data.extend_from_slice(name.as_encoded_bytes());
        File(data.into_boxed_slice())
    }

    fn split(&self) -> (&[u8], &[u8]) {
        let n = usize::from(self.0[0]);
        self.0[1..].split_at(4 * n)
    }

    fn name(&self) -> Cow<'_, OsStr> {
        skwir_core::os_path::from_bytes_ref(self.name_bytes())
    }

    /// Its name as stored: `OsStr::as_encoded_bytes`.
    fn name_bytes(&self) -> &[u8] {
        self.split().1
    }

    fn tags(&self) -> Vec<u32> {
        self.split()
            .0
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect()
    }

    /// The room it is counted as taking (`file_bytes`).
    fn room(&self) -> u64 {
        let (tags, name) = self.split();
        FILE_BYTES + name.len() as u64 + tags.len() as u64
    }

    fn has(&self, ids: &[u32]) -> bool {
        self.split()
            .0
            .as_chunks::<4>()
            .0
            .iter()
            .any(|c| ids.contains(&u32::from_le_bytes(*c)))
    }
}

/// A folder's tagged files, by name, and how many of those not hidden
/// carry each tag.
#[derive(Default)]
struct Folder {
    files: Vec<File>,
    /// By tag number: a folder of photos may carry thousands of keywords.
    counts: Vec<(u32, u32)>,
    /// Its files not hidden.
    visible: u32,
}

impl Folder {
    fn find(&self, name: &OsStr) -> Result<usize, usize> {
        self.files
            .binary_search_by(|f| f.name_bytes().cmp(name.as_encoded_bytes()))
    }

    /// Count `name`'s `tags` in, or out, with the categories they are in
    /// (`parents`, by tag number), each once.
    fn count(&mut self, name: &OsStr, tags: &[u32], parents: &[Option<u32>], add: bool) {
        if hidden(name) {
            return;
        }
        let tags = &with_categories(tags, parents);
        if add {
            self.visible += 1;
        } else {
            self.visible = self.visible.saturating_sub(1);
        }
        for tag in tags {
            match self.counts.binary_search_by_key(tag, |(t, _)| *t) {
                Ok(at) if add => self.counts[at].1 += 1,
                Ok(at) => {
                    self.counts[at].1 = self.counts[at].1.saturating_sub(1);
                    if self.counts[at].1 == 0 {
                        self.counts.remove(at);
                    }
                }
                Err(at) if add => self.counts.insert(at, (*tag, 1)),
                Err(_) => {}
            }
        }
    }

    /// `name` has `tags` now (none: it is forgotten): what it had.
    fn set(&mut self, name: &OsStr, tags: &[u32], parents: &[Option<u32>]) -> Option<Vec<u32>> {
        match self.find(name) {
            Ok(at) => {
                let old = self.files[at].tags();
                if tags.is_empty() {
                    self.files.remove(at);
                } else {
                    self.files[at] = File::new(name, tags);
                }
                self.count(name, &old, parents, false);
                if !tags.is_empty() {
                    self.count(name, tags, parents, true);
                }
                Some(old)
            }
            Err(_) if tags.is_empty() => None,
            Err(at) => {
                self.count(name, tags, parents, true);
                self.files.insert(at, File::new(name, tags));
                None
            }
        }
    }
}

/// `tags` and the categories they are in, each once.
fn with_categories(tags: &[u32], parents: &[Option<u32>]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(tags.len());
    for tag in tags {
        let mut at = Some(*tag);
        while let Some(t) = at {
            if out.contains(&t) {
                break;
            }
            out.push(t);
            at = parents.get(t as usize).copied().flatten();
        }
    }
    out
}

/// One piece: the folders whose path falls in it.
#[derive(Default)]
struct Piece {
    folders: HashMap<PathBuf, Folder>,
    loaded: bool,
    dirty: bool,
    used: Option<Instant>,
}

/// What is known between pieces, stored as one value.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Meta {
    tags: Vec<String>,
    covered: Vec<Covered>,
    /// The room each piece takes.
    bytes: Vec<u64>,
    /// The home folder `home_visible` is of.
    home: Option<PathBuf>,
    /// Files under the home folder outside hidden folders, by tag, as its
    /// tag places count them. `None` from an index that counted the hidden
    /// ones too (`home_counts`, no longer read): made again from every
    /// piece.
    #[serde(default)]
    home_visible: Option<Vec<(u32, u64)>>,
    /// A file was not learned for want of room.
    refused: bool,
}

#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent flags: read, what to write, and room refused"
)]
struct State {
    read: bool,
    tags: Vec<String>,
    ids: HashMap<String, u32>,
    /// Each tag's category, by number.
    parents: Vec<Option<u32>>,
    covered: Vec<Covered>,
    pieces: Vec<Piece>,
    bytes: Vec<u64>,
    /// The folders it holds files of, each with its piece: in path order,
    /// so the folders under one are together.
    folders: BTreeMap<PathBuf, usize>,
    folders_dirty: bool,
    home: Option<PathBuf>,
    /// Files under `home` outside hidden folders, by tag: what its folders
    /// there count, kept known between pieces (`Meta::home_visible`).
    home_counts: HashMap<u32, u64>,
    refused: bool,
    meta_dirty: bool,
}

impl State {
    fn total(&self) -> u64 {
        self.bytes.iter().sum()
    }

    fn id(&mut self, tag: &str) -> u32 {
        if let Some(id) = self.ids.get(tag) {
            return *id;
        }
        let parent = tag_parent(tag).map(|p| self.id(p));
        let id = u32::try_from(self.tags.len()).unwrap_or(u32::MAX);
        self.tags.push(tag.to_owned());
        self.parents.push(parent);
        self.ids.insert(tag.to_owned(), id);
        self.meta_dirty = true;
        id
    }

    /// Each tag's category, the categories missing given a number.
    fn link_categories(&mut self) {
        self.parents.clear();
        let mut at = 0;
        while at < self.tags.len() {
            let tag = self.tags[at].clone();
            let parent = tag_parent(&tag).map(|p| self.id(p));
            // `id` may have pushed the category's own entry meanwhile.
            self.parents.resize(self.tags.len(), None);
            self.parents[at] = parent;
            at += 1;
        }
    }

    /// The folders under `root` (itself included), each with its piece.
    fn under(&self, root: &Path) -> Vec<(PathBuf, usize)> {
        self.folders
            .range(root.to_path_buf()..)
            .take_while(|(p, _)| p.starts_with(root))
            .map(|(p, b)| (p.clone(), *b))
            .collect()
    }

    /// Whether the files of `dir` are counted under the home folder: in
    /// it, outside hidden folders.
    fn at_home(&self, dir: &Path) -> bool {
        self.home
            .as_deref()
            .is_some_and(|home| dir.starts_with(home) && !hidden_under(dir, home))
    }

    /// What a batch changed of the files counted under the home folder, by
    /// tag, added once: only a tag that reaches none is taken out, where
    /// every file learned went over every tag.
    fn add_home(&mut self, changes: HashMap<u32, i64>) {
        for (tag, change) in changes {
            if change == 0 {
                continue;
            }
            let n = self.home_counts.entry(tag).or_default();
            *n = n.saturating_add_signed(change);
            if *n == 0 {
                self.home_counts.remove(&tag);
            }
        }
        self.meta_dirty = true;
    }

    /// Take the folder `path` (of piece `at`) out whole, with the room its
    /// files took and what they counted under the home folder: a folder
    /// moved away or gone, rather than each of its files forgotten, which
    /// moved all those after it, a folder of many thousands quadratic.
    fn take_folder(&mut self, at: usize, path: &Path) -> Option<Folder> {
        let folder = self.pieces[at].folders.remove(path)?;
        let room = folder_bytes(path) + folder.files.iter().map(File::room).sum::<u64>();
        self.bytes[at] = self.bytes[at].saturating_sub(room);
        if self.folders.remove(path).is_some() {
            self.folders_dirty = true;
        }
        if self.at_home(path) {
            let gone = folder
                .counts
                .iter()
                .map(|(tag, n)| (*tag, -i64::from(*n)))
                .collect();
            self.add_home(gone);
        }
        self.pieces[at].dirty = true;
        self.meta_dirty = true;
        Some(folder)
    }
}

/// One read of a file a batch learns, its folder and its name found once:
/// the folder is the first `dir` bytes of its path, the name those from
/// `name` on.
struct Read {
    path: PathBuf,
    dir: usize,
    name: usize,
    tags: Vec<String>,
}

impl Read {
    fn of(path: PathBuf, tags: Vec<String>) -> Option<Read> {
        let dir = path.parent()?.as_os_str().len();
        let name = path.file_name()?.len();
        let len = path.as_os_str().len();
        // A name ends its path as listings join them; one spelled another
        // way (a trailing separator) is joined again.
        if !path
            .as_os_str()
            .as_encoded_bytes()
            .ends_with(path.file_name()?.as_encoded_bytes())
        {
            let joined = path.parent()?.join(path.file_name()?);
            return Read::of(joined, tags);
        }
        Some(Read {
            path,
            dir,
            name: len - name,
            tags,
        })
    }

    fn dir_bytes(&self) -> &[u8] {
        &self.path.as_os_str().as_encoded_bytes()[..self.dir]
    }

    fn name_bytes(&self) -> &[u8] {
        &self.path.as_os_str().as_encoded_bytes()[self.name..]
    }

    fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new(""))
    }

    fn name(&self) -> &OsStr {
        self.path.file_name().unwrap_or_default()
    }
}

/// What becomes of one file a batch learns.
enum Fate {
    /// As it was: nothing changed, or no room for it.
    Same,
    /// Known, or added, with these tags.
    Tags(Vec<u32>),
    /// Forgotten.
    Gone,
}

/// One folder's files being learned: the room taken as it goes, and what
/// changes under the home folder.
struct Learning<'a> {
    dir: &'a Path,
    at: usize,
    limit: u64,
    /// The room the index takes in all.
    total: &'a mut u64,
    /// The folder's files as it goes: a folder holding none takes no room.
    live: usize,
    /// Its files are counted under the home folder.
    at_home: bool,
    home: &'a mut HashMap<u32, i64>,
    changed: bool,
}

impl Learning<'_> {
    /// Learn that the file `name` of `folder` (taken out of its piece) has
    /// `ids` (none: forget it), having had `had` if it was known.
    fn file(
        &mut self,
        state: &mut State,
        folder: &mut Folder,
        name: &OsStr,
        had: Option<&[u32]>,
        ids: Vec<u32>,
    ) -> Fate {
        if had == Some(ids.as_slice()) {
            return Fate::Same;
        }
        if had.is_none() {
            if ids.is_empty() {
                return Fate::Same;
            }
            let more = file_bytes(name, ids.len())
                + if self.live == 0 {
                    folder_bytes(self.dir)
                } else {
                    0
                };
            if *self.total + more > self.limit {
                // No room: no folder can be said to be whole.
                if !state.refused || !state.covered.is_empty() {
                    state.refused = true;
                    state.covered.clear();
                    state.meta_dirty = true;
                    self.changed = true;
                }
                return Fate::Same;
            }
        }
        let parents = &state.parents;
        let counted = self.at_home && !hidden(name);
        let before = state.bytes[self.at];
        let mut bytes = before;
        if let Some(old) = had {
            folder.count(name, old, parents, false);
            bytes = bytes.saturating_sub(file_bytes(name, old.len()));
            if counted {
                for tag in with_categories(old, parents) {
                    *self.home.entry(tag).or_default() -= 1;
                }
            }
        }
        if !ids.is_empty() {
            folder.count(name, &ids, parents, true);
            bytes += file_bytes(name, ids.len());
            if counted {
                for tag in with_categories(&ids, parents) {
                    *self.home.entry(tag).or_default() += 1;
                }
            }
        }
        // The folder's own room, as it comes to hold files or none.
        let was = self.live;
        self.live = self.live + usize::from(had.is_none()) - usize::from(ids.is_empty());
        if was == 0 && self.live > 0 {
            bytes += folder_bytes(self.dir);
        } else if was > 0 && self.live == 0 {
            bytes = bytes.saturating_sub(folder_bytes(self.dir));
        }
        state.bytes[self.at] = bytes;
        *self.total = *self.total - before + bytes;
        state.pieces[self.at].dirty = true;
        state.meta_dirty = true;
        self.changed = true;
        if ids.is_empty() {
            Fate::Gone
        } else {
            Fate::Tags(ids)
        }
    }
}

/// Bytes written and read back: lengths as little-endian u32.
fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(bytes);
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn u32(&mut self) -> Option<u32> {
        let (head, rest) = self.bytes.split_first_chunk::<4>()?;
        self.bytes = rest;
        Some(u32::from_le_bytes(*head))
    }
    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        let (head, rest) = (self.bytes.get(..n)?, self.bytes.get(n..)?);
        self.bytes = rest;
        Some(head)
    }
}

fn encode_piece(piece: &Piece) -> Vec<u8> {
    let mut out = PIECE_MAGIC.to_vec();
    out.extend_from_slice(
        &u32::try_from(piece.folders.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    for (path, folder) in &piece.folders {
        put(&mut out, path.as_os_str().as_encoded_bytes());
        out.extend_from_slice(
            &u32::try_from(folder.files.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for file in &folder.files {
            let tags = file.tags();
            put(&mut out, file.name_bytes());
            out.extend_from_slice(&u32::try_from(tags.len()).unwrap_or(u32::MAX).to_le_bytes());
            for tag in tags {
                out.extend_from_slice(&tag.to_le_bytes());
            }
        }
    }
    out
}

fn decode_piece(bytes: &[u8], parents: &[Option<u32>]) -> Option<HashMap<PathBuf, Folder>> {
    let tags = parents.len();
    let mut r = Reader {
        bytes: bytes.strip_prefix(PIECE_MAGIC)?,
    };
    let mut folders = HashMap::new();
    for _ in 0..r.u32()? {
        let path = PathBuf::from(skwir_core::os_path::from_bytes(r.bytes()?.to_vec()));
        let mut folder = Folder::default();
        for _ in 0..r.u32()? {
            let name = skwir_core::os_path::from_bytes_ref(r.bytes()?);
            let n = r.u32()?;
            let ids: Option<Vec<u32>> = (0..n).map(|_| r.u32()).collect();
            let ids: Vec<u32> = ids?.into_iter().filter(|t| (*t as usize) < tags).collect();
            folder.set(&name, &ids, parents);
        }
        folders.insert(path, folder);
    }
    Some(folders)
}

fn encode_folders(folders: &BTreeMap<PathBuf, usize>) -> Vec<u8> {
    let mut out = Vec::new();
    for path in folders.keys() {
        put(&mut out, path.as_os_str().as_encoded_bytes());
    }
    out
}

fn decode_folders(bytes: &[u8]) -> BTreeMap<PathBuf, usize> {
    let mut r = Reader { bytes };
    let mut out = BTreeMap::new();
    while let Some(path) = r.bytes() {
        let path = PathBuf::from(skwir_core::os_path::from_bytes(path.to_vec()));
        let piece = piece_of(&path);
        out.insert(path, piece);
    }
    out
}

fn folder_bytes(path: &Path) -> u64 {
    FOLDER_BYTES + path.as_os_str().len() as u64
}

fn file_bytes(name: &OsStr, tags: usize) -> u64 {
    FILE_BYTES + name.len() as u64 + 4 * tags as u64
}

/// What the index has to say when it changed: the sidebar's tags.
pub type OnChange = Box<dyn Fn(&Arc<TagIndex>) + Send + Sync>;

/// The index.
pub struct TagIndex {
    state: Mutex<State>,
    slot: Option<CacheSlot>,
    lifetime: CancelToken,
    /// Something changed that is not written yet.
    pending: AtomicBool,
    /// When it was last written.
    written: Mutex<Option<Instant>>,
    on_change: Mutex<Option<OnChange>>,
    /// The room it may take, in bytes (`LIMIT_SETTING`).
    limit: AtomicU64,
    /// Its thread runs: it writes what changed, a little later, and lets
    /// pieces go once unused.
    working: AtomicBool,
    /// Wakes that thread when something changed while it slept.
    nudge: (
        crossbeam_channel::Sender<()>,
        crossbeam_channel::Receiver<()>,
    ),
    /// Itself, for that thread.
    me: Weak<TagIndex>,
}

impl TagIndex {
    pub fn new(slot: Option<CacheSlot>, app: &App, lifetime: CancelToken) -> Arc<Self> {
        let index = Arc::new_cyclic(|me| TagIndex {
            state: Mutex::default(),
            slot,
            lifetime,
            pending: AtomicBool::new(false),
            written: Mutex::new(None),
            on_change: Mutex::default(),
            limit: AtomicU64::new(0),
            working: AtomicBool::new(false),
            nudge: crossbeam_channel::bounded(1),
            me: me.clone(),
        });
        index.set_limit(
            app.settings
                .get::<i64>(LIMIT_SETTING)
                .unwrap_or(LIMIT_DEFAULT),
        );
        index
    }

    /// The room it may take, in megabytes. Raised, it learns again what it
    /// refused; the folders it covers stay uncovered until counted.
    pub fn set_limit(&self, megabytes: i64) {
        let bytes = u64::try_from(megabytes.max(1)).unwrap_or(1) << 20;
        if self.limit.swap(bytes, Ordering::Relaxed) < bytes {
            let mut state = self.state.lock();
            if state.refused {
                state.refused = false;
                state.meta_dirty = true;
            }
        }
    }

    /// Call `f` on a worker after the index changed, once for changes
    /// close together.
    pub fn on_change(&self, f: OnChange) {
        *self.on_change.lock() = Some(f);
    }

    /// The index, what is known between pieces read from the cache the
    /// first time a worker needs it, never at startup.
    fn state(&self) -> MutexGuard<'_, State> {
        let mut state = self.state.lock();
        if !state.read {
            self.read(&mut state);
        }
        state
    }

    fn read(&self, state: &mut State) {
        state.read = true;
        state.pieces = (0..BUCKETS).map(|_| Piece::default()).collect();
        state.bytes = vec![0; BUCKETS];
        let home = std::env::home_dir();
        let slot = self.slot.as_ref();
        let meta: Option<Meta> = slot
            .and_then(|s| s.get(META))
            .and_then(|b| serde_json::from_slice(&b).ok());
        let Some(meta) = meta else {
            // Nothing stored: every piece is empty and read.
            for piece in &mut state.pieces {
                piece.loaded = true;
            }
            state.home = home;
            return;
        };
        state.ids = meta
            .tags
            .iter()
            .enumerate()
            .map(|(i, t)| (t.clone(), u32::try_from(i).unwrap_or(u32::MAX)))
            .collect();
        state.tags = meta.tags;
        state.link_categories();
        state.covered = meta.covered;
        if meta.bytes.len() == BUCKETS {
            state.bytes = meta.bytes;
        }
        state.refused = meta.refused;
        state.folders = slot
            .and_then(|s| s.get(FOLDERS))
            .map(|b| decode_folders(&b))
            .unwrap_or_default();
        match meta.home_visible {
            Some(counts) if meta.home == home => {
                state.home_counts = counts.into_iter().collect();
                state.home = home;
            }
            _ => {
                // Another home, or counts of hidden files too: made again
                // from every piece, as its folders count their files.
                state.home = home;
                self.load_all(state);
                let mut counts: HashMap<u32, u64> = HashMap::new();
                if let Some(home) = &state.home {
                    for (path, folder) in state.pieces.iter().flat_map(|p| p.folders.iter()) {
                        if path.starts_with(home) && !hidden_under(path, home) {
                            for (tag, n) in &folder.counts {
                                *counts.entry(*tag).or_default() += u64::from(*n);
                            }
                        }
                    }
                }
                state.home_counts = counts;
                state.meta_dirty = true;
            }
        }
    }

    /// Read piece `at`, if it is not.
    fn load(&self, state: &mut State, at: usize) {
        let parents = &state.parents;
        let piece = &mut state.pieces[at];
        piece.used = Some(Instant::now());
        if piece.loaded {
            return;
        }
        piece.loaded = true;
        piece.folders = self
            .slot
            .as_ref()
            .and_then(|s| s.get(&piece_key(at)))
            .and_then(|b| decode_piece(&b, parents))
            .unwrap_or_default();
        if self.slot.is_some() {
            self.work();
        }
    }

    fn load_all(&self, state: &mut State) {
        for at in 0..BUCKETS {
            self.load(state, at);
        }
    }

    /// The pieces holding the folders under `root`, read.
    fn load_under(&self, state: &mut State, root: &Path) -> Vec<(PathBuf, usize)> {
        let under = state.under(root);
        self.load_pieces(state, &under);
        under
    }

    /// [`load_under`](Self::load_under), but the folders a walk of `root`
    /// reaches: none in a network mount below it (ADR-077), which a count
    /// never enters and a search never walks, though a pane shown there
    /// taught the index its files. What the index answers of `root` is
    /// what a count of it covers, and nothing there is read to tell.
    fn load_reached(&self, state: &mut State, root: &Path) -> Vec<(PathBuf, usize)> {
        let unreached = mounts_below(root);
        let mut under = state.under(root);
        under.retain(|(path, _)| !unreached.iter().any(|point| path.starts_with(point)));
        self.load_pieces(state, &under);
        under
    }

    fn load_pieces(&self, state: &mut State, folders: &[(PathBuf, usize)]) {
        let pieces: HashSet<usize> = folders.iter().map(|(_, b)| *b).collect();
        for at in pieces {
            self.load(state, at);
        }
    }

    /// Its thread, if it is not running (woken, if it is): it writes what
    /// changed (at most every `WRITE_EVERY`) and tells who listens, and
    /// lets go of the pieces unused, asleep until the next of these is
    /// due (it woke every second for three minutes after any use); it
    /// ends when nothing is left to write or read, and writes what is left
    /// when the plugin or the app ends.
    fn work(&self) {
        if self.working.swap(true, Ordering::AcqRel) {
            let _ = self.nudge.0.try_send(());
            return;
        }
        let this = self.me.clone();
        let signal = self.lifetime.subscribe();
        let nudged = self.nudge.1.clone();
        // A first write waits a moment too, for what comes with the change.
        let first = Instant::now() + FIRST_WRITE;
        let spawned = std::thread::Builder::new()
            .name("skwir-tags-index".into())
            .spawn(move || {
                loop {
                    let Some(wait) = this.upgrade().map(|index| index.next_wait(first)) else {
                        return;
                    };
                    let ended = crossbeam_channel::select! {
                        recv(signal.receiver()) -> _ => true,
                        recv(nudged) -> _ => false,
                        recv(crossbeam_channel::after(wait)) -> _ => false,
                    };
                    let Some(index) = this.upgrade() else { return };
                    let due = index
                        .written
                        .lock()
                        .map_or(Instant::now() >= first, |w| w.elapsed() >= WRITE_EVERY);
                    if index.pending.load(Ordering::Acquire) && (due || ended) {
                        index.pending.store(false, Ordering::Release);
                        index.write();
                        *index.written.lock() = Some(Instant::now());
                        if !ended && let Some(f) = &*index.on_change.lock() {
                            f(&index);
                        }
                    }
                    let read = index.slot.is_some() && index.unload_idle();
                    // What a write or the pieces let go of freed, given
                    // back before it sleeps: it is no scheduler worker.
                    skwir_core::scheduler::release_now();
                    if ended || (!read && !index.pending.load(Ordering::Acquire)) {
                        index.working.store(false, Ordering::Release);
                        // Something changed as it ended: carry on.
                        if ended
                            || !index.pending.load(Ordering::Acquire)
                            || index.working.swap(true, Ordering::AcqRel)
                        {
                            return;
                        }
                    }
                }
            });
        if spawned.is_err() {
            self.working.store(false, Ordering::Release);
        }
    }

    /// How long its thread may sleep: until what changed may be written
    /// (the thread's first write at `first`), or the first piece read goes
    /// unused long enough to be let go of.
    fn next_wait(&self, first: Instant) -> Duration {
        let now = Instant::now();
        let write = self.pending.load(Ordering::Acquire).then(|| {
            self.written
                .lock()
                .map_or(first, |w| w + WRITE_EVERY)
                .saturating_duration_since(now)
        });
        let unload = self
            .state
            .lock()
            .pieces
            .iter()
            .filter(|piece| piece.loaded)
            .map(|piece| {
                piece.used.map_or(Duration::ZERO, |u| {
                    (u + IDLE).saturating_duration_since(now)
                })
            })
            .min();
        write.into_iter().chain(unload).min().unwrap_or(IDLE)
    }

    /// Let go of the pieces unused for a while and written: whether some
    /// are still read.
    fn unload_idle(&self) -> bool {
        let mut state = self.state.lock();
        let mut still = false;
        for piece in &mut state.pieces {
            if !piece.loaded {
                continue;
            }
            if !piece.dirty && piece.used.is_none_or(|u| u.elapsed() >= IDLE) {
                piece.folders = HashMap::new();
                piece.loaded = false;
            } else {
                still = true;
            }
        }
        still
    }

    /// Learn `read` into `state`, past `limit` no new file: whether that
    /// changed it.
    fn learn_into(state: &mut State, read: Vec<(PathBuf, Vec<String>)>, limit: u64) -> bool {
        // By folder, then by name, each as its bytes (`Folder::find`): each
        // folder's files together, in the order it keeps them, learned in
        // one pass. Stable: the later of two reads of a file wins.
        let mut read: Vec<Read> = read
            .into_iter()
            .filter_map(|(path, tags)| Read::of(path, tags))
            .collect();
        read.sort_by(|a, b| (a.dir_bytes(), a.name_bytes()).cmp(&(b.dir_bytes(), b.name_bytes())));
        let mut changed = false;
        let mut total = state.total();
        let mut home: HashMap<u32, i64> = HashMap::new();
        for group in read.chunk_by(|a, b| a.dir_bytes() == b.dir_bytes()) {
            changed |= Self::learn_folder(state, group, limit, &mut total, &mut home);
        }
        if !home.is_empty() {
            state.add_home(home);
        }
        changed
    }

    /// Learn `group`, the reads of one folder's files in its order, into
    /// it: each file changed where it is when none comes or goes, else the
    /// folder's files from the group's first on and the group merged in one
    /// pass, where each file added or forgotten moved all those after it.
    fn learn_folder(
        state: &mut State,
        group: &[Read],
        limit: u64,
        total: &mut u64,
        home: &mut HashMap<u32, i64>,
    ) -> bool {
        let dir = group[0].dir();
        // Each file's last read, its tags by number, before the folder is
        // taken out: `id` may add tags and their categories.
        let run: Vec<(&OsStr, Vec<u32>)> = group
            .iter()
            .enumerate()
            .filter(|(i, r)| {
                group
                    .get(i + 1)
                    .is_none_or(|next| next.name_bytes() != r.name_bytes())
            })
            .map(|(_, r)| {
                let mut ids: Vec<u32> = Vec::new();
                for tag in &r.tags {
                    let id = state.id(tag);
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                (r.name(), ids)
            })
            .collect();
        let at = piece_of(dir);
        // Taken out while its files are learned, and put back where it was.
        let taken = state.pieces[at].folders.get_mut(dir).map(std::mem::take);
        let mut folder = taken.unwrap_or_default();
        let held = !folder.files.is_empty();
        let mut learning = Learning {
            dir,
            at,
            limit,
            total,
            live: folder.files.len(),
            at_home: state.at_home(dir),
            home,
            changed: false,
        };
        // A file comes (unknown, with tags) or goes (known, with none).
        let moves = run
            .iter()
            .any(|(name, ids)| folder.find(name).is_ok() == ids.is_empty());
        if moves {
            // The files before the first of the group stay where they are:
            // files learned in the order they are named come at the end.
            let start = run
                .first()
                .map_or(0, |(name, _)| folder.find(name).unwrap_or_else(|at| at));
            let mut files = std::mem::take(&mut folder.files);
            let mut old = files.split_off(start).into_iter().peekable();
            files.reserve(old.len() + run.len());
            for (name, ids) in run {
                let bytes = name.as_encoded_bytes();
                while let Some(file) = old.next_if(|f| f.name_bytes() < bytes) {
                    files.push(file);
                }
                let known = old.next_if(|f| f.name_bytes() == bytes);
                let had = known.as_ref().map(File::tags);
                match learning.file(state, &mut folder, name, had.as_deref(), ids) {
                    Fate::Same => files.extend(known),
                    Fate::Tags(ids) => files.push(File::new(name, &ids)),
                    Fate::Gone => {}
                }
            }
            files.extend(old);
            folder.files = files;
        } else {
            for (name, ids) in run {
                // Unknown and without tags: nothing to learn.
                let Ok(i) = folder.find(name) else { continue };
                let had = folder.files[i].tags();
                if let Fate::Tags(ids) = learning.file(state, &mut folder, name, Some(&had), ids) {
                    folder.files[i] = File::new(name, &ids);
                }
            }
        }
        let changed = learning.changed;
        let piece = &mut state.pieces[at].folders;
        match (held, folder.files.is_empty()) {
            (true, false) => {
                if let Some(place) = piece.get_mut(dir) {
                    *place = folder;
                }
            }
            (true, true) => {
                piece.remove(dir);
                if state.folders.remove(dir).is_some() {
                    state.folders_dirty = true;
                }
            }
            (false, false) => {
                piece.insert(dir.to_path_buf(), folder);
                state.folders.insert(dir.to_path_buf(), at);
                state.folders_dirty = true;
            }
            (false, true) => {}
        }
        changed
    }

    /// What the provider read of files: each path with its tags, none for
    /// a file without. Writes the index soon when that changed it.
    pub fn learn(self: &Arc<Self>, read: Vec<(PathBuf, Vec<String>)>) {
        let limit = self.limit.load(Ordering::Relaxed);
        let changed = {
            let mut state = self.state();
            let pieces: HashSet<usize> = read
                .iter()
                .filter_map(|(p, _)| p.parent())
                .map(piece_of)
                .collect();
            for at in pieces {
                self.load(&mut state, at);
            }
            Self::learn_into(&mut state, read, limit)
        };
        if changed {
            self.write_soon();
        }
    }

    /// Whether it could not learn some file for want of room: its counts
    /// leave files out.
    pub fn is_full(&self) -> bool {
        self.state().refused
    }

    /// The coverage of `root`: the deepest folder covered that holds it,
    /// if any. A folder covered drops the coverage below it (`cover`), so
    /// one still covered below another was counted after it: its count is
    /// the nearer and the newer, and its folders the fewer to compare.
    pub fn covering(&self, root: &Path) -> Option<Covered> {
        self.state()
            .covered
            .iter()
            .filter(|c| root.starts_with(&c.root))
            .max_by_key(|c| c.root.components().count())
            .cloned()
    }

    /// `root` is covered as of `since`, a count having seen `dirs` (the
    /// folders' [`stable_hash`]es), which the next check compares with.
    pub fn cover(self: &Arc<Self>, root: &Path, since: SystemTime, dirs: &[u64]) {
        let Ok(since) = since.duration_since(UNIX_EPOCH) else {
            return;
        };
        if self.is_full() {
            return;
        }
        if let Some(slot) = &self.slot {
            let bytes: Vec<u8> = dirs.iter().flat_map(|d| d.to_le_bytes()).collect();
            if let Err(error) = slot.put(&dirs_key(root), &bytes) {
                tracing::debug!(%error, "tags: the folders counted were not written");
                return;
            }
        }
        {
            let mut state = self.state();
            state.covered.retain(|c| !c.root.starts_with(root));
            state.covered.push(Covered {
                root: root.to_path_buf(),
                since: (since.as_secs(), since.subsec_nanos()),
            });
            state.meta_dirty = true;
        }
        self.write_soon();
    }

    /// The folders the count of `root` saw, as [`cover`](Self::cover) kept
    /// them.
    pub fn dirs(&self, root: &Path) -> HashSet<u64> {
        self.slot
            .as_ref()
            .and_then(|slot| slot.get(&dirs_key(root)))
            .map(|bytes| {
                bytes
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| u64::from_le_bytes(*c))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The tagged files under `root` it knows, by their folder.
    pub fn by_folder(&self, root: &Path) -> HashMap<PathBuf, Vec<PathBuf>> {
        let mut state = self.state();
        let under = self.load_reached(&mut state, root);
        under
            .into_iter()
            .filter_map(|(path, at)| {
                let folder = state.pieces[at].folders.get(&path)?;
                let files = folder.files.iter().map(|f| path.join(f.name())).collect();
                Some((path, files))
            })
            .collect()
    }

    /// What skwir applied moved files: `from` (and what is under it) is
    /// at `to`, or gone. A folder covered that moved is no longer.
    pub fn moved(self: &Arc<Self>, moves: &[(PathBuf, Option<PathBuf>)]) {
        let limit = u64::MAX;
        let changed = {
            let mut state = self.state();
            let mut changed = false;
            for (from, to) in moves {
                // The files it held under `from`: a folder's, or one file.
                let mut read: Vec<(PathBuf, Vec<String>)> = Vec::new();
                let mut forget: Vec<(PathBuf, Vec<String>)> = Vec::new();
                // Each folder under it goes whole, its files learned where
                // they went.
                for (folder, at) in self.load_under(&mut state, from) {
                    let Some(f) = state.take_folder(at, &folder) else {
                        continue;
                    };
                    changed = true;
                    let (Some(to), Ok(rest)) = (to, folder.strip_prefix(from)) else {
                        continue;
                    };
                    let there = to.join(rest);
                    for file in &f.files {
                        let tags: Vec<String> = file
                            .tags()
                            .into_iter()
                            .map(|t| state.tags[t as usize].clone())
                            .collect();
                        read.push((there.join(file.name()), tags));
                    }
                }
                if let (Some(dir), Some(name)) = (from.parent(), from.file_name()) {
                    let at = piece_of(dir);
                    self.load(&mut state, at);
                    let tags = state.pieces[at]
                        .folders
                        .get(dir)
                        .and_then(|f| f.find(name).ok().map(|i| f.files[i].tags()));
                    if let Some(tags) = tags {
                        let tags: Vec<String> = tags
                            .iter()
                            .map(|t| state.tags[*t as usize].clone())
                            .collect();
                        forget.push((from.clone(), Vec::new()));
                        if let Some(to) = to {
                            read.push((to.clone(), tags));
                        }
                    }
                }
                let pieces: HashSet<usize> = read
                    .iter()
                    .filter_map(|(p, _)| p.parent())
                    .map(piece_of)
                    .collect();
                for at in pieces {
                    self.load(&mut state, at);
                }
                changed |= Self::learn_into(&mut state, forget, limit);
                changed |= Self::learn_into(&mut state, read, limit);
                let before = state.covered.len();
                state.covered.retain(|c| !c.root.starts_with(from));
                if state.covered.len() != before {
                    state.meta_dirty = true;
                    changed = true;
                }
            }
            changed
        };
        if changed {
            self.write_soon();
        }
    }

    /// Each tag with how many files have it, spelled as most of them spell
    /// it, from counts by tag number.
    fn spelled(state: &State, counts: HashMap<u32, u64>) -> Vec<(String, usize)> {
        let mut by_name: HashMap<String, (u64, u64, &str)> = HashMap::new();
        for (id, n) in counts {
            let Some(tag) = state.tags.get(id as usize) else {
                continue;
            };
            let e = by_name
                .entry(tag.to_lowercase())
                .or_insert((0, 0, tag.as_str()));
            e.0 += n;
            // The spelling of the most files; a tie goes to the first.
            if n > e.1 || (n == e.1 && tag.as_str() < e.2) {
                e.1 = n;
                e.2 = tag.as_str();
            }
        }
        by_name
            .into_values()
            .map(|(n, _, tag)| (tag.to_owned(), usize::try_from(n).unwrap_or(usize::MAX)))
            .collect()
    }

    /// The tags of the files under `root` outside hidden folders and
    /// network mounts, each with how many files have it (spelled as most
    /// of them spell it, in the order of their names), and how many files
    /// have a tag.
    pub fn tally(&self, root: &Path) -> (Vec<(String, usize)>, usize) {
        let mut state = self.state();
        let under = self.load_reached(&mut state, root);
        let mut counts: HashMap<u32, u64> = HashMap::new();
        let mut files = 0usize;
        for (path, at) in under {
            if hidden_under(&path, root) {
                continue;
            }
            if let Some(folder) = state.pieces[at].folders.get(&path) {
                files += folder.visible as usize;
                for (tag, n) in &folder.counts {
                    *counts.entry(*tag).or_default() += u64::from(*n);
                }
            }
        }
        let mut out = Self::spelled(&state, counts);
        drop(state);
        out.sort_by_key(|(t, _)| t.to_lowercase());
        (out, files)
    }

    /// The files under `root` known to have `tag`, as the index knows
    /// them: nothing is read. A search's hint, which stats every file it
    /// is named anyway (ADR-026).
    pub fn candidates_with(&self, tag: &str, root: &Path) -> Vec<PathBuf> {
        let mut state = self.state();
        let ids: Vec<u32> = state
            .tags
            .iter()
            .enumerate()
            // A category's files: those of the tags within it.
            .filter(|(_, t)| tag_within(t, tag))
            .map(|(i, _)| u32::try_from(i).unwrap_or(u32::MAX))
            .collect();
        if ids.is_empty() {
            return Vec::new();
        }
        let under = self.load_reached(&mut state, root);
        under
            .into_iter()
            .filter_map(|(path, at)| Some((state.pieces[at].folders.get(&path)?, path)))
            .flat_map(|(folder, path)| {
                folder
                    .files
                    .iter()
                    .filter(|f| f.has(&ids))
                    .map(move |f| path.join(f.name()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The files under `root` known to have `tag`. Those gone since are
    /// forgotten on the way.
    pub fn files_with(self: &Arc<Self>, tag: &str, root: &Path) -> Vec<PathBuf> {
        let found = self.candidates_with(tag, root);
        let (here, gone): (Vec<PathBuf>, Vec<PathBuf>) = found.into_iter().partition(|path| {
            !matches!(std::fs::symlink_metadata(path),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        });
        if !gone.is_empty() {
            self.learn(gone.into_iter().map(|p| (p, Vec::new())).collect());
        }
        here
    }

    /// The tagged files under `root` outside hidden folders and network
    /// mounts, each with its tags as spelled on it: those with a tag within
    /// `within` only, when given. What a tag location lists
    /// (`location.rs`).
    pub fn files_under(&self, root: &Path, within: Option<&str>) -> Vec<(PathBuf, Vec<String>)> {
        let mut state = self.state();
        let ids: Option<Vec<u32>> = within.map(|tag| {
            state
                .tags
                .iter()
                .enumerate()
                .filter(|(_, t)| tag_within(t, tag))
                .map(|(i, _)| u32::try_from(i).unwrap_or(u32::MAX))
                .collect()
        });
        if ids.as_ref().is_some_and(Vec::is_empty) {
            return Vec::new();
        }
        let under = self.load_reached(&mut state, root);
        let mut out = Vec::new();
        for (path, at) in under {
            if hidden_under(&path, root) {
                continue;
            }
            let Some(folder) = state.pieces[at].folders.get(&path) else {
                continue;
            };
            for file in &folder.files {
                if hidden(&file.name()) || ids.as_ref().is_some_and(|ids| !file.has(ids)) {
                    continue;
                }
                let tags = file
                    .tags()
                    .into_iter()
                    .filter_map(|t| state.tags.get(t as usize).cloned())
                    .collect();
                out.push((path.join(file.name()), tags));
            }
        }
        out
    }

    /// The `at_most` tags on the most files under `root`, each with how
    /// many, the most used first (then by name): what [`tally`](Self::tally)
    /// counts, so each opens a tag place of `root` that lists them. Under
    /// the home folder, from what it keeps known, less what the network
    /// mounts below it hold: their pieces alone are read.
    pub fn top(&self, at_most: usize, root: &Path) -> Vec<(String, usize)> {
        let mut top = {
            let mut state = self.state();
            if state.home.as_deref() == Some(root) {
                let counts = self.counts_at_home(&mut state, &mounts_below(root));
                Self::spelled(&state, counts)
            } else {
                drop(state);
                self.tally(root).0
            }
        };
        top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        top.truncate(at_most);
        top
    }

    /// The tags of the files under the home folder outside hidden folders,
    /// by number, less those in `mounts`, the network mounts below it.
    fn counts_at_home(&self, state: &mut State, mounts: &[PathBuf]) -> HashMap<u32, u64> {
        let mut counts = state.home_counts.clone();
        let Some(home) = state.home.clone() else {
            return counts;
        };
        let mut away: BTreeMap<PathBuf, usize> = BTreeMap::new();
        for point in mounts {
            away.extend(state.under(point));
        }
        let away: Vec<(PathBuf, usize)> = away.into_iter().collect();
        self.load_pieces(state, &away);
        for (path, at) in &away {
            let Some(folder) = state.pieces[*at].folders.get(path) else {
                continue;
            };
            if hidden_under(path, &home) {
                continue;
            }
            for (tag, n) in &folder.counts {
                if let Some(count) = counts.get_mut(tag) {
                    *count = count.saturating_sub(u64::from(*n));
                }
            }
        }
        counts.retain(|_, n| *n > 0);
        counts
    }

    /// Write what changed now, rather than soon (measurements).
    pub fn write_now(&self) {
        self.write();
    }

    /// Write what changed a little later, with what changes meanwhile.
    fn write_soon(self: &Arc<Self>) {
        self.pending.store(true, Ordering::Release);
        self.work();
    }

    /// Write what changed: the pieces changed, the folders held if they
    /// changed, and what is known between pieces.
    fn write(&self) {
        let Some(slot) = &self.slot else { return };
        let (pieces, folders, meta) = {
            let mut state = self.state();
            let pieces: Vec<(usize, Vec<u8>)> = state
                .pieces
                .iter_mut()
                .enumerate()
                .filter(|(_, p)| p.dirty && p.loaded)
                .map(|(at, p)| {
                    p.dirty = false;
                    (at, encode_piece(p))
                })
                .collect();
            let folders =
                std::mem::take(&mut state.folders_dirty).then(|| encode_folders(&state.folders));
            let meta = std::mem::take(&mut state.meta_dirty).then(|| {
                serde_json::to_vec(&Meta {
                    tags: state.tags.clone(),
                    covered: state.covered.clone(),
                    bytes: state.bytes.clone(),
                    home: state.home.clone(),
                    home_visible: Some(state.home_counts.iter().map(|(t, n)| (*t, *n)).collect()),
                    refused: state.refused,
                })
            });
            (pieces, folders, meta)
        };
        let mut failed = None;
        for (at, bytes) in pieces {
            if let Err(e) = slot.put(&piece_key(at), &bytes) {
                failed = Some(e);
            }
        }
        if let Some(bytes) = folders
            && let Err(e) = slot.put(FOLDERS, &bytes)
        {
            failed = Some(e);
        }
        if let Some(Ok(bytes)) = meta
            && let Err(e) = slot.put(META, &bytes)
        {
            failed = Some(e);
        }
        if let Some(error) = failed {
            tracing::debug!(%error, "tags: the index was not written");
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
