//! A text read a screen at a time (ADR-103): a book, a document, a page in
//! reading mode, a message, when continuous scrolling of text is off
//! (`reading.text_continuous`). A page is what the panel holds from the
//! line at its top to the last line that fits whole: never through a line
//! or a picture. The next page starts with the line it left out, and Back
//! retraces the pages turned through; with none behind, the page before
//! ends where the one shown starts, but a section's first page is whole. Pages are where the reader is, not a
//! numbering of the section: nothing lays out the whole section first, so a
//! book kept in one long chapter turns at once, and the bar keeps counting
//! chapters or printed pages.
//!
//! The list is laid out as when read continuously, a spacer after its last
//! item so a section's last page may end short (`SPACER`): the list never
//! leaves room under its end, and would draw that page from higher up. The
//! pager (`overlay`) comes after it, so it reads where this frame laid the
//! items and the lines of their texts (`Drawn`, which each text tells as it
//! is prepainted), finds the page's end, where the list is cut (`cut`:
//! nothing is drawn past it, so the paper shows there as around it,
//! see-through or not), keeps the pointer from what is past it, and takes
//! the wheel before the list would scroll.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{Bounds, Context, ListOffset, ListState, Pixels, ScrollEvent, point, prelude::*, px};

use crate::reader_scroll::WheelTurn;
use crate::shell::Shell;

/// A text drawn this frame, in the window's coordinates: where it starts
/// and ends, and the edges of its rows (`gpui::TextLayout::row_edges`):
/// each row's top, then the text's bottom. A row is one line below the
/// last, or more where a formula in it is taller than its text.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub top: f32,
    pub bottom: f32,
    pub edges: Vec<f32>,
}

impl Drawn {
    /// Its row edges from `from` to `to`.
    fn edges_within(&self, from: f32, to: f32) -> &[f32] {
        let first = self.edges.partition_point(|&edge| edge < from);
        let last = self.edges.partition_point(|&edge| edge <= to);
        &self.edges[first..last.max(first)]
    }

    /// Whether `y` is between two of its rows, or outside it.
    fn between_rows(&self, y: f32) -> bool {
        if y <= self.top + NEAR || y >= self.bottom - NEAR || self.edges.len() < 2 {
            return true;
        }
        !self.edges_within(y - NEAR, y + NEAR).is_empty()
    }

    /// The top of the row `y` falls in, within `NEAR` of the next.
    fn row_top(&self, y: f32) -> Option<f32> {
        let after = self.edges.partition_point(|&edge| edge <= y + NEAR);
        self.edges.get(after.checked_sub(1)?).copied()
    }
}

/// What the texts drawn this frame told: filled while the list is
/// prepainted, read by the pager after it.
pub type Texts = Rc<RefCell<Vec<Drawn>>>;

/// Where gpui sets a row is a sum of row heights: within half a pixel.
const NEAR: f32 = 0.5;

/// Frames a page found back from where it ends may take: a few, as the list
/// measures a page above its top meanwhile (`found_back`); more where an
/// item changes height as it is shown (a picture decoded).
const SETTLE_TRIES: u8 = 6;

/// What the inspector's list lays out beyond its view, above and below.
pub const OVERDRAW: f32 = 100.;

/// How long a trackpad's swipe must pause before it turns again: the
/// system says nothing of where one ends (gpui's Wayland and X11 report
/// every scroll as moving), so a swipe that turned a page goes on turning
/// none until the fingers stop.
const SWIPE_PAUSE: Duration = Duration::from_millis(200);

/// Whether a page may end, or start, at `y`: no text drawn across it, but
/// between two of its lines.
fn between_lines(y: f32, texts: &[Drawn]) -> bool {
    texts.iter().all(|text| text.between_rows(y))
}

/// Where a page may end or start from `from` to `to`: the edges of the
/// items, and the top of each line of their texts.
fn edges(from: f32, to: f32, items: &[(f32, f32)], texts: &[Drawn]) -> Vec<f32> {
    let mut edges: Vec<f32> = items
        .iter()
        .flat_map(|&(top, bottom)| [top, bottom])
        .collect();
    for text in texts {
        edges.extend_from_slice(text.edges_within(from, to.min(text.bottom)));
    }
    edges.retain(|&y| y >= from - NEAR && y <= to + NEAR);
    edges
}

