//! Writing where pictures were taken (ADR-022, part 2): each picture taken
//! while a track was recorded gets the track's position at that moment,
//! and a picture's location can be taken out before it is shared.
//!
//! One operation kind, `set_position`, does both: it writes a position,
//! takes every location tag out, or writes back exactly the tags a file
//! held (its own undo). `ExifTool` is the writer (principle 4): run with
//! separate arguments, the file's modification time kept (`-P`, and its
//! fraction of a second put back after), the file rewritten in place so
//! its inode, hard links and extended attributes (tags) stay.
//!
//! A camera's clock is not an instant: it is read in the zone the picture
//! states (`OffsetTimeOriginal`) or else the user's on that date
//! (ADR-017), plus the camera's own error, which the command takes.
//!
//! RAW pictures are left out: they are written through an XMP sidecar,
//! never in place, and sidecars are not written yet.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use bibli_core::app::{App, NotifyLevel};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::keys;
use bibli_core::geo::{Datum, GeoDoc, LatLon, LineKind};
use bibli_core::lru::ByteLru;
use bibli_core::op::{Change, EntryRef, Operation, Plan, ResolveError, StepContext, StepStatus};
use bibli_core::plugin::Requirement;
use bibli_core::registry::MenuItem;
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use bibli_core::{CancelToken, os_path};
use bibli_platform::calendar::Calendar;
use bibli_platform::mime::{self, Kind};
use jiff::civil::DateTime;
use jiff::tz::{Offset, TimeZone};
use parking_lot::Mutex;

use crate::{Format, ID};

/// The operation kind, as plans name it.
pub(crate) const KIND: &str = "set_position";
const PROGRAM: &str = "exiftool";
const MISSING: &str = "Install ExifTool to write pictures' locations";

/// The operation kind, the commands and their menu items (owner `geo`).
pub(crate) fn register(app: &App, lifetime: bibli_core::CancelToken) {
    app.ops.register::<SetPosition>(ID);
    app.commands.register::<GeotagArgs, _>(
        ID,
        "geo.geotag",
        "Geotag pictures from a track",
        "Place the selected pictures along the one selected track (GPX, FIT, any track read): each picture taken while it was recorded gets its position at that moment; proposed for review. Works on a worker: the result arrives as a proposal or a notification",
        move |args, app| geotag(app, args, &lifetime),
    )
        .plans();
    app.commands.register::<RemoveArgs, _>(
        ID,
        "geo.remove_location",
        "Remove pictures' locations",
        "Take the location out of the selected pictures (plan); those without one are left as they are",
        |args, app| remove_location(app, args),
    )
        .plans();
    // Offered only where they apply: pictures beside a file of a track
    // format (which one is a track is read when the command runs), and
    // pictures only.
    let tracks: Vec<String> = crate::track_extensions()
        .map(|extension| format!("has:.{extension}"))
        .collect();
    for (title, command, when, order, icon) in [
        (
            "Geotag from Track",
            "geo.geotag",
            format!("has:image {}", tracks.join("|")),
            160,
            "map",
        ),
        (
            "Remove Location",
            "geo.remove_location",
            "kind:image".to_owned(),
            161,
            "close",
        ),
    ] {
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: title.into(),
                command: Invocation::new(command),
                when: Some(when),
                order,
                icon: Some(icon.into()),
                submenu: None,
            },
        );
    }
}

/// `ExifTool`, for `Plugin::requirements`.
pub(crate) fn requirement() -> Requirement {
    Requirement::program(PROGRAM, "Writing and removing pictures' locations")
}

// ---------------------------------------------------------------- matching

/// Before a track's first point or after its last, or beside a gap, a
/// picture this close to a point is taken where that point is.
pub(crate) const EDGE: Duration = Duration::from_mins(2);
/// Two points further apart in time than this are a gap: a picture taken
/// between them is not placed by interpolation.
pub(crate) const GAP: Duration = Duration::from_mins(5);

/// A position to write: degrees, and metres above sea level.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Position {
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub altitude: Option<f64>,
}

/// A timed point of a track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fix {
    pub time: SystemTime,
    pub at: LatLon,
    /// Above sea level, when the track says so.
    pub altitude: Option<f64>,
}

/// Every timed point of every track of `doc`, all segments, in time order
/// (a point at the same time as an earlier one is dropped). Routes have no
/// times; an altitude is kept only when it is above sea level, as EXIF's.
pub(crate) fn fixes(doc: &GeoDoc) -> Vec<Fix> {
    let mut fixes: Vec<Fix> = doc
        .lines
        .iter()
        .filter(|line| line.kind == LineKind::Track)
        .flat_map(|line| {
            let sea = line.datum == Datum::Sea;
            line.segments.iter().flatten().filter_map(move |p| {
                Some(Fix {
                    time: p.time?,
                    at: p.at,
                    altitude: p.altitude.filter(|_| sea).map(f64::from),
                })
            })
        })
        .collect();
    fixes.sort_by_key(|f| f.time);
    fixes.dedup_by_key(|f| f.time);
    fixes
}

/// Where the track was at `t`: between two points no more than [`GAP`]
/// apart, interpolated by time (the altitude too, when both have one);
/// exactly at a point, that point; otherwise the nearest point no more
/// than [`EDGE`] away (before the first, after the last, or beside a gap:
/// the ends of a track are gaps too), else nowhere.
pub(crate) fn position_at(fixes: &[Fix], t: SystemTime) -> Option<Position> {
    let i = fixes.partition_point(|f| f.time < t);
    let after = fixes.get(i);
    if let Some(f) = after.filter(|f| f.time == t) {
        return Some(at(f));
    }
    let before = i.checked_sub(1).map(|j| &fixes[j]);
    if let (Some(a), Some(b)) = (before, after) {
        let span = b.time.duration_since(a.time).unwrap_or_default();
        if span <= GAP {
            let f = t.duration_since(a.time).unwrap_or_default().as_secs_f64() / span.as_secs_f64();
            return Some(between(a, b, f));
        }
    }
    let from_before = before.and_then(|a| t.duration_since(a.time).ok());
    let to_after = after.and_then(|b| b.time.duration_since(t).ok());
    match (from_before, to_after) {
        (Some(x), Some(y)) if y < x && y <= EDGE => after.map(at),
        (Some(x), _) if x <= EDGE => before.map(at),
        (_, Some(y)) if y <= EDGE => after.map(at),
        _ => None,
    }
}

fn at(f: &Fix) -> Position {
    Position {
        lat: f.at.lat,
        lon: f.at.lon,
        altitude: f.altitude,
    }
}

/// The point `f` (0 to 1) of the way from `a` to `b`, the short way
/// round in longitude: a track over the ±180° meridian stays on it.
fn between(a: &Fix, b: &Fix, f: f64) -> Position {
    let mut dlon = b.at.lon - a.at.lon;
    if dlon > 180. {
        dlon -= 360.;
    } else if dlon < -180. {
        dlon += 360.;
    }
    let mut lon = a.at.lon + f * dlon;
    if lon > 180. {
        lon -= 360.;
    } else if lon < -180. {
        lon += 360.;
    }
    Position {
        lat: a.at.lat + f * (b.at.lat - a.at.lat),
        lon,
        altitude: a.altitude.zip(b.altitude).map(|(x, y)| x + f * (y - x)),
    }
}

/// When a picture was taken, as its camera wrote it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Clock {
    /// The camera's clock: a date and a time of day, no zone.
    pub local: DateTime,
    /// The offset from UTC the picture states, if it does.
    pub stated: Option<Offset>,
}

/// The instant a camera's clock names: in the zone the picture states,
/// else in `tz` on that date (daylight saving time included), plus
/// `offset_seconds`, the camera's own error (positive when it was slow).
pub(crate) fn instant(clock: Clock, tz: &TimeZone, offset_seconds: i64) -> Option<SystemTime> {
    let stamp = match clock.stated {
        Some(offset) => offset.to_timestamp(clock.local).ok()?,
        None => clock.local.to_zoned(tz.clone()).ok()?.timestamp(),
    };
    let stamp = stamp
        .checked_add(jiff::SignedDuration::from_secs(offset_seconds))
        .ok()?;
    Some(SystemTime::from(stamp))
}

/// `2026:07:01 08:30:00`, perhaps with a fraction and a zone
/// (`.25+02:00`, `Z`), as EXIF and XMP write dates.
pub(crate) fn parse_clock(text: &str) -> Option<Clock> {
    let text = text.trim();
    let digits = |range: std::ops::Range<usize>| -> Option<i16> {
        let part = text.get(range)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (y, mo, d) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (h, mi, s) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    let small = |v: i16| i8::try_from(v).ok();
    let local = DateTime::new(
        y,
        small(mo)?,
        small(d)?,
        small(h)?,
        small(mi)?,
        small(s)?,
        0,
    )
    .ok()?;
    let rest = text
        .get(19..)?
        .trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let stated = match rest {
        "" => None,
        "Z" => Some(Offset::UTC),
        zone => Some(parse_offset(zone)?),
    };
    Some(Clock { local, stated })
}

/// `+02:00` or `-05:30`.
pub(crate) fn parse_offset(text: &str) -> Option<Offset> {
    let text = text.trim();
    let (sign, rest) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    let (h, m) = rest.split_once(':').unwrap_or((rest, "0"));
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    Offset::from_seconds(sign * (h * 3600 + m * 60)).ok()
}

// ---------------------------------------------------------------- ExifTool

/// A file's location tags as `ExifTool` reads them with `-n -G1 -struct`:
/// the group and the tag (`GPS:GPSLatitude`, `XMP-drone-dji:GPSLongitude`,
/// `IPTC:City`) and the value, a structure as a JSON object. `GPSVersionID`
/// is left out: `ExifTool` writes it whenever it creates the GPS directory.
pub(crate) type Tags = BTreeMap<String, serde_json::Value>;

/// Where a position can be, as `ExifTool` selects it: the EXIF GPS
/// directory, every XMP tag whose name starts with GPS in any namespace
/// (`XMP-exif`, a drone maker's; the match ignores case), and the
/// `QuickTime` coordinates a HEIF container may carry. Writing a position
/// clears these first.
const POSITION: [&str; 5] = [
    "GPS:all",
    "XMP:GPS*",
    "Keys:GPSCoordinates",
    "UserData:GPSCoordinates",
    "ItemList:GPSCoordinates",
];

/// What else says where a picture was, which removing the location takes
/// out with [`POSITION`] and writing a position leaves: a drone's height
/// (above the sea, above its takeoff) in whichever XMP namespace its maker
/// uses, and the place names and locations of IPTC, IIM and XMP alike
/// (`LocationCreated` and `LocationShown` are structures that may hold
/// coordinates). A drone's flight and gimbal angles stay: they say where
/// it looked, not where it was.
const PLACE: [&str; 14] = [
    "XMP:AbsoluteAltitude",
    "XMP:RelativeAltitude",
    "IPTC:City",
    "IPTC:Sub-location",
    "IPTC:Province-State",
    "IPTC:Country-PrimaryLocationName",
    "IPTC:Country-PrimaryLocationCode",
    "XMP-photoshop:City",
    "XMP-photoshop:State",
    "XMP-photoshop:Country",
    "XMP-iptcCore:Location",
    "XMP-iptcCore:CountryCode",
    "XMP-iptcExt:LocationCreated",
    "XMP-iptcExt:LocationShown",
];

/// Every selector of the location: what is read, and what removing takes
/// out, so the undo of a removal restores all of it.
fn location() -> impl Iterator<Item = &'static str> {
    POSITION.into_iter().chain(PLACE)
}

