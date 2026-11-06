//! A dragged thing's name, under its icon: in the window
//! (`dnd::DragPreview`), and in the picture the compositor shows once a
//! drag of files leaves it (`dnd::FilesIcon`), the same either way.
//!
//! Written as the grid writes names: centred, up to `LINES` lines, a pill
//! at most `WIDTH` wide (at 100 %, growing with the interface's text), a
//! longer name keeping its first lines whole and its end on the last, an
//! ellipsis between (gpui patch 0037); a short one on a pill as wide as it.
//! gpui lays a drag's view out at its least width, where text wraps at each
//! chance, so the width is given: the name's own, measured with the text
//! system, up to the most (`Name::of`).
//!
//! The compositor's picture is drawn here, glyph by glyph through the text
//! system (gpui patch 0048), the name broken and cut as gpui breaks and
//! cuts it in the window (`truncate_wrapped_line`, `wrap_line`), at the
//! window's scale, which the compositor shows at its logical size (patch
//! 0049): its text as sharp as the window's.

use gpui::{App, ExternalDragIcon, Pixels, SharedString, Window, div, point, prelude::*, px};

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
        let most = at(window, WIDTH - 2. * PAD_X);
        // In whole pixels: a width a hair short of the text's would wrap
        // its last word.
        let inner = px(f32::from(own).ceil()).min(most);
        Name { text, inner }
    }

    /// The pill's width.
    pub fn width(&self, window: &Window) -> Pixels {
        self.inner + at(window, 2. * PAD_X)
    }

    /// The name on its pill, as the window draws it.
    pub fn element(&self, theme: &Theme, window: &Window) -> gpui::Div {
        div()
            .flex_none()
            .w(self.width(window))
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
                    .w_full()
                    .overflow_hidden()
                    .text_ellipsis_middle()
                    .line_clamp(LINES)
                    .child(self.text.clone()),
            )
    }

    /// The lines it is written in, as gpui breaks and cuts it in the
    /// window: cut to `LINES` in the middle of the last, then wrapped at
    /// the width given, a newline ending a line.
    fn lines(&self, theme: &Theme, window: &Window, cx: &App) -> Vec<SharedString> {
        let mut wrapper = cx
            .text_system()
            .line_wrapper(gpui::font(theme.font.clone()), painted(window, Text::Body));
        let runs = [run(&self.text, theme)];
        let (cut, _) = wrapper.truncate_wrapped_line(
            self.text.clone(),
            self.inner,
            LINES,
            "…",
            &runs,
            gpui::TruncateFrom::Middle,
        );
        let mut lines = Vec::new();
        for paragraph in cut.split('\n') {
            let mut start = 0;
            let fragments = [gpui::LineFragment::text(paragraph)];
            // A boundary is where the next line starts; the space a line
            // broke at stays on it, and is not centred.
            for boundary in
                wrapper.wrap_line(&fragments, self.inner, gpui::IndentAdjustment::NoIndent)
            {
                lines.push(paragraph[start..boundary.ix].trim_end().to_owned().into());
                start = boundary.ix;
            }
            lines.push(paragraph[start..].trim_end().to_owned().into());
        }
        lines.truncate(LINES);
        lines
    }
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
    let lines = name.lines(theme, window, cx);
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
    for (row, line_text) in lines.into_iter().enumerate() {
        let shaped = window.text_system().shape_line(
            line_text.clone(),
            painted(window, Text::Body),
            &[run(&line_text, theme)],
            None,
        );
        // Centred on the pill, on the line's baseline as gpui places it:
        // the room the line leaves shared above and below.
        let left = pill_x + at(window, PAD_X) + (name.inner - shaped.width) / 2.;
        let top = pill_y + at(window, PAD_Y) + line * row as f32;
        let baseline = top + (line - (shaped.ascent + shaped.descent)) / 2. + shaped.ascent;
        for shaped_run in &shaped.runs {
            for glyph in &shaped_run.glyphs {
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
                    device(left + glyph.position.x) + bounds.origin.x.0,
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