/// Where a page that starts at `top` and may reach `bottom` ends: the
/// lowest edge above `bottom` no text is drawn across. `bottom` itself
/// where there is none past `top` (a line or a picture taller than the
/// page, cut where the panel ends).
pub fn page_end(top: f32, bottom: f32, items: &[(f32, f32)], texts: &[Drawn]) -> f32 {
    edges(top, bottom, items, texts)
        .into_iter()
        .filter(|&y| y > top + NEAR && between_lines(y, texts))
        .reduce(f32::max)
        .map_or(bottom, |y| y.min(bottom))
}

/// Where the page that ends at `end`, `height` tall, starts: the highest
/// edge at most a page above `end` no text is drawn across.
pub fn page_start(end: f32, height: f32, items: &[(f32, f32)], texts: &[Drawn]) -> f32 {
    let from = end - height;
    edges(from, end, items, texts)
        .into_iter()
        .filter(|&y| y < end - NEAR && between_lines(y, texts))
        .reduce(f32::min)
        .map_or(from, |y| y.max(from))
}

/// The top of the line `top` falls in, for a page that would start there
/// to start with that line whole; `top` where no text is drawn across it.
pub fn line_start(top: f32, texts: &[Drawn]) -> f32 {
    texts
        .iter()
        .filter(|text| top > text.top + NEAR && top < text.bottom - NEAR)
        .filter_map(|text| text.row_top(top))
        .fold(top, f32::min)
}

/// A notch of the wheel turns a page, as in the document reader
/// (`WheelTurn`); a trackpad's swipe turns one, then waits for it to pause.
#[derive(Default)]
pub struct Flip {
    turn: WheelTurn,
    /// When the swipe that turned the last page last moved.
    swiped: Option<Instant>,
}

impl Flip {
    /// The page `delta` turns to (1 on, -1 back, 0 none) at `now`; `lines`
    /// for a wheel's notches, `pending` while a section is on its way.
    pub fn turn(&mut self, delta: f32, lines: bool, pending: bool, now: Instant) -> i32 {
        if !lines && let Some(moved) = self.swiped {
            self.swiped = Some(now);
            if now.duration_since(moved) < SWIPE_PAUSE {
                return 0;
            }
            self.swiped = None;
        }
        let turn = self.turn.turn(delta, lines, pending);
        if turn != 0 && !lines {
            self.swiped = Some(now);
        }
        turn
    }
}

/// Where a page found back from its end must end.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ending {
    /// Where the page after it starts.
    At(ListOffset),
    /// Where the section does: the spacer's top.
    End,
}

/// A page being found back from where it ends, over the frames its items
/// take to be laid out.
#[derive(Clone, Copy, Debug)]
struct Settling {
    ending: Ending,
    /// The view was moved to the page's top: the next frame shows it.
    landing: bool,
    /// The frames it took.
    tries: u8,
}

/// What the pager asks of the list after reading a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Move {
    /// Nothing: the page is drawn.
    Stay,
    /// Scroll this far, and read the next frame.
    By(f32),
    /// Go to this offset, and read the next frame: the page above it is
    /// measured as it is laid out (`found_back`).
    To(ListOffset),
}

/// What a turn did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    /// To another page of the section.
    Page,
    /// Past the section's first or last page: the section before or after.
    Section,
    /// Nothing yet: the page turned to is not drawn.
    Wait,
}

/// A frame of the list, as the pager reads it.
pub struct Laid<'a> {
    /// The list's scroll top.
    pub top: ListOffset,
    /// How far the top is from the list's start, items not yet laid out
    /// counted at their estimate: none at the section's start. Never the
    /// top's item number: an item that takes no room (the picture an
    /// inspection carries, empty without one) may stand before it, and
    /// gpui's list scrolled up to its start stops after it.
    pub above: f32,
    /// The top and bottom of its view, in the window.
    pub view: (f32, f32),
    /// What the pages are cut at: the view's width and the text's size.
    pub layout: (f32, f32),
    /// The items laid out from the top: index, top and bottom.
    pub items: &'a [(usize, f32, f32)],
    /// The spacer's index: the section ends at its top.
    pub spacer: usize,
    pub texts: &'a [Drawn],
}

