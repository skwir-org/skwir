//! UI side of thumbnails: builds requests for visible entries, converts the
//! core's RGBA images into GPUI `RenderImage`s once, and keeps a byte-capped
//! LRU that releases atlas textures on eviction.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use bibli_core::EntryId;
use bibli_core::entry::keys;
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

type Key = (EntryId, ThumbSize);

pub struct ThumbCache {
    map: HashMap<Key, (Arc<RenderImage>, usize, u64)>,
    order: VecDeque<(Key, u64)>,
    bytes: usize,
    budget: usize,
    tick: u64,
}

impl Default for ThumbCache {
    fn default() -> Self {
        ThumbCache {
            map: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            budget: 48 << 20,
            tick: 0,
        }
    }
}

impl ThumbCache {
    fn get(&mut self, k: Key) -> Option<Arc<RenderImage>> {
        self.tick += 1;
        let (img, _, stamp) = self.map.get_mut(&k)?;
        *stamp = self.tick;
        self.order.push_back((k, self.tick));
        Some(Arc::clone(img))
    }

    /// Insert a converted image; returns the images evicted to stay in budget.
    fn insert(&mut self, k: Key, img: Arc<RenderImage>, bytes: usize) -> Vec<Arc<RenderImage>> {
        self.tick += 1;
        if let Some((old, b, _)) = self.map.insert(k, (img, bytes, self.tick)) {
            self.bytes -= b;
            drop(old);
        }
        self.bytes += bytes;
        self.order.push_back((k, self.tick));
        let mut evicted = Vec::new();
        while self.bytes > self.budget {
            let Some((old_k, stamp)) = self.order.pop_front() else {
                break;
            };
            if self.map.get(&old_k).is_some_and(|(_, _, s)| *s == stamp)
                && let Some((old, b, _)) = self.map.remove(&old_k)
            {
                self.bytes -= b;
                evicted.push(old);
            }
        }
        evicted
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Convert the core's RGBA8 image into GPUI's BGRA8 `RenderImage`.
pub fn to_render_image(img: &Image) -> Option<Arc<RenderImage>> {
    let mut buf = img.rgba.to_vec();
    for p in buf.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(img.width, img.height, buf)?;
    Some(Arc::new(RenderImage::new(SmallVec::from_elem(
        image::Frame::new(buffer),
        1,
    ))))
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
        if let Some(img) = self.thumbs.get((id, size)) {
            return ThumbView::Ready(img);
        }
        let req = {
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
            if !e.metadata.is_hydrated() {
                return ThumbView::Pending;
            }
            let Some(path) = pane.open.as_ref().and_then(|o| o.path_of(id)) else {
                return ThumbView::Unavailable;
            };
            ThumbRequest {
                entry: EntryRef::new(id, path),
                mime: bibli_platform::mime::by_name(&e.name).map(|m| Arc::from(m.as_str())),
                mtime: e
                    .metadata
                    .value(keys::MODIFIED)
                    .and_then(bibli_core::Value::as_time),
                size,
            }
        };
        match self.app.thumbnails.get(req, Priority::Visible) {
            ThumbState::Ready(img) => {
                let Some(ri) = to_render_image(&img) else {
                    return ThumbView::Unavailable;
                };
                for old in self.thumbs.insert((id, size), Arc::clone(&ri), img.bytes()) {
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
    fn lru_evicts_oldest_over_budget() {
        let mut c = ThumbCache {
            budget: 3 * 64,
            ..ThumbCache::default()
        };
        let k = |n: u64| (EntryId { dev: 1, ino: n }, ThumbSize::Normal);
        assert!(c.insert(k(1), img(4), 64).is_empty());
        assert!(c.insert(k(2), img(4), 64).is_empty());
        assert!(c.insert(k(3), img(4), 64).is_empty());
        assert!(c.get(k(1)).is_some(), "touch 1 so 2 is the oldest");
        let evicted = c.insert(k(4), img(4), 64);
        assert_eq!(evicted.len(), 1);
        assert!(c.get(k(2)).is_none());
        assert!(c.get(k(1)).is_some());
        assert_eq!(c.bytes(), 192);
        assert_eq!(c.len(), 3);
    }
}
