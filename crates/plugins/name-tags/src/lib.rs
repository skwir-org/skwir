//! bibli plugin: tags in file names (ADR-027). Two conventions are known:
//! `TagSpaces`' (`IMG-2653[vacation alps].jpg`, tags in brackets before the
//! extension) and filetags' (`2021-01-01 notes -- work draft.txt`, after
//! ` -- `). Each is a store read with the others; neither is written by
//! an edit, since writing a name is renaming the file: "Tags into Name"
//! and "Tags out of Name" are plans of renames of their own.
//!
//! A name says what its author meant only where the convention is used:
//! `title [id].ext` is how yt-dlp names every download and `[1080p]` ends
//! many a release's name. `TagSpaces`' names are read only in folders a
//! `TagSpaces` location covers (a `.ts` folder there or above); filetags'
//! only when turned on (Notes page).

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use bibli_core::app::App;
use bibli_core::command::Outcome;
use bibli_core::metadata::{Listed, same_tag};
use bibli_core::notes::raw::RawKey;
use bibli_core::notes::{Holds, Kept, Notes, NotesStore, Probe, Tag, Write, Wrote};
use bibli_core::op::{EntryRef, Plan, Rename, RenameOrderError, RenameRequest, order_renames};
use bibli_core::plugin::{Category, Plugin};
use bibli_core::registry::MenuItem;
use bibli_core::workspace::PaneId;

pub(crate) const ID: &str = "name-tags";

/// A convention of tags in names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Convention {
    /// `name[a b].ext`
    #[default]
    Tagspaces,
    /// `name -- a b.ext`
    Filetags,
}

/// A name cut at its extension: `a.tar.gz` is `a.tar` and `gz`, `.bashrc`
/// has none.
fn cut(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(at) if at > 0 => (&name[..at], Some(&name[at + 1..])),
        _ => (name, None),
    }
}

impl Convention {
    /// A name as its base (without tags or extension), its tags and its
    /// extension.
    pub fn split(self, name: &str) -> (String, Vec<String>, Option<String>) {
        let (stem, ext) = cut(name);
        let ext = ext.map(str::to_owned);
        let (base, tags) = match self {
            Convention::Tagspaces => match (stem.strip_suffix(']'), stem.rfind('[')) {
                (Some(_), Some(open)) if open > 0 => (
                    stem[..open].trim_end(),
                    stem[open + 1..stem.len() - 1].split_whitespace().collect(),
                ),
                _ => (stem, Vec::new()),
            },
            Convention::Filetags => match stem.rfind(" -- ") {
                Some(at) => (&stem[..at], stem[at + 4..].split_whitespace().collect()),
                None => (stem, Vec::new()),
            },
        };
        let mut unique: Vec<String> = Vec::new();
        for t in tags {
            if !unique.iter().any(|u| same_tag(u, t)) {
                unique.push(t.to_owned());
            }
        }
        (base.to_owned(), unique, ext)
    }

    /// The name of `base` with `tags` and `ext`.
    pub fn join(self, base: &str, tags: &[String], ext: Option<&str>) -> String {
        let mut name = base.to_owned();
        if !tags.is_empty() {
            match self {
                Convention::Tagspaces => {
                    name.push('[');
                    name.push_str(&tags.join(" "));
                    name.push(']');
                }
                Convention::Filetags => {
                    name.push_str(" -- ");
                    name.push_str(&tags.join(" "));
                }
            }
        }
        if let Some(ext) = ext {
            name.push('.');
            name.push_str(ext);
        }
        name
    }

    /// Whether a name can hold `tag` this way.
    pub fn holds(self, tag: &str) -> bool {
        !tag.is_empty()
            && !tag.contains(char::is_whitespace)
            && !tag.contains('/')
            && (self == Convention::Filetags || !tag.contains(['[', ']']))
    }
}

/// Whether a `TagSpaces` location covers `dir`: a `.ts` folder there or
/// above it.
fn tagspaces_used(dir: &Path) -> bool {
    dir.ancestors().any(|d| d.join(".ts").is_dir())
}

/// Tags in names, one convention, as a store.
pub struct NameStore(pub Convention);

