//! Sidebar: places from the registry, grouped, click to navigate.
//!
//! Rows use the desktop theme's icons, volumes show how full they are, and
//! removable ones can be detached. A row accepts dropped files like any
//! folder; dropping a folder beside the rows bookmarks it. A right-click
//! opens the place's own menu (`Place::menu`: rename or remove a bookmark
//! or a saved search), and a place its owner lets move (`Place::reorder`:
//! bookmarks, saved searches) is put in order by dragging it among the
//! others of its group. A place with a color (a tag's) is the tag's chip,
//! as the views draw it, the chips of a group wrapping together, and a
//! place with a `drop`
//! command takes files dropped on it (a tag gives them itself).
//!
//! The groups are the user's to arrange (`Sections`, remembered with the
//! window's shape): a click on a group's header folds it, dragging the
//! header moves the group, and its right-click menu hides it, or shows
//! one hidden. So are the places their owners offer (Home, the Trash, a
//! volume): moved, renamed and hidden here (`arrange`, `ui.sidebar_place`),
//! where a place the user keeps is its owner's to arrange (`Role`). F6
//! gives the sidebar the keyboard (`Keys`, `Shell::sidebar_key`).

use crate::sides::Sides;
use crate::sizes::TextSizes;
use crate::sizes::sized;
use crate::theme::Chosen;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use gpui::{AnyElement, Context, ExternalPaths, MouseButton, SharedString, div, prelude::*, px};

use skwir_core::Location;
use skwir_core::command::Invocation;
use skwir_core::os_path::PathArg;
use skwir_core::registry::{Opens, Place, Reorder};
use skwir_core::words;
use skwir_core::words::{Size, Words};
use skwir_core::workspace::TabId;

use crate::controls;
use crate::dnd::DragPayload;
use crate::menu::MenuHost;
use crate::shell::Shell;
use crate::theme::Theme;

/// The groups in the order the sidebar ships with: a dialog's window lists
/// first what the user's tabs show, then the folders its program chose in
/// (ADR-144, 8).
const GROUP_ORDER: [&str; 7] = [
    skwir_core::registry::groups::SKWIR_TABS.id,
    skwir_core::registry::groups::RECENT_FOLDERS.id,
    "places",
    "bookmarks",
    "searches",
    "tags",
    "devices",
];
const ICON_PX: f32 = 16.;
/// The sidebar's width until its edge is dragged, and after a double click
/// on it; the narrowest it gets.
pub const DEFAULT_WIDTH: f32 = 200.;
const MIN_WIDTH: f32 = 140.;

/// Marker type for dragging the sidebar's right edge.
#[derive(Clone)]
pub struct SidebarResize;

/// The widest the sidebar may be: the window less what the panes keep and,
/// with the preview open at the right (`preview`), the panel's minimum. The
/// panel gives way to it down to that minimum (`preview::max_width` counts
/// the sidebar as drawn), never the panes.
pub fn max_width(window: f32, panes: usize, preview: bool) -> f32 {
    let kept = crate::preview::PANE_KEEP * panes.max(1) as f32
        + if preview {
            crate::preview::MIN_WIDTH
        } else {
            0.
        };
    (window - kept).max(MIN_WIDTH)
}

fn group_rank(g: &str) -> usize {
    GROUP_ORDER
        .iter()
        .position(|x| *x == g)
        .unwrap_or(GROUP_ORDER.len())
}

/// How the user arranged the sidebar's groups: their order, those put
/// away and those folded. Part of the window's shape (`session.rs`), so
/// the next window is arranged the same.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Sections {
    /// Every group known when the user last moved one, in the order left;
    /// empty until then. A group it does not name comes where the sidebar
    /// ships it, before the first named that ships after it (one new in
    /// this version, `Sections::rank`); any other goes after them.
    pub order: Vec<String>,
    pub hidden: std::collections::BTreeSet<String>,
    pub folded: std::collections::BTreeSet<String>,
    /// How the user arranged each group's places, by group.
    pub places: BTreeMap<String, Arrangement>,
}

/// How the user arranged one group's places in the sidebar itself: those
/// their owners offer (`Role::Offered`: Home, the Trash, a volume), which
/// the user moves, renames and hides, and those suggested, which the user
/// hides. A place the user keeps (a bookmark) is its owner's to order,
/// rename and take out. Keyed by `Place::key`, so that a volume is the
/// same place mounted or not.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Arrangement {
    /// The offered places in the order the user left them, those absent
    /// now included, so that they come back where they were; empty until
    /// one is moved. One it does not name goes after them, where its owner
    /// put it.
    pub order: Vec<String>,
    /// Whether each place the user showed or hid is shown. One it does
    /// not name is, unless its owner offers it hidden (`Place::hidden`).
    pub shown: BTreeMap<String, bool>,
    /// The names the user gave offered places.
    pub titles: BTreeMap<String, String>,
}

impl Arrangement {
    /// Whether the user changed anything in the group.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty() && self.shown.is_empty() && self.titles.is_empty()
    }
}

/// Who arranges a place in the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The user keeps it there (a bookmark, a saved search, a pinned
    /// tag): its owner orders it (`reorder`), renames it (`rename`) and
    /// takes it out (`forget`).
    Kept,
    /// Its owner suggests it (a tag used often): the user can hide it.
    Suggested,
    /// Its owner offers it (Home, the Trash, a volume): the user moves,
    /// renames and hides it, in the sidebar (`ui.sidebar_place`).
    Offered,
    /// Listed from what is in use now (`Place::listed`: what the user's
    /// tabs show, in a dialog's window): opened, nothing more.
    Listed,
}

impl Role {
    pub fn of(place: &Place) -> Self {
        if place.listed {
            Role::Listed
        } else if place.forget.is_some() || place.reorder.is_some() {
            Role::Kept
        } else if place.suggested {
            Role::Suggested
        } else {
            Role::Offered
        }
    }

    /// Whether the user can hide it: a place kept is taken out instead,
    /// and one listed comes and goes with what lists it.
    pub fn hides(self) -> bool {
        !matches!(self, Role::Kept | Role::Listed)
    }
}

/// The command that arranges a place the user does not keep.
const PLACE_COMMAND: &str = "ui.sidebar_place";

/// The group the sidebar lists the tabs in, first, while `ui.tabs_at` asks
/// (`Shell::sidebar_lists_tabs`): folded as the others, not moved or put
/// away (the setting takes it away).
pub const TABS: &str = skwir_core::registry::groups::TABS.id;

/// What the sidebar's keyboard cursor is on: a group's header, a place
/// (by its group and `Place::key`), or, in `TABS`, a tab or the row that
/// opens another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Group(String),
    Place { group: String, key: String },
    Tab(TabId),
    NewTab,
}

impl Item {
    fn of(place: &Place) -> Self {
        Item::Place {
            group: place.group.clone(),
            key: place.key(),
        }
    }

    fn group(&self) -> &str {
        match self {
            Item::Group(group) | Item::Place { group, .. } => group,
            Item::Tab(_) | Item::NewTab => TABS,
        }
    }
}

/// The sidebar's keyboard (F6, `ui.focus_next_part`; `ui.focus_sidebar`): its focus, the
/// cursor, and what the last frame drew, in order, which the keys move
/// along.
pub struct Keys {
    pub focus: gpui::FocusHandle,
    pub cursor: Option<Item>,
    /// Each item drawn with its position among the scrolled column's
    /// children (a line of pills is one child).
    drawn: Vec<(Item, usize)>,
    /// A key moved the cursor: the next frame scrolls it into view.
    reveal: bool,
}

impl Keys {
    pub fn new(focus: gpui::FocusHandle) -> Self {
        Keys {
            focus,
            cursor: None,
            drawn: Vec::new(),
            reveal: false,
        }
    }

    /// The keyboard comes here: the cursor goes where the pane is, as the
    /// next frame finds it, in view.
    pub fn arrive(&mut self) {
        self.cursor = None;
        self.reveal = true;
    }
}

/// What is drawn, in order: the tabs' header, its tabs and New tab, while
/// it lists `tabs`; then each group's header, then its shown places;
/// nothing under a group folded.
fn drawn_items(places: &[&Arranged], sections: &Sections, tabs: &[TabId]) -> Vec<Item> {
    let mut items = Vec::new();
    if !tabs.is_empty() {
        items.push(Item::Group(TABS.to_owned()));
        if !sections.folded.contains(TABS) {
            items.extend(tabs.iter().copied().map(Item::Tab));
            items.push(Item::NewTab);
        }
    }
    for arranged in places {
        let group = &arranged.place.group;
        if items
            .last()
            .is_none_or(|last: &Item| last.group() != group.as_str())
        {
            items.push(Item::Group(group.clone()));
        }
        if arranged.shown && !sections.folded.contains(group) {
            items.push(Item::of(&arranged.place));
        }
    }
    items
}

/// A place as the user arranged it: under the name they gave it, with
/// whether they show it, and how it moves among its group's.
#[derive(Clone, Debug)]
pub struct Arranged {
    pub place: Place,
    pub role: Role,
    pub shown: bool,
    /// Its owner's `reorder` for a place kept, the sidebar's own
    /// (`ui.sidebar_place`) for one offered.
    pub moves: Option<Reorder>,
    /// What tells it apart from another of its group shown under the same
    /// name, said after its name (`told_apart`): the first folder above it
    /// that differs (`skwir/…`).
    pub note: Option<String>,
    /// Where a place that is a folder's path leads, as its tooltip says it
    /// (`~/wip/skwir`).
    pub path: Option<String>,
}

/// Every place, shown or not, as the user arranged them: the groups in
/// their order; within a group, folders before virtual places (Shelf,
/// Recent) whatever order their plugins published them in, then the
/// offered places in the user's order among the slots they hold. A place
/// listed (`Place::listed`) keeps the order it was listed in.
pub fn arrange(mut places: Vec<Place>, sections: &Sections) -> Vec<Arranged> {
    // By name too, so that two groups the sidebar does not know never mix.
    // What is no folder comes after the folders (the servers the tools
    // know, after the mounts), unless it moves: what the user orders keeps
    // its owner's order (a remote bookmark among the folders'), and what
    // is listed the order it was listed in (Skwir Tabs, by use).
    let after = |p: &Place| !p.location.is_path() && p.reorder.is_none() && !p.listed;
    places.sort_by(|a, b| {
        (sections.rank(&a.group), &a.group, after(a)).cmp(&(
            sections.rank(&b.group),
            &b.group,
            after(b),
        ))
    });
    let mut slots: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, place) in places.iter().enumerate() {
        if Role::of(place) == Role::Offered {
            slots.entry(place.group.clone()).or_default().push(i);
        }
    }
    let mut slotted: Vec<Option<Place>> = places.into_iter().map(Some).collect();
    for (group, at) in &slots {
        let Some(order) = sections
            .places
            .get(group)
            .map(|a| &a.order)
            .filter(|o| !o.is_empty())
        else {
            continue;
        };
        let mut offered: Vec<Place> = at.iter().filter_map(|&i| slotted[i].take()).collect();
        // Stable: those the order does not name keep their owners' order.
        offered.sort_by_key(|p| {
            let key = p.key();
            order.iter().position(|k| *k == key).unwrap_or(usize::MAX)
        });
        for (&i, place) in at.iter().zip(offered) {
            slotted[i] = Some(place);
        }
    }
    let mut arranged = slotted
        .into_iter()
        .flatten()
        .map(|mut place| {
            let role = Role::of(&place);
            let key = place.key();
            let arrangement = sections.places.get(&place.group);
            let shown = shown_in_group(&place, sections);
            let mut moves = place.reorder.clone();
            if role == Role::Offered {
                // The name the user gave it is as they wrote it.
                if let Some(title) = arrangement.and_then(|a| a.titles.get(&key)) {
                    place.title = Words::as_written(title.clone());
                }
                moves = Some(Reorder {
                    command: Invocation::with_args(
                        PLACE_COMMAND,
                        &serde_json::json!({ "group": place.group, "key": key, "before": null }),
                    ),
                    before: serde_json::json!(key),
                });
            }
            let path = place
                .location
                .path()
                .map(|path| skwir_core::exclusions::tilde(&path));
            Arranged {
                place,
                role,
                shown,
                moves,
                note: None,
                path,
            }
        })
        .collect::<Vec<_>>();
    tell_apart(&mut arranged);
    arranged
}

/// Each place shown under the name another of its group is shown under,
/// both folders' paths, given what tells it apart (`told_apart`): two
/// bookmarks named `src`, two folders a program chose in.
fn tell_apart(arranged: &mut [Arranged]) {
    let mut named: HashMap<(&str, String), Vec<usize>> = HashMap::new();
    for (at, a) in arranged.iter().enumerate() {
        if a.shown && a.place.location.is_path() {
            named
                .entry((a.place.group.as_str(), a.place.title.say()))
                .or_default()
                .push(at);
        }
    }
    let home = skwir_core::os_path::home();
    let notes: Vec<(usize, Option<String>)> = named
        .into_values()
        .filter(|at| at.len() > 1)
        .flat_map(|at| {
            let paths: Vec<PathBuf> = at
                .iter()
                .filter_map(|&i| arranged[i].place.location.path())
                .collect();
            let paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
            let notes = told_apart(&paths, home.as_deref());
            at.into_iter().zip(notes).collect::<Vec<_>>()
        })
        .collect();
    for (at, note) in notes {
        arranged[at].note = note;
    }
}

