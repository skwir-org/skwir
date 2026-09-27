//! What the flight plan readers share (`fpl`, `pln`, `fms`, `lnmpln`):
//! the route they build, how they name its stops, and feet.
use bibli_core::geo::{Datum, LatLon, LineKind, Point, Waypoint, WaypointKind};

use crate::collect::{self, Collector, Plan, PlanKind};

/// Feet in a metre's terms: flight plans give altitudes in feet.
pub const FOOT: f32 = 0.3048;

/// An altitude written in feet, in metres.
pub fn feet(text: &str) -> Option<f32> {
    collect::altitude(text).map(|feet| feet * FOOT)
}

/// A stop's name is longer than this: its identifier alone labels it.
const NAME_CHARS: usize = 32;

/// How a stop is labelled: its identifier, which is what a pilot reads on
/// a chart, with its name when the plan gives a short one (`XAAA (Example
/// Field)`).
pub fn stop_name(ident: &str, name: &str) -> String {
    let (ident, name) = (ident.trim(), name.trim());
    if ident.is_empty() {
        name.to_owned()
    } else if name.is_empty()
        || name.eq_ignore_ascii_case(ident)
        || name.chars().count() > NAME_CHARS
    {
        ident.to_owned()
    } else {
        format!("{ident} ({name})")
    }
}

/// A stop of a plan's route, as a reader found it.
#[derive(Clone, Copy)]
pub struct Stop<'a> {
    pub ident: &'a str,
    pub name: &'a str,
    pub at: LatLon,
    pub kind: WaypointKind,
    /// Metres above sea level, converted from what the file writes.
    pub altitude: Option<f32>,
}

/// What a plan says about its ends, beside its stops.
#[derive(Default)]
pub struct Ends<'a> {
    pub title: &'a str,
    pub departure: Option<String>,
    pub destination: Option<String>,
    /// Metres above sea level.
    pub cruise: Option<f32>,
}

/// A flight plan's route as it is read: one line, above sea level, of
/// every stop in order, each also a waypoint of that line. Airways are
/// legs like any other: without navigation data, the fixes between two
/// ends of an airway are not known, and the plan's own points are drawn.
#[derive(Default)]
pub struct Route {
    line: Option<usize>,
    airports: Vec<String>,
}

impl Route {
    /// Plans write an altitude of zero for "none" (a fix without a
    /// constraint, an airport whose elevation the writer did not look up):
    /// a field at sea level loses its altitude, which the map can spare.
    pub fn stop(&mut self, into: &mut Collector, stop: Stop<'_>) {
        if self.line.is_none() {
            self.line = Some(into.line(String::new(), LineKind::Route, Datum::Sea));
            into.segment();
        }
        let altitude = stop.altitude.filter(|metres| metres.abs() > f32::EPSILON);
        into.point(Point {
            at: stop.at,
            altitude,
            time: None,
        });
        into.waypoint(Waypoint {
            name: stop_name(stop.ident, stop.name),
            at: stop.at,
            kind: stop.kind,
            altitude: altitude.map(|metres| (metres, Datum::Sea)),
            line: self.line,
        });
        if stop.kind == WaypointKind::Airport && !stop.ident.trim().is_empty() {
            self.airports.push(stop.ident.trim().to_owned());
        }
    }

    /// Name the route and state the plan. When the plan does not name its
    /// ends, they are its first and last airports (a plan that reaches
    /// only one airport has a departure and no destination).
    pub fn finish(mut self, into: &mut Collector, ends: Ends<'_>) {
        into.end_segment();
        let title = ends.title.trim();
        if !title.is_empty() {
            if self.line.is_some() {
                into.name_line(title);
            }
            into.name = Some(title.to_owned());
        }
        let named =
            |ident: Option<String>| ident.map(|i| i.trim().to_owned()).filter(|i| !i.is_empty());
        let last = if self.airports.len() >= 2 {
            self.airports.pop()
        } else {
            None
        };
        let first = self.airports.into_iter().next();
        into.plan = Some(Plan {
            kind: PlanKind::Flight,
            departure: named(ends.departure).or(first),
            destination: named(ends.destination).or(last),
            cruise: ends
                .cruise
                .filter(|metres| metres.abs() > f32::EPSILON)
                .map(|metres| (metres, Datum::Sea)),
        });
    }
}
