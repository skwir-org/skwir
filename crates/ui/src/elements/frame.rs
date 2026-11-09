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
//!
//! While the window lets what is behind it through, the frame's straight
//! edges are a path on whole device pixels, as quads are drawn, and its
//! curves each the exact complement of the card's corner beside it, told
//! how opaque the card is there (`Window::paint_around`, gpui patch 0055):
//! no seam where they meet.
//!
//! Each card is outlined (`Theme::card_edge`), as Dolphin draws its views:
//! a ring a device pixel wide on the frame, just outside the card's shape,
//! its notches followed (`paint_outline`), drawn within the window, so it
//! is not where the card meets the window's edge. It is made of quads, on
//! whole device pixels: a window whole opens no path pass for its frame.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ContentMask, Corners, Element, ElementId, FillOptions, FillRule,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, PathBuilder, PathStyle,
    Pixels, Point, Window, point, px, size,
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
pub struct Cards(Rc<RefCell<Laid>>, crate::elements::ground::Holes);

impl Cards {
    /// The parts lying on the frame as things of it (the tab shown, as
    /// cards), holes in it as a card is (`ground::hole`).
    pub fn holes(&self) -> &crate::elements::ground::Holes {
        &self.1
    }
}

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
    /// How opaque its ground is at each corner (its row's, its foot's):
    /// what the frame's curve beside it is told (`Window::paint_around`).
    under: Corners<f32>,
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
/// through, else under them all (whole, nothing shows between them); each
/// card outlined in `edge`, where the look has one.
pub fn frame(
    cards: &Cards,
    ground: Hsla,
    edge: Option<Hsla>,
    cut: bool,
    child: impl IntoElement,
) -> Frame {
    Frame {
        cards: cards.clone(),
        ground,
        edge,
        cut,
        child: child.into_any_element(),
    }
}

/// `child` as a card of the frame: its content kept within its corners,
/// and a hole in the frame where it is laid out; `under` is how opaque its
/// ground is at each corner, so the frame meets it there without a seam.
pub fn card(cards: &Cards, under: Corners<f32>, child: impl IntoElement) -> Card {
    Card {
        cards: cards.clone(),
        under,
        child: child.into_any_element(),
    }
}

pub struct Frame {
    cards: Cards,
    ground: Hsla,
    edge: Option<Hsla>,
    cut: bool,
    child: AnyElement,
}

pub struct Card {
    cards: Cards,
    under: Corners<f32>,
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
        // On whole device pixels, as the card and its band are.
        let bounds = window.snap_to_device_pixels(bounds);
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
        self.cards.1.clear();
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
        let holes = self.cards.1.laid();
        paint_ground(bounds, &placed, &holes, self.ground, self.cut, window);
        for card in &placed {
            paint_fillets(card, self.ground, self.cut, window);
        }
        if let Some(edge) = self.edge {
            for card in &placed {
                paint_outline(card, edge, bounds, window);
            }
        }
        self.child.paint(window, cx);
    }
}

/// The frame's ground over `bounds`. While it lets what is behind through
/// (`cut`), around the cards and the parts lying on it (`holes`): its
/// straight edges a path on whole device pixels, its curves each the
/// complement of a card's or a part's corner, told how opaque that is
/// (`Window::paint_around`), so the two meet without a seam. Whole, under
/// them all.
fn paint_ground(
    bounds: Bounds<Pixels>,
    cards: &[Placed],
    holes: &[crate::elements::ground::Laid],
    ground: Hsla,
    cut: bool,
    window: &mut Window,
) {
    if !cut || (cards.is_empty() && holes.is_empty()) {
        window.paint_quad(gpui::fill(bounds, ground));
        return;
    }
    let mut path = PathBuilder::fill().with_style(PathStyle::Fill(
        FillOptions::default().with_fill_rule(FillRule::EvenOdd),
    ));
    rounded_rect(&mut path, bounds, Corners::default());
    for card in cards {
        square_outline(&mut path, card);
    }
    for hole in holes {
        rounded_rect(&mut path, hole.bounds, Corners::default());
    }
    match path.build() {
        Ok(path) => window.paint_path(path, ground),
        // Nothing painted rather than a ground twice under the cards.
        Err(error) => tracing::warn!("cannot shape the window's frame: {error}"),
    }
    for card in cards {
        paint_card_corners(card, ground, window);
    }
    for hole in holes {
        crate::elements::ground::paint_corners_around(hole, ground, window);
    }
}

