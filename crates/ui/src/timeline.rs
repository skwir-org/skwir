//! Timeline strip: a histogram of when files changed, under the file area.
//! The plugin counts it (`timeline.histogram`), on a worker: a count reads
//! every file the pane shows, which the window's thread does not wait for.
//!
//! The bars are the finest that show every date until the user picks a
//! width (`ui.timeline_bucket`), which holds while the pane shows what it
//! showed then: each place opens on its whole span. A width too fine for
//! every date shows a window of bars, which the wheel and the strip's
//! arrows move (`ui.timeline_pan`). The width pressed is the width shown.
//!
//! The bars measure files or their bytes (`ui.timeline_measure`), by the
//! time the user chose (`ui.timeline_date`: modified, created, accessed,
//! or a picture's clock), and mark the files selected and today's bar.
//! The strip takes the keyboard (F6, a click): the arrows go from bar to
//! bar, Shift extends, Enter lists the bars chosen, as a drag across them
//! does (`ui.timeline_cursor`, `ui.timeline_list`). To a screen reader it
//! is a list of its bars.

use std::fmt::Write as _;
use std::rc::Rc;

use gpui::{AnyElement, Context, FocusHandle, Window, div, prelude::*, px};
use serde::Deserialize;
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::Location;
use skwir_core::workspace::PaneId;
use skwir_platform::calendar::{Calendar, Period, civil_from_days};

use crate::elements::bars::{self, BarColors, bar_chart};
use crate::elements::popover::PopoverAnchor;
use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::{self, Button};

pub const BUCKETS: [&str; 4] = ["day", "week", "month", "year"];
/// Each bucket's period on the user's calendar.
const PERIODS: [Period; 4] = [Period::Day, Period::Week, Period::Month, Period::Year];
const PLURALS: [&str; 4] = ["days", "weeks", "months", "years"];
const DAY: usize = 0;
/// The times the bars can count, as the plugin and the query language
/// name them, and how the strip's menu says them.
pub const DATES: [(&str, &str); 4] = [
    ("modified", "When modified"),
    ("created", "When created"),
    ("accessed", "When last opened"),
    ("taken", "When taken (pictures and videos)"),
];
const TAKEN: usize = 3;
/// The popup choosing the time (`ui.toolbar_menu`'s `kind`).
pub const DATES_MENU: &str = "timeline_dates";
/// The chart under the strip's controls: bars, their counts and labels.
const CHART_H: f32 = 84.0;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Bar {
    pub start: u64,
    pub end: u64,
    pub count: u64,
    pub bytes: u64,
    /// Of them, the files the pane has selected, and their bytes.
    #[serde(default)]
    pub selected: u64,
    #[serde(default)]
    pub selected_bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Histogram {
    /// The bucket the bars use.
    #[serde(default)]
    pub bucket: Option<String>,
    /// The finest bucket that shows every date at once.
    #[serde(default)]
    pub fits: Option<String>,
    #[serde(default)]
    pub bars: Vec<Bar>,
    #[serde(default)]
    pub unknown: u64,
    /// Dated entries before the bars and after them: outside the window.
    #[serde(default)]
    pub before: u64,
    #[serde(default)]
    pub after: u64,
    /// Entries dated far from the others, never shown.
    #[serde(default)]
    pub outside: u64,
    #[serde(default)]
    pub total: u64,
}

impl Histogram {
    /// The bucket the bars use, as an index into `BUCKETS`.
    fn shown(&self) -> Option<usize> {
        let name = self.bucket.as_deref()?;
        BUCKETS.iter().position(|b| *b == name)
    }

    fn fits(&self) -> Option<usize> {
        let name = self.fits.as_deref()?;
        BUCKETS.iter().position(|b| *b == name)
    }

    /// Whether the bars are a window of a width too fine for every date.
    pub fn windowed(&self) -> bool {
        matches!((self.shown(), self.fits()), (Some(shown), Some(fits)) if shown < fits)
    }

    /// The bar holding instant `t`, or the one at the end it lies past.
    fn bar_of(&self, t: u64) -> Option<usize> {
        let last = self.bars.len().checked_sub(1)?;
        Some(self.bars.partition_point(|b| b.end <= t).min(last))
    }

    /// The instants the bars chosen span, `[start, end)`: from the bar a
    /// range began at (`from`, its first instant) to the keys' bar, the
    /// one drawn. A range longer than a window of bars began outside it:
    /// that end is its own period, the bars' width on `cal`, never the
    /// window's end bar, so what lies past the window stays chosen.
    fn span(&self, from: Option<u64>, cursor: u64, cal: &Calendar) -> Option<(u64, u64)> {
        let (first, last) = (self.bars.first()?, self.bars.last()?);
        let at = &self.bars[self.bar_of(cursor)?];
        let (mut start, mut end) = (at.start, at.end);
        if let Some(from) = from {
            let began = &self.bars[self.bar_of(from)?];
            (start, end) = (start.min(began.start), end.max(began.end));
            let seconds = |day: i64| {
                cal.day_start(day)
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs())
            };
            if let Some(period) = self.shown().map(|i| PERIODS[i]) {
                let day = cal.day_of(epoch(from));
                if from < first.start {
                    start = start.min(seconds(period.floor(day, cal)));
                }
                if from >= last.end {
                    end = end.max(seconds(period.next(day, cal)));
                }
            }
        }
        Some((start, end))
    }
}

/// What a count in the background answers (`timeline.histogram` of the
/// subtree or of pictures' clocks, and `timeline.status`): the plugin says
/// whether it still counts.
#[derive(Clone, Debug, Default, Deserialize)]
struct SubtreeStatus {
    #[serde(default)]
    scanning: bool,
    #[serde(default)]
    histogram: Option<Histogram>,
}

