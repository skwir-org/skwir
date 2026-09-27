//! IGC, the flight recorders' log of gliders, hang gliders and
//! paragliders: text records of a letter each. `A` names the recorder's
//! maker, `H` headers give the date (`HFDTE`), the pilot and the glider,
//! and a `B` record is a fix: `BHHMMSSDDMMmmmNDDDMMmmmEVPPPPPGGGGG`, the
//! time (UTC), latitude and longitude in degrees and thousandths of
//! minutes, validity (`A` a 3D fix, `V` not), then the pressure altitude
//! and the GNSS altitude in metres. Anything after that is an extension
//! (`I` records say which), not read here, and the other records (tasks,
//! events, the security signature) are skipped.
//!
//! A `B` record holds only a time of day: a flight that goes on past
//! midnight UTC (a long flight far east or west) moves to the next day
//! when its time goes back.
use std::io::BufRead;
use std::time::{Duration, SystemTime};

use bibli_core::CancelToken;
use bibli_core::geo::{Datum, LatLon, LineKind, Point};

use crate::collect::{self, Collector};
use crate::text;
use crate::xml::Damaged;

const DAY: u32 = 24 * 60 * 60;

/// Where the reading is.
#[derive(Default)]
struct State {
    /// The flight's date, from its `HFDTE` header.
    midnight: Option<SystemTime>,
    /// Days since that date, and the last fix's time of day.
    days: u32,
    last: Option<u32>,
    glider: Option<String>,
    /// Whether the flight's line was started.
    started: bool,
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut state = State::default();
    let mut recorder = None;
    let damaged = text::lines(input, cancel, text::Kind::Log, |_, line| {
        match line.as_bytes().first() {
            Some(b'A') if recorder.is_none() => {
                // The maker's three-letter code, then the recorder's serial.
                recorder = line.get(1..4).map(str::to_owned);
            }
            Some(b'H') => header(line, &mut state, into),
            Some(b'B') => fix(line, &mut state, into),
            _ => {}
        }
        Ok(())
    })?;
    if into.creator.is_none() {
        into.creator = recorder.filter(|code| code.chars().all(|c| c.is_ascii_alphanumeric()));
    }
    Ok(damaged)
}

/// A header record: `H`, its source (`F` the recorder, `O` an observer,
/// `P` the pilot), a three-letter subject, then its value, after a colon
/// in the current format.
fn header(line: &str, state: &mut State, into: &mut Collector) {
    let Some(subject) = line.get(2..5) else {
        return;
    };
    let rest = &line[5..];
    let value = rest.split_once(':').map(|(_, value)| value.trim());
    match subject {
        // `HFDTE150726` (before 2016) or `HFDTEDATE:150726,01`.
        "DTE" => {
            let date = value.unwrap_or(rest).get(..6).and_then(date);
            if date.is_some() {
                state.midnight = date;
                state.days = 0;
                state.last = None;
            }
        }
        "PLT" => {
            if let Some(pilot) = value.filter(|v| known(v)) {
                into.name = Some(pilot.to_owned());
            }
        }
        "GTY" => {
            if let Some(glider) = value.filter(|v| known(v)) {
                if state.started {
                    into.name_line(glider);
                }
                state.glider = Some(glider.to_owned());
            }
        }
        // The recorder's maker and model: `HFFTYFRTYPE:Maker,Model`.
        "FTY" => {
            if let Some(kind) = value.filter(|v| known(v)) {
                into.creator = Some(kind.replace(',', " "));
            }
        }
        _ => {}
    }
}

/// Whether a header's value says something: recorders write `NIL` or
/// `NKN` (not known) for what the pilot did not set.
fn known(value: &str) -> bool {
    !value.is_empty() && !["NIL", "NKN", "NOT SET"].contains(&value.to_ascii_uppercase().as_str())
}