/// `rect` as a closed path, its corners rounded by `corners`, each held to
/// half its shorter side.
pub(crate) fn rounded_rect(
    path: &mut PathBuilder,
    bounds: Bounds<Pixels>,
    corners: Corners<Pixels>,
) {
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

/// A card's shape, its corners square, as a hole of the frame's path:
/// its rectangle, or with notches its band, what is under them, and the
/// square of each concave corner (`paint_fillets` paints those), on whole
/// device pixels. Clockwise.
fn square_outline(path: &mut PathBuilder, card: &Placed) {
    if !card.notched() {
        rounded_rect(path, card.bounds, Corners::default());
        return;
    }
    let (left, top) = (card.bounds.left(), card.bounds.top());
    let (right, bottom) = (card.bounds.right(), card.bounds.bottom());
    let r = card.radius;
    let [start, end] = card.notches;
    let start = start.map(|n| (n.bounds.right(), n.bounds.bottom()));
    let end = end.map(|n| (n.bounds.left(), n.bounds.bottom()));
    path.move_to(point(start.map_or(left, |(x, _)| x), top));
    match end {
        Some((x, low)) => {
            path.line_to(point(x, top));
            path.line_to(point(x, low - r));
            path.line_to(point(x + r, low - r));
            path.line_to(point(x + r, low));
            path.line_to(point(right, low));
        }
        None => path.line_to(point(right, top)),
    }
    path.line_to(point(right, bottom));
    path.line_to(point(left, bottom));
    match start {
        Some((x, low)) => {
            path.line_to(point(left, low));
            path.line_to(point(x - r, low));
            path.line_to(point(x - r, low - r));
            path.line_to(point(x, low - r));
        }
        None => path.line_to(point(left, top)),
    }
    path.close();
}

/// A card's outline (`Theme::card_edge`): a ring a device pixel wide on
/// the frame, just outside its shape, drawn within the frame's `bounds`
/// (the window's), so it is not where the card meets the window's edge;
/// what lies on the frame beside it (a notch's buttons) is drawn over it.
///
/// From quads, as it is drawn every frame of the look a window has by
/// default: a path would make each of them open gpui's path pass, a
/// multisampled target the size of the window.
fn paint_outline(card: &Placed, edge: Hsla, bounds: Bounds<Pixels>, window: &mut Window) {
    let scale = window.scale_factor();
    let pixel = px(1. / scale);
    window.with_content_mask(Some(ContentMask::new(bounds)), |window| {
        for stroke in strokes(&turns(card, scale)) {
            match stroke {
                Stroke::Run(run) => window.paint_quad(gpui::fill(logical(run, scale), edge)),
                Stroke::Arc { disc, quadrant } => {
                    let disc = logical(disc, scale);
                    let mask = pixels_of(logical(quadrant, scale), scale);
                    window.with_content_mask(Some(mask), |window| {
                        window.paint_quad(gpui::quad(
                            disc,
                            disc.size.width / 2.,
                            // The edge's own color, unseen: gpui mixes a
                            // border with its background unpremultiplied,
                            // and a black one darkens the arc's inner edge.
                            edge.opacity(0.),
                            pixel,
                            edge,
                            gpui::BorderStyle::Solid,
                        ));
                    });
                }
            }
        }
    });
}

/// A corner of a card's shape, in device pixels, and how far it is rounded.
type Turn = (Point<i32>, i32);

/// What a card's outline is drawn in, in device pixels: no two overlap, as
/// it may let what is behind the window through.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stroke {
    /// A straight stretch of it, a device pixel across.
    Run(Bounds<i32>),
    /// A quarter of a ring around a corner's center: the edge of `disc`, a
    /// device pixel deep, within `quadrant`.
    Arc {
        disc: Bounds<i32>,
        quadrant: Bounds<i32>,
    },
}

