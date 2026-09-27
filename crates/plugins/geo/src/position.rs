//! The `position` field (ADR-023): where a picture or a video says it was
//! made, read when a search (`near:`, `inside:`) or a view asks, never at
//! hydration (ADR-020). Pictures by their EXIF, videos by their MP4 or
//! `QuickTime` location; the answer is kept by the file's identity in the
//! core's fields cache. And `geo.place`, the words for that position
//! (`places::describe`), which Details shows as "Place".
use std::ffi::OsStr;
use std::path::Path;

use bibli_core::CancelToken;
use bibli_core::entry::{Value, keys};
use bibli_core::fields::FieldProvider;
use bibli_platform::mime::{self, Kind};

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
    /// Advanced with the place names' data.
    fn version(&self) -> u32 {
        1
    }
    fn keys(&self) -> &'static [&'static str] {
        &[keys::POSITION, PLACE]
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
    fn read(&self, key: &str, path: &Path, _: &CancelToken) -> anyhow::Result<Option<Value>> {
        let Some(kind) = path.file_name().and_then(kind) else {
            return Ok(None);
        };
        let taken = if kind == Kind::Video {
            bibli_platform::mp4::taken(path)
        } else {
            bibli_platform::exif::taken(path)
        };
        Ok(if key == PLACE {
            taken
                .at
                .and_then(crate::places::describe)
                .map(|words| Value::Text(words.into()))
        } else {
            taken.at.map(bibli_core::geo::LatLon::to_value)
        })
    }
}
