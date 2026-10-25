//! skwir plugin: timeline. Files by modification time as a histogram the
//! UI renders as a scrubbable bar chart; a bucket becomes a `query:`
//! location (`modified>=start modified<end in:dir`) when selected.
//!
//! Commands: `timeline.histogram { pane?, bucket?, from?, until?, subtree? }`
//! returns the buckets for the pane's listing (hydrated entries only; the
//! UI hydrates everything first) or, with `subtree`, the [`SubtreeStatus`]
//! of a background walk of the pane's folder, which `timeline.status {
//! pane? }` returns until the walk is done. `timeline.select { start, end,
//! pane?, subtree? }` navigates to the matching query, or refines the
//! pane's search to the period.
//!
//! What a bar counts is what clicking it shows: the entries the query lists
//! (files, folders and links, hidden ones only when the pane shows hidden
//! entries), from the listing (`depth:1`) or from the whole subtree.
//!
//! Buckets are periods of the user's calendar (`skwir_platform::calendar`),
//! like the query language's dates: days from local midnight, daylight
//! saving time included, and weeks from the locale's first weekday. Entries
//! are folded by the civil day they fall on; a bar's instants are the
//! starts of its first day and of the day after its last.
//!
//! Asked no width, the bars are the finest that show every date. A width
//! asked is drawn as asked: when it would need more than [`MAX_BARS`]
//! bars, that many of them are shown, a window the caller moves (`from`,
//! `until`), and the entries on either side are counted (`before`,
//! `after`). Dates far from all the others are never shown (`outside`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::entry::{Value, keys};
use skwir_core::fs::reach;
use skwir_core::os_path::PathArg;
use skwir_core::plugin::Plugin;
use skwir_core::scheduler::Priority;
use skwir_core::workspace::PaneId;
use skwir_platform::calendar::{
    Calendar, Period, civil_day, civil_from_days, date_string, days_from_civil,
};

const ID: &str = "timeline";
/// 9999-12-31T23:59:59Z: later times (a bogus mtime) count as that instant.
const LAST_SECOND: u64 = 253_402_300_799;
/// At most this many bars: a width that would need more shows a window of
/// them. Entries outside the busiest span of this many years are far
/// outside, counted beside the bars (`outside`), so a few bogus dates never
/// decide the width.
pub const MAX_BARS: usize = 400;
/// The entries of a pane read under one hold of the workspace's lock.
const SLICE: usize = 1024;

/// Bucket width.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    Day,
    Week,
    #[default]
    Month,
    Year,
}

impl Bucket {
    const ALL: [Bucket; 4] = [Bucket::Day, Bucket::Week, Bucket::Month, Bucket::Year];

    /// The calendar's period a bar of this width covers.
    pub fn period(self) -> Period {
        match self {
            Bucket::Day => Period::Day,
            Bucket::Week => Period::Week,
            Bucket::Month => Period::Month,
            Bucket::Year => Period::Year,
        }
    }
}

/// Which of a file's times the bars count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Date {
    #[default]
    Modified,
    Created,
    Accessed,
    /// When a picture or a video was taken, as its camera's clock said
    /// (`keys::TAKEN`): read from each file, in the background.
    Taken,
}

impl Date {
    /// Its word in the query language (`modified:2026-07`), which is also
    /// the facet a search refines.
    pub fn word(self) -> &'static str {
        match self {
            Date::Modified => "modified",
            Date::Created => "created",
            Date::Accessed => "accessed",
            Date::Taken => "taken",
        }
    }

    /// What a listing knows of it; `None` for what is read from the file.
    fn listed(self) -> Option<&'static str> {
        match self {
            Date::Modified => Some(keys::MODIFIED),
            Date::Created => Some(keys::CREATED),
            Date::Accessed => Some(keys::ACCESSED),
            Date::Taken => None,
        }
    }

    /// What a stat says of it.
    fn of_stat(self, m: &std::fs::Metadata) -> Option<SystemTime> {
        match self {
            Date::Modified => m.modified().ok(),
            Date::Created => m.created().ok(),
            Date::Accessed => m.accessed().ok(),
            Date::Taken => None,
        }
    }
}

/// The civil day of a camera's clock (`keys::TAKEN`: `20260701083000`), in
/// no zone: the day it shows.
fn clock_day(clock: u64) -> Option<i64> {
    let (y, m, d) = (
        clock / 10_000_000_000,
        clock / 100_000_000 % 100,
        clock / 1_000_000 % 100,
    );
    civil_day(
        i64::try_from(y).ok()?,
        u32::try_from(m).ok()?,
        u32::try_from(d).ok()?,
    )
}

/// Seconds since the epoch of the instant civil day `day` begins at.
fn day_start_secs(day: i64, cal: &Calendar) -> u64 {
    cal.day_start(day)
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// One histogram bar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Bar {
    /// The first instant of the bar's first day, seconds since the epoch.
    pub start: u64,
    /// The first instant of the day after its last.
    pub end: u64,
    pub count: u64,
    pub bytes: u64,
    /// Of them, those the pane has selected, and their bytes.
    pub selected: u64,
    pub selected_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct Histogram {
    /// The bucket the bars use: the one asked for, else `fits`.
    pub bucket: Bucket,
    /// The finest bucket whose bars show every date (but those far
    /// outside) at once; a finer one shows a window of [`MAX_BARS`] bars.
    pub fits: Bucket,
    pub bars: Vec<Bar>,
    /// The time the bars count.
    pub date: Date,
    /// Entries without that time (not hydrated yet, or a file that says
    /// none: no picture's clock).
    pub unknown: u64,
    /// Dated entries before the first bar and after the last, outside the
    /// window shown: a window moved along reaches them.
    pub before: u64,
    pub after: u64,
    /// Entries dated far from the others, outside the busiest span of
    /// [`MAX_BARS`] years: no bar shows them.
    pub outside: u64,
    pub total: u64,
}

/// Which bars a bucket too fine to show every date shows: [`MAX_BARS`] of
/// them, from or until the bar holding an instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// Until the latest dated day not after this instant (now): what was
    /// changed last, not a bogus date in the future.
    Latest(SystemTime),
    /// From the bar holding this instant.
    From(SystemTime),
    /// Until the bar holding this instant.
    Until(SystemTime),
}

impl Default for Window {
    fn default() -> Self {
        Window::Latest(SystemTime::now())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    count: u64,
    bytes: u64,
    selected: u64,
    selected_bytes: u64,
}

impl Tally {
    fn add(&mut self, other: Tally) {
        self.count += other.count;
        self.bytes = self.bytes.saturating_add(other.bytes);
        self.selected += other.selected;
        self.selected_bytes = self.selected_bytes.saturating_add(other.selected_bytes);
    }

