//! A strip of chips that gives way in steps and says how many it left out.
//!
//! An entry's tags take the width they are given: beside a list row's
//! name, what the name leaves; over a tile's picture, the picture's width.
//! Every item has a whole form (a tag's chip, its name after its dot) and
//! a dot. As room shrinks the strip steps down: every item whole; the
//! first ones whole and the rest as dots; dots only; fewer dots and "+N"
//! for the rest; at the last, nothing. Which step fits is known only once
//! the strip has its width, so it is decided here, in prepaint, from each
//! whole form's natural width and the dots' fixed one: never in `render`.
//!
//! A strip that shares a line with something else asks instead for the
//! width that draws every item whole (`ChipStrip::natural`): a wrapping
//! row puts it on a line of its own before it steps down, and what gives
//! way more readily beside it is cut first.
//!
//! What cannot be drawn is never built nor measured: whole forms are
//! built in order until one passes the width (the items after it can
//! only be dots), and a "+N" is measured only for a step whose other
//! parts fit. A row of seventy tags builds the few its width holds, not
//! seventy chips and sixty counts, at every frame.
use std::ops::Range;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window, point, px, size,
};

/// How a strip measures what it lays out, in pixels, at 100 %: laid out
/// at the size of the text where it stands (`Metrics::at`, ADR-117).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// The strip's height; every item is centered in it.
    pub height: f32,
    /// Between two whole forms, and before the dots and the "+N".
    pub gap: f32,
    /// A dot's side.
    pub dot: f32,
    /// From one dot to the next: less than `dot` where they overlap.
    pub dot_step: f32,
    /// The most dots drawn; the rest are counted in the "+N".
    pub max_dots: usize,
    /// Whether any item may be whole: a strip too small for names (a tile
    /// at the small zooms) draws dots alone, whatever its width.
    pub whole: bool,
    /// Whether what it draws stands in the middle of its width (under a
    /// name centered on a tile), else at its start.
    pub center: bool,
}

/// Item `i` of a strip, built when it is asked for.
pub type Item = Box<dyn Fn(usize) -> AnyElement>;

pub struct ChipStrip {
    /// How many items it has.
    count: usize,
    /// Item `i` whole.
    whole: Item,
    /// Item `i` as a dot, `metrics.dot` square.
    dot: Item,
    /// The items left out, as "+N".
    more: Box<dyn Fn(Range<usize>) -> AnyElement>,
    metrics: Metrics,
    /// It asks for the width that draws every item whole, else it takes
    /// what it is given (`ChipStrip::natural`).
    natural: bool,
    /// The first items whole, as built so far, in order.
    built: Vec<AnyElement>,
}

impl ChipStrip {
    /// A strip of `count` items, each built by `whole` or `dot` when it
    /// may be drawn so, the rest counted by `more`.
    pub fn new(
        count: usize,
        whole: Item,
        dot: Item,
        more: Box<dyn Fn(Range<usize>) -> AnyElement>,
        metrics: Metrics,
    ) -> Self {
        ChipStrip {
            count,
            whole,
            dot,
            more,
            metrics,
            natural: false,
            built: Vec::new(),
        }
    }

    /// Item `i` whole, built once.
    fn built(&mut self, i: usize) -> &mut AnyElement {
        while self.built.len() <= i {
            let next = (self.whole)(self.built.len());
            self.built.push(next);
        }
        &mut self.built[i]
    }

    /// What it draws in the middle of its width.
    #[must_use]
    pub fn centered(mut self) -> Self {
        self.metrics.center = true;
        self
    }

    /// It asks for the width that draws every item whole, measured as it
    /// is laid out, up to its container's: in a wrapping row it goes to a
    /// line of its own before it steps down (the preview's header); beside
    /// what gives way more readily (`flex_shrink`), that is cut first (a
    /// folded group's words).
    #[must_use]
    pub fn natural(mut self) -> Self {
        self.natural = true;
        self
    }
}

impl IntoElement for ChipStrip {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// What a strip draws: its first `whole` items whole, the `dots` after
/// them as dots, and the `more` after those counted in a "+N".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fit {
    pub whole: usize,
    pub dots: usize,
    pub more: usize,
}

impl Fit {
    const NOTHING: Fit = Fit {
        whole: 0,
        dots: 0,
        more: 0,
    };
}

/// How wide `fit` of `widths` is, its "+N" `more` wide.
fn span(widths: &[Pixels], fit: Fit, metrics: &Metrics, more: Pixels) -> Pixels {
    let gap = px(metrics.gap);
    let mut used = widths[..fit.whole]
        .iter()
        .copied()
        .fold(px(0.), |a, w| a + w)
        + gap * fit.whole.saturating_sub(1) as f32;
    let mut parts = usize::from(fit.whole > 0);
    if fit.dots > 0 {
        used += px(metrics.dot + metrics.dot_step * (fit.dots - 1) as f32);
        parts += 1;
    }
    if fit.more > 0 {
        used += more;
        parts += 1;
    }
    used + gap * parts.saturating_sub(1) as f32
}

