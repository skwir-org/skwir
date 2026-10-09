//! What the parsers hand their places to. It keeps a bounded document for
//! the map and works out the facts over every point read: a track of a
//! million points keeps `MAX_POINTS` of them, evenly, and its distance,
//! time and climb are still the whole track's.
use std::time::{Duration, SystemTime};

use bibli_core::geo::{
    Area, BoundsBuilder, Datum, GeoDoc, LatLon, Line, LineKind, Point, Waypoint,
};

/// Points kept for the map, across all lines. When a file has more, every
/// other point is let go and the stride doubles, so what is kept stays
/// spread over the whole of it.
pub const MAX_POINTS: usize = 200_000;
/// Segments kept for the map, across all lines: each keeps its ends
/// however it is thinned, so past a quarter of `MAX_POINTS` of them (a
/// map of road segments, a line each) thinning frees nothing.
pub const MAX_SEGMENTS: usize = MAX_POINTS / 4;
pub const MAX_WAYPOINTS: usize = 10_000;
pub const MAX_AREAS: usize = 2_000;
/// Points of all areas' rings together.
pub const MAX_AREA_POINTS: usize = 100_000;
/// A rise or a fall counts once it passes this, from the last turn: GPS
/// elevations wander by a few metres, and summing every wobble would
/// double a day's climb.
pub const CLIMB_THRESHOLD: f32 = 3.;
/// Slower than this between two points is standing still (1 km/h).
pub const MOVING_SPEED: f64 = 1. / 3.6;
/// A longer gap between two points is a pause, whatever was covered: the
/// device slept or lost its signal.
pub const LONGEST_STEP: Duration = Duration::from_mins(5);

/// What a flight plan or a drone mission says about itself beside its
/// route (step D′). Altitudes are in metres, whatever unit the file uses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub kind: PlanKind,
    /// Where it leaves from and where it goes, as the plan names them: an
    /// airport's ICAO code for a flight, a place's name for a mission.
    pub departure: Option<String>,
    pub destination: Option<String>,
    /// The cruising altitude the plan states, and what it is measured from.
    pub cruise: Option<(f32, Datum)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlanKind {
    /// An aircraft's flight plan: airports, fixes, a cruising altitude.
    #[default]
    Flight,
    /// A drone's mission: takeoff, waypoints, home, rally points, a fence.
    Mission,
}

/// What the lines of one kind add up to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Totals {
    /// Lines holding points, counted by `Collector::finish`.
    pub lines: usize,
    pub points: usize,
    /// Metres along the lines, segment by segment.
    pub distance: f64,
    pub climb: f64,
    pub descent: f64,
    pub highest: Option<f32>,
    pub lowest: Option<f32>,
    pub first: Option<SystemTime>,
    pub last: Option<SystemTime>,
    pub moving: Duration,
}

/// Where the segment being read stands.
#[derive(Default)]
struct Segment {
    last: Option<Point>,
    /// The elevation of the last turn, for the climb's threshold.
    turn: Option<f32>,
    /// Points of the segment read so far, stored or not.
    seen: usize,
    /// The last point read was not stored: the segment's end must be.
    pending: Option<Point>,
}

#[derive(Default)]
pub struct Collector {
    doc: GeoDoc,
    bounds: BoundsBuilder,
    stride: usize,
    stored: usize,
    /// Segments kept (`MAX_SEGMENTS`).
    segments: usize,
    /// Nothing more is kept for the map: segments past `MAX_SEGMENTS`, or
    /// thinning no longer making room. The facts still count every point.
    full: bool,
    area_points: usize,
    segment: Segment,
    /// Whether the current line has a segment open.
    open: bool,
    pub tracks: Totals,
    pub routes: Totals,
    /// Places left out: beyond the bounds, or without a position.
    pub dropped_waypoints: usize,
    pub dropped_areas: usize,
    pub invalid: usize,
    /// A flight plan's or a mission's own facts; `None` for a track file.
    pub plan: Option<Plan>,
    /// What the file says about itself.
    pub name: Option<String>,
    pub creator: Option<String>,
    pub description: Option<String>,
}

impl Collector {
    pub fn new() -> Self {
        Collector {
            stride: 1,
            ..Collector::default()
        }
    }

