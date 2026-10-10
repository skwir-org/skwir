//! KML (and the KML inside a KMZ): placemarks and the geometry they hold,
//! points, lines, polygons and Google Earth's timed tracks (`gx:Track`).
//! A line without times (`LineString`) is a route: KML cannot say whether
//! it was recorded or drawn, and only a recording has times. What a file
//! links to (`NetworkLink`, icons, overlays) is never fetched.
use std::io::BufRead;

use skwir_core::CancelToken;
use skwir_core::geo::{Area, Datum, LatLon, LineKind, Point, Waypoint, WaypointKind};

use crate::collect::{self, Collector};
use crate::xml::{self, Damaged, Tag};

/// A placemark being read: its geometry waits for its name, which may come
/// after it.
#[derive(Default)]
struct Placemark {
    name: String,
    /// Its line, once a `LineString` or a track started one.
    line: Option<usize>,
    datum: Option<Datum>,
    points: Vec<(LatLon, Option<f32>)>,
    areas: Vec<Vec<Vec<LatLon>>>,
}

/// `lon,lat[,alt]` tuples separated by whitespace, as `coordinates` holds.
fn tuples(text: &str, invalid: &mut usize) -> Vec<(LatLon, Option<f32>)> {
    text.split_whitespace()
        .filter_map(|tuple| {
            let mut parts = tuple.split(',');
            let lon = parts.next()?;
            let Some(at) = parts.next().and_then(|lat| collect::position(lat, lon)) else {
                *invalid += 1;
                return None;
            };
            Some((at, parts.next().and_then(collect::altitude)))
        })
        .collect()
}

