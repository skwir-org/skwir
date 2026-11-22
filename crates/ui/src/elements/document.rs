//! Document pixels and hit testing share the transform calculated in prepaint.
//! No measured bounds escape into render; normalized text/link boxes remain
//! aligned when the panel is resized or the image is zoomed.

use std::{rc::Rc, sync::Arc};

use gpui::{
    App, Bounds, ContentMask, Corners, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, RenderImage, Style, Window, fill, point, px, relative,
    size,
};
use skwir_core::document::{PageContent, Part, Rect};

type Pointer = Rc<dyn Fn(&str, f32, f32, &mut Window, &mut App)>;

/// What pages cover on screen, reported by their elements in prepaint: the
/// most device pixels any covers on its longer side, and the scale it is
/// shown at (its size on screen over its own). The reader renders pages at
/// that side (`Shell::document_shown`) and zooms from that scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub covered: f32,
    pub scale: f32,
}
pub type Report = Rc<std::cell::Cell<Option<Shown>>>;

/// What of a page shows, reported by its element in prepaint (ADR-147):
/// the reader draws the part that shows of a page covering more than its
/// pixels drawn whole hold (`Shell::document_parts`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    pub page: u32,
    /// Its own size, in CSS pixels.
    pub own: (f32, f32),
    /// The device pixels its longer side covers.
    pub covered: f32,
    /// The longer side of its pixels drawn whole.
    pub held: u32,
    /// What of it shows, in shares of it.
    pub shown: Part,
}
pub type Visible = Rc<std::cell::RefCell<Vec<Seen>>>;
pub struct DocumentElement {
    pub image: Arc<RenderImage>,
    pub page: Arc<Page>,
    pub highlights: Vec<Rect>,
    pub selection: Vec<Rect>,
    /// The theme's `page_match` and `page_selection`, drawn over the page.
    pub colors: (Hsla, Hsla),
    pub zoom: Option<f32>,
    pub fit_width: bool,
    pub on_pointer: Pointer,
    pub report: Option<Report>,
    /// What of the page shows, said in prepaint (`Seen`).
    pub visible: Option<Visible>,
    /// What shows of it drawn sharper than its whole holds (ADR-147),
    /// drawn over it where it lies.
    pub part: Option<(Arc<RenderImage>, Part)>,
    /// The press holding the pages, shared with the layer under them that
    /// moves them (`elements::pan`, ADR-143).
    pub grab: crate::elements::pan::Grab,
    /// A left drag across its text moves the pages rather than selecting it
    /// (`reader::drags`). Wherever a press selects nothing
    /// (`Page::selects_at`),
    /// the page holds the grab itself, and a release that did not move the
    /// pages is a click.
    pub drags: bool,
}
impl IntoElement for DocumentElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

pub struct Prepaint {
    image: Bounds<Pixels>,
    hitbox: Hitbox,
}

/// A page as the reader shows it: what its engine said of it, which it
/// derefs to, and its text in lines, found once as it arrives, on the
/// worker that read it ([`Page::new`]). What a press or the pointer asks
/// of it ([`Page::selects_at`], [`Page::nearest`], the cursor) is a search
/// among its lines, never a walk of its characters, of which a page holds
/// up to `document::MAX_PAGE_CHARS`.
#[derive(Debug)]
pub struct Page {
    content: Arc<PageContent>,
    lines: Lines,
}

impl Page {
    pub fn new(content: Arc<PageContent>) -> Self {
        let lines = Lines::of(&content.chars);
        Self { content, lines }
    }

    /// The character a left press at `(x, y)` (its share of the page's
    /// width and height) begins a selection on: anywhere within a line of
    /// its text (on a letter, between words, a little before its first
    /// letter and after its last, in the leading between it and the next
    /// line of its paragraph), the line's nearest, unless a drag across the
    /// text moves the pages (`drags`). Anywhere else (a margin, between
    /// paragraphs, a picture, a page with no text) the press selects
    /// nothing, so it holds the pages, as one beside the page does
    /// (ADR-143). The page's element and `Shell::reader_pointer` both ask
    /// it, at the press and for the cursor over the page.
    pub fn selects_at(&self, drags: bool, x: f32, y: f32) -> Option<usize> {
        if drags {
            return None;
        }
        self.lines.within(&self.content.chars, x, y)
    }

