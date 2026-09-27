//! bibli plugin: geo. GPS tracks, routes and waypoints (GPX, KML, KMZ,
//! `GeoJSON`) previewed on a map, with what a hiker or a cyclist asks of
//! them: distance, time, climb (ADR-020). Files are read, not obeyed:
//! nothing they link to is fetched.
//!
//! The formats stream: a GPX of a whole day is read without holding its
//! tree, and a track longer than the map can use is thinned evenly
//! (`collect::MAX_POINTS`) while its facts count every point.
//!
//! It also gives the Map view its places (`view`): the pictures and tracks
//! of the location a pane shows, found on workers.
mod collect;
mod geojson;
mod gpx;
mod kml;
mod view;
mod xml;

use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bibli_core::command::Outcome;
use bibli_core::registry::{KeyBinding, ViewSpec};
use bibli_core::workspace::PaneId;
use bibli_core::{
    App, CancelToken,
    geo::GeoDoc,
    inspection::{Detail, InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::inspection::{self, limits};
use collect::{Collector, Totals};

const ID: &str = "geo";
/// What is read of a file, streamed: more than a year of GPS logging.
const MAX_INPUT: u64 = 256 * 1024 * 1024;

/// The Map view's id: its descriptor, and the renderer the UI draws it with.
pub const VIEW: &str = "map";

#[derive(Default)]
pub struct GeoPlugin {
    revision: Arc<std::sync::atomic::AtomicU64>,
    scanner: parking_lot::Mutex<Option<Arc<view::Scanner>>>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PaneArgs {
    /// The pane; the active one when left out.
    pane: Option<PaneId>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PointsArgs {
    pane: Option<PaneId>,
    /// Points already had: only those after them are returned.
    #[serde(default)]
    since: usize,
}

/// What `geo.search_subfolders` looks for: pictures and tracks.
const PLACEABLE: &str = "(kind:image OR ext:gpx,kml,kmz,geojson)";

impl Plugin for GeoPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Maps"
    }
    fn description(&self) -> &'static str {
        "Show GPS tracks, routes and waypoints (GPX, KML, KMZ, GeoJSON) on a map, with their distance, time and climb. Nothing a file links to is fetched"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn register(&self, app: &App) {
        app.previews.register(ID, Arc::new(Provider));
        let mut spec = ViewSpec::named(VIEW, "Map", "map", Arc::clone(&self.revision)).lens();
        // After Usage, the other lens.
        spec.order = 110;
        app.registry.views.add(ID, spec);
        app.registry.keymap.add(
            ID,
            KeyBinding {
                // After Usage's Ctrl+4.
                keys: "ctrl-5".into(),
                command: bibli_core::command::Invocation::with_args(
                    "ui.view",
                    &serde_json::json!({ "id": VIEW }),
                ),
                when: None,
            },
        );
        let scanner = Arc::new(view::Scanner::new(app, Arc::clone(&self.revision)));
        if let Some(old) = self.scanner.lock().replace(Arc::clone(&scanner)) {
            old.stop();
        }
        let scan = Arc::clone(&scanner);
        app.commands.register::<PaneArgs, _>(
            ID,
            "geo.scan",
            "Find where the files are",
            "Find the pictures and tracks of what a pane lists that say where they are, for the Map view",
            move |args, app| {
                scan.scan(app, app.pane_or_active(args.pane))?;
                Ok(Outcome::Done)
            },
        );
        let points = Arc::clone(&scanner);
        app.commands.register::<PointsArgs, _>(
            ID,
            "geo.points",
            "Where the files are",
            "The pictures and tracks of what a pane lists that were placed on the map, after the first `since`",
            move |args, app| {
                let found = points.points(app.pane_or_active(args.pane), args.since);
                Ok(Outcome::Value(serde_json::to_value(found)?))
            },
        );
        let release = Arc::clone(&scanner);
        app.commands.register::<PaneArgs, _>(
            ID,
            "geo.release",
            "Forget where the files are",
            "Stop finding where a pane's files are: its Map view closed",
            move |args, app| {
                release.release(app.pane_or_active(args.pane));
                Ok(Outcome::Done)
            },
        );
        app.commands.register::<PaneArgs, _>(
            ID,
            "geo.search_subfolders",
            "Search subfolders for places",
            "Show the pictures and tracks of the folder and all its subfolders",
            |args, app| {
                app.dispatch(&bibli_core::command::Invocation::with_args(
                    "query.search",
                    &serde_json::json!({
                        "expr": PLACEABLE,
                        "pane": app.pane_or_active(args.pane),
                    }),
                ))
            },
        );
    }
    fn on_disable(&self, _app: &App) {
        if let Some(scanner) = self.scanner.lock().take() {
            scanner.stop();
        }
    }
}

/// A format this plugin reads, told by the file's extension.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Format {
    Gpx,
    Kml,
    Kmz,
    GeoJson,
}

impl Format {
    pub(crate) fn of(path: &Path) -> Option<Self> {
        Some(match inspection::extension(path).as_str() {
            "gpx" => Format::Gpx,
            "kml" => Format::Kml,
            "kmz" => Format::Kmz,
            "geojson" => Format::GeoJson,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Format::Gpx => "GPX",
            Format::Kml => "KML",
            Format::Kmz => "KMZ",
            Format::GeoJson => "GeoJSON",
        }
    }
}

/// Read `path` into `into`; a note when the file was damaged or cut.
fn read(
    path: &Path,
    format: Format,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<String>> {
    let mut notes = Vec::new();
    let file = inspection::open(path)?;
    if file.metadata().is_ok_and(|m| m.len() > MAX_INPUT) {
        notes.push(format!(
            "Only the first {} were read",
            inspection::size(MAX_INPUT)
        ));
    }
    let damaged = match format {
        Format::Gpx => gpx::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Kml => kml::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Kmz => {
            let mut archive = zip::ZipArchive::new(file)?;
            let index = kmz_document(&archive)
                .ok_or_else(|| anyhow::anyhow!("This KMZ holds no KML document"))?;
            let member = archive.by_index(index)?;
            kml::read(BufReader::new(member.take(MAX_INPUT)), into, cancel)?
        }
        Format::GeoJson => {
            let bytes =
                inspection::read_bounded(path, limits::BINARY_INPUT, "This GeoJSON file", cancel)?;
            let document: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|error| anyhow::anyhow!("Not valid GeoJSON: {error}"))?;
            inspection::check(cancel)?;
            geojson::read(&document, into);
            None
        }
    };
    if let Some(xml::Damaged(why)) = damaged {
        notes.push(format!(
            "The file is damaged ({why}); what came before is shown"
        ));
    }
    Ok(notes)
}

/// The KML a KMZ holds: `doc.kml` as Google Earth writes it, else the
/// first KML nearest the top.
fn kmz_document<R: Read + std::io::Seek>(archive: &zip::ZipArchive<R>) -> Option<usize> {
    let names: Vec<(usize, &str)> = (0..archive.len())
        .filter_map(|i| archive.name_for_index(i).map(|name| (i, name)))
        .collect();
    names
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case("doc.kml"))
        .or_else(|| {
            names
                .iter()
                .filter(|(_, name)| name.to_ascii_lowercase().ends_with(".kml"))
                .min_by_key(|(_, name)| name.matches('/').count())
        })
        .map(|(i, _)| *i)
}

pub fn distance(metres: f64) -> String {
    if metres < 1000. {
        format!("{metres:.0} m")
    } else if metres < 100_000. {
        format!("{:.1} km", metres / 1000.)
    } else {
        format!("{:.0} km", metres / 1000.)
    }
}

pub fn duration(taken: Duration) -> String {
    let total = taken.as_secs();
    format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
}

/// An instant in the user's time zone (ADR-017), to the minute; `day` is
/// left out when it is the one given.
fn local(time: SystemTime, zone: &jiff::tz::TimeZone, day: Option<jiff::civil::Date>) -> String {
    let Ok(stamp) = jiff::Timestamp::try_from(time) else {
        return String::new();
    };
    let zoned = stamp.to_zoned(zone.clone());
    if day == Some(zoned.date()) {
        zoned.strftime("%H:%M").to_string()
    } else {
        zoned.strftime("%Y-%m-%d %H:%M").to_string()
    }
}

fn plural(n: usize, one: &str, many: &str) -> Option<String> {
    match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    }
}