impl NotesStore for NameStore {
    fn id(&self) -> &'static str {
        match self.0 {
            Convention::Tagspaces => "tagspaces-names",
            Convention::Filetags => "filetags",
        }
    }
    fn title(&self) -> &'static str {
        match self.0 {
            Convention::Tagspaces => "Tags in names, as TagSpaces writes them: name[a b].ext",
            Convention::Filetags => "Tags in names, as filetags writes them: name -- a b.ext",
        }
    }
    fn kept(&self) -> Kept {
        Kept::Name
    }
    fn holds(&self) -> Holds {
        Holds {
            tags: true,
            limits: match self.0 {
                Convention::Tagspaces => {
                    "tags without spaces, brackets or categories, no rating, no comment"
                }
                Convention::Filetags => "tags without spaces or categories, no rating, no comment",
            },
            ..Holds::default()
        }
    }
    fn default_write(&self) -> Write {
        Write::Off
    }
    fn default_read(&self) -> bool {
        self.0 == Convention::Tagspaces
    }
    fn applies(&self, dir: &Path) -> bool {
        match self.0 {
            Convention::Tagspaces => tagspaces_used(dir),
            Convention::Filetags => true,
        }
    }
    fn fit(&self, notes: &Notes) -> Notes {
        Notes {
            tags: notes
                .tags
                .iter()
                .filter(|t| self.0.holds(&t.name))
                .map(|t| Tag::named(t.name.clone()))
                .collect(),
            ..Notes::default()
        }
    }
    fn read(
        &self,
        _dir: &Path,
        files: &[Probe<'_>],
        _listed: Option<&Listed<'_>>,
    ) -> Vec<Option<Notes>> {
        files
            .iter()
            .map(|f| {
                let (_, tags, _) = self.0.split(f.name.to_str()?);
                (!tags.is_empty()).then(|| Notes {
                    tags: tags.into_iter().map(Tag::named).collect(),
                    ..Notes::default()
                })
            })
            .collect()
    }
    fn keys(&self, _file: &Probe<'_>, _create: bool) -> Vec<RawKey> {
        Vec::new()
    }
    fn write(&self, _file: &Probe<'_>, _notes: &Notes, _create: bool) -> std::io::Result<Wrote> {
        Ok(Wrote::NotHere)
    }
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct NameArgs {
    /// The entries to rename; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<EntryRef>,
    #[serde(default)]
    pane: Option<PaneId>,
    /// `tagspaces` (`name[a b].ext`, the default) or `filetags`
    /// (`name -- a b.ext`).
    #[serde(default)]
    convention: Convention,
}

fn store_id(convention: Convention) -> &'static str {
    NameStore(convention).id()
}

/// "Tags into Name": each entry renamed to hold its tags (all stores'),
/// those the convention can hold.
fn tag_names(app: &App, a: NameArgs) -> anyhow::Result<Outcome> {
    let refs = app.targets(a.pane, a.entries)?;
    let mut plan = Plan::new("put tags in names");
    let mut renames: Vec<RenameRequest> = Vec::new();
    let mut left_out: Vec<String> = Vec::new();
    let policy = app.notes.policy();
    for r in refs {
        let Some(name) = r.path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let notes = app.notes.read_path(&r.path, &policy);
        let (base, _, ext) = a.convention.split(name);
        let tags: Vec<String> = notes
            .tag_names()
            .into_iter()
            .filter(|t| {
                let held = a.convention.holds(t);
                if !held && !left_out.iter().any(|l| same_tag(l, t)) {
                    left_out.push(t.clone());
                }
                held
            })
            .collect();
        let new_name = a.convention.join(&base, &tags, ext.as_deref());
        if new_name != name {
            renames.push(RenameRequest {
                entry: r,
                new_name: new_name.into(),
            });
        }
    }
    for rename in ordered(&renames)? {
        plan.push(rename);
    }
    if !left_out.is_empty() {
        plan = plan.with_note(format!(
            "Left out, as a name cannot hold them: {}.",
            left_out.join(", ")
        ));
    }
    Ok(Outcome::Plan(plan))
}

