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
    TAG_COLORS, TAG_EXCLUSIVE_SETTING, clean_tag, same_tag, tag_leaf, tag_lineage, tag_parent,
    tag_under,
};
use skwir_core::op::{EntryRef, Tenses};
use skwir_core::os_path::PathArg;
use skwir_core::registry::{ChoiceLook, MenuItem};
use skwir_core::words;
use skwir_core::words::{Folder, Name, Words};
use skwir_core::workspace::PaneId;

use crate::location::{Address, SCHEME};
use crate::vocabulary::Vocabulary;

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
    /// `deep` (every file within the tag).
    pub facet: String,
    #[serde(default)]
    pub value: Option<String>,
    /// Narrowed so (`true`) or not; switched when omitted.
    #[serde(default)]
    pub on: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenArgs {
    /// The tag whose files are shown.
    pub tag: String,
    #[serde(default)]
    pub pane: Option<PaneId>,
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
        None => app.pane_location(pane).ok_or_else(|| {
            anyhow::Error::from(words!(tags_pane_shows_nothing = "the pane shows nothing"))
        })?,
    };
    address_of(&location).ok_or_else(|| {
        anyhow::Error::from(words!(tags_pane_shows_no_tags = "the pane shows no tags"))
    })
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
            words!(
                tags_drop_on_one = "files are given one tag: drop them on it",
                note = "An error: files were dropped on every tag at once, which gives them none."
            )
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
        Some(from) => words!(
            tags_move_tag_plan = "move from tag { $from } to tag { $tag }",
            from = from,
            tag = tag.as_str(),
            note = "A plan's title: files dragged from one tag's place to another's. from, tag: the tags' names."
        ),
        None => words!(
            tags_add_tag_plan = "add tag { $tag }",
            tag = tag.as_str(),
            note = "A plan's title: files dropped on a tag's place. tag: its name."
        ),
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
        let name = || Name::of(&r.path);
        let to = || Name(tag.clone());
        let (will, did, already) = match source.as_ref().and_then(|s| s.tag.as_deref()) {
            Some(from) => {
                let from = || Name(from.to_owned());
                (
                    words!(
                        tags_move_file_will = "move { $name } from { $from } to { $tag }",
                        name = name(),
                        from = from(),
                        tag = to(),
                        note = "A step: a file dragged from one tag's place to another's. name: the file's; from, tag: the tags'."
                    ),
                    words!(
                        tags_move_file_did = "moved { $name } from { $from } to { $tag }",
                        name = name(),
                        from = from(),
                        tag = to(),
                        note = "As tags_move_file_will, done."
                    ),
                    words!(
                        tags_move_file_already = "{ $name } was already in { $tag }",
                        name = name(),
                        tag = to(),
                        note = "As tags_move_file_will, found done already."
                    ),
                )
            }
            None => (
                words!(
                    tags_give_file_will = "give { $name } the tag { $tag }",
                    name = name(),
                    tag = to(),
                    note = "A step: a file dropped on a tag's place. name: the file's; tag: the tag's."
                ),
                words!(
                    tags_give_file_did = "gave { $name } the tag { $tag }",
                    name = name(),
                    tag = to(),
                    note = "As tags_give_file_will, done."
                ),
                words!(
                    tags_give_file_already = "{ $name } already had the tag { $tag }",
                    name = name(),
                    tag = to(),
                    note = "As tags_give_file_will, found done already."
                ),
            ),
        };
        Some(
            app.notes
                .edit(r)
                .change_tags(change.clone())
                .saying_tenses(Tenses {
                    will,
                    did,
                    already: Some(already),
                }),
        )
    })
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
        Some(tag) => words!(
            tags_untag_plan = "remove tag { $tag }",
            tag = tag.as_str(),
            note = "A plan's title: files taken out of a tag's place. tag: its name."
        ),
        None => words!(
            tags_untag_all_plan = "remove all tags",
            note = "A plan's title: files taken out of the place of every tag."
        ),
    };
    // Read when the plan is applied, as `drop`'s.
    let change = taken(&address);
    crate::per_entry(files, title, |r| {
        let name = || Name::of(&r.path);
        let (will, did, already) = match &address.tag {
            Some(tag) => {
                let named = || Name(tag.clone());
                (
                    words!(
                        tags_untag_file_will = "remove the tag { $tag } from { $name }",
                        tag = named(),
                        name = name(),
                        note = "A step. tag: the tag's name; name: the file's."
                    ),
                    words!(
                        tags_untag_file_did = "removed the tag { $tag } from { $name }",
                        tag = named(),
                        name = name(),
                        note = "As tags_untag_file_will, done."
                    ),
                    words!(
                        tags_untag_file_already = "the tag { $tag } was already off { $name }",
                        tag = named(),
                        name = name(),
                        note = "As tags_untag_file_will, found done already."
                    ),
                )
            }
            None => (
                words!(
                    tags_untag_all_file_will = "remove the tags of { $name }",
                    name = name(),
                    note = "A step. name: the file's."
                ),
                words!(
                    tags_untag_all_file_did = "removed the tags of { $name }",
                    name = name(),
                    note = "As tags_untag_all_file_will, done."
                ),
                words!(
                    tags_untag_all_file_already = "{ $name } already had no tags",
                    name = name(),
                    note = "As tags_untag_all_file_will, found done already."
                ),
            ),
        };
        let edit = match &change {
            Some(change) => app.notes.edit(r).change_tags(change.clone()),
            None => app.notes.edit(r).tags(Vec::new()),
        };
        Some(edit.saying_tenses(Tenses {
            will,
            did,
            already: Some(already),
        }))
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
    anyhow::ensure!(
        !name.is_empty(),
        words!(tags_needs_name = "a tag needs a name")
    );
    anyhow::ensure!(
        !name.contains(','),
        words!(tags_no_comma = "a tag's name has no comma")
    );
    let tag = match &address.tag {
        Some(parent) => format!("{parent}/{name}"),
        None => name,
    };
    let root = address
        .root()
        .ok_or_else(|| anyhow::Error::from(crate::no_home()))?;
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
        .ok_or_else(|| {
            anyhow::Error::from(words!(
                tags_export_one = "a tag is copied, not all of them",
                note = "An error: the place of every tag was dropped in a folder, where one tag's place makes a folder of its files."
            ))
        })?;
    let root = address
        .root()
        .ok_or_else(|| anyhow::Error::from(crate::no_home()))?;
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
            app.propose(skwir_core::op::Plan::with_steps(
                words!(
                    tags_export_plan = "copy the files tagged { $tag } to { $folder }",
                    tag = tag.as_str(),
                    folder = Folder(Name::of(&dest).0),
                    note = "A plan's title. tag: the tag's name; folder: where the copies go."
                ),
                steps,
            ));
            app.wake();
        },
    );
    Ok(Outcome::Done)
}