impl Laid<'_> {
    /// Where `at` is in this frame, when it was laid out.
    fn y_of(&self, at: ListOffset) -> Option<f32> {
        self.items
            .iter()
            .find(|&&(ix, ..)| ix == at.item_ix)
            .map(|&(_, top, _)| top + f32::from(at.offset_in_item))
    }

    /// The offset of the item drawn at `y`.
    fn offset_at(&self, y: f32) -> ListOffset {
        match self
            .items
            .iter()
            .find(|&&(_, top, bottom)| y >= top && y < bottom)
        {
            Some(&(ix, top, _)) => ListOffset {
                item_ix: ix,
                offset_in_item: px(y - top),
            },
            None => self.items.last().map_or(self.top, |&(ix, ..)| ListOffset {
                item_ix: ix + 1,
                offset_in_item: px(0.),
            }),
        }
    }
}

/// Whether two offsets are one place.
fn same(a: ListOffset, b: ListOffset) -> bool {
    a.item_ix == b.item_ix
        && (f32::from(a.offset_in_item) - f32::from(b.offset_in_item)).abs() <= NEAR
}

/// The pages of the section shown, as the pager left them.
#[derive(Default)]
pub struct Pages {
    /// Whether the text is read a screen at a time.
    pub on: bool,
    /// The texts this frame drew (`MarkdownStyle::lines`).
    pub texts: Texts,
    /// Where the page shown starts, as the pager read it: a top found
    /// elsewhere was moved there otherwise (a link, the find, a printed
    /// page), and what was turned through no longer leads to it.
    at: Option<ListOffset>,
    /// Where the page shown must end: where the one after starts, when it
    /// was turned back to.
    end: Option<ListOffset>,
    /// Where the page after it starts; `None` when it ends the section.
    next: Option<ListOffset>,
    /// Whether it is the section's first.
    first: bool,
    /// Where it ends in the window, when before the view does: what is
    /// below is not drawn (`cut`).
    pub cover: Option<f32>,
    /// The tops of the pages turned through to it, the last the one before.
    behind: Vec<ListOffset>,
    /// A page being found back from where it ends.
    settling: Option<Settling>,
    /// What the pages were cut at: another width or text size cuts them
    /// again from the one shown.
    layout: Option<(f32, f32)>,
    /// The page shown was read: until then a turn waits.
    drawn: bool,
    pub flip: Flip,
}

impl Pages {
    /// Another section came, or the text is read another way: no page is
    /// read of it yet.
    pub fn reset(&mut self) {
        let (on, texts) = (self.on, Rc::clone(&self.texts));
        *self = Self {
            on,
            texts,
            ..Self::default()
        };
    }

