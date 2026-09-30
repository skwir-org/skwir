//! Editing a file's notes: one step writes every store its policy allows,
//! the file's own attributes first, and records the bytes each held; its
//! inverse puts them back.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::raw::{self, Raw, RawKey};
use super::{NoteStores, Notes, Policy, Probe, Tag, Write, Wrote, stars};
use crate::entry::keys;
use crate::metadata::same_tag;
use crate::op::{Change, EntryRef, Operation, ResolveError, StepContext, StepStatus, Words};

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn same_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.iter().any(|y| same_tag(x, y)))
}

/// What an edit does to one field: leaves it, or sets it (`None`
/// removes it).
enum Field<T> {
    Left,
    Set(Option<T>),
}

impl<T: PartialEq> Field<T> {
    /// Whether `now` is already what this makes it.
    fn holds(&self, now: Option<&T>) -> bool {
        match self {
            Field::Left => true,
            Field::Set(to) => to.as_ref() == now,
        }
    }
}

/// Set some of a file's notes: its tags (the whole list), its rating (0
/// removes it), its comment (empty removes it); what is left out stays.
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetNotes {
    pub entry: EntryRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// From 1 to 10, half stars; 0 removes the rating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u64>,
    /// Empty removes the comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Write every store even where the notes already say this, so the
    /// stores that differ agree again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sync: bool,
    /// How the stores were set to be read and written when the plan was
    /// made.
    #[serde(default)]
    pub policy: Policy,
    #[serde(skip)]
    #[schemars(skip)]
    stores: Option<Arc<NoteStores>>,
}

impl std::fmt::Debug for SetNotes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetNotes")
            .field("entry", &self.entry)
            .field("tags", &self.tags)
            .field("rating", &self.rating)
            .field("comment", &self.comment)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SetNotes {
    fn eq(&self, other: &Self) -> bool {
        self.entry == other.entry
            && self.tags == other.tags
            && self.rating == other.rating
            && self.comment == other.comment
            && self.sync == other.sync
            && self.policy == other.policy
    }
}

impl SetNotes {
    pub fn new(stores: Arc<NoteStores>, entry: EntryRef, policy: Policy) -> Self {
        SetNotes {
            entry,
            tags: None,
            rating: None,
            comment: None,
            sync: false,
            policy,
            stores: Some(stores),
        }
    }

    /// Write every store, so those that differ agree.
    #[must_use]
    pub fn sync(mut self) -> Self {
        self.sync = true;
        self
    }

    /// The stores it writes with, given once it was read from data.
    #[must_use]
    pub fn with_stores(mut self, stores: Arc<NoteStores>) -> Self {
        self.stores = Some(stores);
        self
    }

    #[must_use]
    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    #[must_use]
    pub fn rating(mut self, rating: Option<u64>) -> Self {
        self.rating = Some(rating.unwrap_or(0));
        self
    }

    #[must_use]
    pub fn comment(mut self, comment: Option<String>) -> Self {
        self.comment = Some(comment.unwrap_or_default());
        self
    }

    fn stores(&self) -> anyhow::Result<&Arc<NoteStores>> {
        self.stores
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no notes stores to write with"))
    }

    fn rating_to(&self) -> Field<u64> {
        match self.rating {
            None => Field::Left,
            Some(r) => Field::Set((r > 0).then_some(r)),
        }
    }

    fn comment_to(&self) -> Field<String> {
        match &self.comment {
            None => Field::Left,
            Some(c) => {
                let c = c.trim();
                Field::Set((!c.is_empty()).then(|| c.to_owned()))
            }
        }
    }

    /// Whether `notes` already say what this sets.
    fn holds_in(&self, notes: &Notes) -> bool {
        self.tags
            .as_ref()
            .is_none_or(|t| same_set(&notes.tag_names(), t))
            && self.rating_to().holds(notes.rating.as_ref())
            && self.comment_to().holds(notes.comment.as_ref())
    }

