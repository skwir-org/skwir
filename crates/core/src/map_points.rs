//! What the Map view of a pane draws: the files the plugin that finds
//! where files are (the `geo` plugin) placed, handed over as they are,
//! a batch at a time. The view reads them from here when the plugin's
//! view descriptor says there is news, never as JSON (the command
//! `geo.points` gives scripts and agents the same batch, serialized).
use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::entry::EntryId;
use crate::workspace::PaneId;

/// A file placed on the map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub id: EntryId,
    pub lat: f64,
    pub lon: f64,
    /// A track's line, thinned: segments of `[lat, lon]`. Empty for a
    /// picture.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line: Vec<Vec<[f64; 2]>>,
    /// A picture's camera clock (`skwir_platform::exif::Taken::clock`):
    /// the order of pictures taken at one place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<u64>,
}

/// What a pane's map is handed: the points after the first `since` of a
/// generation (another generation starts over), and where the looking is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub generation: u64,
    pub points: Vec<Placed>,
    /// How many points there are in all.
    pub placed: usize,
    pub looked: usize,
    pub candidates: usize,
    pub listed: usize,
    /// Every file of the whole listing was looked at.
    pub complete: bool,
    /// The files are being looked at again beside what this says; it is
    /// replaced only if something moved.
    pub rechecking: bool,
}

/// Where a pane's points come from.
pub trait MapSource: Send + Sync {
    /// The points of `pane` after the first `since`, without the files of
    /// `withheld` (what an agent may not see), `None` while nothing is
    /// looked at for it.
    fn points(&self, pane: PaneId, since: usize, withheld: &HashSet<EntryId>) -> Option<Batch>;
}

/// The one map source, registered by its plugin.
#[derive(Default)]
pub struct MapSources {
    source: RwLock<Option<(String, Arc<dyn MapSource>)>>,
}

impl MapSources {
    pub fn register(&self, owner: &str, source: Arc<dyn MapSource>) {
        *self.source.write() = Some((owner.to_owned(), source));
    }

    pub fn owner(&self) -> Option<String> {
        self.source.read().as_ref().map(|(owner, _)| owner.clone())
    }

    /// The points of `pane` after the first `since`, for the window: no
    /// file withheld.
    pub fn points(&self, pane: PaneId, since: usize) -> Option<Batch> {
        let source = self.source.read().as_ref().map(|(_, s)| Arc::clone(s))?;
        source.points(pane, since, &HashSet::new())
    }

    pub fn unregister_owner(&self, owner: &str) {
        let mut source = self.source.write();
        if source.as_ref().is_some_and(|(o, _)| o == owner) {
            *source = None;
        }
    }
}
