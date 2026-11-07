//! The window's frame (`Theme::cards`, ADR-123): the ground around and
//! between the window's cards (its panes, the preview, the terminal),
//! which the sidebar and the tabs lie on. Each card says where it is as it
//! is laid out (`card`); the frame paints its ground once before them,
//! around them, so what is behind the window shows through each ground as
//! much as its own opacity lets it (ADR-119).
//!
//! A card runs to the window's edge wherever it meets it (the shell leaves
//! a gutter at the top alone, `Shell::frame_inset`): its corners are
//! rounded where it stands apart, square on the window's edge, which the
//! window's own corners round. It keeps what it holds within them (a
//! rounded content mask, gpui patch 0053), and the frame's holes are the
//! same shapes.
//!
//! What a card's top row holds that is the window's rather than the
//! card's (Split, Properties and Preview at its end, the sidebar's toggle
//! and More at its start) lies on the frame, in a notch of the card
//! (`notch`): the row's band ends before it with a rounded corner, and its
//! edge comes down to the files with a concave one, its fillet painted in
//! the band's ground.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ContentMask, Corners, Element, ElementId, FillOptions, FillRule,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, PathBuilder, PathStyle,
    Pixels, Window, point, px,
};

/// How far a card's corners are rounded where it stands apart:
/// `rounded_xl`, in the window's rem.
pub const RADIUS_REMS: f32 = 0.75;

/// The room between two cards, and above them where the window does not
/// meet another (`Window::tiling`): pointer geometry as much as looks (the
/// gutter is the edge dragged), so it keeps its size whatever the text's.
pub const GUTTER: f32 = 6.;

/// Where the cards are this frame: written as they are laid out, read by
/// the frame as it paints.
#[derive(Clone, Default)]
pub struct Cards(Rc<RefCell<Laid>>);

#[derive(Default)]
struct Laid {
    /// The frame's own bounds: the window's edges a card may meet.
    frame: Bounds<Pixels>,
    cards: Vec<Placed>,
}

#[derive(Clone, Copy)]
struct Placed {
    bounds: Bounds<Pixels>,
    corners: Corners<Pixels>,
    /// The radius of a corner where it stands apart.
    radius: Pixels,
    /// Whether its left and its right edge are on the window's.
    on_edge: [bool; 2],
    /// The notches of its top row: at its start and at its end.
    notches: [Option<Notch>; 2],
}

/// Where a notch is cut from a card's top row, and the ground of the band
/// it cuts into (its fillets are painted in it).
#[derive(Clone, Copy)]
struct Notch {
    bounds: Bounds<Pixels>,
    ground: Hsla,
}

impl Placed {
    fn notched(&self) -> bool {
        self.notches.iter().any(Option::is_some)
    }
}

/// The frame under `child`: `ground` painted once around the cards laid
/// out in it, as holes (`cut`) while what is behind the window shows
/// through, else under them all (whole, nothing shows between them).
pub fn frame(cards: &Cards, ground: Hsla, cut: bool, child: impl IntoElement) -> Frame {
    Frame {
        cards: cards.clone(),
        ground,
        cut,
        child: child.into_any_element(),
    }
}

/// `child` as a card of the frame: its content kept within its corners,
/// and a hole in the frame where it is laid out.
pub fn card(cards: &Cards, child: impl IntoElement) -> Card {
    Card {
        cards: cards.clone(),
        child: child.into_any_element(),
    }
}

pub struct Frame {
    cards: Cards,
    ground: Hsla,
    cut: bool,
    child: AnyElement,
}

pub struct Card {
    cards: Cards,
    child: AnyElement,
}

/// `child`, in the top row of the card it is laid out in, as a notch of it
/// on the frame: at the row's start or its end, whichever it is nearer;
/// `ground` is the row's band, which the notch's fillet continues.
pub fn notch(cards: &Cards, ground: Hsla, child: impl IntoElement) -> NotchOf {
    NotchOf {
        cards: cards.clone(),
        ground,
        child: child.into_any_element(),
    }
}

pub struct NotchOf {
    cards: Cards,
    ground: Hsla,
    child: AnyElement,
}

