//! Tile archives: a map's tiles kept in one file, `PMTiles` (`pmtiles.rs`)
//! or `MBTiles` (`mbtiles.rs`, SQLite), read with every length checked
//! (ADR-022, "Backgrounds from a local file"). Raster tiles (PNG, JPEG,
//! WebP) are decoded as downloaded ones are; vector tiles are said to be
//! vector and never drawn (ADR-020: no styling engine).
//!
//! What both formats say about themselves is one [`About`]; a tile is
//! asked of either by `z/x/y` as every tile server addresses it, and
//! answered with its bytes inflated. [`Source`] serves an archive's tiles
//! to a map, on the scheduler's workers.
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use skwir_core::CancelToken;
use skwir_core::geo::Bounds;
use skwir_core::preview::Held;
use skwir_core::scheduler::{Priority, Scheduler};
use skwir_core::tiles::{NoTile, SourceInfo, TileDone, TileRequest, TileSource};

/// A tile inflated is at most this large: a raster tile is tens of
/// kilobytes, a dense vector tile a few megabytes.
pub(crate) const TILE_INFLATED: usize = 16 * 1024 * 1024;
/// An attribution drawn on the map is cut there: it is one line.
const ATTRIBUTION_CHARS: usize = 160;

/// The two kinds of archive, as their names say and their first bytes
/// confirm: neither is in the MIME database, and an `MBTiles` file is any
/// SQLite database to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    PmTiles,
    MbTiles,
}

impl Kind {
    /// The kind a file's name says.
    pub(crate) fn of(path: &Path) -> Option<Kind> {
        match skwir_platform::inspection::extension(path).as_str() {
            "pmtiles" => Some(Kind::PmTiles),
            "mbtiles" => Some(Kind::MbTiles),
            _ => None,
        }
    }

    /// The kind its name says, when its first bytes (`head`, what a
    /// preview holds) are that kind's.
    pub(crate) fn detect(path: &Path, head: &[u8]) -> Option<Kind> {
        let kind = Kind::of(path)?;
        let magic: &[u8] = match kind {
            Kind::PmTiles => crate::pmtiles::MAGIC,
            Kind::MbTiles => b"SQLite format 3\0",
        };
        head.starts_with(magic).then_some(kind)
    }
}

/// What an archive's tiles are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
    Webp,
    Avif,
    /// Mapbox Vector Tiles: shapes to be styled, which skwir does not draw.
    Vector,
    #[default]
    Unknown,
}

impl Format {
    /// As `MBTiles`' `format` names it.
    pub(crate) fn named(name: &str) -> Format {
        match name.trim().to_ascii_lowercase().as_str() {
            "png" => Format::Png,
            "jpg" | "jpeg" => Format::Jpeg,
            "webp" => Format::Webp,
            "avif" => Format::Avif,
            "pbf" | "mvt" => Format::Vector,
            _ => Format::Unknown,
        }
    }

    /// Whether a map draws its tiles: pictures the decoder reads.
    pub(crate) fn drawn(self) -> bool {
        matches!(self, Format::Png | Format::Jpeg | Format::Webp)
    }

    pub(crate) fn describe(self) -> &'static str {
        match self {
            Format::Png => "PNG pictures",
            Format::Jpeg => "JPEG pictures",
            Format::Webp => "WebP pictures",
            Format::Avif => "AVIF pictures",
            Format::Vector => "Vector shapes",
            Format::Unknown => "Unknown",
        }
    }
}

/// What an archive says about itself.
#[derive(Debug, Default)]
pub(crate) struct About {
    /// The container: "`PMTiles` 3", "`MBTiles`".
    pub container: String,
    pub format: Format,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// Where it has tiles; `None` for the whole world.
    pub bounds: Option<Bounds>,
    pub name: String,
    pub description: String,
    /// As text: archives often write it as HTML links.
    pub attribution: String,
    pub version: String,
    /// The layers of vector tiles.
    pub layers: Vec<String>,
    /// How many tiles it holds, when it says so without a count.
    pub tiles: Option<u64>,
    /// Why its tiles, pictures though they are, cannot be read.
    pub unreadable: Option<String>,
}

