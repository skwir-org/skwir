//! Rows of a text read where it is shown (ADR-051): as many as there are,
//! millions included, scrolled by row. The view's place is the row at the
//! top and how far it is scrolled into it (`RowScroll`); only the rows on
//! screen are built and laid out, in `prepaint`, where the width is real,
//! wrapped to it or as wide as they are. Nothing is kept per row: a
//! `gpui::list` keeps an entry for each of its items.
//!
//! A row not read yet is drawn by the view as it likes (an empty row);
//! the rows laid out are told to `on_laid` after layout, for the view to
//! read what it lacks. The scrollbar counts rows (`scrollbar::Scrolls::Rows`).
//! Rows that do not wrap are given how far they are scrolled sideways, to
//! shift what scrolls and keep in place what does not (a gutter).

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, DispatchPhase, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, Pixels,
    ScrollWheelEvent, Size, Style, Window, point, px, relative, size,
};

/// Where a view of rows is, shared by the element, the scrollbar and the
/// view that moves it (a match found, a key).
#[derive(Clone, Debug, Default)]
pub struct RowScroll {
    /// The row at the top of the view.
    pub top: u64,
    /// How far the top row is scrolled up out of view.
    pub offset: f32,
    /// What the wheel or a key asked for and the next layout does: down
    /// when positive.
    pending: f32,
    /// A row to bring into view, a third down the view when it was not in it.
    reveal: Option<u64>,
    /// What to bring into view in it once drawn (a match found): where it
    /// is said to be (`Rows::seen`), in a row long enough to hide it.
    seek: bool,
    /// The last rows stay on the bottom, however tall they turn out once
    /// read, or however many more come (a log written to), until the view
    /// is moved up.
    follow: bool,
    /// How far the rows are scrolled sideways, when they do not wrap.
    x: f32,
    /// What the last layout found.
    widest: f32,
    viewport: (f32, f32),
    average: f32,
    count: u64,
    /// The rows wholly on screen, and whether the last is the last row.
    shown: Range<u64>,
    at_end: bool,
    /// The rows laid out, some maybe only in part.
    laid: Range<u64>,
}

/// The place of a view of rows.
pub type Scroll = Rc<RefCell<RowScroll>>;

/// Where something to bring into view was drawn (a match found), in the
/// window: its bounds, and the left of the text it is in as laid out,
/// scrolled sideways.
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub area: Bounds<Pixels>,
    pub text_left: Pixels,
}

/// Told where a row drew what `RowScroll::reveal_spot` brings into view,
/// by what drew it, once placed.
pub type Seen = Rc<std::cell::Cell<Option<Spot>>>;

impl RowScroll {
    /// Bring `row` into view at the next layout, unless it is wholly there.
    pub fn reveal(&mut self, row: u64) {
        self.reveal = Some(row);
        self.seek = false;
        self.follow = false;
    }

    /// Bring what is in `row` into view, where the row says it drew it
    /// (`Rows::seen`): the row, unless it is laid out already, then the
    /// spot, a third into the view along the way it was out of it.
    pub fn reveal_spot(&mut self, row: u64) {
        self.reveal(row);
        self.seek = true;
    }

    /// Scroll by `pixels`, down when positive, at the next layout.
    pub fn scroll_by(&mut self, pixels: f32) {
        self.pending += pixels;
        self.seek = false;
    }

    /// A row's height, as the last layout found them: what the arrows go.
    pub fn row(&self) -> f32 {
        self.average.max(1.)
    }

    /// A screen less a row, the way Page Down goes.
    pub fn page(&self) -> f32 {
        (self.viewport.1 - self.average).max(self.average)
    }

    /// To the first row, or past the last (the layout stops at the end).
    pub fn to_edge(&mut self, end: bool) {
        self.pending = 0.;
        self.seek = false;
        self.offset = 0.;
        self.top = if end { self.count } else { 0 };
        self.follow = end;
    }

    /// The rows wholly on screen at the last layout.
    pub fn shown(&self) -> Range<u64> {
        self.shown.clone()
    }

    /// The scrollbar's numbers along the rows: offset, how far it can go,
    /// and the viewport, counted at the average row's height.
    pub(crate) fn vertical(&self) -> (f32, f32, f32) {
        let average = self.average.max(1.);
        let max = (self.count as f32 * average - self.viewport.1).max(0.);
        let offset = if self.at_end {
            max
        } else {
            (self.top as f32 * average + self.offset).min(max)
        };
        (offset, max, self.viewport.1)
    }

