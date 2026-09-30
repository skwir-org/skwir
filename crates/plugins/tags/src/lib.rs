//! bibli plugin: tags and the other notes a file carries in its extended
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

mod attrs;
pub mod finder;
mod hints;
mod ops;
mod vocabulary;

use std::collections::BTreeMap;
use std::sync::Arc;

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::keys;
use bibli_core::metadata::{TAG_COLOR_SETTING, TAG_COLORS, same_tag};
use bibli_core::op::{EntryRef, Plan};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem, MenuSource};
use bibli_core::scheduler::Priority;
use bibli_core::settings::SettingSpec;
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;

pub use attrs::{
    parse_tags, read_comment, read_file_tags, read_rating, read_tags, tags_of, write_tags,
};
pub use ops::{SetComment, SetRating, SetTags};

use vocabulary::Vocabulary;

pub(crate) const ID: &str = "tags";
/// The metadata field key.
pub const FIELD: &str = keys::TAGS;

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
    /// The entries to act on; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<EntryRef>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ToggleArgs {
    /// The tag to give or take.
    tag: String,
    /// The entries to act on; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<EntryRef>,
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
    /// The entries to act on; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<EntryRef>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct PaneArgs {
    /// The entries to act on; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<EntryRef>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct RateArgs {
    /// From 1 to 5 stars; 0 removes the rating.
    stars: u8,
    /// The entries to act on; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<EntryRef>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct CommentArgs {
    /// The comment; empty removes it.
    #[serde(default)]
    text: String,
    /// The entries to act on; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<EntryRef>,
    #[serde(default)]
    pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ColorArgs {
    tag: String,
    /// One of `red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`,
    /// `pink`; omitted, the tag wears the color its name gives it.
    #[serde(default)]
    color: Option<String>,
}

/// The entries a command acts on: those it names, or the pane's selection.
fn targets(app: &App, pane: Option<PaneId>, entries: Vec<EntryRef>) -> Vec<EntryRef> {
    if entries.is_empty() {
        app.selected_refs(app.pane_or_active(pane))
    } else {
        entries
    }
}

