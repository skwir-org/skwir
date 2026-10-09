//! Metadata providers: plugins that fill entry fields during hydration
//! (tags from xattrs, MIME types, media dimensions, ...). Called in batches
//! on a worker right after stat, never on the UI thread.

use std::path::Path;
use std::sync::Arc;

use crate::registry::{Contributions, OwnerGate};

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

/// What separates a category from what is in it: `Places/France/Paris` is
/// the tag `Paris` in the category `France`, itself in `Places`, as
/// digiKam writes nested keywords (Lightroom's `|` is read as this). A
/// category is a tag too: every file tagged within it has it, so a search
/// for `Places` finds `Places/France/Paris`, as a folder holds its
/// subfolders; nothing writes the categories onto the file.
pub const TAG_SEPARATOR: char = '/';

/// A tag as typed, made one: each part trimmed, empty parts left out
/// (`" Places / France/ "` is `Places/France`). Empty when nothing is left.
pub fn clean_tag(typed: &str) -> String {
    typed
        .split(TAG_SEPARATOR)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// A tag a crumb or a pill stands for, by its full name (`Places/France`):
/// the files that have it, or those that `lacks` it (“not family”).
#[derive(
    Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct TagRef {
    pub name: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lacks: bool,
}

impl TagRef {
    /// The files that have `name`.
    pub fn of(name: impl Into<String>) -> Self {
        TagRef {
            name: name.into(),
            lacks: false,
        }
    }

    /// The files without `name`.
    pub fn lacking(name: impl Into<String>) -> Self {
        TagRef {
            name: name.into(),
            lacks: true,
        }
    }
}

/// The tag's own name, without its categories: `Paris`.
pub fn tag_leaf(tag: &str) -> &str {
    tag.rsplit_once(TAG_SEPARATOR).map_or(tag, |(_, leaf)| leaf)
}

/// The category the tag is in: `Places/France`; `None` at the top.
pub fn tag_parent(tag: &str) -> Option<&str> {
    tag.rsplit_once(TAG_SEPARATOR).map(|(parent, _)| parent)
}

/// The tag, then each category it is in, the nearest first:
/// `Places/France/Paris`, `Places/France`, `Places`.
pub fn tag_lineage(tag: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(tag), |t| tag_parent(t))
}

/// What follows `category` in `tag` when the tag is within it (ignoring
/// case, as [`same_tag`]): `""` for the category itself, `/France/Paris`
/// for `Places/France/Paris` within `Places`; `None` when it is not.
pub fn tag_under<'a>(tag: &'a str, category: &str) -> Option<&'a str> {
    if category.is_empty() {
        return None;
    }
    tag.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(tag.len()))
        .filter(|&i| i == tag.len() || tag[i..].starts_with(TAG_SEPARATOR))
        .find(|&i| same_tag(&tag[..i], category))
        .map(|i| &tag[i..])
}

/// Whether `tag` is `category` or within it.
pub fn tag_within(tag: &str, category: &str) -> bool {
    tag_under(tag, category).is_some()
}

/// The setting where the tags plugin keeps the categories that hold one
/// tag at a time (`Status`: `Status/Draft` or `Status/Final`): a field of
/// the file, whose value is the tag within it. The UI reads it to show
/// them as fields and to choose among their tags.
pub const TAG_EXCLUSIVE_SETTING: &str = "tags.exclusive";

/// The category among `exclusive` that `tag` is within (not the category
/// itself), the nearest: the field `tag` is a value of.
pub fn exclusive_category<'a>(tag: &str, exclusive: &'a [String]) -> Option<&'a str> {
    tag_lineage(tag)
        .skip(1)
        .find_map(|c| exclusive.iter().find(|e| same_tag(e, c)))
        .map(String::as_str)
}

/// `current` with `add` given and `remove` taken, as spelled where they
/// were already (tags ignore case); one given in a category holding one
/// tag at a time (`exclusive`) takes the place of the one there was. What
/// an edit of several files does to each (`notes::TagChange`).
pub fn edited_tags(
    current: &[String],
    add: &[String],
    remove: &[String],
    exclusive: &[String],
) -> Vec<String> {
    let mut tags: Vec<String> = current
        .iter()
        .filter(|t| !remove.iter().any(|r| same_tag(r, t)))
        .cloned()
        .collect();
    for a in add {
        tags = with_exclusive(&tags, a, exclusive);
    }
    tags
}

