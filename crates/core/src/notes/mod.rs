//! A file's notes (ADR-027): its tags, a rating, a comment, where a
//! download came from, as one model every store and every surface agree
//! on, read from every store skwir knows and written to those the user
//! chooses.
//!
//! A [`NotesStore`] is one place notes are kept (the desktop's extended
//! attributes, a Mac's Finder attributes, the `._` file beside a file on a
//! volume without attributes, an XMP sidecar…), registered by a plugin.
//! The registry ([`NoteStores`]) reads a batch of a folder's files from
//! every store and merges what they say; an edit ([`SetNotes`]) writes
//! every store its policy allows, and its inverse puts back the exact
//! bytes each store held ([`raw`]), whatever the format.

pub(crate) mod commands;
mod edit;
pub mod raw;

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::cache::CacheSlot;
use crate::entry::{EntryId, FileId};
use crate::metadata::{Listed, same_tag};
pub use edit::{RestoreNotes, SetNotes, TagChange, TagRename};
use raw::RawKey;

/// A tag, as spelled, and the color a file gives it (a palette name of
/// [`crate::metadata::TAG_COLORS`]: a Mac's Finder tag's), if one does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Tag {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl Tag {
    pub fn named(name: impl Into<String>) -> Self {
        Tag {
            name: name.into(),
            color: None,
        }
    }
}

/// A rating's highest value: five stars, in half stars.
pub const RATING_MAX: u64 = 10;

/// What a file's notes are. A color belongs to its tag, not to the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Notes {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// From 1 to [`RATING_MAX`], half stars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Where a download came from and the page that led to it: the
    /// system's, read only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referrer: Option<String>,
}

impl Notes {
    /// Each tag made one as a store reads it (`metadata::clean_tag`: no
    /// empty category, no space round one), empties and repeats dropped:
    /// another application's `Places//Été` is `Places/Été` here, as a tag
    /// typed so would be.
    fn clean_tags(&mut self) {
        let mut kept: Vec<Tag> = Vec::with_capacity(self.tags.len());
        for mut tag in std::mem::take(&mut self.tags) {
            tag.name = crate::metadata::clean_tag(&tag.name);
            if !tag.name.is_empty() && !kept.iter().any(|t| same_tag(&t.name, &tag.name)) {
                kept.push(tag);
            }
        }
        self.tags = kept;
    }

    /// Whether it holds what the user gives a file (tags, a rating, a
    /// comment): what a store creates a file for. Where a download came
    /// from is the system's, read only.
    pub fn has_user_notes(&self) -> bool {
        !self.tags.is_empty() || self.rating.is_some() || self.comment.is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
            && self.rating.is_none()
            && self.comment.is_none()
            && self.origin.is_none()
            && self.referrer.is_none()
    }

    /// The tags' names, as spelled.
    pub fn tag_names(&self) -> Vec<String> {
        self.tags.iter().map(|t| t.name.clone()).collect()
    }

    /// The tag `name` (ignoring case), if the notes have it.
    pub fn tag(&self, name: &str) -> Option<&Tag> {
        self.tags.iter().find(|t| same_tag(&t.name, name))
    }

    /// Add what `other` says that these do not: its tags missing here (a
    /// tag already here takes the color `other` gives it if it had none),
    /// and each field these leave unset.
    pub fn merge(&mut self, other: Notes) {
        for tag in other.tags {
            match self.tags.iter_mut().find(|t| same_tag(&t.name, &tag.name)) {
                Some(here) => {
                    if here.color.is_none() {
                        here.color = tag.color;
                    }
                }
                None => self.tags.push(tag),
            }
        }
        self.rating = self.rating.or(other.rating);
        self.comment = self.comment.take().or(other.comment);
        self.origin = self.origin.take().or(other.origin);
        self.referrer = self.referrer.take().or(other.referrer);
    }
}

/// Split a comma-separated tag list, each made one
/// ([`crate::metadata::clean_tag`]), dropping empties and duplicates
/// (tags ignore case: the first spelling stays).
pub fn parse_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split(',') {
        let t = crate::metadata::clean_tag(t);
        let t = t.as_str();
        if !t.is_empty() && !out.iter().any(|x| same_tag(x, t)) {
            out.push(t.to_owned());
        }
    }
    out
}

