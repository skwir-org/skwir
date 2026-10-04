//! The user's calendar: their time zone, daylight saving time included, and
//! the day their week starts on. Everything that turns a date a person reads
//! into instants goes through it (the query language's dates and periods,
//! the timeline's bars), so "today" is the user's today (ADR-017).
//!
//! Days are counted as civil days since 1970-01-01 (`i64`, negative before
//! it): a day number names a date, not an instant, and only
//! [`Calendar::day_start`] turns one into the instant it begins at in the
//! zone. A day is then 23, 24 or 25 hours long.
//!
//! The zone is the system's (`TZ`, else `/etc/localtime`, read by jiff,
//! which notices a change). The first day of the week is the locale's
//! (`LC_TIME`), as `locale week-1stday first_weekday` reports it: that is a
//! process, so [`prewarm`] asks on a worker and the answer is kept; until it
//! arrives, or where there is no `locale`, weeks start on Monday (ISO 8601).

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jiff::Timestamp;
pub use jiff::civil::Weekday;
pub use jiff::tz::TimeZone;

/// A time zone and the first day of the week.
#[derive(Clone, Debug)]
pub struct Calendar {
    tz: TimeZone,
    week_start: Weekday,
}

impl Calendar {
    /// The user's: the system's time zone and the locale's first weekday.
    pub fn system() -> Calendar {
        Calendar {
            tz: TimeZone::system(),
            week_start: week_start(),
        }
    }

    pub fn new(tz: TimeZone, week_start: Weekday) -> Calendar {
        Calendar { tz, week_start }
    }

    /// UTC with weeks from Monday: what tests use when the zone is not
    /// what they are about.
    pub fn utc() -> Calendar {
        Calendar::new(TimeZone::UTC, Weekday::Monday)
    }

    /// A zone of the time zone database by name (`Europe/Paris`).
    pub fn named(zone: &str, week_start: Weekday) -> Option<Calendar> {
        TimeZone::get(zone)
            .ok()
            .map(|tz| Calendar::new(tz, week_start))
    }

    pub fn time_zone(&self) -> &TimeZone {
        &self.tz
    }

    pub fn week_start(&self) -> Weekday {
        self.week_start
    }

    /// The civil day `t` falls on in the zone.
    pub fn day_of(&self, t: SystemTime) -> i64 {
        let Ok(ts) = Timestamp::try_from(t) else {
            // Beyond what a calendar holds: the nearest day it does.
            return if t < UNIX_EPOCH {
                days_from_civil(-9999, 1, 1)
            } else {
                days_from_civil(9999, 12, 31)
            };
        };
        let date = ts.to_zoned(self.tz.clone()).date();
        days_from_civil(
            i64::from(date.year()),
            u32::from(date.month().unsigned_abs()),
            u32::from(date.day().unsigned_abs()),
        )
    }

    /// The first instant of civil day `day` in the zone: its midnight, or
    /// the first instant after a daylight saving time gap that skips it.
    pub fn day_start(&self, day: i64) -> SystemTime {
        let (y, m, d) = civil_from_days(day);
        let zoned = i16::try_from(y)
            .ok()
            .and_then(|y| jiff::civil::Date::new(y, m as i8, d as i8).ok())
            .and_then(|date| date.to_zoned(self.tz.clone()).ok());
        match zoned {
            Some(zoned) => SystemTime::from(zoned.timestamp()),
            // Outside the calendar's years: as if in UTC.
            None => shift(UNIX_EPOCH, day.saturating_mul(86_400)),
        }
    }

    /// The first day of the week that holds `day`.
    pub fn week_floor(&self, day: i64) -> i64 {
        // 1970-01-01 was a Thursday, three days after a Monday.
        let weekday = (day + 3).rem_euclid(7);
        let start = i64::from(self.week_start.to_monday_zero_offset());
        day - (weekday - start).rem_euclid(7)
    }

    /// The instant a date and time of day written without a zone
    /// (`2026-10-04T09:30:00`, as the trash's `.trashinfo` says when a
    /// file was put there) names in the zone; in a daylight saving time
    /// gap or overlap, the instant the clock showed it first.
    pub fn local_time(&self, text: &str) -> Option<SystemTime> {
        let civil: jiff::civil::DateTime = text.trim().parse().ok()?;
        let zoned = civil.to_zoned(self.tz.clone()).ok()?;
        Some(SystemTime::from(zoned.timestamp()))
    }

