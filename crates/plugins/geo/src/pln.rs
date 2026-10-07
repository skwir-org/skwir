//! Microsoft Flight Simulator plans (`.pln`): the `AceXML` document FSX
//! writes (`SimBase.Document` holding a `FlightPlan.FlightPlan`), which
//! `Prepar3D`, MSFS 2020 and MSFS 2024 write too with a few elements more.
//! The plan names its ends (`DepartureID`, `DestinationID`), its cruising
//! altitude in feet (`CruisingAlt`) and a title; its route is the
//! `ATCWaypoint`s in order, each with an `id`, a type (`Airport`,
//! `Intersection`, `VOR`, `NDB`, `User`) and a `WorldPosition` in degrees,
//! minutes and seconds with hemisphere letters and an altitude in feet:
//! `N10° 30' 36.00",E020° 15' 00.00",+001200.00`. FSX writes an altitude of
//! zero for the fixes of a route; the airports carry their elevation.
//!
//! The INI-like plans of FS2004 are another format, not read here.
use std::io::BufRead;

use bibli_core::CancelToken;
use bibli_core::geo::{LatLon, WaypointKind};

use crate::collect::Collector;
use crate::flight::{self, Ends, Route, Stop};
use crate::xml::{self, Damaged, Tag};

const PLAN: &str = "FlightPlan.FlightPlan";

/// An `ATCWaypoint` being read.
#[derive(Default)]
struct Open {
    id: String,
    kind: String,
    position: String,
    icao: String,
}

fn kind(text: &str) -> WaypointKind {
    match text.trim().to_ascii_lowercase().as_str() {
        "airport" => WaypointKind::Airport,
        "intersection" | "vor" | "ndb" => WaypointKind::Fix,
        _ => WaypointKind::Stop,
    }
}

/// One coordinate as the simulators write it: a hemisphere letter, then
/// degrees, minutes and seconds (`N10° 30' 36.00"`), or degrees and
/// decimal minutes, or decimal degrees. The marks between the numbers are
/// not relied on: a file saved in another code page mangles the degree
/// sign. A sign stands for the letter when there is none.
fn angle(text: &str, positive: char, negative: char) -> Option<f64> {
    let text = text.trim();
    let first = text.chars().next()?;
    let last = text.chars().next_back()?;
    let (sign, rest) = if first.eq_ignore_ascii_case(&positive) || first == '+' {
        (1., &text[1..])
    } else if first.eq_ignore_ascii_case(&negative) || first == '-' {
        (-1., &text[1..])
    } else if last.eq_ignore_ascii_case(&positive) {
        (1., &text[..text.len() - 1])
    } else if last.eq_ignore_ascii_case(&negative) {
        (-1., &text[..text.len() - 1])
    } else {
        return None;
    };
    let mut numbers = rest
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter(|part| !part.is_empty());
    let degrees: f64 = numbers.next()?.parse().ok()?;
    let minutes: f64 = numbers.next().map_or(Some(0.), |m| m.parse().ok())?;
    let seconds: f64 = numbers.next().map_or(Some(0.), |s| s.parse().ok())?;
    if numbers.next().is_some() || minutes >= 60. || seconds >= 60. {
        return None;
    }
    Some(sign * (degrees + minutes / 60. + seconds / 3600.))
}