/// Whether `ExifTool`'s selector (`XMP:GPS*`, `GPS:all`, `IPTC:City`)
/// selects the key of its JSON (`XMP-drone-dji:GPSLatitude`): the family 0
/// group `XMP` is any XMP namespace; a `*` ends a prefix, matched as
/// `ExifTool` does, ignoring case.
fn selects(selector: &str, key: &str) -> bool {
    let (Some((want_group, want_tag)), Some((group, tag))) =
        (selector.split_once(':'), key.split_once(':'))
    else {
        return false;
    };
    let group_matches = want_group == group || (want_group == "XMP" && group.starts_with("XMP-"));
    let tag_matches = match want_tag.strip_suffix('*') {
        _ if want_tag == "all" => true,
        Some(prefix) => tag
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix)),
        None => want_tag == tag,
    };
    group_matches && tag_matches
}

/// Whether a key of `ExifTool`'s JSON is a location tag.
fn is_location_tag(key: &str) -> bool {
    !key.ends_with(":GPSVersionID") && location().any(|selector| selects(selector, key))
}

/// The arguments that read `selectors`, or with `=`, take them out.
fn selector_args<'a>(
    selectors: impl Iterator<Item = &'static str> + 'a,
    suffix: &'a str,
) -> impl Iterator<Item = String> + 'a {
    selectors.map(move |selector| format!("-{selector}{suffix}"))
}

fn program() -> anyhow::Result<PathBuf> {
    os_path::find_program(PROGRAM).ok_or_else(|| anyhow::anyhow!(MISSING))
}

/// `ExifTool` with the arguments every run shares: no user configuration
/// (it could rename or redefine tags), numbers as numbers.
fn exiftool(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.args(["-config", "", "-n"]);
    command
}

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);
/// Files read by one `ExifTool` run while planning.
const CHUNK: usize = 100;

/// What one read of a picture found.
#[derive(Clone, Debug, Default)]
pub(crate) struct Read {
    pub tags: Tags,
    pub clock: Option<Clock>,
}

/// Read `paths` with as few `ExifTool` runs as can be: each file's location
/// tags and when it was taken, or why it could not be read.
pub(crate) fn read_many(
    program: &Path,
    paths: &[PathBuf],
    cancel: &CancelToken,
) -> Vec<Result<Read, String>> {
    let mut out: Vec<Option<Result<Read, String>>> = vec![None; paths.len()];
    for (chunk_index, chunk) in paths.chunks(CHUNK).enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        let base = chunk_index * CHUNK;
        let before: Vec<Option<Seen>> = chunk.iter().map(|p| seen_now(p)).collect();
        let mut command = exiftool(program);
        command.args(read_args()).arg("--").args(chunk);
        let Ok(finished) = bibli_platform::process::run_to_exit(
            &mut command,
            None,
            cancel,
            READ_TIMEOUT + Duration::from_secs(chunk.len() as u64),
        ) else {
            continue;
        };
        // A file ExifTool could not open is left out of its list: match by
        // name, and read what is left one by one.
        let names: HashMap<String, usize> = chunk
            .iter()
            .enumerate()
            .map(|(i, p)| (p.to_string_lossy().into_owned(), i))
            .collect();
        for object in parse(&finished.stdout) {
            let Some(i) = object
                .get("SourceFile")
                .and_then(|s| s.as_str())
                .and_then(|s| names.get(s))
            else {
                continue;
            };
            let read = found(&object);
            if let (Ok(read), Some(seen)) = (&read, &before[*i])
                && seen_now(&chunk[*i]).as_ref() == Some(seen)
            {
                remember(seen.clone(), &read.tags);
            }
            out[base + i] = Some(read);
        }
    }
    out.into_iter()
        .zip(paths)
        .map(|(read, path)| read.unwrap_or_else(|| read_one(program, path, cancel)))
        .collect()
}

fn read_args() -> impl Iterator<Item = String> {
    // `-a`: a tag present in two groups is two tags, both restored.
    [
        "-j",
        "-a",
        "-G1",
        "-struct",
        "-DateTimeOriginal",
        "-OffsetTimeOriginal",
    ]
    .into_iter()
    .map(String::from)
    .chain(selector_args(location(), ""))
}

/// One file, when the batch did not say.
fn read_one(program: &Path, path: &Path, cancel: &CancelToken) -> Result<Read, String> {
    let mut command = exiftool(program);
    command.args(read_args()).arg("--").arg(path);
    let finished = bibli_platform::process::run_to_exit(&mut command, None, cancel, READ_TIMEOUT)
        .map_err(|e| format!("{e:#}"))?;
    match parse(&finished.stdout).first() {
        Some(object) => found(object),
        None if finished.stderr.is_empty() => Err("ExifTool could not read it".into()),
        None => Err(finished.stderr),
    }
}

/// `ExifTool`'s JSON: an array with an object per file. A name that is not
/// UTF-8 would make it invalid; replaced, it is still valid.
fn parse(stdout: &[u8]) -> Vec<serde_json::Map<String, serde_json::Value>> {
    serde_json::from_str::<Vec<serde_json::Value>>(&String::from_utf8_lossy(stdout))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| match v {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .collect()
}

fn found(object: &serde_json::Map<String, serde_json::Value>) -> Result<Read, String> {
    if let Some(error) = object.get("ExifTool:Error").and_then(|e| e.as_str()) {
        return Err(error.to_owned());
    }
    let tags = object
        .iter()
        .filter(|(key, _)| is_location_tag(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    // EXIF's own first: `-a` lists XMP's too.
    let field = |name: &str| {
        object
            .get(&format!("ExifIFD:{name}"))
            .or_else(|| {
                object
                    .iter()
                    .find(|(key, _)| key.rsplit(':').next() == Some(name))
                    .map(|(_, value)| value)
            })
            .and_then(|value| value.as_str())
    };
    let clock = field("DateTimeOriginal")
        .and_then(parse_clock)
        .map(|mut clock| {
            if clock.stated.is_none() {
                clock.stated = field("OffsetTimeOriginal").and_then(parse_offset);
            }
            clock
        });
    Ok(Read { tags, clock })
}

/// The location tags of the file at `path` now.
fn read_tags(program: &Path, path: &Path, cancel: &CancelToken) -> anyhow::Result<Tags> {
    let seen = seen_now(path);
    if let Some(tags) = seen.as_ref().and_then(recall) {
        return Ok(tags);
    }
    let read = read_one(program, path, cancel).map_err(|e| anyhow::anyhow!(e))?;
    if let Some(seen) = seen
        && seen_now(path).as_ref() == Some(&seen)
    {
        remember(seen, &read.tags);
    }
    Ok(read.tags)
}

/// A file's state, precise enough that a file with the same one has not
/// been written: its change time moves with any write, even one that puts
/// the modification time back.
/// `None` where the system does not say the change time or the identity
/// (Windows, not read yet: ADR-035), and nothing is remembered.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Seen {
    id: bibli_core::FileId,
    size: u64,
    modified: SystemTime,
    changed: SystemTime,
}

fn seen_now(path: &Path) -> Option<Seen> {
    let m = std::fs::metadata(path).ok()?;
    Some(Seen {
        id: bibli_core::fs::stat::identity(path, &m)?,
        size: m.len(),
        modified: m.modified().ok()?,
        changed: bibli_core::fs::stat::changed(path, &m)?,
    })
}

/// Tags read, by the file's state: a plan's steps are checked by its
/// review, again before it is applied, and each once more as its turn
/// comes, and `ExifTool` is a Perl start-up each time. What planning read
/// in batches serves them all until the file is written.
static READ: LazyLock<Mutex<ByteLru<Seen, Tags>>> =
    LazyLock::new(|| Mutex::new(ByteLru::new(1024 * 1024)));

fn remember(seen: Seen, tags: &Tags) {
    let bytes = 96
        + tags
            .iter()
            .map(|(k, v)| k.len() + v.to_string().len() + 48)
            .sum::<usize>();
    READ.lock().insert(seen, tags.clone(), bytes);
}

fn recall(seen: &Seen) -> Option<Tags> {
    READ.lock().get(seen).cloned()
}

/// `ExifTool`'s `-listwf`: the extensions of the files it can write.
fn writable_extensions(program: &Path, cancel: &CancelToken) -> anyhow::Result<Vec<String>> {
    let mut command = exiftool(program);
    command.arg("-listwf");
    let out = bibli_platform::process::run(&mut command, cancel, READ_TIMEOUT)?;
    let text = String::from_utf8_lossy(&out);
    Ok(text
        .lines()
        .skip(1)
        .flat_map(str::split_whitespace)
        .map(str::to_ascii_lowercase)
        .collect())
}

/// Write `to` into the file at `path`: every location tag out first, then
/// what `to` holds. The modification time is kept, to the nanosecond: a
/// folder sorted by date does not reorder.
fn write(program: &Path, path: &Path, to: &Location, cancel: &CancelToken) -> anyhow::Result<()> {
    let modified = std::fs::metadata(path)?.modified()?;
    // The selectors cleared, then what is written: `ExifTool` applies
    // them in that order.
    let run = |clear: &mut dyn Iterator<Item = &'static str>,
               assignments: Vec<String>,
               import: Option<&Import>|
     -> anyhow::Result<bool> {
        let mut command = exiftool(program);
        command
            .args(["-P", "-overwrite_original_in_place", "-struct"])
            .args(selector_args(clear, "="))
            .args(assignments);
        if let Some(import) = import {
            command.arg(import.argument());
        }
        command.arg("--").arg(path);
        let input = import.and_then(Import::input);
        let finished =
            bibli_platform::process::run_to_exit(&mut command, input, cancel, WRITE_TIMEOUT)?;
        let stdout = String::from_utf8_lossy(&finished.stdout);
        let updated = stdout.lines().any(|line| {
            line.trim()
                .strip_suffix("image files updated")
                .and_then(|n| n.trim().parse::<u32>().ok())
                .is_some_and(|n| n > 0)
        });
        if !finished.status.success() {
            anyhow::bail!(
                "ExifTool could not write “{}”: {}",
                display_name(path),
                finished.stderr
            );
        }
        Ok(updated)
    };
    let result = match to {
        Location::None => run(&mut location(), Vec::new(), None).map(|_| ()),
        // Structures and all, as `ExifTool` read them: its own JSON back.
        Location::Tags(tags) => Import::new(tags)
            .and_then(|import| run(&mut location(), Vec::new(), Some(&import)))
            .map(|_| ()),
        // The position alone: place names stay.
        Location::At(p) => {
            // EXIF where the format has it; XMP where it has only that (GIF).
            if run(&mut POSITION.into_iter(), exif_assignments(p), None)?
                || run(&mut POSITION.into_iter(), xmp_assignments(p), None)?
            {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "ExifTool cannot write a location into “{}”",
                    display_name(path)
                ))
            }
        }
    };
    // Put the time back whatever happened: `-P` keeps whole seconds only.
    let kept = std::fs::File::open(path).and_then(|f| f.set_modified(modified));
    result?;
    kept?;
    Ok(())
}

