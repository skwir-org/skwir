//! Archive contents use the file list's density and column alignment, and
//! its way of giving way: the size drops whole when the name needs the room
//! (`views::list::line`).
use crate::{
    format, icons,
    theme::Theme,
    views::list::{ROW_HEIGHT, column, line},
};
use bibli_core::inspection::Row;
use gpui::{Div, SharedString, div, prelude::*, px, svg};

const SIZE_WIDTH: f32 = 90.;
const ICON_SIZE: f32 = 18.;
const DISCLOSURE_SIZE: f32 = 12.;
/// Narrower than this, a name gives up the size column beside it.
const NAME_MIN: f32 = 120.;
const HEADER_HEIGHT: f32 = 24.;

pub fn header(theme: &Theme) -> Div {
    let name = div()
        .min_w_0()
        .pl(px(DISCLOSURE_SIZE + ICON_SIZE + 8.))
        .child("Name");
    div()
        .h(px(HEADER_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .px_2()
        .text_xs()
        .text_color(theme.text_muted)
        .bg(theme.surface)
        .border_b_1()
        .border_color(theme.border)
        .child(
            line(name, NAME_MIN, HEADER_HEIGHT).child(
                column(SIZE_WIDTH, theme.text_muted, HEADER_HEIGHT)
                    .text_right()
                    .child("Size"),
            ),
        )
}

pub fn row(
    row: &Row,
    icon: Option<&icons::Pic>,
    collapsed: bool,
    selected: bool,
    font: SharedString,
    theme: &Theme,
) -> Div {
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
        |pic| icons::element(pic, ICON_SIZE, theme.text_muted),
    );
    let mut name = div()
        .min_w_0()
        .overflow_hidden()
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
        .child(crate::elements::clip::clipped(
            div()
                .id("member-name")
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle(),
            row.cells.first().cloned().unwrap_or_default(),
            font,
        ));
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
    let size = column(
        SIZE_WIDTH,
        if selected {
            theme.selection_text
        } else {
            theme.text
        },
        ROW_HEIGHT,
    )
    .text_right()
    .child(row.bytes.map_or_else(|| "—".into(), format::size));
    div()
        .h(px(ROW_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .px_2()
        .text_sm()
        .when(!selected, |d| d.hover(|d| d.bg(theme.hover)))
        .when(selected, |d| d.text_color(theme.selection_text))
        .child(line(name, NAME_MIN, ROW_HEIGHT).child(size))
}
