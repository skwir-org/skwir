//! `MAVLink` missions as text (`.waypoints`, `.txt`, `.mission`): the
//! `QGC WPL` format Mission Planner and `QGroundControl` write for `ArduPilot`
//! and PX4. A first line `QGC WPL 110` (or `120`), then one mission item a
//! line, its fields separated by tabs (by spaces, from some tools):
//!
//! `INDEX CURRENT FRAME COMMAND P1 P2 P3 P4 X Y Z AUTOCONTINUE`
//!
//! X and Y are the latitude and longitude, Z the altitude in metres from
//! what the frame says. `ArduPilot` keeps home at index 0. Mission Planner
//! saves fences (commands 5001 to 5004) and rally points (5100) in the same
//! format; they are read too.
//!
//! What commands and frames mean is `MAVLink`'s (`MAV_CMD`, `MAV_FRAME`),
//! shared with the other mission formats in `mission`.
use std::io::BufRead;

use bibli_core::CancelToken;
use bibli_core::geo::{Area, Datum, LatLon, Waypoint, WaypointKind};

use crate::collect::{self, Collector, Plan, PlanKind};
use crate::mission::{Item, Position, Route, circle, fence_name, frame_datum};
use crate::text;
use crate::xml::Damaged;

/// The mission protocol's fence and rally items.
const FENCE_VERTEX_INCLUSION: u64 = 5001;
const FENCE_VERTEX_EXCLUSION: u64 = 5002;
const FENCE_CIRCLE_INCLUSION: u64 = 5003;
const FENCE_CIRCLE_EXCLUSION: u64 = 5004;
const RALLY_POINT: u64 = 5100;

/// The first line of a text mission: `QGC WPL` and the versions whose
/// columns are the ones read here.
fn is_header(line: &str) -> bool {
    let mut words = line.split_whitespace();
    words.next() == Some("QGC")
        && words.next() == Some("WPL")
        && matches!(words.next(), Some("110" | "120"))
        && words.next().is_none()
}

/// Whether the start of a file is a text mission's.
pub fn sniff(head: &[u8]) -> bool {
    let head = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    let first = head.split(|&b| b == b'\n').next().unwrap_or_default();
    std::str::from_utf8(first).is_ok_and(is_header)
}

/// A line of the file, its fields read.
struct Row {
    index: u64,
    frame: u64,
    command: u64,
    /// A fence's vertex count or a circle's radius.
    param1: f64,
    position: Position,
    altitude: Option<f32>,
}

impl Row {
    fn parse(line: &str) -> Option<Row> {
        let fields: Vec<&str> = line.split_whitespace().take(12).collect();
        if fields.len() < 11 {
            return None;
        }
        let number = |i: usize| fields[i].parse::<f64>().ok();
        let position = match (number(8), number(9)) {
            (Some(lat), Some(lon)) => Position::new(lat, lon),
            _ => Position::Invalid,
        };
        Some(Row {
            index: fields[0].parse().ok()?,
            frame: fields[2].parse().ok()?,
            command: fields[3].parse().ok()?,
            param1: number(4).unwrap_or(f64::NAN),
            position,
            altitude: collect::altitude(fields[10]),
        })
    }

    fn at(&self, into: &mut Collector) -> Option<LatLon> {
        if let Position::At(at) = self.position {
            Some(at)
        } else {
            into.invalid();
            None
        }
    }
}

/// A polygon fence being read: consecutive vertices of one command, as
/// many as the first one's count says.
#[derive(Default)]
struct Fence {
    command: u64,
    expected: usize,
    ring: Vec<LatLon>,
}

impl Fence {
    fn vertex(&mut self, row: &Row, into: &mut Collector) {
        let Some(at) = row.at(into) else {
            return;
        };
        if self.ring.is_empty() || self.command != row.command || self.ring.len() >= self.expected {
            self.finish(into);
            self.command = row.command;
            self.expected = if row.param1 >= 1. {
                (row.param1 as usize).min(collect::MAX_AREA_POINTS)
            } else {
                collect::MAX_AREA_POINTS
            };
        }
        self.ring.push(at);
        if self.ring.len() >= self.expected {
            self.finish(into);
        }
    }