/// Tags for `ExifTool` to write back from JSON (`-json=`): on its standard
/// input when they fit there, else from a file in a directory of our own,
/// removed when done.
struct Import {
    json: Vec<u8>,
    file: Option<PrivateFile>,
}

impl Import {
    fn new(tags: &Tags) -> anyhow::Result<Self> {
        let mut object = serde_json::Map::new();
        object.insert("SourceFile".into(), "*".into());
        object.extend(tags.iter().map(|(k, v)| (k.clone(), v.clone())));
        let json = serde_json::to_vec(&[object])?;
        let file = if json.len() > bibli_platform::process::MAX_INPUT {
            Some(PrivateFile::new(&json)?)
        } else {
            None
        };
        Ok(Import { json, file })
    }

    fn argument(&self) -> OsString {
        let mut argument = OsString::from("-json=");
        match &self.file {
            Some(file) => argument.push(&file.path),
            None => argument.push("-"),
        }
        argument
    }

    fn input(&self) -> Option<&[u8]> {
        self.file.is_none().then_some(self.json.as_slice())
    }
}

/// A file only this user can read, in a directory created for it and
/// removed with it.
struct PrivateFile {
    dir: PathBuf,
    path: PathBuf,
}

/// Make the folder `dir`, failing if anything is there, open to its
/// owner alone on Unix; elsewhere the user's temporary folder is already
/// theirs alone.
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(dir)
    }
}

