//! Sidebar: places from the registry, grouped, click to navigate.
//!
//! Rows use the desktop theme's icons, volumes show how full they are, and
//! removable ones can be detached. A row accepts dropped files like any
//! folder; dropping a folder beside the rows bookmarks it. A right-click
//! opens the place's own menu (`Place::menu`: rename or remove a bookmark
//! or a saved search), and a place its owner lets move (`Place::reorder`:
//! bookmarks, saved searches) is put in order by dragging it among the
//! others of its group. A place with a color (a tag's) is a pill of it,
//! the pills of a group wrapping together, and a place with a `drop`
//! command takes files dropped on it (a tag gives them itself).
//!
//! The groups are the user's to arrange (`Sections`, remembered with the
//! window's shape): a click on a group's header folds it, dragging the
//! header moves the group, and its right-click menu hides it, or shows
//! one hidden. So are the places their owners offer (Home, the Trash, a
//! volume): moved, renamed and hidden here (`arrange`, `ui.sidebar_place`),
//! where a place the user keeps is its owner's to arrange (`Role`). F6
//! gives the sidebar the keyboard (`Keys`, `Shell::sidebar_key`).

use std::collections::{BTreeMap, HashMap};

use gpui::{AnyElement, Context, ExternalPaths, MouseButton, SharedString, div, prelude::*, px};

use bibli_core::Location;
use bibli_core::command::Invocation;
use bibli_core::os_path::PathArg;
use bibli_core::registry::{Place, Reorder};

use crate::controls;
use crate::dnd::DragPayload;
use crate::format;
use crate::menu::MenuHost;
use crate::shell::Shell;
use crate::theme::Theme;

const GROUP_ORDER: [&str; 5] = ["places", "bookmarks", "searches", "tags", "devices"];
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
    /// empty until then. A group it does not name goes after them, in
    /// the order the sidebar ships with.
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
}

impl Role {
    pub fn of(place: &Place) -> Self {
        if place.forget.is_some() || place.reorder.is_some() {
            Role::Kept
        } else if place.suggested {
            Role::Suggested
        } else {
            Role::Offered
        }
    }

    /// Whether the user can hide it: a place kept is taken out instead.
    pub fn hides(self) -> bool {
        self != Role::Kept
    }
}

/// The command that arranges a place the user does not keep.
const PLACE_COMMAND: &str = "ui.sidebar_place";

/// What the sidebar's keyboard cursor is on: a group's header, or a place
/// (by its group and `Place::key`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Group(String),
    Place { group: String, key: String },
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
        }
    }
}

/// The sidebar's keyboard (F6, `ui.focus_sidebar`): its focus, the
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
}

/// What is drawn, in order: each group's header, then its shown places
/// unless it is folded.
fn drawn_items(places: &[Arranged], sections: &Sections) -> Vec<Item> {
    let mut items = Vec::new();
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
}

