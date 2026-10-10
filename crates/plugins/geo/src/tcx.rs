//! Garmin's Training Center XML (TCX): activities, as a watch or a bike
//! computer recorded them, and courses, planned to be followed.
//!
//! An activity is laps of tracks of trackpoints; it becomes one track line,
//! a segment per `Track` (a lap, or a part of one after a pause). A course
//! is a route line with its course points (a turn, a summit, water) as its
//! stops. A trackpoint may have no `Position`: a heart-rate sample taken
//! indoors, or while the signal was lost. It is no place, and no error.
use std::io::BufRead;

use skwir_core::CancelToken;
use skwir_core::geo::{Datum, LineKind, Point, Waypoint, WaypointKind};

use crate::collect::{self, Collector};
use crate::xml::{self, Damaged, Tag};

/// A trackpoint or a course point being read, from its children.
#[derive(Default)]
struct Open {
    /// Whether it has a `Position` at all.
    placed: bool,
    lat: String,
    lon: String,
    altitude: Option<f32>,
    time: Option<std::time::SystemTime>,
    name: String,
}

impl Open {
    /// Its point, if its position can be read.
    fn point(&self) -> Option<Point> {
        collect::position(&self.lat, &self.lon).map(|at| Point {
            at,
            altitude: self.altitude,
            time: self.time,
        })
    }
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut path: Vec<String> = Vec::new();
    let mut open: Option<Open> = None;
    xml::walk(input, cancel, |tag| {
        match tag {
            Tag::Start { name, attributes } => {
                match name {
                    "Activity" => {
                        // An activity has no name of its own; its sport
                        // says what it was.
                        let sport = attributes
                            .iter()
                            .find(|(k, _)| k == "Sport")
                            .map(|(_, v)| v.trim())
                            .filter(|sport| !sport.is_empty() && *sport != "Other")
                            .unwrap_or_default();
                        into.line(sport.to_owned(), LineKind::Track, Datum::Sea);
                    }
                    "Course" => {
                        into.line(String::new(), LineKind::Route, Datum::Sea);
                    }
                    "Track" => into.segment(),
                    "Trackpoint" | "CoursePoint" => open = Some(Open::default()),
                    "Position" => {
                        if let Some(open) = open.as_mut() {
                            open.placed = true;
                        }
                    }
                    _ => {}
                }
                path.push(name.to_owned());
            }
            Tag::End { name, text } => {
                path.pop();
                let parent = path.last().map_or("", String::as_str);
                match (name, parent) {
                    ("LatitudeDegrees", "Position") => {
                        if let Some(open) = open.as_mut() {
                            text.clone_into(&mut open.lat);
                        }
                    }
                    ("LongitudeDegrees", "Position") => {
                        if let Some(open) = open.as_mut() {
                            text.clone_into(&mut open.lon);
                        }
                    }
                    ("AltitudeMeters", "Trackpoint" | "CoursePoint") => {
                        if let Some(open) = open.as_mut() {
                            open.altitude = collect::altitude(text);
                        }
                    }
                    ("Time", "Trackpoint" | "CoursePoint") => {
                        if let Some(open) = open.as_mut() {
                            open.time = collect::time(text);
                        }
                    }
                    ("Name", "CoursePoint") => {
                        if let Some(open) = open.as_mut() {
                            text.trim().clone_into(&mut open.name);
                        }
                    }
                    ("Name", "Course") => into.name_line(text),
                    // The device that recorded an activity, else the
                    // program that wrote the file.
                    ("Name", "Creator" | "Author") if into.creator.is_none() => {
                        into.creator = Some(text.trim().to_owned()).filter(|n| !n.is_empty());
                    }
                    ("Trackpoint", _) => {
                        if let Some(open) = open.take().filter(|o| o.placed) {
                            match open.point() {
                                Some(point) => into.point(point),
                                None => into.invalid(),
                            }
                        }
                    }
                    ("CoursePoint", _) => {
                        let Some(open) = open.take().filter(|o| o.placed) else {
                            return Ok(());
                        };
                        match open.point() {
                            Some(point) => into.waypoint(Waypoint {
                                name: open.name,
                                at: point.at,
                                kind: WaypointKind::Stop,
                                altitude: point.altitude.map(|a| (a, Datum::Sea)),
                                line: into.current_line(),
                            }),
                            None => into.invalid(),
                        }
                    }
                    ("Track" | "Activity" | "Course", _) => into.end_segment(),
                    _ => {}
                }
            }
        }
        Ok(())
    })
}

