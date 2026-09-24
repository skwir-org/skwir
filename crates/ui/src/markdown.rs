//! Reading typography for previewed documents.
//!
//! One place decides how a document reads: body and heading sizes, vertical
//! rhythm, code blocks, quotes, lists, tables and rules. The inspector builds
//! a [`MarkdownStyle`] from the theme and renders every block through it, so a
//! document looks the same wherever it is shown and the numbers only change
//! here. `reading` selects the roomier Markdown layout; without it the style
//! keeps the compact spacing used by source, archives and tables.
//!
//! A block is drawn in one pass: the container carries the spacing, the quote
//! border and the list indent whatever the block holds, and only its middle
//! differs between text, a code box and a table row. Nothing may return early
//! and skip the frame, or code inside a quote loses its border.
use crate::{inspector_text, theme::Theme};
use bibli_core::{
    inspection::{Align, Block, BlockKind},
    workspace::PaneId,
};
use gpui::{
    AnyElement, FontWeight, Hsla, RenderImage, SharedString, WeakEntity, div, prelude::*, px,
};
use std::sync::Arc;

/// Heading size relative to the body, indexed by heading level.
const HEADING_SCALE: [f32; 7] = [1., 1.75, 1.4, 1.2, 1., 0.875, 0.85];

#[derive(Clone)]
pub struct MarkdownStyle {
    pub text: Hsla,
    pub text_muted: Hsla,
    pub link: Hsla,
    pub selection_bg: Hsla,
    pub border: Hsla,
    pub body_size: f32,
    pub line_height: f32,
    /// Padding above and below an ordinary block.
    pub block_spacing: f32,
    /// Padding around what stands apart: a code box, a thematic break.
    pub section_spacing: f32,
    heading_sizes: [f32; 7],
    pub heading_top: f32,
    pub heading_bottom: f32,
    /// Whether the top two heading levels are underlined.
    pub heading_rule: bool,
    pub code_size: f32,
    pub code_padding: f32,
    pub code_bg: Hsla,
    pub inline_code_bg: Hsla,
    pub table_header_bg: Hsla,
    pub link_underline: bool,
    pub quote_text: Hsla,
    pub quote_border: Hsla,
    pub list_indent: f32,
    /// Space between the text column and the edge of the pane.
    pub page_padding: f32,
    pub marker_color: Hsla,
    /// Where a picture will be, until it is decoded.
    pub placeholder: Hsla,
    /// The widest the text column may be, when reading asks for a measure.
    pub measure: Option<f32>,
    pub reading: bool,
}

/// Characters per line a reading column aims for.
const MEASURE: f32 = 42.;

impl MarkdownStyle {
    pub fn new(theme: &Theme, body_size: f32, reading: bool) -> Self {
        let heading_sizes = std::array::from_fn(|level| match level {
            0 => body_size,
            _ if reading => body_size * HEADING_SCALE[level],
            _ => body_size + (7 - level) as f32 * 2.,
        });
        Self {
            text: theme.text,
            text_muted: theme.text_muted,
            link: theme.accent,
            selection_bg: theme.selection_bg,
            border: theme.border,
            body_size,
            line_height: if reading { 1.5 } else { 1.35 },
            block_spacing: 8.,
            section_spacing: if reading { 16. } else { 8. },
            heading_sizes,
            heading_top: if reading { 20. } else { 8. },
            heading_bottom: 8.,
            heading_rule: reading,
            code_size: (body_size - 2.).max(8.),
            code_padding: if reading { 12. } else { 8. },
            code_bg: theme.code_bg,
            inline_code_bg: theme.code_bg,
            table_header_bg: theme.code_bg,
            link_underline: reading,
            quote_text: if reading {
                theme.text_muted
            } else {
                theme.text
            },
            quote_border: theme.accent,
            list_indent: if reading { 24. } else { 20. },
            page_padding: if reading { 28. } else { 8. },
            marker_color: if reading {
                theme.text_muted
            } else {
                theme.text
            },
            placeholder: theme.surface,
            measure: reading.then_some(body_size * MEASURE),
            reading,
        }
    }

    pub fn heading_size(&self, level: u8) -> f32 {
        self.heading_sizes[usize::from(level.clamp(1, 6))]
    }
}

/// A block's picture as a row draws it: decoded, or still on its way, and
/// the size it is shown at either way.
pub struct Shown<'a> {
    pub image: Option<&'a Arc<RenderImage>>,
    pub size: (u32, u32),
}
impl<'a> Shown<'a> {
    pub fn ready(image: &'a Arc<RenderImage>) -> Self {
        let size = image.size(0);
        Self {
            image: Some(image),
            size: (
                u32::try_from(size.width.0).unwrap_or(1),
                u32::try_from(size.height.0).unwrap_or(1),
            ),
        }
    }
}