/// "Tags out of Name": each entry renamed without its tags, which the
/// other stores keep first (a step before the rename), so none is lost.
fn untag_names(app: &App, a: NameArgs) -> anyhow::Result<Outcome> {
    let refs = app.targets(a.pane, a.entries)?;
    let mut plan = Plan::new("take tags out of names");
    let mut renames: Vec<RenameRequest> = Vec::new();
    let policy = app.notes.policy();
    let name_store = store_id(a.convention);
    for r in refs {
        let Some(name) = r.path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let (base, tags, ext) = a.convention.split(name);
        if tags.is_empty() {
            continue;
        }
        // The file's tags, the name's with them, kept by the other stores:
        // an edit that reads them without the name.
        let mut all = app.notes.read_path(&r.path, &policy).tag_names();
        for t in &tags {
            if !all.iter().any(|x| same_tag(x, t)) {
                all.push(t.clone());
            }
        }
        let mut keep = app.notes.edit(r.clone()).tags(all);
        keep.policy.read.retain(|s| s != name_store);
        plan.push(keep);
        renames.push(RenameRequest {
            entry: r,
            new_name: a.convention.join(&base, &[], ext.as_deref()).into(),
        });
    }
    // Every file's tags are kept before any name changes.
    for rename in ordered(&renames)? {
        plan.push(rename);
    }
    Ok(Outcome::Plan(plan))
}

/// The renames in an order that never renames onto a name still taken,
/// refused when two files would get one name (`a[x].jpg` and `a[y].jpg`
/// both `a.jpg`) or a name is taken by a file that is not renamed.
fn ordered(requests: &[RenameRequest]) -> anyhow::Result<Vec<Rename>> {
    let name = |path: &Path| {
        path.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    order_renames(requests, |path| path.symlink_metadata().is_ok()).map_err(|e| match e {
        RenameOrderError::Duplicate { target, .. } => {
            anyhow::anyhow!("Several files would be named “{}”", name(&target))
        }
        RenameOrderError::Conflict { item, target } => anyhow::anyhow!(
            "“{}” would be named “{}”, which already exists",
            name(&requests[item].entry.path),
            name(&target)
        ),
        RenameOrderError::InvalidName { reason, .. } => anyhow::anyhow!(reason),
    })
}

fn menu_item(title: &str, command: &str, order: i32, icon: &str) -> MenuItem {
    MenuItem {
        menu: "context".into(),
        title: title.into(),
        command: bibli_core::command::Invocation::new(command),
        // Files' names, never a place's (a tag's has none).
        when: Some("selection !place".into()),
        order,
        icon: Some(icon.into()),
        submenu: Some("Notes".into()),
    }
}

/// The plugin.
#[derive(Debug, Default)]
pub struct NameTagsPlugin;

impl Plugin for NameTagsPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Tags in names"
    }
    fn description(&self) -> &'static str {
        "Tags written in file names, as TagSpaces (name[a b].ext) and filetags (name -- a b.ext) do: read with the other notes, written by renaming"
    }
    fn category(&self) -> Category {
        Category::Files
    }
    fn register(&self, app: &App) {
        app.register_notes_store(ID, Arc::new(NameStore(Convention::Tagspaces)));
        app.register_notes_store(ID, Arc::new(NameStore(Convention::Filetags)));
        app.commands.register::<NameArgs, _>(
            ID,
            "names.tag",
            "Tags into name",
            "Rename the selection to hold its tags in its names (`convention`: `tagspaces`, name[a b].ext, or `filetags`, name -- a b.ext) (plan)",
            |a, app| tag_names(app, a),
        );
        app.commands.register::<NameArgs, _>(
            ID,
            "names.untag",
            "Tags out of name",
            "Rename the selection without the tags in its names, the other stores keeping them first (plan)",
            |a, app| untag_names(app, a),
        );
        app.registry
            .menus
            .add(ID, menu_item("Tags into Name", "names.tag", 170, "tag"));
        app.registry.menus.add(
            ID,
            menu_item("Tags out of Name", "names.untag", 171, "rename"),
        );
    }
}

#[cfg(test)]
mod tests;