/// Midnight UTC of `DDMMYY`. Two-digit years are this century's: IGC
/// began in the 1990s, and those files are rare.
fn date(text: &str) -> Option<SystemTime> {
    let day = digits(text.get(0..2)?)?;
    let month = digits(text.get(2..4)?)?;
    let year = digits(text.get(4..6)?)?;
    let year = if year < 80 { 2000 + year } else { 1900 + year };
    let date = jiff::civil::Date::new(
        i16::try_from(year).ok()?,
        i8::try_from(month).ok()?,
        i8::try_from(day).ok()?,
    )
    .ok()?;
    let zoned = date.to_zoned(jiff::tz::TimeZone::UTC).ok()?;
    Some(SystemTime::from(zoned.timestamp()))
}

/// A number written in ASCII digits only (no sign, no spaces).
fn digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// A `B` record, added to the flight's line.
fn fix(line: &str, state: &mut State, into: &mut Collector) {
    let Some((seconds, lat, lon, altitude)) = b_record(line) else {
        into.invalid();
        return;
    };
    // A recorder that has no fix yet writes zeros.
    if lat == 0. && lon == 0. {
        return;
    }
    let Some(at) = LatLon::new(lat, lon) else {
        into.invalid();
        return;
    };
    if let Some(last) = state.last
        && seconds + DAY / 2 < last
    {
        state.days += 1;
    }
    state.last = Some(seconds);
    let time = state.midnight.map(|midnight| {
        midnight + Duration::from_secs(u64::from(state.days) * u64::from(DAY) + u64::from(seconds))
    });
    if !state.started {
        state.started = true;
        into.line(
            state.glider.clone().unwrap_or_default(),
            LineKind::Track,
            Datum::Sea,
        );
    }
    into.point(Point { at, altitude, time });
}

/// A fix: its time of day in seconds, its latitude and longitude, and its
/// altitude. `None` when the record cannot be read.
fn b_record(line: &str) -> Option<(u32, f64, f64, Option<f32>)> {
    let time = line.get(1..7)?;
    let (hours, minutes, seconds) = (
        digits(time.get(0..2)?)?,
        digits(time.get(2..4)?)?,
        digits(time.get(4..6)?)?,
    );
    if hours >= 24 || minutes >= 60 || seconds >= 60 {
        return None;
    }
    let lat = coordinate(line.get(7..14)?, 2, line.get(14..15)?, 'N', 'S')?;
    let lon = coordinate(line.get(15..23)?, 3, line.get(23..24)?, 'E', 'W')?;
    let valid = line.get(24..25)? == "A";
    let pressure = line.get(25..30).and_then(|p| p.parse::<i32>().ok());
    let gnss = line.get(30..35).and_then(|g| g.parse::<i32>().ok());
    // A 2D fix's GNSS altitude is not measured; the barometer's still is.
    let altitude = gnss
        .filter(|&g| valid && g != 0)
        .or(pressure)
        .and_then(|metres| collect::altitude(&metres.to_string()));
    Some((hours * 3600 + minutes * 60 + seconds, lat, lon, altitude))
}

/// `DDMMmmm` (or `DDDMMmmm`) and its hemisphere, in signed degrees.
fn coordinate(
    text: &str,
    degree_digits: usize,
    side: &str,
    plus: char,
    minus: char,
) -> Option<f64> {
    let degrees = f64::from(digits(text.get(..degree_digits)?)?);
    let thousandths = digits(text.get(degree_digits..)?)?;
    if thousandths >= 60_000 {
        return None;
    }
    let value = degrees + f64::from(thousandths) / 60_000.;
    match side.chars().next()? {
        c if c == plus => Some(value),
        c if c == minus => Some(-value),
        _ => None,
    }
}

