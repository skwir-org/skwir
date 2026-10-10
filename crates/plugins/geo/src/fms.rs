//! X-Plane flight plans (`.fms`), text. Every version starts with a line
//! `I` (or `A`, a file made on a Mac, whose lines may end with a carriage
//! return alone) and a version line. Version 3 (X-Plane 8 to 10) follows
//! with two numbers (almost always `1`, then the count of waypoints less
//! one) and waypoints `type ident altitude lat lon`. Version 11
//! (`1100 Version`) follows with keys (`CYCLE`, `ADEP` or `DEP` for a
//! departure that is not an airport, `ADES` or `DES`, runways and
//! procedures), `NUMENR` (the count of en-route waypoints) and waypoints
//! `type ident via altitude lat lon`, where `via` is `ADEP`, `ADES`,
//! `DRCT` or an airway. Types: 1 airport, 2 NDB, 3 VOR, 11 fix, 28 a
//! latitude and longitude. Altitudes are feet; positions decimal degrees.
//!
//! The extension is also used by other programs, so a file without the
//! header is refused rather than read as a plan. Procedures (a SID, a
//! STAR, an approach) are names only: their fixes are X-Plane's data.
use std::io::BufRead;

use skwir_core::CancelToken;
use skwir_core::geo::WaypointKind;

use crate::collect::{self, Collector};
use crate::flight::{self, Ends, Route, Stop};
use crate::text;
use crate::xml::Damaged;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Version {
    /// X-Plane 8 to 10: `type ident altitude lat lon`.
    Three,
    /// X-Plane 11 and later: `type ident via altitude lat lon`.
    Eleven,
}

/// The version the first two lines state, or why they are not a plan.
fn version(first: &str, second: &str) -> anyhow::Result<Version> {
    let first = first.trim().trim_start_matches('\u{feff}');
    let mut words = second.split_whitespace();
    let (Some(number), Some(word), None) = (words.next(), words.next(), words.next()) else {
        anyhow::bail!("not an X-Plane flight plan");
    };
    anyhow::ensure!(
        (first == "I" || first == "A") && word.eq_ignore_ascii_case("version"),
        "not an X-Plane flight plan"
    );
    match number.parse::<u32>() {
        Ok(3) => Ok(Version::Three),
        Ok(n) if n >= 1100 => Ok(Version::Eleven),
        _ => anyhow::bail!("an X-Plane flight plan of version {number}, which is not read"),
    }
}

fn kind(code: u32) -> WaypointKind {
    match code {
        1 => WaypointKind::Airport,
        2 | 3 | 11 => WaypointKind::Fix,
        // 28, a position with no name of its own, and codes to come.
        _ => WaypointKind::Stop,
    }
}

/// What the keys of a version 11 plan say, and its waypoints so far.
#[derive(Default)]
struct Keys {
    departure: Option<String>,
    destination: Option<String>,
    /// Departure and destination as the `via` column marks them.
    via_departure: Option<String>,
    via_destination: Option<String>,
    expected: Option<usize>,
    read: usize,
}

impl Keys {
    fn key(&mut self, key: &str, value: &str) {
        let value = Some(value.to_owned()).filter(|v| !v.is_empty());
        match key {
            // An airport outranks a point, whichever line comes first.
            "ADEP" => self.departure = value.or(self.departure.take()),
            "DEP" => self.departure = self.departure.take().or(value),
            "ADES" => self.destination = value.or(self.destination.take()),
            "DES" => self.destination = self.destination.take().or(value),
            "NUMENR" => self.expected = value.and_then(|v| v.parse().ok()),
            _ => {}
        }
    }
}

