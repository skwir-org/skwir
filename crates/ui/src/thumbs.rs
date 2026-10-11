//! UI side of thumbnails: builds requests for visible entries, converts the
//! core's RGBA images into GPUI `RenderImage`s once (`Thumb`), and keeps a
//! byte-capped LRU that releases atlas textures on eviction.

use std::sync::Arc;
use std::time::SystemTime;

use gpui::{RenderImage, prelude::*};
use skwir_core::EntryId;
use skwir_core::entry::keys;
use skwir_core::lru::ByteLru;
use skwir_core::scheduler::Priority;
use skwir_core::thumbs::{Image, ThumbRequest, ThumbSize, ThumbState};
use smallvec::SmallVec;

use crate::shell::Shell;
use crate::theme::Theme;

/// What a view draws for an entry.
#[derive(Clone)]
pub enum ThumbView {
    Ready(Thumb),
    Pending,
    Unavailable,
}

/// A thumbnail as the window draws it.
#[derive(Clone)]
pub struct Thumb {
    pub image: Arc<RenderImage>,
    /// The luminance of what it draws (`ink`), when some of it lets its
    /// background through: a drawing, an icon, a model drawn on nothing.
    ink: Option<f32>,
}

/// The least contrast a picture's ink needs with what is behind it to
/// read: WCAG's for graphics (1.4.11).
const LEGIBLE: f32 = 3.;

impl Thumb {
    /// The core's picture converted for the window.
    fn of(img: &Image) -> Option<Thumb> {
        Some(Thumb {
            ink: ink(&img.rgba),
            image: to_render_image(img)?,
        })
    }

    /// Whether it is drawn on `picture_backdrop`: it lets its background
    /// through, and its ink would not read on the window's but reads on
    /// the backdrop. On the dark look, dark lines on nothing; a colored
    /// icon, which reads, keeps no light corners around its own, and a
    /// light one on the light look no white box that would hardly help.
    pub fn on_backdrop(&self, theme: &Theme) -> bool {
        self.ink.is_some_and(|ink| {
            contrast(ink, luminance(theme.bg)) < LEGIBLE
                && contrast(ink, luminance(theme.picture_backdrop)) >= LEGIBLE
        })
    }

    /// Its element, on its backdrop when it needs one (`on_backdrop`). The
    /// backdrop is the element's box, so a view sizes and fits it as it
    /// would the picture.
    pub fn img(&self, theme: &Theme) -> gpui::Img {
        gpui::img(Arc::clone(&self.image)).when(self.on_backdrop(theme), |img| {
            img.bg(theme.picture_backdrop)
        })
    }
}

/// The relative luminance of what `rgba` draws, each pixel weighted by how
/// opaque it is, when some of it is not; `None` for a picture opaque
/// throughout (a backdrop would only fringe its edges) or with nothing
/// drawn. Read once, when the thumbnail is converted.
fn ink(rgba: &[u8]) -> Option<f32> {
    static LINEAR: std::sync::LazyLock<[f32; 256]> =
        std::sync::LazyLock::new(|| std::array::from_fn(|v| linear(v as f32 / 255.)));
    let pixels = rgba.as_chunks::<4>().0;
    if pixels.iter().all(|p| p[3] == 255) {
        return None;
    }
    let (mut sum, mut weight) = (0f32, 0f32);
    for &[r, g, b, a] in pixels {
        let a = f32::from(a) / 255.;
        let [r, g, b] = [r, g, b].map(|c| LINEAR[usize::from(c)]);
        sum += a * (0.2126 * r + 0.7152 * g + 0.0722 * b);
        weight += a;
    }
    (weight > 0.).then(|| sum / weight)
}

/// An sRGB channel (0 to 1) in linear light.
fn linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A color's relative luminance.
fn luminance(color: gpui::Hsla) -> f32 {
    let rgb = color.to_rgb();
    0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b)
}

