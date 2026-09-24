//! Pictures a document names for later (`Picture::Later`), decoded as they
//! come into view.
//!
//! A book's chapter can name a hundred illustrations; decoding them all with
//! the chapter would cost their pixels before a single one is seen. The
//! provider names each with the size it will be shown at, the rows hold that
//! box, and what the rows drew is asked for here, one picture at a time,
//! nearest first, through the provider that read the page
//! (`Previews::picture`). The one before and the one after what is on screen
//! are asked for too, so scrolling finds them ready.
//!
//! Decoded pictures are kept by key up to `CACHE_BYTES`; past it, those
//! farthest from the view go, and their atlas entries with them
//! (`cx.drop_image`), like the reader's pages. A result is accepted only for
//! the generation it was asked in: a new page, or a file read again, starts
//! another.
use crate::{preview::PreviewContent, shell::Shell};
use bibli_core::{CancelToken, inspection::Picture, scheduler::Priority};
use gpui::{Context, RenderImage, Window};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

/// Decoded pixels kept beyond what is on screen.
const CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct Decoded {
    pub image: Arc<RenderImage>,
    bytes: usize,
}

struct Reply {
    generation: u64,
    key: String,
    result: Result<Decoded, String>,
}

pub struct Pictures {
    /// Shared with each frame's rows, which only read it.
    pub(crate) loaded: Rc<HashMap<String, Decoded>>,
    failed: HashSet<String>,
    /// The blocks the rows drew last, to do nothing when they drew the same.
    seen: Vec<usize>,
    /// What the view shows and its neighbours, nearest first.
    wanted: Vec<String>,
    /// How far each picture is from the view, in blocks, for eviction.
    distance: HashMap<String, usize>,
    pending: Option<(String, CancelToken)>,
    generation: u64,
    tx: flume::Sender<Reply>,
    rx: flume::Receiver<Reply>,
}
impl Default for Pictures {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        Self {
            loaded: Rc::default(),
            failed: HashSet::new(),
            seen: Vec::new(),
            wanted: Vec::new(),
            distance: HashMap::new(),
            pending: None,
            generation: 0,
            tx,
            rx,
        }
    }
}
impl Drop for Pictures {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Pictures {
    fn cancel(&mut self) {
        if let Some((_, token)) = self.pending.take() {
            token.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
    }
    /// Forget every picture, for a page that replaces the one they were of.
    /// The caller drops what is returned from the atlas.
    pub(crate) fn clear(&mut self) -> Vec<Arc<RenderImage>> {
        self.cancel();
        self.failed.clear();
        self.seen.clear();
        self.wanted.clear();
        self.distance.clear();
        std::mem::take(&mut self.loaded)
            .values()
            .map(|decoded| decoded.image.clone())
            .collect()
    }
    /// Past the budget, the decoded pictures farthest from the view go.
    fn evict(&mut self) -> Vec<Decoded> {
        let mut bytes: usize = self.loaded.values().map(|d| d.bytes).sum();
        let mut candidates: Vec<String> = self
            .loaded
            .keys()
            .filter(|key| !self.wanted.contains(key))
            .cloned()
            .collect();
        // Farthest first; a picture no block names any more is farthest.
        candidates.sort_by_key(|key| {
            std::cmp::Reverse(self.distance.get(key).copied().unwrap_or(usize::MAX))
        });
        let mut evicted = Vec::new();
        for key in candidates {
            if bytes <= CACHE_BYTES {
                break;
            }
            if let Some(decoded) = Rc::make_mut(&mut self.loaded).remove(&key) {
                bytes -= decoded.bytes;
                evicted.push(decoded);
            }
        }
        evicted
    }
}

/// The pictures to have: those of the blocks drawn (`seen`, block indices),
/// nearest the middle of them first, then the one before and the one after.
/// `pictures` lists every block with a picture for later, in block order.
fn wanted(seen: &[usize], pictures: &[(usize, &str)]) -> Vec<String> {
    let (Some(&first), Some(&last)) = (seen.iter().min(), seen.iter().max()) else {
        return Vec::new();
    };
    let middle = usize::midpoint(first, last);
    let mut shown: Vec<(usize, &str)> = pictures
        .iter()
        .filter(|(index, _)| seen.contains(index))
        .copied()
        .collect();
    shown.sort_by_key(|(index, _)| index.abs_diff(middle));
    let before = pictures.iter().rev().find(|(index, _)| *index < first);
    let after = pictures.iter().find(|(index, _)| *index > last);
    let mut keys: Vec<String> = Vec::new();
    for (_, key) in shown
        .into_iter()
        .chain(after.copied())
        .chain(before.copied())
    {
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_owned());
        }
    }
    keys
}