    /// The character nearest `(x, y)`, wherever it is: where a selection's
    /// drag ends.
    pub fn nearest(&self, x: f32, y: f32) -> Option<usize> {
        self.lines.nearest(&self.content.chars, x, y)
    }
}

impl std::ops::Deref for Page {
    type Target = PageContent;
    fn deref(&self) -> &PageContent {
        &self.content
    }
}

/// How far into the room between two lines of a paragraph a press still
/// selects, as a share of the lower line's height: a gap up to this is the
/// paragraph's leading, shared between its lines at its middle; a wider
/// one (between paragraphs) moves the pages.
const LEADING: f32 = 0.6;

/// How far before a line's first letter and after its last a press still
/// selects, as a share of its height.
const BESIDE: f32 = 0.5;

/// A page's lines of text, by the top of where a press selects from each
/// (its band), with the lowest any band reaches up to each: a search for a
/// point looks at the lines that start above it, going up the page, only
/// while one of them may reach down to it.
#[derive(Debug, Default)]
struct Lines {
    lines: Vec<Line>,
    reach: Vec<f32>,
}

#[derive(Debug)]
struct Line {
    /// The box around its characters.
    bounds: Span,
    /// Where a press selects from it: its box, `BESIDE` before and after
    /// it, and halfway into the leading to the lines of its paragraph.
    band: Span,
    /// Its characters by where they start across, by their index in the
    /// page's.
    chars: Vec<(f32, u32)>,
    /// The widest of them: how far back a search across it looks.
    widest: f32,
}

/// A box by its edges, in shares of the page.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Span {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Span {
    /// A character's box, where it has one: a space an engine makes up has
    /// none, or no height.
    fn of(rect: Rect) -> Option<Span> {
        let span = Span {
            left: rect.x.min(rect.x + rect.width),
            top: rect.y.min(rect.y + rect.height),
            right: rect.x.max(rect.x + rect.width),
            bottom: rect.y.max(rect.y + rect.height),
        };
        let finite = [span.left, span.top, span.right, span.bottom]
            .iter()
            .all(|edge| edge.is_finite());
        (finite && span.bottom > span.top).then_some(span)
    }

    fn height(self) -> f32 {
        self.bottom - self.top
    }

    fn union(self, other: Span) -> Span {
        Span {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }

    /// The square of the way from `(x, y)` to it: none within it.
    fn distance(self, x: f32, y: f32) -> f32 {
        let dx = x - x.clamp(self.left, self.right);
        let dy = y - y.clamp(self.top, self.bottom);
        dx * dx + dy * dy
    }

    /// Whether `next`, read after this character, goes on its line: they
    /// share more than half the height of the lower one (a superscript, a
    /// full stop share less of a letter's but enough).
    fn beside(self, next: Span) -> bool {
        let shared = self.bottom.min(next.bottom) - self.top.max(next.top);
        shared > 0.5 * self.height().min(next.height())
    }
}

/// The nearer of two answers, by their distance, then the first character.
fn nearer(best: Option<(f32, u32)>, other: Option<(f32, u32)>) -> Option<(f32, u32)> {
    match (best, other) {
        (Some(a), Some(b)) => Some(if b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)).is_lt() {
            b
        } else {
            a
        }),
        (a, b) => a.or(b),
    }
}

impl Line {
    /// Its character nearest `(x, y)`, and the square of the way to it.
    fn nearest(
        &self,
        chars: &[skwir_core::document::TextChar],
        x: f32,
        y: f32,
    ) -> Option<(f32, u32)> {
        let to = |index: u32| {
            let span = Span::of(chars[index as usize].bounds)?;
            Some((span.distance(x, y), index))
        };
        let ahead = self.chars.partition_point(|(left, _)| *left <= x);
        let mut best = None;
        // Back across: one that starts further back than the widest goes
        // past `x` is no nearer than the best found.
        for &(left, index) in self.chars[..ahead].iter().rev() {
            if let Some((found, _)) = best {
                let short = x - (left + self.widest);
                if short > 0. && short * short > found {
                    break;
                }
            }
            best = nearer(best, to(index));
        }
        for &(left, index) in &self.chars[ahead..] {
            if let Some((found, _)) = best {
                let short = left - x;
                if short * short > found {
                    break;
                }
            }
            best = nearer(best, to(index));
        }
        best
    }
}