/// The contrast of two relative luminances, from 1 to 21.
fn contrast(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// An entry's picture at one size, as of one modification time: a file
/// edited in place gets a new key, so its old picture is never shown again
/// (it ages out of the cache).
/// A picture is of a file as a type (`thumbs::key`, ADR-049): every name
/// of it that says that type, and every link to it, shows the same one.
type Key = skwir_core::thumbs::Key;

/// Converted pictures, bounded by bytes: the memory the thumbnails setting
/// gives (`Thumbnails::budget`). What this frame and the one before drew is
/// never evicted, the budget held again once it no longer is: evicted
/// while the frame draws it, an image left the atlas and came back at the
/// next frame. What is evicted is given back to the atlas when the next
/// frame begins (`begin_frame`), in render, never while a map or a view
/// lays out.
pub struct ThumbCache {
    lru: ByteLru<Key, Thumb>,
    retired: Vec<Thumb>,
    /// The recency clock when this frame and the one before began.
    frame: u64,
    before: u64,
}

impl Default for ThumbCache {
    fn default() -> Self {
        ThumbCache {
            lru: ByteLru::new(64 << 20),
            retired: Vec::new(),
            frame: 0,
            before: 0,
        }
    }
}

impl ThumbCache {
    fn get(&mut self, k: &Key) -> Option<Thumb> {
        self.lru.get(k).cloned()
    }

    /// Insert a converted image; what it evicts, and the one it replaced,
    /// go back to the atlas when the next frame begins.
    fn insert(&mut self, k: Key, img: Thumb, bytes: usize) {
        let evicted = self.lru.insert_keeping(k, img, bytes, self.before.max(1));
        self.retired.extend(evicted);
    }

    /// A frame begins: the images to give back to the atlas, and from now
    /// on what this frame draws is kept too. `budget` is the setting's.
    pub fn begin_frame(&mut self, budget: usize) -> Vec<Arc<RenderImage>> {
        if self.lru.budget() != budget {
            self.retired.extend(self.lru.set_budget(budget));
        }
        self.before = self.frame;
        self.frame = self.lru.now() + 1;
        self.retired.drain(..).map(|thumb| thumb.image).collect()
    }

    pub fn bytes(&self) -> usize {
        self.lru.bytes()
    }
}

/// Convert the core's RGBA8 image into GPUI's BGRA8 `RenderImage`.
pub fn to_render_image(img: &Image) -> Option<Arc<RenderImage>> {
    owned_render_image(img.width, img.height, img.rgba.to_vec())
}

/// Document pages have one consumer: swizzle the transferred buffer in place.
pub fn owned_render_image(width: u32, height: u32, mut buf: Vec<u8>) -> Option<Arc<RenderImage>> {
    for p in buf.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    bgra_render_image(width, height, buf)
}

/// A document's page as GPUI draws it: one an engine drew in BGRA, the
/// window's order, as it came.
pub fn page_image(
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    order: skwir_core::document::PixelOrder,
) -> Option<Arc<RenderImage>> {
    match order {
        skwir_core::document::PixelOrder::Bgra => bgra_render_image(width, height, pixels),
        skwir_core::document::PixelOrder::Rgba => owned_render_image(width, height, pixels),
    }
}

/// `buf`, BGRA already, as GPUI's image (whose frames say RGBA and hold
/// BGRA).
fn bgra_render_image(width: u32, height: u32, buf: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(width, height, buf)?;
    Some(Arc::new(RenderImage::new(SmallVec::from_buf([
        image::Frame::new(buffer),
    ]))))
}

impl Shell {
    /// The thumbnail for `id`, requesting it at visible priority if needed.
    /// Entries without metadata are asked for once they have it (mtime is
    /// needed for cache freshness): meanwhile `Pending`, or `Unavailable`
    /// for one no thumbnailer makes, drawn as its icon.
    pub fn thumb(&mut self, id: EntryId, size: ThumbSize) -> ThumbView {
        let (key, req, hydrated) = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return ThumbView::Unavailable;
            };
            // An entry of the pane, shown or of a folder opened in place
            // (the tree's, the board's cards).
            let Some(e) = pane.entry(id) else {
                return ThumbView::Unavailable;
            };
            if e.is_dir() {
                return ThumbView::Unavailable;
            }
            let mtime = e
                .metadata
                .value(keys::MODIFIED)
                .and_then(skwir_core::Value::as_time);
            // What is shown: a link's target, the entry's own file else,
            // as the type its name says.
            let shown = e.target.as_ref().map_or(e.file(), |t| t.id);
            let typed = skwir_platform::mime::of_entry(e);
            let key = skwir_core::thumbs::key(
                shown,
                typed.as_ref().map(skwir_platform::mime::Mime::as_str),
                size,
                mtime,
            );
            if let Some(img) = self.thumbs.get(&key) {
                return ThumbView::Ready(img);
            }
            let Some(path) = pane.path_of(id) else {
                return ThumbView::Unavailable;
            };
            // A link's picture is its target's; a pipe or a device has none.
            let Some(content) = e.content(&path) else {
                return ThumbView::Unavailable;
            };
            let req = ThumbRequest {
                entry: content,
                mime: typed.map(|m| Arc::from(m.as_str())),
                mtime,
                size,
            };
            (key, req, e.metadata.is_hydrated())
        };
        // Asked once stat-ed, its time keying the cache. Meanwhile one no
        // thumbnailer makes is drawn as its icon, as while a cache alone is
        // looked in for it, not as a picture on its way.
        if !hydrated {
            return if self.app.thumbnails.makes(&req) {
                ThumbView::Pending
            } else {
                ThumbView::Unavailable
            };
        }
        self.thumb_asked(key, req)
    }

    /// The thumbnail of a file no listing holds (a picture on a place's
    /// tile, `keys::PLACE_PICTURES`), by its identity and path, requested
    /// at visible priority if needed.
    pub fn thumb_of(
        &mut self,
        entry: skwir_core::op::EntryRef,
        mtime: Option<SystemTime>,
        size: ThumbSize,
    ) -> ThumbView {
        let type_named = entry
            .path
            .file_name()
            .and_then(skwir_platform::mime::by_name)
            .map(|m| Arc::from(m.as_str()));
        let req = ThumbRequest {
            entry,
            mime: type_named,
            mtime,
            size,
        };
        let key = req.key();
        if let Some(img) = self.thumbs.get(&key) {
            return ThumbView::Ready(img);
        }
        self.thumb_asked(key, req)
    }

    /// `req` asked of the thumbnails, kept under `key` once ready: taken
    /// from the core's, which keeps it no longer. While only a cache is
    /// looked in for it (no thumbnailer makes its type), it is drawn as a
    /// file without a picture, its icon, not a placeholder: a picture
    /// seldom comes, and is shown if one does.
    fn thumb_asked(&mut self, key: Key, req: ThumbRequest) -> ThumbView {
        match self.app.thumbnails.take(req, Priority::Visible) {
            ThumbState::Ready(img) => {
                let Some(thumb) = Thumb::of(&img) else {
                    return ThumbView::Unavailable;
                };
                self.thumbs.insert(key, thumb.clone(), img.bytes());
                ThumbView::Ready(thumb)
            }
            ThumbState::Pending if self.app.thumbnails.only_in_cache(key) => ThumbView::Unavailable,
            ThumbState::Pending => ThumbView::Pending,
            ThumbState::Unavailable => ThumbView::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::FileId;

    fn img(px: u32) -> Thumb {
        Thumb::of(&Image {
            width: px,
            height: px,
            rgba: Arc::from(vec![0u8; (px * px * 4) as usize]),
        })
        .unwrap()
    }

    /// Four pixels of one color, with these opacities.
    fn picture(rgb: [u8; 3], alphas: [u8; 4]) -> Thumb {
        let rgba: Vec<u8> = alphas
            .iter()
            .flat_map(|&a| [rgb[0], rgb[1], rgb[2], a])
            .collect();
        Thumb::of(&Image {
            width: 2,
            height: 2,
            rgba: rgba.into(),
        })
        .unwrap()
    }

    #[test]
    fn only_ink_that_would_not_read_is_drawn_on_the_backdrop() {
        let (dark, light) = (Theme::dark(), Theme::light());
        let lines = picture([0, 0, 0], [255, 255, 0, 0]);
        assert!(lines.on_backdrop(&dark), "black lines on nothing");
        assert!(!lines.on_backdrop(&light), "they read on the light look");
        let edge = picture([0, 0, 0], [255, 255, 255, 254]);
        assert!(edge.on_backdrop(&dark), "one pixel drawn on nothing");
        let photo = picture([0, 0, 0], [255; 4]);
        assert_eq!(photo.ink, None, "an opaque picture has no backdrop");
        assert!(!photo.on_backdrop(&dark));
        let icon = picture([0xe2, 0x55, 0x2a], [255, 255, 255, 0]);
        assert!(!icon.on_backdrop(&dark), "an orange icon reads as it is");
        let white = picture([255, 255, 255], [255, 0, 0, 0]);
        assert!(
            !white.on_backdrop(&light),
            "a light backdrop would not help white lines"
        );
        let green = picture([0x2e, 0xc2, 0x7e], [255, 255, 255, 0]);
        assert!(
            !green.on_backdrop(&light),
            "white would not make a light green read"
        );
        assert_eq!(picture([0, 0, 0], [0; 4]).ink, None, "nothing drawn");
        // Weighted by opacity: a faint black shadow under a white mark.
        let mut rgba = vec![255, 255, 255, 255, 255, 255, 255, 255];
        rgba.extend([0, 0, 0, 20, 0, 0, 0, 20]);
        let shadowed = Thumb::of(&Image {
            width: 2,
            height: 2,
            rgba: rgba.into(),
        })
        .unwrap();
        assert!(!shadowed.on_backdrop(&dark), "its white reads");
    }

    #[test]
    fn owned_pixels_are_swizzled_without_copying_the_allocation() {
        let pixels = vec![10, 20, 30, 255];
        let address = pixels.as_ptr();
        let image = owned_render_image(1, 1, pixels).unwrap();
        let bytes = image.as_bytes(0).unwrap();
        assert_eq!(bytes, &[30, 20, 10, 255]);
        assert_eq!(
            bytes.as_ptr(),
            address,
            "page transfer must keep the owned buffer"
        );
    }

    /// A picture of a PNG file, as the tests keep them.
    fn key(file: FileId, mtime: Option<SystemTime>) -> Key {
        skwir_core::thumbs::key(file, Some("image/png"), ThumbSize::Normal, mtime)
    }

    #[test]
    fn an_edited_file_is_a_new_picture_and_the_cache_stays_in_budget() {
        let mut c = ThumbCache::default();
        assert!(c.begin_frame(3 * 64).is_empty());
        let id = FileId { dev: 1, ino: 7 };
        let then = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10);
        let now = then + std::time::Duration::from_secs(5);
        c.insert(key(id, Some(then)), img(4), 64);
        assert!(c.get(&key(id, Some(then))).is_some());
        assert!(
            c.get(&key(id, Some(now))).is_none(),
            "edited since: not the old picture"
        );
        // Each in a frame of its own: once two frames old, a picture can go.
        let mut retired = 0;
        for n in 0..10 {
            retired += c.begin_frame(3 * 64).len();
            let k = key(FileId { dev: 1, ino: n }, None);
            c.insert(k, img(4), 64);
            for _ in 0..100 {
                c.get(&k);
            }
        }
        retired += c.begin_frame(3 * 64).len();
        assert_eq!(c.bytes(), 192);
        assert_eq!(retired, 8, "what was evicted goes back to the atlas");
    }

    /// What a frame draws is never evicted while it does, the budget held
    /// again once it no longer is.
    #[test]
    fn what_a_frame_draws_stays_over_the_budget() {
        let mut c = ThumbCache::default();
        c.begin_frame(2 * 64);
        c.begin_frame(2 * 64);
        for n in 0..5 {
            c.insert(key(FileId { dev: 1, ino: n }, None), img(4), 64);
        }
        assert_eq!(c.bytes(), 5 * 64, "all drawn this frame: kept");
        c.begin_frame(2 * 64);
        assert_eq!(c.bytes(), 5 * 64, "and the next");
        c.begin_frame(2 * 64);
        c.insert(key(FileId { dev: 1, ino: 9 }, None), img(4), 64);
        assert_eq!(c.bytes(), 2 * 64, "drawn two frames ago: gone");
    }
}
