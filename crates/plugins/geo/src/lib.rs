//! skwir plugin: geo. GPS tracks, routes and waypoints (GPX, KML, KMZ,
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
#[cfg(test)]
mod any_input;
mod collect;
mod fit;
mod flight;
mod fms;
mod fpl;
mod geojson;
mod geotag;
mod gpx;
mod igc;
mod kml;
mod litchi;
mod lnmpln;
mod mavlink;
mod mission;
mod nmea;
mod places;
mod pln;
mod position;
mod qgc;
mod tcx;
mod text;
mod thumb;
mod view;
mod wpml;
mod xml;

use std::borrow::Cow;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use collect::{Collector, Plan, PlanKind, Totals};
use skwir_core::command::Outcome;
use skwir_core::command::Topic;
use skwir_core::geo::{Datum, LineKind};
use skwir_core::registry::{KeyBinding, ViewSpec};
use skwir_core::words;
use skwir_core::words::{Moment, Shape, Size, Words};
use skwir_core::workspace::PaneId;
use skwir_core::{
    App, CancelToken,
    geo::GeoDoc,
    inspection::{Detail, InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{FileSource, Held, PreviewProvider, PreviewRequest},
};
use skwir_platform::inspection::{self, limits::Bounds};

const ID: &str = "geo";
/// What is read of a file, streamed: more than a year of GPS logging.
const MAX_INPUT: u64 = 256 * 1024 * 1024;

/// The Map view's id: its descriptor, and the renderer the UI draws it with.
pub const VIEW: &str = "map";

#[derive(Default)]
pub struct GeoPlugin {
    revision: Arc<std::sync::atomic::AtomicU64>,
    scanner: parking_lot::Mutex<Option<Arc<view::Scanner>>>,
    /// What its work outlives no further than: cancelled when it is
    /// turned off, so a geotag under way proposes nothing after.
    lifetime: parking_lot::Mutex<skwir_core::CancelToken>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PaneArgs {
    /// The pane; the active one when left out.
    pane: Option<PaneId>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LocateArgs {
    /// A place's name, compared without case or accents.
    name: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct AreaArgs {
    pane: Option<PaneId>,
    /// The area's edges in degrees; `west` past `east` crosses ±180°.
    south: f64,
    west: f64,
    north: f64,
    east: f64,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct PointsArgs {
    pane: Option<PaneId>,
    /// Points already had: only those after them are returned.
    #[serde(default)]
    since: usize,
}

/// What `geo.search_subfolders` looks for: pictures, videos, and the
/// files of the formats read but those too common to look inside.
fn placeable_query() -> String {
    let extensions: Vec<&str> = EXTENSIONS
        .iter()
        .map(|(extension, _)| *extension)
        .filter(|extension| !NOT_PLACED.contains(extension))
        .collect();
    format!("(kind:image OR kind:video OR ext:{})", extensions.join(","))
}

impl Plugin for GeoPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> skwir_core::words::Words {
        skwir_core::words!(geo_plugin_title = "Maps")
    }
    fn description(&self) -> skwir_core::words::Words {
        skwir_core::words!(
            geo_plugin_description = "Show GPS tracks and routes (GPX, KML, KMZ, GeoJSON, TCX, FIT, IGC, NMEA), flight plans (Garmin, Flight Simulator, X-Plane, Little Navmap) and drone missions (QGroundControl, MAVLink, Litchi, DJI) on a map, with their distance, time and climb, and the pictures and videos of a folder in the Map view. Nothing a file links to is fetched"
        )
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![geotag::requirement()]
    }
    fn for_viewer(&self) -> bool {
        true
    }

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[
            Capability::Read,
            Capability::Write,
            Capability::Programs,
        ])
    }
    fn register(&self, app: &App) {
        app.previews.register(ID, Arc::new(Provider));
        app.thumbnails.register(ID, Arc::new(thumb::Lines));
        app.fields.register(ID, Arc::new(position::Positions));
        let lifetime = app.lifetime().child();
        *self.lifetime.lock() = lifetime.clone();
        geotag::register(app, lifetime);
        let mut spec = ViewSpec::named(
            VIEW,
            words!(
                geo_view = "Map",
                note = "The view that places a folder's pictures, videos and tracks on a map, among the views (List, Grid…)."
            ),
            "map",
            Arc::clone(&self.revision),
        )
        .lens();
        // After Usage, the other lens.
        spec.order = 110;
        app.registry.views.add(ID, spec);
        app.registry.keymap.add(
            ID,
            KeyBinding {
                // After Usage's Ctrl+4.
                keys: "ctrl-5".into(),
                command: skwir_core::command::Invocation::with_args(
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
            ID, Topic::View,
            "geo.scan",
            words!(geo_scan = "Find where the files are"),
            words!(
                geo_scan_description = "Find the pictures and tracks of what a pane lists that say where they are, for the Map view",
                note = "Read by agents and in F1's list of actions."
            ),
            move |args, app| {
                scan.scan(app, app.pane_or_active(args.pane))?;
                Ok(Outcome::Done)
            },
        )
        // The Map's own, a lens a window answering another program's dialog
        // does not offer (ADR-144, 1).
        .not_in_dialogs()
        .shows();
        // The Map view reads the points where they are kept; scripts and
        // agents ask `geo.points` for the same batch.
        app.maps.register(
            ID,
            Arc::clone(&scanner) as Arc<dyn skwir_core::map_points::MapSource>,
        );
        let points = Arc::clone(&scanner);
        app.commands.register::<PointsArgs, _>(
            ID, Topic::View,
            "geo.points",
            words!(geo_points = "Where the files are"),
            words!(
                geo_points_description = "The pictures and tracks of what a pane lists that were placed on the map, after the first `since`",
                note = "Read by agents and in F1's list of actions. `since` is the name of the command's argument: keep it as it is."
            ),
            move |args, app| {
                let pane = app.pane_or_active(args.pane);
                let found = points.batch(pane, args.since, &app.withheld(pane));
                Ok(Outcome::Value(serde_json::to_value(found)?))
            },
        )
        .data();
        let release = Arc::clone(&scanner);
        app.commands
            .register::<PaneArgs, _>(
                ID,
                Topic::View,
                "geo.release",
                words!(geo_release = "Forget where the files are"),
                words!(
                    geo_release_description =
                        "Stop finding where a pane's files are: its Map view closed"
                ),
                move |args, app| {
                    release.release(app.pane_or_active(args.pane));
                    Ok(Outcome::Done)
                },
            )
            .not_in_dialogs()
            .shows();
        app.commands.register::<LocateArgs, _>(
            ID, Topic::View,
            "geo.locate",
            words!(geo_locate = "Where is a place"),
            words!(
                geo_locate_description = "Return the places of that name Skwir knows offline, biggest first, with their positions: for scripts and agents"
            ),
            |args, _| {
                let places: Vec<serde_json::Value> = places::find(&args.name)
                    .into_iter()
                    .take(20)
                    .map(|place| {
                        serde_json::json!({
                            "name": place.name,
                            "lat": place.at.lat,
                            "lon": place.at.lon,
                            "country": place.country,
                        })
                    })
                    .collect();
                Ok(Outcome::Value(serde_json::json!({
                    "places": places,
                    "credit": places::attribution(),
                })))
            },
        )
        .data();
        app.commands.register::<AreaArgs, _>(
            ID, Topic::Find,
            "geo.search_area",
            words!(
                geo_search_area = "Search this area",
                note = "An action of the Map view: find the pictures and videos made inside the area the map shows."
            ),
            words!(
                geo_search_area_description = "Show the pictures and videos of the folder and its subfolders that were made inside an area"
            ),
            |args, app| {
                let edge = |value: f64| {
                    let text = format!("{value:.5}");
                    text.trim_end_matches('0').trim_end_matches('.').to_owned()
                };
                app.dispatch(&skwir_core::command::Invocation::with_args(
                    "query.search",
                    &serde_json::json!({
                        "expr": format!(
                            "(kind:image OR kind:video) inside:{},{},{},{}",
                            edge(args.south),
                            edge(args.west),
                            edge(args.north),
                            edge(args.east)
                        ),
                        "pane": app.pane_or_active(args.pane),
                    }),
                ))
            },
        )
        .shows();
        app.commands
            .register::<PaneArgs, _>(
                ID,
                Topic::Find,
                "geo.search_subfolders",
                words!(
                    geo_search_subfolders = "Search subfolders for places",
                    note = "An action of the Map view: put on the map the pictures and tracks of the folder's subfolders too."
                ),
                words!(
                    geo_search_subfolders_description = "Show the pictures and tracks of the folder and all its subfolders"
                ),
                |args, app| {
                    app.dispatch(&skwir_core::command::Invocation::with_args(
                        "query.search",
                        &serde_json::json!({
                            "expr": placeable_query(),
                            "pane": app.pane_or_active(args.pane),
                        }),
                    ))
                },
            )
            .shows();
    }
    fn on_disable(&self, _app: &App) {
        self.lifetime.lock().cancel();
        if let Some(scanner) = self.scanner.lock().take() {
            scanner.stop();
        }
        places::release();
    }
}

/// A format this plugin reads, told by the file's extension.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Format {
    Gpx,
    Kml,
    Kmz,
    GeoJson,
    Tcx,
    Fit,
    Igc,
    Nmea,
    /// Garmin's `.fpl`.
    GarminPlan,
    /// Microsoft Flight Simulator's `.pln`.
    SimPlan,
    /// X-Plane's `.fms`.
    XPlanePlan,
    LittleNavmap,
    /// `QGroundControl`'s JSON `.plan`.
    QgcPlan,
    /// `MAVLink`'s `QGC WPL` text.
    Mavlink,
    Litchi,
}

/// The extensions read, and what they name. Those the content must confirm
/// are `Format::shared`.
const EXTENSIONS: &[(&str, Format)] = &[
    ("gpx", Format::Gpx),
    ("kml", Format::Kml),
    ("kmz", Format::Kmz),
    ("geojson", Format::GeoJson),
    ("tcx", Format::Tcx),
    ("fit", Format::Fit),
    ("igc", Format::Igc),
    ("nmea", Format::Nmea),
    ("fpl", Format::GarminPlan),
    ("pln", Format::SimPlan),
    ("fms", Format::XPlanePlan),
    ("lnmpln", Format::LittleNavmap),
    ("plan", Format::QgcPlan),
    ("waypoints", Format::Mavlink),
    ("mission", Format::Mavlink),
    ("txt", Format::Mavlink),
    ("csv", Format::Litchi),
];

/// The extensions of the formats that can hold a timed track.
pub(crate) fn track_extensions() -> impl Iterator<Item = &'static str> {
    EXTENSIONS
        .iter()
        .filter(|(_, format)| format.timed())
        .map(|(extension, _)| *extension)
}

/// Extensions of files too common to look inside for the Map view: a
/// folder's text files and spreadsheets are not places.
const NOT_PLACED: &[&str] = &["txt", "csv"];
/// What of a file's start `Format::detect` looks at.
const HEAD: usize = 4096;

impl Format {
    /// The format a file's extension names: for an extension other
    /// programs share, only a candidate (`detect` confirms it).
    pub(crate) fn of(path: &Path) -> Option<Self> {
        let extension = inspection::extension(path);
        EXTENSIONS
            .iter()
            .find(|(e, _)| *e == extension)
            .map(|(_, format)| *format)
    }

    /// The format of the file at `path`, as its extension names it and
    /// its first bytes confirm: other programs use `.plan`, `.csv` or
    /// `.txt` for other things, and a file is read as a format only when
    /// it is one. Reads at most `HEAD` bytes: on a worker.
    pub(crate) fn detect(path: &Path) -> Option<Self> {
        Self::confirmed(path, || {
            let mut head = Vec::with_capacity(HEAD);
            inspection::open(path)
                .ok()?
                .take(HEAD as u64)
                .read_to_end(&mut head)
                .ok()?;
            Some(head.into())
        })
    }

    /// The format of a file a preview holds, as `detect` tells it, from
    /// the start already read (ADR-101): nothing is read again.
    pub(crate) fn of_held(held: &Held) -> Option<Self> {
        Self::confirmed(held.path(), || Some(held.head().into()))
    }

    /// The format a file named `path` is, its first bytes asked of `head`
    /// only where they decide, and only `HEAD` of them looked at: a
    /// preview holds more, which must not change the answer.
    fn confirmed<'a>(path: &Path, head: impl FnOnce() -> Option<Cow<'a, [u8]>>) -> Option<Self> {
        let format = Self::of(path)?;
        if !format.confirmed_by_content() {
            return Some(format);
        }
        let head = head()?;
        format
            .sniff(&head[..head.len().min(HEAD)])
            .then_some(format)
    }

    /// Whether the Map view looks inside files of this name.
    pub(crate) fn placeable(name: &Path) -> bool {
        Self::of(name).is_some() && !NOT_PLACED.contains(&inspection::extension(name).as_str())
    }

    /// Whether the format can hold a track timed point by point, which
    /// pictures are placed along (`geo.geotag`): not a plan or a mission.
    fn timed(self) -> bool {
        matches!(
            self,
            Format::Gpx
                | Format::Kml
                | Format::Kmz
                | Format::GeoJson
                | Format::Tcx
                | Format::Fit
                | Format::Igc
                | Format::Nmea
        )
    }

    /// Whether the first bytes are asked: the formats of step A are
    /// taken by their extensions, as they were.
    fn confirmed_by_content(self) -> bool {
        !matches!(
            self,
            Format::Gpx | Format::Kml | Format::Kmz | Format::GeoJson
        )
    }

    fn sniff(self, head: &[u8]) -> bool {
        match self {
            Format::Tcx => tcx::sniff(head),
            Format::Fit => fit::sniff(head),
            Format::Igc => igc::sniff(head),
            Format::Nmea => nmea::sniff(head),
            Format::GarminPlan => fpl::sniff(head),
            Format::SimPlan => pln::sniff(head),
            Format::XPlanePlan => fms::sniff(head),
            Format::LittleNavmap => lnmpln::sniff(head),
            Format::QgcPlan => qgc::sniff(head),
            Format::Mavlink => mavlink::sniff(head),
            Format::Litchi => litchi::sniff(head),
            Format::Gpx | Format::Kml | Format::Kmz | Format::GeoJson => true,
        }
    }

    /// What the format is called, in a map's title: its own name, or
    /// what it is made with.
    fn name(self) -> Words {
        match self {
            // Formats' names, the same in every language.
            Format::Gpx => Words::as_written("GPX"),
            Format::Kml => Words::as_written("KML"),
            Format::Kmz => Words::as_written("KMZ"),
            Format::GeoJson => Words::as_written("GeoJSON"),
            Format::Tcx => Words::as_written("TCX"),
            Format::Fit => Words::as_written("FIT"),
            Format::Igc => Words::as_written("IGC"),
            Format::Nmea => Words::as_written("NMEA"),
            Format::GarminPlan => words!(
                geo_format_garmin_plan = "Garmin flight plan",
                note = "A file format, in a map's title: a flight plan of Garmin's (.fpl)."
            ),
            Format::SimPlan => words!(
                geo_format_sim_plan = "Flight Simulator plan",
                note = "A file format, in a map's title: a flight plan of Microsoft Flight Simulator (.pln)."
            ),
            Format::XPlanePlan => words!(
                geo_format_xplane_plan = "X-Plane plan",
                note = "A file format, in a map's title: a flight plan of X-Plane, a flight simulator (.fms)."
            ),
            Format::LittleNavmap => words!(
                geo_format_little_navmap_plan = "Little Navmap plan",
                note = "A file format, in a map's title: a flight plan of Little Navmap, a flight planning program (.lnmpln)."
            ),
            Format::QgcPlan => words!(
                geo_format_qgc_plan = "QGroundControl plan",
                note = "A file format, in a map's title: a drone mission of QGroundControl, a mission planner (.plan)."
            ),
            Format::Mavlink => words!(
                geo_format_mavlink_mission = "MAVLink mission",
                note = "A file format, in a map's title: a drone mission in MAVLink's text (.waypoints, .mission)."
            ),
            Format::Litchi => words!(
                geo_format_litchi_mission = "Litchi mission",
                note = "A file format, in a map's title: a drone mission of Litchi, a drone app (.csv)."
            ),
        }
    }

    /// Whether the file is text to be read as source.
    fn has_source(self) -> bool {
        !matches!(self, Format::Kmz | Format::Fit)
    }
}

/// Read `source` (a path, or what a preview holds) into `into`, what is
/// read whole within `bounds`; a note when the file was damaged or cut.
fn read<S: FileSource + ?Sized>(
    source: &S,
    format: Format,
    into: &mut Collector,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<Words>> {
    let mut notes = Vec::new();
    let file = inspection::open(source)?;
    if source.length().is_ok_and(|len| len > MAX_INPUT) {
        notes.push(words!(
            geo_read_first = "Only the first { $size } were read",
            size = Size(MAX_INPUT),
            note = "A notice under the map of a file larger than is read. size: how much of it was read (256 MB)."
        ));
    }
    let damaged = match format {
        Format::Gpx => gpx::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Kml => kml::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Tcx => tcx::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Fit => fit::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Igc => igc::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Nmea => nmea::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::GarminPlan => fpl::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::SimPlan => pln::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::XPlanePlan => fms::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::LittleNavmap => lnmpln::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Mavlink => mavlink::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Litchi => litchi::read(BufReader::new(file.take(MAX_INPUT)), into, cancel)?,
        Format::Kmz => {
            drop(file);
            let too_many = words!(
                geo_kmz_members = "{ $count ->
                   *[other] This KMZ holds more than { $count } members
                }",
                count = inspection::limits::CONTAINER_MEMBERS,
                note = "Why a KMZ file (a zipped Google Earth file) is not shown on a map: it holds more files than the preview reads (4,000)."
            );
            let mut package =
                skwir_platform::package::Package::open(source, too_many, false, cancel)?;
            // A DJI mission (WPML) is a KMZ of its own kind.
            if let Some(name) = wpml::MEMBERS.iter().find(|name| package.has(name)) {
                let member = package.reader(name).ok_or_else(|| {
                    anyhow::anyhow!(words!(
                        geo_wpml_unreadable = "This mission cannot be read",
                        note = "Why a DJI drone mission (a KMZ file) is not shown on a map: the mission it holds cannot be unpacked."
                    ))
                })?;
                wpml::read(BufReader::new(member.take(MAX_INPUT)), into, cancel)?
            } else {
                let name = kmz_document(package.names()).ok_or_else(|| {
                    anyhow::anyhow!(words!(
                        geo_kmz_no_kml = "This KMZ holds no KML document",
                        note = "Why a KMZ file (a zipped Google Earth file) is not shown on a map: it holds no .kml file."
                    ))
                })?;
                let member = package.reader(&name).ok_or_else(|| {
                    anyhow::anyhow!(words!(
                        geo_kmz_unreadable = "This KMZ's document cannot be read",
                        note = "Why a KMZ file (a zipped Google Earth file) is not shown on a map: the .kml file it holds cannot be unpacked."
                    ))
                })?;
                kml::read(BufReader::new(member.take(MAX_INPUT)), into, cancel)?
            }
        }
        Format::QgcPlan => {
            let refusal = words!(
                geo_plan_too_large = "This plan exceeds the { $size } preview limit",
                size = words::Size(bounds.binary_input() as u64),
                note = "Why a QGroundControl plan (a drone mission) is not shown on a map: it is larger than the preview reads. size: that bound (16 MB)."
            );
            let bytes = inspection::read_file(source, bounds, refusal, cancel)?;
            let document: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
                anyhow::anyhow!(words!(
                    geo_qgc_invalid = "Not a valid QGroundControl plan: { $error }",
                    error = error.to_string(),
                    note = "Why a .plan file of QGroundControl (a drone mission planner) is not shown on a map. error: what the JSON reader says is wrong, in English."
                ))
            })?;
            inspection::check(cancel)?;
            qgc::read(&document, into);
            None
        }
        Format::GeoJson => {
            let refusal = words!(
                geo_geojson_too_large = "This GeoJSON file exceeds the { $size } preview limit",
                size = words::Size(bounds.binary_input() as u64),
                note = "Why a GeoJSON file is not shown on a map: it is larger than the preview reads. size: that bound (16 MB)."
            );
            let bytes = inspection::read_file(source, bounds, refusal, cancel)?;
            geojson::read(&bytes, into, cancel)?;
            None
        }
    };
    if let Some(xml::Damaged(why)) = damaged {
        notes.push(words!(
            geo_damaged = "The file is damaged ({ $why }); what came before is shown",
            why = why,
            note = "A notice under a map: the file stops making sense part way, and what was read before is drawn. why: what is wrong, in lower case (the file ends inside a record)."
        ));
    }
    Ok(notes)
}