/// What tells each of `paths` apart, folders shown under one name: the
/// first folder above it, counted up from its parent, where it differs
/// from each of the others at that height (`skwir/…`, the `…` standing
/// for the folders between; a parent alone without it), counted no
/// higher than home (`~`) or the root. Where two of these would read the
/// same, the folders above it as far as that one (`skwir/crates`). None
/// for a path another of them is too. Nothing is read.
pub(crate) fn told_apart(paths: &[&Path], home: Option<&Path>) -> Vec<Option<String>> {
    let sep = std::path::MAIN_SEPARATOR_STR;
    // Nearest first, the farthest last.
    let join = |words: &[String]| {
        let mut out = String::new();
        for word in words.iter().rev() {
            if !out.is_empty() && !out.ends_with(sep) {
                out.push_str(sep);
            }
            out.push_str(word);
        }
        out
    };
    let upward: Vec<Vec<String>> = paths.iter().map(|path| above(path, home)).collect();
    let heights: Vec<Option<usize>> = upward
        .iter()
        .enumerate()
        .map(|(i, mine)| {
            (1..=mine.len()).find(|&k| {
                upward
                    .iter()
                    .enumerate()
                    .all(|(j, other)| j == i || other.get(..k) != mine.get(..k))
            })
        })
        .collect();
    let short: Vec<Option<String>> = upward
        .iter()
        .zip(&heights)
        .map(|(mine, k)| match *k {
            Some(1) => Some(mine[0].clone()),
            Some(k) => Some(join(&["…".to_owned(), mine[k - 1].clone()])),
            None => None,
        })
        .collect();
    let mut counted: HashMap<&str, usize> = HashMap::new();
    for note in short.iter().flatten() {
        *counted.entry(note.as_str()).or_default() += 1;
    }
    short
        .iter()
        .zip(upward.iter().zip(&heights))
        .map(|(note, (mine, k))| {
            let note = note.as_deref()?;
            Some(if counted.get(note).copied().unwrap_or(0) > 1 {
                join(&mine[..(*k)?])
            } else {
                note.to_owned()
            })
        })
        .collect()
}

/// The names of the folders above `path`, its parent first: home as `~`,
/// the last of them, or the root (`/`, `C:\`).
fn above(path: &Path, home: Option<&Path>) -> Vec<String> {
    let mut words = Vec::new();
    let mut at = path.parent();
    while let Some(folder) = at {
        if Some(folder) == home {
            words.push("~".to_owned());
            break;
        }
        let Some(name) = folder.file_name() else {
            words.push(folder.to_string_lossy().into_owned());
            break;
        };
        words.push(name.to_string_lossy().into_owned());
        at = folder.parent();
    }
    words
}

/// The places of Skwir Tabs a dialog's window shows at most.
const TABS_SHOWN: usize = 5;

impl Shell {
    /// What the tabs of the user's own windows show, as they were used
    /// (`Workspace::tabs_by_use`: each window's shown tab first, the
    /// window used last first, then the others, the one left last first),
    /// those this window offers alone (not the Trash, ADR-144, 3). Read
    /// once a change of them (`arranged_places`'s `TabsRead`), never at
    /// each frame.
    fn tab_locations(&self) -> Vec<Location> {
        let shown: Vec<Location> = {
            let ws = self.app.workspace.read();
            ws.tabs_by_use(|window| !window.chooses())
                .iter()
                .filter_map(|tab| ws.pane(tab.active).and_then(|pane| pane.shown()).cloned())
                .collect()
        };
        shown
            .into_iter()
            .filter(|location| self.offers_location(location))
            .collect()
    }

    /// The places of Skwir Tabs (ADR-144, 8): what the user's tabs show
    /// (`tab_locations`), each named as its tab names it
    /// (`App::location_title`), listed (`Place::listed`): opened, nothing
    /// arranged. A folder with a folder's icon, anything else with the
    /// icon of a place of its kind, if one has it.
    fn tab_places(&self) -> Vec<Place> {
        let places = self.app.registry.places.all();
        self.tab_locations()
            .into_iter()
            .map(|location| {
                let icon = if location.is_path() {
                    "folder".to_owned()
                } else {
                    places
                        .iter()
                        .find(|place| place.location.scheme == location.scheme)
                        .and_then(|place| place.icon.clone())
                        .unwrap_or_else(|| "folder".to_owned())
                };
                Place {
                    listed: true,
                    ..Place::new(
                        skwir_core::registry::groups::SKWIR_TABS.id,
                        self.app.location_title(&location),
                        &icon,
                        location,
                    )
                }
            })
            .collect()
    }
}

/// Whether the sidebar shows `place` as the user arranged it: its group
/// not put away (folded, it still holds it), and the place not hidden by
/// the user (`Arrangement::shown`) nor offered hidden (`Place::hidden`),
/// unless the user keeps it (`Role::Kept`), which is never hidden.
fn shows(place: &Place, sections: &Sections) -> bool {
    !sections.hidden.contains(&place.group) && shown_in_group(place, sections)
}

/// Whether `place` is shown in its group: one the user keeps always, any
/// other as the user showed or hid it there, else as its owner offers it.
fn shown_in_group(place: &Place, sections: &Sections) -> bool {
    !Role::of(place).hides()
        || sections
            .places
            .get(&place.group)
            .and_then(|a| a.shown.get(&place.key()))
            .copied()
            .unwrap_or(!place.hidden)
}

/// Of `more` (what a section of the shell's own lists from what was used:
/// the folders a program chose in), those no place of `places` the
/// sidebar shows leads to, `limit` at most, in their order: one click
/// reaches the others already, and a dialog's sidebar is short of room
/// (ADR-144, 8).
pub(crate) fn shown_nowhere(
    places: &[Place],
    more: Vec<Place>,
    sections: &Sections,
    limit: usize,
) -> Vec<Place> {
    let shown: std::collections::HashSet<&Location> = places
        .iter()
        .filter(|place| shows(place, sections))
        .map(|place| &place.location)
        .collect();
    let mut seen = std::collections::HashSet::new();
    more.into_iter()
        .filter(|place| !shown.contains(&place.location) && seen.insert(place.location.clone()))
        .take(limit)
        .collect()
}

/// The places arranged (`arrange`) for a revision of the places, of the
/// folders a dialog's program chose in, of what words say, of what a
/// dialog's Skwir Tabs read (`TabsRead`), and an arrangement of them.
pub(crate) type ArrangedAt = (
    (u64, u64, u64, Option<TabsRead>),
    Sections,
    Arranging,
    std::sync::Arc<[Arranged]>,
);

/// What a dialog's Skwir Tabs are read from, each by what counts its
/// changes: what the tabs show (`Workspace::tabs_shown`), and what names
/// them (`App::location_title`: the remotes mounted, the names the user
/// gave places).
pub(crate) type TabsRead = ((u64, u64), u64, u64);

/// For whom the places are arranged (ADR-144, 3 and 7): a window of the
/// user's own, a dialog's, or a dialog's opening files where the files
/// used lately have one to give (`Chooser::offers_recent`), where what
/// lists them comes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arranging {
    Own,
    Dialog,
    DialogOpening,
}

impl Shell {
    /// The places this window offers (`App::places_offered`): a dialog's
    /// none whose files are not to give (the Trash, ADR-144, 3), and the
    /// folders its program last chose in (ADR-144, 8,
    /// `Chooser::folder_places`). What the sidebar lists and its headings'
    /// menus arrange.
    pub(crate) fn places_here(&self) -> Vec<Place> {
        let mut places = self.app.places_offered(self.chooser.is_some());
        if let Some(chooser) = &self.chooser {
            let tabs = shown_nowhere(
                &places,
                self.tab_places(),
                &self.sidebar_sections,
                TABS_SHOWN,
            );
            places.extend(tabs);
            let recent = shown_nowhere(
                &places,
                chooser.folder_places(),
                &self.sidebar_sections,
                crate::chooser::FOLDERS_SHOWN,
            );
            places.extend(recent);
        }
        places
    }

    /// Whether this window offers going to `location`
    /// (`App::offers_location`): the address field's places by their
    /// address ask it.
    pub(crate) fn offers_location(&self, location: &skwir_core::Location) -> bool {
        self.app.offers_location(self.chooser.is_some(), location)
    }

    /// Every place this window offers (`places_here`) as the user arranged
    /// them (`arrange`), kept until the places or their arrangement change:
    /// the sidebar draws them at every frame, and arranging copies and
    /// sorts every place.
    pub fn arranged_places(&self) -> std::sync::Arc<[Arranged]> {
        let revision = (
            self.app.registry.places.revision(),
            self.chooser
                .as_ref()
                .map_or(0, crate::chooser::Chooser::folders_revision),
            skwir_core::words::revision(),
            // What a dialog lists of the user's tabs, read again only once
            // what they show or what names them changed.
            self.chooser.is_some().then(|| {
                (
                    self.app.workspace.read().tabs_shown(),
                    self.app.registry.remotes.revision(),
                    self.app.registry.place_titles_revision(),
                )
            }),
        );
        let arranging = match &self.chooser {
            None => Arranging::Own,
            Some(chooser) if chooser.offers_recent() => Arranging::DialogOpening,
            Some(_) => Arranging::Dialog,
        };
        let mut kept = self.arranged_places.borrow_mut();
        if let Some((at, sections, was, arranged)) = kept.as_ref()
            && *at == revision
            && *sections == self.sidebar_sections
            && *was == arranging
        {
            return std::sync::Arc::clone(arranged);
        }
        // Read after the revision: a change meanwhile arranges again.
        let mut arranged = arrange(self.places_here(), &self.sidebar_sections);
        if arranging == Arranging::DialogOpening {
            recent_first(&mut arranged, |place| {
                self.app.lists_recent(&place.location)
            });
        }
        let arranged: std::sync::Arc<[Arranged]> = arranged.into();
        *kept = Some((
            revision,
            self.sidebar_sections.clone(),
            arranging,
            std::sync::Arc::clone(&arranged),
        ));
        arranged
    }
}

/// What lists the files used lately first in its group, the rest as they
/// were arranged (ADR-144, 7: a dialog opening files, where they have one
/// to give). A place listed keeps the order it was listed in (a tab
/// showing Recent, among Skwir Tabs by use).
fn recent_first(arranged: &mut [Arranged], recent: impl Fn(&Place) -> bool) {
    let mut first: HashMap<String, usize> = HashMap::new();
    for (at, one) in arranged.iter().enumerate() {
        first.entry(one.place.group.clone()).or_insert(at);
    }
    arranged.sort_by_key(|one| {
        let leads = one.role != Role::Listed && recent(&one.place);
        (first[&one.place.group], !leads)
    });
}

/// A folder the sidebar shows, where a trail starts (`address::crumbs`):
/// its path, and the name the sidebar gives it, as words: Skwir's own for
/// a place (Home, the noun whose grammar a sentence naming it reads,
/// ADR-139, 5), the user's as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    pub path: std::path::PathBuf,
    pub name: Words,
}

/// The folders the sidebar shows, in its order and under the names it
/// gives them (the user's, ADR-034), then the remotes mounted (`remotes`,
/// their mount points under their names: a server's place may open a
/// folder of it, and what Skwir named the folder it mounts in is no name
/// of the user's), then the home folder, which stands for what is above
/// it even when the sidebar does not show it: where trails start. A
/// place hidden, in a group put away, or not mounted is none; a remote's
/// mount point is one all the same, under its remote's name, whatever its
/// place (hidden, put away, its connection lost): a folder below it is
/// still on that remote.
pub fn roots(
    places: Vec<Place>,
    remotes: Vec<skwir_core::fs::reach::Known>,
    sections: &Sections,
    home: Option<&std::path::Path>,
) -> Vec<Root> {
    let mut roots: Vec<Root> = arrange(places, sections)
        .into_iter()
        .filter(|a| {
            a.shown
                && !sections.hidden.contains(&a.place.group)
                && a.place.activate.is_none()
                && a.place.unavailable.is_none()
                && a.place.is_its_location()
        })
        .filter_map(|a| {
            Some(Root {
                path: a.place.location.path()?,
                name: a.place.title.clone(),
            })
        })
        .collect();
    let rooted: std::collections::HashSet<std::path::PathBuf> =
        roots.iter().map(|r| r.path.clone()).collect();
    roots.extend(
        remotes
            .into_iter()
            .filter(|known| !rooted.contains(&known.point))
            .map(|known| Root {
                path: known.point,
                name: known.label,
            }),
    );
    if let Some(home) = home.filter(|home| !roots.iter().any(|r| r.path == *home)) {
        roots.push(Root {
            path: home.to_path_buf(),
            name: words!(
                sidebar_root_home = "Home",
                noun,
                note = "Where a folder's trail starts when the sidebar shows none of the folders above it: the home folder."
            ),
        });
    }
    roots
}

/// `roots`, read again only when the places, the remotes mounted, their
/// arrangement, what their words say (`words::revision`) or
/// `ui.full_path` change, and how many times they were. With the full
/// path there are none: every folder's trail starts at the root, each
/// folder under its own name.
#[derive(Default)]
pub struct Roots {
    key: Option<((u64, u64), u64, Sections, bool)>,
    pub roots: Vec<Root>,
    pub generation: u64,
}

impl Shell {
    /// Read the trails' roots again if the places, the remotes mounted or
    /// their arrangement changed since.
    pub fn sync_roots(&mut self) {
        let revision = (
            self.app.registry.places.revision(),
            self.app.registry.remotes.revision(),
        );
        let said = skwir_core::words::revision();
        let full = self.prefs.full_path;
        if self.roots.key.as_ref().is_some_and(|(r, w, s, f)| {
            *r == revision && *w == said && *s == self.sidebar_sections && *f == full
        }) {
            return;
        }
        // The names the user gave places, for everything that names a
        // location (`App::location_title`): their own windows', which every
        // window shares. A dialog's arrangement is the dialogs' (ADR-144,
        // 4), and taken from the user's at the first: written there, it
        // would give back names the user changed since, and a dialog's
        // Skwir Tabs name the tabs as the user's windows do.
        if self.chooser.is_none() {
            let titles = self
                .sidebar_sections
                .places
                .iter()
                .flat_map(|(group, arrangement)| {
                    arrangement
                        .titles
                        .iter()
                        .map(move |(key, title)| ((group.clone(), key.clone()), title.clone()))
                })
                .collect();
            self.app.registry.set_place_titles(titles);
        }
        let home = skwir_core::os_path::home();
        self.roots = Roots {
            roots: if full {
                Vec::new()
            } else {
                roots(
                    self.app.registry.places.all(),
                    self.app.registry.remotes.all(),
                    &self.sidebar_sections,
                    home.as_deref(),
                )
            },
            key: Some((revision, said, self.sidebar_sections.clone(), full)),
            generation: self.roots.generation + 1,
        };
    }
}

impl Sections {
    /// Where `group` goes among the others: where the user left it; one
    /// they left nowhere, a group the sidebar ships with comes where it
    /// ships, before the first group left that ships after it (a group
    /// new in this version, whatever the user moved before); any other
    /// goes last.
    pub fn rank(&self, group: &str) -> (usize, usize) {
        let ships = group_rank(group);
        let placed = self.order.iter().position(|g| g == group).or_else(|| {
            (ships < GROUP_ORDER.len())
                .then(|| self.order.iter().position(|g| group_rank(g) > ships))
                .flatten()
        });
        (placed.unwrap_or(self.order.len()), ships)
    }