/// Where the window of a width too fine for every date is: from or until
/// the bar holding an instant, seconds since the epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    From(u64),
    Until(u64),
}

/// What the bars measure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Measure {
    #[default]
    Files,
    Size,
}

/// What the strip keeps of a pane's visit to a place, which ends when the
/// pane shows something else: the width picked, where its window was
/// moved, the bar the keys are on and the one a range of bars began at.
/// Bars are kept by their first instant: a count again or a window moved
/// finds them where they are.
struct Visit {
    pane: PaneId,
    /// Ends when the pane shows something else.
    lifetime: CancelToken,
    bucket: Option<usize>,
    anchor: Option<Anchor>,
    cursor: Option<u64>,
    from: Option<u64>,
}

/// What a count was asked of: counted in the background (the subtree, or
/// pictures' clocks) or not, and its window.
#[derive(Clone, Copy)]
struct Asked {
    background: bool,
    anchor: Option<Anchor>,
}

#[derive(Default)]
pub struct TimelineState {
    pub open: bool,
    /// Whether the subtree is counted, in a pane that shows a folder.
    pub subtree: bool,
    pub scanning: bool,
    pub measure: Measure,
    /// The time counted, an index into `DATES`.
    pub date: usize,
    pub histogram: Option<Histogram>,
    pub hover: Option<usize>,
    /// The chart's, once the strip was open: where its keys go.
    focus: Option<FocusHandle>,
    /// Where the time's popup hangs.
    dates: PopoverAnchor,
    visits: Vec<Visit>,
    /// The histogram's bars as the chart draws them, made once for each
    /// and each measure: the chart lays out again only for new ones.
    chart: Option<Rc<Vec<bars::Bar>>>,
    computed_for: Option<(PaneId, u64, Option<usize>, Option<Anchor>, bool, usize)>,
    /// Which files it counted: the listing, its names, and what the pane
    /// shows of them (hidden files, the filter and the narrowing, the
    /// folders opened in place). Sorted again, they are the same files.
    counted: Option<(u64, u64, bool, u64, u64)>,
    /// When it last counted: while the dates come in batches (or the
    /// selection moves), at most every `RECOUNT`, the last batch counted
    /// by a wake of its own.
    counted_at: Option<std::time::Instant>,
    recount: Option<gpui::Task<()>>,
    /// Bars the window was moved since the bars shown were asked for: the
    /// next move counts from there.
    panned: i64,
    /// The count a worker is doing: the plugin's answer, and what it was
    /// asked of.
    asked: crate::latest::Latest<(Asked, Result<Option<serde_json::Value>, String>)>,
    /// The wake a count in the background was last asked after
    /// (`crate::wakes`): it ends with a wake of its own, so a draw the
    /// pointer asked never asks it again.
    status_after: Option<u64>,
}

/// How often the bars are counted again while dates come in.
const RECOUNT: std::time::Duration = std::time::Duration::from_millis(300);

/// An epoch second as a time.
fn epoch(seconds: u64) -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds)
}

/// Each bar's label and the coarsest level it is labelled at (`bars::Bar`),
/// from the civil date its period begins on: where a year begins the year,
/// where a month does its name, else the day of the month. The levels: 0
/// every bar, 1 the 1st, 8th, 15th and 22nd of a month (bars of days), 2
/// months, 3 quarters, 4 years, 5 to 10 the years that are multiples of 2,
/// 5, 10, 20, 50 and 100.
fn labels(bucket: usize, starts: &[(i64, u32, u32)]) -> Vec<(String, u8)> {
    starts
        .iter()
        .enumerate()
        .map(|(i, &(y, m, d))| {
            let before = i.checked_sub(1).map(|j| starts[j]);
            let new_year = before.map_or((m, d) == (1, 1), |(py, _, _)| py != y);
            let new_month = before.map_or(d == 1, |(py, pm, _)| (py, pm) != (y, m));
            if new_year {
                let rank = [(100, 10), (50, 9), (20, 8), (10, 7), (5, 6), (2, 5)]
                    .into_iter()
                    .find(|(n, _)| y % n == 0)
                    .map_or(4, |(_, rank)| rank);
                (y.to_string(), rank)
            } else if new_month {
                (MONTHS[month(m)].to_owned(), if m % 3 == 1 { 3 } else { 2 })
            } else {
                let weekly = bucket == DAY && matches!(d, 8 | 15 | 22);
                (d.to_string(), u8::from(weekly))
            }
        })
        .collect()
}

/// A month's index into `MONTHS`.
fn month(m: u32) -> usize {
    (m as usize).clamp(1, 12) - 1
}

/// What a bar is to a screen reader: its period in words.
fn period_words(bucket: usize, (y, m, d): (i64, u32, u32)) -> String {
    let name = MONTH_NAMES[month(m)];
    match bucket {
        DAY => format!("{d} {name} {y}"),
        1 => format!("Week of {d} {name} {y}"),
        2 => format!("{name} {y}"),
        _ => y.to_string(),
    }
}

/// `999`, `1.2k`, `12k`, `1.2M`: a count said in the room of a bar.
fn short_count(n: u64) -> String {
    let tenths = |unit: u64| match n % unit / (unit / 10) {
        0 => String::new(),
        tenth => format!(".{tenth}"),
    };
    match n {
        0..1_000 => n.to_string(),
        1_000..10_000 => format!("{}{}k", n / 1_000, tenths(1_000)),
        10_000..1_000_000 => format!("{}k", n / 1_000),
        1_000_000..10_000_000 => format!("{}{}M", n / 1_000_000, tenths(1_000_000)),
        _ => format!("{}M", n / 1_000_000),
    }
}

