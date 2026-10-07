//! Litchi missions as CSV (`.csv`), as the Litchi app and its Mission Hub
//! export them: a header naming the columns, then a waypoint a row.
//!
//! `latitude,longitude,altitude(m),heading(deg),curvesize(m),rotationdir,
//! gimbalmode,gimbalpitchangle,actiontype1,actionparam1,…,altitudemode,
//! speed(m/s),poi_latitude,poi_longitude,poi_altitude(m),poi_altitudemode,…`
//!
//! Altitudes are above the takeoff unless `altitudemode` is 1, above the
//! ground (older exports have no such column). Exports in imperial units
//! name the columns `altitude(ft)` and so on; they are converted. A
//! waypoint may look at a point of interest (`poi_*`, zero for none), which
//! is placed once however many waypoints look at it. Headings, curves, the
//! gimbal, speeds and actions are left out.
//!
//! The header decides whether a CSV is a mission: a spreadsheet of places
//! is not read as one. Values are plain numbers; quotes around them are
//! tolerated.
use std::collections::HashSet;
use std::io::BufRead;

use bibli_core::CancelToken;
use bibli_core::geo::{Datum, LatLon, Waypoint, WaypointKind};

use crate::collect::{self, Collector, Plan, PlanKind};
use crate::mission::Route;
use crate::xml::Damaged;

const FOOT: f32 = 0.3048;

/// Where the columns read are, from the header.
struct Columns {
    latitude: usize,
    longitude: usize,
    altitude: usize,
    /// Metres in a unit of the file's altitudes.
    scale: f32,
    mode: Option<usize>,
    poi: Option<(usize, usize)>,
    poi_altitude: Option<(usize, f32)>,
    poi_mode: Option<usize>,
}

fn field<'a>(fields: &[&'a str], i: usize) -> Option<&'a str> {
    fields
        .get(i)
        .map(|value| value.trim().trim_matches('"').trim())
}

/// The columns of a Litchi header; `None` for any other CSV. Besides a
/// position and an altitude, a Litchi mission names how its drone turns
/// (`rotationdir`) and points its camera (`gimbalmode`).
fn header(line: &str) -> Option<Columns> {
    let names: Vec<String> = line
        .split(',')
        .map(|name| name.trim().trim_matches('"').to_ascii_lowercase())
        .collect();
    let find = |name: &str| names.iter().position(|n| n == name);
    let metres_or_feet = |name: &str| {
        find(&format!("{name}(m)"))
            .map(|i| (i, 1.))
            .or_else(|| find(&format!("{name}(ft)")).map(|i| (i, FOOT)))
    };
    let (altitude, scale) = metres_or_feet("altitude")?;
    find("rotationdir")?;
    find("gimbalmode")?;
    Some(Columns {
        latitude: find("latitude")?,
        longitude: find("longitude")?,
        altitude,
        scale,
        mode: find("altitudemode"),
        poi: find("poi_latitude").zip(find("poi_longitude")),
        poi_altitude: metres_or_feet("poi_altitude"),
        poi_mode: find("poi_altitudemode"),
    })
}

/// Whether the start of a file is a Litchi mission's header.
pub fn sniff(head: &[u8]) -> bool {
    let head = head.strip_prefix(b"\xef\xbb\xbf").unwrap_or(head);
    let first = head.split(|&b| b == b'\n').next().unwrap_or_default();
    std::str::from_utf8(first).is_ok_and(|line| header(line).is_some())
}

/// `altitudemode`: 1 is above the ground, anything else above the takeoff.
fn datum(fields: &[&str], column: Option<usize>) -> Datum {
    match column.and_then(|i| field(fields, i)) {
        Some("1") => Datum::Ground,
        _ => Datum::Takeoff,
    }
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut columns: Option<Columns> = None;
    let mut route = Route::default();
    let mut rows = 0;
    let mut interests: HashSet<(u64, u64)> = HashSet::new();
    let damaged = crate::text::lines(input, cancel, crate::text::Kind::Plan, |number, line| {
        if number == 0 {
            columns = header(line);
            anyhow::ensure!(
                columns.is_some(),
                "Not a Litchi mission: the CSV's columns are not a mission's"
            );
            return Ok(());
        }
        let Some(columns) = columns.as_ref() else {
            return Ok(());
        };
        if line.trim().is_empty() {
            return Ok(());
        }
        rows += 1;
        let fields: Vec<&str> = line.split(',').collect();
        let at = field(&fields, columns.latitude)
            .zip(field(&fields, columns.longitude))
            .and_then(|(lat, lon)| collect::position(lat, lon));
        let Some(at) = at.filter(|at| at.lat != 0. || at.lon != 0.) else {
            into.invalid();
            return Ok(());
        };
        let altitude = field(&fields, columns.altitude)
            .and_then(collect::altitude)
            .map(|value| (value * columns.scale, datum(&fields, columns.mode)));
        route.stop(rows.to_string(), at, WaypointKind::Stop, altitude, into);
        if let Some(poi) = interest(&fields, columns)
            && interests.len() < collect::MAX_WAYPOINTS
            && interests.insert((poi.at.lat.to_bits(), poi.at.lon.to_bits()))
        {
            into.waypoint(poi);
        }
        Ok(())
    })?;
    anyhow::ensure!(columns.is_some(), "Not a Litchi mission: the file is empty");
    route.finish(into);
    into.plan = Some(Plan {
        kind: PlanKind::Mission,
        ..Plan::default()
    });
    Ok(damaged)
}