    /// `present`, the groups that have places, in the user's order.
    pub fn arranged(&self, present: &[String]) -> Vec<String> {
        let mut groups = present.to_vec();
        groups.sort_by_key(|g| self.rank(g));
        groups.dedup();
        groups
    }

    /// Put `group` before `before`, or last. `present` names the groups
    /// that have places now, so a group that comes back later keeps the
    /// place it had among them.
    pub fn move_before(&mut self, group: &str, before: Option<&str>, present: &[String]) {
        let mut known: Vec<String> = self.order.clone();
        known.extend(GROUP_ORDER.iter().map(|g| (*g).to_owned()));
        known.extend(present.iter().cloned());
        let mut order = self.arranged(&known);
        order.retain(|g| g != group);
        let at = before
            .and_then(|b| order.iter().position(|g| g == b))
            .unwrap_or(order.len());
        order.insert(at, group.to_owned());
        self.order = order;
    }

    /// Whether anything differs from the sidebar as it ships.
    pub fn arranged_by_user(&self) -> bool {
        !self.order.is_empty() || !self.hidden.is_empty()
    }

    /// Put `group`'s offered place `key` before `before`, or last.
    /// `present` names the group's offered places now, in their order, so
    /// that one absent keeps the place it had among them.
    pub fn move_place(&mut self, group: &str, key: &str, before: Option<&str>, present: &[String]) {
        let arrangement = self.places.entry(group.to_owned()).or_default();
        let mut order = arrangement.order.clone();
        order.extend(
            present
                .iter()
                .filter(|k| !arrangement.order.contains(k))
                .cloned(),
        );
        order.retain(|k| k != key);
        let at = before
            .and_then(|b| order.iter().position(|k| k == b))
            .unwrap_or(order.len());
        order.insert(at, key.to_owned());
        arrangement.order = order;
    }
}

/// What F9 or F6 says where the window has no room for the sidebar.
fn too_narrow() -> Words {
    words!(
        sidebar_too_narrow = "The window is too narrow for the sidebar",
        note = "Said when the sidebar is asked for (F9, F6) in a window too narrow to show it beside the files."
    )
}

/// The words a section's heading says: core's, by its id (ADR-139, 8);
/// none for a section core does not own.
fn group_title(group: &str) -> Words {
    skwir_core::registry::groups::words_of(group).unwrap_or_default()
}

/// A place being dragged: to another position in its group, if it moves
/// (`reorder`), or onto files, if it gives them something (`gives`, a
/// tag: `dnd::takes_gifts`).
#[derive(Clone, Debug)]
pub struct PlaceDrag {
    pub group: String,
    /// Its position among the group's movable places when the drag started.
    pub index: usize,
    pub reorder: Option<Reorder>,
    /// What dropping it on files runs, with the files: what its
    /// location takes (`Verbs::drop`, a tag's gives its tag).
    pub gives: Option<Invocation>,
}

/// What a movable place moves among: its group's places moved by the
/// same command.
fn mover(group: &str, moves: &Reorder) -> (String, String) {
    (group.to_owned(), moves.command.name.to_string())
}

impl PlaceDrag {
    /// What it moves among (`mover`), if it moves.
    fn mover(&self) -> Option<(String, String)> {
        Some(mover(&self.group, self.reorder.as_ref()?))
    }

    /// The command that moves it before the place whose `before` is given,
    /// or last.
    fn command(&self, before: Option<serde_json::Value>) -> Option<Invocation> {
        let mut command = self.reorder.as_ref()?.command.clone();
        if let Some(before) = before
            && let Some(args) = command.args.as_object_mut()
        {
            args.insert("before".into(), before);
        }
        Some(command)
    }
}

/// Where a place dragged from `from` goes when dropped in the gap before
/// the place at `gap` (`order.len()` for the end): before that place,
/// `None` for last. The outer `None` when the gap is on either side of the
/// dragged place, where it already is.
pub fn drop_target<T: Clone>(order: &[T], from: usize, gap: usize) -> Option<Option<T>> {
    (gap != from && gap != from + 1).then(|| order.get(gap).cloned())
}

/// What activating `place` runs: its own command (mount, unlock), or
/// opening its location as it opens (`Place::open`, ADR-109).
pub fn open_invocation(place: &Place) -> Invocation {
    place.open()
}

/// Opening `place` in a tab behind the one shown: a middle click on its
/// row, or its menu. One that must be mounted or unlocked first runs that
/// command with `tab`, which opens the volume there once it is mounted;
/// one kept with its filter opens as kept (ADR-109).
pub fn open_in_tab(place: &Place) -> Invocation {
    if let Some(activate) = &place.activate {
        return activate.clone().with("tab", true);
    }
    let tab = Invocation::new("tab.new")
        .with("location", place.location.arg())
        .with("show", false);
    match &place.opens {
        Opens::Lensed => tab,
        Opens::AsKept(filter) => tab.with("as_kept", true).with("filter", filter.as_deref()),
    }
}

/// What opens `place` in a window of its own (ADR-122), for a place that is
/// a location shown with the pane's lens: one its owner opens itself (a
/// volume mounted first) or that is kept as it was shown has none.
pub fn open_in_window(place: &Place) -> Option<Invocation> {
    (place.activate.is_none() && matches!(place.opens, Opens::Lensed))
        .then(|| Invocation::new("window.new").with("location", place.location.arg()))
}

/// Share of the volume in use, 0 to 1.
pub fn used_fraction(free: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.;
    }
    (1. - free as f64 / total as f64).clamp(0., 1.) as f32
}

