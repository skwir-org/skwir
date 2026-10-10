//! GPX 1.0 and 1.1: tracks (`trk`, their segments `trkseg`), routes
//! (`rte`) and waypoints (`wpt`), with elevations and times.
use std::io::BufRead;

use skwir_core::CancelToken;
use skwir_core::geo::{Datum, LineKind, Point, Waypoint, WaypointKind};

use crate::collect::{self, Collector};
use crate::xml::{self, Damaged, Tag};

/// A point being read: its position from the attributes, the rest from
/// its children.
struct Open {
    point: Option<Point>,
    name: String,
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    // Where the walk is: the path of element names from the root.
    let mut path: Vec<String> = Vec::new();
    let mut open: Option<Open> = None;
    xml::walk(input, cancel, |tag| {
        match tag {
            Tag::Start { name, attributes } => {
                let attribute = |key: &str| {
                    attributes
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, v)| v.as_str())
                };
                match name {
                    "gpx" if path.is_empty() => {
                        into.creator = attribute("creator").map(str::to_owned);
                    }
                    "trk" => {
                        into.line(String::new(), LineKind::Track, Datum::Sea);
                    }
                    "rte" => {
                        into.line(String::new(), LineKind::Route, Datum::Sea);
                        into.segment();
                    }
                    "trkseg" => into.segment(),
                    "trkpt" | "rtept" | "wpt" => {
                        let at = collect::position(
                            attribute("lat").unwrap_or(""),
                            attribute("lon").unwrap_or(""),
                        );
                        if at.is_none() {
                            into.invalid();
                        }
                        open = Some(Open {
                            point: at.map(|at| Point {
                                at,
                                altitude: None,
                                time: None,
                            }),
                            name: String::new(),
                        });
                    }
                    _ => {}
                }
                path.push(name.to_owned());
            }
            Tag::End { name, text } => {
                path.pop();
                let parent = path.last().map_or("", String::as_str);
                match (name, parent) {
                    ("ele", "trkpt" | "rtept" | "wpt") => {
                        if let Some(point) = open.as_mut().and_then(|o| o.point.as_mut()) {
                            point.altitude = collect::altitude(text);
                        }
                    }
                    ("time", "trkpt" | "rtept" | "wpt") => {
                        if let Some(point) = open.as_mut().and_then(|o| o.point.as_mut()) {
                            point.time = collect::time(text);
                        }
                    }
                    ("name", "trkpt" | "rtept" | "wpt") => {
                        if let Some(open) = open.as_mut() {
                            text.trim().clone_into(&mut open.name);
                        }
                    }
                    ("name", "trk" | "rte") => into.name_line(text.trim()),
                    ("name", "metadata" | "gpx") => {
                        into.name = Some(text.trim().to_owned()).filter(|n| !n.is_empty());
                    }
                    ("desc", "metadata" | "gpx") => {
                        into.description = Some(text.trim().to_owned()).filter(|d| !d.is_empty());
                    }
                    ("trkpt", _) => {
                        if let Some(point) = open.take().and_then(|o| o.point) {
                            into.point(point);
                        }
                    }
                    ("rtept", _) => {
                        if let Some(Open {
                            point: Some(point),
                            name,
                        }) = open.take()
                        {
                            into.point(point);
                            // A named stop is labelled on the map; the
                            // hundreds of unnamed turns of a route are not.
                            if !name.is_empty() {
                                into.waypoint(Waypoint {
                                    name,
                                    at: point.at,
                                    kind: WaypointKind::Stop,
                                    altitude: point.altitude.map(|a| (a, Datum::Sea)),
                                    line: into.current_line(),
                                });
                            }
                        }
                    }
                    ("wpt", _) => {
                        if let Some(Open {
                            point: Some(point),
                            name,
                        }) = open.take()
                        {
                            into.waypoint(Waypoint {
                                name,
                                at: point.at,
                                kind: WaypointKind::Point,
                                altitude: point.altitude.map(|a| (a, Datum::Sea)),
                                line: None,
                            });
                        }
                    }
                    ("trkseg" | "rte" | "trk", _) => into.end_segment(),
                    _ => {}
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
pub(crate) mod tests {
    use super::*;
    use std::time::Duration;

    pub(crate) const HIKE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="Example Logger" xmlns="http://www.topografix.com/GPX/1/1">
  <metadata><name>Café &amp; ridge</name></metadata>
  <wpt lat="45.0010" lon="6.0010"><ele>1510</ele><name>Hut</name></wpt>
  <wpt lat="bad" lon="6.0"/>
  <trk>
    <name>Morning</name>
    <trkseg>
      <trkpt lat="45.0000" lon="6.0000"><ele>1500</ele><time>2026-03-12T08:00:00Z</time></trkpt>
      <trkpt lat="45.0010" lon="6.0000"><ele>1510</ele><time>2026-03-12T08:01:00Z</time></trkpt>
      <trkpt lat="45.0020" lon="6.0000"><ele>1520</ele><time>2026-03-12T08:02:00Z</time></trkpt>
    </trkseg>
  </trk>
  <rte>
    <name>Way back</name>
    <rtept lat="45.0020" lon="6.0000"><name>Top</name></rtept>
    <rtept lat="45.0000" lon="6.0010"/>
  </rte>
</gpx>"#;

    #[test]
    fn a_gpx_file_gives_its_tracks_routes_and_waypoints() {
        let mut c = Collector::new();
        let damaged = read(HIKE.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_none());
        let (doc, c) = c.finish();
        assert_eq!(c.name.as_deref(), Some("Café & ridge"));
        assert_eq!(c.creator.as_deref(), Some("Example Logger"));
        assert_eq!(c.invalid, 1);
        assert_eq!(doc.lines.len(), 2);
        assert_eq!(doc.lines[0].name, "Morning");
        assert_eq!(doc.lines[0].kind, LineKind::Track);
        assert_eq!(doc.lines[0].segments[0].len(), 3);
        assert_eq!(doc.lines[1].name, "Way back");
        assert_eq!(doc.lines[1].kind, LineKind::Route);
        let names: Vec<_> = doc.waypoints.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["Hut", "Top"]);
        assert_eq!(doc.waypoints[1].kind, WaypointKind::Stop);
        assert_eq!(doc.waypoints[1].line, Some(1));
        assert_eq!(c.tracks.climb, 20.);
        assert_eq!(
            c.tracks
                .last
                .unwrap()
                .duration_since(c.tracks.first.unwrap())
                .ok(),
            Some(Duration::from_secs(120))
        );
        assert!(
            (c.tracks.distance - 222.4).abs() < 0.5,
            "{}",
            c.tracks.distance
        );
        let bounds = doc.bounds.unwrap();
        assert_eq!((bounds.south, bounds.north), (45.0, 45.002));
    }

    #[test]
    fn a_file_cut_short_keeps_what_was_read() {
        let cut = &HIKE[..HIKE.find("<rte>").unwrap() + 20];
        let cut = cut.replace("</trkseg>", "</trksg>");
        let mut c = Collector::new();
        let damaged = read(cut.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_some());
        let (doc, _) = c.finish();
        assert_eq!(doc.lines[0].segments[0].len(), 3);
    }

    #[test]
    fn a_declared_entity_is_not_expanded() {
        let doc = r#"<?xml version="1.0"?>
<!DOCTYPE gpx [<!ENTITY boom "BOOM">]>
<gpx><wpt lat="1" lon="2"><name>&boom;</name></wpt></gpx>"#;
        let mut c = Collector::new();
        read(doc.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, _) = c.finish();
        assert_eq!(doc.waypoints[0].name, "");
    }
}
