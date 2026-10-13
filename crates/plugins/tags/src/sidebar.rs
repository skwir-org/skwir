//! The sidebar's Tags: those the user pinned, in the order they put
//! them (`tags.pinned`); until one is, suggestions, the tags on the most
//! files the index knows under the home folder, drawn fainter. Each opens
//! its tag folder of the home folder (`tags:/family`, ADR-032), in the
//! color it wears (the user's choice, else a file's, else its name's);
//! All Tags opens them all (`tags:/`). Published when the index or the
//! pins change, once for changes close together, and only when what the
//! sidebar shows changed.

use std::sync::Arc;

use parking_lot::Mutex;
use skwir_core::app::App;
use skwir_core::app::NotifyLevel;
use skwir_core::command::Invocation;
use skwir_core::location::Location;
use skwir_core::metadata::{
    TAG_COLOR_SETTING, TAG_COLORS, same_tag, tag_color, tag_lineage, tag_parent,
};
use skwir_core::registry::{Click, Place, Reorder};

use crate::index::TagIndex;
use crate::vocabulary::Vocabulary;

pub const GROUP: &str = "tags";
/// How many tags the sidebar suggests while none is pinned.
pub const SETTING: &str = "tags.sidebar";
pub const DEFAULT: i64 = 12;
/// The tags pinned to the sidebar, in their order.
pub const PINNED: &str = "tags.pinned";

/// A tag as the sidebar shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    tag: String,
    location: String,
    color: String,
    /// Pinned by the user, else suggested.
    pinned: bool,
}

/// What the sidebar was last given.
#[derive(Default)]
pub struct Shown(Mutex<Option<Vec<Row>>>);

impl Shown {
    pub fn forget(&self) {
        *self.0.lock() = None;
    }
}

/// Asking for a tag's new name on every file: its menu's Rename Tag…,
/// and F2 on its pill.
fn rename(tag: &str) -> Invocation {
    Invocation::with_args(
        "ui.ask",
        &serde_json::json!({
            "title": format!("Rename the tag “{tag}” on every file"),
            "accept": "Rename",
            "placeholder": "New name (an existing tag's merges them)",
            "text": tag,
            "command": "tags.rename",
            "args": { "tag": tag },
            "field": "to",
        }),
    )
}

/// Where a tag's pill leads: its tag folder of the home folder.
fn location(tag: &str) -> String {
    crate::location::Address {
        tag: Some(tag.to_owned()),
        ..crate::location::Address::default()
    }
    .location()
    .to_string()
}

/// The colors the user chose for tags, read once for many.
pub struct Colors(toml::Table);

impl Colors {
    pub fn of(app: &App) -> Self {
        Colors(app.settings.get(TAG_COLOR_SETTING).unwrap_or_default())
    }

    /// The color `tag` wears: the user's choice, else a file's, else the
    /// one the user chose for its nearest category, else its name's.
    pub fn of_tag(&self, vocabulary: &Vocabulary, tag: &str) -> String {
        let chosen = |t: &str| {
            self.0
                .get(&t.to_lowercase())
                .and_then(toml::Value::as_str)
                .filter(|c| TAG_COLORS.contains(c))
                .map(str::to_owned)
        };
        chosen(tag)
            .or_else(|| vocabulary.given_color(tag))
            .or_else(|| tag_lineage(tag).skip(1).find_map(chosen))
            .unwrap_or_else(|| TAG_COLORS[tag_color(tag)].to_owned())
    }
}

/// The tags pinned, in their order, each once.
pub fn pinned(app: &App) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in app.settings.get::<Vec<String>>(PINNED).unwrap_or_default() {
        let tag = tag.trim();
        if !tag.is_empty() && !tags.iter().any(|t| same_tag(t, tag)) {
            tags.push(tag.to_owned());
        }
    }
    tags
}

/// An edit of the pins. Its undo is the pins as they were (`Set`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pin {
    /// Pin `tags` before `before`, or last; moves those pinned.
    Put {
        tags: Vec<String>,
        before: Option<String>,
    },
    Unpin {
        tags: Vec<String>,
    },
    /// The pins as listed: an undo or a redo.
    Set(Vec<String>),
}

