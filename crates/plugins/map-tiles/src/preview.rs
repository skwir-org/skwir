//! Tile archives previewed as the maps they hold: what the file says of
//! itself in Details, and on the preview's map its tiles over the
//! background where it has them (`Inspection::tiles`), fitted to its area.
//! Tiles bibli does not draw (vector shapes, AVIF, Brotli) leave the area
//! outlined instead, and the notice says why.
use std::sync::Arc;

use bibli_core::CancelToken;
use bibli_core::geo::{Area, Bounds, GeoDoc, LatLon};
use bibli_core::inspection::{InspectRequest, Inspection, InspectionKind};
use bibli_core::preview::{PreviewProvider, PreviewRequest};
use bibli_core::scheduler::Scheduler;
use bibli_core::tiles::Layer;
use bibli_platform::inspection::{check, details};

use crate::archive::{self, Archive, Format, Kind};

/// The world as Web Mercator shows it: an archive that says nothing of
/// its area covers it.
const WORLD: Bounds = Bounds {
    south: -85.0511,
    north: 85.0511,
    west: -180.,
    east: 180.,
};

pub(crate) struct Provider {
    pub scheduler: Scheduler,
    /// Ends with the plugin: an archive's tiles are no longer read.
    pub lifetime: CancelToken,
}

impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        crate::ID
    }

    /// Before the tables preview, which takes any SQLite database: an
    /// `.mbtiles` file is a map. Settled by content, never a tie.
    fn priority(&self) -> i32 {
        20
    }

    /// On a worker: the file's first bytes are asked what it is.
    fn accepts(&self, req: &PreviewRequest) -> bool {
        Kind::of(&req.entry.path).is_some()
            && req
                .entry
                .resolve()
                .is_ok_and(|path| Kind::detect(&path).is_some())
    }

    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        if view.source {
            return Ok(None);
        }
        check(cancel)?;
        let path = req.entry.resolve()?;
        let archive = archive::open(&path, cancel)?;
        check(cancel)?;
        Ok(Some(inspection(&archive, self)))
    }
}

/// The ring around `area`, its longitudes at most a quarter of the world
/// apart, so the map draws it the short way round, across ±180° if it goes
/// there.
fn ring(area: Bounds) -> Vec<LatLon> {
    let width = area.width();
    let steps = (width / 90.).ceil().max(1.) as usize;
    let along = |lat: f64, from: f64, sign: f64| {
        (0..=steps)
            .filter_map(move |i| LatLon::new(lat, from + sign * width * i as f64 / steps as f64))
    };
    along(area.south, area.west, 1.)
        .chain(along(area.north, area.west + width, -1.))
        .collect()
}

