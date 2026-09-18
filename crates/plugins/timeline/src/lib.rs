//! bibli plugin: timeline. Files by modification time as a histogram the
//! UI renders as a scrubbable bar chart; a bucket becomes a `query:`
//! location (`modified>=start modified<end in:dir`) when selected.
//!
//! Commands: `timeline.histogram { pane?, bucket?, subtree? }` returns the
//! buckets for the pane's listing (hydrated entries only; the UI hydrates
//! everything first) or, with `subtree`, starts a background walk and
//! `timeline.status {}` returns it when done. `timeline.select { start, end,
//! pane? }` navigates to the matching query.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{Value, keys};
use bibli_core::location::Location;
use bibli_core::plugin::Plugin;
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const ID: &str = "timeline";

/// Bucket width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    Day,
    Week,
    #[default]
    Month,
    Year,
}

impl Bucket {
    fn seconds(self) -> u64 {
        match self {
            Bucket::Day => 86_400,
            Bucket::Week => 7 * 86_400,
            // Calendar months are handled by `floor`; this is only a fallback width.
            Bucket::Month => 30 * 86_400,
            Bucket::Year => 365 * 86_400,
        }
    }

    /// Start of the bucket containing `t` (seconds since the epoch, UTC).
    pub fn floor(self, t: u64) -> u64 {
        match self {
            Bucket::Day | Bucket::Week => t - t % self.seconds(),
            Bucket::Month => {
                let (y, m, _) = civil_from_days(t / 86_400);
                days_from_civil(y, m, 1) * 86_400
            }
            Bucket::Year => {
                let (y, _, _) = civil_from_days(t / 86_400);
                days_from_civil(y, 1, 1) * 86_400
            }
        }
    }

    /// Start of the bucket after the one starting at `start`.
    pub fn next(self, start: u64) -> u64 {
        match self {
            Bucket::Day | Bucket::Week => start + self.seconds(),
            Bucket::Month => {
                let (y, m, _) = civil_from_days(start / 86_400);
                let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                days_from_civil(y, m, 1) * 86_400
            }
            Bucket::Year => {
                let (y, _, _) = civil_from_days(start / 86_400);
                days_from_civil(y + 1, 1, 1) * 86_400
            }
        }
    }
}

// Howard Hinnant's civil date algorithms (proleptic Gregorian, UTC).
fn civil_from_days(days: u64) -> (i64, u32, u32) {
    let z = i64::try_from(days).unwrap_or(i64::MAX / 2) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as u64
}

/// `YYYY-MM-DD` for a bucket boundary, for the query language.
pub fn date_string(t: u64) -> String {
    let (y, m, d) = civil_from_days(t / 86_400);
    format!("{y:04}-{m:02}-{d:02}")
}

/// One histogram bar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Bar {
    /// Bucket start, seconds since the epoch.
    pub start: u64,
    pub end: u64,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct Histogram {
    pub bucket: Bucket,
    pub bars: Vec<Bar>,
    /// Entries without a modification time (not hydrated yet).
    pub unknown: u64,
    pub total: u64,
}

/// Bucket `(mtime, size)` samples into a dense, contiguous histogram.
pub fn histogram(
    samples: impl IntoIterator<Item = (SystemTime, u64)>,
    bucket: Bucket,
    unknown: u64,
) -> Histogram {
    let mut map: std::collections::BTreeMap<u64, Bar> = std::collections::BTreeMap::new();
    let mut total = unknown;
    for (t, bytes) in samples {
        let secs = t
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        let start = bucket.floor(secs);
        let bar = map.entry(start).or_insert(Bar {
            start,
            end: bucket.next(start),
            count: 0,
            bytes: 0,
        });
        bar.count += 1;
        bar.bytes += bytes;
        total += 1;
    }
    let mut bars = Vec::new();
    if let (Some(&first), Some(&last)) = (map.keys().next(), map.keys().next_back()) {
        let mut s = first;
        while s <= last {
            bars.push(map.get(&s).cloned().unwrap_or(Bar {
                start: s,
                end: bucket.next(s),
                count: 0,
                bytes: 0,
            }));
            s = bucket.next(s);
        }
    }
    Histogram {
        bucket,
        bars,
        unknown,
        total,
    }
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
    /// Bucket start, seconds since the epoch.
    pub start: u64,
    pub end: u64,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct NoArgs {}

#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct SubtreeStatus {
    pub root: Option<PathBuf>,
    pub scanning: bool,
    pub histogram: Option<Histogram>,
}

#[derive(Default)]
struct State {
    status: SubtreeStatus,
    cancel: Option<CancelToken>,
}

/// The plugin.
#[derive(Default)]
pub struct TimelinePlugin {
    state: Arc<Mutex<State>>,
}

fn pane_dir(app: &App, pane: Option<PaneId>) -> anyhow::Result<PathBuf> {
    let ws = app.workspace.read();
    let pane = pane.unwrap_or_else(|| ws.active_id());
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    p.location()
        .and_then(Location::path)
        .ok_or_else(|| anyhow::anyhow!("pane is not showing a directory"))
}