/// A `WorldPosition` (or a `DepartureLLA`): latitude, longitude and an
/// altitude in feet, separated by commas. The altitude, in metres, is
/// `None` when it is missing or unreadable.
fn world_position(text: &str) -> Option<(LatLon, Option<f32>)> {
    let mut parts = text.split(',');
    let lat = angle(parts.next()?, 'N', 'S')?;
    let lon = angle(parts.next()?, 'E', 'W')?;
    Some((LatLon::new(lat, lon)?, parts.next().and_then(flight::feet)))
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut path: Vec<String> = Vec::new();
    let mut open: Option<Open> = None;
    let mut route = Route::default();
    let mut seen = false;
    let mut title = String::new();
    let (mut departure, mut destination) = (String::new(), String::new());
    let (mut departure_name, mut destination_name) = (String::new(), String::new());
    let mut cruise = None;
    let damaged = xml::walk(input, cancel, |tag| {
        match tag {
            Tag::Start { name, attributes } => {
                if path.is_empty() && name != "SimBase.Document" && name != PLAN {
                    anyhow::bail!("not a flight simulator plan");
                }
                match name {
                    PLAN => seen = true,
                    "ATCWaypoint" => {
                        let id = attributes.iter().find(|(k, _)| k == "id");
                        open = Some(Open {
                            id: id.map(|(_, v)| v.trim().to_owned()).unwrap_or_default(),
                            ..Open::default()
                        });
                    }
                    _ => {}
                }
                path.push(name.to_owned());
            }
            Tag::End { name, text } => {
                path.pop();
                let parent = path.last().map_or("", String::as_str);
                let field = match (name, parent) {
                    ("Title", PLAN) => &mut title,
                    ("DepartureID", PLAN) => &mut departure,
                    ("DestinationID", PLAN) => &mut destination,
                    ("DepartureName", PLAN) => &mut departure_name,
                    ("DestinationName", PLAN) => &mut destination_name,
                    ("CruisingAlt", PLAN) => {
                        cruise = flight::feet(text);
                        return Ok(());
                    }
                    ("Descr", PLAN) => {
                        into.description = Some(text.trim().to_owned()).filter(|d| !d.is_empty());
                        return Ok(());
                    }
                    ("ATCWaypoint", _) => {
                        if let Some(open) = open.take() {
                            stop(
                                &open,
                                &mut route,
                                into,
                                [
                                    (&departure, &departure_name),
                                    (&destination, &destination_name),
                                ],
                            );
                        }
                        return Ok(());
                    }
                    (_, "ATCWaypoint" | "ICAO") => {
                        let Some(open) = open.as_mut() else {
                            return Ok(());
                        };
                        match name {
                            "ATCWaypointType" => &mut open.kind,
                            "WorldPosition" => &mut open.position,
                            "ICAOIdent" => &mut open.icao,
                            _ => return Ok(()),
                        }
                    }
                    _ => return Ok(()),
                };
                text.trim().clone_into(field);
            }
        }
        Ok(())
    })?;
    if !seen && damaged.is_none() {
        anyhow::bail!("not a flight simulator plan");
    }
    route.finish(
        into,
        Ends {
            title: &title,
            departure: Some(departure),
            destination: Some(destination),
            cruise,
        },
    );
    Ok(damaged)
}

/// A waypoint read, on the route. An airport the plan names as an end
/// takes the name the plan gives it.
fn stop(open: &Open, route: &mut Route, into: &mut Collector, ends: [(&String, &String); 2]) {
    let Some((at, altitude)) = world_position(&open.position) else {
        into.invalid();
        return;
    };
    let ident = if open.id.is_empty() {
        &open.icao
    } else {
        &open.id
    };
    let kind = kind(&open.kind);
    let name = ends
        .iter()
        .find(|(end, _)| kind == WaypointKind::Airport && !end.is_empty() && *end == ident)
        .map_or("", |(_, name)| name.as_str());
    route.stop(
        into,
        Stop {
            ident,
            name,
            at,
            kind,
            altitude,
        },
    );
}