    /// `now` less a duration read as a calendar reads it: `7d` is this time
    /// of day seven days ago whatever daylight saving time did in between,
    /// `1mo` the same day a month ago. `None` when `text` is not a duration
    /// jiff reads (`7d`, `3w`, `2h30m`, `1mo`, `1y`).
    pub fn ago(&self, now: SystemTime, text: &str) -> Option<SystemTime> {
        let span: jiff::Span = text.parse().ok()?;
        let zoned = Timestamp::try_from(now).ok()?.to_zoned(self.tz.clone());
        let then = zoned.checked_sub(span).ok()?;
        Some(SystemTime::from(then.timestamp()))
    }
}

fn shift(t: SystemTime, secs: i64) -> SystemTime {
    let d = Duration::from_secs(secs.unsigned_abs());
    if secs >= 0 {
        t.checked_add(d).unwrap_or(t)
    } else {
        t.checked_sub(d).unwrap_or(UNIX_EPOCH)
    }
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm,
/// proleptic Gregorian).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day since 1970-01-01, the inverse of
/// [`days_from_civil`].
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `YYYY-MM-DD` of a civil day, as the query language writes dates.
pub fn date_string(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

static WEEK_START: OnceLock<Weekday> = OnceLock::new();

/// The locale's first day of the week, once [`prewarm`] asked; Monday
/// until then. Never blocks.
pub fn week_start() -> Weekday {
    WEEK_START.get().copied().unwrap_or(Weekday::Monday)
}

/// Ask the locale for its first day of the week and keep the answer. Runs
/// `locale`: blocks for as long as it takes, so call it from a worker.
pub fn prewarm() {
    if WEEK_START.get().is_some() {
        return;
    }
    let mut locale = std::process::Command::new("locale");
    locale
        .args(["week-1stday", "first_weekday"])
        .stdin(std::process::Stdio::null());
    let first = crate::process::run(
        &mut locale,
        &bibli_core::CancelToken::new(),
        Duration::from_secs(2),
    )
    .ok()
    .and_then(|out| first_weekday(&String::from_utf8_lossy(&out)));
    let _ = WEEK_START.set(first.unwrap_or(Weekday::Monday));
}

/// The first weekday from `locale week-1stday first_weekday`: a date the
/// locale's weeks are counted from (`19971130`, a Sunday) and which day of
/// that week comes first (`2`: the day after it, Monday).
fn first_weekday(output: &str) -> Option<Weekday> {
    let mut lines = output.lines().map(str::trim);
    let origin = lines.next()?;
    let nth: i64 = lines.next()?.parse().ok()?;
    let (y, m, d) = (
        origin.get(..4)?.parse().ok()?,
        origin.get(4..6)?.parse().ok()?,
        origin.get(6..8)?.parse().ok()?,
    );
    let day = days_from_civil(y, m, d) + nth - 1;
    // Monday-zero offset of the day; 1970-01-01 was a Thursday (3).
    Weekday::from_monday_zero_offset(i8::try_from((day + 3).rem_euclid(7)).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paris() -> Calendar {
        Calendar::named("Europe/Paris", Weekday::Monday).expect("the tz database has Paris")
    }

    fn utc(y: i64, m: u32, d: u32, h: u64) -> SystemTime {
        shift(UNIX_EPOCH, days_from_civil(y, m, d) * 86_400) + Duration::from_secs(h * 3600)
    }

    #[test]
    fn civil_dates_round_trip_through_days() {
        for day in (-800_000..800_000).step_by(997) {
            let (y, m, d) = civil_from_days(day);
            assert_eq!(days_from_civil(y, m, d), day, "{y}-{m}-{d}");
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(date_string(days_from_civil(2026, 9, 25)), "2026-09-25");
    }

    #[test]
    fn a_day_is_the_zones_day_daylight_saving_time_included() {
        let cal = paris();
        // 23:30 UTC on 2026-09-25 is already the 26th in Paris (UTC+2).
        let late = utc(2026, 9, 25, 23) + Duration::from_secs(1800);
        assert_eq!(cal.day_of(late), days_from_civil(2026, 9, 26));
        assert_eq!(Calendar::utc().day_of(late), days_from_civil(2026, 9, 25));
        // Paris midnight is 22:00 UTC the day before in summer, 23:00 in winter.
        assert_eq!(
            cal.day_start(days_from_civil(2026, 9, 26)),
            utc(2026, 9, 25, 22)
        );
        assert_eq!(
            cal.day_start(days_from_civil(2026, 12, 1)),
            utc(2026, 11, 30, 23)
        );
        // The day the clocks go forward (2026-03-29) lasts 23 hours, the
        // day they go back (2026-10-25) 25.
        let length = |y, m, d| {
            let day = days_from_civil(y, m, d);
            cal.day_start(day + 1)
                .duration_since(cal.day_start(day))
                .unwrap()
                .as_secs()
                / 3600
        };
        assert_eq!(length(2026, 3, 29), 23);
        assert_eq!(length(2026, 10, 25), 25);
        assert_eq!(length(2026, 9, 25), 24);
    }

    #[test]
    fn a_midnight_that_does_not_exist_starts_the_day_after_the_gap() {
        // Santiago moves its clocks forward at midnight: 2026-09-06 begins
        // at 01:00 local, 04:00 UTC.
        let cal = Calendar::named("America/Santiago", Weekday::Monday).unwrap();
        let day = days_from_civil(2026, 9, 6);
        let start = cal.day_start(day);
        assert_eq!(start, utc(2026, 9, 6, 4));
        assert_eq!(cal.day_of(start), day);
        assert_eq!(cal.day_of(start - Duration::from_secs(1)), day - 1);
    }

    #[test]
    fn weeks_start_on_the_locales_day() {
        let friday = days_from_civil(2026, 9, 25);
        let monday = Calendar::utc();
        assert_eq!(monday.week_floor(friday), days_from_civil(2026, 9, 21));
        assert_eq!(
            monday.week_floor(days_from_civil(2026, 9, 21)),
            days_from_civil(2026, 9, 21)
        );
        let sunday = Calendar::new(TimeZone::UTC, Weekday::Sunday);
        assert_eq!(sunday.week_floor(friday), days_from_civil(2026, 9, 20));
        assert_eq!(
            sunday.week_floor(days_from_civil(2026, 9, 20)),
            days_from_civil(2026, 9, 20)
        );
        let saturday = Calendar::new(TimeZone::UTC, Weekday::Saturday);
        assert_eq!(saturday.week_floor(friday), days_from_civil(2026, 9, 19));
        // What `locale` says for fr_FR, en_US, and a locale counting from Monday.
        assert_eq!(first_weekday("19971130\n2\n"), Some(Weekday::Monday));
        assert_eq!(first_weekday("19971130\n1\n"), Some(Weekday::Sunday));
        assert_eq!(first_weekday("19971201\n1\n"), Some(Weekday::Monday));
        assert_eq!(first_weekday("19971130\n7\n"), Some(Weekday::Saturday));
        assert_eq!(first_weekday("garbage"), None);
    }

    #[test]
    fn durations_are_read_on_the_calendar() {
        let cal = paris();
        // Seven days before noon on 2026-10-28 is noon on the 21st, across
        // the night the clocks went back: 169 hours, not 168.
        let now = utc(2026, 10, 28, 11);
        let then = cal.ago(now, "7d").unwrap();
        assert_eq!(then, utc(2026, 10, 21, 10));
        assert_eq!(now.duration_since(then).unwrap().as_secs(), 169 * 3600);
        assert_eq!(cal.ago(now, "3w"), Some(utc(2026, 10, 7, 10)));
        assert_eq!(cal.ago(now, "2h30m"), Some(now - Duration::from_secs(9000)));
        assert_eq!(cal.ago(now, "1mo"), Some(utc(2026, 9, 28, 10)));
        assert_eq!(cal.ago(now, "1y"), Some(utc(2025, 10, 28, 11)));
        assert_eq!(cal.ago(now, "today"), None);
    }
}