/// A rating in stars, in words: "4 stars", "3½ stars".
pub fn stars(rating: u64) -> String {
    let whole = rating / 2;
    let half = if rating % 2 == 1 { "½" } else { "" };
    let plural = if rating <= 2 { "" } else { "s" };
    format!("{whole}{half} star{plural}").replacen("0½", "½", 1)
}

/// Where a store keeps notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kept {
    /// In the file's extended attributes.
    Attributes,
    /// In a file beside it (ADR-028).
    Beside,
    /// In its name.
    Name,
    /// In its content: read on demand, never for a listing.
    Content,
}

/// What a store can hold, and what it cannot, in words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one switch per field of the model"
)]
pub struct Holds {
    pub tags: bool,
    pub tag_colors: bool,
    pub rating: bool,
    pub comment: bool,
    pub origin: bool,
    /// What it cannot hold that the others can ("no comma in a tag").
    pub limits: &'static str,
}

/// How a store is written.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Write {
    /// Never.
    #[default]
    Off,
    /// Where the file's notes already are in it.
    Update,
    /// Updated where present, and started where the file's own
    /// attributes cannot hold its notes (a volume without them): the one
    /// store created there (ADR-027).
    Fallback,
    /// Updated where present, started everywhere else.
    Create,
}

impl Write {
    /// The words the settings show.
    pub fn title(self) -> &'static str {
        match self {
            Write::Off => "Off",
            Write::Update => "Update existing",
            Write::Fallback => "Where attributes are missing",
            Write::Create => "Create",
        }
    }
}

/// What a store did with an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrote {
    /// It changed what it keeps for the file.
    Written,
    /// It keeps the file's notes and they were already so.
    Unchanged,
    /// It keeps nothing for this file and was not asked to start (or the
    /// volume refuses it).
    NotHere,
}

/// A file of a batch, as a store is handed it: its path and name, and the
/// names of its extended attributes, listed once for every store.
pub struct Probe<'a> {
    pub path: &'a Path,
    pub name: &'a OsStr,
    pub attributes: &'a [String],
    /// A folder: a store may keep its notes inside it (`TagSpaces`'
    /// `.ts/tsm.json`).
    pub folder: bool,
}