impl PrivateFile {
    fn new(bytes: &[u8]) -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        loop {
            let dir = std::env::temp_dir().join(format!(
                "bibli-geotag-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            // Fails if anything is there already: never someone else's.
            match create_private_dir(&dir) {
                Ok(()) => {
                    let file = PrivateFile {
                        path: dir.join("location.json"),
                        dir,
                    };
                    std::fs::write(&file.path, bytes)?;
                    return Ok(file);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for PrivateFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn exif_assignments(p: &Position) -> Vec<String> {
    let mut out = vec![
        format!("-GPS:GPSLatitude={}", p.lat.abs()),
        format!("-GPS:GPSLatitudeRef={}", if p.lat < 0. { "S" } else { "N" }),
        format!("-GPS:GPSLongitude={}", p.lon.abs()),
        format!(
            "-GPS:GPSLongitudeRef={}",
            if p.lon < 0. { "W" } else { "E" }
        ),
    ];
    if let Some(alt) = p.altitude {
        out.push(format!("-GPS:GPSAltitude={}", alt.abs()));
        out.push(format!("-GPS:GPSAltitudeRef={}", u8::from(alt < 0.)));
    }
    out
}

/// XMP's coordinates are signed; its altitude has a reference as EXIF's.
fn xmp_assignments(p: &Position) -> Vec<String> {
    let mut out = vec![
        format!("-XMP-exif:GPSLatitude={}", p.lat),
        format!("-XMP-exif:GPSLongitude={}", p.lon),
    ];
    if let Some(alt) = p.altitude {
        out.push(format!("-XMP-exif:GPSAltitude={}", alt.abs()));
        out.push(format!("-XMP-exif:GPSAltitudeRef={}", u8::from(alt < 0.)));
    }
    out
}

/// The position location tags give: EXIF's, else XMP's.
pub(crate) fn position_of(tags: &Tags) -> Option<Position> {
    let text = |key: &str| match tags.get(key)? {
        serde_json::Value::String(s) => Some(s.trim().to_ascii_uppercase()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    let number = |key: &str| text(key)?.parse::<f64>().ok();
    let below = |key: &str| text(key).as_deref() == Some("1");
    let signed = |value: f64, negative: bool| if negative { -value.abs() } else { value };
    if let (Some(lat), Some(lon)) = (number("GPS:GPSLatitude"), number("GPS:GPSLongitude")) {
        return Some(Position {
            lat: signed(
                lat,
                text("GPS:GPSLatitudeRef").is_some_and(|r| r.starts_with('S')),
            ),
            lon: signed(
                lon,
                text("GPS:GPSLongitudeRef").is_some_and(|r| r.starts_with('W')),
            ),
            altitude: number("GPS:GPSAltitude").map(|a| signed(a, below("GPS:GPSAltitudeRef"))),
        });
    }
    let lat = number("XMP-exif:GPSLatitude")?;
    let lon = number("XMP-exif:GPSLongitude")?;
    Some(Position {
        lat,
        lon,
        altitude: number("XMP-exif:GPSAltitude")
            .map(|a| signed(a, below("XMP-exif:GPSAltitudeRef"))),
    })
}

/// Whether the tags give a position: coordinates in EXIF, in an XMP GPS
/// tag of any namespace (a drone's own), in a `QuickTime` key, or in an
/// IPTC location structure. A place name alone is not one.
fn has_location(tags: &Tags) -> bool {
    fn coordinates(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(fields) => fields.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("GPSLatitude") || coordinates(value)
            }),
            serde_json::Value::Array(items) => items.iter().any(coordinates),
            _ => false,
        }
    }
    position_of(tags).is_some()
        || tags.iter().any(|(key, value)| {
            let tag = key.rsplit(':').next().unwrap_or_default();
            tag.eq_ignore_ascii_case("GPSLatitude") || tag == "GPSCoordinates" || coordinates(value)
        })
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

// ---------------------------------------------------------------- operation

/// What a file's location becomes.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Location {
    /// No location: every tag of [`POSITION`] and [`PLACE`] taken out.
    None,
    /// A position, written as EXIF's GPS tags (XMP's for a format without
    /// EXIF), every other tag of [`POSITION`] taken out; place names stay.
    At(Position),
    /// Exactly these tags, as `ExifTool` read them from the file: what an
    /// undo writes back.
    Tags(Tags),
}

impl Location {
    fn of(tags: Tags) -> Self {
        if tags.is_empty() {
            Location::None
        } else {
            Location::Tags(tags)
        }
    }

    /// Whether a file whose location tags are `tags` already holds this.
    fn holds(&self, tags: &Tags) -> bool {
        match self {
            Location::None => tags.is_empty(),
            Location::Tags(wanted) => wanted == tags,
            Location::At(p) => position_of(tags).is_some_and(|q| same_position(p, &q)),
        }
    }
}

/// Equal as far as EXIF's rationals keep them: a millionth of a degree
/// (a tenth of a metre), a centimetre of altitude.
fn same_position(a: &Position, b: &Position) -> bool {
    const DEGREES: f64 = 1e-6;
    (a.lat - b.lat).abs() < DEGREES
        && (a.lon - b.lon).abs() < DEGREES
        && match (a.altitude, b.altitude) {
            (None, None) => true,
            (Some(x), Some(y)) => (x - y).abs() < 0.01,
            _ => false,
        }
}

/// A file as it was when a plan was made.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Planned {
    pub size: u64,
    pub modified: SystemTime,
    /// Its location then: what the review said would be replaced.
    pub before: Location,
}

/// Write, change or take out the location of one picture.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SetPosition {
    pub entry: EntryRef,
    pub to: Location,
    /// The file when the plan was made; a step made from what it was is
    /// refused once it changed. `None` for a plan made without reading the
    /// file (a single action planned on the caller's thread): the undo is
    /// read when the step is applied either way.
    #[serde(default)]
    pub planned: Option<Planned>,
}

impl SetPosition {
    fn changed(&self, path: &Path, tags: &Tags) -> bool {
        let Some(planned) = &self.planned else {
            return false;
        };
        let Ok(meta) = std::fs::metadata(path) else {
            return true;
        };
        meta.len() != planned.size
            || meta.modified().ok() != Some(planned.modified)
            || !planned.before.holds(tags)
    }
}

impl bibli_core::op::Kind for SetPosition {
    const KIND: &'static str = KIND;
}

impl bibli_core::op::Effect for SetPosition {
    fn describe(&self) -> Change {
        let name = display_name(&self.entry.path);
        let (will, did) = match &self.to {
            Location::At(p) => (
                format!("place “{name}” at {:.5}, {:.5}", p.lat, p.lon),
                format!("placed “{name}” at {:.5}, {:.5}", p.lat, p.lon),
            ),
            Location::None => (
                format!("remove the location of “{name}”"),
                format!("removed the location of “{name}”"),
            ),
            Location::Tags(_) => (
                format!("restore the location of “{name}”"),
                format!("restored the location of “{name}”"),
            ),
        };
        Change::Metadata {
            path: self.entry.path.clone(),
            field: keys::POSITION.into(),
            words: Some(bibli_core::op::Words { will, did }),
        }
    }
    fn check(&self, ctx: &StepContext) -> StepStatus {
        let path = match self.entry.resolve() {
            Ok(path) => path,
            Err(ResolveError::Gone(p)) => {
                return StepStatus::Conflict {
                    reason: format!("“{}” no longer exists", display_name(&p)),
                };
            }
            Err(e) => {
                return StepStatus::Conflict {
                    reason: e.to_string(),
                };
            }
        };
        let Ok(program) = program() else {
            return StepStatus::Conflict {
                reason: MISSING.into(),
            };
        };
        let tags = match read_tags(&program, &path, ctx.cancel_token()) {
            Ok(tags) => tags,
            Err(e) => {
                return StepStatus::Conflict {
                    reason: format!("“{}” could not be read: {e:#}", display_name(&path)),
                };
            }
        };
        if self.to.holds(&tags) {
            StepStatus::AlreadyDone
        } else if self.changed(&path, &tags) {
            StepStatus::Conflict {
                reason: format!("“{}” changed since the plan was made", display_name(&path)),
            }
        } else {
            StepStatus::Pending { reversible: true }
        }
    }
    fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let path = self.entry.resolve()?;
        let program = program()?;
        let cancel = ctx.cancel_token();
        // What undo writes back: read now, so it is what the file holds
        // (the check just read it: remembered).
        let before = read_tags(&program, &path, cancel)?;
        write(&program, &path, &self.to, cancel)?;
        let meta = std::fs::metadata(&path)?;
        Ok(Some(Box::new(SetPosition {
            entry: EntryRef::new(self.entry.id, path),
            to: Location::of(before),
            planned: Some(Planned {
                size: meta.len(),
                modified: meta.modified()?,
                before: self.to.clone(),
            }),
        })))
    }
}

// ---------------------------------------------------------------- commands

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct GeotagArgs {
    /// The track and the pictures, by absolute path; the pane's selection
    /// when omitted.
    #[serde(default)]
    entries: Vec<bibli_core::op::EntryArg>,
    /// The pane; the active one when left out.
    #[serde(default)]
    pane: Option<PaneId>,
    /// Seconds to add to the cameras' clocks to get the time they meant:
    /// 120 when a camera ran two minutes slow.
    #[serde(default)]
    offset_seconds: i64,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct RemoveArgs {
    /// The pictures, by absolute path; the pane's selection when omitted.
    #[serde(default)]
    entries: Vec<bibli_core::op::EntryArg>,
    /// The pane; the active one when left out.
    #[serde(default)]
    pane: Option<PaneId>,
}

/// What a geotag command acts on, typed: nothing selected is nothing, for
/// the command to say what to select.
fn typed(
    app: &App,
    pane: Option<PaneId>,
    entries: Vec<bibli_core::op::EntryArg>,
) -> anyhow::Result<Vec<(EntryRef, Option<std::ffi::OsString>)>> {
    match app.targets_typed(pane, entries) {
        Err(error) if bibli_core::command::is_nothing_selected(&error) => Ok(Vec::new()),
        typed => typed,
    }
}

fn geotag(
    app: &App,
    args: GeotagArgs,
    lifetime: &bibli_core::CancelToken,
) -> anyhow::Result<Outcome> {
    let typed = typed(app, args.pane, args.entries)?;
    // By name only here (a link's target's): which one is a track is read
    // on the worker.
    anyhow::ensure!(
        typed
            .iter()
            .filter_map(|(_, name)| name.as_deref())
            .any(|name| Format::of(Path::new(name)).is_some()),
        "Select a track and the pictures to place along it"
    );
    let refs: Vec<EntryRef> = typed.into_iter().map(|(r, _)| r).collect();
    let offset_seconds = args.offset_seconds;
    let weak = app.weak();
    // Ends with the plugin: turned off, it proposes nothing after.
    let cancel = lifetime.child();
    let job = cancel.clone();
    app.scheduler
        .spawn(Priority::Normal, "geo.geotag", cancel, move || {
            let tz = Calendar::system().time_zone().clone();
            let result = plan_geotag(&refs, &tz, offset_seconds, &job);
            let Some(app) = weak.upgrade() else { return };
            if job.is_cancelled() {
                return;
            }
            match result {
                Ok(Geotag::Plan(plan)) => app.propose(plan),
                Ok(Geotag::Nothing(why)) => app.notify(NotifyLevel::Warning, why),
                Err(e) => app.notify(NotifyLevel::Error, format!("{e:#}")),
            }
        });
    Ok(Outcome::Done)
}

/// What matching pictures with a track came to.
#[derive(Debug)]
pub(crate) enum Geotag {
    /// One step per picture placed; its title names what was left out.
    Plan(Plan),
    /// No picture could be placed, and why, in the user's words.
    Nothing(String),
}

/// `1 picture`, `3 pictures`.
fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn pictures(n: usize) -> String {
    count(n, "picture", "pictures")
}

fn is_raw(mime: &mime::Mime) -> bool {
    mime::lineage(mime)
        .iter()
        .any(|m| m.as_str() == "image/x-dcraw")
}

/// Why pictures were left out, each a phrase for the plan's title.
#[derive(Debug, Default)]
struct LeftOut {
    raw: usize,
    unwritable: usize,
    unreadable: usize,
    others: usize,
    placed: usize,
    undated: usize,
    outside: usize,
}

impl LeftOut {
    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        let mut note = |n: usize, text: String| {
            if n > 0 {
                notes.push(text);
            }
        };
        note(
            self.outside,
            format!("{} outside the track's times", pictures(self.outside)),
        );
        note(
            self.placed,
            format!(
                "{} already placed, left as {}",
                pictures(self.placed),
                if self.placed == 1 {
                    "it is"
                } else {
                    "they are"
                }
            ),
        );
        note(
            self.undated,
            format!("{} without the time taken", pictures(self.undated)),
        );
        note(
            self.raw,
            format!(
                "{} left out: their sidecars are not written yet",
                count(self.raw, "RAW picture", "RAW pictures")
            ),
        );
        note(
            self.unwritable,
            format!("{} ExifTool cannot write", pictures(self.unwritable)),
        );
        note(
            self.unreadable,
            format!("{} that could not be read", pictures(self.unreadable)),
        );
        note(
            self.others,
            format!(
                "{} left out",
                count(self.others, "other file", "other files")
            ),
        );
        notes
    }
}

/// Match the pictures among `refs` with the one track among them and
/// make the plan. Reads the track, runs `ExifTool`: on a worker.
pub(crate) fn plan_geotag(
    refs: &[EntryRef],
    tz: &TimeZone,
    offset_seconds: i64,
    cancel: &CancelToken,
) -> anyhow::Result<Geotag> {
    let program = program()?;
    let mut left = LeftOut::default();
    let mut tracks = Vec::new();
    let mut candidates = Vec::new();
    for r in refs {
        anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
        // A link is read, and typed, as what it leads to; a pipe or a
        // device is never read.
        let Some(path) = bibli_core::fs::content_path(&r.path) else {
            left.others += 1;
            continue;
        };
        if Format::detect(&path).is_some() {
            tracks.push(path.into_owned());
            continue;
        }
        let name = path.file_name().map(OsString::from).unwrap_or_default();
        match mime::identify(&name, &path) {
            Some(m) if mime::is_kind(&m, Kind::Image) && is_raw(&m) => left.raw += 1,
            Some(m) if mime::is_kind(&m, Kind::Image) => candidates.push(r.clone()),
            _ => left.others += 1,
        }
    }
    let track = match tracks.as_slice() {
        [track] => track,
        [] => anyhow::bail!("Select a track and the pictures to place along it"),
        more => anyhow::bail!("Select one track: {} are selected", more.len()),
    };
    let track_name = display_name(track);
    let doc = crate::inspect_file(track, cancel)?
        .geo
        .ok_or_else(|| anyhow::anyhow!("“{track_name}” holds no track"))?;
    let fixes = fixes(&doc);
    anyhow::ensure!(
        !fixes.is_empty(),
        "“{track_name}” has no times: only a recorded track can place pictures"
    );
    let writable = writable_extensions(&program, cancel)?;
    candidates.retain(|r| {
        let ok = writable.contains(&bibli_platform::inspection::extension(&r.path));
        if !ok {
            left.unwritable += 1;
        }
        ok
    });
    let paths: Vec<PathBuf> = candidates.iter().map(|r| r.path.clone()).collect();
    let stats: Vec<Option<std::fs::Metadata>> =
        paths.iter().map(|p| std::fs::metadata(p).ok()).collect();
    let reads = read_many(&program, &paths, cancel);
    anyhow::ensure!(!cancel.is_cancelled(), "cancelled");
    let mut steps = Vec::new();
    for ((r, read), meta) in candidates.iter().zip(reads).zip(stats) {
        let (Ok(read), Some(meta)) = (read, meta) else {
            left.unreadable += 1;
            continue;
        };
        if has_location(&read.tags) {
            left.placed += 1;
            continue;
        }
        let Some(t) = read.clock.and_then(|c| instant(c, tz, offset_seconds)) else {
            left.undated += 1;
            continue;
        };
        let Some(position) = position_at(&fixes, t) else {
            left.outside += 1;
            continue;
        };
        steps.push(SetPosition {
            entry: r.clone(),
            to: Location::At(position),
            planned: Some(Planned {
                size: meta.len(),
                modified: meta.modified()?,
                before: Location::of(read.tags),
            }),
        });
    }
    let mut notes = left.notes();
    if offset_seconds != 0 {
        notes.push(format!("cameras' clocks corrected by {offset_seconds:+} s"));
    }
    if steps.is_empty() {
        let mut why = format!("No picture to place along “{track_name}”");
        if !notes.is_empty() {
            why = format!("{why}: {}", notes.join(" · "));
        }
        return Ok(Geotag::Nothing(why));
    }
    let title = format!("place {} along “{track_name}”", what(&steps));
    Ok(Geotag::Plan(plan(title, &notes, steps)))
}

/// The picture by name when there is one, a count otherwise.
fn what(steps: &[SetPosition]) -> String {
    match steps {
        [one] => format!("“{}”", display_name(&one.entry.path)),
        _ => pictures(steps.len()),
    }
}

/// The plan of `steps`, what was left out said under its title.
fn plan(title: String, notes: &[String], steps: Vec<SetPosition>) -> Plan {
    let plan = Plan::with_steps(
        title,
        steps
            .into_iter()
            .map(|s| Box::new(s) as Box<dyn Operation>)
            .collect(),
    );
    if notes.is_empty() {
        plan
    } else {
        plan.with_note(notes.join(" · "))
    }
}

/// The selected pictures' locations taken out: a plan for the caller to
/// apply (a single action, undoable). Nothing is read here, on what may
/// be the UI thread: a picture without a location is already done when
/// the plan runs.
fn remove_location(app: &App, args: RemoveArgs) -> anyhow::Result<Outcome> {
    // Typed by the listings (a link as what it leads to): nothing is read
    // for the selection.
    let typed = typed(app, args.pane, args.entries)?;
    anyhow::ensure!(
        !typed.is_empty(),
        "Select the pictures whose location to remove"
    );
    let mut left = LeftOut::default();
    let mut steps = Vec::new();
    for (r, name) in typed {
        match name.and_then(|name| mime::by_name(&name)) {
            Some(m) if mime::is_kind(&m, Kind::Image) && is_raw(&m) => left.raw += 1,
            Some(m) if mime::is_kind(&m, Kind::Image) => steps.push(SetPosition {
                entry: r,
                to: Location::None,
                planned: None,
            }),
            _ => left.others += 1,
        }
    }
    let notes = left.notes();
    if steps.is_empty() {
        anyhow::bail!(
            "No picture selected{}",
            if notes.is_empty() {
                String::new()
            } else {
                format!(": {}", notes.join(" · "))
            }
        );
    }
    let title = format!("remove the location of {}", what(&steps));
    Ok(Outcome::Plan(plan(title, &notes, steps)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use bibli_core::geo::{Line, Point};
    use bibli_core::location::Location as Place;

    fn utc(text: &str) -> SystemTime {
        SystemTime::from(text.parse::<jiff::Timestamp>().unwrap())
    }

    fn fix(time: &str, lat: f64, lon: f64, altitude: Option<f64>) -> Fix {
        Fix {
            time: utc(time),
            at: LatLon { lat, lon },
            altitude,
        }
    }

    fn close(p: Option<Position>, lat: f64, lon: f64, altitude: Option<f64>) {
        let p = p.expect("a position");
        assert!(
            same_position(&p, &Position { lat, lon, altitude }),
            "{p:?} is not {lat}, {lon}, {altitude:?}"
        );
    }

    fn walk() -> Vec<Fix> {
        vec![
            fix("2026-07-01T06:00:00Z", 10.0, 20.0, Some(100.)),
            fix("2026-07-01T06:01:00Z", 10.001, 20.002, Some(110.)),
            fix("2026-07-01T06:02:00Z", 10.002, 20.004, None),
            // A pause of ten minutes.
            fix("2026-07-01T06:12:00Z", 10.012, 20.004, Some(130.)),
        ]
    }

    #[test]
    fn a_picture_between_two_points_is_placed_by_time() {
        let fixes = walk();
        close(
            position_at(&fixes, utc("2026-07-01T06:00:30Z")),
            10.0005,
            20.001,
            Some(105.),
        );
        close(
            position_at(&fixes, utc("2026-07-01T06:00:15Z")),
            10.00025,
            20.0005,
            Some(102.5),
        );
        // One of the points has no altitude: none is made up.
        close(
            position_at(&fixes, utc("2026-07-01T06:01:30Z")),
            10.0015,
            20.003,
            None,
        );
    }

    #[test]
    fn a_picture_taken_at_a_point_is_at_that_point() {
        let fixes = walk();
        close(
            position_at(&fixes, utc("2026-07-01T06:01:00Z")),
            10.001,
            20.002,
            Some(110.),
        );
        close(
            position_at(&fixes, utc("2026-07-01T06:00:00Z")),
            10.0,
            20.0,
            Some(100.),
        );
    }

    #[test]
    fn before_the_first_point_or_after_the_last_only_two_minutes_count() {
        let fixes = walk();
        let first = |t: &str| position_at(&fixes, utc(t));
        close(first("2026-07-01T05:58:30Z"), 10.0, 20.0, Some(100.));
        close(first("2026-07-01T05:58:00Z"), 10.0, 20.0, Some(100.));
        assert_eq!(first("2026-07-01T05:57:59Z"), None);
        close(first("2026-07-01T06:14:00Z"), 10.012, 20.004, Some(130.));
        assert_eq!(first("2026-07-01T06:14:01Z"), None);
        assert_eq!(position_at(&[], utc("2026-07-01T06:00:00Z")), None);
    }

    #[test]
    fn inside_a_gap_a_picture_is_placed_only_near_its_edges() {
        let fixes = walk();
        let at = |t: &str| position_at(&fixes, utc(t));
        // The middle of the ten-minute pause: nowhere.
        assert_eq!(at("2026-07-01T06:07:00Z"), None);
        assert_eq!(at("2026-07-01T06:04:01Z"), None);
        // Near its edges, as near the ends of the track.
        close(at("2026-07-01T06:03:00Z"), 10.002, 20.004, None);
        close(at("2026-07-01T06:11:00Z"), 10.012, 20.004, Some(130.));
        // Five minutes apart is not yet a gap.
        let short = [
            fix("2026-07-01T06:00:00Z", 10.0, 20.0, None),
            fix("2026-07-01T06:05:00Z", 10.005, 20.0, None),
        ];
        close(
            position_at(&short, utc("2026-07-01T06:02:30Z")),
            10.0025,
            20.0,
            None,
        );
    }

    #[test]
    fn a_track_over_the_date_line_is_followed_the_short_way() {
        let fixes = [
            fix("2026-07-01T06:00:00Z", 10.0, 179.9, None),
            fix("2026-07-01T06:01:00Z", 10.0, -179.9, None),
        ];
        let p = position_at(&fixes, utc("2026-07-01T06:00:15Z")).unwrap();
        assert!((p.lon - 179.95).abs() < 1e-9, "{p:?}");
        let p = position_at(&fixes, utc("2026-07-01T06:00:45Z")).unwrap();
        assert!((p.lon + 179.95).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn every_timed_point_of_every_track_counts_in_time_order() {
        let point = |time: Option<&str>, lat: f64, altitude: Option<f32>| Point {
            at: LatLon { lat, lon: 20.0 },
            altitude,
            time: time.map(utc),
        };
        let doc = GeoDoc {
            lines: vec![
                Line {
                    name: "Afternoon".into(),
                    kind: LineKind::Track,
                    datum: Datum::Sea,
                    segments: vec![
                        vec![point(Some("2026-07-01T12:00:00Z"), 10.2, Some(5.))],
                        vec![
                            point(Some("2026-07-01T13:00:00Z"), 10.3, None),
                            point(None, 10.35, None),
                        ],
                    ],
                },
                Line {
                    name: "Morning".into(),
                    kind: LineKind::Track,
                    datum: Datum::Sea,
                    segments: vec![vec![
                        point(Some("2026-07-01T08:00:00Z"), 10.0, None),
                        point(Some("2026-07-01T08:00:00Z"), 10.01, None),
                    ]],
                },
                Line {
                    name: "Flight".into(),
                    kind: LineKind::Track,
                    datum: Datum::Takeoff,
                    segments: vec![vec![point(Some("2026-07-01T09:00:00Z"), 10.1, Some(50.))]],
                },
                Line {
                    name: "Plan".into(),
                    kind: LineKind::Route,
                    datum: Datum::Sea,
                    segments: vec![vec![point(Some("2026-07-01T10:00:00Z"), 11.0, None)]],
                },
            ],
            ..GeoDoc::default()
        };
        let fixes = fixes(&doc);
        let lats: Vec<f64> = fixes.iter().map(|f| f.at.lat).collect();
        assert_eq!(lats, [10.0, 10.1, 10.2, 10.3]);
        // Above the takeoff is not above the sea.
        assert_eq!(fixes[1].altitude, None);
        assert_eq!(fixes[2].altitude, Some(5.));
    }

    fn clock(text: &str) -> Clock {
        parse_clock(text).unwrap()
    }

    #[test]
    fn a_camera_clock_is_read_in_the_zone_of_its_date() {
        let paris = TimeZone::get("Europe/Paris").unwrap();
        let at = |text: &str| instant(clock(text), &paris, 0).unwrap();
        assert_eq!(at("2026:01:15 08:30:00"), utc("2026-01-15T07:30:00Z"));
        assert_eq!(at("2026:07:01 08:30:00"), utc("2026-07-01T06:30:00Z"));
        // The night the clocks go forward: an hour on the clock either side
        // of it is two, in instants one.
        assert_eq!(at("2026:03:29 01:30:00"), utc("2026-03-29T00:30:00Z"));
        assert_eq!(at("2026:03:29 03:30:00"), utc("2026-03-29T01:30:00Z"));
        // A fixed offset, far from the user's.
        let west = TimeZone::fixed(Offset::from_hours(-5).unwrap());
        assert_eq!(
            instant(clock("2026:07:01 08:30:00"), &west, 0),
            Some(utc("2026-07-01T13:30:00Z"))
        );
    }

    #[test]
    fn a_zone_the_picture_states_wins_and_the_offset_corrects_the_camera() {
        let paris = TimeZone::get("Europe/Paris").unwrap();
        let stated = clock("2026:07:01 08:30:00+09:00");
        assert_eq!(
            instant(stated, &paris, 0),
            Some(utc("2026-06-30T23:30:00Z"))
        );
        let slow = clock("2026:07:01 08:30:00");
        assert_eq!(
            instant(slow, &paris, 120),
            Some(utc("2026-07-01T06:32:00Z"))
        );
        assert_eq!(
            instant(slow, &paris, -3600),
            Some(utc("2026-07-01T05:30:00Z"))
        );
        // The track in UTC, a point a minute for two hours, and a picture
        // in Paris on the night the clocks go forward: 03:30 there is
        // 01:30 UTC, ninety minutes into the track.
        let fixes: Vec<Fix> = (0..=120u32)
            .map(|m| Fix {
                time: utc("2026-03-29T00:00:00Z") + Duration::from_secs(u64::from(m) * 60),
                at: LatLon {
                    lat: 10.0 + 0.001 * f64::from(m),
                    lon: 20.0,
                },
                altitude: None,
            })
            .collect();
        let t = instant(clock("2026:03:29 03:30:00"), &paris, 0).unwrap();
        close(position_at(&fixes, t), 10.09, 20.0, None);
    }

    #[test]
    fn camera_dates_are_read_as_exif_and_xmp_write_them() {
        let c = clock("2026:07:01 08:30:00");
        assert_eq!(c.local, jiff::civil::date(2026, 7, 1).at(8, 30, 0, 0));
        assert_eq!(c.stated, None);
        assert_eq!(
            clock("2026:07:01 08:30:00.25+02:00").stated,
            Some(Offset::from_hours(2).unwrap())
        );
        assert_eq!(
            clock("2026:07:01 08:30:00-05:30").stated,
            Some(Offset::from_seconds(-(5 * 3600 + 30 * 60)).unwrap())
        );
        assert_eq!(clock("2026:07:01 08:30:00Z").stated, Some(Offset::UTC));
        assert_eq!(parse_clock("0000:00:00 00:00:00"), None);
        assert_eq!(parse_clock("    :  :     :  :  "), None);
        assert_eq!(parse_clock("2026:07:01"), None);
    }

    fn tags(pairs: &[(&str, &str)]) -> Tags {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), serde_json::Value::from(*v)))
            .collect()
    }

    #[test]
    fn a_position_is_read_from_exif_first_then_from_xmp() {
        let exif = tags(&[
            ("GPS:GPSLatitude", "10.5"),
            ("GPS:GPSLatitudeRef", "S"),
            ("GPS:GPSLongitude", "20.25"),
            ("GPS:GPSLongitudeRef", "W"),
            ("GPS:GPSAltitude", "12"),
            ("GPS:GPSAltitudeRef", "1"),
            ("XMP-exif:GPSLatitude", "1"),
            ("XMP-exif:GPSLongitude", "2"),
        ]);
        close(position_of(&exif), -10.5, -20.25, Some(-12.));
        let xmp = tags(&[
            ("XMP-exif:GPSLatitude", "-10.5"),
            ("XMP-exif:GPSLongitude", "20.25"),
        ]);
        close(position_of(&xmp), -10.5, 20.25, None);
        assert_eq!(
            position_of(&tags(&[("GPS:GPSTimeStamp", "06:30:00")])),
            None
        );
        assert!(is_location_tag("XMP-exif:GPSDestLatitude"));
        assert!(!is_location_tag("GPS:GPSVersionID"));
        assert!(is_location_tag("XMP-drone-dji:GPSLongtitude"));
        assert!(is_location_tag("Keys:GPSCoordinates"));
        assert!(is_location_tag("XMP-drone-dji:AbsoluteAltitude"));
        assert!(is_location_tag("XMP-iptcExt:LocationShown"));
        assert!(is_location_tag("IPTC:Country-PrimaryLocationCode"));
        assert!(!is_location_tag("XMP-drone-dji:FlightYawDegree"));
        assert!(!is_location_tag("IPTC:ApplicationRecordVersion"));
        assert!(!is_location_tag("Composite:GPSPosition"));
        assert!(!is_location_tag("ExifIFD:DateTimeOriginal"));
        assert!(!is_location_tag("XMP-exif:DateTimeOriginal"));
    }

    // ------------------------------------------------------------ ExifTool

    /// A 1×1 grey baseline JPEG: start, JFIF header, one quantization
    /// table, the frame, a Huffman table for each coefficient class, the
    /// scan with its one byte of data, end.
    fn tiny_jpeg() -> Vec<u8> {
        let mut out = vec![0xff, 0xd8];
        let mut segment = |marker: u8, body: &[u8]| {
            out.extend([0xff, marker]);
            out.extend(u16::try_from(body.len() + 2).unwrap().to_be_bytes());
            out.extend(body);
        };
        segment(0xe0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        let mut quantization = vec![0];
        quantization.extend([1u8; 64]);
        segment(0xdb, &quantization);
        segment(0xc0, &[8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
        let huffman = |class: u8| {
            let mut table = vec![class, 1];
            table.extend([0u8; 15]);
            table.push(0);
            table
        };
        segment(0xc4, &huffman(0x00));
        segment(0xc4, &huffman(0x10));
        segment(0xda, &[1, 1, 0, 0, 0x3f, 0]);
        out.extend([0x3f, 0xff, 0xd9]);
        out
    }

    /// `ExifTool`, or a note that the test is skipped.
    fn tool() -> Option<PathBuf> {
        let found = program().ok();
        if found.is_none() {
            eprintln!("exiftool is not on PATH: skipped");
        }
        found
    }

    /// Under `~/.cache/bibli-tests`: a real filesystem, with nanosecond
    /// times and extended attributes.
    fn scratch() -> tempfile::TempDir {
        let base = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache/bibli-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("geotag-")
            .tempdir_in(base)
            .unwrap()
    }

    /// Run `ExifTool` to prepare a fixture.
    fn prepare(program: &Path, path: &Path, assignments: &[&str]) {
        let mut command = exiftool(program);
        command
            .args(["-overwrite_original"])
            .args(assignments)
            .arg("--")
            .arg(path);
        bibli_platform::process::run(&mut command, &CancelToken::new(), WRITE_TIMEOUT).unwrap();
    }

    fn picture(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, tiny_jpeg()).unwrap();
        path
    }

    fn set_mtime(path: &Path, t: SystemTime) {
        std::fs::File::open(path).unwrap().set_modified(t).unwrap();
    }

    fn mtime(path: &Path) -> SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    fn planned(path: &Path, before: Location) -> Planned {
        let meta = std::fs::metadata(path).unwrap();
        Planned {
            size: meta.len(),
            modified: meta.modified().unwrap(),
            before,
        }
    }

    #[test]
    fn a_position_is_written_kept_to_the_nanosecond_and_undone() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        // A name ExifTool would read as an option, were it not after `--`.
        let path = picture(dir.path(), "-n l'été ✓.jpg");
        let when = utc("2026-07-01T06:30:00.123456789Z");
        set_mtime(&path, when);
        let entry = EntryRef::from_path(&path).unwrap();
        let ctx = StepContext::detached();
        let here = Position {
            lat: 10.123_456,
            lon: 20.654_321,
            altitude: Some(123.4),
        };
        let step = SetPosition {
            entry: entry.clone(),
            to: Location::At(here),
            planned: Some(planned(&path, Location::None)),
        };
        // It travels as a plan does.
        let data: SetPosition = serde_json::from_value(step.args()).unwrap();
        assert_eq!(data, step);
        assert_eq!(step.check(&ctx), StepStatus::Pending { reversible: true });
        let undo = step.apply(&ctx).unwrap().unwrap();

        assert_eq!(mtime(&path), when, "the modification time is kept");
        assert_eq!(
            EntryRef::from_path(&path).unwrap().id,
            entry.id,
            "rewritten in place"
        );
        let taken = bibli_platform::exif::taken(&path).at.unwrap();
        assert!((taken.lat - 10.123_456).abs() < 1e-6, "{taken:?}");
        assert!((taken.lon - 20.654_321).abs() < 1e-6, "{taken:?}");
        let read = read_tags(&program, &path, &CancelToken::new()).unwrap();
        close(position_of(&read), 10.123_456, 20.654_321, Some(123.4));

        // Applied again: nothing to do.
        assert_eq!(step.check(&ctx), StepStatus::AlreadyDone);
        // A plan made before the write, for somewhere else: refused.
        let stale = SetPosition {
            to: Location::At(Position {
                lat: 10.5,
                lon: 20.5,
                altitude: None,
            }),
            ..step.clone()
        };
        assert!(
            matches!(stale.check(&ctx), StepStatus::Conflict { reason } if reason.contains("changed since")),
        );

        // Undo: no location again, the time still kept.
        assert_eq!(undo.check(&ctx), StepStatus::Pending { reversible: true });
        let redo = undo.apply(&ctx).unwrap().unwrap();
        assert_eq!(bibli_platform::exif::taken(&path).at, None);
        assert!(
            read_tags(&program, &path, &CancelToken::new())
                .unwrap()
                .is_empty()
        );
        assert_eq!(mtime(&path), when);
        assert_eq!(undo.check(&ctx), StepStatus::AlreadyDone);
        // Redo: placed again.
        assert_eq!(redo.check(&ctx), StepStatus::Pending { reversible: true });
        redo.apply(&ctx).unwrap().unwrap();
        assert_eq!(redo.check(&ctx), StepStatus::AlreadyDone);
        assert_eq!(mtime(&path), when);
    }

    #[test]
    fn removing_a_location_takes_every_tag_and_undo_puts_them_back() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        let path = picture(dir.path(), "camera.jpg");
        prepare(
            &program,
            &path,
            &[
                "-GPS:GPSLatitude=10.25",
                "-GPS:GPSLatitudeRef=N",
                "-GPS:GPSLongitude=20.75",
                "-GPS:GPSLongitudeRef=E",
                "-GPS:GPSAltitude=5.5",
                "-GPS:GPSAltitudeRef=1",
                "-GPS:GPSTimeStamp=06:30:00",
                "-GPS:GPSDateStamp=2026:07:01",
                "-GPS:GPSImgDirection=123.5",
                "-XMP-exif:GPSLatitude=10.25",
                "-XMP-exif:GPSLongitude=20.75",
            ],
        );
        let cancel = CancelToken::new();
        let before = read_tags(&program, &path, &cancel).unwrap();
        assert_eq!(before.len(), 11, "{before:?}");
        close(position_of(&before), 10.25, 20.75, Some(-5.5));

        let ctx = StepContext::detached();
        let remove = SetPosition {
            entry: EntryRef::from_path(&path).unwrap(),
            to: Location::None,
            planned: None,
        };
        assert_eq!(remove.check(&ctx), StepStatus::Pending { reversible: true });
        let undo = remove.apply(&ctx).unwrap().unwrap();
        assert!(read_tags(&program, &path, &cancel).unwrap().is_empty());
        assert_eq!(bibli_platform::exif::taken(&path).at, None);
        assert_eq!(remove.check(&ctx), StepStatus::AlreadyDone);

        undo.apply(&ctx).unwrap().unwrap();
        assert_eq!(read_tags(&program, &path, &cancel).unwrap(), before);
        assert_eq!(undo.check(&ctx), StepStatus::AlreadyDone);
    }

    /// Every tag of the file that names GPS, in any group, as `ExifTool`
    /// lists them all.
    fn every_gps_tag(program: &Path, path: &Path) -> Vec<String> {
        let mut command = exiftool(program);
        command
            .args(["-j", "-a", "-G1", "-all"])
            .arg("--")
            .arg(path);
        let out =
            bibli_platform::process::run(&mut command, &CancelToken::new(), READ_TIMEOUT).unwrap();
        parse(&out)
            .into_iter()
            .flat_map(|object| object.into_iter().map(|(key, _)| key))
            .filter(|key| key.to_ascii_lowercase().contains("gps"))
            .collect()
    }

    #[test]
    fn a_drone_s_own_location_is_removed_too_and_restored_by_undo() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        let path = picture(dir.path(), "DJI_0001 (copy).jpg");
        prepare(
            &program,
            &path,
            &[
                "-XMP-drone-dji:GpsLatitude=10.25",
                "-XMP-drone-dji:GpsLongitude=20.75",
                "-XMP-drone-dji:AbsoluteAltitude=+123.4",
                "-GPS:GPSLatitude=10.25",
                "-GPS:GPSLatitudeRef=N",
                "-GPS:GPSLongitude=20.75",
                "-GPS:GPSLongitudeRef=E",
            ],
        );
        let cancel = CancelToken::new();
        let before = read_tags(&program, &path, &cancel).unwrap();
        assert!(
            before.contains_key("XMP-drone-dji:GPSLatitude")
                && before.contains_key("XMP-drone-dji:GPSLongitude"),
            "{before:?}"
        );
        assert!(has_location(&before));

        let ctx = StepContext::detached();
        let remove = SetPosition {
            entry: EntryRef::from_path(&path).unwrap(),
            to: Location::None,
            planned: None,
        };
        let undo = remove.apply(&ctx).unwrap().unwrap();
        assert_eq!(every_gps_tag(&program, &path), Vec::<String>::new());
        assert_eq!(remove.check(&ctx), StepStatus::AlreadyDone);

        undo.apply(&ctx).unwrap().unwrap();
        assert_eq!(read_tags(&program, &path, &cancel).unwrap(), before);
        let back = every_gps_tag(&program, &path);
        assert!(
            back.iter().any(|k| k == "XMP-drone-dji:GPSLatitude"),
            "{back:?}"
        );
    }

    /// Everything `ExifTool` lists of the file, structures as objects.
    fn listing(program: &Path, path: &Path) -> serde_json::Map<String, serde_json::Value> {
        let mut command = exiftool(program);
        command
            .args(["-j", "-a", "-G1", "-struct", "-all"])
            .arg("--")
            .arg(path);
        let out =
            bibli_platform::process::run(&mut command, &CancelToken::new(), READ_TIMEOUT).unwrap();
        parse(&out).pop().unwrap()
    }

    /// The groups a location lives in, as the listing has them.
    fn location_groups(
        listing: &serde_json::Map<String, serde_json::Value>,
    ) -> BTreeMap<String, serde_json::Value> {
        listing
            .iter()
            .filter(|(key, _)| {
                [
                    "GPS:",
                    "IPTC:",
                    "XMP-photoshop:",
                    "XMP-iptc",
                    "XMP-drone-dji:",
                    "XMP-exif:",
                ]
                .iter()
                .any(|group| key.starts_with(group))
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    const PLACED_BY_A_DRONE: &[&str] = &[
        "-GPS:GPSLatitude=10.25",
        "-GPS:GPSLatitudeRef=N",
        "-GPS:GPSLongitude=20.75",
        "-GPS:GPSLongitudeRef=E",
        "-XMP-drone-dji:GpsLatitude=10.25",
        "-XMP-drone-dji:GpsLongitude=20.75",
        "-XMP-drone-dji:AbsoluteAltitude=+123.4",
        "-XMP-drone-dji:RelativeAltitude=+50.2",
        "-XMP-drone-dji:FlightYawDegree=+12.5",
        "-XMP-drone-dji:GimbalPitchDegree=-30.0",
        "-IPTC:City=Example Town",
        "-IPTC:Country-PrimaryLocationName=Exampleland",
        "-XMP-photoshop:City=Example Town",
        "-XMP-iptcExt:LocationCreated={GPSLatitude=10.25,GPSLongitude=20.75,City=Example Town}",
    ];

    #[test]
    fn removing_takes_heights_and_place_names_too_and_undo_restores_structures() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        let path = picture(dir.path(), "DJI_0002.jpg");
        prepare(&program, &path, PLACED_BY_A_DRONE);
        let original = location_groups(&listing(&program, &path));
        assert!(
            original.contains_key("XMP-iptcExt:LocationCreated")
                && original.contains_key("IPTC:City"),
            "{original:?}"
        );

        let ctx = StepContext::detached();
        let remove = SetPosition {
            entry: EntryRef::from_path(&path).unwrap(),
            to: Location::None,
            planned: None,
        };
        let undo = remove.apply(&ctx).unwrap().unwrap();
        let after = listing(&program, &path);
        let left: Vec<&String> = after
            .keys()
            .filter(|key| {
                key.to_ascii_lowercase().contains("gps")
                    || [
                        "XMP-drone-dji:AbsoluteAltitude",
                        "XMP-drone-dji:RelativeAltitude",
                        "IPTC:City",
                        "IPTC:Country-PrimaryLocationName",
                        "XMP-photoshop:City",
                        "XMP-iptcExt:LocationCreated",
                    ]
                    .contains(&key.as_str())
            })
            .collect();
        assert!(left.is_empty(), "{left:?}");
        // Where the drone looked is not where it was.
        assert_eq!(after["XMP-drone-dji:FlightYawDegree"], "+12.5");
        assert_eq!(
            after["XMP-drone-dji:GimbalPitchDegree"],
            serde_json::json!(-30.0)
        );
        assert_eq!(remove.check(&ctx), StepStatus::AlreadyDone);

        let redo = undo.apply(&ctx).unwrap().unwrap();
        assert_eq!(location_groups(&listing(&program, &path)), original);
        assert_eq!(undo.check(&ctx), StepStatus::AlreadyDone);

        redo.apply(&ctx).unwrap().unwrap();
        let again = listing(&program, &path);
        assert!(
            !again
                .keys()
                .any(|key| key.to_ascii_lowercase().contains("gps")
                    || key == "IPTC:City"
                    || key == "XMP-iptcExt:LocationCreated"),
            "{again:?}"
        );
    }

    #[test]
    fn a_location_longer_than_a_pipe_holds_is_restored_whole() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        let path = picture(dir.path(), "many places.jpg");
        let shown: Vec<String> = (0..60)
            .map(|i| {
                format!(
                    "-XMP-iptcExt:LocationShown+={{City=Example Town {i},CountryName=Exampleland,GPSLatitude=10.{i:02},GPSLongitude=20.{i:02}}}"
                )
            })
            .collect();
        let shown: Vec<&str> = shown.iter().map(String::as_str).collect();
        prepare(&program, &path, &shown);
        let cancel = CancelToken::new();
        let before = read_tags(&program, &path, &cancel).unwrap();
        assert!(
            serde_json::to_vec(&before).unwrap().len() > 4096,
            "the fixture must not fit a pipe's buffer"
        );

        let ctx = StepContext::detached();
        let remove = SetPosition {
            entry: EntryRef::from_path(&path).unwrap(),
            to: Location::None,
            planned: None,
        };
        let undo = remove.apply(&ctx).unwrap().unwrap();
        assert!(read_tags(&program, &path, &cancel).unwrap().is_empty());
        undo.apply(&ctx).unwrap().unwrap();
        assert_eq!(read_tags(&program, &path, &cancel).unwrap(), before);
        // Its directory went with it.
        let ours = format!("bibli-geotag-{}-", std::process::id());
        let leftover = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().starts_with(&ours));
        assert!(!leftover);
    }

    #[test]
    fn a_place_name_alone_does_not_place_a_picture_and_geotagging_keeps_it() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        std::fs::write(dir.path().join("Walk.gpx"), TRACK).unwrap();
        for (name, extra) in [
            ("Named.jpg", "-IPTC:City=Example Town"),
            (
                "Structured.jpg",
                "-XMP-iptcExt:LocationCreated={GPSLatitude=10.25,GPSLongitude=20.75}",
            ),
        ] {
            let path = picture(dir.path(), name);
            prepare(
                &program,
                &path,
                &["-DateTimeOriginal=2026:07:01 08:02:00", extra],
            );
        }
        let paris = TimeZone::get("Europe/Paris").unwrap();
        let Geotag::Plan(plan) =
            plan_geotag(&refs(dir.path()), &paris, 0, &CancelToken::new()).unwrap()
        else {
            panic!("a plan")
        };
        assert_eq!(plan.title, "place “Named.jpg” along “Walk.gpx”");
        assert_eq!(
            plan.note.as_deref(),
            Some("1 picture already placed, left as it is")
        );
        let result = bibli_core::op::Executor::new(bibli_core::scheduler::Scheduler::new(1))
            .execute_blocking(&plan, &CancelToken::new());
        assert_eq!((result.applied, result.error), (1, None));
        let named = listing(&program, &dir.path().join("Named.jpg"));
        assert_eq!(named["IPTC:City"], "Example Town");
        assert!(named.contains_key("GPS:GPSLatitude"), "{named:?}");
    }

    const TRACK: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="Example Logger" xmlns="http://www.topografix.com/GPX/1/1">
  <trk><name>Walk</name><trkseg>
    <trkpt lat="10.000" lon="20.000"><ele>100</ele><time>2026-07-01T06:00:00Z</time></trkpt>
    <trkpt lat="10.010" lon="20.020"><ele>120</ele><time>2026-07-01T06:04:00Z</time></trkpt>
  </trkseg></trk>
</gpx>"#;

    /// A folder with a track, pictures taken during it and outside it,
    /// one already placed, a RAW one and a text file.
    fn trip(program: &Path, dir: &Path) {
        std::fs::write(dir.join("Walk (day 1).gpx"), TRACK).unwrap();
        for (name, taken) in [
            // In Paris, summer: two hours ahead of UTC.
            ("During.jpg", "2026:07:01 08:02:00"),
            ("Also during.jpg", "2026:07:01 08:01:00"),
            ("Later.jpg", "2026:07:01 09:00:00"),
            ("Placed.jpg", "2026:07:01 08:02:00"),
        ] {
            let path = picture(dir, name);
            prepare(program, &path, &[&format!("-DateTimeOriginal={taken}")]);
        }
        prepare(
            program,
            &dir.join("Placed.jpg"),
            &[
                "-GPS:GPSLatitude=10.5",
                "-GPS:GPSLatitudeRef=N",
                "-GPS:GPSLongitude=20.5",
                "-GPS:GPSLongitudeRef=E",
            ],
        );
        std::fs::write(dir.join("Raw.cr2"), b"II*\0not really a raw").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a mission").unwrap();
    }

    fn refs(dir: &Path) -> Vec<EntryRef> {
        let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        names.sort();
        names
            .into_iter()
            .map(|p| EntryRef::from_path(p).unwrap())
            .collect()
    }

    #[test]
    fn pictures_taken_during_a_track_are_proposed_and_the_rest_named() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        trip(&program, dir.path());
        let paris = TimeZone::get("Europe/Paris").unwrap();
        let cancel = CancelToken::new();
        let Geotag::Plan(plan) = plan_geotag(&refs(dir.path()), &paris, 0, &cancel).unwrap() else {
            panic!("a plan")
        };
        assert_eq!(plan.len(), 2);
        assert_eq!(plan.title, "place 2 pictures along “Walk (day 1).gpx”");
        assert_eq!(
            plan.note.as_deref(),
            Some(
                "1 picture outside the track's times · 1 picture already placed, left as it is · 1 RAW picture left out: their sidecars are not written yet · 1 other file left out"
            )
        );
        let during = plan
            .steps
            .iter()
            .map(|s| serde_json::from_value::<SetPosition>(s.args()).unwrap())
            .find(|s| s.entry.path.ends_with("During.jpg"))
            .unwrap();
        let Location::At(p) = during.to else {
            panic!("a position")
        };
        close(Some(p), 10.005, 20.01, Some(110.));
        assert_eq!(during.planned.unwrap().before, Location::None);
        // The review's dry run: every step pending, read from what
        // planning read.
        let diff = plan.diff(&std::sync::Arc::default());
        assert!(
            diff.steps
                .iter()
                .all(|(_, s)| *s == StepStatus::Pending { reversible: true }),
            "{diff:?}"
        );

        // An hour off: nothing matches, and the notice says why.
        let Geotag::Nothing(why) = plan_geotag(&refs(dir.path()), &paris, 7200, &cancel).unwrap()
        else {
            panic!("nothing to place")
        };
        assert!(
            why.starts_with("No picture to place along “Walk (day 1).gpx”: 3 pictures outside"),
            "{why}"
        );
        assert!(
            why.ends_with("cameras' clocks corrected by +7200 s"),
            "{why}"
        );
    }

