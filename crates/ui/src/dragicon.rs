//! The image that follows the pointer while files are dragged: the entries
//! the drag took as the views draw them, each the thumbnail the window
//! keeps of it or else its icon. One file is its picture alone; several
//! are drawn as `ui.drag_image` says (`Layout`): fanned out behind the one
//! taken, piled under it, side by side in quarters, the last saying how
//! many more, or the one taken alone, a badge counting them but in the
//! mosaic. A drawn file or folder glyph stands in while an icon is drawn,
//! or when there is none.
//!
//! Everything here is plain RGBA8 with straight alpha; no text system is
//! involved, so digits come from a tiny built-in 3x5 font.

use gpui::{ExternalDragIcon, Hsla, Window};
use skwir_core::EntryId;
use skwir_core::document::PixelOrder;
use skwir_core::thumbs::{Image, ThumbSize};
use skwir_core::workspace::PaneId;

use crate::backdrop::Picture;
use crate::icons::IconPixels;
use crate::shell::Shell;

/// Longest side of a lone picture's icon.
pub const ICON_PX: u32 = 64;

/// The side of a lone picture, inside its margin.
const PICTURE_PX: u32 = ICON_PX - 4;

/// Longest side of a card of the fan and the pile, a print's frame
/// included.
const CARD_PX: u32 = 56;

/// The white frame of a thumbnail on a card, as a print has.
const FRAME: u32 = 2;

/// Room around a card for its shadow, and how dark the shadow is where
/// it is darkest, against the card's edge.
const SHADOW: u32 = 3;
const SHADOW_ALPHA: f32 = 70.;

/// The mosaic's side, and the side of each of its quarters.
const MOSAIC_PX: u32 = 64;
const CELL_PX: u32 = 30;
const MOSAIC_CELLS: usize = 4;

/// How far the fan's cards behind lean, and how far below a card's
/// middle they turn about.
const FAN_DEGREES: f32 = 16.;
const FAN_PIVOT: f32 = 34.;

/// How far down and right each card of the pile lies under the one above.
const PILE_STEP: (f32, f32) = (7., 6.);

/// How many of the selection's first entries are looked through for ones
/// of another type, to show them beside the one taken.
const VARIETY_SCAN: usize = 64;

/// How a drag of several files is drawn (`ui.drag_image`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// The one taken in front of two more, fanned out behind it.
    #[default]
    Fan,
    /// The one taken on two more, each a little down and right.
    Pile,
    /// Up to four in quarters, the last saying how many more.
    Mosaic,
    /// The one taken alone.
    Single,
}

impl Layout {
    /// The setting's value; anything else is the default.
    pub fn read(value: Option<&str>) -> Self {
        match value {
            Some("pile") => Self::Pile,
            Some("mosaic") => Self::Mosaic,
            Some("single") => Self::Single,
            _ => Self::Fan,
        }
    }

    /// How many entries of a drag of `count` it draws.
    fn shows(self, count: usize) -> usize {
        match self {
            _ if count <= 1 => count,
            Self::Fan | Self::Pile => count.min(3),
            Self::Mosaic if count <= MOSAIC_CELLS => count,
            Self::Mosaic => MOSAIC_CELLS - 1,
            Self::Single => 1,
        }
    }
}

/// Pixels drawn on the icon: a thumbnail as the window keeps it (BGRA, the
/// order it draws), an icon drawn for the drag or a canvas (RGBA).
pub struct Bitmap<'a> {
    pub width: u32,
    pub height: u32,
    pub bytes: &'a [u8],
    pub order: PixelOrder,
}

impl<'a> Bitmap<'a> {
    /// A picture the window keeps.
    pub fn of_picture(picture: &'a Picture) -> Option<Self> {
        let size = picture.image.size(0);
        Self::checked(Bitmap {
            width: u32::try_from(size.width.0).ok()?,
            height: u32::try_from(size.height.0).ok()?,
            bytes: picture.image.as_bytes(0)?,
            order: PixelOrder::Bgra,
        })
    }

    pub fn of_image(image: &'a Image) -> Option<Self> {
        Self::checked(Bitmap {
            width: image.width,
            height: image.height,
            bytes: &image.rgba,
            order: PixelOrder::Rgba,
        })
    }

    /// `self` when it has pixels, as many as its size says.
    fn checked(self) -> Option<Self> {
        let len = u64::from(self.width) * u64::from(self.height) * 4;
        (len > 0 && self.bytes.len() as u64 >= len).then_some(self)
    }
}