/// A plan of one step per target entry, built from what each entry holds
/// now (read from the file: the listing's field can be stale), leaving out
/// the entries it would not change.
fn per_entry<T: bibli_core::op::Operation + 'static>(
    refs: Vec<EntryRef>,
    title: String,
    step: impl Fn(EntryRef) -> Option<T>,
) -> anyhow::Result<Outcome> {
    if refs.is_empty() {
        anyhow::bail!("nothing selected");
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
/// already (tags ignore case).
fn edited(current: &[String], add: &[String], remove: &[String]) -> Vec<String> {
    let mut tags: Vec<String> = current
        .iter()
        .filter(|t| !remove.iter().any(|r| same_tag(r, t)))
        .cloned()
        .collect();
    for a in add {
        if !tags.iter().any(|t| same_tag(t, a)) {
            tags.push(a.clone());
        }
    }
    tags
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
    vocabulary.used(&add);
    per_entry(
        targets(app, a.pane, a.entries),
        edit_title(&add, &remove),
        |r| {
            let current = read_tags(&r.path);
            let tags = edited(&current, &add, &remove);
            (tags != current).then_some(SetTags {
                entry: r,
                tags,
                finder_colors: Vec::new(),
            })
        },
    )
}

/// `tags.selected`: what the tag editor starts from.
fn selected(app: &App, vocabulary: &Vocabulary, a: PaneArgs) -> serde_json::Value {
    let refs = targets(app, a.pane, a.entries);
    // Keyed by the lowercase form, in the order first met, with the color
    // a file gives the tag (a Finder tag's), if one does.
    let mut counts: Vec<(String, usize, Option<&str>)> = Vec::new();
    for r in &refs {
        let file = read_file_tags(&r.path);
        let colors = file.finder_colors();
        for tag in file.tags {
            let given = colors
                .iter()
                .find(|(t, _)| same_tag(t, &tag))
                .and_then(|(_, digit)| finder::palette(*digit))
                .map(|i| bibli_core::metadata::TAG_COLORS[i]);
            match counts.iter_mut().find(|(t, _, _)| same_tag(t, &tag)) {
                Some((_, n, color)) => {
                    *n += 1;
                    *color = color.or(given);
                }
                None => counts.push((tag, 1, given)),
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
    serde_json::json!({
        "items": refs.len(),
        "tags": tags,
        "known": vocabulary.known(),
    })
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
        "placeholder": "A comment on the selection (empty removes it)",
        "command": "tags.comment",
        "field": "text",
    });
    if let Some(text) = text {
        args["text"] = text.into();
    }
    Invocation::with_args("ui.ask", &args)
}

fn menu_item(title: &str, command: Invocation, order: i32, icon: &str) -> MenuItem {
    MenuItem {
        menu: "context".into(),
        title: title.into(),
        command,
        when: Some("selection".into()),
        order,
        icon: Some(icon.into()),
        submenu: None,
    }
}

/// Record a tag's color (`None`: the one its name gives it) in the
/// settings, on a worker.
fn set_color(app: &App, lifetime: &CancelToken, tag: &str, color: Option<&str>) {
    let (tag, color, weak) = (tag.to_lowercase(), color.map(str::to_owned), app.weak());
    app.scheduler.spawn(
        Priority::Normal,
        "tags.color",
        lifetime.child(),
        move || {
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
                    bibli_core::app::NotifyLevel::Error,
                    format!("The tag's color was not saved: {error:#}"),
                );
            }
            app.wake();
        },
    );
}

/// The tags plugin.
#[derive(Debug, Default)]
pub struct TagsPlugin {
    /// Ends with the plugin's registration: the vocabulary's reads and
    /// writes, a color being saved.
    lifetime: Mutex<Option<CancelToken>>,
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
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Files
    }
    fn on_disable(&self, _app: &App) {
        if let Some(lifetime) = self.lifetime.lock().take() {
            lifetime.cancel();
        }
    }
    #[allow(
        clippy::too_many_lines,
        reason = "one registration, read top to bottom"
    )]
    fn register(&self, app: &App) {
        let lifetime = app.lifetime().child();
        if let Some(previous) = self.lifetime.lock().replace(lifetime.clone()) {
            previous.cancel();
        }
        let vocabulary = Vocabulary::new(Some(app.cache.namespace(ID)), app, lifetime.clone());
        let index = hints::TagIndex::new(Some(app.cache.namespace(ID)), app, lifetime.clone());
        app.metadata.register(
            ID,
            Arc::new(attrs::AttributesProvider {
                vocabulary: Arc::clone(&vocabulary),
                index: Some(Arc::clone(&index)),
            }),
        );
        // What a `tag:` search asks first (ADR-026).
        app.hints.register(ID, Arc::new(hints::Baloo::default()));
        app.hints.register(ID, Arc::new(hints::Learned(index)));
        app.ops.register::<SetTags>(ID, "set_tags");
        app.ops.register::<SetRating>(ID, "set_rating");
        app.ops.register::<SetComment>(ID, "set_comment");
        app.settings.declare(SettingSpec::data(
            TAG_COLOR_SETTING,
            "Colors the user chose for tags, by tag in lowercase: { tag = color }",
            toml::Value::Table(toml::Table::new()),
        ));

        let v = Arc::clone(&vocabulary);
        app.commands.register::<TagsArgs, _>(
            ID,
            "tags.set",
            "Set tags",
            "Replace the selection's tags (plan)",
            move |a, app| {
                let tags = parse_tags(&a.tags.join(","));
                v.used(&tags);
                let title = if tags.is_empty() {
                    "remove tags".to_owned()
                } else {
                    format!("set tags {}", tags.join(", "))
                };
                per_entry(targets(app, a.pane, a.entries), title, |r| {
                    let current = read_tags(&r.path);
                    (current != tags).then(|| SetTags {
                        entry: r,
                        tags: tags.clone(),
                        finder_colors: Vec::new(),
                    })
                })
            },
        );
        let v = Arc::clone(&vocabulary);
        app.commands.register::<TagsArgs, _>(
            ID,
            "tags.add",
            "Add tags",
            "Add tags to the selection (plan)",
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
        );
        let v = Arc::clone(&vocabulary);
        app.commands.register::<TagsArgs, _>(
            ID,
            "tags.remove",
            "Remove tags",
            "Remove tags from the selection (plan)",
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
        );
        let v = Arc::clone(&vocabulary);
        app.commands.register::<EditArgs, _>(
            ID,
            "tags.edit",
            "Edit tags",
            "Add some tags to the selection and remove others, as one plan: each entry keeps its other tags",
            move |a, app| edit_plan(app, &v, a),
        );
        let v = Arc::clone(&vocabulary);
        app.commands.register::<ToggleArgs, _>(
            ID,
            "tags.toggle",
            "Toggle a tag",
            "Give the selection a tag, or take it from every entry when all of them have it: a command to bind to a key (plan)",
            move |a, app| {
                let Some(tag) = parse_tags(&a.tag).pop() else {
                    anyhow::bail!("which tag?");
                };
                let refs = targets(app, a.pane, a.entries);
                let all = !refs.is_empty()
                    && refs
                        .iter()
                        .all(|r| read_tags(&r.path).iter().any(|t| same_tag(t, &tag)));
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
                        entries: refs,
                        pane: a.pane,
                    },
                )
            },
        );
        let v = Arc::clone(&vocabulary);
        app.commands.register::<PaneArgs, _>(
            ID,
            "tags.selected",
            "Tags of the selection",
            "`{ items, tags: [{ name, count, color? }], known }`: the selection's tags, read from its files, with how many carry each and the color a file gives one (a Finder tag's), and the tags known, the most recently used first (blocks: dispatch from a worker)",
            move |a, app| Ok(Outcome::Value(selected(app, &v, a))),
        );
        app.commands.register::<RateArgs, _>(
            ID,
            "tags.rate",
            "Rate",
            "Rate the selection from 1 to 5 stars, or remove its rating with 0 (plan)",
            |a, app| {
                anyhow::ensure!(a.stars <= 5, "a rating is from 1 to 5 stars");
                let rating = (a.stars > 0).then_some(u64::from(a.stars) * 2);
                let title = rating.map_or_else(
                    || "remove the rating".to_owned(),
                    |r| format!("rate {}", ops::stars(r)),
                );
                per_entry(targets(app, a.pane, a.entries), title, |r| {
                    (read_rating(&r.path) != rating).then_some(SetRating { entry: r, rating })
                })
            },
        );
        app.commands.register::<CommentArgs, _>(
            ID,
            "tags.comment",
            "Comment",
            "Set the selection's comment, or remove it with an empty text (plan)",
            |a, app| {
                let text = a.text.trim();
                let comment = (!text.is_empty()).then(|| text.to_owned());
                let title = if comment.is_some() {
                    "set the comment"
                } else {
                    "remove the comment"
                };
                per_entry(targets(app, a.pane, a.entries), title.to_owned(), |r| {
                    (read_comment(&r.path) != comment).then(|| SetComment {
                        entry: r,
                        comment: comment.clone(),
                    })
                })
            },
        );
        let colors = lifetime.clone();
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
                set_color(app, &colors, tag, color);
                Ok(Outcome::Done)
            },
        );
        app.commands.register::<PaneArgs, _>(
            ID,
            "tags.menu",
            "Comment menu",
            "The context menu's Comment…, holding the selected entry's comment",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                let items = remembered_comment(app, pane).map(|comment| MenuItem {
                    submenu: Some("Notes".into()),
                    ..menu_item("Comment…", ask_comment(Some(&comment)), 160, "comment")
                });
                Ok(Outcome::Value(
                    serde_json::json!({ "items": items.into_iter().collect::<Vec<_>>() }),
                ))
            },
        );
        app.commands.register::<ListArgs, _>(
            ID,
            "tags.list",
            "List tags",
            "Tags used in the pane's listing, with counts (tags that differ only in case count as one); with `read`, read from the files whose tags the listing has not read yet (blocks: dispatch from a worker)",
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
                let open = {
                    let ws = app.workspace.read();
                    let Some(p) = ws.pane(pane) else {
                        anyhow::bail!("no such pane");
                    };
                    for e in p.listing.iter() {
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
                            count(read_tags(&path));
                        }
                    }
                }
                let counts: BTreeMap<String, usize> = counts.into_values().collect();
                Ok(Outcome::Value(serde_json::to_value(counts)?))
            },
        );
        app.registry.keymap.add(
            ID,
            KeyBinding {
                // Ctrl+Shift+T reopens a closed tab, as in file managers.
                keys: "ctrl-shift-g".into(),
                command: Invocation::new("ui.tags_prompt"),
                when: Some("pane selection".into()),
            },
        );
        app.registry.menus.add(
            ID,
            menu_item("Tags…", Invocation::new("ui.tags_prompt"), 150, "tag"),
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
                        Invocation::with_args("tags.rate", &serde_json::json!({ "stars": stars })),
                        151 + i32::from(5 - stars),
                        if stars == 0 { "minus" } else { "star" },
                    )
                },
            );
        }
        app.registry.menus.add(
            ID,
            MenuItem {
                submenu: Some("Notes".into()),
                ..menu_item("Comment…", ask_comment(None), 160, "comment")
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
mod tests {
    use super::*;
    use bibli_core::Value;
    use bibli_core::app::AppConfig;
    use bibli_core::location::Location;
    use std::path::Path;
    use std::time::Duration;

    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
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
        xattr::set(f, name, &finder::encode(&tags, framing)).unwrap();
    }

    fn finder_of(f: &Path, name: &str, framing: finder::Framing) -> Vec<(String, u8)> {
        let bytes = xattr::get(f, name).unwrap().expect("the Finder attribute");
        finder::parse(&bytes, framing)
            .unwrap()
            .into_iter()
            .map(|t| (t.name, t.color))
            .collect()
    }

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
        let raw = xattr::get(&shared, SAMBA).unwrap().unwrap();
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
        assert!(xattr::get(&copied, PLAIN).unwrap().is_none());
        assert!(xattr::get(&shared, SAMBA).unwrap().is_none());
    }

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
        let toggle = || {
            let p = plan(&app, "tags.toggle", &serde_json::json!({ "tag": "TODO" }));
            let r = app.executor.execute_blocking(&p, app.lifetime());
            assert!(r.error.is_none());
        };
        // One has it: both get it, in the spelling each already had.
        toggle();
        assert_eq!(read_tags(&a), ["Keep", "todo"]);
        assert_eq!(read_tags(&b), ["TODO"]);
        // Both have it: both lose it, and nothing else.
        toggle();
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
                bibli_core::entry::Provenance::User
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
        assert!(add.steps.is_empty(), "nothing to change");
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

        // tags.list counts what the listing knows (hydrate again to refresh).
        hydrate_all(&app, pane);
        let Outcome::Value(v) = app.dispatch(&Invocation::new("tags.list")).unwrap() else {
            panic!()
        };
        assert_eq!(v["old"], 1);

        // Disabling withdraws the provider, the op kinds, and the contributions.
        app.plugins.set_enabled("tags", false, &app);
        assert!(!app.metadata.ids().contains(&"tags"));
        for kind in ["set_tags", "set_rating", "set_comment"] {
            assert!(
                app.ops
                    .build(&bibli_core::op::OpData {
                        kind: kind.into(),
                        args: serde_json::json!({})
                    })
                    .is_err()
            );
        }
        assert!(app.commands.get("tags.set").is_none());
        assert!(app.registry.menus.all().iter().all(|m| m.title != "Tags…"));
    }

    /// The tag editor's edit: two files with different tags keep what the
    /// other has to themselves; a tag on some of them stays on those.
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
    }

    #[test]
    fn ratings_and_comments_are_set_undone_and_read() {
        let tmp = home_tempdir();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let f = dir.join("pic.jpg");
        std::fs::write(&f, b"x").unwrap();
        xattr::set(&f, attrs::ORIGIN, b"https://example.org/pic.jpg").unwrap();
        let (app, pane) = app_in(tmp.path(), &dir, &["pic.jpg"]);

        let rate = plan(&app, "tags.rate", &serde_json::json!({ "stars": 4 }));
        let changes: Vec<String> = rate
            .steps
            .iter()
            .map(|s| s.describe().to_string())
            .collect();
        assert_eq!(changes, ["rate “pic.jpg” 4 stars"]);
        app.executor.execute_blocking(&rate, app.lifetime());
        assert_eq!(read_rating(&f), Some(8));
        assert_eq!(
            xattr::get(&f, attrs::RATING).unwrap().unwrap(),
            b"8",
            "as Dolphin writes it"
        );
        let comment = plan(
            &app,
            "tags.comment",
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
                m.value(keys::RATING).and_then(bibli_core::Value::as_u64),
                Some(8)
            );
            assert_eq!(
                m.value(keys::COMMENT).and_then(bibli_core::Value::as_str),
                Some("from the “trip”, day 2")
            );
            assert_eq!(
                m.value(keys::ORIGIN).and_then(bibli_core::Value::as_str),
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
        assert!(xattr::get(&f, attrs::RATING).unwrap().is_none());
        assert!(
            app.dispatch(&Invocation::with_args(
                "tags.rate",
                &serde_json::json!({ "stars": 6 })
            ))
            .is_err()
        );
    }

    /// A link carries what it leads to: the target's tags.
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
            edited(&t(&["A", "b"]), &t(&["a", "c"]), &t(&["B"])),
            t(&["A", "c"])
        );
    }
}
