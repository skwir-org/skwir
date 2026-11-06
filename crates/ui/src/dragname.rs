//! A dragged thing's name, under its icon: in the window
//! (`dnd::DragPreview`), and in the picture the compositor shows once a
//! drag of files leaves it (`dnd::FilesIcon`), the same either way.
//!
//! Written as the grid writes names: centred, up to `LINES` lines, a pill
//! at most `WIDTH` wide (at 100 %, growing with the interface's text), a
//! longer name keeping its first lines whole and its end on the last, an
//! ellipsis between (gpui patch 0037); a short one on a pill as wide as it.
//! gpui lays a drag's view out at its least width, where text wraps at each
//! chance, so the text's box is given its width: the name's own, measured
//! with the text system, in whole pixels and one more (laid out, an edge
//! lands on a whole device pixel, and a box a hair narrower wraps the
//! name's last word), up to the most (`Name::of`).
//!
//! The compositor's picture is drawn here, glyph by glyph through the text
//! system (gpui patch 0048), the name laid out as gpui's text element lays
//! it out in the window, at the same width (`truncate_wrapped_line`, then
//! `shape_text`, each row centred as gpui centres it), so it breaks where
//! the window's does; at the window's scale, which the compositor shows at
//! its logical size (patch 0049): its text as sharp as the window's.

use gpui::{App, ExternalDragIcon, Pixels, SharedString, Window, div, point, prelude::*, px};
use smallvec::SmallVec;

use crate::dragicon::Canvas;
use crate::sizes::{Text, TextSizes, painted, sized};
use crate::theme::Theme;

/// The widest a name's pill is, at 100 %: two and a half times the icon.
pub const WIDTH: f32 = 160.;
/// The most lines a name takes.
pub const LINES: usize = 3;
/// A line of a name, at 100 %, as the grid's names.
const LINE: f32 = 17.;
/// Around the text on its pill, at 100 %.
const PAD_X: f32 = 8.;
const PAD_Y: f32 = 4.;
/// The pill's corners, at 100 %.
const RADIUS: f32 = 6.;
/// Between the icon and its name, at 100 %.
pub const GAP: f32 = 4.;

/// A length written at 100 %, in the window's pixels.
fn at(window: &Window, at_100: f32) -> Pixels {
    sized(at_100).to_pixels(window.rem_size())
}

/// The name's one run: the interface's font, in the color drawn on the
/// accent.
fn run(text: &str, theme: &Theme) -> gpui::TextRun {
    gpui::TextRun {
        len: text.len(),
        font: gpui::font(theme.font.clone()),
        color: theme.on_accent,
        background_color: None,
        underline: None,
        strikethrough: None,
        position: gpui::RunPosition::Baseline,
    }
}

/// A name as it is written: its text, and the width its lines are written
/// in, its own up to the pill's most.
pub struct Name {
    pub text: SharedString,
    inner: Pixels,
}

impl Name {
    pub fn of(text: impl Into<SharedString>, theme: &Theme, window: &Window) -> Self {
        let text = text.into();
        let own = window
            .text_system()
            .shape_line(
                text.clone(),
                painted(window, Text::Body),
                &[run(&text, theme)],
                None,
            )
            .width;
        let most = px(f32::from(at(window, WIDTH - 2. * PAD_X)).floor());
        // In whole pixels and one more: laid out, an edge lands on a whole
        // device pixel, and a box a hair short of the text's width wraps
        // its last word ("3" over "items").
        let inner = px(f32::from(own).ceil() + 1.).min(most);
        Name { text, inner }
    }

    /// The pill's width.
    pub fn width(&self, window: &Window) -> Pixels {
        self.inner + at(window, 2. * PAD_X)
    }

    /// The name on its pill, as the window draws it: the text's box is
    /// given its width, the pill holds it with its padding.
    pub fn element(&self, theme: &Theme) -> gpui::Div {
        div()
            .flex_none()
            .px(sized(PAD_X))
            .py(sized(PAD_Y))
            .rounded(sized(RADIUS))
            .bg(theme.accent)
            .text_color(theme.on_accent)
            .font_family(theme.font.clone())
            .text_body()
            .line_height(sized(LINE))
            .text_center()
            .shadow_md()
            .child(
                div()
                    .w(self.inner)
                    .overflow_hidden()
                    .text_ellipsis_middle()
                    .line_clamp(LINES)
                    .child(self.text.clone()),
            )
    }

    /// The name laid out as the window lays it out: cut to `LINES` in
    /// the middle of the last (`truncate_wrapped_line`), then shaped and
    /// wrapped at the width given (`shape_text`), as gpui's text element
    /// does, so that it breaks where the window's breaks.
    fn laid_out(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &App,
    ) -> SmallVec<[gpui::WrappedLine; 1]> {
        let mut wrapper = cx
            .text_system()
            .line_wrapper(gpui::font(theme.font.clone()), painted(window, Text::Body));
        let runs = [run(&self.text, theme)];
        let (cut, runs) = wrapper.truncate_wrapped_line(
            self.text.clone(),
            self.inner,
            LINES,
            "…",
            &runs,
            gpui::TruncateFrom::Middle,
        );
        window
            .text_system()
            .shape_text(
                cut,
                painted(window, Text::Body),
                &runs,
                Some(self.inner),
                Some(LINES),
            )
            .unwrap_or_default()
    }
}

