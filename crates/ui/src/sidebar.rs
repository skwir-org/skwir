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

fn group_title(group: &str) -> String {
    let mut chars = group.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A place being dragged to another position in its group.
#[derive(Clone, Debug)]
pub struct PlaceDrag {
    pub group: String,
    /// Its position among the group's movable places when the drag started.
    pub index: usize,
    pub reorder: Reorder,
}

impl PlaceDrag {
    /// The command that moves it before the place whose `before` is given,
    /// or last.
    fn command(&self, before: Option<serde_json::Value>) -> Invocation {
        let mut command = self.reorder.command.clone();
        if let Some(before) = before
            && let Some(args) = command.args.as_object_mut()
        {
            args.insert("before".into(), before);
        }
        command
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
    // Within a group, folders come before virtual places (Shelf, Recent)
    // whatever order their plugins published them in; the sort is stable.
    places.sort_by_key(|p| (group_rank(&p.group), !p.location.is_path()));
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
            let command = drag.command(None);
            this.run(&command.name, command.args, window, cx);
        }));

    let mut last_group: Option<String> = None;
    let mut indices: HashMap<String, usize> = HashMap::new();
    // A group's tags, drawn as pills that wrap together.
    let mut pills: Vec<AnyElement> = Vec::new();
    for (i, place) in places.into_iter().enumerate() {
        let folded = shell.sidebar_folded.contains(&place.group);
        if last_group.as_deref() != Some(place.group.as_str()) {
            column = wrap_pills(column, &mut pills);
            let group = place.group.clone();
            let hover = theme.hover;
            column = column.child(
                div()
                    .id(SharedString::from(format!("sidebar-group-{group}")))
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.sidebar_folded.remove(&group) {
                            this.sidebar_folded.insert(group.clone());
                        }
                        cx.notify();
                    }))
                    .child(group_title(&place.group))
                    .child(controls::glyph(
                        if folded { "chevron-right" } else { "chevron" },
                        theme.text_muted,
                        12.,
                    )),
            );
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
        if let Some(color) = color {
            pills.push(pill(i, &place, color, is_current, &font, cx));
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
        if let Some(reorder) = &place.reorder {
            let index = indices.entry(place.group.clone()).or_default();
            let drag = PlaceDrag {
                group: place.group.clone(),
                index: *index,
                reorder: reorder.clone(),
            };
            *index += 1;
            let order = orders.get(&place.group).map_or(&[][..], Vec::as_slice);
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
/// dropped on it get what its `drop` gives them (the tag).
fn pill(
    i: usize,
    place: &Place,
    color: usize,
    is_current: bool,
    font: &SharedString,
    cx: &mut Context<Shell>,
) -> AnyElement {
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
        } else {
            gpui::transparent_black()
        })
        .bg(if is_current {
            tone.opacity(0.45)
        } else {
            theme.tag_fill(color)
        })
        .text_sm()
        .text_color(theme.text)
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
    pill.into_any_element()
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
        .child(gap(order, &group, index, true, cx))
        .child(gap(order, &group, index + 1, false, cx))
}

/// Half of a movable place's row, over it: the drop zone of the gap before
/// the place at `before` in `group`, drawn as a line on the row's top or
/// bottom edge. Its hitbox does not block the row's own clicks.
fn gap(
    order: &[serde_json::Value],
    group: &str,
    before: usize,
    top: bool,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let edge = cx.global::<Theme>().accent;
    let order = order.to_vec();
    let (shown, dropped) = (group.to_owned(), group.to_owned());
    let zone = div().absolute().left_0().right_0().h(gpui::relative(0.5));
    let zone = if top { zone.top_0() } else { zone.bottom_0() };
    zone.drag_over::<PlaceDrag>(move |s, drag, _, _| {
        if drag.group != shown || drag.index == before || drag.index + 1 == before {
            s
        } else if top {
            s.border_t_2().border_color(edge)
        } else {
            s.border_b_2().border_color(edge)
        }
    })
    .on_drop(cx.listener(move |this, drag: &PlaceDrag, window, cx| {
        // Handled here even where nothing moves: the sidebar's own drop
        // would put the place last. A place of another group stays put.
        cx.stop_propagation();
        if drag.group != dropped {
            return;
        }
        if let Some(before) = drop_target(&order, drag.index, before) {
            let command = drag.command(before);
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
    use super::{drop_target, group_title, same_place, used_fraction};
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