/// The corners of a card's shape, clockwise: its rectangle, or with notches
/// its band (rounded beside each notch), the band's edge down to the
/// concave fillet into the files, and what is under the notch, rounded at
/// its corner unless that is on the window's edge.
fn turns(card: &Placed, scale: f32) -> Vec<Turn> {
    let device = |length: Pixels| device_pixels(length, scale);
    let b = card.bounds;
    let (left, top) = (device(b.left()), device(b.top()));
    let (right, bottom) = (device(b.right()), device(b.bottom()));
    let (r, c) = (device(card.radius), card.corners);
    let [on_left, on_right] = card.on_edge;
    let [start, end] = card.notches;
    let under_notch = |on_edge: bool| if on_edge { 0 } else { r };
    let mut turns = Vec::with_capacity(8);
    match start {
        Some(notch) => turns.push((point(device(notch.bounds.right()), top), r)),
        None => turns.push((point(left, top), device(c.top_left))),
    }
    match end {
        Some(notch) => {
            let (x, low) = (device(notch.bounds.left()), device(notch.bounds.bottom()));
            turns.push((point(x, top), r));
            turns.push((point(x, low), r));
            turns.push((point(right, low), under_notch(on_right)));
        }
        None => turns.push((point(right, top), device(c.top_right))),
    }
    turns.push((point(right, bottom), device(c.bottom_right)));
    turns.push((point(left, bottom), device(c.bottom_left)));
    if let Some(notch) = start {
        let (x, low) = (device(notch.bounds.right()), device(notch.bounds.bottom()));
        turns.push((point(left, low), under_notch(on_left)));
        turns.push((point(x, low), r));
    }
    turns
}

/// The outline of the shape whose corners are `turns`, clockwise, axis by
/// axis: a device pixel outside each of its edges, and around each of its
/// rounded corners. Where the shape's edges meet square, the row of the
/// edge across takes the corner's pixel.
fn strokes(turns: &[Turn]) -> Vec<Stroke> {
    let count = turns.len();
    let at = |index: usize| turns[index % count].0;
    // One step along the edge from turn `index` to the next.
    let along = |index: usize| {
        let (from, to) = (at(index), at(index + 1));
        point((to.x - from.x).signum(), (to.y - from.y).signum())
    };
    // Clockwise on the screen, what is outside is to the left of the way.
    let outside = |way: Point<i32>| point(way.y, -way.x);
    // 1 where the shape turns right at turn `index` (a corner of it), -1
    // where it turns left (a corner into it).
    let bend = |index: usize| {
        let (coming, going) = (along(index + count - 1), along(index));
        coming.x * going.y - coming.y * going.x
    };
    let mut strokes = Vec::with_capacity(count * 2);
    for index in 0..count {
        let way = along(index);
        let out = outside(way);
        // How far the run stops short of a turn.
        let short = |index: usize| match (turns[index % count].1, bend(index % count)) {
            (0, 1) if way.y == 0 => -1,
            (0, -1) if way.y == 0 => 1,
            (radius, _) => radius,
        };
        let from = at(index) + point(way.x * short(index), way.y * short(index));
        let next = index + 1;
        let to = at(next) - point(way.x * short(next), way.y * short(next));
        if (to.x - from.x) * way.x + (to.y - from.y) * way.y > 0 {
            strokes.push(Stroke::Run(between(from, to + out)));
        }
        let radius = turns[index].1;
        if radius == 0 {
            continue;
        }
        let corner = out + outside(along(index + count - 1));
        let (center, outer, toward) = match bend(index) {
            1 => (at(index) - scaled(corner, radius), radius + 1, corner),
            -1 => (
                at(index) + scaled(corner, radius),
                radius,
                point(-corner.x, -corner.y),
            ),
            _ => continue,
        };
        strokes.push(Stroke::Arc {
            disc: between(center - point(outer, outer), center + point(outer, outer)),
            quadrant: between(center, center + scaled(toward, outer)),
        });
    }
    strokes
}

fn scaled(way: Point<i32>, by: i32) -> Point<i32> {
    point(way.x * by, way.y * by)
}

/// The rectangle with `a` and `b` at opposite corners.
fn between(a: Point<i32>, b: Point<i32>) -> Bounds<i32> {
    Bounds::from_corners(
        point(a.x.min(b.x), a.y.min(b.y)),
        point(a.x.max(b.x), a.y.max(b.y)),
    )
}

/// `length`, on whole device pixels, as a number of them.
#[allow(clippy::cast_possible_truncation)] // a window's length in device pixels
fn device_pixels(length: Pixels, scale: f32) -> i32 {
    (f32::from(length) * scale).round() as i32
}