/// The KML a KMZ holds: `doc.kml` as Google Earth writes it, else the
/// first KML nearest the top.
fn kmz_document<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let names: Vec<&str> = names.collect();
    names
        .iter()
        .find(|name| name.eq_ignore_ascii_case("doc.kml"))
        .or_else(|| {
            names
                .iter()
                .filter(|name| name.to_ascii_lowercase().ends_with(".kml"))
                // The shallowest, then by name: the members' order is the
                // index's, which is no order.
                .min_by_key(|name| (name.matches('/').count(), **name))
        })
        .map(|name| (*name).to_owned())
}

/// A length or a height in metres.
fn in_metres(value: words::Decimal) -> Words {
    words!(
        geo_metres = "{ $metres }\u{a0}m",
        metres = value,
        note = "A length or a height in metres, in a map's Properties (Climb: 120 m). metres: the number."
    )
}

/// A distance in kilometres.
pub(crate) fn in_kilometres(value: words::Decimal) -> Words {
    words!(
        geo_kilometres = "{ $km }\u{a0}km",
        km = value,
        note = "A distance in kilometres, in a map's Properties (Distance: 42.2 km) and in where a picture was taken (5.6 km NE of Example Town). km: the number."
    )
}

/// A position, its latitude and longitude in degrees to five places (a
/// metre or so).
fn position(lat: f64, lon: f64) -> Words {
    words!(
        geo_position = "{ $lat }, { $lon }",
        lat = words::Decimal::of(lat, 5),
        lon = words::Decimal::of(lon, 5),
        note = "A position in a map's Properties (Start: 45.12345, 6.12345): its latitude then its longitude, in degrees. A language whose decimal sign is a comma separates the two otherwise."
    )
}

