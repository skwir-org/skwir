//! Sidebar: places from the registry, grouped, click to navigate.
//!
//! Rows use the desktop theme's icons, volumes show how full they are, and
//! removable ones can be detached. A row accepts dropped files like any
//! folder; dropping a folder beside the rows bookmarks it. A right-click
//! opens the place's own menu (`Place::menu`: rename or remove a bookmark
//! or a saved search), and a place its owner lets move (`Place::reorder`:
//! bookmarks, saved searches) is put in order by dragging it among the
//! others of its group.

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

const GROUP_ORDER: [&str; 4] = ["places", "bookmarks", "searches", "devices"];
const ICON_PX: f32 = 16.;

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
    match (a.path(), b.path()) {
        (Some(a), Some(b)) => a == b,
        (None, None) => a == b,
        _ => false,
    }
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

/// Share of the volume in use, 0 to 1.
pub fn used_fraction(free: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.;
    }
    (1. - free as f64 / total as f64).clamp(0., 1.) as f32
}

pub fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> AnyElement {
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
    for (i, place) in places.into_iter().enumerate() {
        let folded = shell.sidebar_folded.contains(&place.group);
        if last_group.as_deref() != Some(place.group.as_str()) {
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
        // Present but not mounted: clicking mounts it (or asks to unlock it).
        let open = open_invocation(&place);
        let waiting = place.unavailable.is_some();
        let icon: AnyElement = match place
            .icon
            .as_deref()
            .and_then(|name| shell.icons.named(name, ICON_PX as u16))
        {
            Some(path) => crate::icons::themed(
                &path,
                ICON_PX,
                if is_current {
                    theme.selection_text
                } else {
                    theme.text_muted
                },
            ),
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
        if let Some((free, total)) = place.space {
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
            .cursor_pointer()
            .when(is_current, |d| {
                d.bg(theme.selection_bg).text_color(theme.selection_text)
            })
            .when(!is_current, |d| d.hover(move |s| s.bg(hover)))
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
            .child(
                div()
                    .flex_none()
                    .when(waiting, |d| d.opacity(0.55))
                    .child(icon),
            )
            .child(texts.when(waiting, |d| d.text_color(theme.text_muted)));
        if let Some(why) = &place.unavailable {
            tip = Some(why.clone().into());
        }
        if let Some(tip) = tip {
            let font = font.clone();
            row = row.tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), font.clone(), cx));
        }
        if let Some(eject) = place.eject.clone() {
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
    // The bar is a sibling of the scrolling column, not a child: a child
    // would scroll away with the places.
    div()
        .relative()
        .w(px(200.))
        .h_full()
        .flex_none()
        .child(column)
        .child(crate::elements::scrollbar::vertical(
            "sidebar-scrollbar",
            crate::elements::scrollbar::Scrolls::Offset(shell.sidebar_scroll.clone()),
        ))
        .into_any_element()
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