/// `address` narrowed by `facet`: `deep`, every file within its tag, or
/// not (`on`; switched when omitted). What else narrows a tag's place
/// (another tag, a kind, a year) is the pane's lens (ADR-107).
pub fn narrowed(
    mut address: Address,
    facet: &str,
    _value: Option<&str>,
    on: Option<bool>,
) -> anyhow::Result<Address> {
    match facet {
        "deep" => address.deep = on.unwrap_or(!address.deep),
        other => anyhow::bail!(words!(
            tags_narrow_deep_only = "a tag's place is narrowed by `deep`; `{ $facet }` is the pane's lens (query.narrow)",
            facet = other,
            note = "An error for a script or an agent. facet: what it asked to narrow by, as it wrote it."
        )),
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

/// `tags.open`: a tag's place, every file of the home folder that has it
/// and a place for each tag within it, as its pinned pill opens it.
pub fn open(app: &App, a: &OpenArgs) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let tag = clean_tag(&a.tag);
    anyhow::ensure!(!tag.is_empty(), crate::which_tag());
    let address = Address {
        tag: Some(tag),
        ..Address::default()
    };
    app.navigate(pane, &address.location())?;
    Ok(Outcome::Done)
}

/// `tags.pill`: a sidebar's tag clicked with Shift (`also`) or Alt
/// (`here`).
pub fn pill(app: &App, a: &PillArgs) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let tag = clean_tag(&a.tag);
    anyhow::ensure!(!tag.is_empty(), crate::which_tag());
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
        // One more tag the files shown have: the pane's lens (ADR-107).
        (Some(_), "also") => {
            return app.dispatch(&Invocation::with_args(
                "query.narrow",
                &serde_json::json!({ "facet": "also_tag", "value": tag, "pane": pane }),
            ));
        }
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
    let plain = address.scope.is_none() && !address.deep;
    address.tag.as_deref().filter(|_| plain)
}