pub fn block(
    block: &Block,
    index: usize,
    picture: Option<Shown<'_>>,
    selection: Option<inspector_text::Selection>,
    style: &MarkdownStyle,
    mono: &SharedString,
    pane: PaneId,
    entity: WeakEntity<crate::shell::Shell>,
) -> AnyElement {
    let id = ("inspect-block", index);
    let mut row = div().id(id).w_full().px(px(style.page_padding));
    row = if block.kind == BlockKind::Rule {
        row.py(px(style.section_spacing))
    } else if block.heading > 0 {
        row.pt(px(style.heading_top)).pb(px(style.heading_bottom))
    } else if block.code {
        row.py(px(style.section_spacing))
    } else if style.reading && (block.list_depth > 0 || block.quote) {
        // Reading mode keeps the lines of a list or a quotation together.
        row
    } else {
        row.py(px(style.block_spacing))
    };
    if block.quote {
        row = row.border_l_2().border_color(style.quote_border).pl_4();
    }
    let content = match &block.kind {
        // Only the middle differs: a rule inside a quote or a list item
        // keeps the border and the indent like any other block.
        BlockKind::Rule => div()
            .flex_1()
            .min_w_0()
            .h(px(1.))
            .bg(style.border)
            .into_any_element(),
        BlockKind::TableRow { cells, header } => {
            table_row(block, index, cells, *header, selection, style, pane, entity)
        }
        _ if block.code => code_box(block, index, selection, style, mono, pane, entity),
        BlockKind::Text => text_body(block, index, selection, style, pane, entity),
    };
    // The document's own margins (a letter, a verse, a definition) indent
    // the whole block, marker included.
    let indent = block.indent.max(0.) * style.body_size;
    let mut line = div()
        .flex()
        .flex_row()
        .items_start()
        .w_full()
        .min_w_0()
        .pl(px(indent));
    if block.list_depth > 0 {
        // The marker column is reserved for every block of the item, not only
        // the one that shows the bullet, so an item's later paragraphs and its
        // code line up under its first line.
        line = line
            .pl(px(
                indent + style.list_indent * (block.list_depth - 1) as f32
            ))
            .child(
                div()
                    .flex_none()
                    .min_w(px(style.list_indent))
                    .text_color(style.marker_color)
                    .child(block.marker.clone().unwrap_or_default()),
            );
    }
    row.child(line.child(content))
        .when_some(picture, |d, picture| {
            // A picture the page centred (a cover, a logo) stays in the middle;
            // its caption, the block's text, follows it there. Its box is the
            // size it is shown at before it is decoded too, so nothing moves
            // when it arrives: the width, no wider than the column, and the
            // height its proportions give.
            let (width, height) = picture.size;
            let frame = div()
                .flex_none()
                .w(px(width.max(1) as f32))
                .max_w_full()
                .map(|mut d| {
                    d.style().aspect_ratio = Some(width.max(1) as f32 / height.max(1) as f32);
                    d
                });
            let frame = match picture.image {
                Some(image) => frame.child(gpui::img(image.clone()).size_full()),
                None => frame.bg(style.placeholder),
            };
            d.child(
                div()
                    .w_full()
                    .flex()
                    .map(|d| match block.align {
                        Align::Start => d,
                        Align::Center => d.justify_center(),
                        Align::End => d.justify_end(),
                    })
                    .child(frame),
            )
        })
        .into_any_element()
}

fn text_body(
    block: &Block,
    index: usize,
    selection: Option<inspector_text::Selection>,
    style: &MarkdownStyle,
    pane: PaneId,
    entity: WeakEntity<crate::shell::Shell>,
) -> AnyElement {
    let text = inspector_text::render(block, index, selection, style, pane, entity);
    let mut body = div().flex_1().min_w_0().text_color(style.text);
    if block.heading > 0 {
        let size = style.heading_size(block.heading);
        body = body
            .text_size(px(size))
            .line_height(px(size * 1.25))
            .font_weight(FontWeight::SEMIBOLD);
        if block.heading <= 2 && style.heading_rule {
            body = body.border_b_1().border_color(style.border).pb_1();
        }
    } else {
        // A paragraph the document sets apart by its size (a title page
        // line, small print) keeps that proportion to the body.
        let size = style.body_size * block.size.unwrap_or(1.);
        body = body
            .text_size(px(size))
            .line_height(px(size * style.line_height));
    }
    body = match block.align {
        Align::Start => body,
        Align::Center => body.text_center(),
        Align::End => body.text_right(),
    };
    if block.quote {
        body = body.text_color(style.quote_text);
    }
    body.child(text).into_any_element()
}

fn table_row(
    block: &Block,
    index: usize,
    cells: &[std::ops::Range<usize>],
    header: bool,
    selection: Option<inspector_text::Selection>,
    style: &MarkdownStyle,
    pane: PaneId,
    entity: WeakEntity<crate::shell::Shell>,
) -> AnyElement {
    let mut row = div()
        .flex_1()
        .min_w_0()
        .flex()
        .border_b_1()
        .border_color(style.border)
        .when(header, |d| {
            d.font_weight(FontWeight::BOLD).bg(style.table_header_bg)
        });
    // The cells are ranges of the row's own text, so each column draws its
    // stretch of it and keeps the emphasis, code and links written there.
    let id: SharedString = format!("inspection-cell-{index}").into();
    for (column, cell) in cells.iter().enumerate() {
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .px_2()
                .py_1()
                .when(column + 1 < cells.len(), |d| {
                    d.border_r_1().border_color(style.border)
                })
                .text_size(px(style.body_size))
                .child(inspector_text::part(
                    block,
                    index,
                    (id.clone(), column).into(),
                    cell.clone(),
                    selection,
                    style,
                    pane,
                    entity.clone(),
                )),
        );
    }
    row.into_any_element()
}

fn code_box(
    block: &Block,
    index: usize,
    selection: Option<inspector_text::Selection>,
    style: &MarkdownStyle,
    mono: &SharedString,
    pane: PaneId,
    entity: WeakEntity<crate::shell::Shell>,
) -> AnyElement {
    let text = inspector_text::render(block, index, selection, style, pane, entity);
    let mut container = div()
        .flex_1()
        .min_w_0()
        .bg(style.code_bg)
        .rounded_md()
        .border_1()
        .border_color(style.border)
        .p(px(style.code_padding))
        .font_family(mono.clone())
        .text_size(px(style.code_size))
        .line_height(px(style.code_size * style.line_height));
    if let Some(language) = &block.language {
        container = container.child(
            div()
                .pb_1()
                .text_size(px((style.body_size - 4.).max(8.)))
                .text_color(style.text_muted)
                .child(language.clone()),
        );
    }
    container.child(text).into_any_element()
}