/// The facts of the lines of one kind: those of the tracks when there are
/// any, else the routes' (a route has no times).
fn line_facts(totals: &Totals, zone: &jiff::tz::TimeZone) -> Vec<(&'static str, Option<String>)> {
    let span = totals
        .first
        .zip(totals.last)
        .and_then(|(first, last)| last.duration_since(first).ok());
    let start_day = totals.first.and_then(|first| {
        jiff::Timestamp::try_from(first)
            .ok()
            .map(|stamp| stamp.to_zoned(zone.clone()).date())
    });
    let moving = (!totals.moving.is_zero()).then_some(totals.moving);
    let metres = |value: f64| format!("{value:.0} m");
    vec![
        (
            "Distance",
            (totals.distance > 0.).then(|| distance(totals.distance)),
        ),
        ("Duration", span.filter(|s| !s.is_zero()).map(duration)),
        ("Moving time", moving.map(duration)),
        (
            "Moving speed",
            moving.map(|m| format!("{:.1} km/h", totals.distance / m.as_secs_f64() * 3.6)),
        ),
        ("Climb", (totals.climb > 0.).then(|| metres(totals.climb))),
        (
            "Descent",
            (totals.descent > 0.).then(|| metres(totals.descent)),
        ),
        (
            "Highest point",
            totals.highest.map(|h| metres(f64::from(h))),
        ),
        ("Lowest point", totals.lowest.map(|l| metres(f64::from(l)))),
        ("Started", totals.first.map(|t| local(t, zone, None))),
        ("Ended", totals.last.map(|t| local(t, zone, start_day))),
    ]
}