    fn one(bytes: u64, selected: bool) -> Tally {
        Tally {
            count: 1,
            bytes,
            selected: u64::from(selected),
            selected_bytes: if selected { bytes } else { 0 },
        }
    }
}

/// Entries folded by the civil day they fall on, in the calendar's zone,
/// as they are seen: memory grows with the number of distinct days, not of
/// files, and every bucket width can be drawn from it without walking again.
#[derive(Clone, Debug)]
pub struct Days {
    days: BTreeMap<i64, Tally>,
    calendar: Calendar,
}

impl Default for Days {
    /// On the user's calendar.
    fn default() -> Self {
        Days::new(Calendar::system())
    }
}

impl Days {
    pub fn new(calendar: Calendar) -> Self {
        Days {
            days: BTreeMap::new(),
            calendar,
        }
    }

    pub fn add(&mut self, t: SystemTime, bytes: u64) {
        self.add_on(self.day_of(t), bytes, false);
    }

    /// Count a file of `bytes` on civil day `day`, `selected` or not.
    fn add_on(&mut self, day: i64, bytes: u64, selected: bool) {
        self.days
            .entry(day)
            .or_default()
            .add(Tally::one(bytes, selected));
    }

    /// The civil day `t` falls on, a bogus time far ahead the last there is.
    fn day_of(&self, t: SystemTime) -> i64 {
        let secs = t
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
            .min(LAST_SECOND);
        self.calendar.day_of(UNIX_EPOCH + Duration::from_secs(secs))
    }

    fn merge(&mut self, other: Days) {
        for (day, tally) in other.days {
            self.days.entry(day).or_default().add(tally);
        }
    }

    /// Bars of `bucket`, by default the finest that shows every date; a
    /// bucket that would need more than [`MAX_BARS`] bars shows `window`.
    pub fn histogram(&self, bucket: Option<Bucket>, window: Window, unknown: u64) -> Histogram {
        let cal = &self.calendar;
        let total = unknown + self.days.values().map(|t| t.count).sum::<u64>();
        let mut h = Histogram {
            bucket: bucket.unwrap_or_default(),
            fits: Bucket::default(),
            date: Date::default(),
            bars: Vec::new(),
            unknown,
            before: 0,
            after: 0,
            outside: 0,
            total,
        };
        let Some((first, last)) = self.span() else {
            return h;
        };
        let fits = |b: Bucket| b.period().count(first, last, cal) <= MAX_BARS as u64;
        // The span is at most `MAX_BARS` years: years always fit.
        h.fits = Bucket::ALL
            .into_iter()
            .find(|b| fits(*b))
            .unwrap_or(Bucket::Year);
        h.bucket = bucket.unwrap_or(h.fits);
        let period = h.bucket.period();
        // The first days of the first bar and of the last.
        let (from, to) = if fits(h.bucket) {
            (period.floor(first, cal), period.floor(last, cal))
        } else {
            self.window(period, (first, last), window)
        };
        let after = period.next(to, cal);
        let mut sums: BTreeMap<i64, Tally> = BTreeMap::new();
        for (&day, &tally) in &self.days {
            if day < first || day > last {
                h.outside += tally.count;
            } else if day < from {
                h.before += tally.count;
            } else if day >= after {
                h.after += tally.count;
            } else {
                sums.entry(period.floor(day, cal)).or_default().add(tally);
            }
        }
        let mut s = from;
        while s <= to {
            let tally = sums.get(&s).copied().unwrap_or_default();
            let end = period.next(s, cal);
            h.bars.push(Bar {
                start: day_start_secs(s, cal),
                end: day_start_secs(end, cal),
                count: tally.count,
                bytes: tally.bytes,
                selected: tally.selected,
                selected_bytes: tally.selected_bytes,
            });
            s = end;
        }
        h
    }

    /// The dated days the bars show, `(first, last)`: those of the busiest
    /// span of [`MAX_BARS`] years (all of them, as a rule), less the stray
    /// dates at either end. A few bogus dates (a clock set wrong, a time of
    /// zero) must not squeeze the real ones into a few bars.
    fn span(&self) -> Option<(i64, i64)> {
        let (&first, &last) = (self.days.keys().next()?, self.days.keys().next_back()?);
        let (mut first, mut last) =
            if Period::Year.count(first, last, &self.calendar) <= MAX_BARS as u64 {
                (first, last)
            } else {
                let (from, to) = self.busiest_years();
                let mut within = self.days.range(from..=to).map(|(&day, _)| day);
                let first = within.next()?;
                (first, within.next_back().unwrap_or(first))
            };
        // Strays: one entry in a hundred at most, further from the others
        // than they span and by more than a year. Left out at one end, the
        // others span less, and the other end may have strays too.
        let mut few = self
            .days
            .range(first..=last)
            .map(|(_, t)| t.count)
            .sum::<u64>()
            / 100;
        let far = |gap: i64, rest: i64| gap > rest.max(366);
        loop {
            let mut cut = None;
            let mut seen = 0;
            let mut days = self.days.range(first..=last).peekable();
            while let (Some((&day, tally)), Some(&(&next, _))) = (days.next(), days.peek()) {
                seen += tally.count;
                if seen > few {
                    break;
                }
                if far(next - day, last - next) {
                    cut = Some((Some(next), None, seen));
                }
            }
            if cut.is_none() {
                seen = 0;
                let mut days = self.days.range(first..=last).rev().peekable();
                while let (Some((&day, tally)), Some(&(&before, _))) = (days.next(), days.peek()) {
                    seen += tally.count;
                    if seen > few {
                        break;
                    }
                    if far(day - before, before - first) {
                        cut = Some((None, Some(before), seen));
                    }
                }
            }
            let Some((new_first, new_last, left_out)) = cut else {
                break;
            };
            first = new_first.unwrap_or(first);
            last = new_last.unwrap_or(last);
            few -= left_out;
        }
        Some((first, last))
    }

    /// The first days of the first and the last of [`MAX_BARS`] periods
    /// inside `span`, placed as `window` asks: a window at an end of the
    /// span keeps its length.
    fn window(&self, period: Period, (first, last): (i64, i64), window: Window) -> (i64, i64) {
        let cal = &self.calendar;
        let (lo, hi) = (period.floor(first, cal), period.floor(last, cal));
        let n = MAX_BARS - 1;
        let back = |day: i64| (0..n).fold(day, |d, _| period.floor(d - 1, cal));
        let ahead = |day: i64| (0..n).fold(day, |d, _| period.next(d, cal));
        let day = |t: SystemTime| cal.day_of(t).clamp(first, last);
        let last_day = match window {
            Window::From(t) => {
                let start = period.floor(day(t), cal);
                let end = ahead(start);
                return if end > hi {
                    (back(hi), hi)
                } else {
                    (start, end)
                };
            }
            Window::Until(t) => day(t),
            Window::Latest(now) => self
                .days
                .range(first..=day(now))
                .next_back()
                .map_or(first, |(&d, _)| d),
        };
        let end = period.floor(last_day, cal);
        let start = back(end);
        if start < lo {
            (lo, ahead(lo))
        } else {
            (start, end)
        }
    }

