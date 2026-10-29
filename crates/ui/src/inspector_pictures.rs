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
//! (`cx.drop_image`), like the reader's pages. A result is accepted only
//! for what is asked now (`Latest`): a new page, or a file read again,
//! forgets what was asked before.
use crate::{latest::Latest, preview::PreviewContent, shell::Shell};
use gpui::{Context, RenderImage, Window};
use skwir_core::{
    inspection::{Block, Picture},
    scheduler::Priority,
};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

/// Decoded pixels kept beyond what is on screen.
const CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct Decoded {
    pub picture: crate::backdrop::Picture,
    bytes: usize,
}

struct Reply {
    key: String,
    result: Result<Decoded, String>,
}

/// The pictures a page names for later, found once with the page
/// (`inspector::Display::new`), never by a walk through its blocks as the
/// view moves.
#[derive(Default)]
pub(crate) struct Later {
    /// Each block naming one, and its key, in block order.
    blocks: Vec<(usize, Arc<str>)>,
    /// The blocks naming each key, in order: how far a picture is from the
    /// view is how far the nearest of them is.
    at: HashMap<Arc<str>, Vec<usize>>,
}

impl Later {
    pub(crate) fn of(blocks: &[Block]) -> Later {
        Later::named(
            blocks
                .iter()
                .enumerate()
                .filter_map(|(index, block)| match &block.picture {
                    Some(Picture::Later { key, .. }) => Some((index, key.as_str())),
                    _ => None,
                }),
        )
    }

    /// From (block, key) in block order.
    fn named<'a>(named: impl IntoIterator<Item = (usize, &'a str)>) -> Later {
        let mut later = Later::default();
        for (index, key) in named {
            let key: Arc<str> = match later.at.get_key_value(key) {
                Some((shared, _)) => Arc::clone(shared),
                None => key.into(),
            };
            later.at.entry(Arc::clone(&key)).or_default().push(index);
            later.blocks.push((index, key));
        }
        later
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// How far, in blocks, the nearest block naming `key` is from `middle`;
    /// the farthest of all for a key no block names.
    fn distance(&self, key: &str, middle: usize) -> usize {
        let Some(blocks) = self.at.get(key) else {
            return usize::MAX;
        };
        let after = blocks.partition_point(|&block| block < middle);
        let near = |i: usize| blocks.get(i).map(|&block| block.abs_diff(middle));
        near(after)
            .into_iter()
            .chain(after.checked_sub(1).and_then(near))
            .min()
            .unwrap_or(usize::MAX)
    }
}

