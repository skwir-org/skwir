//! The `position` field (ADR-023): where a picture or a video says it was
//! made, read when a search (`near:`, `inside:`) or a view asks, never at
//! hydration (ADR-020). Pictures by their EXIF, videos by their MP4 or
//! `QuickTime` location; the answer is kept by the file's identity in the
//! core's fields cache. And `geo.place`, the words for that position
//! (`places::describe`), which Details shows as "Place", and `geo.clock`,
//! the camera's clock, which orders the pictures taken at one place on
//! the Map. Asked together (`Fields::get_all`), they are one read.
use std::ffi::OsStr;
use std::path::Path;

use skwir_core::CancelToken;
use skwir_core::entry::{Value, keys};
use skwir_core::fields::FieldProvider;
use skwir_platform::mime::{self, Kind};

pub struct Positions;

/// The words for where a picture or a video was made.
pub const PLACE: &str = "geo.place";
/// A picture's camera clock (`skwir_platform::exif::Taken::clock`).
pub const CLOCK: &str = "geo.clock";

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
    /// Advanced with the place names' data.
    fn version(&self) -> u32 {
        1
    }
    fn keys(&self) -> &'static [&'static str] {
        &[keys::POSITION, PLACE, CLOCK]
    }
    fn label(&self, key: &str) -> Option<&'static str> {
        (key == PLACE).then_some("Place")
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
                    PLACE => taken
                        .at
                        .and_then(crate::places::describe)
                        .map(|words| Value::Text(words.into())),
                    CLOCK => taken.clock.map(Value::UInt),
                    _ => taken.at.map(skwir_core::geo::LatLon::to_value),
                })
            })
            .collect()
    }
}
