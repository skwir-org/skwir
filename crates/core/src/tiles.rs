//! Map backgrounds: square tiles of the Web Mercator world, addressed as
//! every tile server addresses them (`z/x/y`, ADR-020). The UI draws them
//! under what a file places on the map and knows no source; a plugin
//! registers the one source in use and says where its tiles come from.
//!
//! A request is asynchronous: the source answers through `done`, on any
//! thread, when it has the tile. A download never holds a scheduler worker
//! while it waits, and cancelling the request (the tile left the view)
//! drops it wherever it is.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

use crate::{CancelToken, thumbs::Image};

/// The colours a source that draws its tiles itself uses, from the UI's
/// theme (RGBA). Part of what a tile is: another theme is another tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Palette {
    pub land: [u8; 4],
    pub water: [u8; 4],
    pub border: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileRequest {
    pub z: u8,
    pub x: u32,
    pub y: u32,
    /// Device pixels per tile pixel (1, or 2 on a dense screen): a source
    /// that draws may draw sharper; one that downloads may ignore it.
    pub scale: u8,
    pub palette: Palette,
}

/// What the UI says about a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInfo {
    /// Shown on the map whenever one of its tiles is: the licence of
    /// OpenStreetMap's data requires it.
    pub attribution: String,
    /// Where the attribution leads when clicked.
    pub attribution_url: Option<String>,
    /// The deepest zoom it serves; deeper views enlarge these tiles.
    pub max_zoom: u8,
    /// Whether it shows streets. A map on a source that does not draws a
    /// graticule over it and, zoomed in, says where detail can be had.
    pub detailed: bool,
    /// The setting that chooses the source, for the UI to link to.
    pub setting: Option<String>,
}

/// A tile or why there is none.
pub type TileResult = anyhow::Result<Image>;
/// Called once with the tile, unless the request was cancelled first.
pub type TileDone = Box<dyn FnOnce(TileResult) + Send>;

pub trait TileSource: Send + Sync {
    fn info(&self) -> SourceInfo;
    /// Start getting a tile; answer through `done`. Must return at once:
    /// it is called on the UI thread. Of the requests waiting, the newest
    /// is served first, as the scheduler serves its jobs: a map asks for
    /// the tiles farthest from its middle first, so the middle comes first.
    fn request(&self, request: TileRequest, cancel: CancelToken, done: TileDone);
}

struct Registered {
    owner: String,
    source: Arc<dyn TileSource>,
    lifetime: CancelToken,
}

/// The source in use, if a plugin gives one.
#[derive(Default)]
pub struct Tiles {
    source: RwLock<Option<Registered>>,
    revision: AtomicU64,
}

impl Tiles {
    pub fn register(&self, owner: &str, source: Arc<dyn TileSource>) {
        let old = self.source.write().replace(Registered {
            owner: owner.to_owned(),
            source,
            lifetime: CancelToken::new(),
        });
        if let Some(old) = old {
            old.lifetime.cancel();
        }
        self.changed();
    }

    /// The source, and a token that ends with its registration: requests
    /// are made under a child of it, so a source withdrawn cannot answer.
    pub fn source(&self) -> Option<(Arc<dyn TileSource>, CancelToken)> {
        self.source
            .read()
            .as_ref()
            .map(|r| (Arc::clone(&r.source), r.lifetime.clone()))
    }

    pub fn owner(&self) -> Option<String> {
        self.source.read().as_ref().map(|r| r.owner.clone())
    }

    /// Advances when the source comes, goes, or changes what it draws (a
    /// setting chose another): tiles of an older revision are not shown.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn changed(&self) {
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn unregister_owner(&self, owner: &str) {
        let mut source = self.source.write();
        if source.as_ref().is_some_and(|r| r.owner == owner) {
            if let Some(old) = source.take() {
                old.lifetime.cancel();
            }
            drop(source);
            self.changed();
        }
    }
}

/// The tiles covering a world rectangle (`x` continuous past 1, as the
/// map keeps it) at zoom `z`: `(x, y)` addresses, the x brought back onto
/// the world, and where each is drawn as whole tiles from `(x0, y0)`.
pub fn cover(z: u8, (x0, y0): (f64, f64), (x1, y1): (f64, f64)) -> Vec<(u32, u32, i64, i64)> {
    let n = 1i64 << z;
    let side = n as f64;
    let (left, right) = ((x0 * side).floor() as i64, (x1 * side).ceil() as i64);
    let (top, bottom) = (
        ((y0 * side).floor() as i64).max(0),
        ((y1 * side).ceil() as i64).min(n),
    );
    let mut tiles = Vec::new();
    for ty in top..bottom {
        for tx in left..right {
            tiles.push((tx.rem_euclid(n) as u32, ty as u32, tx, ty));
        }
    }
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Nothing;
    impl TileSource for Nothing {
        fn info(&self) -> SourceInfo {
            SourceInfo {
                attribution: String::new(),
                attribution_url: None,
                max_zoom: 0,
                detailed: false,
                setting: None,
            }
        }
        fn request(&self, _: TileRequest, _: CancelToken, _: TileDone) {}
    }

    #[test]
    fn a_withdrawn_source_ends_its_requests() {
        let tiles = Tiles::default();
        tiles.register("maps", Arc::new(Nothing));
        let (_, lifetime) = tiles.source().unwrap();
        let before = tiles.revision();
        tiles.unregister_owner("other");
        assert!(tiles.source().is_some());
        tiles.unregister_owner("maps");
        assert!(lifetime.is_cancelled());
        assert!(tiles.source().is_none());
        assert!(tiles.revision() > before);
    }

    #[test]
    fn a_view_is_covered_by_whole_tiles_wrapped_onto_the_world() {
        // Zoom 1: four tiles. A view across x = 1 takes the first column
        // again, drawn to the east.
        let tiles = cover(1, (0.75, 0.25), (1.25, 0.75));
        assert_eq!(
            tiles,
            [(1, 0, 1, 0), (0, 0, 2, 0), (1, 1, 1, 1), (0, 1, 2, 1)]
        );
        // Nothing above the pole.
        assert_eq!(cover(0, (0.2, -0.5), (0.3, 0.1)), [(0, 0, 0, 0)]);
    }
}