/// The civil dates the bars of `h` begin on, on the user's calendar, the
/// plugin's.
fn starts(h: &Histogram) -> Vec<(i64, u32, u32)> {
    let cal = Calendar::system();
    h.bars
        .iter()
        .map(|b| civil_from_days(cal.day_of(epoch(b.start))))
        .collect()
}

/// The bars of `h` as the chart draws them, measuring `measure`.
fn chart_of(h: &Histogram, measure: Measure) -> Rc<Vec<bars::Bar>> {
    let labels = labels(h.shown().unwrap_or(DAY), &starts(h));
    Rc::new(
        h.bars
            .iter()
            .zip(labels)
            .map(|(b, (label, rank))| {
                let (value, marked, tally) = match measure {
                    Measure::Files => (b.count, b.selected, short_count(b.count)),
                    Measure::Size => (b.bytes, b.selected_bytes, format::size(b.bytes)),
                };
                bars::Bar {
                    value,
                    marked,
                    label: label.into(),
                    rank,
                    tally: tally.into(),
                }
            })
            .collect(),
    )
}

/// `25 files, 1.2 MB, 3 selected`: what a bar holds.
fn holds(b: &Bar) -> String {
    let mut words = format!(
        "{}, {}",
        format::count(usize::try_from(b.count).unwrap_or(usize::MAX), "file"),
        format::size(b.bytes)
    );
    if b.selected > 0 {
        let _ = write!(
            words,
            ", {} selected",
            format::thousands(usize::try_from(b.selected).unwrap_or(usize::MAX))
        );
    }
    words
}

impl Shell {
    pub fn toggle_timeline(&mut self, cx: &mut Context<Self>) {
        self.timeline.open = !self.timeline.open;
        self.timeline.computed_for = None;
        self.timeline.hover = None;
        if self.timeline.open {
            self.hydrate_for_timeline();
        }
        self.remember_session();
        cx.notify();
    }

    /// Where the chart's keys go, while the strip is open: a part F6 goes
    /// round (`focus_next_part`).
    pub fn timeline_focus(&self) -> Option<FocusHandle> {
        self.timeline.focus.clone().filter(|_| self.timeline.open)
    }

    /// Ask for the metadata of every entry the timeline cannot date yet.
    /// Only those: asking for all of them again on every listing change
    /// would re-stat the folder for as long as the timeline is open.
    fn hydrate_for_timeline(&mut self) {
        let shown = self.ui().list.sorted.clone();
        self.request_hydrate_off_screen(shown.iter().copied());
    }

    /// Whether the pane shows a folder, whose subtree can be counted: a
    /// search lists what it lists.
    fn timeline_in_folder(&self) -> bool {
        self.app
            .workspace
            .read()
            .pane(self.pane)
            .and_then(|p| p.location())
            .and_then(Location::path)
            .is_some()
    }

    /// What the strip keeps of the pane's visit, if anything.
    fn timeline_visit(&self) -> Option<&Visit> {
        self.timeline
            .visits
            .iter()
            .find(|v| v.pane == self.pane && !v.lifetime.is_cancelled())
    }

    /// The same, kept from now on.
    fn timeline_visit_mut(&mut self) -> Option<&mut Visit> {
        let pane = self.pane;
        self.timeline.visits.retain(|v| !v.lifetime.is_cancelled());
        if !self.timeline.visits.iter().any(|v| v.pane == pane) {
            let lifetime = self.app.workspace.read().pane(pane)?.location_lifetime();
            self.timeline.visits.push(Visit {
                pane,
                lifetime,
                bucket: None,
                anchor: None,
                cursor: None,
                from: None,
            });
        }
        self.timeline.visits.iter_mut().find(|v| v.pane == pane)
    }

    /// Show bars of `bucket` for what the pane shows, until it shows
    /// something else.
    pub fn timeline_set_bucket(&mut self, bucket: &str, cx: &mut Context<Self>) {
        let Some(i) = BUCKETS.iter().position(|b| *b == bucket) else {
            return;
        };
        // A window moved along: the new bars end where it does.
        let anchor = self
            .timeline
            .histogram
            .as_ref()
            .filter(|h| h.windowed())
            .and_then(|h| h.bars.last())
            .map(|b| Anchor::Until(b.start));
        if let Some(visit) = self.timeline_visit_mut() {
            visit.bucket = Some(i);
            visit.anchor = anchor;
            self.timeline.panned = 0;
            cx.notify();
        }
    }

    pub fn timeline_toggle_subtree(&mut self, cx: &mut Context<Self>) {
        self.timeline.subtree = !self.timeline.subtree;
        self.timeline.computed_for = None;
        cx.notify();
    }

    /// `ui.timeline_measure`: bars of files or of their bytes.
    pub fn timeline_set_measure(&mut self, by: &str, cx: &mut Context<Self>) {
        let measure = match by {
            "size" => Measure::Size,
            "files" => Measure::Files,
            _ => return,
        };
        if measure != self.timeline.measure {
            self.timeline.measure = measure;
            self.timeline.chart = self
                .timeline
                .histogram
                .as_ref()
                .map(|h| chart_of(h, measure));
            self.remember_session();
            cx.notify();
        }
    }

    /// `ui.timeline_date`: which time the bars count.
    pub fn timeline_set_date(&mut self, date: &str, cx: &mut Context<Self>) {
        if let Some(i) = DATES.iter().position(|(word, _)| *word == date)
            && i != self.timeline.date
        {
            self.timeline.date = i;
            self.timeline.computed_for = None;
            self.remember_session();
            cx.notify();
        }
    }

