//! The image that follows the pointer while files are dragged: the picture
//! of the entry the drag took as the views draw it, the thumbnail the
//! window keeps of it or else its icon, plus a count badge for multi-item
//! drags. A drawn file or folder glyph stands in while the icon is drawn,
//! or when there is none.
//!
//! Everything here is plain RGBA8 with straight alpha; no text system is
//! involved, so digits come from a tiny built-in 3x5 font.

use gpui::{ExternalDragIcon, Hsla};
use skwir_core::EntryId;
use skwir_core::document::PixelOrder;
use skwir_core::thumbs::{Image, ThumbSize};
use skwir_core::workspace::PaneId;

use crate::backdrop::Picture;
use crate::icons::IconPixels;
use crate::shell::Shell;

/// Longest side of the icon.
pub const ICON_PX: u32 = 64;

/// The side of the picture on it, inside its margin.
const PICTURE_PX: u32 = ICON_PX - 4;

/// Pixels drawn on the icon: a thumbnail as the window keeps it (BGRA, the
/// order it draws), an icon drawn for the drag (RGBA).
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
        Some(Bitmap {
            width: u32::try_from(size.width.0).ok()?,
            height: u32::try_from(size.height.0).ok()?,
            bytes: picture.image.as_bytes(0)?,
            order: PixelOrder::Bgra,
        })
    }

    pub fn of_image(image: &'a Image) -> Self {
        Bitmap {
            width: image.width,
            height: image.height,
            bytes: &image.rgba,
            order: PixelOrder::Rgba,
        }
    }

    /// The pixel at `(x, y)`, RGBA.
    fn at(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width as usize + x) * 4;
        let p = &self.bytes[i..i + 4];
        match self.order {
            PixelOrder::Rgba => [p[0], p[1], p[2], p[3]],
            PixelOrder::Bgra => [p[2], p[1], p[0], p[3]],
        }
    }
}