/// A small RGBA canvas.
#[derive(Clone)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Canvas {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        }
    }

    fn bitmap(&self) -> Bitmap<'_> {
        Bitmap {
            width: self.width,
            height: self.height,
            bytes: &self.rgba,
            order: PixelOrder::Rgba,
        }
    }

    /// The pixel at `(x, y)`, transparent outside.
    #[cfg(test)]
    fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return [0; 4];
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    fn blend(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        let sa = u32::from(color[3]);
        if sa == 0 {
            return;
        }
        // Over nothing, or covering what is there: the color as it is.
        if sa == 255 || self.rgba[i + 3] == 0 {
            self.rgba[i..i + 4].copy_from_slice(&color);
            return;
        }
        let da = u32::from(self.rgba[i + 3]);
        let oa = sa + da * (255 - sa) / 255;
        let dst = &mut self.rgba[i..i + 3];
        for (d, s) in dst.iter_mut().zip(color.iter().take(3)) {
            let blended = u32::from(*s) * sa + u32::from(*d) * da * (255 - sa) / 255;
            *d = blended.checked_div(oa).unwrap_or(0).min(255) as u8;
        }
        self.rgba[i + 3] = oa.min(255) as u8;
    }

    /// `(x, y, w, h)` filled with `color`, what was there replaced.
    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
        let (x0, x1) = (x.max(0), (x + w).min(self.width as i32));
        if x0 >= x1 {
            return;
        }
        for py in y.max(0)..(y + h).min(self.height as i32) {
            let row = (py as u32 * self.width) as usize;
            let line = &mut self.rgba[(row + x0 as usize) * 4..(row + x1 as usize) * 4];
            for pixel in line.as_chunks_mut::<4>().0 {
                *pixel = color;
            }
        }
    }

    /// A soft shadow under the rectangle `(x, y, w, h)`, a pixel lower, on
    /// a canvas with nothing drawn there yet: each pixel darker the nearer
    /// it lies, out to `SHADOW`. What the rectangle covers is left alone,
    /// for it to be drawn there.
    fn shadow(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let reach = SHADOW as f32 + 0.5;
        let (left, right) = (x as f32, (x + w) as f32);
        let (top, bottom) = ((y + 1) as f32, (y + 1 + h) as f32);
        for py in
            (y + 1 - SHADOW as i32).max(0)..(y + 1 + h + SHADOW as i32).min(self.height as i32)
        {
            let my = py as f32 + 0.5;
            let dy = (top - my).max(my - bottom).max(0.);
            let covered_row = py >= y && py < y + h;
            for px in (x - SHADOW as i32).max(0)..(x + w + SHADOW as i32).min(self.width as i32) {
                if covered_row && px >= x && px < x + w {
                    continue;
                }
                let mx = px as f32 + 0.5;
                let distance = (left - mx).max(mx - right).max(0.).hypot(dy);
                if distance < reach {
                    let near = 1. - distance / reach;
                    let alpha = (SHADOW_ALPHA * near * near).round() as u8;
                    let i = ((py as u32 * self.width + px as u32) * 4) as usize;
                    self.rgba[i..i + 4].copy_from_slice(&[0, 0, 0, alpha]);
                }
            }
        }
    }

    /// `color` blended at `(x, y)` by `coverage`, how much of the pixel
    /// (0 to 1) the shape drawn covers: its edges come out smooth.
    fn cover(&mut self, x: i32, y: i32, color: [u8; 4], coverage: f32) {
        if coverage <= 0. {
            return;
        }
        let alpha = (f32::from(color[3]) * coverage.min(1.)).round() as u8;
        self.blend(x, y, [color[0], color[1], color[2], alpha]);
    }

    /// Filled rounded rectangle, its corners smooth.
    pub fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, color: [u8; 4]) {
        let radius = r as f32;
        for py in y..y + h {
            for px in x..x + w {
                let dx = (px - x).min(x + w - 1 - px);
                let dy = (py - y).min(y + h - 1 - py);
                let coverage = if dx < r && dy < r {
                    // From the middle of the corner's circle, `r` in from
                    // both edges, to the pixel's.
                    let (cx, cy) = (radius - dx as f32 - 0.5, radius - dy as f32 - 0.5);
                    covered(cx.hypot(cy), radius)
                } else {
                    1.
                };
                self.cover(px, py, color, coverage);
            }
        }
    }

    /// Filled circle about the pixel `(cx, cy)`, `r` pixels out from it,
    /// its edge smooth.
    pub fn circle(&mut self, cx: i32, cy: i32, r: i32, color: [u8; 4]) {
        let radius = r as f32 + 0.5;
        for py in cy - r - 1..=cy + r + 1 {
            for px in cx - r - 1..=cx + r + 1 {
                let distance = ((px - cx) as f32).hypot((py - cy) as f32);
                self.cover(px, py, color, covered(distance, radius));
            }
        }
    }

    /// Draw `text` (digits and `+`; other chars are skipped) with the 3x5
    /// font at `scale`.
    pub fn digits(&mut self, x: i32, y: i32, text: &str, scale: i32, color: [u8; 4]) {
        let mut cx = x;
        for glyph in text.chars().filter_map(glyph) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..3 {
                    if bits & (1 << (2 - col)) != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                self.blend(
                                    cx + col * scale + sx,
                                    y + row as i32 * scale + sy,
                                    color,
                                );
                            }
                        }
                    }
                }
            }
            cx += 4 * scale;
        }
    }

    /// Width in pixels of `text` at `scale`.
    pub fn digits_width(text: &str, scale: i32) -> i32 {
        let n = text.chars().filter_map(glyph).count() as i32;
        if n == 0 { 0 } else { n * 4 * scale - scale }
    }

    /// `crop` of `src` (x, y, width, height) drawn `width` x `height`: each
    /// pixel the average of those it covers, by their opacity, so a
    /// picture shrinks without its edges breaking up; one enlarged keeps
    /// its pixels whole. Summed across each row first, then down the rows
    /// summed: each pixel of the source is read once.
    fn resampled(src: &Bitmap, crop: [u32; 4], width: u32, height: u32) -> Canvas {
        let [cx, cy, cw, ch] = crop.map(|v| v as usize);
        let (width, height) = (width.max(1) as usize, height.max(1) as usize);
        // The source's columns (rows) each of the output's covers.
        let spans = |of: usize, from: usize, length: usize| -> Vec<(usize, usize)> {
            (0..of)
                .map(|i| {
                    let start = from + i * length / of;
                    let end =
                        (from + ((i + 1) * length).div_ceil(of)).clamp(start + 1, from + length);
                    (start, end)
                })
                .collect()
        };
        let columns = spans(width, cx, cw);
        let rows = spans(height, 0, ch);
        let (red, blue) = match src.order {
            PixelOrder::Rgba => (0, 2),
            PixelOrder::Bgra => (2, 0),
        };
        let bytes = src.bytes;
        let stride = src.width as usize * 4;
        let mut across = vec![[0u32; 4]; ch * width];
        for row in 0..ch {
            let line = (cy + row) * stride;
            for (column, &(start, end)) in columns.iter().enumerate() {
                let mut sum = [0u32; 4];
                let (mut i, stop) = (line + start * 4, line + end * 4);
                while i < stop {
                    let alpha = u32::from(bytes[i + 3]);
                    sum[0] += u32::from(bytes[i + red]) * alpha;
                    sum[1] += u32::from(bytes[i + 1]) * alpha;
                    sum[2] += u32::from(bytes[i + blue]) * alpha;
                    sum[3] += alpha;
                    i += 4;
                }
                across[row * width + column] = sum;
            }
        }
        let mut out = Canvas::new(width as u32, height as u32);
        for (y, &(start, end)) in rows.iter().enumerate() {
            for (x, &(left, right)) in columns.iter().enumerate() {
                let mut sum = [0u64; 4];
                let mut row = start;
                while row < end {
                    let part = across[row * width + x];
                    sum[0] += u64::from(part[0]);
                    sum[1] += u64::from(part[1]);
                    sum[2] += u64::from(part[2]);
                    sum[3] += u64::from(part[3]);
                    row += 1;
                }
                if sum[3] == 0 {
                    continue;
                }
                let covered = ((end - start) * (right - left)) as u64;
                let i = (y * width + x) * 4;
                out.rgba[i] = (sum[0] / sum[3]).min(255) as u8;
                out.rgba[i + 1] = (sum[1] / sum[3]).min(255) as u8;
                out.rgba[i + 2] = (sum[2] / sum[3]).min(255) as u8;
                out.rgba[i + 3] = (sum[3] / covered).min(255) as u8;
            }
        }
        out
    }

    /// `src` scaled to fit `w`x`h` keeping its shape, centered there.
    pub fn blit_fit(&mut self, src: &Bitmap, x: i32, y: i32, w: i32, h: i32) {
        let (dw, dh) = fit(src.width, src.height, w as u32, h as u32);
        let fitted = Canvas::resampled(src, [0, 0, src.width, src.height], dw, dh);
        self.blit(&fitted, x + (w - dw as i32) / 2, y + (h - dh as i32) / 2);
    }

    /// `src` drawn over this, its top-left at `(x, y)`.
    fn blit(&mut self, src: &Canvas, x: i32, y: i32) {
        let (sx0, sx1) = ((-x).max(0), (self.width as i32 - x).min(src.width as i32));
        let (sy0, sy1) = ((-y).max(0), (self.height as i32 - y).min(src.height as i32));
        for sy in sy0..sy1 {
            let row = (sy as u32 * src.width) as usize;
            for sx in sx0..sx1 {
                let i = (row + sx as usize) * 4;
                let color = [
                    src.rgba[i],
                    src.rgba[i + 1],
                    src.rgba[i + 2],
                    src.rgba[i + 3],
                ];
                self.blend(x + sx, y + sy, color);
            }
        }
    }

    /// `src` drawn over this turned by `angle` (radians, clockwise) about
    /// its point `pivot`, which lands at `at`; its edges smoothed.
    fn blit_turned(&mut self, src: &Canvas, pivot: (f32, f32), at: (f32, f32), angle: f32) {
        let (sin, cos) = angle.sin_cos();
        let corners = [
            (0., 0.),
            (src.width as f32, 0.),
            (0., src.height as f32),
            (src.width as f32, src.height as f32),
        ]
        .map(|(x, y)| {
            let (dx, dy) = (x - pivot.0, y - pivot.1);
            (at.0 + dx * cos - dy * sin, at.1 + dx * sin + dy * cos)
        });
        let low = |pick: fn(&(f32, f32)) -> f32| corners.iter().map(pick).fold(f32::MAX, f32::min);
        let high = |pick: fn(&(f32, f32)) -> f32| corners.iter().map(pick).fold(f32::MIN, f32::max);
        let x0 = (low(|c| c.0).floor() as i32).max(0);
        let y0 = (low(|c| c.1).floor() as i32).max(0);
        let x1 = (high(|c| c.0).ceil() as i32).min(self.width as i32);
        let y1 = (high(|c| c.1).ceil() as i32).min(self.height as i32);
        let (w, h) = (src.width as f32 + 1., src.height as f32 + 1.);
        for y in y0..y1 {
            for x in x0..x1 {
                // Back to where this pixel's middle comes from; the corners
                // of the box around it lie off the canvas.
                let (dx, dy) = (x as f32 + 0.5 - at.0, y as f32 + 0.5 - at.1);
                let u = pivot.0 + dx * cos + dy * sin;
                let v = pivot.1 - dx * sin + dy * cos;
                if u > -1. && v > -1. && u < w && v < h {
                    self.blend(x, y, src.sample(u, v));
                }
            }
        }
    }

    /// The color at `(u, v)`, pixels' middles at half units, between the
    /// four nearest by their opacity; transparent beyond the edge, so an
    /// edge drawn turned comes out smooth.
    fn sample(&self, u: f32, v: f32) -> [u8; 4] {
        let (x, y) = (u - 0.5, v - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        // The weights in 256ths: of what lies right of and below the point.
        let fx = ((x - x0) * 256.).round() as u32;
        let fy = ((y - y0) * 256.).round() as u32;
        let (x0, y0) = (x0 as i32, y0 as i32);
        let mut sum = [0u32; 4];
        let mut add = |px: i32, py: i32, weight: u32| {
            if weight == 0
                || px < 0
                || py < 0
                || px >= self.width as i32
                || py >= self.height as i32
            {
                return;
            }
            let i = ((py as u32 * self.width + px as u32) * 4) as usize;
            // Its alpha in 65536ths, times each channel, in 256ths.
            let a = u32::from(self.rgba[i + 3]) * weight;
            sum[0] += (u32::from(self.rgba[i]) * a) >> 8;
            sum[1] += (u32::from(self.rgba[i + 1]) * a) >> 8;
            sum[2] += (u32::from(self.rgba[i + 2]) * a) >> 8;
            sum[3] += a;
        };
        add(x0, y0, (256 - fx) * (256 - fy));
        add(x0 + 1, y0, fx * (256 - fy));
        add(x0, y0 + 1, (256 - fx) * fy);
        add(x0 + 1, y0 + 1, fx * fy);
        if sum[3] == 0 {
            return [0; 4];
        }
        let total = u64::from(sum[3]);
        [
            ((u64::from(sum[0]) << 8) / total).min(255) as u8,
            ((u64::from(sum[1]) << 8) / total).min(255) as u8,
            ((u64::from(sum[2]) << 8) / total).min(255) as u8,
            ((sum[3] + 32768) >> 16).min(255) as u8,
        ]
    }

    pub fn into_icon(self, hotspot_x: u32, hotspot_y: u32) -> ExternalDragIcon {
        ExternalDragIcon {
            width: self.width,
            height: self.height,
            rgba: self.rgba,
            hotspot_x,
            hotspot_y,
        }
    }
}

