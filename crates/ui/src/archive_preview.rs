//! Archive contents use the file list's density and column alignment, and
//! its way of giving way: the size drops whole when the name needs the room
//! (`views::list::line`). They are written at the files' size, as the list
//! is (ADR-117): the inspector draws them in its rem (`rem`), and their
//! lengths follow it.
use crate::sizes::TextSizes;
use crate::{
    format, icons,
    theme::Theme,
    views::list::{DEFAULT_ROW_ZOOM, column, files_scale, line, row_height},
};
use gpui::{Div, div, prelude::*, px, svg};
use skwir_core::inspection::Row;

const SIZE_WIDTH: f32 = 90.;
const ICON_SIZE: f32 = 18.;
const DISCLOSURE_SIZE: f32 = 12.;
/// Narrower than this, a name gives up the size column beside it.
const NAME_MIN: f32 = 120.;
const HEADER_HEIGHT: f32 = 24.;

/// `members` (the header or the list of an archive's members) at the
/// files' size.
pub fn rem(theme: &Theme, members: impl IntoElement) -> crate::elements::rem::Rem {
    crate::elements::rem::rem(theme.sizes.rem(crate::sizes::Part::Files), members)
}

pub fn header(theme: &Theme) -> Div {
    let scale = files_scale(theme);
    let height = scale.f(HEADER_HEIGHT);
    // Over the names: past the triangle and the icon, which keep their
    // size, and the two gaps of a row (`gap_1`), which grow with the text.
    let name = div()
        .min_w_0()
        .pl(px(DISCLOSURE_SIZE + ICON_SIZE + scale.f(8.)))
        .child("Name");
    div()
        .h(px(height))
        .flex_none()
        .flex()
        .items_center()
        .px_2()
        .text_small()
        .text_color(theme.text_muted)
        .bg(theme.on_bg(theme.surface))
        .border_b_1()
        .border_color(theme.border)
        .child(
            line(name, NAME_MIN, height, scale).child(
                column(scale.f(SIZE_WIDTH), theme.text_muted, height)
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
    theme: &Theme,
) -> Div {
    let scale = files_scale(theme);
    let height = row_height(DEFAULT_ROW_ZOOM, scale);
    let icon = icon.map_or_else(
        || {
            div()
                .size(px(ICON_SIZE))
                .rounded_sm()
                .bg(if selected {
                    theme.selection_text
                } else if row.branch {
                    theme.accent
                } else {
                    theme.text_muted
                })
                .into_any_element()
        },
        |pic| icons::element(pic, ICON_SIZE, icons::Ink::beside(theme, selected)),
    );
    let mut name = div()
        .min_w_0()
        .overflow_hidden()
        .gap_1()
        // Indentation belongs to Name only; sizes must stay aligned at every depth.
        .pl(px(row.depth.min(32) as f32 * scale.f(14.)))
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
        ));
    if let Some(status) = row.cells.get(2).filter(|s| !s.is_empty()) {
        name = name.child(
            div()
                .flex_none()
                .px_1()
                .rounded_sm()
                .text_small()
                .text_color(theme.text_muted)
                .bg(theme.code_bg)
                .child(status.clone()),
        );
    }
    let size = column(scale.f(SIZE_WIDTH), theme.text, height)
        .text_right()
        .child(row.bytes.map_or_else(|| "—".into(), format::size));
    div()
        .h(px(height))
        .w_full()
        .flex()
        .items_center()
        .px_2()
        .text_body()
        .when(!selected, |d| d.hover(|d| d.bg(theme.hover)))
        .child(line(name, NAME_MIN, height, scale).child(size))
}