/// What `tags.keep` is given: the core `keep`'s pane, and the filter left
/// out when it keeps the place plain (ADR-109).
#[derive(Clone, Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct KeepArgs {
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// The filter the place was shown with, which nothing keeps: the
    /// toast says it is not kept with the tag.
    #[serde(default)]
    pub unkept: Option<String>,
}

/// `tags.keep`, Ctrl+D in a tag location: its tag pinned, when it is one
/// tag of the home folder, not narrowed (`keepable`).
pub fn keep(app: &App, a: &KeepArgs) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let address = shown(app, pane, None)?;
    let Some(tag) = pinned_tag(&address).map(str::to_owned) else {
        anyhow::bail!(words!(
            tags_keep_one =
                "a tag of the home folder, not narrowed, is kept in the sidebar: pinned",
            note = "An error: what Ctrl+D keeps of a tag's place."
        ));
    };
    app.dispatch(&Invocation::with_args(
        "tags.pin",
        &serde_json::json!({ "tag": tag, "unkept": a.unkept }),
    ))
}

fn item(title: Words, command: Invocation, order: i32, icon: &str) -> MenuItem {
    MenuItem::new("context", title, icon, order, command).when("place")
}

/// A tag's colors: one choice, drawn as swatches (each value's
/// `MenuItem::color`), from its name first, `color` the one it was given.
fn color_items(tag: &str, color: Option<&str>) -> Vec<MenuItem> {
    // Its values meet by its id, its row saying its name.
    let choice = words!(
        tags_color_choice = "Color",
        note = "The name of a tag's menu's row of colors."
    );
    std::iter::once((
        words!(
            tags_color_from_name = "From Its Name",
            note = "A tag's color: the one its name gives it, none chosen."
        ),
        None,
    ))
    .chain(
        TAG_COLORS
            .iter()
            .map(|name| (color_name(name), Some(*name))),
    )
    .map(|(title, value)| {
        let item = item(
            title,
            Invocation::with_args(
                "tags.color",
                &serde_json::json!({ "tag": tag, "color": value }),
            ),
            0,
            "",
        )
        .value_of(
            "tags.color",
            choice.clone(),
            ChoiceLook::Colors,
            color == value,
        );
        match value {
            Some(value) => item.colored(value),
            None => item,
        }
    })
    .collect()
}

/// What a color of the tags' palette (`TAG_COLORS`) is called.
fn color_name(color: &str) -> Words {
    match color {
        "red" => words!(
            tags_color_red = "Red",
            note = "A color a tag can wear, in its menu."
        ),
        "orange" => words!(
            tags_color_orange = "Orange",
            note = "A color a tag can wear, in its menu."
        ),
        "yellow" => words!(
            tags_color_yellow = "Yellow",
            note = "A color a tag can wear, in its menu."
        ),
        "green" => words!(
            tags_color_green = "Green",
            note = "A color a tag can wear, in its menu."
        ),
        "teal" => words!(
            tags_color_teal = "Teal",
            note = "A color a tag can wear, in its menu."
        ),
        "blue" => words!(
            tags_color_blue = "Blue",
            note = "A color a tag can wear, in its menu."
        ),
        "purple" => words!(
            tags_color_purple = "Purple",
            note = "A color a tag can wear, in its menu."
        ),
        "pink" => words!(
            tags_color_pink = "Pink",
            note = "A color a tag can wear, in its menu."
        ),
        // A color the palette gained that no message names yet: its id.
        other => Words::as_written(other.to_owned()),
    }
}

/// `tags.place_menu`: the items of the tags selected in a tag location
/// (`tag_items`, `one_at_a_time` of the tag, among those `known`).
pub fn place_menu(app: &App, known: &[String], a: PaneOnly) -> Outcome {
    let pane = app.pane_or_active(a.pane);
    let tags = selected_places(app, pane);
    let root: Option<PathArg> = app
        .pane_location(pane)
        .as_ref()
        .and_then(address_of)
        .and_then(|a| a.scope)
        .map(PathArg::from);
    let mut items = tag_items(app, &tags, root.as_ref(), false);
    items.extend(one_at_a_time(app, &tags, false, known));
    Outcome::Value(skwir_core::registry::menu_answer(&items))
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
    /// Shown on the selected files (a chip of an entry, of Properties,
    /// of the preview): the menu also acts on them.
    #[serde(default)]
    pub files: bool,
    /// The pane the menu opens in, which a menu gives every source: the
    /// one whose selection `files` acts on.
    #[serde(default)]
    pub pane: Option<PaneId>,
}

