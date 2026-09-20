//! Sidebar: places from the registry, grouped, click to navigate.
//!
//! Rows use the desktop theme's icons, volumes show how full they are, and
//! removable ones can be detached. A row accepts dropped files like any
//! folder; dropping a folder beside the rows bookmarks it.

use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};

use crate::controls;
use crate::dnd::DragPayload;
use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;

const GROUP_ORDER: [&str; 3] = ["places", "bookmarks", "devices"];
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

    let drop_tint = theme.selection_bg.opacity(0.35);
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
            for path in &payload.paths {
                this.run(
                    "places.bookmark",
                    serde_json::json!({ "path": path }),
                    window,
                    cx,
                );
            }
        }));

    let mut last_group: Option<String> = None;
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
        let location = place.location.to_string();
        // Present but not mounted: clicking mounts it (or asks to unlock it).
        let activate = place.activate.clone();
        let waiting = activate.is_some();
        let icon: AnyElement = match place
            .icon
            .as_deref()
            .and_then(|name| shell.icons.named(name, ICON_PX as u16))
        {
            Some(path) => crate::icons::element(&path, ICON_PX),
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
            .on_click(cx.listener(move |this, _, window, cx| match &activate {
                Some(activate) => this.run(&activate.name, activate.args.clone(), window, cx),
                None => this.run(
                    "navigate",
                    serde_json::json!({ "location": location }),
                    window,
                    cx,
                ),
            }))
            .child(
                div()
                    .flex_none()
                    .when(waiting, |d| d.opacity(0.55))
                    .child(icon),
            )
            .child(texts.when(waiting, |d| d.text_color(theme.text_muted)));
        if waiting {
            tip = Some(
                if place
                    .activate
                    .as_ref()
                    .is_some_and(|a| &*a.name == "ui.unlock_prompt")
                {
                    "Locked. Click to enter its passphrase and open it."
                } else {
                    "Not mounted. Click to mount and open it."
                }
                .into(),
            );
        }
        if let Some(tip) = tip {
            let font = font.clone();
            row = row.tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), font.clone(), cx));
        }
        if let Some(eject) = place.eject.clone() {
            let hover = theme.border;
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

#[cfg(test)]
mod tests {
    use super::{group_title, used_fraction};

    #[test]
    fn usage_is_the_share_that_is_not_free() {
        assert!((used_fraction(25, 100) - 0.75).abs() < f32::EPSILON);
        assert!(used_fraction(0, 0).abs() < f32::EPSILON);
        assert!(
            (used_fraction(200, 100)).abs() < f32::EPSILON,
            "never negative"
        );
        assert_eq!(group_title("devices"), "Devices");
    }
}