/// The point of interest a row looks at, if any.
fn interest(fields: &[&str], columns: &Columns) -> Option<Waypoint> {
    let (lat, lon) = columns.poi?;
    let at: LatLon = collect::position(field(fields, lat)?, field(fields, lon)?)?;
    if at.lat == 0. && at.lon == 0. {
        return None;
    }
    let altitude = columns.poi_altitude.and_then(|(i, scale)| {
        field(fields, i)
            .and_then(collect::altitude)
            .map(|value| (value * scale, datum(fields, columns.poi_mode)))
    });
    Some(Waypoint {
        name: "Point of interest".into(),
        at,
        kind: WaypointKind::Point,
        altitude,
        line: None,
    })
}

#[cfg(test)]
// The values compared are written in the missions, or products exact
// enough to compare with a margin.
#[allow(clippy::float_cmp)]
pub(crate) mod tests {
    use super::*;
    use bibli_core::geo::LineKind;

    /// A header as Litchi writes it, fifteen actions and all.
    pub(crate) fn litchi_header(unit: &str) -> String {
        let actions: Vec<String> = (1..=15)
            .map(|i| format!("actiontype{i},actionparam{i}"))
            .collect();
        format!(
            "latitude,longitude,altitude({unit}),heading(deg),curvesize({unit}),rotationdir,\
             gimbalmode,gimbalpitchangle,{},altitudemode,speed(m/s),poi_latitude,poi_longitude,\
             poi_altitude({unit}),poi_altitudemode,photo_timeinterval,photo_distinterval",
            actions.join(",")
        )
    }

    pub(crate) fn row(
        lat: &str,
        lon: &str,
        altitude: f32,
        mode: u8,
        poi: (f64, f64, f32),
    ) -> String {
        let actions = vec!["-1,0"; 15].join(",");
        format!(
            "{lat},{lon},{altitude},0,0.2,0,2,-90,{actions},{mode},5,{},{},{},0,-1,-1",
            poi.0, poi.1, poi.2
        )
    }

    fn read_text(text: &str) -> anyhow::Result<Collector> {
        let mut c = Collector::new();
        let damaged = read(text.as_bytes(), &mut c, &CancelToken::new())?;
        assert!(damaged.is_none());
        Ok(c)
    }

    #[test]
    fn a_mission_gives_its_waypoints_in_order() {
        let tower = (10.002, 20.002, 15.);
        let text = [
            litchi_header("m"),
            row("10.0", "20.0", 30., 0, (0., 0., 0.)),
            row("10.001", "20.0", 45.5, 0, tower),
            row("ten", "20.0", 45., 0, tower),
            row("10.002", "20.001", 20., 1, tower),
        ]
        .join("\r\n");
        let (doc, c) = read_text(&text).unwrap().finish();
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
                (LatLon::new(10., 20.).unwrap(), Some(30.)),
                (LatLon::new(10.001, 20.).unwrap(), Some(45.5)),
                (LatLon::new(10.002, 20.001).unwrap(), Some(20.)),
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
                ("1", WaypointKind::Stop, Some((30., Datum::Takeoff))),
                ("2", WaypointKind::Stop, Some((45.5, Datum::Takeoff))),
                (
                    "Point of interest",
                    WaypointKind::Point,
                    Some((15., Datum::Takeoff))
                ),
                ("4", WaypointKind::Stop, Some((20., Datum::Ground))),
            ]
        );
        assert_eq!(doc.waypoints[0].line, Some(0));
        assert_eq!(c.invalid, 1);
        assert_eq!(c.plan.unwrap().kind, PlanKind::Mission);
    }

    #[test]
    fn feet_are_converted_to_metres() {
        let text = [
            litchi_header("ft"),
            row("10.0", "20.0", 100., 0, (0., 0., 0.)),
        ]
        .join("\n");
        let (doc, _) = read_text(&text).unwrap().finish();
        let altitude = doc.lines[0].segments[0][0].altitude.unwrap();
        assert!((altitude - 30.48).abs() < 1e-4, "{altitude}");
    }

    #[test]
    fn an_older_header_without_altitude_mode_is_above_the_takeoff() {
        let text = "latitude,longitude,altitude(m),heading(deg),curvesize(m),rotationdir,gimbalmode,gimbalpitchangle\n\
                    10.0,20.0,30,0,0,0,0,0\n\
                    10.001,20.0,35,0,0,0,0,0\n";
        let (doc, _) = read_text(text).unwrap().finish();
        assert_eq!(doc.lines[0].segments[0].len(), 2);
        assert_eq!(doc.waypoints[1].altitude, Some((35., Datum::Takeoff)));
    }

    #[test]
    fn a_csv_that_is_not_a_mission_is_refused() {
        assert!(read_text("name,latitude,longitude,altitude(m)\nA,10,20,5\n").is_err());
        assert!(read_text("").is_err());
        assert!(sniff(litchi_header("m").as_bytes()));
        assert!(sniff(
            format!("\u{feff}{}\r\n10,20", litchi_header("ft")).as_bytes()
        ));
        assert!(!sniff(b"latitude,longitude\n"));
    }
}