impl IntoElement for NotchOf {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for NotchOf {
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
        // The card it is in prepaints before what it holds: the last laid.
        if let Some(card) = self.cards.0.borrow_mut().cards.last_mut() {
            let end = (card.bounds.right() - bounds.right()).abs()
                < (bounds.left() - card.bounds.left()).abs();
            card.notches[usize::from(end)] = Some(Notch {
                bounds,
                ground: self.ground,
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

impl IntoElement for Frame {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl IntoElement for Card {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Frame {
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
        // The cards say where they are as they are laid out, below.
        {
            let mut laid = self.cards.0.borrow_mut();
            laid.frame = bounds;
            laid.cards.clear();
        }
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
        let placed = self.cards.0.borrow().cards.clone();
        paint_ground(bounds, &placed, self.ground, self.cut, window);
        for card in &placed {
            paint_fillets(card, window);
        }
        self.child.paint(window, cx);
    }
}

/// The frame's ground over `bounds`: around `cards` when `cut`, else
/// under them too.
fn paint_ground(
    bounds: Bounds<Pixels>,
    cards: &[Placed],
    ground: Hsla,
    cut: bool,
    window: &mut Window,
) {
    if !cut || cards.is_empty() {
        window.paint_quad(gpui::fill(bounds, ground));
        return;
    }
    let mut path = PathBuilder::fill().with_style(PathStyle::Fill(
        FillOptions::default().with_fill_rule(FillRule::EvenOdd),
    ));
    rounded_rect(&mut path, bounds, Corners::default());
    for card in cards {
        outline(&mut path, card);
    }
    match path.build() {
        Ok(path) => window.paint_path(path, ground),
        // Nothing painted rather than a ground twice under the cards.
        Err(error) => tracing::warn!("cannot shape the window's frame: {error}"),
    }
}

/// A closed rectangle, each corner rounded by its radius, held to half its
/// shorter side.
fn rounded_rect(path: &mut PathBuilder, bounds: Bounds<Pixels>, corners: Corners<Pixels>) {
    let most = (bounds.size.width / 2.).min(bounds.size.height / 2.);
    let r = corners.map(|radius| (*radius).min(most).max(px(0.)));
    let (left, top) = (bounds.left(), bounds.top());
    let (right, bottom) = (bounds.right(), bounds.bottom());
    let arc = |path: &mut PathBuilder, radius: Pixels, to| {
        if radius > px(0.) {
            path.arc_to(point(radius, radius), px(0.), false, true, to);
        }
    };
    path.move_to(point(left + r.top_left, top));
    path.line_to(point(right - r.top_right, top));
    arc(path, r.top_right, point(right, top + r.top_right));
    path.line_to(point(right, bottom - r.bottom_right));
    arc(path, r.bottom_right, point(right - r.bottom_right, bottom));
    path.line_to(point(left + r.bottom_left, bottom));
    arc(path, r.bottom_left, point(left, bottom - r.bottom_left));
    path.line_to(point(left, top + r.top_left));
    arc(path, r.top_left, point(left + r.top_left, top));
    path.close();
}

/// A card's outline: a rounded rectangle, less its notches (`notch`),
/// each a rounded corner where the row's band ends and a concave one where
/// its edge comes down to the files. Clockwise.
fn outline(path: &mut PathBuilder, card: &Placed) {
    if !card.notched() {
        rounded_rect(path, card.bounds, card.corners);
        return;
    }
    let (left, top) = (card.bounds.left(), card.bounds.top());
    let (right, bottom) = (card.bounds.right(), card.bounds.bottom());
    let (r, c) = (card.radius, card.corners);
    // The files' corner under a notch: square on the window's edge.
    let under = |on_edge: bool| if on_edge { px(0.) } else { r };
    let [on_left, on_right] = card.on_edge;
    let arc = |path: &mut PathBuilder, radius: Pixels, sweep: bool, to| {
        if radius > px(0.) {
            path.arc_to(point(radius, radius), px(0.), false, sweep, to);
        }
    };
    let [start, end] = card.notches;
    let start = start.map(|n| (n.bounds.right(), n.bounds.bottom()));
    let end = end.map(|n| (n.bounds.left(), n.bounds.bottom()));
    // Along the top, from the start notch's band to the end notch's.
    let (from, top_left) = start.map_or((left, c.top_left), |(x, _)| (x, r));
    let (to, top_right) = end.map_or((right, c.top_right), |(x, _)| (x, r));
    path.move_to(point(from + top_left, top));
    path.line_to(point(to - top_right, top));
    arc(path, top_right, true, point(to, top + top_right));
    if let Some((x, low)) = end {
        let corner = under(on_right);
        path.line_to(point(x, low - r));
        arc(path, r, false, point(x + r, low));
        path.line_to(point(right - corner, low));
        arc(path, corner, true, point(right, low + corner));
    }
    path.line_to(point(right, bottom - c.bottom_right));
    arc(
        path,
        c.bottom_right,
        true,
        point(right - c.bottom_right, bottom),
    );
    path.line_to(point(left + c.bottom_left, bottom));
    arc(
        path,
        c.bottom_left,
        true,
        point(left, bottom - c.bottom_left),
    );
    if let Some((x, low)) = start {
        let corner = under(on_left);
        path.line_to(point(left, low + corner));
        arc(path, corner, true, point(left + corner, low));
        path.line_to(point(x - r, low));
        arc(path, r, false, point(x, low - r));
        path.line_to(point(x, top + top_left));
    } else {
        path.line_to(point(left, top + top_left));
    }
    arc(path, top_left, true, point(from + top_left, top));
    path.close();
}

/// The fillets of a card's notches: the band's ground between its edge
/// coming down and the files, under the concave corner.
fn paint_fillets(card: &Placed, window: &mut Window) {
    let r = card.radius;
    for (index, notch) in card.notches.iter().enumerate() {
        let Some(notch) = notch else { continue };
        let low = notch.bounds.bottom();
        let mut path = PathBuilder::fill();
        if index == 0 {
            // At the start: the band's left edge, the fillet left of it.
            let x = notch.bounds.right();
            path.move_to(point(x, low - r));
            path.line_to(point(x, low));
            path.line_to(point(x - r, low));
            path.arc_to(point(r, r), px(0.), false, false, point(x, low - r));
        } else {
            let x = notch.bounds.left();
            path.move_to(point(x, low - r));
            path.line_to(point(x, low));
            path.line_to(point(x + r, low));
            path.arc_to(point(r, r), px(0.), false, true, point(x, low - r));
        }
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, notch.ground);
        }
    }
}

/// Which of a card's edges laid out at `bounds` are on the edge of the
/// frame at `frame` (the window's): top, right, bottom, left.
fn on_edges(bounds: Bounds<Pixels>, frame: Bounds<Pixels>) -> [bool; 4] {
    let on = |a: Pixels, b: Pixels| (a - b).abs() < px(0.5);
    [
        on(bounds.top(), frame.top()),
        on(bounds.right(), frame.right()),
        on(bounds.bottom(), frame.bottom()),
        on(bounds.left(), frame.left()),
    ]
}

/// A card's corners laid out at `bounds` in a frame at `frame`: `radius`
/// where it stands apart, square where either of its edges is on the
/// frame's (the window's) edge.
fn corners(bounds: Bounds<Pixels>, frame: Bounds<Pixels>, radius: Pixels) -> Corners<Pixels> {
    let [top, right, bottom, left] = on_edges(bounds, frame);
    let round = |edge: bool, other: bool| if edge || other { px(0.) } else { radius };
    Corners {
        top_left: round(top, left),
        top_right: round(top, right),
        bottom_right: round(bottom, right),
        bottom_left: round(bottom, left),
    }
}

impl Element for Card {
    type RequestLayoutState = ();
    type PrepaintState = Corners<Pixels>;

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
    ) -> Corners<Pixels> {
        let radius = gpui::rems(RADIUS_REMS).to_pixels(window.rem_size());
        let corners = {
            let mut laid = self.cards.0.borrow_mut();
            let corners = corners(bounds, laid.frame, radius);
            let [_, right, _, left] = on_edges(bounds, laid.frame);
            laid.cards.push(Placed {
                bounds,
                corners,
                radius,
                on_edge: [left, right],
                notches: [None, None],
            });
            corners
        };
        let mask = mask(bounds, corners);
        window.with_content_mask(Some(mask), |window| self.child.prepaint(window, cx));
        corners
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        corners: &mut Corners<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mask = mask(bounds, *corners);
        window.with_content_mask(Some(mask), |window| self.child.paint(window, cx));
    }
}

/// What a card holds is kept within it, its corners included.
fn mask(bounds: Bounds<Pixels>, corners: Corners<Pixels>) -> ContentMask<Pixels> {
    ContentMask {
        bounds,
        rounded_bounds: bounds,
        corner_radii: corners,
    }
}

#[cfg(test)]
mod tests {
    use super::corners;
    use gpui::{Bounds, point, px, size};

    /// A card is square where it meets the window's edge and rounded where
    /// it stands apart: the preview at the right, under a gutter at the
    /// top, is round at its top left alone.
    #[test]
    fn a_card_is_square_on_the_window_s_edge() {
        let frame = Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(800.)));
        let r = px(12.);
        let preview = Bounds::new(point(px(700.), px(8.)), size(px(300.), px(792.)));
        let c = corners(preview, frame, r);
        assert_eq!(
            [c.top_left, c.top_right, c.bottom_right, c.bottom_left],
            [r, px(0.), px(0.), px(0.)]
        );
        // A pane beside the sidebar, the preview at its right: round at
        // its top, square at its foot.
        let pane = Bounds::new(point(px(220.), px(8.)), size(px(472.), px(792.)));
        let c = corners(pane, frame, r);
        assert_eq!(
            [c.top_left, c.top_right, c.bottom_right, c.bottom_left],
            [r, r, px(0.), px(0.)]
        );
        // Half a pixel off the edge, as layout rounds, is on it.
        let pane = Bounds::new(point(px(220.), px(8.)), size(px(472.), px(791.6)));
        assert_eq!(corners(pane, frame, r).bottom_left, px(0.));
    }
}
