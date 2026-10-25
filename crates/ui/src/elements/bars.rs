//! A bar chart: a bar for each period, side by side, the tallest filling
//! the chart. What fits is decided in `prepaint`, from the bounds it really
//! has: the labels under the bars, as many as have room (every bar's, else
//! those of coarser and coarser periods: a bar's `rank`), and each bar's
//! count above it when every count has room.
//!
//! The pointer over a bar reads it (`on_hover`), a click on it chooses it
//! (`on_click`), and the wheel moves along, a bar for each bar's width it
//! turned (`on_pan`).

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    App, Bounds, ContentMask, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId,
    Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, Pixels, ScrollWheelEvent, ShapedLine,
    SharedString, Style, TextRun, Window, point, px, relative, size,
};

/// One bar.
pub struct Bar {
    pub count: u64,
    /// Said under it where there is room.
    pub label: SharedString,
    /// The coarsest level it is labelled at: at level 0 every bar is, at
    /// each level above only those whose rank reaches it. The chart says
    /// the finest level whose labels all have room.
    pub rank: u8,
    /// Its count as said above it.
    pub tally: SharedString,
}

/// The colors a chart draws with (`theme.rs`).
#[derive(Clone, Copy)]
pub struct BarColors {
    pub bar: Hsla,
    /// The bar under the pointer.
    pub hover: Hsla,
    /// Behind the bar under the pointer, its whole height.
    pub column: Hsla,
    pub text: Hsla,
    /// The marks where a label's period begins.
    pub tick: Hsla,
}

type HoverFn = Rc<dyn Fn(Option<usize>, &mut Window, &mut App)>;
type ClickFn = Rc<dyn Fn(usize, &mut Window, &mut App)>;
type PanFn = Rc<dyn Fn(i64, &mut Window, &mut App)>;

pub struct BarChart {
    id: ElementId,
    /// Shared: the chart lays out again only when it is handed others
    /// (or its bounds change), not at each frame the pointer moves.
    bars: Rc<Vec<Bar>>,
    colors: BarColors,
    hovered: Option<usize>,
    on_hover: HoverFn,
    on_click: ClickFn,
    on_pan: Option<PanFn>,
}

