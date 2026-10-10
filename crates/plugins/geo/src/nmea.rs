//! NMEA 0183 logs: the sentences a GPS receiver writes, one a line, as a
//! logger or a phone saves them. Two carry a fix: `RMC` (position, status,
//! time of day and date) and `GGA` (position, fix quality, altitude above
//! mean sea level, time of day only). Any talker is read (`$GP` GPS, `$GN`
//! several systems, `$GL`, `$GA`, `$BD`…); the other sentences (satellites,
//! precision, `GLL` which repeats `RMC`) and proprietary ones are skipped.
//!
//! A receiver writes both for each fix, so a `GGA` and an `RMC` with the
//! same time of day are one point: the position of the first, the
//! altitude of the `GGA`, the date of the `RMC`. A `GGA` alone takes the
//! date of the latest `RMC`, the next day when its time went back past
//! midnight; before any date it has no time.
use std::io::BufRead;
use std::time::{Duration, SystemTime};

use jiff::civil::Date;
use skwir_core::CancelToken;
use skwir_core::geo::{Datum, LatLon, LineKind, Point};

use crate::collect::{self, Collector};
use crate::xml::Damaged;

const DAY_MS: u32 = 24 * 60 * 60 * 1000;

/// A fix read from a sentence, waiting for the other sentence of its
/// time.
struct Fix {
    /// Milliseconds since midnight UTC.
    time: Option<u32>,
    at: LatLon,
    altitude: Option<f32>,
    /// The date of an `RMC`.
    date: Option<Date>,
}

#[derive(Default)]
struct State {
    pending: Option<Fix>,
    /// The latest `RMC`'s date and time of day.
    dated: Option<(Date, u32)>,
    started: bool,
}

impl State {
    fn add(&mut self, fix: Fix, into: &mut Collector) {
        match self.pending.as_mut() {
            Some(pending) if pending.time.is_some() && pending.time == fix.time => {
                pending.altitude = pending.altitude.or(fix.altitude);
                pending.date = pending.date.or(fix.date);
            }
            _ => {
                self.flush(into);
                self.pending = Some(fix);
            }
        }
    }

    fn flush(&mut self, into: &mut Collector) {
        let Some(fix) = self.pending.take() else {
            return;
        };
        let time = fix.time.and_then(|ms| {
            let date = match (fix.date, self.dated) {
                // Past midnight since that date was given.
                (None, Some((date, at))) if ms + DAY_MS / 2 < at => date.tomorrow().ok()?,
                (Some(date), _) | (None, Some((date, _))) => date,
                (None, None) => return None,
            };
            let midnight = date.to_zoned(jiff::tz::TimeZone::UTC).ok()?;
            Some(SystemTime::from(midnight.timestamp()) + Duration::from_millis(u64::from(ms)))
        });
        if !self.started {
            self.started = true;
            into.line(String::new(), LineKind::Track, Datum::Sea);
        }
        into.point(Point {
            at: fix.at,
            altitude: fix.altitude,
            time,
        });
    }
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut state = State::default();
    let damaged = crate::text::lines(input, cancel, crate::text::Kind::Log, |_, line| {
        sentence(line, &mut state, into);
        Ok(())
    })?;
    state.flush(into);
    Ok(damaged)
}

/// What a fix sentence is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Rmc,
    Gga,
}