impl Shell {
    /// The rows just drew the blocks `seen`:
    /// keep what they show, let go of what is far, and ask for what is
    /// missing. Called after layout, never while painting; it redraws only
    /// when a picture arrives.
    pub(crate) fn inspector_pictures(
        &mut self,
        seen: Vec<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preview.inspection.pictures.seen == seen {
            return;
        }
        let Some(PreviewContent::Inspection(display)) = &self.preview.content else {
            return;
        };
        let later: Vec<(usize, &str)> = display
            .data
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| match &block.picture {
                Some(Picture::Later { key, .. }) => Some((index, key.as_str())),
                _ => None,
            })
            .collect();
        let wanted = wanted(&seen, &later);
        let middle = seen
            .iter()
            .min()
            .zip(seen.iter().max())
            .map_or(0, |(first, last)| usize::midpoint(*first, *last));
        let mut distance = HashMap::new();
        for (index, key) in &later {
            let far = index.abs_diff(middle);
            distance
                .entry((*key).to_owned())
                .and_modify(|d: &mut usize| *d = (*d).min(far))
                .or_insert(far);
        }
        let pictures = &mut self.preview.inspection.pictures;
        pictures.seen = seen;
        pictures.distance = distance;
        if pictures.wanted == wanted {
            return;
        }
        pictures.wanted = wanted;
        if pictures
            .pending
            .as_ref()
            .is_some_and(|(key, _)| !pictures.wanted.contains(key))
        {
            pictures.cancel();
        }
        for decoded in pictures.evict() {
            cx.drop_image(decoded.image, Some(window));
        }
        self.request_picture();
    }

    /// Ask for the first wanted picture not yet decoded, if none is on its
    /// way: one at a time, like the reader's pages.
    fn request_picture(&mut self) {
        let Some(PreviewContent::Inspection(display)) = &self.preview.content else {
            return;
        };
        let Some((request, view)) = display.origin.clone() else {
            return;
        };
        let pictures = &mut self.preview.inspection.pictures;
        if pictures.pending.is_some() {
            return;
        }
        let Some(key) = pictures
            .wanted
            .iter()
            .find(|key| !pictures.loaded.contains_key(*key) && !pictures.failed.contains(*key))
            .cloned()
        else {
            return;
        };
        let cancel = self.app.lifetime().child();
        pictures.pending = Some((key.clone(), cancel.clone()));
        let generation = pictures.generation;
        let tx = pictures.tx.clone();
        let previews = Arc::clone(&self.app.previews);
        let wake = self.app.waker_handle();
        self.app.scheduler.spawn(
            Priority::Visible,
            "preview-picture",
            cancel.clone(),
            move || {
                let result = previews
                    .picture(&request, &view, &key, &cancel)
                    .and_then(|image| {
                        let image =
                            image.ok_or_else(|| anyhow::anyhow!("the picture is not there"))?;
                        let bytes = image.bytes();
                        let image = crate::thumbs::to_render_image(&image)
                            .ok_or_else(|| anyhow::anyhow!("invalid picture"))?;
                        Ok(Decoded { image, bytes })
                    })
                    .map_err(|error| format!("{error:#}"));
                if !cancel.is_cancelled() {
                    let _ = tx.send(Reply {
                        generation,
                        key,
                        result,
                    });
                    wake();
                }
            },
        );
    }

    /// Take the pictures that arrived, as the core wakes the UI.
    pub(crate) fn sync_pictures(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut arrived = false;
        while let Ok(reply) = self.preview.inspection.pictures.rx.try_recv() {
            let pictures = &mut self.preview.inspection.pictures;
            if reply.generation != pictures.generation {
                continue;
            }
            pictures.pending = None;
            match reply.result {
                Ok(decoded) => {
                    if let Some(old) = Rc::make_mut(&mut pictures.loaded).insert(reply.key, decoded)
                    {
                        cx.drop_image(old.image, Some(window));
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, "preview picture");
                    pictures.failed.insert(reply.key);
                }
            }
            arrived = true;
            for decoded in pictures.evict() {
                cx.drop_image(decoded.image, Some(window));
            }
            self.request_picture();
        }
        if arrived {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_drawn_comes_first_then_its_neighbours() {
        let pictures = [(2, "a"), (5, "b"), (9, "c"), (12, "d"), (20, "e")];
        assert_eq!(wanted(&[8, 9, 10, 11, 12], &pictures), ["c", "d", "e", "b"]);
        assert_eq!(wanted(&[0, 1], &pictures), ["a"], "only the one after");
        assert!(wanted(&[], &pictures).is_empty());
        // One picture named by two blocks is asked for once.
        assert_eq!(wanted(&[3, 4, 5], &[(4, "x"), (5, "x")]), ["x"]);
    }
}
