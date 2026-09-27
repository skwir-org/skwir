//! `QGroundControl` plans (`.plan`): JSON with `"fileType": "Plan"` holding
//! a mission, a geofence and rally points. Positions are `[lat, lon]` or
//! `[lat, lon, alt]`, latitude first, unlike `GeoJSON`.
//!
//! Mission items are `MAVLink` commands (`mavlink`). A `SimpleItem` holds one
//! in `command`, `frame` and `params`, the command's seven parameters, of
//! which the fifth to seventh are the latitude, longitude and altitude
//! (`null` where `QGroundControl` had NaN); version 1 plans put the position
//! in `coordinate` instead. A `ComplexItem` is a pattern `QGroundControl`
//! expands into items: a survey over a polygon, a corridor scan along a
//! polyline, a structure scan around a polygon, a landing pattern. The
//! items it expanded to are saved with it (`TransectStyleComplexItem.Items`)
//! and are its path; they are points of the route but not stops of their
//! own, as the plan shows the pattern as one item.
//!
//! The planned home is where the mission returns to, not a point of its
//! path; its altitude is above the sea. Rally points are above the home.
//! Speeds (`cruiseSpeed`, `hoverSpeed`) and the vehicle are left out.
use bibli_core::geo::{Area, Datum, LatLon, WaypointKind};
use serde_json::Value;

use crate::collect::{Collector, Plan, PlanKind};
use crate::mission::{self, Item, Position, Route};

/// Whether the start of a file is a plan's: its `fileType`, which
/// `QGroundControl` writes first, as it sorts the keys.
pub fn sniff(head: &[u8]) -> bool {
    const KEY: &[u8] = b"\"fileType\"";
    let Some(start) = head.windows(KEY.len()).position(|w| w == KEY) else {
        return false;
    };
    let rest = head[start + KEY.len()..].trim_ascii_start();
    rest.strip_prefix(b":")
        .is_some_and(|rest| rest.trim_ascii_start().starts_with(b"\"Plan\""))
}

pub fn read(document: &Value, into: &mut Collector) {
    into.creator = document
        .get("groundStation")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|name| !name.is_empty());
    let mut route = Route::default();
    if let Some(mission) = document.get("mission") {
        if let Some((Position::At(at), altitude)) =
            mission.get("plannedHomePosition").and_then(coordinate)
        {
            route.home(at, altitude.map(|metres| (metres, Datum::Sea)), into);
        }
        for (i, item) in list(mission.get("items")).enumerate() {
            match item.get("type").and_then(Value::as_str) {
                Some("ComplexItem") => complex(item, i, &mut route, into),
                _ => simple(item, i, true, &mut route, into),
            }
        }
    }
    let destination = route.finish(into);
    if let Some(fence) = document.get("geoFence") {
        geofence(fence, into);
    }
    for (i, point) in list(document.get("rallyPoints").and_then(|r| r.get("points"))).enumerate() {
        match coordinate(point) {
            Some((Position::At(at), altitude)) => into.waypoint(bibli_core::geo::Waypoint {
                name: format!("R{}", i + 1),
                at,
                kind: WaypointKind::Rally,
                altitude: altitude.map(|metres| (metres, Datum::Takeoff)),
                line: None,
            }),
            _ => into.invalid(),
        }
    }
    into.plan = Some(Plan {
        kind: PlanKind::Mission,
        destination,
        ..Plan::default()
    });
}

fn list(value: Option<&Value>) -> impl Iterator<Item = &Value> {
    value.and_then(Value::as_array).into_iter().flatten()
}

/// Degrees as the plan writes them: `null` is NaN, which `MAVLink` reads as
/// "where the vehicle is".
fn degrees(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    if value.is_null() {
        Some(f64::NAN)
    } else {
        value.as_f64()
    }
}

fn metres(value: Option<&Value>) -> Option<f32> {
    value
        .and_then(Value::as_f64)
        .map(|metres| metres as f32)
        .filter(|metres| metres.is_finite() && metres.abs() < 1.0e6)
}

fn position(lat: Option<&Value>, lon: Option<&Value>) -> Position {
    match (degrees(lat), degrees(lon)) {
        (Some(lat), Some(lon)) => Position::new(lat, lon),
        _ => Position::Invalid,
    }
}

/// `[lat, lon]` or `[lat, lon, alt]`.
fn coordinate(value: &Value) -> Option<(Position, Option<f32>)> {
    let values = value.as_array()?;
    Some((
        position(values.first(), values.get(1)),
        metres(values.get(2)),
    ))
}

