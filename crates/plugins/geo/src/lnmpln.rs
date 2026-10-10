//! Little Navmap plans (`.lnmpln`, root `LittleNavmap`, its schema at
//! `https://www.littlenavmap.org/schema/lnmpln.xsd`). A `Flightplan` holds
//! a `Header` (`CruisingAlt` in feet, and `CruisingAltF`, the same as a
//! decimal, which newer versions add; `ProgramName`, `ProgramVersion`,
//! `Comment`), the start position (`Departure`: a parking spot or a
//! runway, not an ident), `Alternates` and the route, `Waypoints`: each
//! `Waypoint` with an `Ident`, a `Name` for airports and navaids, a `Type`
//! (`AIRPORT`, `WAYPOINT`, `VOR`, `NDB`, `USER`, `UNKNOWN`), its `Airway`
//! and `Pos Lon Lat Alt`, decimal degrees and feet. The first and last
//! waypoints are the departure and destination airports; there is no
//! title. Alternates are airports of the file, not stops of the route.
//! Procedures (`SID`, `STAR`, `Approach`) are names only: their fixes are
//! the simulator's data.
use std::io::BufRead;

use skwir_core::CancelToken;
use skwir_core::geo::{Datum, Waypoint, WaypointKind};

use crate::collect::{self, Collector};
use crate::flight::{self, Ends, Route, Stop};
use crate::xml::{self, Damaged, Tag};

/// A `Waypoint` or an `Alternate` being read.
#[derive(Default)]
struct Open {
    name: String,
    ident: String,
    kind: String,
    lat: String,
    lon: String,
    alt: String,
}

fn kind(text: &str) -> WaypointKind {
    match text.trim().to_ascii_uppercase().as_str() {
        "AIRPORT" => WaypointKind::Airport,
        "WAYPOINT" | "VOR" | "NDB" => WaypointKind::Fix,
        _ => WaypointKind::Stop,
    }
}