    #[test]
    fn a_selection_without_one_track_is_refused_with_the_reason() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        trip(&program, dir.path());
        std::fs::write(dir.path().join("Other.gpx"), TRACK).unwrap();
        let paris = TimeZone::get("Europe/Paris").unwrap();
        let cancel = CancelToken::new();
        let error = plan_geotag(&refs(dir.path()), &paris, 0, &cancel).unwrap_err();
        assert_eq!(error.to_string(), "Select one track: 2 are selected");
        let pictures: Vec<EntryRef> = refs(dir.path())
            .into_iter()
            .filter(|r| r.path.extension().is_some_and(|e| e == "jpg"))
            .collect();
        let error = plan_geotag(&pictures, &paris, 0, &cancel).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Select a track and the pictures to place along it"
        );
    }

    fn app(dir: &Path) -> std::sync::Arc<App> {
        let app = App::new(AppConfig::ephemeral(dir.join(".state"))).unwrap();
        register(&app, bibli_core::CancelToken::new());
        app
    }

    fn select(app: &App, dir: &Path, names: &[&str]) -> PaneId {
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Place::from_path(dir)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": names, "pane": pane }),
        ))
        .unwrap();
        pane
    }

    #[test]
    fn the_geotag_command_proposes_its_plan_from_a_worker() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        trip(&program, dir.path());
        let app = app(dir.path());
        let pane = select(
            &app,
            dir.path(),
            &["Walk (day 1).gpx", "During.jpg", "Later.jpg"],
        );
        let out = app
            .dispatch(&Invocation::with_args(
                "geo.geotag",
                &serde_json::json!({ "pane": pane }),
            ))
            .unwrap();
        assert!(matches!(out, Outcome::Done));
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let plan = loop {
            if let Some(plan) = app.take_proposals().pop() {
                break plan;
            }
            let notes = app.take_notifications();
            assert!(notes.is_empty(), "{notes:?}");
            assert!(std::time::Instant::now() < deadline, "no proposal");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(plan.len(), 1);
        assert!(
            plan.title.starts_with("place “During.jpg” along"),
            "{}",
            plan.title
        );
        // Rebuilt from its data, as a reviewed plan may be.
        let rebuilt = Plan::from_data(&plan.to_data(), &app.ops).unwrap();
        let result = app.executor.execute_blocking(&rebuilt, &CancelToken::new());
        assert_eq!((result.applied, result.error), (1, None));
        assert!(
            bibli_platform::exif::taken(&dir.path().join("During.jpg"))
                .at
                .is_some()
        );

        // A selection without a track is refused at once.
        let pane = select(&app, dir.path(), &["During.jpg"]);
        let error = app
            .dispatch(&Invocation::with_args(
                "geo.geotag",
                &serde_json::json!({ "pane": pane }),
            ))
            .unwrap_err();
        assert!(format!("{error:#}").contains("Select a track"), "{error:#}");
    }

    #[test]
    fn the_remove_command_returns_a_plan_that_skips_pictures_without_a_location() {
        let Some(program) = tool() else { return };
        let dir = scratch();
        trip(&program, dir.path());
        let app = app(dir.path());
        let pane = select(&app, dir.path(), &["Placed.jpg", "During.jpg", "Raw.cr2"]);
        let Outcome::Plan(plan) = app
            .dispatch(&Invocation::with_args(
                "geo.remove_location",
                &serde_json::json!({ "pane": pane }),
            ))
            .unwrap()
        else {
            panic!("a plan")
        };
        assert_eq!(plan.len(), 2);
        assert_eq!(plan.title, "remove the location of 2 pictures");
        assert_eq!(
            plan.note.as_deref(),
            Some("1 RAW picture left out: their sidecars are not written yet")
        );
        // Each step says what it will do and what it did.
        let first = plan.diff(&std::sync::Arc::default()).steps[0].0.clone();
        let bibli_core::op::Change::Metadata {
            words: Some(words), ..
        } = first
        else {
            panic!("{first:?}");
        };
        assert!(
            words.will.starts_with("remove the location of “"),
            "{}",
            words.will
        );
        assert!(
            words.did.starts_with("removed the location of “"),
            "{}",
            words.did
        );
        let result = app.executor.execute_blocking(&plan, &CancelToken::new());
        assert_eq!((result.applied, result.skipped, result.error), (1, 1, None));
        assert!(result.undoable);
        assert_eq!(
            bibli_platform::exif::taken(&dir.path().join("Placed.jpg")).at,
            None
        );
        // Undo puts it back.
        app.executor.undo_blocking(&CancelToken::new()).unwrap();
        let back = bibli_platform::exif::taken(&dir.path().join("Placed.jpg"))
            .at
            .unwrap();
        assert!((back.lat - 10.5).abs() < 1e-9 && (back.lon - 20.5).abs() < 1e-9);

        let pane = select(&app, dir.path(), &["notes.txt"]);
        let error = app
            .dispatch(&Invocation::with_args(
                "geo.remove_location",
                &serde_json::json!({ "pane": pane }),
            ))
            .unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "No picture selected: 1 other file left out"
        );
    }
}