    pub(crate) fn horizontal(&self) -> (f32, f32, f32) {
        let max = (self.widest - self.viewport.0).max(0.);
        (self.x.min(max), max, self.viewport.0)
    }

    /// The scrollbar put the view `to` pixels along the rows.
    pub(crate) fn scroll_vertically_to(&mut self, to: f32) {
        let average = self.average.max(1.);
        let (_, max, _) = self.vertical();
        self.pending = 0.;
        self.seek = false;
        if to >= max && max > 0. {
            return self.to_edge(true);
        }
        let top = ((to / average).floor().max(0.) as u64).min(self.count.saturating_sub(1));
        self.follow = false;
        self.top = top;
        self.offset = (to - top as f32 * average).clamp(0., average);
    }

    pub(crate) fn scroll_horizontally_to(&mut self, to: f32) {
        self.x = to.max(0.);
        self.seek = false;
    }
}

/// How far to move (sideways, down) to bring `spot` into the rows' view
/// `bounds`, scrolled `shift` sideways: a third into the view along an
/// axis it is out of, and not sideways when rows wrap. What a popup over
/// the rows hides (`covered`, a find field's) is out of view down to its
/// foot, where it is over the spot. `None` when it is in view. The text
/// starts at its left as laid out, unscrolled.
fn bring(
    spot: Spot,
    bounds: Bounds<Pixels>,
    shift: f32,
    wrap: bool,
    covered: Option<Bounds<Pixels>>,
) -> Option<(f32, f32)> {
    let area = spot.area;
    let left = f32::from(spot.text_left) + shift;
    let right = f32::from(bounds.right());
    let (from, to) = (f32::from(area.left()), f32::from(area.right()));
    let dx = if !wrap && (from < left || to > right) {
        from - (left + (right - left) / 3.)
    } else {
        0.
    };
    // Where the spot will be once moved sideways: under the popup or not.
    let moved = |x: Pixels| x - px(dx);
    let bottom = f32::from(bounds.bottom());
    let top = covered
        .filter(|hidden| {
            hidden.intersects(&bounds)
                && hidden.left() < moved(area.right())
                && hidden.right() > moved(area.left())
        })
        .map_or(f32::from(bounds.top()), |hidden| {
            f32::from(hidden.bottom().max(bounds.top())).min(bottom)
        });
    let dy = if f32::from(area.top()) < top || f32::from(area.bottom()) > bottom {
        f32::from(area.top()) - (top + (bottom - top) / 3.)
    } else {
        0.
    };
    (dx != 0. || dy != 0.).then_some((dx, dy))
}

/// `top` and `offset` moved by `pending` pixels (down when positive) over
/// rows of the heights `height` gives, `count` of them: never above the
/// first row, nor past the last's top.
fn settle(
    (mut top, mut offset): (u64, f32),
    mut pending: f32,
    count: u64,
    mut height: impl FnMut(u64) -> f32,
) -> (u64, f32) {
    if count == 0 {
        return (0, 0.);
    }
    top = top.min(count - 1);
    while pending > 0. {
        let h = height(top);
        if offset + pending < h || top + 1 >= count {
            offset = (offset + pending).min(h.max(0.));
            break;
        }
        pending -= h - offset;
        offset = 0.;
        top += 1;
    }
    while pending < 0. {
        if offset + pending >= 0. {
            offset += pending;
            break;
        }
        pending += offset;
        offset = 0.;
        if top == 0 {
            break;
        }
        top -= 1;
        offset = height(top);
    }
    (top, offset)
}

/// The place that shows the last rows at the bottom of `viewport`, if
/// `(top, offset)` leaves room under them: rows `top..count` measure
/// `below` from the top row's top. Unchanged when they fill the view.
fn fill(
    (top, offset): (u64, f32),
    below: f32,
    viewport: f32,
    count: u64,
    height: impl FnMut(u64) -> f32,
) -> (u64, f32) {
    let room = viewport - (below - offset);
    if room <= 0. {
        return (top, offset);
    }
    settle((top, offset), -room, count, height)
}