fn facts(doc: &GeoDoc, c: &Collector) -> Vec<Detail> {
    let zone = bibli_platform::calendar::Calendar::system()
        .time_zone()
        .clone();
    let main = if c.tracks.lines > 0 {
        &c.tracks
    } else {
        &c.routes
    };
    let mut facts = line_facts(main, &zone);
    if c.tracks.lines > 0 && c.routes.distance > 0. {
        facts.push(("Route distance", Some(distance(c.routes.distance))));
    }
    let start = doc
        .lines
        .first()
        .and_then(|l| l.segments.first())
        .and_then(|s| s.first())
        .map(|p| p.at)
        .or_else(|| doc.waypoints.first().map(|w| w.at));
    facts.extend([
        ("Tracks", inspection::count(c.tracks.lines)),
        ("Routes", inspection::count(c.routes.lines)),
        ("Waypoints", inspection::count(doc.waypoints.len())),
        ("Areas", inspection::count(doc.areas.len())),
        (
            "Points",
            inspection::count(c.tracks.points + c.routes.points),
        ),
        (
            "Start",
            start.map(|at| format!("{:.5}, {:.5}", at.lat, at.lon)),
        ),
        ("Name", c.name.clone()),
        ("Description", c.description.clone()),
        ("Created by", c.creator.clone()),
    ]);
    inspection::details(facts)
}