fn sentence(line: &str, state: &mut State, into: &mut Collector) {
    // Some loggers write a time or a source before the sentence.
    let Some(start) = line.find('$') else {
        return;
    };
    let body = &line[start + 1..];
    let (data, checksum) = match body.split_once('*') {
        Some((data, checksum)) => (data, Some(checksum)),
        None => (body, None),
    };
    let fields: Vec<&str> = data.split(',').collect();
    // `$` then a talker of two letters and a sentence of three; a
    // proprietary sentence starts with `P`.
    let address = fields[0];
    if address.len() != 5 || address.starts_with('P') {
        return;
    }
    let kind = match address.get(2..) {
        Some("RMC") => Kind::Rmc,
        Some("GGA") => Kind::Gga,
        _ => return,
    };
    if let Some(checksum) = checksum {
        let sum = data.bytes().fold(0u8, |sum, b| sum ^ b);
        let written = checksum
            .get(..2)
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if written != Some(sum) {
            into.invalid();
            return;
        }
    }
    let field = |i: usize| fields.get(i).copied().unwrap_or_default();
    let fix = match kind {
        Kind::Rmc => {
            // `V`: the receiver has no fix, and says so.
            if field(2) != "A" {
                return;
            }
            let Some(at) = position(field(3), field(4), field(5), field(6)) else {
                into.invalid();
                return;
            };
            Fix {
                time: time_of_day(field(1)),
                at,
                altitude: None,
                date: date(field(9)),
            }
        }
        Kind::Gga => {
            // Quality 0 is no fix.
            if matches!(field(6), "" | "0") {
                return;
            }
            let Some(at) = position(field(2), field(3), field(4), field(5)) else {
                into.invalid();
                return;
            };
            Fix {
                time: time_of_day(field(1)),
                at,
                altitude: (field(10) == "M")
                    .then(|| collect::altitude(field(9)))
                    .flatten(),
                date: None,
            }
        }
    };
    let dated = fix.date.zip(fix.time);
    state.add(fix, into);
    // After the fix it belongs to: a fix still waiting is dated by the
    // sentences before it.
    if dated.is_some() {
        state.dated = dated;
    }
}

/// `ddmm.mmmm,N` and `dddmm.mmmm,E`: degrees and minutes, then the
/// hemisphere.
fn position(lat: &str, north: &str, lon: &str, east: &str) -> Option<LatLon> {
    let lat = degrees(lat)?;
    let lon = degrees(lon)?;
    let lat = match north {
        "N" => lat,
        "S" => -lat,
        _ => return None,
    };
    let lon = match east {
        "E" => lon,
        "W" => -lon,
        _ => return None,
    };
    LatLon::new(lat, lon)
}

/// Degrees then two digits of whole minutes, then the minutes' fraction.
fn degrees(text: &str) -> Option<f64> {
    let whole = text.find('.').unwrap_or(text.len());
    let split = whole.checked_sub(2).filter(|&split| split > 0)?;
    let degrees = text.get(..split)?;
    if !degrees.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let minutes = text.get(split..)?;
    if minutes.starts_with(['-', '+']) {
        return None;
    }
    let minutes: f64 = minutes.parse().ok()?;
    if !(0. ..60.).contains(&minutes) {
        return None;
    }
    Some(f64::from(degrees.parse::<u16>().ok()?) + minutes / 60.)
}

/// `hhmmss` or `hhmmss.sss`, in milliseconds since midnight.
fn time_of_day(text: &str) -> Option<u32> {
    let number = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = text.get(range)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (hours, minutes, seconds) = (number(0..2)?, number(2..4)?, number(4..6)?);
    if hours >= 24 || minutes >= 60 || seconds >= 60 {
        return None;
    }
    let fraction = match text.get(6..)? {
        "" => 0,
        rest if rest.starts_with('.') => {
            let fraction: f64 = format!("0{rest}").parse().ok()?;
            (fraction * 1000.).round() as u32
        }
        _ => return None,
    };
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + fraction.min(999))
}

/// `ddmmyy`. Receivers write two-digit years; those before 1980, when GPS
/// time began, are this century's.
fn date(text: &str) -> Option<Date> {
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let day: i8 = text[0..2].parse().ok()?;
    let month: i8 = text[2..4].parse().ok()?;
    let year: i16 = text[4..6].parse().ok()?;
    let year = if year < 80 { 2000 + year } else { 1900 + year };
    Date::new(year, month, day).ok()
}

