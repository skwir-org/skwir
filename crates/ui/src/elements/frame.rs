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
//! is not where the card meets the window's edge.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ContentMask, Corners, Element, ElementId, FillOptions, FillRule,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, PathBuilder, PathStyle,
    Pixels, Window, point, px, size,
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
/// the frame, between its shape grown by that pixel and its shape, drawn
/// within the frame's `bounds` (the window's), so it is not where the card
/// meets the window's edge; what lies on the frame beside it (a notch's
/// buttons) is drawn over it.
fn paint_outline(card: &Placed, edge: Hsla, bounds: Bounds<Pixels>, window: &mut Window) {
    let pixel = px(1. / window.scale_factor());
    let mut path = PathBuilder::fill().with_style(PathStyle::Fill(
        FillOptions::default().with_fill_rule(FillRule::EvenOdd),
    ));
    shape(&mut path, card, pixel);
    shape(&mut path, card, px(0.));
    match path.build() {
        Ok(path) => window.with_content_mask(Some(ContentMask::new(bounds)), |window| {
            window.paint_path(path, edge);
        }),
        Err(error) => tracing::warn!("cannot shape a card's outline: {error}"),
    }
}

/// A card's shape, its corners as it rounds them, grown by `grow` on every
/// side: its rounded rectangle, or with notches its band (rounded beside
/// each notch), the band's edge down to the concave fillet into the
/// files, and what is under the notch, rounded at its corner unless that is
/// on the window's edge. Clockwise.
fn shape(path: &mut PathBuilder, card: &Placed, grow: Pixels) {
    let grown = |radius: Pixels| {
        if radius > px(0.) {
            radius + grow
        } else {
            px(0.)
        }
    };
    if !card.notched() {
        rounded_rect(
            path,
            card.bounds.dilate(grow),
            card.corners.map(|c| grown(*c)),
        );
        return;
    }
    let b = card.bounds;
    let (r, c) = (card.radius, card.corners);
    let [on_left, on_right] = card.on_edge;
    let (left, top, right, bottom) = (
        b.left() - grow,
        b.top() - grow,
        b.right() + grow,
        b.bottom() + grow,
    );
    // A convex corner of `radius` (its center `radius` in from the
    // card's), grown; a concave one shrinks by as much.
    let convex = |path: &mut PathBuilder, radius: Pixels, to| {
        if radius > px(0.) {
            let radius = radius + grow;
            path.arc_to(point(radius, radius), px(0.), false, true, to);
        }
    };
    let concave = |path: &mut PathBuilder, to| {
        let radius = r - grow;
        path.arc_to(point(radius, radius), px(0.), false, false, to);
    };
    // How far in from the card's edge a corner of `radius` ends: out by
    // `grow` where it is square, at the grown shape's corner.
    let at = |radius: Pixels| if radius > px(0.) { radius } else { -grow };
    let [start, end] = card.notches;
    // The band's top, from beside the notch at its start (or the card's
    // corner) to beside the one at its end.
    match start {
        Some(notch) => path.move_to(point(notch.bounds.right() + r, top)),
        None => path.move_to(point(b.left() + at(c.top_left), top)),
    }
    if let Some(notch) = end {
        let (x, low) = (notch.bounds.left(), notch.bounds.bottom());
        path.line_to(point(x - r, top));
        convex(path, r, point(x + grow, b.top() + r));
        path.line_to(point(x + grow, low - r));
        concave(path, point(x + r, low - grow));
        let corner = if on_right { px(0.) } else { r };
        path.line_to(point(b.right() - at(corner), low - grow));
        convex(path, corner, point(right, low + corner));
    } else {
        path.line_to(point(b.right() - at(c.top_right), top));
        convex(path, c.top_right, point(right, b.top() + c.top_right));
    }
    path.line_to(point(right, b.bottom() - at(c.bottom_right)));
    convex(
        path,
        c.bottom_right,
        point(b.right() - c.bottom_right, bottom),
    );
    path.line_to(point(b.left() + at(c.bottom_left), bottom));
    convex(path, c.bottom_left, point(left, b.bottom() - c.bottom_left));
    if let Some(notch) = start {
        let (x, low) = (notch.bounds.right(), notch.bounds.bottom());
        let corner = if on_left { px(0.) } else { r };
        path.line_to(point(left, low + at(corner)));
        convex(path, corner, point(b.left() + corner, low - grow));
        path.line_to(point(x - r, low - grow));
        concave(path, point(x - grow, low - r));
        path.line_to(point(x - grow, b.top() + r));
        convex(path, r, point(x + r, top));
    } else {
        path.line_to(point(left, b.top() + at(c.top_left)));
        convex(path, c.top_left, point(b.left() + c.top_left, top));
    }
    path.close();
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
            window.with_content_mask(Some(ContentMask::new(square)), |window| {
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
