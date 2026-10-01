//! The sidebar's Tags: those the user pinned, in the order they put
//! them (`tags.pinned`); until one is, suggestions, the tags on the most
//! files the index knows under the home folder, drawn fainter. Each is a
//! search of the home folder for it, in the color it wears (the user's
//! choice, else a file's, else its name's). Published when the index or
//! the pins change, once for changes close together, and only when what
//! the sidebar shows changed.

use std::sync::Arc;

use bibli_core::app::App;
use bibli_core::app::NotifyLevel;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;
use bibli_core::metadata::{TAG_COLOR_SETTING, TAG_COLORS, same_tag, tag_color};
use bibli_core::registry::{MenuItem, Place, Reorder};
use parking_lot::Mutex;

use crate::hints::TagIndex;
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

/// A tag row's own items: pin or unpin it, rename it or take it from
/// every file (each a plan to review), or choose its color.
fn menu(tag: &str, pinned: bool) -> Vec<MenuItem> {
    let mut items = vec![
        if pinned {
            menu_item(
                "Unpin From Sidebar",
                Invocation::with_args("tags.unpin", &serde_json::json!({ "tag": tag })),
                50,
                "pin",
            )
        } else {
            menu_item(
                "Pin to Sidebar",
                Invocation::with_args("tags.pin", &serde_json::json!({ "tag": tag })),
                50,
                "pin",
            )
        },
        menu_item(
            "Rename Tag…",
            Invocation::with_args(
                "ui.ask",
                &serde_json::json!({
                    "title": format!("Rename the tag “{tag}” on every file"),
                    "placeholder": "New name (an existing tag's merges them)",
                    "text": tag,
                    "command": "tags.rename",
                    "args": { "tag": tag },
                    "field": "to",
                }),
            ),
            100,
            "rename",
        ),
        menu_item(
            "Remove Tag From Every File…",
            Invocation::with_args("tags.delete", &serde_json::json!({ "tag": tag })),
            300,
            "trash",
        ),
    ];
    let mut color = |title: &str, color: Option<&str>, order| {
        items.push(MenuItem {
            submenu: Some("Color".into()),
            ..menu_item(
                title,
                Invocation::with_args(
                    "tags.color",
                    &serde_json::json!({ "tag": tag, "color": color }),
                ),
                order,
                "appearance",
            )
        });
    };
    color("From Its Name", None, 200);
    for (i, name) in TAG_COLORS.iter().enumerate() {
        let mut title = (*name).to_owned();
        title[..1].make_ascii_uppercase();
        color(
            &title,
            Some(name),
            201 + i32::try_from(i).unwrap_or_default(),
        );
    }
    items
}