/// Every place, shown or not, as the user arranged them: the groups in
/// their order; within a group, folders before virtual places (Shelf,
/// Recent) whatever order their plugins published them in, then the
/// offered places in the user's order among the slots they hold.
pub fn arrange(mut places: Vec<Place>, sections: &Sections) -> Vec<Arranged> {
    // By name too, so that two groups the sidebar does not know never mix.
    places.sort_by(|a, b| {
        (sections.rank(&a.group), &a.group, !a.location.is_path()).cmp(&(
            sections.rank(&b.group),
            &b.group,
            !b.location.is_path(),
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
    slotted
        .into_iter()
        .flatten()
        .map(|mut place| {
            let role = Role::of(&place);
            let key = place.key();
            let arrangement = sections.places.get(&place.group);
            let shown = !role.hides()
                || arrangement
                    .and_then(|a| a.shown.get(&key))
                    .copied()
                    .unwrap_or(!place.hidden);
            let mut moves = place.reorder.clone();
            if role == Role::Offered {
                if let Some(title) = arrangement.and_then(|a| a.titles.get(&key)) {
                    place.title.clone_from(title);
                }
                moves = Some(Reorder {
                    command: Invocation::with_args(
                        PLACE_COMMAND,
                        &serde_json::json!({ "group": place.group, "key": key, "before": null }),
                    ),
                    before: serde_json::json!(key),
                });
            }
            Arranged {
                place,
                role,
                shown,
                moves,
            }
        })
        .collect()
}

impl Sections {
    /// Where `group` goes among the others.
    pub fn rank(&self, group: &str) -> (usize, usize) {
        let placed = self.order.iter().position(|g| g == group);
        (placed.unwrap_or(self.order.len()), group_rank(group))
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

fn group_title(group: &str) -> String {
    let mut chars = group.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
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
    /// What dropping it on files runs, with the files (`Place::drop`).
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

/// Whether two locations are the same place: a folder by its path (`~`
/// and the exact bytes resolved), anything else as written.
pub fn same_place(a: &Location, b: &Location) -> bool {
    a.same_place(b)
}

/// What activating `place` runs: its own command (mount, unlock), or
/// opening its location.
pub fn open_invocation(place: &Place) -> Invocation {
    if let Some(activate) = &place.activate {
        return activate.clone();
    }
    // A folder goes by its path, which need not be UTF-8.
    let location = match place.location.path() {
        Some(path) => serde_json::json!(PathArg::from(path)),
        None => serde_json::json!(place.location.to_string()),
    };
    Invocation {
        name: "navigate".into(),
        args: serde_json::json!({ "location": location }),
    }
}

/// Opening `place` in a tab behind the one shown: a middle click on its
/// row, or its menu. One that must be mounted or unlocked first runs that
/// command with `tab`, which opens the volume there once it is mounted.
pub fn open_in_tab(place: &Place) -> Invocation {
    if let Some(activate) = &place.activate {
        let mut run = activate.clone();
        if let Some(args) = run.args.as_object_mut() {
            args.insert("tab".into(), serde_json::json!(true));
        } else {
            run.args = serde_json::json!({ "tab": true });
        }
        return run;
    }
    let location = match place.location.path() {
        Some(path) => serde_json::json!(PathArg::from(path)),
        None => serde_json::json!(place.location.to_string()),
    };
    Invocation {
        name: "tab.new".into(),
        args: serde_json::json!({ "location": location, "show": false }),
    }
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
    // Groups in the user's order, those put away left out; places as the
    // user arranged them (`arrange`). A group whose places are all hidden
    // keeps its header, whose menu shows them again.
    let sections = shell.sidebar_sections.clone();
    let places: Vec<Arranged> = arrange(shell.app.registry.places.all(), &sections)
        .into_iter()
        .filter(|a| !sections.hidden.contains(&a.place.group))
        .collect();
    let groups: Vec<String> = sections.arranged(
        &places
            .iter()
            .map(|a| a.place.group.clone())
            .collect::<Vec<_>>(),
    );
    // With the keyboard, the cursor is on something drawn: where it was,
    // else the place shown, else the first.
    let focused = shell.sidebar_keys.focus.is_focused(window);
    let items = drawn_items(&places, &sections);
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
    let font = shell.font.clone();
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

    let drop_tint = theme.drop_target;
    let mut column = div()
        .id("sidebar")
        .size_full()
        .flex()
        .flex_col()
        .overflow_y_scroll()
        .track_scroll(&shell.sidebar_scroll)
        // The keyboard comes here with F6 only: a click opens a place and
        // leaves the keyboard where it was, with the files.
        .track_focus(&shell.sidebar_keys.focus)
        .key_context(crate::keymap::SIDEBAR)
        .on_any_mouse_down(|_, window, _| window.prevent_default())
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .py_1()
        // A folder dropped beside the rows becomes a bookmark; a drop on a
        // row is that row's and never reaches here.
        .drag_over::<DragPayload>(move |s, _, _, _| s.bg(drop_tint))
        .on_drop(cx.listener(|this, payload: &DragPayload, window, cx| {
            for path in this.drag_paths(payload.pane) {
                this.bookmark(&path, window, cx);
            }
        }))
        // The same from another application, or from our own drag once the
        // compositor carried it.
        .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(drop_tint))
        .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
            for path in paths.paths() {
                this.bookmark(path, window, cx);
            }
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

    let mut last_group: Option<String> = None;
    let mut indices: HashMap<(String, String), usize> = HashMap::new();
    // A group's tags, drawn as pills that wrap together.
    let mut pills: Vec<AnyElement> = Vec::new();
    // How many children the column has: what the cursor scrolls to.
    let mut children = 0;
    for (i, arranged) in places.into_iter().enumerate() {
        let Arranged {
            place,
            moves,
            shown,
            ..
        } = arranged;
        let folded = sections.folded.contains(&place.group);
        if last_group.as_deref() != Some(place.group.as_str()) {
            column = wrap_pills(column, &mut pills, &mut children);
            let item = Item::Group(place.group.clone());
            let at_cursor = cursor.as_ref() == Some(&item);
            let menu_here = marked.as_ref() == Some(&item);
            column = column.child(
                group_header(&place.group, folded, &groups, at_cursor, cx)
                    .children(shell.host_marker(MenuHost::Sidebar, menu_here)),
            );
            drawn.push((item, children));
            children += 1;
            last_group = Some(place.group.clone());
        }
        if folded || !shown {
            continue;
        }
        let item = Item::of(&place);
        let at_cursor = cursor.as_ref() == Some(&item);
        let menu_here = marked.as_ref() == Some(&item);
        let is_current = place.location.to_string() == current;
        let color = place.color.as_deref().and_then(|c| {
            bibli_core::metadata::TAG_COLORS
                .iter()
                .position(|x| *x == c)
        });
        // Its position among the places it moves among, if it moves.
        let index = moves.as_ref().map(|moves| {
            let index = indices.entry(mover(&place.group, moves)).or_default();
            *index += 1;
            *index - 1
        });
        let drag = PlaceDrag {
            group: place.group.clone(),
            index: index.unwrap_or_default(),
            reorder: moves.clone(),
            gives: place.drop.clone(),
        };
        let order = moves
            .as_ref()
            .and_then(|moves| orders.get(&mover(&place.group, moves)))
            .map_or(&[][..], Vec::as_slice);
        if let Some(color) = color {
            let mut pill = pill(i, &place, color, is_current, at_cursor, &font, cx)
                .children(shell.host_marker(MenuHost::Sidebar, menu_here));
            if drag.reorder.is_some() || drag.gives.is_some() {
                pill = movable_pill(pill, place.title.clone(), drag, order, cx);
            }
            // In the line of pills that is the column's next child.
            drawn.push((item, children));
            pills.push(pill.into_any_element());
            continue;
        }
        column = wrap_pills(column, &mut pills, &mut children);
        // Present but not mounted: clicking mounts it (or asks to unlock it).
        let open = open_invocation(&place);
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
                if is_current {
                    theme.selection_text
                } else {
                    theme.text_muted
                },
            ),
            None if busy.is_some() => controls::glyph(
                "busy",
                if is_current {
                    theme.selection_text
                } else {
                    theme.accent
                },
                ICON_PX,
            )
            .into_any_element(),
            None => div()
                .size(px(10.))
                .mx(px(3.))
                .rounded_sm()
                .bg(theme.text_muted)
                .flex_none()
                .into_any_element(),
        };
        // A title cut by the sidebar's width is said whole by the row's
        // tooltip, before what the row has to say of its own.
        let (title, clip) = crate::elements::clip::text(place.title.clone());
        let mut texts = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(div().truncate().child(title));
        let mut tip: Option<SharedString> = None;
        if let Some(busy) = &busy {
            texts = texts.child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(if is_current {
                        theme.selection_text
                    } else {
                        theme.text_muted
                    })
                    .child(busy.clone()),
            );
        } else if let Some((free, total)) = place.space {
            let used = used_fraction(free, total);
            tip = Some(format!("{} free of {}", format::size(free), format::size(total)).into());
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
            .id(("place", i))
            .flex_none()
            .min_h(px(26.))
            .py(px(3.))
            .mx_1()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(if at_cursor {
                theme.accent
            } else {
                gpui::transparent_black()
            })
            .when(busy.is_none(), gpui::Styled::cursor_pointer)
            .when(is_current, |d| {
                d.bg(theme.selection_bg).text_color(theme.selection_text)
            })
            .when(!is_current, |d| d.hover(move |s| s.bg(hover)))
            .on_click(cx.listener({
                let busy = busy.is_some();
                let item = item.clone();
                move |this, _, window, cx| {
                    this.sidebar_keys.cursor = Some(item.clone());
                    // A second click while it unlocks would ask again.
                    if !busy {
                        this.run(&open.name, open.args.clone(), window, cx);
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
                let tab = open_in_tab(&place);
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
            tip = Some(why.clone().into());
        }
        if let Some(busy) = &busy {
            tip = Some(busy.clone().into());
        }
        row = row.optional_tooltip(clip.tooltip(tip, font.clone()));
        if let Some(eject) = place.eject.clone().filter(|_| busy.is_none()) {
            let hover = theme.selection_bg;
            row = row.child(
                div()
                    .id(("eject", i))
                    .flex_none()
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.run(&eject.name, eject.args.clone(), window, cx);
                    }))
                    .child(controls::glyph("eject", theme.text_muted, 13.)),
            );
        }
        if drag.reorder.is_some() {
            row = movable_row(row, place.title.clone(), drag, order, cx);
        }
        if let Some(drop) = place.drop.clone() {
            row = takes_drops(row, drop, cx);
        } else if let Some(path) = place.location.path() {
            row = crate::dnd::droppable_dir(row, path, cx);
        }
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
    let edge = div()
        .id("sidebar-resize")
        .absolute()
        .top_0()
        .right_0()
        .h_full()
        .w(px(6.))
        .cursor(gpui::CursorStyle::ResizeLeftRight)
        .hover(|style| style.bg(theme.handle))
        .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
            if event.click_count() == 2 {
                this.sidebar_width = DEFAULT_WIDTH;
                this.remember_session();
                cx.notify();
            }
        }))
        .on_drag(SidebarResize, |_, _, _, cx| {
            cx.new(|_| crate::preview::NoDragView)
        });
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
        .child(div().relative().flex_1().min_h_0().child(column).child(
            crate::elements::scrollbar::vertical(
                "sidebar-scrollbar",
                crate::elements::scrollbar::Scrolls::Offset(shell.sidebar_scroll.clone()),
            ),
        ))
        .child(edge)
        // Where a menu the keys opened hangs when its row is not drawn:
        // outside the scrolled column, whose height it would add to.
        .children(shell.host_area_marker(MenuHost::Sidebar))
        .into_any_element()
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

/// A group's header: a click folds the group, a drag moves it among the
/// others (`shown`, in their order), a right click opens its menu. While
/// another group is dragged over it, a line marks where that one lands,
/// above its upper half and below its lower half, as with places.
fn group_header(
    group: &str,
    folded: bool,
    shown: &[String],
    at_cursor: bool,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.global::<Theme>().clone();
    let hover = theme.hover;
    let at = shown.iter().position(|g| g == group).unwrap_or(0);
    let title = group_title(group);
    let (fold, menu) = (group.to_owned(), group.to_owned());
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
    div()
        .id(SharedString::from(format!("sidebar-group-{group}")))
        .relative()
        .flex_none()
        .mx_1()
        .mt_2()
        .px_2()
        .h(px(22.))
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
        .text_xs()
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
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.open_sections_menu(Some(&menu), event.position, window, cx);
            }),
        )
        .on_drag(drag, {
            let title = title.clone();
            move |_, position, _, cx| {
                cx.new(|_| crate::dnd::DragPreview::named(title.clone(), position))
            }
        })
        .child(title)
        .child(controls::glyph(
            if folded { "chevron-right" } else { "chevron" },
            theme.text_muted,
            12.,
        ))
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
            .gap(px(4.))
            .children(pills.drain(..)),
    )
}

