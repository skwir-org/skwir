//! bibli plugin: query as location.
//!
//! A query (`*.pdf modified<7d size>10M in:~/docs`) is a location. Results
//! stream in as a virtual listing, so every view, selection, and operation
//! works on them unchanged. Saved searches sit in the sidebar under their
//! own heading and are kept like bookmarks: the pane's keep button and
//! Ctrl+D save the search shown, and each is renamed, edited, removed and
//! put in order with Undo (`saved`). The plan's `search` item (walker,
//! content search, plocate) lives here too.

pub mod facets;
pub mod lang;
pub mod plocate;
pub mod provider;
pub mod saved;
pub mod similar;

use std::sync::Arc;

use bibli_core::app::{App, NotifyLevel};
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, NoArgs, Outcome};
use bibli_core::location::Location;
use bibli_core::os_path::PathArg;
use bibli_core::plugin::Plugin;
use bibli_core::registry::{Keeper, KeyBinding, MenuItem, Place, Reorder};
use bibli_core::scheduler::Priority;
use bibli_core::settings::SettingSpec;
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;

pub use provider::{QueryProvider, SCHEME};
use saved::Edit;
pub use saved::SavedQuery;

const ID: &str = "query";
const SAVED_KEY: &str = "query.saved";
/// The sidebar section of saved searches.
const GROUP: &str = "searches";

