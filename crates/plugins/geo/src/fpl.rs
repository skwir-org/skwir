//! Garmin flight plans (`.fpl`, namespace
//! `http://www8.garmin.com/xmlschemas/FlightPlan/v1`), as Garmin's
//! avionics, Garmin Pilot and most planning tools exchange them. The file
//! holds a table of waypoints (`waypoint-table`), in no order, and a route
//! (`route`) whose `route-point`s name them by identifier, type and country
//! code, in the order they are flown. The schema puts the table first, but
//! the route is resolved once the whole file is read, so a writer that puts
//! it first loses nothing. Waypoints of the table the route does not use
//! are places of the file like any other. Elevations are metres, as the
//! schema says (the plan has no cruising altitude).

use std::collections::HashMap;
use std::io::BufRead;

use skwir_core::CancelToken;
use skwir_core::geo::{Datum, LatLon, Waypoint, WaypointKind};

use crate::collect::{self, Collector};
use crate::flight::{Ends, Route, Stop, stop_name};
use crate::xml::{self, Damaged, Tag, root};

pub const NAMESPACE: &str = "http://www8.garmin.com/xmlschemas/FlightPlan/v1";

/// A waypoint of the table, as its children say it.
#[derive(Default)]
struct Entry {
    identifier: String,
    kind: String,
    country: String,
    lat: String,
    lon: String,
    comment: String,
    elevation: String,
}

/// A waypoint of the table, read.
struct Place {
    identifier: String,
    comment: String,
    kind: WaypointKind,
    /// `None` when its position could not be read (counted once, when read).
    at: Option<LatLon>,
    elevation: Option<f32>,
    used: bool,
}

/// A `route-point`: the waypoint it names.
#[derive(Default)]
struct Reference {
    identifier: String,
    kind: String,
    country: String,
}

fn kind(text: &str) -> WaypointKind {
    match text.trim().to_ascii_uppercase().as_str() {
        "AIRPORT" => WaypointKind::Airport,
        "NDB" | "VOR" | "INT" | "INT-VRP" => WaypointKind::Fix,
        // `USER WAYPOINT`, and whatever a writer invents.
        _ => WaypointKind::Stop,
    }
}

/// The table, looked up the way the schema keys it: identifier, type and
/// country code. Writers disagree on country codes (the schema wants them
/// empty for user waypoints; some fill them, some leave them out of the
/// route), so identifier and type alone are the fallback.
#[derive(Default)]
struct Table {
    places: Vec<Place>,
    by_key: HashMap<(String, String, String), usize>,
    by_ident: HashMap<(String, String), usize>,
}

impl Table {
    fn add(&mut self, entry: &Entry, into: &mut Collector) {
        if self.places.len() >= collect::MAX_WAYPOINTS {
            into.dropped_waypoints += 1;
            return;
        }
        let at = collect::position(&entry.lat, &entry.lon);
        if at.is_none() {
            into.invalid();
        }
        let identifier = entry.identifier.trim().to_owned();
        let kind_key = entry.kind.trim().to_ascii_uppercase();
        let index = self.places.len();
        self.by_key
            .entry((
                identifier.clone(),
                kind_key.clone(),
                entry.country.trim().to_owned(),
            ))
            .or_insert(index);
        self.by_ident
            .entry((identifier.clone(), kind_key))
            .or_insert(index);
        self.places.push(Place {
            identifier,
            comment: entry.comment.trim().to_owned(),
            kind: kind(&entry.kind),
            at,
            elevation: collect::altitude(&entry.elevation),
            used: false,
        });
    }

