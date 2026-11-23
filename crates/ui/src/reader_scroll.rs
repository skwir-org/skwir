//! Virtual document scrolling and bounded, demand-driven page rendering.
use crate::sides::Sides;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, Context, ListAlignment, ListOffset, ListState, RenderImage, ScrollEvent,
    ScrollHandle, Window, div, point, prelude::*, px,
};
use skwir_core::words::Words;
use skwir_core::{CancelToken, scheduler::Priority};

use crate::{elements::document::Page, preview::PreviewContent, shell::Shell, theme::Theme};

const CACHE_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone)]
pub(crate) struct CachedPage {
    pub image: Arc<RenderImage>,
    /// Its text in lines, found as it was read (`document::Page`).
    pub content: Arc<Page>,
    bytes: usize,
    /// The side it was asked at: drawn again only when pages are shown
    /// larger than that, whatever pixels the engine could give.
    side: u32,
    /// The part of the page it is (ADR-147); `None` for the whole.
    part: Option<skwir_core::document::Part>,
}

/// Steps pages are rendered at: a zoom step of 1.25 crosses at most one.
const SIDE_STEP: f32 = 512.;

/// The side to render a page at that covers `covered` device pixels on its
/// longer side: the next step, a little short allowed, up to
/// `MAX_PART_SIDE`. Drawn whole, a page is held within what a page may
/// hold (`document::held_side`); past it, what shows of it is drawn at
/// this side (`Shell::document_parts`, ADR-147).
pub(crate) fn page_side(covered: f32) -> u32 {
    let steps = if covered.is_finite() {
        (covered / 1.05 / SIDE_STEP).ceil().max(1.)
    } else {
        1.
    };
    ((steps * SIDE_STEP).min(skwir_core::document::MAX_PART_SIDE as f32)) as u32
}

/// The side to render a page at that is fitted in an area `width` by
/// `height` device pixels, its shape unknown until it is rendered: an
/// upright page of the usual shape (ISO's), as wide as the area or as
/// tall. A page of another shape covers less, or more: shown larger, it is
/// drawn again where it is (`Shell::document_shown`).
pub(crate) fn area_side(width: f32, height: f32) -> u32 {
    page_side((width * std::f32::consts::SQRT_2).min(height))
}

/// The side a document's page opened at `side` is first drawn at, to stand
/// in for it while it is drawn (`preview::placeholder`): the smallest
/// step, when it is asked larger. A scan's images are read at a fraction
/// of their size there: a page slow to draw shows at once, blurred.
pub(crate) fn draft_side(side: u32) -> Option<u32> {
    let smallest = SIDE_STEP as u32;
    (side > smallest).then_some(smallest)
}

/// The side a page shown in the preview's content area would be rendered
/// at, as the area was last laid out (`preview::watch_area`), for the
/// preview of each generation: the first page of the first document is
/// rendered at it, before any page was shown to say (`PreviewState::page_side`).
/// The preview's job reads it once it has opened the document, the frame
/// that lays the area out for that preview having come meanwhile.
#[derive(Default)]
pub(crate) struct Area {
    state: parking_lot::Mutex<Measures>,
}

#[derive(Default)]
struct Measures {
    /// The generation of the preview the area was last laid out for, and
    /// the side it gives a page.
    measured: Option<(u64, u32)>,
    /// The room a picture is fitted in there, as last laid out, and the
    /// preview and the place it was laid out for: a picture's side is
    /// worked out from it and its own size (ADR-146).
    room: Option<Laid>,
    /// Where the area is shown, counted (`Area::moved`): the panel, peek,
    /// the whole screen.
    place: u64,
    /// The jobs waiting for the next measure: each a child of its job's
    /// cancel, cancelled by the measure, so one wait ends on either.
    waiting: Vec<skwir_core::cancel::CancelToken>,
}

/// A room as the area was laid out: for the preview `generation`, in the
/// place `place` (`Measures::place`).
#[derive(Clone, Copy)]
struct Laid {
    generation: u64,
    place: u64,
    room: crate::preview::Room,
}

impl Area {
    /// The longest the job waits for the area of its preview to be laid
    /// out: a window not drawn (another workspace) never lays it out.
    const WAIT: std::time::Duration = std::time::Duration::from_millis(200);

    /// The area of the preview `generation` was laid out, at `side`, giving
    /// a picture `room`.
    pub(crate) fn measured(&self, generation: u64, side: u32, room: crate::preview::Room) {
        let mut state = self.state.lock();
        let place = state.place;
        // A room laid out for another preview, or in another place, is
        // what a picture waiting for its own may take (`room_for`).
        let fresh = state
            .room
            .is_none_or(|laid| (laid.generation, laid.place) != (generation, place));
        state.room = Some(Laid {
            generation,
            place,
            room,
        });
        let moved = state.measured != Some((generation, side));
        if moved {
            state.measured = Some((generation, side));
        }
        if moved || fresh {
            for waiting in state.waiting.drain(..) {
                waiting.cancel();
            }
        }
    }

    /// The area is shown elsewhere now (the whole screen entered or left,
    /// peek opened or closed): a room laid out before is another place's,
    /// and a picture asked from now on waits for the area's next layout
    /// (`room_for`).
    pub(crate) fn moved(&self) {
        self.state.lock().place += 1;
    }

    /// The side measured for the preview `generation`, or for a later one
    /// (the same area, laid out for the preview that followed), waiting
    /// `WAIT` at most for the frame that lays it out; else the last
    /// measured, else the smallest step. A job cancelled meanwhile (its
    /// preview gone) waits no longer. Called on a worker.
    pub(crate) fn side(&self, generation: u64, cancel: &skwir_core::cancel::CancelToken) -> u32 {
        let deadline = std::time::Instant::now() + Self::WAIT;
        loop {
            let woken = {
                let mut state = self.state.lock();
                if let Some((laid_out, side)) = state.measured
                    && laid_out >= generation
                {
                    return side;
                }
                let woken = cancel.child();
                state.waiting.push(woken.clone());
                woken
            };
            let signal = woken.subscribe();
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if signal.receiver().recv_timeout(left).is_err() || cancel.is_cancelled() {
                break;
            }
        }
        let state = self.state.lock();
        state.measured.map_or(SIDE_STEP as u32, |(_, side)| side)
    }

    /// The room a picture of the preview `generation` is fitted in: the
    /// area as laid out for it, or for a later one, in the place it is
    /// shown now, waiting `WAIT` at most for the frame that lays it out, as
    /// a first page does (`side`). What the area held before is no measure
    /// of it: a line saying why the last file was not shown, the panel left
    /// for the whole screen. A window that draws nothing, or a job cancelled
    /// meanwhile, takes the room last laid out; `None` if there was none.
    /// Called on a worker.
    pub(crate) fn room_for(
        &self,
        generation: u64,
        cancel: &skwir_core::cancel::CancelToken,
    ) -> Option<crate::preview::Room> {
        let deadline = std::time::Instant::now() + Self::WAIT;
        loop {
            let woken = {
                let mut state = self.state.lock();
                if let Some(laid) = state.room
                    && laid.generation >= generation
                    && laid.place == state.place
                {
                    return Some(laid.room);
                }
                let woken = cancel.child();
                state.waiting.push(woken.clone());
                woken
            };
            let signal = woken.subscribe();
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if signal.receiver().recv_timeout(left).is_err() || cancel.is_cancelled() {
                return self.last_room();
            }
        }
    }

    /// The room a picture was last given, as the area was last laid out,
    /// without waiting: what the window's thread reads.
    pub(crate) fn last_room(&self) -> Option<crate::preview::Room> {
        self.state.lock().room.map(|laid| laid.room)
    }
}

/// Hand what of the pages showed this frame to the shell
/// (`Shell::document_parts`), when it differs from what it last had
/// (`last`): what shows of a page past what is held of it whole is drawn
/// apart (ADR-147).
pub(crate) fn report_seen(
    visible: &crate::elements::document::Visible,
    last: &Rc<Vec<crate::elements::document::Seen>>,
    entity: gpui::WeakEntity<Shell>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let seen = std::mem::take(&mut *visible.borrow_mut());
    if seen.as_slice() == last.as_slice() {
        return;
    }
    window.defer(cx, move |window, cx| {
        if let Some(shell) = entity.upgrade() {
            shell.update(cx, |shell, cx| shell.document_parts(seen, window, cx));
        }
    });
}

/// What of page `seen` to draw apart, at what side (ADR-147): where it
/// covers more device pixels than its pixels drawn whole hold, the part
/// that shows and a margin round it of half as much each way, as many
/// pixels as a page may hold (`document::PAGE_PIXELS`, counted as
/// `Part::most_pixels` counts them, never fewer than an engine draws), at
/// the side it covers, or smaller where more of it shows than a page may
/// hold. `None` while what is drawn whole suffices (at the side asked, or
/// at all the side a part could be drawn at), or `held` (a part drawn or
/// asked, and its side) holds what shows at that side, drawn no more than
/// twice as large.
pub(crate) fn part_wanted(
    seen: &crate::elements::document::Seen,
    held: Option<(skwir_core::document::Part, u32)>,
) -> Option<(skwir_core::document::Part, u32)> {
    use skwir_core::document::{PAGE_PIXELS, Part};
    if !seen.covered.is_finite() || seen.covered <= seen.held as f32 * 1.05 {
        return None;
    }
    let mut side = page_side(seen.covered);
    // The page drawn whole at that side holds it (`whole_side`): it is
    // drawn whole again, no part.
    if skwir_core::document::held_side(seen.own.0, seen.own.1, side) as f32 * 1.05 >= seen.covered {
        return None;
    }
    // More of it shows than a page may hold at that side: drawn smaller,
    // with no margin; no sharper than the whole, not at all.
    let shown = seen.shown.most_pixels(seen.own, side);
    if shown > PAGE_PIXELS {
        let mut smaller = (f64::from(side) * (PAGE_PIXELS as f64 / shown as f64).sqrt()) as u32;
        while smaller > 64 && seen.shown.most_pixels(seen.own, smaller) > PAGE_PIXELS {
            smaller -= smaller.div_ceil(64);
        }
        if smaller as f32 <= seen.held as f32 * 1.05 {
            return None;
        }
        side = smaller;
    }
    // A part drawn at that side or a little short, holding what shows, is
    // kept; one drawn at more than twice it (zoomed out since) is drawn
    // again: shown that much smaller, its letters would shimmer. What it
    // misses by less than half a pixel there (an engine rounds a part to
    // its whole pixels, a share's float a hair either way) it holds: asked
    // again, it would come back the same, and be asked again.
    if let Some((part, held_side)) = held
        && held_side as f32 * 1.05 >= side as f32
        && held_side <= side.saturating_mul(2)
        && part.holds(&within_half_a_pixel(seen, side))
    {
        return None;
    }
    let around = |margin: f32| {
        let (across, down) = (
            (seen.shown.right - seen.shown.left) * margin,
            (seen.shown.bottom - seen.shown.top) * margin,
        );
        Part {
            left: (seen.shown.left - across).max(0.),
            top: (seen.shown.top - down).max(0.),
            right: (seen.shown.right + across).min(1.),
            bottom: (seen.shown.bottom + down).min(1.),
        }
    };
    let room = PAGE_PIXELS as f64 / seen.shown.most_pixels(seen.own, side).max(1) as f64;
    let mut margin = ((room.sqrt() - 1.) / 2.).clamp(0., 0.5) as f32;
    // The whole pixels round the margin count too: a little less of it
    // until the part is within the bound, what shows alone at worst.
    let mut part = around(margin);
    while margin > 0. && part.most_pixels(seen.own, side) > PAGE_PIXELS {
        margin = (margin - 0.005).max(0.);
        part = around(margin);
    }
    Some((part, side))
}

/// What shows of page `seen`, less half a pixel each way of the page laid
/// with `side` on its longer side: what a part drawn at that side must
/// hold to hold what shows.
fn within_half_a_pixel(
    seen: &crate::elements::document::Seen,
    side: u32,
) -> skwir_core::document::Part {
    let longer = seen.own.0.max(seen.own.1).max(1.);
    let pixel = |length: f32| 0.5 * longer / (side.max(1) as f32 * length.max(1.));
    let (across, down) = (pixel(seen.own.0), pixel(seen.own.1));
    let shown = seen.shown;
    let (left, right) = (shown.left + across, shown.right - across);
    let (top, bottom) = (shown.top + down, shown.bottom - down);
    skwir_core::document::Part {
        left: left.min(right),
        top: top.min(bottom),
        right: right.max(left),
        bottom: bottom.max(top),
    }
}

/// Whether `part`, a part drawn of page `seen` (ADR-147), holds more
/// pixels along the page's longer side than its drawing whole shows over
/// the same share (`Seen::held`): an engine with no more pixels than the
/// whole holds (a picture at its own size) draws its part at the whole's.
fn sharper(seen: &crate::elements::document::Seen, part: &CachedPage) -> bool {
    let Some(at) = part.part else {
        return false;
    };
    let size = part.image.size(0);
    let (share, drawn) = if seen.own.0 >= seen.own.1 {
        (at.right - at.left, size.width.0 as f32)
    } else {
        (at.bottom - at.top, size.height.0 as f32)
    };
    drawn > seen.held as f32 * share * 1.05 + 1.
}

