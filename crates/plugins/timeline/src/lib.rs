//! bibli plugin: timeline. Files by modification time as a histogram the
//! UI renders as a scrubbable bar chart; a bucket becomes a `query:`
//! location (`modified>=start modified<end in:dir`) when selected.
//!
//! Commands: `timeline.histogram { pane?, bucket?, subtree? }` returns the
//! buckets for the pane's listing (hydrated entries only; the UI hydrates
//! everything first) or, with `subtree`, the [`SubtreeStatus`] of a
//! background walk of the pane's folder, which `timeline.status { pane? }`
//! returns until the walk is done. `timeline.select { start, end, pane?,
//! subtree? }` navigates to the matching query.
//!
//! What a bar counts is what clicking it shows: the entries the query lists
//! (files, folders and links, hidden ones only when the pane shows hidden
//! entries), from the listing (`depth:1`) or from the whole subtree.
//!
//! Buckets are periods of the user's calendar (`bibli_platform::calendar`),
//! like the query language's dates: days from local midnight, daylight
//! saving time included, and weeks from the locale's first weekday. Entries
//! are folded by the civil day they fall on; a bar's instants are the
//! starts of its first day and of the day after its last.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{Value, keys};
use bibli_core::os_path::PathArg;
use bibli_core::plugin::Plugin;
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use bibli_platform::calendar::{Calendar, civil_from_days, date_string, days_from_civil};
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const ID: &str = "timeline";
/// 9999-12-31T23:59:59Z: later times (a bogus mtime) count as that instant.
const LAST_SECOND: u64 = 253_402_300_799;
/// At most this many bars: a finer bucket that would need more is widened
/// (day, week, month, year), and entries outside the busiest span of years
/// are counted as `outside`.
pub const MAX_BARS: usize = 400;

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

    /// First day of the bucket holding civil day `day`.
    pub fn floor(self, day: i64, cal: &Calendar) -> i64 {
        match self {
            Bucket::Day => day,
            Bucket::Week => cal.week_floor(day),
            Bucket::Month => {
                let (y, m, _) = civil_from_days(day);
                days_from_civil(y, m, 1)
            }
            Bucket::Year => days_from_civil(civil_from_days(day).0, 1, 1),
        }
    }

    /// First day of the bucket after the one starting on `start`.
    pub fn next(self, start: i64) -> i64 {
        match self {
            Bucket::Day => start + 1,
            Bucket::Week => start + 7,
            Bucket::Month => {
                let (y, m, _) = civil_from_days(start);
                let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                days_from_civil(y, m, 1)
            }
            Bucket::Year => days_from_civil(civil_from_days(start).0 + 1, 1, 1),
        }
    }

    /// How many buckets `[first, last]` spans (both are bucket starts).
    fn span(self, first: i64, last: i64) -> u64 {
        let month = |day: i64| {
            let (y, m, _) = civil_from_days(day);
            y * 12 + i64::from(m)
        };
        let n = match self {
            Bucket::Day => last - first,
            Bucket::Week => (last - first) / 7,
            Bucket::Month => month(last) - month(first),
            Bucket::Year => (month(last) - month(first)) / 12,
        };
        u64::try_from(n).unwrap_or(0) + 1
    }
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
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct Histogram {
    /// The bucket the bars use: wider than the one asked for when that one
    /// would need more than [`MAX_BARS`] bars.
    pub bucket: Bucket,
    pub bars: Vec<Bar>,
    /// Entries without a modification time (not hydrated yet).
    pub unknown: u64,
    /// Entries dated outside the bars (far from the others).
    #[serde(default)]
    pub outside: u64,
    pub total: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    count: u64,
    bytes: u64,
}

impl Tally {
    fn add(&mut self, other: Tally) {
        self.count += other.count;
        self.bytes = self.bytes.saturating_add(other.bytes);
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
        let secs = t
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
            .min(LAST_SECOND);
        let day = self.calendar.day_of(UNIX_EPOCH + Duration::from_secs(secs));
        self.days
            .entry(day)
            .or_default()
            .add(Tally { count: 1, bytes });
    }