/// How much of a pixel whose middle lies `distance` from a circle's lies
/// within its `radius`, from 0 to 1, read across the pixel's width.
fn covered(distance: f32, radius: f32) -> f32 {
    (radius - distance + 0.5).clamp(0., 1.)
}

/// The size `width` x `height` takes to fit `w` x `h`, its shape kept.
fn fit(width: u32, height: u32, w: u32, h: u32) -> (u32, u32) {
    let scale =
        (f64::from(w) / f64::from(width.max(1))).min(f64::from(h) / f64::from(height.max(1)));
    let side = |length: u32| ((f64::from(length) * scale).round() as u32).max(1);
    (side(width), side(height))
}

/// A character's glyph in the 3x5 font: a digit or `+`.
fn glyph(ch: char) -> Option<[u8; 5]> {
    match ch {
        '+' => Some(PLUS_3X5),
        _ => ch.to_digit(10).map(|d| FONT_3X5[d as usize]),
    }
}

/// 3x5 digit glyphs, one row per byte, most significant of the low 3 bits on the left.
const FONT_3X5: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

const PLUS_3X5: [u8; 5] = [0b000, 0b010, 0b111, 0b010, 0b000];

const PAPER: [u8; 4] = [250, 250, 250, 255];
const PAPER_EDGE: [u8; 4] = [160, 160, 160, 255];
const FOLDER: [u8; 4] = [90, 150, 230, 255];
const FOLDER_TAB: [u8; 4] = [70, 125, 205, 255];
const BADGE: [u8; 4] = [230, 70, 70, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const LINE: [u8; 4] = [200, 200, 200, 255];
/// The mosaic's last quarter, saying how many more.
const MORE: [u8; 4] = [95, 107, 122, 255];

/// A generic file glyph: a sheet with a folded corner and text lines.
pub fn file_glyph(c: &mut Canvas) {
    c.round_rect(14, 6, 36, 52, 3, PAPER_EDGE);
    c.round_rect(15, 7, 34, 50, 3, PAPER);
    for (i, w) in [22, 26, 18, 24].iter().enumerate() {
        c.round_rect(20, 22 + i as i32 * 8, *w, 3, 1, LINE);
    }
}

/// A generic folder glyph.
pub fn folder_glyph(c: &mut Canvas) {
    c.round_rect(8, 14, 24, 8, 3, FOLDER_TAB);
    c.round_rect(8, 18, 48, 34, 4, FOLDER);
}

/// The glyph for a folder or a file, `ICON_PX` square.
fn glyph_canvas(is_dir: bool) -> Canvas {
    let mut c = Canvas::new(ICON_PX, ICON_PX);
    if is_dir {
        folder_glyph(&mut c);
    } else {
        file_glyph(&mut c);
    }
    c
}

/// The radius of the count badge for `count`, and its text.
fn badge_size(count: usize) -> (i32, String) {
    let text = count.to_string();
    let tw = Canvas::digits_width(&text, 2);
    ((tw / 2 + 6).max(10), text)
}

/// The count badge centered at `(cx, cy)`.
fn badge_at(c: &mut Canvas, cx: i32, cy: i32, count: usize) {
    let (r, text) = badge_size(count);
    let scale = 2;
    let tw = Canvas::digits_width(&text, scale);
    c.circle(cx, cy, r, BADGE);
    c.digits(cx - tw / 2, cy - 5 * scale / 2, &text, scale, WHITE);
}

/// The count badge in the bottom-right corner.
pub fn badge(c: &mut Canvas, count: usize) {
    let (r, _) = badge_size(count);
    let (cx, cy) = (c.width as i32 - r - 2, c.height as i32 - r - 2);
    badge_at(c, cx, cy, count);
}

/// What a drawn entry shows: the thumbnail the window keeps, its icon, or
/// the glyph standing in for it.
enum Face<'a> {
    Thumb(Bitmap<'a>),
    Icon(Bitmap<'a>),
    Glyph(bool),
}

impl Face<'_> {
    /// What `layout` lays out of it, kept until it changes: the whole
    /// image of `Single`, a card of the fan and the pile, a quarter of the
    /// mosaic.
    fn piece(&self, layout: Layout) -> Canvas {
        match layout {
            Layout::Single => self.alone(),
            Layout::Fan | Layout::Pile => self.card(),
            Layout::Mosaic => self.cell(CELL_PX),
        }
    }

    /// The image of it alone, `ICON_PX` square: its picture fitted in the
    /// margin, else a glyph.
    fn alone(&self) -> Canvas {
        let mut c = Canvas::new(ICON_PX, ICON_PX);
        let side = PICTURE_PX as i32;
        let margin = (ICON_PX - PICTURE_PX) as i32 / 2;
        match self {
            Face::Thumb(picture) | Face::Icon(picture) => {
                c.blit_fit(picture, margin, margin, side, side);
            }
            Face::Glyph(true) => folder_glyph(&mut c),
            Face::Glyph(false) => file_glyph(&mut c),
        }
        c
    }

    /// Drawn on a card of the fan or the pile, in the room around it for
    /// its shadow (`SHADOW`): a thumbnail as a print, in a white frame on
    /// a soft shadow; an icon or a glyph as it is.
    fn card(&self) -> Canvas {
        let s = SHADOW as i32;
        match self {
            Face::Thumb(picture) => {
                let inner = CARD_PX - 2 * FRAME;
                let (w, h) = fit(picture.width, picture.height, inner, inner);
                let (fw, fh) = ((w + 2 * FRAME) as i32, (h + 2 * FRAME) as i32);
                let mut c = Canvas::new(fw as u32 + 2 * SHADOW, fh as u32 + 2 * SHADOW);
                c.shadow(s, s, fw, fh);
                c.fill(s, s, fw, fh, WHITE);
                let print = Canvas::resampled(picture, [0, 0, picture.width, picture.height], w, h);
                c.blit(&print, s + FRAME as i32, s + FRAME as i32);
                c
            }
            Face::Icon(icon) => Self::fitted(icon, CARD_PX),
            Face::Glyph(is_dir) => Self::fitted(&glyph_canvas(*is_dir).bitmap(), CARD_PX),
        }
    }

    /// `src` fitted within `side`, in the room for a shadow it does not
    /// cast.
    fn fitted(src: &Bitmap, side: u32) -> Canvas {
        let (w, h) = fit(src.width, src.height, side, side);
        let mut c = Canvas::new(w + 2 * SHADOW, h + 2 * SHADOW);
        let drawn = Canvas::resampled(src, [0, 0, src.width, src.height], w, h);
        c.blit(&drawn, SHADOW as i32, SHADOW as i32);
        c
    }

    /// Drawn in a quarter of the mosaic, `side` square: a thumbnail
    /// filling it, cropped to its middle; an icon or a glyph within it.
    fn cell(&self, side: u32) -> Canvas {
        let mut c = Canvas::new(side, side);
        match self {
            Face::Thumb(picture) => {
                let (w, h) = (picture.width, picture.height);
                let crop = if w > h {
                    [(w - h) / 2, 0, h, h]
                } else {
                    [0, (h - w) / 2, w, w]
                };
                c.blit(&Canvas::resampled(picture, crop, side, side), 0, 0);
            }
            Face::Icon(icon) => c.blit_fit(icon, 0, 0, side as i32, side as i32),
            Face::Glyph(is_dir) => {
                let glyph = glyph_canvas(*is_dir);
                c.blit_fit(&glyph.bitmap(), 0, 0, side as i32, side as i32);
            }
        }
        c
    }
}

/// A card of the fan or the pile where it lies: the middle of its card,
/// before it turns, and how far it turns about the point `FAN_PIVOT`
/// below that.
struct Placed<'a> {
    card: &'a Canvas,
    middle: (f32, f32),
    angle: f32,
}