    /// Move a window of bars `by` bars later (negative: earlier), as far
    /// as there are dates: from the bars shown, and the moves not counted
    /// yet.
    pub fn timeline_pan(&mut self, by: i64, cx: &mut Context<Self>) {
        let Some(h) = self.timeline.histogram.as_ref().filter(|h| h.windowed()) else {
            return;
        };
        let Some(last) = h.bars.len().checked_sub(1) else {
            return;
        };
        let n = i64::try_from(last).unwrap_or(i64::MAX);
        let shift = (self.timeline.panned + by).clamp(
            if h.before == 0 { 0 } else { -n },
            if h.after == 0 { 0 } else { n },
        );
        let at = |i: i64| h.bars[usize::try_from(i).unwrap_or(0).min(last)].start;
        let anchor = if shift > 0 {
            Anchor::From(at(shift))
        } else {
            Anchor::Until(at(n + shift))
        };
        if let Some(visit) = self.timeline_visit_mut()
            && visit.anchor != Some(anchor)
        {
            visit.anchor = Some(anchor);
            self.timeline.panned = shift;
            cx.notify();
        }
    }

    /// `ui.timeline_pan`: half a window earlier or later.
    pub fn timeline_pan_toward(&mut self, toward: &str, cx: &mut Context<Self>) {
        let half = self
            .timeline
            .histogram
            .as_ref()
            .map_or(1, |h| i64::try_from(h.bars.len() / 2).unwrap_or(1).max(1));
        match toward {
            "earlier" => self.timeline_pan(-half, cx),
            "later" => self.timeline_pan(half, cx),
            _ => {}
        }
    }

    /// `ui.timeline_cursor`: the keys' bar `by` bars on, or the first or
    /// the last, extending the bars chosen with `extend`. Past an end of a
    /// window, the window moves half its length.
    pub fn timeline_cursor(
        &mut self,
        by: i64,
        to: Option<&str>,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(h) = &self.timeline.histogram else {
            return;
        };
        let Some(last) = h.bars.len().checked_sub(1) else {
            return;
        };
        let current = self
            .timeline_visit()
            .and_then(|v| v.cursor)
            .and_then(|t| h.bar_of(t));
        let index = |i: usize| i64::try_from(i).unwrap_or(i64::MAX);
        let target = match (to, current) {
            (Some("first"), _) => 0,
            (Some("last"), _) => index(last),
            // The first key lands on the bar under the pointer, else the last.
            (_, None) => index(self.timeline.hover.unwrap_or(last)),
            (_, Some(at)) => index(at) + by,
        };
        let clamped = usize::try_from(target.max(0)).unwrap_or(0).min(last);
        let starts = (current.map(|i| h.bars[i].start), h.bars[clamped].start);
        let past = if !h.windowed() || target == index(clamped) {
            None
        } else if target < 0 {
            (h.before > 0).then_some("earlier")
        } else {
            (h.after > 0).then_some("later")
        };
        if let Some(visit) = self.timeline_visit_mut() {
            visit.from = if extend {
                visit.from.or(starts.0).or(Some(starts.1))
            } else {
                None
            };
            visit.cursor = Some(starts.1);
        }
        if let Some(toward) = past {
            self.timeline_pan_toward(toward, cx);
        }
        cx.notify();
    }

    /// The bars chosen, first and last: from where a range began to the
    /// keys' bar, or that bar alone.
    fn timeline_chosen(&self) -> Option<(usize, usize)> {
        let h = self.timeline.histogram.as_ref()?;
        let visit = self.timeline_visit()?;
        let cursor = h.bar_of(visit.cursor?)?;
        let from = visit.from.and_then(|t| h.bar_of(t)).unwrap_or(cursor);
        Some((from.min(cursor), from.max(cursor)))
    }