    /// The span of at most [`MAX_BARS`] years holding the most entries, as
    /// `(first day, last day)`.
    fn busiest_years(&self) -> (i64, i64) {
        let mut years: BTreeMap<i64, u64> = BTreeMap::new();
        for (&day, tally) in &self.days {
            *years.entry(civil_from_days(day).0).or_default() += tally.count;
        }
        let years: Vec<(i64, u64)> = years.into_iter().collect();
        let (mut best, mut best_sum, mut sum, mut lo) = ((0, 0), 0, 0, 0);
        for hi in 0..years.len() {
            sum += years[hi].1;
            while years[hi].0 - years[lo].0 >= i64::try_from(MAX_BARS).unwrap_or(i64::MAX) {
                sum -= years[lo].1;
                lo += 1;
            }
            if sum > best_sum {
                (best, best_sum) = ((lo, hi), sum);
            }
        }
        let first = days_from_civil(years[best.0].0, 1, 1);
        let last = days_from_civil(years[best.1].0 + 1, 1, 1) - 1;
        (first, last)
    }
}

/// The days `[first, end)` as a `modified:` value says them: a year
/// (`2026`), a month (`2026-05`), a day, or a range. A range includes its
/// end date in the query language, a bar excludes its end: the last day
/// is the one before `end`.
fn period_words(first: i64, end: i64) -> String {
    let last = (end - 1).max(first);
    let ((y, m, d), (ly, lm, _)) = (civil_from_days(first), civil_from_days(last));
    let next = civil_from_days(last + 1);
    if first == last {
        date_string(first)
    } else if (m, d) == (1, 1) && ly == y && (next.1, next.2) == (1, 1) {
        format!("{y:04}")
    } else if d == 1 && (ly, lm) == (y, m) && next.2 == 1 {
        format!("{y:04}-{m:02}")
    } else {
        format!("{}..{}", date_string(first), date_string(last))
    }
}

/// Bucket `(mtime, size)` samples into a dense, contiguous histogram of
/// `cal`'s periods.
pub fn histogram(
    samples: impl IntoIterator<Item = (SystemTime, u64)>,
    bucket: Option<Bucket>,
    window: Window,
    unknown: u64,
    cal: &Calendar,
) -> Histogram {
    let mut days = Days::new(cal.clone());
    for (t, bytes) in samples {
        days.add(t, bytes);
    }
    days.histogram(bucket, window, unknown)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct HistogramArgs {
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// The bars' width; by default the finest that shows every date.
    #[serde(default)]
    pub bucket: Option<Bucket>,
    /// With a width too fine to show every date in [`MAX_BARS`] bars: the
    /// bars start at the one holding this instant, seconds since the epoch…
    #[serde(default)]
    pub from: Option<u64>,
    /// …or end at the one holding it. By default they end at the latest
    /// date not after today.
    #[serde(default)]
    pub until: Option<u64>,
    /// Walk the whole subtree in the background instead of the listing.
    #[serde(default)]
    pub subtree: bool,
    /// Which time the bars count: by default when files were modified.
    /// A picture's clock (`taken`) is read from each file in the
    /// background, as a subtree is walked.
    #[serde(default)]
    pub date: Date,
}

impl HistogramArgs {
    fn window(&self) -> Window {
        let at = |secs: u64| UNIX_EPOCH + Duration::from_secs(secs.min(LAST_SECOND));
        match (self.from, self.until) {
            (Some(from), _) => Window::From(at(from)),
            (None, Some(until)) => Window::Until(at(until)),
            (None, None) => Window::default(),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SelectArgs {
    /// A bar's start, seconds since the epoch: the first instant of a day.
    pub start: u64,
    /// The first instant after it (several bars: after the last).
    pub end: u64,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// Search the subtree rather than the folder itself; by default, what
    /// the pane's last histogram counted.
    #[serde(default)]
    pub subtree: Option<bool>,
    /// Which time; by default, the one the pane's last histogram counted.
    #[serde(default)]
    pub date: Option<Date>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct StatusArgs {
    #[serde(default)]
    pub pane: Option<PaneId>,
}

/// What a count in the background says: a walk of the subtree, or the
/// clocks of the pictures a pane lists.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct SubtreeStatus {
    pub root: Option<PathArg>,
    pub scanning: bool,
    pub histogram: Option<Histogram>,
}

/// What a count in the background counts.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Counted {
    /// Every entry below a folder, with or without the hidden ones.
    Subtree {
        dir: PathBuf,
        hidden: bool,
        date: Date,
    },
    /// The files a pane lists, each read (a picture's clock), told apart
    /// by what they are and which are selected (`Listed::signature`).
    Listing { files: u64 },
}

/// One count in the background.
struct Walk {
    what: Counted,
    generation: u64,
    /// Ends with the pane's visit and with the plugin.
    cancel: CancelToken,
    /// Its days, once counted, and the entries it could not date.
    days: Option<(Arc<Days>, u64)>,
    /// What the count it took over from had: the same files, another
    /// selection or a file more, shown until it is counted.
    before: Option<(Arc<Days>, u64)>,
}

#[derive(Default)]
struct PaneState {
    /// What the last histogram counted, for `timeline.select`.
    subtree: bool,
    date: Date,
    /// The bars it was asked for, which the walk's answer draws.
    bucket: Option<Bucket>,
    window: Window,
    walk: Option<Walk>,
}

impl PaneState {
    fn status(&self) -> SubtreeStatus {
        let Some(walk) = &self.walk else {
            return SubtreeStatus::default();
        };
        let root = match &walk.what {
            Counted::Subtree { dir, .. } => Some(PathArg::from(dir.as_path())),
            Counted::Listing { .. } => None,
        };
        SubtreeStatus {
            root,
            scanning: walk.days.is_none() && !walk.cancel.is_cancelled(),
            histogram: walk
                .days
                .as_ref()
                .or(walk.before.as_ref())
                .map(|(d, unknown)| {
                    let mut h = d.histogram(self.bucket, self.window, *unknown);
                    h.date = self.date;
                    h
                }),
        }
    }
}

#[derive(Default)]
struct State {
    panes: HashMap<PaneId, PaneState>,
    generation: u64,
    /// Cancelled on disable.
    lifetime: Option<CancelToken>,
}

/// The plugin.
#[derive(Default)]
pub struct TimelinePlugin {
    state: Arc<Mutex<State>>,
}

/// What a pane shows, read once: its folder, whether it shows hidden
/// entries, and the token that ends with this visit.
fn pane_view(app: &App, pane: PaneId) -> anyhow::Result<(PathBuf, bool, CancelToken)> {
    let dir = app.pane_dir(pane)?;
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    Ok((dir, p.sort.show_hidden, p.location_lifetime()))
}

/// A picture's clock as a civil day, read on demand (`keys::TAKEN`):
/// remembered by the core's fields, read once per file until it changes.
/// `known` is the file's identity when the caller has it (a listing's
/// file, a walk's stat), so it is not stat-ed again.
fn taken_day(
    fields: &skwir_core::fields::Fields,
    path: &Path,
    known: Option<skwir_core::fields::Identity>,
    cancel: &CancelToken,
) -> Option<i64> {
    fields
        .get_all(&[keys::TAKEN], path, known, cancel)
        .pop()
        .flatten()
        .as_ref()
        .and_then(Value::as_u64)
        .and_then(clock_day)
}

/// Nanoseconds since 1970 of a time, as a file's identity holds it.
fn nanos(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

/// Every entry below `root` (not `root` itself), folded by the day of its
/// `date`, as a search walks: never into a network mount below it
/// (ADR-077). A picture's clock is read from each file. `None` when
/// cancelled. Run in `root`'s lane: a remote's folder is walked there one
/// after the other, never on rayon's threads, which no lane holds.
fn walk_days(
    root: &Path,
    hidden: bool,
    date: Date,
    fields: &skwir_core::fields::Fields,
    cancel: &CancelToken,
) -> Option<(Days, u64)> {
    use rayon::prelude::*;
    type Level = (Days, u64, Vec<PathBuf>);
    let one = |dir: &Path| -> Level {
        let mut days = Days::default();
        let (mut unknown, mut subdirs) = (0, Vec::new());
        if cancel.is_cancelled() {
            return (days, unknown, subdirs);
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if !hidden && skwir_core::fs::is_hidden(&e) {
                    continue;
                }
                // Told before its stat, which its server would answer.
                if e.file_type().is_ok_and(|t| t.is_dir()) && reach::is_point(&e.path()) {
                    continue;
                }
                // `DirEntry::metadata` does not follow links: a link is
                // counted by its own date, and never walked into.
                let Ok(m) = e.metadata() else { continue };
                let bytes = if m.is_dir() { 0 } else { m.len() };
                let day = if date == Date::Taken {
                    // A file's own clock; a link's, through the link.
                    let path = e.path();
                    let known = m
                        .is_file()
                        .then(|| skwir_core::fs::stat::identity(&path, &m))
                        .flatten()
                        .map(|id| (id, m.len(), m.modified().map_or(0, nanos)));
                    (!m.is_dir())
                        .then(|| taken_day(fields, &path, known, cancel))
                        .flatten()
                } else {
                    date.of_stat(&m).map(|t| days.day_of(t))
                };
                match day {
                    Some(day) => days.add_on(day, bytes, false),
                    None => unknown += 1,
                }
                if m.is_dir() {
                    subdirs.push(e.path());
                }
            }
        }
        (days, unknown, subdirs)
    };
    let fold = |(mut a, au, mut a_dirs): Level, (b, bu, b_dirs): Level| {
        a.merge(b);
        a_dirs.extend(b_dirs);
        (a, au + bu, a_dirs)
    };
    let remote = reach::of(root).is_some();
    let (mut days, mut unknown) = (Days::default(), 0);
    let mut level = vec![root.to_path_buf()];
    while !level.is_empty() {
        if cancel.is_cancelled() {
            return None;
        }
        let (d, u, next) = if remote {
            level
                .iter()
                .map(|dir| one(dir))
                .fold((Days::default(), 0, Vec::new()), fold)
        } else {
            level
                .par_iter()
                .map(|dir| one(dir))
                .reduce(|| (Days::default(), 0, Vec::new()), fold)
        };
        days.merge(d);
        unknown += u;
        level = next;
    }
    (!cancel.is_cancelled()).then_some((days, unknown))
}

/// A file a pane lists, as a count reads it.
struct Listed {
    path: Option<PathBuf>,
    /// Its identity, for a file the listing stat-ed (no link).
    known: Option<skwir_core::fields::Identity>,
    bytes: u64,
    selected: bool,
}

impl Listed {
    /// What tells one list of files from another: the files, where they
    /// are, and which are selected.
    fn signature(files: &[Listed]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        for f in files {
            (&f.path, f.known, f.selected).hash(&mut h);
        }
        h.finish()
    }
}

impl TimelinePlugin {
    /// The pane's shown entries (`Pane::order`: hidden files and
    /// companions as it shows them, filtered by name, narrowed), read a
    /// slice at a time, each under a hold of the lock of its own: the
    /// window's thread applies what the core sends meanwhile. `each` is
    /// told an entry, whether it is selected, and where it is.
    fn each_shown(
        app: &App,
        pane: PaneId,
        with_path: bool,
        mut each: impl FnMut(&skwir_core::Entry, bool, Option<PathBuf>),
    ) -> anyhow::Result<()> {
        let withheld = app.withheld(pane);
        let order = app
            .workspace
            .read()
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?
            .order();
        for slice in order.chunks(SLICE) {
            let ws = app.workspace.read();
            let p = ws
                .pane(pane)
                .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
            for id in slice.iter().filter(|id| !withheld.contains(id)) {
                if let Some(e) = p.entry(*id) {
                    // `path_of` answers from memory, under the lock by
                    // contract (`OpenLocation`).
                    let path = with_path.then(|| p.path_of(*id)).flatten();
                    each(e, p.selection.contains(*id), path);
                }
            }
        }
        Ok(())
    }

    fn listing_histogram(
        &self,
        app: &App,
        pane: PaneId,
        date: Date,
        bucket: Option<Bucket>,
        window: Window,
    ) -> anyhow::Result<Histogram> {
        let key = date.listed().unwrap_or(keys::MODIFIED);
        let mut unknown = 0;
        let mut dated: Vec<(SystemTime, u64, bool)> = Vec::new();
        Self::each_shown(app, pane, false, |e, selected, _| {
            // A folder weighs nothing here, as in a walk: its files do.
            let bytes = if e.is_dir() {
                0
            } else {
                e.metadata
                    .value(keys::SIZE)
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            };
            match e.metadata.value(key).and_then(Value::as_time) {
                Some(t) => dated.push((t, bytes, selected)),
                None => unknown += 1,
            }
        })?;
        // The days are told after the lock is let go.
        let mut days = Days::default();
        for &(t, bytes, selected) in &dated {
            days.add_on(days.day_of(t), bytes, selected);
        }
        let mut state = self.state.lock();
        let ps = state.panes.entry(pane).or_default();
        ps.subtree = false;
        ps.date = date;
        ps.bucket = bucket;
        ps.window = window;
        let mut h = days.histogram(bucket, window, unknown);
        h.date = date;
        Ok(h)
    }

    /// Count `what` in the background for the pane, unless a count of it
    /// (running or done) is already there for this visit: the UI asks again
    /// whenever the listing changes, and that must not restart a walk.
    /// `work` counts, in `lane`, with the walk's token.
    // What is counted, how it is drawn, and where and how it is read: one
    // call for both kinds of count.
    #[allow(clippy::too_many_arguments)]
    fn in_background(
        &self,
        app: &App,
        pane: PaneId,
        what: Counted,
        (date, bucket, window): (Date, Option<Bucket>, Window),
        visit: &CancelToken,
        lane: skwir_core::scheduler::Lane,
        work: impl FnOnce(&CancelToken) -> Option<(Days, u64)> + Send + 'static,
    ) -> anyhow::Result<SubtreeStatus> {
        let mut state = self.state.lock();
        let Some(lifetime) = state.lifetime.clone() else {
            anyhow::bail!("the timeline is disabled");
        };
        {
            // Forget panes that were closed.
            let ws = app.workspace.read();
            state.panes.retain(|id, ps| {
                let open = ws.pane(*id).is_some();
                if !open && let Some(walk) = &ps.walk {
                    walk.cancel.cancel();
                }
                open
            });
        }
        state.generation += 1;
        let generation = state.generation;
        let ps = state.panes.entry(pane).or_default();
        ps.subtree = matches!(what, Counted::Subtree { .. });
        ps.date = date;
        ps.bucket = bucket;
        ps.window = window;
        let current = ps
            .walk
            .as_ref()
            .is_some_and(|w| w.what == what && !w.cancel.is_cancelled());
        if !current {
            // The same visit's listing, counted again: what it had stays
            // shown meanwhile. Another folder's never is.
            let before = ps.walk.take().and_then(|old| {
                let same = matches!(
                    (&old.what, &what),
                    (Counted::Listing { .. }, Counted::Listing { .. })
                ) && !old.cancel.is_cancelled();
                old.cancel.cancel();
                if same { old.days.or(old.before) } else { None }
            });
            let cancel = lifetime.linked(visit);
            ps.walk = Some(Walk {
                what,
                generation,
                cancel: cancel.clone(),
                days: None,
                before,
            });
            let shared = Arc::clone(&self.state);
            let wake = app.waker_handle();
            let task_cancel = cancel.clone();
            app.scheduler.spawn_in(
                lane,
                Priority::Background,
                "timeline.walk",
                cancel,
                move || {
                    let Some((days, unknown)) = work(&task_cancel) else {
                        return;
                    };
                    let mut s = shared.lock();
                    // Only the count this pane still waits for may answer.
                    if let Some(walk) = s
                        .panes
                        .get_mut(&pane)
                        .and_then(|ps| ps.walk.as_mut())
                        .filter(|w| w.generation == generation)
                    {
                        walk.days = Some((Arc::new(days), unknown));
                        walk.before = None;
                        drop(s);
                        wake();
                    }
                },
            );
        }
        Ok(ps.status())
    }

    /// Walk the pane's folder.
    fn subtree(
        &self,
        app: &App,
        pane: PaneId,
        asked: (Date, Option<Bucket>, Window),
    ) -> anyhow::Result<SubtreeStatus> {
        let (dir, hidden, visit) = pane_view(app, pane)?;
        let date = asked.0;
        let what = Counted::Subtree {
            dir: dir.clone(),
            hidden,
            date,
        };
        let fields = Arc::clone(&app.fields);
        let lane = reach::lane(&dir);
        self.in_background(app, pane, what, asked, &visit, lane, move |cancel| {
            walk_days(&dir, hidden, date, &fields, cancel)
        })
    }

    /// Read the clock of each picture the pane lists.
    fn listing_taken(
        &self,
        app: &App,
        pane: PaneId,
        asked: (Date, Option<Bucket>, Window),
    ) -> anyhow::Result<SubtreeStatus> {
        let mut files = Vec::new();
        Self::each_shown(app, pane, true, |e, selected, path| {
            let size = e.metadata.value(keys::SIZE).and_then(Value::as_u64);
            let modified = e
                .metadata
                .value(keys::MODIFIED)
                .and_then(Value::as_time)
                .map(nanos);
            files.push(Listed {
                path: path.filter(|_| !e.is_dir()),
                known: size
                    .zip(modified)
                    .filter(|_| e.kind == skwir_core::EntryKind::File)
                    .map(|(len, modified)| (e.file(), len, modified)),
                bytes: if e.is_dir() { 0 } else { size.unwrap_or(0) },
                selected,
            });
        })?;
        let visit = app
            .workspace
            .read()
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?
            .location_lifetime();
        let what = Counted::Listing {
            files: Listed::signature(&files),
        };
        // A remote's files are read in its lane, one after the other.
        let lane = files
            .iter()
            .find_map(|f| f.path.as_deref())
            .map_or_else(|| reach::lane(Path::new("/")), reach::lane);
        let fields = Arc::clone(&app.fields);
        self.in_background(app, pane, what, asked, &visit, lane, move |cancel| {
            let (mut days, mut unknown) = (Days::default(), 0);
            for f in &files {
                if cancel.is_cancelled() {
                    return None;
                }
                match f
                    .path
                    .as_deref()
                    .and_then(|path| taken_day(&fields, path, f.known, cancel))
                {
                    Some(day) => days.add_on(day, f.bytes, f.selected),
                    None => unknown += 1,
                }
            }
            Some((days, unknown))
        })
    }
}

impl Plugin for TimelinePlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Timeline"
    }
    fn description(&self) -> &'static str {
        "Files by modification date, as a histogram you can scrub"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Browsing
    }
    fn register(&self, app: &App) {
        if let Some(previous) = self.state.lock().lifetime.replace(app.lifetime().child()) {
            previous.cancel();
        }
        // The locale's first weekday is a process away: asked once, on a
        // worker, before anyone reads a week.
        app.scheduler.spawn(
            Priority::Background,
            "calendar.week",
            app.lifetime().child(),
            skwir_platform::calendar::prewarm,
        );
        let this = Arc::new(TimelinePlugin {
            state: Arc::clone(&self.state),
        });
        let t = Arc::clone(&this);
        app.commands
            .register::<HistogramArgs, _>(
                ID,
                "timeline.histogram",
                "Timeline",
                "Histogram of the listing's modification times (or creation, access, or pictures' clocks), or of the subtree's in the background",
                move |a, app| {
                    let asked = (a.date, a.bucket, a.window());
                    let pane = app.pane_or_active(a.pane);
                    let value = if a.subtree {
                        serde_json::to_value(t.subtree(app, pane, asked)?)?
                    } else if a.date == Date::Taken {
                        serde_json::to_value(t.listing_taken(app, pane, asked)?)?
                    } else {
                        let (date, bucket, window) = asked;
                        serde_json::to_value(t.listing_histogram(app, pane, date, bucket, window)?)?
                    };
                    Ok(Outcome::Value(value))
                },
            )
            .data();
        let t = Arc::clone(&this);
        app.commands
            .register::<StatusArgs, _>(
                ID,
                "timeline.status",
                "Timeline status",
                "The pane's subtree histogram, once the walk is done",
                move |a, app| {
                    let pane = app.pane_or_active(a.pane);
                    let status = t
                        .state
                        .lock()
                        .panes
                        .get(&pane)
                        .map(PaneState::status)
                        .unwrap_or_default();
                    Ok(Outcome::Value(serde_json::to_value(status)?))
                },
            )
            .data();
        let t = Arc::clone(&this);
        app.commands
            .register::<SelectArgs, _>(
                ID,
                "timeline.select",
                "Show files from a period",
                "Navigate to a query for files modified (or created, accessed, taken) in [start, end), or narrow the pane's search to them",
                move |a, app| {
                    let pane = app.pane_or_active(a.pane);
                    let (last_subtree, last_date) = t
                        .state
                        .lock()
                        .panes
                        .get(&pane)
                        .map_or((false, Date::default()), |ps| (ps.subtree, ps.date));
                    let date = a.date.unwrap_or(last_date);
                    // A bar starts at the first instant of a day of the user's
                    // calendar, the one the query's dates are read on.
                    let cal = Calendar::system();
                    let day = |secs: u64| cal.day_of(UNIX_EPOCH + Duration::from_secs(secs));
                    let Ok((dir, hidden, _)) = pane_view(app, pane) else {
                        // A search (no folder shown) is refined to the
                        // period, its words, root and depth kept: a bar of
                        // a bar's files is a click away, and Back returns.
                        return app.dispatch(&Invocation::try_with_args(
                            "query.refine",
                            &serde_json::json!({
                                "facet": date.word(),
                                "value": period_words(day(a.start), day(a.end)),
                                "pane": pane,
                            }),
                        )?);
                    };
                    let subtree = a.subtree.unwrap_or(last_subtree);
                    // One term, which a bar of the search then replaces
                    // (`query.refine`): never two that a refine would
                    // leave one of.
                    let mut expr = format!(
                        "{}:{}",
                        date.word(),
                        period_words(day(a.start), day(a.end))
                    );
                    if !subtree {
                        expr.push_str(" depth:1");
                    }
                    if hidden {
                        expr.push_str(" hidden:true");
                    }
                    // Pass the root as data; the query command owns query-language
                    // quoting, including apostrophes and whitespace in paths.
                    app.dispatch(&Invocation::try_with_args(
                        "query.search",
                        &serde_json::json!({
                            "expr": expr,
                            "root": PathArg::from(dir),
                            "pane": pane,
                        }),
                    )?)
                },
            )
            .shows();
        app.registry.keymap.add(
            ID,
            skwir_core::registry::KeyBinding {
                keys: "ctrl-shift-l".into(),
                command: Invocation::new("ui.timeline"),
                when: None,
            },
        );
        app.registry.menus.add(
            ID,
            skwir_core::registry::MenuItem {
                menu: "context".into(),
                title: "Timeline…".into(),
                command: Invocation::new("ui.timeline"),
                when: Some("empty folder".into()),
                order: 440,
                icon: Some("calendar".into()),
                submenu: None,
            },
        );
    }
    fn on_disable(&self, _app: &App) {
        let mut state = self.state.lock();
        if let Some(lifetime) = state.lifetime.take() {
            lifetime.cancel();
        }
        state.panes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i64, m: u32, d: u32) -> SystemTime {
        Calendar::utc().day_start(days_from_civil(y, m, d)) + Duration::from_secs(100)
    }

    fn histogram(
        samples: impl IntoIterator<Item = (SystemTime, u64)>,
        bucket: Option<Bucket>,
        unknown: u64,
    ) -> Histogram {
        let now = at(2026, 10, 25);
        super::histogram(
            samples,
            bucket,
            Window::Latest(now),
            unknown,
            &Calendar::utc(),
        )
    }

    /// The first day of each bar.
    fn starts(h: &Histogram) -> Vec<String> {
        let cal = Calendar::utc();
        h.bars
            .iter()
            .map(|b| date_string(cal.day_of(UNIX_EPOCH + Duration::from_secs(b.start))))
            .collect()
    }

    /// The user's days: a file modified at 23:30 UTC on the 25th is on the
    /// 26th in Paris, and the bar for it begins at Paris midnight.
    #[test]
    fn bars_are_the_users_days() {
        use skwir_platform::calendar::Weekday;
        let paris = Calendar::named("Europe/Paris", Weekday::Monday).unwrap();
        let late =
            Calendar::utc().day_start(days_from_civil(2026, 9, 25)) + Duration::from_mins(1410);
        let h = super::histogram([(late, 1)], Some(Bucket::Day), Window::default(), 0, &paris);
        assert_eq!(h.bars.len(), 1);
        let bar = &h.bars[0];
        let day26 = days_from_civil(2026, 9, 26);
        assert_eq!(bar.start, day_start_secs(day26, &paris));
        assert_eq!(bar.end - bar.start, 86_400);
        assert_eq!(
            bar.start,
            day_start_secs(day26, &Calendar::utc()) - 2 * 3600,
            "22:00 UTC, Paris midnight in summer"
        );
        // The day the clocks go back is a 25-hour bar.
        let back =
            Calendar::utc().day_start(days_from_civil(2026, 10, 25)) + Duration::from_hours(12);
        let h = super::histogram([(back, 1)], None, Window::default(), 0, &paris);
        assert_eq!(h.bucket, Bucket::Day, "the finest that fits");
        assert_eq!(h.bars[0].end - h.bars[0].start, 25 * 3600);
    }

    #[test]
    fn histogram_is_dense_and_counts_bytes() {
        let day = |d: u32| at(2026, 1, d);
        let h = histogram([(day(1), 10), (day(1), 5), (day(4), 1)], None, 2);
        assert_eq!((h.bucket, h.fits), (Bucket::Day, Bucket::Day));
        assert_eq!(h.bars.len(), 4, "days 1..=4, empty days included");
        assert_eq!(h.bars[0].count, 2);
        assert_eq!(h.bars[0].bytes, 15);
        assert_eq!(h.bars[1].count, 0);
        assert_eq!(h.bars[3].count, 1);
        assert_eq!(h.unknown, 2);
        assert_eq!(h.total, 5);
    }

    #[test]
    fn unasked_the_bars_are_the_finest_that_show_every_date() {
        // Ten years are too many days and weeks: months show them all.
        let ten = [(at(2015, 1, 1), 1), (at(2024, 12, 31), 1)];
        let h = histogram(ten, None, 0);
        assert_eq!(
            (h.bucket, h.fits, h.bars.len()),
            (Bucket::Month, Bucket::Month, 120)
        );
        assert_eq!((h.before, h.after), (0, 0));
        let h = histogram(ten, Some(Bucket::Year), 0);
        assert_eq!(
            (h.bucket, h.fits, h.bars.len()),
            (Bucket::Year, Bucket::Month, 10)
        );
        // A bogus far-future date is far outside: the others decide.
        let mut samples: Vec<_> = (1..=28).map(|d| (at(2026, 2, d), 1)).collect();
        // The year 9892: far, and still a time Windows can hold.
        samples.push((UNIX_EPOCH + Duration::from_secs(250_000_000_000), 1));
        let h = histogram(samples, None, 0);
        assert_eq!((h.bucket, h.bars.len()), (Bucket::Day, 28));
        assert_eq!((h.before, h.after, h.outside), (0, 0, 1));
        assert_eq!(h.bars.iter().map(|b| b.count).sum::<u64>(), 28);
        assert_eq!(h.total, 29);
    }

    #[test]
    fn a_stray_date_is_left_out_and_a_real_one_kept() {
        // 300 files over ten years, and a clock set to 2099 (and to 1970):
        // months show the ten years, the strays counted aside.
        let mut samples: Vec<_> = (0..300)
            .map(|i| (at(2016 + i % 10, 1 + (i as u32 % 12), 1), 1))
            .collect();
        samples.push((at(2099, 1, 1), 1));
        let h = histogram(samples.clone(), None, 0);
        assert_eq!((h.bucket, h.outside), (Bucket::Month, 1));
        assert_eq!(starts(&h)[0], "2016-01-01");
        assert_eq!(starts(&h).last().unwrap(), "2025-12-01");
        samples.push((UNIX_EPOCH, 1));
        let h = histogram(samples.clone(), None, 0);
        assert_eq!((h.bucket, h.outside, h.before), (Bucket::Month, 2, 0));
        // Fewer than a hundred files: nothing is a stray.
        let h = histogram(samples[250..].to_vec(), None, 0);
        assert_eq!(h.outside, 0);
        // A day before all the others is not far.
        let mut near: Vec<_> = (0..150).map(|_| (at(2026, 10, 20), 1)).collect();
        near.push((at(2026, 10, 19), 1));
        let h = histogram(near, None, 0);
        assert_eq!((h.bars.len(), h.outside), (2, 0));
        // Old scans, far before but many: they are dates of their own.
        let mut scans = samples[..300].to_vec();
        scans.extend((0..10).map(|_| (at(1998, 5, 5), 1)));
        let h = histogram(scans, None, 0);
        assert_eq!((h.bucket, h.outside), (Bucket::Month, 0));
        assert_eq!(starts(&h)[0], "1998-05-01");
    }

    #[test]
    fn a_width_too_fine_shows_a_window_of_it() {
        // One file in 2015, two on the last day of 2024.
        let ten = [
            (at(2015, 1, 1), 1),
            (at(2024, 12, 31), 1),
            (at(2024, 12, 31), 1),
        ];
        // Days, as asked, until the last date: the earlier one beside them.
        let h = histogram(ten, Some(Bucket::Day), 0);
        assert_eq!(
            (h.bucket, h.fits, h.bars.len()),
            (Bucket::Day, Bucket::Month, MAX_BARS)
        );
        assert_eq!(starts(&h).last().unwrap(), "2024-12-31");
        assert_eq!((h.before, h.after), (1, 0));
        assert_eq!(h.bars.last().unwrap().count, 2);
        let cal = Calendar::utc();
        let window = |bucket, window| super::histogram(ten, Some(bucket), window, 0, &cal);
        // From a day: the window starts at its bar.
        let h = window(Bucket::Day, Window::From(at(2020, 3, 4)));
        assert_eq!(starts(&h)[0], "2020-03-04");
        assert_eq!((h.bars.len(), h.before, h.after), (MAX_BARS, 1, 2));
        // Until one: it ends at its bar.
        let h = window(Bucket::Week, Window::Until(at(2024, 3, 4)));
        assert_eq!(h.bars.len(), MAX_BARS);
        assert_eq!(
            starts(&h).last().unwrap(),
            &date_string(cal.week_floor(days_from_civil(2024, 3, 4)))
        );
        // Past an end, a window keeps its length, against that end.
        let h = window(Bucket::Day, Window::From(at(2024, 12, 1)));
        assert_eq!(
            (h.bars.len(), starts(&h).last().unwrap().as_str()),
            (MAX_BARS, "2024-12-31")
        );
        let h = window(Bucket::Day, Window::Until(at(2001, 1, 1)));
        assert_eq!(
            (h.bars.len(), starts(&h)[0].as_str()),
            (MAX_BARS, "2015-01-01")
        );
        assert_eq!((h.before, h.after), (0, 2));
        // By default, until what changed last before now, not a bogus
        // date after it.
        let mut samples = ten.to_vec();
        samples.push((at(2040, 1, 1), 1));
        let h = super::histogram(
            samples,
            Some(Bucket::Day),
            Window::Latest(at(2026, 10, 25)),
            0,
            &cal,
        );
        assert_eq!(starts(&h).last().unwrap(), "2024-12-31");
        assert_eq!((h.before, h.after), (1, 1));
    }

    #[test]
    fn a_period_in_a_searchs_words() {
        let day = |y, m, d| days_from_civil(y, m, d);
        assert_eq!(period_words(day(2026, 7, 1), day(2026, 7, 2)), "2026-07-01");
        assert_eq!(period_words(day(2026, 7, 1), day(2026, 8, 1)), "2026-07");
        assert_eq!(period_words(day(2026, 12, 1), day(2027, 1, 1)), "2026-12");
        assert_eq!(period_words(day(2026, 1, 1), day(2027, 1, 1)), "2026");
        // A week across two months, a part of a month.
        assert_eq!(
            period_words(day(2026, 6, 29), day(2026, 7, 6)),
            "2026-06-29..2026-07-05"
        );
        assert_eq!(
            period_words(day(2026, 7, 1), day(2026, 7, 8)),
            "2026-07-01..2026-07-07"
        );
    }

    #[test]
    fn subtree_walk_counts_what_a_query_lists() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join("a/b/f1"), b"1").unwrap();
        std::fs::write(tmp.path().join("f2"), b"22").unwrap();
        std::fs::write(tmp.path().join(".git/x"), b"333").unwrap();
        let count = |d: &Days| d.days.values().map(|t| t.count).sum::<u64>();
        let bytes = |d: &Days| d.days.values().map(|t| t.bytes).sum::<u64>();
        let fields = skwir_core::fields::Fields::default();
        let walk = |hidden, cancel: &CancelToken| {
            walk_days(tmp.path(), hidden, Date::Modified, &fields, cancel)
        };
        // a, a/b, a/b/f1, f2: folders count, hidden entries do not.
        let (d, unknown) = walk(false, &CancelToken::new()).unwrap();
        assert_eq!((count(&d), bytes(&d), unknown), (4, 3, 0));
        let (d, _) = walk(true, &CancelToken::new()).unwrap();
        assert_eq!((count(&d), bytes(&d)), (6, 6));
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(walk(false, &cancelled).is_none());
    }

    /// Answers a picture's clock from the digits the file holds.
    struct Clocks;

    impl skwir_core::fields::FieldProvider for Clocks {
        fn id(&self) -> &'static str {
            "test-clocks"
        }
        fn keys(&self) -> &'static [&'static str] {
            &[keys::TAKEN]
        }
        fn accepts(&self, _: &str, name: &std::ffi::OsStr) -> bool {
            name.to_string_lossy().ends_with(".jpg")
        }
        fn read(&self, _: &str, path: &Path, _: &CancelToken) -> anyhow::Result<Option<Value>> {
            Ok(std::fs::read_to_string(path)?
                .trim()
                .parse()
                .ok()
                .map(Value::UInt))
        }
    }

    #[test]
    fn a_walk_counts_pictures_by_the_day_their_camera_says() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("trip")).unwrap();
        std::fs::write(tmp.path().join("trip/one.jpg"), b"20230415083000").unwrap();
        std::fs::write(tmp.path().join("trip/two.jpg"), b"20230415233000").unwrap();
        std::fs::write(tmp.path().join("no clock.jpg"), b"").unwrap();
        std::fs::write(tmp.path().join("notes.txt"), b"20990101000000").unwrap();
        let fields = skwir_core::fields::Fields::default();
        fields.register("test", Arc::new(Clocks));
        let (d, unknown) =
            walk_days(tmp.path(), false, Date::Taken, &fields, &CancelToken::new()).unwrap();
        // The day its digits say, in no zone; the folder, the text and the
        // picture without a clock have no such date.
        assert_eq!(
            d.days.keys().copied().collect::<Vec<_>>(),
            [days_from_civil(2023, 4, 15)]
        );
        assert_eq!((d.days.values().next().unwrap().count, unknown), (2, 3));
        assert_eq!(clock_day(20_230_231_000_000), None, "no such day");
    }

    #[test]
    fn bars_count_the_selection_and_pictures_clocks_of_a_listing() {
        use skwir_core::app::AppConfig;
        use skwir_core::location::Location;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("r");
        std::fs::create_dir_all(&root).unwrap();
        for (name, clock) in [("a.jpg", "20230415083000"), ("b.jpg", "20230416083000")] {
            std::fs::write(root.join(name), clock).unwrap();
        }
        std::fs::write(root.join("c.txt"), b"text").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins.add(Arc::new(TimelinePlugin::default()), &app);
        app.fields.register("test", Arc::new(Clocks));
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let ids = app.workspace.read().pane(pane).unwrap().order();
        app.dispatch(&Invocation::with_args(
            "hydrate",
            &serde_json::json!({ "ids": ids, "pane": pane }),
        ))
        .unwrap();
        let ask = |name: &str, args: serde_json::Value| -> serde_json::Value {
            match app.dispatch(&Invocation::with_args(name, &args)).unwrap() {
                Outcome::Value(v) => v,
                other => panic!("{name}: {other:?}"),
            }
        };
        let a = {
            let ws = app.workspace.read();
            let p = ws.pane(pane).unwrap();
            p.listing.iter().find(|e| e.name == "a.jpg").unwrap().id
        };
        app.workspace
            .write()
            .pane_mut(pane)
            .unwrap()
            .selection
            .set([a]);
        // Modified: all three today, one of them selected.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let h = loop {
            let h = ask("timeline.histogram", serde_json::json!({}));
            if h["unknown"] == 0 {
                break h;
            }
            assert!(std::time::Instant::now() < deadline, "never hydrated");
            app.pump();
            std::thread::sleep(Duration::from_millis(5));
        };
        let bars = h["bars"].as_array().unwrap();
        assert_eq!(
            (bars.len(), &bars[0]["count"], &bars[0]["selected"]),
            (1, &3.into(), &1.into())
        );
        assert_eq!(bars[0]["selected_bytes"], 14);
        // Taken: two days, the text without a clock, read in the background.
        let first = ask("timeline.histogram", serde_json::json!({ "date": "taken" }));
        assert!(first.get("scanning").is_some(), "{first}");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let done = loop {
            let s = ask("timeline.status", serde_json::json!({}));
            if !s["histogram"].is_null() && s["scanning"] == false {
                break s;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the clocks were never read"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        let h = &done["histogram"];
        assert_eq!(
            (h["date"].as_str(), h["unknown"].as_u64()),
            (Some("taken"), Some(1))
        );
        let bars = h["bars"].as_array().unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(
            (&bars[0]["count"], &bars[0]["selected"]),
            (&1.into(), &1.into())
        );
    }

    #[test]
    fn a_subtree_walk_answers_once_and_is_not_restarted() {
        use skwir_core::app::AppConfig;
        use skwir_core::location::Location;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("r");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("a/f"), b"1").unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
        app.plugins.add(Arc::new(TimelinePlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let ask = |name: &str, args: serde_json::Value| -> serde_json::Value {
            match app.dispatch(&Invocation::with_args(name, &args)).unwrap() {
                Outcome::Value(v) => v,
                other => panic!("{name}: {other:?}"),
            }
        };
        let first = ask(
            "timeline.histogram",
            serde_json::json!({"subtree": true, "bucket": "year"}),
        );
        assert_eq!(first["root"], serde_json::json!(root));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let done = loop {
            let s = ask("timeline.status", serde_json::json!({}));
            if !s["histogram"].is_null() {
                break s;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the walk never answered"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(done["scanning"], false);
        assert_eq!(done["histogram"]["total"], 2);
        // Asking again (the listing changed) answers at once from the walk.
        let again = ask(
            "timeline.histogram",
            serde_json::json!({"subtree": true, "bucket": "day"}),
        );
        assert_eq!(again["scanning"], false);
        assert_eq!(again["histogram"]["total"], 2);
        assert_eq!(again["histogram"]["bucket"], "day");
    }
}