impl Placed<'_> {
    /// Its point that lands at its middle, on its canvas, and where its
    /// pivot is, there and on the drag's image laid around `(0, 0)`.
    fn pivot(&self) -> ((f32, f32), (f32, f32)) {
        let (w, h) = (self.card.width as f32, self.card.height as f32);
        (
            (w / 2., h / 2. + FAN_PIVOT),
            (self.middle.0, self.middle.1 + FAN_PIVOT),
        )
    }

    /// The corners of its canvas where it lands.
    fn corners(&self) -> [(f32, f32); 4] {
        let (from, to) = self.pivot();
        let (sin, cos) = self.angle.sin_cos();
        let (w, h) = (self.card.width as f32, self.card.height as f32);
        [(0., 0.), (w, 0.), (0., h), (w, h)].map(|(x, y)| {
            let (dx, dy) = (x - from.0, y - from.1);
            (to.0 + dx * cos - dy * sin, to.1 + dx * sin + dy * cos)
        })
    }
}

/// The image of one entry, or of several as `Single` draws them: its
/// image alone (`Face::alone`), a badge when `count` is more than one.
fn single(alone: &Canvas, count: usize) -> ExternalDragIcon {
    let mut c = alone.clone();
    if count > 1 {
        badge(&mut c, count);
    }
    let (x, y) = (c.width / 2, c.height / 2);
    c.into_icon(x, y)
}

