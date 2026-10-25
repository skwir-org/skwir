//! The user's calendar: their time zone, daylight saving time included, and
//! the day their week starts on. Everything that turns a date a person reads
//! into instants goes through it (the query language's dates and periods,
//! the timeline's bars), so "today" is the user's today (ADR-017).
//!
//! Days are counted as civil days since 1970-01-01 (`i64`, negative before
//! it): a day number names a date, not an instant, and only
//! [`Calendar::day_start`] turns one into the instant it begins at in the
//! zone. A day is then 23, 24 or 25 hours long. Weeks, months and years
//! are [`Period`]s of those days, the one reading of them: the query's
//! dates and named periods and the timeline's bars ask it, never count
//! months of their own.
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
        self.clock_of(t).0
    }

    /// What the clock in the zone shows at `t`: the civil day, and the
    /// seconds since its midnight the clock reads (an hour a clock change
    /// skips is never shown, one it repeats is shown twice), as a camera's
    /// clock keeps a time. Beyond what a calendar holds, the nearest
    /// instant it does.
    pub fn clock_of(&self, t: SystemTime) -> (i64, u32) {
        let Ok(ts) = Timestamp::try_from(t) else {
            return if t < UNIX_EPOCH {
                (days_from_civil(-9999, 1, 1), 0)
            } else {
                (days_from_civil(9999, 12, 31), 86_399)
            };
        };
        let shown = ts.to_zoned(self.tz.clone()).datetime();
        let (date, time) = (shown.date(), shown.time());
        let day = days_from_civil(
            i64::from(date.year()),
            u32::from(date.month().unsigned_abs()),
            u32::from(date.day().unsigned_abs()),
        );
        let seconds = u32::from(time.hour().unsigned_abs()) * 3600
            + u32::from(time.minute().unsigned_abs()) * 60
            + u32::from(time.second().unsigned_abs());
        (day, seconds)
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

    /// The year `t` falls in, in the zone.
    pub fn year_of(&self, t: SystemTime) -> i64 {
        civil_from_days(self.day_of(t)).0
    }
}

/// A period of the calendar, counted in civil days: what a date names
/// (`2026-05` is a month), what `this-week` names, a bar of the timeline.
/// Weeks start on the calendar's first weekday.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Period {
    Day,
    Week,
    Month,
    Year,
}

impl Period {
    /// The first day of the period that holds `day`.
    pub fn floor(self, day: i64, cal: &Calendar) -> i64 {
        match self {
            Period::Day => day,
            Period::Week => cal.week_floor(day),
            Period::Month => {
                let (y, m, _) = civil_from_days(day);
                days_from_civil(y, m, 1)
            }
            Period::Year => days_from_civil(civil_from_days(day).0, 1, 1),
        }
    }

    /// The first day of the period after the one that holds `day`.
    pub fn next(self, day: i64, cal: &Calendar) -> i64 {
        match self {
            Period::Day => day + 1,
            Period::Week => cal.week_floor(day) + 7,
            Period::Month => {
                let (y, m, _) = civil_from_days(day);
                let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                days_from_civil(y, m, 1)
            }
            Period::Year => days_from_civil(civil_from_days(day).0 + 1, 1, 1),
        }
    }

    /// The period that holds `day`: its first day and the first day of
    /// the next, `[start, end)`.
    pub fn around(self, day: i64, cal: &Calendar) -> (i64, i64) {
        (self.floor(day, cal), self.next(day, cal))
    }

    /// The period before the one that holds `day`.
    pub fn before(self, day: i64, cal: &Calendar) -> (i64, i64) {
        self.around(self.floor(day, cal) - 1, cal)
    }