    fn merge(&mut self, other: Days) {
        for (day, tally) in other.days {
            self.days.entry(day).or_default().add(tally);
        }
    }

    /// Bars for `bucket` (or the first wider one that fits in [`MAX_BARS`]).
    pub fn histogram(&self, bucket: Bucket, unknown: u64) -> Histogram {
        let cal = &self.calendar;
        let total = unknown + self.days.values().map(|t| t.count).sum::<u64>();
        let mut h = Histogram {
            bucket,
            bars: Vec::new(),
            unknown,
            outside: 0,
            total,
        };
        let (Some(&first), Some(&last)) = (self.days.keys().next(), self.days.keys().next_back())
        else {
            return h;
        };
        let fits = |b: Bucket| b.span(b.floor(first, cal), b.floor(last, cal)) <= MAX_BARS as u64;
        h.bucket = Bucket::ALL
            .into_iter()
            .filter(|b| *b >= bucket)
            .find(|b| fits(*b))
            .unwrap_or(Bucket::Year);
        let (from, to) = if fits(h.bucket) {
            (first, last)
        } else {
            self.busiest_years()
        };
        let mut sums: BTreeMap<i64, Tally> = BTreeMap::new();
        for (&day, &tally) in &self.days {
            if day < from || day > to {
                h.outside += tally.count;
            } else {
                sums.entry(h.bucket.floor(day, cal)).or_default().add(tally);
            }
        }
        let mut s = h.bucket.floor(from, cal);
        while s <= to {
            let tally = sums.get(&s).copied().unwrap_or_default();
            let end = h.bucket.next(s);
            h.bars.push(Bar {
                start: day_start_secs(s, cal),
                end: day_start_secs(end, cal),
                count: tally.count,
                bytes: tally.bytes,
            });
            s = end;
        }
        h
    }