    fn find(&self, reference: &Reference) -> Option<usize> {
        let identifier = reference.identifier.trim().to_owned();
        let kind_key = reference.kind.trim().to_ascii_uppercase();
        self.by_key
            .get(&(
                identifier.clone(),
                kind_key.clone(),
                reference.country.trim().to_owned(),
            ))
            .or_else(|| self.by_ident.get(&(identifier, kind_key)))
            .copied()
    }
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut path: Vec<String> = Vec::new();
    let mut table = Table::default();
    let mut entry: Option<Entry> = None;
    let mut references: Vec<Reference> = Vec::new();
    let mut reference: Option<Reference> = None;
    let mut title = String::new();
    let damaged = xml::walk(input, cancel, |tag| {
        match tag {
            Tag::Start { name, .. } => {
                match name {
                    "flight-plan" => {}
                    _ if path.is_empty() => anyhow::bail!("not a Garmin flight plan"),
                    "waypoint" => entry = Some(Entry::default()),
                    "route-point" => reference = Some(Reference::default()),
                    _ => {}
                }
                path.push(name.to_owned());
            }
            Tag::End { name, text } => {
                path.pop();
                let parent = path.last().map_or("", String::as_str);
                match (name, parent) {
                    ("waypoint", "waypoint-table") => {
                        if let Some(entry) = entry.take() {
                            table.add(&entry, into);
                        }
                    }
                    (_, "waypoint") => {
                        if let Some(entry) = entry.as_mut() {
                            let field = match name {
                                "identifier" => &mut entry.identifier,
                                "type" => &mut entry.kind,
                                "country-code" => &mut entry.country,
                                "lat" => &mut entry.lat,
                                "lon" => &mut entry.lon,
                                "comment" => &mut entry.comment,
                                "elevation" => &mut entry.elevation,
                                _ => return Ok(()),
                            };
                            text.trim().clone_into(field);
                        }
                    }
                    ("route-point", "route") => {
                        if let Some(reference) = reference.take() {
                            if references.len() < collect::MAX_WAYPOINTS {
                                references.push(reference);
                            } else {
                                into.dropped_waypoints += 1;
                            }
                        }
                    }
                    (_, "route-point") => {
                        if let Some(reference) = reference.as_mut() {
                            let field = match name {
                                "waypoint-identifier" => &mut reference.identifier,
                                "waypoint-type" => &mut reference.kind,
                                "waypoint-country-code" => &mut reference.country,
                                _ => return Ok(()),
                            };
                            text.trim().clone_into(field);
                        }
                    }
                    ("route-name", "route") => text.trim().clone_into(&mut title),
                    ("file-description", "flight-plan") => {
                        into.description = Some(text.trim().to_owned()).filter(|d| !d.is_empty());
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })?;

    let mut route = Route::default();
    for reference in &references {
        let Some(index) = table.find(reference) else {
            // A route point the table does not hold has no position.
            into.invalid();
            continue;
        };
        let place = &mut table.places[index];
        place.used = true;
        if let Some(at) = place.at {
            route.stop(
                into,
                Stop {
                    ident: &place.identifier,
                    name: &place.comment,
                    at,
                    kind: place.kind,
                    altitude: place.elevation,
                },
            );
        }
    }
    for place in table.places.iter().filter(|p| !p.used) {
        if let Some(at) = place.at {
            into.waypoint(Waypoint {
                name: stop_name(&place.identifier, &place.comment),
                at,
                kind: place.kind,
                altitude: place.elevation.map(|metres| (metres, Datum::Sea)),
                line: None,
            });
        }
    }
    route.finish(
        into,
        Ends {
            title: &title,
            ..Ends::default()
        },
    );
    Ok(damaged)
}

/// Whether `head` begins a Garmin flight plan: a `flight-plan` root in
/// Garmin's namespace.
pub fn sniff(head: &[u8]) -> bool {
    root(head).is_some_and(|(name, attributes)| {
        name == "flight-plan" && attributes.iter().any(|(_, value)| value == NAMESPACE)
    })
}

#[cfg(test)]
// The values compared are read from short decimals and compared with the
// same decimals, or are products checked with a tolerance.
#[allow(clippy::float_cmp)]
pub(crate) mod tests {
    use super::*;
    use crate::collect::{Plan, PlanKind};
    use crate::flight::feet;
    use skwir_core::geo::LineKind;

    pub(crate) const PLAN: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<flight-plan xmlns="http://www8.garmin.com/xmlschemas/FlightPlan/v1">
  <file-description>An invented trip</file-description>
  <created>20260301T10:00:00Z</created>
  <waypoint-table>
    <waypoint>
      <identifier>XBBB</identifier>
      <type>AIRPORT</type>
      <country-code>XX</country-code>
      <lat>10.900000</lat>
      <lon>20.900000</lon>
      <comment>EXAMPLE FIELD</comment>
      <elevation>120</elevation>
    </waypoint>
    <waypoint>
      <identifier>XAAA</identifier>
      <type>AIRPORT</type>
      <country-code>XX</country-code>
      <lat>10.100000</lat>
      <lon>20.100000</lon>
      <comment>A NAME FAR TOO LONG TO SHOW BESIDE ITS IDENT ON A MAP</comment>
      <elevation>250.5</elevation>
    </waypoint>
    <waypoint>
      <identifier>FIXAA</identifier>
      <type>INT</type>
      <country-code>XX</country-code>
      <lat>10.500000</lat>
      <lon>20.500000</lon>
      <comment />
    </waypoint>
    <waypoint>
      <identifier>USR01</identifier>
      <type>USER WAYPOINT</type>
      <country-code />
      <lat>10.600000</lat>
      <lon>20.400000</lon>
      <comment>LAKE</comment>
    </waypoint>
    <waypoint>
      <identifier>SPARE</identifier>
      <type>VOR</type>
      <country-code>XX</country-code>
      <lat>10.700000</lat>
      <lon>20.700000</lon>
      <comment />
    </waypoint>
    <waypoint>
      <identifier>BROKEN</identifier>
      <type>INT</type>
      <country-code>XX</country-code>
      <lat>north</lat>
      <lon>20.700000</lon>
      <comment />
    </waypoint>
  </waypoint-table>
  <route>
    <route-name>XAAA TO XBBB</route-name>
    <flight-plan-index>1</flight-plan-index>
    <route-point>
      <waypoint-identifier>XAAA</waypoint-identifier>
      <waypoint-type>AIRPORT</waypoint-type>
      <waypoint-country-code>XX</waypoint-country-code>
    </route-point>
    <route-point>
      <waypoint-identifier>FIXAA</waypoint-identifier>
      <waypoint-type>INT</waypoint-type>
      <waypoint-country-code>XX</waypoint-country-code>
    </route-point>
    <route-point>
      <waypoint-identifier>USR01</waypoint-identifier>
      <waypoint-type>USER WAYPOINT</waypoint-type>
      <waypoint-country-code>XX</waypoint-country-code>
    </route-point>
    <route-point>
      <waypoint-identifier>XBBB</waypoint-identifier>
      <waypoint-type>AIRPORT</waypoint-type>
      <waypoint-country-code>XX</waypoint-country-code>
    </route-point>
  </route>
</flight-plan>"#;

    fn read_str(text: &str) -> (skwir_core::geo::GeoDoc, Collector, Option<Damaged>) {
        let mut c = Collector::new();
        let damaged = read(text.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, c) = c.finish();
        (doc, c, damaged)
    }

    #[test]
    fn the_route_follows_its_points_in_the_order_flown() {
        let (doc, c, damaged) = read_str(PLAN);
        assert!(damaged.is_none());
        assert_eq!(doc.lines.len(), 1);
        let line = &doc.lines[0];
        assert_eq!(line.name, "XAAA TO XBBB");
        assert_eq!(line.kind, LineKind::Route);
        assert_eq!(line.datum, Datum::Sea);
        let lats: Vec<_> = line.segments[0].iter().map(|p| p.at.lat).collect();
        assert_eq!(lats, [10.1, 10.5, 10.6, 10.9]);
        // Elevations are metres already.
        let altitudes: Vec<_> = line.segments[0].iter().map(|p| p.altitude).collect();
        assert_eq!(altitudes, [Some(250.5), None, None, Some(120.)]);
        assert_eq!(c.name.as_deref(), Some("XAAA TO XBBB"));
        assert_eq!(c.description.as_deref(), Some("An invented trip"));
    }

    #[test]
    fn stops_are_airports_fixes_and_user_waypoints_and_the_rest_are_places() {
        let (doc, c, _) = read_str(PLAN);
        let stops: Vec<_> = doc
            .waypoints
            .iter()
            .map(|w| (w.name.as_str(), w.kind, w.line))
            .collect();
        assert_eq!(
            stops,
            [
                ("XAAA", WaypointKind::Airport, Some(0)),
                ("FIXAA", WaypointKind::Fix, Some(0)),
                ("USR01 (LAKE)", WaypointKind::Stop, Some(0)),
                ("XBBB (EXAMPLE FIELD)", WaypointKind::Airport, Some(0)),
                ("SPARE", WaypointKind::Fix, None),
            ]
        );
        assert_eq!(doc.waypoints[0].altitude, Some((250.5, Datum::Sea)));
        // The waypoint whose latitude is a word.
        assert_eq!(c.invalid, 1);
    }

    #[test]
    fn the_plan_goes_from_its_first_airport_to_its_last() {
        let (_, c, _) = read_str(PLAN);
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
    fn a_route_point_missing_from_the_table_is_skipped() {
        let plan = PLAN.replace(
            "<waypoint-identifier>FIXAA</waypoint-identifier>",
            "<waypoint-identifier>NOWHERE</waypoint-identifier>",
        );
        let (doc, c, _) = read_str(&plan);
        assert_eq!(doc.lines[0].segments[0].len(), 3);
        assert_eq!(c.invalid, 2);
    }

    #[test]
    fn a_country_code_the_route_disagrees_on_still_finds_its_waypoint() {
        // The route gives the user waypoint a country code the table left
        // empty, as some writers do.
        let (doc, _, _) = read_str(PLAN);
        assert!(doc.waypoints.iter().any(|w| w.name == "USR01 (LAKE)"));
    }

    #[test]
    fn a_file_cut_short_keeps_the_route_read_so_far() {
        // Cut inside the fourth waypoint, and closed by a wrong tag.
        let cut = format!(
            "{}</waypoint-tabel>",
            &PLAN[..PLAN.find("<identifier>USR01").unwrap()]
        );
        let (doc, c, damaged) = read_str(&cut);
        assert!(damaged.is_some());
        // No route was read: the table's waypoints are places.
        assert!(doc.lines.is_empty());
        let names: Vec<_> = doc.waypoints.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["XBBB (EXAMPLE FIELD)", "XAAA", "FIXAA"]);
        assert!(c.plan.is_some());
    }

    #[test]
    fn another_document_is_refused() {
        let mut c = Collector::new();
        let gpx = r#"<gpx><wpt lat="1" lon="2"/></gpx>"#;
        assert!(read(gpx.as_bytes(), &mut c, &CancelToken::new()).is_err());
    }

    #[test]
    fn a_garmin_plan_is_recognised_from_its_first_bytes() {
        assert!(sniff(PLAN.as_bytes()));
        assert!(sniff(&PLAN.as_bytes()[..200]));
        let bom = format!("\u{feff}{PLAN}");
        assert!(sniff(bom.as_bytes()));
        assert!(!sniff(b"<flight-plan><waypoint-table/></flight-plan>"));
        assert!(!sniff(
            b"<gpx xmlns=\"http://www.topografix.com/GPX/1/1\"/>"
        ));
        assert!(!sniff(b"I\n1100 Version\n"));
    }

    #[test]
    fn a_stop_is_named_by_its_ident_and_a_short_name() {
        assert_eq!(stop_name("XAAA", ""), "XAAA");
        assert_eq!(stop_name("XAAA", "xaaa"), "XAAA");
        assert_eq!(stop_name("XAAA", "Example Field"), "XAAA (Example Field)");
        assert_eq!(stop_name("", "Example Field"), "Example Field");
        assert_eq!(stop_name("XAAA", &"long ".repeat(10)), "XAAA");
        assert!((feet("1000").unwrap() - 304.8).abs() < 1e-3);
    }
}