    /// Start a line; the points that follow belong to it.
    pub fn line(&mut self, name: String, kind: LineKind, datum: Datum) -> usize {
        self.end_segment();
        if self.full && !self.doc.lines.is_empty() {
            return self.doc.lines.len() - 1;
        }
        self.doc.lines.push(Line {
            name,
            kind,
            datum,
            segments: Vec::new(),
        });
        self.doc.lines.len() - 1
    }

    /// Start a segment of the current line: nothing joins it to the last.
    pub fn segment(&mut self) {
        self.end_segment();
        if self.doc.lines.is_empty() {
            self.line(String::new(), LineKind::Track, Datum::Sea);
        }
        if self.segments >= MAX_SEGMENTS {
            self.full = true;
        }
        if !self.full
            && let Some(line) = self.doc.lines.last_mut()
        {
            line.segments.push(Vec::new());
            self.segments += 1;
        }
        self.open = true;
    }

    fn totals(&mut self, kind: LineKind) -> &mut Totals {
        match kind {
            LineKind::Track => &mut self.tracks,
            LineKind::Route => &mut self.routes,
        }
    }

    /// A point of the current segment, opened if there is none.
    pub fn point(&mut self, point: Point) {
        if !self.open {
            self.segment();
        }
        let kind = self.doc.lines.last().map_or(LineKind::Track, |l| l.kind);
        self.bounds.add(point.at);
        let segment = std::mem::take(&mut self.segment);
        let segment = self.count(kind, segment, point);
        self.segment = segment;
        if self.segment.seen.is_multiple_of(self.stride) {
            self.store(point);
            self.segment.pending = None;
        } else {
            self.segment.pending = Some(point);
        }
        self.segment.seen += 1;
    }

    /// The facts of one more point, from the one before it in its segment.
    fn count(&mut self, kind: LineKind, mut segment: Segment, point: Point) -> Segment {
        let totals = self.totals(kind);
        totals.points += 1;
        if let Some(altitude) = point.altitude {
            totals.highest = Some(totals.highest.map_or(altitude, |h| h.max(altitude)));
            totals.lowest = Some(totals.lowest.map_or(altitude, |l| l.min(altitude)));
            match segment.turn {
                None => segment.turn = Some(altitude),
                Some(turn) if altitude - turn >= CLIMB_THRESHOLD => {
                    totals.climb += f64::from(altitude - turn);
                    segment.turn = Some(altitude);
                }
                Some(turn) if turn - altitude >= CLIMB_THRESHOLD => {
                    totals.descent += f64::from(turn - altitude);
                    segment.turn = Some(altitude);
                }
                Some(_) => {}
            }
        }
        if let Some(time) = point.time {
            totals.first = Some(totals.first.map_or(time, |f| f.min(time)));
            totals.last = Some(totals.last.map_or(time, |l| l.max(time)));
        }
        if let Some(last) = segment.last {
            let step = last.at.distance(point.at);
            totals.distance += step;
            if let (Some(from), Some(to)) = (last.time, point.time)
                && let Ok(taken) = to.duration_since(from)
                && taken <= LONGEST_STEP
                && !taken.is_zero()
                && step / taken.as_secs_f64() >= MOVING_SPEED
            {
                totals.moving += taken;
            }
        }
        segment.last = Some(point);
        segment
    }

    fn store(&mut self, point: Point) {
        if self.full {
            return;
        }
        if let Some(segment) = self
            .doc
            .lines
            .last_mut()
            .and_then(|line| line.segments.last_mut())
        {
            segment.push(point);
            self.stored += 1;
        }
        if self.stored > MAX_POINTS {
            self.thin();
        }
    }

    /// Let every other point go, the ends of each segment kept, and read
    /// on at twice the stride.
    fn thin(&mut self) {
        let mut stored = 0;
        for segment in self.doc.lines.iter_mut().flat_map(|l| &mut l.segments) {
            let last = segment.len().saturating_sub(1);
            let mut index = 0;
            segment.retain(|_| {
                let keep = index % 2 == 0 || index == last;
                index += 1;
                keep
            });
            stored += segment.len();
        }
        self.stored = stored;
        self.stride = self.stride.saturating_mul(2);
        // Ends kept are most of what is stored: thinning again would free
        // nothing, and each point would walk every segment again.
        if self.stored > MAX_POINTS * 3 / 4 {
            self.full = true;
        }
    }

