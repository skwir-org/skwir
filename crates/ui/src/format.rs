//! Small formatting helpers: humanized sizes, timestamps, thousands separators.

use std::time::SystemTime;

/// `1.2 MB`, `512 B`: as every preview and Details say it.
pub use skwir_platform::inspection::size;

/// `YYYY-MM-DD HH:MM` in the local time zone.
pub fn time(t: SystemTime) -> String {
    local(t, "%Y-%m-%d %H:%M")
}

/// With the seconds, where the exact value is asked for (Details).
pub fn exact_time(t: SystemTime) -> String {
    local(t, "%Y-%m-%d %H:%M:%S")
}

/// `YYYY-MM-DD`.
pub fn day(t: SystemTime) -> String {
    local(t, "%Y-%m-%d")
}

fn local(t: SystemTime, format: &str) -> String {
    jiff::Timestamp::try_from(t).map_or_else(
        |_| "—".into(),
        |ts| {
            ts.to_zoned(jiff::tz::TimeZone::system())
                .strftime(format)
                .to_string()
        },
    )
}

/// A date the way people say it: `Today 14:02`, `Yesterday 09:30`,
/// `Tuesday 18:45` within the last week, `12 Mar` this year, `12 Mar 2024`.
/// Nothing here depends on the minute, so a list that is not re-rendered
/// goes stale only at midnight, when the shell refreshes once.
pub fn friendly_time(t: SystemTime, now: &jiff::Zoned) -> String {
    let Ok(timestamp) = jiff::Timestamp::try_from(t) else {
        return "—".into();
    };
    let when = timestamp.to_zoned(now.time_zone().clone());
    let (date, today) = (when.date(), now.date());
    let format = if date == today {
        "Today %H:%M"
    } else if today.yesterday().is_ok_and(|yesterday| yesterday == date) {
        "Yesterday %H:%M"
    } else if date < today
        && today
            .checked_sub(jiff::Span::new().days(7))
            .is_ok_and(|week_ago| date > week_ago)
    {
        "%A %H:%M"
    } else if date.year() == today.year() {
        "%-d %b"
    } else {
        "%-d %b %Y"
    };
    when.strftime(format).to_string()
}

/// `1 item`, `3 items`.
pub fn count(n: usize, singular: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{} {singular}s", thousands(n))
    }
}

/// `1,234,567`.
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes() {
        assert_eq!(super::size(0), "0 B");
        assert_eq!(super::size(1023), "1023 B");
        assert_eq!(super::size(1536), "1.5 KB");
        assert_eq!(super::size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn friendly_dates_are_relative_to_the_day_not_the_minute() {
        let tz = jiff::tz::TimeZone::UTC;
        let now = jiff::civil::date(2026, 9, 20)
            .at(15, 0, 0, 0)
            .to_zoned(tz.clone())
            .unwrap();
        let at = |y, m, d, h, min| {
            std::time::SystemTime::from(
                jiff::civil::date(y, m, d)
                    .at(h, min, 0, 0)
                    .to_zoned(tz.clone())
                    .unwrap()
                    .timestamp(),
            )
        };
        assert_eq!(
            super::friendly_time(at(2026, 9, 20, 9, 5), &now),
            "Today 09:05"
        );
        assert_eq!(
            super::friendly_time(at(2026, 9, 19, 23, 59), &now),
            "Yesterday 23:59"
        );
        assert_eq!(
            super::friendly_time(at(2026, 9, 15, 8, 0), &now),
            "Tuesday 08:00"
        );
        assert_eq!(super::friendly_time(at(2026, 9, 13, 8, 0), &now), "13 Sep");
        assert_eq!(
            super::friendly_time(at(2024, 3, 2, 8, 0), &now),
            "2 Mar 2024"
        );
        // A clock ahead of ours (another machine's files) is not "this week".
        assert_eq!(super::friendly_time(at(2026, 9, 22, 8, 0), &now), "22 Sep");
    }

    #[test]
    fn counts_agree_in_number() {
        assert_eq!(super::count(1, "item"), "1 item");
        assert_eq!(super::count(0, "item"), "0 items");
        assert_eq!(super::count(1200, "item"), "1,200 items");
    }

    #[test]
    fn thousands() {
        assert_eq!(super::thousands(0), "0");
        assert_eq!(super::thousands(999), "999");
        assert_eq!(super::thousands(1000), "1,000");
        assert_eq!(super::thousands(1_234_567), "1,234,567");
    }
}