impl Pin {
    /// Apply to `list`: whether it changed.
    pub fn apply(&self, list: &mut Vec<String>) -> bool {
        let named = |tags: &[String], t: &str| tags.iter().any(|x| same_tag(x, t));
        let mut out: Vec<String> = match self {
            Pin::Put { tags, before } => {
                // Before one of those it moves: before the next that stays,
                // so a tag pinned before itself stays where it is.
                let before = before.as_deref().and_then(|b| {
                    if !named(tags, b) {
                        return Some(b.to_owned());
                    }
                    let at = list.iter().position(|t| same_tag(t, b))?;
                    list[at..].iter().find(|t| !named(tags, t)).cloned()
                });
                let mut out: Vec<String> =
                    list.iter().filter(|t| !named(tags, t)).cloned().collect();
                let at = before
                    .and_then(|b| out.iter().position(|t| same_tag(t, &b)))
                    .unwrap_or(out.len());
                let pinned = tags.iter().map(|t| {
                    // A tag pinned keeps the spelling it was pinned with.
                    list.iter().find(|x| same_tag(x, t)).unwrap_or(t).clone()
                });
                out.splice(at..at, pinned);
                out
            }
            Pin::Unpin { tags } => list.iter().filter(|t| !named(tags, t)).cloned().collect(),
            Pin::Set(tags) => tags.clone(),
        };
        out.dedup_by(|a, b| same_tag(a, b));
        let changed = out != *list;
        *list = out;
        changed
    }

    /// What it did, in a sentence: `was` the pins before.
    fn words(&self, was: &[String]) -> String {
        let pinned = |t: &str| was.iter().any(|x| same_tag(x, t));
        match self {
            Pin::Put { tags, .. } => match tags.as_slice() {
                [tag] if pinned(tag) => format!("Moved the tag “{tag}”"),
                [tag] => format!("Pinned the tag “{tag}” to the sidebar"),
                _ => format!("Pinned {} tags to the sidebar", tags.len()),
            },
            Pin::Unpin { tags } => match tags.as_slice() {
                [tag] => format!("Unpinned the tag “{tag}”"),
                _ => format!("Unpinned {} tags", tags.len()),
            },
            Pin::Set(_) => "Put the pinned tags back as they were".to_owned(),
        }
    }
}

/// The command that puts the pins back as `pins`, from a toast's button.
fn set_invocation(pins: &[String], undo: bool) -> Invocation {
    Invocation::with_args(
        "tags.pins",
        &serde_json::json!({ "tags": pins, "undo": undo }),
    )
}

/// Apply `edit` to the pins and save them, on a worker; a toast says what
/// changed, with Undo (Redo once `undo`). `then` runs once they are saved.
pub fn edit(
    app: &App,
    saving: &Arc<Mutex<()>>,
    edit: Pin,
    undo: bool,
    lifetime: &skwir_core::cancel::CancelToken,
    then: impl FnOnce(&App) + Send + 'static,
) {
    let (saving, weak) = (Arc::clone(saving), app.weak());
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Normal,
        "tags.pin",
        lifetime.child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let _saving = saving.lock();
            let was = pinned(&app);
            let mut list = was.clone();
            if !edit.apply(&mut list) {
                return;
            }
            app.settings.set(
                PINNED,
                toml::Value::Array(list.into_iter().map(toml::Value::String).collect()),
            );
            if let Err(error) = app.settings.save() {
                app.notify(
                    NotifyLevel::Error,
                    format!("The pinned tags were not saved: {error:#}"),
                );
            } else {
                app.notify_undoable(edit.words(&was), undo, set_invocation(&was, !undo));
            }
            then(&app);
            app.wake();
        },
    );
}