    /// Close the segment being read: its last point is kept even when the
    /// stride would skip it, so a line ends where it ended.
    pub fn end_segment(&mut self) {
        if let Some(point) = self.segment.pending.take() {
            self.store(point);
        }
        self.segment = Segment::default();
        self.open = false;
    }

    /// Name the current line: formats write the name after its start.
    pub fn name_line(&mut self, name: &str) {
        if let Some(line) = self.doc.lines.last_mut() {
            line.name = label(name);
        }
    }

    /// What the current line's altitudes are measured from, when a format
    /// says so after its points.
    pub fn line_datum(&mut self, datum: Datum) {
        if let Some(line) = self.doc.lines.last_mut() {
            line.datum = datum;
        }
    }

    /// Lines so far, for a stop to name the line it belongs to.
    pub fn current_line(&self) -> Option<usize> {
        self.doc.lines.len().checked_sub(1)
    }

    pub fn waypoint(&mut self, mut waypoint: Waypoint) {
        if self.doc.waypoints.len() >= MAX_WAYPOINTS {
            self.dropped_waypoints += 1;
            return;
        }
        waypoint.name = label(&waypoint.name);
        self.bounds.add(waypoint.at);
        self.doc.waypoints.push(waypoint);
    }

    pub fn area(&mut self, mut area: Area) {
        area.name = label(&area.name);
        let points: usize = area.rings.iter().map(Vec::len).sum();
        if points == 0 {
            return;
        }
        if self.doc.areas.len() >= MAX_AREAS || self.area_points + points > MAX_AREA_POINTS {
            self.dropped_areas += 1;
            return;
        }
        self.area_points += points;
        for at in area.rings.iter().flatten() {
            self.bounds.add(*at);
        }
        self.doc.areas.push(area);
    }

    /// A position a parser could not read.
    pub fn invalid(&mut self) {
        self.invalid += 1;
    }

    /// Whether the kept points were thinned, and to one in how many.
    pub fn stride(&self) -> usize {
        self.stride
    }

    pub fn finish(mut self) -> (GeoDoc, Self) {
        self.end_segment();
        let mut doc = std::mem::take(&mut self.doc);
        for line in &mut doc.lines {
            line.segments.retain(|segment| !segment.is_empty());
        }
        doc.lines.retain(|line| !line.segments.is_empty());
        // Lines are counted once they are known to hold points: a format
        // may start one that ends up empty.
        self.tracks.lines = doc
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Track)
            .count();
        self.routes.lines = doc
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Route)
            .count();
        doc.bounds = self.bounds.finish();
        (doc, self)
    }
}

/// Names are labels on a map: a paragraph pasted into one is cut.
pub const LABEL_CHARS: usize = 120;

pub fn label(text: &str) -> String {
    let text = text.trim();
    let mut label: String = text.chars().take(LABEL_CHARS).collect();
    if label.len() < text.len() {
        label.push('…');
    }
    label
}

/// A position from text, as formats write numbers: `45.1`, ` -6.25 `.
pub fn position(lat: &str, lon: &str) -> Option<LatLon> {
    LatLon::new(lat.trim().parse().ok()?, lon.trim().parse().ok()?)
}

/// An altitude in metres, if it is a number a place can have.
pub fn altitude(text: &str) -> Option<f32> {
    text.trim()
        .parse::<f32>()
        .ok()
        .filter(|metres| metres.is_finite() && metres.abs() < 1.0e6)
}

