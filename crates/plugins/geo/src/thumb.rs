//! A track's picture in the grid: its lines drawn to scale in the Web
//! Mercator projection, the way they go, on nothing: the grid's tile is
//! their paper, in either theme. Tracks are orange and routes blue (a plan
//! is a route), each over a white casing so it reads on any tile; the
//! start is a dot, the end a ring, areas a light fill. A fallback: a
//! thumbnailer the desktop installed for these files comes first.
use skwir_core::CancelToken;
use skwir_core::geo::{GeoDoc, LatLon, LineKind};
use skwir_core::thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform,
};

/// Room left around the lines, as a share of the side.
const PADDING: f32 = 0.1;
/// A thumbnail is never narrower than this share of its longest side: a
/// straight road north would otherwise be a sliver.
const LEAST_ASPECT: f32 = 0.5;
const TRACK: [u8; 4] = [0xe8, 0x59, 0x0c, 0xff];
const ROUTE: [u8; 4] = [0x1c, 0x7e, 0xd6, 0xff];
const CASING: [u8; 4] = [0xff, 0xff, 0xff, 0xe6];
/// Waypoints are dotted while they are few enough to tell apart.
const MOST_WAYPOINTS: usize = 200;

pub struct Lines;

impl ThumbnailProvider for Lines {
    fn id(&self) -> &'static str {
        "geo-lines"
    }
    fn stage(&self) -> Stage {
        Stage::Fallback
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        crate::Format::of(&req.entry.path).is_some()
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = req.entry.resolve()?;
        skwir_platform::thumb_providers::ensure_file(&path)?;
        let Some(format) = crate::Format::detect(&path) else {
            // A `.csv` or a `.txt` that is not a plan.
            return Ok(None);
        };
        let data = crate::inspect_as(
            &path,
            format,
            skwir_platform::inspection::limits::Bounds::QUICK,
            cancel,
        )?;
        Ok(data.geo.and_then(|doc| draw(&doc, req.size.pixels())))
    }
}

/// Where a position lands on the square world of side 1, its longitude
/// brought next to `west` when the file crosses the ±180° meridian.
fn project(at: LatLon, west: Option<f64>) -> (f64, f64) {
    let lon = match west {
        Some(west) if at.lon < west => at.lon + 360.,
        _ => at.lon,
    };
    let lat = at.lat.clamp(-85.05, 85.05).to_radians();
    (
        (lon + 180.) / 360.,
        (1. - (lat.tan() + 1. / lat.cos()).ln() / std::f64::consts::PI) / 2.,
    )
}

fn paint(rgba: [u8; 4]) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]));
    paint
}