/// A waypoint line, on the route; a line whose position cannot be read is
/// counted and skipped.
fn waypoint(
    words: &[&str],
    code: u32,
    version: Version,
    keys: &mut Keys,
    route: &mut Route,
    into: &mut Collector,
) {
    keys.read += 1;
    // Type 0 is an empty slot of an old plan.
    if code == 0 {
        return;
    }
    let (ident, altitude, lat, lon) = match (version, words) {
        (Version::Three, [_, ident, altitude, lat, lon, ..]) => (*ident, *altitude, *lat, *lon),
        (Version::Eleven, [_, ident, via, altitude, lat, lon, ..]) => {
            match *via {
                "ADEP" => keys.via_departure = Some((*ident).to_owned()),
                "ADES" => keys.via_destination = Some((*ident).to_owned()),
                _ => {}
            }
            (*ident, *altitude, *lat, *lon)
        }
        _ => {
            into.invalid();
            return;
        }
    };
    let Some(at) = collect::position(lat, lon) else {
        into.invalid();
        return;
    };
    route.stop(
        into,
        Stop {
            ident,
            name: "",
            at,
            kind: kind(code),
            altitude: flight::feet(altitude),
        },
    );
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut header: Vec<String> = Vec::new();
    let mut stated = None;
    let mut route = Route::default();
    let mut keys = Keys::default();
    let mut damaged = text::lines(input, cancel, text::Kind::Plan, |_, line| {
        // The first two lines that are not blank say what the file is.
        let Some(version) = stated else {
            if !line.trim().is_empty() {
                header.push(line.to_owned());
            }
            if let [first, second] = header.as_slice() {
                stated = Some(version(first, second)?);
            }
            return Ok(());
        };
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(first) = words.first() else {
            return Ok(());
        };
        match first.parse::<u32>() {
            // Version 3's two numbers after the header.
            Ok(_) if words.len() == 1 => {}
            Ok(code) => waypoint(&words, code, version, &mut keys, &mut route, into),
            Err(_) if version == Version::Eleven => {
                keys.key(first, words.get(1).copied().unwrap_or(""));
            }
            Err(_) => {}
        }
        Ok(())
    })?;
    // A file that ended before its header said what it is.
    anyhow::ensure!(stated.is_some(), "not an X-Plane flight plan");
    if damaged.is_none()
        && let Some(expected) = keys.expected
        && keys.read < expected
    {
        damaged = Some(Damaged(format!(
            "the plan lists {expected} waypoints and holds {}",
            keys.read
        )));
    }
    route.finish(
        into,
        Ends {
            departure: keys.departure.or(keys.via_departure),
            destination: keys.destination.or(keys.via_destination),
            ..Ends::default()
        },
    );
    Ok(damaged)
}

