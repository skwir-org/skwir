//! A file's notes (ADR-027): its tags, a rating, a comment, where a
//! download came from, as one model every store and every surface agree
//! on, read from every store bibli knows and written to those the user
//! chooses.
//!
//! A [`NotesStore`] is one place notes are kept (the desktop's extended
//! attributes, a Mac's Finder attributes, the `._` file beside a file on a
//! volume without attributes, an XMP sidecar…), registered by a plugin.
//! The registry ([`NoteStores`]) reads a batch of a folder's files from
//! every store and merges what they say; an edit ([`SetNotes`]) writes
//! every store its policy allows, and its inverse puts back the exact
//! bytes each store held ([`raw`]), whatever the format.

mod edit;
pub mod raw;

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::metadata::same_tag;
pub use edit::{RestoreNotes, SetNotes};
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

/// Split a comma-separated tag list, trimming and dropping empties and
/// duplicates (tags ignore case: the first spelling stays).
pub fn parse_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split(',') {
        let t = t.trim();
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
}

/// One place notes are kept.
pub trait NotesStore: Send + Sync {
    fn id(&self) -> &'static str;
    /// Its name in the settings and the review ("The desktop's attributes").
    fn title(&self) -> &'static str;
    fn kept(&self) -> Kept;
    fn holds(&self) -> Holds;
    /// How it is written unless the user chose otherwise.
    fn default_write(&self) -> Write;
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
    /// The notes it keeps for each of `files`, in their order; `None` for
    /// a file it keeps nothing for. Blocking: called on a worker, with a
    /// batch of one folder's files.
    fn read(&self, dir: &Path, files: &[Probe<'_>]) -> Vec<Option<Notes>>;
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
    xattr::list_deref(path)
        .map(|names| names.filter_map(|n| n.into_string().ok()).collect())
        .unwrap_or_default()
}

/// The registered stores, in the order they are read: the platform's own
/// first, as it decides what the others do not say.
#[derive(Default)]
pub struct NoteStores {
    list: RwLock<Vec<(Arc<str>, Arc<dyn NotesStore>)>>,
    /// The user's choices, `None` until the settings are read: then each
    /// store's default.
    chosen: RwLock<Option<Policy>>,
}

impl NoteStores {
    pub fn register(&self, owner: &str, store: Arc<dyn NotesStore>) {
        self.list.write().push((owner.into(), store));
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.list.write().retain(|(o, _)| &**o != owner);
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
            let read = chosen.as_ref().is_none_or(|c| c.reads(id));
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

    /// The notes of a batch of `dir`'s files, each merged from every store
    /// `policy` reads, and each store's own. Blocking: call on a worker.
    pub fn read(
        &self,
        dir: &Path,
        names: &[&OsStr],
        policy: &Policy,
    ) -> Vec<(Notes, Vec<(&'static str, Notes)>)> {
        let paths: Vec<std::path::PathBuf> = names.iter().map(|n| dir.join(n)).collect();
        let attributes: Vec<Vec<String>> = if self.reads_attributes(policy) {
            paths.iter().map(|p| attribute_names(p)).collect()
        } else {
            vec![Vec::new(); paths.len()]
        };
        let probes: Vec<Probe<'_>> = paths
            .iter()
            .zip(names)
            .zip(&attributes)
            .map(|((path, name), attributes)| Probe {
                path,
                name,
                attributes,
            })
            .collect();
        let mut out: Vec<(Notes, Vec<(&'static str, Notes)>)> =
            (0..probes.len()).map(|_| Default::default()).collect();
        for store in self.stores() {
            if !policy.reads(store.id()) || !store.applies(dir) {
                continue;
            }
            for (i, found) in store.read(dir, &probes).into_iter().enumerate() {
                if let (Some(found), Some((merged, each))) = (found, out.get_mut(i)) {
                    merged.merge(found.clone());
                    each.push((store.id(), found));
                }
            }
        }
        out
    }

    /// The notes of one file, merged. Blocking: call on a worker.
    pub fn read_path(&self, path: &Path, policy: &Policy) -> Notes {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Notes::default();
        };
        self.read(dir, &[name], policy)
            .pop()
            .map(|(notes, _)| notes)
            .unwrap_or_default()
    }

    /// An edit of `entry`'s notes, written as the stores are now set to be.
    pub fn edit(self: &Arc<Self>, entry: crate::op::EntryRef) -> SetNotes {
        SetNotes::new(Arc::clone(self), entry, self.policy())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