/// The sidebar, `width` px wide (`Shell::sidebar_drawn_width`).
pub fn render(
    shell: &mut Shell,
    width: f32,
    window: &mut gpui::Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let current = shell.ui().address_shown.clone();
    let (shown, filter) = {
        let ws = shell.app.workspace.read();
        let p = ws.pane(shell.pane);
        (
            p.and_then(|p| p.location().cloned()),
            p.and_then(|p| p.filter().map(str::to_owned)),
        )
    };
    // A remote folder is also its address's place (a saved place keeps it
    // so, ADR-077): asked once, outside the workspace's lock.
    let current_kept = shown
        .map(|shown| shell.app.kept_location(&shown).to_string())
        .filter(|kept| *kept != current);
    // Groups in the user's order, those put away left out; places as the
    // user arranged them (`arrange`). A group whose places are all hidden
    // keeps its header, whose menu shows them again.
    let sections = shell.sidebar_sections.clone();
    let arranged = shell.arranged_places();
    let places: Vec<&Arranged> = arranged
        .iter()
        .filter(|a| !sections.hidden.contains(&a.place.group))
        .collect();
    let groups: Vec<String> = sections.arranged(
        &places
            .iter()
            .map(|a| a.place.group.clone())
            .collect::<Vec<_>>(),
    );
    // A ring lasts while the place that rings is drawn: one folded away,
    // put away, unpinned or renamed is never left by the pointer.
    if shell.ringed.as_ref().is_some_and(|(at, _)| {
        !places.iter().any(|a| {
            a.shown && !sections.folded.contains(&a.place.group) && a.place.location == *at
        })
    }) {
        shell.ringed = None;
    }
    // The tabs, first, while the sidebar lists them (`ui.tabs_at`).
    let tabs: Vec<TabId> = if shell.sidebar_lists_tabs() {
        shell.sync_tab_labels();
        shell.tab_labels.labels.iter().map(|l| l.tab).collect()
    } else {
        Vec::new()
    };
    // With the keyboard, the cursor is on something drawn: where it was,
    // else the place shown, else the first.
    let focused = shell.sidebar_keys.focus.is_focused(window);
    let items = drawn_items(&places, &sections, &tabs);
    if focused
        && shell
            .sidebar_keys
            .cursor
            .as_ref()
            .is_none_or(|c| !items.contains(c))
    {
        shell.sidebar_keys.cursor = places
            .iter()
            .filter(|a| a.shown && !sections.folded.contains(&a.place.group))
            .find(|a| a.place.location.to_string() == current)
            .map(|a| Item::of(&a.place))
            .or_else(|| items.first().cloned());
    }
    // Its ring only while the sidebar has the keyboard; where a menu the
    // keys opened hangs from all the same, as the menu takes the keyboard.
    let marked = shell.sidebar_keys.cursor.clone();
    let cursor = marked.clone().filter(|_| focused);
    let mut drawn: Vec<(Item, usize)> = Vec::with_capacity(items.len());
    // The movable places drawn, in order, by group and by the command that
    // moves them, with what names each to it: a place moves among those
    // its own command moves (bookmarks among bookmarks, Home among the
    // places the sidebar arranges).
    let mut orders: HashMap<(String, String), Vec<serde_json::Value>> = HashMap::new();
    for arranged in places.iter().filter(|a| a.shown) {
        if let Some(moves) = &arranged.moves {
            orders
                .entry(mover(&arranged.place.group, moves))
                .or_default()
                .push(moves.before.clone());
        }
    }
    // Shared by every gap of the group: each gap copied the whole order,
    // the square of the group's places each frame.
    let orders: HashMap<(String, String), std::sync::Arc<[serde_json::Value]>> = orders
        .into_iter()
        .map(|(mover, order)| (mover, order.into()))
        .collect();
    let no_order: std::sync::Arc<[serde_json::Value]> = std::sync::Arc::new([]);

    // A drop's tint while a drag is over it, laid on the sidebar's ground
    // (its own, or the frame as cards), which is painted once below it.
    let ground = theme.chrome_color();
    let drop_tint = crate::color::overlay(ground, ground.blend(theme.drop_target));
    let a11y = shell.a11y;
    // A tree for assistive technology: each group an item of the first
    // level, its places under it, the keys' cursor the active descendant.
    let mut column = div()
        .id("sidebar")
        .role(gpui::Role::Tree)
        .when(a11y, |d| {
            d.aria_label(
                words!(
                    sidebar_name = "Sidebar",
                    note = "What a screen reader calls the sidebar: the places, bookmarks, tags and devices at the window's left."
                )
                .say(),
            )
        })
        .size_full()
        .flex()
        .flex_col()
        .overflow_y_scroll()
        .track_scroll(&shell.sidebar_scroll)
        // The keyboard comes here with F6 only: a click opens a place and
        // leaves the keyboard where it was, with the files.
        .track_focus(&shell.sidebar_keys.focus)
        .key_context(crate::keymap::placed(
            crate::keymap::SIDEBAR,
            shell.chooser.is_some(),
        ))
        .on_any_mouse_down(|_, window, _| window.prevent_default())
        .when(theme.lines(), |d| d.border_end(px(1.)).border_color(theme.border))
        .py_1()
        // A folder dropped beside the rows becomes a bookmark; a drop on a
        // row is that row's and never reaches here.
        .drag_over::<DragPayload>(move |s, _, _, _| s.bg(drop_tint))
        .on_drop(cx.listener(|this, payload: &DragPayload, window, cx| {
            let paths = this.drag_paths(payload.pane);
            this.dropped_beside_places(&paths, window, cx);
        }))
        // The same from another application, or from our own drag once the
        // compositor carried it.
        .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(drop_tint))
        .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
            this.dropped_beside_places(paths.paths(), window, cx);
        }))
        // A place dropped below the rows goes last in its group.
        .on_drop(cx.listener(|this, drag: &PlaceDrag, window, cx| {
            if let Some(command) = drag.command(None) {
                this.run(&command.name, command.args, window, cx);
            }
        }))
        // Beside the rows, the menu of the groups: those put away.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_sections_menu(None, event.position, window, cx);
            }),
        );

    // How many children the column has: what the cursor scrolls to.
    let mut children = 0;
    if !tabs.is_empty() {
        let item = Item::Group(TABS.to_owned());
        let folded = sections.folded.contains(TABS);
        let at_cursor = cursor.as_ref() == Some(&item);
        column = column.child(
            group_header(TABS, folded, None, at_cursor, cx)
                .role(gpui::Role::TreeItem)
                .when(a11y, |d| {
                    d.aria_label(group_title(TABS).say())
                        .aria_level(1)
                        .aria_expanded(!folded)
                })
                .when(
                    at_cursor,
                    StatefulInteractiveElement::aria_active_descendant,
                ),
        );
        drawn.push((item, children));
        children += 1;
        if !folded {
            let tab = |item: &Option<Item>| match item {
                Some(Item::Tab(tab)) => Some(*tab),
                _ => None,
            };
            for (id, row) in crate::tabs::sidebar_rows(shell, tab(&cursor), tab(&marked), cx) {
                column = column.child(row);
                drawn.push((Item::Tab(id), children));
                children += 1;
            }
            let at_cursor = cursor == Some(Item::NewTab);
            column = column.child(crate::tabs::sidebar_new_tab(shell, at_cursor, cx));
            drawn.push((Item::NewTab, children));
            children += 1;
        }
    }
    let mut last_group: Option<String> = None;
    let mut indices: HashMap<(String, String), usize> = HashMap::new();
    // A group's tags, drawn as their chips, wrapping together.
    let mut pills: Vec<AnyElement> = Vec::new();
    for arranged in places {
        let Arranged {
            place,
            moves,
            shown,
            note,
            path,
            ..
        } = arranged;
        let i = row_key(place);
        let folded = sections.folded.contains(&place.group);
        if last_group.as_deref() != Some(place.group.as_str()) {
            column = wrap_pills(column, &mut pills, &mut children);
            let item = Item::Group(place.group.clone());
            let at_cursor = cursor.as_ref() == Some(&item);
            let menu_here = marked.as_ref() == Some(&item);
            column = column.child(
                group_header(&place.group, folded, Some(&groups), at_cursor, cx)
                    .role(gpui::Role::TreeItem)
                    .when(a11y, |d| {
                        d.aria_label(group_title(&place.group).say())
                            .aria_level(1)
                            .aria_expanded(!folded)
                    })
                    .when(
                        at_cursor,
                        StatefulInteractiveElement::aria_active_descendant,
                    )
                    .children(shell.host_marker(MenuHost::Sidebar, menu_here)),
            );
            drawn.push((item, children));
            children += 1;
            last_group = Some(place.group.clone());
        }
        if folded || !shown {
            continue;
        }
        // What it is called, as the sidebar says it.
        let said = place.title.say();
        let item = Item::of(place);
        let at_cursor = cursor.as_ref() == Some(&item);
        let menu_here = marked.as_ref() == Some(&item);
        // A saved place is the one shown only filtered as it was kept
        // (ADR-109); any other is its location's.
        let named = place.location.to_string();
        let is_current = (named == current || current_kept.as_ref() == Some(&named))
            && (place.is_its_location() || place.filter() == filter.as_deref());
        let color = place.color.as_deref().and_then(|c| {
            skwir_core::metadata::TAG_COLORS
                .iter()
                .position(|x| *x == c)
        });
        // Its position among the places it moves among, if it moves.
        let index = moves.as_ref().map(|moves| {
            let index = indices.entry(mover(&place.group, moves)).or_default();
            *index += 1;
            *index - 1
        });
        // What its location takes, it gives files it is dragged onto.
        let gives = if place.location.is_path() {
            None
        } else {
            shell.app.verbs(&place.location).drop
        };
        let drag = PlaceDrag {
            group: place.group.clone(),
            index: index.unwrap_or_default(),
            reorder: moves.clone(),
            gives: gives.clone(),
        };
        let order = moves
            .as_ref()
            .and_then(|moves| orders.get(&mover(&place.group, moves)))
            .unwrap_or(&no_order);
        if let Some(color) = color {
            let mut pill = pill(i, place, color, is_current, at_cursor, cx)
                .role(gpui::Role::TreeItem)
                .when(a11y, |d| {
                    d.aria_label(said.clone())
                        .aria_level(2)
                        .aria_selected(is_current)
                })
                .when(
                    at_cursor,
                    StatefulInteractiveElement::aria_active_descendant,
                )
                .children(shell.host_marker(MenuHost::Sidebar, menu_here));
            pill = rings_its_files(pill, &place.location, cx);
            pill = takes_drops(pill, &place.location, gives.is_some(), cx);
            if drag.reorder.is_some() || drag.gives.is_some() {
                pill = movable_pill(pill, said.clone(), drag, order, cx);
            }
            // In the line of pills that is the column's next child.
            drawn.push((item, children));
            pills.push(pill.into_any_element());
            continue;
        }
        column = wrap_pills(column, &mut pills, &mut children);
        // Present but not mounted: clicking mounts it (or asks to unlock it).
        let open = open_invocation(place);
        let waiting = place.unavailable.is_some();
        // Its owner is working on it (unlocking, mounting, unmounting): an
        // hourglass in place of its icon, still, as nothing here animates,
        // and what is being done in place of its details.
        let busy = place.busy.clone();
        let themed = if busy.is_some() {
            None
        } else {
            place
                .icon
                .as_deref()
                .and_then(|name| shell.icons.named(name, ICON_PX as u16))
        };
        let icon: AnyElement = match themed {
            Some(path) => crate::icons::themed(
                &path,
                ICON_PX,
                crate::icons::Ink::beside(&theme, is_current),
            ),
            None if busy.is_some() => {
                controls::glyph("busy", theme.accent, ICON_PX).into_any_element()
            }
            // A shape, not ink: chosen in the selection's text.
            None => div()
                .size(px(10.))
                .mx(px(3.))
                .rounded_sm()
                .bg(if is_current {
                    theme.selection_text
                } else {
                    theme.text_muted
                })
                .flex_none()
                .into_any_element(),
        };
        // A title cut by the sidebar's width is said whole by the row's
        // tooltip, before what the row has to say of its own. What tells
        // it apart from a place of the same name follows it, cut first.
        let (title, clip) = crate::elements::clip::text(said.clone());
        let named = div()
            .min_w_0()
            .flex()
            .items_baseline()
            .gap(sized(6.))
            .whitespace_nowrap()
            .child(div().flex_shrink_0().max_w_full().truncate().child(title))
            .when_some(note.clone(), |d, note| {
                d.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_small()
                        .text_color(theme.text_muted)
                        .child(note),
                )
            });
        let mut texts = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(sized(3.))
            .child(named);
        let mut tip: Option<SharedString> = None;
        if let Some(busy) = &busy {
            texts = texts.child(
                div()
                    .text_small()
                    .truncate()
                    .text_color(theme.text_muted)
                    .child(busy.say()),
            );
        } else if let Some((free, total)) = place.space {
            let used = used_fraction(free, total);
            tip = Some(
                words!(
                    sidebar_space = "{ $free } free of { $total }",
                    free = Size(free),
                    total = Size(total),
                    note = "A volume's tooltip in the sidebar: the room left on it, of its size (12.3 GB free of 500 GB)."
                )
                .say()
                .into(),
            );
            texts = texts.child(
                div()
                    .h(px(3.))
                    .w_full()
                    .rounded_full()
                    .bg(theme.border)
                    .child(div().h_full().rounded_full().w(gpui::relative(used)).bg(
                        if used > 0.9 {
                            theme.warning
                        } else {
                            theme.accent
                        },
                    )),
            );
        }
        let hover = theme.hover;
        let mut row = div()
            .id(crate::elements::known_by("place", i))
            .flex_none()
            .min_h(sized(26.))
            .py(sized(3.))
            .mx_1()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .border_1()
            // The cursor's ring is a mark: on the selection, in its text.
            .border_color(match (at_cursor, is_current) {
                (true, true) => theme.selection_text,
                (true, false) => theme.accent,
                (false, _) => gpui::transparent_black(),
            })
            .when(busy.is_none(), gpui::Styled::cursor_pointer)
            .when(is_current, |d| d.chosen(&theme))
            .when(!is_current, |d| d.hover(move |s| s.bg(hover)))
            .on_click(cx.listener({
                let busy = busy.is_some();
                let item = item.clone();
                let clicks = place.clicks.clone();
                move |this, ev: &gpui::ClickEvent, window, cx| {
                    this.sidebar_keys.cursor = Some(item.clone());
                    // A second click while it unlocks would ask again.
                    if !busy {
                        let run = clicked(&clicks, &open, ev.modifiers(), this.pane);
                        this.run(&run.name, run.args, window, cx);
                    }
                }
            }))
            .children(shell.host_marker(MenuHost::Sidebar, menu_here))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let (group, key) = (place.group.clone(), place.key());
                    move |this, event: &gpui::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_place_menu(&group, &key, event.position, window, cx);
                    }
                }),
            )
            // The middle button opens it in a tab behind the one shown;
            // not again while its owner works on it, as a click.
            .when(busy.is_none(), |row| {
                let tab = open_in_tab(place);
                row.on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.run(&tab.name, tab.args.clone(), window, cx);
                    }),
                )
            })
            .child(
                div()
                    .flex_none()
                    .when(waiting && busy.is_none(), |d| d.opacity(0.55))
                    .child(icon),
            )
            .child(texts.when(waiting, |d| d.text_color(theme.text_muted)));
        if let Some(why) = &place.unavailable {
            tip = Some(why.say().into());
        }
        if let Some(busy) = &busy {
            tip = Some(busy.say().into());
        }
        // Where a folder's place leads, first: what tells two of a name
        // apart said whole.
        if let Some(path) = path {
            tip = Some(match tip {
                Some(tip) => format!("{path}\n{tip}").into(),
                None => path.clone().into(),
            });
        }
        // What the tooltip says of it (where it leads, its space, why it
        // waits, what its owner does) is what a screen reader says after
        // its name.
        row = row
            .role(gpui::Role::TreeItem)
            .when(a11y, |d| {
                d.aria_label(said.clone())
                    .aria_level(2)
                    .aria_selected(is_current)
                    .when_some(tip.clone(), StatefulInteractiveElement::aria_description)
            })
            .when(
                at_cursor,
                StatefulInteractiveElement::aria_active_descendant,
            );
        row = rings_its_files(row, &place.location, cx);
        row = row.optional_tooltip(clip.tooltip(tip));
        if let Some(eject) = place.eject.clone().filter(|_| busy.is_none()) {
            row = row.child(
                div()
                    .id(crate::elements::known_by("eject", i))
                    .role(gpui::Role::Button)
                    .when(a11y, |d| {
                        d.aria_label(
                            words!(
                                sidebar_eject = "Eject { $place }",
                                place = place.title.clone(),
                                note = "What a screen reader calls a volume's eject button in the sidebar; place: the volume, as the sidebar names it."
                            )
                            .say(),
                        )
                    })
                    .flex_none()
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .hover(|s| s.chosen(&theme))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.run(&eject.name, eject.args.clone(), window, cx);
                    }))
                    .child(controls::glyph("eject", theme.text_muted, 13.)),
            );
        }
        if drag.reorder.is_some() {
            row = movable_row(row, said, drag, order, cx);
        }
        row = takes_drops(row, &place.location, gives.is_some(), cx);
        column = column.child(row);
        drawn.push((item, children));
        children += 1;
    }
    column = wrap_pills(column, &mut pills, &mut children);
    // What the keys move along, and the cursor brought into view once a
    // key moved it, now that its row's place is known.
    if shell.sidebar_keys.reveal
        && let Some((_, child)) = drawn
            .iter()
            .find(|(item, _)| Some(item) == shell.sidebar_keys.cursor.as_ref())
    {
        shell.sidebar_scroll.scroll_to_item(*child);
    }
    shell.sidebar_keys.reveal = false;
    shell.sidebar_keys.drawn = drawn;
    // The bar is a sibling of the scrolling column, not a child: a child
    // would scroll away with the places.
    // Its right edge is dragged like the preview's left one; a double click
    // gives the sidebar its default width back.
    let edge = crate::elements::edge::edge(
        "sidebar-resize",
        crate::elements::edge::Side::End,
        SidebarResize,
        |this| this.sidebar_width = DEFAULT_WIDTH,
        cx,
    );
    // Its header holds the window's toggle and More, level with the panes'
    // rows; the places scroll below it.
    let header = crate::address::sidebar_header(shell, cx);
    div()
        .relative()
        .w(px(width))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .child(header)
        // Its ground, once, around the tab shown when the tabs are listed
        // here (`elements::ground`); the frame's as cards.
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(match theme.chrome_ground() {
                    Some(color) => {
                        crate::elements::ground::ground(&shell.sidebar_holes, color, column)
                            .into_any_element()
                    }
                    None => column.into_any_element(),
                })
                .child(crate::elements::scrollbar::vertical(
                    "sidebar-scrollbar",
                    crate::elements::scrollbar::Scrolls::Offset(shell.sidebar_scroll.clone()),
                )),
        )
        .child(edge)
        // Where a menu the keys opened hangs when its row is not drawn:
        // outside the scrolled column, whose height it would add to.
        .children(shell.host_area_marker(MenuHost::Sidebar))
        .into_any_element()
}

/// What a click on a place runs: with a modifier, what the place says
/// (`Place::clicks`: a tag's pill narrows with Shift and shows its files
/// here with Alt, All Tags with Alt the tags of the folder shown), given
/// the pane; else opening it.
fn clicked(
    clicks: &[skwir_core::registry::Click],
    open: &Invocation,
    modifiers: gpui::Modifiers,
    pane: skwir_core::workspace::PaneId,
) -> Invocation {
    let held = if modifiers.shift {
        "shift"
    } else if modifiers.alt {
        "alt"
    } else {
        ""
    };
    clicks
        .iter()
        .find(|click| click.modifier == held)
        .map_or_else(
            || open.clone(),
            |click| click.command.clone().with("pane", pane),
        )
}

/// A group dragged by its header to another position among the groups.
#[derive(Clone, Debug)]
pub struct GroupDrag {
    pub group: String,
}

/// The command that puts `group` before `before`, or last.
fn move_group(group: &str, before: Option<&str>) -> Invocation {
    Invocation::with_args(
        "ui.sidebar_section",
        &serde_json::json!({ "group": group, "before": before }),
    )
}

/// A group's header: a click folds the group. A group the user arranges
/// among the others (`shown`, in their order; not the tabs) is moved by
/// dragging its header and has a menu on a right click; while another
/// group is dragged over it, a line marks where that one lands, above its
/// upper half and below its lower half, as with places.
fn group_header(
    group: &str,
    folded: bool,
    shown: Option<&[String]>,
    at_cursor: bool,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.global::<Theme>().clone();
    let hover = theme.hover;
    let title = group_title(group).say();
    let fold = group.to_owned();
    let header = div()
        .id(SharedString::from(format!("sidebar-group-{group}")))
        .relative()
        .flex_none()
        .mx_1()
        .mt_2()
        .px_2()
        .h(sized(22.))
        .flex()
        .items_center()
        .justify_between()
        .rounded_md()
        .border_1()
        .border_color(if at_cursor {
            theme.accent
        } else {
            gpui::transparent_black()
        })
        .text_small()
        .text_color(theme.text_muted)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.sidebar_keys.cursor = Some(Item::Group(fold.clone()));
            this.run(
                "ui.sidebar_section",
                serde_json::json!({ "group": fold }),
                window,
                cx,
            );
        }))
        .child(title.clone())
        .child(controls::glyph(
            if folded { "chevron-right" } else { "chevron" },
            theme.text_muted,
            12.,
        ));
    let Some(shown) = shown else {
        return header;
    };
    let at = shown.iter().position(|g| g == group).unwrap_or(0);
    let menu = group.to_owned();
    let drag = GroupDrag {
        group: group.to_owned(),
    };
    let zone = |top: bool| {
        let edge = theme.accent;
        // The group the line puts the dragged one before: this one from
        // above, the next from below.
        let before = if top {
            shown.get(at).cloned()
        } else {
            shown.get(at + 1).cloned()
        };
        let (here, dropped) = (group.to_owned(), group.to_owned());
        let lands = before.clone();
        let zone = div().absolute().left_0().right_0().h(gpui::relative(0.5));
        let zone = if top { zone.top_0() } else { zone.bottom_0() };
        zone.drag_over::<GroupDrag>(move |s, drag, _, _| {
            // Nothing moves where it already is.
            if drag.group == here || lands.as_deref() == Some(drag.group.as_str()) {
                s
            } else if top {
                s.border_t_2().border_color(edge)
            } else {
                s.border_b_2().border_color(edge)
            }
        })
        .on_drop(cx.listener(move |this, drag: &GroupDrag, window, cx| {
            cx.stop_propagation();
            if drag.group == dropped || before.as_deref() == Some(drag.group.as_str()) {
                return;
            }
            let command = move_group(&drag.group, before.as_deref());
            this.run(&command.name, command.args, window, cx);
        }))
    };
    let (upper, lower) = (zone(true), zone(false));
    header
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_sections_menu(Some(&menu), event.position, window, cx);
            }),
        )
        .on_drag(drag, move |_, position, _, cx| {
            cx.new(|_| crate::dnd::DragPreview::named(title.clone(), position))
        })
        .child(upper)
        .child(lower)
}