pub fn distance(metres: f64) -> Words {
    if metres < 1000. {
        in_metres(words::Decimal::of(metres, 0))
    } else if metres < 100_000. {
        in_kilometres(words::Decimal::of(metres / 1000., 1))
    } else {
        in_kilometres(words::Decimal::of(metres / 1000., 0))
    }
}

pub fn duration(taken: Duration) -> String {
    let total = taken.as_secs();
    format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
}

/// An instant in the user's time zone (ADR-017), to the minute, as the
/// window writes dates (by the locale, or ISO 8601 where the dates setting
/// says so); `day` is left out when it is the one given.
fn local(time: SystemTime, zone: &jiff::tz::TimeZone, day: Option<jiff::civil::Date>) -> String {
    let Ok(stamp) = jiff::Timestamp::try_from(time) else {
        return String::new();
    };
    let shape = if day == Some(stamp.to_zoned_datetime(zone.clone()).date()) {
        Shape::Time
    } else {
        Shape::DateTime
    };
    Moment::of(time, shape)
        .map(Moment::said)
        .unwrap_or_default()
}

/// The labels two lists of facts share: a track's and a plan's.
fn distance_label() -> Words {
    words!(
        geo_fact_distance = "Distance",
        note = "A label in a map's Properties: how long the track, the route or the plan is."
    )
}

