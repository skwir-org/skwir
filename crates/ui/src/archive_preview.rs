//! Archive contents use the file list's density and column alignment.
use crate::{format, icons, theme::Theme, views::list::ROW_HEIGHT};
use bibli_core::inspection::Row;
use gpui::{Div, div, prelude::*, px, svg};
use std::path::Path;

const SIZE_WIDTH: f32 = 90.;
const ICON_SIZE: f32 = 18.;
const DISCLOSURE_SIZE: f32 = 12.;

pub fn header(theme: &Theme) -> Div {
    div()
        .h(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .text_xs()
        .text_color(theme.text_muted)
        .bg(theme.surface)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pl(px(DISCLOSURE_SIZE + ICON_SIZE + 8.))
                .child("Name"),
        )
        .child(
            div()
                .w(px(SIZE_WIDTH))
                .flex_none()
                .text_right()
                .child("Size"),
        )
}

pub fn row(row: &Row, icon: Option<&Path>, collapsed: bool, selected: bool, theme: &Theme) -> Div {
    let icon = icon.map_or_else(
        || {
            div()
                .size(px(ICON_SIZE))
                .rounded_sm()
                .bg(if row.branch {
                    theme.accent
                } else {
                    theme.text_muted
                })
                .into_any_element()
        },
        |path| icons::element(path, ICON_SIZE),
    );
    let mut name = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap_1()
        // Indentation belongs to Name only; sizes must stay aligned at every depth.
        .pl(px(row.depth.min(32) as f32 * 14.))
        .child(
            div()
                .w(px(DISCLOSURE_SIZE))
                .flex_none()
                .when(row.branch, |d| {
                    d.child(
                        svg()
                            .path(if collapsed {
                                "toolbar/right.svg"
                            } else {
                                "toolbar/chevron.svg"
                            })
                            .size(px(DISCLOSURE_SIZE))
                            .text_color(theme.text_muted),
                    )
                }),
        )
        .child(div().size(px(ICON_SIZE)).flex_none().child(icon))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .line_clamp(1)
                .text_ellipsis()
                .child(row.cells.first().cloned().unwrap_or_default()),
        );
    if let Some(status) = row.cells.get(2).filter(|s| !s.is_empty()) {
        name = name.child(
            div()
                .flex_none()
                .px_1()
                .rounded_sm()
                .text_xs()
                .text_color(theme.text_muted)
                .bg(theme.code_bg)
                .child(status.clone()),
        );
    }
    div()
        .h(px(ROW_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .text_sm()
        .when(!selected, |d| d.hover(|d| d.bg(theme.hover)))
        .when(selected, |d| d.text_color(theme.selection_text))
        .child(name)
        .child(
            div()
                .w(px(SIZE_WIDTH))
                .flex_none()
                .text_right()
                .text_color(if selected {
                    theme.selection_text
                } else {
                    theme.text
                })
                .child(row.bytes.map_or_else(|| "—".into(), format::size)),
        )
}
