//! What drone missions share, whatever their format (`mavlink`, `qgc`,
//! `litchi`, `wpml`): what `MAVLink`'s commands and frames mean
//! (`MAV_CMD`, `MAV_FRAME`), and [`Route`], which turns a mission's items
//! into its path and its places.
use bibli_core::geo::{Datum, EARTH_RADIUS, LatLon, LineKind, Point, Waypoint, WaypointKind};

use crate::collect::Collector;

/// `MAV_CMD_NAV_RETURN_TO_LAUNCH`: back home from wherever the vehicle is.
/// It has no position of its own.
pub(crate) const RETURN_TO_LAUNCH: u64 = 20;
/// `MAV_CMD_NAV_LAND` and `MAV_CMD_NAV_LOITER_TO_ALT`, for the landing
/// patterns `QGroundControl` saves as positions rather than items.
pub(crate) const LAND: u64 = 21;
pub(crate) const LOITER_TO_ALT: u64 = 31;
/// `MAV_FRAME_GLOBAL` and `MAV_FRAME_GLOBAL_RELATIVE_ALT`, likewise.
pub(crate) const FRAME_SEA: u64 = 0;
pub(crate) const FRAME_TAKEOFF: u64 = 3;

/// Points of the ring a circular fence is drawn as.
pub(crate) const CIRCLE_POINTS: usize = 32;
/// What a mission item is on the map, from its command: `None` for one
/// that does not send the vehicle to a position (a speed, a camera, a
/// jump, a region of interest the camera looks at).
pub(crate) fn stop_kind(command: u64) -> Option<WaypointKind> {
    Some(match command {
        // NAV_TAKEOFF, NAV_VTOL_TAKEOFF.
        22 | 84 => WaypointKind::Takeoff,
        // NAV_LAND, NAV_VTOL_LAND.
        21 | 85 => WaypointKind::Landing,
        // NAV_WAYPOINT, NAV_LOITER_UNLIM, _TURNS, _TIME, _TO_ALT,
        // NAV_SPLINE_WAYPOINT, NAV_PAYLOAD_PLACE.
        16 | 17 | 18 | 19 | 31 | 82 | 94 => WaypointKind::Stop,
        _ => return None,
    })
}

/// What an item's altitude is measured from, from its frame: `None` for a
/// frame that is not a place on the Earth (local, body, the mission's own
/// frame for commands without a position).
pub(crate) fn frame_datum(frame: u64) -> Option<Datum> {
    match frame {
        // GLOBAL, GLOBAL_INT.
        0 | 5 => Some(Datum::Sea),
        // GLOBAL_RELATIVE_ALT, GLOBAL_RELATIVE_ALT_INT.
        3 | 6 => Some(Datum::Takeoff),
        // GLOBAL_TERRAIN_ALT, GLOBAL_TERRAIN_ALT_INT.
        10 | 11 => Some(Datum::Ground),
        _ => None,
    }
}

/// Where a mission item says it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Position {
    At(LatLon),
    /// Where the vehicle already is: `MAVLink` writes 0, 0 (or NaN) for a
    /// takeoff or a landing on the spot.
    Here,
    /// Numbers that are not a position.
    Invalid,
}

impl Position {
    pub(crate) fn new(lat: f64, lon: f64) -> Self {
        // Exactly zero is how the protocol says "no position", not a
        // place in the Gulf of Guinea.
        let unset = lat == 0. && lon == 0.;
        if lat.is_nan() || lon.is_nan() || unset {
            return Position::Here;
        }
        LatLon::new(lat, lon).map_or(Position::Invalid, Position::At)
    }
}

/// A mission item, as any `MAVLink` format gives it.
pub(crate) struct Item {
    pub(crate) name: String,
    pub(crate) command: u64,
    pub(crate) frame: u64,
    pub(crate) position: Position,
    pub(crate) altitude: Option<f32>,
}

/// A mission's path, fed its items in order: one route line, created at
/// its first point, whose datum is the one most of its stops use (a
/// mission may mix frames; its points keep their values as written).
#[derive(Default)]
pub(crate) struct Route {
    started: bool,
    /// Stops so far measured from each datum, in `slot` order.
    datums: [usize; 3],
    /// Where the vehicle is: the last point, or home before any.
    here: Option<LatLon>,
    /// Whether the last command that moved the vehicle was a return home.
    returns: bool,
}

fn slot(datum: Datum) -> usize {
    match datum {
        Datum::Takeoff => 0,
        Datum::Sea => 1,
        Datum::Ground => 2,
    }
}

