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
//! one hidden.

use std::collections::HashMap;

use gpui::{AnyElement, Context, ExternalPaths, MouseButton, SharedString, div, prelude::*, px};

use bibli_core::Location;
use bibli_core::command::Invocation;
use bibli_core::os_path::PathArg;
use bibli_core::registry::{Place, Reorder};

use crate::controls;
use crate::dnd::DragPayload;
use crate::format;
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

impl PlaceDrag {
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
/// row, or its menu.
pub fn open_in_tab(place: &Place) -> Invocation {
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
pub fn render(shell: &mut Shell, width: f32, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let current = shell.ui().address_shown.clone();
    let mut places = shell.app.registry.places.all();
    // Groups in the user's order, those put away left out. Within a group,
    // folders come before virtual places (Shelf, Recent) whatever order
    // their plugins published them in; the sort is stable.
    let sections = shell.sidebar_sections.clone();
    places.retain(|p| !sections.hidden.contains(&p.group));
    places.sort_by_key(|p| (sections.rank(&p.group), !p.location.is_path()));
    let shown: Vec<String> =
        sections.arranged(&places.iter().map(|p| p.group.clone()).collect::<Vec<_>>());
    let font = shell.font.clone();
    // Each group's movable places, in order, by what names them to the
    // group's move command.
    let mut orders: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for place in &places {
        if let Some(reorder) = &place.reorder {
            orders
                .entry(place.group.clone())
                .or_default()
                .push(reorder.before.clone());
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
    let mut indices: HashMap<String, usize> = HashMap::new();
    // A group's tags, drawn as pills that wrap together.
    let mut pills: Vec<AnyElement> = Vec::new();
    for (i, place) in places.into_iter().enumerate() {
        let folded = sections.folded.contains(&place.group);
        if last_group.as_deref() != Some(place.group.as_str()) {
            column = wrap_pills(column, &mut pills);
            column = column.child(group_header(&place.group, folded, &shown, cx));
            last_group = Some(place.group.clone());
        }
        if folded {
            continue;
        }
        let is_current = place.location.to_string() == current;
        let color = place.color.as_deref().and_then(|c| {
            bibli_core::metadata::TAG_COLORS
                .iter()
                .position(|x| *x == c)
        });
        // Its position among its group's movable places, if it moves.
        let index = place.reorder.as_ref().map(|_| {
            let index = indices.entry(place.group.clone()).or_default();
            *index += 1;
            *index - 1
        });
        let drag = PlaceDrag {
            group: place.group.clone(),
            index: index.unwrap_or_default(),
            reorder: place.reorder.clone(),
            gives: place.drop.clone(),
        };
        let order = orders.get(&place.group).map_or(&[][..], Vec::as_slice);
        if let Some(color) = color {
            let mut pill = pill(i, &place, color, is_current, &font, cx);
            if drag.reorder.is_some() || drag.gives.is_some() {
                pill = movable_pill(pill, place.title.clone(), drag, order, cx);
            }
            pills.push(pill.into_any_element());
            continue;
        }
        column = wrap_pills(column, &mut pills);
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
        let mut texts = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(div().truncate().child(place.title.clone()));
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
            .border_color(gpui::transparent_black())
            .when(busy.is_none(), gpui::Styled::cursor_pointer)
            .when(is_current, |d| {
                d.bg(theme.selection_bg).text_color(theme.selection_text)
            })
            .when(!is_current, |d| d.hover(move |s| s.bg(hover)))
            .on_click(cx.listener({
                let busy = busy.is_some();
                move |this, _, window, cx| {
                    // A second click while it unlocks would ask again.
                    if !busy {
                        this.run(&open.name, open.args.clone(), window, cx);
                    }
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let place = place.clone();
                    move |this, event: &gpui::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_place_menu(&place, event.position, window, cx);
                    }
                }),
            )
            // The middle button opens it in a tab behind the one shown.
            .when(place.activate.is_none(), |row| {
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
        if let Some(tip) = tip {
            let font = font.clone();
            row = row.tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), font.clone(), cx));
        }
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
        if let Some(path) = place.location.path() {
            row = crate::dnd::droppable_dir(row, path, cx);
        }
        column = column.child(row);
    }
    column = wrap_pills(column, &mut pills);
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
        .text_xs()
        .text_color(theme.text_muted)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .on_click(cx.listener(move |this, _, window, cx| {
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
) -> gpui::Stateful<gpui::Div> {
    if pills.is_empty() {
        return column;
    }
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
    font: &SharedString,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.global::<Theme>().clone();
    let tone = theme.tags[color % theme.tags.len()];
    let open = open_invocation(place);
    let tab = open_in_tab(place);
    let title: SharedString = place.title.clone().into();
    let tip = title.clone();
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
        .border_color(if is_current {
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
        .on_click(cx.listener(move |this, _, window, cx| {
            this.run(&open.name, open.args.clone(), window, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener({
                let place = place.clone();
                move |this, event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_place_menu(&place, event.position, window, cx);
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
        let (tint, edge) = (theme.drop_target, theme.accent);
        let from_paths = drop.clone();
        pill = pill
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
            }));
    }
    pill
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
    let (group, index, moves) = (drag.group.clone(), drag.index, drag.reorder.is_some());
    let pill = pill.relative().on_drag(drag, move |_, position, _, cx| {
        cx.new(|_| crate::dnd::DragPreview::named(label.clone(), position))
    });
    if moves {
        pill.child(gap(order, &group, index, Edge::Left, cx))
            .child(gap(order, &group, index + 1, Edge::Right, cx))
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
    let (group, index) = (drag.group.clone(), drag.index);
    row.relative()
        .on_drag(drag, move |_, position, _, cx| {
            cx.new(|_| crate::dnd::DragPreview::named(label.clone(), position))
        })
        .child(gap(order, &group, index, Edge::Top, cx))
        .child(gap(order, &group, index + 1, Edge::Bottom, cx))
}

/// Half of a movable place, over it: the drop zone of the gap before the
/// place at `before` in `group`, drawn as a line on `side` (a row's top or
/// bottom, a pill's left or right). Its hitbox does not block the place's
/// own clicks.
fn gap(
    order: &[serde_json::Value],
    group: &str,
    before: usize,
    side: Edge,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let edge = cx.global::<Theme>().accent;
    let order = order.to_vec();
    let (shown, dropped) = (group.to_owned(), group.to_owned());
    let zone = div().absolute();
    let zone = match side {
        Edge::Top => zone.left_0().right_0().h(gpui::relative(0.5)).top_0(),
        Edge::Bottom => zone.left_0().right_0().h(gpui::relative(0.5)).bottom_0(),
        Edge::Left => zone.top_0().bottom_0().w(gpui::relative(0.5)).left_0(),
        Edge::Right => zone.top_0().bottom_0().w(gpui::relative(0.5)).right_0(),
    };
    zone.drag_over::<PlaceDrag>(move |s, drag, _, _| {
        if drag.reorder.is_none()
            || drag.group != shown
            || drag.index == before
            || drag.index + 1 == before
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
        // would put the place last. A place of another group stays put.
        cx.stop_propagation();
        if drag.group != dropped {
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
    /// moving and hiding it; everywhere, showing a group put away.
    pub fn open_sections_menu(
        &mut self,
        group: Option<&str>,
        position: gpui::Point<gpui::Pixels>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let item = |title: String, args: serde_json::Value, order: i32, icon: &str| {
            bibli_core::registry::MenuItem {
                menu: "sidebar".into(),
                title,
                command: Invocation::with_args("ui.sidebar_section", &args),
                when: None,
                order,
                icon: Some(icon.into()),
                submenu: None,
            }
        };
        let sections = &self.sidebar_sections;
        let present = self.sidebar_groups();
        let shown: Vec<String> = sections.arranged(
            &present
                .iter()
                .filter(|g| !sections.hidden.contains(*g))
                .cloned()
                .collect::<Vec<_>>(),
        );
        let mut items = Vec::new();
        if let Some(group) = group {
            let folded = sections.folded.contains(group);
            items.push(item(
                if folded { "Unfold" } else { "Fold" }.into(),
                serde_json::json!({ "group": group, "fold": !folded }),
                0,
                if folded { "unfold" } else { "fold" },
            ));
            if let Some(at) = shown.iter().position(|g| g == group) {
                if at > 0 {
                    items.push(item(
                        "Move Up".into(),
                        serde_json::json!({ "group": group, "before": shown[at - 1] }),
                        100,
                        "up",
                    ));
                }
                if at + 1 < shown.len() {
                    items.push(item(
                        "Move Down".into(),
                        serde_json::json!({ "group": group, "before": shown.get(at + 2) }),
                        101,
                        "down",
                    ));
                }
            }
            items.push(item(
                format!("Hide {}", group_title(group)),
                serde_json::json!({ "group": group, "hidden": true }),
                300,
                "eye-off",
            ));
        }
        for hidden in &sections.hidden {
            items.push(item(
                format!("Show {}", group_title(hidden)),
                serde_json::json!({ "group": hidden, "hidden": false }),
                400,
                "eye",
            ));
        }
        if sections.arranged_by_user() {
            items.push(item(
                "Reset Sections".into(),
                serde_json::json!({ "reset": true }),
                410,
                "revert",
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
    use super::{Sections, drop_target, group_title, same_place, used_fraction};
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