    /// How many periods there are from the one that holds `first` to the
    /// one that holds `last`, both counted; none when the period of `last`
    /// comes first.
    pub fn count(self, first: i64, last: i64, cal: &Calendar) -> u64 {
        let month = |day: i64| {
            let (y, m, _) = civil_from_days(day);
            y * 12 + i64::from(m)
        };
        let between = match self {
            Period::Day => last - first,
            Period::Week => (cal.week_floor(last) - cal.week_floor(first)) / 7,
            Period::Month => month(last) - month(first),
            Period::Year => civil_from_days(last).0 - civil_from_days(first).0,
        };
        u64::try_from(between + 1).unwrap_or(0)
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

/// The civil day of a date, when there is such a date: `2026-02-31` does
/// not roll into March.
pub fn civil_day(y: i64, m: u32, d: u32) -> Option<i64> {
    let day = days_from_civil(y, m, d);
    (civil_from_days(day) == (y, m, d)).then_some(day)
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

/// Ask the locale for its first day of the week and keep the answer. On
/// Linux runs `locale`: blocks for as long as it takes, so call it from a
/// worker.
pub fn prewarm() {
    // One asks, the others wait for its answer: two plugins prewarming at
    // startup each ran `locale`.
    WEEK_START.get_or_init(|| system_week_start().unwrap_or(Weekday::Monday));
}

/// The locale's first weekday, as the system says it: glibc's `locale`
/// on Linux and the free desktops, the user's locale on Windows, the
/// current calendar on macOS (compiled, never run there).
#[cfg(all(unix, not(target_os = "macos")))]
fn system_week_start() -> Option<Weekday> {
    let mut locale = std::process::Command::new("locale");
    locale
        .args(["week-1stday", "first_weekday"])
        .stdin(std::process::Stdio::null());
    let out = crate::process::run(
        &mut locale,
        &skwir_core::CancelToken::new(),
        Duration::from_secs(2),
    )
    .ok()?;
    first_weekday(&String::from_utf8_lossy(&out))
}

#[cfg(windows)]
fn system_week_start() -> Option<Weekday> {
    Weekday::from_monday_zero_offset(crate::windows::first_weekday()?).ok()
}

#[cfg(target_os = "macos")]
fn system_week_start() -> Option<Weekday> {
    // 1 for Sunday to 7 for Saturday.
    let first = objc2_foundation::NSCalendar::currentCalendar().firstWeekday();
    Weekday::from_sunday_one_offset(i8::try_from(first).ok()?).ok()
}

/// The first weekday from `locale week-1stday first_weekday`: a date the
/// locale's weeks are counted from (`19971130`, a Sunday) and which day of
/// that week comes first (`2`: the day after it, Monday).
#[cfg(any(all(unix, not(target_os = "macos")), test))]
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

    /// The clock as Paris reads it: 23:30 UTC on 2026-09-25 is 01:30 on
    /// the 26th in summer, 00:30 once the clocks went back; the second
    /// 01:30 of the day they go back reads as the first.
    #[test]
    fn a_clock_reads_the_zones_day_and_time() {
        let paris = paris();
        let at = |y, m, d, secs: u64| {
            shift(UNIX_EPOCH, days_from_civil(y, m, d) * 86_400) + Duration::from_secs(secs)
        };
        let late = 23 * 3600 + 30 * 60;
        assert_eq!(
            paris.clock_of(at(2026, 9, 25, late)),
            (days_from_civil(2026, 9, 26), 5400)
        );
        assert_eq!(
            paris.clock_of(at(2026, 11, 25, late)),
            (days_from_civil(2026, 11, 26), 1800)
        );
        // 2026-10-25: 00:30 and 01:30 UTC are both 02:30 in Paris.
        let twice = days_from_civil(2026, 10, 25);
        assert_eq!(paris.clock_of(at(2026, 10, 25, 1800)), (twice, 9000));
        assert_eq!(paris.clock_of(at(2026, 10, 25, 5400)), (twice, 9000));
        assert_eq!(
            paris.day_of(at(2026, 9, 25, late)),
            days_from_civil(2026, 9, 26)
        );
        // Beyond what a calendar holds: its ends.
        let before = UNIX_EPOCH - Duration::from_secs(400_000_000_000);
        assert_eq!(paris.clock_of(before), (days_from_civil(-9999, 1, 1), 0));
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
        let late = utc(2026, 9, 25, 23) + Duration::from_mins(30);
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
    fn a_date_that_does_not_exist_names_no_day() {
        assert_eq!(civil_day(2026, 9, 25), Some(days_from_civil(2026, 9, 25)));
        assert_eq!(civil_day(2024, 2, 29), Some(days_from_civil(2024, 2, 29)));
        for (y, m, d) in [(2026, 2, 29), (2026, 2, 31), (2026, 4, 31), (2026, 13, 1)] {
            assert_eq!(civil_day(y, m, d), None, "{y}-{m}-{d}");
        }
        assert_eq!(civil_day(2026, 0, 1), None);
        assert_eq!(civil_day(2026, 1, 0), None);
    }

    #[test]
    fn periods_are_the_calendars() {
        let cal = Calendar::utc();
        let day = |y, m, d| days_from_civil(y, m, d);
        let friday = day(2026, 9, 18);
        let around = |period: Period, d: i64| {
            let (start, end) = period.around(d, &cal);
            (date_string(start), date_string(end))
        };
        let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        assert_eq!(
            around(Period::Day, friday),
            pair("2026-09-18", "2026-09-19")
        );
        assert_eq!(
            around(Period::Week, friday),
            pair("2026-09-14", "2026-09-21")
        );
        assert_eq!(
            around(Period::Month, friday),
            pair("2026-09-01", "2026-10-01")
        );
        assert_eq!(
            around(Period::Year, friday),
            pair("2026-01-01", "2027-01-01")
        );
        // December's next month is January of the next year.
        assert_eq!(
            around(Period::Month, day(2026, 12, 31)),
            pair("2026-12-01", "2027-01-01")
        );
        // A period's first day is its own; the day before is the last one's.
        assert_eq!(Period::Week.floor(day(2026, 9, 14), &cal), day(2026, 9, 14));
        assert_eq!(
            Period::Week.before(day(2026, 9, 14), &cal),
            (day(2026, 9, 7), day(2026, 9, 14))
        );
        assert_eq!(
            Period::Month.before(day(2026, 1, 20), &cal),
            (day(2025, 12, 1), day(2026, 1, 1))
        );
        assert_eq!(
            Period::Year.before(friday, &cal),
            (day(2025, 1, 1), day(2026, 1, 1))
        );
        // Weeks of a locale that starts them on Sunday.
        let sunday = Calendar::new(TimeZone::UTC, Weekday::Sunday);
        assert_eq!(Period::Week.floor(friday, &sunday), day(2026, 9, 13));
        assert_eq!(Period::Week.next(friday, &sunday), day(2026, 9, 20));
    }

    #[test]
    fn periods_are_counted_by_walking_them() {
        let cal = Calendar::utc();
        let friday = days_from_civil(2026, 9, 18);
        for period in [Period::Day, Period::Week, Period::Month, Period::Year] {
            for (first, last) in [
                (0, 0),
                (0, 4),
                (0, 11),
                (friday, friday + 30),
                (friday - 400, friday + 800),
            ] {
                let mut walked = 1;
                let mut start = period.floor(first, &cal);
                while period.next(start, &cal) <= last {
                    start = period.next(start, &cal);
                    walked += 1;
                }
                assert_eq!(period.count(first, last, &cal), walked, "{period:?}");
            }
            assert_eq!(period.count(friday, friday - 800, &cal), 0, "{period:?}");
        }
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
        assert_eq!(cal.ago(now, "2h30m"), Some(now - Duration::from_mins(150)));
        assert_eq!(cal.ago(now, "1mo"), Some(utc(2026, 9, 28, 10)));
        assert_eq!(cal.ago(now, "1y"), Some(utc(2025, 10, 28, 11)));
        assert_eq!(cal.ago(now, "today"), None);
    }
}