/// The first card in front, those after it behind: fanned out to either
/// side (`fan`) or each a step down and right (else), the badge on the
/// front card's corner. The pointer holds the front card's middle.
fn deck(cards: &[&Canvas], count: usize, fan: bool) -> ExternalDragIcon {
    let lean = FAN_DEGREES.to_radians();
    let mut placed: Vec<Placed> = cards
        .iter()
        .enumerate()
        .map(|(i, card)| {
            let (middle, angle) = match (fan, i, cards.len()) {
                (_, 0, _) => ((0., 0.), 0.),
                (true, _, 2) => ((0., 0.), lean),
                (true, 1, _) => ((0., 0.), -lean),
                (true, _, _) => ((0., 0.), lean),
                (false, i, _) => {
                    let step = i as f32;
                    ((PILE_STEP.0 * step, PILE_STEP.1 * step), 0.)
                }
            };
            Placed {
                card,
                middle,
                angle,
            }
        })
        .collect();
    // Drawn from the back.
    placed.reverse();
    let front = placed.last().map_or((0., 0.), |p| {
        let s = SHADOW as f32;
        (
            (p.card.width as f32) / 2. - s,
            (p.card.height as f32) / 2. - s,
        )
    });
    let (radius, _) = badge_size(count);
    let badge_middle = (front.0 - 3., front.1 - 3.);
    let r = radius as f32 + 1.;
    let mut low = (badge_middle.0 - r, badge_middle.1 - r);
    let mut high = (badge_middle.0 + r, badge_middle.1 + r);
    for corner in placed.iter().flat_map(Placed::corners) {
        low = (low.0.min(corner.0), low.1.min(corner.1));
        high = (high.0.max(corner.0), high.1.max(corner.1));
    }
    let origin = (-low.0.floor(), -low.1.floor());
    let mut c = Canvas::new(
        (high.0.ceil() + origin.0) as u32,
        (high.1.ceil() + origin.1) as u32,
    );
    for p in &placed {
        let (from, to) = p.pivot();
        let to = (to.0 + origin.0, to.1 + origin.1);
        if p.angle == 0. {
            let left = to.0 - from.0;
            let top = to.1 - from.1;
            c.blit(p.card, left.round() as i32, top.round() as i32);
        } else {
            c.blit_turned(p.card, from, to, p.angle);
        }
    }
    let at = |v: f32, o: f32| (v + o).round() as i32;
    badge_at(
        &mut c,
        at(badge_middle.0, origin.0),
        at(badge_middle.1, origin.1),
        count,
    );
    c.into_icon(origin.0 as u32, origin.1 as u32)
}