/// Hand what the pages covered this frame to the shell, when it changes the
/// side they are rendered at or the scale a zoom step starts from.
pub(crate) fn report_shown(
    report: &crate::elements::document::Report,
    side: Option<u32>,
    scale: Option<f32>,
    entity: gpui::WeakEntity<Shell>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let Some(shown) = report.take() else {
        return;
    };
    if Some(page_side(shown.covered)) == side
        && scale.is_some_and(|s| (s - shown.scale).abs() < 1e-4)
    {
        return;
    }
    window.defer(cx, move |window, cx| {
        if let Some(shell) = entity.upgrade() {
            shell.update(cx, |shell, cx| shell.document_shown(shown, window, cx));
        }
    });
}
struct PageResult {
    /// The generation a page whole was asked in; a part's, the number of
    /// its request (`PendingPart::serial`).
    generation: u64,
    number: u32,
    result: Result<CachedPage, Words>,
    /// What shows of the page (ADR-147), not the page whole.
    part: bool,
}
/// The part of a page being drawn (ADR-147): its page, what of it and at
/// what side, the number its answer comes back with, and its own cancel,
/// apart from the whole pages'.
struct PendingPart {
    page: u32,
    part: skwir_core::document::Part,
    side: u32,
    serial: u64,
    cancel: CancelToken,
}
pub struct DocumentScroll {
    pub continuous: bool,
    pub list: ListState,
    pub single: ScrollHandle,
    /// The pages scrolled sideways, when a zoom makes them wider than the
    /// panel.
    sideways: ScrollHandle,
    /// Shared with each frame's rows, which only read them: a frame costs
    /// a reference, not a copy of every page seen.
    pub(crate) pages: Rc<BTreeMap<u32, CachedPage>>,
    sizes: Rc<BTreeMap<u32, (f32, f32)>>,
    /// The widest page seen, in page units, for the sideways scroll at a zoom.
    widest: f32,
    errors: Rc<BTreeMap<u32, Words>>,
    /// What shows of pages past what is held of them whole, drawn sharper
    /// (ADR-147): one part a page, kept while the page shows and needs one.
    pub(crate) parts: Rc<BTreeMap<u32, CachedPage>>,
    /// What of the pages showed last, as their elements said it
    /// (`report_seen`): what the part drawn is chosen from
    /// (`choose_part`).
    pub(crate) seen: Rc<Vec<crate::elements::document::Seen>>,
    /// The part being drawn, one at a time, and the number the last was
    /// asked with.
    part_pending: Option<PendingPart>,
    part_serial: u64,
    /// Pages whose part could not be drawn: not asked again until the
    /// page is visited or the pages refreshed, as a page's failure.
    refused: std::collections::BTreeSet<u32>,
    /// A part waits for a search to end (`choose_part`).
    part_waits: bool,
    wanted: Vec<u32>,
    pending: Option<(u32, CancelToken)>,
    generation: u64,
    tx: flume::Sender<PageResult>,
    rx: flume::Receiver<PageResult>,
    layout: Rc<RefCell<Option<(gpui::Size<gpui::Pixels>, Option<f32>, bool)>>>,
    /// Where Ctrl and the wheel zoomed from, put back under the pointer
    /// by the layout at the new zoom (`continuous_element`).
    zoom_anchor: Rc<std::cell::Cell<Option<ZoomAnchor>>>,
    wheel: WheelTurn,
    pub bottom_on_load: bool,
}
impl Default for DocumentScroll {
    fn default() -> Self {
        Self::new(false)
    }
}
impl DocumentScroll {
    /// Pages read continuously or a page at a time, none of them known yet.
    pub(crate) fn new(continuous: bool) -> Self {
        let (tx, rx) = flume::unbounded();
        Self {
            continuous,
            list: ListState::new(0, ListAlignment::Top, px(100.)),
            single: ScrollHandle::new(),
            sideways: ScrollHandle::new(),
            pages: Rc::default(),
            sizes: Rc::default(),
            widest: 0.,
            errors: Rc::default(),
            parts: Rc::default(),
            seen: Rc::default(),
            part_pending: None,
            part_serial: 0,
            refused: std::collections::BTreeSet::new(),
            part_waits: false,
            wanted: Vec::new(),
            pending: None,
            generation: 0,
            tx,
            rx,
            layout: Rc::default(),
            zoom_anchor: Rc::default(),
            wheel: WheelTurn::default(),
            bottom_on_load: false,
        }
    }
}
impl Drop for DocumentScroll {
    fn drop(&mut self) {
        self.cancel_all();
    }
}
impl DocumentScroll {
    fn evict_offscreen(&mut self, current: u32) -> Vec<CachedPage> {
        // Visible/prefetched pixels stay available even when they exceed the
        // cache target. The limit bounds retention, not viewport dimensions.
        let mut bytes: usize = self.pages.values().map(|p| p.bytes).sum();
        let mut candidates: Vec<_> = self
            .pages
            .keys()
            .copied()
            .filter(|p| !self.wanted.contains(p))
            .collect();
        candidates.sort_by_key(|p| std::cmp::Reverse(p.abs_diff(current)));
        let mut evicted = Vec::new();
        // What showed of a page goes as soon as it is not wanted.
        let wanted: std::collections::BTreeSet<u32> = self.wanted.iter().copied().collect();
        let gone: Vec<u32> = self
            .parts
            .keys()
            .copied()
            .filter(|page| !wanted.contains(page))
            .collect();
        for page in gone {
            if let Some(part) = Rc::make_mut(&mut self.parts).remove(&page) {
                evicted.push(part);
            }
        }
        for number in candidates {
            if bytes <= CACHE_BYTES {
                break;
            }
            if let Some(page) = Rc::make_mut(&mut self.pages).remove(&number) {
                bytes -= page.bytes;
                evicted.push(page);
            }
        }
        evicted
    }
    /// The page read continuously under `pointer` (in the window) and the
    /// share of it across and down there, as the last layout placed the
    /// rows. `opened` is as `Shell::reader_zoom_toward` has it.
    fn page_under(
        &self,
        pointer: gpui::Point<gpui::Pixels>,
        opened: (u32, (f32, f32)),
    ) -> Option<(u32, (f32, f32))> {
        let list = &self.list;
        let first = list.logical_scroll_top().item_ix;
        let (index, row) = (first..list.item_count())
            .map_while(|index| list.bounds_for_item(index).map(|row| (index, row)))
            .find(|(_, row)| pointer.y < row.bottom())?;
        let number = index as u32 + 1;
        let (width, height) = if number == opened.0 {
            opened.1
        } else {
            self.sizes.get(&number).copied().unwrap_or(opened.1)
        };
        // A row is the page, centred across the column, and its gap.
        let down = (f32::from(row.size.height) - PAGE_GAP).max(1.);
        let across = (width * down / height.max(1.)).max(1.);
        let left = f32::from(row.left()) + (f32::from(row.size.width) - across) / 2.;
        let share = (
            (f32::from(pointer.x) - left) / across,
            f32::from(pointer.y - row.top()) / down,
        );
        Some((number, share))
    }
    /// The page whole being drawn given up, and every one asked before:
    /// their answers are let go (`generation`). The part being drawn is
    /// not: what shows, not which pages are read ahead, says whether it is
    /// still wanted (`choose_part`).
    fn cancel(&mut self) {
        if let Some((_, token)) = self.pending.take() {
            token.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
    }
    /// The part being drawn given up: its answer is let go, being another
    /// request's number (`PendingPart::serial`).
    fn cancel_part(&mut self) {
        if let Some(pending) = self.part_pending.take() {
            pending.cancel.cancel();
        }
    }
    /// Everything being drawn given up: the pages go, or the reader does.
    fn cancel_all(&mut self) {
        self.cancel();
        self.cancel_part();
        self.part_waits = false;
    }
    /// Choose again what of the pages is drawn apart (ADR-147), from what
    /// showed last (`seen`): the parts of pages no longer shown, or that
    /// need none at the zoom they show at, are let go (returned, for the
    /// atlas); a part being drawn that is no longer what would be asked is
    /// given up; then, unless one is still being drawn, a search runs
    /// (`searching`: it waits for it, `part_waits`) or the session draws
    /// no part sharper than its whole (`draws`, `draws_parts`; nor does a
    /// page in `refused`), the page that most
    /// needs one (`part_wanted`), the one showing most first, and what to
    /// draw of it. One at a time, chosen again whenever what shows
    /// changes, a part comes back or fails, or a search ends: a view that
    /// moved meanwhile, or a second page in view, is never left without.
    fn choose_part(
        &mut self,
        searching: bool,
        draws: bool,
    ) -> (
        Vec<CachedPage>,
        Option<(u32, skwir_core::document::Part, u32)>,
    ) {
        // The last said of each page: a layout tried again in the same
        // frame (gpui's list after an autoscroll) said it once before.
        let seen: BTreeMap<u32, crate::elements::document::Seen> =
            self.seen.iter().map(|seen| (seen.page, *seen)).collect();
        let gone: Vec<u32> = self
            .parts
            .keys()
            .copied()
            .filter(|page| {
                seen.get(page)
                    .is_none_or(|seen| part_wanted(seen, None).is_none())
            })
            .collect();
        let mut let_go = Vec::new();
        for page in gone {
            if let Some(part) = Rc::make_mut(&mut self.parts).remove(&page) {
                let_go.push(part);
            }
        }
        if let Some(pending) = &self.part_pending {
            let holds = seen.get(&pending.page).is_some_and(|seen| {
                part_wanted(seen, None).is_some()
                    && part_wanted(seen, Some((pending.part, pending.side))).is_none()
            });
            if holds {
                return (let_go, None);
            }
            self.cancel_part();
        }
        self.part_waits = searching;
        if searching || !draws {
            return (let_go, None);
        }
        let chosen = seen
            .values()
            .filter(|seen| !self.refused.contains(&seen.page))
            .filter_map(|seen| {
                let held = self
                    .parts
                    .get(&seen.page)
                    .and_then(|cached| Some((cached.part?, cached.side)));
                let (part, side) = part_wanted(seen, held)?;
                let area =
                    (seen.shown.right - seen.shown.left) * (seen.shown.bottom - seen.shown.top);
                Some((area, seen.page, part, side))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, page, part, side)| (page, part, side));
        (let_go, chosen)
    }
    /// What shows of page `page` drawn sharper, and where it lies on it.
    pub(crate) fn part_of(
        &self,
        page: u32,
    ) -> Option<(Arc<RenderImage>, skwir_core::document::Part)> {
        let cached = self.parts.get(&page)?;
        Some((cached.image.clone(), cached.part?))
    }
    /// Page `page`, kept rendered at `side` or larger, or at as much as it
    /// may hold whole there: shown at once.
    fn kept(&self, page: u32, side: u32) -> Option<&CachedPage> {
        let whole = self.whole_side(page, side);
        self.pages.get(&page).filter(|kept| kept.side >= whole)
    }
    /// The side page `page` is drawn whole at when pages are drawn at
    /// `side`: as much as a page of its shape may hold
    /// (`document::held_side`), what shows of it past that drawn apart
    /// (ADR-147). Its shape as the engine said it, else that of the nearest
    /// page known before it, or after (the one shown first at worst,
    /// `keep_shown_page`), as a row counts a page not drawn yet as one that
    /// was; an upright page of ISO's shape only before any is known.
    /// Counted as ISO's until each came, a webtoon's every page was drawn
    /// whole twice, first 273 pixels across.
    pub(crate) fn whole_side(&self, page: u32, side: u32) -> u32 {
        let (width, height) = self
            .sizes
            .get(&page)
            .or_else(|| self.sizes.range(..page).next_back().map(|(_, size)| size))
            .or_else(|| self.sizes.range(page..).next().map(|(_, size)| size))
            .copied()
            .unwrap_or((1., std::f32::consts::SQRT_2));
        skwir_core::document::held_side(width, height, side)
    }
    /// Whether `image` is a page kept here: what shows it lets go of it
    /// without dropping it from the atlas, which the cache does as the page
    /// goes.
    pub(crate) fn holds(&self, image: &Arc<RenderImage>) -> bool {
        self.pages
            .values()
            .any(|page| Arc::ptr_eq(&page.image, image))
    }
    /// A render failure stays visible until the user visits that page
    /// explicitly or refreshes; then the page is requested again.
    fn forget_errors(&mut self, only: Option<u32>) -> bool {
        if let Some(number) = only {
            self.refused.remove(&number);
            return Rc::make_mut(&mut self.errors).remove(&number).is_some();
        }
        self.refused.clear();
        let had = !self.errors.is_empty();
        self.errors = Rc::default();
        had
    }
}

/// One event of the wheel as a counter of steps reads it (`WheelTurn`),
/// along one axis, in what the device said: a wheel's notches (whole
/// lines, [`notches`]), lines in parts (a high-resolution wheel's steps
/// between its notches; a touchpad's on X11, all of `XInput`'s smooth
/// scrolling, and through Windows' wheel messages), or a touchpad's
/// pixels. Negative is down (or right), as gpui says it. Every wheel that
/// counts steps reads its event through this, never by whether gpui said
/// lines: lines in parts are no notch, and counted as one each a short
/// slide of a touchpad went to the end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wheeled {
    Notches(f32),
    Lines(f32),
    Pixels(f32),
}