fn waypoints_label() -> Words {
    words!(
        geo_fact_waypoints = "Waypoints",
        note = "A label in a map's Properties: how many waypoints (places marked by name) the file holds."
    )
}

fn highest_point_label() -> Words {
    words!(
        geo_fact_highest_point = "Highest point",
        note =
            "A label in a map's Properties: the highest altitude the track or the mission reaches."
    )
}

fn areas_label() -> Words {
    words!(
        geo_fact_areas = "Areas",
        note = "A label in a map's Properties: how many areas (outlined shapes) the file holds."
    )
}

fn name_label() -> Words {
    words!(
        geo_fact_name = "Name",
        note = "A label in a map's Properties: the name the file gives itself."
    )
}

fn description_label() -> Words {
    words!(
        geo_fact_description = "Description",
        note = "A label in a map's Properties: what the file says of itself."
    )
}

fn created_by_label() -> Words {
    words!(
        geo_fact_created_by = "Created by",
        note = "A label in a map's Properties: the device or the program that wrote the file."
    )
}

/// The facts of the lines of one kind: those of the tracks when there are
/// any, else the routes' (a route has no times).
fn line_facts(totals: &Totals, zone: &jiff::tz::TimeZone) -> Vec<(Words, Option<String>)> {
    let span = totals
        .first
        .zip(totals.last)
        .and_then(|(first, last)| last.duration_since(first).ok());
    let start_day = totals.first.and_then(|first| {
        jiff::Timestamp::try_from(first)
            .ok()
            .map(|stamp| stamp.to_zoned_datetime(zone.clone()).date())
    });
    let moving = (!totals.moving.is_zero()).then_some(totals.moving);
    let metres = |value: f64| in_metres(words::Decimal::of(value, 0)).say();
    vec![
        (
            distance_label(),
            (totals.distance > 0.).then(|| distance(totals.distance).say()),
        ),
        (
            words!(
                geo_fact_duration = "Duration",
                note = "A label in a map's Properties: how long the track took, from its first point to its last."
            ),
            span.filter(|s| !s.is_zero()).map(duration),
        ),
        (
            words!(
                geo_fact_moving_time = "Moving time",
                note = "A label in a map's Properties: how long the track was on the move, its stops left out."
            ),
            moving.map(duration),
        ),
        (
            words!(
                geo_fact_moving_speed = "Moving speed",
                note = "A label in a map's Properties: the track's average speed while it was on the move."
            ),
            moving.map(|m| {
                words!(
                    geo_speed = "{ $speed }\u{a0}km/h",
                    speed = words::Decimal::of(totals.distance / m.as_secs_f64() * 3.6, 1),
                    note =
                        "A speed in kilometres per hour, in a map's Properties. speed: the number."
                )
                .say()
            }),
        ),
        (
            words!(
                geo_fact_climb = "Climb",
                note = "A label in a map's Properties: how much the track climbed, every rise added up."
            ),
            (totals.climb > 0.).then(|| metres(totals.climb)),
        ),
        (
            words!(
                geo_fact_descent = "Descent",
                note = "A label in a map's Properties: how much the track went down, every descent added up."
            ),
            (totals.descent > 0.).then(|| metres(totals.descent)),
        ),
        (
            highest_point_label(),
            totals.highest.map(|h| metres(f64::from(h))),
        ),
        (
            words!(
                geo_fact_lowest_point = "Lowest point",
                note = "A label in a map's Properties: the lowest altitude the track reaches."
            ),
            totals.lowest.map(|l| metres(f64::from(l))),
        ),
        (
            words!(
                geo_fact_started = "Started",
                note = "A label in a map's Properties: when the track's first point was recorded."
            ),
            totals.first.map(|t| local(t, zone, None)),
        ),
        (
            words!(
                geo_fact_ended = "Ended",
                note = "A label in a map's Properties: when the track's last point was recorded."
            ),
            totals.last.map(|t| local(t, zone, start_day)),
        ),
    ]
}