/// A rectangle of device pixels in the window's own units.
#[allow(clippy::cast_precision_loss)] // a window's length in device pixels
fn logical(device: Bounds<i32>, scale: f32) -> Bounds<Pixels> {
    let at = |value: i32| px(value as f32 / scale);
    Bounds::from_corners(
        point(at(device.left()), at(device.top())),
        point(at(device.right()), at(device.bottom())),
    )
}

/// A mask of exactly the device pixels of `bounds`, which lies on whole
/// ones: gpui covers a mask to device pixels, floor and ceiling, so an edge
/// a rounding error past a whole pixel would keep one more. A quarter of a
/// pixel inside is not.
fn pixels_of(bounds: Bounds<Pixels>, scale: f32) -> ContentMask<Pixels> {
    let inside = px(0.25 / scale);
    ContentMask::new(Bounds::from_corners(
        point(bounds.left() + inside, bounds.top() + inside),
        point(bounds.right() - inside, bounds.bottom() - inside),
    ))
}

/// The frame's curves beside a card's rounded corners: in the square of
/// each, the complement of the rectangle the corner rounds (the card, its
/// band beside a notch, what is under a notch), told how opaque the card
/// is there.
fn paint_card_corners(card: &Placed, ground: Hsla, window: &mut Window) {
    use crate::elements::ground::{Toward, corner_square};
    let b = card.bounds;
    let (r, c, under) = (card.radius, card.corners, card.under);
    let [on_left, on_right] = card.on_edge;
    let [start, end] = card.notches;
    let around = |rect: Bounds<Pixels>,
                  corners: Corners<Pixels>,
                  corner: Toward,
                  opacity: f32,
                  window: &mut Window| {
        let radius = corner.of(corners);
        if radius > px(0.) {
            let square = corner_square(rect, corner, radius, window);
            window.paint_around(square, rect, corners, ground, opacity);
        }
    };
    if !card.notched() {
        around(b, c, Toward::TopLeft, under.top_left, window);
        around(b, c, Toward::TopRight, under.top_right, window);
        around(b, c, Toward::BottomRight, under.bottom_right, window);
        around(b, c, Toward::BottomLeft, under.bottom_left, window);
        return;
    }
    // The band, between the notches, over what is under them.
    let low = start
        .iter()
        .chain(end.iter())
        .map(|n| n.bounds.bottom())
        .fold(b.top(), Pixels::max);
    let band = Bounds::from_corners(
        point(start.map_or(b.left(), |n| n.bounds.right()), b.top()),
        point(end.map_or(b.right(), |n| n.bounds.left()), low),
    );
    let band_corners = Corners {
        top_left: if start.is_some() { r } else { c.top_left },
        top_right: if end.is_some() { r } else { c.top_right },
        ..Corners::default()
    };
    around(band, band_corners, Toward::TopLeft, under.top_left, window);
    around(
        band,
        band_corners,
        Toward::TopRight,
        under.top_right,
        window,
    );
    // Under the notches: its corner beside one is square on the window's
    // edge, round elsewhere.
    let body = Bounds::from_corners(point(b.left(), low), b.bottom_right());
    let under_notch = |notch: bool, on_edge: bool| if notch && !on_edge { r } else { px(0.) };
    let body_corners = Corners {
        top_left: under_notch(start.is_some(), on_left),
        top_right: under_notch(end.is_some(), on_right),
        ..c
    };
    around(
        body,
        body_corners,
        Toward::TopLeft,
        under.bottom_left,
        window,
    );
    around(
        body,
        body_corners,
        Toward::TopRight,
        under.bottom_right,
        window,
    );
    around(
        body,
        body_corners,
        Toward::BottomRight,
        under.bottom_right,
        window,
    );
    around(
        body,
        body_corners,
        Toward::BottomLeft,
        under.bottom_left,
        window,
    );
}

