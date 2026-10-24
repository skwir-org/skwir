//! Timeline strip: a histogram of modification times under the file area.
//! The plugin counts it (`timeline.histogram`), on a worker: a count reads
//! every file the pane shows, which the window's thread does not wait for.

use gpui::{AnyElement, Context, div, prelude::*, px};
use serde::Deserialize;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::workspace::PaneId;

use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::Button;

pub const BUCKETS: [&str; 4] = ["day", "week", "month", "year"];
const BAR_AREA_H: f32 = 64.0;

#[derive(Clone, Debug, Deserialize)]
pub struct Bar {
    pub start: u64,
    pub end: u64,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Histogram {
    /// The bucket the bars use: coarser than the one asked for when that
    /// one would need too many bars.
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub bars: Vec<Bar>,
    #[serde(default)]
    pub unknown: u64,
    /// Entries dated far from the others, outside the bars.
    #[serde(default)]
    pub outside: u64,
    #[serde(default)]
    pub total: u64,
}

/// What `timeline.histogram {subtree}` and `timeline.status` answer: the
/// walk runs in the plugin, which says whether it still does.
#[derive(Clone, Debug, Default, Deserialize)]
struct SubtreeStatus {
    #[serde(default)]
    scanning: bool,
    #[serde(default)]
    histogram: Option<Histogram>,
}

#[derive(Default)]
pub struct TimelineState {
    pub open: bool,
    pub bucket: usize,
    pub subtree: bool,
    pub scanning: bool,
    pub histogram: Option<Histogram>,
    pub hover: Option<usize>,
    computed_for: Option<(PaneId, u64, usize, bool)>,
    /// Which files it counted: the listing, its names, and what the pane
    /// shows of them (hidden files, the filter and the narrowing, the
    /// folders opened in place). Sorted again, they are the same files.
    counted: Option<(u64, u64, bool, u64, u64)>,
    /// When it last counted: while the dates come in batches, at most
    /// every `RECOUNT`, the last batch counted by a wake of its own.
    counted_at: Option<std::time::Instant>,
    recount: Option<gpui::Task<()>>,
    /// The count a worker is doing: the plugin's answer, and whether it
    /// was asked of the subtree.
    asked: crate::latest::Latest<(bool, Result<Option<serde_json::Value>, String>)>,
}

/// How often the bars are counted again while dates come in.
const RECOUNT: std::time::Duration = std::time::Duration::from_millis(300);

/// Bar heights in px for `counts`, the tallest reaching `max_h`; a non-zero
/// count is at least 2 px so it stays visible.
pub fn bar_heights(counts: &[u64], max_h: f32) -> Vec<f32> {
    let max = counts.iter().copied().max().unwrap_or(0);
    counts
        .iter()
        .map(|&c| {
            if c == 0 || max == 0 {
                0.0
            } else {
                (c as f32 / max as f32 * max_h).max(2.0)
            }
        })
        .collect()
}

/// An epoch second as a time.
fn epoch(seconds: u64) -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds)
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

    /// Ask for the metadata of every entry the timeline cannot date yet.
    /// Only those: asking for all of them again on every listing change
    /// would re-stat the folder for as long as the timeline is open.
    fn hydrate_for_timeline(&mut self) {
        let shown = self.ui().list.sorted.clone();
        self.request_hydrate_off_screen(shown.iter().copied());
    }

    pub fn timeline_set_bucket(&mut self, bucket: &str, cx: &mut Context<Self>) {
        if let Some(i) = BUCKETS.iter().position(|b| *b == bucket) {
            self.timeline.bucket = i;
            self.timeline.computed_for = None;
            cx.notify();
        }
    }

    pub fn timeline_toggle_subtree(&mut self, cx: &mut Context<Self>) {
        self.timeline.subtree = !self.timeline.subtree;
        self.timeline.computed_for = None;
        cx.notify();
    }

    /// Recompute when the listing changed (hydration lands as events), and
    /// poll the subtree walk while it runs.
    pub fn sync_timeline(&mut self, cx: &mut Context<Self>) {
        if !self.timeline.open {
            return;
        }
        if let Some((subtree, answer)) = self.timeline.asked.take() {
            self.take_histogram(subtree, answer, cx);
        }
        // What the bars count: the files' dates and names, never every
        // batch of other metadata.
        let Some((version, shown, lifetime)) = ({
            let ws = self.app.workspace.read();
            ws.pane(self.pane).map(|p| {
                (
                    p.listing.modified_version() ^ p.listing.names_version().rotate_left(32),
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
        let key = (
            self.pane,
            version,
            self.timeline.bucket,
            self.timeline.subtree,
        );
        let other_files = self.timeline.counted != Some(shown);
        // Dates arriving batch after batch: counted at most every
        // `RECOUNT`, and once more after the last.
        let dates_only = self
            .timeline
            .computed_for
            .is_some_and(|(pane, _, bucket, subtree)| {
                (pane, bucket, subtree) == (key.0, key.2, key.3)
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
            let subtree = self.timeline.subtree;
            let ask = Invocation::with_args(
                "timeline.histogram",
                &serde_json::json!({
                    "bucket": BUCKETS[self.timeline.bucket],
                    "subtree": subtree,
                    "pane": self.pane,
                }),
            );
            let app = std::sync::Arc::clone(&self.app);
            let worker = std::sync::Arc::clone(&self.app);
            let lane = self.pane_lane(self.pane);
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
                    (subtree, answer)
                },
            );
            self.timeline.computed_for = Some(key);
        }
        if self.timeline.subtree
            && self.timeline.scanning
            && let Some(v) = self.query_value(
                "timeline.status",
                serde_json::json!({ "pane": self.pane }),
                cx,
            )
            && let Ok(s) = serde_json::from_value::<SubtreeStatus>(v)
        {
            self.timeline.scanning = s.scanning;
            if let Some(h) = s.histogram {
                self.timeline.histogram = Some(h);
            }
        }
    }
}

impl Shell {
    /// What the plugin counted, once a worker has it.
    fn take_histogram(
        &mut self,
        subtree: bool,
        answer: Result<Option<serde_json::Value>, String>,
        cx: &mut Context<Self>,
    ) {
        match answer {
            Ok(Some(v)) if subtree => {
                // The walk runs in the plugin; `timeline.status` follows it.
                match serde_json::from_value::<SubtreeStatus>(v) {
                    Ok(status) => {
                        self.timeline.scanning = status.scanning;
                        self.timeline.histogram = status.histogram;
                    }
                    Err(e) => tracing::warn!("timeline.histogram: {e}"),
                }
            }
            Ok(Some(v)) => {
                self.timeline.scanning = false;
                match serde_json::from_value::<Histogram>(v) {
                    Ok(h) => self.timeline.histogram = Some(h),
                    Err(e) => tracing::warn!("timeline.histogram: {e}"),
                }
            }
            Ok(None) => self.timeline.histogram = None,
            Err(e) => {
                self.timeline.histogram = None;
                self.set_status(e, true, cx);
            }
        }
        cx.notify();
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if !shell.timeline.open {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let t = &shell.timeline;
    let mut controls = div()
        .h(px(26.))
        .px_2()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .child(div().text_color(theme.text_muted).child("Timeline"));
    // The strip's own commands, through the shared button: a tooltip,
    // focus, and the keys a button answers.
    for (i, bucket) in BUCKETS.iter().enumerate() {
        controls = controls.child(
            Button::new(
                format!("tl-{bucket}"),
                "",
                format!("A bar for each {bucket}"),
                "ui.timeline_bucket",
            )
            .args(serde_json::json!({ "bucket": bucket }))
            .label(*bucket)
            .active(i == t.bucket)
            .render(shell, cx)
            .h(px(22.)),
        );
    }
    controls = controls.child(
        Button::new(
            "tl-subtree",
            "",
            "Count the files of the folders inside too",
            "ui.timeline_subtree",
        )
        .label(format!(
            "subtree{}",
            if t.scanning { " (scanning…)" } else { "" }
        ))
        .active(t.subtree)
        .render(shell, cx)
        .h(px(22.)),
    );
    let info = match (&t.histogram, t.hover) {
        (Some(h), Some(i)) if i < h.bars.len() => {
            let b = &h.bars[i];
            format!(
                "{} → {} · {} · {}",
                format::day(epoch(b.start)),
                format::day(epoch(b.end)),
                format::count(usize::try_from(b.count).unwrap_or(usize::MAX), "file"),
                format::size(b.bytes)
            )
        }
        (Some(h), _) => {
            let mut parts = vec![
                format!("{} dated", format::thousands(h.total as usize)),
                format!("{} without a date", format::thousands(h.unknown as usize)),
            ];
            if h.outside > 0 {
                parts.push(format!(
                    "{} far outside",
                    format::thousands(h.outside as usize)
                ));
            }
            // Asked for days over decades: the plugin drew wider bars.
            if let Some(bucket) = h.bucket.as_deref().filter(|b| *b != BUCKETS[t.bucket]) {
                parts.push(format!("shown by {bucket}"));
            }
            parts.push("click a bar to list its files".to_owned());
            parts.join(" · ")
        }
        (None, _) => "no data".into(),
    };
    controls = controls
        .child(div().flex_1())
        .child(crate::elements::clip::clipped(
            div()
                .id("timeline-info")
                .text_color(theme.text_muted)
                .truncate(),
            info,
        ));

    let mut bars_row = div()
        .h(px(BAR_AREA_H))
        .w_full()
        .flex()
        .flex_row()
        .items_end()
        .gap_px()
        .px_2();
    if let Some(h) = &t.histogram {
        let counts: Vec<u64> = h.bars.iter().map(|b| b.count).collect();
        let heights = bar_heights(&counts, BAR_AREA_H - 4.0);
        let (pane, subtree) = (shell.pane, t.subtree);
        for (i, (b, hgt)) in h.bars.iter().zip(heights).enumerate() {
            let (start, end) = (b.start, b.end);
            let hovered = t.hover == Some(i);
            bars_row = bars_row.child(
                div()
                    .id(crate::elements::known_by("bar", (start, end)))
                    .flex_1()
                    .min_w(px(2.))
                    .h(px(BAR_AREA_H - 4.0))
                    .flex()
                    .items_end()
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, hov, _, cx| {
                        this.timeline.hover = if *hov { Some(i) } else { None };
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run(
                            "timeline.select",
                            serde_json::json!({
                                "start": start,
                                "end": end,
                                "pane": pane,
                                "subtree": subtree,
                            }),
                            window,
                            cx,
                        );
                    }))
                    .child(div().w_full().h(px(hgt)).rounded_sm().bg(if hovered {
                        theme.accent
                    } else {
                        theme.chart
                    })),
            );
        }
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
            .child(bars_row)
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::bar_heights;

    #[test]
    fn heights_scale_to_the_tallest_and_keep_small_bars_visible() {
        let h = bar_heights(&[0, 1, 50, 100], 60.0);
        assert!(h[0].abs() < f32::EPSILON);
        assert!((h[1] - 2.0).abs() < 1e-6, "minimum visible height");
        assert!((h[2] - 30.0).abs() < 1e-6);
        assert!((h[3] - 60.0).abs() < 1e-6);
        assert!(bar_heights(&[], 60.0).is_empty());
        assert!(
            bar_heights(&[0, 0], 60.0)
                .iter()
                .all(|h| h.abs() < f32::EPSILON)
        );
    }
}