/// `tags.tag_menu`: a tag's menu where it is shown outside a tag location:
/// a file's chip (`tags`, `files`: an entry's, in Properties, in the
/// preview's header), its pill in the sidebar (`location`, whose pin the
/// sidebar takes out itself). Nothing for a place that is no tag's.
/// Answers from memory, `known` the tags the plugin knows.
pub fn tag_menu(app: &App, known: &[String], a: TagMenuArgs) -> Outcome {
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
    let mut items = match (a.files, tags.as_slice()) {
        (true, [tag]) => {
            let mut items = file_items(app, tag, a.pane);
            // The tag editor of the files the chip pressed was made one
            // of, first of what changes them (2xx): where a file shows its tags,
            // the chips are the way to it. The context menu has its own
            // (Tags…).
            if selected(app, a.pane) > 0 {
                items.push(item(
                    words!(
                        tags_edit_item = "Edit Tags…",
                        note = "The first item of the files' menu's Tags submenu: it opens the tag editor."
                    ),
                    Invocation::new("ui.tags_prompt"),
                    210,
                    "tag",
                ));
            }
            items
        }
        _ => Vec::new(),
    };
    items.extend(tag_items(app, &tags, None, on_pin));
    items.extend(one_at_a_time(app, &tags, a.files, known));
    Outcome::Value(skwir_core::registry::menu_answer(&items))
}

/// The most tags the context menu's Tags submenu checks.
const FILES_MENU_TAGS: usize = 8;
/// The most selected files whose tags the context menu reads: what their
/// listing knows, so a menu is no walk of a huge selection.
const FILES_MENU_READ: usize = 1000;

/// `tags.files_menu`: in the context menu of selected files, the checks
/// of its Tags submenu (ADR-099): the tags they have, as their listing
/// read them, then the pinned ones; among them those pinned first, then by
/// name, at most `FILES_MENU_TAGS`. A tag every file selected is known to
/// have is checked, one some are known to have partly (a dash): a file
/// whose notes its listing has not read yet, or past the first
/// `FILES_MENU_READ`, is not known to. A place selected (a tag's within
/// the one shown) is no file. Pressing a check sets what it then shows
/// (`tags.toggle` with `on`, told by the menu, which stays open for the
/// next): a dash or off gives the tag to them all, on takes it from them
/// all. What a tag itself offers (its files here, its place, its color)
/// is its chip's menu and the sidebar's. Answers from memory.
pub fn files_menu(app: &App, vocabulary: &Vocabulary, a: PaneOnly) -> Outcome {
    let pane = app.pane_or_active(a.pane);
    // Each tag, with how many of the files known have it, and how many
    // files are selected.
    let mut tags: Vec<(String, usize)> = Vec::new();
    let mut files = 0;
    {
        let ws = app.workspace.read();
        if let Some(p) = ws.pane(pane) {
            let mut places = 0;
            for id in p.selection.iter().take(FILES_MENU_READ) {
                if id.is_place() {
                    places += 1;
                    continue;
                }
                let Some(e) = p.entry(id).filter(|e| e.metadata.is_filled()) else {
                    continue;
                };
                for tag in crate::tags_of(&e.metadata).unwrap_or_default() {
                    match tags.iter_mut().find(|(t, _)| same_tag(t, &tag)) {
                        Some((_, count)) => *count += 1,
                        None => tags.push((tag, 1)),
                    }
                }
            }
            files = p.selection.len() - places;
        }
    }
    if files == 0 {
        return Outcome::Value(serde_json::json!({ "items": [] }));
    }
    let pins = crate::sidebar::pinned(app);
    for pin in &pins {
        if !tags.iter().any(|(t, _)| same_tag(t, pin)) {
            tags.push((pin.clone(), 0));
        }
    }
    tags.sort_by_cached_key(|(t, count)| {
        (
            *count == 0,
            pins.iter()
                .position(|p| same_tag(p, t))
                .unwrap_or(usize::MAX),
            t.to_lowercase(),
        )
    });
    tags.truncate(FILES_MENU_TAGS);
    // Each drawn as its chip, in the color it wears everywhere.
    let colors = crate::sidebar::Colors::of(app);
    let items: Vec<MenuItem> = tags
        .iter()
        .zip(510..)
        .map(|((tag, count), order)| {
            // Its own name, unless another's is the same in another
            // category.
            let leaf = tag_leaf(tag);
            let name = if tags
                .iter()
                .any(|(t, _)| !same_tag(t, tag) && same_tag(tag_leaf(t), leaf))
            {
                tag.as_str()
            } else {
                leaf
            };
            let on = *count == files;
            // A tag's name, as the user wrote it.
            let item = MenuItem::new(
                "context",
                Words::as_written(name.to_owned()),
                "",
                order,
                Invocation::with_args(
                    "tags.toggle",
                    &serde_json::json!({ "tag": tag, "pane": pane, "on": !on }),
                ),
            )
            .submenu(skwir_core::registry::submenus::tags())
            .colored(&colors.of_tag(vocabulary, tag));
            match *count {
                0 => item.checked(false),
                _ if on => item.checked(true),
                _ => item.partly(),
            }
        })
        .collect();
    Outcome::Value(skwir_core::registry::menu_answer(&items))
}

