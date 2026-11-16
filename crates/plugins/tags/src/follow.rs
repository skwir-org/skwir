//! What the user set about a tag (its pin, its color) follows it when it
//! is renamed, merged or removed on every file under the home folder: a
//! step of the same plan, so the review shows it and an undo puts the pins
//! and colors back with the files. A rename under a folder leaves them:
//! the tag still names files elsewhere.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Weak;

use skwir_core::app::App;
use skwir_core::metadata::{TAG_COLOR_SETTING, same_tag, tag_under};
use skwir_core::op::{Change, Operation, StepContext, StepStatus, Tenses};
use skwir_core::words;
use skwir_core::words::{Name, Words};

use crate::Sidebar;
use crate::sidebar::{self, PINNED};

pub const FOLLOW: &str = "tags_follow";
pub const RESTORE: &str = "tags_settings";

/// What a step needs of the plugin: the app, and the sidebar to publish
/// the pins it changed.
#[derive(Clone)]
pub struct Ctx {
    app: Weak<App>,
    sidebar: Sidebar,
}

impl Ctx {
    pub fn new(app: &App, sidebar: Sidebar) -> Self {
        Ctx {
            app: app.weak(),
            sidebar,
        }
    }
}

/// Pins and the colors of some tags, as they are or are to be.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub pins: Vec<String>,
    /// By tag in lowercase; `None` for the color its name gives it.
    pub colors: BTreeMap<String, Option<String>>,
    /// The categories holding one tag at a time, when they change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusive: Option<Vec<String>>,
}

/// The pins and colors `from` renamed `to` (`None`: removed) leaves:
/// `None` when nothing changes. `colors` is the user's choices. A
/// category takes what is within it along: `Places/France` pinned
/// follows `Places` renamed `Lieux` as `Lieux/France`.
pub fn followed(
    pins: &[String],
    colors: &toml::Table,
    exclusive: &[String],
    from: &[String],
    to: Option<&str>,
) -> Option<Settings> {
    // What `t` becomes: `None` when no `from` holds it, `Some(None)` gone.
    let moved = |t: &str| -> Option<Option<String>> {
        from.iter()
            .find_map(|f| tag_under(t, f))
            .map(|rest| to.map(|to| format!("{to}{rest}")))
    };
    let mut out = Settings {
        pins: Vec::new(),
        colors: BTreeMap::new(),
        exclusive: None,
    };
    // A category holding one tag at a time keeps that renamed.
    let mut fields: Vec<String> = Vec::new();
    for field in exclusive {
        let field = match moved(field) {
            None => field.clone(),
            Some(None) => continue,
            Some(Some(new)) => new,
        };
        if !fields.iter().any(|f| same_tag(f, &field)) {
            fields.push(field);
        }
    }
    if fields != exclusive {
        out.exclusive = Some(fields);
    }
    for pin in pins {
        let pin = match moved(pin) {
            None => pin.clone(),
            Some(None) => continue,
            Some(Some(new)) => new,
        };
        // Merged into a tag pinned already: where the first of them was.
        if !out.pins.iter().any(|p| same_tag(p, &pin)) {
            out.pins.push(pin);
        }
    }
    if to.is_some() {
        for f in from {
            for (key, color) in colors {
                let (Some(Some(new)), Some(color)) = (
                    tag_under(key, f).map(|rest| to.map(|to| format!("{to}{rest}"))),
                    color.as_str(),
                ) else {
                    continue;
                };
                let new = new.to_lowercase();
                if new == *key {
                    continue;
                }
                out.colors.entry(key.clone()).or_insert(None);
                if !colors.contains_key(&new) && !out.colors.get(&new).is_some_and(Option::is_some)
                {
                    out.colors.insert(new, Some(color.to_owned()));
                }
            }
        }
    }
    (out.pins != pins || !out.colors.is_empty() || out.exclusive.is_some()).then_some(out)
}

/// The settings as they are, for the keys `changing` names: what puts
/// them back.
fn current(app: &App, changing: &Settings) -> Settings {
    let colors: toml::Table = app.settings.get(TAG_COLOR_SETTING).unwrap_or_default();
    Settings {
        pins: sidebar::pinned(app),
        exclusive: changing
            .exclusive
            .as_ref()
            .map(|_| crate::fields::exclusive(app)),
        colors: changing
            .colors
            .keys()
            .map(|k| {
                (
                    k.clone(),
                    colors
                        .get(k)
                        .and_then(toml::Value::as_str)
                        .map(str::to_owned),
                )
            })
            .collect(),
    }
}

