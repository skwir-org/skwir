//! skwir plugin: tags and the other notes a file carries in its extended
//! attributes, so they travel with the file and need no database: tags in
//! `user.xdg.tags` (comma-separated, the freedesktop convention), a rating
//! in `user.baloo.rating` and a comment in `user.xdg.comment`, as Dolphin
//! writes them, and where a download came from in `user.xdg.origin.url`
//! and `user.xdg.referrer.url`, as browsers and download tools record it.
//!
//! A metadata provider surfaces them as fields during hydration (one
//! `listxattr` per file); operations write the first three with proper
//! inverses. The tags the user has are remembered as suggestions in the
//! cache (`vocabulary`), and the color a user chose for a tag is a setting
//! (`tags.colors`); every other tag wears the color its name gives it.

#[cfg(test)]
mod any_input;
mod appledouble;
mod attrs;
mod browse;
mod count;
mod everywhere;
mod fields;
pub mod finder;
mod follow;
mod hints;
pub mod index;
pub mod location;
mod sidebar;
mod vocabulary;
pub mod xdg;

use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::Mutex;
use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Form, Invocation, Outcome};
use skwir_core::entry::keys;
use skwir_core::fs::reach;
use skwir_core::metadata::{TAG_COLOR_SETTING, TAG_COLORS, same_tag};
use skwir_core::op::{EntryRef, Plan};
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::registry::{KeyBinding, MenuItem, MenuSource};
use skwir_core::scheduler::Priority;
use skwir_core::settings::SettingSpec;
use skwir_core::workspace::PaneId;

pub use attrs::tags_of;
pub use skwir_core::notes::parse_tags;
use skwir_core::notes::{NoteStores, Notes};

use vocabulary::Vocabulary;

pub(crate) const ID: &str = "tags";
/// The metadata field key.
pub const FIELD: &str = keys::TAGS;

/// The plugin's stores (ADR-027), the desktop's first: it is this
/// platform's own.
fn stores() -> Vec<Arc<dyn skwir_core::notes::NotesStore>> {
    vec![
        Arc::new(xdg::XdgStore),
        Arc::new(finder::FinderStore),
        Arc::new(appledouble::AppleDoubleStore::default()),
    ]
}

fn register_stores(registry: &NoteStores) {
    for store in stores() {
        registry.register(ID, store);
    }
}

/// A file's notes as the plugin's stores keep them by default, read
/// without an app (tools and tests). Blocking.
pub fn read_notes(path: &std::path::Path) -> Notes {
    let stores = NoteStores::default();
    register_stores(&stores);
    stores.read_path(path, &stores.policy())
}

/// A file's tags, from every store. Blocking.
pub fn read_tags(path: &std::path::Path) -> Vec<String> {
    read_notes(path).tag_names()
}

pub fn read_rating(path: &std::path::Path) -> Option<u64> {
    read_notes(path).rating
}

pub fn read_comment(path: &std::path::Path) -> Option<String> {
    read_notes(path).comment
}

/// Write `tags` as the file's desktop tags (`user.xdg.tags`) alone, an
/// empty list removing them: fixtures and tools, not edits, which go
/// through the stores.
///
/// # Errors
/// When the attribute cannot be written.
pub fn write_tags(path: &std::path::Path, tags: &[String]) -> std::io::Result<()> {
    let joined = tags.join(",");
    xdg::write(
        path,
        xdg::TAGS,
        (!tags.is_empty()).then_some(joined.as_str()),
    )
}

/// A file's notes, as the app's stores keep them now. Blocking.
fn notes_of(app: &App, path: &std::path::Path) -> Notes {
    app.notes.read_path(path, &app.notes.policy())
}

