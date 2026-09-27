//! Places on the Earth, as the `geo` plugin reads them from a file (a GPS
//! track, a route, a flight or drone plan) and the UI draws them on a map
//! (ADR-020). Coordinates are WGS 84 degrees, as every format here writes
//! them; distances are metres on a sphere of the Earth's mean radius.
//!
//! A document is bounded by the provider that builds it: a line keeps at
//! most as many points as the map can use, however many the file has.
use std::time::SystemTime;

/// The Earth's mean radius (IUGG), for distances along its surface.
pub const EARTH_RADIUS: f64 = 6_371_008.8;

/// A position in decimal degrees: latitude north, longitude east.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    /// A position, if it is one: finite, latitude within ±90°, longitude
    /// within ±180° (a longitude up to 360°, which some formats write, is
    /// brought back into range).
    pub fn new(lat: f64, lon: f64) -> Option<Self> {
        if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90. || lon.abs() > 360. {
            return None;
        }
        let lon = if lon > 180. {
            lon - 360.
        } else if lon < -180. {
            lon + 360.
        } else {
            lon
        };
        Some(LatLon { lat, lon })
    }

    /// The distance to `other` along the Earth's surface (haversine).
    pub fn distance(self, other: LatLon) -> f64 {
        let (lat1, lat2) = (self.lat.to_radians(), other.lat.to_radians());
        let dlat = lat2 - lat1;
        let dlon = (other.lon - self.lon).to_radians();
        let a = (dlat / 2.).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.).sin().powi(2);
        2. * EARTH_RADIUS * a.sqrt().min(1.).asin()
    }
}

/// A rectangle of latitude and longitude. `west` greater than `east` means
/// it crosses the ±180° meridian: a track over the Bering Strait spans a few
/// degrees, not the rest of the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub south: f64,
    pub north: f64,
    pub west: f64,
    pub east: f64,
}

impl Bounds {
    pub fn crosses_antimeridian(&self) -> bool {
        self.west > self.east
    }

    /// How many degrees of longitude it spans, across the meridian or not.
    pub fn width(&self) -> f64 {
        if self.crosses_antimeridian() {
            self.east + 360. - self.west
        } else {
            self.east - self.west
        }
    }

    pub fn center(&self) -> LatLon {
        let mut lon = self.west + self.width() / 2.;
        if lon > 180. {
            lon -= 360.;
        }
        LatLon {
            lat: f64::midpoint(self.south, self.north),
            lon,
        }
    }
}

/// Gathers positions into the narrowest [`Bounds`] that holds them: of the
/// two ways round the Earth, the one that does not pass the largest gap.
/// Constant memory, so a provider can feed it every point it reads.
#[derive(Clone, Debug, Default)]
pub struct BoundsBuilder {
    lat: Option<(f64, f64)>,
    /// Longitudes as written (−180…180) and shifted to 0…360.
    plain: Option<(f64, f64)>,
    shifted: Option<(f64, f64)>,
}

fn widen(range: &mut Option<(f64, f64)>, value: f64) {
    *range = Some(match *range {
        Some((low, high)) => (low.min(value), high.max(value)),
        None => (value, value),
    });
}

impl BoundsBuilder {
    pub fn add(&mut self, at: LatLon) {
        widen(&mut self.lat, at.lat);
        widen(&mut self.plain, at.lon);
        widen(
            &mut self.shifted,
            if at.lon < 0. { at.lon + 360. } else { at.lon },
        );
    }

    pub fn finish(&self) -> Option<Bounds> {
        let (south, north) = self.lat?;
        let (west, east) = self.plain?;
        let (shifted_west, shifted_east) = self.shifted?;
        // Across the meridian only when that is narrower: a set that
        // straddles 0° shifted would span it the long way.
        let (west, east) = if shifted_east - shifted_west < east - west {
            let back = |lon: f64| if lon > 180. { lon - 360. } else { lon };
            (back(shifted_west), back(shifted_east))
        } else {
            (west, east)
        };
        Some(Bounds {
            south,
            north,
            west,
            east,
        })
    }
}