/// The picture of `doc`, fitted to `side` pixels; `None` when nothing in
/// it has a position.
pub fn draw(doc: &GeoDoc, side: u32) -> Option<Image> {
    let bounds = doc.bounds?;
    let west = bounds.crosses_antimeridian().then_some(bounds.west);
    let points = || {
        doc.lines
            .iter()
            .flat_map(|l| &l.segments)
            .flatten()
            .map(|p| p.at)
            .chain(doc.waypoints.iter().map(|w| w.at))
            .chain(doc.areas.iter().flat_map(|a| &a.rings).flatten().copied())
    };
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for at in points() {
        let (x, y) = project(at, west);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let (width, height) = (x1 - x0, y1 - y0);
    let longest = width.max(height);
    let side_f = side as f32;
    // The image keeps the lines' proportions, never thinner than
    // `LEAST_ASPECT`; a single place is a square.
    let (w, h) = if longest <= 0. {
        (side_f, side_f)
    } else {
        let aspect = |a: f64| ((a / longest) as f32).max(LEAST_ASPECT);
        (side_f * aspect(width), side_f * aspect(height))
    };
    let mut pixmap = Pixmap::new(w.round().max(1.) as u32, h.round().max(1.) as u32)?;
    let inner = side_f * (1. - 2. * PADDING);
    let scale = if longest > 0. {
        f64::from(inner) / longest
    } else {
        0.
    };
    let (cx, cy) = (f64::midpoint(x0, x1), f64::midpoint(y0, y1));
    let at = |p: LatLon| {
        let (x, y) = project(p, west);
        (
            ((x - cx) * scale) as f32 + w / 2.,
            ((y - cy) * scale) as f32 + h / 2.,
        )
    };
    let unit = side_f / 128.;

    for area in &doc.areas {
        let mut path = PathBuilder::new();
        for ring in &area.rings {
            for (i, p) in ring.iter().enumerate() {
                let (x, y) = at(*p);
                if i == 0 {
                    path.move_to(x, y);
                } else {
                    path.line_to(x, y);
                }
            }
            path.close();
        }
        if let Some(path) = path.finish() {
            let mut fill = paint(ROUTE);
            fill.set_color_rgba8(ROUTE[0], ROUTE[1], ROUTE[2], 0x30);
            pixmap.fill_path(&path, &fill, FillRule::EvenOdd, Transform::identity(), None);
            let stroke = Stroke {
                width: 1.5 * unit,
                ..Stroke::default()
            };
            pixmap.stroke_path(&path, &paint(ROUTE), &stroke, Transform::identity(), None);
        }
    }

    // Routes under tracks: where someone meant to go, then where they went.
    let mut ends = Vec::new();
    for kind in [LineKind::Route, LineKind::Track] {
        let mut path = PathBuilder::new();
        for line in doc.lines.iter().filter(|l| l.kind == kind) {
            for segment in &line.segments {
                let mut last: Option<(f32, f32)> = None;
                for (i, point) in segment.iter().enumerate() {
                    let (x, y) = at(point.at);
                    // Points closer than half a pixel add nothing.
                    let far = last.is_none_or(|(lx, ly)| (x - lx).abs() + (y - ly).abs() >= 0.5);
                    if i == 0 {
                        path.move_to(x, y);
                    } else if far || i + 1 == segment.len() {
                        path.line_to(x, y);
                    } else {
                        continue;
                    }
                    last = Some((x, y));
                }
            }
            if kind == LineKind::Track {
                let first = line.segments.first().and_then(|s| s.first());
                let final_ = line.segments.last().and_then(|s| s.last());
                if let (Some(first), Some(final_)) = (first, final_) {
                    ends.push((at(first.at), at(final_.at)));
                }
            }
        }
        let Some(path) = path.finish() else {
            continue;
        };
        let round = |width: f32| Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        let color = if kind == LineKind::Track {
            TRACK
        } else {
            ROUTE
        };
        pixmap.stroke_path(
            &path,
            &paint(CASING),
            &round(4.5 * unit),
            Transform::identity(),
            None,
        );
        pixmap.stroke_path(
            &path,
            &paint(color),
            &round(2.5 * unit),
            Transform::identity(),
            None,
        );
    }

    let dot =
        |pixmap: &mut Pixmap, (x, y): (f32, f32), radius: f32, fill: [u8; 4], ring: [u8; 4]| {
            if let Some(circle) = PathBuilder::from_circle(x, y, radius) {
                pixmap.fill_path(
                    &circle,
                    &paint(fill),
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
                let stroke = Stroke {
                    width: 1.5 * unit,
                    ..Stroke::default()
                };
                pixmap.stroke_path(&circle, &paint(ring), &stroke, Transform::identity(), None);
            }
        };
    if doc.waypoints.len() <= MOST_WAYPOINTS {
        for waypoint in &doc.waypoints {
            dot(&mut pixmap, at(waypoint.at), 2. * unit, ROUTE, CASING);
        }
    }
    for (start, end) in ends {
        dot(&mut pixmap, end, 3.5 * unit, CASING, TRACK);
        dot(&mut pixmap, start, 3.5 * unit, TRACK, CASING);
    }

    // Straight RGBA, as the thumbnail pipeline takes it.
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some(Image {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba: rgba.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::geo::{BoundsBuilder, Datum, Line, Point};

    fn at(lat: f64, lon: f64) -> LatLon {
        LatLon::new(lat, lon).unwrap()
    }

    fn doc(points: &[(f64, f64)]) -> GeoDoc {
        let mut bounds = BoundsBuilder::default();
        let segment: Vec<Point> = points
            .iter()
            .map(|&(lat, lon)| {
                bounds.add(at(lat, lon));
                Point {
                    at: at(lat, lon),
                    altitude: None,
                    time: None,
                }
            })
            .collect();
        GeoDoc {
            lines: vec![Line {
                name: String::new(),
                kind: LineKind::Track,
                datum: Datum::Sea,
                segments: vec![segment],
            }],
            bounds: bounds.finish(),
            ..GeoDoc::default()
        }
    }

    fn alpha(image: &Image, x: u32, y: u32) -> u8 {
        image.rgba[((y * image.width + x) * 4 + 3) as usize]
    }

    #[test]
    fn a_track_is_drawn_to_scale_on_nothing() {
        // East, a little north: wider than tall.
        let image = draw(&doc(&[(10., 20.), (10.1, 20.4)]), 128).unwrap();
        assert_eq!(image.width, 128);
        assert!(image.height >= 64 && image.height < 128, "{}", image.height);
        // The corners are empty; the middle of the line is drawn.
        assert_eq!(alpha(&image, 0, 0), 0);
        assert!(alpha(&image, image.width / 2, image.height / 2) > 0);
        // Straight RGBA: a drawn pixel of the line is orange, not darkened.
        let i = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
        assert!(image.rgba[i] > 0xc0, "{:?}", &image.rgba[i..i + 4]);
    }

    #[test]
    fn a_road_due_north_is_not_a_sliver_and_a_lone_place_is_square() {
        let north = draw(&doc(&[(10., 20.), (11., 20.)]), 128).unwrap();
        assert_eq!((north.width, north.height), (64, 128));
        let mut one = doc(&[]);
        one.lines.clear();
        let mut bounds = BoundsBuilder::default();
        bounds.add(at(10., 20.));
        one.bounds = bounds.finish();
        one.waypoints.push(skwir_core::geo::Waypoint {
            name: String::new(),
            at: at(10., 20.),
            kind: skwir_core::geo::WaypointKind::Point,
            altitude: None,
            line: None,
        });
        let alone = draw(&one, 128).unwrap();
        assert_eq!((alone.width, alone.height), (128, 128));
        assert!(alpha(&alone, 64, 64) > 0);
        assert!(draw(&GeoDoc::default(), 128).is_none());
    }

    #[test]
    fn a_track_over_the_antimeridian_is_drawn_whole() {
        let image = draw(&doc(&[(60., 179.8), (60., -179.8)]), 128).unwrap();
        // A short hop, drawn across the middle, not across the world.
        assert!(alpha(&image, 64, image.height / 2) > 0);
    }
}