    /// `ui.timeline_list`: show the files of the bars chosen.
    pub fn timeline_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start, end)) = self.timeline.histogram.as_ref().and_then(|h| {
            let visit = self.timeline_visit()?;
            h.span(visit.from, visit.cursor?, &Calendar::system())
        }) else {
            return;
        };
        let args = serde_json::json!({
            "start": start,
            "end": end,
            "pane": self.pane,
            "subtree": self.timeline.subtree && self.timeline_in_folder(),
            "date": DATES[self.timeline.date].0,
        });
        self.run("timeline.select", args, window, cx);
    }

    /// The pointer pressed on bar `i`: the strip takes the keyboard, and
    /// the bars chosen begin there.
    fn timeline_press(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.timeline_focus() {
            window.focus(&focus, cx);
        }
        let Some(start) = self.bar_start(i) else {
            return;
        };
        if let Some(visit) = self.timeline_visit_mut() {
            visit.from = Some(start);
            visit.cursor = Some(start);
        }
        cx.notify();
    }

    /// Dragged to bar `i`: the bars chosen reach it.
    fn timeline_drag(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(start) = self.bar_start(i) else {
            return;
        };
        if let Some(visit) = self.timeline_visit_mut() {
            visit.cursor = Some(start);
        }
        cx.notify();
    }

    fn bar_start(&self, i: usize) -> Option<u64> {
        Some(self.timeline.histogram.as_ref()?.bars.get(i)?.start)
    }

    /// Recompute when the listing changed (hydration lands as events), and
    /// poll a count in the background while it runs.
    pub fn sync_timeline(&mut self, cx: &mut Context<Self>) {
        if !self.timeline.open {
            return;
        }
        if self.timeline.focus.is_none() {
            self.timeline.focus = Some(cx.focus_handle());
        }
        if let Some((asked, answer)) = self.timeline.asked.take() {
            self.take_histogram(asked, answer, cx);
        }
        // What the bars count: the files' dates and names and which are
        // selected, never every batch of other metadata.
        let Some((version, shown, lifetime)) = ({
            let ws = self.app.workspace.read();
            ws.pane(self.pane).map(|p| {
                (
                    p.listing.modified_version()
                        ^ p.listing.names_version().rotate_left(32)
                        ^ p.selection.revision().rotate_left(16),
                    (
                        p.listing.id(),
                        p.listing.names_version(),
                        p.sort.show_hidden,
                        p.filter_version(),
                        p.tree_version(),
                    ),
                    p.location_lifetime(),
                )
            })
        }) else {
            return;
        };
        self.timeline.visits.retain(|v| !v.lifetime.is_cancelled());
        let (bucket, anchor) = self
            .timeline_visit()
            .map_or((None, None), |v| (v.bucket, v.anchor));
        let subtree = self.timeline.subtree && self.timeline_in_folder();
        let date = self.timeline.date;
        let key = (self.pane, version, bucket, anchor, subtree, date);
        let other_files = self.timeline.counted != Some(shown);
        // Dates arriving batch after batch, or the selection moving:
        // counted at most every `RECOUNT`, and once more after the last.
        let dates_only =
            self.timeline
                .computed_for
                .is_some_and(|(pane, _, bucket, anchor, subtree, date)| {
                    (pane, bucket, anchor, subtree, date) == (key.0, key.2, key.3, key.4, key.5)
                })
                && !other_files;
        if dates_only
            && self.timeline.computed_for != Some(key)
            && let Some(at) = self.timeline.counted_at
            && at.elapsed() < RECOUNT
        {
            if self.timeline.recount.is_none() {
                let wait = RECOUNT.saturating_sub(at.elapsed());
                self.timeline.recount = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = this.update(cx, |shell, cx| {
                        shell.timeline.recount = None;
                        cx.notify();
                    });
                }));
            }
            return;
        }
        if self.timeline.computed_for != Some(key) || other_files {
            self.timeline.counted_at = Some(std::time::Instant::now());
            self.timeline.counted = Some(shown);
            // Entries without metadata need it before they count; dates
            // coming in are the answers to what was asked already.
            if !dates_only {
                self.hydrate_for_timeline();
            }
            let mut args = serde_json::json!({
                "subtree": subtree,
                "pane": self.pane,
                "date": DATES[date].0,
            });
            if let Some(bucket) = bucket {
                args["bucket"] = BUCKETS[bucket].into();
            }
            match anchor {
                Some(Anchor::From(at)) => args["from"] = at.into(),
                Some(Anchor::Until(at)) => args["until"] = at.into(),
                None => {}
            }
            let ask = Invocation::with_args("timeline.histogram", &args);
            let app = std::sync::Arc::clone(&self.app);
            let worker = std::sync::Arc::clone(&self.app);
            let lane = self.pane_lane(self.pane);
            let asked = Asked {
                background: subtree || date == TAKEN,
                anchor,
            };
            self.timeline.asked.ask(
                &app,
                lane,
                &lifetime,
                skwir_core::scheduler::Priority::Visible,
                "timeline-count",
                move |_| {
                    let answer = match worker.dispatch(&ask) {
                        Ok(Outcome::Value(v)) => Ok(Some(v)),
                        Ok(_) => Ok(None),
                        Err(e) => Err(format!("{e:#}")),
                    };
                    (asked, answer)
                },
            );
            self.timeline.computed_for = Some(key);
        }
        // A count in the background tells it has ended by waking the
        // window: asked once a wake came since it was last asked, never at
        // each draw (the pointer over the bars). Read before asking, so a
        // count ending meanwhile is asked again.
        let wakes = crate::wakes();
        if self.timeline.scanning && self.timeline.status_after != Some(wakes) {
            self.timeline.status_after = Some(wakes);
            if let Some(v) = self.query_value(
                "timeline.status",
                serde_json::json!({ "pane": self.pane }),
                cx,
            ) && let Ok(s) = serde_json::from_value::<SubtreeStatus>(v)
            {
                self.timeline.scanning = s.scanning;
                if let Some(h) = s.histogram {
                    self.set_histogram(Some(h));
                }
            }
        }
    }
}

impl Shell {
    /// What the plugin counted, once a worker has it.
    fn take_histogram(
        &mut self,
        asked: Asked,
        answer: Result<Option<serde_json::Value>, String>,
        cx: &mut Context<Self>,
    ) {
        // The bars of the window moved to: moves count from them now.
        if self.timeline_visit().and_then(|v| v.anchor) == asked.anchor {
            self.timeline.panned = 0;
        }
        match answer {
            Ok(Some(v)) if asked.background => {
                // The count runs in the plugin; `timeline.status` follows it.
                match serde_json::from_value::<SubtreeStatus>(v) {
                    Ok(status) => {
                        self.timeline.scanning = status.scanning;
                        self.set_histogram(status.histogram);
                    }
                    Err(e) => tracing::warn!("timeline.histogram: {e}"),
                }
            }
            Ok(Some(v)) => {
                self.timeline.scanning = false;
                match serde_json::from_value::<Histogram>(v) {
                    Ok(h) => self.set_histogram(Some(h)),
                    Err(e) => tracing::warn!("timeline.histogram: {e}"),
                }
            }
            Ok(None) => self.set_histogram(None),
            Err(e) => {
                self.set_histogram(None);
                self.set_status(e, true, cx);
            }
        }
        cx.notify();
    }

    fn set_histogram(&mut self, h: Option<Histogram>) {
        // The same bars keep their chart, and so what it laid out.
        if h == self.timeline.histogram {
            return;
        }
        self.timeline.chart = h.as_ref().map(|h| chart_of(h, self.timeline.measure));
        if h.as_ref()
            .is_none_or(|h| self.timeline.hover.is_some_and(|i| i >= h.bars.len()))
        {
            self.timeline.hover = None;
        }
        self.timeline.histogram = h;
    }
}