/// Up to four quarters; with more entries than that, three and a last
/// quarter saying how many more. Two lie side by side, three as a pyramid.
fn mosaic(cells: &[&Canvas], count: usize) -> ExternalDragIcon {
    let mut c = Canvas::new(MOSAIC_PX, MOSAIC_PX);
    let (near, far, mid) = (
        1,
        (MOSAIC_PX - CELL_PX - 1) as i32,
        ((MOSAIC_PX - CELL_PX) / 2) as i32,
    );
    let quarters = [(near, near), (far, near), (near, far), (far, far)];
    let more = count > cells.len();
    let places: &[(i32, i32)] = match cells.len() + usize::from(more) {
        2 => &[(near, mid), (far, mid)],
        3 => &[(near, near), (far, near), (mid, far)],
        _ => &quarters,
    };
    for (cell, &(x, y)) in cells.iter().zip(places) {
        c.blit(cell, x, y);
    }
    if more && let Some(&(x, y)) = places.get(cells.len()) {
        let side = CELL_PX as i32;
        c.round_rect(x, y, side, side, 4, MORE);
        let text = format!("+{}", count - cells.len());
        let room = side - 4;
        if let Some(scale) = [2, 1]
            .into_iter()
            .find(|&s| Canvas::digits_width(&text, s) <= room)
        {
            let tw = Canvas::digits_width(&text, scale);
            c.digits(
                x + (side - tw) / 2,
                y + (side - 5 * scale) / 2,
                &text,
                scale,
                WHITE,
            );
        } else {
            // Too many to say: an ellipsis.
            for dot in -1..=1 {
                c.circle(x + side / 2 + dot * 6, y + side / 2, 2, WHITE);
            }
        }
    }
    c.into_icon(MOSAIC_PX / 2, MOSAIC_PX / 2)
}

/// What a drag of files shows, read as it starts: the entries it took as
/// the views draw them, how many it carries, and how they are laid out.
#[derive(Default)]
pub struct DragLook {
    pub count: usize,
    pub layout: Layout,
    /// What it draws, the entry taken first.
    pub cards: Vec<Card>,
}

/// An entry a drag draws.
#[derive(Default)]
pub struct Card {
    pub is_dir: bool,
    /// The thumbnail the window keeps of it: what its view drew.
    pub thumb: Option<Picture>,
    /// Without one, its icon while it is drawn for the drag on GPUI's
    /// executor (`icons::pixels`): a glyph stands in until it arrives.
    pub coming: Option<IconPixels>,
    /// Its icon, drawn.
    pub icon: Option<Image>,
    /// What the layout lays out of it, once drawn (`Face::piece`): drawn
    /// again only when its icon arrives.
    piece: Option<Canvas>,
}

impl Card {
    fn face(&self) -> Face<'_> {
        let thumb = self.thumb.as_ref().and_then(Bitmap::of_picture);
        let icon = || self.icon.as_ref().and_then(Bitmap::of_image);
        match (thumb, icon()) {
            (Some(thumb), _) => Face::Thumb(thumb),
            (None, Some(icon)) => Face::Icon(icon),
            (None, None) => Face::Glyph(self.is_dir),
        }
    }
}

impl DragLook {
    /// The image as it can be drawn now: each entry's thumbnail, else its
    /// icon once drawn, else a glyph. What an entry gives the layout is
    /// drawn once and kept, until its icon arrives.
    pub fn drawn(&mut self) -> ExternalDragIcon {
        let layout = if self.cards.len() > 1 {
            self.layout
        } else {
            Layout::Single
        };
        let shown = if layout == Layout::Single {
            1
        } else {
            self.cards.len()
        };
        for card in self.cards.iter_mut().take(shown) {
            if card.piece.is_none() {
                let piece = card.face().piece(layout);
                card.piece = Some(piece);
            }
        }
        let pieces: Vec<&Canvas> = self
            .cards
            .iter()
            .take(shown)
            .filter_map(|c| c.piece.as_ref())
            .collect();
        match (layout, pieces.as_slice()) {
            (_, []) => single(&Face::Glyph(false).alone(), self.count),
            (Layout::Single, [alone, ..]) => single(alone, self.count),
            (Layout::Fan, cards) => deck(cards, self.count, true),
            (Layout::Pile, cards) => deck(cards, self.count, false),
            (Layout::Mosaic, cells) => mosaic(cells, self.count),
        }
    }

    /// Take the icons drawn since it was last asked: whether one arrived.
    /// Asked while drawing, so that the view drawing is drawn again when
    /// one does.
    pub fn take_icons(&mut self, window: &mut Window, cx: &mut gpui::App) -> bool {
        let mut arrived = false;
        for card in &mut self.cards {
            let Some(coming) = &card.coming else {
                continue;
            };
            if let Some(icon) = crate::icons::pixels(coming, window, cx) {
                card.coming = None;
                if icon.is_some() {
                    arrived = true;
                    card.icon = icon;
                    card.piece = None;
                }
            }
        }
        arrived
    }
}

