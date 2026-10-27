//! skwir plugin: tags in file names (ADR-027). Two conventions are known:
//! `TagSpaces`' (`IMG-2653[vacation alps].jpg`, tags in brackets before the
//! extension) and filetags' (`2021-01-01 notes -- work draft.txt`, after
//! ` -- `). Each is a store read with the others. Writing a name is
//! renaming the file: "Tags into Name" and "Tags out of Name" are plans
//! of renames of their own, and a store set to Update has an edit of a
//! file whose name holds tags rename it beside (`NotesStore::renamed`,
//! ADR-027's amendment of 2026-10-18), the name's tags kept in their order.
//!
//! A name says what its author meant only where the convention is used:
//! `title [id].ext` is how yt-dlp names every download and `[1080p]` ends
//! many a release's name. `TagSpaces`' names are read only in folders a
//! `TagSpaces` location covers (a `.ts` folder there or above); filetags'
//! only when turned on (Tags and notes page).

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use skwir_core::app::{App, NotifyLevel};
use skwir_core::command::Outcome;
use skwir_core::command::Topic;
use skwir_core::metadata::{Listed, same_tag};
use skwir_core::notes::raw::RawKey;
use skwir_core::notes::{Holds, Kept, Notes, NotesStore, Probe, Tag, Write, Wrote};
use skwir_core::op::{EntryRef, Plan, Rename, RenameOrderError, RenameRequest, order_renames};
use skwir_core::plugin::{Category, Plugin};
use skwir_core::registry::MenuItem;
use skwir_core::workspace::PaneId;

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

    /// `name` holding those of `tags` it can, where it holds tags and they
    /// differ (case aside): its own kept in their order, the others after
    /// them. `None` for a name that holds none, which is never started,
    /// and for one already saying them.
    pub fn retagged(self, name: &str, tags: &[String]) -> Option<String> {
        let (base, held, ext) = self.split(name);
        if held.is_empty() {
            return None;
        }
        let mut wanted: Vec<&String> = Vec::new();
        for t in tags.iter().filter(|t| self.holds(t)) {
            if !wanted.iter().any(|w| same_tag(w, t)) {
                wanted.push(t);
            }
        }
        if held.len() == wanted.len() && held.iter().all(|h| wanted.iter().any(|t| same_tag(t, h)))
        {
            return None;
        }
        let mut kept: Vec<String> = held
            .into_iter()
            .filter(|h| wanted.iter().any(|t| same_tag(t, h)))
            .collect();
        for t in wanted {
            if !kept.iter().any(|k| same_tag(k, t)) {
                kept.push(t.clone());
            }
        }
        Some(self.join(&base, &kept, ext.as_deref()))
    }
}

/// Tags in names, one convention, as a store.
pub struct NameStore {
    convention: Convention,
    /// Whether a `TagSpaces` location covers a folder: a `.ts` folder
    /// there or above it.
    tagspaces: skwir_core::companions::MarkedAbove,
}

impl NameStore {
    pub fn new(convention: Convention) -> Self {
        NameStore {
            convention,
            tagspaces: skwir_core::companions::MarkedAbove::new(".ts"),
        }
    }
}