/// What the strip says beside its controls: the bar under the pointer (or
/// the keys'), or what the bars show.
fn info(t: &TimelineState, focused_bar: Option<usize>) -> String {
    let Some(h) = &t.histogram else {
        return if t.scanning {
            "counting…".into()
        } else {
            "no data".into()
        };
    };
    let files = |n: u64| format::count(usize::try_from(n).unwrap_or(usize::MAX), "file");
    let number = |n: u64| format::thousands(usize::try_from(n).unwrap_or(usize::MAX));
    // `end` is the first instant of the day after a bar's last: its last
    // day holds the instant before.
    let days = |start: u64, end: u64| {
        let (first, last) = (
            format::day(epoch(start)),
            format::day(epoch(end.saturating_sub(1).max(start))),
        );
        if first == last {
            first
        } else {
            format!("{first} → {last}")
        }
    };
    if let Some(b) = t.hover.or(focused_bar).and_then(|i| h.bars.get(i)) {
        let mut said = format!(
            "{} · {} · {}",
            days(b.start, b.end),
            files(b.count),
            format::size(b.bytes)
        );
        if b.selected > 0 {
            let _ = write!(said, " · {} selected", number(b.selected));
        }
        return said;
    }
    let mut parts = Vec::new();
    if t.scanning {
        parts.push("counting…".to_owned());
    }
    if let (Some(first), Some(last)) = (h.bars.first(), h.bars.last()) {
        parts.push(days(first.start, last.end));
    }
    parts.push(format!(
        "{} dated",
        number(h.total - h.unknown.min(h.total))
    ));
    if h.unknown > 0 {
        parts.push(format!("{} without a date", number(h.unknown)));
    }
    if h.before > 0 {
        parts.push(format!("{} earlier", files(h.before)));
    }
    if h.after > 0 {
        parts.push(format!("{} later", files(h.after)));
    }
    if h.outside > 0 {
        parts.push(format!("{} far outside", number(h.outside)));
    }
    parts.push("click or drag across bars to list their files".to_owned());
    parts.join(" · ")
}