fn datum(mode: &str) -> Datum {
    match mode.trim() {
        "relativeToGround" => Datum::Ground,
        _ => Datum::Sea,
    }
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut path: Vec<String> = Vec::new();
    let mut placemark: Option<Placemark> = None;
    // A timed track: its instants and its positions, paired in order.
    let mut whens: Vec<Option<std::time::SystemTime>> = Vec::new();
    let mut coords: Vec<(LatLon, Option<f32>)> = Vec::new();
    let mut rings: Vec<Vec<LatLon>> = Vec::new();
    let mut invalid = 0;
    let damaged = xml::walk(input, cancel, |tag| {
        match tag {
            Tag::Start { name, .. } => {
                match name {
                    "Placemark" => placemark = Some(Placemark::default()),
                    "Track" => {
                        whens.clear();
                        coords.clear();
                    }
                    "Polygon" => rings.clear(),
                    _ => {}
                }
                path.push(name.to_owned());
            }
            Tag::End { name, text } => {
                path.pop();
                let parent = path.last().map_or("", String::as_str);
                match (name, parent) {
                    ("name", "Placemark") => {
                        if let Some(p) = placemark.as_mut() {
                            text.trim().clone_into(&mut p.name);
                        }
                    }
                    ("name", "Document") if into.name.is_none() => {
                        into.name = Some(text.trim().to_owned()).filter(|n| !n.is_empty());
                    }
                    ("description", "Document") if into.description.is_none() => {
                        into.description = Some(text.trim().to_owned()).filter(|d| !d.is_empty());
                    }
                    ("altitudeMode", _) => {
                        placemark.get_or_insert_default().datum = Some(datum(text));
                    }
                    ("coordinates", "Point") => {
                        let p = placemark.get_or_insert_default();
                        p.points
                            .extend(tuples(text, &mut invalid).into_iter().take(1));
                    }
                    ("coordinates", "LineString") => {
                        let points = tuples(text, &mut invalid);
                        let p = placemark.get_or_insert_default();
                        if p.line.is_none() {
                            p.line = Some(into.line(String::new(), LineKind::Route, Datum::Sea));
                        }
                        into.segment();
                        for (at, altitude) in points {
                            into.point(Point {
                                at,
                                altitude,
                                time: None,
                            });
                        }
                        into.end_segment();
                    }
                    ("coordinates", "LinearRing") => {
                        let ring: Vec<LatLon> = tuples(text, &mut invalid)
                            .into_iter()
                            .map(|(at, _)| at)
                            .collect();
                        if !ring.is_empty() {
                            rings.push(ring);
                        }
                    }
                    ("Polygon", _) => {
                        let polygon = std::mem::take(&mut rings);
                        if !polygon.is_empty() {
                            placemark.get_or_insert_default().areas.push(polygon);
                        }
                    }
                    ("when", "Track") => whens.push(collect::time(text)),
                    ("coord", "Track") => {
                        // `gx:coord` is `lon lat alt`, space-separated.
                        let mut parts = text.split_whitespace();
                        let at = parts
                            .next()
                            .zip(parts.next())
                            .and_then(|(lon, lat)| collect::position(lat, lon));
                        match at {
                            Some(at) => coords.push((at, parts.next().and_then(collect::altitude))),
                            None => invalid += 1,
                        }
                    }
                    ("Track", _) => {
                        let p = placemark.get_or_insert_default();
                        if p.line.is_none() {
                            p.line = Some(into.line(String::new(), LineKind::Track, Datum::Sea));
                        }
                        into.segment();
                        for (i, (at, altitude)) in coords.drain(..).enumerate() {
                            into.point(Point {
                                at,
                                altitude,
                                time: whens.get(i).copied().flatten(),
                            });
                        }
                        into.end_segment();
                    }
                    ("Placemark", _) => {
                        if let Some(p) = placemark.take() {
                            flush(p, into);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })?;
    // Geometry outside any placemark.
    if let Some(p) = placemark.take() {
        flush(p, into);
    }
    for _ in 0..invalid {
        into.invalid();
    }
    Ok(damaged)
}

fn flush(p: Placemark, into: &mut Collector) {
    if p.line.is_some() {
        into.name_line(&p.name);
        into.line_datum(p.datum.unwrap_or_default());
    }
    for (at, altitude) in p.points {
        into.waypoint(Waypoint {
            name: p.name.clone(),
            at,
            kind: WaypointKind::Point,
            altitude: altitude.map(|a| (a, p.datum.unwrap_or_default())),
            line: None,
        });
    }
    for rings in p.areas {
        into.area(Area {
            name: p.name.clone(),
            rings,
        });
    }
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const TRIP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document>
  <name>Trip</name>
  <NetworkLink><Link><href>https://example.org/more.kml</href></Link></NetworkLink>
  <Placemark>
    <Point><coordinates>6.001,45.001,1510</coordinates></Point>
    <name>Lake</name>
  </Placemark>
  <Placemark>
    <name>Planned</name>
    <LineString>
      <altitudeMode>relativeToGround</altitudeMode>
      <coordinates>
        6.0,45.0,10 6.0,45.001,12
        6.001,45.002,15
      </coordinates>
    </LineString>
  </Placemark>
  <Placemark>
    <name>Walked</name>
    <gx:Track>
      <when>2026-03-12T08:00:00Z</when>
      <when>2026-03-12T08:01:00Z</when>
      <gx:coord>6.0 45.0 100</gx:coord>
      <gx:coord>6.0 45.001 110</gx:coord>
    </gx:Track>
  </Placemark>
  <Placemark>
    <name>Field</name>
    <Polygon>
      <outerBoundaryIs><LinearRing><coordinates>6,45 6.01,45 6.01,45.01 6,45</coordinates></LinearRing></outerBoundaryIs>
      <innerBoundaryIs><LinearRing><coordinates>6.002,45.002 6.003,45.002 6.003,45.003 6.002,45.002</coordinates></LinearRing></innerBoundaryIs>
    </Polygon>
  </Placemark>
</Document>
</kml>"#;

    #[test]
    fn a_kml_file_gives_its_placemarks() {
        let mut c = Collector::new();
        assert!(
            read(TRIP.as_bytes(), &mut c, &CancelToken::new())
                .unwrap()
                .is_none()
        );
        let (doc, c) = c.finish();
        assert_eq!(c.name.as_deref(), Some("Trip"));
        // The name came after the point.
        assert_eq!(doc.waypoints[0].name, "Lake");
        assert_eq!(doc.waypoints[0].altitude, Some((1510., Datum::Sea)));
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.lines[0].name, "Planned");
        assert_eq!(doc.lines[0].kind, LineKind::Route);
        assert_eq!(doc.lines[0].datum, Datum::Ground);
        assert_eq!(doc.lines[0].segments[0].len(), 3);
        assert_eq!(doc.lines[1].name, "Walked");
        assert_eq!(doc.lines[1].kind, LineKind::Track);
        assert!(doc.lines[1].segments[0][1].time.is_some());
        assert_eq!(c.tracks.climb, 10.);
        assert_eq!(doc.areas.len(), 1);
        assert_eq!(doc.areas[0].rings.len(), 2);
        assert_eq!(doc.areas[0].name, "Field");
    }
}
