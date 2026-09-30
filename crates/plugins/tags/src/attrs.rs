//! The provider that fills a listing's notes fields: every store read,
//! for a batch of one folder's files, their notes merged (ADR-027). A
//! file's attribute names are listed once for every store that keeps
//! notes in attributes: most files have none, so a listing costs one call
//! per entry whatever the number of stores and keys.

use std::path::Path;
use std::sync::Arc;

use bibli_core::entry::{Entry, Metadata, Provenance, Value, keys};
use bibli_core::metadata::MetadataProvider;
use bibli_core::notes::{NoteStores, Notes};

use crate::vocabulary::Vocabulary;

pub fn tags_field(tags: &[String]) -> Value {
    Value::List(
        tags.iter()
            .map(|t| Value::Text(Arc::from(t.as_str())))
            .collect(),
    )
}

/// Tags recorded in an entry's metadata, if the provider filled them.
pub fn tags_of(metadata: &Metadata) -> Option<Vec<String>> {
    match metadata.value(keys::TAGS)? {
        Value::List(items) => Some(
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
        ),
        _ => None,
    }
}

/// Set an entry's notes fields from `notes`.
fn fill_entry(metadata: &mut Metadata, notes: &Notes) {
    let text = |t: &str| Value::Text(Arc::from(t));
    if !notes.tags.is_empty() {
        metadata.set(keys::TAGS, tags_field(&notes.tag_names()), Provenance::User);
        let colors: Vec<Value> = notes
            .tags
            .iter()
            .filter_map(|t| Some(Value::List(vec![text(&t.name), text(t.color.as_deref()?)])))
            .collect();
        if !colors.is_empty() {
            metadata.set(keys::TAG_COLORS, Value::List(colors), Provenance::User);
        }
    }
    if let Some(rating) = notes.rating {
        metadata.set(keys::RATING, Value::UInt(rating), Provenance::User);
    }
    if let Some(comment) = &notes.comment {
        metadata.set(keys::COMMENT, text(comment), Provenance::User);
    }
    if let Some(origin) = &notes.origin {
        metadata.set(keys::ORIGIN, text(origin), Provenance::Filesystem);
    }
    if let Some(referrer) = &notes.referrer {
        metadata.set(keys::REFERRER, text(referrer), Provenance::Filesystem);
    }
}

/// Fills the tags, rating, comment, origin and referrer fields from the
/// notes stores, and tells the vocabulary the tags it saw (with the colors
/// files give them) and the index which files have tags (and which no
/// longer have).
pub struct AttributesProvider {
    pub stores: Arc<NoteStores>,
    pub vocabulary: Arc<Vocabulary>,
    pub index: Option<Arc<crate::hints::TagIndex>>,
}

impl MetadataProvider for AttributesProvider {
    fn id(&self) -> &'static str {
        crate::ID
    }
    fn fill(&self, dir: &Path, entries: &mut [Entry]) {
        self.fill_with(dir, None, entries);
    }
    fn fill_listed(
        &self,
        dir: &Path,
        listed: &bibli_core::metadata::Listed<'_>,
        entries: &mut [Entry],
    ) {
        self.fill_with(dir, Some(listed), entries);
    }
}

impl AttributesProvider {
    fn fill_with(
        &self,
        dir: &Path,
        listed: Option<&bibli_core::metadata::Listed<'_>>,
        entries: &mut [Entry],
    ) {
        let policy = self.stores.policy();
        let files: Vec<bibli_core::notes::Named<'_>> = entries
            .iter()
            .map(|e| (e.name.as_os_str(), Some(e.id)))
            .collect();
        let read = self.stores.read(dir, &files, &policy, listed);
        let mut seen: Vec<String> = Vec::new();
        let mut given: Vec<(String, String)> = Vec::new();
        let mut learned: Vec<(std::path::PathBuf, Vec<String>)> = Vec::new();
        for (e, read) in entries.iter_mut().zip(read) {
            let notes = read.notes;
            let tags = notes.tag_names();
            if self.index.is_some() {
                learned.push((dir.join(&e.name), tags.clone()));
            }
            if notes.is_empty() {
                continue;
            }
            fill_entry(&mut e.metadata, &notes);
            let titles = |ids: &[&str]| {
                Value::List(
                    self.stores
                        .titles(ids)
                        .into_iter()
                        .map(|t| Value::Text(Arc::from(t)))
                        .collect(),
                )
            };
            let kept: Vec<&str> = read.each.iter().map(|(id, _)| *id).collect();
            e.metadata
                .set(keys::NOTES_KEPT, titles(&kept), Provenance::Filesystem);
            if !read.differ.is_empty() {
                e.metadata.set(
                    keys::NOTES_DIFFER,
                    titles(&read.differ),
                    Provenance::Filesystem,
                );
            }
            given.extend(
                notes
                    .tags
                    .iter()
                    .filter_map(|t| Some((t.name.clone(), t.color.clone()?))),
            );
            seen.extend(tags);
        }
        if !seen.is_empty() {
            self.vocabulary.saw(&seen);
        }
        if !given.is_empty() {
            let given: Vec<(String, &str)> =
                given.iter().map(|(t, c)| (t.clone(), c.as_str())).collect();
            self.vocabulary.given(&given);
        }
        if let Some(index) = &self.index {
            index.learn(learned);
        }
    }
}