/// Whether `head` begins an X-Plane plan: `I` or `A`, then a version line.
pub fn sniff(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    let mut lines = text
        .split(['\n', '\r'])
        .map(|line| line.trim().trim_start_matches('\u{feff}'))
        .filter(|line| !line.is_empty());
    match (lines.next(), lines.next()) {
        (Some(first), Some(second)) => version(first, second).is_ok(),
        _ => false,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::collect::{Plan, PlanKind};
    use skwir_core::geo::{Datum, LineKind};

    pub(crate) const ELEVEN: &str = "I
1100 Version
CYCLE 2601
ADEP XAAA
DEPRWY RW09
ADES XBBB
DESRWY RW27
NUMENR 5
1 XAAA ADEP 500.000000 10.100000 20.100000
3 VORAA DRCT 0.000000 10.300000 20.300000
11 FIXAA X1 12000.000000 10.500000 20.500000
28 +10.700_+20.700 DRCT 8000.000000 -10.700000 -20.700000
1 XBBB ADES 100.000000 10.900000 20.900000
";

    const THREE: &str = "A\r3 version\r1\r4\r\
1 XAAA 0.000000 10.100000 20.100000\r\
2 NDBAA 3000 10.200000 20.200000\r\
11 FIXAA 5000.000000 north 20.300000\r\
0 ---- 0.000000 0.000000 0.000000\r\
1 XBBB 100.000000 10.900000 20.900000\r";

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
    fn a_version_11_plan_gives_its_route_in_order_with_altitudes_in_metres() {
        let (doc, c, damaged) = read_str(ELEVEN);
        assert!(damaged.is_none());
        assert_eq!(doc.lines.len(), 1);
        let line = &doc.lines[0];
        assert_eq!(line.kind, LineKind::Route);
        assert_eq!(line.datum, Datum::Sea);
        let points = &line.segments[0];
        let lats: Vec<_> = points.iter().map(|p| p.at.lat).collect();
        assert_eq!(lats, [10.1, 10.3, 10.5, -10.7, 10.9]);
        assert_eq!((points[3].at.lat, points[3].at.lon), (-10.7, -20.7));
        let altitudes: Vec<_> = points.iter().map(feet).collect();
        assert_eq!(
            altitudes,
            [Some(500.), None, Some(12000.), Some(8000.), Some(100.)]
        );
        assert!((points[2].altitude.unwrap() - 3657.6).abs() < 1e-2);
        assert_eq!(c.invalid, 0);
    }

    #[test]
    fn waypoints_are_airports_fixes_and_positions() {
        let (doc, _, _) = read_str(ELEVEN);
        let stops: Vec<_> = doc
            .waypoints
            .iter()
            .map(|w| (w.name.as_str(), w.kind, w.line))
            .collect();
        assert_eq!(
            stops,
            [
                ("XAAA", WaypointKind::Airport, Some(0)),
                ("VORAA", WaypointKind::Fix, Some(0)),
                ("FIXAA", WaypointKind::Fix, Some(0)),
                ("+10.700_+20.700", WaypointKind::Stop, Some(0)),
                ("XBBB", WaypointKind::Airport, Some(0)),
            ]
        );
        let (metres, datum) = doc.waypoints[3].altitude.unwrap();
        assert!((metres - 2438.4).abs() < 1e-2);
        assert_eq!(datum, Datum::Sea);
    }

    #[test]
    fn the_plan_names_its_ends_from_its_keys_or_its_airports() {
        let (_, c, _) = read_str(ELEVEN);
        let expected = Some(Plan {
            kind: PlanKind::Flight,
            departure: Some("XAAA".into()),
            destination: Some("XBBB".into()),
            cruise: None,
        });
        assert_eq!(c.plan, expected);
        // A departure from a point, named by DEP.
        let from_point = ELEVEN.replace("ADEP XAAA", "DEP VORAA");
        let (_, c, _) = read_str(&from_point);
        assert_eq!(c.plan.unwrap().departure.as_deref(), Some("VORAA"));
        // Without keys: the via column, then the airports.
        let (_, c, _) = read_str(THREE);
        assert_eq!(c.plan, expected);
    }

    #[test]
    fn a_version_3_plan_with_mac_line_ends_is_read() {
        let (doc, c, damaged) = read_str(THREE);
        assert!(damaged.is_none());
        let points = &doc.lines[0].segments[0];
        let lats: Vec<_> = points.iter().map(|p| p.at.lat).collect();
        // The fix's latitude is a word; the empty slot is no place.
        assert_eq!(lats, [10.1, 10.2, 10.9]);
        assert_eq!(c.invalid, 1);
        let altitudes: Vec<_> = points.iter().map(feet).collect();
        assert_eq!(altitudes, [None, Some(3000.), Some(100.)]);
        assert_eq!(doc.waypoints[1].kind, WaypointKind::Fix);
    }

    #[test]
    fn a_plan_cut_short_keeps_its_waypoints_and_says_so() {
        let cut = &ELEVEN[..ELEVEN.find("28 +10.700").unwrap()];
        let (doc, _, damaged) = read_str(cut);
        assert!(damaged.is_some());
        assert_eq!(doc.lines[0].segments[0].len(), 3);
    }

    #[test]
    fn a_file_that_is_not_a_plan_is_refused() {
        for text in [
            "",
            "I\n",
            "FMS configuration\nversion = 2\n",
            "I\n12 version\n",
            "<?xml version=\"1.0\"?><flight-plan/>",
            "I\nversion 1100\n1 XAAA ADEP 0 10 20\n",
        ] {
            let mut c = Collector::new();
            assert!(
                read(text.as_bytes(), &mut c, &CancelToken::new()).is_err(),
                "{text:?}"
            );
            assert!(!sniff(text.as_bytes()), "{text:?}");
        }
        let long = format!("I\n{}", "x".repeat(100_000));
        assert!(read(long.as_bytes(), &mut Collector::new(), &CancelToken::new()).is_err());
    }

    #[test]
    fn an_x_plane_plan_is_recognised_from_its_first_bytes() {
        assert!(sniff(ELEVEN.as_bytes()));
        assert!(sniff(THREE.as_bytes()));
        assert!(sniff(b"\xef\xbb\xbfI\r\n1100 Version\r\nCYCLE 2601\r\n"));
    }
}