/// Whether `head` begins a simulator plan: a `FlightPlan.FlightPlan`, or
/// the `SimBase.Document` around one (other `AceXML` documents, a mission
/// or a scenery's sources, share that root).
pub fn sniff(head: &[u8]) -> bool {
    let needle = PLAN.as_bytes();
    crate::xml::root(head).is_some_and(|(name, _)| {
        name == PLAN
            || (name == "SimBase.Document" && head.windows(needle.len()).any(|w| w == needle))
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::collect::{Plan, PlanKind};
    use bibli_core::geo::{Datum, LineKind};

    pub(crate) const PLAN_FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<SimBase.Document Type="AceXML" version="1,0">
    <Descr>AceXML Document</Descr>
    <FlightPlan.FlightPlan>
        <Title>XAAA to XBBB</Title>
        <FPType>IFR</FPType>
        <RouteType>HighAlt</RouteType>
        <CruisingAlt>12000</CruisingAlt>
        <DepartureID>XAAA</DepartureID>
        <DepartureLLA>N10° 06' 00.00",E020° 06' 00.00",+000500.00</DepartureLLA>
        <DestinationID>XBBB</DestinationID>
        <DestinationLLA>N10° 54' 00.00",E020° 54' 00.00",+000100.00</DestinationLLA>
        <Descr>XAAA, XBBB</Descr>
        <DepartureName>Example Field</DepartureName>
        <DestinationName>Other Invented Airport With A Long Name</DestinationName>
        <AppVersion>
            <AppVersionMajor>10</AppVersionMajor>
            <AppVersionBuild>1</AppVersionBuild>
        </AppVersion>
        <ATCWaypoint id="XAAA">
            <ATCWaypointType>Airport</ATCWaypointType>
            <WorldPosition>N10° 06' 00.00",E020° 06' 00.00",+000500.00</WorldPosition>
            <ICAO>
                <ICAOIdent>XAAA</ICAOIdent>
            </ICAO>
        </ATCWaypoint>
        <ATCWaypoint id="FIXAA">
            <ATCWaypointType>Intersection</ATCWaypointType>
            <WorldPosition>N10° 30' 36.00",E020° 15' 00.00",+000000.00</WorldPosition>
            <ATCAirway>X1</ATCAirway>
            <ICAO>
                <ICAORegion>XX</ICAORegion>
                <ICAOIdent>FIXAA</ICAOIdent>
            </ICAO>
        </ATCWaypoint>
        <ATCWaypoint id="VORAA">
            <ATCWaypointType>VOR</ATCWaypointType>
            <WorldPosition>N10° 45' 00.00",E020° 45' 00.00",+012000.00</WorldPosition>
            <ICAO><ICAOIdent>VORAA</ICAOIdent></ICAO>
        </ATCWaypoint>
        <ATCWaypoint id="BAD">
            <ATCWaypointType>User</ATCWaypointType>
            <WorldPosition>somewhere</WorldPosition>
        </ATCWaypoint>
        <ATCWaypoint id="LAKE">
            <ATCWaypointType>User</ATCWaypointType>
            <WorldPosition>N10° 50' 00.00",E020° 50' 00.00",+003000.00</WorldPosition>
        </ATCWaypoint>
        <ATCWaypoint id="XBBB">
            <ATCWaypointType>Airport</ATCWaypointType>
            <WorldPosition>N10° 54' 00.00",E020° 54' 00.00",+000100.00</WorldPosition>
            <ICAO>
                <ICAOIdent>XBBB</ICAOIdent>
            </ICAO>
        </ATCWaypoint>
    </FlightPlan.FlightPlan>
</SimBase.Document>"#;

    fn read_str(text: &str) -> (bibli_core::geo::GeoDoc, Collector, Option<Damaged>) {
        let mut c = Collector::new();
        let damaged = read(text.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, c) = c.finish();
        (doc, c, damaged)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn degrees_minutes_and_seconds_are_read_with_their_hemispheres() {
        let (at, altitude) =
            world_position("S10° 30' 36.00\",W020° 15' 00.00\",+001000.00").unwrap();
        assert!(close(at.lat, -10.51), "{}", at.lat);
        assert!(close(at.lon, -20.25), "{}", at.lon);
        assert!((altitude.unwrap() - 304.8).abs() < 1e-3);
        // A mangled degree sign, decimal minutes, a letter after.
        assert!(close(angle("N10Â° 30.6'", 'N', 'S').unwrap(), 10.51));
        assert!(close(angle("10.51S", 'N', 'S').unwrap(), -10.51));
        assert!(close(angle("-20.25", 'E', 'W').unwrap(), -20.25));
        assert!(angle("N10° 61' 00\"", 'N', 'S').is_none());
        assert!(angle("10.5", 'N', 'S').is_none());
        assert!(world_position("N95° 00' 00\",E020° 00' 00\",+0").is_none());
        // No altitude is not no position.
        assert!(
            world_position("N10° 00' 00\",E020° 00' 00\"")
                .unwrap()
                .1
                .is_none()
        );
    }

    #[test]
    fn the_route_is_its_waypoints_in_order_with_altitudes_in_metres() {
        let (doc, c, damaged) = read_str(PLAN_FILE);
        assert!(damaged.is_none());
        assert_eq!(doc.lines.len(), 1);
        let line = &doc.lines[0];
        assert_eq!(line.name, "XAAA to XBBB");
        assert_eq!(line.kind, LineKind::Route);
        let points = &line.segments[0];
        assert_eq!(points.len(), 5);
        assert!(close(points[1].at.lat, 10.51));
        assert!(close(points[1].at.lon, 20.25));
        let feet = |p: &bibli_core::geo::Point| p.altitude.map(|m| (m / flight::FOOT).round());
        let altitudes: Vec<_> = points.iter().map(feet).collect();
        // The fix's zero is no altitude.
        assert_eq!(
            altitudes,
            [Some(500.), None, Some(12000.), Some(3000.), Some(100.)]
        );
        assert_eq!(c.invalid, 1);
        assert_eq!(c.name.as_deref(), Some("XAAA to XBBB"));
        assert_eq!(c.description.as_deref(), Some("XAAA, XBBB"));
    }

    #[test]
    fn waypoints_are_airports_fixes_and_user_points_named_by_the_plan() {
        let (doc, _, _) = read_str(PLAN_FILE);
        let stops: Vec<_> = doc
            .waypoints
            .iter()
            .map(|w| (w.name.as_str(), w.kind, w.line))
            .collect();
        assert_eq!(
            stops,
            [
                ("XAAA (Example Field)", WaypointKind::Airport, Some(0)),
                ("FIXAA", WaypointKind::Fix, Some(0)),
                ("VORAA", WaypointKind::Fix, Some(0)),
                ("LAKE", WaypointKind::Stop, Some(0)),
                ("XBBB", WaypointKind::Airport, Some(0)),
            ]
        );
        let (metres, datum) = doc.waypoints[2].altitude.unwrap();
        assert!((metres - 3657.6).abs() < 1e-2);
        assert_eq!(datum, Datum::Sea);
    }

    #[test]
    fn the_plan_states_its_ends_and_cruising_altitude() {
        let (_, c, _) = read_str(PLAN_FILE);
        let plan = c.plan.unwrap();
        assert_eq!(plan.kind, PlanKind::Flight);
        assert_eq!(plan.departure.as_deref(), Some("XAAA"));
        assert_eq!(plan.destination.as_deref(), Some("XBBB"));
        let (metres, datum) = plan.cruise.unwrap();
        assert!((metres - 3657.6).abs() < 1e-2);
        assert_eq!(datum, Datum::Sea);
    }

    #[test]
    fn a_plan_without_its_ends_takes_its_first_and_last_airports() {
        let bare = PLAN_FILE
            .replace("<DepartureID>XAAA</DepartureID>", "")
            .replace("<DestinationID>XBBB</DestinationID>", "")
            .replace("<CruisingAlt>12000</CruisingAlt>", "");
        let (_, c, _) = read_str(&bare);
        assert_eq!(
            c.plan,
            Some(Plan {
                kind: PlanKind::Flight,
                departure: Some("XAAA".into()),
                destination: Some("XBBB".into()),
                cruise: None,
            })
        );
    }

    #[test]
    fn a_file_cut_short_keeps_the_waypoints_before() {
        let end = PLAN_FILE.find("<ATCWaypoint id=\"VORAA\">").unwrap();
        let cut = format!("{}</FlightPlan.FlightPlam>", &PLAN_FILE[..end]);
        let (doc, c, damaged) = read_str(&cut);
        assert!(damaged.is_some());
        assert_eq!(doc.lines[0].segments[0].len(), 2);
        assert_eq!(c.plan.unwrap().departure.as_deref(), Some("XAAA"));
    }

    #[test]
    fn another_document_is_refused() {
        let mut c = Collector::new();
        let other = r#"<SimBase.Document Type="AceXML"><Mission.Mission/></SimBase.Document>"#;
        assert!(read(other.as_bytes(), &mut c, &CancelToken::new()).is_err());
        let gpx = r#"<gpx><wpt lat="1" lon="2"/></gpx>"#;
        assert!(read(gpx.as_bytes(), &mut Collector::new(), &CancelToken::new()).is_err());
    }

    #[test]
    fn a_simulator_plan_is_recognised_from_its_first_bytes() {
        assert!(sniff(PLAN_FILE.as_bytes()));
        assert!(sniff(b"<FlightPlan.FlightPlan><Title>x</Title>"));
        assert!(!sniff(
            b"<SimBase.Document Type=\"AceXML\"><Mission.Mission/>"
        ));
        assert!(!sniff(b"[flightplan]\ntitle=XAAA to XBBB\n"));
        assert!(!sniff(b"<LittleNavmap><Flightplan/></LittleNavmap>"));
    }
}
