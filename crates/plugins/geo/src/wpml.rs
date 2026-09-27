//! DJI WPML: the missions DJI Pilot 2 and `FlightHub` write, a KMZ holding
//! `wpmz/template.kml` (what the user planned) and `wpmz/waylines.wpml`
//! (what the drone executes), both KML with elements of the
//! `http://www.dji.com/wpmz/1.0.x` namespace. The executed one is read when
//! there ([`MEMBERS`]); the caller opens the member.
//!
//! Each `Folder` is a wayline, its `Placemark`s the waypoints: a
//! `Point/coordinates` (`lon,lat`) and `wpml:index`, from 0, which orders
//! them whatever the document's order. Heights are
//! `wpml:executeHeight` in the waylines, measured as the folder's
//! `wpml:executeHeightMode` says, and `wpml:height` in the template,
//! measured as its `wpml:heightMode` says, or the folder's
//! `wpml:globalHeight` where `wpml:useGlobalHeight` is 1;
//! `wpml:ellipsoidHeight` is above the WGS 84 ellipsoid, taken as the sea.
//! A mapping template's `Polygon` is the area it surveys.
//!
//! The mission's `wpml:takeOffRefPoint` (`lat,lon,alt`) is where it takes
//! off, and `wpml:finishAction` where it ends. Actions (photos, gimbal
//! turns), speeds and the drone's model are left out.
use std::io::BufRead;

use bibli_core::CancelToken;
use bibli_core::geo::{Area, Datum, LatLon, Waypoint, WaypointKind};

use crate::collect::{self, Collector, Plan, PlanKind};
use crate::mission::Route;
use crate::xml::{self, Damaged, Tag};

/// The documents of a mission's KMZ, the one to read first.
pub const MEMBERS: [&str; 2] = ["wpmz/waylines.wpml", "wpmz/template.kml"];

/// Waypoints kept of a wayline, to be put in order: DJI's own limit is
/// 65 535.
const MAX_MARKS: usize = 100_000;

/// What a height mode measures from.
fn datum(mode: &str) -> Option<Datum> {
    match mode.trim() {
        "relativeToStartPoint" => Some(Datum::Takeoff),
        "WGS84" | "EGM96" => Some(Datum::Sea),
        "aboveGroundLevel" | "realTimeFollowSurface" => Some(Datum::Ground),
        _ => None,
    }
}

/// A waypoint being read.
#[derive(Default)]
struct Mark {
    index: Option<u32>,
    at: Option<LatLon>,
    height: Option<f32>,
    ellipsoid: Option<f32>,
    global: bool,
    rings: Vec<Vec<LatLon>>,
}

/// A wayline being read: its waypoints wait for its end, to be put in
/// order and measured from what the folder says, wherever it says it.
#[derive(Default)]
struct Folder {
    marks: Vec<Mark>,
    datum: Option<Datum>,
    global: Option<f32>,
}

/// `lon,lat[,alt]` tuples separated by whitespace.
fn tuples(text: &str, into: &mut Collector) -> Vec<LatLon> {
    text.split_whitespace()
        .filter_map(|tuple| {
            let mut parts = tuple.split(',');
            let lon = parts.next()?;
            let at = parts.next().and_then(|lat| collect::position(lat, lon));
            if at.is_none() {
                into.invalid();
            }
            at
        })
        .collect()
}

/// State across the walk.
#[derive(Default)]
struct Mission {
    route: Route,
    folder: Option<Folder>,
    mark: Option<Mark>,
    cruise: Option<(f32, Datum)>,
    destination: Option<String>,
}

impl Mission {
    /// Put a wayline's waypoints on the route, in their order, as a
    /// segment of their own.
    fn flush(&mut self, folder: Folder, into: &mut Collector) {
        let datum = folder.datum.unwrap_or(Datum::Takeoff);
        if self.cruise.is_none() {
            self.cruise = folder.global.map(|height| (height, datum));
        }
        let mut marks = folder.marks;
        marks.sort_by_key(|mark| mark.index.unwrap_or(u32::MAX));
        for (i, mark) in marks.into_iter().enumerate() {
            for ring in mark.rings.into_iter().filter(|ring| !ring.is_empty()) {
                into.area(Area {
                    name: "Survey".into(),
                    rings: vec![ring],
                });
            }
            let Some(at) = mark.at else {
                continue;
            };
            let altitude = match (mark.global, folder.global) {
                (true, Some(height)) => Some((height, datum)),
                _ => mark
                    .height
                    .map(|height| (height, datum))
                    .or(mark.ellipsoid.map(|height| (height, Datum::Sea))),
            };
            // Numbered from 1, as DJI Pilot shows them.
            let name = mark
                .index
                .map_or(i + 1, |index| index as usize + 1)
                .to_string();
            self.route
                .stop(name, at, WaypointKind::Stop, altitude, into);
        }
        into.end_segment();
    }
}

