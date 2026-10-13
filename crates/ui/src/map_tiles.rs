//! The tiles maps are drawn on (ADR-020), as the UI keeps them: asked of
//! the registered source for what a map shows, decoded and uploaded on the
//! source's thread, kept up to `BUDGET` bytes while maps are shown, and
//! let go (their atlas entries too) when no map is.
//!
//! A map asks in its `prepaint` for the tiles it covers (`get`); what it
//! did not ask for in a frame and is still on its way is cancelled
//! (`end_frame`), so panning past a place never downloads it. A tile of
//! another source revision (the setting chose another background) is
//! never shown. A file's own tiles (a tile archive's `Layer`, drawn by its
//! preview over the background) are kept beside the background's, by the
//! layer's id; one the file does not have is left blank and never asked
//! for again.
use std::collections::HashMap;
use std::sync::Arc;

use gpui::RenderImage;
use skwir_core::CancelToken;
use skwir_core::tiles::{Layer, NoTile, Palette, SourceInfo, TileRequest, Tiles};

/// How long a tile that failed is left blank before it is asked again.
const RETRY: std::time::Duration = std::time::Duration::from_secs(30);

/// Decoded tiles kept: at 256 pixels a side, about 190 of them; at 512,
/// about 48, several screens of a large panel.
const BUDGET: usize = 48 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    /// 0 for the background's tiles, else the file's `Layer::id`.
    source: u64,
    /// The background's revision; 0 for a file's, which never changes.
    revision: u64,
    request: TileRequest,
}

enum Entry {
    Loading(CancelToken),
    Ready {
        image: Arc<RenderImage>,
        bytes: usize,
        used: u64,
    },
    /// Asked again once `RETRY` has passed, or for another source.
    Failed(std::time::Instant),
    /// A tile the file does not have (`NoTile`): blank, never asked again.
    Empty,
}

/// Why a tile did not come.
enum Lack {
    NoTile,
    Failed(String),
}

type Arrival = (Key, Result<Arc<RenderImage>, Lack>);

/// A tile that could not be had: of which source (`Key::source`), of which
/// revision, and why.
pub struct Failure {
    pub source: u64,
    pub revision: u64,
    pub error: String,
}

/// How long a view waited for its tiles: from the first frame that lacked
/// one to the first that had them all. Traced at debug level
/// (`RUST_LOG=skwir_ui::map_tiles=debug`), the measure of this module.
struct Waiting {
    since: std::time::Instant,
    asked: usize,
    frames: usize,
    /// When the tile at the middle of the view was drawn.
    middle_ms: Option<u64>,
}

/// The backgrounds to choose from, as the setting that chooses them lists
/// them, and the one chosen.
#[derive(Clone, Debug)]
pub struct Backgrounds {
    pub setting: String,
    pub current: String,
    /// Values and labels.
    pub choices: Vec<(String, String)>,
}

impl Backgrounds {
    pub fn label(&self) -> &str {
        self.choices
            .iter()
            .find(|(value, _)| *value == self.current)
            .map_or(self.current.as_str(), |(_, label)| label.as_str())
    }
}

pub struct TileStore {
    tiles: Arc<Tiles>,
    wake: skwir_core::location::Waker,
    tx: flume::Sender<Arrival>,
    rx: flume::Receiver<Arrival>,
    entries: HashMap<Key, Entry>,
    frame: u64,
    /// Tiles on their way that a map asked for this frame.
    wanted: Vec<Key>,
    ready: usize,
    /// Let go, waiting for a window to evict their atlas entries.
    evicted: Vec<Arc<RenderImage>>,
    info: Option<(u64, Option<SourceInfo>, Option<Backgrounds>)>,
    app: Arc<skwir_core::App>,
    /// Why the last tile failed, until someone says so.
    pub failure: Option<Failure>,
    /// A view waiting for tiles, measured until it has them all.
    waiting: Option<Waiting>,
    /// Tiles the frame being drawn lacks, and those it asked for.
    lacking: usize,
    asked: usize,
    /// The tile at the middle of the view drawn this frame, for the measure.
    middle: Option<Key>,
}

impl TileStore {
    pub fn new(app: &Arc<skwir_core::App>) -> TileStore {
        let (tx, rx) = flume::unbounded();
        TileStore {
            tiles: Arc::clone(&app.tiles),
            wake: app.waker_handle(),
            tx,
            rx,
            entries: HashMap::new(),
            frame: 0,
            wanted: Vec::new(),
            ready: 0,
            evicted: Vec::new(),
            info: None,
            failure: None,
            waiting: None,
            lacking: 0,
            asked: 0,
            middle: None,
            app: Arc::clone(app),
        }
    }

