//! Metadata providers: plugins that fill entry fields during hydration
//! (tags from xattrs, MIME types, media dimensions, ...). Called in batches
//! on a worker right after stat, never on the UI thread.

use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::entry::Entry;

/// Shared on-disk tag format used by the tags provider and copy operations.
/// The field that holds tags in memory is [`crate::entry::keys::TAGS`].
pub const TAGS_XATTR: &str = "user.xdg.tags";

/// Whether two tags are the same tag. Tags ignore case ("Work" and "work"
/// are one tag; the spelling first written is kept), so adding, removing
/// and searching all agree. Unicode lowercase, no allocation.
pub fn same_tag(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// The setting where the tags plugin keeps the colors the user chose,
/// `{ tag in lowercase = color }`; the UI reads it to draw tags.
pub const TAG_COLOR_SETTING: &str = "tags.colors";

/// The colors a tag can wear, by name: the tags plugin stores a user's
/// choice under one of these (`tags.colors`), and the UI draws each with a
/// token of its theme. A tag without a choice wears [`tag_color`]'s.
pub const TAG_COLORS: [&str; 8] = [
    "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink",
];

/// The color a tag wears when the user chose none, as an index into
/// [`TAG_COLORS`]: from its name, ignoring case, so every view, every
/// session and every machine agrees without storing anything. A tag named
/// after a color wears it ("Red", as a Mac's first tags are named).
pub fn tag_color(tag: &str) -> usize {
    if let Some(named) = TAG_COLORS.iter().position(|c| same_tag(c, tag)) {
        return named;
    }
    // FNV-1a: stable across releases, unlike std's hasher.
    let mut hash: u32 = 0x811c_9dc5;
    for c in tag.chars().flat_map(char::to_lowercase) {
        let mut buf = [0; 4];
        for b in c.encode_utf8(&mut buf).bytes() {
            hash = (hash ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
    }
    hash as usize % TAG_COLORS.len()
}

/// Fills fields on freshly stat-ed entries.
pub trait MetadataProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Fill what you know for each `(path, entry)`. Set fields with your own
    /// provenance (`Provenance::Plugin { name }`) and namespaced keys.
    fn fill(&self, dir: &Path, entries: &mut [Entry]);
}

/// The registered providers, in registration order.
#[derive(Default)]
pub struct MetadataProviders {
    list: RwLock<Vec<(Arc<str>, Arc<dyn MetadataProvider>)>>,
}

impl MetadataProviders {
    pub fn register(&self, owner: &str, provider: Arc<dyn MetadataProvider>) {
        self.list.write().push((owner.into(), provider));
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.list.write().retain(|(o, _)| &**o != owner);
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.list.read().iter().map(|(_, p)| p.id()).collect()
    }

    /// Run every provider over `entries` (all children of `dir`).
    pub fn fill(&self, dir: &Path, entries: &mut [Entry]) {
        if entries.is_empty() {
            return;
        }
        let providers: Vec<Arc<dyn MetadataProvider>> = self
            .list
            .read()
            .iter()
            .map(|(_, p)| Arc::clone(p))
            .collect();
        for p in providers {
            p.fill(dir, entries);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_named_after_a_color_wears_it() {
        for (i, name) in TAG_COLORS.iter().enumerate() {
            assert_eq!(tag_color(name), i);
            assert_eq!(tag_color(&name.to_uppercase()), i);
        }
        assert_eq!(tag_color("Work"), tag_color("work"), "case never matters");
        assert!(tag_color("reddish") < TAG_COLORS.len());
    }
}