/// How many of the first items fit `room` whole, side by side: no step
/// draws more of them whole, whatever follows (the widths only add up).
fn fitting(widths: &[Pixels], room: Pixels, metrics: &Metrics) -> usize {
    let mut used = px(0.);
    for (i, w) in widths.iter().enumerate() {
        used += *w + if i > 0 { px(metrics.gap) } else { px(0.) };
        if used > room {
            return i;
        }
    }
    widths.len()
}

/// The step of `n` items that fits `room`, as the module says: the most
/// items whole, then the most dots. `widths` are the first items' whole
/// widths, at least up to the first that does not fit after those before
/// it (`fitting`), all of them otherwise. `more(n)` measures the "+N" for
/// `n` left out, asked only where the rest of the step fits.
fn fit(
    widths: &[Pixels],
    n: usize,
    room: Pixels,
    metrics: &Metrics,
    mut more: impl FnMut(usize) -> Pixels,
) -> Fit {
    let mut fits = |fit: Fit| -> bool {
        // With a "+N" of no width, the least the step can take.
        if span(widths, fit, metrics, px(0.)) > room {
            return false;
        }
        let more = if fit.more > 0 { more(fit.more) } else { px(0.) };
        span(widths, fit, metrics, more) <= room
    };
    let first = if metrics.whole {
        fitting(widths, room, metrics)
    } else {
        0
    };
    // The first `whole` items whole and the rest as dots, as many as
    // are drawn: from as many whole as fit down to none.
    for whole in (0..=first).rev() {
        let dots = (n - whole).min(metrics.max_dots);
        let fit = Fit {
            whole,
            dots,
            more: n - whole - dots,
        };
        if fits(fit) {
            return fit;
        }
    }
    // Fewer dots, the "+N" counting the rest.
    for dots in (1..n.min(metrics.max_dots)).rev() {
        let fit = Fit {
            whole: 0,
            dots,
            more: n - dots,
        };
        if fits(fit) {
            return fit;
        }
    }
    Fit::NOTHING
}

pub struct Placed {
    fit: Fit,
    dots: Vec<AnyElement>,
    more: Option<AnyElement>,
}

impl Metrics {
    /// These metrics at `scale`: what holds text (the strip's height, the
    /// gaps between chips) grows with it; a dot is drawn as an icon is, at
    /// its own size, and so is the step between dots, which makes them
    /// overlap as they were written to (ADR-117).
    #[must_use]
    pub fn at(self, scale: crate::sizes::Scale) -> Self {
        Metrics {
            height: scale.f(self.height),
            gap: scale.f(self.gap),
            ..self
        }
    }
}