/// Write `to`, returning what was there.
fn write(app: &App, ctx: &Ctx, to: &Settings) -> anyhow::Result<Settings> {
    let saving = ctx.sidebar.saving.lock();
    let before = current(app, to);
    let mut colors: toml::Table = app.settings.get(TAG_COLOR_SETTING).unwrap_or_default();
    for (key, color) in &to.colors {
        match color {
            Some(c) => colors.insert(key.clone(), toml::Value::String(c.clone())),
            None => colors.remove(key),
        };
    }
    app.settings
        .set(TAG_COLOR_SETTING, toml::Value::Table(colors));
    app.settings.set(
        PINNED,
        toml::Value::Array(to.pins.iter().cloned().map(toml::Value::String).collect()),
    );
    if let Some(fields) = &to.exclusive {
        app.settings.set(
            skwir_core::metadata::TAG_EXCLUSIVE_SETTING,
            toml::Value::Array(fields.iter().cloned().map(toml::Value::String).collect()),
        );
    }
    app.settings.save().map_err(|e| {
        e.context(words!(
            tags_pins_not_saved = "the pinned tags were not saved"
        ))
    })?;
    drop(saving);
    ctx.sidebar.publish(app);
    Ok(before)
}

fn change(root: &std::path::Path, will: Words, did: Words, already: Words) -> Change {
    Change::Metadata {
        path: root.to_path_buf(),
        field: PINNED.into(),
        words: Some(Tenses {
            will,
            did,
            already: Some(already),
        }),
    }
}

/// What a step says when the plugin that made it is off.
fn plugin_off() -> Words {
    words!(
        tags_plugin_off = "the Tags plugin is off",
        note = "Why a step of a plan about tags' pins and colors cannot run."
    )
}

