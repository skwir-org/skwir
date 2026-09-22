//! Timeline strip: a histogram of modification times under the file area.

use bibli_core::workspace::PaneId;
use gpui::{AnyElement, Context, div, prelude::*, px};
use serde::Deserialize;

use crate::format;
use crate::shell::Shell;
use crate::theme::Theme;

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
}

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

/// `YYYY-MM-DD` for an epoch second.
pub fn day_label(epoch: u64) -> String {
    jiff::Timestamp::from_second(epoch as i64).map_or_else(
        |_| "?".into(),
        |t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d")
                .to_string()
        },
    )
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
        let need: Vec<bibli_core::EntryId> = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return;
            };
            self.ui()
                .list
                .sorted
                .iter()
                .copied()
                .filter(|id| {
                    pane.listing
                        .get(*id)
                        .is_some_and(|e| !e.metadata.is_hydrated())
                })
                .collect()
        };
        self.request_hydrate(need);
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
        let version = self.ui().list.version().unwrap_or(0);
        let key = (
            self.pane,
            version,
            self.timeline.bucket,
            self.timeline.subtree,
        );
        if self.timeline.computed_for != Some(key) {
            // Entries without metadata need it before they count.
            self.hydrate_for_timeline();
            let args = serde_json::json!({
                "bucket": BUCKETS[self.timeline.bucket],
                "subtree": self.timeline.subtree,
                "pane": self.pane,
            });
            match self.query_value("timeline.histogram", args, cx) {
                Some(v) if self.timeline.subtree => {
                    // The walk runs in the plugin; `timeline.status` follows it.
                    match serde_json::from_value::<SubtreeStatus>(v) {
                        Ok(status) => {
                            self.timeline.scanning = status.scanning;
                            self.timeline.histogram = status.histogram;
                        }
                        Err(e) => tracing::warn!("timeline.histogram: {e}"),
                    }
                }
                Some(v) => {
                    self.timeline.scanning = false;
                    match serde_json::from_value::<Histogram>(v) {
                        Ok(h) => self.timeline.histogram = Some(h),
                        Err(e) => tracing::warn!("timeline.histogram: {e}"),
                    }
                }
                None => self.timeline.histogram = None,
            }
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

fn chip(
    id: &'static str,
    label: String,
    active: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
    on: impl Fn(&mut Shell, &mut gpui::Window, &mut Context<Shell>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_md()
        .text_xs()
        .cursor_pointer()
        .when(active, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(!active, |d| d.hover(|s| s.bg(theme.hover)))
        .on_click(cx.listener(move |this, _, window, cx| on(this, window, cx)))
        .child(label)
        .into_any_element()
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
    for (i, b) in BUCKETS.iter().enumerate() {
        let name = (*b).to_owned();
        controls = controls.child(chip(
            ["tl-day", "tl-week", "tl-month", "tl-year"][i],
            name.clone(),
            i == t.bucket,
            &theme,
            cx,
            move |this, _, cx| this.timeline_set_bucket(&name, cx),
        ));
    }
    controls = controls.child(chip(
        "tl-subtree",
        format!("subtree{}", if t.scanning { " (scanning…)" } else { "" }),
        t.subtree,
        &theme,
        cx,
        |this, _, cx| this.timeline_toggle_subtree(cx),
    ));
    let info = match (&t.histogram, t.hover) {
        (Some(h), Some(i)) if i < h.bars.len() => {
            let b = &h.bars[i];
            format!(
                "{} → {} · {} files · {}",
                day_label(b.start),
                day_label(b.end),
                format::thousands(b.count as usize),
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
        .child(div().text_color(theme.text_muted).truncate().child(info));

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
                    .id(("bar", i))
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