impl Element for ChipStrip {
    type RequestLayoutState = ();
    type PrepaintState = Placed;
    fn id(&self) -> Option<ElementId> {
        None
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
        // At the size of the text where it stands, once a frame: prepaint
        // reads what this set.
        self.metrics = self.metrics.at(crate::sizes::Scale::of(window));
        let height = px(self.metrics.height);
        if self.natural {
            let natural = size(AvailableSpace::MaxContent, AvailableSpace::Definite(height));
            let n = self.count;
            let widths: Vec<Pixels> = if self.metrics.whole {
                (0..n)
                    .map(|i| self.built(i).layout_as_root(natural, window, cx).width)
                    .collect()
            } else {
                Vec::new()
            };
            let all = if self.metrics.whole {
                Fit {
                    whole: n,
                    dots: 0,
                    more: 0,
                }
            } else {
                Fit {
                    whole: 0,
                    dots: n.min(self.metrics.max_dots),
                    more: 0,
                }
            };
            let widest = span(&widths, all, &self.metrics, px(0.));
            let mut style = Style {
                flex_grow: 0.,
                flex_shrink: 1.,
                ..Style::default()
            };
            style.min_size.width = px(0.).into();
            style.max_size.width = gpui::relative(1.).into();
            let layout = window.request_measured_layout(style, move |known, _, _, _| {
                size(
                    known.width.unwrap_or(widest),
                    known.height.unwrap_or(height),
                )
            });
            return (layout, ());
        }
        // What it is given, with no basis of its own.
        let mut style = Style {
            flex_grow: 1.,
            flex_shrink: 1.,
            flex_basis: px(0.).into(),
            ..Style::default()
        };
        style.min_size.width = px(0.).into();
        style.size.height = height.into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Placed {
        let metrics = self.metrics;
        let height = px(metrics.height);
        let natural = size(AvailableSpace::MaxContent, AvailableSpace::Definite(height));
        let n = self.count;
        let room = bounds.size.width;
        // The first items whole, up to the first that does not fit after
        // those before it: none after it is drawn whole.
        let mut sizes: Vec<gpui::Size<Pixels>> = Vec::new();
        if metrics.whole {
            let mut used = px(0.);
            for i in 0..n {
                let measured = self.built(i).layout_as_root(natural, window, cx);
                used += measured.width + if i > 0 { px(metrics.gap) } else { px(0.) };
                sizes.push(measured);
                if used > room {
                    break;
                }
            }
        }
        let widths: Vec<Pixels> = sizes.iter().map(|s| s.width).collect();
        // Each "+N" measured is kept, to draw the one that fits.
        let mut counts: Vec<(usize, AnyElement, gpui::Size<Pixels>)> = Vec::new();
        let fit = fit(&widths, n, room, &metrics, |left| {
            if let Some((_, _, measured)) = counts.iter().find(|(m, _, _)| *m == left) {
                return measured.width;
            }
            let mut count = (self.more)(n - left..n);
            let measured = count.layout_as_root(natural, window, cx);
            counts.push((left, count, measured));
            measured.width
        });
        let gap = px(metrics.gap);
        let centered = |h: Pixels| bounds.origin.y + (height - h) / 2.;
        let mut x = bounds.origin.x;
        if metrics.center {
            let more = counts
                .iter()
                .find(|(left, _, _)| *left == fit.more)
                .map_or(px(0.), |(_, _, measured)| measured.width);
            x += ((bounds.size.width - span(&widths, fit, &metrics, more)) / 2.).max(px(0.));
        }
        for (item, measured) in self.built.iter_mut().zip(&sizes).take(fit.whole) {
            item.prepaint_at(point(x, centered(measured.height)), window, cx);
            x += measured.width + gap;
        }
        let dot = px(metrics.dot);
        let mut dots = Vec::with_capacity(fit.dots);
        for i in fit.whole..fit.whole + fit.dots {
            let mut d = (self.dot)(i);
            d.layout_as_root(size(dot, dot).map(AvailableSpace::Definite), window, cx);
            d.prepaint_at(point(x, centered(dot)), window, cx);
            dots.push(d);
            x += px(metrics.dot_step);
        }
        if fit.dots > 0 {
            x += dot - px(metrics.dot_step) + gap;
        }
        let more = (fit.more > 0)
            .then(|| {
                let at = counts.iter().position(|(left, _, _)| *left == fit.more)?;
                let (_, mut count, measured) = counts.swap_remove(at);
                count.prepaint_at(point(x, centered(measured.height)), window, cx);
                Some(count)
            })
            .flatten();
        Placed { fit, dots, more }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        placed: &mut Placed,
        window: &mut Window,
        cx: &mut App,
    ) {
        for item in self.built.iter_mut().take(placed.fit.whole) {
            item.paint(window, cx);
        }
        for dot in &mut placed.dots {
            dot.paint(window, cx);
        }
        if let Some(more) = &mut placed.more {
            more.paint(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(w: &[f32]) -> Vec<Pixels> {
        w.iter().map(|w| px(*w)).collect()
    }

    /// Dots of 8 px, 10 px apart, at most three; 4 px between parts.
    const ROW: Metrics = Metrics {
        height: 18.,
        gap: 4.,
        dot: 8.,
        dot_step: 10.,
        max_dots: 3,
        whole: true,
        center: false,
    };

    fn of(whole: usize, dots: usize, more: usize) -> Fit {
        Fit { whole, dots, more }
    }

    /// At a larger text, the strip and its gaps grow; its dots are drawn at
    /// their own size (`tags::strip_dot`), so they are laid out at it too,
    /// overlapping as written.
    #[test]
    fn a_larger_text_grows_the_strip_not_its_dots() {
        let at = ROW.at(crate::sizes::Scale(1.5));
        assert_eq!((at.height, at.gap), (27., 6.));
        assert_eq!((at.dot, at.dot_step), (ROW.dot, ROW.dot_step));
    }

    #[test]
    fn it_steps_from_whole_to_dots_to_a_count() {
        let chips = widths(&[40., 30., 50.]);
        let badge = |_| px(20.);
        // 40 + 4 + 30 + 4 + 50 = 128.
        assert_eq!(fit(&chips, chips.len(), px(128.), &ROW, badge), of(3, 0, 0));
        // Two whole, the third a dot: 40 + 4 + 30 + 4 + 8 = 86.
        assert_eq!(fit(&chips, chips.len(), px(127.), &ROW, badge), of(2, 1, 0));
        assert_eq!(fit(&chips, chips.len(), px(86.), &ROW, badge), of(2, 1, 0));
        // One whole, two dots: 40 + 4 + 8 + 10 = 62.
        assert_eq!(fit(&chips, chips.len(), px(85.), &ROW, badge), of(1, 2, 0));
        assert_eq!(fit(&chips, chips.len(), px(62.), &ROW, badge), of(1, 2, 0));
        // Dots only: 8 + 10 + 10 = 28.
        assert_eq!(fit(&chips, chips.len(), px(61.), &ROW, badge), of(0, 3, 0));
        assert_eq!(fit(&chips, chips.len(), px(28.), &ROW, badge), of(0, 3, 0));
        // Fewer dots and the count: 8 + 10 + 4 + 20 = 42 does not fit
        // in 27; 8 + 4 + 20 = 32 neither: nothing.
        assert_eq!(fit(&chips, chips.len(), px(27.), &ROW, badge), of(0, 0, 0));
        assert_eq!(fit(&[], 0, px(0.), &ROW, badge), of(0, 0, 0));
    }

    #[test]
    fn dots_beyond_the_most_drawn_are_counted() {
        let chips = widths(&[40., 40., 40., 40., 40., 40.]);
        let badge = |_| px(16.);
        // Two whole, three dots, "+1": 40 + 4 + 40 + 4 + 28 + 4 + 16 = 136.
        assert_eq!(fit(&chips, chips.len(), px(136.), &ROW, badge), of(2, 3, 1));
        // None whole: three dots and "+3", 28 + 4 + 16 = 48.
        assert_eq!(fit(&chips, chips.len(), px(48.), &ROW, badge), of(0, 3, 3));
        // Two dots and "+4": 18 + 4 + 16 = 38; one and "+5": 28.
        assert_eq!(fit(&chips, chips.len(), px(47.), &ROW, badge), of(0, 2, 4));
        assert_eq!(fit(&chips, chips.len(), px(37.), &ROW, badge), of(0, 1, 5));
        assert_eq!(fit(&chips, chips.len(), px(27.), &ROW, badge), of(0, 0, 0));
    }

    #[test]
    fn a_strip_without_names_draws_dots_however_wide() {
        let chips = widths(&[40., 30.]);
        let dots = Metrics {
            whole: false,
            ..ROW
        };
        assert_eq!(
            fit(&chips, chips.len(), px(1000.), &dots, |_| px(20.)),
            of(0, 2, 0)
        );
    }

    #[test]
    fn overlapping_dots_take_less_room() {
        // Dots of 12 px overlapping by a third: 12 + 8 + 8 = 28.
        let over = Metrics {
            dot: 12.,
            dot_step: 8.,
            ..ROW
        };
        let chips = widths(&[60., 60., 60.]);
        assert_eq!(
            fit(&chips, chips.len(), px(28.), &over, |_| px(20.)),
            of(0, 3, 0)
        );
        assert_eq!(
            fit(&chips, chips.len(), px(27.), &over, |_| px(20.)),
            of(0, 0, 0)
        );
    }

    #[test]
    fn each_count_is_measured_for_what_it_says() {
        let chips = widths(&[40., 40., 40., 40., 40.]);
        let mut asked = Vec::new();
        let shown = fit(&chips, chips.len(), px(40.), &ROW, |n| {
            asked.push(n);
            px(10. * n as f32)
        });
        // One whole and three dots take 76 px before any "+1": it is not
        // asked. Three dots and "+2" (28 + 4 + 20 = 52), two and "+3"
        // (18 + 4 + 30), one and "+4" (8 + 4 + 40) are: none fits.
        assert_eq!(shown, of(0, 0, 0));
        assert_eq!(asked, [2, 3, 4]);
    }

    /// The widths of the first items up to the first that does not fit
    /// after those before it say what all of them would: the step is the
    /// same, and only the count drawn is measured.
    #[test]
    fn the_first_widths_say_what_all_would() {
        let all = widths(&[40.; 70]);
        let mut asked = Vec::new();
        let mut badge = |n: usize| {
            asked.push(n);
            px(if n >= 10 { 22. } else { 16. })
        };
        // 40 + 4 + 40 + 4 + 40 = 128 for three; a fourth passes 150.
        assert_eq!(fitting(&all, px(150.), &ROW), 3);
        let fit_all = fit(&all, 70, px(150.), &ROW, &mut badge);
        let first = fit(&all[..4], 70, px(150.), &ROW, &mut badge);
        assert_eq!(fit_all, first);
        // Two whole, three dots, "+65": 84 + 4 + 28 + 4 + 22 = 142.
        assert_eq!(first, of(2, 3, 65));
        assert_eq!(asked, [65, 65], "one count, once a search");
    }
}
