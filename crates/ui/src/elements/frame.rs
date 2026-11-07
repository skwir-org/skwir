//! The window's frame (`Theme::cards`, ADR-123): the ground around and
//! between the window's cards (its panes, the preview, the terminal),
//! which the sidebar and the tabs lie on. Each card says where it is as it
//! is laid out (`card`); the frame paints its ground once before them,
//! around them, so what is behind the window shows through each ground as
//! much as its own opacity lets it (ADR-119), then the ring around the
//! card commands act on.
//!
//! A card's corners are rounded and keep what it holds within them (its
//! own `overflow_hidden`, gpui's rounded content mask, patch 0053); the
//! frame's holes are the same rounded rectangles.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, BorderStyle, Bounds, Corners, Edges, Element, ElementId, FillOptions,
    FillRule, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, PathBuilder,
    PathStyle, Pixels, Window, point, px, transparent_black,
};

/// How far a card's corners are rounded: `rounded_xl`, in the window's
/// rem.
pub const RADIUS_REMS: f32 = 0.75;

/// How wide the ring around the card commands act on is, outside its edge.
pub const RING: f32 = 2.;

/// The room between two cards, and between a card and the window's edge
/// where the window does not meet another (`Window::tiling`): pointer
/// geometry as much as looks (the gutter is the edge dragged), so it keeps
/// its size whatever the text's.
pub const GUTTER: f32 = 8.;

/// Where the cards are this frame: written as they are laid out, read by
/// the frame as it paints.
#[derive(Clone, Default)]
pub struct Cards(Rc<RefCell<Vec<Placed>>>);

#[derive(Clone, Copy)]
struct Placed {
    bounds: Bounds<Pixels>,
    radius: Pixels,
    ring: Option<Hsla>,
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

/// `child` as a card of the frame: a hole in it where it is laid out,
/// rounded as its own corners are, and `ring` around it when it is the one
/// commands act on.
pub fn card(cards: &Cards, ring: Option<Hsla>, child: impl IntoElement) -> Card {
    Card {
        cards: cards.clone(),
        ring,
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
    ring: Option<Hsla>,
    child: AnyElement,
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
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        // The cards say where they are as they are laid out, below.
        self.cards.0.borrow_mut().clear();
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
        let placed = self.cards.0.borrow().clone();
        paint_ground(bounds, &placed, self.ground, self.cut, window);
        self.child.paint(window, cx);
        // Over the frame, outside the card: what it holds is never under it.
        for card in placed {
            if let Some(ring) = card.ring {
                window.paint_quad(gpui::quad(
                    card.bounds.dilate(px(RING)),
                    Corners::all(card.radius + px(RING)),
                    transparent_black(),
                    Edges::all(px(RING)),
                    ring,
                    BorderStyle::Solid,
                ));
            }
        }
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
    rounded_rect(&mut path, bounds, px(0.));
    for card in cards {
        rounded_rect(&mut path, card.bounds, card.radius);
    }
    match path.build() {
        Ok(path) => window.paint_path(path, ground),
        // Nothing painted rather than a ground twice under the cards.
        Err(error) => tracing::warn!("cannot shape the window's frame: {error}"),
    }
}

/// A closed rounded rectangle, its radius held to half its shorter side.
fn rounded_rect(path: &mut PathBuilder, bounds: Bounds<Pixels>, radius: Pixels) {
    let radius = radius
        .min(bounds.size.width / 2.)
        .min(bounds.size.height / 2.)
        .max(px(0.));
    let (left, top) = (bounds.left(), bounds.top());
    let (right, bottom) = (bounds.right(), bounds.bottom());
    let radii = point(radius, radius);
    path.move_to(point(left + radius, top));
    path.line_to(point(right - radius, top));
    if radius > px(0.) {
        path.arc_to(radii, px(0.), false, true, point(right, top + radius));
    }
    path.line_to(point(right, bottom - radius));
    if radius > px(0.) {
        path.arc_to(radii, px(0.), false, true, point(right - radius, bottom));
    }
    path.line_to(point(left + radius, bottom));
    if radius > px(0.) {
        path.arc_to(radii, px(0.), false, true, point(left, bottom - radius));
    }
    path.line_to(point(left, top + radius));
    if radius > px(0.) {
        path.arc_to(radii, px(0.), false, true, point(left + radius, top));
    }
    path.close();
}

impl Element for Card {
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
        let radius = gpui::rems(RADIUS_REMS).to_pixels(window.rem_size());
        self.cards.0.borrow_mut().push(Placed {
            bounds,
            radius,
            ring: self.ring,
        });
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