pub struct Rows {
    id: ElementId,
    scroll: Scroll,
    count: u64,
    wrap: bool,
    row: Rc<dyn Fn(u64, f32, &mut Window, &mut App) -> AnyElement>,
    on_laid: Option<Rc<dyn Fn(Range<u64>, &mut Window, &mut App)>>,
    seen: Option<Seen>,
    covered: Option<Covered>,
}

/// Where a popup over the rows was last drawn (`preview_bar::Field::drawn`).
pub type Covered = Rc<std::cell::Cell<Option<Bounds<Pixels>>>>;

/// `count` rows, each built by `row` (given how far rows are scrolled
/// sideways), scrolled by `scroll`. They wrap to the width when `wrap`,
/// else scroll sideways.
pub fn rows(
    id: impl Into<ElementId>,
    scroll: Scroll,
    count: u64,
    wrap: bool,
    row: impl Fn(u64, f32, &mut Window, &mut App) -> AnyElement + 'static,
) -> Rows {
    Rows {
        id: id.into(),
        scroll,
        count,
        wrap,
        row: Rc::new(row),
        on_laid: None,
        seen: None,
        covered: None,
    }
}

impl Rows {
    /// Told the rows laid out, after each layout.
    pub fn on_laid(mut self, laid: impl Fn(Range<u64>, &mut Window, &mut App) + 'static) -> Self {
        self.on_laid = Some(Rc::new(laid));
        self
    }

    /// Where the rows tell they drew what `RowScroll::reveal_spot` brings
    /// into view, as they are placed.
    pub fn seen(mut self, seen: Seen) -> Self {
        self.seen = Some(seen);
        self
    }

    /// What a popup over the rows hides of them, as it was last drawn:
    /// what is brought into view is brought out from under it.
    pub fn covered(mut self, covered: Option<Covered>) -> Self {
        self.covered = covered;
        self
    }
}