/// One place notes are kept.
pub trait NotesStore: Send + Sync {
    fn id(&self) -> &'static str;
    /// Its name in the settings and the review ("The desktop's attributes").
    fn title(&self) -> &'static str;
    fn kept(&self) -> Kept;
    fn holds(&self) -> Holds;
    /// How it is written unless the user chose otherwise. A store in names
    /// is written only by a rename of its own: `Update` puts a step
    /// renaming the file beside an edit ([`Self::renamed`]); it is never
    /// started.
    fn default_write(&self) -> Write;
    /// Whether it is read unless the user chose otherwise.
    fn default_read(&self) -> bool {
        true
    }
    /// Whether it keeps anything for files in `dir`: a volume check, asked
    /// once per folder.
    fn applies(&self, _dir: &Path) -> bool {
        true
    }
    /// Whether the files of `dir` already use it (its `._` or sidecar
    /// files are there): the store a volume without attributes creates in
    /// is the one its folder already uses (ADR-027).
    fn in_use(&self, _dir: &Path) -> bool {
        false
    }
    /// What it can hold of `notes`: what it reads back after writing them.
    /// Comparing a store with this, not with the notes, tells a store
    /// that disagrees from one that cannot say as much.
    fn fit(&self, notes: &Notes) -> Notes {
        let holds = self.holds();
        Notes {
            tags: if holds.tags {
                notes
                    .tags
                    .iter()
                    .map(|t| Tag {
                        name: t.name.clone(),
                        color: t.color.clone().filter(|_| holds.tag_colors),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            rating: notes.rating.filter(|_| holds.rating),
            comment: notes.comment.clone().filter(|_| holds.comment),
            origin: None,
            referrer: None,
        }
    }
    /// The notes it keeps for each of `files`, in their order; `None` for
    /// a file it keeps nothing for. Blocking: called on a worker, with a
    /// batch of one folder's files.
    /// With `listed`, the folder's names as its listing knows them: a store
    /// looking for files beside a file asks it rather than read the folder.
    fn read(
        &self,
        dir: &Path,
        files: &[Probe<'_>],
        listed: Option<&Listed<'_>>,
    ) -> Vec<Option<Notes>>;
    /// Where it keeps (or would start keeping, with `create`) `file`'s
    /// notes: what an edit may change, snapshot for its undo.
    fn keys(&self, file: &Probe<'_>, create: bool) -> Vec<RawKey>;
    /// Keep `notes` as `file`'s, keeping what it holds that the model
    /// does not; start keeping them if there is nothing yet only with
    /// `create`.
    ///
    /// # Errors
    /// When it keeps the file's notes and cannot write them.
    fn write(&self, file: &Probe<'_>, notes: &Notes, create: bool) -> std::io::Result<Wrote>;
    /// A store in names: the name `name` takes to hold what it can of
    /// `notes`, where that changes it. `None` where the name holds the
    /// same tags already, where it holds none (a name is never started),
    /// and for every other store. From the name alone: nothing is read.
    fn renamed(&self, name: &OsStr, notes: &Notes) -> Option<std::ffi::OsString> {
        let _ = (name, notes);
        None
    }
}

/// How each store is read and written, captured when a plan is made so
/// applying or redoing it never reads a setting that changed since.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Policy {
    /// The stores read, by id, in order.
    pub read: Vec<String>,
    /// How each store is written, by id.
    pub write: Vec<(String, Write)>,
}

impl Policy {
    pub fn reads(&self, id: &str) -> bool {
        self.read.iter().any(|r| r == id)
    }

    pub fn write_of(&self, id: &str) -> Write {
        self.write
            .iter()
            .find(|(s, _)| s == id)
            .map_or(Write::Off, |(_, w)| *w)
    }
}

/// The attribute names of `path`, through a link: listed once per file
/// for every store that keeps notes in attributes.
pub fn attribute_names(path: &Path) -> Vec<String> {
    crate::fs::xattr::list_deref(path)
        .map(|names| {
            names
                .into_iter()
                .filter_map(|n| n.into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether two stores' notes say the same: the same tags (case aside, in
/// any order), rating and comment. Colors and origins are not the user's
/// edits.
pub fn same(a: &Notes, b: &Notes) -> bool {
    a.tags.len() == b.tags.len()
        && a.tags.iter().all(|t| b.tag(&t.name).is_some())
        && a.rating == b.rating
        && a.comment == b.comment
}

/// What skwir wrote for a file last (kept in the cache by its identity):
/// the notes, and what each store read back right after.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub written: Notes,
    pub each: Vec<(String, Notes)>,
}

/// The merge of each store's notes against what skwir last wrote
/// (ADR-027): a store that changed since was edited by another
/// application, and its edit wins, added and removed tags alike, a rating
/// or a comment changed; a store that holds nothing now (a `._` file
/// deleted, attributes a copy dropped) is not an edit. The first store's
/// edit wins over a later one's.
pub fn merged_since(snapshot: &Snapshot, each: &[(&'static str, Notes)]) -> Notes {
    let mut out = snapshot.written.clone();
    out.origin = None;
    out.referrer = None;
    for (id, now) in each.iter().rev() {
        let before = snapshot
            .each
            .iter()
            .find(|(s, _)| s == id)
            .map(|(_, n)| n.clone())
            .unwrap_or_default();
        if same(now, &before) {
            continue;
        }
        for tag in &before.tags {
            if now.tag(&tag.name).is_none() {
                out.tags.retain(|t| !same_tag(&t.name, &tag.name));
            }
        }
        for tag in &now.tags {
            if before.tag(&tag.name).is_none() && out.tag(&tag.name).is_none() {
                out.tags.push(tag.clone());
            }
        }
        if now.rating != before.rating {
            out.rating = now.rating;
        }
        if now.comment != before.comment {
            out.comment.clone_from(&now.comment);
        }
    }
    // Colors and where a download came from: whatever a store says.
    for (_, now) in each {
        for tag in &mut out.tags {
            if tag.color.is_none() {
                tag.color = now.tag(&tag.name).and_then(|t| t.color.clone());
            }
        }
        out.origin = out.origin.take().or_else(|| now.origin.clone());
        out.referrer = out.referrer.take().or_else(|| now.referrer.clone());
    }
    out
}

/// A file's notes as read: merged, each store's, and the stores that do
/// not say what the merge does.
#[derive(Clone, Debug, Default)]
pub struct Read {
    pub notes: Notes,
    pub each: Vec<(&'static str, Notes)>,
    pub differ: Vec<&'static str>,
}

/// A file of a batch: its name, its identity when known (what skwir last
/// wrote for it is kept by it), and whether it is a folder.
pub type Named<'a> = (&'a OsStr, Option<EntryId>, bool);

/// The registered stores, in the order they are read: the platform's own
/// first, as it decides what the others do not say.
#[derive(Default)]
pub struct NoteStores {
    list: RwLock<Vec<(Arc<str>, Arc<dyn NotesStore>)>>,
    /// What the stores hold together, kept as they come and go.
    held: RwLock<Holds>,
    /// The user's choices, `None` until the settings are read: then each
    /// store's default.
    chosen: RwLock<Option<Policy>>,
    /// What skwir wrote last for each file it edited.
    snapshots: RwLock<Option<CacheSlot>>,
    /// The settings declared for each owner's stores, withdrawn with them.
    declared: RwLock<Vec<(Arc<str>, String)>>,
}

/// The settings page of the stores: `notes.<store>.read`,
/// `notes.<store>.write`.
pub const SETTINGS: &str = "notes";

/// A store's read setting.
pub fn read_key(store: &str) -> String {
    format!("{SETTINGS}.{store}.read")
}

/// A store's write setting.
pub fn write_key(store: &str) -> String {
    format!("{SETTINGS}.{store}.write")
}

/// A snapshot is a name's (ADR-049): what is kept beside a file is the
/// name's, and what is kept in its attributes, the same under each name,
/// is merged as an edit made elsewhere when read under another, which is
/// what it is there. The name is the file under its name in the folder
/// holding it (`EntryId::in_folder`), whatever its reader knows it by (a
/// listing's entry, a tag's or the shelf's by its path): every view of a
/// file finds what skwir wrote. None is kept of a file whose number a
/// mount made up (`FileId::lasts`): its notes are merged as when none was
/// kept.
fn snapshot_key(file: FileId, folder: FileId, name: &OsStr) -> Option<Vec<u8>> {
    let id = EntryId::in_folder(file, folder, name);
    id.file
        .lasts()
        .then(|| format!("{}:{}:{:x}", id.file.dev, id.file.ino, id.at).into_bytes())
}

/// The folder at `dir` as its listing knows it, a link to it followed
/// (`Dir::identity`). Reads the disk: on a worker.
fn folder_of(dir: &Path) -> Option<FileId> {
    let meta = std::fs::metadata(dir).ok()?;
    crate::fs::stat::identity(dir, &meta)
}

/// The key of `path`'s snapshot, its file being `file`. Reads the disk.
fn snapshot_key_at(file: FileId, path: &Path) -> Option<Vec<u8>> {
    snapshot_key(file, folder_of(path.parent()?)?, path.file_name()?)
}

/// What the notes' snapshots may take in the cache.
const SNAPSHOT_BYTES: u64 = 32 << 20;

impl NoteStores {
    pub fn register(&self, owner: &str, store: Arc<dyn NotesStore>) {
        let mut list = self.list.write();
        list.push((owner.into(), store));
        *self.held.write() = Self::together(&list);
    }

    pub fn unregister_owner(&self, owner: &str) {
        let mut list = self.list.write();
        list.retain(|(o, _)| &**o != owner);
        *self.held.write() = Self::together(&list);
    }

    /// What the stores registered hold together: whether skwir keeps a
    /// rating or a comment anywhere, for the surfaces that show one. From
    /// memory, cheap enough to ask as they are drawn.
    pub fn held(&self) -> Holds {
        *self.held.read()
    }

    fn together(list: &[(Arc<str>, Arc<dyn NotesStore>)]) -> Holds {
        list.iter().fold(Holds::default(), |all, (_, store)| {
            let holds = store.holds();
            Holds {
                tags: all.tags || holds.tags,
                tag_colors: all.tag_colors || holds.tag_colors,
                rating: all.rating || holds.rating,
                comment: all.comment || holds.comment,
                origin: all.origin || holds.origin,
                limits: "",
            }
        })
    }

    /// Record that `key` was declared for `owner`'s store.
    pub(crate) fn declared(&self, owner: &str, key: String) {
        self.declared.write().push((owner.into(), key));
    }

    /// The settings declared for `owner`'s stores, forgotten.
    pub(crate) fn take_declared(&self, owner: &str) -> Vec<String> {
        let mut declared = self.declared.write();
        let (theirs, rest): (Vec<_>, Vec<_>) = declared.drain(..).partition(|(o, _)| &**o == owner);
        *declared = rest;
        theirs.into_iter().map(|(_, k)| k).collect()
    }

    pub fn stores(&self) -> Vec<Arc<dyn NotesStore>> {
        self.list
            .read()
            .iter()
            .map(|(_, s)| Arc::clone(s))
            .collect()
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.list.read().iter().map(|(_, s)| s.id()).collect()
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn NotesStore>> {
        self.list
            .read()
            .iter()
            .find(|(_, s)| s.id() == id)
            .map(|(_, s)| Arc::clone(s))
    }

    /// Where the snapshots of what skwir wrote are kept.
    pub fn keep_snapshots(&self, slot: CacheSlot) {
        *self.snapshots.write() = Some(slot);
    }

    fn snapshot(&self, key: &[u8]) -> Option<Snapshot> {
        let bytes = self.snapshots.read().as_ref()?.get(key)?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Remember what skwir just wrote for the file `file` at `path`.
    /// Kept within `SNAPSHOT_BYTES`, the oldest let go of first: a file
    /// was kept for every file ever edited. One let go of says nothing of
    /// which store skwir wrote last: the stores' notes are merged as when
    /// none was kept.
    pub(crate) fn remember(&self, file: FileId, path: &Path, snapshot: &Snapshot) {
        let slot = self.snapshots.read().clone();
        if let (Some(slot), Some(key), Ok(bytes)) = (
            slot,
            snapshot_key_at(file, path),
            serde_json::to_vec(snapshot),
        ) {
            match slot.put(&key, &bytes) {
                Ok(()) => slot.trim_soon(bytes.len() as u64, SNAPSHOT_BYTES),
                Err(error) => tracing::debug!(%error, "notes: the snapshot was not kept"),
            }
        }
    }

    /// Forget what skwir wrote for the file `file` at `path` (an undo put
    /// back what was there before).
    pub(crate) fn forget(&self, file: FileId, path: &Path) {
        if let (Some(slot), Some(key)) =
            (self.snapshots.read().as_ref(), snapshot_key_at(file, path))
        {
            let _ = slot.remove(&key);
        }
    }

    /// Replace the user's choices (the settings, `notes.*`).
    pub fn choose(&self, policy: Option<Policy>) {
        *self.chosen.write() = policy;
    }

    /// How the stores are read and written now: the user's choices, and
    /// each store's default for what they did not choose.
    pub fn policy(&self) -> Policy {
        let chosen = self.chosen.read().clone();
        let stores = self.stores();
        let mut policy = Policy::default();
        for s in &stores {
            let id = s.id();
            let read = chosen.as_ref().map_or(s.default_read(), |c| c.reads(id));
            if read {
                policy.read.push(id.to_owned());
            }
            let write = chosen
                .as_ref()
                .and_then(|c| c.write.iter().find(|(x, _)| x == id).map(|(_, w)| *w))
                .unwrap_or_else(|| s.default_write());
            policy.write.push((id.to_owned(), write));
        }
        policy
    }

    /// Whether any store read keeps notes in attributes: then a batch
    /// lists each file's attribute names.
    fn reads_attributes(&self, policy: &Policy) -> bool {
        self.stores()
            .iter()
            .any(|s| s.kept() == Kept::Attributes && policy.reads(s.id()))
    }

    /// The notes of a batch of `dir`'s files, as every store `policy`
    /// reads keeps them, merged. Blocking: call on a worker.
    pub fn read(
        &self,
        dir: &Path,
        files: &[Named<'_>],
        policy: &Policy,
        listed: Option<&Listed<'_>>,
    ) -> Vec<Read> {
        let paths: Vec<std::path::PathBuf> = files.iter().map(|(n, _, _)| dir.join(n)).collect();
        let attributes: Vec<Vec<String>> = if self.reads_attributes(policy) {
            paths.iter().map(|p| attribute_names(p)).collect()
        } else {
            vec![Vec::new(); paths.len()]
        };
        let probes: Vec<Probe<'_>> = paths
            .iter()
            .zip(files)
            .zip(&attributes)
            .map(|((path, (name, _, folder)), attributes)| Probe {
                path,
                name,
                attributes,
                folder: *folder,
            })
            .collect();
        let stores: Vec<Arc<dyn NotesStore>> = self
            .stores()
            .into_iter()
            .filter(|s| policy.reads(s.id()) && s.applies(dir))
            .collect();
        let mut each: Vec<Vec<(&'static str, Notes)>> = vec![Vec::new(); probes.len()];
        for store in &stores {
            for (i, found) in store.read(dir, &probes, listed).into_iter().enumerate() {
                if let (Some(mut found), Some(list)) = (found, each.get_mut(i)) {
                    found.clean_tags();
                    list.push((store.id(), found));
                }
            }
        }
        // Snapshots are a name's in its folder, however `files` name
        // theirs: asked only of a file whose stores disagree.
        let folder = std::cell::OnceCell::new();
        each.into_iter()
            .zip(files)
            .map(|(each, (name, id, _))| {
                let key = || {
                    snapshot_key(
                        id.as_ref()?.file,
                        (*folder.get_or_init(|| folder_of(dir)))?,
                        name,
                    )
                };
                self.merge(each, key)
            })
            .collect()
    }

    /// Merge what each store says of one file: all together when they agree
    /// (each saying what it can of the union), else against what skwir
    /// wrote last, when it remembers.
    fn merge(
        &self,
        each: Vec<(&'static str, Notes)>,
        key: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Read {
        let mut union = Notes::default();
        for (_, notes) in &each {
            union.merge(notes.clone());
        }
        // A name is never said to differ: it changes only by a rename
        // (`SetNotes::unfolds`), never to make the stores agree.
        let differ_from = |notes: &Notes| -> Vec<&'static str> {
            each.iter()
                .filter(|(store, now)| {
                    self.get(store)
                        .is_some_and(|s| s.kept() != Kept::Name && !same(now, &s.fit(notes)))
                })
                .map(|(store, _)| *store)
                .collect()
        };
        let differ = differ_from(&union);
        if differ.is_empty() {
            return Read {
                notes: union,
                each,
                differ,
            };
        }
        let Some(snapshot) = key().and_then(|key| self.snapshot(&key)) else {
            return Read {
                notes: union,
                each,
                differ,
            };
        };
        let notes = merged_since(&snapshot, &each);
        let differ = differ_from(&notes);
        Read {
            notes,
            each,
            differ,
        }
    }

    /// The notes of one file, merged. Blocking: call on a worker.
    pub fn read_path(&self, path: &Path, policy: &Policy) -> Notes {
        self.read_one(path, policy).notes
    }

    /// One file's notes as read. Blocking: call on a worker.
    pub fn read_one(&self, path: &Path, policy: &Policy) -> Read {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Read::default();
        };
        let id = crate::op::EntryRef::from_path(path).and_then(|r| r.entry());
        self.read(dir, &[(name, id, path.is_dir())], policy, None)
            .pop()
            .unwrap_or_default()
    }

    /// An edit of `entry`'s notes, written as the stores are now set to be.
    pub fn edit(self: &Arc<Self>, entry: crate::op::EntryRef) -> SetNotes {
        SetNotes::new(Arc::clone(self), entry, self.policy())
    }

    /// The titles of stores, by id, for the user.
    pub fn titles(&self, ids: &[&str]) -> Vec<&'static str> {
        ids.iter()
            .filter_map(|id| self.get(id).map(|s| s.title()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_snapshot_is_kept_of_a_file_whose_number_a_mount_made_up() {
        let (file, folder, name) = (FileId::new(5, 7), FileId::new(5, 2), OsStr::new("it's.txt"));
        let at = EntryId::in_folder(file, folder, name).at;
        assert_eq!(
            snapshot_key(file, folder, name).unwrap(),
            format!("5:7:{at:x}").into_bytes(),
            "as before"
        );
        crate::fs::reach::pretend_made_up(6, 1 << 40);
        assert_eq!(snapshot_key(FileId::new(6, 7), folder, name), None);
    }

    #[test]
    fn notes_merge_keeping_the_first_store_and_adding_what_it_lacks() {
        let mut notes = Notes {
            tags: vec![Tag::named("Work"), Tag::named("home")],
            rating: Some(6),
            ..Notes::default()
        };
        notes.merge(Notes {
            tags: vec![
                Tag {
                    name: "WORK".into(),
                    color: Some("red".into()),
                },
                Tag::named("mac"),
            ],
            rating: Some(10),
            comment: Some("from the Mac".into()),
            ..Notes::default()
        });
        assert_eq!(notes.tag_names(), ["Work", "home", "mac"]);
        assert_eq!(notes.tag("work").unwrap().color.as_deref(), Some("red"));
        assert_eq!(notes.rating, Some(6), "the first store's");
        assert_eq!(notes.comment.as_deref(), Some("from the Mac"));
    }

    fn notes(tags: &[&str], rating: Option<u64>) -> Notes {
        Notes {
            tags: tags.iter().map(|t| Tag::named(*t)).collect(),
            rating,
            ..Notes::default()
        }
    }

    #[test]
    fn another_applications_edit_wins_over_what_skwir_wrote() {
        let snapshot = Snapshot {
            written: notes(&["a", "b", "x"], Some(6)),
            each: vec![
                ("xdg".into(), notes(&["a", "b", "x"], Some(6))),
                ("finder".into(), notes(&["a", "b", "x"], None)),
                ("aside".into(), notes(&["a", "b", "x"], Some(6))),
            ],
        };
        // The Mac took `b` and `x` away and added `mac`; the `._` file was
        // deleted; the desktop's attributes are as skwir left them.
        let each = [
            ("xdg", notes(&["a", "b", "x"], Some(6))),
            ("finder", notes(&["a", "mac"], None)),
        ];
        let merged = merged_since(&snapshot, &each);
        assert_eq!(merged.tag_names(), ["a", "mac"]);
        assert_eq!(
            merged.rating,
            Some(6),
            "the Finder holds no rating to change"
        );
        // Two stores changed the rating: the first one's wins.
        let each = [
            ("xdg", notes(&["a", "b", "x"], Some(10))),
            ("aside", notes(&["a", "b", "x"], Some(2))),
        ];
        assert_eq!(merged_since(&snapshot, &each).rating, Some(10));
        // A store skwir never wrote says what it has as new.
        let each = [("tagspaces", notes(&["ts"], None))];
        assert_eq!(
            merged_since(&snapshot, &each).tag_names(),
            ["a", "b", "x", "ts"]
        );
    }

    #[test]
    fn tags_and_ratings_read_as_the_desktop_writes_them() {
        assert_eq!(parse_tags(" a, b ,a,,c "), ["a", "b", "c"]);
        assert_eq!(parse_tags("Work, work,WORK, Été, été"), ["Work", "Été"]);
        assert_eq!(stars(10), "5 stars");
        assert_eq!(stars(2), "1 star");
        assert_eq!(stars(7), "3½ stars");
        assert_eq!(stars(1), "½ star");
    }
}