/// Where a search for `tag` is: the query plugin's to say. `None` while
/// it is off, with nothing to search with.
fn location(app: &App, tag: &str) -> Option<String> {
    let ask = Invocation::with_args(
        "query.location",
        &serde_json::json!({ "facet": "tag", "value": tag }),
    );
    app.commands.get(&ask.name)?;
    match app.dispatch(&ask) {
        Ok(Outcome::Value(v)) => v["location"].as_str().map(str::to_owned),
        _ => None,
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

/// An edit of the pins, and what undoes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pin {
    /// Pin `tag` before `before`, or last; moves it if pinned.
    Pin {
        tag: String,
        before: Option<String>,
    },
    Unpin {
        tag: String,
    },
}

impl Pin {
    /// Apply to `list`: the edit that reverts it, `None` when nothing
    /// changed.
    pub fn apply(&self, list: &mut Vec<String>) -> Option<Pin> {
        let at = |list: &[String], tag: &str| list.iter().position(|t| same_tag(t, tag));
        match self {
            Pin::Pin { tag, before } => {
                let was = at(list, tag);
                // Before itself: where it is.
                if was.is_some() && before.as_deref().is_some_and(|b| same_tag(b, tag)) {
                    return None;
                }
                let back = match was {
                    Some(i) => Pin::Pin {
                        tag: list[i].clone(),
                        before: list.get(i + 1).cloned(),
                    },
                    None => Pin::Unpin { tag: tag.clone() },
                };
                let mut moved = list.clone();
                if let Some(i) = was {
                    moved.remove(i);
                }
                let to = before
                    .as_deref()
                    .and_then(|b| at(&moved, b))
                    .unwrap_or(moved.len());
                moved.insert(to, tag.clone());
                (moved != *list).then(|| {
                    *list = moved;
                    back
                })
            }
            Pin::Unpin { tag } => {
                let i = at(list, tag)?;
                let back = Pin::Pin {
                    tag: list.remove(i),
                    before: list.get(i).cloned(),
                };
                Some(back)
            }
        }
    }

    /// The command that applies this edit, from a toast's button.
    pub fn invocation(&self, undo: bool) -> Invocation {
        match self {
            Pin::Pin { tag, before } => Invocation::with_args(
                "tags.pin",
                &serde_json::json!({ "tag": tag, "before": before, "undo": undo }),
            ),
            Pin::Unpin { tag } => Invocation::with_args(
                "tags.unpin",
                &serde_json::json!({ "tag": tag, "undo": undo }),
            ),
        }
    }

    /// What it did, in a sentence.
    fn words(&self, was_pinned: bool) -> String {
        match self {
            Pin::Pin { tag, .. } if was_pinned => format!("Moved the tag “{tag}”"),
            Pin::Pin { tag, .. } => format!("Pinned the tag “{tag}” to the sidebar"),
            Pin::Unpin { tag } => format!("Unpinned the tag “{tag}”"),
        }
    }
}

/// Apply `edit` to the pins and save them, on a worker; a toast says what
/// changed, with Undo (Redo once `undo`). `then` runs once they are saved.
pub fn edit(
    app: &App,
    saving: &Arc<Mutex<()>>,
    edit: Pin,
    undo: bool,
    lifetime: &bibli_core::cancel::CancelToken,
    then: impl FnOnce(&App) + Send + 'static,
) {
    let (saving, weak) = (Arc::clone(saving), app.weak());
    app.scheduler.spawn(
        bibli_core::scheduler::Priority::Normal,
        "tags.pin",
        lifetime.child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let _saving = saving.lock();
            let mut list = pinned(&app);
            let was_pinned = match &edit {
                Pin::Pin { tag, .. } | Pin::Unpin { tag } => list.iter().any(|t| same_tag(t, tag)),
            };
            let Some(inverse) = edit.apply(&mut list) else {
                return;
            };
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
                app.notify_with_action(
                    NotifyLevel::Info,
                    edit.words(was_pinned),
                    if undo { "Redo" } else { "Undo" },
                    inverse.invocation(!undo),
                );
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
    let colors: toml::Table = app.settings.get(TAG_COLOR_SETTING).unwrap_or_default();
    let row = |tag: String, pinned: bool| {
        let location = location(app, &tag)?;
        let color = colors
            .get(&tag.to_lowercase())
            .and_then(toml::Value::as_str)
            .filter(|c| TAG_COLORS.contains(c))
            .map(str::to_owned)
            .or_else(|| vocabulary.given_color(&tag))
            .unwrap_or_else(|| TAG_COLORS[tag_color(&tag)].to_owned());
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
        let mut rows: Vec<Row> = index
            .top(usize::try_from(at_most).unwrap_or_default(), &home)
            .into_iter()
            .filter_map(|(tag, _)| row(tag, false))
            .collect();
        // In the order of their names, so a pill stays put while counts
        // move.
        rows.sort_by_key(|r| r.tag.to_lowercase());
        rows
    } else {
        pins.into_iter().filter_map(|tag| row(tag, true)).collect()
    };
    {
        let mut last = shown.0.lock();
        if last.as_ref() == Some(&rows) {
            return;
        }
        *last = Some(rows.clone());
    }
    let places = rows.into_iter().map(|r| {
        let one = serde_json::json!({ "tag": &r.tag });
        Place {
            menu: menu(&r.tag, r.pinned),
            drop: Some(Invocation::with_args(
                "tags.add",
                &serde_json::json!({ "tags": [&r.tag] }),
            )),
            forget: r.pinned.then(|| Invocation::with_args("tags.unpin", &one)),
            reorder: r.pinned.then(|| Reorder {
                command: Invocation::with_args("tags.pin", &one),
                before: serde_json::json!(&r.tag),
            }),
            suggested: !r.pinned,
            title: r.tag,
            location: Location::parse(&r.location),
            icon: None,
            group: GROUP.into(),
            space: None,
            eject: None,
            activate: None,
            unavailable: None,
            busy: None,
            color: Some(r.color),
        }
    });
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
    fn pins_move_and_come_undone_where_they_were() {
        let mut pins = list(&["a", "b", "c"]);
        let pin = |tag: &str, before: Option<&str>| Pin::Pin {
            tag: tag.into(),
            before: before.map(str::to_owned),
        };
        // A new pin, before b; undone, it is gone.
        let back = pin("Été", Some("b")).apply(&mut pins).unwrap();
        assert_eq!(pins, list(&["a", "Été", "b", "c"]));
        assert_eq!(
            back,
            Pin::Unpin {
                tag: "Été".into()
            }
        );
        back.apply(&mut pins).unwrap();
        assert_eq!(pins, list(&["a", "b", "c"]));
        // Moved last, and back before c.
        let back = pin("A", None).apply(&mut pins).unwrap();
        assert_eq!(pins, list(&["b", "c", "A"]));
        assert_eq!(back, pin("a", Some("b")));
        // Unpinned, and back where it was.
        let back = Pin::Unpin { tag: "c".into() }.apply(&mut pins).unwrap();
        assert_eq!(pins, list(&["b", "A"]));
        back.apply(&mut pins).unwrap();
        assert_eq!(pins, list(&["b", "c", "A"]));
        // Nothing moves where it already is.
        assert!(pin("c", Some("A")).apply(&mut pins).is_none());
        assert!(pin("b", Some("b")).apply(&mut pins).is_none());
        assert!(Pin::Unpin { tag: "z".into() }.apply(&mut pins).is_none());
    }
}
