//! An area's ground painted once around the parts drawn in it as things of
//! the area (ADR-119, amended 2026-11-07): the address field and the view
//! buttons' group in a pane's row, the tab shown in the strip or the
//! sidebar. Each part says where it is as it is laid out (`hole`) and
//! paints its own ground at the area's opacity; the area paints its own
//! around them (`ground`), so the desktop shows through both as through
//! the area, never through two grounds laid one on the other. Whole, the
//! area paints under them too: nothing shows through to tell.
//!
//! Where a part's rounded corner meets the area's ground, the area's curve
//! is the exact complement of the part's, told how opaque the part is
//! (`Window::paint_around`, gpui patch 0055): two edges antialiased apart
//! would let a seam of what is behind through. The straight edges are a
//! path on whole device pixels, where quads are drawn.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ContentMask, Corners, Element, ElementId, FillOptions, FillRule,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, PathBuilder, PathStyle,
    Pixels, Rems, Window, point, px, size,
};

/// Where the parts of an area are this frame: written as they are laid
/// out, read as the area paints.
#[derive(Clone, Default)]
pub struct Holes(Rc<RefCell<Vec<Laid>>>);

/// A part as it is laid out: its rounded rectangle, on whole device
/// pixels, what of it is not clipped away (a tab scrolled under the
/// sidebar's header), and how opaque its own ground is.
#[derive(Clone, Copy)]
pub struct Laid {
    pub bounds: Bounds<Pixels>,
    pub shown: Bounds<Pixels>,
    pub corners: Corners<Pixels>,
    pub under: f32,
}

impl Laid {
    /// The square of `corner` of its shape, as far as it is shown: none
    /// where the corner is square, or clipped away.
    fn square(&self, corner: Toward, scale: f32) -> Option<Bounds<Pixels>> {
        let radius = corner.of(self.corners);
        if radius <= px(0.) {
            return None;
        }
        let square = square_at(self.bounds, corner, radius, scale).intersect(&self.shown);
        (!square.is_empty()).then_some(square)
    }
}

