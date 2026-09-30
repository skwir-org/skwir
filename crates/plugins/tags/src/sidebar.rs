//! The sidebar's Tags: the tags on the most files the index knows under
//! the home folder, each a search of the home folder for it, in the color
//! it wears (the user's choice, else a file's, else its name's). Published
//! when the index changes, once for changes close together, and only when
//! what the sidebar shows changed.

use std::sync::Arc;

use bibli_core::app::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;
use bibli_core::metadata::{TAG_COLOR_SETTING, TAG_COLORS, tag_color};
use bibli_core::registry::{MenuItem, Place};
use parking_lot::Mutex;

use crate::hints::TagIndex;
use crate::vocabulary::Vocabulary;

pub const GROUP: &str = "tags";
/// How many tags the sidebar lists.
pub const SETTING: &str = "tags.sidebar";
pub const DEFAULT: i64 = 10;

/// What the sidebar was last given: each tag, its location and color.
#[derive(Default)]
pub struct Shown(Mutex<Option<Vec<(String, String, String)>>>);

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

/// A tag row's own items: rename it or take it from every file (each a
/// plan to review), or choose its color.
fn menu(tag: &str) -> Vec<MenuItem> {
    let mut items = vec![
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

/// Publish the sidebar's tags from `index` if they changed. Reads the
/// index: call on a worker.
pub fn publish(app: &App, index: &Arc<TagIndex>, vocabulary: &Vocabulary, shown: &Shown) {
    let Some(home) = std::env::home_dir() else {
        return;
    };
    let at_most = app
        .settings
        .get::<i64>(SETTING)
        .unwrap_or(DEFAULT)
        .clamp(0, 50);
    let colors: toml::Table = app.settings.get(TAG_COLOR_SETTING).unwrap_or_default();
    let mut rows: Vec<(String, String, String)> = index
        .top(usize::try_from(at_most).unwrap_or_default(), &home)
        .into_iter()
        .filter_map(|(tag, _)| {
            let location = location(app, &tag)?;
            let color = colors
                .get(&tag.to_lowercase())
                .and_then(toml::Value::as_str)
                .filter(|c| TAG_COLORS.contains(c))
                .map(str::to_owned)
                .or_else(|| vocabulary.given_color(&tag))
                .unwrap_or_else(|| TAG_COLORS[tag_color(&tag)].to_owned());
            Some((tag, location, color))
        })
        .collect();
    // In the order of their names, so a row stays put while counts move.
    rows.sort_by_key(|(tag, _, _)| tag.to_lowercase());
    {
        let mut last = shown.0.lock();
        if last.as_ref() == Some(&rows) {
            return;
        }
        *last = Some(rows.clone());
    }
    let places = rows.into_iter().map(|(tag, location, color)| Place {
        menu: menu(&tag),
        title: tag,
        location: Location::parse(&location),
        icon: None,
        group: GROUP.into(),
        space: None,
        eject: None,
        activate: None,
        unavailable: None,
        busy: None,
        forget: None,
        reorder: None,
        color: Some(color),
    });
    if app.registry.places.replace_owner(crate::ID, places) {
        app.wake();
    }
}