/// Header fields, kept until the end: `CruisingAltF` outranks
/// `CruisingAlt` whichever comes first.
#[derive(Default)]
struct Header {
    cruise: Option<f32>,
    cruise_decimal: Option<f32>,
    program: String,
    version: String,
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut path: Vec<String> = Vec::new();
    let mut open: Option<Open> = None;
    let mut route = Route::default();
    let mut header = Header::default();
    let damaged = xml::walk(input, cancel, |tag| {
        match tag {
            Tag::Start { name, attributes } => {
                if path.is_empty() && name != "LittleNavmap" {
                    anyhow::bail!("not a Little Navmap plan");
                }
                let parent = path.last().map_or("", String::as_str);
                match (name, parent) {
                    ("Waypoint", "Waypoints") | ("Alternate", "Alternates") => {
                        open = Some(Open::default());
                    }
                    ("Pos", "Waypoint" | "Alternate") => {
                        if let Some(open) = open.as_mut() {
                            // The schema writes `Lat`; lower case is
                            // tolerated.
                            for (key, value) in attributes {
                                let field = match key.to_ascii_lowercase().as_str() {
                                    "lat" => &mut open.lat,
                                    "lon" => &mut open.lon,
                                    "alt" => &mut open.alt,
                                    _ => continue,
                                };
                                value.trim().clone_into(field);
                            }
                        }
                    }
                    _ => {}
                }
                path.push(name.to_owned());
            }
            Tag::End { name, text } => {
                path.pop();
                let parent = path.last().map_or("", String::as_str);
                let text = text.trim();
                match (name, parent) {
                    ("Waypoint", "Waypoints") => {
                        if let Some(open) = open.take() {
                            stop(&open, &mut route, into);
                        }
                    }
                    ("Alternate", "Alternates") => {
                        if let Some(open) = open.take() {
                            alternate(&open, into);
                        }
                    }
                    ("Name" | "Ident" | "Type", "Waypoint" | "Alternate") => {
                        if let Some(open) = open.as_mut() {
                            let field = match name {
                                "Name" => &mut open.name,
                                "Ident" => &mut open.ident,
                                _ => &mut open.kind,
                            };
                            text.clone_into(field);
                        }
                    }
                    ("CruisingAlt", "Header") => header.cruise = flight::feet(text),
                    ("CruisingAltF", "Header") => header.cruise_decimal = flight::feet(text),
                    ("ProgramName", "Header") => text.clone_into(&mut header.program),
                    ("ProgramVersion", "Header") => text.clone_into(&mut header.version),
                    ("Comment", "Header") => {
                        into.description = Some(text.to_owned()).filter(|d| !d.is_empty());
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })?;
    if !header.program.is_empty() {
        into.creator = Some(if header.version.is_empty() {
            header.program
        } else {
            format!("{} {}", header.program, header.version)
        });
    }
    route.finish(
        into,
        Ends {
            cruise: header.cruise_decimal.or(header.cruise),
            ..Ends::default()
        },
    );
    Ok(damaged)
}

fn stop(open: &Open, route: &mut Route, into: &mut Collector) {
    let Some(at) = collect::position(&open.lat, &open.lon) else {
        into.invalid();
        return;
    };
    route.stop(
        into,
        Stop {
            ident: &open.ident,
            name: &open.name,
            at,
            kind: kind(&open.kind),
            altitude: flight::feet(&open.alt),
        },
    );
}

/// An alternate airport: a place of the plan, on no line.
fn alternate(open: &Open, into: &mut Collector) {
    let Some(at) = collect::position(&open.lat, &open.lon) else {
        into.invalid();
        return;
    };
    let altitude = flight::feet(&open.alt).filter(|metres| metres.abs() > f32::EPSILON);
    into.waypoint(Waypoint {
        name: flight::stop_name(&open.ident, &open.name),
        at,
        kind: kind(&open.kind),
        altitude: altitude.map(|metres| (metres, Datum::Sea)),
        line: None,
    });
}

/// Whether `head` begins a Little Navmap plan: a `LittleNavmap` root.
pub fn sniff(head: &[u8]) -> bool {
    crate::xml::root(head).is_some_and(|(name, _)| name == "LittleNavmap")
}

#[cfg(test)]
// The values compared are read from short decimals and compared with the
// same decimals.
#[allow(clippy::float_cmp)]
pub(crate) mod tests {
    use super::*;
    use crate::collect::{Plan, PlanKind};
    use skwir_core::geo::LineKind;

    pub(crate) const PLAN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<LittleNavmap xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="https://www.littlenavmap.org/schema/lnmpln.xsd">
  <Flightplan>
    <Header>
      <FlightplanType>IFR</FlightplanType>
      <CruisingAlt>9000</CruisingAlt>
      <CruisingAltF>9500.00000000</CruisingAltF>
      <Comment>An invented flight</Comment>
      <CreationDate>2026-03-01T10:00:00+01:00</CreationDate>
      <FileVersion>1.2</FileVersion>
      <ProgramName>Little Navmap</ProgramName>
      <ProgramVersion>3.0.0</ProgramVersion>
      <Documentation>https://www.littlenavmap.org/lnmpln.html</Documentation>
    </Header>
    <SimData Cycle="2601">XP12</SimData>
    <NavData Cycle="2601">XP12</NavData>
    <AircraftPerformance>
      <FilePath>example.lnmperf</FilePath>
      <Type>C172</Type>
      <Name>Example performance</Name>
    </AircraftPerformance>
    <Departure>
      <Pos Lon="20.100500" Lat="10.100500" Alt="500.00"/>
      <Start>PARKING 1</Start>
      <Type>Parking</Type>
      <Heading>90</Heading>
    </Departure>
    <Procedures>
      <SID><Name>XDEP1</Name><Runway>09</Runway></SID>
    </Procedures>
    <Alternates>
      <Alternate>
        <Name>Spare Field</Name>
        <Ident>XCCC</Ident>
        <Type>AIRPORT</Type>
        <Pos Lon="20.950000" Lat="10.950000" Alt="80.00"/>
      </Alternate>
    </Alternates>
    <Waypoints>
      <Waypoint>
        <Name>Example Field</Name>
        <Ident>XAAA</Ident>
        <Type>AIRPORT</Type>
        <Pos Lon="20.100000" Lat="10.100000" Alt="500.00"/>
      </Waypoint>
      <Waypoint>
        <Name>Example Beacon</Name>
        <Ident>VORAA</Ident>
        <Region>XX</Region>
        <Type>VOR</Type>
        <Pos Lon="20.300000" Lat="10.300000" Alt="9500.00"/>
      </Waypoint>
      <Waypoint>
        <Ident>FIXAA</Ident>
        <Region>XX</Region>
        <Airway>X1</Airway>
        <Type>WAYPOINT</Type>
        <Pos Lon="20.500000" Lat="10.500000" Alt="9500.00"/>
      </Waypoint>
      <Waypoint>
        <Ident>WP1</Ident>
        <Type>USER</Type>
        <Comment>Turn here</Comment>
        <Pos Lon="-20.700000" Lat="-10.700000"/>
      </Waypoint>
      <Waypoint>
        <Ident>BAD</Ident>
        <Type>USER</Type>
        <Pos Lon="east" Lat="10.8"/>
      </Waypoint>
      <Waypoint>
        <Name>Other Field</Name>
        <Ident>XBBB</Ident>
        <Type>AIRPORT</Type>
        <Pos Lon="20.900000" Lat="10.900000" Alt="100.00"/>
      </Waypoint>
    </Waypoints>
  </Flightplan>
</LittleNavmap>"#;

    fn read_str(text: &str) -> (skwir_core::geo::GeoDoc, Collector, Option<Damaged>) {
        let mut c = Collector::new();
        let damaged = read(text.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, c) = c.finish();
        (doc, c, damaged)
    }

    fn feet(p: &skwir_core::geo::Point) -> Option<f32> {
        p.altitude.map(|m| (m / flight::FOOT).round())
    }

    #[test]
    fn the_route_is_the_waypoints_in_order_with_altitudes_in_metres() {
        let (doc, c, damaged) = read_str(PLAN);
        assert!(damaged.is_none());
        assert_eq!(doc.lines.len(), 1);
        let line = &doc.lines[0];
        assert_eq!(line.kind, LineKind::Route);
        assert_eq!(line.datum, Datum::Sea);
        let points = &line.segments[0];
        let at: Vec<_> = points.iter().map(|p| (p.at.lat, p.at.lon)).collect();
        assert_eq!(
            at,
            [
                (10.1, 20.1),
                (10.3, 20.3),
                (10.5, 20.5),
                (-10.7, -20.7),
                (10.9, 20.9)
            ]
        );
        let altitudes: Vec<_> = points.iter().map(feet).collect();
        assert_eq!(
            altitudes,
            [Some(500.), Some(9500.), Some(9500.), None, Some(100.)]
        );
        assert_eq!(c.invalid, 1);
        assert_eq!(c.creator.as_deref(), Some("Little Navmap 3.0.0"));
        assert_eq!(c.description.as_deref(), Some("An invented flight"));
        assert_eq!(c.name, None);
    }

    #[test]
    fn waypoints_are_airports_fixes_user_points_and_alternates() {
        let (doc, _, _) = read_str(PLAN);
        let places: Vec<_> = doc
            .waypoints
            .iter()
            .map(|w| (w.name.as_str(), w.kind, w.line))
            .collect();
        assert_eq!(
            places,
            [
                ("XCCC (Spare Field)", WaypointKind::Airport, None),
                ("XAAA (Example Field)", WaypointKind::Airport, Some(0)),
                ("VORAA (Example Beacon)", WaypointKind::Fix, Some(0)),
                ("FIXAA", WaypointKind::Fix, Some(0)),
                ("WP1", WaypointKind::Stop, Some(0)),
                ("XBBB (Other Field)", WaypointKind::Airport, Some(0)),
            ]
        );
        let (metres, datum) = doc.waypoints[1].altitude.unwrap();
        assert!((metres - 152.4).abs() < 1e-3);
        assert_eq!(datum, Datum::Sea);
    }

    #[test]
    fn the_plan_flies_from_its_first_airport_to_its_last_at_its_cruise() {
        let (_, c, _) = read_str(PLAN);
        let plan = c.plan.unwrap();
        assert_eq!(plan.kind, PlanKind::Flight);
        assert_eq!(plan.departure.as_deref(), Some("XAAA"));
        assert_eq!(plan.destination.as_deref(), Some("XBBB"));
        // The decimal cruise, 9500 ft, over the whole one.
        let (metres, datum) = plan.cruise.unwrap();
        assert!((metres - 2895.6).abs() < 1e-2);
        assert_eq!(datum, Datum::Sea);
        // An older file, with the whole one only.
        let old = PLAN.replace("<CruisingAltF>9500.00000000</CruisingAltF>", "");
        let (_, c, _) = read_str(&old);
        assert!((c.plan.unwrap().cruise.unwrap().0 - 2743.2).abs() < 1e-2);
    }

    #[test]
    fn a_plan_without_a_header_is_still_read() {
        let bare = r#"<LittleNavmap><Flightplan><Waypoints>
<Waypoint><Ident>XAAA</Ident><Type>AIRPORT</Type><Pos lon="20.1" lat="10.1"/></Waypoint>
<Waypoint><Ident>FIXAA</Ident><Type>WAYPOINT</Type><Pos lon="20.5" lat="10.5"/></Waypoint>
</Waypoints></Flightplan></LittleNavmap>"#;
        let (doc, c, _) = read_str(bare);
        assert_eq!(doc.lines[0].segments[0].len(), 2);
        assert_eq!(
            c.plan,
            Some(Plan {
                kind: PlanKind::Flight,
                departure: Some("XAAA".into()),
                destination: None,
                cruise: None,
            })
        );
        assert_eq!(c.creator, None);
    }

    #[test]
    fn a_file_cut_short_keeps_the_waypoints_before() {
        let end = PLAN.find("<Ident>FIXAA</Ident>").unwrap();
        let cut = format!("{}</Waypiont>", &PLAN[..end]);
        let (doc, c, damaged) = read_str(&cut);
        assert!(damaged.is_some());
        assert_eq!(doc.lines[0].segments[0].len(), 2);
        let plan = c.plan.unwrap();
        assert_eq!(plan.departure.as_deref(), Some("XAAA"));
        assert_eq!(plan.destination, None);
    }

    #[test]
    fn another_document_is_refused() {
        let mut c = Collector::new();
        let gpx = r#"<gpx><wpt lat="1" lon="2"/></gpx>"#;
        assert!(read(gpx.as_bytes(), &mut c, &CancelToken::new()).is_err());
    }

    #[test]
    fn a_little_navmap_plan_is_recognised_from_its_first_bytes() {
        assert!(sniff(PLAN.as_bytes()));
        assert!(sniff(
            &PLAN.as_bytes()[..PLAN.find("<Flightplan>").unwrap()]
        ));
        assert!(!sniff(b"<SimBase.Document><FlightPlan.FlightPlan/>"));
        assert!(!sniff(b"I\n1100 Version\n"));
    }
}