/// Whether the first bytes of a file look like TCX: its root element,
/// after the XML declaration and perhaps a comment.
pub fn sniff(head: &[u8]) -> bool {
    head.windows(b"<TrainingCenterDatabase".len())
        .any(|window| window == b"<TrainingCenterDatabase")
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
pub(crate) mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    pub(crate) const RIDE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<TrainingCenterDatabase xmlns="http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2">
  <Activities>
    <Activity Sport="Biking">
      <Id>2026-03-12T08:00:00Z</Id>
      <Lap StartTime="2026-03-12T08:00:00Z">
        <TotalTimeSeconds>120</TotalTimeSeconds>
        <Track>
          <Trackpoint>
            <Time>2026-03-12T08:00:00Z</Time>
            <Position><LatitudeDegrees>10.0000</LatitudeDegrees><LongitudeDegrees>20.0000</LongitudeDegrees></Position>
            <AltitudeMeters>200.0</AltitudeMeters>
            <HeartRateBpm><Value>110</Value></HeartRateBpm>
          </Trackpoint>
          <Trackpoint>
            <Time>2026-03-12T08:00:30Z</Time>
            <HeartRateBpm><Value>112</Value></HeartRateBpm>
          </Trackpoint>
          <Trackpoint>
            <Time>2026-03-12T08:01:00Z</Time>
            <Position><LatitudeDegrees>10.0010</LatitudeDegrees><LongitudeDegrees>20.0000</LongitudeDegrees></Position>
            <AltitudeMeters>210.0</AltitudeMeters>
          </Trackpoint>
        </Track>
      </Lap>
      <Lap StartTime="2026-03-12T08:05:00Z">
        <Track>
          <Trackpoint>
            <Time>2026-03-12T08:05:00Z</Time>
            <Position><LatitudeDegrees>10.0020</LatitudeDegrees><LongitudeDegrees>20.0000</LongitudeDegrees></Position>
          </Trackpoint>
          <Trackpoint>
            <Time>2026-03-12T08:06:00Z</Time>
            <Position><LatitudeDegrees>north</LatitudeDegrees><LongitudeDegrees>20.0</LongitudeDegrees></Position>
          </Trackpoint>
        </Track>
      </Lap>
      <Creator xsi:type="Device_t" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
        <Name>Example Watch</Name>
      </Creator>
    </Activity>
  </Activities>
  <Author xsi:type="Application_t" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
    <Name>Example Exporter</Name>
  </Author>
</TrainingCenterDatabase>"#;

    const COURSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<TrainingCenterDatabase xmlns="http://www.garmin.com/xmlschemas/TrainingCenterDatabase/v2">
  <Courses>
    <Course>
      <Name>Loop &amp; back</Name>
      <Lap><TotalTimeSeconds>600</TotalTimeSeconds></Lap>
      <Track>
        <Trackpoint><Position><LatitudeDegrees>10.0</LatitudeDegrees><LongitudeDegrees>20.0</LongitudeDegrees></Position><AltitudeMeters>100</AltitudeMeters></Trackpoint>
        <Trackpoint><Position><LatitudeDegrees>10.0</LatitudeDegrees><LongitudeDegrees>20.001</LongitudeDegrees></Position><AltitudeMeters>104</AltitudeMeters></Trackpoint>
        <Trackpoint><Position><LatitudeDegrees>10.0</LatitudeDegrees><LongitudeDegrees>20.002</LongitudeDegrees></Position><AltitudeMeters>100</AltitudeMeters></Trackpoint>
      </Track>
      <CoursePoint>
        <Name>Spring</Name>
        <Time>2026-03-12T09:00:00Z</Time>
        <Position><LatitudeDegrees>10.0</LatitudeDegrees><LongitudeDegrees>20.001</LongitudeDegrees></Position>
        <AltitudeMeters>104</AltitudeMeters>
        <PointType>Water</PointType>
      </CoursePoint>
    </Course>
  </Courses>
  <Author><Name>Example Planner</Name></Author>
</TrainingCenterDatabase>"#;

    #[test]
    #[allow(
        clippy::duration_suboptimal_units,
        reason = "a time since 1970 is written as the seconds or days it is"
    )]
    fn an_activity_is_a_track_with_a_segment_per_track() {
        let mut c = Collector::new();
        assert!(
            read(RIDE.as_bytes(), &mut c, &CancelToken::new())
                .unwrap()
                .is_none()
        );
        let (doc, c) = c.finish();
        assert_eq!(c.creator.as_deref(), Some("Example Watch"));
        assert_eq!(doc.lines.len(), 1);
        let line = &doc.lines[0];
        assert_eq!(line.name, "Biking");
        assert_eq!(line.kind, LineKind::Track);
        assert_eq!(line.segments.len(), 2);
        // The heart-rate sample has no place; the unreadable one is invalid.
        assert_eq!(line.segments[0].len(), 2);
        assert_eq!(line.segments[1].len(), 1);
        assert_eq!(c.invalid, 1);
        assert_eq!(c.tracks.points, 3);
        let first = line.segments[0][0];
        assert_eq!((first.at.lat, first.at.lon), (10., 20.));
        assert_eq!(first.altitude, Some(200.));
        assert_eq!(
            first.time,
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_773_302_400))
        );
        assert_eq!(c.tracks.climb, 10.);
        assert_eq!(
            c.tracks
                .last
                .unwrap()
                .duration_since(c.tracks.first.unwrap())
                .ok(),
            Some(Duration::from_secs(300))
        );
    }

    #[test]
    fn a_course_is_a_route_with_its_points_as_stops() {
        let mut c = Collector::new();
        assert!(
            read(COURSE.as_bytes(), &mut c, &CancelToken::new())
                .unwrap()
                .is_none()
        );
        let (doc, c) = c.finish();
        assert_eq!(c.creator.as_deref(), Some("Example Planner"));
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0].name, "Loop & back");
        assert_eq!(doc.lines[0].kind, LineKind::Route);
        assert_eq!(doc.lines[0].segments[0].len(), 3);
        assert_eq!(c.routes.points, 3);
        assert_eq!(c.routes.climb, 4.);
        assert_eq!(c.routes.descent, 4.);
        assert_eq!(doc.waypoints.len(), 1);
        let spring = &doc.waypoints[0];
        assert_eq!(spring.name, "Spring");
        assert_eq!(spring.kind, WaypointKind::Stop);
        assert_eq!(spring.line, Some(0));
        assert_eq!(spring.altitude, Some((104., Datum::Sea)));
        assert_eq!((spring.at.lat, spring.at.lon), (10., 20.001));
    }

    #[test]
    fn a_file_cut_short_keeps_what_was_read() {
        let cut = &RIDE[..RIDE.find("<Lap StartTime=\"2026-03-12T08:05").unwrap()];
        let cut = format!("{cut}</Activity></Lap>");
        let mut c = Collector::new();
        let damaged = read(cut.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_some());
        let (doc, _) = c.finish();
        assert_eq!(doc.lines[0].segments[0].len(), 2);
    }

    #[test]
    fn tcx_is_recognised_by_its_root() {
        assert!(sniff(RIDE.as_bytes()));
        assert!(!sniff(b"<?xml version=\"1.0\"?><gpx></gpx>"));
    }
}