/// “a”, “b”.
///
/// Transitional: a list is joined here until words take a list of names
/// that each language joins its own way (PLAN.md, ADR-139 step F).
fn quoted(tags: &[String]) -> String {
    tags.iter()
        .map(|t| format!("“{t}”"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The pins and colors follow `from` renamed `to`, or removed.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Follow {
    pub from: Vec<String>,
    pub to: Option<String>,
    /// The home folder, where the plan acts.
    #[serde(with = "skwir_core::os_path")]
    pub root: PathBuf,
    /// Whether the pins and the colors move, as the plan was made: its
    /// words stay what the review said once it is applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    moves: Option<(bool, bool)>,
    #[serde(skip)]
    ctx: Option<Ctx>,
}

impl std::fmt::Debug for Follow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Follow")
            .field("from", &self.from)
            .field("to", &self.to)
            .finish_non_exhaustive()
    }
}

impl Follow {
    pub fn new(ctx: &Ctx, from: Vec<String>, to: Option<String>, root: PathBuf) -> Self {
        let mut follow = Follow {
            from,
            to,
            root,
            moves: None,
            ctx: Some(ctx.clone()),
        };
        follow.moves = follow.what_moves();
        follow
    }

    /// Whether the pins and the colors would move now.
    fn what_moves(&self) -> Option<(bool, bool)> {
        let app = self.ctx.as_ref()?.app.upgrade()?;
        let target = self.target(&app)?;
        Some((
            target.pins != sidebar::pinned(&app),
            !target.colors.is_empty(),
        ))
    }

    /// What it would leave, `None` when nothing changes.
    pub fn target(&self, app: &App) -> Option<Settings> {
        let colors: toml::Table = app.settings.get(TAG_COLOR_SETTING).unwrap_or_default();
        followed(
            &sidebar::pinned(app),
            &colors,
            &crate::fields::exclusive(app),
            &self.from,
            self.to.as_deref(),
        )
    }
}

impl Operation for Follow {
    fn kind(&self) -> &'static str {
        FOLLOW
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        let from = quoted(&self.from);
        // What moves, when known: the pin, the color, or both.
        let (pins, colors) = self
            .moves
            .or_else(|| self.what_moves())
            .unwrap_or((true, true));
        let named = |tag: &String| Name(tag.clone());
        match &self.to {
            Some(new) if pins && !colors => change(
                &self.root,
                words!(
                    tags_follow_pin_will = "pin { $to } where { $from } was",
                    to = named(new),
                    from = from.as_str(),
                    note = "A step of renaming tags. to: the tag's new name; from: the tags renamed, each quoted, joined by commas."
                ),
                words!(
                    tags_follow_pin_did = "pinned { $to } where { $from } was",
                    to = named(new),
                    from = from.as_str(),
                    note = "As tags_follow_pin_will, done."
                ),
                words!(
                    tags_follow_pin_already = "{ $to } was already pinned where { $from } was",
                    to = named(new),
                    from = from.as_str(),
                    note = "As tags_follow_pin_will, found done already."
                ),
            ),
            Some(new) if colors && !pins => change(
                &self.root,
                words!(
                    tags_follow_color_will = "give { $to } the color { $from } had",
                    to = named(new),
                    from = from.as_str(),
                    note = "A step of renaming tags. to: the tag's new name; from: the tags renamed, each quoted, joined by commas."
                ),
                words!(
                    tags_follow_color_did = "gave { $to } the color { $from } had",
                    to = named(new),
                    from = from.as_str(),
                    note = "As tags_follow_color_will, done."
                ),
                words!(
                    tags_follow_color_already = "{ $to } already had the color { $from } had",
                    to = named(new),
                    from = from.as_str(),
                    note = "As tags_follow_color_will, found done already."
                ),
            ),
            Some(new) => change(
                &self.root,
                words!(
                    tags_follow_both_will = "keep { $to } pinned and colored as { $from } were",
                    to = named(new),
                    from = from.as_str(),
                    note = "A step of renaming tags. to: the tag's new name; from: the tags renamed, each quoted, joined by commas."
                ),
                words!(
                    tags_follow_both_did = "kept { $to } pinned and colored as { $from } were",
                    to = named(new),
                    from = from.as_str(),
                    note = "As tags_follow_both_will, done."
                ),
                words!(
                    tags_follow_both_already =
                        "{ $to } was already pinned and colored as { $from } were",
                    to = named(new),
                    from = from.as_str(),
                    note = "As tags_follow_both_will, found done already."
                ),
            ),
            None => change(
                &self.root,
                words!(
                    tags_follow_unpin_will = "unpin { $from }",
                    from = from.as_str(),
                    note = "A step of removing tags from every file. from: the tags, each quoted, joined by commas."
                ),
                words!(
                    tags_follow_unpin_did = "unpinned { $from }",
                    from = from.as_str(),
                    note = "As tags_follow_unpin_will, done."
                ),
                words!(
                    tags_follow_unpin_already = "{ $from } was already unpinned",
                    from = from.as_str(),
                    note = "As tags_follow_unpin_will, found done already."
                ),
            ),
        }
    }
    fn check(&self, _: &StepContext) -> StepStatus {
        let app = self.ctx.as_ref().and_then(|c| c.app.upgrade());
        match app {
            None => StepStatus::Conflict {
                reason: plugin_off().say(),
            },
            Some(app) if self.target(&app).is_none() => StepStatus::AlreadyDone,
            Some(_) => StepStatus::Pending { reversible: true },
        }
    }
    fn apply(&self, _: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let ctx = self
            .ctx
            .as_ref()
            .ok_or_else(|| anyhow::Error::from(plugin_off()))?;
        let app = ctx
            .app
            .upgrade()
            .ok_or_else(|| anyhow::Error::from(plugin_off()))?;
        let Some(target) = self.target(&app) else {
            return Ok(None);
        };
        let before = write(&app, ctx, &target)?;
        Ok(Some(Box::new(Restore {
            settings: before,
            root: self.root.clone(),
            ctx: Some(ctx.clone()),
        })))
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
}

/// The pins and some colors put as they were: an undo of [`Follow`].
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Restore {
    pub settings: Settings,
    #[serde(with = "skwir_core::os_path")]
    pub root: PathBuf,
    #[serde(skip)]
    ctx: Option<Ctx>,
}

impl std::fmt::Debug for Restore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Restore")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl Operation for Restore {
    fn kind(&self) -> &'static str {
        RESTORE
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        change(
            &self.root,
            words!(
                tags_restore_will = "put the pinned tags and their colors back",
                note = "A step that undoes what renaming tags did to their pins and colors."
            ),
            words!(
                tags_restore_did = "put the pinned tags and their colors back",
                note = "As tags_restore_will, done."
            ),
            words!(
                tags_restore_already = "the pinned tags and their colors were already back",
                note = "As tags_restore_will, found done already."
            ),
        )
    }
    fn check(&self, _: &StepContext) -> StepStatus {
        if self.ctx.as_ref().and_then(|c| c.app.upgrade()).is_some() {
            StepStatus::Pending { reversible: true }
        } else {
            StepStatus::Conflict {
                reason: plugin_off().say(),
            }
        }
    }
    fn apply(&self, _: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let ctx = self
            .ctx
            .as_ref()
            .ok_or_else(|| anyhow::Error::from(plugin_off()))?;
        let app = ctx
            .app
            .upgrade()
            .ok_or_else(|| anyhow::Error::from(plugin_off()))?;
        let before = write(&app, ctx, &self.settings)?;
        Ok(Some(Box::new(Restore {
            settings: before,
            root: self.root.clone(),
            ctx: Some(ctx.clone()),
        })))
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
}