impl Wheeled {
    /// `event` up and down.
    pub fn vertical(event: &ScrollEvent) -> Self {
        Self::of(event, |_, y| y)
    }

    /// `event` along the axis it moved most: a sideways swipe counts as
    /// much as a turn of the wheel (the tab strip's).
    pub fn along(event: &ScrollEvent) -> Self {
        Self::of(event, |x, y| if y.abs() >= x.abs() { y } else { x })
    }

    fn of(event: &ScrollEvent, axis: impl Fn(f32, f32) -> f32) -> Self {
        match event.delta {
            gpui::ScrollDelta::Lines(lines) if notches(&event.delta) => {
                Self::Notches(axis(lines.x, lines.y))
            }
            gpui::ScrollDelta::Lines(lines) => Self::Lines(axis(lines.x, lines.y)),
            gpui::ScrollDelta::Pixels(pixels) => {
                Self::Pixels(axis(f32::from(pixels.x), f32::from(pixels.y)))
            }
        }
    }

    /// Whether it is a wheel's notches.
    pub fn notches(self) -> bool {
        matches!(self, Self::Notches(_))
    }
}

/// How many lines a wheel's notch says: gpui's `SCROLL_LINES` on Wayland
/// and X11, Windows' own number by default. Lines in parts make a step
/// once they add up to one, so a high-resolution wheel's notch is one
/// step as a plain wheel's is.
const NOTCH_LINES: f32 = 3.;
/// How far a touchpad's pixels go for a step: it has to mean it.
const STEP_PIXELS: f32 = 60.;

/// Steps counted from the wheel (`Wheeled`): a notch is a step at once,
/// lines in parts a step once they make a notch, a touchpad's pixels one
/// at `STEP_PIXELS`, so no step is ever more than a notch's worth. A
/// touchpad gesture must accumulate intent before it turns a page. The
/// invariant that prevents racing turns is that no turn starts while a
/// page request is pending: a gesture arriving then is refused and its
/// accumulated intent discarded, so the next gesture starts fresh once the
/// page has arrived. The grid's Ctrl+wheel zoom, the tab strip and a
/// player's volume count steps the same way, with nothing ever pending.
#[derive(Default)]
pub struct WheelTurn {
    /// What the gesture has added up toward its next step, in its unit.
    accumulated: f32,
    /// Whether that is lines (else pixels): a change of unit starts over.
    in_lines: bool,
}
impl WheelTurn {
    /// The step `wheeled` makes, 1 down (or right), -1 up, else 0.
    pub fn turn(&mut self, wheeled: Wheeled, pending: bool) -> i32 {
        let (amount, in_lines, step) = match wheeled {
            Wheeled::Notches(lines) => (lines, true, 0.),
            Wheeled::Lines(lines) => (lines, true, NOTCH_LINES),
            Wheeled::Pixels(pixels) => (pixels, false, STEP_PIXELS),
        };
        if !amount.is_finite() || amount == 0. {
            return 0;
        }
        if pending || self.in_lines != in_lines || self.accumulated.signum() != amount.signum() {
            self.accumulated = 0.;
        }
        self.in_lines = in_lines;
        if pending {
            return 0;
        }
        self.accumulated += amount;
        // Parts that add up to a step in floating point may fall a hair
        // short of it (eight eighths of a notch).
        if self.accumulated.abs() >= step - 1e-3 {
            self.accumulated = 0.;
            if amount < 0. { 1 } else { -1 }
        } else {
            0
        }
    }

    /// What the gesture added up, forgotten (the wheel went elsewhere).
    pub fn reset(&mut self) {
        self.accumulated = 0.;
    }
}

/// How far a line of the wheel moves pages, read continuously or a page at
/// a time: 40 px, so a notch (three lines on Linux) goes 120 px, as
/// Chromium's does there (its PDF viewer's). gpui's list counts a line
/// 20 px, a `div` the window's line height (26 px): both crawled through
/// a page. Only a wheel's notches go at it ([`notches`]).
pub(crate) const WHEEL_LINE: gpui::Pixels = px(40.);

/// Whether `delta` is a wheel's notches: lines, each axis that moves a
/// whole number of them. gpui says lines for a touchpad too on X11 (all of
/// `XInput`'s smooth scrolling) and through Windows' wheel messages, and for
/// a high-resolution wheel's steps between notches, but those come as
/// parts of a line; a notch is `SCROLL_LINES` whole (Wayland's, X11's and
/// Windows' lines a notch). A touchpad's pixels, and lines in parts, go as
/// gpui counts them.
fn notches(delta: &gpui::ScrollDelta) -> bool {
    let gpui::ScrollDelta::Lines(lines) = delta else {
        return false;
    };
    let whole =
        |n: f32| n.is_finite() && (n == 0. || (n.abs() >= 1. && (n - n.round()).abs() < 1e-3));
    (lines.x != 0. || lines.y != 0.) && whole(lines.x) && whole(lines.y)
}

/// How far a plain wheel moves the column down (up when negative): its
/// notches at `WHEEL_LINE`, `None` for what is left to gpui's own
/// scrolling (a touchpad's pixels or its lines in parts, a wheel turned
/// sideways, one with a modifier).
fn wheel_step(event: &ScrollEvent) -> Option<f32> {
    let gpui::ScrollDelta::Lines(lines) = event.delta else {
        return None;
    };
    (!event.modifiers.modified() && lines.x == 0. && notches(&event.delta))
        .then(|| -lines.y * f32::from(WHEEL_LINE))
}

