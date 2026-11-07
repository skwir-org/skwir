//! An area's ground painted once around the parts drawn in it as things of
//! the area (ADR-119, amended 2026-11-07): the address field and the view
//! buttons' group in a pane's row, the tab shown in the strip or the
//! sidebar. Each part says where it is as it is laid out (`hole`) and
//! paints its own ground at the area's opacity; the area paints its own
//! around them (`ground`), so the desktop shows through both as through
//! the area, never through two grounds laid one on the other. Whole, the
//! area paints under them too: nothing shows through to tell.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Corners, Element, ElementId, FillOptions, FillRule, GlobalElementId,
    Hsla, InspectorElementId, IntoElement, LayoutId, PathBuilder, PathStyle, Pixels, Rems, Window,
};

/// Where the parts of an area are this frame, each a rounded rectangle:
/// written as they are laid out, read as the area paints.
#[derive(Clone, Default)]
pub struct Holes(Rc<RefCell<Vec<(Bounds<Pixels>, Corners<Pixels>)>>>);

impl Holes {
    pub(crate) fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub(crate) fn laid(&self) -> Vec<(Bounds<Pixels>, Corners<Pixels>)> {
        self.0.borrow().clone()
    }
}

/// `child`'s area, `color` painted under it once: around the holes laid
/// out in it while `color` lets what is behind through, under it all
/// while it is whole.
pub fn ground(holes: &Holes, color: Hsla, child: impl IntoElement) -> Ground {
    Ground {
        holes: holes.clone(),
        color,
        corners: Corners::default(),
        child: child.into_any_element(),
    }
}

impl Ground {
    /// The area's own corners, rounded as it is.
    pub fn rounded(mut self, corners: Corners<Rems>) -> Self {
        self.corners = corners;
        self
    }
}

/// `child` as a part of the area `holes` is of: a hole in its ground where
/// it is laid out, rounded by `radius` as its own corners are.
pub fn hole(holes: &Holes, radius: Rems, child: impl IntoElement) -> Hole {
    Hole {
        holes: holes.clone(),
        radius,
        child: child.into_any_element(),
    }
}

pub struct Ground {
    holes: Holes,
    color: Hsla,
    corners: Corners<Rems>,
    child: AnyElement,
}

pub struct Hole {
    holes: Holes,
    radius: Rems,
    child: AnyElement,
}

impl IntoElement for Ground {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl IntoElement for Hole {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Ground {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.holes.clear();
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let rem = window.rem_size();
        let corners = self
            .corners
            .map(|radius| radius.to_pixels(rem))
            .clamp_radii_for_quad_size(bounds.size);
        paint_around(bounds, corners, &self.holes.laid(), self.color, window);
        self.child.paint(window, cx);
    }
}

impl Element for Hole {
    type RequestLayoutState = ();
    type PrepaintState = ();

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
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let radius = self.radius.to_pixels(window.rem_size());
        self.holes
            .0
            .borrow_mut()
            .push((bounds, Corners::all(radius)));
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// `color` over `bounds`, around `holes` while it lets what is behind
/// through, under them too while it is whole.
fn paint_around(
    bounds: Bounds<Pixels>,
    corners: Corners<Pixels>,
    holes: &[(Bounds<Pixels>, Corners<Pixels>)],
    color: Hsla,
    window: &mut Window,
) {
    if color.a >= 1. || holes.is_empty() {
        window.paint_quad(gpui::fill(bounds, color).corner_radii(corners));
        return;
    }
    let mut path = PathBuilder::fill().with_style(PathStyle::Fill(
        FillOptions::default().with_fill_rule(FillRule::EvenOdd),
    ));
    crate::elements::frame::rounded_rect(&mut path, bounds, corners);
    for (hole, corners) in holes {
        crate::elements::frame::rounded_rect(&mut path, *hole, *corners);
    }
    match path.build() {
        Ok(path) => window.paint_path(path, color),
        // Nothing painted rather than a ground twice under the parts.
        Err(error) => tracing::warn!("cannot shape an area's ground: {error}"),
    }
}