    /// Read a frame of the list: where the page shown ends, and where the
    /// next starts. A page found back from its end, or one whose top is
    /// in a line, moves the list first.
    pub fn read(&mut self, laid: &Laid<'_>) -> Move {
        let (top, bottom) = laid.view;
        let height = bottom - top;
        let items: Vec<(f32, f32)> = laid.items.iter().map(|&(_, t, b)| (t, b)).collect();
        if self.layout != Some(laid.layout) {
            // Another width or text size: its lines are elsewhere, and the
            // page starts again with the one at the top whole.
            if self.layout.is_some() && self.settling.is_none() {
                self.at = None;
            }
            self.layout = Some(laid.layout);
        }
        let at_start = laid.above <= NEAR;
        if let Some(settling) = self.settling.as_mut() {
            settling.tries += 1;
            let at = match settling.ending {
                Ending::At(at) => at,
                Ending::End => ListOffset {
                    item_ix: laid.spacer,
                    offset_in_item: px(0.),
                },
            };
            if !settling.landing && settling.tries <= SETTLE_TRIES {
                // Covered until found: no page drawn on the way.
                self.cover = Some(top);
                let Some(end) = laid.y_of(at) else {
                    return Move::To(at);
                };
                // The view first reaches a page above the end, so what the
                // page holds is laid out; then it moves down to the first
                // whole line there, shown the frame after.
                if top > end - height + NEAR && !at_start {
                    return Move::By(end - height - top);
                }
                let by = page_start(end, height, &items, laid.texts) - top;
                if by.abs() > NEAR {
                    settling.landing = true;
                    return Move::By(by);
                }
            }
            self.settling = None;
            // At the section's start the page is its first, whole: one ending
            // where the page turned from starts may hold only the headings.
            self.end = (!at_start).then_some(at);
            self.at = Some(laid.top);
        }
        if !self.at.is_some_and(|at| same(at, laid.top)) {
            // Moved otherwise: a page starts with its first line whole.
            self.behind.clear();
            self.end = None;
            let start = line_start(top, laid.texts);
            if start < top - NEAR {
                self.at = None;
                self.cover = None;
                return Move::By(start - top);
            }
        }
        let limit = self
            .end
            .and_then(|end| laid.y_of(end))
            .map_or(bottom, |y| y.min(bottom));
        let end = page_end(top, limit, &items, laid.texts);
        let section_end = laid.y_of(ListOffset {
            item_ix: laid.spacer,
            offset_in_item: px(0.),
        });
        self.next = match section_end {
            Some(y) if y <= end + NEAR => None,
            _ => Some(laid.offset_at(end)),
        };
        self.first = at_start;
        self.cover = (end < bottom - NEAR).then_some(end);
        self.at = Some(laid.top);
        self.drawn = true;
        tracing::trace!(top = ?laid.top, view = ?laid.view, end, next = ?self.next, "a page read");
        Move::Stay
    }

    /// Turn to the next page; past the last, the next section.
    pub fn forward(&mut self, list: &ListState) -> Turn {
        if !self.drawn {
            return Turn::Wait;
        }
        let Some(next) = self.next else {
            return Turn::Section;
        };
        self.behind.push(self.top(list));
        self.end = None;
        self.go(list, next);
        Turn::Page
    }

    /// Turn back: to the page turned from, else the page that ends where
    /// this one starts; before the first, the section before.
    pub fn back(&mut self, list: &ListState) -> Turn {
        if !self.drawn {
            return Turn::Wait;
        }
        if self.first {
            return Turn::Section;
        }
        let top = self.top(list);
        if let Some(before) = self.behind.pop() {
            self.end = Some(top);
            self.go(list, before);
            return Turn::Page;
        }
        self.found_back(list, Ending::At(top));
        Turn::Page
    }

    /// The section's first page.
    pub fn first_page(&mut self, list: &ListState) {
        self.behind.clear();
        self.end = None;
        self.go(
            list,
            ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            },
        );
    }

    /// The section's last page: the one that ends where it does.
    pub fn last_page(&mut self, list: &ListState) {
        self.behind.clear();
        self.found_back(list, Ending::End);
    }

    /// Whether item `item` is drawn whole on the page shown, as last read.
    pub fn holds(&self, list: &ListState, item: usize) -> bool {
        let view = list.viewport_bounds();
        list.bounds_for_item(item).is_some_and(|bounds| {
            let end = self.cover.map_or(view.bottom(), px);
            bounds.top() >= view.top() - px(NEAR) && bounds.bottom() <= end + px(NEAR)
        })
    }

    /// Where the page shown starts: as the pager read it, else where the
    /// list is.
    fn top(&self, list: &ListState) -> ListOffset {
        self.at.unwrap_or_else(|| list.logical_scroll_top())
    }

    fn go(&mut self, list: &ListState, to: ListOffset) {
        list.scroll_to(to);
        self.at = Some(to);
        self.drawn = false;
    }

    /// The page that ends at `ending`, found over the next frames from
    /// where the list is. Until it is, the list measures a page above its
    /// top as it lays out (its overdraw a view tall): the move up a page
    /// then crosses heights known, where the estimates of items not laid
    /// out yet (a long paragraph counted as a short one) would leave it
    /// short of the page, a frame per item.
    fn found_back(&mut self, list: &ListState, ending: Ending) {
        list.set_overdraw(list.viewport_bounds().size.height.max(px(OVERDRAW)));
        self.end = None;
        self.settling = Some(Settling {
            ending,
            landing: false,
            tries: 0,
        });
        self.at = None;
        self.drawn = false;
    }
}