/// At most this many files are read by `tags.list { read }`: enough for
/// the tags a search found, bounded for one that found a disk.
const READ_AT_MOST: usize = 20_000;

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct ListArgs {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Also read the tags of entries whose tags were not read yet.
    #[serde(default)]
    read: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct TagsArgs {
    #[serde(default)]
    tags: Vec<String>,
    /// The entries to act on, by absolute path; the pane's selection when
    /// omitted.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ExclusiveArgs {
    /// The category (`Status`).
    tag: String,
    /// One tag of it at a time; several again when false.
    on: bool,
    /// Set by the toast's Undo, so the next toast offers Redo.
    #[serde(default)]
    undo: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ToggleArgs {
    /// The tag to give or take.
    tag: String,
    /// The entries to act on, by absolute path; the pane's selection when
    /// omitted.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct EditArgs {
    /// Tags to give every selected entry that lacks them.
    #[serde(default)]
    add: Vec<String>,
    /// Tags to take from every selected entry that has them.
    #[serde(default)]
    remove: Vec<String>,
    /// The entries to act on, by absolute path; the pane's selection when
    /// omitted.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct PaneArgs {
    /// The entries to act on, by absolute path; the pane's selection when
    /// omitted.
    #[serde(default)]
    entries: Vec<skwir_core::op::EntryArg>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RenameArgs {
    /// The tag to rename.
    #[serde(default)]
    tag: String,
    /// More tags to rename with it: all merge into `to`.
    #[serde(default)]
    tags: Vec<String>,
    /// Its new name; a tag of that name already on a file merges with it.
    to: String,
    /// Where the files are; the home folder when omitted.
    #[serde(default)]
    root: Option<PathArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct MoveArgs {
    /// The tag to move.
    #[serde(default)]
    tag: String,
    /// More tags to move with it.
    #[serde(default)]
    tags: Vec<String>,
    /// The category they go into (`Places/France`); out of their category,
    /// to the top, when omitted.
    #[serde(default)]
    into: Option<String>,
    /// Where the files are; the home folder when omitted.
    #[serde(default)]
    root: Option<PathArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct DeleteArgs {
    /// The tag to take from every file.
    #[serde(default)]
    tag: String,
    /// More tags to take with it.
    #[serde(default)]
    tags: Vec<String>,
    /// Where the files are; the home folder when omitted.
    #[serde(default)]
    root: Option<PathArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct PinArgs {
    #[serde(default)]
    tag: String,
    #[serde(default)]
    tags: Vec<String>,
    /// The pinned tag it goes before; last when omitted.
    #[serde(default)]
    before: Option<String>,
    /// Set by the toast's Undo, so the next toast offers Redo.
    #[serde(default)]
    undo: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ColorArgs {
    tag: String,
    /// One of `red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`,
    /// `pink`; omitted, the tag wears the color its name gives it.
    #[serde(default)]
    color: Option<String>,
}

/// A plan of one step per target entry, built from what each entry holds
/// now (read from the file: the listing's field can be stale), leaving out
/// the entries it would not change.
fn per_entry<T: skwir_core::op::Operation + 'static>(
    refs: Vec<EntryRef>,
    title: String,
    step: impl Fn(EntryRef) -> Option<T>,
) -> anyhow::Result<Outcome> {
    if refs.is_empty() {
        return Err(skwir_core::command::NothingSelected.into());
    }
    let mut plan = Plan::new(title);
    for r in refs {
        if let Some(op) = step(r) {
            plan.push(op);
        }
    }
    Ok(Outcome::Plan(plan))
}

/// `tags` with `add` given and `remove` taken, as spelled where they were
/// already (tags ignore case); one given in a category holding one tag at
/// a time (`exclusive`) takes the place of the one there was.
#[cfg(test)]
fn edited(
    current: &[String],
    add: &[String],
    remove: &[String],
    exclusive: &[String],
) -> Vec<String> {
    skwir_core::metadata::edited_tags(current, add, remove, exclusive)
}

/// Whether a listed entry's notes hold `tag` (`keys::TAGS`).
fn has_tag(e: &skwir_core::Entry, tag: &str) -> bool {
    match e.metadata.value(keys::TAGS) {
        Some(skwir_core::Value::List(items)) => items
            .iter()
            .filter_map(skwir_core::Value::as_str)
            .any(|t| same_tag(t, tag)),
        Some(skwir_core::Value::Text(t)) => same_tag(t, tag),
        _ => false,
    }
}

/// "add tag work", "remove tags a, b", "add tag x and remove tag y".
fn edit_title(add: &[String], remove: &[String]) -> String {
    let part = |verb: &str, tags: &[String]| {
        let noun = if tags.len() == 1 { "tag" } else { "tags" };
        format!("{verb} {noun} {}", tags.join(", "))
    };
    match (add.is_empty(), remove.is_empty()) {
        (false, true) => part("add", add),
        (true, false) => part("remove", remove),
        (false, false) => format!("{} and {}", part("add", add), part("remove", remove)),
        (true, true) => "change tags".into(),
    }
}

fn edit_plan(app: &App, vocabulary: &Arc<Vocabulary>, a: EditArgs) -> anyhow::Result<Outcome> {
    let add = parse_tags(&a.add.join(","));
    let remove = parse_tags(&a.remove.join(","));
    let exclusive = fields::exclusive(app);
    vocabulary.used(&add);
    // What each file holds is read when the plan is applied, where a file
    // that would not change is skipped: nothing is read here, on the
    // thread that dispatched it.
    let change = skwir_core::notes::TagChange {
        add: add.clone(),
        remove: remove.clone(),
        exclusive,
        ..Default::default()
    };
    per_entry(
        app.targets(a.pane, a.entries)?,
        edit_title(&add, &remove),
        |r| Some(app.notes.edit(r).change_tags(change.clone())),
    )
}

/// `tags.selected`: what the tag editor starts from.
fn selected(app: &App, vocabulary: &Vocabulary, a: PaneArgs) -> anyhow::Result<serde_json::Value> {
    // Nothing selected is no tags; an entry named that is not, an error.
    let refs = match app.targets(a.pane, a.entries) {
        Err(error) if skwir_core::command::is_nothing_selected(&error) => Vec::new(),
        refs => refs?,
    };
    // Keyed by the lowercase form, in the order first met, with the color
    // a file gives the tag (a Finder tag's), if one does.
    let mut counts: Vec<(String, usize, Option<String>)> = Vec::new();
    for r in &refs {
        for tag in notes_of(app, &r.path).tags {
            match counts.iter_mut().find(|(t, _, _)| same_tag(t, &tag.name)) {
                Some((_, n, color)) => {
                    *n += 1;
                    if color.is_none() {
                        *color = tag.color;
                    }
                }
                None => counts.push((tag.name, 1, tag.color)),
            }
        }
    }
    let tags: Vec<serde_json::Value> = counts
        .into_iter()
        .map(|(name, count, color)| {
            let mut tag = serde_json::json!({ "name": name, "count": count });
            if let Some(color) = color {
                tag["color"] = color.into();
            }
            tag
        })
        .collect();
    Ok(serde_json::json!({
        "items": refs.len(),
        "tags": tags,
        "known": known(sidebar::pinned(app), vocabulary.known()),
    }))
}

/// The tags to offer: those pinned first, in their order, then the rest
/// known, the most recently used first.
fn known(pinned: Vec<String>, known: Vec<String>) -> Vec<String> {
    let mut out = pinned;
    for tag in known {
        if !out.iter().any(|t| same_tag(t, &tag)) {
            out.push(tag);
        }
    }
    out
}

/// The comment of the pane's one selected entry, from what hydration read.
fn remembered_comment(app: &App, pane: PaneId) -> Option<String> {
    let ws = app.workspace.read();
    let p = ws.pane(pane)?;
    let mut ids = p.selection.iter();
    let id = ids.next()?;
    if ids.next().is_some() {
        return None;
    }
    let comment = p.entry(id)?.metadata.value(keys::COMMENT)?;
    comment.as_str().map(str::to_owned)
}

fn ask_comment(text: Option<&str>) -> Invocation {
    let mut args = serde_json::json!({
        "title": "Comment",
        "accept": "Save",
        "placeholder": "A comment on the selection (empty removes it)",
        "command": "notes.comment",
        "field": "text",
    });
    if let Some(text) = text {
        args["text"] = text.into();
    }
    Invocation::with_args("ui.ask", &args)
}

/// An item of the files' menu: a file's notes; a place (a tag's) has its
/// own menu.
fn menu_item(title: &str, command: Invocation, order: i32, icon: &str) -> MenuItem {
    MenuItem::new("context", title, icon, order, command).when("selection !place")
}

/// The lane of `refs`' files where they share one (ADR-077), else the
/// shared one: what reads them all in one job runs there.
fn one_lane(refs: &[EntryRef]) -> skwir_core::scheduler::Lane {
    match reach::by_lane(refs, |r| &r.path).as_slice() {
        [(lane, _)] => *lane,
        _ => skwir_core::scheduler::Lane::SHARED,
    }
}

/// Record a tag's color (`None`: the one its name gives it) in the
/// settings: on a worker from the window's thread, else before answering
/// (a script's, an agent's or a `skwir cmd` without a window, which ends
/// once answered).
fn set_color(app: &App, lifetime: &CancelToken, sidebar: &Sidebar, tag: &str, color: Option<&str>) {
    let (tag, color, weak) = (tag.to_lowercase(), color.map(str::to_owned), app.weak());
    let sidebar = sidebar.clone();
    let save = move || {
        let Some(app) = weak.upgrade() else { return };
        let mut colors: toml::Table = app.settings.get(TAG_COLOR_SETTING).unwrap_or_default();
        match color {
            Some(color) => colors.insert(tag, toml::Value::String(color)),
            None => colors.remove(&tag),
        };
        app.settings
            .set(TAG_COLOR_SETTING, toml::Value::Table(colors));
        if let Err(error) = app.settings.save() {
            app.notify(
                skwir_core::app::NotifyLevel::Error,
                format!("The tag's color was not saved: {error:#}"),
            );
        }
        // The sidebar's pill wears it at once.
        sidebar.publish(&app);
        app.wake();
    };
    if skwir_core::scheduler::on_window_thread() {
        app.scheduler
            .spawn(Priority::Normal, "tags.color", lifetime.child(), save);
    } else {
        save();
    }
}

/// The tags plugin.
#[derive(Default)]
pub struct TagsPlugin {
    /// Ends with the plugin's registration: the vocabulary's reads and
    /// writes, a color being saved.
    lifetime: Mutex<Option<CancelToken>>,
    /// What the sidebar shows is made of, while registered.
    sidebar: Mutex<Option<Sidebar>>,
}

impl std::fmt::Debug for TagsPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TagsPlugin").finish_non_exhaustive()
    }
}

/// The index, the vocabulary and what the sidebar shows of them.
#[derive(Clone)]
pub(crate) struct Sidebar {
    index: Arc<index::TagIndex>,
    vocabulary: Arc<Vocabulary>,
    shown: Arc<sidebar::Shown>,
    /// One edit of the pins at a time.
    pub(crate) saving: Arc<Mutex<()>>,
    /// The tag locations open, which list what the index knows.
    places: Arc<location::Places>,
}

impl Sidebar {
    pub(crate) fn publish(&self, app: &App) {
        sidebar::publish(app, &self.index, &self.vocabulary, &self.shown);
        self.places.touch();
    }
}

/// A command's root, as a path.
fn root_of(root: Option<PathArg>) -> Option<std::path::PathBuf> {
    root.map(std::path::PathBuf::from)
}

impl Plugin for TagsPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Tags"
    }
    fn description(&self) -> &'static str {
        "Tags, ratings and comments kept in the files' extended attributes, no database; where downloads came from"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
    }
    fn on_disable(&self, _app: &App) {
        if let Some(lifetime) = self.lifetime.lock().take() {
            lifetime.cancel();
        }
        if let Some(state) = self.sidebar.lock().take() {
            state.shown.forget();
        }
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|k| k == index::LIMIT_SETTING) {
            let state = self.sidebar.lock().clone();
            if let Some(state) = state {
                state.index.set_limit(
                    app.settings
                        .get::<i64>(index::LIMIT_SETTING)
                        .unwrap_or(index::LIMIT_DEFAULT),
                );
                state.publish(app);
            }
        }
        if !keys
            .iter()
            .any(|k| k == sidebar::SETTING || k == sidebar::PINNED || k == TAG_COLOR_SETTING)
        {
            return;
        }
        let state = self.sidebar.lock().clone();
        if let Some(state) = state {
            state.publish(app);
        }
    }
    #[allow(
        clippy::too_many_lines,
        reason = "one registration, read top to bottom"
    )]
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[
            Capability::Read,
            Capability::Write,
            Capability::Programs,
        ])
    }
    fn register(&self, app: &App) {
        let lifetime = app.lifetime().child();
        if let Some(previous) = self.lifetime.lock().replace(lifetime.clone()) {
            previous.cancel();
        }
        let vocabulary = Vocabulary::new(Some(app.cache.namespace(ID)), app, lifetime.clone());
        app.settings.declare(
            SettingSpec::integer(
                index::LIMIT_SETTING,
                "Room for tagged files",
                "How much memory skwir may give what it knows of tagged files, about sixteen thousand files a megabyte, for tags browsed as folders, the sidebar's suggestions and quicker tag searches. Past it, new tagged files are left out of the tags' folders, and skwir says so.",
                index::LIMIT_DEFAULT,
                (8, 4096),
            )
            .unit("MB")
            .page(skwir_core::settings::page::NOTES)
            .section("Tags"),
        );
        let index = index::TagIndex::new(Some(app.cache.namespace(ID)), app, lifetime.clone());
        app.metadata.register(
            ID,
            Arc::new(attrs::AttributesProvider {
                stores: Arc::clone(&app.notes),
                vocabulary: Arc::clone(&vocabulary),
                index: Some(Arc::clone(&index)),
            }),
        );
        for store in stores() {
            app.register_notes_store(ID, store);
        }
        app.companions
            .register(ID, Arc::new(appledouble::AsideCompanion));
        // The tags as places to browse (ADR-032): `tags:` locations.
        let places = location::Places::new(Arc::clone(&index), app, lifetime.clone());
        app.register_provider(
            ID,
            Arc::new(location::Provider {
                places: Arc::clone(&places),
            }),
        );
        // A folder's tags, a button at the end of its address: lit while
        // they are shown, it goes back to the folder, as Alt+G does.
        app.registry.aspects.add(
            ID,
            skwir_core::registry::Aspect {
                scheme: location::SCHEME.into(),
                icon: "tag".into(),
                command: Invocation::with_args(
                    "tags.browse",
                    &serde_json::json!({ "toggle": true }),
                ),
                open_title: "Tags of this folder".into(),
                close_title: "Back to the folder".into(),
            },
        );
        app.registry.keepers.add(
            ID,
            skwir_core::registry::Keeper {
                scheme: location::SCHEME.into(),
                save: Invocation::new("tags.keep"),
                save_title: "Pin this tag to the sidebar".into(),
                forget_title: "Unpin this tag from the sidebar".into(),
                keeps: Some(browse::keepable),
            },
        );
        app.registry.menu_sources.add(
            ID,
            skwir_core::registry::MenuSource {
                menu: "context".into(),
                when: Some("place".into()),
                command: Invocation::new("tags.place_menu"),
                replaces: Vec::new(),
            },
        );
        // A tag's one menu wherever else it is shown: its pill in the
        // sidebar (asked with the place's location), its chip in Properties
        // (with its name).
        for menu in ["place", "tag"] {
            app.registry.menu_sources.add(
                ID,
                skwir_core::registry::MenuSource {
                    menu: menu.into(),
                    when: None,
                    command: Invocation::new("tags.tag_menu"),
                    replaces: Vec::new(),
                },
            );
        }
        // An edit applied is learned by the index at once: a file edited
        // while no pane shows it (a tag renamed everywhere) is not read
        // again soon; a file moved, renamed or trashed takes its tags along
        // in it. The tag locations follow the index.
        {
            let (weak, index, lifetime) = (app.weak(), Arc::clone(&index), lifetime.clone());
            let places = Arc::clone(&places);
            app.executor.observe(
                ID,
                Arc::new(move |changes| {
                    use skwir_core::op::Change;
                    // Files to read again (their tags changed, or they came
                    // back or were copied), and files that moved.
                    let mut paths: Vec<std::path::PathBuf> = Vec::new();
                    let mut moves: Vec<(std::path::PathBuf, Option<std::path::PathBuf>)> =
                        Vec::new();
                    for c in changes {
                        match c {
                            Change::Metadata { path, field, .. } if field == keys::TAGS => {
                                paths.push(path.clone());
                            }
                            Change::Restore { path } | Change::Copy { to: path, .. } => {
                                paths.push(path.clone());
                            }
                            Change::Rename { from, to } | Change::Move { from, to } => {
                                moves.push((from.clone(), Some(to.clone())));
                            }
                            Change::Trash { path }
                            | Change::Delete { path }
                            | Change::Remove { path } => moves.push((path.clone(), None)),
                            Change::Swap { a, b } => paths.extend([a.clone(), b.clone()]),
                            _ => {}
                        }
                    }
                    // Read where each is now: a file whose name holds its
                    // tags was renamed after its edit.
                    if !moves.is_empty() {
                        let follow = skwir_core::op::Follow::new(changes);
                        paths = paths.iter().filter_map(|p| follow.through(p).at).collect();
                    }
                    // What moved in from where no count covered is read
                    // (counted, a folder) as what is copied: the index
                    // carries along only the files it knew.
                    for (from, to) in &moves {
                        if let Some(to) = to
                            && index.covering(from).is_none()
                            && index.covering(to).is_some()
                            && !paths.contains(to)
                        {
                            paths.push(to.clone());
                        }
                    }
                    places.changed(
                        &paths
                            .iter()
                            .chain(
                                moves.iter().flat_map(|(from, to)| {
                                    std::iter::once(from).chain(to.as_ref())
                                }),
                            )
                            .map(std::path::PathBuf::as_path)
                            .collect::<Vec<_>>(),
                    );
                    if !moves.is_empty() {
                        index.moved(&moves);
                        // The tag locations shown follow at once, not when
                        // the index is next written.
                        places.touch();
                    }
                    let Some(app) = weak.upgrade().filter(|_| !paths.is_empty()) else {
                        return;
                    };
                    // Each remote's files are read in its lane: a plan's
                    // files may be on several (ADR-077).
                    for (lane, paths) in reach::by_lane(paths, |p| p) {
                        let (index, weak, places) =
                            (Arc::clone(&index), weak.clone(), Arc::clone(&places));
                        let job = lifetime.child();
                        app.scheduler.spawn_in(
                            lane,
                            Priority::Background,
                            "tags.learn",
                            job.clone(),
                            move || {
                                let Some(app) = weak.upgrade() else { return };
                                let learned = paths
                                    .iter()
                                    .map(|p| {
                                        let tags = notes_of(&app, p).tag_names();
                                        (p.clone(), tags)
                                    })
                                    .collect();
                                index.learn(learned);
                                // A folder restored, copied or moved in brings
                                // its files: the trash made the index forget
                                // them, and their change times did not move,
                                // so no check would read them again. They are
                                // counted.
                                for folder in paths.iter().filter(|p| {
                                    std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir())
                                }) {
                                    if let Err(error) =
                                        crate::count::run(&app, &index, folder, None, &job)
                                    {
                                        tracing::debug!("tags: a folder brought back: {error:#}");
                                    }
                                }
                                places.touch();
                            },
                        );
                    }
                }),
            );
        }
        // The sidebar's tags, from the index: again whenever it changes,
        // and first on a worker, which reads it from the cache.
        let state = Sidebar {
            index: Arc::clone(&index),
            vocabulary: Arc::clone(&vocabulary),
            shown: Arc::new(sidebar::Shown::default()),
            saving: Arc::new(Mutex::new(())),
            places: Arc::clone(&places),
        };
        {
            let (weak, state) = (app.weak(), state.clone());
            index.on_change(Box::new(move |_| {
                if let Some(app) = weak.upgrade() {
                    state.publish(&app);
                }
            }));
        }
        {
            let (weak, state) = (app.weak(), state.clone());
            app.scheduler.spawn(
                Priority::Background,
                "tags.sidebar",
                lifetime.child(),
                move || {
                    if let Some(app) = weak.upgrade() {
                        state.publish(&app);
                    }
                },
            );
        }
        *self.sidebar.lock() = Some(state.clone());
        let sidebar = state;
        app.settings.declare(
            SettingSpec::integer(
                sidebar::SETTING,
                "Tags suggested in the sidebar",
                "Until a tag is pinned, how many the sidebar suggests, those on the most files first; 0 suggests none.",
                sidebar::DEFAULT,
                (0, 50),
            )
            .page(skwir_core::settings::page::NOTES)
            .section("Tags"),
        );
        app.settings.declare(SettingSpec::data(
            sidebar::PINNED,
            "The tags pinned to the sidebar, in their order",
            toml::Value::Array(Vec::new()),
        ));
        app.settings.declare(SettingSpec::data(
            skwir_core::metadata::TAG_EXCLUSIVE_SETTING,
            "The categories a file has one tag of at a time: fields, whose values are the tags within them",
            toml::Value::Array(Vec::new()),
        ));
        {
            let fields = sidebar.clone();
            app.commands.register::<ExclusiveArgs, _>(
                ID,
                "tags.exclusive",
                "One tag of a category at a time",
                "Make a category (`tag`) hold one tag per file at a time, a field whose values are its tags (`on`), or several again",
                move |a, app| {
                    let tag = skwir_core::metadata::clean_tag(&a.tag);
                    anyhow::ensure!(!tag.is_empty(), "which category?");
                    fields::set(app, &fields.saving, &tag, a.on, a.undo);
                    Ok(Outcome::Done)
                },
            );
        }
        for (name, title, description) in [
            (
                "tags.pin",
                "Pin tags",
                "Keep tags (`tag`, `tags`) in the sidebar, before the pinned tag `before` or last; moves those pinned",
            ),
            (
                "tags.unpin",
                "Unpin tags",
                "Take tags (`tag`, `tags`) out of the sidebar; their files keep them",
            ),
            (
                "tags.pins",
                "Set pinned tags",
                "Pin exactly `tags`, in their order: what an Undo of a pin runs",
            ),
        ] {
            let (pins, lifetime) = (sidebar.clone(), lifetime.clone());
            app.commands
                .register::<PinArgs, _>(ID, name, title, description, move |a, app| {
                    let tags = everywhere::named(a.tag, a.tags);
                    let edit = match name {
                        "tags.pins" => sidebar::Pin::Set(tags),
                        _ if tags.is_empty() => anyhow::bail!("which tag?"),
                        "tags.unpin" => sidebar::Pin::Unpin { tags },
                        _ => sidebar::Pin::Put {
                            tags,
                            before: a.before,
                        },
                    };
                    let publish = pins.clone();
                    sidebar::edit(app, &pins.saving, edit, a.undo, &lifetime, move |app| {
                        publish.publish(app);
                    });
                    Ok(Outcome::Done)
                });
        }
        // What a `tag:` search asks first (ADR-026).
        app.hints.register(ID, Arc::new(hints::Baloo::default()));
        #[cfg(target_os = "macos")]
        app.hints.register(ID, Arc::new(hints::Spotlight));
        app.hints.register(ID, Arc::new(hints::Learned(index)));
        app.settings.declare(SettingSpec::data(
            TAG_COLOR_SETTING,
            "Colors the user chose for tags, by tag in lowercase: { tag = color }",
            toml::Value::Table(toml::Table::new()),
        ));

        let v = Arc::clone(&vocabulary);
        app.commands
            .register::<TagsArgs, _>(
                ID,
                "tags.set",
                "Set tags",
                "Replace the tags of the entries given, or the selection's (plan)",
                move |a, app| {
                    let tags = parse_tags(&a.tags.join(","));
                    v.used(&tags);
                    let title = if tags.is_empty() {
                        "remove tags".to_owned()
                    } else {
                        format!("set tags {}", tags.join(", "))
                    };
                    // A file that has them already is skipped when applied.
                    per_entry(app.targets(a.pane, a.entries)?, title, |r| {
                        Some(app.notes.edit(r).tags(tags.clone()))
                    })
                },
            )
            .plans();
        let v = Arc::clone(&vocabulary);
        app.commands
            .register::<TagsArgs, _>(
                ID,
                "tags.add",
                "Add tags",
                "Add tags to the entries given, or the selection (plan)",
                move |a, app| {
                    edit_plan(
                        app,
                        &v,
                        EditArgs {
                            add: a.tags,
                            remove: Vec::new(),
                            entries: a.entries,
                            pane: a.pane,
                        },
                    )
                },
            )
            .plans();
        let v = Arc::clone(&vocabulary);
        app.commands
            .register::<TagsArgs, _>(
                ID,
                "tags.remove",
                "Remove tags",
                "Remove tags from the entries given, or the selection (plan)",
                move |a, app| {
                    edit_plan(
                        app,
                        &v,
                        EditArgs {
                            add: Vec::new(),
                            remove: a.tags,
                            entries: a.entries,
                            pane: a.pane,
                        },
                    )
                },
            )
            .plans();
        let v = Arc::clone(&vocabulary);
        app.commands.register::<EditArgs, _>(
            ID,
            "tags.edit",
            "Edit tags",
            "Add some tags to the entries given, or the selection, and remove others, as one plan: each entry keeps its other tags",
            move |a, app| edit_plan(app, &v, a),
        )
        .plans();
        let v = Arc::clone(&vocabulary);
        app.commands.register::<ToggleArgs, _>(
            ID,
            "tags.toggle",
            "Toggle a tag",
            "Give the entries given, or the selection, a tag, or take it from every entry when all of them have it: a command to bind to a key (plan)",
            move |a, app| {
                let Some(tag) = parse_tags(&a.tag).pop() else {
                    anyhow::bail!("which tag?");
                };
                let refs = app.targets(a.pane, a.entries)?;
                let lane = one_lane(&refs);
                // What the pane knows of each first: a key's toggle reads
                // a file only where its listing has not.
                let pane = app.pane_or_active(a.pane);
                let known: Vec<Option<bool>> = {
                    let ws = app.workspace.read();
                    refs.iter()
                        .map(|r| {
                            let e = ws.pane(pane)?.entry_at(r.id, &r.path)?;
                            e.metadata.is_hydrated().then(|| has_tag(e, &tag))
                        })
                        .collect()
                };
                let unread = known.iter().any(Option::is_none);
                let v = Arc::clone(&v);
                let toggle = move |app: &App| {
                    let all = refs.iter().zip(known).all(|(r, known)| {
                        known.unwrap_or_else(|| notes_of(app, &r.path).tag(&tag).is_some())
                    });
                    let (add, remove) = if all {
                        (Vec::new(), vec![tag])
                    } else {
                        (vec![tag], Vec::new())
                    };
                    edit_plan(
                        app,
                        &v,
                        EditArgs {
                            add,
                            remove,
                            entries: refs.into_iter().map(Into::into).collect(),
                            pane: a.pane,
                        },
                    )
                };
                if !unread || !skwir_core::scheduler::on_window_thread() {
                    return toggle(app);
                }
                // Files to read, from the window's thread: on a worker in
                // their lane, the plan then applied as the window applies
                // a user's own.
                let weak = app.weak();
                app.scheduler.spawn_in(
                    lane,
                    Priority::Visible,
                    "tags.toggle",
                    app.lifetime().child(),
                    move || {
                        let Some(app) = weak.upgrade() else { return };
                        match toggle(&app) {
                            Ok(Outcome::Plan(plan)) => app.apply_whole(plan),
                            Ok(_) => {}
                            Err(error) => app.notify(skwir_core::app::NotifyLevel::Error, format!("{error:#}")),
                        }
                    },
                );
                Ok(Outcome::Done)
            },
        )
        .plans();
        // Its forms, one per pinned tag (`sidebar::toggle_forms`): set here
        // as the pins stand, since the sidebar's first publish, on a
        // worker, may run before the command is registered.
        {
            let pinned = sidebar::pinned(app);
            let pinned: Vec<&str> = pinned.iter().map(String::as_str).collect();
            app.commands
                .set_forms(ID, "tags.toggle", sidebar::toggle_forms(&pinned));
        }
        let v = Arc::clone(&vocabulary);
        app.commands.register::<PaneArgs, _>(
            ID,
            "tags.selected",
            "Tags of the selection",
            "`{ items, tags: [{ name, count, color? }], known }`: the selection's tags, read from its files, with how many carry each and the color a file gives one (a Finder tag's), and the tags known, those pinned first, then the most recently used",
            move |a, app| Ok(Outcome::Value(selected(app, &v, a)?)),
        )
        .data();
        // A tag's pin and color follow it renamed on every file.
        let follows = follow::Ctx::new(app, sidebar.clone());
        follow::register(app, &follows);
        let (v, everywhere, f) = (Arc::clone(&vocabulary), lifetime.clone(), follows.clone());
        app.commands.register::<RenameArgs, _>(
            ID,
            "tags.rename",
            "Rename tag everywhere",
            "Rename a tag (or merge `tags`) on every file under the home folder (or `root`) that has it, merging it with a tag of the new name where a file has both: the files are searched on a worker and the plan proposed for review",
            move |a, app| {
                let from = everywhere::named(a.tag, a.tags);
                everywhere::start(app, &everywhere, &f, from, Some(&a.to), root_of(a.root))?;
                v.used(&parse_tags(&a.to));
                Ok(Outcome::Done)
            },
        )
        .plans();
        let (everywhere, f) = (lifetime.clone(), follows.clone());
        app.commands.register::<MoveArgs, _>(
            ID,
            "tags.move",
            "Move tag into category",
            "Put a tag (or `tags`) into the category `into`, keeping its own name (`Paris` into `Places/France` is `Places/France/Paris`), or out of its category without `into`, on every file under the home folder (or `root`): the files are searched on a worker and the plan proposed for review",
            move |a, app| {
                let tags = everywhere::named(a.tag, a.tags);
                everywhere::move_into(
                    app,
                    &everywhere,
                    &f,
                    tags,
                    a.into.as_deref(),
                    root_of(a.root),
                )?;
                Ok(Outcome::Done)
            },
        )
        .plans();
        let (everywhere, f) = (lifetime.clone(), follows.clone());
        app.commands.register::<DeleteArgs, _>(
            ID,
            "tags.delete",
            "Remove tag everywhere",
            "Take a tag (or `tags`) from every file under the home folder (or `root`) that has it: the files are searched on a worker and the plan proposed for review",
            move |a, app| {
                let from = everywhere::named(a.tag, a.tags);
                everywhere::start(app, &everywhere, &f, from, None, root_of(a.root))?;
                Ok(Outcome::Done)
            },
        )
        .plans();
        // What gestures do in a tag location (`browse.rs`).
        let (everywhere, f) = (lifetime.clone(), follows.clone());
        app.commands.register::<browse::DropArgs, _>(
            ID,
            "tags.drop",
            "Drop on a tag",
            "Give files dropped or pasted on a tag location (`location`) its tag: a pane's selection (`pane`) or `entries`; with `move`, out of the tag location they come from (the pane's, or `from`). Tags dropped go into it, a plan to review",
            move |a, app| browse::drop(app, &everywhere, &f, a),
        )
        .plans();
        let (everywhere, f) = (lifetime.clone(), follows);
        app.commands.register::<browse::UntagArgs, _>(
            ID,
            "tags.untag",
            "Remove from tag",
            "Take the tag a tag location shows (the pane's, or `location`) off the selected files; tags selected there are taken off every file, a plan to review. What Delete does in a tag location",
            move |a, app| browse::untag(app, &everywhere, &f, &a),
        )
        .plans();
        {
            let places = Arc::clone(&places);
            app.commands.register::<browse::NewArgs, _>(
                ID,
                "tags.new",
                "New tag",
                "Make a tag `name` in the tag a tag location shows (`location`): shown there until a file is given it",
                move |a, app| browse::new(app, &places, &a),
            );
        }
        app.commands.register::<browse::NarrowArgs, _>(
            ID,
            "tags.narrow",
            "Narrow a tag location",
            "Show, in the tag location the pane shows, only the files that also have a tag (`facet` `also_tag`), none that have one (`not_tag`), of a `kind`, `modified` a year, or every file within the tag (`deep`); `on` false takes it back, omitted it switches",
            |a, app| browse::narrow(app, &a),
        )
        .shows();
        app.commands.register::<browse::OpenArgs, _>(
            ID,
            "tags.open",
            "Open a tag",
            "Show the files of the home folder that have a tag (or a tag within it), as its pinned pill does, in the pane given or the active one",
            |a, app| browse::open(app, &a),
        )
        .shows();
        app.commands.register::<browse::PillArgs, _>(
            ID,
            "tags.pill",
            "A tag, here",
            "A tag of the sidebar clicked with Shift (`how` `also`: the files shown that have it too) or Alt (`here`: its files in the folder shown)",
            |a, app| browse::pill(app, &a),
        )
        .shows();
        let browsing = app.commands.register::<browse::BrowseArgs, _>(
            ID,
            "tags.browse",
            "Tags of a folder",
            "Show every tag of the files under a folder (`in`, an absolute path) as places to browse, each a folder of its files: the folder the pane shows when omitted, or the one a tag location or a search there is of; with `toggle` and no folder, a tag location goes back to its folder instead",
            |a, app| browse::browse(app, &a),
        );
        browsing.forms([Form::new(
            serde_json::json!({ "toggle": true }),
            "Tags of this folder, or back to it",
        )]);
        browsing.shows();
        app.commands.register::<browse::PaneOnly, _>(
            ID,
            "tags.keep",
            "Pin the tag shown",
            "Pin the tag a pane's tag location shows: one tag of the home folder, not narrowed",
            |a, app| browse::keep(app, a),
        );
        {
            let places = Arc::clone(&places);
            app.commands.register::<browse::ExportArgs, _>(
                ID,
                "tags.export",
                "Copy a tag into a folder",
                "Make in `dest` a folder of the files a tag location (`location`) lists, named as its tag, with a folder inside for each tag within it: a plan proposed for review. What a tag dropped or pasted in a folder does",
                move |a, app| browse::export(app, &places, a),
            )
        .plans();
        }
        {
            let places = Arc::clone(&places);
            app.commands.register::<browse::CountedArgs, _>(
                ID,
                "tags.counted",
                "Tags counted",
                "`{ root, counting, counted, partial }`: whether the tags of the files under `root` (the home folder when omitted) are being counted for its tag locations, as of when a count covers them (`counted`, in seconds since the epoch), and whether some are left out for want of room (`partial`). Answers from memory",
                move |a, _| {
                    let root = a
                        .root
                        .map(PathArg::into_inner)
                        .or_else(std::env::home_dir)
                        .ok_or_else(|| anyhow::anyhow!("no home folder"))?;
                    Ok(Outcome::Value(places.counted(&root)))
                },
            )
        .data();
        }
        app.commands.register::<browse::TagMenuArgs, _>(
            ID,
            "tags.tag_menu",
            "A tag's menu",
            "The items of a tag's menu where it is shown: its chip in Properties (tags), its pill in the sidebar (location). Answers from memory",
            |a, app| Ok(browse::tag_menu(app, a)),
        )
        .data();
        app.commands
            .register::<browse::PaneOnly, _>(
                ID,
                "tags.files_menu",
                "Tags of the selection's menu",
                "The context menu's submenu of each tag the selected files have: its files in the folder shown, its place, its color, taking it from them",
                |a, app| Ok(browse::files_menu(app, a)),
            )
            .data();
        app.commands.register::<browse::PaneOnly, _>(
            ID,
            "tags.place_menu",
            "Tags' menu",
            "The context menu's items for the tags selected in a tag location. Answers from memory",
            |a, app| Ok(browse::place_menu(app, a)),
        )
        .data();
        let (colors, color_sidebar) = (lifetime.clone(), sidebar.clone());
        app.commands.register::<ColorArgs, _>(
            ID,
            "tags.color",
            "Tag color",
            "Choose the color a tag wears everywhere, or give it back the one its name gives it",
            move |a, app| {
                let tag = a.tag.trim();
                anyhow::ensure!(!tag.is_empty(), "which tag?");
                let color = a.color.as_deref().filter(|c| !c.is_empty());
                if let Some(color) = color {
                    anyhow::ensure!(
                        TAG_COLORS.contains(&color),
                        "a tag's color is one of {}",
                        TAG_COLORS.join(", ")
                    );
                }
                set_color(app, &colors, &color_sidebar, tag, color);
                Ok(Outcome::Done)
            },
        );
        app.commands
            .register::<PaneArgs, _>(
                ID,
                "tags.menu",
                "Comment menu",
                "The context menu's Comment…, holding the selected entry's comment",
                |a, app| {
                    let pane = app.pane_or_active(a.pane);
                    let items = remembered_comment(app, pane).map(|comment| MenuItem {
                        submenu: Some("Notes".into()),
                        ..menu_item("Comment…", ask_comment(Some(&comment)), 560, "comment")
                    });
                    Ok(Outcome::Value(
                        serde_json::json!({ "items": items.into_iter().collect::<Vec<_>>() }),
                    ))
                },
            )
            .data();
        app.commands.register::<ListArgs, _>(
            ID,
            "tags.list",
            "List tags",
            "Tags used in the pane's listing, with counts (tags that differ only in case count as one); with `read`, read from the files whose tags the listing has not read yet too",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                // Keyed by the lowercase form, shown as first spelled.
                let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
                let mut count = |tags: Vec<String>| {
                    for t in tags {
                        counts.entry(t.to_lowercase()).or_insert((t, 0)).1 += 1;
                    }
                };
                // What the listing read, and the files it has not, taken
                // under the lock and read after it.
                let mut unread = Vec::new();
                let withheld = app.withheld(pane);
                let open = {
                    let ws = app.workspace.read();
                    let Some(p) = ws.pane(pane) else {
                        anyhow::bail!("no such pane");
                    };
                    for e in p.listing.iter().filter(|e| !withheld.contains(&e.id)) {
                        match tags_of(&e.metadata) {
                            Some(tags) => count(tags),
                            None if a.read && unread.len() < READ_AT_MOST => unread.push(e.id),
                            None => {}
                        }
                    }
                    p.open.clone()
                };
                if let Some(open) = open {
                    for id in unread {
                        if let Some(path) = open.path_of(id) {
                            count(notes_of(app, &path).tag_names());
                        }
                    }
                }
                let counts: BTreeMap<String, usize> = counts.into_values().collect();
                Ok(Outcome::Value(serde_json::to_value(counts)?))
            },
        )
        .data();
        app.registry.keymap.add(
            ID,
            KeyBinding {
                // Ctrl+Shift+T reopens a closed tab, as in file managers.
                keys: "ctrl-shift-g".into(),
                command: Invocation::new("ui.tags_prompt"),
                when: Some("pane selection !place".into()),
            },
        );
        app.registry.keymap.add(
            ID,
            KeyBinding {
                // From a folder or a search there: its tags; from its tags,
                // back to it, as the address's Tags button does.
                keys: "alt-g".into(),
                command: Invocation::with_args(
                    "tags.browse",
                    &serde_json::json!({ "toggle": true }),
                ),
                when: Some("pane".into()),
            },
        );
        app.registry.menus.add(
            ID,
            menu_item("Tags…", Invocation::new("ui.tags_prompt"), 550, "tag"),
        );
        // The folder's own menu, beside Search here and the Usage view.
        app.registry.menus.add(
            ID,
            MenuItem::new(
                "context",
                "Browse tags here",
                "tag",
                425,
                Invocation::new("tags.browse"),
            )
            .when("empty folder"),
        );
        // Rarely set from a menu: grouped, the menu expands them in place.
        for stars in (0..=5u8).rev() {
            let title = match stars {
                0 => "No rating".to_owned(),
                1 => "1 star".to_owned(),
                n => format!("{n} stars"),
            };
            app.registry.menus.add(
                ID,
                MenuItem {
                    submenu: Some("Notes".into()),
                    ..menu_item(
                        &title,
                        Invocation::with_args("notes.rate", &serde_json::json!({ "stars": stars })),
                        551 + i32::from(5 - stars),
                        if stars == 0 { "minus" } else { "star" },
                    )
                },
            );
        }
        app.registry.menus.add(
            ID,
            MenuItem {
                submenu: Some("Notes".into()),
                ..menu_item("Comment…", ask_comment(None), 560, "comment")
            },
        );
        // A submenu for each tag of the selected files: what a right press
        // on its chip offers, where the keys reach it.
        app.registry.menu_sources.add(
            ID,
            MenuSource {
                menu: "context".into(),
                when: Some("selection".into()),
                command: Invocation::new("tags.files_menu"),
                replaces: Vec::new(),
            },
        );
        // The comment already there, to edit rather than retype.
        app.registry.menu_sources.add(
            ID,
            MenuSource {
                menu: "context".into(),
                when: Some("selection single".into()),
                command: Invocation::new("tags.menu"),
                replaces: vec![ask_comment(None)],
            },
        );
    }
}