/// A tag's place as a pill of its color: it opens as a row does (a click,
/// a middle click for a tab behind, a right click for its menu), and files
/// dropped on it get what its `drop` gives them (the tag). One its owner
/// suggests is an outline of its color, the pills kept filled.
fn pill(
    i: usize,
    place: &Place,
    color: usize,
    is_current: bool,
    at_cursor: bool,
    font: &SharedString,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.global::<Theme>().clone();
    let tone = theme.tags[color % theme.tags.len()];
    let open = open_invocation(place);
    let tab = open_in_tab(place);
    // A tag in a category says its own name; the tooltip says where it is.
    let title: SharedString = bibli_core::metadata::tag_leaf(&place.title)
        .to_owned()
        .into();
    let tip: SharedString = format!(
        "{} · Shift+click: only what has it too · Alt+click: in this folder",
        place.title
    )
    .into();
    let font = font.clone();
    let mut pill = div()
        .id(("place", i))
        .flex_none()
        .max_w_full()
        .h(px(22.))
        .px(px(9.))
        .flex()
        .items_center()
        .rounded(px(11.))
        .border_1()
        .border_color(if is_current || at_cursor {
            theme.accent
        } else if place.suggested {
            tone.opacity(0.6)
        } else {
            gpui::transparent_black()
        })
        .bg(if is_current {
            tone.opacity(0.45)
        } else if place.suggested {
            gpui::transparent_black()
        } else {
            theme.tag_fill(color)
        })
        .text_sm()
        .text_color(if place.suggested && !is_current {
            theme.text_muted
        } else {
            theme.text
        })
        .cursor_pointer()
        .hover(move |s| s.border_color(tone))
        .on_click(cx.listener({
            let tag = place.title.clone();
            let item = Item::of(place);
            move |this, ev: &gpui::ClickEvent, window, cx| {
                this.sidebar_keys.cursor = Some(item.clone());
                // Shift narrows what is shown to the files that have the
                // tag too (or no longer), Alt shows its files in the
                // folder shown: the tags plugin says how (`tags.pill`).
                let m = ev.modifiers();
                let how = if m.shift {
                    "also"
                } else if m.alt {
                    "here"
                } else {
                    this.run(&open.name, open.args.clone(), window, cx);
                    return;
                };
                let pane = this.pane;
                this.run(
                    "tags.pill",
                    serde_json::json!({ "tag": tag, "how": how, "pane": pane }),
                    window,
                    cx,
                );
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
        .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), font.clone(), cx))
        .child(div().truncate().child(title));
    if let Some(drop) = place.drop.clone() {
        pill = takes_drops(pill, drop, cx);
    }
    pill
}

/// A place that takes files dropped on it (`Place::drop`: a tag gives them
/// itself, the Trash takes them in): `drop` runs with `pane` for a drag
/// from a pane (its selection), `entries` for another application's files.
fn takes_drops(
    place: gpui::Stateful<gpui::Div>,
    drop: Invocation,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.global::<Theme>();
    let (tint, edge) = (theme.drop_target, theme.accent);
    let from_paths = drop.clone();
    place
        .drag_over::<DragPayload>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, payload: &DragPayload, window, cx| {
            cx.stop_propagation();
            let mut args = drop.args.clone();
            args["pane"] = serde_json::json!(payload.pane);
            this.run(&drop.name, args, window, cx);
        }))
        .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
            cx.stop_propagation();
            let entries: Vec<bibli_core::op::EntryRef> = paths
                .paths()
                .iter()
                .filter_map(bibli_core::op::EntryRef::from_path)
                .collect();
            let mut args = from_paths.args.clone();
            args["entries"] = serde_json::json!(entries);
            this.run(&from_paths.name, args, window, cx);
        }))
}