pub fn bar_chart(
    id: impl Into<ElementId>,
    bars: Rc<Vec<Bar>>,
    colors: BarColors,
    on_hover: impl Fn(Option<usize>, &mut Window, &mut App) + 'static,
    on_click: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> BarChart {
    BarChart {
        id: id.into(),
        bars,
        colors,
        hovered: None,
        on_hover: Rc::new(on_hover),
        on_click: Rc::new(on_click),
        on_pan: None,
    }
}

impl BarChart {
    /// The bar under the pointer, as the last hover callback said.
    pub fn hovered(mut self, index: Option<usize>) -> Self {
        self.hovered = index;
        self
    }

    /// Let the wheel move along: `bars` later (negative: earlier).
    pub fn on_pan(mut self, pan: impl Fn(i64, &mut Window, &mut App) + 'static) -> Self {
        self.on_pan = Some(Rc::new(pan));
        self
    }
}

impl IntoElement for BarChart {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// Inside the element: a margin on each side, the labels' line at the
/// bottom, the counts' above the bars when they are said.
const SIDE: f32 = 8.;
const AXIS: f32 = 16.;
const TALLY: f32 = 13.;
const FONT: f32 = 11.;
/// Room kept between two labels, and between a count and the next.
const SPACE: f32 = 6.;
/// A label at a period's start is drawn this far after its tick.
const AFTER_TICK: f32 = 3.;
/// A bar that counts something is never lower than this.
const LOWEST: f32 = 2.;

/// Bar heights in px for `counts`, the tallest reaching `max_h`; a non-zero
/// count is at least `LOWEST` so it stays visible.
pub fn bar_heights(counts: impl Iterator<Item = u64> + Clone, max_h: f32) -> Vec<f32> {
    let max = counts.clone().max().unwrap_or(0);
    counts
        .map(|c| {
            if c == 0 || max == 0 {
                0.0
            } else {
                (c as f32 / max as f32 * max_h).max(LOWEST)
            }
        })
        .collect()
}

/// Which bars are labelled, from left to right: at each level, from the
/// finest, those whose rank reaches it, until one level's all have room.
/// `place(i)` is where bar `i`'s label would begin and end. Level 0's
/// labels stand under their bars; above, a label begins at its bar.
fn choose_labels(
    ranks: &[u8],
    every: impl Fn(usize) -> (f32, f32),
    at_start: impl Fn(usize) -> (f32, f32),
    right: f32,
) -> (Vec<usize>, bool) {
    let top = ranks.iter().copied().max().unwrap_or(0);
    // Level 0, each bar's own: all of them, or none.
    let all: Vec<usize> = (0..ranks.len()).collect();
    if fits(&all, &every, right) {
        return (all, true);
    }
    for level in 1..=top {
        let chosen: Vec<usize> = (0..ranks.len()).filter(|&i| ranks[i] >= level).collect();
        if fits(&chosen, &at_start, right) {
            return (chosen, false);
        }
    }
    // The coarsest has no room for all: as many as do, left to right.
    let mut kept = Vec::new();
    let mut end = f32::MIN;
    for i in (0..ranks.len()).filter(|&i| ranks[i] >= top) {
        let (from, to) = at_start(i);
        if from >= end + SPACE && to <= right {
            kept.push(i);
            end = to;
        }
    }
    (kept, false)
}

/// Whether the labels of `chosen` stand apart, the last one inside.
fn fits(chosen: &[usize], place: &impl Fn(usize) -> (f32, f32), right: f32) -> bool {
    let mut end = f32::MIN;
    for &i in chosen {
        let (from, to) = place(i);
        if from < end + SPACE || to > right {
            return false;
        }
        end = to;
    }
    true
}

struct Laid {
    /// Each bar: its slot (the whole height it answers the pointer in)
    /// and the bar drawn in it.
    slots: Vec<Bounds<Pixels>>,
    bars: Vec<Bounds<Pixels>>,
    /// Labels with where they are drawn, and the ticks of those that
    /// begin at their bar.
    labels: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    ticks: Vec<Bounds<Pixels>>,
    tallies: Vec<(gpui::Point<Pixels>, ShapedLine)>,
    /// From one bar to the next.
    pitch: f32,
    left: Pixels,
}

/// What a chart laid out, kept between frames by what it was laid out from
/// (its bars, as shared, its bounds, the font and the scale): a frame drawn
/// as the pointer moves shapes nothing again.
struct Kept {
    from: (usize, Bounds<Pixels>, SharedString, u32),
    laid: Rc<Laid>,
    /// Wheel turned by less than a bar, kept for the next turn.
    wheel: Rc<Cell<f32>>,
}

pub struct Prepaint {
    laid: Rc<Laid>,
    wheel: Rc<Cell<f32>>,
    hitbox: Hitbox,
}

fn shape(
    text: &SharedString,
    color: Hsla,
    window: &Window,
    shaped: &mut HashMap<SharedString, ShapedLine>,
) -> ShapedLine {
    shaped
        .entry(text.clone())
        .or_insert_with(|| {
            let run = TextRun {
                len: text.len(),
                font: window.text_style().font(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
                position: gpui::RunPosition::Baseline,
            };
            window
                .text_system()
                .shape_line(text.clone(), px(FONT), &[run], None)
        })
        .clone()
}

fn lay_out(bars: &[Bar], colors: BarColors, bounds: Bounds<Pixels>, window: &Window) -> Laid {
    let n = bars.len().max(1);
    let left = bounds.origin.x + px(SIDE);
    let width = (f32::from(bounds.size.width) - 2. * SIDE).max(1.);
    let right = f32::from(left) + width;
    let pitch = width / n as f32;
    // Bars apart where they have room for it.
    let gap = if pitch >= 3. { 1. } else { 0. };
    let x = |i: usize| (i as f32 * pitch).round();
    let mut labels_shaped = HashMap::new();
    let mut tallies_shaped = HashMap::new();
    // Counts are said when every one has room above its bar.
    let tallies: Vec<Option<ShapedLine>> = bars
        .iter()
        .map(|b| (b.count > 0).then(|| shape(&b.tally, colors.text, window, &mut tallies_shaped)))
        .collect();
    let say_tallies = tallies
        .iter()
        .flatten()
        .all(|t| f32::from(t.width) + SPACE <= pitch);
    let top = bounds.origin.y + px(if say_tallies { TALLY } else { 4. });
    let floor = bounds.origin.y + bounds.size.height - px(AXIS);
    let high = f32::from(floor - top).max(1.);
    let heights = bar_heights(bars.iter().map(|b| b.count), high);
    let mut slots = Vec::with_capacity(bars.len());
    let mut drawn = Vec::with_capacity(bars.len());
    let mut said = Vec::new();
    for (i, h) in heights.iter().enumerate() {
        let (from, to) = (x(i), x(i + 1));
        let w = (to - from - gap).max(1.);
        let slot = Bounds::new(
            point(left + px(from), bounds.origin.y),
            size(px(w), floor - bounds.origin.y),
        );
        let bar = Bounds::new(point(slot.origin.x, floor - px(*h)), size(px(w), px(*h)));
        if say_tallies && let Some(tally) = &tallies[i] {
            let at = point(
                bar.origin.x + (bar.size.width - tally.width) / 2.,
                bar.origin.y - px(TALLY),
            );
            said.push((at, tally.clone()));
        }
        slots.push(slot);
        drawn.push(bar);
    }
    // Labels: shaped once each (most repeat: day numbers, months).
    let shaped: Vec<ShapedLine> = bars
        .iter()
        .map(|b| shape(&b.label, colors.text, window, &mut labels_shaped))
        .collect();
    let base = f32::from(left);
    let every = |i: usize| {
        let w = f32::from(shaped[i].width);
        let middle = base + f32::midpoint(x(i), x(i + 1));
        (middle - w / 2., middle + w / 2.)
    };
    let at_start = |i: usize| {
        let from = base + x(i) + AFTER_TICK;
        (from, from + f32::from(shaped[i].width))
    };
    let ranks: Vec<u8> = bars.iter().map(|b| b.rank).collect();
    let (chosen, centred) = choose_labels(&ranks, every, at_start, right);
    let line = floor + px(2.);
    let mut labels = Vec::with_capacity(chosen.len());
    let mut ticks = Vec::new();
    for i in chosen {
        let from = if centred { every(i).0 } else { at_start(i).0 };
        labels.push((point(px(from), line), shaped[i].clone()));
        if !centred {
            ticks.push(Bounds::new(
                point(left + px(x(i)), floor),
                size(px(1.), px(4.)),
            ));
        }
    }
    Laid {
        slots,
        bars: drawn,
        labels,
        ticks,
        tallies: said,
        pitch,
        left,
    }
}

impl Element for BarChart {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Prepaint {
        let from = (
            Rc::as_ptr(&self.bars) as usize,
            bounds,
            window.text_style().font().family,
            window.scale_factor().to_bits(),
        );
        let (bars, colors) = (Rc::clone(&self.bars), self.colors);
        let (laid, wheel) = match id {
            Some(id) => window.with_element_state(id, |kept: Option<Kept>, window| {
                let wheel = kept
                    .as_ref()
                    .map_or_else(Rc::default, |kept| Rc::clone(&kept.wheel));
                let laid = match kept.filter(|kept| kept.from == from) {
                    Some(kept) => kept.laid,
                    None => Rc::new(lay_out(&bars, colors, bounds, window)),
                };
                let kept = Kept {
                    from,
                    laid: Rc::clone(&laid),
                    wheel: Rc::clone(&wheel),
                };
                ((laid, wheel), kept)
            }),
            None => (
                Rc::new(lay_out(&bars, colors, bounds, window)),
                Rc::default(),
            ),
        };
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        Prepaint {
            laid,
            wheel,
            hitbox,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let colors = self.colors;
        let laid = Rc::clone(&prepaint.laid);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(slot) = self.hovered.and_then(|i| laid.slots.get(i)) {
                window.paint_quad(gpui::fill(*slot, colors.column).corner_radii(px(2.)));
            }
            for (i, bar) in laid.bars.iter().enumerate() {
                if bar.size.height > px(0.) {
                    let color = if self.hovered == Some(i) {
                        colors.hover
                    } else {
                        colors.bar
                    };
                    let radius = if bar.size.width >= px(4.) { 2. } else { 0. };
                    window.paint_quad(gpui::fill(*bar, color).corner_radii(px(radius)));
                }
            }
            for tick in &laid.ticks {
                window.paint_quad(gpui::fill(*tick, colors.tick));
            }
            let lines = (laid.labels.iter().map(|l| (l, AXIS - 2.)))
                .chain(laid.tallies.iter().map(|t| (t, TALLY)));
            for ((at, text), height) in lines {
                if let Err(error) =
                    text.paint(*at, px(height), gpui::TextAlign::Left, None, window, cx)
                {
                    tracing::debug!(%error, "chart label");
                }
            }
        });

        let hitbox = prepaint.hitbox.clone();
        window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
        let (pitch, left, count) = (laid.pitch, laid.left, laid.bars.len());
        let bar_at = move |p: gpui::Point<Pixels>| {
            let i = (f32::from(p.x - left) / pitch).floor();
            (i >= 0. && (i as usize) < count).then_some(i as usize)
        };
        let current = self.hovered;
        let on_hover = Rc::clone(&self.on_hover);
        let hb = hitbox.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let over = if hb.is_hovered(window) {
                bar_at(ev.position)
            } else {
                None
            };
            if over != current {
                on_hover(over, window, cx);
            }
        });
        // The pointer gone from the window is on no bar: GPUI keeps the
        // position it left at.
        let on_hover = Rc::clone(&self.on_hover);
        window.on_mouse_event(move |_: &MouseExitEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && current.is_some() {
                on_hover(None, window, cx);
            }
        });
        let on_click = Rc::clone(&self.on_click);
        let hb = hitbox.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && ev.button == MouseButton::Left
                && hb.is_hovered(window)
                && let Some(i) = bar_at(ev.position)
            {
                on_click(i, window, cx);
            }
        });
        if let Some(on_pan) = self.on_pan.clone() {
            let wheel = Rc::clone(&prepaint.wheel);
            window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                    return;
                }
                let delta = ev.delta.pixel_delta(px(AXIS));
                // Across or down, whichever the wheel turned more: up and
                // left go back in time.
                let turned = if delta.x.abs() > delta.y.abs() {
                    delta.x
                } else {
                    delta.y
                };
                let total = wheel.get() + f32::from(turned);
                let bars = (total / pitch.max(1.)).trunc();
                wheel.set(total - bars * pitch.max(1.));
                if bars != 0. {
                    on_pan(-(bars as i64), window, cx);
                }
                cx.stop_propagation();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_scale_to_the_tallest_and_keep_small_bars_visible() {
        let h = bar_heights([0, 1, 50, 100].into_iter(), 60.0);
        assert!(h[0].abs() < f32::EPSILON);
        assert!((h[1] - LOWEST).abs() < 1e-6, "minimum visible height");
        assert!((h[2] - 30.0).abs() < 1e-6);
        assert!((h[3] - 60.0).abs() < 1e-6);
        assert!(bar_heights(std::iter::empty(), 60.0).is_empty());
        assert!(
            bar_heights([0, 0].into_iter(), 60.0)
                .iter()
                .all(|h| h.abs() < f32::EPSILON)
        );
    }

    /// Labels 10 wide on bars `pitch` apart, each `at_start` beginning at
    /// its bar, `every` centred under it.
    fn chosen(ranks: &[u8], pitch: f32, right: f32) -> (Vec<usize>, bool) {
        let every = |i: usize| {
            let middle = (i as f32 + 0.5) * pitch;
            (middle - 5., middle + 5.)
        };
        let at_start = |i: usize| (i as f32 * pitch, i as f32 * pitch + 10.);
        choose_labels(ranks, every, at_start, right)
    }

    #[test]
    fn the_finest_labels_that_all_have_room() {
        // Months, a year starting at the fourth: wide bars say each one.
        let ranks = [0, 0, 0, 4, 0, 0];
        assert_eq!(chosen(&ranks, 20., 120.), (vec![0, 1, 2, 3, 4, 5], true));
        // Narrow ones only the year's.
        assert_eq!(chosen(&ranks, 5., 30.), (vec![3], false));
        // Quarters in between, where years would say less than there is
        // room for.
        let ranks = [3, 0, 0, 3, 0, 0, 4, 0, 0, 3, 0, 0];
        assert_eq!(chosen(&ranks, 6., 72.), (vec![0, 3, 6, 9], false));
        // Not even every year has room: those that do, left to right.
        let ranks = [4, 4, 4, 4];
        assert_eq!(chosen(&ranks, 9., 36.), (vec![0, 2], false));
        // The last label must not run out of the chart.
        assert_eq!(chosen(&[4], 20., 8.), (vec![], false));
    }
}