/// The strip's controls: its widths, the arrows of a window, the subtree,
/// what the bars measure, the time they count, and what they say.
fn controls(
    shell: &Shell,
    folder: bool,
    focused_bar: Option<usize>,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> gpui::Div {
    let t = &shell.timeline;
    let shown = t.histogram.as_ref().and_then(Histogram::shown);
    let fits = t.histogram.as_ref().and_then(Histogram::fits);
    let windowed = t.histogram.as_ref().is_some_and(Histogram::windowed);
    let mut controls = div()
        .h(px(26.))
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .child(div().text_color(theme.text_muted).child("Timeline"));
    // The strip's own commands, through the shared button: a tooltip,
    // focus, and the keys a button answers. The width pressed is the one
    // shown, picked or the finest that shows every date.
    for (i, bucket) in BUCKETS.iter().enumerate() {
        let title = if fits.is_some_and(|fits| i < fits) {
            format!(
                "A bar for each {bucket}: too many for the strip, shown a part at a time that the wheel and the arrows move"
            )
        } else {
            format!("A bar for each {bucket}")
        };
        controls = controls.child(
            Button::new(format!("tl-{bucket}"), "", title, "ui.timeline_bucket")
                .args(serde_json::json!({ "bucket": bucket }))
                .label(*bucket)
                .active(shown == Some(i))
                .render(shell, cx)
                .h(px(22.)),
        );
    }
    if windowed && let (Some(h), Some(shown)) = (&t.histogram, shown) {
        let plural = PLURALS[shown];
        for (toward, icon, title, more) in [
            ("earlier", "left", "Earlier", h.before),
            ("later", "right", "Later", h.after),
        ] {
            controls = controls.child(
                Button::new(
                    format!("tl-{toward}"),
                    icon,
                    format!("{title} {plural}"),
                    "ui.timeline_pan",
                )
                .args(serde_json::json!({ "toward": toward }))
                .enabled(more > 0)
                .render(shell, cx)
                .h(px(22.)),
            );
        }
    }
    controls = controls.child(
        Button::new(
            "tl-subtree",
            "",
            "Count the files of the folders inside too",
            "ui.timeline_subtree",
        )
        .label("subtree")
        .active(t.subtree && folder)
        .enabled(folder)
        .render(shell, cx)
        .h(px(22.)),
    );
    for (by, measure, title) in [
        (
            "files",
            Measure::Files,
            "Bars as tall as their number of files",
        ),
        ("size", Measure::Size, "Bars as tall as their files' size"),
    ] {
        controls = controls.child(
            Button::new(format!("tl-by-{by}"), "", title, "ui.timeline_measure")
                .args(serde_json::json!({ "by": by }))
                .label(by)
                .active(t.measure == measure)
                .render(shell, cx)
                .h(px(22.)),
        );
    }
    let open = shell
        .toolbar_menu
        .as_ref()
        .is_some_and(|menu| menu.kind == DATES_MENU);
    controls = controls.child(
        Button::new(
            "tl-date",
            "",
            "Which time the bars count",
            "ui.toolbar_menu",
        )
        .args(serde_json::json!({ "kind": DATES_MENU }))
        .label(DATES[t.date].0)
        .anchor_if(&t.dates, true)
        .active(open)
        .dropdown()
        .render(shell, cx)
        .h(px(22.)),
    );
    controls
        .child(div().flex_1())
        .child(crate::elements::clip::clipped(
            div()
                .id("timeline-info")
                .text_color(theme.text_muted)
                .truncate(),
            info(t, focused_bar),
        ))
}

/// The popup choosing the time the bars count.
fn dates_popup(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let popup = shell
        .toolbar_menu
        .as_ref()
        .filter(|menu| menu.kind == DATES_MENU)?;
    let mut rows = div().flex().flex_col().gap_1();
    for (i, (word, label)) in DATES.iter().enumerate() {
        let chosen = i == shell.timeline.date;
        rows = rows.child(
            Button::new(
                format!("tl-date-{word}"),
                if chosen { "check" } else { "" },
                *label,
                "ui.timeline_date",
            )
            .args(serde_json::json!({ "date": word }))
            .label(*label)
            .active(chosen)
            .menu()
            .render(shell, cx),
        );
    }
    let panel = toolbar::menu_panel(popup, "ui.toolbar_menu", theme, cx).child(rows);
    // The strip is at the foot of the window: no room below it.
    Some(shell.timeline.dates.popup_above_for(
        &crate::elements::adaptive::Chosen::default(),
        vec![panel.into_any_element()],
    ))
}

pub fn render(shell: &Shell, window: &Window, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.timeline.open {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let t = &shell.timeline;
    let folder = shell.timeline_in_folder();
    let focus = t.focus.clone();
    let focused = focus.as_ref().is_some_and(|f| f.is_focused(window));
    let chosen = shell.timeline_chosen();
    let ranging = shell.timeline_visit().is_some_and(|v| v.from.is_some());
    let cursor = t
        .histogram
        .as_ref()
        .zip(shell.timeline_visit().and_then(|v| v.cursor))
        .and_then(|(h, t)| h.bar_of(t));
    let focused_bar = cursor.filter(|_| focused);
    let controls = controls(shell, folder, focused_bar, &theme, cx);

    let mut chart_area = div()
        .id("timeline-chart")
        .role(gpui::Role::ListBox)
        .aria_label("Timeline")
        .relative()
        .h(px(CHART_H))
        .w_full()
        .key_context(crate::keymap::TIMELINE);
    if let Some(focus) = &focus {
        chart_area = chart_area.track_focus(focus);
    }
    if let (Some(h), Some(chart)) = (&t.histogram, &t.chart) {
        let pane = shell.pane;
        let colors = BarColors {
            bar: theme.chart,
            marked: theme.accent,
            column: theme.hover,
            chosen: theme.selection_bg,
            cursor: theme.accent,
            text: theme.text_muted,
            tick: theme.border,
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let today = h.bars.iter().position(|b| b.start <= now && now < b.end);
        let (hovered, pressed, dragged, released) =
            (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let mut chart = bar_chart(
            ("timeline-bars", pane.0 as usize),
            Rc::clone(chart),
            colors,
            move |i, _, cx| {
                hovered.update(cx, |shell, cx| {
                    shell.timeline.hover = i;
                    cx.notify();
                });
            },
            move |i, window, cx| {
                pressed.update(cx, |shell, cx| {
                    shell.with_pane(pane, |shell| shell.timeline_press(i, window, cx));
                });
            },
            move |i, _, cx| {
                dragged.update(cx, |shell, cx| {
                    shell.with_pane(pane, |shell| shell.timeline_drag(i, cx));
                });
            },
            move |window, cx| {
                released.update(cx, |shell, cx| {
                    shell.with_pane(pane, |shell| shell.timeline_list(window, cx));
                });
            },
        )
        .hovered(t.hover)
        // A range is shown while it is chosen; one bar is the keys',
        // ringed while they are here.
        .chosen(chosen.filter(|_| ranging))
        .cursor(focused_bar)
        .today(today);
        if h.windowed() {
            let entity = cx.entity();
            chart = chart.on_pan(move |by, _, cx| {
                entity.update(cx, |shell, cx| {
                    shell.with_pane(pane, |shell| shell.timeline_pan(by, cx));
                });
            });
        }
        // To a screen reader: what the bars count and how to go through
        // them, then each bar, the keys' one said as focused (it claims
        // the active descendant, as a list's entry does). Elements of no
        // size, built only while a reader listens.
        if shell.a11y {
            let bucket = h.shown().unwrap_or(DAY);
            let mut said = div().absolute().size_0().overflow_hidden();
            for (i, (b, start)) in h.bars.iter().zip(starts(h)).enumerate() {
                said = said.child(
                    div()
                        .id(("timeline-bar", i))
                        .role(gpui::Role::ListBoxOption)
                        .aria_label(period_words(bucket, start))
                        .aria_description(holds(b))
                        .aria_position_in_set(i)
                        .aria_selected(chosen.is_some_and(|(a, b)| (a..=b).contains(&i)))
                        .when(
                            cursor == Some(i),
                            StatefulInteractiveElement::aria_active_descendant,
                        ),
                );
            }
            chart_area = chart_area
                .aria_size_of_set(h.bars.len())
                .aria_description(format!(
                    "{} by {}, {}. Left and Right go from bar to bar, Shift extends, Enter lists the files",
                    DATES[t.date].1,
                    BUCKETS[bucket],
                    info(t, None)
                ))
                .child(said);
        }
        chart_area = chart_area.child(chart);
    }
    Some(
        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .child(controls)
            .child(chart_area)
            .children(dates_popup(shell, &theme, cx))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(start: u64, end: u64) -> Bar {
        Bar {
            start,
            end,
            count: 0,
            bytes: 0,
            selected: 0,
            selected_bytes: 0,
        }
    }

    #[test]
    fn labels_say_the_period_each_bar_begins() {
        // Days across the end of a year.
        let days = [(2025, 12, 30), (2025, 12, 31), (2026, 1, 1), (2026, 1, 2)];
        let said = labels(DAY, &days);
        assert_eq!(
            said,
            [
                ("30".into(), 0),
                ("31".into(), 0),
                ("2026".into(), 5),
                ("2".into(), 0)
            ]
        );
        // The 8th, 15th and 22nd of a month are a level of their own for
        // days, not for weeks.
        assert_eq!(
            labels(DAY, &[(2026, 3, 7), (2026, 3, 8)])[1],
            ("8".into(), 1)
        );
        assert_eq!(labels(1, &[(2026, 3, 1), (2026, 3, 8)])[1], ("8".into(), 0));
        // A week begins a month when the month it begins in is a new one:
        // its name, a quarter's higher.
        let weeks = [(2026, 3, 23), (2026, 3, 30), (2026, 4, 6)];
        assert_eq!(labels(1, &weeks)[2], ("Apr".into(), 3));
        // Months; the first bar begins a month, January a year.
        let months = [(2019, 11, 1), (2019, 12, 1), (2020, 1, 1), (2020, 2, 1)];
        let said: Vec<_> = labels(2, &months);
        assert_eq!(said[0], ("Nov".into(), 2));
        assert_eq!(said[2], ("2020".into(), 8), "a multiple of 20");
        // A first bar that begins mid-month says its day.
        assert_eq!(labels(DAY, &[(2026, 3, 9)])[0], ("9".into(), 0));
    }

    #[test]
    fn a_bar_said_in_words() {
        assert_eq!(period_words(DAY, (2026, 3, 9)), "9 March 2026");
        assert_eq!(period_words(1, (2026, 3, 9)), "Week of 9 March 2026");
        assert_eq!(period_words(2, (2026, 3, 1)), "March 2026");
        assert_eq!(period_words(3, (2026, 1, 1)), "2026");
        let b = Bar {
            count: 25,
            bytes: 2048,
            selected: 3,
            selected_bytes: 10,
            ..bar(0, 1)
        };
        assert_eq!(
            holds(&b),
            format!("25 files, {}, 3 selected", format::size(2048))
        );
    }

    #[test]
    fn counts_said_in_a_bars_room() {
        assert_eq!(short_count(0), "0");
        assert_eq!(short_count(999), "999");
        assert_eq!(short_count(1_000), "1k");
        assert_eq!(short_count(1_250), "1.2k");
        assert_eq!(short_count(9_999), "9.9k");
        assert_eq!(short_count(12_345), "12k");
        assert_eq!(short_count(1_500_000), "1.5M");
        assert_eq!(short_count(25_000_000), "25M");
    }

    #[test]
    fn a_window_is_a_width_finer_than_fits() {
        let h = |bucket: &str, fits: &str| Histogram {
            bucket: Some(bucket.into()),
            fits: Some(fits.into()),
            ..Histogram::default()
        };
        assert!(h("day", "month").windowed());
        assert!(!h("month", "month").windowed());
        assert!(!h("year", "month").windowed());
        assert!(!Histogram::default().windowed());
    }

    #[test]
    fn an_instant_finds_its_bar_or_the_end_it_lies_past() {
        let h = Histogram {
            bars: vec![bar(10, 20), bar(20, 30), bar(30, 40)],
            ..Histogram::default()
        };
        assert_eq!(h.bar_of(20), Some(1));
        assert_eq!(h.bar_of(39), Some(2));
        assert_eq!(h.bar_of(5), Some(0));
        assert_eq!(h.bar_of(99), Some(2));
        assert_eq!(Histogram::default().bar_of(5), None);
    }

    #[test]
    fn a_range_longer_than_the_window_keeps_what_lies_past_it() {
        const D: u64 = 86_400;
        // Days 10 to 12 shown of a span too long for every day.
        let h = Histogram {
            bucket: Some("day".into()),
            fits: Some("month".into()),
            bars: (10..13).map(|d| bar(d * D, (d + 1) * D)).collect(),
            ..Histogram::default()
        };
        let cal = Calendar::utc();
        // The keys' bar alone, and a range inside the window.
        assert_eq!(h.span(None, 11 * D, &cal), Some((11 * D, 12 * D)));
        assert_eq!(h.span(Some(12 * D), 10 * D, &cal), Some((10 * D, 13 * D)));
        // Begun before the window, or after it: from that day itself.
        assert_eq!(h.span(Some(4 * D), 12 * D, &cal), Some((4 * D, 13 * D)));
        assert_eq!(h.span(Some(20 * D), 10 * D, &cal), Some((10 * D, 21 * D)));
        // Begun at a day of a finer width, now in a week outside: its week.
        let weeks = Histogram {
            bucket: Some("week".into()),
            fits: Some("year".into()),
            bars: vec![bar(28 * D, 35 * D)],
            ..Histogram::default()
        };
        // Day 10 (1970-01-11) is a Sunday: its week began on Monday, day 4.
        assert_eq!(
            weeks.span(Some(10 * D), 30 * D, &cal),
            Some((4 * D, 35 * D))
        );
        assert_eq!(Histogram::default().span(None, 0, &cal), None);
    }

    #[test]
    fn bars_measure_files_or_bytes_and_mark_the_selected() {
        let h = Histogram {
            bucket: Some("year".into()),
            bars: vec![Bar {
                count: 4,
                bytes: 3_000,
                selected: 1,
                selected_bytes: 1_000,
                ..bar(1_767_225_600, 1_798_761_600)
            }],
            ..Histogram::default()
        };
        let files = chart_of(&h, Measure::Files);
        assert_eq!((files[0].value, files[0].marked), (4, 1));
        assert_eq!(&*files[0].tally, "4");
        let size = chart_of(&h, Measure::Size);
        assert_eq!((size[0].value, size[0].marked), (3_000, 1_000));
        assert_eq!(&*size[0].tally, format::size(3_000));
    }
}