    fn finish(&mut self, into: &mut Collector) {
        if !self.ring.is_empty() {
            into.area(Area {
                name: fence_name(self.command == FENCE_VERTEX_INCLUSION).into(),
                rings: vec![std::mem::take(&mut self.ring)],
            });
        }
    }
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut route = Route::default();
    let mut fence = Fence::default();
    let mut rally = 0;
    let mut header = false;
    let damaged = text::lines(input, cancel, text::Kind::Plan, |number, line| {
        if number == 0 {
            anyhow::ensure!(
                is_header(line),
                "Not a MAVLink mission: its first line is not “QGC WPL 110”"
            );
            header = true;
            return Ok(());
        }
        if line.trim().is_empty() {
            return Ok(());
        }
        let Some(row) = Row::parse(line) else {
            into.invalid();
            return Ok(());
        };
        match row.command {
            FENCE_VERTEX_INCLUSION | FENCE_VERTEX_EXCLUSION => fence.vertex(&row, into),
            FENCE_CIRCLE_INCLUSION | FENCE_CIRCLE_EXCLUSION => {
                if let Some(center) = row.at(into) {
                    match circle(center, row.param1) {
                        Some(ring) => into.area(Area {
                            name: fence_name(row.command == FENCE_CIRCLE_INCLUSION).into(),
                            rings: vec![ring],
                        }),
                        None => into.invalid(),
                    }
                }
            }
            RALLY_POINT => {
                if let Some(at) = row.at(into) {
                    rally += 1;
                    let datum = frame_datum(row.frame).unwrap_or(Datum::Takeoff);
                    into.waypoint(Waypoint {
                        name: format!("R{rally}"),
                        at,
                        kind: WaypointKind::Rally,
                        altitude: row.altitude.map(|metres| (metres, datum)),
                        line: None,
                    });
                }
            }
            // ArduPilot's home, whatever command it is written as.
            _ if row.index == 0 => {
                if let Position::At(at) = row.position {
                    let datum = frame_datum(row.frame).unwrap_or(Datum::Sea);
                    route.home(at, row.altitude.map(|metres| (metres, datum)), into);
                }
            }
            command => route.item(
                Item {
                    name: row.index.to_string(),
                    command,
                    frame: row.frame,
                    position: row.position,
                    altitude: row.altitude,
                },
                true,
                into,
            ),
        }
        Ok(())
    })?;
    anyhow::ensure!(header, "Not a MAVLink mission: the file is empty");
    fence.finish(into);
    let destination = route.finish(into);
    into.plan = Some(Plan {
        kind: PlanKind::Mission,
        destination,
        ..Plan::default()
    });
    Ok(damaged)
}

