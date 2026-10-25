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
use skwir_platform::calendar::{Calendar, Period, civil_from_days, date_string, days_from_civil};

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
    /// The bucket the bars use: the one asked for, else `fits`.
    pub bucket: Bucket,
    /// The finest bucket whose bars show every date (but those far
    /// outside) at once; a finer one shows a window of [`MAX_BARS`] bars.
    pub fits: Bucket,
    pub bars: Vec<Bar>,
    /// Entries without a modification time (not hydrated yet).
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

    /// Bars of `bucket`, by default the finest that shows every date; a
    /// bucket that would need more than [`MAX_BARS`] bars shows `window`.
    pub fn histogram(&self, bucket: Option<Bucket>, window: Window, unknown: u64) -> Histogram {
        let cal = &self.calendar;
        let total = unknown + self.days.values().map(|t| t.count).sum::<u64>();
        let mut h = Histogram {
            bucket: bucket.unwrap_or_default(),
            fits: Bucket::default(),
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
        SubtreeStatus {
            root: Some(PathArg::from(walk.dir.as_path())),
            scanning: walk.days.is_none() && !walk.cancel.is_cancelled(),
            histogram: walk
                .days
                .as_ref()
                .map(|d| d.histogram(self.bucket, self.window, 0)),
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

/// Every entry below `root` (not `root` itself), folded by day, as a search
/// walks: never into a network mount below it (ADR-077). `None` when
/// cancelled. Run in `root`'s lane: a remote's folder is walked there one
/// after the other, never on rayon's threads, which no lane holds.
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
    let fold = |(mut a, mut a_dirs): (Days, Vec<PathBuf>), (b, b_dirs): (Days, Vec<PathBuf>)| {
        a.merge(b);
        a_dirs.extend(b_dirs);
        (a, a_dirs)
    };
    let remote = reach::of(root).is_some();
    let mut days = Days::default();
    let mut level = vec![root.to_path_buf()];
    while !level.is_empty() {
        if cancel.is_cancelled() {
            return None;
        }
        let (d, next) = if remote {
            level
                .iter()
                .map(|dir| one(dir, hidden, cancel))
                .fold((Days::default(), Vec::new()), fold)
        } else {
            level
                .par_iter()
                .map(|dir| one(dir, hidden, cancel))
                .reduce(|| (Days::default(), Vec::new()), fold)
        };
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
        bucket: Option<Bucket>,
        window: Window,
    ) -> anyhow::Result<Histogram> {
        let mut unknown = 0;
        let mut days = Days::default();
        let withheld = app.withheld(pane);
        // What the pane shows (`Pane::order`): hidden files and companions
        // as it shows them, filtered by name, narrowed.
        let order = app
            .workspace
            .read()
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?
            .order();
        // Read a slice at a time, each under a hold of the lock of its
        // own: the window's thread applies what the core sends meanwhile.
        // The days are told after the slice lets the lock go.
        let mut dated: Vec<(SystemTime, u64)> = Vec::new();
        for slice in order.chunks(SLICE) {
            dated.clear();
            {
                let ws = app.workspace.read();
                let p = ws
                    .pane(pane)
                    .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
                for e in slice
                    .iter()
                    .filter(|id| !withheld.contains(id))
                    .filter_map(|id| p.entry(*id))
                {
                    match e.metadata.value(keys::MODIFIED).and_then(Value::as_time) {
                        Some(t) => dated.push((
                            t,
                            e.metadata
                                .value(keys::SIZE)
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                        )),
                        None => unknown += 1,
                    }
                }
            }
            for &(t, bytes) in &dated {
                days.add(t, bytes);
            }
        }
        let mut state = self.state.lock();
        let ps = state.panes.entry(pane).or_default();
        ps.subtree = false;
        ps.bucket = bucket;
        ps.window = window;
        Ok(days.histogram(bucket, window, unknown))
    }

    /// Start walking the pane's folder unless a walk of it (running or
    /// done) is already there for this visit: the UI asks again whenever
    /// the listing changes, and that must not restart it.
    fn subtree(
        &self,
        app: &App,
        pane: PaneId,
        bucket: Option<Bucket>,
        window: Window,
    ) -> anyhow::Result<SubtreeStatus> {
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
        ps.window = window;
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
            app.scheduler.spawn_in(
                reach::lane(&dir),
                Priority::Background,
                "timeline.walk",
                cancel,
                move || {
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
                },
            );
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
                "Histogram of modification times for the listing, or the subtree in the background",
                move |a, app| {
                    let (bucket, window) = (a.bucket, a.window());
                    let pane = app.pane_or_active(a.pane);
                    let value = if a.subtree {
                        serde_json::to_value(t.subtree(app, pane, bucket, window)?)?
                    } else {
                        serde_json::to_value(t.listing_histogram(app, pane, bucket, window)?)?
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
                "Navigate to a query for files modified in [start, end), or narrow the pane's search to them",
                move |a, app| {
                    let pane = app.pane_or_active(a.pane);
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
                                "facet": "modified",
                                "value": period_words(day(a.start), day(a.end)),
                                "pane": pane,
                            }),
                        )?);
                    };
                    let subtree = a.subtree.unwrap_or_else(|| {
                        t.state.lock().panes.get(&pane).is_some_and(|ps| ps.subtree)
                    });
                    // One term, which a bar of the search then replaces
                    // (`query.refine`): never two that a refine would
                    // leave one of.
                    let mut expr = format!("modified:{}", period_words(day(a.start), day(a.end)));
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