/// A ring of `[lat, lon]`; vertices that are not positions are counted.
fn ring(value: Option<&Value>, into: &mut Collector) -> Vec<LatLon> {
    list(value)
        .filter_map(|vertex| {
            let at = vertex.as_array().and_then(|values| {
                LatLon::new(values.first()?.as_f64()?, values.get(1)?.as_f64()?)
            });
            if at.is_none() {
                into.invalid();
            }
            at
        })
        .collect()
}

/// The item's name on the map: its sequence number as the plan shows it,
/// else its place in the list.
fn name(item: &Value, i: usize) -> String {
    item.get("doJumpId")
        .and_then(Value::as_u64)
        .map_or_else(|| (i + 1).to_string(), |id| id.to_string())
}

fn simple(item: &Value, i: usize, labelled: bool, route: &mut Route, into: &mut Collector) {
    let Some(command) = item.get("command").and_then(Value::as_u64) else {
        return;
    };
    let (position, altitude) = match item.get("params").and_then(Value::as_array) {
        Some(params) => (
            self::position(params.get(4), params.get(5)),
            metres(params.get(6)),
        ),
        None => item
            .get("coordinate")
            .and_then(coordinate)
            .unwrap_or((Position::Invalid, None)),
    };
    route.item(
        Item {
            name: name(item, i),
            command,
            frame: item
                .get("frame")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX),
            position,
            altitude,
        },
        labelled,
        into,
    );
}

fn complex(item: &Value, i: usize, route: &mut Route, into: &mut Collector) {
    let area = |into: &mut Collector, name: &str| {
        let ring = ring(item.get("polygon"), into);
        if !ring.is_empty() {
            into.area(Area {
                name: name.into(),
                rings: vec![ring],
            });
        }
    };
    match item.get("complexItemType").and_then(Value::as_str) {
        Some("survey") => area(into, "Survey"),
        Some("StructureScan") => area(into, "Structure scan"),
        Some("fwLandingPattern" | "vtolLandingPattern") => {
            return landing(item, i, route, into);
        }
        _ => {}
    }
    let transect = item.get("TransectStyleComplexItem").unwrap_or(item);
    let items: Vec<&Value> = list(transect.get("Items")).collect();
    if items.is_empty() {
        // A pattern saved without its items: its turns, without heights.
        for point in list(transect.get("VisualTransectPoints")) {
            match coordinate(point) {
                Some((Position::At(at), altitude)) => route.pass(at, altitude, into),
                _ => into.invalid(),
            }
        }
    } else {
        for (j, item) in items.into_iter().enumerate() {
            simple(item, j, false, route, into);
        }
    }
}

/// A landing pattern: the loiter it descends from, then where it lands.
fn landing(item: &Value, i: usize, route: &mut Route, into: &mut Collector) {
    let relative = item
        .get("altitudesAreRelative")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let frame = if relative {
        mission::FRAME_TAKEOFF
    } else {
        mission::FRAME_SEA
    };
    for (key, command, labelled) in [
        ("loiterCoordinate", mission::LOITER_TO_ALT, false),
        ("landingCoordinate", mission::LAND, true),
    ] {
        if let Some((position, altitude)) = item.get(key).and_then(coordinate) {
            route.item(
                Item {
                    name: name(item, i),
                    command,
                    frame,
                    position,
                    altitude,
                },
                labelled,
                into,
            );
        }
    }
}

/// Polygons and circles, each to stay in (`inclusion`) or out of.
fn geofence(fence: &Value, into: &mut Collector) {
    let inclusion = |value: &Value| {
        value
            .get("inclusion")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    };
    for polygon in list(fence.get("polygons")) {
        let ring = ring(polygon.get("polygon"), into);
        if !ring.is_empty() {
            into.area(Area {
                name: mission::fence_name(inclusion(polygon)).into(),
                rings: vec![ring],
            });
        }
    }
    for circle in list(fence.get("circles")) {
        let shape = circle.get("circle").unwrap_or(circle);
        let center = shape.get("center").and_then(coordinate);
        let radius = shape.get("radius").and_then(Value::as_f64);
        match (center, radius) {
            (Some((Position::At(center), _)), Some(radius)) => {
                match mission::circle(center, radius) {
                    Some(ring) => into.area(Area {
                        name: mission::fence_name(inclusion(circle)).into(),
                        rings: vec![ring],
                    }),
                    None => into.invalid(),
                }
            }
            _ => into.invalid(),
        }
    }
}