fn walk_samples(root: &Path, cancel: &CancelToken) -> Vec<(SystemTime, u64)> {
    use rayon::prelude::*;
    fn one(dir: &Path, cancel: &CancelToken, out: &mut Vec<(SystemTime, u64)>) -> Vec<PathBuf> {
        let mut subdirs = Vec::new();
        if cancel.is_cancelled() {
            return subdirs;
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if let Ok(m) = e.metadata() {
                    if m.is_dir() {
                        subdirs.push(e.path());
                    } else if let Ok(t) = m.modified() {
                        out.push((t, m.len()));
                    }
                }
            }
        }
        subdirs
    }
    let mut out = Vec::new();
    let mut level = vec![root.to_path_buf()];
    while !level.is_empty() && !cancel.is_cancelled() {
        let results: Vec<(Vec<(SystemTime, u64)>, Vec<PathBuf>)> = level
            .par_iter()
            .map(|d| {
                let mut o = Vec::new();
                let subs = one(d, cancel, &mut o);
                (o, subs)
            })
            .collect();
        level = Vec::new();
        for (o, subs) in results {
            out.extend(o);
            level.extend(subs);
        }
    }
    out
}

impl TimelinePlugin {
    fn listing_histogram(
        app: &App,
        pane: Option<PaneId>,
        bucket: Bucket,
    ) -> anyhow::Result<Histogram> {
        let ws = app.workspace.read();
        let pane = pane.unwrap_or_else(|| ws.active_id());
        let p = ws
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        let mut unknown = 0;
        let samples: Vec<(SystemTime, u64)> = p
            .listing
            .iter()
            .filter_map(|e| {
                let t = e.metadata.value(keys::MODIFIED).and_then(Value::as_time);
                if t.is_none() {
                    unknown += 1;
                }
                t.map(|t| {
                    (
                        t,
                        e.metadata
                            .value(keys::SIZE)
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    )
                })
            })
            .collect();
        Ok(histogram(samples, bucket, unknown))
    }

    fn start_subtree(&self, app: &App, dir: PathBuf, bucket: Bucket) {
        let cancel = app.lifetime().child();
        {
            let mut s = self.state.lock();
            if let Some(c) = s.cancel.take() {
                c.cancel();
            }
            s.status = SubtreeStatus {
                root: Some(dir.clone()),
                scanning: true,
                histogram: None,
            };
            s.cancel = Some(cancel.clone());
        }
        let state = Arc::clone(&self.state);
        let wake = app.waker_handle();
        let task_cancel = cancel.clone();
        app.scheduler
            .spawn(Priority::Background, "timeline.walk", cancel, move || {
                let samples = walk_samples(&dir, &task_cancel);
                let h = histogram(samples, bucket, 0);
                let mut s = state.lock();
                if s.status.root.as_deref() == Some(dir.as_path()) {
                    s.status.scanning = false;
                    if !task_cancel.is_cancelled() {
                        s.status.histogram = Some(h);
                    }
                }
                drop(s);
                wake();
            });
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
    fn register(&self, app: &App) {
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
                if a.subtree {
                    let dir = pane_dir(app, a.pane)?;
                    t.start_subtree(app, dir, bucket);
                    return Ok(Outcome::Done);
                }
                Ok(Outcome::Value(serde_json::to_value(
                    Self::listing_histogram(app, a.pane, bucket)?,
                )?))
            },
        );
        let t = Arc::clone(&this);
        app.commands.register::<NoArgs, _>(
            ID,
            "timeline.status",
            "Timeline status",
            "The subtree histogram, once the walk is done",
            move |_, _| {
                Ok(Outcome::Value(serde_json::to_value(
                    &t.state.lock().status,
                )?))
            },
        );
        app.commands.register::<SelectArgs, _>(
            ID,
            "timeline.select",
            "Show files from a period",
            "Navigate to a query for files modified in [start, end)",
            |a, app| {
                let dir = pane_dir(app, a.pane)?;
                // Date ranges include the whole end date in the query language;
                // histogram buckets instead exclude their end boundary.
                let expr = format!(
                    "modified>={} modified<{}",
                    date_string(a.start),
                    date_string(a.end)
                );
                // Pass the root as data; the query command owns query-language
                // quoting, including apostrophes and whitespace in paths.
                app.dispatch(&Invocation::with_args(
                    "query.search",
                    &serde_json::json!({ "expr": expr, "root": dir, "pane": a.pane }),
                ))
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
                when: None,
                order: 440,
            },
        );
    }
    fn on_disable(&self, _app: &App) {
        if let Some(c) = self.state.lock().cancel.take() {
            c.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_math_round_trips() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 9, 18), (2100, 12, 31)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d));
        }
        assert_eq!(
            date_string(days_from_civil(2026, 9, 18) * 86_400 + 5),
            "2026-09-18"
        );
        let s = Bucket::Month.floor(days_from_civil(2026, 9, 18) * 86_400 + 12345);
        assert_eq!(date_string(s), "2026-09-01");
        assert_eq!(date_string(Bucket::Month.next(s)), "2026-10-01");
        assert_eq!(
            date_string(Bucket::Year.next(Bucket::Year.floor(s))),
            "2027-01-01"
        );
    }

    #[test]
    fn histogram_is_dense_and_counts_bytes() {
        let day = |d: u64| {
            UNIX_EPOCH + Duration::from_secs(days_from_civil(2026, 1, d as u32) * 86_400 + 100)
        };
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
    fn subtree_walk_counts_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        std::fs::write(tmp.path().join("a/b/f1"), b"1").unwrap();
        std::fs::write(tmp.path().join("f2"), b"22").unwrap();
        let s = walk_samples(tmp.path(), &CancelToken::new());
        assert_eq!(s.len(), 2);
        assert_eq!(s.iter().map(|x| x.1).sum::<u64>(), 3);
    }
}