/// The items `list` laid out this frame from its top, to the first below
/// its view: read after the list was prepainted.
fn laid_out(list: &ListState, spacer: usize) -> Vec<(usize, f32, f32)> {
    let view = list.viewport_bounds();
    let mut items = Vec::new();
    for ix in list.logical_scroll_top().item_ix..=spacer {
        let Some(bounds) = list.bounds_for_item(ix) else {
            break;
        };
        items.push((ix, f32::from(bounds.top()), f32::from(bounds.bottom())));
        if bounds.top() > view.bottom() {
            break;
        }
    }
    items
}

/// The pager over a text read a screen at a time, after the list in its
/// parent and before what is drawn over it (its scrollbar, the zoom's
/// wheel): it reads the frame the list just laid out, moves the list where
/// a page is still being found, says where the page ends (`Pages::cover`,
/// where `cut` stops drawing the list) and keeps the pointer from what is
/// past it, and turns the page on the wheel, in the capture phase, before
/// the list would scroll. A wheel with Ctrl zooms, with Shift goes
/// sideways: left to what is under it.
pub fn overlay(
    pages: Rc<RefCell<Pages>>,
    list: ListState,
    spacer: usize,
    size: f32,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let shell = cx.weak_entity();
    gpui::canvas(
        move |bounds, window, _| {
            let view = list.viewport_bounds();
            let items = laid_out(&list, spacer);
            let mut pages = pages.borrow_mut();
            let texts = std::mem::take(&mut *pages.texts.borrow_mut());
            let moved = pages.read(&Laid {
                top: list.logical_scroll_top(),
                above: -f32::from(list.scroll_px_offset_for_scrollbar().y),
                view: (f32::from(view.top()), f32::from(view.bottom())),
                layout: (f32::from(view.size.width), size),
                items: &items,
                spacer,
                texts: &texts,
            });
            match moved {
                Move::Stay => {}
                Move::By(by) => list.scroll_by(px(by)),
                Move::To(at) => list.scroll_to(at),
            }
            // A page found, the list lays out no more than it shows.
            if pages.settling.is_none() {
                list.set_overdraw(px(OVERDRAW));
            }
            if moved != Move::Stay {
                window.on_next_frame(|window, _| window.refresh());
            }
            let covered = pages.cover.map(|from| {
                Bounds::from_corners(point(view.left(), px(from)), view.bottom_right())
            });
            if let Some(area) = covered {
                window.insert_hitbox(area, gpui::HitboxBehavior::BlockMouse);
            }
            window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal)
        },
        move |_, wheel, window, _| {
            window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                let held = &event.modifiers;
                if !phase.capture()
                    || !wheel.should_handle_scroll(window)
                    || crate::keymap::secondary(held)
                    || held.shift
                    || held.alt
                    || held.platform
                {
                    return;
                }
                let delta = event.delta.pixel_delta(window.line_height());
                if delta.y == px(0.) || delta.x.abs() > delta.y.abs() {
                    return;
                }
                cx.stop_propagation();
                let lines = matches!(event.delta, gpui::ScrollDelta::Lines(_));
                shell
                    .update(cx, |shell, cx| {
                        shell.inspect_flip(f32::from(delta.y), lines, window, cx);
                    })
                    .ok();
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// `child`, the list of a text read a screen at a time, cut at the page's
/// end (`Pages::cover`, which the pager finds once the list is laid out,
/// before anything is painted): nothing past it is drawn, so the paper
/// under it shows there as around it, see-through or not (ADR-119), where
/// a cover painted over it would be a ground of its own. Without `pages`,
/// `child` as it is.
pub fn cut(pages: Option<Rc<RefCell<Pages>>>, child: impl IntoElement) -> gpui::AnyElement {
    match pages {
        Some(pages) => Cut {
            pages,
            child: child.into_any_element(),
        }
        .into_any_element(),
        None => child.into_any_element(),
    }
}

struct Cut {
    pages: Rc<RefCell<Pages>>,
    child: gpui::AnyElement,
}

impl IntoElement for Cut {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for Cut {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        // Found by the pager's prepaint, which comes after the list's and
        // before every paint.
        let end = self.pages.borrow().cover;
        let Some(end) = end.filter(|end| end.is_finite()) else {
            self.child.paint(window, cx);
            return;
        };
        let (top, bottom) = (f32::from(bounds.top()), f32::from(bounds.bottom()));
        let end = px(end.max(top).min(bottom));
        let shown = Bounds::from_corners(bounds.origin, point(bounds.right(), end));
        window.with_content_mask(Some(gpui::ContentMask::new(shown)), |window| {
            self.child.paint(window, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paragraph's text, `lines` lines of `line` from `top`.
    fn text(top: f32, lines: u8, line: f32) -> Drawn {
        rows(top, &vec![line; usize::from(lines)])
    }

    /// A text of rows `heights` tall from `top`.
    fn rows(top: f32, heights: &[f32]) -> Drawn {
        let mut edges = vec![top];
        for height in heights {
            edges.push(edges.last().unwrap() + height);
        }
        Drawn {
            top,
            bottom: *edges.last().unwrap(),
            edges,
        }
    }

    #[test]
    fn a_page_ends_after_its_last_whole_line() {
        // Two paragraphs of 20 px lines in items padded 8 px: the second
        // runs past the panel's 300 px.
        let items = [(0., 116.), (116., 332.)];
        let texts = [text(8., 5, 20.), text(124., 10, 20.)];
        let end = page_end(0., 300., &items, &texts);
        assert_eq!(end, 284., "after the second paragraph's eighth line");
        // Nothing shorter than a line is lost: the next page starts there.
        assert!(between_lines(end, &texts));
        // A cut through a line is never one.
        assert!(!between_lines(290., &texts));
    }

    #[test]
    fn a_picture_that_does_not_fit_starts_the_next_page() {
        let items = [(0., 116.), (116., 520.)];
        let texts = [text(8., 5, 20.)];
        assert_eq!(page_end(0., 300., &items, &texts), 116.);
        // Taller than the page itself, it is cut where the panel ends.
        assert_eq!(page_end(116., 416., &items, &texts), 416.);
    }

    #[test]
    fn cells_side_by_side_are_cut_between_lines_of_both() {
        // A row of two cells: 18 px lines from 4, and 24 px lines from 4.
        let items = [(0., 400.)];
        let texts = [text(4., 20, 18.), text(4., 16, 24.)];
        let end = page_end(0., 300., &items, &texts);
        assert_eq!(end, 292., "4 + 16 × 18 = 4 + 12 × 24");
        assert!(between_lines(end, &texts));
    }

    #[test]
    fn a_page_found_back_from_its_end_starts_with_a_whole_line() {
        let items = [(0., 116.), (116., 332.), (332., 600.)];
        let texts = [text(8., 5, 20.), text(124., 10, 20.), text(340., 13, 20.)];
        // The page that ends where 400 starts: no higher than 100.
        let start = page_start(400., 300., &items, &texts);
        assert_eq!(
            start, 108.,
            "the first paragraph's end, the first edge below 100"
        );
        assert!(between_lines(start, &texts));
        assert!(start >= 100.);
        // Forward from there, the page ends where it was asked to.
        assert_eq!(page_end(start, 400., &items, &texts), 400.);
    }

    #[test]
    fn a_row_a_formula_grew_is_kept_whole() {
        // 20 px lines from 8, the third row 46 px for a formula.
        let items = [(0., 200.)];
        let texts = [rows(8., &[20., 20., 46., 20., 20.])];
        assert_eq!(page_end(0., 90., &items, &texts), 48., "above the tall row");
        assert_eq!(page_end(0., 95., &items, &texts), 94., "below it");
        assert!(!between_lines(68., &texts), "20 px into the tall row");
        assert_eq!(line_start(70., &texts), 48.);
        assert_eq!(page_start(134., 60., &items, &texts), 94.);
    }

    #[test]
    fn a_top_in_a_line_moves_up_to_that_lines_top() {
        let texts = [text(124., 10, 20.)];
        assert_eq!(line_start(150., &texts), 144.);
        assert_eq!(line_start(144.2, &texts), 144., "already a line's top");
        assert_eq!(line_start(120., &texts), 120., "the padding above it");
    }

    #[test]
    fn a_notch_turns_a_page_and_a_swipe_turns_one_until_it_pauses() {
        let mut flip = Flip::default();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        // Notches turn one each, however close.
        assert_eq!(flip.turn(-3., true, false, at(0)), 1);
        assert_eq!(flip.turn(-3., true, false, at(10)), 1);
        assert_eq!(flip.turn(3., true, false, at(20)), -1);
        // A swipe turns once it moved enough, then not while it goes on.
        let mut ms = 1000;
        let mut turned = 0;
        for _ in 0..40 {
            turned += flip.turn(-25., false, false, at(ms));
            ms += 10;
        }
        assert_eq!(turned, 1, "one page a swipe");
        // After a pause, the next swipe turns again.
        assert_eq!(flip.turn(-40., false, false, at(ms + 300)), 0);
        assert_eq!(flip.turn(-40., false, false, at(ms + 310)), 1);
        // Nothing turns while a section is on its way.
        assert_eq!(flip.turn(-3., true, true, at(ms + 1000)), 0);
    }

    fn offset(item_ix: usize, offset: f32) -> ListOffset {
        ListOffset {
            item_ix,
            offset_in_item: px(offset),
        }
    }

    #[test]
    fn the_next_page_starts_where_the_last_ended_and_back_retraces_it() {
        let list = ListState::new(4, gpui::ListAlignment::Top, px(100.));
        let mut pages = Pages {
            on: true,
            ..Pages::default()
        };
        // Items laid out from the top: the picture (empty), two paragraphs,
        // the spacer at 332.
        let items = [
            (0, 0., 0.),
            (1, 0., 116.),
            (2, 116., 332.),
            (3, 332., 1332.),
        ];
        let texts = [text(8., 5, 20.), text(124., 10, 20.)];
        let laid = Laid {
            top: offset(0, 0.),
            above: 0.,
            view: (0., 300.),
            layout: (600., 16.),
            items: &items,
            spacer: 3,
            texts: &texts,
        };
        assert_eq!(pages.read(&laid), Move::Stay);
        assert_eq!(pages.cover, Some(284.));
        assert_eq!(pages.forward(&list), Turn::Page);
        assert_eq!(list.logical_scroll_top(), offset(2, 168.));
        // A second notch before the page is drawn waits for it.
        assert_eq!(pages.forward(&list), Turn::Wait);
        // Drawn from there, the section ends on this page.
        let items = [(2, -168., 48.), (3, 48., 1048.)];
        let texts = [text(-160., 10, 20.)];
        let laid = Laid {
            top: offset(2, 168.),
            above: 284.,
            items: &items,
            texts: &texts,
            ..laid
        };
        assert_eq!(pages.read(&laid), Move::Stay);
        assert_eq!(pages.cover, Some(48.), "the spacer is covered");
        assert_eq!(pages.forward(&list), Turn::Section);
        // Back is the page turned from, which ends where this one starts.
        assert_eq!(pages.back(&list), Turn::Page);
        assert_eq!(list.logical_scroll_top(), offset(0, 0.));
        assert_eq!(pages.end, Some(offset(2, 168.)));
    }

    /// Three paragraphs of ten 20 px lines in 216 px items after the
    /// picture's empty item, the spacer after them; the panel 300 px. The
    /// frame read with content `y` at the view's top: gpui's list, at its
    /// start, has its top at the first paragraph's.
    fn read(pages: &mut Pages, y: f32) -> Move {
        let items: Vec<(usize, f32, f32)> = [(1, 0.), (2, 216.), (3, 432.), (4, 648.)]
            .into_iter()
            .map(|(ix, top)| (ix, top - y, top - y + if ix == 4 { 1000. } else { 216. }))
            .filter(|&(_, _, bottom)| bottom > 0.)
            .collect();
        let texts: Vec<Drawn> = [8., 224., 440.]
            .into_iter()
            .map(|top| text(top - y, 10, 20.))
            .collect();
        let (ix, at, _) = items[0];
        pages.read(&Laid {
            top: offset(ix, -at),
            above: y,
            view: (0., 300.),
            layout: (600., 16.),
            items: &items,
            spacer: 4,
            texts: &texts,
        })
    }

    #[test]
    fn a_page_turned_back_to_with_nothing_behind_ends_where_the_shown_one_starts() {
        // A link showed the third paragraph at the top.
        let list = ListState::new(5, gpui::ListAlignment::Top, px(100.));
        let mut pages = Pages::default();
        assert_eq!(read(&mut pages, 432.), Move::Stay);
        assert_eq!(pages.back(&list), Turn::Page);
        // Up a page from its top, so what the page before holds is laid out…
        assert_eq!(read(&mut pages, 432.), Move::By(-300.));
        // …then down to the first whole line there: the first's 8th line.
        assert_eq!(read(&mut pages, 132.), Move::By(16.));
        // Shown, it ends where the page turned from starts.
        assert_eq!(read(&mut pages, 148.), Move::Stay);
        assert_eq!(pages.cover, Some(284.));
        assert_eq!(pages.next, Some(offset(3, 0.)));
        // The section's last page is found the same way from its end.
        // Already a page above its end: down to the second's 8th line.
        pages.last_page(&list);
        assert_eq!(read(&mut pages, 148.), Move::By(216.));
        assert_eq!(read(&mut pages, 364.), Move::Stay);
        assert_eq!(pages.cover, Some(284.));
        assert_eq!(pages.next, None, "it ends the section");
    }

    #[test]
    fn the_section_starts_where_nothing_above_the_top_takes_room() {
        // The second paragraph at the top, nothing behind it.
        let list = ListState::new(5, gpui::ListAlignment::Top, px(100.));
        let mut pages = Pages::default();
        assert_eq!(read(&mut pages, 216.), Move::Stay);
        assert_eq!(pages.back(&list), Turn::Page);
        assert_eq!(read(&mut pages, 216.), Move::By(-300.));
        // The list stops at its start, its top the first paragraph's (item
        // 1, the picture's item before it empty): the page found there is
        // the section's first, whole, ending after the second's 4th line
        // rather than where the one turned from starts.
        assert_eq!(read(&mut pages, 0.), Move::Stay);
        assert_eq!(pages.cover, Some(284.));
        assert_eq!(pages.next, Some(offset(2, 68.)));
        assert_eq!(pages.back(&list), Turn::Section);
    }

    #[test]
    fn a_page_moved_to_otherwise_forgets_the_pages_behind_it() {
        let list = ListState::new(4, gpui::ListAlignment::Top, px(100.));
        let mut pages = Pages::default();
        let items = [
            (0, 0., 0.),
            (1, 0., 116.),
            (2, 116., 332.),
            (3, 332., 1332.),
        ];
        let texts = [text(8., 5, 20.), text(124., 10, 20.)];
        let laid = Laid {
            top: offset(0, 0.),
            above: 0.,
            view: (0., 300.),
            layout: (600., 16.),
            items: &items,
            spacer: 3,
            texts: &texts,
        };
        pages.read(&laid);
        pages.forward(&list);
        // A link moved the list into a line: the page starts at its top.
        let moved = Laid {
            top: offset(2, 30.),
            above: 146.,
            items: &[(2, -30., 186.), (3, 186., 1186.)],
            texts: &[text(-22., 10, 20.)],
            ..laid
        };
        // 22 px into the text, the line from 20 px.
        assert_eq!(pages.read(&moved), Move::By(-2.));
        assert!(pages.behind.is_empty());
        // Another width or text size cuts them again too.
        let mut pages = Pages::default();
        pages.read(&laid);
        pages.forward(&list);
        let narrower = Laid {
            top: offset(2, 168.),
            above: 284.,
            layout: (500., 16.),
            items: &[(2, -168., 48.), (3, 48., 1048.)],
            texts: &[text(-160., 10, 20.)],
            ..laid
        };
        pages.read(&narrower);
        assert!(pages.behind.is_empty());
    }
}