/// A small RGBA canvas.
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

    fn blend(&mut self, x: i32, y: i32, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        let sa = u32::from(color[3]);
        if sa == 0 {
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

    /// Filled rounded rectangle.
    pub fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, color: [u8; 4]) {
        for py in y..y + h {
            for px in x..x + w {
                let dx = (px - x).min(x + w - 1 - px);
                let dy = (py - y).min(y + h - 1 - py);
                let inside = if dx < r && dy < r {
                    let cx = r - 1 - dx;
                    let cy = r - 1 - dy;
                    cx * cx + cy * cy <= r * r
                } else {
                    true
                };
                if inside {
                    self.blend(px, py, color);
                }
            }
        }
    }

    /// Filled circle.
    pub fn circle(&mut self, cx: i32, cy: i32, r: i32, color: [u8; 4]) {
        for py in cy - r..=cy + r {
            for px in cx - r..=cx + r {
                if (px - cx) * (px - cx) + (py - cy) * (py - cy) <= r * r {
                    self.blend(px, py, color);
                }
            }
        }
    }

    /// Draw `text` (digits only; other chars are skipped) with the 3x5 font at `scale`.
    pub fn digits(&mut self, x: i32, y: i32, text: &str, scale: i32, color: [u8; 4]) {
        let mut cx = x;
        for ch in text.chars() {
            let Some(d) = ch.to_digit(10) else { continue };
            let glyph = FONT_3X5[d as usize];
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
        let n = text.chars().filter(char::is_ascii_digit).count() as i32;
        if n == 0 { 0 } else { n * 4 * scale - scale }
    }

    /// Nearest-neighbour blit of `src` scaled to fit `w`x`h`, centered.
    pub fn blit_fit(&mut self, src: &Bitmap, x: i32, y: i32, w: i32, h: i32) {
        let (src_w, src_h) = (src.width, src.height);
        if src_w == 0 || src_h == 0 || src.bytes.len() < (src_w * src_h * 4) as usize {
            return;
        }
        let scale = (f64::from(w) / f64::from(src_w)).min(f64::from(h) / f64::from(src_h));
        let dw = (f64::from(src_w) * scale).round().max(1.0) as i32;
        let dh = (f64::from(src_h) * scale).round().max(1.0) as i32;
        let ox = x + (w - dw) / 2;
        let oy = y + (h - dh) / 2;
        for py in 0..dh {
            for px in 0..dw {
                let sx = (u64::from(px as u32) * u64::from(src_w) / u64::from(dw as u32)) as usize;
                let sy = (u64::from(py as u32) * u64::from(src_h) / u64::from(dh as u32)) as usize;
                self.blend(ox + px, oy + py, src.at(sx, sy));
            }
        }
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

const PAPER: [u8; 4] = [250, 250, 250, 255];
const PAPER_EDGE: [u8; 4] = [160, 160, 160, 255];
const FOLDER: [u8; 4] = [90, 150, 230, 255];
const FOLDER_TAB: [u8; 4] = [70, 125, 205, 255];
const BADGE: [u8; 4] = [230, 70, 70, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const LINE: [u8; 4] = [200, 200, 200, 255];

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

/// The count badge in the bottom-right corner.
pub fn badge(c: &mut Canvas, count: usize) {
    let text = count.to_string();
    let scale = 2;
    let tw = Canvas::digits_width(&text, scale);
    let r = (tw / 2 + 6).max(10);
    let (cx, cy) = (c.width as i32 - r - 2, c.height as i32 - r - 2);
    c.circle(cx, cy, r, BADGE);
    c.digits(cx - tw / 2, cy - 5 * scale / 2, &text, scale, WHITE);
}

/// The icon of a drag of `count` files: `picture` fitted, else a glyph
/// for a folder or a file; a badge when several.
pub fn drawn(picture: Option<&Bitmap>, is_dir: bool, count: usize) -> ExternalDragIcon {
    let mut c = Canvas::new(ICON_PX, ICON_PX);
    let side = PICTURE_PX as i32;
    let margin = (ICON_PX - PICTURE_PX) as i32 / 2;
    match picture {
        Some(picture) => c.blit_fit(picture, margin, margin, side, side),
        None if is_dir => folder_glyph(&mut c),
        None => file_glyph(&mut c),
    }
    if count > 1 {
        badge(&mut c, count);
    }
    c.into_icon(ICON_PX / 2, ICON_PX / 2)
}

/// What a drag of files shows, read as it starts: the entry it took as the
/// views draw it, and how many it carries.
#[derive(Default)]
pub struct DragLook {
    pub count: usize,
    pub is_dir: bool,
    /// The thumbnail the window keeps of it: what its view drew.
    pub thumb: Option<Picture>,
    /// Without one, its icon, drawn for the drag on GPUI's executor
    /// (`icons::pixels`): a glyph stands in until it arrives.
    pub icon: Option<IconPixels>,
}

impl DragLook {
    /// The icon as it can be drawn now: the thumbnail, else a glyph.
    pub fn drawn(&self) -> ExternalDragIcon {
        let picture = self.thumb.as_ref().and_then(Bitmap::of_picture);
        drawn(picture.as_ref(), self.is_dir, self.count)
    }
}

impl Shell {
    /// What a drag of `pane`'s selection shows: the entry `grabbed` (a row
    /// of a multi-selection pressed) or else the selection's first, its
    /// thumbnail if the window keeps one, else its icon, a shape drawn in
    /// `tint` as the views draw it.
    pub fn drag_look(&mut self, pane: PaneId, grabbed: Option<EntryId>, tint: Hsla) -> DragLook {
        let ws = self.app.workspace.read();
        let Some(pane) = ws.pane(pane) else {
            return DragLook::default();
        };
        let id = grabbed
            .filter(|id| pane.selection.contains(*id))
            .or_else(|| pane.selection.iter().next());
        let count = pane.selection.len();
        let Some((id, e)) = id.and_then(|id| Some((id, pane.entry(id)?))) else {
            return DragLook {
                count,
                ..DragLook::default()
            };
        };
        // Either size a view drew, the nearer the icon's side first.
        let thumb = self.thumbs.kept(e, &[ThumbSize::Normal, ThumbSize::Large]);
        let icon = thumb.is_none().then(|| {
            self.icons
                .pixels_for_entry(e, || pane.path_of(id), PICTURE_PX, tint)
        });
        DragLook {
            count,
            is_dir: e.is_dir(),
            thumb,
            icon,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_thumbnail_the_window_keeps_keeps_its_colors_on_the_icon() {
        let bgra = [30u8, 20, 10, 255].repeat(4);
        let kept = Bitmap {
            width: 2,
            height: 2,
            bytes: &bgra,
            order: PixelOrder::Bgra,
        };
        let icon = drawn(Some(&kept), false, 1);
        let centre = ((ICON_PX / 2 * ICON_PX + ICON_PX / 2) * 4) as usize;
        assert_eq!(&icon.rgba[centre..centre + 4], &[10, 20, 30, 255]);
        assert_eq!(&icon.rgba[..4], &[0, 0, 0, 0], "its margin shows through");
    }

    #[test]
    fn digits_render_every_glyph_and_out_of_bounds_is_clipped() {
        let mut c = Canvas::new(16, 8);
        c.digits(-5, -3, "0123456789", 1, WHITE);
        c.digits(100, 100, "42", 3, WHITE);
        assert_eq!(Canvas::digits_width("42", 2), 14);
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