#[cfg(test)]
// Helpers of the tests that run on Unix alone (ADR-035).
#[cfg_attr(not(unix), allow(dead_code, unused_imports))]
mod tests {
    use super::*;
    use skwir_core::Value;
    use skwir_core::app::AppConfig;
    use skwir_core::location::Location;
    use std::path::Path;
    use std::time::Duration;

    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("tags-")
            .tempdir_in(base)
            .unwrap()
    }

    fn hydrate_all(app: &App, pane: PaneId) {
        let ids: Vec<_> = app
            .workspace
            .read()
            .pane(pane)
            .unwrap()
            .listing
            .ids()
            .collect();
        app.dispatch(&Invocation::with_args(
            "hydrate",
            &serde_json::json!({ "ids": ids, "pane": pane }),
        ))
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.pump();
            if app
                .workspace
                .read()
                .pane(pane)
                .unwrap()
                .listing
                .iter()
                .all(|e| e.metadata.is_hydrated())
            {
                return;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn plan(app: &App, name: &str, args: &serde_json::Value) -> Plan {
        match app.dispatch(&Invocation::with_args(name, args)).unwrap() {
            Outcome::Plan(plan) => plan,
            _ => panic!("{name} gives a plan"),
        }
    }

    /// An app with the plugin, showing `dir`, every entry hydrated, and
    /// `names` selected.
    fn app_in(tmp: &Path, dir: &Path, names: &[&str]) -> (Arc<App>, PaneId) {
        let app = App::new(AppConfig::ephemeral(tmp)).unwrap();
        app.plugins.add(Arc::new(TagsPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(dir)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        hydrate_all(&app, pane);
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": names }),
        ))
        .unwrap();
        (app, pane)
    }

    /// A Mac's tags on `f` under the attribute `name`, framed as found.
    fn finder_tags(f: &Path, name: &str, framing: finder::Framing, tags: &[(&str, u8)]) {
        let tags: Vec<finder::Tag> = tags
            .iter()
            .map(|(name, color)| finder::Tag {
                name: (*name).into(),
                color: *color,
            })
            .collect();
        skwir_core::fs::xattr::set(f, name, &finder::encode(&tags, framing)).unwrap();
    }

    fn finder_of(f: &Path, name: &str, framing: finder::Framing) -> Vec<(String, u8)> {
        let bytes = skwir_core::fs::xattr::get(f, name)
            .unwrap()
            .expect("the Finder attribute");
        finder::parse(&bytes, framing)
            .unwrap()
            .into_iter()
            .map(|t| (t.name, t.color))
            .collect()
    }

    /// The items of `menu` (a source's answer) by submenu, their titles
    /// in order.
    fn submenus(answer: &serde_json::Value) -> Vec<(String, Vec<String>)> {
        let mut items: Vec<MenuItem> = serde_json::from_value(answer["items"].clone()).unwrap();
        items.sort_by_key(|item| item.order);
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for item in items {
            let submenu = item.submenu.unwrap_or_default();
            match out.iter_mut().find(|(name, _)| *name == submenu) {
                Some((_, titles)) => titles.push(item.title),
                None => out.push((submenu, vec![item.title])),
            }
        }
        out
    }

    #[test]
    fn each_pinned_tag_is_a_command_to_bind_to_a_key() {
        let tmp = home_tempdir();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(TagsPlugin::default()), &app);
        let forms = |app: &App| -> Vec<(String, serde_json::Value)> {
            app.commands
                .list()
                .into_iter()
                .find(|spec| &*spec.name == "tags.toggle")
                .expect("tags.toggle")
                .forms
                .into_iter()
                .map(|form| (form.title, form.args))
                .collect()
        };
        assert!(forms(&app).is_empty(), "nothing pinned");
        app.dispatch(&Invocation::with_args(
            "tags.pins",
            &serde_json::json!({ "tags": ["print", "Places/Paris"] }),
        ))
        .unwrap();
        assert_eq!(
            forms(&app),
            [
                (
                    "Toggle tag “print”".to_owned(),
                    serde_json::json!({ "tag": "print" })
                ),
                (
                    "Toggle tag “Places/Paris”".to_owned(),
                    serde_json::json!({ "tag": "Places/Paris" })
                ),
            ]
        );
        app.dispatch(&Invocation::with_args(
            "tags.unpin",
            &serde_json::json!({ "tag": "print" }),
        ))
        .unwrap();
        assert_eq!(forms(&app).len(), 1, "an unpinned tag's goes");
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn the_context_menu_gives_each_tag_of_the_selection_a_submenu() {
        let tmp = home_tempdir();
        let dir = tmp.path().join("it's tagged");
        std::fs::create_dir(&dir).unwrap();
        for (name, tags) in [
            ("a.txt", &["zeta", "alpha"][..]),
            ("b.txt", &["Places/Paris", "pinned"][..]),
            ("c.txt", &["Other/Paris", "beta"][..]),
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
            write_tags(
                &dir.join(name),
                &tags.iter().map(|t| (*t).to_owned()).collect::<Vec<_>>(),
            )
            .unwrap();
        }
        let (app, pane) = app_in(tmp.path(), &dir, &["a.txt", "b.txt"]);
        app.settings.set(
            crate::sidebar::PINNED,
            toml::Value::Array(vec![toml::Value::String("pinned".into())]),
        );
        let menu = |pane| {
            let Outcome::Value(v) = app
                .dispatch(&Invocation::with_args(
                    "tags.files_menu",
                    &serde_json::json!({ "pane": pane }),
                ))
                .unwrap()
            else {
                panic!("an answer")
            };
            submenus(&v)
        };
        let names: Vec<String> = menu(pane).into_iter().map(|(name, _)| name).collect();
        assert_eq!(
            names,
            ["Tag “pinned”", "Tag “alpha”", "Tag “Paris”", "Tag “zeta”"],
            "the pinned first, then by name"
        );
        let (_, alpha) = &menu(pane)[1];
        assert_eq!(
            alpha[..3],
            ["Show Its Files Here", "Open Tag", "Color: From Its Name"]
        );
        assert_eq!(alpha.last().unwrap(), "Remove From Selected Files");
        // Two tags of one name in two categories say their categories.
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["b.txt", "c.txt"] }),
        ))
        .unwrap();
        let names: Vec<String> = menu(pane).into_iter().map(|(name, _)| name).collect();
        assert_eq!(
            names,
            [
                "Tag “pinned”",
                "Tag “beta”",
                "Tag “Other/Paris”",
                "Tag “Places/Paris”"
            ]
        );
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn finder_tags_are_read_with_ours_and_edited_with_them() {
        const PLAIN: &str = "user.com.apple.metadata:_kMDItemUserTags";
        const SAMBA: &str = "user.DosStream.com.apple.metadata\u{F022}_kMDItemUserTags:$DATA";
        let tmp = home_tempdir();
        let dir = tmp.path().join("from a Mac");
        std::fs::create_dir(&dir).unwrap();
        let copied = dir.join("it's copied.txt");
        let shared = dir.join("shared.txt");
        std::fs::write(&copied, b"x").unwrap();
        std::fs::write(&shared, b"x").unwrap();
        write_tags(&copied, &["work".into(), "home".into()]).unwrap();
        finder_tags(
            &copied,
            PLAIN,
            finder::Framing::Plain,
            &[("Rouge", 6), ("WORK", 0), ("Projet", 4)],
        );
        finder_tags(&shared, SAMBA, finder::Framing::Stream, &[("Gris", 1)]);
        assert_eq!(read_tags(&copied), ["work", "home", "Rouge", "Projet"]);
        assert_eq!(read_tags(&shared), ["Gris"]);

        // The provider lists them as one, with the colors a Mac gave.
        let (app, pane) = app_in(tmp.path(), &dir, &["it's copied.txt", "shared.txt"]);
        {
            let ws = app.workspace.read();
            let listing = &ws.pane(pane).unwrap().listing;
            let e = listing.find_by_name("it's copied.txt".as_ref()).unwrap();
            assert_eq!(
                tags_of(&e.metadata).unwrap(),
                ["work", "home", "Rouge", "Projet"]
            );
            let text = |v: &Value| v.as_str().unwrap().to_owned();
            let Some(Value::List(colors)) = e.metadata.value(keys::TAG_COLORS) else {
                panic!("the Finder colors")
            };
            let colors: Vec<(String, String)> = colors
                .iter()
                .map(|pair| match pair {
                    Value::List(p) => (text(&p[0]), text(&p[1])),
                    _ => panic!(),
                })
                .collect();
            assert_eq!(
                colors,
                [
                    ("Rouge".into(), "red".into()),
                    ("Projet".into(), "blue".into())
                ]
            );
            let e = listing.find_by_name("shared.txt".as_ref()).unwrap();
            assert_eq!(tags_of(&e.metadata).unwrap(), ["Gris"]);
            assert!(
                e.metadata.value(keys::TAG_COLORS).is_none(),
                "gray has no color of ours"
            );
        }
        let Outcome::Value(v) = app.dispatch(&Invocation::new("tags.selected")).unwrap() else {
            panic!()
        };
        let rouge = v["tags"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "Rouge")
            .unwrap();
        assert_eq!(rouge["color"], "red");

        // An edit holds on the Mac too: both attributes say the same, a
        // tag kept keeps its color, and Samba's framing is kept.
        let edit = plan(
            &app,
            "tags.edit",
            &serde_json::json!({ "add": ["new"], "remove": ["Rouge", "Gris"] }),
        );
        let r = app.executor.execute_blocking(&edit, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(read_tags(&copied), ["work", "home", "Projet", "new"]);
        assert_eq!(
            finder_of(&copied, PLAIN, finder::Framing::Plain),
            [
                ("work".into(), 0),
                ("home".into(), 0),
                ("Projet".into(), 4),
                ("new".into(), 0)
            ]
        );
        assert_eq!(read_tags(&shared), ["new"]);
        let raw = skwir_core::fs::xattr::get(&shared, SAMBA).unwrap().unwrap();
        assert_eq!(raw.last(), Some(&0), "Samba's NUL stays");
        assert_eq!(
            finder_of(&shared, SAMBA, finder::Framing::Stream),
            [("new".into(), 0)]
        );

        // Undo gives the removed tags back with their colors.
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&copied), ["work", "home", "Rouge", "Projet"]);
        let colors: Vec<(String, u8)> = finder_of(&copied, PLAIN, finder::Framing::Plain)
            .into_iter()
            .filter(|(_, c)| *c != 0)
            .collect();
        assert_eq!(colors, [("Rouge".into(), 6), ("Projet".into(), 4)]);
        assert_eq!(
            finder_of(&shared, SAMBA, finder::Framing::Stream),
            [("Gris".into(), 1)]
        );

        // Removing every tag removes the Mac's attribute too.
        let clear = plan(&app, "tags.set", &serde_json::json!({ "tags": [] }));
        app.executor.execute_blocking(&clear, app.lifetime());
        assert!(read_tags(&copied).is_empty() && read_tags(&shared).is_empty());
        assert!(
            skwir_core::fs::xattr::get(&copied, PLAIN)
                .unwrap()
                .is_none()
        );
        assert!(
            skwir_core::fs::xattr::get(&shared, SAMBA)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_macs_notes_beside_a_file_on_a_stick_are_read_and_edited_there() {
        use crate::appledouble::{self, AppleDouble};
        // The test's disk takes attributes: say it is a stick.
        appledouble::EVERYWHERE.store(true, std::sync::atomic::Ordering::Relaxed);
        let plist = |value: plist::Value| {
            let mut out = Vec::new();
            value.to_writer_binary(&mut out).unwrap();
            out
        };
        let tags = finder::encode(
            &[
                finder::Tag {
                    name: "Rouge".into(),
                    color: 6,
                },
                finder::Tag {
                    name: "Été".into(),
                    color: 0,
                },
            ],
            finder::Framing::Plain,
        );
        let comment = plist(plist::Value::String("For the report".into()));
        let froms = plist(plist::Value::Array(vec![
            plist::Value::String("https://example.org/it.pdf".into()),
            plist::Value::String("https://example.org/".into()),
        ]));
        let quarantine = b"0083;66e0f1a2;Safari;".as_slice();
        let tmp = home_tempdir();
        let dir = tmp.path().join("STICK");
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("it's on the stick.pdf");
        std::fs::write(&file, b"x").unwrap();
        std::fs::write(dir.join("plain.txt"), b"x").unwrap();
        std::fs::write(
            dir.join("._it's on the stick.pdf"),
            appledouble::tests::as_a_mac_writes(&[
                ("com.apple.quarantine", quarantine),
                (finder::TAGS, &tags),
                (finder::COMMENT, &comment),
                (finder::WHERE_FROMS, &froms),
            ]),
        )
        .unwrap();
        assert_eq!(read_tags(&file), ["Rouge", "Été"]);

        let (app, pane) = app_in(tmp.path(), &dir, &["it's on the stick.pdf"]);
        {
            let ws = app.workspace.read();
            let listing = &ws.pane(pane).unwrap().listing;
            let e = listing
                .find_by_name("it's on the stick.pdf".as_ref())
                .unwrap();
            assert_eq!(tags_of(&e.metadata).unwrap(), ["Rouge", "Été"]);
            let text = |key| {
                e.metadata
                    .value(key)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            };
            assert_eq!(text(keys::COMMENT).as_deref(), Some("For the report"));
            assert_eq!(
                text(keys::ORIGIN).as_deref(),
                Some("https://example.org/it.pdf")
            );
            assert_eq!(
                text(keys::REFERRER).as_deref(),
                Some("https://example.org/")
            );
            assert!(e.metadata.value(keys::TAG_COLORS).is_some(), "Rouge is red");
            let plain = listing.find_by_name("plain.txt".as_ref()).unwrap();
            assert!(tags_of(&plain.metadata).is_none());
        }

        // An edit is written where the Mac will read it, beside every
        // other attribute, a kept tag keeping its color.
        #[cfg(unix)]
        let mode = |f: &Path| {
            use std::os::unix::fs::PermissionsExt;
            let aside = appledouble::sidecar(f).unwrap();
            std::fs::metadata(aside).unwrap().permissions().mode() & 0o777
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let aside = appledouble::sidecar(&file).unwrap();
            std::fs::set_permissions(aside, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let aside = |f: &Path| {
            AppleDouble::parse(&std::fs::read(appledouble::sidecar(f).unwrap()).unwrap()).unwrap()
        };
        let edit = plan(
            &app,
            "tags.edit",
            &serde_json::json!({ "add": ["new"], "remove": ["Été"] }),
        );
        let r = app.executor.execute_blocking(&edit, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(read_tags(&file), ["Rouge", "new"]);
        let written = aside(&file);
        let theirs = finder::parse(written.get(finder::TAGS).unwrap(), finder::Framing::Plain);
        assert_eq!(
            theirs.unwrap(),
            [
                finder::Tag {
                    name: "Rouge".into(),
                    color: 6
                },
                finder::Tag {
                    name: "new".into(),
                    color: 0
                }
            ]
        );
        assert_eq!(written.get("com.apple.quarantine"), Some(quarantine));
        assert_eq!(written.get(finder::COMMENT), Some(comment.as_slice()));
        // The desktop's tags ride along, named as a Mac names attributes.
        assert_eq!(written.get("xdg.tags"), Some(b"Rouge,new".as_slice()));
        #[cfg(unix)]
        assert_eq!(mode(&file), 0o644, "the file replaced keeps its mode");

        // Undone, and every tag removed: the attribute goes, the rest stays.
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&file), ["Rouge", "Été"]);
        let clear = plan(&app, "tags.set", &serde_json::json!({ "tags": [] }));
        let r = app.executor.execute_blocking(&clear, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(read_tags(&file).is_empty());
        let cleared = aside(&file);
        assert_eq!(cleared.get(finder::TAGS), None);
        assert_eq!(cleared.get("com.apple.quarantine"), Some(quarantine));
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.unwrap().file_name().into_string().ok())
            .filter(|n| n.starts_with("._") && n != "._it's on the stick.pdf")
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn a_macs_edit_after_skwir_wrote_wins_and_the_stores_are_made_to_agree() {
        const FINDER: &str = "user.com.apple.metadata:_kMDItemUserTags";
        let tmp = home_tempdir();
        let dir = tmp.path().join("synced");
        std::fs::create_dir(&dir).unwrap();
        let f = dir.join("it's both.txt");
        std::fs::write(&f, b"x").unwrap();
        write_tags(&f, &["a".into(), "b".into()]).unwrap();
        finder_tags(&f, FINDER, finder::Framing::Plain, &[("a", 0), ("b", 6)]);
        let (app, pane) = app_in(tmp.path(), &dir, &["it's both.txt"]);
        let add = plan(&app, "tags.add", &serde_json::json!({ "tags": ["x"] }));
        assert!(
            app.executor
                .execute_blocking(&add, app.lifetime())
                .error
                .is_none()
        );
        let read = || app.notes.read_one(&f, &app.notes.policy());
        assert_eq!(read().notes.tag_names(), ["a", "b", "x"]);
        assert!(read().differ.is_empty());

        // On the Mac, `b` and `x` are taken away and `mac` is given.
        finder_tags(&f, FINDER, finder::Framing::Plain, &[("a", 0), ("mac", 4)]);
        let now = read();
        assert_eq!(now.notes.tag_names(), ["a", "mac"], "the Mac's edit wins");
        assert_eq!(now.differ, ["xdg"]);
        // Without what skwir wrote (another machine, a cache deleted): all.
        assert_eq!(read_tags(&f), ["a", "b", "x", "mac"]);
        // Properties says where they are, and where they differ.
        hydrate_all(&app, pane);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.pump();
            let differ = app
                .workspace
                .read()
                .pane(pane)
                .unwrap()
                .listing
                .find_by_name("it's both.txt".as_ref())
                .and_then(|e| e.metadata.value(keys::NOTES_DIFFER).cloned());
            if let Some(Value::List(titles)) = differ {
                assert_eq!(
                    titles[0].as_str(),
                    Some(skwir_core::notes::NotesStore::title(&xdg::XdgStore))
                );
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }

        let agree = plan(&app, "notes.agree", &serde_json::json!({}));
        assert_eq!(agree.steps.len(), 1);
        assert!(
            app.executor
                .execute_blocking(&agree, app.lifetime())
                .error
                .is_none()
        );
        assert_eq!(
            skwir_core::fs::xattr::get(&f, xdg::TAGS)
                .unwrap()
                .as_deref(),
            Some(b"a,mac".as_slice())
        );
        assert!(read().differ.is_empty());
        let again = plan(&app, "notes.agree", &serde_json::json!({}));
        let r = app.executor.execute_blocking(&again, app.lifetime());
        assert_eq!((r.applied, r.skipped), (0, 1), "they agree already");
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(
            skwir_core::fs::xattr::get(&f, xdg::TAGS)
                .unwrap()
                .as_deref(),
            Some(b"a,b,x".as_slice())
        );
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn a_macs_file_keeps_its_notes_renamed_and_copied_to_a_disk() {
        use crate::appledouble;
        appledouble::EVERYWHERE.store(true, std::sync::atomic::Ordering::Relaxed);
        let tmp = home_tempdir();
        let stick = tmp.path().join("STICK");
        let disk = tmp.path().join("disk");
        std::fs::create_dir(&stick).unwrap();
        std::fs::create_dir(&disk).unwrap();
        let file = stick.join("it's a.pdf");
        std::fs::write(&file, b"x").unwrap();
        let tags = finder::encode(
            &[finder::Tag {
                name: "Rouge".into(),
                color: 6,
            }],
            finder::Framing::Plain,
        );
        std::fs::write(
            stick.join("._it's a.pdf"),
            appledouble::tests::as_a_mac_writes(&[(finder::TAGS, &tags)]),
        )
        .unwrap();
        let (app, _) = app_in(tmp.path(), &stick, &["it's a.pdf"]);
        let run = |op: Box<dyn skwir_core::op::Operation>| {
            let mut plan = Plan::new("op");
            plan.steps.push(op);
            let r = app.executor.execute_blocking(&plan, app.lifetime());
            assert!(r.error.is_none(), "{:?}", r.error);
        };
        run(Box::new(skwir_core::op::Rename {
            entry: EntryRef::from_path(&file).unwrap(),
            new_name: "b.pdf".into(),
        }));
        let renamed = stick.join("b.pdf");
        assert!(stick.join("._b.pdf").exists());
        assert_eq!(read_tags(&renamed), ["Rouge"]);
        // Copied to a disk, which keeps attributes: the Mac's become the
        // file's own, color and all, and no `._` file follows.
        run(Box::new(skwir_core::op::Copy {
            entry: EntryRef::from_path(&renamed).unwrap(),
            dest_dir: disk.clone(),
            new_name: None,
            preserve_attributes: true,
        }));
        let copy = disk.join("b.pdf");
        assert!(!disk.join("._b.pdf").exists());
        assert!(
            skwir_core::fs::xattr::get(&copy, "user.com.apple.metadata:_kMDItemUserTags")
                .unwrap()
                .is_some()
        );
        appledouble::EVERYWHERE.store(false, std::sync::atomic::Ordering::Relaxed);
        let notes = read_notes(&copy);
        assert_eq!(notes.tag_names(), ["Rouge"]);
        assert_eq!(notes.tag("Rouge").unwrap().color.as_deref(), Some("red"));
    }

    /// Against a folder whose volume refuses attributes (a FUSE mount made
    /// with `bindfs --xattr-none`, a FAT stick): a copy onto it keeps the
    /// file's notes in a `._` file, which a copy back makes attributes
    /// again, and an edit there starts one for a file that has none.
    /// `SKWIR_NO_XATTR_DIR=<folder> cargo nextest run -p skwir-plugin-tags
    /// --run-ignored only refusing`.
    #[test]
    #[ignore = "needs a folder on a volume without attributes"]
    fn onto_a_volume_refusing_attributes_notes_go_beside_and_come_back() {
        let refusing = std::path::PathBuf::from(
            std::env::var_os("SKWIR_NO_XATTR_DIR").expect("SKWIR_NO_XATTR_DIR"),
        );
        let tmp = home_tempdir();
        let home = tmp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let file = home.join("it's tagged.txt");
        std::fs::write(&file, b"x").unwrap();
        write_tags(&file, &["work".into(), "Été".into()]).unwrap();
        skwir_core::fs::xattr::set(&file, xdg::RATING, b"8").unwrap();
        let stick = refusing.join(format!("skwir-refusing-{}", std::process::id()));
        std::fs::create_dir(&stick).unwrap();
        assert!(
            skwir_core::fs::xattr::set(&stick, "user.test", b"x").is_err(),
            "attributes are refused"
        );
        let (app, _) = app_in(tmp.path(), &home, &["it's tagged.txt"]);
        let run = |op: Box<dyn skwir_core::op::Operation>| {
            let mut plan = Plan::new("op");
            plan.steps.push(op);
            let r = app.executor.execute_blocking(&plan, app.lifetime());
            assert!(r.error.is_none(), "{:?}", r.error);
        };
        run(Box::new(skwir_core::op::Copy {
            entry: EntryRef::from_path(&file).unwrap(),
            dest_dir: stick.clone(),
            new_name: None,
            preserve_attributes: true,
        }));
        let copied = stick.join("it's tagged.txt");
        assert!(stick.join("._it's tagged.txt").exists());
        let notes = read_notes(&copied);
        assert_eq!(
            (notes.tag_names(), notes.rating),
            (vec!["work".to_owned(), "Été".to_owned()], Some(8))
        );
        // Back to the disk: attributes again, no `._` file.
        let back = home.join("back");
        std::fs::create_dir(&back).unwrap();
        run(Box::new(skwir_core::op::Copy {
            entry: EntryRef::from_path(&copied).unwrap(),
            dest_dir: back.clone(),
            new_name: None,
            preserve_attributes: true,
        }));
        assert!(!back.join("._it's tagged.txt").exists());
        assert_eq!(
            skwir_core::fs::xattr::get(back.join("it's tagged.txt"), xdg::TAGS)
                .unwrap()
                .as_deref(),
            Some("work,Été".as_bytes())
        );
        // A file of the stick without notes: tagging it starts its `._`.
        let plain = stick.join("plain.txt");
        std::fs::write(&plain, b"x").unwrap();
        let edit = app
            .notes
            .edit(EntryRef::from_path(&plain).unwrap())
            .tags(vec!["new".into()]);
        run(Box::new(edit));
        assert!(stick.join("._plain.txt").exists());
        assert_eq!(read_tags(&plain), ["new"]);
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(!stick.join("._plain.txt").exists());
        std::fs::remove_dir_all(&stick).unwrap();
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn the_notes_page_says_which_stores_are_read_and_written() {
        const FINDER: &str = "user.com.apple.metadata:_kMDItemUserTags";
        let tmp = home_tempdir();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let f = dir.join("it's both.txt");
        std::fs::write(&f, b"x").unwrap();
        write_tags(&f, &["ours".into()]).unwrap();
        finder_tags(&f, FINDER, finder::Framing::Plain, &[("mac", 4)]);
        let (app, _) = app_in(tmp.path(), &dir, &["it's both.txt"]);
        let keys: Vec<String> = app
            .settings
            .schema()
            .into_iter()
            .map(|s| s.key)
            .filter(|k| k.starts_with("notes."))
            .collect();
        assert!(keys.contains(&"notes.xdg.write".to_owned()), "{keys:?}");
        assert!(keys.contains(&"notes.appledouble.write".to_owned()));
        let set = |key: &str, value: toml::Value| {
            app.settings.set(key, value);
            app.apply_notes_settings();
        };
        let read = || app.notes.read_path(&f, &app.notes.policy()).tag_names();
        assert_eq!(read(), ["ours", "mac"]);
        // Not read: the desktop's tags are left out.
        set("notes.xdg.read", toml::Value::Boolean(false));
        assert_eq!(read(), ["mac"]);
        set("notes.xdg.read", toml::Value::Boolean(true));
        // Not written: an edit leaves the desktop's attribute as it was and
        // goes to the Finder's.
        set("notes.xdg.write", toml::Value::String("off".into()));
        let add = plan(&app, "tags.add", &serde_json::json!({ "tags": ["new"] }));
        assert!(
            app.executor
                .execute_blocking(&add, app.lifetime())
                .error
                .is_none()
        );
        assert_eq!(
            skwir_core::fs::xattr::get(&f, xdg::TAGS)
                .unwrap()
                .as_deref(),
            Some(b"ours".as_slice())
        );
        assert_eq!(
            finder_of(&f, FINDER, finder::Framing::Plain),
            [("ours".into(), 0), ("mac".into(), 4), ("new".into(), 0)]
        );
        // Captured when the plan was made: a plan made now writes both.
        set("notes.xdg.write", toml::Value::String("create".into()));
        let add = plan(&app, "tags.add", &serde_json::json!({ "tags": ["more"] }));
        assert!(
            app.executor
                .execute_blocking(&add, app.lifetime())
                .error
                .is_none()
        );
        assert_eq!(
            skwir_core::fs::xattr::get(&f, xdg::TAGS)
                .unwrap()
                .as_deref(),
            Some(b"ours,mac,new,more".as_slice())
        );
        // Off with the plugin: no Tags and notes page.
        app.plugins.set_enabled("tags", false, &app);
        assert!(
            !app.settings
                .schema()
                .iter()
                .any(|s| s.key.starts_with("notes."))
        );
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn a_toggle_gives_a_tag_to_all_or_takes_it_from_all() {
        let tmp = home_tempdir();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let (a, b) = (dir.join("a.txt"), dir.join("it's b.txt"));
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();
        write_tags(&a, &["Keep".into(), "todo".into()]).unwrap();
        let (app, _) = app_in(tmp.path(), &dir, &["a.txt", "it's b.txt"]);
        // The toggle decides by what the pane knows of each file, which
        // its watcher brings up to date after each edit.
        let toggle = |then: fn(&std::path::Path) -> bool| {
            let p = plan(&app, "tags.toggle", &serde_json::json!({ "tag": "TODO" }));
            let r = app.executor.execute_blocking(&p, app.lifetime());
            assert!(r.error.is_none());
            let listed = |app: &App| {
                let ws = app.workspace.read();
                let p = ws.pane(ws.active_id()).unwrap();
                p.listing
                    .iter()
                    .find(|e| e.name == "it's b.txt")
                    .is_some_and(|e| has_tag(e, "todo"))
            };
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while listed(&app) != then(&b) && std::time::Instant::now() < deadline {
                app.workspace.write().pump();
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        // One has it: both get it, in the spelling each already had.
        toggle(|_| true);
        assert_eq!(read_tags(&a), ["Keep", "todo"]);
        assert_eq!(read_tags(&b), ["TODO"]);
        // Both have it: both lose it, and nothing else.
        toggle(|_| false);
        assert_eq!(read_tags(&a), ["Keep"]);
        assert!(read_tags(&b).is_empty());
        assert!(
            app.dispatch(&Invocation::with_args(
                "tags.toggle",
                &serde_json::json!({ "tag": " , " })
            ))
            .is_err()
        );
    }

    /// From the window's thread, a toggle of files the pane has not read
    /// reads them on a worker, then applies its plan as the user's own.
    #[cfg(unix)]
    #[test]
    fn a_toggle_from_the_window_reads_off_its_thread() {
        let tmp = home_tempdir();
        let file = tmp.path().join("it's Été.txt");
        std::fs::write(&file, b"x").unwrap();
        write_tags(&file, &["Keep".into()]).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins.add(Arc::new(TagsPlugin::default()), &app);
        skwir_core::scheduler::mark_window_thread(&app.scheduler);
        let entries = serde_json::json!([EntryRef::from_path(&file).unwrap()]);
        let answered = app
            .dispatch(&Invocation::with_args(
                "tags.toggle",
                &serde_json::json!({ "tag": "todo", "entries": entries }),
            ))
            .unwrap();
        assert!(matches!(answered, Outcome::Done), "nothing read here");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while read_tags(&file) != ["Keep", "todo"] {
            assert!(std::time::Instant::now() < deadline, "never tagged");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn write_read_and_clear() {
        let tmp = home_tempdir();
        let f = tmp.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        assert!(read_tags(&f).is_empty());
        write_tags(&f, &["work".into(), "urgent".into()]).unwrap();
        assert_eq!(read_tags(&f), ["work", "urgent"]);
        write_tags(&f, &[]).unwrap();
        assert!(read_tags(&f).is_empty());
        write_tags(&f, &[]).unwrap();
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn provider_fills_tags_and_plans_apply_and_undo() {
        let tmp = home_tempdir();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let f = dir.join("doc.txt");
        std::fs::write(&f, b"x").unwrap();
        write_tags(&f, &["old".into()]).unwrap();
        let (app, pane) = app_in(tmp.path(), &dir, &["doc.txt"]);
        assert!(app.metadata.ids().contains(&"tags"));
        {
            let ws = app.workspace.read();
            let e = ws
                .pane(pane)
                .unwrap()
                .listing
                .find_by_name("doc.txt".as_ref())
                .unwrap();
            assert_eq!(tags_of(&e.metadata).unwrap(), ["old"]);
            assert_eq!(
                e.metadata.get(FIELD).unwrap().provenance,
                skwir_core::entry::Provenance::User
            );
        }

        let add = plan(
            &app,
            "tags.add",
            &serde_json::json!({ "tags": ["new", "old"] }),
        );
        let r = app.executor.execute_blocking(&add, app.lifetime());
        assert!(r.error.is_none());
        assert_eq!(read_tags(&f), ["old", "new"]);
        // Re-apply is a no-op.
        let again = Plan::from_data(&add.to_data(), &app.ops).unwrap();
        let r2 = app.executor.execute_blocking(&again, app.lifetime());
        assert_eq!((r2.applied, r2.skipped), (0, 1));

        let remove = plan(&app, "tags.remove", &serde_json::json!({ "tags": ["old"] }));
        app.executor.execute_blocking(&remove, app.lifetime());
        assert_eq!(read_tags(&f), ["new"]);
        // Tags ignore case: adding "NEW" changes nothing, removing "NEW"
        // removes it.
        let add = plan(&app, "tags.add", &serde_json::json!({ "tags": ["NEW"] }));
        let r = app.executor.execute_blocking(&add, app.lifetime());
        assert_eq!((r.applied, r.skipped), (0, 1), "nothing to change");
        let remove = plan(&app, "tags.remove", &serde_json::json!({ "tags": ["NEW"] }));
        app.executor.execute_blocking(&remove, app.lifetime());
        assert!(read_tags(&f).is_empty());
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&f), ["new"]);
        let set = plan(&app, "tags.set", &serde_json::json!({ "tags": [] }));
        app.executor.execute_blocking(&set, app.lifetime());
        assert!(read_tags(&f).is_empty());

        // Undo walks back: set([]) -> ["new"], remove -> ["old","new"], add -> ["old"].
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&f), ["new"]);
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&f), ["old", "new"]);
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&f), ["old"]);

        // tags.list counts what the listing knows: once the folder's watcher
        // brought the undone tags (an entry already hydrated is not waited
        // for by hydrating again).
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.pump();
            let shown = app
                .workspace
                .read()
                .pane(pane)
                .unwrap()
                .listing
                .find_by_name("doc.txt".as_ref())
                .and_then(|e| tags_of(&e.metadata));
            if shown.as_deref() == Some(&["old".to_owned()][..]) {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "{shown:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
        let Outcome::Value(v) = app.dispatch(&Invocation::new("tags.list")).unwrap() else {
            panic!()
        };
        assert_eq!(v["old"], 1);

        // Disabling withdraws the provider and the contributions.
        app.plugins.set_enabled("tags", false, &app);
        assert!(!app.metadata.ids().contains(&"tags"));
        assert!(app.commands.get("tags.set").is_none());
        assert!(app.registry.menus.all().iter().all(|m| m.title != "Tags…"));
    }

    /// The tag editor's edit: two files with different tags keep what the
    /// other has to themselves; a tag on some of them stays on those.
    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn an_edit_adds_and_removes_without_spreading_the_others() {
        let tmp = home_tempdir();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let (a, b) = (dir.join("it's a.txt"), dir.join("b é.txt"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        write_tags(&a, &["Work".into(), "draft".into()]).unwrap();
        write_tags(&b, &["home".into(), "draft".into()]).unwrap();
        let (app, _) = app_in(tmp.path(), &dir, &["it's a.txt", "b é.txt"]);

        let Outcome::Value(v) = app.dispatch(&Invocation::new("tags.selected")).unwrap() else {
            panic!()
        };
        assert_eq!(v["items"], 2);
        let count = |name: &str| {
            v["tags"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == name)
                .map(|t| t["count"].as_u64().unwrap())
        };
        assert_eq!(
            (count("Work"), count("draft"), count("home")),
            (Some(1), Some(2), Some(1))
        );

        let edit = plan(
            &app,
            "tags.edit",
            &serde_json::json!({ "add": ["urgent", "WORK"], "remove": ["Draft"] }),
        );
        assert_eq!(edit.title, "add tags urgent, WORK and remove tag Draft");
        app.executor.execute_blocking(&edit, app.lifetime());
        assert_eq!(read_tags(&a), ["Work", "urgent"], "spelling kept");
        assert_eq!(read_tags(&b), ["home", "urgent", "WORK"]);
        // One plan, one undo.
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&a), ["Work", "draft"]);
        assert_eq!(read_tags(&b), ["home", "draft"]);

        let Outcome::Value(v) = app.dispatch(&Invocation::new("tags.selected")).unwrap() else {
            panic!()
        };
        let known: Vec<&str> = v["known"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t.as_str())
            .collect();
        let mut used_last = known[..2].to_vec();
        used_last.sort_unstable();
        assert_eq!(used_last, ["Work", "urgent"], "used first: {known:?}");
        // Those pinned come before.
        assert_eq!(
            super::known(
                vec!["home".into(), "x".into()],
                known.iter().map(|t| (*t).to_owned()).collect()
            )[..3],
            ["home", "x", known[0]]
        );
    }

    // Extended attributes: Unix's.
    #[cfg(unix)]
    #[test]
    fn ratings_and_comments_are_set_undone_and_read() {
        let tmp = home_tempdir();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let f = dir.join("pic.jpg");
        std::fs::write(&f, b"x").unwrap();
        skwir_core::fs::xattr::set(&f, xdg::ORIGIN, b"https://example.org/pic.jpg").unwrap();
        let (app, pane) = app_in(tmp.path(), &dir, &["pic.jpg"]);

        let rate = plan(&app, "notes.rate", &serde_json::json!({ "stars": 4 }));
        let changes: Vec<String> = rate
            .steps
            .iter()
            .map(|s| s.describe().to_string())
            .collect();
        assert_eq!(changes, ["rate “pic.jpg” 4 stars"]);
        app.executor.execute_blocking(&rate, app.lifetime());
        assert_eq!(read_rating(&f), Some(8));
        assert_eq!(
            skwir_core::fs::xattr::get(&f, xdg::RATING)
                .unwrap()
                .unwrap(),
            b"8",
            "as Dolphin writes it"
        );
        let comment = plan(
            &app,
            "notes.comment",
            &serde_json::json!({ "text": "  from the “trip”, day 2  " }),
        );
        app.executor.execute_blocking(&comment, app.lifetime());
        assert_eq!(read_comment(&f).as_deref(), Some("from the “trip”, day 2"));

        // The watcher sees the attributes change, and the listing reads
        // them again.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.pump();
            let ws = app.workspace.read();
            let e = ws
                .pane(pane)
                .unwrap()
                .listing
                .find_by_name("pic.jpg".as_ref())
                .unwrap();
            let m = &e.metadata;
            if m.value(keys::COMMENT).is_none() {
                assert!(std::time::Instant::now() < deadline, "the listing followed");
                drop(ws);
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            assert_eq!(
                m.value(keys::RATING).and_then(skwir_core::Value::as_u64),
                Some(8)
            );
            assert_eq!(
                m.value(keys::COMMENT).and_then(skwir_core::Value::as_str),
                Some("from the “trip”, day 2")
            );
            assert_eq!(
                m.value(keys::ORIGIN).and_then(skwir_core::Value::as_str),
                Some("https://example.org/pic.jpg")
            );
            break;
        }
        // The menu offers the comment there to edit.
        let Outcome::Value(menu) = app.dispatch(&Invocation::new("tags.menu")).unwrap() else {
            panic!()
        };
        assert_eq!(
            menu["items"][0]["command"]["args"]["text"],
            "from the “trip”, day 2"
        );

        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_comment(&f), None);
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_rating(&f), None);
        assert!(
            skwir_core::fs::xattr::get(&f, xdg::RATING)
                .unwrap()
                .is_none()
        );
        assert!(
            app.dispatch(&Invocation::with_args(
                "notes.rate",
                &serde_json::json!({ "stars": 6 })
            ))
            .is_err()
        );
    }

    /// A link carries what it leads to: the target's tags.
    #[cfg(unix)]
    #[test]
    fn a_link_is_tagged_through_to_what_it_leads_to() {
        let tmp = home_tempdir();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let target = dir.join("target.txt");
        std::fs::write(&target, b"x").unwrap();
        std::os::unix::fs::symlink(&target, dir.join("link")).unwrap();
        let (app, _) = app_in(tmp.path(), &dir, &["link"]);
        let add = plan(&app, "tags.add", &serde_json::json!({ "tags": ["linked"] }));
        let r = app.executor.execute_blocking(&add, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(read_tags(&target), ["linked"]);
    }

    #[test]
    fn a_tag_color_is_one_of_the_palette() {
        let tmp = home_tempdir();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(TagsPlugin::default()), &app);
        let color = |tag: &str, color: Option<&str>| {
            app.dispatch(&Invocation::with_args(
                "tags.color",
                &serde_json::json!({ "tag": tag, "color": color }),
            ))
        };
        assert!(color("Work", Some("mauve")).is_err());
        color("Work", Some("teal")).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let stored = || {
            app.settings
                .get::<toml::Table>(TAG_COLOR_SETTING)
                .and_then(|t| t.get("work").and_then(|c| c.as_str().map(str::to_owned)))
        };
        while stored().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(stored().as_deref(), Some("teal"));
    }

    #[test]
    fn edit_titles_say_what_changes() {
        let t = |s: &[&str]| s.iter().map(|&x| x.to_owned()).collect::<Vec<_>>();
        assert_eq!(edit_title(&t(&["a"]), &[]), "add tag a");
        assert_eq!(edit_title(&[], &t(&["a", "b"])), "remove tags a, b");
        assert_eq!(
            edited(&t(&["A", "b"]), &t(&["a", "c"]), &t(&["B"]), &[]),
            t(&["A", "c"])
        );
        // One tag of a field at a time.
        assert_eq!(
            edited(
                &t(&["Status/Draft", "b"]),
                &t(&["status/Final"]),
                &[],
                &t(&["Status"])
            ),
            t(&["b", "status/Final"])
        );
    }
}