    /// `notes` with this edit made: a tag kept keeps its color.
    fn applied_to(&self, notes: &Notes) -> Notes {
        let mut out = notes.clone();
        if let Some(tags) = &self.tags {
            out.tags = tags
                .iter()
                .map(|name| Tag {
                    name: name.clone(),
                    color: notes.tag(name).and_then(|t| t.color.clone()),
                })
                .collect();
        }
        if let Field::Set(rating) = self.rating_to() {
            out.rating = rating;
        }
        if let Field::Set(comment) = self.comment_to() {
            out.comment = comment;
        }
        out
    }

    fn words(&self) -> (String, String, &'static str) {
        let name = name_of(&self.entry.path);
        if let Some(tags) = &self.tags {
            return if tags.is_empty() {
                (
                    format!("remove the tags of “{name}”"),
                    format!("removed the tags of “{name}”"),
                    keys::TAGS,
                )
            } else {
                let tags = tags.join(", ");
                (
                    format!("tag “{name}” {tags}"),
                    format!("tagged “{name}” {tags}"),
                    keys::TAGS,
                )
            };
        }
        if let Field::Set(rating) = self.rating_to() {
            return match rating {
                Some(r) => (
                    format!("rate “{name}” {}", stars(r)),
                    format!("rated “{name}” {}", stars(r)),
                    keys::RATING,
                ),
                None => (
                    format!("remove the rating of “{name}”"),
                    format!("removed the rating of “{name}”"),
                    keys::RATING,
                ),
            };
        }
        match self.comment_to() {
            Field::Set(Some(_)) | Field::Left => (
                format!("set the comment of “{name}”"),
                format!("set the comment of “{name}”"),
                keys::COMMENT,
            ),
            Field::Set(None) => (
                format!("remove the comment of “{name}”"),
                format!("removed the comment of “{name}”"),
                keys::COMMENT,
            ),
        }
    }
}