impl NotesStore for NameStore {
    fn id(&self) -> &'static str {
        match self.convention {
            Convention::Tagspaces => "tagspaces-names",
            Convention::Filetags => "filetags",
        }
    }
    fn title(&self) -> &'static str {
        match self.convention {
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
            limits: match self.convention {
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
        self.convention == Convention::Tagspaces
    }
    fn applies(&self, dir: &Path) -> bool {
        match self.convention {
            Convention::Tagspaces => self.tagspaces.covers(dir),
            Convention::Filetags => true,
        }
    }
    fn fit(&self, notes: &Notes) -> Notes {
        Notes {
            tags: notes
                .tags
                .iter()
                .filter(|t| self.convention.holds(&t.name))
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
                let (_, tags, _) = self.convention.split(f.name.to_str()?);
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
    fn renamed(&self, name: &OsStr, notes: &Notes) -> Option<std::ffi::OsString> {
        self.convention
            .retagged(name.to_str()?, &notes.tag_names())
            .map(Into::into)
    }
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct NameArgs {
    /// The entries to rename, by absolute path; the pane's selection when
    /// omitted.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
    /// `tagspaces` (`name[a b].ext`, the default) or `filetags`
    /// (`name -- a b.ext`).
    #[serde(default)]
    convention: Convention,
}

fn store_id(convention: Convention) -> &'static str {
    NameStore::new(convention).id()
}

/// A listed entry's tags (`keys::TAGS`), every store's.
fn listed_tags(e: &skwir_core::Entry) -> Vec<String> {
    match e.metadata.value(skwir_core::entry::keys::TAGS) {
        Some(skwir_core::Value::List(items)) => items
            .iter()
            .filter_map(skwir_core::Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(skwir_core::Value::Text(t)) => vec![t.to_string()],
        _ => Vec::new(),
    }
}

/// Make the plan `make` makes of the targets, which reads their notes and
/// stats the names they would take: where the command runs, answering the
/// plan, off the window's thread (a script's, an agent's); on it, on a
/// worker in the targets' lane (ADR-077), the plan then applied as the
/// window applies a user's own (`App::apply_whole`), or what refused it
/// said. What the pane knows is taken where the command runs, with the
/// targets.
fn planned(
    app: &App,
    a: NameArgs,
    make: fn(&App, Vec<EntryRef>, Vec<Option<Vec<String>>>, Convention) -> anyhow::Result<Plan>,
) -> anyhow::Result<Outcome> {
    let refs = app.targets(a.pane, a.entries)?;
    // What the pane knows of each file's tags, read for the others only.
    let known: Vec<Option<Vec<String>>> = {
        let pane = app.pane_or_active(a.pane);
        let ws = app.workspace.read();
        refs.iter()
            .map(|r| {
                let e = ws.pane(pane)?.entry_at(r.id, &r.path)?;
                e.metadata.is_hydrated().then(|| listed_tags(e))
            })
            .collect()
    };
    if !skwir_core::scheduler::on_window_thread() {
        return make(app, refs, known, a.convention).map(Outcome::Plan);
    }
    let lane = match refs.split_first() {
        Some((first, rest)) => {
            let lane = skwir_core::fs::reach::lane(&first.path);
            if rest
                .iter()
                .all(|r| skwir_core::fs::reach::lane(&r.path) == lane)
            {
                lane
            } else {
                skwir_core::scheduler::Lane::SHARED
            }
        }
        None => skwir_core::scheduler::Lane::SHARED,
    };
    let weak = app.weak();
    let convention = a.convention;
    app.scheduler.spawn_in(
        lane,
        skwir_core::scheduler::Priority::Visible,
        "names.plan",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            match make(&app, refs, known, convention) {
                Ok(plan) => app.apply_whole(plan),
                Err(error) => app.notify(NotifyLevel::Error, format!("{error:#}")),
            }
        },
    );
    Ok(Outcome::Done)
}

/// "Tags into Name": each entry renamed to hold its tags (all stores'),
/// those the convention can hold. `known`: what the pane knows of each
/// one's tags; the others' are read.
fn tag_names(
    app: &App,
    refs: Vec<EntryRef>,
    known: Vec<Option<Vec<String>>>,
    convention: Convention,
) -> anyhow::Result<Plan> {
    let mut plan = Plan::new("put tags in names");
    let mut renames: Vec<RenameRequest> = Vec::new();
    let mut left_out: Vec<String> = Vec::new();
    let policy = app.notes.policy();
    for (r, known) in refs.into_iter().zip(known) {
        let Some(name) = r.path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let names = known.unwrap_or_else(|| app.notes.read_path(&r.path, &policy).tag_names());
        let (base, _, ext) = convention.split(name);
        let tags: Vec<String> = names
            .into_iter()
            .filter(|t| {
                let held = convention.holds(t);
                if !held && !left_out.iter().any(|l| same_tag(l, t)) {
                    left_out.push(t.clone());
                }
                held
            })
            .collect();
        let new_name = convention.join(&base, &tags, ext.as_deref());
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
    Ok(plan)
}

/// "Tags out of Name": each entry renamed without its tags, which the
/// other stores keep first (a step before the rename), so none is lost.
fn untag_names(
    app: &App,
    refs: Vec<EntryRef>,
    _known: Vec<Option<Vec<String>>>,
    convention: Convention,
) -> anyhow::Result<Plan> {
    let mut plan = Plan::new("take tags out of names");
    let mut renames: Vec<RenameRequest> = Vec::new();
    let name_store = store_id(convention);
    for r in refs {
        let Some(name) = r.path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let (base, tags, ext) = convention.split(name);
        if tags.is_empty() {
            continue;
        }
        // The file's tags, the name's with them, kept by the other stores:
        // an edit that reads them without the name, when it is applied.
        let mut keep = app
            .notes
            .edit(r.clone())
            .change_tags(skwir_core::notes::TagChange {
                add: tags.clone(),
                ..Default::default()
            });
        keep.policy.read.retain(|s| s != name_store);
        keep.policy.write.retain(|(s, _)| s != name_store);
        plan.push(keep);
        renames.push(RenameRequest {
            entry: r,
            new_name: convention.join(&base, &[], ext.as_deref()).into(),
        });
    }
    // Every file's tags are kept before any name changes.
    for rename in ordered(&renames)? {
        plan.push(rename);
    }
    Ok(plan)
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
        command: skwir_core::command::Invocation::new(command),
        // Files' names, never a place's (a tag's has none).
        when: Some("selection !place".into()),
        order,
        icon: Some(icon.into()),
        // With the tags they write, in the context menu's Tags submenu
        // (ADR-099).
        submenu: Some("Tags".into()),
        checked: None,
        choice: None,
        color: None,
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
        "Tags written in file names, as TagSpaces (name[a b].ext) and filetags (name -- a b.ext) do: read with the other notes, written by renaming, on request or as an edit changes them (Tags and notes page)"
    }
    fn category(&self) -> Category {
        Category::Files
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Write])
    }
    fn register(&self, app: &App) {
        app.register_notes_store(ID, Arc::new(NameStore::new(Convention::Tagspaces)));
        app.register_notes_store(ID, Arc::new(NameStore::new(Convention::Filetags)));
        app.commands.register::<NameArgs, _>(
            ID, Topic::Notes,
            "names.tag",
            "Tags into name",
            "Rename the entries given, or the selection, to hold their tags in their names (`convention`: `tagspaces`, name[a b].ext, or `filetags`, name -- a b.ext) (plan)",
            |a, app| planned(app, a, tag_names),
        )
        .plans();
        app.commands.register::<NameArgs, _>(
            ID, Topic::Notes,
            "names.untag",
            "Tags out of name",
            "Rename the entries given, or the selection, without the tags in their names, the other stores keeping them first (plan)",
            |a, app| planned(app, a, untag_names),
        )
        .plans();
        app.registry.menus.add(
            ID,
            menu_item("Write Tags into Name", "names.tag", 570, "tag"),
        );
        app.registry.menus.add(
            ID,
            menu_item("Take Tags out of Name", "names.untag", 571, "rename"),
        );
    }
}

#[cfg(test)]
mod any_input;
#[cfg(test)]
mod tests;