impl Route {
    /// Where the vehicle returns to; not a point of the path.
    pub(crate) fn home(
        &mut self,
        at: LatLon,
        altitude: Option<(f32, Datum)>,
        into: &mut Collector,
    ) {
        into.waypoint(Waypoint {
            name: "Home".into(),
            at,
            kind: WaypointKind::Home,
            altitude,
            line: None,
        });
        self.here.get_or_insert(at);
    }

    /// A `MAVLink` item: a point of the path when its command goes to a
    /// position, and with `labelled` a stop on the map too. Other commands
    /// are left out, and a position that is not one is counted invalid.
    pub(crate) fn item(&mut self, item: Item, labelled: bool, into: &mut Collector) {
        if item.command == RETURN_TO_LAUNCH {
            self.returns = true;
            return;
        }
        let Some(kind) = stop_kind(item.command) else {
            return;
        };
        let Some(datum) = frame_datum(item.frame) else {
            into.invalid();
            return;
        };
        let at = match item.position {
            Position::At(at) => at,
            Position::Here => match self.here {
                Some(at) => at,
                // On the spot, with no spot known yet.
                None => return,
            },
            Position::Invalid => {
                into.invalid();
                return;
            }
        };
        let altitude = item.altitude.map(|metres| (metres, datum));
        if labelled {
            self.stop(item.name, at, kind, altitude, into);
        } else {
            self.datums[slot(datum)] += 1;
            self.pass(at, item.altitude, into);
        }
    }

    /// A stop of the path, labelled on the map.
    pub(crate) fn stop(
        &mut self,
        name: String,
        at: LatLon,
        kind: WaypointKind,
        altitude: Option<(f32, Datum)>,
        into: &mut Collector,
    ) {
        if let Some((_, datum)) = altitude {
            self.datums[slot(datum)] += 1;
        }
        self.pass(at, altitude.map(|(metres, _)| metres), into);
        into.waypoint(Waypoint {
            name,
            at,
            kind,
            altitude,
            line: into.current_line(),
        });
    }

    /// A point of the path that is no stop of its own: a survey's turns.
    pub(crate) fn pass(&mut self, at: LatLon, altitude: Option<f32>, into: &mut Collector) {
        if !self.started {
            into.line(String::new(), LineKind::Route, Datum::Takeoff);
            self.started = true;
        }
        into.point(Point {
            at,
            altitude,
            time: None,
        });
        self.here = Some(at);
        self.returns = false;
    }

    /// Close the path. Where the mission ends, when that is not its last
    /// point: `Home` after a return to launch.
    pub(crate) fn finish(self, into: &mut Collector) -> Option<String> {
        if self.started {
            into.end_segment();
            into.line_datum(self.datum());
        }
        self.returns.then(|| "Home".to_owned())
    }

    /// The datum most stops use; above the takeoff, a drone's usual, when
    /// none says or on a tie with it.
    fn datum(&self) -> Datum {
        let mut best = Datum::Takeoff;
        for datum in [Datum::Sea, Datum::Ground] {
            if self.datums[slot(datum)] > self.datums[slot(best)] {
                best = datum;
            }
        }
        best
    }
}

/// What a fence is called on the map: the area the vehicle must stay in,
/// or one it must keep out of.
pub(crate) fn fence_name(inclusion: bool) -> &'static str {
    if inclusion {
        "Geofence"
    } else {
        "Exclusion zone"
    }
}

/// A circle of `radius` metres as a ring of [`CIRCLE_POINTS`], on the
/// sphere; `None` for a radius that is not one.
pub(crate) fn circle(center: LatLon, radius: f64) -> Option<Vec<LatLon>> {
    if !radius.is_finite() || radius <= 0. || radius >= EARTH_RADIUS {
        return None;
    }
    let angle = radius / EARTH_RADIUS;
    let (lat, lon) = (center.lat.to_radians(), center.lon.to_radians());
    (0..CIRCLE_POINTS)
        .map(|k| {
            let bearing = std::f64::consts::TAU * k as f64 / CIRCLE_POINTS as f64;
            let to_lat = (lat.sin() * angle.cos() + lat.cos() * angle.sin() * bearing.cos()).asin();
            let to_lon = lon
                + (bearing.sin() * angle.sin() * lat.cos())
                    .atan2(angle.cos() - lat.sin() * to_lat.sin());
            LatLon::new(to_lat.to_degrees(), to_lon.to_degrees())
        })
        .collect()
}
