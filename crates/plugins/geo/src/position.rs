//! The `position` field (ADR-023): where a picture or a video says it was
//! made, read when a search (`near:`, `inside:`) or a view asks, never at
//! hydration (ADR-020). Pictures by their EXIF, videos by their MP4 or
//! `QuickTime` location; the answer is kept by the file's identity in the
//! core's fields cache. And `geo.place`, the words for that position
//! (`places::describe`), which Properties shows as "Place", and `taken`,
//! the camera's clock (`keys::TAKEN`), which orders the pictures taken at
//! one place on the Map and dates them on the timeline. Asked together
//! (`Fields::get_all`), they are one read.
use std::ffi::OsStr;
use std::path::Path;

use skwir_core::CancelToken;
use skwir_core::entry::{Value, keys};
use skwir_core::fields::FieldProvider;
use skwir_core::words;
use skwir_core::words::Words;
use skwir_platform::mime::{self, Kind};

pub struct Positions;

/// The words for where a picture or a video was made.
pub const PLACE: &str = "geo.place";

/// What a file of this name is, if its position can be read.
fn kind(name: &OsStr) -> Option<Kind> {
    let mime = mime::by_name(name)?;
    [Kind::Image, Kind::Video]
        .into_iter()
        .find(|kind| mime::is_kind(&mime, *kind))
}

impl FieldProvider for Positions {
    fn id(&self) -> &'static str {
        "geo-positions"
    }
    /// Advanced with the place names' data, and with what a place keeps:
    /// 2, where it is as data (`places::Placed`), said where it is shown.
    fn version(&self) -> u32 {
        2
    }
    fn keys(&self) -> &'static [&'static str] {
        &[keys::POSITION, PLACE, keys::TAKEN]
    }
    fn label(&self, key: &str) -> Option<Words> {
        (key == PLACE).then(|| {
            words!(
                geo_place_label = "Place",
                note = "A row's label in a picture's or a video's Properties: the town it was taken in or near."
            )
        })
    }
    /// A place, in words: its name, or how far and which way from one.
    fn said(&self, key: &str, value: &Value) -> Option<Words> {
        (key == PLACE)
            .then(|| crate::places::Placed::of(value))
            .flatten()
            .map(|placed| placed.words())
    }
    /// `GeoNames`' names are credited (CC BY 4.0); Natural Earth's are in
    /// the public domain.
    fn credit(&self, key: &str) -> Option<String> {
        (key == PLACE)
            .then(crate::places::attribution)
            .flatten()
            .map(str::to_owned)
    }
    fn accepts(&self, _: &str, name: &OsStr) -> bool {
        kind(name).is_some()
    }
    fn read(&self, key: &str, path: &Path, cancel: &CancelToken) -> anyhow::Result<Option<Value>> {
        self.read_keys(&[key], path, cancel)
            .pop()
            .unwrap_or(Ok(None))
    }
    fn read_keys(
        &self,
        keys: &[&str],
        path: &Path,
        _: &CancelToken,
    ) -> Vec<anyhow::Result<Option<Value>>> {
        // A link's content is read, and its name types it.
        let name = skwir_core::fs::content_path(path)
            .and_then(|content| content.file_name().map(std::ffi::OsStr::to_owned));
        let Some(kind) = name.as_deref().and_then(kind) else {
            return keys.iter().map(|_| Ok(None)).collect();
        };
        let taken = if kind == Kind::Video {
            skwir_platform::mp4::taken(path)
        } else {
            skwir_platform::exif::taken(path)
        };
        keys.iter()
            .map(|&key| {
                Ok(match key {
                    // Kept as data, by the file's identity; said where it
                    // is shown (`said`).
                    PLACE => taken
                        .at
                        .and_then(crate::places::describe)
                        .map(|placed| placed.to_value()),
                    keys::TAKEN => taken.clock.map(Value::UInt),
                    _ => taken.at.map(skwir_core::geo::LatLon::to_value),
                })
            })
            .collect()
    }
}
