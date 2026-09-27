//! `GeoJSON` (RFC 7946): features and bare geometries. Lines are routes
//! unless their feature gives each point a time (`coordTimes`, which
//! converters from GPX and KML write), and then they are tracks.
use bibli_core::geo::{Area, Datum, LatLon, LineKind, Point, Waypoint, WaypointKind};
use serde_json::Value;

use crate::collect::{self, Collector};

/// Nesting of geometry collections followed; a deeper one is left out.
const DEPTH: usize = 16;

fn position(value: &Value) -> Option<(LatLon, Option<f32>)> {
    let pair = value.as_array()?;
    let lon = pair.first()?.as_f64()?;
    let lat = pair.get(1)?.as_f64()?;
    let altitude = pair
        .get(2)
        .and_then(Value::as_f64)
        .map(|a| a as f32)
        .filter(|a| a.is_finite());
    Some((LatLon::new(lat, lon)?, altitude))
}

fn positions(value: &Value, into: &mut Collector) -> Vec<(LatLon, Option<f32>)> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|p| {
                    let at = position(p);
                    if at.is_none() {
                        into.invalid();
                    }
                    at
                })
                .collect()
        })
        .unwrap_or_default()
}

fn times(value: Option<&Value>) -> Vec<Option<std::time::SystemTime>> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|t| t.as_str().and_then(collect::time))
                .collect()
        })
        .unwrap_or_default()
}

/// A feature's name, as the usual properties call it.
fn name(properties: Option<&Value>) -> String {
    ["name", "title", "Name"]
        .into_iter()
        .find_map(|key| properties?.get(key)?.as_str())
        .unwrap_or_default()
        .to_owned()
}

pub fn read(document: &Value, into: &mut Collector) {
    match document.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            for feature in document
                .get("features")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                read_feature(feature, into);
            }
        }
        Some("Feature") => read_feature(document, into),
        _ => geometry(document, "", None, into, 0),
    }
}

fn read_feature(feature: &Value, into: &mut Collector) {
    let properties = feature.get("properties");
    let name = name(properties);
    if let Some(geometry_value) = feature.get("geometry") {
        let times = properties.and_then(|p| p.get("coordTimes"));
        geometry(geometry_value, &name, times, into, 0);
    }
}

fn geometry(value: &Value, name: &str, times: Option<&Value>, into: &mut Collector, depth: usize) {
    let coordinates = value.get("coordinates");
    let draw = |into: &mut Collector, lines: Vec<(&Value, Vec<Option<std::time::SystemTime>>)>| {
        let recorded = lines.iter().any(|(_, t)| t.iter().any(Option::is_some));
        let kind = if recorded {
            LineKind::Track
        } else {
            LineKind::Route
        };
        into.line(collect::label(name), kind, Datum::Sea);
        for (line, times) in lines {
            into.segment();
            for (i, (at, altitude)) in positions(line, into).into_iter().enumerate() {
                into.point(Point {
                    at,
                    altitude,
                    time: times.get(i).copied().flatten(),
                });
            }
            into.end_segment();
        }
    };
    let polygon = |into: &mut Collector, rings: &Value| {
        let rings: Vec<Vec<LatLon>> = rings
            .as_array()
            .into_iter()
            .flatten()
            .map(|ring| {
                positions(ring, into)
                    .into_iter()
                    .map(|(at, _)| at)
                    .collect()
            })
            .filter(|ring: &Vec<LatLon>| !ring.is_empty())
            .collect();
        into.area(Area {
            name: name.to_owned(),
            rings,
        });
    };
    let point = |into: &mut Collector, at: Option<(LatLon, Option<f32>)>| match at {
        Some((at, altitude)) => into.waypoint(Waypoint {
            name: name.to_owned(),
            at,
            kind: WaypointKind::Point,
            altitude: altitude.map(|a| (a, Datum::Sea)),
            line: None,
        }),
        None => into.invalid(),
    };
    match value.get("type").and_then(Value::as_str) {
        Some("Point") => point(into, coordinates.and_then(position)),
        Some("MultiPoint") => {
            for at in coordinates.and_then(Value::as_array).into_iter().flatten() {
                point(into, position(at));
            }
        }
        Some("LineString") => {
            if let Some(line) = coordinates {
                draw(into, vec![(line, self::times(times))]);
            }
        }
        Some("MultiLineString") => {
            let all: Vec<_> = coordinates
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, line)| {
                    let times = times.and_then(Value::as_array).and_then(|t| t.get(i));
                    (line, self::times(times))
                })
                .collect();
            if !all.is_empty() {
                draw(into, all);
            }
        }
        Some("Polygon") => {
            if let Some(rings) = coordinates {
                polygon(into, rings);
            }
        }
        Some("MultiPolygon") => {
            for rings in coordinates.and_then(Value::as_array).into_iter().flatten() {
                polygon(into, rings);
            }
        }
        Some("GeometryCollection") if depth < DEPTH => {
            for part in value
                .get("geometries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                geometry(part, name, None, into, depth + 1);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_collection_gives_its_features() {
        let document: Value = serde_json::from_str(
            r#"{"type":"FeatureCollection","features":[
              {"type":"Feature","properties":{"name":"Spring"},
               "geometry":{"type":"Point","coordinates":[6.0,45.0,1200]}},
              {"type":"Feature","properties":{"name":"Path"},
               "geometry":{"type":"LineString","coordinates":[[6.0,45.0],[6.0,45.001],[6.0,95.0]]}},
              {"type":"Feature","properties":{"name":"Run","coordTimes":["2026-03-12T08:00:00Z","2026-03-12T08:01:00Z"]},
               "geometry":{"type":"LineString","coordinates":[[6.0,45.0,100],[6.0,45.001,104]]}},
              {"type":"Feature","properties":{},
               "geometry":{"type":"MultiPolygon","coordinates":[[[[6,45],[6.1,45],[6.1,45.1],[6,45]]]]}}
            ]}"#,
        )
        .unwrap();
        let mut c = Collector::new();
        read(&document, &mut c);
        let (doc, c) = c.finish();
        assert_eq!(doc.waypoints[0].name, "Spring");
        assert_eq!(doc.lines[0].kind, LineKind::Route);
        assert_eq!(doc.lines[0].segments[0].len(), 2);
        assert_eq!(c.invalid, 1);
        assert_eq!(doc.lines[1].name, "Run");
        assert_eq!(doc.lines[1].kind, LineKind::Track);
        assert_eq!(c.tracks.climb, 4.);
        assert_eq!(doc.areas.len(), 1);
    }

    #[test]
    fn a_bare_geometry_is_read_too() {
        let document: Value =
            serde_json::from_str(r#"{"type":"Point","coordinates":[-170.5,-10.25]}"#).unwrap();
        let mut c = Collector::new();
        read(&document, &mut c);
        let (doc, _) = c.finish();
        assert_eq!(doc.waypoints[0].at, LatLon::new(-10.25, -170.5).unwrap());
    }
}
