//! Small formatting helpers: humanized sizes, timestamps, thousands separators.

use std::time::SystemTime;

/// `1.2 MB`, `512 B`: as every preview and Properties say it.
pub use skwir_platform::inspection::size;

/// `YYYY-MM-DD HH:MM` in the local time zone.
pub fn time(t: SystemTime) -> String {
    local(t, "%Y-%m-%d %H:%M")
}

/// With the seconds, where the exact value is asked for (Properties).
pub fn exact_time(t: SystemTime) -> String {
    local(t, "%Y-%m-%d %H:%M:%S")
}

/// `HH:MM`, the time of day, in the local time zone: when something
/// happened this session.
pub fn clock(t: SystemTime) -> String {
    local(t, "%H:%M")
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

/// `1 item`, `3 items`, said in the user's language (ADR-139).
pub fn items(n: usize) -> String {
    skwir_core::words!(
        count_items = "{ $count ->
        [one] 1 item
       *[other] { $count } items
    }",
        count = n
    )
    .say()
}

/// `1 file`, `3 files`, said in the user's language.
pub fn files(n: usize) -> String {
    skwir_core::words!(
        count_files = "{ $count ->
        [one] 1 file
       *[other] { $count } files
    }",
        count = n
    )
    .say()
}

/// `1 folder`, `3 folders`, said in the user's language.
pub fn folders(n: usize) -> String {
    skwir_core::words!(
        count_folders = "{ $count ->
        [one] 1 folder
       *[other] { $count } folders
    }",
        count = n
    )
    .say()
}

/// `1 item`, `3 items`: a noun made plural by its English `s`, until each
/// count is a message (PLAN.md, step E; `items`, `files`, `folders`).
pub fn count(n: usize, singular: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{} {singular}s", thousands(n))
    }
}

/// A speed, as words to place in a message: `23.4 MB/s`.
pub fn speed_words(bytes_a_second: f64) -> skwir_core::words::Words {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Shown, rounded.
    let bytes = bytes_a_second.max(0.0).round() as u64;
    skwir_core::words!(
        format_speed = "{ $size }/s",
        size = skwir_core::words::Size(bytes),
        note = "A speed: what is moved each second (23.4 MB/s)."
    )
}

/// How long is left, roughly, so that it counts down rather than ticks:
/// `a few seconds left`, `about 40 s left` (by fives), `about 12 min
/// left`, `about 2 h 10 min left` (by tens of minutes), `more than a day
/// left`, as words to place in a message.
pub fn time_left_words(left: std::time::Duration) -> skwir_core::words::Words {
    use skwir_core::words;
    let secs = left.as_secs();
    match secs {
        0..10 => words!(
            format_left_few_seconds = "a few seconds left",
            note = "How long an action has left, under its bar. Begins in lower case."
        ),
        10..60 => words!(
            format_left_seconds = "about { $seconds } s left",
            seconds = ((secs + 2) / 5 * 5).clamp(10, 55),
            note = "How long an action has left, under its bar, by fives of seconds; s is the \
                    second's symbol. Begins in lower case."
        ),
        60..3_570 => words!(
            format_left_minutes = "about { $minutes } min left",
            minutes = (secs + 30) / 60,
            note = "How long an action has left, under its bar; min is the minute's symbol. \
                    Begins in lower case."
        ),
        3_570..86_400 => {
            let minutes = (secs + 300) / 600 * 10;
            match (minutes / 60, minutes % 60) {
                (hours, 0) => words!(
                    format_left_hours = "about { $hours } h left",
                    hours = hours,
                    note = "How long an action has left, under its bar; h is the hour's symbol. \
                            Begins in lower case."
                ),
                (hours, minutes) => words!(
                    format_left_hours_minutes = "about { $hours } h { $minutes } min left",
                    hours = hours,
                    minutes = minutes,
                    note = "How long an action has left, under its bar, by tens of minutes; h \
                            and min are the hour's and the minute's symbols. Begins in lower \
                            case."
                ),
            }
        }
        _ => words!(
            format_left_more_than_a_day = "more than a day left",
            note = "How long an action has left, under its bar. Begins in lower case."
        ),
    }
}

/// How long something took, as words to place in a message: `45 s`, `3
/// min 20 s`, `1 h 5 min`.
pub fn took_words(took: std::time::Duration) -> skwir_core::words::Words {
    use skwir_core::words;
    let secs = took.as_secs();
    match (secs / 3_600, secs / 60 % 60, secs % 60) {
        (0, 0, s) => words!(
            format_took_seconds = "{ $seconds } s",
            seconds = s,
            note = "How long an action took; s is the second's symbol."
        ),
        (0, m, 0) => words!(
            format_took_minutes = "{ $minutes } min",
            minutes = m,
            note = "How long an action took; min is the minute's symbol."
        ),
        (0, m, s) => words!(
            format_took_minutes_seconds = "{ $minutes } min { $seconds } s",
            minutes = m,
            seconds = s,
            note = "How long an action took; min and s are the minute's and the second's \
                    symbols."
        ),
        (h, 0, _) => words!(
            format_took_hours = "{ $hours } h",
            hours = h,
            note = "How long an action took; h is the hour's symbol."
        ),
        (h, m, _) => words!(
            format_took_hours_minutes = "{ $hours } h { $minutes } min",
            hours = h,
            minutes = m,
            note = "How long an action took; h and min are the hour's and the minute's symbols."
        ),
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
    use std::time::Duration;

    /// A speed in the sizes' units; a time left rough, by steps that grow
    /// with it; what took long, to the minute past an hour.
    #[test]
    fn speeds_and_times_are_said_roughly() {
        let speed = |bytes| super::speed_words(bytes).english();
        assert_eq!(speed(1536.0), "1.5 kB/s");
        assert_eq!(speed(-1.0), "0 B/s");
        let left = |secs| super::time_left_words(Duration::from_secs(secs)).english();
        assert_eq!(left(3), "a few seconds left");
        assert_eq!(left(12), "about 10 s left");
        assert_eq!(left(43), "about 45 s left");
        assert_eq!(left(59), "about 55 s left");
        assert_eq!(left(60), "about 1 min left");
        assert_eq!(left(150), "about 3 min left");
        assert_eq!(left(3_599), "about 1 h left");
        assert_eq!(left(3_600), "about 1 h left");
        assert_eq!(left(7_890), "about 2 h 10 min left");
        assert_eq!(left(90_000), "more than a day left");
        let took = |secs| super::took_words(Duration::from_secs(secs)).english();
        assert_eq!(took(45), "45 s");
        assert_eq!(took(180), "3 min");
        assert_eq!(took(200), "3 min 20 s");
        assert_eq!(took(3_900), "1 h 5 min");
        assert_eq!(took(7_230), "2 h");
    }

    /// Counted as the setting says (ADR-139, 9): a thousand to the next by
    /// default, 1024 in binary multiples, each at its bound.
    #[test]
    fn sizes() {
        use skwir_core::fs::{SizeUnits, size_said_in};
        assert_eq!(super::size(0), "0 B");
        assert_eq!(super::size(999), "999 B");
        assert_eq!(super::size(1000), "1.0 kB");
        assert_eq!(super::size(1536), "1.5 kB");
        assert_eq!(super::size(5_000_000), "5.0 MB");
        let binary = |bytes| size_said_in(bytes, SizeUnits::Binary);
        assert_eq!(binary(1023), "1023 B");
        assert_eq!(binary(1024), "1.0 KiB");
        assert_eq!(binary(1536), "1.5 KiB");
        assert_eq!(binary(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(binary(4_000_000_000_000), "3.6 TiB");
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