#[derive(Default)]
pub struct Pictures {
    /// Shared with each frame's rows, which only read it.
    pub(crate) loaded: Rc<HashMap<String, Decoded>>,
    /// What could not be decoded, never asked again; the rows draw its
    /// description in its box.
    pub(crate) failed: Rc<HashSet<String>>,
    /// The blocks the rows drew last, to do nothing when they drew the same.
    seen: Vec<usize>,
    /// What the view shows and its neighbours, nearest first.
    wanted: Vec<String>,
    /// The pictures of the page, and the middle of the blocks drawn last:
    /// how far each picture is from the view, for eviction.
    later: Arc<Later>,
    middle: usize,
    /// The picture on its way, one at a time.
    asking: Option<String>,
    asked: Latest<Reply>,
}
impl Pictures {
    fn cancel(&mut self) {
        self.asking = None;
        self.asked.forget();
    }
    /// Forget every picture, for a page that replaces the one they were of.
    /// The caller drops what is returned from the atlas.
    pub(crate) fn clear(&mut self) -> Vec<Arc<RenderImage>> {
        self.cancel();
        self.failed = Rc::default();
        self.seen.clear();
        self.wanted.clear();
        self.later = Arc::default();
        self.middle = 0;
        std::mem::take(&mut self.loaded)
            .values()
            .map(|decoded| decoded.picture.image.clone())
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
        candidates
            .sort_by_cached_key(|key| std::cmp::Reverse(self.later.distance(key, self.middle)));
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
fn wanted(seen: &[usize], later: &Later) -> Vec<String> {
    let (Some(&first), Some(&last)) = (seen.iter().min(), seen.iter().max()) else {
        return Vec::new();
    };
    let middle = usize::midpoint(first, last);
    let pictures = &later.blocks;
    // Those between the first and the last drawn, found by their blocks.
    let from = pictures.partition_point(|(index, _)| *index < first);
    let to = pictures.partition_point(|(index, _)| *index <= last);
    let mut shown: Vec<&(usize, Arc<str>)> = pictures[from..to]
        .iter()
        .filter(|(index, _)| seen.contains(index))
        .collect();
    shown.sort_by_key(|(index, _)| index.abs_diff(middle));
    let before = from.checked_sub(1).and_then(|i| pictures.get(i));
    let after = pictures.get(to);
    let mut keys: Vec<String> = Vec::new();
    for (_, key) in shown.into_iter().chain(after).chain(before) {
        if !keys.iter().any(|k| **k == **key) {
            keys.push(key.to_string());
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
        let later = Arc::clone(&display.later);
        let wanted = wanted(&seen, &later);
        let middle = seen
            .iter()
            .min()
            .zip(seen.iter().max())
            .map_or(0, |(first, last)| usize::midpoint(*first, *last));
        let pictures = &mut self.preview.inspection.pictures;
        pictures.seen = seen;
        pictures.later = later;
        pictures.middle = middle;
        if pictures.wanted == wanted {
            return;
        }
        pictures.wanted = wanted;
        if pictures
            .asking
            .as_ref()
            .is_some_and(|key| !pictures.wanted.contains(key))
        {
            pictures.cancel();
        }
        for decoded in pictures.evict() {
            cx.drop_image(decoded.picture.image, Some(window));
        }
        self.request_picture();
    }

    /// Ask for the first wanted picture not yet decoded, if none is on its
    /// way: one at a time, like the reader's pages.
    fn request_picture(&mut self) {
        let Some(PreviewContent::Inspection(display)) = &self.preview.content else {
            return;
        };
        let Some((request, view)) = display.origin.as_deref().cloned() else {
            return;
        };
        let pictures = &mut self.preview.inspection.pictures;
        if pictures.asked.is_pending() {
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
        pictures.asking = Some(key.clone());
        let previews = Arc::clone(&self.app.previews);
        pictures.asked.ask(
            &self.app,
            skwir_core::fs::reach::lane(&request.entry.path),
            self.app.lifetime(),
            Priority::Visible,
            "preview-picture",
            move |cancel| {
                let result = previews
                    .picture(&request, &view, &key, cancel)
                    .and_then(|image| {
                        let image =
                            image.ok_or_else(|| anyhow::anyhow!("the picture is not there"))?;
                        let bytes = image.bytes();
                        let picture = crate::backdrop::Picture::of(&image)
                            .ok_or_else(|| anyhow::anyhow!("invalid picture"))?;
                        Ok(Decoded { picture, bytes })
                    })
                    .map_err(|error| format!("{error:#}"));
                Reply { key, result }
            },
        );
    }

    /// Take the pictures that arrived, as the core wakes the UI.
    pub(crate) fn sync_pictures(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pictures = &mut self.preview.inspection.pictures;
        if let Some(reply) = pictures.asked.take() {
            pictures.asking = None;
            match reply.result {
                Ok(decoded) => {
                    if let Some(old) = Rc::make_mut(&mut pictures.loaded).insert(reply.key, decoded)
                    {
                        cx.drop_image(old.picture.image, Some(window));
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, "preview picture");
                    Rc::make_mut(&mut pictures.failed).insert(reply.key);
                }
            }
            for decoded in pictures.evict() {
                cx.drop_image(decoded.picture.image, Some(window));
            }
            self.request_picture();
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_drawn_comes_first_then_its_neighbours() {
        let pictures = Later::named([(2, "a"), (5, "b"), (9, "c"), (12, "d"), (20, "e")]);
        assert_eq!(wanted(&[8, 9, 10, 11, 12], &pictures), ["c", "d", "e", "b"]);
        assert_eq!(wanted(&[0, 1], &pictures), ["a"], "only the one after");
        assert_eq!(wanted(&[21, 22], &pictures), ["e"], "only the one before");
        assert!(wanted(&[], &pictures).is_empty());
        // One picture named by two blocks is asked for once.
        assert_eq!(
            wanted(&[3, 4, 5], &Later::named([(4, "x"), (5, "x")])),
            ["x"]
        );
    }

    /// A picture is as far from the view as the nearest block naming it;
    /// one no block names is the farthest, the first to go.
    #[test]
    fn a_picture_is_as_far_as_its_nearest_block() {
        let later = Later::named([(2, "a"), (10, "b"), (40, "a"), (41, "c")]);
        assert_eq!(later.distance("a", 38), 2, "the block after");
        assert_eq!(later.distance("a", 5), 3, "the block before");
        assert_eq!(later.distance("a", 0), 2);
        assert_eq!(later.distance("a", 90), 50);
        assert_eq!(later.distance("b", 10), 0);
        assert_eq!(later.distance("gone", 10), usize::MAX);
        assert!(Later::named([]).is_empty());
        assert!(!later.is_empty());
    }
}
