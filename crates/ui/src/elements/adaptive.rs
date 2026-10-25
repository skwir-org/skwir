//! Select the richest row that fits, using intrinsic measurement in prepaint.
//! Breakpoints follow the controls' actual widths rather than render-time bounds.
//!
//! Variants share element identity: a button that appears in several variants
//! must carry the same element id path in each, so its focus handle (stored in
//! gpui element state keyed by that path) survives a breakpoint change.
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window, px, relative, size,
};

pub struct AdaptiveRow {
    pub variants: Vec<AnyElement>,
    pub height: f32,
    /// Receives the chosen variant during prepaint, for a popover drawn later
    /// in the same frame (`PopoverAnchor::popup_for`). Created per render:
    /// nothing measured survives the frame.
    pub chosen: Option<Chosen>,
}

/// The variant an [`AdaptiveRow`] chose this frame.
pub type Chosen = std::rc::Rc<std::cell::Cell<Option<usize>>>;
impl IntoElement for AdaptiveRow {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// The first of `count` variants whose natural width fits `available`, or the
/// last one (the most compact) when none fits. `natural` measures one variant
/// and is called lazily, in order, until a variant fits. Empty rows select
/// nothing.
fn select(
    count: usize,
    available: Pixels,
    mut natural: impl FnMut(usize) -> Pixels,
) -> Option<usize> {
    let last = count.checked_sub(1)?;
    Some(
        (0..last)
            .find(|&index| natural(index) <= available)
            .unwrap_or(last),
    )
}

impl Element for AdaptiveRow {
    type RequestLayoutState = ();
    type PrepaintState = Option<usize>;
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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = px(self.height).into();
        style.flex_shrink = 0.;
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
    ) -> Option<usize> {
        let variants = &mut self.variants;
        let selected = select(variants.len(), bounds.size.width, |index| {
            variants[index]
                .layout_as_root(
                    size(
                        AvailableSpace::MaxContent,
                        AvailableSpace::Definite(bounds.size.height),
                    ),
                    window,
                    cx,
                )
                .width
        })?;
        let row = &mut variants[selected];
        row.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        row.prepaint_at(bounds.origin, window, cx);
        if let Some(chosen) = &self.chosen {
            chosen.set(Some(selected));
        }
        Some(selected)
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        selected: &mut Option<usize>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(selected) = *selected {
            self.variants[selected].paint(window, cx);
        }
    }
}

/// The richest of several versions of one control that fits beside what
/// shares its line: segmented buttons where there is room, a dropdown
/// where there is not. Richest first.
///
/// It is a flex item that knows its sizes before the line is laid out:
/// each version is measured in `request_layout`, and the measured layout
/// gives the widest as its natural width and the narrowest as its minimum
/// (nothing, where it `cuts`). A neighbour with a minimum of its own
/// (`flex_1().min_w(..)`) then takes what the widest leaves, or the
/// control shrinks towards the narrowest; `prepaint` draws the richest
/// version that fits the width it got, against its right edge. Like
/// `AdaptiveRow`, versions share element ids, so focus survives a change
/// of version.
pub struct Fit {
    pub variants: Vec<AnyElement>,
    /// Where even the narrowest does not fit, it is drawn narrower still,
    /// cut as its own style says (a text's ellipsis): a folded group's
    /// gist. Else the narrowest is the least it takes.
    pub cuts: bool,
}

impl IntoElement for Fit {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// The natural sizes of the versions, measured in `request_layout`.
pub struct Measured(Vec<gpui::Size<Pixels>>);

impl Element for Fit {
    type RequestLayoutState = Measured;
    type PrepaintState = Option<usize>;
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
    ) -> (LayoutId, Measured) {
        let sizes: Vec<gpui::Size<Pixels>> = self
            .variants
            .iter_mut()
            .map(|variant| {
                variant.layout_as_root(
                    size(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
                    window,
                    cx,
                )
            })
            .collect();
        let widest = sizes.iter().map(|s| s.width).fold(px(0.), Pixels::max);
        let narrowest = if self.cuts {
            px(0.)
        } else {
            sizes.iter().map(|s| s.width).fold(widest, Pixels::min)
        };
        let height = sizes.iter().map(|s| s.height).fold(px(0.), Pixels::max);
        let style = Style {
            flex_grow: 0.,
            flex_shrink: 1.,
            ..Style::default()
        };
        let layout = window.request_measured_layout(style, move |known, available, _, _| {
            let width = known.width.unwrap_or(match available.width {
                AvailableSpace::MinContent => narrowest,
                AvailableSpace::MaxContent => widest,
                AvailableSpace::Definite(room) => room.clamp(narrowest, widest),
            });
            size(width, known.height.unwrap_or(height))
        });
        (layout, Measured(sizes))
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        measured: &mut Measured,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<usize> {
        let widths = &measured.0;
        // Half a pixel of rounding is not a reason to give up the buttons.
        let selected = select(self.variants.len(), bounds.size.width + px(0.5), |index| {
            widths[index].width
        })?;
        let natural = measured.0[selected];
        let width = natural.width.min(bounds.size.width);
        let variant = &mut self.variants[selected];
        variant.layout_as_root(
            size(
                AvailableSpace::Definite(width),
                AvailableSpace::Definite(natural.height),
            ),
            window,
            cx,
        );
        let origin = bounds.origin
            + gpui::point(
                bounds.size.width - width,
                (bounds.size.height - natural.height) / 2.,
            );
        variant.prepaint_at(origin, window, cx);
        Some(selected)
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Measured,
        selected: &mut Option<usize>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(selected) = *selected {
            self.variants[selected].paint(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rows_select_nothing_without_panicking() {
        assert_eq!(select(0, px(500.), |_| unreachable!()), None);
    }

    #[test]
    fn picks_the_first_fitting_variant_or_the_last_one() {
        let widths = [px(600.), px(400.), px(200.)];
        assert_eq!(select(3, px(700.), |i| widths[i]), Some(0));
        assert_eq!(select(3, px(450.), |i| widths[i]), Some(1));
        assert_eq!(select(3, px(400.), |i| widths[i]), Some(1));
        assert_eq!(select(3, px(100.), |i| widths[i]), Some(2));
        // A single variant is chosen without being measured.
        assert_eq!(select(1, px(10.), |_| unreachable!()), Some(0));
    }

    #[test]
    fn measures_only_until_a_variant_fits() {
        let mut measured = Vec::new();
        let widths = [px(600.), px(400.), px(200.)];
        let selected = select(3, px(450.), |i| {
            measured.push(i);
            widths[i]
        });
        assert_eq!(selected, Some(1));
        assert_eq!(measured, vec![0, 1]);
    }
}