fn inspection(archive: &Arc<dyn Archive>, provider: &Provider) -> Inspection {
    let about = archive.about();
    let area = about.bounds.unwrap_or(WORLD);
    let drawn = about.format.drawn() && about.unreadable.is_none();
    let notice = if drawn {
        String::new()
    } else if let Some(why) = &about.unreadable {
        format!("{why}: the map shows where it has tiles")
    } else if about.format == Format::Vector {
        "Vector tiles are not drawn: the map shows where the file has tiles".into()
    } else {
        format!(
            "{} tiles are not drawn: the map shows where the file has tiles",
            match about.format {
                Format::Avif => "AVIF",
                _ => "Its",
            }
        )
    };
    let doc = GeoDoc {
        // Its own tiles show where it has them; an outline would cover
        // them.
        areas: if drawn {
            Vec::new()
        } else {
            vec![Area {
                name: about.name.clone(),
                rings: vec![ring(area)],
            }]
        },
        bounds: Some(area),
        ..GeoDoc::default()
    };
    let zoom = if about.min_zoom == about.max_zoom {
        about.min_zoom.to_string()
    } else {
        format!("{} to {}", about.min_zoom, about.max_zoom)
    };
    let corners = about.bounds.map(|b| {
        format!(
            "{:.5}, {:.5} to {:.5}, {:.5}",
            b.south, b.west, b.north, b.east
        )
    });
    let title = if about.name.is_empty() {
        about.container.clone()
    } else {
        format!("{} · {}", about.container, about.name)
    };
    Inspection {
        kind: InspectionKind::Map,
        title,
        details: details([
            ("Name", Some(about.name.clone())),
            ("Description", Some(about.description.clone())),
            ("Format", Some(about.container.clone())),
            ("Tiles", Some(about.format.describe().to_owned())),
            ("Zoom", Some(zoom)),
            ("Area", corners),
            ("Tile count", about.tiles.map(|n| n.to_string())),
            ("Layers", Some(about.layers.join(", "))),
            ("Attribution", Some(about.attribution.clone())),
            ("Version", Some(about.version.clone())),
        ]),
        notice,
        // A database or a binary file: no source to read.
        source_available: false,
        geo: Some(Arc::new(doc)),
        tiles: drawn.then(|| {
            Layer::new(Arc::new(archive::Source {
                archive: Arc::clone(archive),
                scheduler: provider.scheduler.clone(),
                lifetime: provider.lifetime.clone(),
            }))
        }),
        ..Inspection::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::{App, AppConfig};
    use bibli_core::op::EntryRef;

    fn fact<'a>(data: &'a Inspection, label: &str) -> Option<&'a str> {
        data.details
            .iter()
            .find(|d| d.label == label)
            .map(|d| d.value.as_str())
    }

    fn request(path: &std::path::Path) -> PreviewRequest {
        PreviewRequest {
            entry: EntryRef::from_path(path).expect("the file"),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 512,
            text_size: None,
        }
    }

    /// A PNG tile of one color, as a raster archive holds.
    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(256, 256, image::Rgba([10, 120, 200, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn a_raster_archive_previews_as_its_map_and_serves_its_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(dir.path().join("state"))).unwrap();
        let provider = Provider {
            scheduler: app.scheduler.clone(),
            lifetime: CancelToken::new(),
        };
        let path = dir.path().join("hills.pmtiles");
        std::fs::write(
            &path,
            crate::pmtiles::tests::write(&[(3, 4, 2, png())], 2, true),
        )
        .unwrap();
        let req = request(&path);
        assert!(provider.accepts(&req));
        // A file named so but not one is left to the others.
        let other = dir.path().join("not.pmtiles");
        std::fs::write(&other, b"plain text").unwrap();
        assert!(!provider.accepts(&request(&other)));

        let data = provider
            .inspect(&req, &InspectRequest::default(), &CancelToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(data.kind, InspectionKind::Map);
        assert_eq!(data.title, "PMTiles 3 · Invented Hills");
        assert_eq!(fact(&data, "Tiles"), Some("PNG pictures"));
        assert_eq!(fact(&data, "Zoom"), Some("3"));
        assert_eq!(fact(&data, "Layers"), Some("water, roads"));
        assert!(data.notice.is_empty(), "{}", data.notice);
        let doc = data.geo.as_deref().unwrap();
        assert!(doc.areas.is_empty(), "its tiles show where it has them");
        let layer = data.tiles.clone().expect("its tiles");
        assert_eq!((layer.info.min_zoom, layer.info.max_zoom), (3, 3));
        assert!(layer.info.holds(3, 4, 2));

        let ask = |x: u32, y: u32| {
            let (tx, rx) = std::sync::mpsc::channel();
            layer.source.request(
                bibli_core::tiles::TileRequest {
                    z: 3,
                    x,
                    y,
                    scale: 1,
                    palette: bibli_core::tiles::Palette::default(),
                },
                CancelToken::new(),
                Box::new(move |tile| {
                    let _ = tx.send(tile);
                }),
            );
            rx.recv_timeout(std::time::Duration::from_secs(20))
                .expect("an answer")
        };
        let tile = ask(4, 2).unwrap();
        assert_eq!((tile.width, tile.height), (256, 256));
        let missing = ask(4, 3).err().unwrap();
        assert!(missing.is::<bibli_core::tiles::NoTile>(), "{missing:#}");
    }

    #[test]
    fn a_vector_archive_shows_where_it_has_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(dir.path().join("state"))).unwrap();
        let provider = Provider {
            scheduler: app.scheduler.clone(),
            lifetime: CancelToken::new(),
        };
        let path = dir.path().join("roads.mbtiles");
        crate::mbtiles::tests::write(
            &path,
            &[(0, 0, 0, b"\x1f\x8b-not-read")],
            &[
                ("name", "Invented Roads"),
                ("format", "pbf"),
                ("bounds", "170,60,-170,70"),
                ("minzoom", "0"),
                ("maxzoom", "6"),
            ],
        );
        let req = request(&path);
        assert!(provider.accepts(&req));
        let data = provider
            .inspect(&req, &InspectRequest::default(), &CancelToken::new())
            .unwrap()
            .unwrap();
        assert!(data.tiles.is_none(), "vector tiles are not drawn");
        assert!(data.notice.starts_with("Vector tiles are not drawn"));
        assert_eq!(fact(&data, "Format"), Some("MBTiles"));
        assert_eq!(fact(&data, "Zoom"), Some("0 to 6"));
        let doc = data.geo.as_deref().unwrap();
        let ring = &doc.areas[0].rings[0];
        // Across ±180°, the short way: 20° wide, not 340°.
        assert_eq!(ring.first().map(|at| at.lon), Some(170.));
        assert!(ring.iter().all(|at| at.lon >= 170. || at.lon <= -170.));
    }
}