impl Lines {
    /// The lines of `chars`, read in their order: a character goes on the
    /// line of the one before it where it lies beside it, and a line break
    /// the engine reads ends one wherever its box is.
    fn of(chars: &[skwir_core::document::TextChar]) -> Self {
        let mut lines: Vec<Line> = Vec::new();
        // The last character's box, while its line is open.
        let mut open: Option<Span> = None;
        for (index, ch) in chars.iter().enumerate() {
            let Ok(index) = u32::try_from(index) else {
                break;
            };
            if let Some(span) = Span::of(ch.bounds) {
                let width = span.right - span.left;
                match lines
                    .last_mut()
                    .filter(|_| open.is_some_and(|last| last.beside(span)))
                {
                    Some(line) => {
                        line.bounds = line.bounds.union(span);
                        line.chars.push((span.left, index));
                        line.widest = line.widest.max(width);
                    }
                    None => lines.push(Line {
                        bounds: span,
                        band: span,
                        chars: vec![(span.left, index)],
                        widest: width,
                    }),
                }
                open = Some(span);
            }
            if ch.text.contains(['\n', '\r']) {
                open = None;
            }
        }
        for line in &mut lines {
            let beside = line.bounds.height() * BESIDE;
            line.band = Span {
                left: line.bounds.left - beside,
                right: line.bounds.right + beside,
                ..line.bounds
            };
            line.chars.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        // A line and the next one read, below it and across from it, with
        // no more than a leading between them: each takes half of it.
        for at in 1..lines.len() {
            let (above, below) = (lines[at - 1].bounds, lines[at].bounds);
            let gap = below.top - above.bottom;
            let across = above.left.max(below.left) < above.right.min(below.right);
            if across && gap > 0. && gap <= LEADING * above.height().min(below.height()) {
                lines[at - 1].band.bottom = above.bottom + gap / 2.;
                lines[at].band.top = below.top - gap / 2.;
            }
        }
        lines.sort_by(|a, b| a.band.top.total_cmp(&b.band.top));
        let reach = lines
            .iter()
            .scan(f32::NEG_INFINITY, |lowest, line| {
                *lowest = lowest.max(line.band.bottom);
                Some(*lowest)
            })
            .collect();
        Self { lines, reach }
    }

    /// The character a press at `(x, y)` selects from: the nearest of the
    /// line whose band holds it, the nearest line's where bands meet.
    fn within(&self, chars: &[skwir_core::document::TextChar], x: f32, y: f32) -> Option<usize> {
        let starts = self.lines.partition_point(|line| line.band.top <= y);
        let mut best = None;
        for at in (0..starts).rev() {
            if self.reach[at] < y {
                break;
            }
            let line = &self.lines[at];
            if line.band.contains(x, y) {
                best = nearer(best, line.nearest(chars, x, y));
            }
        }
        best.map(|(_, index)| index as usize)
    }

    /// The character nearest `(x, y)` on the whole page: looked for up the
    /// page from it, then down, each way only while a line may be nearer
    /// than the best found.
    fn nearest(&self, chars: &[skwir_core::document::TextChar], x: f32, y: f32) -> Option<usize> {
        let starts = self.lines.partition_point(|line| line.band.top <= y);
        let mut best: Option<(f32, u32)> = None;
        let look = |line: &Line, best: &mut Option<(f32, u32)>| {
            if best.is_none_or(|(found, _)| line.bounds.distance(x, y) <= found) {
                *best = nearer(*best, line.nearest(chars, x, y));
            }
        };
        for at in (0..starts).rev() {
            if let Some((found, _)) = best {
                let above = y - self.reach[at];
                if above > 0. && above * above > found {
                    break;
                }
            }
            look(&self.lines[at], &mut best);
        }
        for line in &self.lines[starts..] {
            if let Some((found, _)) = best {
                let below = line.band.top - y;
                if below * below > found {
                    break;
                }
            }
            look(line, &mut best);
        }
        best.map(|(_, index)| index as usize)
    }
}

/// The cursor a page drawn at `image` asks for with the pointer at `at`.
/// While a press holds the pages, none: the hand closed on them is the
/// layer's, over a link or text too; while one selects, the I-beam,
/// wherever it goes. Else a link's pointing hand, the I-beam where a press
/// selects, and elsewhere the open hand where the pages can move. A page
/// not under the pointer asks for nothing, and looks for nothing under it.
/// Asked at paint and at each move of the pointer, which draws again only
/// where the answer changes (gpui sets a cursor only after a draw, or as
/// the pointer goes onto another hitbox).
fn cursor_at(
    grab: &crate::elements::pan::Grab,
    page: &Page,
    drags: bool,
    image: Bounds<Pixels>,
    at: gpui::Point<Pixels>,
) -> Option<gpui::CursorStyle> {
    if grab.grabbing() {
        return None;
    }
    if grab.selecting() {
        return Some(gpui::CursorStyle::IBeam);
    }
    if !image.contains(&at) {
        return None;
    }
    let (x, y) = share(image, at);
    if page.links.iter().any(|link| link.bounds.contains(x, y)) {
        Some(gpui::CursorStyle::PointingHand)
    } else if page.selects_at(drags, x, y).is_some() {
        Some(gpui::CursorStyle::IBeam)
    } else {
        grab.movable().then_some(gpui::CursorStyle::OpenHand)
    }
}

/// Where `at` is on a page drawn at `image`: its share of the page's width
/// and height.
fn share(image: Bounds<Pixels>, at: gpui::Point<Pixels>) -> (f32, f32) {
    let x = f32::from(at.x - image.origin.x) / f32::from(image.size.width).max(1.);
    let y = f32::from(at.y - image.origin.y) / f32::from(image.size.height).max(1.);
    (x, y)
}

pub fn fitted_rect(
    width: f32,
    height: f32,
    page_width: f32,
    page_height: f32,
) -> (f32, f32, f32, f32) {
    let scale = (width / page_width.max(1.))
        .min(height / page_height.max(1.))
        .max(0.);
    let (w, h) = (page_width * scale, page_height * scale);
    ((width - w) / 2., (height - h) / 2., w, h)
}

impl Element for DocumentElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;
    fn id(&self) -> Option<ElementId> {
        Some(("document-page", self.page.page as usize).into())
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
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
        if let Some(zoom) = self.zoom {
            style.size.width = px(self.page.width * zoom).into();
            style.size.height = px(self.page.height * zoom).into();
            style.flex_shrink = 0.;
        } else if self.fit_width {
            style.size.width = relative(1.).into();
            style.aspect_ratio = Some(self.page.width / self.page.height.max(1.));
            style.flex_shrink = 0.;
        } else {
            style.size.width = relative(1.).into();
            style.size.height = relative(1.).into();
        }
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Prepaint {
        let (x, y, w, h) = fitted_rect(
            bounds.size.width.into(),
            bounds.size.height.into(),
            self.page.width,
            self.page.height,
        );
        let image = Bounds::new(bounds.origin + point(px(x), px(y)), size(px(w), px(h)));
        if let Some(report) = &self.report {
            let shown = Shown {
                covered: w.max(h) * window.scale_factor(),
                scale: w / self.page.width.max(1.),
            };
            if report.get().is_none_or(|seen| seen.covered < shown.covered) {
                report.set(Some(shown));
            }
        }
        if let Some(visible) = &self.visible {
            let view = window.content_mask().bounds.intersect(&image);
            if view.size.width > px(0.) && view.size.height > px(0.) {
                let size = self.image.size(0);
                let (left, top) = share(image, view.origin);
                let (right, bottom) = share(image, view.bottom_right());
                visible.borrow_mut().push(Seen {
                    page: self.page.page,
                    own: (self.page.width, self.page.height),
                    covered: w.max(h) * window.scale_factor(),
                    held: u32::try_from(size.width.0.max(size.height.0)).unwrap_or(0),
                    shown: Part {
                        left: left.clamp(0., 1.),
                        top: top.clamp(0., 1.),
                        right: right.clamp(0., 1.),
                        bottom: bottom.clamp(0., 1.),
                    },
                });
            }
        }
        // The page alone, not the room around it a page at a time: a press
        // there is the layer's under it, and its hand shows there.
        let hitbox = window.insert_hitbox(image, HitboxBehavior::Normal);
        Prepaint { image, hitbox }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        _: &mut App,
    ) {
        let image_bounds = prepaint.image;
        let cursor = cursor_at(
            &self.grab,
            &self.page,
            self.drags,
            image_bounds,
            window.mouse_position(),
        );
        if let Some(cursor) = cursor {
            window.set_cursor_style(cursor, &prepaint.hitbox);
        }
        window.with_content_mask(Some(ContentMask::new(bounds)), |window| {
            let _ = window.paint_image(
                image_bounds,
                image_bounds,
                Corners::default(),
                self.image.clone(),
                0,
                false,
            );
            // What shows of it drawn sharper, over its place.
            if let Some((part, at)) = &self.part {
                let bounds = Bounds::from_corners(
                    image_bounds.origin
                        + point(
                            image_bounds.size.width * at.left,
                            image_bounds.size.height * at.top,
                        ),
                    image_bounds.origin
                        + point(
                            image_bounds.size.width * at.right,
                            image_bounds.size.height * at.bottom,
                        ),
                );
                let _ =
                    window.paint_image(bounds, bounds, Corners::default(), part.clone(), 0, false);
            }
            for (rects, color) in [
                (&self.highlights, self.colors.0),
                (&self.selection, self.colors.1),
            ] {
                for rect in rects {
                    let bounds = Bounds::new(
                        image_bounds.origin
                            + point(
                                image_bounds.size.width * rect.x,
                                image_bounds.size.height * rect.y,
                            ),
                        size(
                            image_bounds.size.width * rect.width,
                            image_bounds.size.height * rect.height,
                        ),
                    );
                    window.paint_quad(fill(bounds, color));
                }
            }
        });
        let coordinates = move |p: gpui::Point<Pixels>| share(image_bounds, p);
        let callback = self.on_pointer.clone();
        let hitbox = prepaint.hitbox.clone();
        let (grab, drags, page) = (self.grab.clone(), self.drags, self.page.clone());
        window.on_mouse_event(
            move |ev: &MouseDownEvent, phase: DispatchPhase, window, cx| {
                if phase.bubble()
                    && ev.button == MouseButton::Left
                    && hitbox.is_hovered(window)
                    && image_bounds.contains(&ev.position)
                {
                    let (x, y) = coordinates(ev.position);
                    // On a character it selects; anywhere else it holds the
                    // pages, and the page keeps it to tell its click.
                    if page.selects_at(drags, x, y).is_some() {
                        grab.select(ev.position);
                    } else {
                        grab.hold(ev.position);
                    }
                    callback("down", x, y, window, cx);
                    cx.stop_propagation();
                }
            },
        );
        let callback = self.on_pointer.clone();
        let (grab, page) = (self.grab.clone(), self.page.clone());
        window.on_mouse_event(
            move |ev: &MouseMoveEvent, phase: DispatchPhase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                // A drag that moves the pages is the layer's: the page has
                // nothing to select as it goes.
                if ev.pressed_button == Some(MouseButton::Left) && grab.selecting() {
                    let (x, y) = coordinates(ev.position);
                    callback("move", x, y, window, cx);
                }
                // Onto a link, a line of text or the room beside them, or
                // off the page: drawn again for its cursor, once.
                if cursor_at(&grab, &page, drags, image_bounds, ev.position) != cursor {
                    window.refresh();
                }
            },
        );
        let callback = self.on_pointer.clone();
        window.on_mouse_event(move |ev: &MouseUpEvent, phase: DispatchPhase, window, cx| {
            if phase.bubble() && ev.button == MouseButton::Left {
                let (x, y) = coordinates(ev.position);
                callback("up", x, y, window, cx);
            }
        });
    }
}