/// Whether the first bytes of a file look like an NMEA log: a line that
/// is a sentence, `$` then its address and a comma.
pub fn sniff(head: &[u8]) -> bool {
    head.split(|&b| b == b'\n').any(|line| {
        let line = line.trim_ascii_start();
        line.len() > 7
            && line[0] == b'$'
            && line[1..6]
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            && line[6] == b','
    })
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    /// A sentence with its checksum.
    fn nmea(data: &str) -> String {
        let sum = data.bytes().fold(0u8, |sum, b| sum ^ b);
        format!("${data}*{sum:02X}\r\n")
    }

    fn seconds(time: Option<SystemTime>) -> f64 {
        time.unwrap()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }

    /// 2026-03-12T08:00:00Z.
    const EIGHT: f64 = 1_773_302_400.;

    fn log() -> String {
        [
            // Before any date: no time.
            nmea("GPGGA,075959.00,1000.000,N,02000.000,E,1,08,0.9,95.0,M,0.0,M,,"),
            nmea("GPGSV,3,1,11,01,40,083,46,02,17,308,41,12,07,344,39,14,22,228,45"),
            // One point of the two, however they are ordered.
            nmea("GNGGA,080000.00,1000.000,N,02000.000,E,1,08,0.9,100.0,M,0.0,M,,"),
            nmea("GNRMC,080000.00,A,1000.000,N,02000.000,E,0.0,0.0,120326,,,A"),
            nmea("GNRMC,080001.50,A,1000.060,N,02000.000,E,0.0,0.0,120326,,,A"),
            nmea("GNGGA,080001.50,1000.060,N,02000.000,E,1,08,0.9,104.0,M,0.0,M,,"),
            // No fix.
            nmea("GNRMC,080002.00,V,,,,,,,120326,,,N"),
            nmea("GNGGA,080002.00,,,,,0,00,,,M,,M,,"),
            // A bad checksum, and a sentence that claims a place it lacks.
            "$GNRMC,080003.00,A,1000.120,N,02000.000,E,0.0,0.0,120326,,,A*00\r\n".into(),
            nmea("GNGGA,080004.00,10xx.000,N,02000.000,E,1,08,0.9,104.0,M,0.0,M,,"),
            "garbage\r\n\r\n".into(),
            // A GGA alone, dated by the RMC before it.
            nmea("GNGGA,080005.00,1000.120,S,02000.000,W,1,08,0.9,110.0,M,0.0,M,,"),
        ]
        .concat()
    }

    #[test]
    fn an_nmea_log_gives_one_point_per_fix() {
        let mut c = Collector::new();
        assert!(
            read(log().as_bytes(), &mut c, &CancelToken::new())
                .unwrap()
                .is_none()
        );
        let (doc, c) = c.finish();
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0].kind, LineKind::Track);
        let points = &doc.lines[0].segments[0];
        assert_eq!(points.len(), 4, "{points:?}");
        assert_eq!(c.invalid, 2);
        assert_eq!(points[0].time, None);
        assert_eq!(points[0].altitude, Some(95.));
        assert_eq!((points[1].at.lat, points[1].at.lon), (10., 20.));
        assert_eq!(points[1].altitude, Some(100.));
        assert_eq!(seconds(points[1].time), EIGHT);
        assert_eq!(points[2].at.lat, 10. + 0.06 / 60.);
        assert_eq!(points[2].altitude, Some(104.));
        assert_eq!(seconds(points[2].time), EIGHT + 1.5);
        assert_eq!(points[3].at.lat, -(10. + 0.12 / 60.));
        assert_eq!(points[3].at.lon, -20.);
        assert_eq!(seconds(points[3].time), EIGHT + 5.);
    }

    #[test]
    fn a_gga_past_midnight_takes_the_next_day() {
        let log = [
            nmea("GPRMC,235959.00,A,1000.000,N,02000.000,E,0.0,0.0,311226,,,A"),
            nmea("GPGGA,000001.00,1000.000,N,02000.000,E,1,08,0.9,100.0,M,0.0,M,,"),
        ]
        .concat();
        let mut c = Collector::new();
        read(log.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, _) = c.finish();
        let points = &doc.lines[0].segments[0];
        assert_eq!(seconds(points[1].time) - seconds(points[0].time), 2.);
    }

    #[test]
    fn sentences_without_a_checksum_are_read() {
        let log = "$GPRMC,080000,A,1000.000,N,02000.000,E,0.0,0.0,120326,,\n";
        let mut c = Collector::new();
        read(log.as_bytes(), &mut c, &CancelToken::new()).unwrap();
        let (doc, _) = c.finish();
        assert_eq!(seconds(doc.lines[0].segments[0][0].time), EIGHT);
    }

    #[test]
    fn nmea_is_recognised_by_its_sentences() {
        assert!(sniff(log().as_bytes()));
        assert!(!sniff(b"just $5, a price"));
    }
}