    pub fn revision(&self) -> u64 {
        self.tiles.revision()
    }

    /// What the source says about itself, and the backgrounds its setting
    /// offers: read once per revision, which a change of that setting
    /// advances.
    fn refresh(&mut self) {
        let revision = self.revision();
        if self.info.as_ref().is_some_and(|(r, _, _)| *r == revision) {
            return;
        }
        let info = self.tiles.source().map(|(source, _)| source.info());
        let backgrounds = info
            .as_ref()
            .and_then(|info| info.setting.as_deref())
            .and_then(|key| {
                let spec = self.app.settings.spec(key)?;
                let current = self.app.settings.text(key).unwrap_or_else(|| {
                    skwir_core::settings::SettingSpec::format_value(&spec.default)
                });
                let choices = spec
                    .constraints
                    .choices
                    .iter()
                    .map(|value| (value.clone(), spec.choice_label(value).to_owned()))
                    .collect();
                Some(Backgrounds {
                    setting: key.to_owned(),
                    current,
                    choices,
                })
            });
        self.info = Some((revision, info, backgrounds));
    }

    pub fn info(&mut self) -> Option<SourceInfo> {
        self.refresh();
        self.info.as_ref().and_then(|(_, info, _)| info.clone())
    }

    pub fn backgrounds(&mut self) -> Option<Backgrounds> {
        self.refresh();
        self.info
            .as_ref()
            .and_then(|(_, _, backgrounds)| backgrounds.clone())
    }

    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.lacking = 0;
        self.asked = 0;
        self.middle = None;
    }

    /// The key of a tile of the background (`file` none) or of a file.
    fn key(&self, file: Option<&Layer>, request: TileRequest) -> Key {
        match file {
            Some(file) => Key {
                source: file.id,
                revision: 0,
                request,
            },
            None => Key {
                source: 0,
                revision: self.revision(),
                request,
            },
        }
    }

    /// The background's tile at the middle of the view, for the measure
    /// only.
    pub fn middle(&mut self, request: TileRequest) {
        self.middle = Some(self.key(None, request));
    }

    /// The tile of the background (`file` none) or of a file, if it is
    /// here; asked for if it is not.
    pub fn get(&mut self, file: Option<&Layer>, request: TileRequest) -> Option<Arc<RenderImage>> {
        let key = self.key(file, request);
        let frame = self.frame;
        match self.entries.get_mut(&key) {
            Some(Entry::Ready { image, used, .. }) => {
                *used = frame;
                return Some(Arc::clone(image));
            }
            Some(Entry::Loading(_)) => {
                // Still wanted: `end_frame` keeps it.
                self.wanted.push(key);
                self.lacking += 1;
                return None;
            }
            Some(Entry::Failed(when)) if when.elapsed() < RETRY => return None,
            Some(Entry::Empty) => return None,
            Some(Entry::Failed(_)) | None => {}
        }
        // A file's source lives as long as its preview holds it; the
        // background's ends with its registration.
        let (source, cancel) = if let Some(file) = file {
            (Arc::clone(&file.source), CancelToken::new())
        } else {
            let (source, lifetime) = self.tiles.source()?;
            (source, lifetime.child())
        };
        let (tx, wake) = (self.tx.clone(), Arc::clone(&self.wake));
        self.entries.insert(key, Entry::Loading(cancel.clone()));
        self.wanted.push(key);
        self.lacking += 1;
        self.asked += 1;
        source.request(
            request,
            cancel,
            Box::new(move |tile| {
                let image = tile
                    .map_err(|error| {
                        if error.is::<NoTile>() {
                            Lack::NoTile
                        } else {
                            Lack::Failed(format!("{error:#}"))
                        }
                    })
                    .and_then(|image| {
                        crate::thumbs::to_render_image(&image)
                            .ok_or_else(|| Lack::Failed("the tile is not a picture".to_owned()))
                    });
                if tx.send((key, image)).is_ok() {
                    wake();
                }
            }),
        );
        None
    }

    /// A tile already here, without asking for it: a coarser one stands in
    /// while a finer one comes.
    pub fn cached(
        &mut self,
        file: Option<&Layer>,
        request: TileRequest,
    ) -> Option<Arc<RenderImage>> {
        let key = self.key(file, request);
        match self.entries.get_mut(&key) {
            Some(Entry::Ready { image, used, .. }) => {
                *used = self.frame;
                Some(Arc::clone(image))
            }
            _ => None,
        }
    }

    /// After a window frame's maps asked for what they show (the shell
    /// calls it once a frame, never a map: two maps drawn in one frame
    /// would each cancel what the other asked for): cancel what none of
    /// them shows any more, forget other revisions, and keep the kept
    /// tiles in budget, the least recently drawn going first.
    pub fn end_frame(&mut self) {
        let middle_drawn = self
            .middle
            .is_some_and(|key| matches!(self.entries.get(&key), Some(Entry::Ready { .. })));
        match (&mut self.waiting, self.lacking) {
            (None, 0) => {}
            (None, _) => {
                self.waiting = Some(Waiting {
                    since: std::time::Instant::now(),
                    asked: self.asked,
                    frames: 1,
                    middle_ms: None,
                });
            }
            (Some(waiting), 0) => {
                let ms = waiting.since.elapsed().as_millis() as u64;
                tracing::debug!(
                    ms,
                    middle_ms = waiting.middle_ms.unwrap_or(ms),
                    asked = waiting.asked,
                    frames = waiting.frames + 1,
                    "map tiles: the view has them all"
                );
                self.waiting = None;
            }
            (Some(waiting), _) => {
                waiting.asked += self.asked;
                waiting.frames += 1;
                if middle_drawn && waiting.middle_ms.is_none() {
                    waiting.middle_ms = Some(waiting.since.elapsed().as_millis() as u64);
                }
            }
        }
        let revision = self.revision();
        let wanted: std::collections::HashSet<Key> = self.wanted.drain(..).collect();
        let mut gone = Vec::new();
        for (key, entry) in &self.entries {
            let stale = key.source == 0 && key.revision != revision;
            match entry {
                Entry::Loading(cancel) if stale || !wanted.contains(key) => {
                    cancel.cancel();
                    gone.push(*key);
                }
                Entry::Ready { .. } | Entry::Failed(_) if stale => gone.push(*key),
                _ => {}
            }
        }
        for key in gone {
            self.remove(&key);
        }
        if self.ready > BUDGET {
            let mut kept: Vec<(u64, Key)> = self
                .entries
                .iter()
                .filter_map(|(key, entry)| match entry {
                    Entry::Ready { used, .. } if *used < self.frame => Some((*used, *key)),
                    _ => None,
                })
                .collect();
            kept.sort_unstable_by_key(|(used, _)| *used);
            for (_, key) in kept {
                if self.ready <= BUDGET {
                    break;
                }
                self.remove(&key);
            }
        }
    }

    fn remove(&mut self, key: &Key) {
        match self.entries.remove(key) {
            Some(Entry::Ready { image, bytes, .. }) => {
                self.ready -= bytes;
                self.evicted.push(image);
            }
            Some(Entry::Loading(cancel)) => cancel.cancel(),
            _ => {}
        }
    }

    /// Take what the source sent. True when a map has more to draw.
    pub fn receive(&mut self) -> bool {
        let mut arrived = false;
        while let Ok((key, result)) = self.rx.try_recv() {
            // Only what is still awaited: a tile cancelled on its way stays
            // gone.
            if !matches!(self.entries.get(&key), Some(Entry::Loading(_))) {
                if let Ok(image) = result {
                    self.evicted.push(image);
                }
                continue;
            }
            match result {
                Ok(image) => {
                    let size = image.size(0);
                    let bytes = usize::try_from(size.width.0).unwrap_or(0)
                        * usize::try_from(size.height.0).unwrap_or(0)
                        * 4;
                    self.ready += bytes;
                    self.entries.insert(
                        key,
                        Entry::Ready {
                            image,
                            bytes,
                            used: self.frame,
                        },
                    );
                }
                Err(Lack::NoTile) => {
                    self.entries.insert(key, Entry::Empty);
                }
                Err(Lack::Failed(error)) => {
                    self.entries
                        .insert(key, Entry::Failed(std::time::Instant::now()));
                    self.failure = Some(Failure {
                        source: key.source,
                        revision: key.revision,
                        error,
                    });
                }
            }
            arrived = true;
        }
        arrived
    }

    /// Let every tile go: no map is shown.
    pub fn clear(&mut self) {
        let keys: Vec<Key> = self.entries.keys().copied().collect();
        for key in keys {
            self.remove(&key);
        }
        self.wanted.clear();
    }

    /// Tiles let go, for their atlas entries to be evicted with a window.
    pub fn take_evicted(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.evicted)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The palette a drawn source uses, from the theme's map tokens.
pub fn palette(theme: &crate::theme::Theme) -> Palette {
    let rgba = |color: gpui::Hsla| {
        let c = gpui::Rgba::from(color);
        [c.r, c.g, c.b, c.a].map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
    };
    Palette {
        land: rgba(theme.map_land),
        water: rgba(theme.map_water),
        border: rgba(theme.map_border),
    }
}