/// A row of a wrapped line: where it starts and ends along the line, and
/// whether a glyph is on it.
struct Row {
    from: gpui::WrapBoundary,
    to: Option<gpui::WrapBoundary>,
    start: Pixels,
    end: Pixels,
}

/// The rows `line` wraps to, at most `most`.
fn rows(line: &gpui::WrappedLine, most: usize) -> Vec<Row> {
    let layout = &line.unwrapped_layout;
    let x = |at: &gpui::WrapBoundary| layout.runs[at.run_ix].glyphs[at.glyph_ix].position.x;
    let mut rows = Vec::new();
    let mut from = gpui::WrapBoundary {
        run_ix: 0,
        glyph_ix: 0,
    };
    let mut start = px(0.);
    for boundary in &line.wrap_boundaries {
        rows.push(Row {
            from,
            to: Some(*boundary),
            start,
            end: x(boundary),
        });
        from = *boundary;
        start = x(boundary);
    }
    rows.push(Row {
        from,
        to: None,
        start,
        end: layout.width,
    });
    rows.truncate(most);
    rows
}

/// The picture the compositor shows once a drag leaves the window: `icon`
/// with the name under it on its pill, as the window draws them, at the
/// window's scale; its hotspot the icon's.
pub fn picture(
    icon: &ExternalDragIcon,
    name: &Name,
    theme: &Theme,
    window: &Window,
    cx: &App,
) -> ExternalDragIcon {
    let scale = window.scale_factor();
    // A logical length in the picture's pixels.
    let device = |length: Pixels| (f32::from(length) * scale).round() as i32;
    let icon_scale = if icon.scale > 0. { icon.scale } else { 1. };
    let (icon_w, icon_h) = (
        px(icon.width as f32 / icon_scale),
        px(icon.height as f32 / icon_scale),
    );
    let laid = name.laid_out(theme, window, cx);
    let mut lines = Vec::new();
    for wrapped in &laid {
        let left = LINES.saturating_sub(lines.len());
        lines.extend(rows(wrapped, left).into_iter().map(|row| (wrapped, row)));
    }
    let line = at(window, LINE);
    let (pill_w, pill_h) = (
        name.width(window),
        line * lines.len() as f32 + at(window, 2. * PAD_Y),
    );
    let gap = at(window, GAP);
    let width = icon_w.max(pill_w);
    let height = icon_h + gap + pill_h;
    let mut canvas = Canvas::new(device(width).max(1) as u32, device(height).max(1) as u32);
    let icon_x = device((width - icon_w) / 2.);
    canvas.blit_fit(
        &crate::dragicon::Bitmap::of_icon(icon),
        icon_x,
        0,
        device(icon_w),
        device(icon_h),
    );
    let (pill_x, pill_y) = ((width - pill_w) / 2., icon_h + gap);
    canvas.round_rect(
        device(pill_x),
        device(pill_y),
        device(pill_w),
        device(pill_h),
        device(at(window, RADIUS)),
        rgba(theme.accent),
    );
    let color = rgba(theme.on_accent);
    let text = cx.text_system();
    for (index, (wrapped, row)) in lines.into_iter().enumerate() {
        let layout = &wrapped.unwrapped_layout;
        // Each row centred on the text's box, its width to the next row's
        // start, and on the baseline, as gpui paints them: the room the
        // line leaves shared above and below.
        let left = pill_x + at(window, PAD_X) + (name.inner - (row.end - row.start)) / 2.;
        let top = pill_y + at(window, PAD_Y) + line * index as f32;
        let baseline = top + (line - (layout.ascent + layout.descent)) / 2. + layout.ascent;
        for (run_ix, shaped_run) in layout.runs.iter().enumerate() {
            for (glyph_ix, glyph) in shaped_run.glyphs.iter().enumerate() {
                let at_glyph = gpui::WrapBoundary { run_ix, glyph_ix };
                if at_glyph < row.from || row.to.is_some_and(|to| at_glyph >= to) {
                    continue;
                }
                let params = gpui::RenderGlyphParams {
                    font_id: shaped_run.font_id,
                    glyph_id: glyph.id,
                    font_size: shaped_run.font_size(painted(window, Text::Body)),
                    subpixel_variant: point(0, 0),
                    scale_factor: scale,
                    is_emoji: glyph.is_emoji,
                    subpixel_rendering: false,
                    dilation: 0,
                };
                let Ok(bounds) = text.raster_bounds(&params) else {
                    continue;
                };
                if bounds.size.width.0 <= 0 || bounds.size.height.0 <= 0 {
                    continue;
                }
                let Ok((size, pixels)) = text.rasterize_glyph(&params) else {
                    continue;
                };
                canvas.glyph(
                    device(left + glyph.position.x - row.start) + bounds.origin.x.0,
                    device(baseline + glyph.position.y) + bounds.origin.y.0,
                    (size.width.0, size.height.0),
                    &pixels,
                    glyph.is_emoji,
                    color,
                );
            }
        }
    }
    let hotspot = |at: u32| (at as f32 / icon_scale * scale).round() as u32;
    let mut picture = canvas.into_icon(
        icon_x.max(0) as u32 + hotspot(icon.hotspot_x),
        hotspot(icon.hotspot_y),
    );
    picture.scale = scale;
    picture
}

/// `color` as straight-alpha RGBA8.
fn rgba(color: gpui::Hsla) -> [u8; 4] {
    let color = color.to_rgb();
    [color.r, color.g, color.b, color.a].map(|c| (c.clamp(0., 1.) * 255.).round() as u8)
}
