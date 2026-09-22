//! UI side of thumbnails: builds requests for visible entries, converts the
//! core's RGBA images into GPUI `RenderImage`s once, and keeps a byte-capped
//! LRU that releases atlas textures on eviction.

use std::sync::Arc;
use std::time::SystemTime;

use bibli_core::EntryId;
use bibli_core::entry::keys;
use bibli_core::lru::ByteLru;
use bibli_core::op::EntryRef;
use bibli_core::scheduler::Priority;
use bibli_core::thumbs::{Image, ThumbRequest, ThumbSize, ThumbState};
use gpui::{Context, RenderImage, Window};
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
type Key = (EntryId, ThumbSize, Option<SystemTime>);

/// Converted pictures, bounded by bytes. Evicted images are handed back so
/// their atlas textures can be released.
pub struct ThumbCache(ByteLru<Key, Arc<RenderImage>>);

impl Default for ThumbCache {
    fn default() -> Self {
        ThumbCache(ByteLru::new(48 << 20))
    }
}

impl ThumbCache {
    fn get(&mut self, k: &Key) -> Option<Arc<RenderImage>> {
        self.0.get(k).cloned()
    }

    /// Insert a converted image; returns the images to release: those
    /// evicted to stay in budget, and the one it replaced.
    fn insert(&mut self, k: Key, img: Arc<RenderImage>, bytes: usize) -> Vec<Arc<RenderImage>> {
        self.0.insert(k, img, bytes)
    }

    pub fn bytes(&self) -> usize {
        self.0.bytes()
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
    let buffer = image::RgbaImage::from_raw(width, height, buf)?;
    Some(Arc::new(RenderImage::new(SmallVec::from_buf([
        image::Frame::new(buffer),
    ]))))
}

impl Shell {
    /// The thumbnail for `id`, requesting it at visible priority if needed.
    /// Entries without metadata get a hydrate request first (mtime is needed
    /// for cache freshness) and report `Pending`.
    pub fn thumb(
        &mut self,
        id: EntryId,
        size: ThumbSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ThumbView {
        let (key, req) = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return ThumbView::Unavailable;
            };
            let Some(e) = pane.listing.get(id) else {
                return ThumbView::Unavailable;
            };
            if e.is_dir() {
                return ThumbView::Unavailable;
            }
            let mtime = e
                .metadata
                .value(keys::MODIFIED)
                .and_then(bibli_core::Value::as_time);
            let key = (id, size, mtime);
            if let Some(img) = self.thumbs.get(&key) {
                return ThumbView::Ready(img);
            }
            if !e.metadata.is_hydrated() {
                return ThumbView::Pending;
            }
            let Some(path) = pane.open.as_ref().and_then(|o| o.path_of(id)) else {
                return ThumbView::Unavailable;
            };
            let req = ThumbRequest {
                entry: EntryRef::new(id, path),
                mime: bibli_platform::mime::by_name(&e.name).map(|m| Arc::from(m.as_str())),
                mtime,
                size,
            };
            (key, req)
        };
        match self.app.thumbnails.get(req, Priority::Visible) {
            ThumbState::Ready(img) => {
                let Some(ri) = to_render_image(&img) else {
                    return ThumbView::Unavailable;
                };
                for old in self.thumbs.insert(key, Arc::clone(&ri), img.bytes()) {
                    cx.drop_image(old, Some(window));
                }
                ThumbView::Ready(ri)
            }
            ThumbState::Pending => ThumbView::Pending,
            ThumbState::Unavailable => ThumbView::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn an_edited_file_is_a_new_picture_and_the_cache_stays_in_budget() {
        let mut c = ThumbCache(ByteLru::new(3 * 64));
        let id = EntryId { dev: 1, ino: 7 };
        let then = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10);
        let now = then + std::time::Duration::from_secs(5);
        assert!(
            c.insert((id, ThumbSize::Normal, Some(then)), img(4), 64)
                .is_empty()
        );
        assert!(c.get(&(id, ThumbSize::Normal, Some(then))).is_some());
        assert!(
            c.get(&(id, ThumbSize::Normal, Some(now))).is_none(),
            "edited since: not the old picture"
        );
        for n in 0..10 {
            let k = (EntryId { dev: 1, ino: n }, ThumbSize::Normal, None);
            c.insert(k, img(4), 64);
            for _ in 0..100 {
                c.get(&k);
            }
        }
        assert_eq!(c.bytes(), 192);
    }
}
