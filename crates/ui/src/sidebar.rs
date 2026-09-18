//! Sidebar: places from the registry, grouped, click to navigate.

use gpui::{AnyElement, Context, div, prelude::*, px};

use crate::shell::Shell;
use crate::theme::Theme;

const GROUP_ORDER: [&str; 3] = ["places", "bookmarks", "devices"];

fn group_rank(g: &str) -> usize {
    GROUP_ORDER
        .iter()
        .position(|x| *x == g)
        .unwrap_or(GROUP_ORDER.len())
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let current = shell.address_shown.clone();
    let mut places = shell.app.registry.places.all();
    places.sort_by_key(|p| group_rank(&p.group));

    let mut column = div()
        .w(px(200.))
        .h_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .py_1();

    let mut last_group: Option<String> = None;
    for (i, place) in places.into_iter().enumerate() {
        if last_group.as_deref() != Some(place.group.as_str()) {
            column = column.child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(place.group.to_uppercase()),
            );
            last_group = Some(place.group.clone());
        }
        let is_current = place.location.to_string() == current;
        let location = place.location.to_string();
        let color = if place.group == "places" {
            theme.accent
        } else {
            theme.text_muted
        };
        column = column.child(
            div()
                .id(("place", i))
                .h(px(26.))
                .mx_1()
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .cursor_pointer()
                .when(is_current, |d| {
                    d.bg(theme.selection_bg).text_color(theme.selection_text)
                })
                .hover(|s| s.bg(theme.border))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run(
                        "navigate",
                        serde_json::json!({ "location": location }),
                        window,
                        cx,
                    );
                }))
                .child(div().size(px(10.)).rounded_sm().bg(color).flex_none())
                .child(div().truncate().child(place.title.clone())),
        );
    }
    column.into_any_element()
}