/// A pill dragged: onto files, which it gives what it gives (its tag),
/// and, if it moves, among its group's like a movable row, a line on its
/// left or right edge marking where the dragged one lands.
fn movable_pill(
    pill: gpui::Stateful<gpui::Div>,
    label: String,
    drag: PlaceDrag,
    order: &[serde_json::Value],
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let (moves, index) = (drag.mover(), drag.index);
    let pill = pill.relative().on_drag(drag, move |_, position, _, cx| {
        cx.new(|_| crate::dnd::DragPreview::named(label.clone(), position))
    });
    if let Some(moves) = moves {
        pill.child(gap(order, &moves, index, Edge::Left, cx))
            .child(gap(order, &moves, index + 1, Edge::Right, cx))
    } else {
        pill
    }
}

/// Which edge of a movable place a gap's line is drawn on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
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
    order: &[serde_json::Value],
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
/// top or bottom, a pill's left or right). Its hitbox does not block the
/// place's own clicks.
fn gap(
    order: &[serde_json::Value],
    moves: &(String, String),
    before: usize,
    side: Edge,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let edge = cx.global::<Theme>().accent;
    let order = order.to_vec();
    let (shown, dropped) = (moves.clone(), moves.clone());
    let zone = div().absolute();
    let zone = match side {
        Edge::Top => zone.left_0().right_0().h(gpui::relative(0.5)).top_0(),
        Edge::Bottom => zone.left_0().right_0().h(gpui::relative(0.5)).bottom_0(),
        Edge::Left => zone.top_0().bottom_0().w(gpui::relative(0.5)).left_0(),
        Edge::Right => zone.top_0().bottom_0().w(gpui::relative(0.5)).right_0(),
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
            Edge::Left => s.border_l_2(),
            Edge::Right => s.border_r_2(),
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
        !self.sidebar_hidden && self.sidebar_fits(f32::from(window.viewport_size().width))
    }

    /// How wide the sidebar is drawn: as dragged, narrower while the window
    /// is too small for that (the remembered width stays as the user left
    /// it), and 0 when it is not drawn.
    pub fn sidebar_drawn_width(&self, window: &gpui::Window) -> f32 {
        if !self.sidebar_shown(window) {
            return 0.;
        }
        self.sidebar_width.min(self.sidebar_width_limit(window))
    }

    fn sidebar_width_limit(&self, window: &gpui::Window) -> f32 {
        let preview = self.preview.open && !self.layout.bottom();
        max_width(
            f32::from(window.viewport_size().width),
            self.panes_across(),
            preview,
        )
    }

    /// The sidebar's edge follows the pointer: the sidebar starts at the
    /// window's left edge, so the pointer's x is its width.
    pub fn on_sidebar_drag(
        &mut self,
        ev: &gpui::DragMoveEvent<SidebarResize>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(ev.event.position.x);
        self.sidebar_width = x.clamp(MIN_WIDTH, self.sidebar_width_limit(window));
        cx.notify();
    }

    /// `ui.toggle_sidebar` (F9): put the sidebar away, or bring it back. A
    /// window too narrow for it says so rather than seeming to ignore the key.
    pub fn toggle_sidebar(&mut self, window: &gpui::Window, cx: &mut Context<Self>) {
        let shown = self.sidebar_shown(window);
        self.sidebar_hidden = shown;
        if !shown && !self.sidebar_shown(window) {
            self.set_status("The window is too narrow for the sidebar", false, cx);
        }
        self.remember_session();
        cx.notify();
    }

    /// The groups that have places, put away or not.
    fn sidebar_groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = self
            .app
            .registry
            .places
            .all()
            .into_iter()
            .map(|p| p.group)
            .collect();
        groups.sort();
        groups.dedup();
        groups
    }

    /// The place of `group` named `key` (`Place::key`), as the user
    /// arranged it.
    pub fn sidebar_place_of(&self, group: &str, key: &str) -> Option<Arranged> {
        arrange(self.app.registry.places.all(), &self.sidebar_sections)
            .into_iter()
            .find(|a| a.place.group == group && a.place.key() == key)
    }

    /// What renames the place: its owner's command for one kept
    /// (`Place::rename`), the sidebar's for one offered, which asks for
    /// the name it shows under.
    pub fn sidebar_rename(&self, arranged: &Arranged) -> Option<Invocation> {
        match arranged.role {
            Role::Kept => arranged.place.rename.clone(),
            Role::Suggested => None,
            Role::Offered => {
                let place = &arranged.place;
                let own = self
                    .app
                    .registry
                    .places
                    .find(|p| p.group == place.group && p.key() == place.key())
                    .map_or_else(|| place.title.clone(), |p| p.title);
                Some(Invocation::with_args(
                    "ui.ask",
                    &serde_json::json!({
                        "title": format!("Rename “{}” in the sidebar", place.title),
                        "placeholder": format!("Name (empty: “{own}”)"),
                        "text": place.title,
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
        let order: Vec<serde_json::Value> =
            arrange(self.app.registry.places.all(), &self.sidebar_sections)
                .into_iter()
                .filter(|a| a.shown)
                .filter_map(|a| {
                    let m = a.moves?;
                    (mover(&a.place.group, &m) == along).then_some(m.before)
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
            self.set_status("That place is no longer in the sidebar", true, cx);
            return;
        };
        // A place the user keeps is its owner's to arrange: its menu says
        // how (Rename…, Remove from Bookmarks).
        let kept = format!(
            "“{}” is kept in the sidebar by its owner: its menu arranges it",
            arranged.place.title
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
                    crate::toast::Toast {
                        text: format!("Hid “{}” from the sidebar", arranged.place.title),
                        tone: crate::toast::Tone::Info,
                        action: Some(crate::toast::Action {
                            label: "Undo".into(),
                            command: PLACE_COMMAND.into(),
                            args: serde_json::json!({
                                "group": group, "key": key, "hidden": false,
                            }),
                        }),
                    },
                    cx,
                );
            }
        } else if let Some(before) = args.get("before") {
            if arranged.role != Role::Offered {
                self.set_status(kept, true, cx);
                return;
            }
            let present: Vec<String> =
                arrange(self.app.registry.places.all(), &self.sidebar_sections)
                    .into_iter()
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
            let own = self
                .app
                .registry
                .places
                .find(|p| p.group == group && p.key() == key)
                .map(|p| p.title)
                .unwrap_or_default();
            let titles = &mut self
                .sidebar_sections
                .places
                .entry(group.clone())
                .or_default()
                .titles;
            if title.is_empty() || title == own {
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

    /// `ui.focus_sidebar` (F6): the keyboard goes to the sidebar, its
    /// cursor on the place shown, or back to the files from there. A
    /// sidebar put away comes back for it.
    pub fn focus_sidebar(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        if self.sidebar_keys.focus.is_focused(window) {
            window.focus(&self.ui().focus);
            cx.notify();
            return;
        }
        if !self.sidebar_shown(window) {
            if !self.sidebar_fits(f32::from(window.viewport_size().width)) {
                self.set_status("The window is too narrow for the sidebar", false, cx);
                return;
            }
            self.sidebar_hidden = false;
            self.remember_session();
        }
        // The cursor goes where the pane is, as the frame finds it.
        self.sidebar_keys.cursor = None;
        self.sidebar_keys.reveal = true;
        window.focus(&self.sidebar_keys.focus);
        cx.notify();
    }

    /// The place under the sidebar's cursor, as the user arranged it.
    fn sidebar_cursor_place(&self) -> Option<Arranged> {
        match self.sidebar_keys.cursor.as_ref()? {
            Item::Place { group, key } => self.sidebar_place_of(group, key),
            Item::Group(_) => None,
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
    /// `open` runs the place (and gives the keyboard back to the files)
    /// or folds the group, `tab` opens the place in a tab behind; `menu`
    /// opens its menu; `move_up` and `move_down` move it or its group;
    /// `rename` renames it; `remove` takes out a place the user keeps and
    /// hides another.
    pub fn sidebar_key(&mut self, key: &str, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let at = self.sidebar_cursor_at();
        let last = self.sidebar_keys.drawn.len().saturating_sub(1);
        let cursor = self.sidebar_keys.cursor.clone();
        match (key, cursor) {
            ("up", _) => self.sidebar_cursor_to(at.map_or(0, |at| at.saturating_sub(1)), cx),
            ("down", _) => self.sidebar_cursor_to(at.map_or(0, |at| (at + 1).min(last)), cx),
            ("first", _) => self.sidebar_cursor_to(0, cx),
            ("last", _) => self.sidebar_cursor_to(last, cx),
            ("left", Some(Item::Place { group, .. })) => {
                self.sidebar_keys.cursor = Some(Item::Group(group));
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
                    && let Some((Item::Place { group: next, .. }, _)) =
                        self.sidebar_keys.drawn.get(at + 1)
                    && *next == group
                {
                    self.sidebar_cursor_to(at + 1, cx);
                }
            }
            ("open", Some(Item::Group(group))) => {
                self.sidebar_section(&serde_json::json!({ "group": group }), cx);
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
                    window.focus(&self.ui().focus);
                    open_invocation(&arranged.place)
                };
                self.run(&run.name, run.args, window, cx);
            }
            ("menu", Some(item)) => {
                match &item {
                    Item::Group(group) => {
                        self.open_sections_menu(Some(group), gpui::Point::default(), window, cx);
                    }
                    Item::Place { group, key } => {
                        self.open_place_menu(group, key, gpui::Point::default(), window, cx);
                    }
                }
                if let Some(menu) = &mut self.menu {
                    menu.highlighted = Some(0);
                    menu.keyboard = Some(crate::menu::KeyboardAnchor::new(MenuHost::Sidebar));
                }
            }
            ("move_up" | "move_down", Some(item)) => {
                let delta = if key == "move_up" { -1 } else { 1 };
                let run = match &item {
                    Item::Group(group) => self.group_move(group, delta),
                    Item::Place { .. } => self
                        .sidebar_cursor_place()
                        .and_then(|a| self.sidebar_move(&a, delta)),
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
        let item = |title: String, args: serde_json::Value, order: i32, icon: Option<&str>| {
            bibli_core::registry::MenuItem {
                menu: "sidebar".into(),
                title,
                command: Invocation::with_args("ui.sidebar_section", &args),
                when: None,
                order,
                icon: icon.map(str::to_owned),
                submenu: None,
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
                if folded { "Unfold" } else { "Fold" }.into(),
                serde_json::json!({ "group": group, "fold": !folded }),
                0,
                Some(if folded { "unfold" } else { "fold" }),
            ));
            if let Some(at) = shown.iter().position(|g| *g == group) {
                if at > 0 {
                    items.push(item(
                        "Move Up".into(),
                        serde_json::json!({ "group": group, "before": shown[at - 1] }),
                        100,
                        Some("up"),
                    ));
                }
                if at + 1 < shown.len() {
                    items.push(item(
                        "Move Down".into(),
                        serde_json::json!({ "group": group, "before": shown.get(at + 2) }),
                        101,
                        Some("down"),
                    ));
                }
            }
            // Its places the user can hide, checked while shown: those
            // hidden come back from here.
            let place_item = |title: String, args: serde_json::Value, order, icon: Option<&str>| {
                bibli_core::registry::MenuItem {
                    command: Invocation::with_args(PLACE_COMMAND, &args),
                    ..item(title, serde_json::Value::Null, order, icon)
                }
            };
            let places = arrange(self.app.registry.places.all(), sections);
            for (i, arranged) in places
                .iter()
                .filter(|a| a.place.group == group && a.role.hides())
                .enumerate()
            {
                items.push(place_item(
                    arranged.place.title.clone(),
                    serde_json::json!({
                        "group": group,
                        "key": arranged.place.key(),
                        "hidden": arranged.shown,
                    }),
                    // In one group of the menu, however many there are.
                    200 + i32::try_from(i.min(99)).unwrap_or_default(),
                    arranged.shown.then_some("check"),
                ));
            }
            if sections.places.get(group).is_some_and(|a| !a.is_empty()) {
                items.push(place_item(
                    format!("Reset {}", group_title(group)),
                    serde_json::json!({ "group": group, "reset": true }),
                    300,
                    Some("revert"),
                ));
            }
        }
        // Every group, checked while shown.
        for (i, g) in all.iter().enumerate() {
            let hidden = sections.hidden.contains(g);
            items.push(item(
                group_title(g),
                serde_json::json!({ "group": g, "hidden": !hidden }),
                400 + i32::try_from(i).unwrap_or_default(),
                (!hidden).then_some("check"),
            ));
        }
        if sections.arranged_by_user() {
            items.push(item(
                "Reset Sections".into(),
                serde_json::json!({ "reset": true }),
                500,
                Some("revert"),
            ));
        }
        if items.is_empty() {
            return;
        }
        self.menu = Some(crate::menu::MenuState::open(position, items, window, cx));
        cx.notify();
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
    use super::{Role, Sections, arrange, drop_target, group_title, same_place, used_fraction};
    use bibli_core::Location;
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

    fn place(group: &str, location: &str) -> bibli_core::registry::Place {
        bibli_core::registry::Place {
            title: location.trim_start_matches('/').to_owned(),
            location: Location::parse(location),
            icon: None,
            group: group.into(),
            space: None,
            eject: None,
            activate: None,
            unavailable: None,
            busy: None,
            menu: Vec::new(),
            forget: None,
            reorder: None,
            color: None,
            drop: None,
            suggested: false,
            key: None,
            hidden: false,
            rename: None,
        }
    }

    fn titles(arranged: &[super::Arranged]) -> Vec<String> {
        arranged
            .iter()
            .filter(|a| a.shown)
            .map(|a| a.place.title.clone())
            .collect()
    }

    /// The places offered are the user's to put in order, rename and hide;
    /// a bookmark stays where its owner keeps it, and a volume is the same
    /// place by its key whether it is mounted or not.
    #[test]
    fn offered_places_go_where_the_user_put_them_under_their_names() {
        let mut bookmark = place("places", "/bookmarked");
        bookmark.forget = Some(bibli_core::command::Invocation::new("places.unbookmark"));
        let recent = place("places", "recent:");
        let mut efi = place("devices", "/dev/sda1");
        efi.title = "EFI".into();
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
                .find(|a| a.place.title == title)
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
        let place = |location: &str| bibli_core::registry::Place {
            title: "Backup".into(),
            location: Location::parse(location),
            icon: None,
            group: "devices".into(),
            space: None,
            eject: None,
            activate: None,
            unavailable: None,
            busy: None,
            menu: Vec::new(),
            forget: None,
            reorder: None,
            color: None,
            drop: None,
            suggested: false,
            key: None,
            hidden: false,
            rename: None,
        };
        let mut locked = place("device:/dev/sdz");
        locked.activate = Some(bibli_core::command::Invocation::with_args(
            "ui.unlock_prompt",
            &serde_json::json!({ "device": "/dev/sdz", "title": "Backup" }),
        ));
        let run = super::open_in_tab(&locked);
        assert_eq!(&*run.name, "ui.unlock_prompt");
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
        plain.activate = Some(bibli_core::command::Invocation::new("places.mount"));
        assert_eq!(
            super::open_in_tab(&plain).args,
            serde_json::json!({ "tab": true })
        );
        let run = super::open_in_tab(&place("/run/media/example/Backup"));
        assert_eq!(&*run.name, "tab.new");
        assert_eq!(run.args["show"], serde_json::json!(false));
    }

    #[test]
    fn usage_is_the_share_that_is_not_free() {
        assert!((used_fraction(25, 100) - 0.75).abs() < f32::EPSILON);
        assert!(used_fraction(0, 0).abs() < f32::EPSILON);
        assert!(
            (used_fraction(200, 100)).abs() < f32::EPSILON,
            "never negative"
        );
        assert_eq!(group_title("devices"), "Devices");
        assert_eq!(group_title("searches"), "Searches");
    }

    #[test]
    fn a_folder_is_the_same_place_by_its_path_anything_else_as_written() {
        let home = Location::parse("~").path().unwrap();
        assert!(same_place(
            &Location::parse("~/a b"),
            &Location::from_path(home.join("a b"))
        ));
        assert!(!same_place(
            &Location::from_path("/a"),
            &Location::from_path("/b")
        ));
        assert!(same_place(
            &Location::parse("query:ext:pdf in:/x"),
            &Location::new("query", "ext:pdf in:/x")
        ));
        assert!(!same_place(
            &Location::parse("query:ext:pdf"),
            &Location::parse("query:ext:png")
        ));
        assert!(!same_place(
            &Location::parse("/query"),
            &Location::parse("query:/")
        ));
    }
}
