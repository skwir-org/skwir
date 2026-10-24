//! What gestures do in a tag location (ADR-032, `location.rs`): files
//! dropped or pasted on a tag are given it (`tags.drop`), moved from
//! another tag they lose that one, and tags dragged onto it go into it;
//! Delete takes the tag off them (`tags.untag`), and a tag selected is
//! taken off every file, a plan to review; New Folder makes a tag
//! (`tags.new`); the Narrow by bar and the sidebar's pills narrow it
//! (`tags.narrow`, `tags.pill`); Ctrl+D pins its tag (`tags.keep`); a tag
//! selected has its own menu items (`tags.place_menu`).

use std::path::Path;

use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::entry::keys;
use skwir_core::location::Location;
use skwir_core::metadata::{
    TAG_COLORS, TAG_EXCLUSIVE_SETTING, clean_tag, same_tag, tag_leaf, tag_parent,
};
use skwir_core::op::EntryRef;
use skwir_core::os_path::PathArg;
use skwir_core::registry::MenuItem;
use skwir_core::workspace::PaneId;
use skwir_platform::mime::Kind;

use crate::location::{Address, SCHEME};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DropArgs {
    /// The tag location dropped on (`tags:/Places/France`).
    pub location: String,
    /// The pane they come from: its selection, and where it is.
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// Files by where they are (another application's, the clipboard's;
    /// a script's or an agent's, by absolute path), instead of a pane's.
    #[serde(default)]
    pub entries: Vec<skwir_core::op::EntryArg>,
    /// They move: out of the tag they came from.
    #[serde(default, rename = "move")]
    pub moved: bool,
    /// Where `entries` were cut or copied, or the column of a board the
    /// pane's selection was dragged from.
    #[serde(default)]
    pub from: Option<String>,
    /// Tag locations pasted with `entries`: their tags go into this one.
    #[serde(default)]
    pub places: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UntagArgs {
    /// The tag location they are taken out of; the pane's when omitted.
    #[serde(default)]
    pub location: Option<String>,
    /// The files to take out, by absolute path; the pane's selection
    /// (its files, and its tags, taken off every file) when omitted.
    #[serde(default)]
    pub entries: Vec<skwir_core::op::EntryArg>,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NewArgs {
    /// The tag location the tag is made in.
    pub location: String,
    /// Its name, in the tag shown.
    pub name: String,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NarrowArgs {
    /// The tag location narrowed; the pane's when omitted.
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// `also_tag` (files that also have the tag), `not_tag` (none that
    /// have it), `kind`, `modified` (a year), `deep` (every file within).
    pub facet: String,
    #[serde(default)]
    pub value: Option<String>,
    /// Narrowed so (`true`) or not; switched when omitted.
    #[serde(default)]
    pub on: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PillArgs {
    /// The tag of the sidebar's pill.
    pub tag: String,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// `also`: the files shown that have it too (Shift+click); `here`: its
    /// files in the folder shown (Alt+click).
    pub how: String,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct BrowseArgs {
    /// The folder whose tags are shown, an absolute path; when omitted,
    /// the one the pane shows, or the one what it shows is of (a tag
    /// location's, a search's).
    #[serde(default, rename = "in")]
    pub folder: Option<PathArg>,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// With no folder named, from a tag location: back to the folder it is
    /// of, so that one gesture shows a folder's tags and leaves them (the
    /// address's Tags button, Alt+G).
    #[serde(default)]
    pub toggle: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExportArgs {
    /// The tag location exported.
    pub location: String,
    /// The folder it is copied into.
    pub dest: PathArg,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct CountedArgs {
    /// The folder whose tags; the home folder when omitted.
    #[serde(default)]
    pub root: Option<PathArg>,
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct PaneOnly {
    #[serde(default)]
    pub pane: Option<PaneId>,
}

/// The tag location `location` is, if it is one.
fn address_of(location: &Location) -> Option<Address> {
    (&*location.scheme == SCHEME).then(|| Address::parse(&location.rest))
}

/// The tag location shown by `pane`, or named.
fn shown(app: &App, pane: PaneId, named: Option<&str>) -> anyhow::Result<Address> {
    let location = match named {
        Some(text) => Location::parse(text),
        None => app
            .pane_location(pane)
            .ok_or_else(|| anyhow::anyhow!("the pane shows nothing"))?,
    };
    address_of(&location).ok_or_else(|| anyhow::anyhow!("the pane shows no tags"))
}

/// The tags of the places selected in `pane` (a tag location's tags).
pub fn selected_places(app: &App, pane: PaneId) -> Vec<String> {
    let ws = app.workspace.read();
    let Some(p) = ws.pane(pane) else {
        return Vec::new();
    };
    p.selection
        .iter()
        .filter(|id| id.is_place())
        .filter_map(|id| {
            p.entry(id)?
                .metadata
                .value(keys::PLACE_TAG)?
                .as_str()
                .map(str::to_owned)
        })
        .collect()
}

/// What taking a file out of `address` takes from it: its tag, or every
/// tag within it where it lists everything within; `None` at the top,
/// where it takes every tag.
fn taken(address: &Address) -> Option<skwir_core::notes::TagChange> {
    let tag = address.tag.clone()?;
    Some(if address.deep {
        skwir_core::notes::TagChange {
            within: vec![tag],
            ..Default::default()
        }
    } else {
        skwir_core::notes::TagChange {
            remove: vec![tag],
            ..Default::default()
        }
    })
}

/// `tags.drop`.
pub fn drop(
    app: &App,
    lifetime: &CancelToken,
    follows: &crate::follow::Ctx,
    a: DropArgs,
) -> anyhow::Result<Outcome> {
    let target = shown(app, app.pane_or_active(a.pane), Some(&a.location))?;
    let Some(tag) = target.tag.clone() else {
        // On the top of the tags: tags come out of their categories.
        let pasted: Vec<String> = a
            .places
            .iter()
            .filter_map(|l| address_of(&Location::parse(l))?.tag)
            .collect();
        let places = if a.entries.is_empty() && pasted.is_empty() {
            selected_places(app, app.pane_or_active(a.pane))
        } else {
            pasted
        };
        let places: Vec<String> = places
            .into_iter()
            .filter(|p| tag_parent(p).is_some())
            .collect();
        anyhow::ensure!(
            !places.is_empty(),
            "files are given one tag: drop them on it"
        );
        crate::everywhere::move_into(app, lifetime, follows, places, None, target.root())?;
        return Ok(Outcome::Done);
    };
    let pasted: Vec<String> = a
        .places
        .iter()
        .filter_map(|l| address_of(&Location::parse(l))?.tag)
        .collect();
    let (files, places, from) = if a.entries.is_empty() && pasted.is_empty() {
        let pane = app.pane_or_active(a.pane);
        // Dragged from a column of a board, from that column's tag.
        let from = a
            .from
            .as_deref()
            .map(Location::parse)
            .or_else(|| app.pane_location(pane));
        (app.selected_refs(pane), selected_places(app, pane), from)
    } else {
        let from = a.from.as_deref().map(Location::parse);
        (skwir_core::op::EntryArg::refs(a.entries)?, pasted, from)
    };
    // Tags dragged onto it go into it, on every file: a plan to review.
    let places: Vec<String> = places
        .into_iter()
        .filter(|p| {
            !same_tag(p, &tag) && tag_parent(p).is_none_or(|parent| !same_tag(parent, &tag))
        })
        .collect();
    if !places.is_empty() {
        crate::everywhere::move_into(app, lifetime, follows, places, Some(&tag), target.root())?;
    }
    if files.is_empty() {
        return Ok(Outcome::Done);
    }
    // Moved from another tag: out of that one.
    let source = from
        .filter(|_| a.moved)
        .and_then(|l| address_of(&l))
        .filter(|s| s.tag.as_deref().is_some_and(|t| !same_tag(t, &tag)));
    let exclusive = crate::fields::exclusive(app);
    let title = match source.as_ref().and_then(|s| s.tag.as_deref()) {
        Some(from) => format!("move from tag {from} to tag {tag}"),
        None => format!("add tag {tag}"),
    };
    // What each file holds is read when the plan is applied: the tag of
    // the source taken (and what is under it, where it shows that), this
    // one given.
    let change = skwir_core::notes::TagChange {
        add: vec![tag.clone()],
        exclusive,
        ..source.as_ref().and_then(taken).unwrap_or_default()
    };
    crate::per_entry(files, title, |r| {
        let name = name_of(&r.path);
        let (will, did) = match source.as_ref().and_then(|s| s.tag.as_deref()) {
            Some(from) => (
                format!("move “{name}” from “{from}” to “{tag}”"),
                format!("moved “{name}” from “{from}” to “{tag}”"),
            ),
            None => (
                format!("give “{name}” the tag “{tag}”"),
                format!("gave “{name}” the tag “{tag}”"),
            ),
        };
        Some(
            app.notes
                .edit(r)
                .change_tags(change.clone())
                .saying(will, did),
        )
    })
}

/// A file's name, as a sentence says it.
fn name_of(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// `tags.untag`.
pub fn untag(
    app: &App,
    lifetime: &CancelToken,
    follows: &crate::follow::Ctx,
    a: &UntagArgs,
) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let address = shown(app, pane, a.location.as_deref())?;
    let files = if a.entries.is_empty() {
        let places = selected_places(app, pane);
        if !places.is_empty() {
            crate::everywhere::start(app, lifetime, follows, places, None, address.root())?;
        }
        app.selected_refs(pane)
    } else {
        skwir_core::op::EntryArg::refs(a.entries.clone())?
    };
    if files.is_empty() {
        return Ok(Outcome::Done);
    }
    let title = match &address.tag {
        Some(tag) => format!("remove tag {tag}"),
        None => "remove all tags".to_owned(),
    };
    // Read when the plan is applied, as `drop`'s.
    let change = taken(&address);
    crate::per_entry(files, title, |r| {
        let name = name_of(&r.path);
        let (will, did) = match &address.tag {
            Some(tag) => (
                format!("remove the tag “{tag}” from “{name}”"),
                format!("removed the tag “{tag}” from “{name}”"),
            ),
            None => (
                format!("remove the tags of “{name}”"),
                format!("removed the tags of “{name}”"),
            ),
        };
        let edit = match &change {
            Some(change) => app.notes.edit(r).change_tags(change.clone()),
            None => app.notes.edit(r).tags(Vec::new()),
        };
        Some(edit.saying(will, did))
    })
}

/// `tags.new`.
pub fn new(
    app: &App,
    places: &std::sync::Arc<crate::location::Places>,
    a: &NewArgs,
) -> anyhow::Result<Outcome> {
    let address = Address::parse(&Location::parse(&a.location).rest.clone());
    let name = clean_tag(&a.name);
    anyhow::ensure!(!name.is_empty(), "a tag needs a name");
    anyhow::ensure!(!name.contains(','), "a tag's name has no comma");
    let tag = match &address.tag {
        Some(parent) => format!("{parent}/{name}"),
        None => name,
    };
    let root = address
        .root()
        .ok_or_else(|| anyhow::anyhow!("no home folder"))?;
    places.draft(&root, &tag);
    // Selected once listed, to drop files on.
    let pane = app.pane_or_active(a.pane);
    if let Some(p) = app.workspace.write().pane_mut(pane) {
        p.reveal_coming(tag_leaf(&tag).into());
    }
    app.wake();
    Ok(Outcome::Done)
}

/// `tags.export`: a folder of the tag's files, and of the tags within it,
/// made in `dest`, as a plan proposed for review. Reads the index and
/// stats every file: on a worker.
pub fn export(
    app: &App,
    places: &std::sync::Arc<crate::location::Places>,
    a: ExportArgs,
) -> anyhow::Result<Outcome> {
    let address = Address::parse(&Location::parse(&a.location).rest);
    let tag = address
        .tag
        .clone()
        .ok_or_else(|| anyhow::anyhow!("a tag is copied, not all of them"))?;
    let root = address
        .root()
        .ok_or_else(|| anyhow::anyhow!("no home folder"))?;
    let dest = a.dest.into_inner();
    let (places, weak) = (std::sync::Arc::clone(places), app.weak());
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Normal,
        "tags.export",
        app.lifetime().child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let files = places.tagged(&address, &root);
            let Some(made) = crate::location::export(&address, &files, &dest, Path::exists) else {
                return;
            };
            let mut steps: Vec<Box<dyn skwir_core::op::Operation>> = made
                .folders
                .iter()
                .map(|path| {
                    Box::new(skwir_core::op::Mkdir { path: path.clone() })
                        as Box<dyn skwir_core::op::Operation>
                })
                .collect();
            for (from, folder, name) in made.copies {
                let Some(entry) = EntryRef::from_path(&from) else {
                    continue;
                };
                let mut copy = skwir_core::op::Copy::from_settings(entry, folder, &app.settings);
                if from.file_name() != Some(name.as_os_str()) {
                    copy.new_name = Some(name);
                }
                steps.push(Box::new(copy));
            }
            let place = dest.file_name().map_or_else(
                || dest.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            app.propose(skwir_core::op::Plan::with_steps(
                format!("copy the files tagged {tag} to {place}"),
                steps,
            ));
            app.wake();
        },
    );
    Ok(Outcome::Done)
}

/// `address` narrowed by `facet` (`value`), or not any more: `on` says
/// which, else it switches.
pub fn narrowed(
    mut address: Address,
    facet: &str,
    value: Option<&str>,
    on: Option<bool>,
) -> anyhow::Result<Address> {
    fn switch<T: PartialEq>(
        list: &mut Vec<T>,
        value: T,
        on: Option<bool>,
        same: impl Fn(&T, &T) -> bool,
    ) {
        let at = list.iter().position(|v| same(v, &value));
        match (at, on) {
            (Some(i), None | Some(false)) => {
                list.remove(i);
            }
            (None, None | Some(true)) => list.push(value),
            _ => {}
        }
    }
    let value = value.map(str::trim).unwrap_or_default();
    match facet {
        "also_tag" | "with" => {
            let tag = clean_tag(value);
            anyhow::ensure!(!tag.is_empty(), "which tag?");
            address.without.retain(|t| !same_tag(t, &tag));
            switch(&mut address.with, tag, on, |a, b| same_tag(a, b));
        }
        "not_tag" | "without" => {
            let tag = clean_tag(value);
            anyhow::ensure!(!tag.is_empty(), "which tag?");
            address.with.retain(|t| !same_tag(t, &tag));
            switch(&mut address.without, tag, on, |a, b| same_tag(a, b));
        }
        "kind" => {
            let kind =
                Kind::from_name(value).ok_or_else(|| anyhow::anyhow!("no kind “{value}”"))?;
            switch(&mut address.kinds, kind, on, PartialEq::eq);
        }
        "modified" | "year" => {
            let year: i64 = value
                .parse()
                .map_err(|_| anyhow::anyhow!("a year, as four digits"))?;
            switch(&mut address.years, year, on, PartialEq::eq);
        }
        "deep" => address.deep = on.unwrap_or(!address.deep),
        other => anyhow::bail!("tag locations are not narrowed by `{other}`"),
    }
    Ok(address)
}

/// `tags.narrow`.
pub fn narrow(app: &App, a: &NarrowArgs) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let address = shown(app, pane, a.location.as_deref())?;
    let address = narrowed(address, &a.facet, a.value.as_deref(), a.on)?;
    app.navigate(pane, &address.location())?;
    Ok(Outcome::Done)
}

/// `tags.pill`: a sidebar's tag clicked with Shift (`also`) or Alt
/// (`here`).
pub fn pill(app: &App, a: &PillArgs) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let tag = clean_tag(&a.tag);
    anyhow::ensure!(!tag.is_empty(), "which tag?");
    let shown = app.pane_location(pane);
    if let Some(location) = shown.as_ref().filter(|l| &*l.scheme == "query") {
        // A search narrows itself, as the query plugin says.
        let _ = location;
        let facet = if a.how == "also" { "also_tag" } else { "tag" };
        return app.dispatch(&Invocation::with_args(
            "query.refine",
            &serde_json::json!({ "facet": facet, "value": tag, "pane": pane }),
        ));
    }
    let address = match (shown.as_ref().and_then(address_of), a.how.as_str()) {
        (Some(here), "also") => narrowed(here, "also_tag", Some(&tag), None)?,
        (Some(here), _) => Address {
            tag: Some(tag),
            scope: here.scope,
            ..Address::default()
        },
        (None, _) => Address {
            tag: Some(tag),
            scope: shown
                .as_ref()
                .and_then(Location::path)
                .filter(|p| Some(p) != std::env::home_dir().as_ref()),
            ..Address::default()
        },
    };
    app.navigate(pane, &address.location())?;
    Ok(Outcome::Done)
}

/// `tags.browse`: every tag of a folder (`tags:/?in=…`) in the pane: the
/// folder named, else the one it shows, or the one what it shows is of
/// (its trail's base: a tag location's, a search's), else the home
/// folder. The folder's own menu (Browse tags here), and All Tags clicked
/// with Alt. With `toggle`, a tag location goes back to its folder.
pub fn browse(app: &App, a: &BrowseArgs) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    if a.toggle && a.folder.is_none() {
        let back = app
            .pane_location(pane)
            .filter(|shown| &*shown.scheme == crate::location::SCHEME)
            .and_then(|shown| app.trail(&shown))
            .and_then(|trail| trail.base);
        if let Some(folder) = back {
            app.navigate(pane, &skwir_core::Location::from_path(folder))?;
            return Ok(Outcome::Done);
        }
    }
    let folder = match &a.folder {
        Some(folder) => Some(skwir_core::os_path::absolute(folder)?.to_owned()),
        None => app.pane_location(pane).and_then(|shown| {
            shown
                .path()
                .or_else(|| app.trail(&shown).and_then(|trail| trail.base))
        }),
    };
    let address = folder.map_or_else(Address::default, |folder| Address::of_folder(&folder));
    app.navigate(pane, &address.location())?;
    Ok(Outcome::Done)
}

/// Whether a tag location is kept in the sidebar (`Keeper::keeps`): one
/// tag of the home folder, not narrowed, its pin.
pub fn keepable(location: &skwir_core::Location) -> bool {
    pinned_tag(&crate::location::Address::parse(&location.rest)).is_some()
}

fn pinned_tag(address: &crate::location::Address) -> Option<&str> {
    let plain = address.scope.is_none() && !address.narrowed() && !address.deep;
    address.tag.as_deref().filter(|_| plain)
}

/// `tags.keep`, Ctrl+D in a tag location: its tag pinned, when it is one
/// tag of the home folder, not narrowed (`keepable`).
pub fn keep(app: &App, a: PaneOnly) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let address = shown(app, pane, None)?;
    let Some(tag) = pinned_tag(&address).map(str::to_owned) else {
        anyhow::bail!("a tag of the home folder, not narrowed, is kept in the sidebar: pinned");
    };
    app.dispatch(&Invocation::with_args(
        "tags.pin",
        &serde_json::json!({ "tag": tag }),
    ))
}

fn item(title: &str, command: Invocation, order: i32, icon: &str) -> MenuItem {
    MenuItem::new("context", title, icon, order, command).when("place")
}

/// `tags.place_menu`: the items of the tags selected in a tag location
/// (`tag_items`).
pub fn place_menu(app: &App, a: PaneOnly) -> Outcome {
    let pane = app.pane_or_active(a.pane);
    let tags = selected_places(app, pane);
    let root: Option<PathArg> = app
        .pane_location(pane)
        .as_ref()
        .and_then(address_of)
        .and_then(|a| a.scope)
        .map(PathArg::from);
    Outcome::Value(serde_json::json!({ "items": tag_items(app, &tags, root.as_ref(), false) }))
}

/// `tags.tag_menu`'s arguments: tags by name (a chip in Properties), or a
/// place of the sidebar (`location`), which is a tag's when it is a tag
/// folder of the home folder.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct TagMenuArgs {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub location: Option<skwir_core::os_path::PathArg>,
    /// The pane the menu opens in, which a menu gives every source.
    #[serde(default, rename = "pane")]
    pub _pane: Option<PaneId>,
}

/// `tags.tag_menu`: a tag's menu where it is shown outside a tag location:
/// its chip in Properties (`tags`), its pill in the sidebar (`location`,
/// whose pin the sidebar takes out itself). Nothing for a place that is no
/// tag's. Answers from memory.
pub fn tag_menu(app: &App, a: TagMenuArgs) -> Outcome {
    let (tags, on_pin) = match a
        .location
        .as_deref()
        .map(|l| Location::parse_os(l.as_os_str()))
    {
        Some(location) => match address_of(&location).as_ref().and_then(pinned_tag) {
            Some(tag) => {
                let pinned = crate::sidebar::pinned(app).iter().any(|p| same_tag(p, tag));
                (vec![tag.to_owned()], pinned)
            }
            None => (Vec::new(), false),
        },
        None => (a.tags, false),
    };
    Outcome::Value(serde_json::json!({ "items": tag_items(app, &tags, None, on_pin) }))
}

/// The one menu of tags, wherever they are shown (a tag location's
/// entries, a sidebar pill, a chip in Properties), answered from memory (the
/// pins, the fields, the colors' names): pin or unpin them (not `on_pin`,
/// a pin of the sidebar, which takes itself out), their color, put them in
/// a category or out of it, one tag at a time, merge them, take them off
/// every file (of `root` only, when given).
pub fn tag_items(
    app: &App,
    tags: &[String],
    root: Option<&PathArg>,
    on_pin: bool,
) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if tags.is_empty() {
        return items;
    }
    let pins = crate::sidebar::pinned(app);
    let all_pinned = tags.iter().all(|t| pins.iter().any(|p| same_tag(p, t)));
    let them = serde_json::json!({ "tags": tags });
    if all_pinned && !on_pin {
        items.push(item(
            "Unpin From Sidebar",
            Invocation::with_args("tags.unpin", &them),
            410,
            "pin",
        ));
    } else if !all_pinned {
        items.push(item(
            "Pin to Sidebar",
            Invocation::with_args("tags.pin", &them),
            410,
            "pin",
        ));
    }
    items.push(item(
        "Move to Category…",
        Invocation::with_args(
            "ui.ask",
            &serde_json::json!({
                "title": "Move into a category, keeping the names",
                "accept": "Move",
                "placeholder": "Category (Places/France)",
                "command": "tags.move",
                "args": { "tags": tags, "root": root },
                "field": "into",
            }),
        ),
        120,
        "move",
    ));
    if tags.iter().all(|t| tag_parent(t).is_some()) {
        let title = match tags {
            [one] => format!(
                "Move Out of “{}”",
                tag_parent(one).map(tag_leaf).unwrap_or_default()
            ),
            _ => "Move Out of Their Categories".to_owned(),
        };
        items.push(item(
            &title,
            Invocation::with_args(
                "tags.move",
                &serde_json::json!({ "tags": tags, "root": root }),
            ),
            121,
            "move",
        ));
    }
    if let [tag] = tags {
        let fields: Vec<String> = app.settings.get(TAG_EXCLUSIVE_SETTING).unwrap_or_default();
        let on = fields.iter().any(|f| same_tag(f, tag));
        items.push(item(
            "One Tag at a Time",
            Invocation::with_args(
                "tags.exclusive",
                &serde_json::json!({ "tag": tag, "on": !on }),
            ),
            150,
            if on { "check" } else { "tag" },
        ));
        let mut color = |title: &str, color: Option<&str>, order| {
            items.push(MenuItem {
                submenu: Some("Color".into()),
                ..item(
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
        color("From Its Name", None, 140);
        for (i, name) in TAG_COLORS.iter().enumerate() {
            let mut title = (*name).to_owned();
            title[..1].make_ascii_uppercase();
            color(
                &title,
                Some(name),
                141 + i32::try_from(i).unwrap_or_default(),
            );
        }
    }
    if tags.len() > 1 {
        items.push(item(
            "Merge Into One…",
            Invocation::with_args(
                "ui.ask",
                &serde_json::json!({
                    "title": "Merge these tags into one, on every file",
                    "accept": "Merge",
                    "placeholder": "The tag they become",
                    "command": "tags.rename",
                    "args": { "tags": tags, "root": root },
                    "field": "to",
                }),
            ),
            122,
            "rename",
        ));
    }
    items.push(item(
        "Remove Tag From Every File…",
        Invocation::with_args(
            "tags.delete",
            &serde_json::json!({ "tags": tags, "root": root }),
        ),
        300,
        "trash",
    ));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrowing_switches_and_a_tag_is_with_or_without_never_both() {
        let a = Address::parse("/Places");
        let a = narrowed(a, "also_tag", Some("family"), None).unwrap();
        assert_eq!(a.with, ["family"]);
        let a = narrowed(a, "not_tag", Some("Family"), None).unwrap();
        assert!(a.with.is_empty() && a.without == ["Family"]);
        let a = narrowed(a, "kind", Some("image"), None).unwrap();
        let a = narrowed(a, "modified", Some("2024"), None).unwrap();
        assert_eq!(
            (a.kinds.as_slice(), a.years.as_slice()),
            (&[Kind::Image][..], &[2024][..])
        );
        let a = narrowed(a, "kind", Some("image"), Some(true)).unwrap();
        assert_eq!(a.kinds.len(), 1, "on keeps it on");
        let a = narrowed(a, "deep", None, None).unwrap();
        assert!(a.deep);
        assert!(narrowed(a, "size", Some("1"), None).is_err());
    }

    #[test]
    fn a_tag_location_takes_files_out_by_its_own_tag() {
        let tags: Vec<String> = ["places/france", "Places/France/Paris", "Places"]
            .map(str::to_owned)
            .to_vec();
        let france = Address::parse("/Places/France");
        assert_eq!(
            taken(&france).unwrap().applied(&tags),
            ["Places/France/Paris", "Places"],
            "Paris stays: a tag of its own"
        );
        let deep = Address::parse("/Places/France?deep");
        assert_eq!(taken(&deep).unwrap().applied(&tags), ["Places"]);
        assert!(
            taken(&Address::parse("/")).is_none(),
            "every tag, at the top"
        );
    }
}