/// What a tag's menu adds where files show it (their chips, Properties,
/// the preview's header): its files in the folder shown (`tags.pill`, as
/// its pill clicked with Alt), its place (`tags.open`), and taking it from
/// the files selected in `pane`, which the chip pressed was made one of.
pub fn file_items(app: &App, tag: &str, pane: Option<PaneId>) -> Vec<MenuItem> {
    let selected = selected(app, pane);
    let mut items = vec![
        item(
            words!(
                tags_show_files_here = "Show Its Files Here",
                note = "An item of a tag's menu: its files in the folder shown."
            ),
            Invocation::with_args(
                "tags.pill",
                &serde_json::json!({ "tag": tag, "how": "here" }),
            ),
            10,
            "filter",
        ),
        item(
            words!(
                tags_open_tag = "Open Tag",
                note = "An item of a tag's menu: its place, every file that has it."
            ),
            Invocation::with_args("tags.open", &serde_json::json!({ "tag": tag })),
            11,
            "tag",
        ),
    ];
    if selected > 0 {
        items.push(item(
            words!(
                tags_remove_from_files = "{ $count ->
                        [1] Remove From This File
                       *[other] Remove From Selected Files
                    }",
                count = selected,
                note = "An item of a tag's menu where a file shows it: takes it from the files selected. count: how many."
            ),
            Invocation::with_args("tags.remove", &serde_json::json!({ "tags": [tag] })),
            290,
            "minus",
        ));
    }
    items
}

/// How many files are selected in `pane`, or the active one.
fn selected(app: &App, pane: Option<PaneId>) -> usize {
    let pane = app.pane_or_active(pane);
    app.workspace
        .read()
        .pane(pane)
        .map_or(0, |p| p.selection.len())
}

/// "One “Places” Tag at a Time": make a category hold one tag at a time
/// (`tags.exclusive`), or several again (checked while it holds one).
/// Offered for a category (a tag that tags are known under: `known`, or
/// the menu's own `tags`) or one already holding one at a time; on a tag
/// with nothing under it, it would change nothing. A pill or a tag's
/// place offers its own tag. A `chip` shows a file's tag, a value: it
/// offers the categories the tag is in, the outermost first (`Places`,
/// then `Norway`, for `Places/Norway/Fjords`), and the tag itself only
/// where tags are under it.
pub fn one_at_a_time(app: &App, tags: &[String], chip: bool, known: &[String]) -> Vec<MenuItem> {
    let [tag] = tags else {
        return Vec::new();
    };
    let fields: Vec<String> = app.settings.get(TAG_EXCLUSIVE_SETTING).unwrap_or_default();
    let mut candidates: Vec<&str> = if chip {
        tag_lineage(tag).collect()
    } else {
        vec![tag.as_str()]
    };
    candidates.reverse();
    let under = |category: &str| {
        known
            .iter()
            .chain(tags)
            .any(|k| tag_under(k, category).is_some_and(|rest| !rest.is_empty()))
    };
    candidates
        .into_iter()
        .filter_map(|category| {
            let on = fields.iter().any(|f| same_tag(f, category));
            (on || under(category)).then_some((category, on))
        })
        .zip(260..)
        .map(|((category, on), order)| {
            item(
                words!(
                    tags_one_at_a_time = "One { $category } Tag at a Time",
                    category = Name(tag_leaf(category).to_owned()),
                    note = "An item of a tag's menu, checked while on: the tags within a category become the values of one field, a file having one of them at a time. category: its name."
                ),
                Invocation::with_args(
                    "tags.exclusive",
                    &serde_json::json!({ "tag": category, "on": !on }),
                ),
                order,
                "tag",
            )
            .checked(on)
        })
        .collect()
}