/// The query plugin.
#[derive(Debug, Default)]
pub struct QueryPlugin {
    /// Cancelled on disable: every walk and stat the provider started ends,
    /// and no saved-search edit still queued runs.
    lifetime: Mutex<Option<CancelToken>>,
    /// Held across a read-modify-write of the saved searches, so two edits
    /// never drop each other's change.
    saving: Arc<Mutex<()>>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    /// The query expression, e.g. `*.pdf modified<7d`.
    expr: String,
    /// Subtree to search; the pane's current directory when omitted.
    #[serde(default)]
    root: Option<PathArg>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SaveArgs {
    /// The search; the one the pane shows when omitted.
    #[serde(default)]
    expr: Option<String>,
    /// Its name in the sidebar; made from the search when omitted.
    #[serde(default)]
    name: Option<String>,
    /// The saved search it goes before; last when omitted.
    #[serde(default)]
    before: Option<String>,
    #[serde(default)]
    pane: Option<PaneId>,
    /// Set by a toast's Undo: the result offers Redo instead.
    #[serde(default)]
    undo: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ForgetArgs {
    /// The saved search.
    #[serde(default)]
    expr: Option<String>,
    /// Or its name, when `expr` is omitted.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    undo: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RenameArgs {
    /// The saved search.
    expr: String,
    /// Its new name; empty for one made from the search.
    name: String,
    #[serde(default)]
    undo: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct MoveArgs {
    /// The saved search.
    expr: String,
    /// The saved search it goes before; last when omitted.
    #[serde(default)]
    before: Option<String>,
    #[serde(default)]
    undo: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct PaneOnly {
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct SimilarArgs {
    /// What the files found share with the selected one.
    by: similar::By,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct DescribeArgs {
    /// The search; the one the pane shows when omitted.
    #[serde(default)]
    expr: Option<String>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct LocationArgs {
    /// A facet, as `query.refine` names them (`tag`, `kind`, `size`, …).
    facet: String,
    /// Its value (`work` for `tag`).
    value: String,
    /// Where it searches; the home folder when omitted.
    #[serde(default)]
    root: Option<PathArg>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RefineArgs {
    /// A chip's id: `words`, `kind`, `modified`, `size`, `tag`, `content`,
    /// `where` or `advanced`.
    facet: String,
    /// What to set it to (a kind toggles, `where` takes `subtree`, `here`
    /// or `hidden`, `advanced` the index of the part to remove); omitted,
    /// the facet is cleared.
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct EditArgs {
    /// The saved search.
    expr: String,
    /// What it searches for from now on; its name and place stay.
    to: String,
    #[serde(default)]
    undo: bool,
}

/// The location string for `expr` rooted at `root` (unless `expr` names a
/// root with `in:`, as the parser reads it).
pub fn location_for(expr: &str, root: Option<&std::path::Path>) -> Location {
    let has_root = lang::parse(expr).is_ok_and(|q| q.root_given);
    let rest = match root {
        Some(r) if !has_root => format!("{expr} in:{}", lang::quote_os(r.as_os_str())),
        _ => expr.to_owned(),
    };
    // An empty search is every entry below the root: ` in:/x` reads the
    // same as `in:/x`, but only one of them is the text a person expects.
    let rest = rest.trim().to_owned();
    Location::new(SCHEME, rest)
}

/// A name for a search saved without one: what it looks for and where
/// (`*.pdf modified<7d in Documents`).
pub fn suggested_name(expr: &str) -> String {
    let Ok(query) = lang::parse(expr) else {
        return expr.to_owned();
    };
    let what = query
        .expr
        .as_ref()
        .map_or_else(|| "Everything".to_owned(), lang::Expr::label);
    if !query.root_given {
        return what;
    }
    format!("{what} in {}", place_name(&query.root))
}

/// A search's root as its name and its chip say it: Home for the home
/// folder, else the folder's name.
pub fn place_name(root: &std::path::Path) -> String {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    if home.as_deref() == Some(root) {
        "Home".to_owned()
    } else {
        root.file_name().map_or_else(
            || root.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }
}

/// The search a pane shows, or for a folder, an empty one rooted there
/// (every entry below it), which a first chip then narrows.
fn pane_search(app: &App, pane: PaneId) -> anyhow::Result<String> {
    if let Some(location) = app.pane_location(pane).filter(|l| &*l.scheme == SCHEME) {
        return Ok(location.rest);
    }
    let dir = app
        .pane_dir(pane)
        .map_err(|_| anyhow::anyhow!("the pane shows no search and no folder"))?;
    Ok(location_for("", Some(&dir)).rest)
}

/// Saved searches from settings.
pub fn saved(app: &App) -> Vec<SavedQuery> {
    app.settings
        .get::<Vec<SavedQuery>>(SAVED_KEY)
        .unwrap_or_default()
}

fn check(expr: &str) -> anyhow::Result<()> {
    lang::parse(expr)
        .map(drop)
        .map_err(|e| anyhow::anyhow!("query: {e}"))
}

/// The name a new name means: the one made from the search when empty.
fn name_or_suggested(name: &str, expr: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        suggested_name(expr)
    } else {
        name.to_owned()
    }
}

/// Apply `edit` to the saved searches and persist them: the edit that
/// reverts it, `None` when it changed nothing. One edit at a time.
fn apply_edit(app: &App, saving: &Mutex<()>, edit: &Edit) -> anyhow::Result<Option<Edit>> {
    let _saving = saving.lock();
    let mut list = saved(app);
    let Some(inverse) = edit.apply(&mut list).map_err(anyhow::Error::msg)? else {
        return Ok(None);
    };
    let arr = list
        .iter()
        .map(|s| {
            let mut t = toml::Table::new();
            t.insert("name".into(), toml::Value::String(s.name.clone()));
            t.insert("expr".into(), toml::Value::String(s.expr.clone()));
            toml::Value::Table(t)
        })
        .collect();
    app.settings.set(SAVED_KEY, toml::Value::Array(arr));
    app.settings
        .save()
        .map_err(|e| anyhow::anyhow!("the saved searches were not written: {e:#}"))?;
    register_places(app);
    Ok(Some(inverse))
}

/// The command that applies `edit`, from a toast's button.
fn invocation(edit: &Edit, undo: bool) -> Invocation {
    let (name, mut args) = match edit {
        Edit::Add { name, expr, before } => (
            "query.save",
            serde_json::json!({ "expr": expr, "name": name, "before": before }),
        ),
        Edit::Remove { expr } => ("query.forget", serde_json::json!({ "expr": expr })),
        Edit::Rename { expr, name } => (
            "query.rename",
            serde_json::json!({ "expr": expr, "name": name }),
        ),
        Edit::Move { expr, before } => (
            "query.move",
            serde_json::json!({ "expr": expr, "before": before }),
        ),
        Edit::Change { expr, to } => ("query.edit", serde_json::json!({ "expr": expr, "to": to })),
    };
    args["undo"] = serde_json::json!(undo);
    Invocation::with_args(name, &args)
}

/// What an edit did, in a sentence, or what it found when it did nothing.
/// `shown` is the search's name before the edit, when it was saved.
fn describe(edit: &Edit, shown: Option<&str>, done: bool) -> Option<String> {
    let name = || shown.map_or_else(|| suggested_name(edit.expr()), str::to_owned);
    Some(match (edit, done) {
        (Edit::Add { name: new, .. }, true) => format!("Saved search “{new}”"),
        (Edit::Add { .. }, false) => format!("This search is already saved as “{}”", name()),
        (Edit::Remove { .. }, true) => format!("Removed saved search “{}”", name()),
        (Edit::Remove { .. }, false) => "This search is not saved".to_owned(),
        (Edit::Rename { name: new, .. }, true) => {
            format!("Renamed saved search “{}” to “{new}”", name())
        }
        (Edit::Move { .. }, true) => format!("Moved saved search “{}”", name()),
        (Edit::Change { .. }, true) => format!("Changed saved search “{}”", name()),
        // Renamed to its own name, dropped where it was, edited to the same
        // search, or no longer saved: nothing to say.
        (Edit::Rename { .. } | Edit::Move { .. } | Edit::Change { .. }, false) => return None,
    })
}

/// Edits the saved searches on a worker (the settings file is written) and
/// says what happened with the way back: Undo, or Redo after an undo.
#[derive(Clone)]
struct Editor {
    saving: Arc<Mutex<()>>,
    lifetime: CancelToken,
}

impl Editor {
    fn run(&self, app: &App, edit: Edit, undo: bool) {
        let (saving, weak) = (Arc::clone(&self.saving), app.weak());
        app.scheduler.spawn(
            Priority::Normal,
            "query-saved",
            self.lifetime.child(),
            move || {
                let Some(app) = weak.upgrade() else { return };
                let shown = saved(&app)
                    .into_iter()
                    .find(|s| s.expr == edit.expr())
                    .map(|s| s.name);
                match apply_edit(&app, &saving, &edit) {
                    Ok(Some(inverse)) => {
                        if let Some(text) = describe(&edit, shown.as_deref(), true) {
                            app.notify_with_action(
                                NotifyLevel::Info,
                                text,
                                if undo { "Redo" } else { "Undo" },
                                invocation(&inverse, !undo),
                            );
                        }
                    }
                    Ok(None) => {
                        if let Some(text) = describe(&edit, shown.as_deref(), false) {
                            app.notify(NotifyLevel::Info, text);
                        }
                    }
                    Err(error) => app.notify(
                        NotifyLevel::Error,
                        format!("The saved searches were not changed: {error:#}"),
                    ),
                }
                app.wake();
            },
        );
    }
}

/// An item of a saved search's own menu.
fn menu_item(title: &str, command: Invocation, order: i32, icon: &str) -> MenuItem {
    MenuItem {
        menu: "place".into(),
        title: title.into(),
        command,
        when: None,
        order,
        icon: Some(icon.into()),
        submenu: None,
    }
}

fn place(search: SavedQuery) -> Place {
    let expr = serde_json::json!(search.expr);
    let menu = vec![
        menu_item(
            "Rename…",
            Invocation::with_args(
                "ui.ask",
                &serde_json::json!({
                    "title": "Rename saved search",
                    "placeholder": "Name (empty: made from the search)",
                    "text": search.name,
                    "command": "query.rename",
                    "args": { "expr": expr },
                    "field": "name",
                }),
            ),
            100,
            "rename",
        ),
        menu_item(
            "Edit Search…",
            Invocation::with_args(
                "ui.ask",
                &serde_json::json!({
                    "title": "Edit saved search",
                    "placeholder": "kind:document modified:this-week *.pdf  (in: picks the root)",
                    "text": search.expr,
                    "command": "query.edit",
                    "args": { "expr": expr },
                    "field": "to",
                }),
            ),
            110,
            "search",
        ),
        menu_item(
            "Remove Saved Search",
            Invocation::with_args("query.forget", &serde_json::json!({ "expr": expr })),
            300,
            "minus",
        ),
    ];
    Place {
        title: search.name,
        location: Location::new(SCHEME, search.expr),
        icon: Some("folder-saved-search".into()),
        group: GROUP.into(),
        space: None,
        eject: None,
        activate: None,
        unavailable: None,
        busy: None,
        menu,
        forget: Some(Invocation::with_args(
            "query.forget",
            &serde_json::json!({ "expr": expr }),
        )),
        reorder: Some(Reorder {
            command: Invocation::with_args("query.move", &serde_json::json!({ "expr": expr })),
            before: expr,
        }),
        color: None,
        drop: None,
    }
}

fn register_places(app: &App) {
    app.registry
        .places
        .replace_owner(ID, saved(app).into_iter().map(place));
}

impl Plugin for QueryPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Query"
    }

    fn description(&self) -> &'static str {
        "Search as a location: name, size, date, tag, MIME, and content queries; saved searches in the sidebar"
    }

    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Browsing
    }

    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        vec![bibli_core::plugin::Requirement::program(
            "plocate",
            "Instant name searches over whole disks (a directory walk is used otherwise)",
        )]
    }

    fn on_disable(&self, _app: &App) {
        if let Some(lifetime) = self.lifetime.lock().take() {
            lifetime.cancel();
        }
    }

    fn register(&self, app: &App) {
        let lifetime = app.lifetime().child();
        if let Some(previous) = self.lifetime.lock().replace(lifetime.clone()) {
            previous.cancel();
        }
        let searches = provider::Searches::default();
        app.register_provider(
            ID,
            Arc::new(QueryProvider::new(lifetime.clone(), searches.clone())),
        );
        // What bibli changed, an open search sees at once.
        app.executor
            .observe(ID, Arc::new(move |changes| searches.changed(changes)));
        // The locale's first weekday is a process away: asked once, on a
        // worker, before anyone reads a week.
        app.scheduler.spawn(
            Priority::Background,
            "calendar.week",
            lifetime.child(),
            bibli_platform::calendar::prewarm,
        );

        app.settings.declare(SettingSpec::data(
            SAVED_KEY,
            "Saved searches shown in the sidebar, in order: [{ name, expr }]",
            toml::Value::Array(Vec::new()),
        ));
        register_places(app);

        app.commands.register::<SearchArgs, _>(
            ID,
            "query.search",
            "Search",
            "Open a query as the pane's location (`*.pdf modified<7d size>10M`; `in:` picks the root)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                lang::parse(&a.expr).map_err(|e| anyhow::anyhow!("query: {e}"))?;
                let root = match a.root {
                    Some(r) => Some(
                        Location::parse_os(r.as_os_str())
                            .path()
                            .ok_or_else(|| anyhow::anyhow!("root must be a path"))?,
                    ),
                    // A pane showing another query searches from home.
                    None => app.pane_dir(pane).ok(),
                };
                app.navigate(pane, &location_for(&a.expr, root.as_deref()))?;
                Ok(Outcome::Done)
            },
        );

        let editor = Editor {
            saving: Arc::clone(&self.saving),
            lifetime: lifetime.clone(),
        };
        let edits = editor.clone();
        app.commands.register::<SaveArgs, _>(
            ID,
            "query.save",
            "Save search",
            "Keep a search in the sidebar, the one the pane shows when `expr` is omitted",
            move |a, app| {
                let expr = if let Some(expr) = a.expr {
                    expr
                } else {
                    let pane = app.pane_or_active(a.pane);
                    app.pane_location(pane)
                        .filter(|l| &*l.scheme == SCHEME)
                        .map(|l| l.rest)
                        .ok_or_else(|| anyhow::anyhow!("the pane is not showing a search"))?
                };
                check(&expr)?;
                let name = name_or_suggested(a.name.as_deref().unwrap_or_default(), &expr);
                let edit = Edit::Add {
                    name,
                    expr,
                    before: a.before,
                };
                edits.run(app, edit, a.undo);
                Ok(Outcome::Done)
            },
        );

        let edits = editor.clone();
        app.commands.register::<ForgetArgs, _>(
            ID,
            "query.forget",
            "Remove saved search",
            "Take a saved search out of the sidebar, named by its search or its name",
            move |a, app| {
                let expr = match (a.expr, a.name) {
                    (Some(expr), _) => expr,
                    (None, Some(name)) => saved(app)
                        .into_iter()
                        .find(|s| s.name == name)
                        .map(|s| s.expr)
                        .ok_or_else(|| anyhow::anyhow!("no saved search named “{name}”"))?,
                    (None, None) => anyhow::bail!("which saved search? pass its expr or name"),
                };
                edits.run(app, Edit::Remove { expr }, a.undo);
                Ok(Outcome::Done)
            },
        );

        let edits = editor.clone();
        app.commands.register::<RenameArgs, _>(
            ID,
            "query.rename",
            "Rename saved search",
            "Change the name a saved search shows in the sidebar",
            move |a, app| {
                let name = name_or_suggested(&a.name, &a.expr);
                edits.run(app, Edit::Rename { expr: a.expr, name }, a.undo);
                Ok(Outcome::Done)
            },
        );

        let edits = editor.clone();
        app.commands.register::<MoveArgs, _>(
            ID,
            "query.move",
            "Move saved search",
            "Put a saved search before another in the sidebar, or last",
            move |a, app| {
                let edit = Edit::Move {
                    expr: a.expr,
                    before: a.before,
                };
                edits.run(app, edit, a.undo);
                Ok(Outcome::Done)
            },
        );

        let edits = editor;
        app.commands.register::<EditArgs, _>(
            ID,
            "query.edit",
            "Edit saved search",
            "Change what a saved search looks for; its name and place in the sidebar stay",
            move |a, app| {
                let to = a.to.trim().to_owned();
                check(&to)?;
                edits.run(app, Edit::Change { expr: a.expr, to }, a.undo);
                Ok(Outcome::Done)
            },
        );

        app.commands.register::<NoArgs, _>(
            ID,
            "query.list",
            "List saved searches",
            "Return the saved searches, in sidebar order",
            |_, app| Ok(Outcome::Value(serde_json::to_value(saved(app))?)),
        );
        app.commands.register::<DescribeArgs, _>(
            ID,
            "query.describe",
            "Describe search",
            "The search's chips for the designer: each facet (words, kind, modified, size, tag, content, where) with its value and the invocation every choice runs, then the parts no facet holds",
            |a, app| {
                let expr = match a.expr {
                    Some(expr) => expr,
                    None => pane_search(app, app.pane_or_active(a.pane))?,
                };
                let tags_known = app.commands.get("tags.list").is_some();
                let description = facets::describe(&expr, tags_known)
                    .map_err(|e| anyhow::anyhow!("query: {e}"))?;
                Ok(Outcome::Value(serde_json::to_value(description)?))
            },
        );
        app.commands.register::<PaneOnly, _>(
            ID,
            "query.tag_choices",
            "Tags to refine by",
            "The Tag chip's picker: the tags on the pane's files, read from them (blocks: `ui.choose` runs it on a worker), each refining the search",
            |a, app| {
                // The tags plugin reads tags; without it, only a tag to type.
                let counts = app
                    .commands
                    .get("tags.list")
                    .and_then(|_| {
                        let list = Invocation::with_args(
                            "tags.list",
                            &serde_json::json!({ "pane": a.pane, "read": true }),
                        );
                        match app.dispatch(&list) {
                            Ok(Outcome::Value(v)) => serde_json::from_value(v).ok(),
                            _ => None,
                        }
                    })
                    .unwrap_or_default();
                Ok(Outcome::Value(facets::tag_choices(&counts)))
            },
        );
        let similar_lifetime = lifetime.clone();
        app.commands.register::<SimilarArgs, _>(
            ID,
            "query.similar",
            "Find similar",
            "Search for files like the selected one: of the same kind, modified the same day, of a similar size, or sharing a tag, in the folder searched or shown",
            move |a, app| {
                similar::start(app, a.pane, a.by, &similar_lifetime)?;
                Ok(Outcome::Done)
            },
        );
        for (title, by, when, order) in [
            ("Same Kind", similar::By::Kind, "single", 470),
            ("Same Day", similar::By::Day, "single", 471),
            ("Similar Size", similar::By::Size, "single file", 472),
            ("Same Tags", similar::By::Tags, "single", 473),
        ] {
            app.registry.menus.add(
                ID,
                MenuItem {
                    menu: "context".into(),
                    title: title.into(),
                    command: Invocation::with_args(
                        "query.similar",
                        &serde_json::json!({ "by": by }),
                    ),
                    when: Some(when.into()),
                    order,
                    icon: Some("search".into()),
                    submenu: Some("Find Similar".into()),
                },
            );
        }
        app.commands.register::<RefineArgs, _>(
            ID,
            "query.refine",
            "Refine search",
            "Change one facet of the pane's search (or start one in its folder) and show the result",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let expr = pane_search(app, pane)?;
                let refined = facets::refine(&expr, &a.facet, a.value.as_deref())
                    .map_err(|e| anyhow::anyhow!("query: {e}"))?;
                app.navigate(pane, &Location::new(SCHEME, refined))?;
                Ok(Outcome::Done)
            },
        );
        app.commands.register::<LocationArgs, _>(
            ID,
            "query.location",
            "Search location",
            "`{ location }`: the search for one facet's value (`tag` `work`) from `root` or the home folder, for whoever shows or runs a search without writing its syntax",
            |a, _| {
                let expr = facets::refine("", &a.facet, Some(&a.value))
                    .map_err(|e| anyhow::anyhow!("query: {e}"))?;
                let root = match a.root {
                    Some(r) => Some(
                        Location::parse_os(r.as_os_str())
                            .path()
                            .ok_or_else(|| anyhow::anyhow!("root must be a path"))?,
                    ),
                    None => None,
                };
                let location = location_for(&expr, root.as_deref());
                Ok(Outcome::Value(
                    serde_json::json!({ "location": location.to_string() }),
                ))
            },
        );
        app.registry.keepers.add(
            ID,
            Keeper {
                scheme: SCHEME.into(),
                save: Invocation::new("query.save"),
                save_title: "Save this search".into(),
                forget_title: "Remove saved search".into(),
            },
        );

        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-f".into(),
                command: Invocation::new("ui.search"),
                when: None,
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Search here…".into(),
                command: Invocation::new("ui.search"),
                when: Some("empty folder".into()),
                order: 420,
                icon: Some("search".into()),
                submenu: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use bibli_core::location::ListingEvent;
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    #[test]
    fn generated_locations_preserve_literal_roots() {
        for root in [
            "/images/2026-08 Trip d'Example - Summit",
            "/images/d'Example",
            "/images/two words",
            "/images/(trip)",
            "/images/\"trip\"",
            "/images/back\\slash",
            "/images/back\\slash with space\\",
            "/images/tab\there",
            "/images/new\nline",
            "/images/unicode\u{2003}space",
        ] {
            let location = location_for("type:file", Some(Path::new(root)));
            let parsed = lang::parse(&location.rest).unwrap();
            assert_eq!(parsed.root, Path::new(root));
            // The canonical query printer must preserve the same root too.
            assert_eq!(lang::parse(&parsed.to_string()).unwrap().root, parsed.root);
        }
    }

    /// A tree with varied names, sizes, mtimes, contents, and a hidden dir.
    fn tree(root: &Path) {
        std::fs::create_dir_all(root.join("docs/deep/deeper")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::create_dir_all(root.join("pics")).unwrap();
        std::fs::write(root.join("docs/report.pdf"), vec![0u8; 3000]).unwrap();
        std::fs::write(root.join("docs/notes.txt"), "hello world\nneedle here\n").unwrap();
        std::fs::write(root.join("docs/deep/old.txt"), "ancient").unwrap();
        std::fs::write(root.join("docs/deep/deeper/bottom.md"), "x").unwrap();
        std::fs::write(root.join("pics/cat.png"), vec![1u8; 20_000]).unwrap();
        std::fs::write(root.join("pics/dog.JPG"), vec![1u8; 10]).unwrap();
        std::fs::write(root.join(".hidden/secret.txt"), "needle").unwrap();
        std::fs::write(root.join("binary.dat"), b"nee\x00dle").unwrap();
        let old = SystemTime::now() - Duration::from_secs(30 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(root.join("docs/deep/old.txt"))
            .unwrap()
            .set_modified(old)
            .unwrap();
    }

    fn run(app: &App, expr: &str, root: &Path) -> BTreeSet<String> {
        let loc = location_for(expr, Some(root));
        let open = app
            .open_location(&loc)
            .unwrap_or_else(|e| panic!("{expr}: {e:#}"));
        let mut names = BTreeSet::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match open
                .events()
                .recv_deadline(deadline)
                .expect("query completes")
            {
                ListingEvent::Added(batch) | ListingEvent::Updated(batch) => {
                    for e in batch {
                        let p = open.path_of(e.id).expect("path_of");
                        names.insert(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
                    }
                }
                ListingEvent::Complete => break,
                ListingEvent::Error(e) => panic!("{expr}: {e}"),
                ListingEvent::Removed(_) | ListingEvent::Changed => {}
            }
        }
        names
    }

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn queries_over_a_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);

        assert_eq!(
            run(&app, "*.txt", &root),
            set(&["docs/notes.txt", "docs/deep/old.txt"])
        );
        assert_eq!(
            run(&app, "*.txt hidden:true", &root),
            set(&["docs/notes.txt", "docs/deep/old.txt", ".hidden/secret.txt"])
        );
        assert_eq!(
            run(&app, "ext:png,jpg", &root),
            set(&["pics/cat.png", "pics/dog.JPG"])
        );
        assert_eq!(
            run(&app, "type:dir", &root),
            set(&["docs", "docs/deep", "docs/deep/deeper", "pics"])
        );
        assert_eq!(run(&app, "type:dir depth:1", &root), set(&["docs", "pics"]));
        assert_eq!(
            run(&app, "size>1k", &root),
            set(&["docs/report.pdf", "pics/cat.png"])
        );
        assert_eq!(
            run(&app, "size<100 type:file", &root),
            set(&[
                "docs/notes.txt",
                "docs/deep/old.txt",
                "docs/deep/deeper/bottom.md",
                "pics/dog.JPG",
                "binary.dat"
            ])
        );
        assert_eq!(run(&app, "modified>7d", &root), set(&["docs/deep/old.txt"]));
        assert_eq!(
            run(&app, "*.txt modified<7d", &root),
            set(&["docs/notes.txt"])
        );
        assert_eq!(run(&app, "content:needle", &root), set(&["docs/notes.txt"]));
        assert_eq!(
            run(&app, "content:/^hello/", &root),
            set(&["docs/notes.txt"])
        );
        assert_eq!(
            run(&app, "NOT type:dir -ext:txt depth:2", &root),
            set(&[
                "docs/report.pdf",
                "pics/cat.png",
                "pics/dog.JPG",
                "binary.dat"
            ])
        );
        assert_eq!(
            run(&app, "cat OR dog", &root),
            set(&["pics/cat.png", "pics/dog.JPG"])
        );
        assert_eq!(
            run(&app, "mime:image/*", &root),
            set(&["pics/cat.png", "pics/dog.JPG"])
        );
        assert_eq!(
            run(&app, "kind:image", &root),
            set(&["pics/cat.png", "pics/dog.JPG"])
        );
        assert_eq!(
            run(&app, "kind:documents,folders depth:1", &root),
            set(&["docs", "pics"])
        );
        assert_eq!(
            run(&app, "kind:document", &root),
            set(&["docs/report.pdf", "docs/deep/deeper/bottom.md"])
        );
        // Whatever the time of day: last week began at least seven days ago,
        // and old.txt was modified thirty days ago.
        assert_eq!(
            run(&app, "kind:text modified>=last-week", &root),
            set(&["docs/notes.txt", "docs/deep/deeper/bottom.md"])
        );
        assert_eq!(
            run(&app, "(report OR notes) ext:txt", &root),
            set(&["docs/notes.txt"])
        );
        // `in:` inside the expression wins over the root argument; a relative
        // one cannot be resolved and the location refuses to open.
        assert!(
            app.open_location(&location_for("in:docs/deep", Some(&root)))
                .is_err()
        );
        assert_eq!(
            run(
                &app,
                &format!("in:{} ext:md", root.join("docs").display()),
                &root
            ),
            set(&["docs/deep/deeper/bottom.md"])
        );
    }

    #[test]
    fn a_link_is_found_as_what_it_leads_to() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        tree(&root);
        let link = |target: &str, name: &str| {
            std::os::unix::fs::symlink(target, root.join(name)).unwrap();
        };
        // No link's own name says what it leads to.
        link("pics/cat.png", "a cat's link");
        link("docs", "docs link");
        link("docs/notes.txt", "notes link");
        link("nowhere.png", "broken.png");
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);

        // Type terms match what a link opens; a broken link is of no kind.
        assert_eq!(
            run(&app, "kind:image", &root),
            set(&["pics/cat.png", "pics/dog.JPG", "a cat's link"])
        );
        assert_eq!(
            run(&app, "mime:image/png", &root),
            set(&["pics/cat.png", "a cat's link"])
        );
        assert_eq!(run(&app, "mime:inode/symlink", &root), set(&["broken.png"]));
        // A link to a folder is one, and a link still; the search does
        // not go down into it (no loops, no file found twice).
        assert_eq!(
            run(&app, "type:dir depth:1", &root),
            set(&["docs", "pics", "docs link"])
        );
        assert_eq!(
            run(&app, "type:symlink", &root),
            set(&["a cat's link", "docs link", "notes link", "broken.png"])
        );
        // What reads a file reads it through the link.
        assert_eq!(
            run(&app, "content:needle", &root),
            set(&["docs/notes.txt", "notes link"])
        );
        // Name terms keep the link's own name.
        assert_eq!(
            run(&app, "ext:png", &root),
            set(&["pics/cat.png", "broken.png"])
        );
    }

    #[test]
    fn a_pipe_is_found_as_what_it_is_and_never_read() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        tree(&root);
        // Named as a picture: the name says nothing of what it is.
        rustix::fs::mknodat(
            rustix::fs::CWD,
            root.join("stream.png"),
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o600),
            0,
        )
        .unwrap();
        std::os::unix::fs::symlink("stream.png", root.join("a pipe's link")).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);

        assert_eq!(
            run(&app, "kind:image", &root),
            set(&["pics/cat.png", "pics/dog.JPG"])
        );
        assert_eq!(
            run(&app, "mime:inode/fifo", &root),
            set(&["stream.png", "a pipe's link"])
        );
        assert!(!run(&app, "type:file", &root).contains("stream.png"));
        // A search that reads files skips it rather than wait for a writer.
        assert_eq!(run(&app, "content:needle", &root), set(&["docs/notes.txt"]));
        // Name terms keep its own name.
        assert!(run(&app, "ext:png", &root).contains("stream.png"));
    }

    #[test]
    fn hydrate_updates_and_removed_on_vanish() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let open = app
            .open_location(&location_for("*.png", Some(&root)))
            .unwrap();
        let mut listing = bibli_core::Listing::default();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !listing.is_complete() {
            listing.apply(open.events().recv_deadline(deadline).unwrap());
        }
        assert_eq!(listing.len(), 1);
        let id = listing.ids().next().unwrap();
        assert!(
            listing.get(id).unwrap().metadata.is_hydrated(),
            "matches are stat-ed for identity"
        );
        std::fs::remove_file(root.join("pics/cat.png")).unwrap();
        open.hydrate(&[id], bibli_core::scheduler::Priority::Visible);
        loop {
            let ev = open.events().recv_deadline(deadline).unwrap();
            let removed = matches!(ev, ListingEvent::Removed(_));
            listing.apply(ev);
            if removed {
                break;
            }
        }
        assert!(listing.is_empty());
    }

    /// The notification the last edit ended with: edits run on a worker,
    /// so each test step waits for it before the next.
    fn note(app: &App) -> bibli_core::app::Notification {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let mut notes = app.take_notifications();
            if let Some(last) = notes.pop() {
                assert!(notes.is_empty(), "one edit, one notification: {notes:?}");
                return last;
            }
            assert!(std::time::Instant::now() < deadline, "no notification");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Run `name` and return its notification's action (Undo or Redo).
    fn edit(app: &App, name: &str, args: serde_json::Value) -> Invocation {
        let invocation = Invocation {
            name: name.into(),
            args,
        };
        app.dispatch(&invocation).unwrap();
        let note = note(app);
        note.action
            .unwrap_or_else(|| panic!("{name}: {}", note.text))
            .command
    }

    fn exprs(app: &App) -> Vec<String> {
        saved(app).into_iter().map(|s| s.expr).collect()
    }

    #[test]
    fn saved_searches_are_kept_edited_and_undone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("it's a \"root\"");
        std::fs::create_dir(&root).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        assert!(app.provider(SCHEME).is_some());

        // The pane's search, kept through the core command the button and
        // Ctrl+D run, named after what it looks for and where.
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        app.dispatch(&Invocation::with_args(
            "query.search",
            &serde_json::json!({ "expr": "ext:pdf" }),
        ))
        .unwrap();
        let shown = app.pane_location(pane).unwrap();
        let first = shown.rest.clone();
        let undo = edit(&app, "keep", serde_json::json!({}));
        assert_eq!(exprs(&app), [first.as_str()]);
        assert_eq!(saved(&app)[0].name, "ext:pdf in it's a \"root\"");
        let places = app.registry.places.all();
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].group, GROUP);
        assert_eq!(
            places[0].location, shown,
            "the place is what the pane shows"
        );
        assert!(places[0].forget.is_some() && places[0].reorder.is_some());
        // Keeping it again says so and changes nothing.
        app.dispatch(&Invocation::new("keep")).unwrap();
        let again = note(&app);
        assert!(again.action.is_none() && again.text.contains("already saved"));
        // Undo, then Redo, from the notifications' buttons.
        let redo = edit(&app, &undo.name, undo.args.clone());
        assert!(exprs(&app).is_empty());
        assert_eq!(redo.args["undo"], serde_json::json!(false));
        edit(&app, &redo.name, redo.args.clone());
        assert_eq!(exprs(&app), [first.as_str()]);

        // An invalid search is refused before anything is written.
        let save = |name: &str, expr: &str| serde_json::json!({ "name": name, "expr": expr });
        assert!(
            app.dispatch(&Invocation::with_args("query.save", &save("bad", "(x")))
                .is_err()
        );
        edit(&app, "query.save", save("Big PDFs", "ext:pdf size>10M"));
        edit(&app, "query.save", save("Recent", "modified<7d"));
        let all = [first.as_str(), "ext:pdf size>10M", "modified<7d"];
        assert_eq!(exprs(&app), all);

        // Put in order by dragging: before another, or last; and back.
        let undo = edit(
            &app,
            "query.move",
            serde_json::json!({ "expr": "modified<7d", "before": first }),
        );
        assert_eq!(exprs(&app), [all[2], all[0], all[1]]);
        edit(&app, &undo.name, undo.args.clone());
        assert_eq!(exprs(&app), all);

        // Renamed; an empty name is the one made from the search.
        let undo = edit(
            &app,
            "query.rename",
            serde_json::json!({ "expr": "modified<7d", "name": "  " }),
        );
        assert_eq!(saved(&app)[2].name, "modified<7d");
        edit(&app, &undo.name, undo.args.clone());
        assert_eq!(saved(&app)[2].name, "Recent");

        // Edited: the search changes, its name and place stay.
        let change = |to: &str| serde_json::json!({ "expr": "ext:pdf size>10M", "to": to });
        assert!(
            app.dispatch(&Invocation::with_args("query.edit", &change("size>")))
                .is_err()
        );
        edit(&app, "query.edit", change(" ext:pdf size>1G "));
        assert_eq!(exprs(&app), [all[0], "ext:pdf size>1G", all[2]]);
        assert_eq!(saved(&app)[1].name, "Big PDFs");

        // Removed by its name, as before, or by the place's own command.
        edit(
            &app,
            "query.forget",
            serde_json::json!({ "name": "Recent" }),
        );
        assert!(
            app.dispatch(&Invocation::with_args(
                "query.forget",
                &serde_json::json!({ "name": "Recent" })
            ))
            .is_err()
        );
        let forget = app.registry.places.all()[0].forget.clone().unwrap();
        edit(&app, &forget.name, forget.args.clone());
        assert_eq!(exprs(&app), ["ext:pdf size>1G"]);

        assert!(app.registry.keymap.all().iter().any(|b| b.keys == "ctrl-f"));
        assert!(app.plugins.set_enabled(ID, false, &app));
        assert!(app.provider(SCHEME).is_none());
        assert!(app.commands.get("query.search").is_none());
        assert!(app.registry.places.all().is_empty());
        assert!(app.registry.keepers.all().is_empty());
        assert!(app.registry.keymap.all().is_empty());
        assert!(app.registry.menus.all().is_empty());
        assert!(
            app.settings
                .schema()
                .iter()
                .all(|s| !s.key.starts_with("query."))
        );
        // Nothing keeps searches any more.
        assert!(app.dispatch(&Invocation::new("keep")).is_err());
    }

    /// A terminal or a command run from a search works in the folder that
    /// holds the selection: the search itself is no folder.
    #[test]
    fn a_search_works_in_the_folder_of_its_selection() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &location_for("*.txt", Some(&root)))
            .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        assert!(app.pane_dir(pane).is_err());
        assert!(app.work_dir(pane).is_err(), "nothing selected, no folder");
        let select = |names: &[&str]| {
            app.dispatch(&Invocation::with_args(
                "select",
                &serde_json::json!({ "names": names }),
            ))
            .unwrap();
        };
        select(&["old.txt"]);
        assert_eq!(app.work_dir(pane).unwrap(), root.join("docs/deep"));
        select(&["old.txt", "notes.txt"]);
        assert_eq!(app.work_dir(pane).unwrap(), root.join("docs"));
    }

    /// The designer's path: a chip on a folder starts a search there, each
    /// further chip narrows it, and the pane's search describes itself.
    #[test]
    fn chips_start_and_narrow_a_search() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("it's here");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        let refine = |facet: &str, value: &str| {
            app.dispatch(&Invocation::with_args(
                "query.refine",
                &serde_json::json!({ "facet": facet, "value": value }),
            ))
            .unwrap();
            assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        };
        let names = || -> BTreeSet<String> {
            let ws = app.workspace.read();
            ws.pane(pane)
                .unwrap()
                .listing
                .iter()
                .map(|e| e.name_str().into_owned())
                .collect()
        };
        refine("kind", "image");
        assert_eq!(names(), set(&["cat.png", "dog.JPG"]));
        refine("size", ">1k");
        assert_eq!(names(), set(&["cat.png"]));
        let location = app.pane_location(pane).unwrap();
        assert_eq!(lang::parse(&location.rest).unwrap().root, root);
        let described = match app.dispatch(&Invocation::new("query.describe")).unwrap() {
            Outcome::Value(v) => v,
            other => panic!("{other:?}"),
        };
        let chip = |id: &str| {
            described["chips"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap()
                .clone()
        };
        assert_eq!(chip("kind")["value"], "Images");
        assert_eq!(chip("size")["value"], ">1k");
        assert_eq!(chip("where")["title"], "In it's here");
        // A choice's command is what the UI runs: clearing the size.
        let clear: Invocation = serde_json::from_value(chip("size")["clear"].clone()).unwrap();
        app.dispatch(&clear).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        assert_eq!(names(), set(&["cat.png", "dog.JPG"]));
    }

    #[test]
    fn a_suggested_name_says_what_and_where() {
        assert_eq!(suggested_name("*.pdf modified<7d"), "*.pdf modified<7d");
        assert_eq!(
            suggested_name(r#"ext:png in:"/pics/two words" depth:2"#),
            "ext:png in two words"
        );
        assert_eq!(suggested_name("in:/srv"), "Everything in srv");
        assert_eq!(suggested_name("in:/"), "Everything in /");
        assert_eq!(
            suggested_name(r#""two words" OR -draft type:file"#),
            "two words OR NOT draft type:file"
        );
    }

    #[test]
    fn search_command_navigates_the_pane() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "query.search",
            &serde_json::json!({"expr": "ext:pdf"}),
        ))
        .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert_eq!(p.location().unwrap().scheme.as_ref(), SCHEME);
        assert_eq!(p.listing.len(), 1);
        assert_eq!(p.listing.iter().next().unwrap().name_str(), "report.pdf");
    }

    #[test]
    fn plocate_is_used_for_name_only_queries_over_root() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let hit = tmp.path().join("hit-report.txt");
        std::fs::write(&hit, "x").unwrap();
        let script = format!(
            "#!/bin/sh\nprintf '%s\\0%s\\0' '{}' '/nonexistent/report.txt'\n",
            hit.display()
        );
        std::fs::write(bin.join("plocate"), script).unwrap();
        std::fs::set_permissions(bin.join("plocate"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let q = lang::parse("report in:/").unwrap();
        let paths =
            plocate::locate_with(&bin.join("plocate"), &q.name_substrings().unwrap(), false)
                .unwrap();
        assert_eq!(
            paths,
            vec![
                hit.clone(),
                std::path::PathBuf::from("/nonexistent/report.txt")
            ]
        );
        assert!(
            plocate::try_locate(&lang::parse("report in:/tmp").unwrap()).is_none(),
            "only over /"
        );
        assert!(
            plocate::try_locate(&lang::parse("report size>1k in:/").unwrap()).is_none(),
            "only name-only"
        );
        // A database that cannot answer (it says why) is not "no matches".
        std::fs::write(
            bin.join("broken"),
            "#!/bin/sh\necho 'no database' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(bin.join("broken"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert!(plocate::locate_with(&bin.join("broken"), &["x".into()], false).is_none());
        std::fs::write(bin.join("empty"), "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(bin.join("empty"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        assert_eq!(
            plocate::locate_with(&bin.join("empty"), &["x".into()], false),
            Some(Vec::new())
        );
    }

    #[test]
    fn a_facet_search_is_a_location_whoever_asks_needs_no_syntax() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let location = |args: serde_json::Value| {
            let Outcome::Value(v) = app
                .dispatch(&Invocation::with_args("query.location", &args))
                .unwrap()
            else {
                panic!("a value")
            };
            Location::parse(v["location"].as_str().unwrap())
        };
        let home = location(serde_json::json!({ "facet": "tag", "value": "it's \"done\" now" }));
        assert_eq!(&*home.scheme, SCHEME);
        let q = lang::parse(&home.rest).unwrap();
        assert_eq!(q.required_tag(), Some("it's \"done\" now"));
        assert!(!q.root_given, "the home folder: {}", home.rest);
        let root = Path::new("/two words/in:x");
        let there = location(serde_json::json!({
            "facet": "tag", "value": "work", "root": PathArg::from(root),
        }));
        let q = lang::parse(&there.rest).unwrap();
        assert_eq!((q.required_tag(), q.root.as_path()), (Some("work"), root));
        assert!(
            app.dispatch(&Invocation::with_args(
                "query.location",
                &serde_json::json!({ "facet": "nothing", "value": "x" }),
            ))
            .is_err()
        );
    }

    #[test]
    fn a_folder_whose_name_is_not_utf8_can_be_searched() {
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(std::ffi::OsStr::from_bytes(b"caf\xe9 (x)"));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.pdf"), b"x").unwrap();
        std::fs::write(root.join("b.txt"), b"x").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.dispatch(&Invocation::with_args(
            "query.search",
            &serde_json::json!({"expr": "ext:pdf", "root": PathArg::from(root.as_path())}),
        ))
        .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        assert_eq!(lang::parse(&p.location().unwrap().rest).unwrap().root, root);
        assert_eq!(p.listing.len(), 1);
        assert_eq!(p.listing.iter().next().unwrap().name_str(), "a.pdf");
    }

    #[test]
    fn a_root_that_is_not_a_folder_is_a_listing_error() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let open = app
            .open_location(&location_for("x", Some(&tmp.path().join("missing"))))
            .expect("opening does no I/O");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        match open.events().recv_deadline(deadline).unwrap() {
            ListingEvent::Error(e) => assert!(e.contains("not a folder"), "{e}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn location_for_reads_in_like_the_parser() {
        let root = Path::new("/r");
        // A quoted `in:` is part of a name: the root is still added.
        let l = location_for("name:\"a in:b\"", Some(root));
        assert_eq!(lang::parse(&l.rest).unwrap().root, root);
        let l = location_for("x in:/other", Some(root));
        assert_eq!(lang::parse(&l.rest).unwrap().root, Path::new("/other"));
    }

    #[test]
    fn tags_are_matched_in_batches_per_folder() {
        use bibli_core::entry::{Entry, Provenance, Value, keys};
        use bibli_core::metadata::MetadataProvider;
        /// Tags every `*.tagged` file, counting calls and entries.
        #[derive(Default)]
        struct Fake(parking_lot::Mutex<(usize, usize)>);
        impl MetadataProvider for Fake {
            fn id(&self) -> &'static str {
                "fake"
            }
            fn fill(&self, _: &Path, entries: &mut [Entry]) {
                let mut n = self.0.lock();
                n.0 += 1;
                n.1 += entries.len();
                for e in entries {
                    if e.name_str().ends_with(".tagged") {
                        e.metadata.set(
                            keys::TAGS,
                            Value::List(vec![Value::Text("Work".into())]),
                            Provenance::User,
                        );
                    }
                }
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        for i in 0..100 {
            let ext = if i % 10 == 0 { "tagged" } else { "plain" };
            std::fs::write(root.join(format!("f{i}.{ext}")), b"x").unwrap();
        }
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        let fake = Arc::new(Fake::default());
        app.metadata.register("fake", fake.clone());
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        assert_eq!(run(&app, "tag:work", &root).len(), 10);
        let (calls, entries) = *fake.0.lock();
        assert_eq!(entries, 100);
        assert!(calls <= 10, "{calls} provider calls for 100 files");
    }

    #[test]
    fn disabling_the_plugin_stops_its_walks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("t");
        std::fs::create_dir(&root).unwrap();
        tree(&root);
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins.add(Arc::new(QueryPlugin::default()), &app);
        let open = app
            .open_location(&location_for("txt", Some(&root)))
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !matches!(
            open.events().recv_deadline(deadline).unwrap(),
            ListingEvent::Complete
        ) {}
        assert!(app.plugins.set_enabled(ID, false, &app));
        // A location that outlives the plugin walks no more.
        open.refresh();
        let deadline = std::time::Instant::now() + Duration::from_millis(300);
        while let Ok(ev) = open.events().recv_deadline(deadline) {
            assert!(
                !matches!(ev, ListingEvent::Complete | ListingEvent::Added(_)),
                "walked after disable: {ev:?}"
            );
        }
    }
}