/// Publish the sidebar's tags if they changed: those pinned, else the
/// suggestions from `index`. Reads the index: call on a worker.
pub fn publish(app: &App, index: &Arc<TagIndex>, vocabulary: &Vocabulary, shown: &Shown) {
    let Some(home) = std::env::home_dir() else {
        return;
    };
    // One at a time: a publish from the pins as they were (the index's
    // news) must not land after one from the pins as they are.
    let mut last = shown.0.lock();
    let colors = Colors::of(app);
    let row = |tag: String, pinned: bool| {
        let location = location(&tag);
        let color = colors.of_tag(vocabulary, &tag);
        Some(Row {
            tag,
            location,
            color,
            pinned,
        })
    };
    let pins = pinned(app);
    let rows: Vec<Row> = if pins.is_empty() {
        let at_most = app
            .settings
            .get::<i64>(SETTING)
            .unwrap_or(DEFAULT)
            .clamp(0, 50);
        // The tags at the top: a category stands for those within it,
        // which its search narrows to.
        let at_most = usize::try_from(at_most).unwrap_or_default();
        let mut rows: Vec<Row> = index
            .top(usize::MAX, &home)
            .into_iter()
            .filter(|(tag, _)| tag_parent(tag).is_none())
            .take(at_most)
            .filter_map(|(tag, _)| row(tag, false))
            .collect();
        // In the order of their names, so a pill stays put while counts
        // move.
        rows.sort_by_key(|r| r.tag.to_lowercase());
        rows
    } else {
        pins.into_iter().filter_map(|tag| row(tag, true)).collect()
    };
    if last.as_ref() == Some(&rows) {
        return;
    }
    *last = Some(rows.clone());
    let places = rows.into_iter().map(|r| {
        let one = serde_json::json!({ "tag": &r.tag });
        // Its menu is the tag's (`tags.tag_menu`, a source of the places'
        // menus), read when it opens.
        Place {
            forget: r.pinned.then(|| Invocation::with_args("tags.unpin", &one)),
            reorder: r.pinned.then(|| Reorder {
                command: Invocation::with_args("tags.pin", &one),
                before: serde_json::json!(&r.tag),
            }),
            suggested: !r.pinned,
            rename: Some(rename(&r.tag)),
            color: Some(r.color),
            // Shift: the files shown that have it too; Alt: its files in the
            // folder shown (`tags.pill`).
            clicks: [("shift", "also"), ("alt", "here")]
                .into_iter()
                .map(|(modifier, how)| Click {
                    modifier: modifier.into(),
                    command: Invocation::with_args(
                        "tags.pill",
                        &serde_json::json!({ "tag": &r.tag, "how": how }),
                    ),
                })
                .collect(),
            ..Place::new(GROUP, r.tag.clone(), "", Location::parse(&r.location))
        }
    });
    // Last, every tag: the home folder's tags, browsed as folders; with
    // Alt, those of the folder shown (`tags.browse`), as a tag's pill
    // shows its files there.
    let all = Some(Place {
        clicks: vec![Click {
            modifier: "alt".into(),
            command: Invocation::new("tags.browse"),
        }],
        ..Place::new(
            GROUP,
            "All Tags",
            "tag",
            crate::location::Address::default().location(),
        )
    });
    let places = places.chain(all);
    if app.registry.places.replace_owner(crate::ID, places) {
        app.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::Pin;

    fn list(v: &[&str]) -> Vec<String> {
        v.iter().map(|t| (*t).to_owned()).collect()
    }

    #[test]
    fn pins_go_where_they_are_put_and_come_back_as_they_were() {
        let mut pins = list(&["a", "b", "c"]);
        let pin = |tags: &[&str], before: Option<&str>| Pin::Put {
            tags: list(tags),
            before: before.map(str::to_owned),
        };
        let was = pins.clone();
        assert!(pin(&["Été"], Some("b")).apply(&mut pins));
        assert_eq!(pins, list(&["a", "Été", "b", "c"]));
        assert!(Pin::Set(was).apply(&mut pins));
        assert_eq!(pins, list(&["a", "b", "c"]));
        // Moved last, keeping how it was spelled.
        assert!(pin(&["A"], None).apply(&mut pins));
        assert_eq!(pins, list(&["b", "c", "a"]));
        // Several at once, new and pinned, before another.
        assert!(pin(&["x", "a"], Some("c")).apply(&mut pins));
        assert_eq!(pins, list(&["b", "x", "a", "c"]));
        assert!(
            Pin::Unpin {
                tags: list(&["X", "c"])
            }
            .apply(&mut pins)
        );
        assert_eq!(pins, list(&["b", "a"]));
        // Nothing moves where it already is.
        assert!(!pin(&["a"], None).apply(&mut pins));
        assert!(!pin(&["b"], Some("b")).apply(&mut pins));
        assert!(!pin(&["b"], Some("a")).apply(&mut pins));
        assert!(!Pin::Unpin { tags: list(&["z"]) }.apply(&mut pins));
    }
}