/// What the file holds, in a few words: `1 track, 12 waypoints`.
fn summary(doc: &GeoDoc, c: &Collector) -> String {
    [
        plural(c.tracks.lines, "track", "tracks"),
        plural(c.routes.lines, "route", "routes"),
        plural(doc.waypoints.len(), "waypoint", "waypoints"),
        plural(doc.areas.len(), "area", "areas"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ")
}

/// Read a file into what the map shows and what Details says.
pub(crate) fn inspect_file(path: &Path, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    let format = Format::of(path).ok_or_else(|| anyhow::anyhow!("Not a map file"))?;
    let mut collector = Collector::new();
    let mut notes = read(path, format, &mut collector, cancel)?;
    let (doc, c) = collector.finish();
    if doc.bounds.is_none() {
        notes.insert(0, "Nothing in this file has a position".into());
    }
    if c.stride() > 1 {
        notes.push(format!("The map shows one point in {}", c.stride()));
    }
    if c.invalid > 0 {
        notes.push(format!(
            "{} left out: not a position",
            plural(c.invalid, "point", "points").unwrap_or_default()
        ));
    }
    let dropped = c.dropped_waypoints + c.dropped_areas;
    if dropped > 0 {
        notes.push(format!("{dropped} more places than the map shows"));
    }
    let summary = summary(&doc, &c);
    Ok(Inspection {
        kind: InspectionKind::Map,
        title: if summary.is_empty() {
            format.name().to_owned()
        } else {
            format!("{} · {summary}", format.name())
        },
        details: facts(&doc, &c),
        notice: notes.join(" · "),
        // A KMZ is compressed: it has no source to read.
        source_available: format != Format::Kmz,
        geo: Some(Arc::new(doc)),
        ..Inspection::default()
    })
}

struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        ID
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        Format::of(&req.entry.path).is_some()
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        // The source is the shared code view (the worker's fallback).
        if view.source {
            return Ok(None);
        }
        let path = req.entry.resolve()?;
        inspect_file(&path, cancel).map(Some)
    }
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fact<'a>(data: &'a Inspection, label: &str) -> Option<&'a str> {
        data.details
            .iter()
            .find(|d| d.label == label)
            .map(|d| d.value.as_str())
    }

    #[test]
    fn a_track_says_how_far_how_long_and_how_high() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("it's a café walk.gpx");
        std::fs::write(&path, gpx::tests::HIKE).unwrap();
        let data = inspect_file(&path, &CancelToken::new()).unwrap();
        assert_eq!(data.kind, InspectionKind::Map);
        assert_eq!(data.title, "GPX · 1 track, 1 route, 2 waypoints");
        assert_eq!(fact(&data, "Distance"), Some("222 m"));
        assert_eq!(fact(&data, "Duration"), Some("0:02:00"));
        assert_eq!(fact(&data, "Climb"), Some("20 m"));
        assert_eq!(fact(&data, "Highest point"), Some("1520 m"));
        assert_eq!(fact(&data, "Name"), Some("Café & ridge"));
        assert_eq!(fact(&data, "Start"), Some("45.00000, 6.00000"));
        assert!(data.source_available);
        assert!(data.notice.contains("1 point left out"), "{}", data.notice);
        let geo = data.geo.as_ref().unwrap();
        assert_eq!(geo.lines.len(), 2);
    }

    #[test]
    fn a_kmz_is_read_through_its_kml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trip.kmz");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("files/icon.png", options).unwrap();
        zip.write_all(b"not a picture").unwrap();
        zip.start_file("doc.kml", options).unwrap();
        zip.write_all(
            b"<kml><Placemark><name>Here</name><Point><coordinates>2.35,48.85</coordinates></Point></Placemark></kml>",
        )
        .unwrap();
        zip.finish().unwrap();
        let data = inspect_file(&path, &CancelToken::new()).unwrap();
        assert_eq!(data.title, "KMZ · 1 waypoint");
        assert!(!data.source_available);
        assert_eq!(data.geo.as_ref().unwrap().waypoints[0].name, "Here");
    }

    #[test]
    fn a_file_without_positions_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.geojson");
        std::fs::write(&path, r#"{"type":"FeatureCollection","features":[]}"#).unwrap();
        let data = inspect_file(&path, &CancelToken::new()).unwrap();
        assert!(
            data.notice
                .starts_with("Nothing in this file has a position")
        );
        assert!(data.geo.as_ref().unwrap().bounds.is_none());
        std::fs::write(&path, "{ not json").unwrap();
        let error = inspect_file(&path, &CancelToken::new()).unwrap_err();
        assert!(
            error.to_string().starts_with("Not valid GeoJSON"),
            "{error}"
        );
    }

    #[test]
    fn distances_and_durations_read_as_people_say_them() {
        assert_eq!(distance(850.4), "850 m");
        assert_eq!(distance(42_195.), "42.2 km");
        assert_eq!(distance(1_234_567.), "1235 km");
        assert_eq!(
            duration(Duration::from_secs(3 * 3600 + 42 * 60 + 10)),
            "3:42:10"
        );
    }
}