impl Shell {
    /// What a drag of `pane`'s selection shows, laid out as `ui.drag_image`
    /// says: the entry `grabbed` (a row of a multi-selection pressed) or
    /// else the selection's first, and as many more as the layout draws,
    /// one of each other type first. Each is its thumbnail if the window
    /// keeps one, else its icon, a shape drawn in `tint` as the views draw
    /// it.
    pub fn drag_look(&mut self, pane: PaneId, grabbed: Option<EntryId>, tint: Hsla) -> DragLook {
        let layout = self.prefs.drag_image;
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(pane) else {
            return DragLook::default();
        };
        let count = pane.selection.len();
        let Some(front) = grabbed
            .filter(|id| pane.selection.contains(*id))
            .or_else(|| pane.selection.iter().next())
        else {
            return DragLook {
                count,
                layout,
                cards: Vec::new(),
            };
        };
        let wanted = layout.shows(count);
        let kind = |id: EntryId| {
            pane.entry(id)
                .map(|e| (e.is_dir(), skwir_platform::mime::of_entry(e).map(|m| m.0)))
        };
        let mut ids = vec![front];
        let mut kinds = vec![kind(front)];
        let mut alike = Vec::new();
        for id in pane
            .selection
            .iter()
            .filter(|id| *id != front)
            .take(VARIETY_SCAN)
        {
            if ids.len() == wanted {
                break;
            }
            let k = kind(id);
            if kinds.contains(&k) {
                alike.push(id);
            } else {
                kinds.push(k);
                ids.push(id);
            }
        }
        let missing = wanted.saturating_sub(ids.len());
        ids.extend(alike.into_iter().take(missing));
        // Fewer looked through than wanted: the next ones as they come.
        let missing = wanted.saturating_sub(ids.len());
        if missing > 0 {
            let more: Vec<EntryId> = pane
                .selection
                .iter()
                .filter(|id| !ids.contains(id))
                .take(missing)
                .collect();
            ids.extend(more);
        }
        let cards = ids
            .into_iter()
            .filter_map(|id| {
                let e = pane.entry(id)?;
                // Either size a view drew, the nearer a card's side first.
                let thumb = self.thumbs.kept(e, &[ThumbSize::Normal, ThumbSize::Large]);
                let coming = thumb.is_none().then(|| {
                    self.icons
                        .pixels_for_entry(e, || pane.path_of(id), PICTURE_PX, tint)
                });
                Some(Card {
                    is_dir: e.is_dir(),
                    thumb,
                    coming,
                    icon: None,
                    piece: None,
                })
            })
            .collect();
        DragLook {
            count,
            layout,
            cards,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-color picture, RGBA, `w` x `h`.
    fn plain(w: u32, h: u32, rgba: [u8; 4]) -> Image {
        Image {
            width: w,
            height: h,
            rgba: rgba.repeat((w * h) as usize).into(),
        }
    }

    /// A look of `count` entries, those drawn the icons given.
    fn look(layout: Layout, count: usize, icons: &[Image]) -> DragLook {
        DragLook {
            count,
            layout,
            cards: icons
                .iter()
                .map(|icon| Card {
                    icon: Some(icon.clone()),
                    ..Card::default()
                })
                .collect(),
        }
    }

    fn pixel(icon: &ExternalDragIcon, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * icon.width + x) * 4) as usize;
        [
            icon.rgba[i],
            icon.rgba[i + 1],
            icon.rgba[i + 2],
            icon.rgba[i + 3],
        ]
    }

    fn has(icon: &ExternalDragIcon, color: [u8; 4]) -> bool {
        icon.rgba.as_chunks::<4>().0.contains(&color)
    }

    const RED: [u8; 4] = [200, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 200, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 200, 255];

    #[test]
    fn the_setting_names_a_layout_and_says_how_many_it_draws() {
        assert_eq!(Layout::read(None), Layout::Fan);
        assert_eq!(Layout::read(Some("mosaic")), Layout::Mosaic);
        assert_eq!(Layout::read(Some("nonsense")), Layout::Fan);
        assert_eq!(Layout::Fan.shows(1), 1);
        assert_eq!(Layout::Fan.shows(30), 3);
        assert_eq!(Layout::Pile.shows(2), 2);
        assert_eq!(Layout::Mosaic.shows(4), 4);
        assert_eq!(
            Layout::Mosaic.shows(5),
            3,
            "the last quarter says how many more"
        );
        assert_eq!(Layout::Single.shows(30), 1);
    }

    #[test]
    fn one_file_is_its_picture_alone_whatever_the_layout() {
        let red = plain(4, 4, RED);
        let alone = look(Layout::Single, 1, std::slice::from_ref(&red)).drawn();
        for layout in [Layout::Fan, Layout::Pile, Layout::Mosaic] {
            let drawn = look(layout, 1, std::slice::from_ref(&red)).drawn();
            assert_eq!(drawn.rgba, alone.rgba, "{layout:?}");
        }
        assert_eq!((alone.width, alone.height), (ICON_PX, ICON_PX));
    }

    #[test]
    fn a_fan_holds_the_one_taken_with_the_others_leaning_out_behind() {
        let icons = [plain(8, 8, RED), plain(8, 8, GREEN), plain(8, 8, BLUE)];
        let fan = look(Layout::Fan, 30, &icons).drawn();
        assert!(fan.width > CARD_PX && fan.height > CARD_PX);
        // The pointer holds the front card's middle.
        assert_eq!(pixel(&fan, fan.hotspot_x, fan.hotspot_y), RED);
        // The cards behind show beside it, the left one top-left of it.
        let (x, y) = (fan.hotspot_x as usize, fan.hotspot_y as usize);
        let half = (CARD_PX / 2) as usize;
        let left = (0..x - half).any(|px| pixel(&fan, px as u32, (y - half / 2) as u32) == GREEN);
        let right = (x + half..fan.width as usize)
            .any(|px| pixel(&fan, px as u32, (y - half / 2) as u32) == BLUE);
        assert!(left && right, "both lean out");
        assert!(has(&fan, BADGE), "the badge counts them");
    }

    #[test]
    fn a_pile_lays_each_card_a_step_down_and_right() {
        let icons = [plain(8, 8, RED), plain(8, 8, GREEN), plain(8, 8, BLUE)];
        let pile = look(Layout::Pile, 3, &icons).drawn();
        let (x, y) = (pile.hotspot_x, pile.hotspot_y);
        let half = CARD_PX / 2;
        assert_eq!(pixel(&pile, x, y), RED);
        // Past the front card's left edge, nothing; past its right, the
        // others.
        assert_eq!(pixel(&pile, x - half - 2, y)[3], 0);
        // Past the badge on its corner, and the second card's, the third.
        let beyond = (x + half + 10, y + half + 10);
        assert_eq!(pixel(&pile, beyond.0, beyond.1), BLUE);
    }