impl Holes {
    pub(crate) fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub(crate) fn laid(&self) -> Vec<Laid> {
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
/// it is laid out, rounded by `radius` as its own corners are; `under` is
/// how opaque its own ground is.
pub fn hole(holes: &Holes, radius: Rems, under: f32, child: impl IntoElement) -> Hole {
    Hole {
        holes: holes.clone(),
        radius,
        under,
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
    under: f32,
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
        let bounds = window.snap_to_device_pixels(bounds);
        let corners = self
            .corners
            .map(|radius| snap_length(radius.to_pixels(window.rem_size()), window))
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
        // As the part's quad is drawn: on whole device pixels, its radius
        // as it is written. Only what is shown is a hole: a part clipped
        // away (scrolled under a header) paints nothing of its own there.
        let radius = self.radius.to_pixels(window.rem_size());
        let shown = window.snap_to_device_pixels(bounds.intersect(&window.content_mask().bounds));
        if !shown.is_empty() {
            self.holes.0.borrow_mut().push(Laid {
                bounds: window.snap_to_device_pixels(bounds),
                shown,
                corners: Corners::all(radius),
                under: self.under,
            });
        }
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

/// `color` over `bounds`, rounded by `corners`: around `holes` while it
/// lets what is behind through, its straight edges a path, its own rounded
/// corners each a quad in its corner's square, the holes' each the
/// complement of the part there; under them too while it is whole.
fn paint_around(
    bounds: Bounds<Pixels>,
    corners: Corners<Pixels>,
    holes: &[Laid],
    color: Hsla,
    window: &mut Window,
) {
    if color.a >= 1. || holes.is_empty() {
        window.paint_quad(gpui::fill(bounds, color).corner_radii(corners));
        return;
    }
    let squares: Vec<(Toward, Bounds<Pixels>)> = Toward::ALL
        .into_iter()
        .filter(|corner| corner.of(corners) > px(0.))
        .map(|corner| {
            (
                corner,
                corner_square(bounds, corner, corner.of(corners), window),
            )
        })
        .collect();
    let mut path = PathBuilder::fill().with_style(PathStyle::Fill(
        FillOptions::default().with_fill_rule(FillRule::EvenOdd),
    ));
    less_corner_squares(&mut path, bounds, &squares);
    for hole in holes {
        crate::elements::frame::rounded_rect(&mut path, hole.shown, Corners::default());
    }
    match path.build() {
        Ok(path) => window.paint_path(path, color),
        // Nothing painted rather than a ground twice under the parts.
        Err(error) => tracing::warn!("cannot shape an area's ground: {error}"),
    }
    for (_, square) in squares {
        window.with_content_mask(Some(ContentMask::new(square)), |window| {
            window.paint_quad(gpui::fill(bounds, color).corner_radii(corners));
        });
    }
    for hole in holes {
        paint_corners_around(hole, color, window);
    }
}

/// `bounds` as a closed path, less the squares of its rounded corners
/// (`squares`), which their quads paint. Clockwise.
fn less_corner_squares(
    path: &mut PathBuilder,
    bounds: Bounds<Pixels>,
    squares: &[(Toward, Bounds<Pixels>)],
) {
    let side = |corner: Toward| {
        squares
            .iter()
            .find(|(of, _)| *of == corner)
            .map_or(px(0.), |(_, square)| square.size.width)
    };
    let (l, t, r, b) = (bounds.left(), bounds.top(), bounds.right(), bounds.bottom());
    let (tl, tr) = (side(Toward::TopLeft), side(Toward::TopRight));
    let (br, bl) = (side(Toward::BottomRight), side(Toward::BottomLeft));
    path.move_to(point(l + tl, t));
    path.line_to(point(r - tr, t));
    path.line_to(point(r - tr, t + tr));
    path.line_to(point(r, t + tr));
    path.line_to(point(r, b - br));
    path.line_to(point(r - br, b - br));
    path.line_to(point(r - br, b));
    path.line_to(point(l + bl, b));
    path.line_to(point(l + bl, b - bl));
    path.line_to(point(l, b - bl));
    path.line_to(point(l, t + tl));
    path.line_to(point(l + tl, t + tl));
    path.close();
}

/// A ground's curves around a part: in the square of each of its rounded
/// corners, as far as it is shown, the complement of the part, told how
/// opaque the part is.
pub(crate) fn paint_corners_around(hole: &Laid, color: Hsla, window: &mut Window) {
    for corner in Toward::ALL {
        if let Some(square) = hole.square(corner, window.scale_factor()) {
            window.paint_around(square, hole.bounds, hole.corners, color, hole.under);
        }
    }
}

/// A corner of a rectangle.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

impl Toward {
    pub const ALL: [Toward; 4] = [
        Toward::TopLeft,
        Toward::TopRight,
        Toward::BottomRight,
        Toward::BottomLeft,
    ];

    /// Its radius among `corners`.
    pub fn of(self, corners: Corners<Pixels>) -> Pixels {
        match self {
            Toward::TopLeft => corners.top_left,
            Toward::TopRight => corners.top_right,
            Toward::BottomRight => corners.bottom_right,
            Toward::BottomLeft => corners.bottom_left,
        }
    }
}

/// The square at `rect`'s `corner` that holds its arc of `radius`, inside
/// it, on whole device pixels.
pub fn corner_square(
    rect: Bounds<Pixels>,
    corner: Toward,
    radius: Pixels,
    window: &Window,
) -> Bounds<Pixels> {
    square_at(rect, corner, radius, window.scale_factor())
}

fn square_at(rect: Bounds<Pixels>, corner: Toward, radius: Pixels, scale: f32) -> Bounds<Pixels> {
    let side = px((f32::from(radius) * scale).ceil() / scale)
        .min(rect.size.width)
        .min(rect.size.height);
    let x = match corner {
        Toward::TopLeft | Toward::BottomLeft => rect.left(),
        Toward::TopRight | Toward::BottomRight => rect.right() - side,
    };
    let y = match corner {
        Toward::TopLeft | Toward::TopRight => rect.top(),
        Toward::BottomRight | Toward::BottomLeft => rect.bottom() - side,
    };
    Bounds::new(point(x, y), size(side, side))
}

/// `length` on whole device pixels: a radius the shapes beside it meet
/// exactly.
pub fn snap_length(length: Pixels, window: &Window) -> Pixels {
    let scale = window.scale_factor();
    px((f32::from(length) * scale).round() / scale)
}

#[cfg(test)]
mod tests {
    use super::{Laid, Toward};
    use gpui::{Bounds, Corners, point, px, size};

    /// A tab scrolled under the sidebar's header is a hole only as far as
    /// it is shown: the corners clipped away have no curve to paint, and
    /// one the header cuts through keeps what is left of its square.
    #[test]
    fn a_part_clipped_away_leaves_no_hole_there() {
        let bounds = Bounds::new(point(px(10.), px(100.)), size(px(100.), px(40.)));
        let shown_from = |top: f32| Laid {
            bounds,
            shown: Bounds::from_corners(point(px(10.), px(top)), bounds.bottom_right()),
            corners: Corners::all(px(8.)),
            under: 1.,
        };
        // Its top corners wholly under the header.
        let clipped = shown_from(110.);
        assert!(clipped.square(Toward::TopLeft, 1.).is_none());
        assert!(clipped.square(Toward::TopRight, 1.).is_none());
        assert!(clipped.square(Toward::BottomLeft, 1.).is_some());
        // The header through them: what is left of their square.
        let square = shown_from(104.).square(Toward::TopRight, 1.).unwrap();
        assert_eq!(square.top(), px(104.));
        assert_eq!(square.bottom(), px(108.));
        assert_eq!(square.left(), px(102.));
        // Not clipped, the whole of it.
        let whole = shown_from(100.).square(Toward::TopLeft, 1.).unwrap();
        assert_eq!(whole.size, size(px(8.), px(8.)));
    }
}