/// `tags` with `added` given, a tag in a category holding one at a time
/// taking the place of the one there was (`exclusive`).
pub fn with_exclusive(tags: &[String], added: &str, exclusive: &[String]) -> Vec<String> {
    let field = exclusive_category(added, exclusive);
    let mut out: Vec<String> = tags
        .iter()
        .filter(|t| {
            !field.is_some_and(|f| {
                exclusive_category(t, exclusive).is_some_and(|g| same_tag(f, g))
                    && !same_tag(t, added)
            })
        })
        .cloned()
        .collect();
    if !out.iter().any(|t| same_tag(t, added)) {
        out.push(added.to_owned());
    }
    out
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
/// after a color wears it ("Red", as a Mac's first tags are named). A tag
/// in a category wears the category's at the top, so a category's tags
/// read as one group.
pub fn tag_color(tag: &str) -> usize {
    let tag = tag_lineage(tag).last().unwrap_or(tag);
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
    /// `fill`, for a batch of a folder whose names are all known: `listed`
    /// says whether a name is in it, so a provider that looks for files
    /// beside a file (ADR-028) does not read the folder again.
    fn fill_listed(&self, dir: &Path, _listed: &Listed<'_>, entries: &mut [Entry]) {
        self.fill(dir, entries);
    }
}

/// Whether a name is in a folder, as its listing knows.
pub type Listed<'a> = dyn Fn(&std::ffi::OsStr) -> bool + 'a;

/// The registered providers, in registration order.
#[derive(Default)]
pub struct MetadataProviders {
    list: Contributions<Arc<dyn MetadataProvider>>,
}

impl MetadataProviders {
    pub fn gated(gate: Arc<OwnerGate>) -> Self {
        MetadataProviders {
            list: Contributions::gated(gate),
        }
    }

    pub fn register(&self, owner: &str, provider: Arc<dyn MetadataProvider>) {
        self.list.add(owner, provider);
    }

    pub fn unregister_owner(&self, owner: &str) {
        self.list.remove_owner(owner);
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.list.all().iter().map(|p| p.id()).collect()
    }

    /// Run every provider over `entries` (all children of `dir`).
    /// Fill `entries` of `dir` from every provider, with `listed` when the
    /// folder's names are all known.
    pub fn fill_listed(&self, dir: &Path, listed: &Listed<'_>, entries: &mut [Entry]) {
        if entries.is_empty() {
            return;
        }
        let providers = self.list.all();
        for p in providers {
            p.fill_listed(dir, listed, entries);
        }
    }

    pub fn fill(&self, dir: &Path, entries: &mut [Entry]) {
        if entries.is_empty() {
            return;
        }
        let providers = self.list.all();
        for p in providers {
            p.fill(dir, entries);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_is_within_its_categories() {
        assert_eq!(
            clean_tag(" Places / France/ /Paris "),
            "Places/France/Paris"
        );
        assert_eq!(clean_tag(" / "), "");
        assert_eq!(tag_leaf("Places/France/Paris"), "Paris");
        assert_eq!(tag_leaf("Work"), "Work");
        assert_eq!(tag_parent("Places/France/Paris"), Some("Places/France"));
        assert_eq!(tag_parent("Work"), None);
        assert_eq!(
            tag_lineage("a/b/c").collect::<Vec<_>>(),
            ["a/b/c", "a/b", "a"]
        );
        assert_eq!(
            tag_under("Places/France/Paris", "places"),
            Some("/France/Paris")
        );
        assert_eq!(tag_under("Places/France", "PLACES/france"), Some(""));
        assert_eq!(
            tag_under("Placesque", "Places"),
            None,
            "a name that only begins so"
        );
        assert_eq!(tag_under("Places", "Places/France"), None);
        assert_eq!(tag_under("Été/Août", "été"), Some("/Août"));
        assert!(tag_within("a/b", "a") && !tag_within("a", "a/b") && !tag_within("a", ""));
        assert_eq!(
            tag_color("Places/France"),
            tag_color("places"),
            "the category's"
        );
    }

    #[test]
    fn a_category_of_one_tag_at_a_time_is_a_field() {
        let exclusive = vec!["status".to_owned()];
        assert_eq!(
            exclusive_category("Status/Draft", &exclusive),
            Some("status")
        );
        assert_eq!(
            exclusive_category("Status", &exclusive),
            None,
            "the field itself"
        );
        assert_eq!(exclusive_category("work", &exclusive), None);
        let tags: Vec<String> = ["work", "Status/Draft"].map(str::to_owned).into();
        assert_eq!(
            with_exclusive(&tags, "Status/Final", &exclusive),
            ["work", "Status/Final"]
        );
        assert_eq!(
            with_exclusive(&tags, "Places/x", &exclusive),
            ["work", "Status/Draft", "Places/x"]
        );
        assert_eq!(
            with_exclusive(&tags, "status/draft", &exclusive),
            tags,
            "there already"
        );
    }

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