/// Metres in a foot: flight plans state altitudes in feet.
const FOOT: f32 = 0.3048;
/// Metres in a nautical mile, the distance of a flight.
const NAUTICAL_MILE: f64 = 1852.;

/// An altitude as a plan's reader thinks of it: feet for a flight
/// (`35 000 ft · 10 668 m`), metres from its datum for a mission.
fn plan_altitude(kind: PlanKind, metres: f32, datum: Datum) -> Words {
    let written = words::Decimal::of(f64::from(metres), 0);
    match kind {
        PlanKind::Flight => {
            let feet = group_thousands((metres / FOOT / 100.).round() * 100.);
            match datum {
                Datum::Sea => words!(
                    geo_altitude_flight = "{ $feet }\u{a0}ft · { $metres }\u{a0}m",
                    feet = feet,
                    metres = written,
                    note = "A flight plan's altitude above the sea, in feet then metres, in a map's Properties. feet, metres: the numbers."
                ),
                Datum::Ground => words!(
                    geo_altitude_flight_ground =
                        "{ $feet }\u{a0}ft · { $metres }\u{a0}m above ground",
                    feet = feet,
                    metres = written,
                    note = "A flight plan's altitude above the ground below, in feet then metres, in a map's Properties. feet, metres: the numbers."
                ),
                Datum::Takeoff => words!(
                    geo_altitude_flight_takeoff =
                        "{ $feet }\u{a0}ft · { $metres }\u{a0}m above takeoff",
                    feet = feet,
                    metres = written,
                    note = "A flight plan's altitude above where it took off, in feet then metres, in a map's Properties. feet, metres: the numbers."
                ),
            }
        }
        PlanKind::Mission => match datum {
            Datum::Sea => in_metres(written),
            Datum::Ground => words!(
                geo_altitude_ground = "{ $metres }\u{a0}m above ground",
                metres = written,
                note = "A drone mission's height above the ground below, in a map's Properties. metres: the number."
            ),
            Datum::Takeoff => words!(
                geo_altitude_takeoff = "{ $metres }\u{a0}m above takeoff",
                metres = written,
                note = "A drone mission's height above where the drone took off, in a map's Properties. metres: the number."
            ),
        },
    }
}