/// The fillets of a card's notches: in the square of each concave corner,
/// the band's ground around a disc of the frame, the band told how opaque
/// the frame is (nothing while it is whole and under it all), so the two
/// meet without a seam.
fn paint_fillets(card: &Placed, frame: Hsla, cut: bool, window: &mut Window) {
    let r = card.radius;
    for (index, notch) in card.notches.iter().enumerate() {
        let Some(notch) = notch else { continue };
        let low = notch.bounds.bottom();
        // At the start, the band's left edge, the fillet left of it; at
        // the end, its right edge, the fillet right of it.
        let (square_left, disc_left) = if index == 0 {
            let x = notch.bounds.right();
            (x - r, x - 2. * r)
        } else {
            let x = notch.bounds.left();
            (x, x)
        };
        let square = Bounds::new(point(square_left, low - r), size(r, r));
        let disc = Bounds::new(point(disc_left, low - 2. * r), size(2. * r, 2. * r));
        if cut {
            let mask = pixels_of(square, window.scale_factor());
            window.with_content_mask(Some(mask), |window| {
                window.paint_quad(gpui::fill(disc, frame).corner_radii(Corners::all(r)));
            });
        }
        let under = if cut { frame.a } else { 0. };
        window.paint_around(square, disc, Corners::all(r), notch.ground, under);
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
        // On whole device pixels, its radius too, as the quads it holds are
        // drawn: the frame's straight edges meet them exactly.
        let radius = crate::elements::ground::snap_length(
            gpui::rems(RADIUS_REMS).to_pixels(window.rem_size()),
            window,
        );
        let bounds = window.snap_to_device_pixels(bounds);
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
                under: self.under,
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
        let mask = mask(window.snap_to_device_pixels(bounds), *corners);
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
    use super::{Notch, Placed, Stroke, corners, strokes, turns};
    use gpui::{Bounds, Corners, Hsla, Pixels, Point, point, px, size};
    use std::collections::{HashMap, HashSet};

    /// A card at (x, y), every length in device pixels at `scale`.
    fn card(
        (x, y, width, height): (f32, f32, f32, f32),
        scale: f32,
        rounded: bool,
        notches: [Option<(f32, f32, f32, f32)>; 2],
    ) -> Placed {
        let bounds = |(x, y, width, height): (f32, f32, f32, f32)| {
            Bounds::new(
                point(px(x / scale), px(y / scale)),
                size(px(width / scale), px(height / scale)),
            )
        };
        let radius = px(12. / scale);
        Placed {
            bounds: bounds((x, y, width, height)),
            corners: Corners::all(if rounded { radius } else { px(0.) }),
            radius,
            on_edge: [!rounded; 2],
            notches: notches.map(|notch| {
                notch.map(|notch| Notch {
                    bounds: bounds(notch),
                    ground: Hsla::default(),
                })
            }),
            under: Corners::default(),
        }
    }

    /// The device pixels a stroke draws in, and how many strokes draw in
    /// each: an arc's whole quadrant counts, as that is what it is kept to.
    fn cells(strokes: &[Stroke]) -> HashMap<(i32, i32), u32> {
        let mut count = HashMap::new();
        for stroke in strokes {
            let area = match *stroke {
                Stroke::Run(run) => run,
                Stroke::Arc { quadrant, .. } => quadrant,
            };
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    *count.entry((x, y)).or_insert(0) += 1;
                }
            }
        }
        count
    }

    /// The pixels a stroke paints: a run's, and those of an arc its ring
    /// passes through.
    fn ring(strokes: &[Stroke]) -> HashSet<(i32, i32)> {
        let mut ring = HashSet::new();
        for stroke in strokes {
            match *stroke {
                Stroke::Run(run) => {
                    for y in run.top()..run.bottom() {
                        for x in run.left()..run.right() {
                            ring.insert((x, y));
                        }
                    }
                }
                Stroke::Arc { disc, quadrant } => {
                    #[allow(clippy::cast_precision_loss)] // a few pixels
                    let (cx, cy, outer) = (
                        (disc.left() + disc.right()) as f32 / 2.,
                        (disc.top() + disc.bottom()) as f32 / 2.,
                        disc.size.width as f32 / 2.,
                    );
                    for y in quadrant.top()..quadrant.bottom() {
                        for x in quadrant.left()..quadrant.right() {
                            #[allow(clippy::cast_precision_loss)] // a few pixels
                            let far = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
                            if far > outer - 1. && far < outer {
                                ring.insert((x, y));
                            }
                        }
                    }
                }
            }
        }
        ring
    }

    /// One closed line: each pixel has at least two neighbours, and from
    /// any of them all the others are reached.
    fn is_unbroken(ring: &HashSet<(i32, i32)>) -> bool {
        let neighbours = |(x, y): (i32, i32)| {
            (-1..=1)
                .flat_map(move |dy| (-1..=1).map(move |dx| (x + dx, y + dy)))
                .filter(move |&near| near != (x, y))
        };
        let Some(&first) = ring.iter().next() else {
            return false;
        };
        let mut seen = HashSet::from([first]);
        let mut waiting = vec![first];
        while let Some(pixel) = waiting.pop() {
            for near in neighbours(pixel).filter(|near| ring.contains(near)) {
                if seen.insert(near) {
                    waiting.push(near);
                }
            }
        }
        seen.len() == ring.len()
            && ring
                .iter()
                .all(|&pixel| neighbours(pixel).filter(|near| ring.contains(near)).count() >= 2)
    }

    const SCALES: [f32; 4] = [1., 1.25, 1.5, 2.];
    const END: (f32, f32, f32, f32) = (700., 30., 100., 40.);
    const START: (f32, f32, f32, f32) = (200., 30., 80., 40.);

    /// However a card is cut (square on the window's edges, a notch at
    /// either end or both), at every scale, its outline is one line, no
    /// two of its strokes on one pixel.
    #[test]
    fn a_card_s_outline_is_unbroken_and_drawn_once() {
        let shape = (200., 30., 600., 400.);
        for scale in SCALES {
            for rounded in [true, false] {
                for notches in [
                    [None, None],
                    [None, Some(END)],
                    [Some(START), None],
                    [Some(START), Some(END)],
                ] {
                    let card = card(shape, scale, rounded, notches);
                    let strokes = strokes(&turns(&card, scale));
                    assert!(
                        cells(&strokes).values().all(|&count| count == 1),
                        "strokes overlap at {scale}, rounded {rounded}, {notches:?}"
                    );
                    assert!(
                        is_unbroken(&ring(&strokes)),
                        "a broken ring at {scale}, rounded {rounded}, {notches:?}"
                    );
                }
            }
        }
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Bounds<i32> {
        Bounds::from_corners(Point::new(left, top), Point::new(right, bottom))
    }

    /// Outside the card by a device pixel, around a rounded corner a disc
    /// a pixel larger, and into a notch's fillet a disc its size.
    #[test]
    fn the_outline_lies_outside_the_shape_and_follows_a_notch() {
        let card = card((200., 30., 600., 400.), 1., true, [None, Some(END)]);
        let strokes = strokes(&turns(&card, 1.));
        // The band's top, then its rounded corner beside the notch.
        assert!(strokes.contains(&Stroke::Run(rect(212, 29, 688, 30))));
        assert!(strokes.contains(&Stroke::Arc {
            disc: rect(675, 29, 701, 55),
            quadrant: rect(688, 29, 701, 42),
        }));
        // Down the band's edge, then the concave fillet into the files,
        // whose disc is the fillet's own.
        assert!(strokes.contains(&Stroke::Run(rect(700, 42, 701, 58))));
        assert!(strokes.contains(&Stroke::Arc {
            disc: rect(700, 46, 724, 70),
            quadrant: rect(700, 58, 712, 70),
        }));
        // On along the files' top, to the corner under the notch.
        assert!(strokes.contains(&Stroke::Run(rect(712, 69, 788, 70))));
        assert!(strokes.contains(&Stroke::Arc {
            disc: rect(775, 69, 801, 95),
            quadrant: rect(788, 69, 801, 82),
        }));
    }

    /// Square on the window's edges, its top row takes the corner's pixel,
    /// and no arc is drawn.
    #[test]
    fn a_square_corner_is_one_pixel_drawn_once() {
        let card = card((0., 0., 100., 50.), 1., false, [None, None]);
        let strokes = strokes(&turns(&card, 1.));
        assert_eq!(strokes.len(), 4);
        assert!(strokes.contains(&Stroke::Run(rect(-1, -1, 101, 0))));
        assert!(strokes.contains(&Stroke::Run(rect(100, 0, 101, 50))));
        assert!(strokes.contains(&Stroke::Run(rect(-1, 50, 101, 51))));
        assert!(strokes.contains(&Stroke::Run(rect(-1, 0, 0, 50))));
    }

    /// A length at a scale that does not divide it comes back a whole
    /// number of device pixels, the same one.
    #[test]
    fn device_pixels_survive_the_round_trip() {
        for scale in SCALES {
            for device in 0..2000 {
                #[allow(clippy::cast_precision_loss)] // a window's length
                let logical: Pixels = px(device as f32 / scale);
                assert_eq!(super::device_pixels(logical, scale), device);
            }
        }
    }

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
