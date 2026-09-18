//! Select the richest row that fits, using intrinsic measurement in prepaint.
//! Breakpoints follow the controls' actual widths rather than render-time bounds.
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window, px, relative, size,
};

pub struct AdaptiveRow {
    pub variants: Vec<AnyElement>,
    pub height: f32,
}
impl IntoElement for AdaptiveRow {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for AdaptiveRow {
    type RequestLayoutState = ();
    type PrepaintState = usize;
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
    ) -> usize {
        let last = self.variants.len() - 1;
        let selected = self
            .variants
            .iter_mut()
            .enumerate()
            .find_map(|(i, row)| {
                let natural = row.layout_as_root(
                    size(
                        AvailableSpace::MaxContent,
                        AvailableSpace::Definite(bounds.size.height),
                    ),
                    window,
                    cx,
                );
                (natural.width <= bounds.size.width || i == last).then_some(i)
            })
            .unwrap_or(last);
        let row = &mut self.variants[selected];
        row.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        row.prepaint_at(bounds.origin, window, cx);
        selected
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        selected: &mut usize,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.variants[*selected].paint(window, cx);
    }
}