/// `35000` as `35 000`, as altitudes are read.
fn group_thousands(value: f32) -> String {
    let digits = format!("{value:.0}");
    let (sign, digits) = digits
        .strip_prefix('-')
        .map_or(("", digits.as_str()), |d| ("-", d));
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    format!("{sign}{out}")
}

/// What a flight plan or a mission says: where from and to, its legs and
/// its distance, how high.
fn plan_facts(plan: &Plan, doc: &GeoDoc, c: &Collector) -> Vec<(Words, Option<String>)> {
    let legs = c.routes.points.saturating_sub(c.routes.lines);
    let datum = doc
        .lines
        .iter()
        .find(|l| l.kind == LineKind::Route)
        .map_or(Datum::Sea, |l| l.datum);
    let flight = plan.kind == PlanKind::Flight;
    let distance = (c.routes.distance > 0.).then(|| {
        if flight {
            words!(
                geo_flight_distance = "{ $distance } · { $nautical }\u{a0}NM",
                distance = distance(c.routes.distance),
                nautical = words::Decimal::of(c.routes.distance / NAUTICAL_MILE, 0),
                note = "A flight plan's distance in a map's Properties: in metres or kilometres, then in nautical miles. distance: the first, with its unit; nautical: the number of nautical miles."
            )
            .say()
        } else {
            distance(c.routes.distance).say()
        }
    });
    let highest = c
        .routes
        .highest
        .map(|metres| plan_altitude(plan.kind, metres, datum).say());
    vec![
        (
            words!(
                geo_fact_departure = "Departure",
                note =
                    "A label in a map's Properties: where a flight plan or a drone mission starts."
            ),
            plan.departure.clone(),
        ),
        (
            words!(
                geo_fact_destination = "Destination",
                note =
                    "A label in a map's Properties: where a flight plan or a drone mission ends."
            ),
            plan.destination.clone(),
        ),
        (
            if flight {
                words!(
                    geo_fact_legs = "Legs",
                    note = "A label in a map's Properties: how many legs (stretches from one waypoint to the next) a flight plan has."
                )
            } else {
                waypoints_label()
            },
            inspection::count(if flight { legs } else { doc.waypoints.len() }),
        ),
        (distance_label(), distance),
        (
            if flight {
                words!(
                    geo_fact_cruise = "Cruise",
                    note = "A label in a map's Properties: the altitude a flight plan cruises at."
                )
            } else {
                words!(
                    geo_fact_flight_height = "Flight height",
                    note = "A label in a map's Properties: the height a drone mission flies at."
                )
            },
            plan.cruise
                .map(|(metres, datum)| plan_altitude(plan.kind, metres, datum).say()),
        ),
        (
            if flight {
                words!(
                    geo_fact_highest_waypoint = "Highest waypoint",
                    note = "A label in a map's Properties: the altitude of a flight plan's highest waypoint."
                )
            } else {
                highest_point_label()
            },
            highest.filter(|_| plan.cruise.is_none()),
        ),
    ]
}

/// Facts with their labels, in the order given, those without a value
/// left out.
fn details(facts: Vec<(Words, Option<String>)>) -> Vec<Detail> {
    inspection::details(facts)
}

fn facts(doc: &GeoDoc, c: &Collector) -> Vec<Detail> {
    if let Some(plan) = &c.plan {
        let mut facts = plan_facts(plan, doc, c);
        facts.extend([
            (areas_label(), inspection::count(doc.areas.len())),
            (name_label(), c.name.clone()),
            (description_label(), c.description.clone()),
            (created_by_label(), c.creator.clone()),
        ]);
        return details(facts);
    }
    let zone = skwir_platform::calendar::Calendar::system()
        .time_zone()
        .clone();
    let main = if c.tracks.lines > 0 {
        &c.tracks
    } else {
        &c.routes
    };
    let mut facts = line_facts(main, &zone);
    if c.tracks.lines > 0 && c.routes.distance > 0. {
        facts.push((
            words!(
                geo_fact_route_distance = "Route distance",
                note = "A label in a map's Properties: how long the file's routes (the ways planned) are, beside its tracks (the ways recorded)."
            ),
            Some(distance(c.routes.distance).say()),
        ));
    }
    let start = doc
        .lines
        .first()
        .and_then(|l| l.segments.first())
        .and_then(|s| s.first())
        .map(|p| p.at)
        .or_else(|| doc.waypoints.first().map(|w| w.at));
    facts.extend([
        (
            words!(
                geo_fact_tracks = "Tracks",
                note = "A label in a map's Properties: how many tracks (ways recorded) the file holds."
            ),
            inspection::count(c.tracks.lines),
        ),
        (
            words!(
                geo_fact_routes = "Routes",
                note = "A label in a map's Properties: how many routes (ways planned) the file holds."
            ),
            inspection::count(c.routes.lines),
        ),
        (waypoints_label(), inspection::count(doc.waypoints.len())),
        (areas_label(), inspection::count(doc.areas.len())),
        (
            words!(
                geo_fact_points = "Points",
                note = "A label in a map's Properties: how many points the file's tracks and routes have."
            ),
            inspection::count(c.tracks.points + c.routes.points),
        ),
        (
            words!(
                geo_fact_start = "Start",
                note = "A label in a map's Properties: the latitude and longitude of the file's first point."
            ),
            start.map(|at| position(at.lat, at.lon).say()),
        ),
        (name_label(), c.name.clone()),
        (description_label(), c.description.clone()),
        (created_by_label(), c.creator.clone()),
    ]);
    details(facts)
}

