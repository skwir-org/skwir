//! A line of chips that keeps what fits and says how many it left out.
//!
//! The chips a list row shows beside a name (its tags) take only the width
//! the name leaves. Those that fit are drawn whole, in order; when some do
//! not, a "+N" chip (built by `more` for the count) ends the line in the
//! place of as many as it needs. Which fit is known only once the line has
//! its width, so it is decided here, in prepaint, from each chip's natural
//! width: never in `render`.
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window, point, px, size,
};

pub struct ChipLine {
    pub chips: Vec<AnyElement>,
    /// The chip that says how many were left out.
    pub more: Box<dyn Fn(usize) -> AnyElement>,
    pub height: f32,
    pub gap: f32,
}

impl IntoElement for ChipLine {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// How many of the chips of `widths` to draw, and whether "+N" follows
/// them, in `room` with `gap` between chips; `more(n)` measures the "+N"
/// for `n` left out. All when they fit; otherwise as many as fit beside
/// the "+N", which may be none; nothing at all when not even it fits.
fn fit(
    widths: &[Pixels],
    room: Pixels,
    gap: Pixels,
    mut more: impl FnMut(usize) -> Pixels,
) -> (usize, bool) {
    let span = |n: usize| -> Pixels {
        widths[..n].iter().copied().fold(px(0.), |a, w| a + w) + gap * n.saturating_sub(1) as f32
    };
    if span(widths.len()) <= room {
        return (widths.len(), false);
    }
    for shown in (0..widths.len()).rev() {
        let badge = more(widths.len() - shown);
        let used = if shown == 0 {
            badge
        } else {
            span(shown) + gap + badge
        };
        if used <= room {
            return (shown, true);
        }
    }
    (0, false)
}

pub struct Placed {
    shown: usize,
    more: Option<AnyElement>,
}

impl Element for ChipLine {
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
        // What the name leaves, with no basis of its own.
        let mut style = Style {
            flex_grow: 1.,
            flex_shrink: 1.,
            flex_basis: px(0.).into(),
            ..Style::default()
        };
        style.min_size.width = px(0.).into();
        style.size.height = px(self.height).into();
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
        let natural = size(
            AvailableSpace::MaxContent,
            AvailableSpace::Definite(px(self.height)),
        );
        let widths: Vec<Pixels> = self
            .chips
            .iter_mut()
            .map(|chip| chip.layout_as_root(natural, window, cx).width)
            .collect();
        let gap = px(self.gap);
        let mut badges: Vec<(usize, AnyElement, Pixels)> = Vec::new();
        let (shown, with_more) = fit(&widths, bounds.size.width, gap, |n| {
            let mut badge = (self.more)(n);
            let width = badge.layout_as_root(natural, window, cx).width;
            badges.push((n, badge, width));
            width
        });
        let mut x = bounds.origin.x;
        for (chip, width) in self.chips.iter_mut().zip(&widths).take(shown) {
            chip.prepaint_at(point(x, bounds.origin.y), window, cx);
            x += *width + gap;
        }
        let more = with_more
            .then(|| badges.pop())
            .flatten()
            .map(|(n, mut badge, _)| {
                debug_assert_eq!(n, self.chips.len() - shown);
                badge.prepaint_at(point(x, bounds.origin.y), window, cx);
                badge
            });
        Placed { shown, more }
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
        for chip in self.chips.iter_mut().take(placed.shown) {
            chip.paint(window, cx);
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

    #[test]
    fn all_fit_or_the_last_give_way_to_the_count() {
        let chips = widths(&[40., 30., 50.]);
        let badge = |_| px(20.);
        // 40 + 4 + 30 + 4 + 50 = 128.
        assert_eq!(fit(&chips, px(128.), px(4.), badge), (3, false));
        // Two and "+1": 40 + 4 + 30 + 4 + 20 = 98.
        assert_eq!(fit(&chips, px(127.), px(4.), badge), (2, true));
        assert_eq!(fit(&chips, px(98.), px(4.), badge), (2, true));
        // One and "+2": 40 + 4 + 20 = 64.
        assert_eq!(fit(&chips, px(97.), px(4.), badge), (1, true));
        // Only "+3".
        assert_eq!(fit(&chips, px(63.), px(4.), badge), (0, true));
        assert_eq!(fit(&chips, px(20.), px(4.), badge), (0, true));
        // Not even that.
        assert_eq!(fit(&chips, px(19.), px(4.), badge), (0, false));
        assert_eq!(fit(&[], px(0.), px(4.), badge), (0, false));
    }

    #[test]
    fn the_count_is_measured_for_what_it_says() {
        let chips = widths(&[40., 40., 40.]);
        // "+1" is narrower than "+10" would be: each count is measured.
        let mut asked = Vec::new();
        let shown = fit(&chips, px(90.), px(0.), |n| {
            asked.push(n);
            px(10. * n as f32)
        });
        assert_eq!(shown, (2, true));
        assert_eq!(asked, [1]);
    }
}