    /// The span of at most [`MAX_BARS`] years holding the most entries, as
    /// `(first day, last day)`: a few bogus far-future or far-past dates
    /// must not squeeze the real ones into one bar.
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

/// Bucket `(mtime, size)` samples into a dense, contiguous histogram of
/// `cal`'s periods.
pub fn histogram(
    samples: impl IntoIterator<Item = (SystemTime, u64)>,
    bucket: Bucket,
    unknown: u64,
    cal: &Calendar,
) -> Histogram {
    let mut days = Days::new(cal.clone());
    for (t, bytes) in samples {
        days.add(t, bytes);
    }
    days.histogram(bucket, unknown)
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct HistogramArgs {
    #[serde(default)]
    pub pane: Option<PaneId>,
    #[serde(default)]
    pub bucket: Option<Bucket>,
    /// Walk the whole subtree in the background instead of the listing.
    #[serde(default)]
    pub subtree: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SelectArgs {
    /// A bar's start, seconds since the epoch: the first instant of a day.
    pub start: u64,
    pub end: u64,
    #[serde(default)]
    pub pane: Option<PaneId>,
    /// Search the subtree rather than the folder itself; by default, what
    /// the pane's last histogram counted.
    #[serde(default)]
    pub subtree: Option<bool>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct StatusArgs {
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct SubtreeStatus {
    pub root: Option<PathArg>,
    pub scanning: bool,
    pub histogram: Option<Histogram>,
}

/// One subtree walk: of which folder, with or without hidden entries.
struct Walk {
    dir: PathBuf,
    hidden: bool,
    generation: u64,
    /// Ends with the pane's visit to the folder and with the plugin.
    cancel: CancelToken,
    days: Option<Arc<Days>>,
}

#[derive(Default)]
struct PaneState {
    /// What the last histogram counted, for `timeline.select`.
    subtree: bool,
    bucket: Bucket,
    walk: Option<Walk>,
}

impl PaneState {
    fn status(&self) -> SubtreeStatus {
        let Some(walk) = &self.walk else {
            return SubtreeStatus::default();
        };
        SubtreeStatus {
            root: Some(PathArg::from(walk.dir.as_path())),
            scanning: walk.days.is_none() && !walk.cancel.is_cancelled(),
            histogram: walk.days.as_ref().map(|d| d.histogram(self.bucket, 0)),
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

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

/// Every entry below `root` (not `root` itself), folded by day. `None` when
/// cancelled.
fn walk_days(root: &Path, hidden: bool, cancel: &CancelToken) -> Option<Days> {
    use rayon::prelude::*;
    fn one(dir: &Path, hidden: bool, cancel: &CancelToken) -> (Days, Vec<PathBuf>) {
        let mut days = Days::default();
        let mut subdirs = Vec::new();
        if cancel.is_cancelled() {
            return (days, subdirs);
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if !hidden && is_hidden(&e.file_name()) {
                    continue;
                }
                // `DirEntry::metadata` does not follow links: a link is
                // counted by its own date, and never walked into.
                let Ok(m) = e.metadata() else { continue };
                if let Ok(t) = m.modified() {
                    days.add(t, if m.is_dir() { 0 } else { m.len() });
                }
                if m.is_dir() {
                    subdirs.push(e.path());
                }
            }
        }
        (days, subdirs)
    }
    let mut days = Days::default();
    let mut level = vec![root.to_path_buf()];
    while !level.is_empty() {
        if cancel.is_cancelled() {
            return None;
        }
        let (d, next) = level.par_iter().map(|dir| one(dir, hidden, cancel)).reduce(
            || (Days::default(), Vec::new()),
            |(mut a, mut a_dirs), (b, b_dirs)| {
                a.merge(b);
                a_dirs.extend(b_dirs);
                (a, a_dirs)
            },
        );
        days.merge(d);
        level = next;
    }
    (!cancel.is_cancelled()).then_some(days)
}

impl TimelinePlugin {
    fn listing_histogram(
        &self,
        app: &App,
        pane: PaneId,
        bucket: Bucket,
    ) -> anyhow::Result<Histogram> {
        let mut unknown = 0;
        let mut days = Days::default();
        {
            let ws = app.workspace.read();
            let p = ws
                .pane(pane)
                .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
            for e in p.listing.iter() {
                if !p.sort.show_hidden && is_hidden(&e.name) {
                    continue;
                }
                match e.metadata.value(keys::MODIFIED).and_then(Value::as_time) {
                    Some(t) => days.add(
                        t,
                        e.metadata
                            .value(keys::SIZE)
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    ),
                    None => unknown += 1,
                }
            }
        }
        let mut state = self.state.lock();
        let ps = state.panes.entry(pane).or_default();
        ps.subtree = false;
        ps.bucket = bucket;
        Ok(days.histogram(bucket, unknown))
    }

    /// Start walking the pane's folder unless a walk of it (running or
    /// done) is already there for this visit: the UI asks again whenever
    /// the listing changes, and that must not restart it.
    fn subtree(&self, app: &App, pane: PaneId, bucket: Bucket) -> anyhow::Result<SubtreeStatus> {
        let (dir, hidden, visit) = pane_view(app, pane)?;
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
        ps.subtree = true;
        ps.bucket = bucket;
        let current = ps
            .walk
            .as_ref()
            .is_some_and(|w| w.dir == dir && w.hidden == hidden && !w.cancel.is_cancelled());
        if !current {
            if let Some(old) = ps.walk.take() {
                old.cancel.cancel();
            }
            let cancel = lifetime.linked(&visit);
            ps.walk = Some(Walk {
                dir: dir.clone(),
                hidden,
                generation,
                cancel: cancel.clone(),
                days: None,
            });
            let shared = Arc::clone(&self.state);
            let wake = app.waker_handle();
            let task_cancel = cancel.clone();
            app.scheduler
                .spawn(Priority::Background, "timeline.walk", cancel, move || {
                    let Some(days) = walk_days(&dir, hidden, &task_cancel) else {
                        return;
                    };
                    let mut s = shared.lock();
                    // Only the walk this pane still waits for may answer.
                    if let Some(walk) = s
                        .panes
                        .get_mut(&pane)
                        .and_then(|ps| ps.walk.as_mut())
                        .filter(|w| w.generation == generation)
                    {
                        walk.days = Some(Arc::new(days));
                        drop(s);
                        wake();
                    }
                });
        }
        Ok(ps.status())
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
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Browsing
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
            bibli_platform::calendar::prewarm,
        );
        let this = Arc::new(TimelinePlugin {
            state: Arc::clone(&self.state),
        });
        let t = Arc::clone(&this);
        app.commands.register::<HistogramArgs, _>(
            ID,
            "timeline.histogram",
            "Timeline",
            "Histogram of modification times for the listing, or the subtree in the background",
            move |a, app| {
                let bucket = a.bucket.unwrap_or_default();
                let pane = app.pane_or_active(a.pane);
                let value = if a.subtree {
                    serde_json::to_value(t.subtree(app, pane, bucket)?)?
                } else {
                    serde_json::to_value(t.listing_histogram(app, pane, bucket)?)?
                };
                Ok(Outcome::Value(value))
            },
        );
        let t = Arc::clone(&this);
        app.commands.register::<StatusArgs, _>(
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
        );
        let t = Arc::clone(&this);
        app.commands.register::<SelectArgs, _>(
            ID,
            "timeline.select",
            "Show files from a period",
            "Navigate to a query for files modified in [start, end)",
            move |a, app| {
                let pane = app.pane_or_active(a.pane);
                let (dir, hidden, _) = pane_view(app, pane)?;
                let subtree = a.subtree.unwrap_or_else(|| {
                    t.state.lock().panes.get(&pane).is_some_and(|ps| ps.subtree)
                });
                // Date ranges include the whole end date in the query language;
                // histogram buckets instead exclude their end boundary.
                // A bar starts at the first instant of a day of the user's
                // calendar, the one the query's dates are read on.
                let cal = Calendar::system();
                let day = |secs: u64| cal.day_of(UNIX_EPOCH + Duration::from_secs(secs));
                let mut expr = format!(
                    "modified>={} modified<{}",
                    date_string(day(a.start)),
                    date_string(day(a.end))
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
        );
        app.registry.keymap.add(
            ID,
            bibli_core::registry::KeyBinding {
                keys: "ctrl-shift-l".into(),
                command: Invocation::new("ui.timeline"),
                when: None,
            },
        );
        app.registry.menus.add(
            ID,
            bibli_core::registry::MenuItem {
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
        bucket: Bucket,
        unknown: u64,
    ) -> Histogram {
        super::histogram(samples, bucket, unknown, &Calendar::utc())
    }

    #[test]
    fn buckets_are_calendar_periods() {
        let cal = Calendar::utc();
        let day = days_from_civil(2026, 9, 18);
        let s = Bucket::Month.floor(day, &cal);
        assert_eq!(date_string(s), "2026-09-01");
        assert_eq!(date_string(Bucket::Month.next(s)), "2026-10-01");
        assert_eq!(
            date_string(Bucket::Year.next(Bucket::Year.floor(s, &cal))),
            "2027-01-01"
        );
        assert_eq!(
            date_string(Bucket::Month.next(days_from_civil(2026, 12, 1))),
            "2027-01-01"
        );
    }

    #[test]
    fn weeks_start_on_the_calendars_day() {
        let monday = Calendar::utc();
        // 2026-09-18 is a Friday; its week starts Monday 2026-09-14.
        let friday = days_from_civil(2026, 9, 18);
        let start = Bucket::Week.floor(friday, &monday);
        assert_eq!(date_string(start), "2026-09-14");
        assert_eq!(date_string(Bucket::Week.next(start)), "2026-09-21");
        // A Monday is its own week's start; the Sunday before is not.
        let m = days_from_civil(2026, 9, 14);
        assert_eq!(Bucket::Week.floor(m, &monday), m);
        assert_eq!(
            date_string(Bucket::Week.floor(m - 1, &monday)),
            "2026-09-07"
        );
        // Weeks of a locale that starts them on Sunday.
        let sunday = Calendar::new(jiff_tz_utc(), bibli_platform::calendar::Weekday::Sunday);
        assert_eq!(
            date_string(Bucket::Week.floor(friday, &sunday)),
            "2026-09-13"
        );
        for (first, last) in [(0, 0), (0, 4), (0, 11), (friday, friday + 30)] {
            let (first, last) = (
                Bucket::Week.floor(first, &monday),
                Bucket::Week.floor(last, &monday),
            );
            let mut n = 1;
            let mut s = first;
            while s < last {
                s = Bucket::Week.next(s);
                n += 1;
            }
            assert_eq!(Bucket::Week.span(first, last), n);
        }
    }

    fn jiff_tz_utc() -> bibli_platform::calendar::TimeZone {
        Calendar::utc().time_zone().clone()
    }

    /// The user's days: a file modified at 23:30 UTC on the 25th is on the
    /// 26th in Paris, and the bar for it begins at Paris midnight.
    #[test]
    fn bars_are_the_users_days() {
        use bibli_platform::calendar::Weekday;
        let paris = Calendar::named("Europe/Paris", Weekday::Monday).unwrap();
        let late = Calendar::utc().day_start(days_from_civil(2026, 9, 25))
            + Duration::from_secs(23 * 3600 + 1800);
        let h = super::histogram([(late, 1)], Bucket::Day, 0, &paris);
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
            Calendar::utc().day_start(days_from_civil(2026, 10, 25)) + Duration::from_secs(43_200);
        let h = super::histogram([(back, 1)], Bucket::Day, 0, &paris);
        assert_eq!(h.bars[0].end - h.bars[0].start, 25 * 3600);
    }

    #[test]
    fn histogram_is_dense_and_counts_bytes() {
        let day = |d: u32| at(2026, 1, d);
        let h = histogram([(day(1), 10), (day(1), 5), (day(4), 1)], Bucket::Day, 2);
        assert_eq!(h.bars.len(), 4, "days 1..=4, empty days included");
        assert_eq!(h.bars[0].count, 2);
        assert_eq!(h.bars[0].bytes, 15);
        assert_eq!(h.bars[1].count, 0);
        assert_eq!(h.bars[3].count, 1);
        assert_eq!(h.unknown, 2);
        assert_eq!(h.total, 5);
    }

    #[test]
    fn bars_are_bounded() {
        // Ten years are too many days and weeks: the bucket widens.
        let h = histogram([(at(2015, 1, 1), 1), (at(2024, 12, 31), 1)], Bucket::Day, 0);
        assert_eq!(h.bucket, Bucket::Month);
        assert_eq!(h.bars.len(), 120);
        let h = histogram(
            [(at(2015, 1, 1), 1), (at(2024, 12, 31), 1)],
            Bucket::Year,
            0,
        );
        assert_eq!((h.bucket, h.bars.len()), (Bucket::Year, 10));
        // A bogus far-future date does not flatten everything else.
        let mut samples: Vec<_> = (1..=28).map(|d| (at(2026, 2, d), 1)).collect();
        samples.push((UNIX_EPOCH + Duration::from_secs(u64::MAX / 2), 1));
        let h = histogram(samples, Bucket::Day, 0);
        assert_eq!(h.bucket, Bucket::Year);
        assert!(h.bars.len() <= MAX_BARS);
        assert_eq!(h.outside, 1);
        assert_eq!(h.bars.iter().map(|b| b.count).sum::<u64>(), 28);
        assert_eq!(h.total, 29);
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
        // a, a/b, a/b/f1, f2: folders count, hidden entries do not.
        let d = walk_days(tmp.path(), false, &CancelToken::new()).unwrap();
        assert_eq!((count(&d), bytes(&d)), (4, 3));
        let d = walk_days(tmp.path(), true, &CancelToken::new()).unwrap();
        assert_eq!((count(&d), bytes(&d)), (6, 6));
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert!(walk_days(tmp.path(), false, &cancelled).is_none());
    }

    #[test]
    fn a_subtree_walk_answers_once_and_is_not_restarted() {
        use bibli_core::app::AppConfig;
        use bibli_core::location::Location;
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