/// What an altitude is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Datum {
    /// Above mean sea level: a GPS track's elevations, a flight plan's
    /// altitudes.
    #[default]
    Sea,
    /// Above the ground below.
    Ground,
    /// Above where the flight took off: a drone mission's heights.
    Takeoff,
}

/// A point of a line: where, how high, and when it was recorded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub at: LatLon,
    /// Metres from the line's [`Line::datum`].
    pub altitude: Option<f32>,
    pub time: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineKind {
    /// Where someone went, as it was recorded.
    #[default]
    Track,
    /// Where someone means to go: a planned route, a flight plan, a
    /// mission's path.
    Route,
}

/// A track or a route. A track that was paused or lost its signal has
/// several segments; nothing joins them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub name: String,
    pub kind: LineKind,
    pub datum: Datum,
    pub segments: Vec<Vec<Point>>,
}

/// What a named place of a file is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WaypointKind {
    /// A place of interest.
    #[default]
    Point,
    /// A stop of a route, in its order ([`Waypoint::line`]).
    Stop,
    Airport,
    /// A navigation fix of a flight plan (an intersection, a beacon).
    Fix,
    Takeoff,
    Landing,
    /// Where a drone returns to.
    Home,
    /// Where a drone may be sent instead of home.
    Rally,
}

/// A named place: a waypoint, a stop of a route, an airport of a plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub name: String,
    pub at: LatLon,
    pub kind: WaypointKind,
    pub altitude: Option<(f32, Datum)>,
    /// The line it is a stop of, as an index of [`GeoDoc::lines`].
    pub line: Option<usize>,
}

/// A surface: a polygon's outer ring, then its holes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Area {
    pub name: String,
    pub rings: Vec<Vec<LatLon>>,
}

/// Everything a file places on the map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeoDoc {
    pub lines: Vec<Line>,
    pub waypoints: Vec<Waypoint>,
    pub areas: Vec<Area>,
    /// What all of it covers; `None` when nothing has a position.
    pub bounds: Option<Bounds>,
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn at(lat: f64, lon: f64) -> LatLon {
        LatLon::new(lat, lon).expect("a position")
    }

    #[test]
    fn a_position_is_checked_and_brought_into_range() {
        assert!(LatLon::new(91., 0.).is_none());
        assert!(LatLon::new(f64::NAN, 0.).is_none());
        assert!(LatLon::new(0., 400.).is_none());
        assert_eq!(at(10., 190.).lon, -170.);
        assert_eq!(at(10., -190.).lon, 170.);
    }

    #[test]
    fn a_degree_of_the_equator_is_about_111_km() {
        let d = at(0., 0.).distance(at(0., 1.));
        assert!((d - 111_195.).abs() < 1., "{d}");
        // Across the meridian, the short way.
        let d = at(0., 179.5).distance(at(0., -179.5));
        assert!((d - 111_195.).abs() < 1., "{d}");
    }

    #[test]
    fn bounds_take_the_short_way_round() {
        let mut near_zero = BoundsBuilder::default();
        near_zero.add(at(1., -2.));
        near_zero.add(at(3., 4.));
        let b = near_zero.finish().expect("bounds");
        assert_eq!((b.west, b.east, b.south, b.north), (-2., 4., 1., 3.));
        assert!(!b.crosses_antimeridian());

        let mut strait = BoundsBuilder::default();
        strait.add(at(65., 179.));
        strait.add(at(66., -178.));
        let b = strait.finish().expect("bounds");
        assert!(b.crosses_antimeridian());
        assert_eq!((b.west, b.east), (179., -178.));
        assert!((b.width() - 3.).abs() < 1e-9);
        assert!((b.center().lon + 179.5).abs() < 1e-9);

        assert!(BoundsBuilder::default().finish().is_none());
    }
}