/// Whether the first bytes of a file look like IGC: an `A` record (a
/// maker's three-letter code) followed by headers, or a date header.
pub fn sniff(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next().unwrap_or_default();
    let a_record = first.starts_with('A')
        && first
            .get(1..4)
            .is_some_and(|code| code.bytes().all(|b| b.is_ascii_alphanumeric()))
        && lines.next().is_some_and(|l| l.starts_with('H'));
    a_record || text.lines().any(|l| l.trim_start().starts_with("HFDTE"))
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    const FLIGHT: &str = "AXXXABC Example recorder\r\n\
HFDTEDATE:120326,01\r\n\
HFPLTPILOTINCHARGE:Test Pilot\r\n\
HFGTYGLIDERTYPE:Example Wing\r\n\
HFFTYFRTYPE:Example,Logger 2\r\n\
I013638FXA\r\n\
\r\n\
B0800001000000N02000000EA0100001050000\r\n\
B0801001000600N02000000EA0101001060000\r\n\
LXXX a comment\r\n\
B0802001001200N02000000EV0102000000000\r\n\
B08030010?1200N02000000EA0102001060000\r\n\
B0804000000000N00000000EV0000000000000\r\n\
GABCDEF0123456789\r\n";

    fn seconds(time: Option<SystemTime>) -> u64 {
        time.unwrap()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn an_igc_flight_gives_its_fixes_with_their_date() {
        let mut c = Collector::new();
        assert!(
            read(FLIGHT.as_bytes(), &mut c, &CancelToken::new())
                .unwrap()
                .is_none()
        );
        let (doc, c) = c.finish();
        assert_eq!(c.name.as_deref(), Some("Test Pilot"));
        assert_eq!(c.creator.as_deref(), Some("Example Logger 2"));
        assert_eq!(doc.lines.len(), 1);
        let line = &doc.lines[0];
        assert_eq!(line.name, "Example Wing");
        assert_eq!(line.kind, LineKind::Track);
        assert_eq!(line.datum, Datum::Sea);
        // The damaged record is invalid; the one without a fix is skipped.
        assert_eq!(c.invalid, 1);
        let points = &line.segments[0];
        assert_eq!(points.len(), 3);
        assert_eq!((points[0].at.lat, points[0].at.lon), (10., 20.));
        assert_eq!(points[1].at.lat, 10. + 600. / 60_000.);
        // GNSS altitude when it has one, else the pressure altitude.
        assert_eq!(points[0].altitude, Some(1050.));
        assert_eq!(points[2].altitude, Some(1020.));
        // 2026-03-12T08:00:00Z.
        assert_eq!(seconds(points[0].time), 1_773_302_400);
        assert_eq!(seconds(points[2].time), 1_773_302_520);
    }

    #[test]
    fn a_flight_past_midnight_moves_to_the_next_day() {
        let flight = "AXXXABC\nHFDTE311226\n\
B2359501000000S02000000WA0010000100\n\
B0000101000600S02000000WA0010000100\n";
        let mut c = Collector::new();
        read(flight.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, c) = c.finish();
        let points = &doc.lines[0].segments[0];
        assert_eq!((points[0].at.lat, points[0].at.lon), (-10., -20.));
        assert_eq!(
            seconds(points[1].time) - seconds(points[0].time),
            20,
            "{points:?}"
        );
        // The recorder's code, with no type header.
        assert_eq!(c.creator.as_deref(), Some("XXX"));
    }

    #[test]
    fn fixes_before_any_date_have_no_time() {
        let flight = "B0800001000000N02000000EA0100001050\n";
        let mut c = Collector::new();
        read(flight.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, _) = c.finish();
        assert_eq!(doc.lines[0].segments[0][0].time, None);
    }

    #[test]
    fn a_line_too_long_is_skipped() {
        let long = format!(
            "L{}\nB0800001000000N02000000EA0100001050\n",
            "x".repeat(10_000)
        );
        let mut c = Collector::new();
        read(long.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, c) = c.finish();
        assert_eq!(doc.lines[0].segments[0].len(), 1);
        assert_eq!(c.invalid, 0);
    }

    #[test]
    fn igc_is_recognised_by_its_first_records() {
        assert!(sniff(FLIGHT.as_bytes()));
        assert!(sniff(b"HFDTE120326\r\nB08..."));
        assert!(!sniff(b"A plain text file\nwith lines"));
    }
}