/// An instant as GPX and KML write them (RFC 3339: `2026-03-12T14:02:00Z`,
/// with or without fractions or an offset).
pub fn time(text: &str) -> Option<SystemTime> {
    text.trim()
        .parse::<jiff::Timestamp>()
        .ok()
        .map(SystemTime::from)
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn point(lat: f64, lon: f64, altitude: Option<f32>, seconds: Option<u64>) -> Point {
        Point {
            at: LatLon::new(lat, lon).unwrap(),
            altitude,
            time: seconds.map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s)),
        }
    }

    /// A map of short lines (road segments, a line each) is kept within
    /// bounds and read in linear time: each segment keeps its ends, so
    /// thinning alone could never make room, and each point walked every
    /// segment again.
    #[test]
    fn many_short_lines_stay_bounded() {
        let mut c = Collector::new();
        let lines = MAX_SEGMENTS * 3;
        for i in 0..lines {
            c.line(String::new(), LineKind::Route, Datum::Sea);
            let at = (i % 1000) as f64 * 0.0001;
            c.point(point(45. + at, 6., None, None));
            c.point(point(45. + at, 6.001, None, None));
            c.point(point(45. + at, 6.002, None, None));
        }
        let (doc, c) = c.finish();
        let kept: usize = doc
            .lines
            .iter()
            .flat_map(|l| &l.segments)
            .map(Vec::len)
            .sum();
        assert!(kept <= MAX_POINTS, "{kept}");
        assert_eq!(c.routes.points, lines * 3, "every point still counted");
    }

    #[test]
    fn climb_ignores_wobbles_and_counts_real_rises() {
        let mut c = Collector::new();
        c.line("Walk".into(), LineKind::Track, Datum::Sea);
        // Up 10 m through wobbles of 1 m, then down 5 m.
        for (i, altitude) in [100., 101., 100., 102., 105., 110., 109., 105.]
            .into_iter()
            .enumerate()
        {
            c.point(point(45., 6. + i as f64 * 0.001, Some(altitude), None));
        }
        let (_, c) = c.finish();
        assert_eq!(c.tracks.climb, 10.);
        assert_eq!(c.tracks.descent, 5.);
        assert_eq!(c.tracks.highest, Some(110.));
        assert_eq!(c.tracks.lowest, Some(100.));
    }

    #[test]
    fn moving_time_leaves_out_standing_still_and_pauses() {
        let mut c = Collector::new();
        c.segment();
        // 111 m in 60 s (moving), 0 m in 60 s (still), 111 m in 10 min (a
        // pause, too long a step).
        c.point(point(0., 0., None, Some(0)));
        c.point(point(0., 0.001, None, Some(60)));
        c.point(point(0., 0.001, None, Some(120)));
        c.point(point(0., 0.002, None, Some(720)));
        let (_, c) = c.finish();
        assert_eq!(c.tracks.moving, Duration::from_secs(60));
        assert!(
            (c.tracks.distance - 222.4).abs() < 0.5,
            "{}",
            c.tracks.distance
        );
        assert_eq!(
            c.tracks
                .last
                .unwrap()
                .duration_since(c.tracks.first.unwrap())
                .ok(),
            Some(Duration::from_mins(12))
        );
    }

    #[test]
    fn segments_are_not_joined() {
        let mut c = Collector::new();
        c.segment();
        c.point(point(0., 0., None, None));
        c.point(point(0., 0.001, None, None));
        c.segment();
        c.point(point(0., 1., None, None));
        c.point(point(0., 1.001, None, None));
        let (doc, c) = c.finish();
        assert_eq!(doc.lines[0].segments.len(), 2);
        // Two steps of 111 m, not the degree between the segments.
        assert!(c.tracks.distance < 250., "{}", c.tracks.distance);
    }

    #[test]
    fn a_long_track_is_thinned_evenly_and_keeps_its_ends() {
        let mut c = Collector::new();
        c.segment();
        let n = MAX_POINTS * 3 + 7;
        for i in 0..n {
            c.point(point(0., i as f64 * 1e-5, None, None));
        }
        let (doc, c) = c.finish();
        let kept = &doc.lines[0].segments[0];
        assert!(kept.len() <= MAX_POINTS + 1, "{}", kept.len());
        assert!(kept.len() > MAX_POINTS / 4, "{}", kept.len());
        assert_eq!(kept.first().unwrap().at.lon, 0.);
        assert_eq!(kept.last().unwrap().at.lon, (n - 1) as f64 * 1e-5);
        assert_eq!(c.tracks.points, n);
        assert!(c.stride() >= 4);
        // Every point counted, not only those kept.
        let expected = LatLon::new(0., 0.)
            .unwrap()
            .distance(LatLon::new(0., (n - 1) as f64 * 1e-5).unwrap());
        assert!(
            (c.tracks.distance - expected).abs() < 1.,
            "{}",
            c.tracks.distance
        );
    }

    #[test]
    fn times_are_read_as_rfc_3339() {
        let t = time("2026-03-12T14:02:00Z").unwrap();
        assert_eq!(
            t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
            1_773_324_120
        );
        let offset = time("2026-03-12T16:02:00.500+02:00").unwrap();
        assert_eq!(
            offset.duration_since(t).unwrap(),
            Duration::from_millis(500)
        );
        assert!(time("yesterday").is_none());
    }
}