impl Operation for SetNotes {
    fn kind(&self) -> &'static str {
        "set_notes"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        let (will, did, field) = self.words();
        Change::Metadata {
            path: self.entry.path.clone(),
            field: field.into(),
            words: Some(Words { will, did }),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        if self.rating.is_some_and(|r| r > super::RATING_MAX) {
            return StepStatus::Conflict {
                reason: format!("a rating is from 1 to {}", super::RATING_MAX),
            };
        }
        let Ok(stores) = self.stores() else {
            return StepStatus::Conflict {
                reason: "no notes stores to write with".into(),
            };
        };
        match self.entry.resolve() {
            Ok(p) => {
                let read = stores.read_one(&p, &self.policy);
                // Asked to make the stores agree, it is done when they do.
                let agreed = !self.sync || read.differ.is_empty();
                if self.holds_in(&read.notes) && agreed {
                    StepStatus::AlreadyDone
                } else {
                    StepStatus::Pending { reversible: true }
                }
            }
            Err(ResolveError::Gone(p)) => StepStatus::Conflict {
                reason: format!("{} no longer exists", p.display()),
            },
            Err(e) => StepStatus::Conflict {
                reason: e.to_string(),
            },
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        anyhow::ensure!(
            self.rating.is_none_or(|r| r <= super::RATING_MAX),
            "a rating is from 1 to {}",
            super::RATING_MAX
        );
        let stores = Arc::clone(self.stores()?);
        let path = self.entry.resolve()?;
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            anyhow::bail!("{} has no folder", path.display());
        };
        let current = stores.read_path(&path, &self.policy);
        let target = self.applied_to(&current);
        let attributes = super::attribute_names(&path);
        let probe = Probe {
            path: &path,
            name,
            attributes: &attributes,
        };
        // The stores written: the file's own first, then those that start
        // keeping notes only where those could not (a volume without
        // attributes), the one its folder already uses first.
        let mut first = Vec::new();
        let mut fallback = Vec::new();
        for store in stores.stores() {
            let mode = self.policy.write_of(store.id());
            if mode == Write::Off || !store.applies(dir) {
                continue;
            }
            if mode == Write::Fallback {
                fallback.push(store);
            } else {
                first.push((store, mode == Write::Create));
            }
        }
        fallback.sort_by_key(|s| !s.in_use(dir));
        let mut keys: Vec<RawKey> = Vec::new();
        for (store, create) in &first {
            keys.extend(store.keys(&probe, *create));
        }
        for (i, store) in fallback.iter().enumerate() {
            keys.extend(store.keys(&probe, i == 0));
        }
        let before = raw::snapshot(&path, &keys)?;
        let failed = |error: std::io::Error, what: &str| {
            // Nothing is left half written.
            let _ = raw::restore(&path, &before);
            anyhow::anyhow!("{what} {}: {error}", path.display())
        };
        let mut held = false;
        for (store, create) in &first {
            match store.write(&probe, &target, *create) {
                Ok(Wrote::Written | Wrote::Unchanged) => held = true,
                Ok(Wrote::NotHere) => {}
                Err(e) => return Err(failed(e, "write the notes of")),
            }
        }
        for (i, store) in fallback.iter().enumerate() {
            let create = !held && i == 0;
            match store.write(&probe, &target, create) {
                Ok(Wrote::Written | Wrote::Unchanged) => held = true,
                Ok(Wrote::NotHere) => {}
                Err(e) => return Err(failed(e, "write the notes of")),
            }
        }
        if !held {
            let _ = raw::restore(&path, &before);
            anyhow::bail!(
                "{}: its volume keeps no notes, and no store is set to keep them beside it",
                path.display()
            );
        }
        let after = raw::snapshot(&path, &keys)?;
        // What each store says now, as bibli left it: the next read tells
        // another application's edit from this one by it.
        let attributes = super::attribute_names(&path);
        let probe = Probe {
            path: &path,
            name,
            attributes: &attributes,
        };
        let each = stores
            .stores()
            .into_iter()
            .filter(|s| self.policy.reads(s.id()) && s.applies(dir))
            .filter_map(|s| {
                let found = s.read(dir, std::slice::from_ref(&probe)).pop()??;
                Some((s.id().to_owned(), found))
            })
            .collect();
        stores.remember(
            self.entry.id,
            &super::Snapshot {
                written: target,
                each,
            },
        );
        let (_, _, field) = self.words();
        Ok(Some(Box::new(RestoreNotes {
            entry: EntryRef::new(self.entry.id, path),
            field: field.to_owned(),
            before,
            after,
            stores: Some(stores),
        })))
    }
}

/// Put back what a file's stores kept before an edit, byte for byte: the
/// inverse of [`SetNotes`], and of itself.
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RestoreNotes {
    pub entry: EntryRef,
    /// The field the edit changed, for whoever follows changes.
    pub field: String,
    /// What to put back.
    pub before: Vec<Raw>,
    /// What the edit left: if the stores hold something else now, another
    /// application changed them, and that is not overwritten.
    pub after: Vec<Raw>,
    /// Whose snapshot of what bibli wrote is forgotten once the bytes are
    /// back.
    #[serde(skip)]
    #[schemars(skip)]
    stores: Option<Arc<NoteStores>>,
}

impl std::fmt::Debug for RestoreNotes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreNotes")
            .field("entry", &self.entry)
            .field("field", &self.field)
            .finish_non_exhaustive()
    }
}

impl PartialEq for RestoreNotes {
    fn eq(&self, other: &Self) -> bool {
        self.entry == other.entry
            && self.field == other.field
            && self.before == other.before
            && self.after == other.after
    }
}

impl RestoreNotes {
    /// The registry it forgets snapshots in, given once read from data.
    #[must_use]
    pub fn with_stores(mut self, stores: Arc<NoteStores>) -> Self {
        self.stores = Some(stores);
        self
    }

    fn keys(&self) -> Vec<RawKey> {
        self.before.iter().map(|r| r.key.clone()).collect()
    }
}