/// The pills gathered so far, as one line that wraps, left-aligned (a
/// justified line would stretch the gaps of its last pills apart).
fn wrap_pills(
    column: gpui::Stateful<gpui::Div>,
    pills: &mut Vec<AnyElement>,
    children: &mut usize,
) -> gpui::Stateful<gpui::Div> {
    if pills.is_empty() {
        return column;
    }
    *children += 1;
    column.child(
        div()
            .flex_none()
            .mx_1()
            .px_1()
            .py_1()
            .flex()
            .flex_wrap()
            .gap(sized(4.))
            .children(pills.drain(..)),
    )
}

/// What a place's row is known by from one frame to the next: the place it
/// shows, never where it stands (`elements::known_by`). A place that comes
/// to stand where another was (a server's, once the mount above it is let
/// go of) is not given that one's hover or tooltip.
fn row_key(place: &Place) -> u64 {
    // By its English, which no language chosen changes.
    let title = place.title.english();
    crate::elements::key_of((&place.group, &place.location, &place.key, &title))
}

/// A place as a sentence names it: Skwir's own words for it (Home,
/// Trash), or a plugin's, as the noun they are, whose gender and forms a
/// translation reads (« la Corbeille masquée », ADR-139, 5); a tag's by
/// the tag it stands for (data); anything else (a volume's label, a name
/// the user gave) as a name, said plainly, quoted and isolated by the
/// sentence.
fn named(place: &Place) -> words::Arg {
    match &place.tag {
        Some(tag) => words::Name(tag.clone()).into(),
        None if place.title.is_message() => place.title.clone().into(),
        None => words::Name(place.title.say_plain()).into(),
    }
}

/// A tag's place as its chip (`tags::place_chip`), as the views draw tags:
/// it opens as a row does (a click, a middle click for a tab behind, a
/// right click for its menu), and files dropped on it get what its `drop`
/// gives them (the tag). One its owner suggests has its dot without the
/// tint; the one shown is filled deeper and edged with the accent, as the
/// keyboard's is edged.
fn pill(
    i: u64,
    place: &Place,
    color: usize,
    is_current: bool,
    at_cursor: bool,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.global::<Theme>().clone();
    let tone = theme.tag(color);
    let open = open_invocation(place);
    let tab = open_in_tab(place);
    // A tag in a category says its own name, from the tag it stands for
    // (data, never its title read back); the tooltip says where it is.
    let title: SharedString = place
        .tag
        .as_deref()
        .map_or_else(
            || place.title.say(),
            |tag| skwir_core::metadata::tag_leaf(tag).to_owned(),
        )
        .into();
    let tip: SharedString = words!(
        sidebar_tag_tip = "{ $tag } · Shift+click: only what has it too · Alt+click: in this folder",
        tag = place.title.clone(),
        note = "A tag's tooltip in the sidebar: its name, then what a click with Shift does (shows only what has this tag too) and with Alt (its files in the folder shown)."
    )
    .say()
    .into();
    crate::tags::place_chip(color, !place.suggested, &theme)
        .id(crate::elements::known_by("place", i))
        .max_w_full()
        .border_1()
        .border_color(if is_current || at_cursor {
            theme.accent
        } else {
            gpui::transparent_black()
        })
        .when(is_current, |d| d.bg(theme.tag_shown(color)))
        .text_color(if place.suggested && !is_current {
            theme.text_muted
        } else {
            theme.text
        })
        .cursor_pointer()
        .hover(move |s| s.border_color(tone))
        .on_click(cx.listener({
            let clicks = place.clicks.clone();
            let item = Item::of(place);
            move |this, ev: &gpui::ClickEvent, window, cx| {
                this.sidebar_keys.cursor = Some(item.clone());
                let run = clicked(&clicks, &open, ev.modifiers(), this.pane);
                this.run(&run.name, run.args, window, cx);
            }
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener({
                let (group, key) = (place.group.clone(), place.key());
                move |this, event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_place_menu(&group, &key, event.position, window, cx);
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.run(&tab.name, tab.args.clone(), window, cx);
            }),
        )
        .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
        .child(div().truncate().child(title))
}

/// Pointed at, a place that says tags of what it lists (a tag's pill, a
/// saved search for one: `LocationProvider::tags_shown`) rings the files
/// the pane shows that have them (`Shell::ringed`), so its files stand out
/// where they are.
fn rings_its_files(
    place: gpui::Stateful<gpui::Div>,
    location: &Location,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let location = location.clone();
    place.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
        this.ring_tags_of(&location, *hovered, cx);
    }))
}

/// A place takes what its location takes (ADR-032): a folder's row is a
/// folder's drop target, any other's is its location's (`Verbs::drop`,
/// `takes`: the Trash trashes, a tag gives its tag, the shelf shelves).
/// One that takes nothing (Recent, a search) still swallows a drop, which
/// would otherwise reach the column and be bookmarked; in a dialog's
/// window, what is dropped there is gone to, as anywhere in it (ADR-144,
/// 10). Either way the column is not marked under it
/// (`dnd::marked_alone`): what a drop there does is the place's.
fn takes_drops(
    place: gpui::Stateful<gpui::Div>,
    location: &Location,
    takes: bool,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    if let Some(path) = location.path() {
        return crate::dnd::droppable_dir(place, path, cx);
    }
    if takes {
        return crate::dnd::droppable_place(place, location.clone(), cx);
    }
    crate::dnd::marked_alone(place, cx)
        .on_drop(cx.listener(|this, payload: &DragPayload, window, cx| {
            cx.stop_propagation();
            let paths = this.drag_paths(payload.pane);
            this.dialog_takes_drop(&paths, window, cx);
        }))
        .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
            cx.stop_propagation();
            this.dialog_takes_drop(paths.paths(), window, cx);
        }))
}

/// A pill dragged: onto files, which it gives what it gives (its tag),
/// and, if it moves, among its group's like a movable row, a line on its
/// left or right edge marking where the dragged one lands.
fn movable_pill(
    pill: gpui::Stateful<gpui::Div>,
    label: String,
    drag: PlaceDrag,
    order: &std::sync::Arc<[serde_json::Value]>,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let (moves, index) = (drag.mover(), drag.index);
    let pill = pill.relative().on_drag(drag, move |_, position, _, cx| {
        cx.new(|_| crate::dnd::DragPreview::named(label.clone(), position))
    });
    if let Some(moves) = moves {
        pill.child(gap(order, &moves, index, Edge::Start, cx))
            .child(gap(order, &moves, index + 1, Edge::End, cx))
    } else {
        pill
    }
}

/// Which edge of a movable place a gap's line is drawn on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    Top,
    Bottom,
    /// A pill's start: its left, its right mirrored (`sides`).
    Start,
    /// A pill's end.
    End,
}

/// A movable place's row. It can be dragged, and while a place of its
/// group is dragged over it, a line marks the gap it would land in: the one
/// above the row over its upper half, the one below over its lower half. A
/// line, not a highlighted row, because a highlighted row reads as "into
/// this folder".
fn movable_row(
    row: gpui::Stateful<gpui::Div>,
    label: String,
    drag: PlaceDrag,
    order: &std::sync::Arc<[serde_json::Value]>,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let (moves, index) = (drag.mover().unwrap_or_default(), drag.index);
    row.relative()
        .on_drag(drag, move |_, position, _, cx| {
            cx.new(|_| crate::dnd::DragPreview::named(label.clone(), position))
        })
        .child(gap(order, &moves, index, Edge::Top, cx))
        .child(gap(order, &moves, index + 1, Edge::Bottom, cx))
}

/// Half of a movable place, over it: the drop zone of the gap before the
/// place at `before` among those that move as it does (`moves`: its group
/// and the command that moves them), drawn as a line on `side` (a row's
/// top or bottom, a pill's start or end). Its hitbox does not block the
/// place's own clicks.
fn gap(
    order: &std::sync::Arc<[serde_json::Value]>,
    moves: &(String, String),
    before: usize,
    side: Edge,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let edge = cx.global::<Theme>().accent;
    let order = std::sync::Arc::clone(order);
    let (shown, dropped) = (moves.clone(), moves.clone());
    let zone = div().absolute();
    let zone = match side {
        Edge::Top => zone.left_0().right_0().h(gpui::relative(0.5)).top_0(),
        Edge::Bottom => zone.left_0().right_0().h(gpui::relative(0.5)).bottom_0(),
        Edge::Start => zone
            .top_0()
            .bottom_0()
            .w(gpui::relative(0.5))
            .inset_start(px(0.)),
        Edge::End => zone
            .top_0()
            .bottom_0()
            .w(gpui::relative(0.5))
            .inset_end(px(0.)),
    };
    zone.drag_over::<PlaceDrag>(move |s, drag, _, _| {
        if drag.mover().as_ref() != Some(&shown) || drag.index == before || drag.index + 1 == before
        {
            return s;
        }
        let s = s.border_color(edge);
        match side {
            Edge::Top => s.border_t_2(),
            Edge::Bottom => s.border_b_2(),
            Edge::Start => s.border_start(px(2.)),
            Edge::End => s.border_end(px(2.)),
        }
    })
    .on_drop(cx.listener(move |this, drag: &PlaceDrag, window, cx| {
        // Handled here even where nothing moves: the sidebar's own drop
        // would put the place last. A place of another group, or one moved
        // by another command, stays put.
        cx.stop_propagation();
        if drag.mover().as_ref() != Some(&dropped) {
            return;
        }
        if let Some(before) = drop_target(&order, drag.index, before)
            && let Some(command) = drag.command(before)
        {
            this.run(&command.name, command.args, window, cx);
        }
    }))
}

impl Shell {
    /// Whether the sidebar is drawn: not put away, and the window has room
    /// for it beside the panes (its width is known before layout).
    pub fn sidebar_shown(&self, window: &gpui::Window) -> bool {
        !self.sidebar_hidden
            && self.sidebar_fits(f32::from(crate::title_bar::body(window).size.width))
    }

    /// How wide the sidebar is drawn: as dragged, at the interface's text
    /// size (the width is kept at 100 %, ADR-117), narrower while the
    /// window is too small for that (the remembered width stays as the
    /// user left it), and 0 when it is not drawn.
    pub fn sidebar_drawn_width(&self, window: &gpui::Window) -> f32 {
        if !self.sidebar_shown(window) {
            return 0.;
        }
        crate::sizes::Scale::of(window)
            .f(self.sidebar_width)
            .min(self.sidebar_width_limit(window))
    }

    fn sidebar_width_limit(&self, window: &gpui::Window) -> f32 {
        let preview = self.preview.panel_open() && !self.layout.bottom();
        max_width(
            f32::from(crate::title_bar::body(window).size.width),
            self.panes_across(),
            preview,
        )
    }

    /// The sidebar's edge follows the pointer: the sidebar starts at the
    /// window's start edge (its body's, within the margin of a frame skwir
    /// draws: the left, the right mirrored), so the pointer's distance from
    /// there is its width.
    pub fn on_sidebar_drag(
        &mut self,
        ev: &gpui::DragMoveEvent<SidebarResize>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(crate::sides::from_start_edge(
            crate::title_bar::body(window),
            ev.event.position.x,
        ));
        let drawn = x.clamp(MIN_WIDTH, self.sidebar_width_limit(window));
        self.sidebar_width = crate::sizes::Scale::of(window).at_100(drawn);
        cx.notify();
    }

    /// `ui.toggle_sidebar` (F9): put the sidebar away, or bring it back. A
    /// window too narrow for it says so rather than seeming to ignore the key.
    pub fn toggle_sidebar(&mut self, window: &gpui::Window, cx: &mut Context<Self>) {
        let shown = self.sidebar_shown(window);
        self.sidebar_hidden = shown;
        // Hidden under the pointer, its place is never left.
        if shown {
            self.ringed = None;
        }
        if !shown && !self.sidebar_shown(window) {
            self.set_status(too_narrow(), false, cx);
        }
        self.remember_session();
        cx.notify();
    }