#[cfg(test)]
// The values compared are written in the missions and exact in binary
// floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::mission::CIRCLE_POINTS;
    use bibli_core::geo::LineKind;

    fn at(lat: f64, lon: f64) -> LatLon {
        LatLon::new(lat, lon).unwrap()
    }

    fn mission(lines: &[&str], end: &str) -> String {
        lines.iter().map(|line| [*line, end].concat()).collect()
    }

    const SURVEY: &[&str] = &[
        "QGC WPL 110",
        // Home, as ArduPilot keeps it.
        "0\t1\t0\t16\t0\t0\t0\t0\t10.000000\t20.000000\t488.000000\t1",
        // A takeoff on the spot.
        "1\t0\t3\t22\t15\t0\t0\t0\t0\t0\t30\t1",
        "2\t0\t3\t16\t0\t0\t0\t0\t10.001\t20.0\t50\t1",
        // A change of speed has no place.
        "3\t0\t3\t178\t1\t8\t-1\t0\t0\t0\t0\t1",
        "4\t0\t3\t16\t0\t0\t0\t0\tnorth\t20.0\t50\t1",
        // Above the ground, and written with spaces.
        "5 0 10 16 0 0 0 0 10.002 20.001 40 1",
        // A landing where the vehicle is, then home.
        "6\t0\t3\t21\t0\t0\t0\t0\t0\t0\t0\t1",
        "7\t0\t3\t20\t0\t0\t0\t0\t0\t0\t0\t1",
    ];

    fn read_text(text: &str) -> anyhow::Result<Collector> {
        let mut c = Collector::new();
        let damaged = read(text.as_bytes(), &mut c, &CancelToken::new())?;
        assert!(damaged.is_none());
        Ok(c)
    }

    #[test]
    fn a_mission_gives_its_route_home_and_stops() {
        let c = read_text(&mission(SURVEY, "\n")).unwrap();
        let (doc, c) = c.finish();
        assert_eq!(doc.lines.len(), 1);
        let line = &doc.lines[0];
        assert_eq!(line.kind, LineKind::Route);
        // Three of four stops are above the takeoff.
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
                (at(10.002, 20.001), Some(40.)),
                (at(10.002, 20.001), Some(0.)),
            ]
        );
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
                ("2", WaypointKind::Stop, Some((50., Datum::Takeoff))),
                ("5", WaypointKind::Stop, Some((40., Datum::Ground))),
                ("6", WaypointKind::Landing, Some((0., Datum::Takeoff))),
            ]
        );
        assert!(doc.waypoints[1..].iter().all(|w| w.line == Some(0)));
        assert_eq!(doc.waypoints[0].line, None);
        assert_eq!(c.invalid, 1);
        let plan = c.plan.unwrap();
        assert_eq!(plan.kind, PlanKind::Mission);
        assert_eq!(plan.destination.as_deref(), Some("Home"));
    }

    #[test]
    fn line_ends_of_windows_and_a_byte_order_mark_are_read() {
        let text = format!("\u{feff}{}", mission(SURVEY, "\r\n"));
        let (doc, c) = read_text(&text).unwrap().finish();
        assert_eq!(doc.lines[0].segments[0].len(), 4);
        assert_eq!(doc.waypoints.len(), 5);
        assert_eq!(c.invalid, 1);
    }

    #[test]
    fn fences_and_rally_points_are_read() {
        let text = mission(
            &[
                "QGC WPL 110",
                "0\t0\t0\t5001\t4\t0\t0\t0\t9.99\t19.99\t0\t1",
                "1\t0\t0\t5001\t4\t0\t0\t0\t9.99\t20.01\t0\t1",
                "2\t0\t0\t5001\t4\t0\t0\t0\t10.01\t20.01\t0\t1",
                "3\t0\t0\t5001\t4\t0\t0\t0\t10.01\t19.99\t0\t1",
                "4\t0\t0\t5004\t100\t0\t0\t0\t10.0\t20.0\t0\t1",
                "5\t0\t3\t5100\t0\t0\t0\t0\t10.005\t20.005\t40\t1",
            ],
            "\n",
        );
        let (doc, _) = read_text(&text).unwrap().finish();
        assert!(doc.lines.is_empty());
        assert_eq!(doc.areas.len(), 2);
        assert_eq!(doc.areas[0].name, "Geofence");
        assert_eq!(doc.areas[0].rings[0].len(), 4);
        assert_eq!(doc.areas[1].name, "Exclusion zone");
        let ring = &doc.areas[1].rings[0];
        assert_eq!(ring.len(), CIRCLE_POINTS);
        let center = at(10., 20.);
        assert!(
            ring.iter()
                .all(|p| (p.distance(center) - 100.).abs() < 0.01)
        );
        assert_eq!(doc.waypoints[0].name, "R1");
        assert_eq!(doc.waypoints[0].kind, WaypointKind::Rally);
        assert_eq!(doc.waypoints[0].altitude, Some((40., Datum::Takeoff)));
    }

    #[test]
    fn a_file_that_is_not_a_mission_is_refused() {
        assert!(read_text("latitude,longitude\n10,20\n").is_err());
        assert!(read_text("").is_err());
        assert!(read_text("QGC WPL 999\n").is_err());
        assert!(sniff(b"QGC WPL 110\r\n0\t1"));
        assert!(!sniff(b"QGC WPLX 110\n"));
    }

    #[test]
    fn text_that_is_not_utf8_keeps_the_lines_before() {
        let mut bytes = mission(&SURVEY[..4], "\n").into_bytes();
        bytes.extend_from_slice(b"5\t0\t3\t16\t\xff\n");
        let mut c = Collector::new();
        let damaged = read(bytes.as_slice(), &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_some());
        let (doc, _) = c.finish();
        assert_eq!(doc.lines[0].segments[0].len(), 2);
    }

    #[test]
    fn a_datum_is_the_one_most_stops_use() {
        let text = mission(
            &[
                "QGC WPL 120",
                "1\t0\t0\t16\t0\t0\t0\t0\t10.001\t20.0\t500\t1",
                "2\t0\t0\t16\t0\t0\t0\t0\t10.002\t20.0\t510\t1",
                "3\t0\t3\t16\t0\t0\t0\t0\t10.003\t20.0\t50\t1",
            ],
            "\n",
        );
        let (doc, c) = read_text(&text).unwrap().finish();
        assert_eq!(doc.lines[0].datum, Datum::Sea);
        // No return home: the mission ends at its last stop.
        assert_eq!(c.plan.unwrap().destination, None);
    }
}