impl About {
    /// The facts its metadata gives, as `PMTiles`' JSON and `MBTiles`' table
    /// (its `json` row merged in) both name them.
    pub(crate) fn read_metadata(&mut self, meta: &serde_json::Map<String, serde_json::Value>) {
        let text = |key: &str| {
            meta.get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_owned()
        };
        self.name = text("name");
        self.description = text("description");
        self.attribution = plain(&text("attribution"));
        self.version = text("version");
        self.layers = meta
            .get("vector_layers")
            .and_then(serde_json::Value::as_array)
            .map(|layers| {
                layers
                    .iter()
                    .filter_map(|layer| layer.get("id")?.as_str())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
    }
}

/// A file of tiles, opened.
pub(crate) trait Archive: Send + Sync {
    fn about(&self) -> &About;
    /// Tile `z/x/y`, `y` counted from the north, inflated; `None` when the
    /// archive does not have it.
    fn tile(&self, z: u8, x: u32, y: u32, cancel: &CancelToken) -> anyhow::Result<Option<Vec<u8>>>;
}

/// Open the archive a preview holds (ADR-101), as its name and first
/// bytes say it is: a `PMTiles` file read through what is held, an
/// `MBTiles` database by SQLite, which reads by path.
pub(crate) fn open(held: &Arc<Held>, cancel: &CancelToken) -> anyhow::Result<Arc<dyn Archive>> {
    match Kind::detect(held.path(), held.head()) {
        Some(Kind::PmTiles) => Ok(Arc::new(crate::pmtiles::PmTiles::open(held)?)),
        Some(Kind::MbTiles) => Ok(Arc::new(crate::mbtiles::MbTiles::of_held(held, cancel)?)),
        None => anyhow::bail!("Not a tile archive"),
    }
}

/// How a part of an archive is compressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Compression {
    None,
    Gzip,
    Brotli,
    Zstd,
    /// The archive does not say: gzip if it starts as gzip does.
    Unknown,
}

impl Compression {
    /// Why skwir cannot read what is compressed this way, if it cannot.
    pub(crate) fn unreadable(self) -> Option<&'static str> {
        match self {
            Compression::Brotli => Some("compressed with Brotli, which skwir does not read"),
            Compression::Zstd => Some("compressed with Zstandard, which skwir does not read"),
            Compression::None | Compression::Gzip | Compression::Unknown => None,
        }
    }
}