impl Operation for RestoreNotes {
    fn kind(&self) -> &'static str {
        "restore_notes"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        let name = name_of(&self.entry.path);
        Change::Metadata {
            path: self.entry.path.clone(),
            field: self.field.clone(),
            words: Some(Words {
                will: format!("put back the notes of “{name}”"),
                did: format!("put back the notes of “{name}”"),
            }),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        let path = match self.entry.resolve() {
            Ok(p) => p,
            Err(ResolveError::Gone(p)) => {
                return StepStatus::Conflict {
                    reason: format!("{} no longer exists", p.display()),
                };
            }
            Err(e) => {
                return StepStatus::Conflict {
                    reason: e.to_string(),
                };
            }
        };
        match raw::snapshot(&path, &self.keys()) {
            Ok(now) if now == self.before => StepStatus::AlreadyDone,
            Ok(now) if now == self.after => StepStatus::Pending { reversible: true },
            Ok(_) => StepStatus::Conflict {
                reason: format!(
                    "the notes of {} changed since, elsewhere",
                    name_of(&self.entry.path)
                ),
            },
            Err(e) => StepStatus::Conflict {
                reason: e.to_string(),
            },
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let path = self.entry.resolve()?;
        let now = raw::snapshot(&path, &self.keys())?;
        anyhow::ensure!(
            now == self.after || now == self.before,
            "the notes of {} changed since, elsewhere",
            path.display()
        );
        raw::restore(&path, &self.before)
            .map_err(|e| anyhow::anyhow!("put back the notes of {}: {e}", path.display()))?;
        if let Some(stores) = &self.stores {
            stores.forget(self.entry.id);
        }
        Ok(Some(Box::new(RestoreNotes {
            entry: EntryRef::new(self.entry.id, path),
            field: self.field.clone(),
            before: now,
            after: self.before.clone(),
            stores: self.stores.clone(),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{Holds, Kept, NotesStore};

    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("notes-edit-")
            .tempdir_in(base)
            .unwrap()
    }

    /// A file's own attributes on a volume that refuses them.
    struct Refused;

    impl NotesStore for Refused {
        fn id(&self) -> &'static str {
            "refused"
        }
        fn title(&self) -> &'static str {
            "Refused"
        }
        fn kept(&self) -> Kept {
            Kept::Attributes
        }
        fn holds(&self) -> Holds {
            Holds::default()
        }
        fn default_write(&self) -> Write {
            Write::Create
        }
        fn read(&self, _: &Path, files: &[Probe<'_>]) -> Vec<Option<Notes>> {
            files.iter().map(|_| None).collect()
        }
        fn keys(&self, _: &Probe<'_>, _: bool) -> Vec<RawKey> {
            Vec::new()
        }
        fn write(&self, _: &Probe<'_>, _: &Notes, _: bool) -> std::io::Result<Wrote> {
            Ok(Wrote::NotHere)
        }
    }

    /// Notes as JSON in `<name>.notes` beside the file.
    struct Beside(&'static str, Write);

    impl Beside {
        fn at(&self, path: &Path) -> std::path::PathBuf {
            let mut name = path.file_name().unwrap().to_os_string();
            name.push(format!(".{}", self.0));
            path.with_file_name(name)
        }
    }

    impl NotesStore for Beside {
        fn id(&self) -> &'static str {
            self.0
        }
        fn title(&self) -> &'static str {
            self.0
        }
        fn kept(&self) -> Kept {
            Kept::Beside
        }
        fn holds(&self) -> Holds {
            Holds {
                tags: true,
                rating: true,
                comment: true,
                ..Holds::default()
            }
        }
        fn default_write(&self) -> Write {
            self.1
        }
        fn in_use(&self, dir: &Path) -> bool {
            std::fs::read_dir(dir).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(&format!(".{}", self.0))
            })
        }
        fn read(&self, _: &Path, files: &[Probe<'_>]) -> Vec<Option<Notes>> {
            files
                .iter()
                .map(|f| serde_json::from_slice(&std::fs::read(self.at(f.path)).ok()?).ok())
                .collect()
        }
        fn keys(&self, f: &Probe<'_>, _: bool) -> Vec<RawKey> {
            vec![RawKey::Beside {
                path: self.at(f.path).file_name().unwrap().into(),
            }]
        }
        fn write(&self, f: &Probe<'_>, notes: &Notes, create: bool) -> std::io::Result<Wrote> {
            let at = self.at(f.path);
            if !at.exists() && !create {
                return Ok(Wrote::NotHere);
            }
            std::fs::write(at, serde_json::to_vec(notes).unwrap())?;
            Ok(Wrote::Written)
        }
    }

    fn stores(list: Vec<Arc<dyn NotesStore>>) -> Arc<NoteStores> {
        let stores = Arc::new(NoteStores::default());
        for s in list {
            stores.register("test", s);
        }
        stores
    }

    #[test]
    fn where_attributes_are_refused_the_folders_own_store_is_started_and_undone_exactly() {
        let tmp = home_tempdir();
        let file = tmp.path().join("it's a.txt");
        std::fs::write(&file, b"x").unwrap();
        let (a, b) = (Beside("a", Write::Fallback), Beside("b", Write::Fallback));
        let b_at = b.at(&file);
        // The folder already uses `b`: that one is started, not `a`.
        std::fs::write(tmp.path().join("other.txt.b"), b"{}").unwrap();
        let stores = stores(vec![Arc::new(Refused), Arc::new(a), Arc::new(b)]);
        let ctx = StepContext::detached();
        let entry = EntryRef::from_path(&file).unwrap();
        let edit = stores.edit(entry.clone()).tags(vec!["work".into()]);
        assert_eq!(edit.check(&ctx), StepStatus::Pending { reversible: true });
        let undo = edit.apply(&ctx).unwrap().unwrap();
        assert!(b_at.exists() && !tmp.path().join("it's a.txt.a").exists());
        assert_eq!(
            stores.read_path(&file, &stores.policy()).tag_names(),
            ["work"]
        );
        assert_eq!(edit.check(&ctx), StepStatus::AlreadyDone);

        // Undone: nothing was there, nothing is.
        let redo = undo.apply(&ctx).unwrap().unwrap();
        assert!(!b_at.exists());
        redo.apply(&ctx).unwrap();
        assert!(b_at.exists());

        // Changed elsewhere since: undo says so rather than overwrite it.
        let undo = stores
            .edit(entry.clone())
            .rating(Some(8))
            .apply(&ctx)
            .unwrap()
            .unwrap();
        std::fs::write(&b_at, br#"{"tags":[{"name":"theirs"}]}"#).unwrap();
        assert!(matches!(undo.check(&ctx), StepStatus::Conflict { .. }));
        assert!(undo.apply(&ctx).is_err());
        assert_eq!(
            std::fs::read(&b_at).unwrap(),
            br#"{"tags":[{"name":"theirs"}]}"#
        );
    }

    #[test]
    fn with_no_store_to_keep_them_an_edit_says_so_and_writes_nothing() {
        let tmp = home_tempdir();
        let file = tmp.path().join("a.txt");
        std::fs::write(&file, b"x").unwrap();
        let stores = stores(vec![
            Arc::new(Refused),
            Arc::new(Beside("a", Write::Update)),
        ]);
        let entry = EntryRef::from_path(&file).unwrap();
        let err = stores
            .edit(entry)
            .tags(vec!["x".into()])
            .apply(&StepContext::detached())
            .unwrap_err();
        assert!(err.to_string().contains("keeps no notes"), "{err}");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn an_edit_is_data_that_comes_back_with_its_stores() {
        let stores = stores(vec![Arc::new(Beside("a", Write::Create))]);
        let edit = stores
            .edit(EntryRef::new(
                crate::entry::EntryId { dev: 1, ino: 2 },
                "/x/a.txt",
            ))
            .tags(vec!["work".into()])
            .comment(None);
        let data = edit.args();
        assert_eq!(
            data["policy"]["write"][0],
            serde_json::json!(["a", "create"])
        );
        let back: SetNotes = serde_json::from_value(data).unwrap();
        assert_eq!(back.with_stores(stores), edit);
    }
}