/// What a plan is, in a few words: `Flight plan · XAAA → XBBB`.
fn plan_summary(plan: &Plan, doc: &GeoDoc) -> Words {
    if plan.kind == PlanKind::Flight {
        return match (&plan.departure, &plan.destination) {
            (Some(from), Some(to)) => words!(
                geo_summary_flight_route = "Flight plan · { $from } → { $to }",
                from = from.as_str(),
                to = to.as_str(),
                note = "The title of a flight plan's preview. from, to: where it departs and where it lands, as the plan names them (an airport's code, XAAA)."
            ),
            _ => words!(
                geo_summary_flight = "Flight plan",
                note =
                    "The title of a flight plan's preview, when it does not say where from and to."
            ),
        };
    }
    match doc.waypoints.len() {
        0 => words!(
            geo_summary_mission = "Drone mission",
            note = "The title of a drone mission's preview, when it has no waypoint."
        ),
        count => words!(
            geo_summary_mission_waypoints = "Drone mission · { $count ->
                [one] { $count } waypoint
               *[other] { $count } waypoints
            }",
            count = count,
            note = "The title of a drone mission's preview. count: how many waypoints it has."
        ),
    }
}

/// What the file holds, a few words each: `1 track`, `12 waypoints`.
fn contents(doc: &GeoDoc, c: &Collector) -> Vec<Words> {
    let mut contents = Vec::new();
    if c.tracks.lines > 0 {
        contents.push(words!(
            geo_contents_tracks = "{ $count ->
                [one] { $count } track
               *[other] { $count } tracks
            }",
            count = c.tracks.lines,
            note = "Part of a map's title, after its format (GPX · 1 track, 2 waypoints): how many tracks (ways recorded) the file holds."
        ));
    }
    if c.routes.lines > 0 {
        contents.push(words!(
            geo_contents_routes = "{ $count ->
                [one] { $count } route
               *[other] { $count } routes
            }",
            count = c.routes.lines,
            note = "Part of a map's title, after its format (GPX · 1 route, 2 waypoints): how many routes (ways planned) the file holds."
        ));
    }
    if !doc.waypoints.is_empty() {
        contents.push(words!(
            geo_contents_waypoints = "{ $count ->
                [one] { $count } waypoint
               *[other] { $count } waypoints
            }",
            count = doc.waypoints.len(),
            note = "Part of a map's title, after its format (GPX · 1 track, 2 waypoints): how many waypoints (places marked by name) the file holds."
        ));
    }
    if !doc.areas.is_empty() {
        contents.push(words!(
            geo_contents_areas = "{ $count ->
                [one] { $count } area
               *[other] { $count } areas
            }",
            count = doc.areas.len(),
            note = "Part of a map's title, after its format (KML · 3 areas): how many areas (outlined shapes) the file holds."
        ));
    }
    contents
}

/// What is not a map file: refused.
fn not_a_map() -> Words {
    words!(
        geo_not_a_map = "Not a map file",
        note = "Why a file is not shown on a map: its name says a format of tracks or plans, and it holds none."
    )
}

/// Read a file into what the map shows and what Properties says, within
/// `bounds`.
pub(crate) fn inspect_file(
    path: &Path,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    let format = Format::detect(path).ok_or_else(|| anyhow::anyhow!(not_a_map()))?;
    inspect_as(path, format, bounds, cancel)
}

/// Read a file of a format already known: at its path, or what a preview
/// holds of it.
pub(crate) fn inspect_as<S: FileSource + ?Sized>(
    source: &S,
    format: Format,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    let mut collector = Collector::new();
    let mut notes = read(source, format, &mut collector, bounds, cancel)?;
    let (doc, c) = collector.finish();
    if doc.bounds.is_none() {
        notes.insert(
            0,
            words!(
                geo_no_position = "Nothing in this file has a position",
                note = "A notice in place of a map: the file holds no point with a latitude and a longitude."
            ),
        );
    }
    if c.stride() > 1 {
        notes.push(words!(
            geo_thinned = "The map shows one point in { $stride }",
            stride = c.stride(),
            note = "A notice under a map of a track with more points than are drawn: one point of every few is drawn. stride: how many points each drawn point stands for."
        ));
    }
    if c.invalid > 0 {
        notes.push(words!(
            geo_invalid_points = "{ $count ->
                [one] { $count } point left out: not a position
               *[other] { $count } points left out: not a position
            }",
            count = c.invalid,
            note = "A notice under a map: points of the file whose latitude or longitude is not one are not drawn."
        ));
    }
    let dropped = c.dropped_waypoints + c.dropped_areas;
    if dropped > 0 {
        notes.push(words!(
            geo_dropped_places = "{ $count ->
                [one] { $count } more place is not shown on the map
               *[other] { $count } more places are not shown on the map
            }",
            count = dropped,
            note = "A notice under a map of a file with more waypoints and areas than are drawn. count: how many are not drawn."
        ));
    }
    // "Flight plan · XAAA → XBBB": the format says less than that.
    let title = if let Some(plan) = &c.plan {
        plan_summary(plan, &doc)
    } else {
        {
            let contents = contents(&doc, &c);
            if contents.is_empty() {
                format.name()
            } else {
                words!(
                    geo_title = "{ $format } · { $contents }",
                    format = format.name(),
                    contents = words::List::and(contents),
                    note = "The title of a map's preview: the file's format, then what it holds (GPX · 1 track, 1 route and 2 waypoints). contents: a list of counts, each its words (1 track)."
                )
            }
        }
    };
    Ok(Inspection {
        kind: InspectionKind::Map,
        title,
        details: facts(&doc, &c),
        notices: notes,
        // A KMZ is compressed and a FIT file binary: no source to read.
        source_available: format.has_source(),
        geo: Some(Arc::new(doc)),
        ..Inspection::default()
    })
}

struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        ID
    }
    /// Before the readers of tables and text, which take a name whole: a
    /// `.csv` or a `.txt` this provider accepts is a mission its first
    /// bytes confirmed, which a table of numbers would show less well. A
    /// provider that settles a shared name by content outranks one that
    /// takes the name; never a tie, which registration order would settle.
    fn priority(&self) -> i32 {
        20
    }
    /// On a worker: a file of a shared extension is asked what it is, of
    /// the start the preview holds.
    fn accepts(&self, req: &PreviewRequest) -> bool {
        match Format::of(&req.entry.path) {
            None => false,
            Some(format) if !format.confirmed_by_content() => true,
            Some(_) => req
                .held()
                .is_ok_and(|held| Format::of_held(&held).is_some()),
        }
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
        let held = req.held()?;
        let format = Format::of_held(&held).ok_or_else(|| anyhow::anyhow!(not_a_map()))?;
        inspect_as(&held, format, Bounds::of(view), cancel).map(Some)
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
            .find(|d| d.label.english() == label)
            .map(|d| d.value.as_str())
    }

    /// What its notices say together, in English; empty for none.
    fn notice(data: &Inspection) -> String {
        data.notice().map(|said| said.english()).unwrap_or_default()
    }

    #[test]
    fn a_track_says_how_far_how_long_and_how_high() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("it's a café walk.gpx");
        std::fs::write(&path, gpx::tests::HIKE).unwrap();
        let data = inspect_file(&path, Bounds::QUICK, &CancelToken::new()).unwrap();
        assert_eq!(data.kind, InspectionKind::Map);
        assert_eq!(
            data.title.english(),
            "GPX · 1 track, 1 route, and 2 waypoints"
        );
        assert_eq!(fact(&data, "Distance"), Some("222\u{a0}m"));
        assert_eq!(fact(&data, "Duration"), Some("0:02:00"));
        assert_eq!(fact(&data, "Climb"), Some("20\u{a0}m"));
        assert_eq!(fact(&data, "Highest point"), Some("1,520\u{a0}m"));
        assert_eq!(fact(&data, "Name"), Some("Café & ridge"));
        assert_eq!(fact(&data, "Start"), Some("45.00000, 6.00000"));
        assert!(data.source_available);
        assert!(
            notice(&data).contains("1 point left out"),
            "{}",
            notice(&data)
        );
        let geo = data.geo.as_ref().unwrap();
        assert_eq!(geo.lines.len(), 2);
    }

    /// A KMZ of one place, beside a picture.
    fn kmz() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("files/icon.png", options).unwrap();
        zip.write_all(b"not a picture").unwrap();
        zip.start_file("doc.kml", options).unwrap();
        zip.write_all(
            b"<kml><Placemark><name>Here</name><Point><coordinates>2.35,48.85</coordinates></Point></Placemark></kml>",
        )
        .unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn a_kmz_is_read_through_its_kml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trip.kmz");
        std::fs::write(&path, kmz()).unwrap();
        let data = inspect_file(&path, Bounds::QUICK, &CancelToken::new()).unwrap();
        assert_eq!(data.title.english(), "KMZ · 1 waypoint");
        assert!(!data.source_available);
        assert_eq!(data.geo.as_ref().unwrap().waypoints[0].name, "Here");
    }

    #[test]
    fn a_preview_reads_what_is_held_never_the_path() {
        let dir = tempfile::tempdir().unwrap();
        // Paths that lead nowhere: only what is held can be read.
        let gone = |name: &str| dir.path().join("gone").join(name);
        let cancel = CancelToken::new();
        let show = |req: &PreviewRequest| {
            Provider
                .inspect(req, &InspectRequest::default(), &cancel)
                .unwrap()
                .unwrap()
        };
        let mission = mavlink::tests::SURVEY.join("\n");
        let req =
            PreviewRequest::holding(gone("survey.txt"), mission.as_bytes(), Some("text/plain"));
        assert!(Provider.accepts(&req));
        let data = show(&req);
        assert_eq!(data.kind, InspectionKind::Map);
        assert_eq!(data.title.english(), "Drone mission · 5 waypoints");
        // A text file of that name is not a mission: its start says so.
        let notes = PreviewRequest::holding(
            gone("notes.txt"),
            b"QGC WPL is what a mission starts with\n".as_slice(),
            Some("text/plain"),
        );
        assert!(!Provider.accepts(&notes));
        let hike = PreviewRequest::holding(
            gone("hike.gpx"),
            gpx::tests::HIKE.as_bytes(),
            Some("application/gpx+xml"),
        );
        assert!(Provider.accepts(&hike));
        assert_eq!(
            show(&hike).title.english(),
            "GPX · 1 track, 1 route, and 2 waypoints"
        );
        let trip = PreviewRequest::holding(
            gone("trip.kmz"),
            kmz(),
            Some("application/vnd.google-earth.kmz"),
        );
        assert!(Provider.accepts(&trip));
        assert_eq!(show(&trip).title.english(), "KMZ · 1 waypoint");
    }

    #[test]
    fn a_file_without_positions_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.geojson");
        std::fs::write(&path, r#"{"type":"FeatureCollection","features":[]}"#).unwrap();
        let data = inspect_file(&path, Bounds::QUICK, &CancelToken::new()).unwrap();
        assert!(notice(&data).starts_with("Nothing in this file has a position"));
        assert!(data.geo.as_ref().unwrap().bounds.is_none());
        std::fs::write(&path, "{ not json").unwrap();
        let error = inspect_file(&path, Bounds::QUICK, &CancelToken::new()).unwrap_err();
        assert!(
            error.to_string().starts_with("Not valid GeoJSON"),
            "{error}"
        );
    }

    #[test]
    fn distances_and_durations_read_as_people_say_them() {
        assert_eq!(distance(850.4).english(), "850\u{a0}m");
        assert_eq!(distance(42_195.).english(), "42.2\u{a0}km");
        assert_eq!(distance(1_234_567.).english(), "1,235\u{a0}km");
        assert_eq!(
            duration(Duration::from_secs(3 * 3600 + 42 * 60 + 10)),
            "3:42:10"
        );
    }
}