/// Read one WPML document (a template or waylines). A damaged document
/// keeps the waypoints read before the damage.
pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut path: Vec<String> = Vec::new();
    let mut mission = Mission::default();
    let damaged = xml::walk(input, cancel, |tag| {
        match tag {
            Tag::Start { name, .. } => {
                match name {
                    "Folder" => mission.folder = Some(Folder::default()),
                    "Placemark" => mission.mark = Some(Mark::default()),
                    _ => {}
                }
                path.push(name.to_owned());
            }
            Tag::End { name, text } => {
                path.pop();
                let parent = path.last().map_or("", String::as_str);
                end(name, parent, text, &mut mission, into);
            }
        }
        Ok(())
    })?;
    // A document cut short: its last wayline as far as it was read.
    if let Some(folder) = mission.folder.take() {
        mission.flush(folder, into);
    }
    let destination = mission.destination.take();
    let cruise = mission.cruise;
    mission.route.finish(into);
    into.plan = Some(Plan {
        kind: PlanKind::Mission,
        destination,
        cruise,
        ..Plan::default()
    });
    Ok(damaged)
}

/// The waypoint being read, or one for a position outside any.
fn mark(mission: &mut Mission) -> &mut Mark {
    mission.mark.get_or_insert_default()
}

fn end(name: &str, parent: &str, text: &str, mission: &mut Mission, into: &mut Collector) {
    match (name, parent) {
        ("author", "Document") => {
            into.creator = Some(text.trim().to_owned()).filter(|a| !a.is_empty());
        }
        ("finishAction", "missionConfig") => {
            mission.destination = match text.trim() {
                "goHome" => Some("Home".into()),
                "backToFirstWaypoint" => Some("First waypoint".into()),
                _ => None,
            };
        }
        ("takeOffRefPoint", "missionConfig") => {
            let mut parts = text.trim().split(',');
            let at = parts
                .next()
                .zip(parts.next())
                .and_then(|(lat, lon)| collect::position(lat, lon));
            match at {
                Some(at) => into.waypoint(Waypoint {
                    name: "Takeoff".into(),
                    at,
                    kind: WaypointKind::Takeoff,
                    altitude: parts
                        .next()
                        .and_then(collect::altitude)
                        .map(|height| (height, Datum::Sea)),
                    line: None,
                }),
                None => into.invalid(),
            }
        }
        ("heightMode" | "executeHeightMode", _) => {
            if let Some(folder) = mission.folder.as_mut() {
                folder.datum = datum(text).or(folder.datum);
            }
        }
        ("globalHeight", "Folder") => {
            if let Some(folder) = mission.folder.as_mut() {
                folder.global = collect::altitude(text);
            }
        }
        ("index", "Placemark") => mark(mission).index = text.trim().parse().ok(),
        ("height" | "executeHeight", "Placemark") => {
            mark(mission).height = collect::altitude(text);
        }
        ("ellipsoidHeight", "Placemark") => {
            mark(mission).ellipsoid = collect::altitude(text);
        }
        ("useGlobalHeight", "Placemark") => mark(mission).global = text.trim() == "1",
        ("coordinates", "Point") => {
            let at = tuples(text, into).into_iter().next();
            mark(mission).at = at;
        }
        ("coordinates", "LinearRing") => {
            let ring = tuples(text, into);
            mark(mission).rings.push(ring);
        }
        ("Placemark", _) => {
            if let Some(mark) = mission.mark.take() {
                let folder = mission.folder.get_or_insert_default();
                if folder.marks.len() < MAX_MARKS {
                    folder.marks.push(mark);
                } else {
                    into.dropped_waypoints += 1;
                }
            }
        }
        ("Folder", _) => {
            if let Some(folder) = mission.folder.take() {
                mission.flush(folder, into);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
// The values compared are written in the missions and exact in binary
// floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use bibli_core::geo::LineKind;

    fn at(lat: f64, lon: f64) -> LatLon {
        LatLon::new(lat, lon).unwrap()
    }

    /// Waylines as DJI Pilot writes them, the waypoints out of order.
    const WAYLINES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:wpml="http://www.dji.com/wpmz/1.0.6">
<Document>
  <wpml:author>Example Pilot</wpml:author>
  <wpml:missionConfig>
    <wpml:flyToWaylineMode>safely</wpml:flyToWaylineMode>
    <wpml:finishAction>goHome</wpml:finishAction>
    <wpml:takeOffSecurityHeight>20</wpml:takeOffSecurityHeight>
    <wpml:takeOffRefPoint>10.000000,20.000000,512.5</wpml:takeOffRefPoint>
  </wpml:missionConfig>
  <Folder>
    <wpml:templateId>0</wpml:templateId>
    <wpml:executeHeightMode>relativeToStartPoint</wpml:executeHeightMode>
    <wpml:waylineId>0</wpml:waylineId>
    <Placemark>
      <Point><coordinates>20.002,10.002</coordinates></Point>
      <wpml:index>2</wpml:index>
      <wpml:executeHeight>70</wpml:executeHeight>
    </Placemark>
    <Placemark>
      <Point><coordinates>
        20.0,10.0
      </coordinates></Point>
      <wpml:index>0</wpml:index>
      <wpml:executeHeight>50</wpml:executeHeight>
      <wpml:actionGroup><wpml:actionGroupId>0</wpml:actionGroupId><wpml:action><wpml:actionId>0</wpml:actionId></wpml:action></wpml:actionGroup>
    </Placemark>
    <Placemark>
      <Point><coordinates>20.001,10.001</coordinates></Point>
      <wpml:index>1</wpml:index>
      <wpml:executeHeight>60</wpml:executeHeight>
    </Placemark>
  </Folder>
</Document>
</kml>"#;

    const TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:wpml="http://www.dji.com/wpmz/1.0.2">
<Document>
  <wpml:missionConfig><wpml:finishAction>noAction</wpml:finishAction></wpml:missionConfig>
  <Folder>
    <wpml:templateType>waypoint</wpml:templateType>
    <wpml:waylineCoordinateSysParam>
      <wpml:coordinateMode>WGS84</wpml:coordinateMode>
      <wpml:heightMode>EGM96</wpml:heightMode>
    </wpml:waylineCoordinateSysParam>
    <wpml:globalHeight>580</wpml:globalHeight>
    <Placemark>
      <Point><coordinates>20.0,10.0</coordinates></Point>
      <wpml:index>0</wpml:index>
      <wpml:ellipsoidHeight>590</wpml:ellipsoidHeight>
      <wpml:height>575</wpml:height>
      <wpml:useGlobalHeight>1</wpml:useGlobalHeight>
    </Placemark>
    <Placemark>
      <Point><coordinates>20.001,10.001</coordinates></Point>
      <wpml:index>1</wpml:index>
      <wpml:height>600</wpml:height>
      <wpml:useGlobalHeight>0</wpml:useGlobalHeight>
    </Placemark>
    <Placemark>
      <Polygon><outerBoundaryIs><LinearRing><coordinates>
        20.0,10.0,0 20.002,10.0,0 20.002,10.002,0 20.0,10.002,0
      </coordinates></LinearRing></outerBoundaryIs></Polygon>
    </Placemark>
  </Folder>
</Document>
</kml>"#;

    #[test]
    fn waylines_give_their_waypoints_in_index_order() {
        let mut c = Collector::new();
        let damaged = read(WAYLINES.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_none());
        let (doc, c) = c.finish();
        assert_eq!(c.creator.as_deref(), Some("Example Pilot"));
        let plan = c.plan.unwrap();
        assert_eq!(plan.kind, PlanKind::Mission);
        assert_eq!(plan.destination.as_deref(), Some("Home"));
        assert_eq!(plan.cruise, None);
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
                (at(10., 20.), Some(50.)),
                (at(10.001, 20.001), Some(60.)),
                (at(10.002, 20.002), Some(70.)),
            ]
        );
        let stops: Vec<_> = doc
            .waypoints
            .iter()
            .map(|w| (w.name.as_str(), w.kind, w.altitude, w.line))
            .collect();
        assert_eq!(
            stops,
            [
                (
                    "Takeoff",
                    WaypointKind::Takeoff,
                    Some((512.5, Datum::Sea)),
                    None
                ),
                (
                    "1",
                    WaypointKind::Stop,
                    Some((50., Datum::Takeoff)),
                    Some(0)
                ),
                (
                    "2",
                    WaypointKind::Stop,
                    Some((60., Datum::Takeoff)),
                    Some(0)
                ),
                (
                    "3",
                    WaypointKind::Stop,
                    Some((70., Datum::Takeoff)),
                    Some(0)
                ),
            ]
        );
    }

    #[test]
    fn a_template_gives_its_heights_global_height_and_survey() {
        let mut c = Collector::new();
        read(TEMPLATE.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, c) = c.finish();
        let plan = c.plan.unwrap();
        assert_eq!(plan.cruise, Some((580., Datum::Sea)));
        assert_eq!(plan.destination, None);
        let line = &doc.lines[0];
        assert_eq!(line.datum, Datum::Sea);
        let altitudes: Vec<_> = line.segments[0].iter().map(|p| p.altitude).collect();
        assert_eq!(altitudes, [Some(580.), Some(600.)]);
        assert_eq!(doc.areas.len(), 1);
        assert_eq!(doc.areas[0].name, "Survey");
        assert_eq!(doc.areas[0].rings[0].len(), 4);
    }

    #[test]
    fn a_damaged_document_keeps_what_came_before() {
        let cut = &WAYLINES[..WAYLINES.find("<wpml:index>1").unwrap()];
        let cut = format!("{cut}</Folder></Placemark>");
        let mut c = Collector::new();
        let damaged = read(cut.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_some());
        let (doc, _) = c.finish();
        let points: Vec<_> = doc.lines[0].segments[0].iter().map(|p| p.at).collect();
        assert_eq!(points, [at(10., 20.), at(10.002, 20.002)]);
    }
}