#[cfg(test)]
// The values compared are written in the plans and exact in binary
// floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use bibli_core::geo::LineKind;

    fn at(lat: f64, lon: f64) -> LatLon {
        LatLon::new(lat, lon).unwrap()
    }

    const PLAN: &str = r#"{
      "fileType": "Plan",
      "geoFence": {
        "circles": [
          {"circle": {"center": [10.003, 20.001], "radius": 50}, "inclusion": false, "version": 1}
        ],
        "polygons": [
          {"inclusion": true, "polygon": [[9.99, 19.99], [9.99, 20.01], [10.01, 20.01], [10.01, 19.99]], "version": 1}
        ],
        "version": 2
      },
      "groundStation": "QGroundControl",
      "mission": {
        "cruiseSpeed": 15,
        "firmwareType": 12,
        "hoverSpeed": 5,
        "items": [
          {"autoContinue": true, "command": 22, "doJumpId": 1, "frame": 3,
           "params": [15, 0, 0, null, 10.0, 20.0, 30], "type": "SimpleItem"},
          {"autoContinue": true, "command": 178, "doJumpId": 2, "frame": 2,
           "params": [1, 8, -1, 0, 0, 0, 0], "type": "SimpleItem"},
          {"autoContinue": true, "command": 16, "doJumpId": 3, "frame": 3,
           "params": [0, 0, 0, null, 10.001, 20.0, 50], "type": "SimpleItem"},
          {"TransectStyleComplexItem": {
             "Items": [
               {"autoContinue": true, "command": 16, "doJumpId": 4, "frame": 3,
                "params": [0, 0, 0, null, 10.002, 20.0, 60], "type": "SimpleItem"},
               {"autoContinue": true, "command": 206, "doJumpId": 5, "frame": 2,
                "params": [8, 0, 1, 0, 0, 0, 0], "type": "SimpleItem"},
               {"autoContinue": true, "command": 16, "doJumpId": 6, "frame": 3,
                "params": [0, 0, 0, null, 10.004, 20.002, 60], "type": "SimpleItem"}
             ],
             "VisualTransectPoints": [[10.002, 20.0], [10.004, 20.002]]
           },
           "complexItemType": "survey",
           "polygon": [[10.002, 20.0], [10.002, 20.002], [10.004, 20.002], [10.004, 20.0]],
           "type": "ComplexItem", "version": 5},
          {"autoContinue": true, "command": 16, "doJumpId": 7, "frame": 3,
           "params": [0, 0, 0, null, "north", 20.0, 50], "type": "SimpleItem"},
          {"autoContinue": true, "command": 21, "doJumpId": 8, "frame": 3,
           "params": [0, 0, 0, null, 10.0, 20.0, 0], "type": "SimpleItem"}
        ],
        "plannedHomePosition": [10.0, 20.0, 488],
        "vehicleType": 2,
        "version": 2
      },
      "rallyPoints": {"points": [[10.005, 20.005, 40]], "version": 2},
      "version": 1
    }"#;

    fn read_json(text: &str) -> Collector {
        let document: Value = serde_json::from_str(text).unwrap();
        let mut c = Collector::new();
        read(&document, &mut c);
        c
    }

    #[test]
    fn a_plan_gives_its_route_stops_fence_and_rally_points() {
        let (doc, c) = read_json(PLAN).finish();
        assert_eq!(c.creator.as_deref(), Some("QGroundControl"));
        let plan = c.plan.unwrap();
        assert_eq!(plan.kind, PlanKind::Mission);
        assert_eq!(plan.destination, None);
        assert_eq!(plan.cruise, None);
        assert_eq!(doc.lines.len(), 1);
        let line = &doc.lines[0];
        assert_eq!(line.kind, LineKind::Route);
        assert_eq!(line.datum, Datum::Takeoff);
        let points: Vec<_> = line.segments[0]
            .iter()
            .map(|p| (p.at, p.altitude))
            .collect();
        assert_eq!(
            points,
            [
                (at(10., 20.), Some(30.)),
                (at(10.001, 20.), Some(50.)),
                (at(10.002, 20.), Some(60.)),
                (at(10.004, 20.002), Some(60.)),
                (at(10., 20.), Some(0.)),
            ]
        );
        // The survey's turns are not stops of their own.
        let stops: Vec<_> = doc
            .waypoints
            .iter()
            .map(|w| (w.name.as_str(), w.kind, w.altitude))
            .collect();
        assert_eq!(
            stops,
            [
                ("Home", WaypointKind::Home, Some((488., Datum::Sea))),
                ("1", WaypointKind::Takeoff, Some((30., Datum::Takeoff))),
                ("3", WaypointKind::Stop, Some((50., Datum::Takeoff))),
                ("8", WaypointKind::Landing, Some((0., Datum::Takeoff))),
                ("R1", WaypointKind::Rally, Some((40., Datum::Takeoff))),
            ]
        );
        assert_eq!(doc.waypoints[1].line, Some(0));
        assert_eq!(doc.waypoints[4].line, None);
        // The waypoint whose latitude is a word.
        assert_eq!(c.invalid, 1);
        let areas: Vec<_> = doc
            .areas
            .iter()
            .map(|a| (a.name.as_str(), a.rings[0].len()))
            .collect();
        assert_eq!(
            areas,
            [("Survey", 4), ("Geofence", 4), ("Exclusion zone", 32)]
        );
        let center = at(10.003, 20.001);
        assert!(
            doc.areas[2].rings[0]
                .iter()
                .all(|p| (p.distance(center) - 50.).abs() < 0.01)
        );
    }

    #[test]
    fn a_takeoff_on_the_spot_a_pattern_without_items_and_a_return_home() {
        let (doc, c) = read_json(
            r#"{"fileType": "Plan", "mission": {
              "plannedHomePosition": [10.0, 20.0, 488],
              "items": [
                {"type": "SimpleItem", "command": 22, "frame": 0, "coordinate": [0, 0, 520]},
                {"type": "ComplexItem", "complexItemType": "CorridorScan",
                 "polyline": [[10.001, 20.0], [10.002, 20.0]],
                 "TransectStyleComplexItem": {"VisualTransectPoints": [[10.001, 20.0], [10.002, 20.0]]}},
                {"type": "SimpleItem", "command": 16, "frame": 0, "params": [0, 0, 0, null, 10.003, 20.0, 530]},
                {"type": "SimpleItem", "command": 16, "frame": 3, "params": [0, 0, 0, null, 10.004, 20.0, 40]},
                {"type": "SimpleItem", "command": 20, "frame": 2, "params": [0, 0, 0, 0, 0, 0, 0]}
              ]}}"#,
        )
        .finish();
        let line = &doc.lines[0];
        // Two stops above the sea, one above the takeoff.
        assert_eq!(line.datum, Datum::Sea);
        let points: Vec<_> = line.segments[0].iter().map(|p| p.at).collect();
        assert_eq!(
            points,
            [
                at(10., 20.),
                at(10.001, 20.),
                at(10.002, 20.),
                at(10.003, 20.),
                at(10.004, 20.)
            ]
        );
        assert_eq!(line.segments[0][1].altitude, None);
        assert_eq!(doc.waypoints[1].kind, WaypointKind::Takeoff);
        assert_eq!(doc.waypoints[1].name, "1");
        assert_eq!(doc.waypoints[1].altitude, Some((520., Datum::Sea)));
        assert_eq!(c.plan.unwrap().destination.as_deref(), Some("Home"));
    }

    #[test]
    fn a_landing_pattern_descends_to_its_landing() {
        let (doc, _) = read_json(
            r#"{"fileType": "Plan", "mission": {"items": [
                {"type": "ComplexItem", "complexItemType": "fwLandingPattern",
                 "altitudesAreRelative": true,
                 "loiterCoordinate": [10.01, 20.0, 40], "landingCoordinate": [10.0, 20.0, 0]}
              ]}}"#,
        )
        .finish();
        assert_eq!(doc.lines[0].segments[0].len(), 2);
        assert_eq!(doc.waypoints.len(), 1);
        assert_eq!(doc.waypoints[0].kind, WaypointKind::Landing);
        assert_eq!(doc.waypoints[0].altitude, Some((0., Datum::Takeoff)));
    }

    #[test]
    fn a_plan_is_known_by_its_file_type() {
        assert!(sniff(
            br#"{
    "fileType": "Plan",
    "geoFence": {"#
        ));
        assert!(sniff(br#"{"fileType":"Plan"}"#));
        assert!(!sniff(br#"{"fileType": "Mission"}"#));
        assert!(!sniff(br#"{"type": "FeatureCollection"}"#));
    }
}