/// Build a virtual document list with the current viewport dimensions. Layout
/// inputs never make a round trip through `Shell::render`.
pub struct DocumentViewport {
    pub build: Box<dyn FnOnce(gpui::Size<Pixels>, &mut Window, &mut App) -> gpui::AnyElement>,
    pub laid_out: Box<dyn FnOnce(&mut Window, &mut App)>,
}
impl IntoElement for DocumentViewport {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for DocumentViewport {
    type RequestLayoutState = ();
    type PrepaintState = gpui::AnyElement;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
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
    ) -> gpui::AnyElement {
        let build = std::mem::replace(&mut self.build, Box::new(|_, _, _| unreachable!()));
        let mut child = build(bounds.size, window, cx);
        child.layout_as_root(bounds.size.map(gpui::AvailableSpace::Definite), window, cx);
        child.prepaint_at(bounds.origin, window, cx);
        let laid_out = std::mem::replace(&mut self.laid_out, Box::new(|_, _| {}));
        laid_out(window, cx);
        child
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        child: &mut gpui::AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::document::TextChar;

    fn letter(text: &str, x: f32, y: f32, width: f32, height: f32) -> TextChar {
        TextChar {
            text: text.into(),
            bounds: Rect {
                x,
                y,
                width,
                height,
            },
        }
    }

    fn page_of(chars: Vec<TextChar>) -> Page {
        Page::new(Arc::new(PageContent {
            page: 1,
            width: 600.,
            height: 800.,
            chars,
            links: Vec::new(),
        }))
    }

    /// A press selects from anywhere within a line of the text: on a
    /// letter, between words, a little before the first letter and after
    /// the last, in the leading between two lines of a paragraph. The
    /// margins, the room between paragraphs and what has no height move
    /// the pages.
    #[test]
    fn a_press_selects_within_a_line_of_the_text() {
        let unboxed = |text: &str| letter(text, 0., 0., 0., 0.);
        let page = page_of(vec![
            // A paragraph's first line, "ab cd", its space made up.
            letter("a", 0.10, 0.10, 0.02, 0.02),
            letter("b", 0.12, 0.10, 0.02, 0.02),
            unboxed(" "),
            letter("c", 0.20, 0.10, 0.02, 0.02),
            letter("d", 0.22, 0.10, 0.02, 0.02),
            unboxed("\r\n"),
            // Its second, a leading of a quarter of a line below.
            letter("e", 0.10, 0.125, 0.02, 0.02),
            letter("f", 0.12, 0.125, 0.02, 0.02),
            unboxed("\r\n"),
            // The next paragraph's, well below.
            letter("g", 0.10, 0.20, 0.02, 0.02),
            letter("h", 0.12, 0.20, 0.02, 0.02),
            // A character with no height.
            letter("i", 0.50, 0.50, 0.02, 0.),
        ]);
        let at = |x, y| page.selects_at(false, x, y);
        assert_eq!(at(0.11, 0.11), Some(0), "a letter");
        assert_eq!(at(0.16, 0.11), Some(1), "between words, the nearer");
        assert_eq!(at(0.19, 0.11), Some(3));
        assert_eq!(at(0.095, 0.11), Some(0), "just before the first letter");
        assert_eq!(at(0.245, 0.11), Some(4), "just after the last");
        assert_eq!(at(0.11, 0.122), Some(0), "the leading's upper half");
        assert_eq!(at(0.11, 0.123), Some(6), "its lower half");
        assert_eq!(at(0.11, 0.21), Some(9));
        assert_eq!(at(0.085, 0.11), None, "the margin");
        assert_eq!(at(0.26, 0.11), None);
        assert_eq!(at(0.11, 0.17), None, "between paragraphs");
        assert_eq!(at(0.11, 0.09), None, "above the first line");
        assert_eq!(at(0.51, 0.5), None, "no height, no line");
        // Where the drag across text moves the pages, nothing selects.
        assert_eq!(page.selects_at(true, 0.11, 0.11), None);
        assert_eq!(page_of(Vec::new()).selects_at(false, 0.5, 0.5), None);
    }

    /// What the lines answer is what a walk of every character would: the
    /// nearest character (the first of those as near), and on a letter
    /// that letter, on pages of two columns of paragraphs, letters of
    /// every width, spaces with no box.
    #[test]
    fn the_lines_answer_as_every_character_would() {
        let mut seed = 0x2545_f491_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            f32::from(u16::try_from(seed >> 16).unwrap()) / 65_535.
        };
        let mut chars = Vec::new();
        for column in [0.05, 0.55] {
            let mut y = 0.05;
            for _ in 0..40 {
                let mut x = column;
                while x < column + 0.35 {
                    let width = 0.004 + 0.01 * next();
                    if next() < 0.15 {
                        chars.push(letter(" ", 0., 0., 0., 0.));
                    } else {
                        chars.push(letter("x", x, y, width, 0.015 + 0.005 * next()));
                    }
                    x += width;
                }
                chars.push(letter("\n", 0., 0., 0., 0.));
                y += if next() < 0.2 { 0.04 } else { 0.022 };
            }
        }
        let walked = |x: f32, y: f32| {
            chars
                .iter()
                .enumerate()
                .filter_map(|(index, ch)| Some((Span::of(ch.bounds)?.distance(x, y), index)))
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
                .map(|(_, index)| index)
        };
        let page = page_of(chars.clone());
        for _ in 0..4000 {
            let (x, y) = (1.4 * next() - 0.2, 1.4 * next() - 0.2);
            assert_eq!(page.nearest(x, y), walked(x, y), "at {x}, {y}");
        }
        for (index, ch) in chars.iter().enumerate().filter(|(_, ch)| ch.text == "x") {
            let r = ch.bounds;
            let (x, y) = (r.x + r.width / 2., r.y + r.height / 2.);
            assert_eq!(page.selects_at(false, x, y), Some(index));
        }
    }