/// The one menu of tags, wherever they are shown (a tag location's
/// entries, a sidebar pill, a chip in Properties), answered from memory (the
/// pins, the fields, the colors' names): pin or unpin them (not `on_pin`,
/// a pin of the sidebar, which takes itself out), their color, put them in
/// a category or out of it, merge them, take them off every file (of
/// `root` only, when given). Its callers add `one_at_a_time`.
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
            words!(tags_unpin_item = "Unpin From Sidebar"),
            Invocation::with_args("tags.unpin", &them),
            410,
            "pin",
        ));
    } else if !all_pinned {
        items.push(item(
            words!(tags_pin_item = "Pin to Sidebar"),
            Invocation::with_args("tags.pin", &them),
            410,
            "pin",
        ));
    }
    items.push(item(
        words!(
            tags_move_item = "Move to Category…",
            note = "An item of a tag's menu: it asks for the category the tags go into."
        ),
        Invocation::with_args(
            "ui.ask",
            &serde_json::json!({
                "title": words!(
                    tags_move_ask = "Move into a category, keeping the names",
                    note = "The title of the question Move to Category… asks."
                )
                .to_data(),
                "accept": words!(
                    tags_move_accept = "Move",
                    note = "The button that moves tags into the category typed."
                )
                .to_data(),
                "placeholder": words!(tags_move_placeholder = "Category (Places/France)").to_data(),
                "command": "tags.move",
                "args": { "tags": tags, "root": root },
                "field": "into",
            }),
        ),
        220,
        "move",
    ));
    if tags.iter().all(|t| tag_parent(t).is_some()) {
        let title = match tags {
            [one] => words!(
                tags_move_out_item = "Move Out of { $category }",
                category = Name(tag_parent(one).map(tag_leaf).unwrap_or_default().to_owned()),
                note = "An item of a tag's menu. category: the category it is in."
            ),
            _ => words!(tags_move_out_all_item = "Move Out of Their Categories"),
        };
        items.push(item(
            title,
            Invocation::with_args(
                "tags.move",
                &serde_json::json!({ "tags": tags, "root": root }),
            ),
            221,
            "move",
        ));
    }
    if let [tag] = tags {
        let chosen: toml::Table = app
            .settings
            .get(skwir_core::metadata::TAG_COLOR_SETTING)
            .unwrap_or_default();
        let color = chosen
            .get(&tag.to_lowercase())
            .and_then(toml::Value::as_str);
        for (i, mut value) in color_items(tag, color).into_iter().enumerate() {
            value.order = 250 + i32::try_from(i).unwrap_or_default();
            items.push(value);
        }
    }
    if tags.len() > 1 {
        items.push(item(
            words!(
                tags_merge_item = "Merge Into One…",
                note = "An item of the menu of several tags: it asks for the tag they become."
            ),
            Invocation::with_args(
                "ui.ask",
                &serde_json::json!({
                    "title": words!(tags_merge_ask = "Merge these tags into one, on every file").to_data(),
                    "accept": words!(
                        tags_merge_accept = "Merge",
                        note = "The button that merges the tags into the one typed."
                    )
                    .to_data(),
                    "placeholder": words!(tags_merge_placeholder = "The tag they become").to_data(),
                    "command": "tags.rename",
                    "args": { "tags": tags, "root": root },
                    "field": "to",
                }),
            ),
            222,
            "rename",
        ));
    }
    items.push(item(
        words!(tags_remove_everywhere_item = "Remove Tag From Every File…"),
        Invocation::with_args(
            "tags.delete",
            &serde_json::json!({ "tags": tags, "root": root }),
        ),
        290,
        "trash",
    ));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag's place is narrowed by how deep it holds files alone: what
    /// else narrows it is the pane's lens.
    #[test]
    fn a_tag_s_place_narrows_by_depth_alone() {
        let a = Address::parse("/Places");
        let a = narrowed(a, "deep", None, None).unwrap();
        assert!(a.deep);
        let a = narrowed(a, "deep", None, Some(true)).unwrap();
        assert!(a.deep, "on keeps it on");
        for facet in ["also_tag", "not_tag", "kind", "modified"] {
            assert!(narrowed(a.clone(), facet, Some("x"), None).is_err());
        }
        assert_eq!(
            Address::parse("/Places?with=family&kind=image&year=2024"),
            Address::parse("/Places"),
            "what was its own narrowing is no part of its address"
        );
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