    #[test]
    fn a_mosaic_says_how_many_more_in_its_last_quarter() {
        let icons = [plain(8, 8, RED), plain(8, 8, GREEN), plain(8, 8, BLUE)];
        let mosaic = look(Layout::Mosaic, 7, &icons).drawn();
        assert_eq!((mosaic.width, mosaic.height), (MOSAIC_PX, MOSAIC_PX));
        let middle = |(x, y): (u32, u32)| pixel(&mosaic, x + CELL_PX / 2, y + CELL_PX / 4);
        let far = MOSAIC_PX - CELL_PX - 1;
        assert_eq!(middle((1, 1)), RED);
        assert_eq!(middle((far, 1)), GREEN);
        assert_eq!(middle((1, far)), BLUE);
        assert_eq!(middle((far, far)), MORE);
        assert!(has(&mosaic, WHITE), "+4 written on it");
        assert!(!has(&mosaic, BADGE), "no badge besides");
        // Two side by side, nothing more to say.
        let two = look(Layout::Mosaic, 2, &icons[..2]).drawn();
        assert!(!has(&two, MORE));
    }

    #[test]
    fn a_thumbnail_the_window_keeps_keeps_its_colors_on_the_icon() {
        let bgra = [30u8, 20, 10, 255].repeat(4);
        let kept = Bitmap {
            width: 2,
            height: 2,
            bytes: &bgra,
            order: PixelOrder::Bgra,
        };
        let icon = single(&Face::Thumb(kept).alone(), 1);
        let centre = ((ICON_PX / 2 * ICON_PX + ICON_PX / 2) * 4) as usize;
        assert_eq!(&icon.rgba[centre..centre + 4], &[10, 20, 30, 255]);
        assert_eq!(&icon.rgba[..4], &[0, 0, 0, 0], "its margin shows through");
    }

    #[test]
    fn a_picture_shrinks_by_averaging_what_shows() {
        // Black and white columns, and a transparent pixel that must not
        // darken what it is averaged with.
        let rgba = [
            [0, 0, 0, 255],
            [255, 255, 255, 255],
            [255, 255, 255, 255],
            [9, 9, 9, 0],
        ]
        .concat();
        let src = Bitmap {
            width: 2,
            height: 2,
            bytes: &rgba,
            order: PixelOrder::Rgba,
        };
        let one = Canvas::resampled(&src, [0, 0, 2, 2], 1, 1);
        assert_eq!(one.pixel(0, 0), [170, 170, 170, 191]);
        let right = Canvas::resampled(&src, [1, 0, 1, 2], 1, 1);
        assert_eq!(right.pixel(0, 0), [255, 255, 255, 127]);
    }

    #[test]
    fn a_canvas_turned_lands_about_its_pivot() {
        let mut src = Canvas::new(4, 2);
        src.round_rect(0, 0, 4, 2, 0, RED);
        // Unturned, it lands where its pivot says.
        let mut c = Canvas::new(10, 10);
        c.blit_turned(&src, (0., 0.), (3., 3.), 0.);
        assert_eq!(c.pixel(3, 3), RED);
        assert_eq!(c.pixel(6, 4), RED);
        assert_eq!(c.pixel(7, 3)[3], 0);
        // A quarter turn clockwise about its top-left: down from there.
        let mut c = Canvas::new(10, 10);
        c.blit_turned(&src, (0., 0.), (5., 1.), std::f32::consts::FRAC_PI_2);
        assert_eq!(c.pixel(3, 3), RED);
        assert_eq!(c.pixel(4, 4), RED);
        assert_eq!(c.pixel(5, 3)[3], 0);
    }

    #[test]
    fn the_badge_has_a_smooth_edge() {
        let mut c = Canvas::new(40, 40);
        badge_at(&mut c, 20, 20, 3);
        let alphas: Vec<u8> = c.rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
        assert!(
            alphas.iter().any(|a| (1..255).contains(a)),
            "partly covered"
        );
        assert_eq!(c.pixel(20, 12), BADGE, "whole inside");
        assert_eq!(c.pixel(20, 30)[3], 255);
        assert_eq!(c.pixel(20, 32)[3], 0, "nothing past its edge");
        // As round every way: a pixel its edge crosses, turned about its
        // middle, is covered as much.
        let edge = |x: i32, y: i32| c.pixel(20 + x, 20 + y)[3];
        assert!((1..255).contains(&edge(7, 8)));
        for (x, y) in [(-7, -8), (8, -7), (-8, 7), (7, -8)] {
            assert_eq!(edge(x, y), edge(7, 8), "({x}, {y})");
        }
    }

    #[test]
    fn glyphs_and_badge_produce_opaque_pixels_within_bounds() {
        for draw in [file_glyph as fn(&mut Canvas), folder_glyph] {
            let mut c = Canvas::new(ICON_PX, ICON_PX);
            draw(&mut c);
            badge(&mut c, 12);
            assert_eq!(c.rgba.len(), (ICON_PX * ICON_PX * 4) as usize);
            assert!(
                c.rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 255),
                "something was drawn"
            );
            assert!(
                c.rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 0),
                "corners stay transparent"
            );
        }
    }

    #[test]
    fn digits_render_every_glyph_and_out_of_bounds_is_clipped() {
        let mut c = Canvas::new(16, 8);
        c.digits(-5, -3, "0123456789+", 1, WHITE);
        c.digits(100, 100, "42", 3, WHITE);
        assert_eq!(Canvas::digits_width("42", 2), 14);
        assert_eq!(Canvas::digits_width("+42", 2), 22);
        let mut c2 = Canvas::new(8, 8);
        c2.digits(0, 0, "8", 1, WHITE);
        let lit = c2
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0)
            .count();
        assert_eq!(lit, 13, "an 8 lights 13 of the 15 cells");
    }

    #[test]
    fn blit_fit_scales_and_centers() {
        let src = vec![255u8; 2 * 2 * 4];
        let src = Bitmap {
            width: 2,
            height: 2,
            bytes: &src,
            order: PixelOrder::Rgba,
        };
        let mut c = Canvas::new(10, 10);
        c.blit_fit(&src, 0, 0, 10, 6);
        // 2x2 scaled by 3 -> 6x6 centered horizontally in 10: columns 2..8.
        let px = |x: u32, y: u32| c.rgba[((y * 10 + x) * 4 + 3) as usize];
        assert_eq!(px(1, 0), 0);
        assert_eq!(px(2, 0), 255);
        assert_eq!(px(7, 5), 255);
        assert_eq!(px(8, 5), 0);
    }
}