    /// The cursor is asked for where the pointer is, by one function at
    /// paint and at each move: a page that is not under it asks for none.
    #[test]
    fn the_cursor_is_what_is_under_the_pointer() {
        use crate::elements::pan::Grab;
        use gpui::CursorStyle;
        use skwir_core::document::{Destination, Link};
        let page = Page::new(Arc::new(PageContent {
            page: 1,
            width: 100.,
            height: 100.,
            chars: vec![TextChar {
                text: "a".into(),
                bounds: Rect {
                    x: 0.1,
                    y: 0.1,
                    width: 0.1,
                    height: 0.1,
                },
            }],
            links: vec![Link {
                bounds: Rect {
                    x: 0.5,
                    y: 0.5,
                    width: 0.2,
                    height: 0.1,
                },
                destination: Destination::Page(2),
            }],
        }));
        // Drawn at (100, 100), 100 px square: a share is a pixel past 100.
        let image = Bounds::new(point(px(100.), px(100.)), size(px(100.), px(100.)));
        let at = |x: f32, y: f32| point(px(100. + x), px(100. + y));
        let grab = Grab::default();
        let cursor = |at| cursor_at(&grab, &page, false, image, at);
        assert_eq!(cursor(at(15., 15.)), Some(CursorStyle::IBeam), "text");
        assert_eq!(cursor(at(60., 55.)), Some(CursorStyle::PointingHand));
        assert_eq!(cursor(at(90., 90.)), None, "the room, nothing to move");
        assert_eq!(cursor(at(-50., 15.)), None, "another page's");
        // Where a drag across the text moves the pages, no I-beam.
        assert_eq!(cursor_at(&grab, &page, true, image, at(15., 15.)), None);
        // A selection's I-beam goes wherever its drag does.
        grab.select(at(15., 15.));
        assert_eq!(cursor(at(-50., 15.)), Some(CursorStyle::IBeam));
        assert_eq!(cursor(at(60., 55.)), Some(CursorStyle::IBeam));
    }

    #[test]
    fn fit_keeps_selection_and_pixels_in_the_same_letterboxed_area() {
        assert_eq!(fitted_rect(400., 600., 200., 200.), (0., 100., 400., 400.));
        assert_eq!(fitted_rect(800., 200., 400., 200.), (200., 0., 400., 200.));
    }
}