/// `bytes` inflated, within `limit`: more is an error, never a truncation.
pub(crate) fn inflate(
    bytes: Vec<u8>,
    compression: Compression,
    limit: usize,
) -> anyhow::Result<Vec<u8>> {
    if let Some(why) = compression.unreadable() {
        anyhow::bail!("The tiles are {why}");
    }
    let gzip = match compression {
        Compression::Gzip => true,
        Compression::Unknown => bytes.starts_with(&[0x1f, 0x8b]),
        _ => false,
    };
    if !gzip {
        anyhow::ensure!(bytes.len() <= limit, "A part of the archive is too large");
        return Ok(bytes);
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .take(limit as u64 + 1)
        .read_to_end(&mut out)?;
    anyhow::ensure!(out.len() <= limit, "A part of the archive is too large");
    Ok(out)
}

/// The text of an attribution written as HTML (`<a href="…">© Example</a>`),
/// on one line: tags left out, the common entities read.
pub(crate) fn plain(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        // A tag begins with a name, `/` or `!`; any other `<` is text.
        let tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
        if let Some(end) = after.find('>').filter(|_| tag) {
            rest = &after[end + 1..];
        } else {
            text.push('<');
            rest = after;
        }
    }
    text.push_str(rest);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let entity = &rest[start..];
        let read = entity.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &entity[1..end];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                "copy" => '©',
                _ => {
                    let code = match name.strip_prefix('#') {
                        Some(hex) if hex.starts_with(['x', 'X']) => {
                            u32::from_str_radix(&hex[1..], 16).ok()
                        }
                        Some(decimal) => decimal.parse().ok(),
                        None => None,
                    };
                    char::from_u32(code?)?
                }
            };
            Some((c, end + 1))
        });
        if let Some((c, len)) = read {
            out.push(c);
            rest = &entity[len..];
        } else {
            out.push('&');
            rest = &entity[1..];
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An archive's tiles for a map (`tiles::Layer`), each read and decoded on
/// a worker. Its requests end with the plugin (`lifetime`).
pub(crate) struct Source {
    pub archive: Arc<dyn Archive>,
    pub scheduler: Scheduler,
    pub lifetime: CancelToken,
}

impl TileSource for Source {
    fn info(&self) -> SourceInfo {
        let about = self.archive.about();
        let mut attribution: String = about.attribution.chars().take(ATTRIBUTION_CHARS).collect();
        if attribution.len() < about.attribution.len() {
            attribution.push('…');
        }
        SourceInfo {
            attribution,
            attribution_url: None,
            max_zoom: about.max_zoom,
            min_zoom: about.min_zoom,
            bounds: about.bounds,
            // A picture of the place, which a graticule would only cover.
            detailed: true,
            setting: None,
        }
    }

    fn request(&self, request: TileRequest, cancel: CancelToken, done: TileDone) {
        let archive = Arc::clone(&self.archive);
        let cancel = self.lifetime.linked(&cancel);
        let token = cancel.clone();
        self.scheduler
            .spawn(Priority::Visible, "map-tile", cancel, move || {
                let tile = match archive.tile(request.z, request.x, request.y, &token) {
                    Ok(Some(bytes)) => skwir_platform::inspection::image(&bytes),
                    Ok(None) => Err(NoTile.into()),
                    Err(error) => Err(error),
                };
                if !token.is_cancelled() {
                    done(tile);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attribution_in_html_reads_as_its_text() {
        assert_eq!(
            plain(
                r#"<a href="https://example.org/" target="_blank">&copy; Example Maps</a>  <a href="https://example.com">&#169;&nbsp;Contributors &amp; co</a>"#
            ),
            "© Example Maps © Contributors & co"
        );
        assert_eq!(plain("Plain & simple, a < b"), "Plain & simple, a < b");
        assert_eq!(plain("&#x41;&unknown;"), "A&unknown;");
        assert_eq!(plain("<b>unclosed"), "unclosed");
    }

    #[test]
    fn what_is_inflated_is_bounded() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&[7u8; 1000]).unwrap();
        let packed = gz.finish().unwrap();
        assert_eq!(
            inflate(packed.clone(), Compression::Gzip, 1000).unwrap(),
            [7u8; 1000]
        );
        assert_eq!(
            inflate(packed.clone(), Compression::Unknown, 1000)
                .unwrap()
                .len(),
            1000
        );
        assert!(inflate(packed, Compression::Gzip, 999).is_err());
        assert_eq!(
            inflate(b"raw".to_vec(), Compression::Unknown, 3).unwrap(),
            b"raw"
        );
        assert!(inflate(b"raw".to_vec(), Compression::Brotli, 3).is_err());
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(128))]
        /// A tile archive as no tool writes it, the user's choice of
        /// background: opened and read, or refused; never a panic.
        #[test]
        fn any_tile_archive_is_read_without_a_panic(bytes in proptest::prop_oneof![
            skwir_core::testing::mutated(&include_bytes!("testdata/hills.pmtiles")[..]),
            skwir_core::testing::mutated(&include_bytes!("testdata/roads.pmtiles")[..]),
            skwir_core::testing::bytes(1024),
        ]) {
            let held = Arc::new(Held::of_bytes("any.pmtiles", bytes));
            let cancel = CancelToken::new();
            if let Ok(archive) = open(&held, &cancel) {
                let _ = archive.about();
                for (z, x, y) in [(0, 0, 0), (1, 1, 0), (2, 1, 2), (14, 8000, 5000)] {
                    let _ = archive.tile(z, x, y, &cancel);
                }
            }
        }
    }
}
