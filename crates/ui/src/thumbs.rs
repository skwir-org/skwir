//! UI side of thumbnails: builds requests for visible entries, converts the
//! core's RGBA images into GPUI `RenderImage`s once, and keeps a byte-capped
//! LRU that releases atlas textures on eviction.

use std::sync::Arc;
use std::time::SystemTime;

use bibli_core::EntryId;
use bibli_core::entry::keys;
use bibli_core::lru::ByteLru;
use bibli_core::scheduler::Priority;
use bibli_core::thumbs::{Image, ThumbRequest, ThumbSize, ThumbState};
use gpui::RenderImage;
use smallvec::SmallVec;

use crate::shell::Shell;

/// What a view draws for an entry.
#[derive(Clone)]
pub enum ThumbView {
    Ready(Arc<RenderImage>),
    Pending,
    Unavailable,
}

/// An entry's picture at one size, as of one modification time: a file
/// edited in place gets a new key, so its old picture is never shown again
/// (it ages out of the cache).
/// A picture is of a file as a type (`thumbs::key`, ADR-049): every name
/// of it that says that type, and every link to it, shows the same one.
type Key = bibli_core::thumbs::Key;

/// Converted pictures, bounded by bytes: the memory the thumbnails setting
/// gives (`Thumbnails::budget`). What this frame and the one before drew is
/// never evicted, the budget held again once it no longer is: evicted
/// while the frame draws it, an image left the atlas and came back at the
/// next frame. What is evicted is given back to the atlas when the next
/// frame begins (`begin_frame`), in render, never while a map or a view
/// lays out.
pub struct ThumbCache {
    lru: ByteLru<Key, Arc<RenderImage>>,
    retired: Vec<Arc<RenderImage>>,
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
    fn get(&mut self, k: &Key) -> Option<Arc<RenderImage>> {
        self.lru.get(k).cloned()
    }

    /// Insert a converted image; what it evicts, and the one it replaced,
    /// go back to the atlas when the next frame begins.
    fn insert(&mut self, k: Key, img: Arc<RenderImage>, bytes: usize) {
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
        std::mem::take(&mut self.retired)
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
    order: bibli_core::document::PixelOrder,
) -> Option<Arc<RenderImage>> {
    match order {
        bibli_core::document::PixelOrder::Bgra => bgra_render_image(width, height, pixels),
        bibli_core::document::PixelOrder::Rgba => owned_render_image(width, height, pixels),
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
                .and_then(bibli_core::Value::as_time);
            // What is shown: a link's target, the entry's own file else,
            // as the type its name says.
            let shown = e.target.as_ref().map_or(e.file(), |t| t.id);
            let typed = bibli_platform::mime::of_entry(e);
            let key = bibli_core::thumbs::key(
                shown,
                typed.as_ref().map(bibli_platform::mime::Mime::as_str),
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
        entry: bibli_core::op::EntryRef,
        mtime: Option<SystemTime>,
        size: ThumbSize,
    ) -> ThumbView {
        let type_named = entry
            .path
            .file_name()
            .and_then(bibli_platform::mime::by_name)
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
                let Some(ri) = to_render_image(&img) else {
                    return ThumbView::Unavailable;
                };
                self.thumbs.insert(key, Arc::clone(&ri), img.bytes());
                ThumbView::Ready(ri)
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
    use bibli_core::FileId;

    fn img(px: u32) -> Arc<RenderImage> {
        to_render_image(&Image {
            width: px,
            height: px,
            rgba: Arc::from(vec![0u8; (px * px * 4) as usize]),
        })
        .unwrap()
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
        bibli_core::thumbs::key(file, Some("image/png"), ThumbSize::Normal, mtime)
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