/// The wheel over pages read continuously, at a document's step
/// (`WHEEL_LINE`): drawn after the column, its listener comes before the
/// list's (bubbling runs last drawn first), and after what is drawn over
/// the pages, which keeps the wheel.
fn wheel_steps(list: ListState, shell: gpui::WeakEntity<Shell>) -> impl IntoElement {
    gpui::canvas(
        |bounds, window, _| window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal),
        move |_, hitbox, window, _| {
            window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                if !phase.bubble() || !hitbox.should_handle_scroll(window) {
                    return;
                }
                let Some(step) = wheel_step(event) else {
                    return;
                };
                list.scroll_by(px(step));
                shell.update(cx, |_, cx| cx.notify()).ok();
                cx.stop_propagation();
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The room under each page read continuously, before the next.
const PAGE_GAP: f32 = 12.;

/// The scale a page `page` long (across, down) is shown at, read
/// continuously in a view `view` long: the zoom, else as wide as the view
/// (Fit width), else whole above its gap (Fit).
fn page_scale(zoom: Option<f32>, fit_width: bool, view: (f32, f32), page: (f32, f32)) -> f32 {
    zoom.unwrap_or_else(|| {
        let across = view.0 / page.0.max(1.);
        if fit_width {
            across
        } else {
            across.min((view.1 - PAGE_GAP).max(1.) / page.1.max(1.))
        }
    })
}

/// Where Ctrl and the wheel zoomed from, read continuously: the page under
/// the pointer, the share of it across and down there, and where the
/// pointer is from the view's corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ZoomAnchor {
    page: u32,
    share: (f32, f32),
    at: (f32, f32),
}

/// Where the column lands for `anchor` to be under the pointer again, its
/// pages shown `shown(page)` large (across, down) in a column
/// `content_width` wide, in a view `width` wide: the row at the top and how
/// far down into it, and the sideways offset. Rows above the page are
/// counted at their own heights; the start and the sides stop it.
fn landing(
    anchor: ZoomAnchor,
    shown: impl Fn(u32) -> (f32, f32),
    content_width: f32,
    width: f32,
) -> ((usize, f32), f32) {
    let (across, down) = shown(anchor.page);
    let mut top = anchor.at.1 - anchor.share.1 * down;
    let mut row = anchor.page.saturating_sub(1) as usize;
    while top > 0. && row > 0 {
        row -= 1;
        top -= shown(row as u32 + 1).1.max(1.) + PAGE_GAP;
    }
    let left = (content_width - across) / 2.;
    let x =
        (anchor.at.0 - left - anchor.share.0 * across).clamp((width - content_width).min(0.), 0.);
    ((row, (-top).max(0.)), x)
}

/// Whether a page shown a page at a time, scrolled to `offset` of `-most`
/// (its `ScrollHandle`'s), is at its edge toward `toward` (-1 up, 1 down):
/// an arrow or the wheel there turns the page. The handle clamps to
/// exactly these bounds, so they are compared exactly, as numbers: gpui
/// orders `Pixels` totally, -0 below 0, so a page that fits (offset and
/// most 0) was never at its bottom edge for `-offset >= most` in `Pixels`.
fn at_page_edge(offset: f32, most: f32, toward: i32) -> bool {
    if toward > 0 {
        -offset >= most
    } else {
        offset >= 0.
    }
}

/// A backward turn lands on the new page's bottom edge, but only when the
/// turn actually changed the page: at page 1 the current page keeps its offset.
fn lands_at_bottom(turn: i32, before: u32, after: u32) -> bool {
    turn < 0 && after != before
}

impl Shell {
    /// `ui.reader_continuous`: the reader's own toggle. The choice is the
    /// user's, not this document's, so it is remembered and the next
    /// document opens the same way.
    pub fn reader_continuous(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let continuous = !self.preview.reader.scroll.continuous;
        self.set_reader_continuous(continuous, window, cx);
        crate::settings::remember_reader_mode(self, continuous, cx);
    }

    /// Read this document as one scrolling column, or a page at a time.
    pub fn set_reader_continuous(
        &mut self,
        continuous: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &self.preview.reader.session else {
            return;
        };
        if self.preview.reader.scroll.continuous == continuous {
            return;
        }
        let count = crate::reader::page_count(session.as_ref()) as usize;
        let scroll = &mut self.preview.reader.scroll;
        scroll.continuous = continuous;
        scroll.cancel_all();
        scroll.wanted.clear();
        scroll.errors = Rc::default();
        scroll.list.reset(count);
        // Forget the laid-out signature so the next build sizes the pages,
        // and with them the scrollbar, from scratch.
        scroll.layout.borrow_mut().take();
        scroll.list.scroll_to(ListOffset {
            item_ix: self.preview.page as usize - 1,
            offset_in_item: px(0.),
        });
        scroll.single.set_offset(point(px(0.), px(0.)));
        self.preview.reader.selection = None;
        self.preview.reader.selection_page = None;
        self.preview.reader.cancel_search();
        if !self.preview.reader.scroll.continuous {
            if let Some(page) =
                Rc::make_mut(&mut self.preview.reader.scroll.pages).remove(&self.preview.page)
            {
                let size = page.image.size(0);
                self.replace_preview_content(
                    Some(PreviewContent::Image {
                        image: page.image,
                        width: size.width.0 as u32,
                        height: size.height.0 as u32,
                        detail: Some(skwir_core::preview::PreviewInfo::Document {
                            page: self.preview.page,
                            pages: count as u32,
                        }),
                        document: Some(page.content),
                        facts: Vec::new(),
                        pixels: crate::preview::Pixels::Own,
                        ink: None,
                    }),
                    window,
                    cx,
                );
            }
            self.clear_document_pages(window, cx);
        }
        self.reader_mode_changed();
        cx.notify();
    }

    /// A document session arrived, in a reader that already reads the way
    /// the user chose (`let_reader_go`): read continuously, its column gets
    /// an item per page, at the page asked for, or, read again after its
    /// file changed, at the same place over its new number of pages.
    pub(crate) fn reader_opened(
        &mut self,
        session: Arc<dyn skwir_core::document::DocumentSession>,
        cx: &mut Context<Self>,
    ) {
        let first = self.preview.reader.session.is_none();
        let reopened = std::mem::take(&mut self.preview.reader.reopened);
        let count = crate::reader::page_count(session.as_ref()) as usize;
        // The outline is built once for the session, not at each page it
        // draws (a page at a time hands the same session again).
        let same = self.preview.reader.session.as_ref().is_some_and(|had| {
            std::ptr::eq(
                Arc::as_ptr(had).cast::<()>(),
                Arc::as_ptr(&session).cast::<()>(),
            )
        });
        if !same {
            self.preview.reader.outline = crate::reader::Outline::of(&session.info().outlines);
        }
        self.preview.reader.session = Some(session);
        self.preview
            .reader
            .focus
            .get_or_insert_with(|| cx.focus_handle());
        if !first {
            return;
        }
        // A book laid out again lands where it was read (`reader_land`).
        let relaid = self.preview.reader.relayout.take().filter(|_| reopened);
        let scroll = &mut self.preview.reader.scroll;
        if scroll.continuous {
            let mut top = if reopened {
                scroll.list.logical_scroll_top()
            } else {
                ListOffset {
                    item_ix: (self.preview.page as usize).saturating_sub(1),
                    offset_in_item: px(0.),
                }
            };
            top.item_ix = top.item_ix.min(count.saturating_sub(1));
            scroll.list.reset(count);
            scroll.layout.borrow_mut().take();
            scroll.list.scroll_to(top);
        }
        if let Some(anchor) = relaid {
            self.reader_land(anchor, cx);
        }
    }

    /// The document changed on disk (a PDF rebuilt by LaTeX): read it again
    /// where the reader is. The page, the zoom, the mode and the place in
    /// the column stay; the old session, its pages and its matches go.
    pub(crate) fn reader_reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.reader.session.is_none() {
            return;
        }
        self.clear_document_pages(window, cx);
        let reader = &mut self.preview.reader;
        reader.cancel_search();
        reader.hits.clear();
        reader.active_hit = None;
        reader.selection = None;
        reader.selection_page = None;
        reader.down = None;
        reader.session = None;
        reader.outline = None;
        reader.tried = false;
        reader.reopened = true;
        std::mem::take(&mut reader.lifetime).cancel();
        reader.scroll.sizes = Rc::default();
        reader.scroll.widest = 0.;
    }

    pub fn reader_wheel(
        &mut self,
        event: &ScrollEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A wheel's notches at a document's step, the rest as a `div`
        // counts them.
        let wheel = notches(&event.delta);
        let line = if wheel {
            WHEEL_LINE
        } else {
            window.line_height()
        };
        let delta = event.delta.pixel_delta(line);
        let pending = self.preview.phase != crate::preview::PreviewPhase::Idle;
        let scroll = &mut self.preview.reader.scroll;
        let offset = scroll.single.offset();
        let maximum = scroll.single.max_offset();
        let toward = if delta.y < px(0.) { 1 } else { -1 };
        let at_edge = at_page_edge(offset.y.into(), maximum.y.into(), toward);
        let vertical = delta.y.abs() > delta.x.abs();
        if !at_edge
            || !vertical
            || crate::keymap::secondary(&event.modifiers)
            || event.modifiers.alt
            || event.modifiers.platform
            || event.modifiers.shift
        {
            scroll.wheel.reset();
            scroll.single.set_offset(point(
                (offset.x + delta.x).clamp(-maximum.x, px(0.)),
                (offset.y + delta.y).clamp(-maximum.y, px(0.)),
            ));
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let turn = scroll.wheel.turn(Wheeled::vertical(event), pending);
        // Keep edge gestures out of the surrounding file list even at page 1/N.
        cx.stop_propagation();
        if turn != 0 {
            let before = self.preview.page;
            self.run(
                "ui.preview_page",
                serde_json::json!({"delta":turn}),
                window,
                cx,
            );
            self.preview.reader.scroll.bottom_on_load =
                lands_at_bottom(turn, before, self.preview.page);
        }
    }

    /// An arrow over the pages (`Shell::preview_scroll`): whether they
    /// scroll along its way. Read continuously, the column moves. A page at
    /// a time, Up or Down at its edge turns it, as the wheel does, and a
    /// turn back lands on the bottom of the page before; while a turn is on
    /// its way they wait, so a held key never scrolls the page left behind
    /// and the next one starts at its top.
    pub(crate) fn reader_arrow(
        &mut self,
        dx: i32,
        dy: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let scroll = &self.preview.reader.scroll;
        if dx != 0 {
            let across = if scroll.continuous {
                &scroll.sideways
            } else {
                &scroll.single
            };
            return crate::preview::arrow(across, dx, 0);
        }
        if scroll.continuous {
            let view = f32::from(scroll.list.viewport_bounds().size.height);
            scroll
                .list
                .scroll_by(px(crate::preview::arrow_step(view) * dy as f32));
            return true;
        }
        let shown = matches!(
            &self.preview.content,
            Some(PreviewContent::Image { document: Some(page), .. }) if page.page == self.preview.page
        );
        if !shown {
            return true;
        }
        let (offset, most) = (scroll.single.offset(), scroll.single.max_offset());
        if !at_page_edge(offset.y.into(), most.y.into(), dy) {
            crate::preview::arrow(&scroll.single, 0, dy);
            return true;
        }
        let before = self.preview.page;
        self.preview_page(dy, window, cx);
        self.preview.reader.scroll.bottom_on_load = lands_at_bottom(dy, before, self.preview.page);
        true
    }

    /// Ctrl and the wheel zoomed the pages from `zoom` (or Fit width) to
    /// `to`: what is under `pointer` (in the window) stays there
    /// (`Shell::preview_zoom_toward`). A page at a time scrolls now. Read
    /// continuously, the page under the pointer and the share of it there
    /// wait for the layout at the new zoom, where the rows' heights are
    /// known (`continuous_element`); a turn of the wheel that layout has
    /// not caught up with keeps the place the last one found. `opened` is
    /// the page the preview opened on and its size, which pages not seen
    /// yet are counted at.
    pub(crate) fn reader_zoom_toward(
        &mut self,
        opened: (u32, (f32, f32)),
        (zoom, fit_width): (Option<f32>, bool),
        to: f32,
        pointer: gpui::Point<gpui::Pixels>,
    ) {
        let scroll = &self.preview.reader.scroll;
        if !scroll.continuous {
            let before = crate::preview::Lay {
                own: opened.1,
                zoom,
                fit_width,
                shown_as: crate::preview::ShownAs::Page,
            };
            return crate::preview::zoom_toward(&scroll.single, before, to, pointer);
        }
        // The view, not the column scrolled sideways inside it.
        let at = pointer - scroll.sideways.bounds().origin;
        let at = (f32::from(at.x), f32::from(at.y));
        let anchor = match scroll.zoom_anchor.take() {
            Some(anchor) => Some(ZoomAnchor { at, ..anchor }),
            None => scroll
                .page_under(pointer, opened)
                .map(|(page, share)| ZoomAnchor { page, share, at }),
        };
        scroll.zoom_anchor.set(anchor);
    }

    pub(crate) fn clear_document_pages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scroll = &mut self.preview.reader.scroll;
        scroll.cancel_all();
        // Render failures describe pages that no longer exist here.
        scroll.errors = Rc::default();
        scroll.refused.clear();
        for page in std::mem::take(&mut scroll.pages).values() {
            cx.drop_image(page.image.clone(), Some(window));
        }
        for part in std::mem::take(&mut scroll.parts).values() {
            cx.drop_image(part.image.clone(), Some(window));
        }
        scroll.seen = Rc::default();
    }

    /// Let the reader go, with its pages and the page the panel shows of
    /// its document, which the next reader has no column for. The next
    /// one reads the way the user last chose (`reading.continuous`) from
    /// the start: what stands for its first page and the page itself are
    /// drawn that way, never a page at a time first.
    pub(crate) fn let_reader_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_document_pages(window, cx);
        if matches!(
            &self.preview.content,
            Some(PreviewContent::Image {
                document: Some(_),
                ..
            })
        ) {
            self.replace_preview_content(None, window, cx);
        }
        self.preview.reader = crate::reader::ReaderState::new(
            crate::settings::reader_mode(self),
            crate::settings::reader_drags(self),
        );
    }

    /// An explicit visit renders a failed page again.
    pub(crate) fn reader_retry_page(&mut self, number: u32) {
        if self.preview.reader.scroll.forget_errors(Some(number)) {
            self.request_document_page();
        }
    }

    /// A manual refresh retries every failed page of the continuous document.
    pub(crate) fn reader_retry_pages(&mut self) {
        if self.preview.reader.scroll.forget_errors(None) {
            self.request_document_page();
        }
    }

    fn document_viewport(
        &mut self,
        wanted: Vec<u32>,
        current: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.preview.reader.scroll.continuous {
            return;
        }
        let changed = self.preview.page != current;
        self.preview.page = current;
        let scroll = &mut self.preview.reader.scroll;
        if scroll
            .pending
            .as_ref()
            .is_some_and(|(p, _)| !wanted.contains(p))
        {
            scroll.cancel();
        }
        if changed || scroll.wanted != wanted {
            tracing::trace!(page = current, requested = ?wanted, cached = scroll.pages.len(), "document viewport");
        }
        scroll.wanted = wanted;
        for page in scroll.evict_offscreen(current) {
            cx.drop_image(page.image, Some(window));
        }
        self.request_document_page();
        if changed {
            cx.notify();
        }
    }

    /// Pages were shown at `shown` (`report_shown`): render the next ones
    /// at the side they cover, and draw again the ones shown larger than
    /// they were rendered. A smaller side leaves drawn pages as they are.
    pub(crate) fn document_shown(
        &mut self,
        shown: crate::elements::document::Shown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preview.reader.shown_scale = Some(shown.scale);
        let side = page_side(shown.covered);
        if self.preview.page_side == Some(side) {
            return;
        }
        let grew = self.preview.page_side.is_none_or(|known| side > known);
        self.preview.page_side = Some(side);
        tracing::debug!(side, "document pages rendered at");
        if !grew || self.preview.reader.session.is_none() {
            return;
        }
        if self.preview.reader.scroll.continuous {
            self.request_document_page();
        } else if self.preview.reader.shown_side
            < self
                .preview
                .reader
                .scroll
                .whole_side(self.preview.page, side)
        {
            // The page shown is drawn again, where it is.
            self.cancel_pending_preview();
            self.preview.refresh = true;
            self.preview.phase = crate::preview::PreviewPhase::Queued;
            cx.notify();
        }
    }

    /// What of the pages showed (`report_seen`): what is drawn apart of
    /// them is chosen again (`want_part`, ADR-147).
    pub(crate) fn document_parts(
        &mut self,
        seen: Vec<crate::elements::document::Seen>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preview.reader.scroll.seen = Rc::new(seen);
        self.want_part(window, cx);
    }

    /// Choose again what of the pages is drawn apart, and have it drawn
    /// (`DocumentScroll::choose_part`): the parts let go leave the atlas.
    /// Called whenever what shows changes, a part comes back or fails, and
    /// a search a part waited for ends.
    fn want_part(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let searching = self.preview.reader.search.is_pending();
        let draws = self
            .preview
            .reader
            .session
            .as_ref()
            .is_some_and(|session| session.draws_parts());
        let (let_go, chosen) = self.preview.reader.scroll.choose_part(searching, draws);
        for part in let_go {
            cx.drop_image(part.image, Some(window));
        }
        if let Some((number, part, side)) = chosen {
            self.request_document_part(number, part, side);
        }
    }

    /// Draw `part` of page `number` at `side`, apart from the page whole.
    fn request_document_part(&mut self, number: u32, part: skwir_core::document::Part, side: u32) {
        let Some(session) = self.preview.reader.session.clone() else {
            return;
        };
        let scroll = &mut self.preview.reader.scroll;
        let cancel = self.preview.reader.lifetime.child();
        tracing::debug!(page = number, side, ?part, "draw what shows of a page");
        scroll.part_serial = scroll.part_serial.wrapping_add(1);
        let serial = scroll.part_serial;
        scroll.part_pending = Some(PendingPart {
            page: number,
            part,
            side,
            serial,
            cancel: cancel.clone(),
        });
        let (tx, wake) = (scroll.tx.clone(), self.waker());
        self.app.scheduler.spawn(
            Priority::Visible,
            "document-page-part",
            cancel.clone(),
            move || {
                let result = session
                    .render_part(number, side, part, &cancel)
                    .and_then(|page| {
                        // A part asked and none said is no part: shown as
                        // one, the page whole would be squeezed into it.
                        let drawn = page
                            .part
                            .ok_or_else(|| anyhow::anyhow!("the part drawn was not said"))?;
                        let bytes = page.pixels.len();
                        let image = crate::thumbs::page_image(
                            page.width,
                            page.height,
                            page.pixels,
                            page.order,
                        )
                        .ok_or_else(|| anyhow::anyhow!("invalid document bitmap"))?;
                        Ok(CachedPage {
                            image,
                            content: Arc::new(Page::new(page.content)),
                            bytes,
                            side,
                            part: Some(drawn),
                        })
                    })
                    .map_err(Words::of_error);
                if !cancel.is_cancelled() {
                    let _ = tx.send(PageResult {
                        generation: serial,
                        number,
                        result,
                        part: true,
                    });
                    wake();
                }
            },
        );
    }

    fn request_document_page(&mut self) {
        let Some(session) = self.preview.reader.session.clone() else {
            return;
        };
        // Known since the page the session came with (`preview_arrived`).
        let Some(side) = self.preview.page_side else {
            return;
        };
        let initial = match &self.preview.content {
            Some(PreviewContent::Image {
                document: Some(page),
                ..
            }) => Some(page.page),
            _ => None,
        };
        // The page the preview opened on is drawn from its content until
        // it is shown larger than it was rendered.
        let initial = initial.filter(|page| {
            self.preview.reader.shown_side >= self.preview.reader.scroll.whole_side(*page, side)
        });
        let scroll = &mut self.preview.reader.scroll;
        if scroll.pending.is_some() {
            return;
        }
        let Some(number) = scroll.wanted.iter().copied().find(|p| {
            Some(*p) != initial
                && scroll
                    .pages
                    .get(p)
                    .is_none_or(|cached| cached.side < scroll.whole_side(*p, side))
                && !scroll.errors.contains_key(p)
        }) else {
            return;
        };
        // As much as it may hold whole; what shows past that is drawn apart.
        let side = scroll.whole_side(number, side);
        // Speculative neighbours must not interrupt an explicitly started find.
        // A missing current page still has priority over background searching.
        if self.preview.reader.search.is_pending() && number != self.preview.page {
            return;
        }
        self.preview.reader.cancel_search();
        let scroll = &mut self.preview.reader.scroll;
        let cancel = self.preview.reader.lifetime.child();
        tracing::trace!(page = number, "render scrolling document page");
        scroll.pending = Some((number, cancel.clone()));
        let generation = scroll.generation;
        let tx = scroll.tx.clone();
        let wake = self.waker();
        self.app.scheduler.spawn(
            Priority::Visible,
            "document-scroll-page",
            cancel.clone(),
            move || {
                let result = session
                    .render(number, side, &cancel)
                    .and_then(|page| {
                        let bytes = page.pixels.len();
                        let image = crate::thumbs::page_image(
                            page.width,
                            page.height,
                            page.pixels,
                            page.order,
                        )
                        .ok_or_else(|| {
                            anyhow::Error::from(skwir_core::words!(
                                reader_invalid_bitmap = "invalid document bitmap",
                                note = "Drawn in place of a document's page whose picture the reader gave cannot be shown."
                            ))
                        })?;
                        Ok(CachedPage {
                            image,
                            content: Arc::new(Page::new(page.content)),
                            bytes,
                            side,
                            part: None,
                        })
                    })
                    .map_err(Words::of_error);
                if !cancel.is_cancelled() {
                    let _ = tx.send(PageResult {
                        generation,
                        number,
                        result,
                        part: false,
                    });
                    wake();
                }
            },
        );
    }

    pub fn sync_document_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Ok(reply) = self.preview.reader.scroll.rx.try_recv() {
            let scroll = &mut self.preview.reader.scroll;
            // What shows of a page, drawn sharper: over the page, while it
            // is wanted; a failure leaves the page as it was drawn, and its
            // part is not asked again. Only the part last asked is taken,
            // and what to draw apart is chosen again as it comes.
            if reply.part {
                if scroll
                    .part_pending
                    .as_ref()
                    .is_none_or(|pending| pending.serial != reply.generation)
                {
                    continue;
                }
                scroll.part_pending = None;
                let seen = scroll
                    .seen
                    .iter()
                    .rev()
                    .find(|seen| seen.page == reply.number)
                    .copied();
                match reply.result {
                    // An engine with no more pixels than the whole holds (a
                    // picture at its own size) drew it no sharper: not kept,
                    // and not asked again.
                    Ok(part) if seen.is_some_and(|seen| !sharper(&seen, &part)) => {
                        tracing::debug!(page = reply.number, "part no sharper than the whole");
                        scroll.refused.insert(reply.number);
                    }
                    Ok(part) => {
                        if let Some(old) =
                            Rc::make_mut(&mut scroll.parts).insert(reply.number, part)
                        {
                            cx.drop_image(old.image, Some(window));
                        }
                        cx.notify();
                    }
                    Err(error) => {
                        tracing::debug!(page = reply.number, "part not drawn: {}", error.english());
                        scroll.refused.insert(reply.number);
                    }
                }
                self.want_part(window, cx);
                continue;
            }
            if reply.generation != scroll.generation {
                continue;
            }
            scroll.pending = None;
            // Read a page at a time, the page turned to while it was read
            // ahead is shown as it comes (`turn_from_kept`).
            let (shown, showing) = match &self.preview.content {
                Some(PreviewContent::Image {
                    image,
                    document: Some(page),
                    ..
                }) => (Some(page.page), Some(Arc::clone(image))),
                _ => (None, None),
            };
            let turned_to = !scroll.continuous
                && reply.number == self.preview.page
                && shown.is_some_and(|shown| shown != reply.number);
            match reply.result {
                Ok(page) => {
                    let size = (page.content.width, page.content.height);
                    let assumed = Rc::make_mut(&mut scroll.sizes).insert(reply.number, size);
                    scroll.widest = scroll.widest.max(size.0);
                    // The page it replaces stays in the atlas while the
                    // panel shows it (a page at a time).
                    if let Some(old) = Rc::make_mut(&mut scroll.pages).insert(reply.number, page)
                        && !showing
                            .as_ref()
                            .is_some_and(|shown| Arc::ptr_eq(shown, &old.image))
                    {
                        cx.drop_image(old.image, Some(window));
                    }
                    if turned_to && let Some(kept) = scroll.pages.get(&reply.number).cloned() {
                        self.show_kept(reply.number, kept, window, cx);
                        continue;
                    }
                    if !scroll.continuous {
                        self.request_document_page();
                        continue;
                    }
                    // The placeholder stood at the size we assumed for this
                    // page. If the real page is that size, its row does not
                    // change and there is nothing to measure again. If it is
                    // not, remeasure that row alone — never splice it: a
                    // spliced item forgets its height, and a document whose
                    // total height drops every time a page arrives has a
                    // scrollbar that jumps under the hand dragging it.
                    if assumed != Some(size) {
                        scroll
                            .list
                            .remeasure_items(reply.number as usize - 1..reply.number as usize);
                    }
                }
                Err(error) => {
                    Rc::make_mut(&mut scroll.errors).insert(reply.number, error);
                    if turned_to {
                        // The page turned to is asked as any page is, and
                        // its failure said as any page's.
                        self.preview.phase = crate::preview::PreviewPhase::Queued;
                        cx.notify();
                        continue;
                    }
                    if !scroll.continuous {
                        self.request_document_page();
                        continue;
                    }
                }
            }
            self.request_document_page();
            cx.notify();
        }
        // A part that waited for a search is asked once it ended.
        if self.preview.reader.scroll.part_waits && !self.preview.reader.search.is_pending() {
            self.want_part(window, cx);
        }
    }

    /// Read a page at a time, the page the panel shows of its document
    /// (`preview.content`) is kept with the others the reader read, at the
    /// side it was rendered at, and the pages after and before it are read
    /// ahead: a turn to either shows it at once (`turn_from_kept`).
    pub(crate) fn keep_shown_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Read either way, the shape of the page shown is known from now
        // on, for it and the pages near it drawn whole (`whole_side`).
        if let Some(PreviewContent::Image {
            document: Some(page),
            ..
        }) = &self.preview.content
        {
            let size = (page.width, page.height);
            let scroll = &mut self.preview.reader.scroll;
            if scroll.sizes.get(&page.page) != Some(&size) {
                Rc::make_mut(&mut scroll.sizes).insert(page.page, size);
                scroll.widest = scroll.widest.max(size.0);
            }
        }
        let reader = &self.preview.reader;
        if reader.scroll.continuous || reader.session.is_none() {
            return;
        }
        let Some(PreviewContent::Image {
            image,
            document: Some(page),
            ..
        }) = &self.preview.content
        else {
            return;
        };
        let side = reader.shown_side;
        if page.page == self.preview.page && reader.scroll.kept(page.page, side).is_none() {
            let pixels = image.size(0);
            let kept = CachedPage {
                image: Arc::clone(image),
                content: Arc::clone(page),
                bytes: (pixels.width.0.max(0) as usize) * (pixels.height.0.max(0) as usize) * 4,
                side,
                part: None,
            };
            let scroll = &mut self.preview.reader.scroll;
            if let Some(old) = Rc::make_mut(&mut scroll.pages).insert(kept.content.page, kept) {
                cx.drop_image(old.image, Some(window));
            }
        }
        self.read_pages_ahead(window, cx);
    }

    /// Read a page at a time: have the page shown and the ones after and
    /// before it, those far from it let go of past the cache's budget.
    fn read_pages_ahead(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.preview.reader.session else {
            return;
        };
        let count = crate::reader::page_count(session.as_ref());
        let page = self.preview.page;
        let scroll = &mut self.preview.reader.scroll;
        scroll.wanted = single_wanted(page, count);
        for gone in scroll.evict_offscreen(page) {
            cx.drop_image(gone.image, Some(window));
        }
        self.request_document_page();
    }

    /// Read a page at a time, the page turned to (`preview.page`): shown at
    /// once when it is kept, or as it comes when it is being read ahead.
    /// Whether it was: no render is asked for it then. Anything else read
    /// ahead gives way to the render asked for it.
    pub(crate) fn turn_from_kept(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let reader = &self.preview.reader;
        if reader.scroll.continuous
            || reader.session.is_none()
            || !matches!(
                &self.preview.content,
                Some(PreviewContent::Image {
                    document: Some(_),
                    ..
                })
            )
        {
            return false;
        }
        let page = self.preview.page;
        if let Some(kept) = self
            .preview
            .page_side
            .and_then(|side| reader.scroll.kept(page, side))
            .cloned()
        {
            self.show_kept(page, kept, window, cx);
            return true;
        }
        if reader
            .scroll
            .pending
            .as_ref()
            .is_some_and(|(pending, _)| *pending == page)
        {
            self.preview.phase = crate::preview::PreviewPhase::Loading;
            return true;
        }
        self.preview.reader.scroll.cancel();
        false
    }

    /// Show page `page` as it was kept, as if it had just been rendered.
    fn show_kept(
        &mut self,
        page: u32,
        kept: CachedPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &self.preview.reader.session else {
            return;
        };
        let pages = crate::reader::page_count(session.as_ref());
        tracing::trace!(page, side = kept.side, "page shown as kept");
        self.cancel_pending_preview();
        let size = kept.image.size(0);
        self.preview.reader.shown_side = kept.side;
        self.replace_preview_content(
            Some(PreviewContent::Image {
                image: kept.image,
                width: u32::try_from(size.width.0).unwrap_or(0),
                height: u32::try_from(size.height.0).unwrap_or(0),
                detail: Some(skwir_core::preview::PreviewInfo::Document { page, pages }),
                document: Some(kept.content),
                facts: Vec::new(),
                pixels: crate::preview::Pixels::Own,
                ink: None,
            }),
            window,
            cx,
        );
        self.preview.phase = crate::preview::PreviewPhase::Idle;
        let reader = &mut self.preview.reader;
        reader.selection = None;
        reader.selection_page = None;
        reader.down = None;
        if std::mem::take(&mut reader.scroll.bottom_on_load) {
            reader.scroll.single.scroll_to_bottom();
        }
        self.read_pages_ahead(window, cx);
        cx.notify();
    }
}