impl IntoElement for Rows {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct Prepaint {
    rows: Vec<AnyElement>,
    hitbox: Hitbox,
}

impl Element for Rows {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
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
        style.size.height = relative(1.).into();
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
    ) -> Prepaint {
        let count = self.count;
        let space = size(
            if self.wrap {
                AvailableSpace::Definite(bounds.size.width)
            } else {
                AvailableSpace::MaxContent
            },
            AvailableSpace::MinContent,
        );
        let viewport = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        // Sideways, as far as the widest row seen allows.
        let shift = {
            let mut scroll = self.scroll.borrow_mut();
            scroll.x = if self.wrap {
                0.
            } else {
                scroll.x.clamp(0., (scroll.widest - viewport.0).max(0.))
            };
            scroll.x
        };
        let row = Rc::clone(&self.row);
        let measure = |i: u64, window: &mut Window, cx: &mut App| -> (AnyElement, Size<Pixels>) {
            let mut element = row(i, shift, window, cx);
            let size = element.layout_as_root(space, window, cx);
            (element, size)
        };
        let mut scroll = self.scroll.borrow_mut();
        scroll.viewport = viewport;
        scroll.count = count;
        // A row asked for: left alone when wholly on screen, or laid out
        // when what is in it is brought into view after, else a third down
        // the view, what comes before it above.
        let around = if scroll.seek {
            scroll.laid.clone()
        } else {
            scroll.shown.clone()
        };
        if let Some(row) = scroll.reveal.take().filter(|row| *row < count)
            && !around.contains(&row)
        {
            scroll.top = row;
            scroll.offset = 0.;
            scroll.pending = -viewport.1 / 3.;
        }
        let mut pending = std::mem::take(&mut scroll.pending);
        // Following the end: from the last row, the layout fills the view
        // above it; moving up lets go.
        if scroll.follow {
            if pending < 0. {
                scroll.follow = false;
                scroll.top = count;
                scroll.offset = 0.;
            } else {
                (scroll.top, scroll.offset, pending) = (count, 0., 0.);
            }
        }
        let (mut top, mut offset) = settle((scroll.top, scroll.offset), pending, count, |i| {
            f32::from(measure(i, window, cx).1.height)
        });
        let mut laid: Vec<(AnyElement, f32, Size<Pixels>)> = Vec::new();
        for pass in 0..2 {
            laid.clear();
            let mut y = -offset;
            let mut i = top;
            while i < count && y < viewport.1 {
                let (element, size) = measure(i, window, cx);
                laid.push((element, y, size));
                y += f32::from(size.height);
                i += 1;
            }
            // Past the end: the last rows go to the bottom, once.
            let below = y + offset;
            let moved = if pass == 0 && i >= count {
                fill((top, offset), below, viewport.1, count, |i| {
                    f32::from(measure(i, window, cx).1.height)
                })
            } else {
                (top, offset)
            };
            if moved == (top, offset) {
                break;
            }
            (top, offset) = moved;
        }
        let heights: f32 = laid.iter().map(|(_, _, size)| f32::from(size.height)).sum();
        if !laid.is_empty() {
            scroll.average = heights / laid.len() as f32;
        }
        let bottom = laid
            .last()
            .map_or(0., |(_, y, size)| y + f32::from(size.height));
        scroll.at_end = top + laid.len() as u64 >= count && bottom <= viewport.1 + 0.5;
        let whole = |(_, y, size): &&(AnyElement, f32, Size<Pixels>)| {
            *y >= -0.5 && y + f32::from(size.height) <= viewport.1 + 0.5
        };
        let first = laid.iter().position(|row| whole(&row)).unwrap_or(0) as u64;
        let wholly = laid.iter().filter(whole).count() as u64;
        scroll.shown = top + first..top + first + wholly;
        scroll.top = top;
        scroll.offset = offset;
        if self.wrap {
            scroll.x = 0.;
            scroll.widest = viewport.0;
        } else {
            let widest = laid
                .iter()
                .map(|(_, _, size)| f32::from(size.width))
                .fold(viewport.0, f32::max);
            scroll.widest = widest;
            scroll.x = scroll.x.clamp(0., (widest - viewport.0).max(0.));
        }
        let range = top..top + laid.len() as u64;
        scroll.laid = range.clone();
        drop(scroll);
        if let Some(seen) = &self.seen {
            seen.set(None);
        }
        let mut elements = Vec::with_capacity(laid.len());
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (mut element, y, _) in laid {
                element.prepaint_at(point(bounds.origin.x, bounds.origin.y + px(y)), window, cx);
                elements.push(element);
            }
        });
        // What was to be brought into view was drawn: where it is shows
        // how far to go, in the next frame.
        let mut scroll = self.scroll.borrow_mut();
        if scroll.seek
            && let Some(spot) = self.seen.as_ref().and_then(|seen| seen.get())
        {
            scroll.seek = false;
            let covered = self.covered.as_ref().and_then(|covered| covered.get());
            if let Some((dx, dy)) = bring(spot, bounds, shift, self.wrap, covered) {
                scroll.x = (shift + dx).max(0.);
                scroll.pending += dy;
                window.on_next_frame(|window, _| window.refresh());
            }
        }
        drop(scroll);
        if let Some(on_laid) = self.on_laid.clone() {
            window.defer(cx, move |window, cx| on_laid(range, window, cx));
        }
        Prepaint {
            rows: elements,
            hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for row in &mut prepaint.rows {
                row.paint(window, cx);
            }
        });
        let scroll = Rc::clone(&self.scroll);
        let hitbox = prepaint.hitbox.id;
        let wrap = self.wrap;
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, _| {
            // Ctrl and the wheel size the text: someone else's.
            if phase != DispatchPhase::Bubble
                || !hitbox.should_handle_scroll(window)
                || event.modifiers.secondary()
            {
                return;
            }
            let mut scroll = scroll.borrow_mut();
            let line = px(scroll.average.max(1.));
            let delta = event.delta.pixel_delta(line);
            // Each axis of the wheel moves its own way: Shift and the wheel
            // arrive sideways from every platform. Rows that wrap leave it
            // to what scrolls around them (bytes wider than the panel).
            scroll.pending -= f32::from(delta.y);
            scroll.seek = false;
            if !wrap {
                scroll.x = (scroll.x - f32::from(delta.x)).max(0.);
            }
            drop(scroll);
            window.refresh();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows 20 high, but every tenth (a wrapped line) 60.
    fn height(row: u64) -> f32 {
        if row % 10 == 9 { 60. } else { 20. }
    }

    #[test]
    fn the_wheel_moves_row_by_row_and_stops_at_either_end() {
        assert_eq!(settle((0, 0.), 30., 100, height), (1, 10.));
        assert_eq!(settle((1, 10.), -30., 100, height), (0, 0.));
        assert_eq!(settle((0, 5.), -50., 100, height), (0, 0.), "the first row");
        // Over a tall row: into it, then past it.
        assert_eq!(settle((9, 0.), 40., 100, height), (9, 40.));
        assert_eq!(settle((9, 40.), 30., 100, height), (10, 10.));
        assert_eq!(settle((10, 10.), -30., 100, height), (9, 40.));
        // Never past the last row's top, however far asked.
        assert_eq!(settle((95, 0.), 1e9, 100, height), (99, 60.));
        assert_eq!(settle((5, 0.), 100., 0, height), (0, 0.), "no rows");
    }

    #[test]
    fn the_last_rows_rest_on_the_bottom() {
        // Rows 97 to 99 measure 20 + 20 + 60 = 100 under a view of 200:
        // five rows of 20 come down above them.
        assert_eq!(fill((97, 0.), 100., 200., 100, height), (92, 0.));
        // Over a tall row, into it.
        assert_eq!(fill((90, 0.), 160., 200., 100, height), (89, 20.));
        // They fill it: nothing moves.
        assert_eq!(fill((90, 0.), 220., 200., 100, height), (90, 0.));
        // Fewer rows than the view holds: from the first.
        assert_eq!(fill((2, 0.), 100., 400., 5, height), (0, 0.));
    }

    #[test]
    fn the_end_is_followed_until_the_view_moves_up() {
        let mut scroll = RowScroll {
            count: 100,
            ..RowScroll::default()
        };
        scroll.to_edge(true);
        assert!(scroll.follow);
        scroll.scroll_vertically_to(0.);
        assert!(!scroll.follow, "the scrollbar took it elsewhere");
        scroll.to_edge(true);
        scroll.reveal(40);
        assert!(!scroll.follow, "a row was asked for");
    }

    #[test]
    fn a_spot_out_of_view_comes_a_third_into_it() {
        // Rows from x 100 to 700 and y 50 to 350, their text from 180 (a
        // gutter before it).
        let bounds = Bounds::new(point(px(100.), px(50.)), size(px(600.), px(300.)));
        let spot = |x: f32, y: f32, text_left: f32| Spot {
            area: Bounds::new(point(px(x), px(y)), size(px(40.), px(18.))),
            text_left: px(text_left),
        };
        let third = 180. + 520. / 3.;
        assert_eq!(bring(spot(400., 100., 180.), bounds, 0., false, None), None);
        // Past the right edge of rows that do not wrap: along them.
        assert_eq!(
            bring(spot(1000., 100., 180.), bounds, 0., false, None),
            Some((1000. - third, 0.))
        );
        // Scrolled 500 along, the text laid out from -320: one under the
        // gutter comes back.
        assert_eq!(
            bring(spot(150., 100., -320.), bounds, 500., false, None),
            Some((150. - third, 0.))
        );
        // Rows that wrap never go sideways; below the view, down.
        assert_eq!(
            bring(spot(400., 400., 180.), bounds, 0., true, None),
            Some((0., 400. - 150.))
        );
        assert_eq!(bring(spot(1000., 100., 180.), bounds, 0., true, None), None);
        // A find field hangs over the top of the view, from x 400 to 680
        // and down to y 140: a match under it comes a third into what it
        // leaves, one beside it stays.
        let field = Some(Bounds::new(
            point(px(400.), px(10.)),
            size(px(280.), px(130.)),
        ));
        assert_eq!(
            bring(spot(500., 100., 180.), bounds, 0., false, field),
            Some((0., 100. - (140. + 210. / 3.)))
        );
        assert_eq!(
            bring(spot(200., 100., 180.), bounds, 0., false, field),
            None
        );
        // Moved sideways out from under it, it is not under it.
        assert_eq!(
            bring(spot(1000., 100., 180.), bounds, 0., false, field),
            Some((1000. - third, 0.))
        );
    }

    #[test]
    fn the_scrollbar_counts_rows_and_reaches_the_end() {
        let mut scroll = RowScroll {
            count: 1000,
            average: 20.,
            viewport: (300., 200.),
            ..RowScroll::default()
        };
        assert_eq!(scroll.vertical(), (0., 19_800., 200.));
        scroll.scroll_vertically_to(9_910.);
        assert_eq!((scroll.top, scroll.offset), (495, 10.));
        scroll.scroll_vertically_to(19_800.);
        assert_eq!(scroll.top, 1000, "past the end: the layout settles it");
        scroll.at_end = true;
        assert!((scroll.vertical().0 - 19_800.).abs() < 0.5, "at the end");
    }
}
