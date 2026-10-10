//! `GeoJSON` (RFC 7946): features and bare geometries. Lines are routes
//! unless their feature gives each point a time (`coordTimes`, which
//! converters from GPX and KML write), and then they are tracks.
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use skwir_core::CancelToken;
use skwir_core::geo::{Area, Datum, LatLon, LineKind, Point, Waypoint, WaypointKind};

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

/// The places the `GeoJSON` text `bytes` holds, into `into`, a feature
/// at a time: a collection is never held whole as values, which took six
/// to ten times the file on each worker scanning (a file of road
/// segments holds hundreds of thousands). Stopped between features once
/// `cancel` fires.
pub fn read(bytes: &[u8], into: &mut Collector, cancel: &CancelToken) -> anyhow::Result<()> {
    let mut reading = Reading {
        into,
        cancel,
        cancelled: false,
    };
    let mut text = serde_json::Deserializer::from_slice(bytes);
    let rest = serde::Deserializer::deserialize_map(&mut text, Top(&mut reading))
        .and_then(|rest| text.end().map(|()| rest));
    let rest = match rest {
        Ok(rest) => rest,
        Err(_) if reading.cancelled => {
            skwir_platform::inspection::check(cancel)?;
            anyhow::bail!("preview cancelled")
        }
        Err(error) => anyhow::bail!("Not valid GeoJSON: {error}"),
    };
    let rest = Value::Object(rest);
    match rest.get("type").and_then(Value::as_str) {
        // Its features were read as they came.
        Some("FeatureCollection") => {}
        Some("Feature") => read_feature(&rest, reading.into),
        _ => geometry(&rest, "", None, reading.into, 0),
    }
    Ok(())
}

/// Where a document's features go as they are read.
struct Reading<'a, 'c> {
    into: &'a mut Collector,
    cancel: &'c CancelToken,
    /// Whether the reading stopped for `cancel`, not for the text.
    cancelled: bool,
}

/// The document's object: its features read one by one, everything else
/// kept as values (a feature's or a geometry's own members, small).
struct Top<'r, 'a, 'c>(&'r mut Reading<'a, 'c>);

impl<'de> Visitor<'de> for Top<'_, '_, '_> {
    type Value = Map<String, Value>;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a GeoJSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut rest = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let collection = rest
                .get("type")
                .is_none_or(|kind| kind == "FeatureCollection");
            if key == "features" && collection {
                map.next_value_seed(Features(&mut *self.0))?;
            } else {
                let value = map.next_value::<Value>()?;
                rest.insert(key, value);
            }
        }
        Ok(rest)
    }
}

/// A collection's features, each read into the collector as it is
/// parsed, then let go of.
struct Features<'r, 'a, 'c>(&'r mut Reading<'a, 'c>);

impl<'de> DeserializeSeed<'de> for Features<'_, '_, '_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for Features<'_, '_, '_> {
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a list of features")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        for n in 0usize.. {
            if n.is_multiple_of(256) && self.0.cancel.is_cancelled() {
                self.0.cancelled = true;
                return Err(serde::de::Error::custom("cancelled"));
            }
            let Some(feature) = seq.next_element::<Value>()? else {
                break;
            };
            read_feature(&feature, self.0.into);
        }
        Ok(())
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
        let document = br#"{"type":"FeatureCollection","features":[
              {"type":"Feature","properties":{"name":"Spring"},
               "geometry":{"type":"Point","coordinates":[6.0,45.0,1200]}},
              {"type":"Feature","properties":{"name":"Path"},
               "geometry":{"type":"LineString","coordinates":[[6.0,45.0],[6.0,45.001],[6.0,95.0]]}},
              {"type":"Feature","properties":{"name":"Run","coordTimes":["2026-03-12T08:00:00Z","2026-03-12T08:01:00Z"]},
               "geometry":{"type":"LineString","coordinates":[[6.0,45.0,100],[6.0,45.001,104]]}},
              {"type":"Feature","properties":{},
               "geometry":{"type":"MultiPolygon","coordinates":[[[[6,45],[6.1,45],[6.1,45.1],[6,45]]]]}}
            ]}"#;
        let mut c = Collector::new();
        read(document, &mut c, &CancelToken::new()).unwrap();
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
    fn features_are_read_as_they_come_and_a_cancel_stops_them() {
        // The type said after the features, as some writers order them.
        let document = br#"{"features":[
              {"type":"Feature","properties":{"name":"A"},"geometry":{"type":"Point","coordinates":[6,45]}},
              {"type":"Feature","properties":{"name":"B"},"geometry":{"type":"Point","coordinates":[7,46]}}
            ],"type":"FeatureCollection"}"#;
        let mut c = Collector::new();
        read(document, &mut c, &CancelToken::new()).unwrap();
        let (doc, _) = c.finish();
        let names: Vec<_> = doc.waypoints.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["A", "B"]);

        let cancel = CancelToken::new();
        cancel.cancel();
        let mut c = Collector::new();
        assert!(read(document, &mut c, &cancel).is_err());
        assert!(c.finish().0.waypoints.is_empty());

        let mut c = Collector::new();
        let error = read(b"[1, 2]", &mut c, &CancelToken::new()).unwrap_err();
        assert!(
            error.to_string().starts_with("Not valid GeoJSON"),
            "{error}"
        );
    }

    #[test]
    fn a_bare_geometry_is_read_too() {
        let document = br#"{"type":"Point","coordinates":[-170.5,-10.25]}"#;
        let mut c = Collector::new();
        read(document, &mut c, &CancelToken::new()).unwrap();
        let (doc, _) = c.finish();
        assert_eq!(doc.waypoints[0].at, LatLon::new(-10.25, -170.5).unwrap());
    }
}