pub fn continuous_element(
    shell: &Shell,
    image: Arc<RenderImage>,
    page: Arc<Page>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let reader = &shell.preview.reader;
    let scroll = &reader.scroll;
    // The page the preview opened on stands beside the cache: it is drawn
    // from the preview's content, without a copy of the cache to hold it.
    let pages = Rc::clone(&scroll.pages);
    let sizes = Rc::clone(&scroll.sizes);
    let fallback = (page.width, page.height);
    let widest = scroll.widest.max(page.width);
    let errors = Rc::clone(&scroll.errors);
    let list = scroll.list.clone();
    let after_list = list.clone();
    let bar_list = list.clone();
    let wheel_list = list.clone();
    let pan_list = list.clone();
    let sideways = scroll.sideways.clone();
    let (grab, drag_moves) = (shell.preview.grab.clone(), reader.drag_moves);
    let pan_grab = grab.clone();
    let count = list.item_count() as u32;
    let layout = scroll.layout.clone();
    let zoom_anchor = scroll.zoom_anchor.clone();
    let requested = Rc::new(RefCell::new(Vec::new()));
    let seen = requested.clone();
    // What the rows cover, read once they are laid out, and what of them
    // shows (ADR-147).
    let report = crate::elements::document::Report::default();
    let row_report = report.clone();
    let visible = crate::elements::document::Visible::default();
    let row_visible = visible.clone();
    let parts = Rc::clone(&scroll.parts);
    let last_seen = Rc::clone(&scroll.seen);
    let (side, scale) = (shell.preview.page_side, reader.shown_scale);
    let entity = cx.entity().downgrade();
    let after_entity = entity.clone();
    let wheel_entity = entity.clone();
    let lifetime = reader.lifetime.clone();
    let zoom = shell.preview.zoom;
    let fit_width = shell.preview.fit_width;
    let selection_page = reader.selection_page;
    let selection = reader.selection;
    let hit = reader.active_hit.and_then(|i| reader.hits.get(i)).cloned();
    let theme = cx.global::<Theme>().clone();
    let viewport = crate::elements::document::DocumentViewport {
        build: Box::new(move |bounds, _, _| {
            let width = f32::from(bounds.width).max(1.);
            let height = f32::from(bounds.height).max(1.);
            // A page that has not been drawn is worth about as much as the
            // one we know: without that, the scrollbar would measure the
            // handful of pages on screen and call the book nearly read.
            let estimate =
                page_scale(zoom, fit_width, (width, height), fallback) * fallback.1 + PAGE_GAP;
            let content_width = zoom.map_or(width, |z| width.max(widest * z));
            let opened = page.page;
            let size_of = move |number: u32| {
                if number == opened {
                    fallback
                } else {
                    sizes.get(&number).copied().unwrap_or(fallback)
                }
            };
            // Ctrl and the wheel zoomed here: what was under the pointer
            // lands back there, at the scales this layout gives the pages.
            let anchor = zoom_anchor.take();
            let signature = (bounds, zoom, fit_width);
            if layout.borrow().as_ref() != Some(&signature) {
                let offset = list.logical_scroll_top();
                list.reset_with_uniform_height(list.item_count(), px(estimate.max(1.)));
                match anchor {
                    Some(anchor) => {
                        let shown = |number| {
                            let (across, down) = size_of(number);
                            let scale =
                                page_scale(zoom, fit_width, (width, height), (across, down));
                            (across * scale, down * scale)
                        };
                        let ((item_ix, down), x) = landing(anchor, shown, content_width, width);
                        list.scroll_to(ListOffset {
                            item_ix,
                            offset_in_item: px(down),
                        });
                        sideways.set_offset(point(px(x), px(0.)));
                    }
                    None => list.scroll_to(offset),
                }
                *layout.borrow_mut() = Some(signature);
            }
            let rows = gpui::list(list, move |index, _, _| {
                let number = index as u32 + 1;
                seen.borrow_mut().push(number);
                let (pw, ph) = size_of(number);
                let scale = page_scale(zoom, fit_width, (width, height), (pw, ph));
                let row_height = (ph * scale).max(1.);
                let mut row = div()
                    .id(("continuous-page", index))
                    .w(px(content_width))
                    .h(px(row_height + PAGE_GAP))
                    .pb(px(PAGE_GAP))
                    .flex()
                    .justify_center();
                // A page drawn again sharper replaces the one it opened on.
                let shown = pages
                    .get(&number)
                    .map(|cached| (cached.image.clone(), cached.content.clone()))
                    .or_else(|| (number == page.page).then(|| (image.clone(), page.clone())));
                if let Some((image, content)) = shown {
                    row = row.child(div().w(px(pw * scale)).h(px(row_height)).child(
                        crate::reader::surface(
                            image,
                            content,
                            None,
                            false,
                            selection_page,
                            selection,
                            hit.clone(),
                            &theme,
                            entity.clone(),
                            Some(row_report.clone()),
                            (&grab, drag_moves),
                            (
                                Some(row_visible.clone()),
                                parts.get(&number).and_then(|cached| {
                                    Some((cached.image.clone(), cached.part?))
                                }),
                            ),
                        ),
                    ));
                } else {
                    row = row.child(
                        div()
                            .w(px(pw * scale))
                            .h(px(row_height))
                            .bg(theme.surface)
                            .text_color(theme.text_muted)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                errors.get(&number).map_or_else(
                                    || {
                                        skwir_core::words!(
                                            reader_page_waits = "Page { $number }…",
                                            number = number,
                                            note = "Drawn in place of a document's page while it is drawn. number: the page's."
                                        )
                                        .say()
                                    },
                                    Words::say,
                                ),
                            ),
                    );
                }
                row.into_any_element()
            })
            .w(px(content_width))
            .h(bounds.height);
            let bar = crate::elements::scrollbar::vertical(
                "document-scrollbar",
                crate::elements::scrollbar::Scrolls::Items(bar_list.clone()),
            )
            .across(sideways.clone());
            let across = crate::elements::scrollbar::horizontal(
                "document-scrollbar-sideways",
                sideways.clone(),
            )
            .across(crate::elements::scrollbar::Scrolls::Items(bar_list));
            // The pages are the document's, left to right whatever the
            // window's direction (`sides`).
            div()
                .relative()
                .size_full()
                .left_to_right()
                // Under the pages: a drag beside or between them, or across
                // one that drags, moves the column (ADR-143).
                .child(crate::elements::pan::layer(
                    &pan_grab,
                    crate::elements::pan::Moves::Column {
                        list: pan_list.clone(),
                        across: sideways.clone(),
                    },
                    true,
                ))
                .child(
                    div()
                        .id("document-horizontal-scroll")
                        .size_full()
                        .overflow_x_scroll()
                        .track_scroll(&sideways)
                        .child(rows),
                )
                .child(bar)
                .child(across)
                .child(wheel_steps(wheel_list, wheel_entity))
                .into_any_element()
        }),
        laid_out: Box::new(move |window, cx| {
            let mut wanted = requested.borrow().clone();
            let center = f32::from(after_list.viewport_bounds().center().y);
            let current = page_at_center(
                center,
                wanted.iter().filter_map(|number| {
                    after_list
                        .bounds_for_item(*number as usize - 1)
                        .map(|b| (*number, f32::from(b.top())..f32::from(b.bottom())))
                }),
            )
            .unwrap_or((after_list.logical_scroll_top().item_ix as u32 + 1).min(count));
            wanted = wanted_pages(wanted, current, count);
            report_shown(&report, side, scale, after_entity.clone(), window, cx);
            report_seen(&visible, &last_seen, after_entity.clone(), window, cx);
            window.defer(cx, move |window, cx| {
                if !lifetime.is_cancelled()
                    && let Some(entity) = after_entity.upgrade()
                {
                    entity.update(cx, |shell, cx| {
                        shell.document_viewport(wanted, current, window, cx);
                    });
                }
            });
        }),
    };
    crate::reader::expose(div().id("reader-pages"), shell)
        .flex_1()
        .min_h_0()
        .w_full()
        .when_some(reader.focus.as_ref(), |d, focus| {
            d.track_focus(focus).key_context(crate::keymap::READER)
        })
        .child(viewport)
        .into_any_element()
}