/// Register both kinds, so a plan read back (the journal, an agent's)
/// holds them.
pub fn register(app: &App, ctx: &Ctx) {
    let c = ctx.clone();
    app.ops.register_with(crate::ID, FOLLOW, move |v| {
        let mut op: Follow = serde_json::from_value(v)?;
        op.ctx = Some(c.clone());
        Ok(Box::new(op))
    });
    let c = ctx.clone();
    app.ops.register_with(crate::ID, RESTORE, move |v| {
        let mut op: Restore = serde_json::from_value(v)?;
        op.ctx = Some(c.clone());
        Ok(Box::new(op))
    });
}

/// Whether a plan acting under `root` (the home folder when `None`) acts
/// on every file a tag names.
pub fn everywhere(root: Option<&std::path::Path>) -> Option<PathBuf> {
    let home = std::env::home_dir()?;
    match root {
        None => Some(home),
        Some(r)
            if r == home
                || skwir_core::os_path::canonical(r).ok()
                    == skwir_core::os_path::canonical(&home).ok() =>
        {
            Some(home)
        }
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::followed;

    fn list(v: &[&str]) -> Vec<String> {
        v.iter().map(|t| (*t).to_owned()).collect()
    }

    #[test]
    fn a_pin_and_a_color_follow_a_tag_renamed_or_merged() {
        let mut colors = toml::Table::new();
        colors.insert("trip".into(), toml::Value::String("teal".into()));
        // Renamed: the pin stays where it was, the color goes with it.
        let s = followed(
            &list(&["a", "Trip", "b"]),
            &colors,
            &[],
            &list(&["trip"]),
            Some("travel"),
        )
        .unwrap();
        assert_eq!(s.pins, list(&["a", "travel", "b"]));
        assert_eq!(s.colors.get("travel"), Some(&Some("teal".into())));
        assert_eq!(s.colors.get("trip"), Some(&None));
        // Merged into one pinned already: one pin, its own color kept.
        colors.insert("b".into(), toml::Value::String("red".into()));
        let s = followed(
            &list(&["trip", "a", "b"]),
            &colors,
            &[],
            &list(&["trip", "a"]),
            Some("b"),
        )
        .unwrap();
        assert_eq!(s.pins, list(&["b"]));
        assert_eq!(s.colors.get("b"), None, "b keeps red");
        // Removed: unpinned, colors left alone.
        let s = followed(&list(&["a", "trip"]), &colors, &[], &list(&["trip"]), None).unwrap();
        assert_eq!(s.pins, list(&["a"]));
        assert!(s.colors.is_empty());
        // Nothing of theirs: nothing to do.
        assert!(followed(&list(&["a"]), &colors, &[], &list(&["zz"]), Some("y")).is_none());
    }

    #[test]
    fn what_is_within_a_category_follows_it() {
        let mut colors = toml::Table::new();
        colors.insert("places/france".into(), toml::Value::String("blue".into()));
        let s = followed(
            &list(&["Places/France", "work", "Placesque"]),
            &colors,
            &list(&["Places/Status"]),
            &list(&["places"]),
            Some("Lieux"),
        )
        .unwrap();
        assert_eq!(
            s.exclusive,
            Some(list(&["Lieux/Status"])),
            "a field follows"
        );
        assert_eq!(s.pins, list(&["Lieux/France", "work", "Placesque"]));
        assert_eq!(s.colors.get("lieux/france"), Some(&Some("blue".into())));
        assert_eq!(s.colors.get("places/france"), Some(&None));
        // Moved into a category.
        let s = followed(
            &list(&["Paris"]),
            &colors,
            &[],
            &list(&["Paris"]),
            Some("Places/Paris"),
        )
        .unwrap();
        assert_eq!(s.pins, list(&["Places/Paris"]));
        // A category removed takes its pins.
        let s = followed(
            &list(&["Places/France", "a"]),
            &colors,
            &[],
            &list(&["Places"]),
            None,
        )
        .unwrap();
        assert_eq!(s.pins, list(&["a"]));
    }
}