    /// The groups that have places, put away or not.
    fn sidebar_groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = self.places_here().into_iter().map(|p| p.group).collect();
        groups.sort();
        groups.dedup();
        groups
    }

    /// The place of `group` named `key` (`Place::key`), as the user
    /// arranged it.
    pub fn sidebar_place_of(&self, group: &str, key: &str) -> Option<Arranged> {
        self.arranged_places()
            .iter()
            .find(|a| a.place.group == group && a.place.key() == key)
            .cloned()
    }

    /// What renames the place: its owner's command for one kept or
    /// suggested (`Place::rename`: a bookmark, a saved search, a tag on
    /// every file), the sidebar's for one offered, which asks for the
    /// name it shows under.
    pub fn sidebar_rename(&self, arranged: &Arranged) -> Option<Invocation> {
        match arranged.role {
            // A suggested tag is renamed on every file, as a pinned one.
            Role::Kept | Role::Suggested => arranged.place.rename.clone(),
            Role::Listed => None,
            Role::Offered => {
                let place = &arranged.place;
                // Its names as the sidebar says them: the field shows one.
                // Plain: each is a name in other words, isolated there.
                let said = place.title.say_plain();
                let own = self
                    .app
                    .registry
                    .places
                    .find(|p| p.group == place.group && p.key() == place.key())
                    .map_or_else(|| said.clone(), |p| p.title.say_plain());
                // The field holds the name the user gave it, if any: left
                // empty, the place takes back its own (never told by
                // comparing words, ADR-139).
                let given = self
                    .sidebar_sections
                    .places
                    .get(&place.group)
                    .and_then(|a| a.titles.get(&place.key()))
                    .cloned()
                    .unwrap_or_default();
                Some(Invocation::with_args(
                    "ui.ask",
                    &serde_json::json!({
                        "title": words!(
                            sidebar_rename_title = "Rename { $place } in the sidebar",
                            place = named(place),
                            note = "The title of the question renaming a place the sidebar shows (Home, a volume) under a name of the user's, there only."
                        )
                        .to_data(),
                        "accept": words!(
                            sidebar_rename_accept = "Rename",
                            note = "The button that renames a place in the sidebar."
                        )
                        .to_data(),
                        "placeholder": words!(
                            sidebar_rename_placeholder = "Name (empty: { $own })",
                            own = words::Name(own),
                            note = "The empty field of the question renaming a place in the sidebar: left empty, the place takes back its own name, own."
                        )
                        .to_data(),
                        "text": given,
                        "command": PLACE_COMMAND,
                        "args": { "group": place.group, "key": place.key() },
                        "field": "title",
                    }),
                ))
            }
        }
    }

    /// What moves the place `delta` places up (negative) or down among the
    /// shown places of its group that move as it does; `None` at an end,
    /// or for a place that does not move.
    pub fn sidebar_move(&self, arranged: &Arranged, delta: isize) -> Option<Invocation> {
        let moves = arranged.moves.as_ref()?;
        let group = &arranged.place.group;
        let along = mover(group, moves);
        let order: Vec<serde_json::Value> = self
            .arranged_places()
            .iter()
            .filter(|a| a.shown)
            .filter_map(|a| {
                let m = a.moves.as_ref()?;
                (mover(&a.place.group, m) == along).then(|| m.before.clone())
            })
            .collect();
        let at = order.iter().position(|b| *b == moves.before)?;
        let to = at
            .checked_add_signed(delta)
            .filter(|to| *to < order.len())?;
        // Before the place it passes going up, after it going down.
        let before = if delta < 0 {
            order.get(to).cloned()
        } else {
            order.get(to + 1).cloned()
        };
        let drag = PlaceDrag {
            group: group.clone(),
            index: at,
            reorder: Some(moves.clone()),
            gives: None,
        };
        drag.command(before)
    }

    /// `ui.sidebar_place`: arrange a place of the sidebar the user does
    /// not keep there (`Role`), named by `group` and `key`
    /// (`Place::key`). With `hidden` it is hidden or shown again; with
    /// `before` (another's key, or null for last) it moves among the
    /// places offered; with `title` it takes that name (empty: its own).
    /// `group` with `reset` gives the group back its places as their
    /// owners offer them.
    pub fn sidebar_place(&mut self, args: &serde_json::Value, cx: &mut Context<Self>) {
        let text = |k: &str| args.get(k).and_then(serde_json::Value::as_str);
        let Some(group) = text("group").map(str::to_owned) else {
            return;
        };
        if args.get("reset").and_then(serde_json::Value::as_bool) == Some(true) {
            self.sidebar_sections.places.remove(&group);
            self.remember_session();
            cx.notify();
            return;
        }
        let Some(key) = text("key").map(str::to_owned) else {
            return;
        };
        let Some(arranged) = self.sidebar_place_of(&group, &key) else {
            self.set_status(
                words!(
                    sidebar_place_gone = "That place is no longer in the sidebar",
                    note = "Said when a place of the sidebar was arranged (hidden, moved, renamed) as it went from the sidebar."
                ),
                true,
                cx,
            );
            return;
        };
        // A place the user keeps is its owner's to arrange: its menu says
        // how (Rename…, Remove from Bookmarks).
        let kept = words!(
            sidebar_kept_by_owner =
                "{ $place } is kept in the sidebar by its owner: its menu arranges it",
            place = named(&arranged.place),
            note = "Said when a place the user keeps (a bookmark, a saved search) is arranged as the sidebar arranges the places it offers: its own menu renames, moves or removes it. place: its name, quoted, whose gender is not known: where words would agree with it, name it as a place (the place “Docs”)."
        );
        if let Some(hidden) = args.get("hidden").and_then(serde_json::Value::as_bool) {
            if !arranged.role.hides() {
                self.set_status(kept, true, cx);
                return;
            }
            self.sidebar_sections
                .places
                .entry(group.clone())
                .or_default()
                .shown
                .insert(key.clone(), !hidden);
            if hidden && arranged.shown {
                self.toast(
                    crate::toast::Toast::new(
                        words!(
                            sidebar_hid = "Hid { $place } from the sidebar",
                            place = named(&arranged.place),
                            note = "Said after a place was hidden from the sidebar, with Undo. place: its name, quoted, whose gender is not known: where words would agree with it, name it as a place (the place “Docs” hidden)."
                        ),
                        crate::toast::Tone::Info,
                        vec![crate::toast::Action {
                            label: words!(
                                sidebar_hid_undo = "Undo",
                                note = "The button showing again a place just hidden from the sidebar."
                            ),
                            command: PLACE_COMMAND.into(),
                            args: serde_json::json!({
                                "group": group, "key": key, "hidden": false,
                            }),
                            answers: true,
                        }],
                    ),
                    cx,
                );
            }
        } else if let Some(before) = args.get("before") {
            if arranged.role != Role::Offered {
                self.set_status(kept, true, cx);
                return;
            }
            let present: Vec<String> = self
                .arranged_places()
                .iter()
                .filter(|a| a.place.group == group && a.role == Role::Offered)
                .map(|a| a.place.key())
                .collect();
            self.sidebar_sections
                .move_place(&group, &key, before.as_str(), &present);
        } else if let Some(title) = args.get("title") {
            if arranged.role != Role::Offered {
                self.set_status(kept, true, cx);
                return;
            }
            let title = title.as_str().map(str::trim).unwrap_or_default();
            // Left empty, it takes back its own name (the question says
            // so): what was typed is never compared with its words.
            let titles = &mut self
                .sidebar_sections
                .places
                .entry(group.clone())
                .or_default()
                .titles;
            if title.is_empty() {
                titles.remove(&key);
            } else {
                titles.insert(key, title.to_owned());
            }
        } else {
            return;
        }
        if self
            .sidebar_sections
            .places
            .get(&group)
            .is_some_and(Arrangement::is_empty)
        {
            self.sidebar_sections.places.remove(&group);
        }
        self.remember_session();
        cx.notify();
    }

    /// `ui.focus_sidebar`: the keyboard goes to the sidebar, its cursor on
    /// the place shown, or back to the files from there. A sidebar put
    /// away comes back for it. F6 goes round the parts of the window
    /// instead (`Shell::focus_next_part`).
    pub fn focus_sidebar(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if self.sidebar_keys.focus.is_focused(window) {
            window.focus(&self.ui().focus, cx);
            cx.notify();
            return;
        }
        if !self.sidebar_shown(window) {
            if !self.sidebar_fits(f32::from(crate::title_bar::body(window).size.width)) {
                self.set_status(too_narrow(), false, cx);
                return;
            }
            self.sidebar_hidden = false;
            self.remember_session();
        }
        self.sidebar_keys.arrive();
        window.focus(&self.sidebar_keys.focus, cx);
        cx.notify();
    }

    /// The place under the sidebar's cursor, as the user arranged it.
    fn sidebar_cursor_place(&self) -> Option<Arranged> {
        match self.sidebar_keys.cursor.as_ref()? {
            Item::Place { group, key } => self.sidebar_place_of(group, key),
            Item::Group(_) | Item::Tab(_) | Item::NewTab => None,
        }
    }

    /// Put the sidebar's cursor on the item drawn at `at`, in view.
    fn sidebar_cursor_to(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some((item, _)) = self.sidebar_keys.drawn.get(at) {
            self.sidebar_keys.cursor = Some(item.clone());
            self.sidebar_keys.reveal = true;
            cx.notify();
        }
    }

    /// Where the sidebar's cursor is among what is drawn.
    fn sidebar_cursor_at(&self) -> Option<usize> {
        let cursor = self.sidebar_keys.cursor.as_ref()?;
        self.sidebar_keys
            .drawn
            .iter()
            .position(|(item, _)| item == cursor)
    }

    /// The sidebar's keys (`Sidebar` context), `key` naming what to do:
    /// `up`, `down`, `first`, `last` move the cursor; `left` goes to the
    /// group's header and folds it, `right` unfolds it and goes in;
    /// `open` runs the place, shows the tab or opens one (and gives the
    /// keyboard back to the files) or folds the group, `tab` opens the
    /// place in a tab behind; `menu` opens its menu; `move_up` and
    /// `move_down` move it, the tab or its group; `rename` renames it;
    /// `remove` takes out a place the user keeps, hides another, and
    /// closes a tab.
    pub fn sidebar_key(&mut self, key: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let at = self.sidebar_cursor_at();
        let last = self.sidebar_keys.drawn.len().saturating_sub(1);
        let cursor = self.sidebar_keys.cursor.clone();
        match (key, cursor) {
            ("up", _) => self.sidebar_cursor_to(at.map_or(0, |at| at.saturating_sub(1)), cx),
            ("down", _) => self.sidebar_cursor_to(at.map_or(0, |at| (at + 1).min(last)), cx),
            ("first", _) => self.sidebar_cursor_to(0, cx),
            ("last", _) => self.sidebar_cursor_to(last, cx),
            ("left", Some(item @ (Item::Place { .. } | Item::Tab(_) | Item::NewTab))) => {
                self.sidebar_keys.cursor = Some(Item::Group(item.group().to_owned()));
                self.sidebar_keys.reveal = true;
                cx.notify();
            }
            ("left", Some(Item::Group(group))) => {
                self.sidebar_section(&serde_json::json!({ "group": group, "fold": true }), cx);
            }
            ("right", Some(Item::Group(group))) => {
                if self.sidebar_sections.folded.contains(&group) {
                    self.sidebar_section(&serde_json::json!({ "group": group, "fold": false }), cx);
                } else if let Some(at) = at
                    && let Some((next, _)) = self.sidebar_keys.drawn.get(at + 1)
                    && !matches!(next, Item::Group(_))
                    && next.group() == group
                {
                    self.sidebar_cursor_to(at + 1, cx);
                }
            }
            ("open", Some(Item::Group(group))) => {
                self.sidebar_section(&serde_json::json!({ "group": group }), cx);
            }
            ("open", Some(Item::Tab(tab))) => {
                window.focus(&self.ui().focus, cx);
                self.run("tab.select", serde_json::json!({ "tab": tab }), window, cx);
            }
            ("open", Some(Item::NewTab)) => {
                window.focus(&self.ui().focus, cx);
                self.run("tab.new", serde_json::json!({}), window, cx);
            }
            ("open" | "tab", Some(Item::Place { .. })) => {
                let Some(arranged) = self.sidebar_cursor_place() else {
                    return;
                };
                // Nothing again while its owner works on it, as a click.
                if arranged.place.busy.is_some() {
                    return;
                }
                let run = if key == "tab" {
                    open_in_tab(&arranged.place)
                } else {
                    // Where it opens is where the keys go next.
                    window.focus(&self.ui().focus, cx);
                    open_invocation(&arranged.place)
                };
                self.run(&run.name, run.args, window, cx);
            }
            ("menu", Some(item)) => {
                match &item {
                    Item::Group(group) if group == TABS => return,
                    Item::Group(group) => {
                        self.open_sections_menu(Some(group), gpui::Point::default(), window, cx);
                    }
                    Item::Place { group, key } => {
                        self.open_place_menu(group, key, gpui::Point::default(), window, cx);
                    }
                    Item::Tab(tab) => {
                        self.open_tab_menu(*tab, gpui::Point::default(), window, cx);
                    }
                    Item::NewTab => return,
                }
                if let Some(menu) = &mut self.menu {
                    menu.highlighted = Some(0);
                    menu.keyboard = Some(crate::menu::KeyboardAnchor::new(MenuHost::Sidebar));
                }
            }
            ("move_up" | "move_down", Some(item)) => {
                let delta = if key == "move_up" { -1 } else { 1 };
                let run = match &item {
                    Item::Group(group) if group == TABS => None,
                    Item::Group(group) => self.group_move(group, delta),
                    Item::Place { .. } => self
                        .sidebar_cursor_place()
                        .and_then(|a| self.sidebar_move(&a, delta)),
                    Item::Tab(tab) => self.tab_move(*tab, delta),
                    Item::NewTab => None,
                };
                if let Some(run) = run {
                    self.sidebar_keys.reveal = true;
                    self.run(&run.name, run.args, window, cx);
                }
            }
            ("rename", Some(Item::Place { .. })) => {
                if let Some(run) = self
                    .sidebar_cursor_place()
                    .and_then(|a| self.sidebar_rename(&a))
                {
                    self.run(&run.name, run.args, window, cx);
                }
            }
            ("remove", Some(Item::Tab(tab))) => {
                // The cursor stays where the tab was: on the next one, or
                // the one before the last.
                let labels = &self.tab_labels.labels;
                let next = labels.iter().position(|l| l.tab == tab).and_then(|at| {
                    labels
                        .get(at + 1)
                        .or_else(|| at.checked_sub(1).and_then(|before| labels.get(before)))
                });
                self.sidebar_keys.cursor = next.map(|l| Item::Tab(l.tab));
                self.sidebar_keys.reveal = true;
                self.run("tab.close", serde_json::json!({ "tab": tab }), window, cx);
            }
            ("remove", Some(Item::Place { group, key })) => {
                let Some(arranged) = self.sidebar_cursor_place() else {
                    return;
                };
                let run = if arranged.role.hides() {
                    Some(Invocation::with_args(
                        PLACE_COMMAND,
                        &serde_json::json!({ "group": group, "key": key, "hidden": true }),
                    ))
                } else {
                    arranged.place.forget.clone()
                };
                if let Some(run) = run {
                    // The cursor stays where the place was: on the next one.
                    if let Some(at) = at {
                        let next = if at < last {
                            at + 1
                        } else {
                            at.saturating_sub(1)
                        };
                        self.sidebar_cursor_to(next, cx);
                    }
                    self.run(&run.name, run.args, window, cx);
                }
            }
            _ => {}
        }
    }

    /// What moves tab `tab` `delta` places along the tabs; `None` at an end.
    fn tab_move(&self, tab: TabId, delta: isize) -> Option<Invocation> {
        let labels = &self.tab_labels.labels;
        let at = labels.iter().position(|l| l.tab == tab)?;
        let to = at
            .checked_add_signed(delta)
            .filter(|to| *to < labels.len())?;
        Some(Invocation::with_args(
            "tab.move",
            &serde_json::json!({ "tab": tab, "to": to }),
        ))
    }

    /// What moves `group` `delta` places among the groups shown.
    fn group_move(&self, group: &str, delta: isize) -> Option<Invocation> {
        let sections = &self.sidebar_sections;
        let shown: Vec<String> = sections
            .arranged(&self.sidebar_groups())
            .into_iter()
            .filter(|g| !sections.hidden.contains(g))
            .collect();
        let at = shown.iter().position(|g| g == group)?;
        let to = at
            .checked_add_signed(delta)
            .filter(|to| *to < shown.len())?;
        let before = if delta < 0 {
            shown.get(to)
        } else {
            shown.get(to + 1)
        };
        Some(move_group(group, before.map(String::as_str)))
    }

    /// `ui.sidebar_section`: arrange the sidebar's groups. `group` alone
    /// folds or unfolds it (`fold` says which); with `hidden` it is put
    /// away or shown; with `before` (a group, or null for last) it moves.
    /// `reset` shows every group in the order the sidebar ships with.
    pub fn sidebar_section(&mut self, args: &serde_json::Value, cx: &mut Context<Self>) {
        let flag = |k: &str| args.get(k).and_then(serde_json::Value::as_bool);
        let sections = &mut self.sidebar_sections;
        if flag("reset") == Some(true) {
            sections.order.clear();
            sections.hidden.clear();
        } else if let Some(group) = args.get("group").and_then(serde_json::Value::as_str) {
            let group = group.to_owned();
            if let Some(before) = args.get("before") {
                let present = self.sidebar_groups();
                self.sidebar_sections
                    .move_before(&group, before.as_str(), &present);
            } else if let Some(hidden) = flag("hidden") {
                if hidden {
                    sections.hidden.insert(group);
                } else {
                    sections.hidden.remove(&group);
                }
            } else {
                let fold = flag("fold").unwrap_or(!sections.folded.contains(&group));
                if fold {
                    sections.folded.insert(group);
                } else {
                    sections.folded.remove(&group);
                }
            }
        } else {
            return;
        }
        self.remember_session();
        cx.notify();
    }

    /// The menu of the sidebar's groups: for `group`'s header, folding,
    /// moving and hiding it; everywhere, every group with a check while
    /// it is shown, to show or hide it, as the list header's menu does
    /// for columns.
    pub fn open_sections_menu(
        &mut self,
        group: Option<&str>,
        position: gpui::Point<gpui::Pixels>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let item = |title: Words, args: serde_json::Value, order: i32, icon: Option<&str>| {
            skwir_core::registry::MenuItem {
                menu: "sidebar".into(),
                title,
                command: Invocation::with_args("ui.sidebar_section", &args),
                when: None,
                order,
                icon: icon.map(str::to_owned),
                submenu: None,
                checked: None,
                choice: None,
                color: None,
            }
        };
        let sections = &self.sidebar_sections;
        let mut present = self.sidebar_groups();
        for hidden in &sections.hidden {
            if !present.contains(hidden) {
                present.push(hidden.clone());
            }
        }
        let all = sections.arranged(&present);
        let shown: Vec<&String> = all
            .iter()
            .filter(|g| !sections.hidden.contains(*g))
            .collect();
        let mut items = Vec::new();
        if let Some(group) = group {
            let folded = sections.folded.contains(group);
            items.push(item(
                if folded {
                    words!(
                        sidebar_unfold = "Unfold",
                        note = "An item of a sidebar section's menu: show the places the folded section holds."
                    )
                } else {
                    words!(
                        sidebar_fold = "Fold",
                        note = "An item of a sidebar section's menu: hide the places the section holds, its heading left."
                    )
                },
                serde_json::json!({ "group": group, "fold": !folded }),
                0,
                Some(if folded { "unfold" } else { "fold" }),
            ));
            if let Some(at) = shown.iter().position(|g| *g == group) {
                if at > 0 {
                    items.push(item(
                        words!(
                            sidebar_move_up = "Move Up",
                            note = "An item of a sidebar section's menu: put the section before the one above it."
                        ),
                        serde_json::json!({ "group": group, "before": shown[at - 1] }),
                        240,
                        Some("up"),
                    ));
                }
                if at + 1 < shown.len() {
                    items.push(item(
                        words!(
                            sidebar_move_down = "Move Down",
                            note = "An item of a sidebar section's menu: put the section after the one below it."
                        ),
                        serde_json::json!({ "group": group, "before": shown.get(at + 2) }),
                        241,
                        Some("down"),
                    ));
                }
            }
            // Its places the user can hide, checked while shown: those
            // hidden come back from here.
            let place_item = |title: Words, args: serde_json::Value, order, icon: Option<&str>| {
                skwir_core::registry::MenuItem {
                    command: Invocation::with_args(PLACE_COMMAND, &args),
                    ..item(title, serde_json::Value::Null, order, icon)
                }
            };
            let places = arrange(self.places_here(), sections);
            for (i, arranged) in places
                .iter()
                .filter(|a| a.place.group == group && a.role.hides())
                .enumerate()
            {
                items.push(
                    place_item(
                        arranged.place.title.clone(),
                        serde_json::json!({
                            "group": group,
                            "key": arranged.place.key(),
                            "hidden": arranged.shown,
                        }),
                        // What the group shows (4xx, ADR-099), in one group
                        // of the menu however many there are.
                        400 + i32::try_from(i.min(99)).unwrap_or_default(),
                        None,
                    )
                    .checked(arranged.shown),
                );
                // A tag's place is said as its chip, as the sidebar draws it.
                if let (Some(item), Some(color)) = (items.last_mut(), &arranged.place.color) {
                    item.color = Some(color.clone());
                }
            }
            if sections.places.get(group).is_some_and(|a| !a.is_empty()) {
                items.push(place_item(
                    words!(
                        sidebar_reset_section = "Reset { $section }",
                        section = group_title(group),
                        note = "An item of a sidebar section's menu: give the section back its places as their owners offer them, none hidden, moved or renamed; section: the section's heading (Places, Devices)."
                    ),
                    serde_json::json!({ "group": group, "reset": true }),
                    290,
                    Some("revert"),
                ));
            }
        }
        // What the group's owners add to it (Add Server… in Network, Clear
        // Recent Folders), registered for its menu (`MenuItem::menu`,
        // `sidebar:<group>`): each before the first of the sidebar's own
        // that comes after it by its order, so that what adds goes first
        // and what empties beside what moves the section (ADR-099).
        if let Some(group) = group {
            let menu = format!("sidebar:{group}");
            let mut own: Vec<_> = self
                .app
                .registry
                .menus
                .all()
                .into_iter()
                .filter(|item| item.menu == menu)
                .collect();
            own.sort_by_key(|item| item.order);
            for item in own {
                let at = items
                    .iter()
                    .position(|mine| mine.order > item.order)
                    .unwrap_or(items.len());
                items.insert(at, item);
            }
        }
        // Every group, checked while shown.
        for (i, g) in all.iter().enumerate() {
            let hidden = sections.hidden.contains(g);
            // The sidebar's sections, after what the group shows: a group
            // of the menu's own (6xx).
            items.push(
                item(
                    group_title(g),
                    serde_json::json!({ "group": g, "hidden": !hidden }),
                    600 + i32::try_from(i).unwrap_or_default(),
                    None,
                )
                .checked(!hidden),
            );
        }
        if sections.arranged_by_user() {
            items.push(item(
                words!(
                    sidebar_reset_sections = "Reset Sections",
                    note = "An item of the sidebar sections' menu: every section back where it was, unfolded and shown."
                ),
                serde_json::json!({ "reset": true }),
                690,
                Some("revert"),
            ));
        }
        self.open_menu(position, items, window, cx);
    }

    /// What is dropped beside the rows: in a dialog's window, gone to
    /// (ADR-144, 10), never bookmarked; elsewhere each folder bookmarked.
    fn dropped_beside_places(
        &mut self,
        paths: &[std::path::PathBuf],
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog_takes_drop(paths, window, cx) {
            return;
        }
        for path in paths {
            self.bookmark(path, window, cx);
        }
    }

    /// Bookmark a folder dropped beside the rows. The command checks that
    /// it is one, on a worker: asking could block on a sleeping disk.
    fn bookmark(
        &mut self,
        path: &std::path::Path,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.run(
            "places.bookmark",
            serde_json::json!({ "path": PathArg::from(path) }),
            window,
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{Role, Sections, arrange, drop_target, group_title, used_fraction};
    use skwir_core::Location;
    use skwir_core::words::Words;
    use std::path::PathBuf;

    #[test]
    fn the_sidebar_may_take_what_the_panes_and_the_preview_do_not_need() {
        // One pane keeps 240 px; the rest of a 1280 px window may go.
        assert!((super::max_width(1280., 1, false) - 1040.).abs() < 1e-3);
        // The preview at the right keeps its own minimum beside it.
        assert!((super::max_width(1280., 2, true) - 600.).abs() < 1e-3);
        // A window too small still draws the sidebar at its narrowest.
        assert!((super::max_width(400., 2, true) - super::MIN_WIDTH).abs() < 1e-3);
    }

    #[test]
    fn a_dropped_place_lands_in_the_gap_the_line_marks() {
        let order: Vec<PathBuf> = ["/a", "/b", "/c"].map(PathBuf::from).into();
        let before = |p: &str| Some(Some(PathBuf::from(p)));
        assert_eq!(drop_target(&order, 0, 2), before("/c"), "down, below /b");
        assert_eq!(drop_target(&order, 0, 3), Some(None), "last");
        assert_eq!(drop_target(&order, 2, 0), before("/a"), "up, first");
        assert_eq!(drop_target(&order, 2, 1), before("/b"));
        // The gaps on either side of it leave it where it is.
        assert_eq!(drop_target(&order, 1, 1), None);
        assert_eq!(drop_target(&order, 1, 2), None);
    }

    #[test]
    fn groups_go_where_the_user_put_them_and_new_ones_after() {
        let owned = |v: &[&str]| v.iter().map(|g| (*g).to_owned()).collect::<Vec<_>>();
        let present = owned(&["devices", "places", "tags", "bookmarks"]);
        let mut sections = Sections::default();
        assert_eq!(
            sections.arranged(&present),
            owned(&["places", "bookmarks", "tags", "devices"]),
            "as the sidebar ships"
        );
        sections.move_before("tags", Some("places"), &present);
        assert_eq!(
            sections.arranged(&present),
            owned(&["tags", "places", "bookmarks", "devices"])
        );
        sections.move_before("places", None, &present);
        assert_eq!(
            sections.arranged(&present),
            owned(&["tags", "bookmarks", "devices", "places"])
        );
        // Searches had no places when the user moved the others: it comes
        // back where it was among them, and a group never seen goes last.
        let later = owned(&["places", "searches", "tags", "plugin", "bookmarks"]);
        assert_eq!(
            sections.arranged(&later),
            owned(&["tags", "bookmarks", "searches", "places", "plugin"])
        );
        assert!(sections.arranged_by_user());
        let toml = toml::Value::try_from(&sections).unwrap();
        assert_eq!(toml.try_into::<Sections>().unwrap(), sections);
    }

    /// A server's place opening a folder below its mount names that
    /// folder; the mount's point, which no place opens, is named by its
    /// remote, not by the folder Skwir mounted it in, even with its place
    /// put away.
    #[test]
    fn a_remote_s_mount_point_is_a_root_under_its_name() {
        use skwir_core::fs::reach::Known;
        let known = |point: &str, label: &str| Known {
            point: PathBuf::from(point),
            address: None,
            label: Words::as_written(label.to_owned()),
            thumbnails: skwir_core::fs::reach::Thumbnails::default(),
            forget: None,
            trash: None,
            looked_at: None,
        };
        let mut photos = place("network", "/run/m/nas/srv/photos");
        photos.title = Words::as_written("Photos");
        // Its place in a group put away.
        let mut hidden = place("devices", "/run/m/other");
        hidden.title = Words::as_written("Other");
        let mut sections = Sections::default();
        sections.hidden.insert("devices".to_owned());
        let roots = super::roots(
            vec![photos, hidden],
            vec![
                known("/run/m/nas", "me@nas"),
                known("/run/m/other", "other host"),
            ],
            &sections,
            None,
        );
        let named: Vec<(String, String)> = roots
            .iter()
            .map(|root| (root.path.display().to_string(), root.name.to_string()))
            .collect();
        assert_eq!(
            named,
            [
                ("/run/m/nas/srv/photos".to_owned(), "Photos".to_owned()),
                ("/run/m/nas".to_owned(), "me@nas".to_owned()),
                ("/run/m/other".to_owned(), "other host".to_owned()),
            ]
        );
    }

    fn place(group: &str, location: &str) -> skwir_core::registry::Place {
        skwir_core::registry::Place::new(
            group,
            skwir_core::words::Words::as_written(location.trim_start_matches('/').to_owned()),
            "",
            Location::parse(location),
        )
    }

    /// A place's title is named in a sentence as the words it is where it
    /// is a message, one come back from its data too (a plugin's, a place
    /// across the window's JSON), else as a name said plainly (UB-7).
    #[test]
    fn a_place_is_named_as_its_words_or_as_a_name() {
        use skwir_core::words::Arg;
        let home = skwir_core::words!(test_sidebar_home = "Home", noun);
        let mut named = place("places", "/x");
        named.title = home.clone();
        assert!(matches!(super::named(&named), Arg::Words(_)));
        named.title = serde_json::from_value(home.to_data()).unwrap();
        assert!(
            matches!(super::named(&named), Arg::Words(_)),
            "a message carried as data is still one"
        );
        let label = place("devices", "/Holiday photos");
        assert_eq!(
            super::named(&label),
            Arg::Name("Holiday photos".into()),
            "said plainly, no isolation of its own"
        );
    }

    /// A group the sidebar ships with that the user's order does not
    /// name (Recent Folders, new since they moved the others) comes where
    /// it ships, first, not after the groups they left.
    #[test]
    fn a_new_group_comes_where_it_ships_among_those_the_user_moved() {
        let owned = |v: &[&str]| v.iter().map(|g| (*g).to_owned()).collect::<Vec<_>>();
        let present = owned(&["places", "tags", "devices"]);
        let mut sections = Sections::default();
        sections.move_before("tags", Some("places"), &present);
        sections
            .order
            .retain(|g| g != skwir_core::registry::groups::RECENT_FOLDERS.id);
        let dialog = owned(&["devices", "recent_folders", "places", "tags", "plugin"]);
        assert_eq!(
            sections.arranged(&dialog),
            owned(&["recent_folders", "tags", "places", "devices", "plugin"])
        );
        sections.move_before("recent_folders", None, &dialog);
        assert_eq!(
            sections.arranged(&dialog),
            owned(&["tags", "places", "devices", "plugin", "recent_folders"]),
            "moved, it stays where the user put it"
        );
    }

    /// Folders shown under one name are told apart by the first folder
    /// above each that differs, a parent alone where it is enough, up to
    /// home or the root; where two of those read the same, by the folders
    /// above as far as that one. A path two places share tells nothing.
    #[test]
    fn folders_of_one_name_are_told_apart_by_the_folder_above_that_differs() {
        use std::path::Path;
        let home = Path::new("/home/someone");
        let told = |paths: &[&str]| {
            let paths: Vec<&Path> = paths.iter().map(Path::new).collect();
            super::told_apart(&paths, Some(home))
        };
        let some = |note: &str| Some(note.to_owned());
        let sep = std::path::MAIN_SEPARATOR_STR;
        let at = |words: &[&str]| words.join(sep);
        assert_eq!(
            told(&[
                "/home/someone/wip/skwir/skwir/crates/core/src",
                "/home/someone/wip/skwir/skwir-site/src",
            ]),
            [some("core"), some("skwir-site")],
            "their parents differ"
        );
        assert_eq!(
            told(&[
                "/home/someone/wip/skwir/src",
                "/home/someone/old/skwir/src",
                "/home/someone/src",
            ]),
            [
                some(&at(&["wip", "…"])),
                some(&at(&["old", "…"])),
                some("~")
            ],
            "a parent alike, the folder above it"
        );
        // The root, above all (a Windows root says its own separator).
        #[cfg(unix)]
        assert_eq!(told(&["/a/src", "/b/a/src"]), [some("/…"), some("b/…")]);
        assert_eq!(
            told(&[
                "/home/someone/m/a/src",
                "/home/someone/m/b/src",
                "/home/someone/n/a/src",
                "/home/someone/n/b/src",
            ]),
            [
                some(&at(&["m", "a"])),
                some(&at(&["m", "b"])),
                some(&at(&["n", "a"])),
                some(&at(&["n", "b"])),
            ],
            "the folders above as far as the one that differs"
        );
        assert_eq!(
            told(&["/home/someone/src", "/home/someone/src"]),
            [None, None],
            "one folder twice"
        );
    }

    /// Two places of a group shown under one name, both folders, are told
    /// apart; a place alone under its name, or one of another group, is
    /// not. Every folder's place says where it leads, from home.
    #[test]
    fn places_of_one_name_in_a_group_are_told_apart() {
        let home = skwir_core::os_path::home().unwrap_or_else(|| PathBuf::from("/home/someone"));
        let at = |rest: &str| home.join(rest).to_string_lossy().into_owned();
        let named = |group: &str, path: &str| {
            skwir_core::registry::Place::new(
                group,
                Words::as_written("src".to_owned()),
                "",
                Location::from_path(PathBuf::from(path)),
            )
        };
        let arranged = arrange(
            vec![
                named("bookmarks", &at("wip/one/src")),
                named("bookmarks", &at("wip/two/src")),
                named("places", &at("three/src")),
                place("bookmarks", "/elsewhere"),
            ],
            &Sections::default(),
        );
        let notes: Vec<Option<&str>> = arranged.iter().map(|a| a.note.as_deref()).collect();
        assert_eq!(notes, [None, Some("one"), Some("two"), None]);
        let sep = std::path::MAIN_SEPARATOR_STR;
        assert_eq!(
            arranged[1].path.as_deref(),
            Some(["~", "wip", "one", "src"].join(sep).as_str())
        );
        assert_eq!(arranged[3].path.as_deref(), Some("/elsewhere"));
    }

    /// What a section of the shell's own lists from what was used leaves
    /// out what another place the sidebar shows leads to (in a section
    /// folded too), a place hidden or a section put away showing nothing,
    /// and lists each once, `limit` at most.
    #[test]
    fn what_another_place_shows_is_not_listed_again() {
        let offered = vec![
            place("places", "/home/a/Downloads"),
            {
                let mut hidden = place("places", "/home/a/Templates");
                hidden.hidden = true;
                hidden
            },
            place("devices", "/media/stick"),
        ];
        let recent = |paths: &[&str]| -> Vec<skwir_core::registry::Place> {
            paths
                .iter()
                .map(|path| place("recent_folders", path))
                .collect()
        };
        let mut sections = Sections::default();
        sections.folded.insert("places".to_owned());
        let left = super::shown_nowhere(
            &offered,
            recent(&[
                "/home/a/Downloads",
                "/home/a/Templates",
                "/srv/x",
                "/srv/x",
                "/srv/y",
            ]),
            &sections,
            2,
        );
        assert_eq!(titles_of(&left), ["home/a/Templates", "srv/x"]);
        sections.hidden.insert("devices".to_owned());
        let left = super::shown_nowhere(&offered, recent(&["/media/stick"]), &sections, 5);
        assert_eq!(titles_of(&left), ["media/stick"], "its section put away");
    }

    /// A place listed from what is in use (a tab's folder in a dialog) is
    /// shown, never hidden, moved, renamed or taken out.
    #[test]
    fn a_listed_place_is_shown_and_never_arranged() {
        let mut tab = place("skwir_tabs", "/home/a/wip");
        tab.listed = true;
        let mut sections = Sections::default();
        sections
            .places
            .entry("skwir_tabs".to_owned())
            .or_default()
            .shown
            .insert(tab.key(), false);
        let arranged = arrange(vec![tab], &sections);
        assert_eq!(arranged[0].role, Role::Listed);
        assert!(arranged[0].shown, "no hiding it");
        assert!(arranged[0].moves.is_none());
        assert!(!Role::Listed.hides());
    }

    /// Skwir Tabs keep the order the tabs were used in (RS-3): a tab
    /// showing a tag, a search or Recent comes where its use puts it, not
    /// after the folders, nor first as Recent is in a dialog opening files.
    #[test]
    fn listed_places_keep_the_order_they_were_listed_in() {
        let listed = |location: &str| {
            let mut tab = place("skwir_tabs", location);
            tab.listed = true;
            tab
        };
        let mut arranged = arrange(
            vec![
                listed("tags:/work"),
                listed("/home/a/src"),
                listed("recent:"),
                listed("/home/a/docs"),
            ],
            &Sections::default(),
        );
        let order = ["tags:/work", "home/a/src", "recent:", "home/a/docs"];
        assert_eq!(titles(&arranged), order);
        super::recent_first(&mut arranged, |place| &*place.location.scheme == "recent");
        assert_eq!(
            titles(&arranged),
            order,
            "Recent leads only where it is offered"
        );
    }

    fn titles_of(places: &[skwir_core::registry::Place]) -> Vec<String> {
        places.iter().map(|p| p.title.english()).collect()
    }

    fn titles(arranged: &[super::Arranged]) -> Vec<String> {
        arranged
            .iter()
            .filter(|a| a.shown)
            .map(|a| a.place.title.english())
            .collect()
    }

    /// What is no folder comes after the folders of its group unless it
    /// moves: a remote bookmark stays where its owner keeps it, among the
    /// folders', and a server the tools know goes after the mounts.
    #[test]
    fn a_place_that_moves_keeps_its_owners_order() {
        let moving = |location: &str| {
            let mut p = place("bookmarks", location);
            p.forget = Some(skwir_core::command::Invocation::new("places.unbookmark"));
            p.reorder = Some(skwir_core::registry::Reorder {
                command: skwir_core::command::Invocation::new("places.move_bookmark"),
                before: serde_json::json!(location),
            });
            p
        };
        let all = vec![
            moving("/a"),
            moving("sftp://me@example.com/docs"),
            moving("/c"),
            place("network", "sftp://server.example.com"),
            place("network", "/run/user/1000/gvfs/sftp:host=nas"),
        ];
        assert_eq!(
            titles(&arrange(all, &Sections::default())),
            [
                "a",
                "sftp://me@example.com/docs",
                "c",
                "run/user/1000/gvfs/sftp:host=nas",
                "sftp://server.example.com",
            ]
        );
    }

    /// The places offered are the user's to put in order, rename and hide;
    /// a bookmark stays where its owner keeps it, and a volume is the same
    /// place by its key whether it is mounted or not.
    #[test]
    fn offered_places_go_where_the_user_put_them_under_their_names() {
        let mut bookmark = place("places", "/bookmarked");
        bookmark.forget = Some(skwir_core::command::Invocation::new("places.unbookmark"));
        let recent = place("places", "recent:");
        let mut efi = place("devices", "/dev/sda1");
        efi.title = Words::as_written("EFI");
        efi.key = Some("volume:1234".into());
        efi.hidden = true;
        let all = vec![
            recent,
            place("places", "/home"),
            bookmark,
            place("places", "/docs"),
            efi.clone(),
        ];
        let mut sections = Sections::default();
        assert_eq!(
            titles(&arrange(all.clone(), &sections)),
            ["home", "bookmarked", "docs", "recent:"],
            "folders first, the EFI partition offered hidden"
        );
        let roles: Vec<Role> = arrange(all.clone(), &sections)
            .iter()
            .map(|a| a.role)
            .collect();
        assert_eq!(roles[1], Role::Kept);
        assert_eq!(roles[0], Role::Offered);
        // Recent moves before Home; the bookmark keeps its slot.
        let present = ["/home", "/docs", "recent:"].map(str::to_owned);
        sections.move_place("places", "recent:", Some("/home"), &present);
        assert_eq!(
            titles(&arrange(all.clone(), &sections)),
            ["recent:", "bookmarked", "home", "docs"]
        );
        // Last, and a place the order does not name goes after it.
        sections.move_place("places", "/home", None, &present);
        let mut later = all.clone();
        later.push(place("places", "/new"));
        assert_eq!(
            titles(&arrange(later, &sections)),
            ["recent:", "bookmarked", "docs", "home", "new"]
        );
        // Renamed and hidden by key: the volume mounted elsewhere is the
        // same place; shown, it stays shown.
        let arrangement = sections.places.entry("places".into()).or_default();
        arrangement.titles.insert("/docs".into(), "Papers".into());
        arrangement.shown.insert("/home".into(), false);
        sections
            .places
            .entry("devices".into())
            .or_default()
            .shown
            .insert("volume:1234".into(), true);
        efi.location = Location::parse("/run/media/example/EFI");
        let mut mounted = all;
        mounted.pop();
        mounted.push(efi);
        let arranged = arrange(mounted, &sections);
        assert_eq!(
            titles(&arranged),
            ["recent:", "bookmarked", "Papers", "EFI"]
        );
        // The sidebar moves what it offers; a bookmark moves by its owner.
        let moves = |title: &str| {
            arranged
                .iter()
                .find(|a| a.place.title.english() == title)
                .and_then(|a| a.moves.as_ref())
                .map(|m| m.command.name.to_string())
        };
        assert_eq!(moves("Papers").as_deref(), Some("ui.sidebar_place"));
        assert_eq!(moves("bookmarked"), None);
        let toml = toml::Value::try_from(&sections).unwrap();
        assert_eq!(toml.try_into::<Sections>().unwrap(), sections);
    }

    /// A middle click on a volume to mount or unlock runs that command
    /// with `tab`, keeping its arguments; any other place opens a tab
    /// behind the one shown.
    #[test]
    fn a_volume_to_mount_opens_in_a_tab_once_mounted() {
        let place = |location: &str| {
            skwir_core::registry::Place::new(
                "devices",
                Words::as_written("Backup"),
                "",
                Location::parse(location),
            )
        };
        let mut locked = place("device:/dev/sdz");
        locked.activate = Some(skwir_core::command::Invocation::with_args(
            "places.open_locked",
            &serde_json::json!({ "device": "/dev/sdz", "title": "Backup" }),
        ));
        let run = super::open_in_tab(&locked);
        assert_eq!(&*run.name, "places.open_locked");
        assert_eq!(
            run.args,
            serde_json::json!({ "device": "/dev/sdz", "title": "Backup", "tab": true })
        );
        assert_eq!(
            super::open_invocation(&locked).args.get("tab"),
            None,
            "a click opens it in the pane"
        );
        let mut plain = place("device:/dev/sdy1");
        plain.activate = Some(skwir_core::command::Invocation::new("places.mount"));
        assert_eq!(
            super::open_in_tab(&plain).args,
            serde_json::json!({ "tab": true })
        );
        let run = super::open_in_tab(&place("/run/media/example/Backup"));
        assert_eq!(&*run.name, "tab.new");
        assert_eq!(run.args["show"], serde_json::json!(false));
    }

    /// Listed in the sidebar, the tabs come first: their header, each tab
    /// and New tab, then the groups of places; folded, the header alone,
    /// and what the keys walk follows.
    #[test]
    fn the_tabs_come_before_the_places_and_fold_as_a_group() {
        use super::{Item, TABS, drawn_items};
        use skwir_core::workspace::TabId;
        let arranged = arrange(
            vec![place("places", "/home"), place("devices", "/dev/sda1")],
            &Sections::default(),
        );
        let places: Vec<&super::Arranged> = arranged.iter().collect();
        let tabs = [TabId(1), TabId(2)];
        let group = |g: &str| Item::Group(g.to_owned());
        let at = |g: &str, key: &str| Item::Place {
            group: g.to_owned(),
            key: key.to_owned(),
        };
        let home = at("places", &arranged[0].place.key());
        let disk = at("devices", &arranged[1].place.key());
        assert_eq!(
            drawn_items(&places, &Sections::default(), &tabs),
            [
                group(TABS),
                Item::Tab(TabId(1)),
                Item::Tab(TabId(2)),
                Item::NewTab,
                group("places"),
                home.clone(),
                group("devices"),
                disk.clone(),
            ]
        );
        let mut folded = Sections::default();
        folded.folded.insert(TABS.to_owned());
        assert_eq!(
            drawn_items(&places, &folded, &tabs),
            [
                group(TABS),
                group("places"),
                home.clone(),
                group("devices"),
                disk.clone(),
            ]
        );
        // Not listed there: the places alone.
        assert_eq!(
            drawn_items(&places, &Sections::default(), &[]),
            [group("places"), home, group("devices"), disk]
        );
        assert_eq!(group_title(TABS).english(), "Tabs");
    }

    #[test]
    fn usage_is_the_share_that_is_not_free() {
        assert!((used_fraction(25, 100) - 0.75).abs() < f32::EPSILON);
        assert!(used_fraction(0, 0).abs() < f32::EPSILON);
        assert!(
            (used_fraction(200, 100)).abs() < f32::EPSILON,
            "never negative"
        );
        assert_eq!(group_title("devices").english(), "Devices");
        assert_eq!(group_title("searches").english(), "Saved");
    }
}