fn page_at_center(
    center: f32,
    pages: impl Iterator<Item = (u32, std::ops::Range<f32>)>,
) -> Option<u32> {
    pages
        .filter(|(_, bounds)| bounds.contains(&center))
        .map(|(number, _)| number)
        .next()
}

/// The pages to have read a page at a time, `page` shown of `count`: it,
/// then the one after and the one before, read ahead.
fn single_wanted(page: u32, count: u32) -> Vec<u32> {
    [Some(page), page.checked_add(1), page.checked_sub(1)]
        .into_iter()
        .flatten()
        .filter(|page| (1..=count).contains(page))
        .collect()
}

// Measurement can include an overdraw row only on the first frame. Keep
// neighbours explicitly so a later measurement cannot cancel their prefetch.
fn wanted_pages(mut visible: Vec<u32>, current: u32, count: u32) -> Vec<u32> {
    visible.extend(current.saturating_sub(1).max(1)..=current.saturating_add(1).min(count));
    visible.retain(|p| *p > 0 && *p <= count);
    visible.sort_unstable();
    visible.dedup();
    visible.sort_by_key(|p| p.abs_diff(current));
    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_page_ignores_a_sliver_of_the_preceding_page() {
        assert_eq!(
            page_at_center(400., [(23, -700.0..0.01), (24, 0.01..800.0)].into_iter()),
            Some(24)
        );
        assert_eq!(
            page_at_center(400., [(1, 0.0..500.0), (2, 512.0..1200.0)].into_iter()),
            Some(1)
        );
    }

    #[test]
    fn pages_are_rendered_at_the_step_they_cover() {
        use skwir_core::document::MAX_PART_SIDE;
        // A little short of a step is that step; past it, the next.
        assert_eq!(page_side(426.), 512);
        assert_eq!(page_side(1067.), 1024);
        assert_eq!(page_side(1100.), 1536);
        assert_eq!(page_side(0.), 512);
        assert_eq!(page_side(f32::NAN), 512);
        // Past what a page may hold whole: the side what shows of it is
        // drawn at (ADR-147).
        assert_eq!(page_side(10_000.), 9728);
        assert_eq!(page_side(100_000.), MAX_PART_SIDE);
        // A page whole is held as much as its shape may hold.
        let mut state = DocumentScroll::default();
        assert_eq!(state.whole_side(1, 9728), 4096, "an A4 page, unknown yet");
        // A webtoon's: the pages not seen yet are counted as the nearest
        // known, never as ISO's.
        Rc::make_mut(&mut state.sizes).insert(1, (800., 12_000.));
        let tall = state.whole_side(1, 30_208);
        assert!(tall > 13_000, "{tall}");
        assert_eq!(state.whole_side(5, 30_208), tall);
        Rc::make_mut(&mut state.sizes).insert(9, (600., 800.));
        assert_eq!(state.whole_side(10, 30_208), 4096);
        assert_eq!(state.whole_side(4, 30_208), tall, "the nearest before");
    }

    /// A page covering more than its pixels drawn whole hold has what shows
    /// of it drawn apart, with a margin of half as much each way where a
    /// page may hold it, at the side it covers; asked once while what was
    /// drawn or asked holds it (ADR-147).
    #[test]
    fn what_shows_of_a_page_zoomed_past_its_whole_is_drawn_apart() {
        use crate::elements::document::Seen;
        use skwir_core::document::{PAGE_PIXELS, Part};
        let seen = Seen {
            page: 3,
            own: (816., 1056.),
            covered: 8000.,
            held: 4096,
            shown: Part {
                left: 0.4,
                top: 0.4,
                right: 0.6,
                bottom: 0.5,
            },
        };
        let (part, side) = part_wanted(&seen, None).expect("a part");
        assert_eq!(side, 7680);
        // Half as much again each way.
        assert!(
            (part.left - 0.3).abs() < 1e-4 && (part.right - 0.7).abs() < 1e-4,
            "{part:?}"
        );
        assert!(
            (part.top - 0.35).abs() < 1e-4 && (part.bottom - 0.55).abs() < 1e-4,
            "{part:?}"
        );
        // Held: not asked again; zoomed further, asked anew.
        assert_eq!(part_wanted(&seen, Some((part, side))), None);
        let further = Seen {
            covered: 12_000.,
            ..seen
        };
        assert!(part_wanted(&further, Some((part, side))).is_some());
        // A part missing what shows by less than half a pixel at its side
        // holds it (asked again, it would come back the same); by more, not.
        let pixel = 1. / (7680. * 816. / 1056.);
        let short = |by: f32| Part {
            left: seen.shown.left + by * pixel,
            ..seen.shown
        };
        assert_eq!(part_wanted(&seen, Some((short(0.3), side))), None);
        assert!(part_wanted(&seen, Some((short(2.), side))).is_some());
        // Zoomed out to less than half the side it was drawn at: asked
        // again at its own.
        let (large, large_side) = part_wanted(&further, None).unwrap();
        let out = Seen {
            covered: 5000.,
            ..seen
        };
        assert!(large_side > 2 * page_side(5000.));
        assert!(part_wanted(&out, Some((large, large_side))).is_some());
        // Within what the whole holds: none.
        let fits = Seen {
            covered: 4200.,
            ..seen
        };
        assert_eq!(part_wanted(&fits, None), None);
        // A tall page whose whole, drawn at that side, holds it: drawn
        // whole again, no part.
        let tall = Seen {
            own: (533., 8000.),
            covered: 7600.,
            held: 2048,
            ..seen
        };
        assert_eq!(part_wanted(&tall, None), None);
        // What shows larger than a page may hold: a smaller side, as much
        // of a margin as is left, within the bound.
        let wall = Seen {
            covered: 30_000.,
            shown: Part {
                left: 0.2,
                top: 0.2,
                right: 0.8,
                bottom: 0.8,
            },
            ..seen
        };
        let (part, side) = part_wanted(&wall, None).unwrap();
        assert!(part.holds(&wall.shown), "{part:?}");
        assert!(side < 28_672 && side as f32 > 4096. * 1.05, "{side}");
        assert!(part.most_pixels(wall.own, side) <= PAGE_PIXELS);
        // At that side, the part drawn and asked again: held.
        assert_eq!(part_wanted(&wall, Some((part, side))), None);
        // The whole page showing at that zoom: no part sharper than the
        // whole, none.
        let whole = Seen {
            shown: Part {
                left: 0.,
                top: 0.,
                right: 1.,
                bottom: 1.,
            },
            ..wall
        };
        assert_eq!(part_wanted(&whole, None), None);
    }

    /// What is drawn apart is chosen again whenever something changes, one
    /// part at a time (ADR-147): the second page in view has its part once
    /// the first's came back; a part being drawn for a view left gives way
    /// to the view's, its answer let go; one still holding what shows is
    /// waited for; a page gone or zoomed out lets its part go; a page whose
    /// part failed is not asked again; a search running holds parts back.
    /// The part asked before the view moved was all that was ever drawn.
    #[test]
    fn what_is_drawn_apart_is_chosen_again_as_things_change() {
        use crate::elements::document::Seen;
        use skwir_core::document::Part;
        let seen = |page: u32, top: f32, bottom: f32| Seen {
            page,
            own: (816., 1056.),
            covered: 8000.,
            held: 4096,
            shown: Part {
                left: 0.3,
                top,
                right: 0.7,
                bottom,
            },
        };
        let image = || crate::thumbs::owned_render_image(1, 1, vec![0; 4]).unwrap();
        let drawn = |page: u32, part: Part, side: u32| CachedPage {
            image: image(),
            content: Arc::new(Page::new(Arc::new(skwir_core::document::PageContent {
                page,
                width: 816.,
                height: 1056.,
                chars: Vec::new(),
                links: Vec::new(),
            }))),
            bytes: 4,
            side,
            part: Some(part),
        };
        let ask = |state: &mut DocumentScroll, page: u32, part: Part, side: u32| {
            let cancel = CancelToken::new();
            state.part_serial += 1;
            state.part_pending = Some(PendingPart {
                page,
                part,
                side,
                serial: state.part_serial,
                cancel: cancel.clone(),
            });
            cancel
        };
        let mut state = DocumentScroll::new(true);
        // The bottom of page 3, the top of page 4: page 3 shows most.
        state.seen = Rc::new(vec![seen(3, 0.6, 1.), seen(4, 0., 0.3)]);
        // A session that draws no part sharper than its whole is asked none.
        assert_eq!(state.choose_part(false, false).1, None);
        let (gone, chosen) = state.choose_part(false, true);
        assert!(gone.is_empty());
        let (page, part, side) = chosen.expect("a part");
        assert_eq!(page, 3);
        let cancel = ask(&mut state, page, part, side);
        // Nothing moved: what is being drawn is waited for.
        assert_eq!(state.choose_part(false, true).1, None);
        assert!(!cancel.is_cancelled());
        // It came back: page 4 has its own at once, with nothing moving.
        state.part_pending = None;
        Rc::make_mut(&mut state.parts).insert(3, drawn(3, part, side));
        let (page, part, side) = state.choose_part(false, true).1.expect("the second page's");
        assert_eq!(page, 4);
        let cancel = ask(&mut state, page, part, side);
        // The view moves down past what is being drawn: it gives way to
        // the view's, and page 3, gone, lets its part go.
        state.seen = Rc::new(vec![seen(4, 0.5, 0.8)]);
        let (gone, chosen) = state.choose_part(false, true);
        assert!(cancel.is_cancelled() && state.part_pending.is_none());
        assert_eq!(gone.len(), 1);
        assert!(state.parts.is_empty());
        let (page, part, side) = chosen.expect("what shows now");
        assert_eq!(page, 4);
        assert!(part.holds(&seen(4, 0.5, 0.8).shown));
        // A failure: not asked again until the page is visited.
        state.refused.insert(4);
        assert_eq!(state.choose_part(false, true).1, None);
        state.forget_errors(Some(4));
        assert!(state.refused.is_empty());
        // Zoomed out to where the whole suffices: the part goes.
        Rc::make_mut(&mut state.parts).insert(4, drawn(4, part, side));
        state.seen = Rc::new(vec![Seen {
            covered: 4000.,
            ..seen(4, 0.5, 0.8)
        }]);
        let (gone, chosen) = state.choose_part(false, true);
        assert_eq!((gone.len(), chosen), (1, None));
        // A search running holds parts back, until it ends.
        state.seen = Rc::new(vec![seen(4, 0.5, 0.8)]);
        assert_eq!(state.choose_part(true, true).1, None);
        assert!(state.part_waits);
        assert!(state.choose_part(false, true).1.is_some());
        assert!(!state.part_waits);
        // A layout tried twice in a frame said a page twice: the last holds.
        state.seen = Rc::new(vec![seen(4, 0., 0.1), seen(4, 0.5, 0.8)]);
        let (_, part, _) = state.choose_part(false, true).1.unwrap();
        assert!(part.holds(&seen(4, 0.5, 0.8).shown) && part.top > 0.2);
        // The whole pages given up leave the part being drawn alone.
        let cancel = ask(&mut state, 4, part, side);
        state.cancel();
        assert!(!cancel.is_cancelled());
        state.cancel_all();
        assert!(cancel.is_cancelled());
    }

    /// A part drawn with no more pixels than the whole shows over the same
    /// share (a comic's page at its own size, a drawing's whole cut) is no
    /// sharper: the reader keeps it not and asks it no more (ADR-147).
    #[test]
    fn a_part_no_sharper_than_the_whole_is_not_kept() {
        use crate::elements::document::Seen;
        use skwir_core::document::Part;
        let part = |width: u32, height: u32, at: Part| CachedPage {
            image: crate::thumbs::owned_render_image(
                width,
                height,
                vec![0; width as usize * height as usize * 4],
            )
            .unwrap(),
            content: Arc::new(Page::new(Arc::new(skwir_core::document::PageContent {
                page: 1,
                width: 1.,
                height: 1.,
                chars: Vec::new(),
                links: Vec::new(),
            }))),
            bytes: 0,
            side: 7168,
            part: Some(at),
        };
        let band = Part {
            left: 0.,
            top: 0.4,
            right: 1.,
            bottom: 0.6,
        };
        // A comic's page of 1200 × 1800, held whole at its own size: its
        // part decodes at that size, 360 rows of the 1800.
        let comic = Seen {
            page: 1,
            own: (1200., 1800.),
            covered: 7200.,
            held: 1800,
            shown: band,
        };
        assert!(!sharper(&comic, &part(1200, 360, band)));
        // A PDF's page held at 4096, its part drawn at 7680: sharper.
        let pdf = Seen {
            own: (816., 1056.),
            held: 4096,
            ..comic
        };
        assert!(sharper(&pdf, &part(1187, 1536, band)));
        // A landscape page is counted along its width.
        let wide = Seen {
            own: (1056., 816.),
            held: 4096,
            ..comic
        };
        let across = Part {
            left: 0.4,
            top: 0.,
            right: 0.6,
            bottom: 1.,
        };
        assert!(sharper(&wide, &part(1536, 1187, across)));
        assert!(!sharper(&wide, &part(819, 1187, across)));
    }

    /// A part sized to as many pixels as a page may hold is drawn by every
    /// engine, however it lays the page's pixels: on a screen of two device
    /// pixels a pixel, an A4 page at 400 % had what shows of it in the
    /// middle refused by all five, its whole pixels rounding it past the
    /// bound counted by its shares alone (ADR-147).
    #[test]
    fn a_part_sized_to_the_bound_is_drawn_by_every_engine() {
        use crate::elements::document::Seen;
        use skwir_core::document::{PAGE_PIXELS, Part};
        let own = (793.7, 1122.5);
        let seen = Seen {
            page: 1,
            own,
            covered: 8980.,
            held: 4096,
            shown: Part {
                left: 0.3,
                top: 0.4,
                right: 0.703,
                bottom: 0.578,
            },
        };
        let (part, side) = part_wanted(&seen, None).expect("a part");
        assert_eq!(side, 8704);
        assert!(part.holds(&seen.shown));
        assert!(
            part.left > 0. && part.right < 1.,
            "a margin each way: {part:?}"
        );
        let scale = f64::from(side) / f64::from(own.1);
        let laid = |length: f32| f64::from(length) * scale;
        for (width, height) in [
            (laid(own.0).round(), laid(own.1).round()),
            (laid(own.0).ceil(), laid(own.1).ceil()),
        ] {
            let (width, height) = (width as u32, height as u32);
            let [_, _, w, h] = part.pixels(width, height);
            assert!(u64::from(w) * u64::from(h) <= PAGE_PIXELS, "{w}×{h}");
            assert!(part.drawn(width, height).is_ok());
        }
    }

    /// Before any page was shown, the first is rendered at the side an
    /// upright page of the usual shape covers in the content's area: as
    /// tall as the area, or as wide, never its longer side regardless.
    #[test]
    fn a_first_page_is_rendered_at_the_side_its_area_gives_it() {
        // The panel at the right: a page as tall as it.
        assert_eq!(area_side(896., 1230.), 1536);
        // A narrow one: a page as wide as it.
        assert_eq!(area_side(320., 1100.), 512);
        // The panel at the bottom, wide and short: a page as tall as it.
        assert_eq!(area_side(2500., 500.), 512);
    }

    /// A document opens on its page drawn at the smallest step first, to
    /// stand in for it, unless that is the side it is asked at.
    #[test]
    fn a_page_opened_larger_than_the_smallest_step_is_drawn_small_first() {
        assert_eq!(draft_side(512), None);
        assert_eq!(draft_side(1024), Some(512));
        assert_eq!(draft_side(skwir_core::document::MAX_SIDE), Some(512));
    }

    /// A room any measure gives: what pages' tests do not read.
    const ROOM: crate::preview::Room = crate::preview::Room {
        width: 800.,
        height: 600.,
        scale: 1.,
    };

    /// A picture's room is the area as laid out for its own preview, or a
    /// later one, in the place it is shown: waited for until that layout
    /// comes, `WAIT` at most, the last room taken then. What the area held
    /// for the preview before (a line saying why a file is not shown) is
    /// not its room, nor the panel's once the whole screen shows it.
    #[test]
    fn a_picture_takes_the_room_laid_out_for_it() {
        let none = skwir_core::cancel::CancelToken::new();
        let area = Arc::new(Area::default());
        assert_eq!(area.last_room(), None);
        let started = std::time::Instant::now();
        assert_eq!(area.room_for(1, &none), None);
        assert!(started.elapsed() >= Area::WAIT);
        let waiting = {
            let (area, none) = (Arc::clone(&area), none.clone());
            std::thread::spawn(move || area.room_for(1, &none))
        };
        std::thread::sleep(std::time::Duration::from_millis(20));
        area.measured(1, 1024, ROOM);
        assert_eq!(waiting.join().unwrap(), Some(ROOM));
        // Laid out for this preview or a later one: at once.
        let started = std::time::Instant::now();
        assert_eq!(area.room_for(1, &none), Some(ROOM));
        assert!(started.elapsed() < Area::WAIT);
        // The next preview's frame lays out the room it shows in, not the
        // strip the last one drew: the job waits for it.
        let strip = crate::preview::Room {
            height: 50.,
            ..ROOM
        };
        area.measured(1, 1024, strip);
        let waiting = {
            let (area, none) = (Arc::clone(&area), none.clone());
            std::thread::spawn(move || area.room_for(2, &none))
        };
        std::thread::sleep(std::time::Duration::from_millis(20));
        area.measured(2, 1024, ROOM);
        assert_eq!(waiting.join().unwrap(), Some(ROOM));
        // Shown elsewhere (the whole screen): laid out there first.
        let screen = crate::preview::Room {
            width: 1600.,
            height: 1000.,
            ..ROOM
        };
        area.moved();
        let waiting = {
            let (area, none) = (Arc::clone(&area), none.clone());
            std::thread::spawn(move || area.room_for(2, &none))
        };
        std::thread::sleep(std::time::Duration::from_millis(20));
        area.measured(2, 1024, screen);
        assert_eq!(waiting.join().unwrap(), Some(screen));
        assert_eq!(area.last_room(), Some(screen));
        // Never laid out for it (a window not drawn): the last room, once
        // the wait is over.
        let started = std::time::Instant::now();
        assert_eq!(area.room_for(3, &none), Some(screen));
        assert!(started.elapsed() >= Area::WAIT);
    }

    /// The job rendering a first page takes the side measured for its own
    /// preview, waiting for the frame that lays its area out; a window that
    /// draws nothing leaves it the side last measured, or the smallest.
    #[test]
    fn a_first_page_waits_for_its_own_area() {
        let none = skwir_core::cancel::CancelToken::new();
        let area = Arc::new(Area::default());
        assert_eq!(area.side(1, &none), 512);
        area.measured(1, 2048, ROOM);
        let started = std::time::Instant::now();
        assert_eq!(area.side(2, &none), 2048);
        assert!(started.elapsed() >= Area::WAIT);
        // Laid out while the job waits: not the side measured before.
        let waiting = {
            let (area, none) = (Arc::clone(&area), none.clone());
            std::thread::spawn(move || area.side(2, &none))
        };
        area.measured(2, 1024, ROOM);
        assert_eq!(waiting.join().unwrap(), 1024);
    }

    /// A later preview's area is the same area: a job whose own preview
    /// was never laid out (another followed at once) takes it without
    /// waiting, and a job cancelled while it waits stops waiting.
    #[test]
    fn a_first_page_waits_no_longer_than_its_preview_lasts() {
        let none = skwir_core::cancel::CancelToken::new();
        let area = Arc::new(Area::default());
        area.measured(5, 1536, ROOM);
        let started = std::time::Instant::now();
        assert_eq!(area.side(3, &none), 1536);
        assert!(started.elapsed() < Area::WAIT);
        let cancel = skwir_core::cancel::CancelToken::new();
        let waiting = {
            let (area, cancel) = (Arc::clone(&area), cancel.clone());
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                (area.side(6, &cancel), started.elapsed())
            })
        };
        // Before it waits or while it does: either way it stops.
        cancel.cancel();
        let (side, waited) = waiting.join().unwrap();
        assert_eq!(side, 1536);
        assert!(waited < Area::WAIT, "{waited:?}");
    }

    #[test]
    fn cache_releases_distant_pages_but_keeps_visible_pixels_and_geometry() {
        let mut state = DocumentScroll::default();
        for number in 1..=8 {
            Rc::make_mut(&mut state.pages).insert(
                number,
                CachedPage {
                    image: crate::thumbs::owned_render_image(1, 1, vec![0; 4]).unwrap(),
                    content: Arc::new(Page::new(Arc::new(skwir_core::document::PageContent {
                        page: number,
                        width: 600.,
                        height: 800.,
                        chars: Vec::new(),
                        links: Vec::new(),
                    }))),
                    // Exercise accounting with tiny synthetic buffers.
                    bytes: CACHE_BYTES / 4,
                    side: 1024,
                    part: None,
                },
            );
            Rc::make_mut(&mut state.sizes).insert(number, (600., 800.));
        }
        state.wanted = vec![4, 5];
        assert_eq!(state.evict_offscreen(4).len(), 4);
        assert_eq!(state.pages.len(), 4);
        assert!([3, 4, 5].iter().all(|p| state.pages.contains_key(p)));
        assert!([1, 7, 8].iter().all(|p| !state.pages.contains_key(p)));
        assert_eq!(state.sizes.len(), 8);
        assert!(state.evict_offscreen(4).is_empty());
    }

    /// Read a page at a time, the page shown is had first, then the ones
    /// after and before it; a page kept at the side asked or larger is
    /// shown at once, a smaller one rendered again.
    #[test]
    fn a_page_at_a_time_reads_ahead_and_turns_to_what_it_kept() {
        assert_eq!(single_wanted(5, 9), [5, 6, 4]);
        assert_eq!(single_wanted(1, 9), [1, 2], "none before the first");
        assert_eq!(single_wanted(9, 9), [9, 8], "none after the last");
        assert_eq!(single_wanted(1, 1), [1]);
        let mut state = DocumentScroll::new(false);
        let image = crate::thumbs::owned_render_image(1, 1, vec![0; 4]).unwrap();
        let page = |number, side| CachedPage {
            image: Arc::clone(&image),
            content: Arc::new(Page::new(Arc::new(skwir_core::document::PageContent {
                page: number,
                width: 600.,
                height: 800.,
                chars: Vec::new(),
                links: Vec::new(),
            }))),
            bytes: 4,
            side,
            part: None,
        };
        Rc::make_mut(&mut state.pages).insert(6, page(6, 1536));
        Rc::make_mut(&mut state.pages).insert(4, page(4, 512));
        assert!(
            state.kept(6, 1024).is_some(),
            "rendered larger: shown as it is"
        );
        assert!(state.kept(4, 1024).is_none(), "rendered smaller: again");
        assert!(state.kept(7, 512).is_none());
        assert!(state.holds(&image));
        let other = crate::thumbs::owned_render_image(1, 1, vec![0; 4]).unwrap();
        assert!(!state.holds(&other), "an image the cache does not hold");
    }

    #[test]
    fn prefetch_survives_the_list_only_measuring_visible_rows_on_later_frames() {
        assert_eq!(
            wanted_pages(vec![1, 2], 1, 100),
            wanted_pages(vec![1], 1, 100)
        );
        assert_eq!(wanted_pages(vec![100], 100, 100), vec![100, 99]);
        assert_eq!(wanted_pages(vec![4, 5, 6, 7], 5, 100), vec![5, 4, 6, 7]);
        assert_eq!(wanted_pages(vec![1], 1, 1), vec![1]);
    }

    #[test]
    fn cancelling_invalidates_late_page_results_and_cancels_the_running_job() {
        let mut state = DocumentScroll::default();
        let token = CancelToken::new();
        let old_generation = state.generation;
        state.pending = Some((2, token.clone()));
        state.cancel();
        assert!(token.is_cancelled());
        assert!(state.pending.is_none());
        assert_ne!(old_generation, state.generation);
        let token = CancelToken::new();
        state.pending = Some((3, token.clone()));
        drop(state);
        assert!(token.is_cancelled());
    }

    #[test]
    fn trackpad_accumulates_intent_before_turning() {
        use Wheeled::{Notches, Pixels};
        let mut wheel = WheelTurn::default();
        assert_eq!(wheel.turn(Pixels(-20.), false), 0);
        assert_eq!(wheel.turn(Pixels(-40.), false), 1);
        // A turn consumes the accumulated intent.
        assert_eq!(wheel.turn(Pixels(-20.), false), 0);
        // Reversing direction starts over.
        assert_eq!(wheel.turn(Pixels(50.), false), 0);
        assert_eq!(wheel.turn(Pixels(-30.), false), 0);
        assert_eq!(wheel.turn(Pixels(-30.), false), 1);
        // Mouse detents turn at once.
        assert_eq!(wheel.turn(Notches(3.), false), -1);
        assert_eq!(wheel.turn(Pixels(f32::NAN), false), 0);
        assert_eq!(wheel.turn(Pixels(0.), false), 0);
    }

    /// No step is more than a notch's worth, whatever says it: a
    /// high-resolution wheel's eighths make one step a notch, and the
    /// lines in parts a touchpad gives on X11 add up as pixels do rather
    /// than each making a step.
    #[test]
    fn lines_in_parts_make_a_step_a_notch() {
        let event = |y: f32| ScrollEvent {
            delta: gpui::ScrollDelta::Lines(point(0., y)),
            ..ScrollEvent::default()
        };
        let mut wheel = WheelTurn::default();
        // A high-resolution wheel's notch, in eighths (Wayland's value120).
        for notch in 0..20 {
            let steps: i32 = (0..8)
                .map(|_| wheel.turn(Wheeled::vertical(&event(-3. / 8.)), false))
                .sum();
            assert_eq!(steps, 1, "notch {notch}");
        }
        // A touchpad's slide on X11: a tenth of a line at a time.
        let mut wheel = WheelTurn::default();
        let steps: i32 = (0..10)
            .map(|_| wheel.turn(Wheeled::vertical(&event(-0.1)), false))
            .sum();
        assert_eq!(steps, 0, "a line's slide is no step");
        let steps: i32 = (0..20)
            .map(|_| wheel.turn(Wheeled::vertical(&event(-0.1)), false))
            .sum();
        assert_eq!(steps, 1, "three lines' slide is one");
        // A plain wheel's notch, whole lines, and two at once: one step.
        assert_eq!(wheel.turn(Wheeled::vertical(&event(-3.)), false), 1);
        assert_eq!(wheel.turn(Wheeled::vertical(&event(-6.)), false), 1);
        assert!(Wheeled::vertical(&event(-3.)).notches());
        assert!(!Wheeled::vertical(&event(-0.4)).notches());
        // Sideways counts where it moved most.
        let sideways = ScrollEvent {
            delta: gpui::ScrollDelta::Pixels(point(px(-50.), px(10.))),
            ..ScrollEvent::default()
        };
        assert_eq!(Wheeled::along(&sideways), Wheeled::Pixels(-50.));
        assert_eq!(Wheeled::vertical(&sideways), Wheeled::Pixels(10.));
    }

    #[test]
    fn page_turns_wait_for_the_pending_request() {
        use Wheeled::{Notches, Pixels};
        let mut wheel = WheelTurn::default();
        assert_eq!(wheel.turn(Pixels(-80.), true), 0);
        assert_eq!(wheel.turn(Notches(-3.), true), 0);
        // Refused intent does not carry over into the next gesture.
        assert_eq!(wheel.turn(Pixels(-40.), false), 0);
        assert_eq!(wheel.turn(Pixels(-40.), false), 1);
        assert_eq!(wheel.turn(Notches(-3.), false), 1);
        // Intent accumulated before the request began is discarded too.
        assert_eq!(wheel.turn(Pixels(-40.), false), 0);
        assert_eq!(wheel.turn(Pixels(-40.), true), 0);
        assert_eq!(wheel.turn(Pixels(-40.), false), 0);
    }

    #[test]
    fn a_wheel_notch_moves_pages_as_chromium_s_viewer_does() {
        let wheel = |delta, modifiers| ScrollEvent {
            delta,
            modifiers,
            ..ScrollEvent::default()
        };
        let lines = |x, y| gpui::ScrollDelta::Lines(point(x, y));
        let plain = gpui::Modifiers::default();
        // A notch is three lines on Linux, toward the reader (down) when
        // negative: 120 px.
        assert_eq!(wheel_step(&wheel(lines(0., -3.), plain)), Some(120.));
        assert_eq!(wheel_step(&wheel(lines(0., 3.), plain)), Some(-120.));
        // The rest is gpui's: a touchpad's pixels, sideways, Ctrl's zoom.
        let pixels = gpui::ScrollDelta::Pixels(point(px(0.), px(-30.)));
        assert_eq!(wheel_step(&wheel(pixels, plain)), None);
        assert_eq!(wheel_step(&wheel(lines(-3., 0.), plain)), None);
        assert_eq!(wheel_step(&wheel(lines(0., f32::NAN), plain)), None);
        let ctrl = gpui::Modifiers::control();
        assert_eq!(wheel_step(&wheel(lines(0., -3.), ctrl)), None);
        // Lines in parts are a touchpad's (X11 says lines for it) or a
        // high-resolution wheel's steps between notches: gpui's too.
        assert_eq!(wheel_step(&wheel(lines(0., -0.4), plain)), None);
        assert_eq!(wheel_step(&wheel(lines(0., -1.7), plain)), None);
        // Two notches at once, and one an X11 valuator gives a hair off.
        assert_eq!(wheel_step(&wheel(lines(0., -6.), plain)), Some(240.));
        let off = wheel_step(&wheel(lines(0., -3.000_000_4), plain));
        assert!(
            off.is_some_and(|step| (step - 120.).abs() < 0.01),
            "{off:?}"
        );
        // A page at a time, a notch sideways goes at a document's step
        // too; lines in parts are counted as a `div` counts them.
        assert!(notches(&lines(-3., 0.)));
        assert!(!notches(&lines(0.5, -3.)));
        assert!(!notches(&lines(0., 0.)));
        assert!(!notches(&pixels));
    }

    #[test]
    fn a_zoom_lands_what_was_under_the_pointer_back_under_it() {
        // Pages shown 600 × 800, a column 600 wide in a view 400 wide.
        let shown = |_| (600., 800.);
        let anchor = ZoomAnchor {
            page: 3,
            share: (0.5, 0.25),
            at: (200., 300.),
        };
        // The page starts 100 down the view: the row before it is on top,
        // 712 into it (its 800 and the gap under it).
        assert_eq!(landing(anchor, shown, 600., 400.), ((1, 712.), -100.));
        // Far down the page, the page itself is on top.
        let low = ZoomAnchor {
            share: (0.5, 0.75),
            at: (200., 100.),
            ..anchor
        };
        assert_eq!(landing(low, shown, 600., 400.), ((2, 500.), -100.));
        // The start stops it, and a column narrower than the view does not
        // go sideways.
        let first = ZoomAnchor {
            page: 1,
            share: (0.5, 0.),
            at: (100., 300.),
        };
        assert_eq!(landing(first, shown, 600., 800.), ((0, 0.), 0.));
        // Rows above count at their own heights.
        let mixed = |page| {
            if page == 2 {
                (600., 400.)
            } else {
                (600., 800.)
            }
        };
        assert_eq!(landing(anchor, mixed, 600., 400.), ((1, 312.), -100.));
    }

    #[test]
    fn an_arrow_or_the_wheel_turns_a_page_only_at_its_edge() {
        // A page taller than the panel: 800 to scroll.
        assert!(at_page_edge(0., 800., -1), "Up at the top");
        assert!(!at_page_edge(0., 800., 1), "Down at the top scrolls");
        assert!(!at_page_edge(-400., 800., -1));
        assert!(!at_page_edge(-400., 800., 1));
        assert!(at_page_edge(-800., 800., 1), "Down at the bottom");
        // A page that fits turns either way at once.
        assert!(at_page_edge(0., 0., 1));
        assert!(at_page_edge(0., 0., -1));
        // A handle at its top holds 0 or -0 alike: a page that fits is at
        // both its edges, so the wheel turns it either way.
        assert!(at_page_edge(-0., 0., 1) && at_page_edge(-0., 0., -1));
    }

    #[test]
    fn bottom_anchor_only_after_a_backward_turn_changed_the_page() {
        assert!(lands_at_bottom(-1, 5, 4));
        assert!(!lands_at_bottom(-1, 1, 1));
        assert!(!lands_at_bottom(1, 4, 5));
        assert!(!lands_at_bottom(1, 9, 9));
    }

    #[test]
    fn narrower_later_measurement_keeps_the_requested_neighbours() {
        let first = wanted_pages(vec![5, 6, 7, 8], 5, 100);
        let later = wanted_pages(vec![5], 5, 100);
        assert!([4, 5, 6].iter().all(|p| later.contains(p)));
        assert!(later.iter().all(|p| first.contains(p)));
        assert_eq!(later, vec![5, 4, 6]);
        // Rows the later frame no longer shows are dropped, neighbours are not.
        assert!(!later.contains(&7));
        assert_eq!(wanted_pages(vec![2], 1, 100), vec![1, 2]);
    }

    #[test]
    fn explicit_visits_and_refresh_forget_render_failures() {
        let mut state = DocumentScroll::default();
        assert!(!state.forget_errors(Some(3)));
        assert!(!state.forget_errors(None));
        Rc::make_mut(&mut state.errors).insert(3, Words::as_written("boom"));
        Rc::make_mut(&mut state.errors).insert(7, Words::as_written("boom"));
        assert!(state.forget_errors(Some(3)));
        assert!(!state.forget_errors(Some(3)));
        assert!(state.errors.contains_key(&7));
        assert!(state.forget_errors(None));
        assert!(state.errors.is_empty());
    }
}
