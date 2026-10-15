//! Virtual document scrolling and bounded, demand-driven page rendering.
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, Context, ListAlignment, ListOffset, ListState, RenderImage, ScrollHandle,
    ScrollWheelEvent, Window, div, point, prelude::*, px,
};
use skwir_core::{CancelToken, document::PageContent, scheduler::Priority};

use crate::{preview::PreviewContent, shell::Shell, theme::Theme};

const CACHE_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone)]
pub(crate) struct CachedPage {
    pub image: Arc<RenderImage>,
    pub content: Arc<PageContent>,
    bytes: usize,
    /// The side it was asked at: drawn again only when pages are shown
    /// larger than that, whatever pixels the engine could give.
    side: u32,
}

/// Steps pages are rendered at: a zoom step of 1.25 crosses at most one.
const SIDE_STEP: f32 = 512.;

/// The side to render a page at that covers `covered` device pixels on its
/// longer side: the next step, a little short allowed, up to `MAX_SIDE`.
pub(crate) fn page_side(covered: f32) -> u32 {
    let steps = if covered.is_finite() {
        (covered / 1.05 / SIDE_STEP).ceil().max(1.)
    } else {
        1.
    };
    ((steps * SIDE_STEP) as u32).min(skwir_core::document::MAX_SIDE)
}

/// Hand what the pages covered this frame to the shell, when it changes the
/// side they are rendered at or the scale a zoom step starts from.
pub(crate) fn report_shown(
    report: &crate::elements::document::Report,
    side: u32,
    scale: Option<f32>,
    entity: gpui::WeakEntity<Shell>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let Some(shown) = report.take() else {
        return;
    };
    if page_side(shown.covered) == side && scale.is_some_and(|s| (s - shown.scale).abs() < 1e-4) {
        return;
    }
    window.defer(cx, move |window, cx| {
        if let Some(shell) = entity.upgrade() {
            shell.update(cx, |shell, cx| shell.document_shown(shown, window, cx));
        }
    });
}
struct PageResult {
    generation: u64,
    number: u32,
    result: Result<CachedPage, String>,
}
pub struct DocumentScroll {
    pub continuous: bool,
    pub list: ListState,
    pub single: ScrollHandle,
    /// Shared with each frame's rows, which only read them: a frame costs
    /// a reference, not a copy of every page seen.
    pub(crate) pages: Rc<BTreeMap<u32, CachedPage>>,
    sizes: Rc<BTreeMap<u32, (f32, f32)>>,
    /// The widest page seen, in page units, for the sideways scroll at a zoom.
    widest: f32,
    errors: Rc<BTreeMap<u32, String>>,
    wanted: Vec<u32>,
    pending: Option<(u32, CancelToken)>,
    generation: u64,
    tx: flume::Sender<PageResult>,
    rx: flume::Receiver<PageResult>,
    layout: Rc<RefCell<Option<(gpui::Size<gpui::Pixels>, Option<f32>, bool)>>>,
    wheel: WheelTurn,
    pub bottom_on_load: bool,
}
impl Default for DocumentScroll {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        Self {
            continuous: false,
            list: ListState::new(0, ListAlignment::Top, px(100.)),
            single: ScrollHandle::new(),
            pages: Rc::default(),
            sizes: Rc::default(),
            widest: 0.,
            errors: Rc::default(),
            wanted: Vec::new(),
            pending: None,
            generation: 0,
            tx,
            rx,
            layout: Rc::default(),
            wheel: WheelTurn::default(),
            bottom_on_load: false,
        }
    }
}
impl Drop for DocumentScroll {
    fn drop(&mut self) {
        self.cancel();
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
    fn cancel(&mut self) {
        if let Some((_, token)) = self.pending.take() {
            token.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
    }
    /// A render failure stays visible until the user visits that page
    /// explicitly or refreshes; then the page is requested again.
    fn forget_errors(&mut self, only: Option<u32>) -> bool {
        if let Some(number) = only {
            return Rc::make_mut(&mut self.errors).remove(&number).is_some();
        }
        let had = !self.errors.is_empty();
        self.errors = Rc::default();
        had
    }
}

/// A trackpad gesture must accumulate intent before it turns a page. Mouse
/// detents bypass the pixel threshold. The invariant that prevents racing
/// turns is that no turn starts while a page request is pending: a gesture
/// arriving then is refused and its accumulated intent discarded, so the
/// next gesture starts fresh once the page has arrived. The grid's
/// Ctrl+wheel zoom counts steps the same way, with nothing ever pending.
#[derive(Default)]
pub struct WheelTurn {
    accumulated: f32,
}
impl WheelTurn {
    pub fn turn(&mut self, delta: f32, lines: bool, pending: bool) -> i32 {
        if !delta.is_finite() || delta == 0. {
            return 0;
        }
        if pending || self.accumulated.signum() != delta.signum() {
            self.accumulated = 0.;
        }
        if pending {
            return 0;
        }
        self.accumulated += delta;
        if lines || self.accumulated.abs() >= 60. {
            self.accumulated = 0.;
            if delta < 0. { 1 } else { -1 }
        } else {
            0
        }
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
        let count = session.info().pages as usize;
        let scroll = &mut self.preview.reader.scroll;
        scroll.continuous = continuous;
        scroll.cancel();
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

    /// A document session arrived. One that just opened reads the way the
    /// user last chose (`reading.continuous`, or the choice not saved yet);
    /// one read again after its file changed keeps the mode it had, over its
    /// new number of pages, at the same place.
    pub(crate) fn reader_opened(
        &mut self,
        session: Arc<dyn skwir_core::document::DocumentSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let first = self.preview.reader.session.is_none();
        let reopened = std::mem::take(&mut self.preview.reader.reopened);
        let count = session.info().pages as usize;
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
        if reopened && scroll.continuous {
            let mut top = scroll.list.logical_scroll_top();
            top.item_ix = top.item_ix.min(count.saturating_sub(1));
            scroll.list.reset(count);
            scroll.layout.borrow_mut().take();
            scroll.list.scroll_to(top);
            if let Some(anchor) = relaid {
                self.reader_land(anchor, cx);
            }
            return;
        }
        if reopened {
            if let Some(anchor) = relaid {
                self.reader_land(anchor, cx);
            }
            return;
        }
        let continuous = crate::settings::reader_mode(self);
        self.set_reader_continuous(continuous, window, cx);
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
        reader.tried = false;
        reader.reopened = true;
        std::mem::take(&mut reader.lifetime).cancel();
        reader.scroll.sizes = Rc::default();
        reader.scroll.widest = 0.;
    }

    pub fn reader_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(window.line_height());
        let pending = self.preview.phase != crate::preview::PreviewPhase::Idle;
        let scroll = &mut self.preview.reader.scroll;
        // The handle clamps to exactly these bounds, so compare them exactly.
        let offset = scroll.single.offset();
        let maximum = scroll.single.max_offset();
        let at_edge = if delta.y < px(0.) {
            -offset.y >= maximum.height
        } else {
            offset.y >= px(0.)
        };
        let vertical = delta.y.abs() > delta.x.abs();
        if !at_edge
            || !vertical
            || event.modifiers.secondary()
            || event.modifiers.alt
            || event.modifiers.platform
            || event.modifiers.shift
        {
            scroll.wheel.accumulated = 0.;
            scroll.single.set_offset(point(
                (offset.x + delta.x).clamp(-maximum.width, px(0.)),
                (offset.y + delta.y).clamp(-maximum.height, px(0.)),
            ));
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let turn = scroll.wheel.turn(
            f32::from(delta.y),
            matches!(event.delta, gpui::ScrollDelta::Lines(_)),
            pending,
        );
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

    pub(crate) fn clear_document_pages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scroll = &mut self.preview.reader.scroll;
        scroll.cancel();
        // Render failures describe pages that no longer exist here.
        scroll.errors = Rc::default();
        for page in std::mem::take(&mut scroll.pages).values() {
            cx.drop_image(page.image.clone(), Some(window));
        }
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
        if side == self.preview.page_side {
            return;
        }
        let grew = side > self.preview.page_side;
        self.preview.page_side = side;
        tracing::debug!(side, "document pages rendered at");
        if !grew || self.preview.reader.session.is_none() {
            return;
        }
        if self.preview.reader.scroll.continuous {
            self.request_document_page();
        } else if self.preview.reader.shown_side < side {
            // The page shown is drawn again, where it is.
            self.cancel_pending_preview();
            self.preview.refresh = true;
            self.preview.phase = crate::preview::PreviewPhase::Queued;
            cx.notify();
        }
    }

    fn request_document_page(&mut self) {
        let Some(session) = self.preview.reader.session.clone() else {
            return;
        };
        let side = self.preview.page_side;
        let initial = match &self.preview.content {
            Some(PreviewContent::Image {
                document: Some(page),
                ..
            }) => Some(page.page),
            _ => None,
        };
        // The page the preview opened on is drawn from its content until
        // it is shown larger than it was rendered.
        let initial = initial.filter(|_| self.preview.reader.shown_side >= side);
        let scroll = &mut self.preview.reader.scroll;
        if !scroll.continuous || scroll.pending.is_some() {
            return;
        }
        let Some(number) = scroll.wanted.iter().copied().find(|p| {
            Some(*p) != initial
                && scroll.pages.get(p).is_none_or(|cached| cached.side < side)
                && !scroll.errors.contains_key(p)
        }) else {
            return;
        };
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
        let wake = self.app.waker_handle();
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
                        .ok_or_else(|| anyhow::anyhow!("invalid document bitmap"))?;
                        Ok(CachedPage {
                            image,
                            content: page.content,
                            bytes,
                            side,
                        })
                    })
                    .map_err(|e| format!("{e:#}"));
                if !cancel.is_cancelled() {
                    let _ = tx.send(PageResult {
                        generation,
                        number,
                        result,
                    });
                    wake();
                }
            },
        );
    }

    pub fn sync_document_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Ok(reply) = self.preview.reader.scroll.rx.try_recv() {
            let scroll = &mut self.preview.reader.scroll;
            if reply.generation != scroll.generation || !scroll.continuous {
                continue;
            }
            scroll.pending = None;
            match reply.result {
                Ok(page) => {
                    let size = (page.content.width, page.content.height);
                    let assumed = Rc::make_mut(&mut scroll.sizes).insert(reply.number, size);
                    scroll.widest = scroll.widest.max(size.0);
                    if let Some(old) = Rc::make_mut(&mut scroll.pages).insert(reply.number, page) {
                        cx.drop_image(old.image, Some(window));
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
                }
            }
            self.request_document_page();
            cx.notify();
        }
    }
}

pub fn continuous_element(
    shell: &Shell,
    image: Arc<RenderImage>,
    page: Arc<PageContent>,
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
    let count = list.item_count() as u32;
    let layout = scroll.layout.clone();
    let requested = Rc::new(RefCell::new(Vec::new()));
    let seen = requested.clone();
    // What the rows cover, read once they are laid out.
    let report = crate::elements::document::Report::default();
    let row_report = report.clone();
    let (side, scale) = (shell.preview.page_side, reader.shown_scale);
    let entity = cx.entity().downgrade();
    let after_entity = entity.clone();
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
            let (fw, fh) = fallback;
            let estimate = zoom.unwrap_or_else(|| {
                if fit_width {
                    width / fw.max(1.)
                } else {
                    (width / fw.max(1.)).min((height - 12.).max(1.) / fh.max(1.))
                }
            }) * fh
                + 12.;
            let signature = (bounds, zoom, fit_width);
            if layout.borrow().as_ref() != Some(&signature) {
                let offset = list.logical_scroll_top();
                list.reset_with_uniform_height(list.item_count(), px(estimate.max(1.)));
                list.scroll_to(offset);
                *layout.borrow_mut() = Some(signature);
            }
            let content_width = zoom.map_or(width, |z| width.max(widest * z));
            let rows = gpui::list(list, move |index, _, _| {
                let number = index as u32 + 1;
                seen.borrow_mut().push(number);
                let (pw, ph) = if number == page.page {
                    fallback
                } else {
                    sizes.get(&number).copied().unwrap_or(fallback)
                };
                let scale = zoom.unwrap_or_else(|| {
                    if fit_width {
                        width / pw.max(1.)
                    } else {
                        (width / pw.max(1.)).min((height - 12.).max(1.) / ph.max(1.))
                    }
                });
                let row_height = (ph * scale).max(1.);
                let mut row = div()
                    .id(("continuous-page", index))
                    .w(px(content_width))
                    .h(px(row_height + 12.))
                    .pb(px(12.))
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
                                errors
                                    .get(&number)
                                    .cloned()
                                    .unwrap_or_else(|| format!("Page {number}…")),
                            ),
                    );
                }
                row.into_any_element()
            })
            .w(px(content_width))
            .h(bounds.height);
            let bar = crate::elements::scrollbar::vertical(
                "document-scrollbar",
                crate::elements::scrollbar::Scrolls::Items(bar_list),
            );
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("document-horizontal-scroll")
                        .size_full()
                        .overflow_x_scroll()
                        .child(rows),
                )
                .child(bar)
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
    div()
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
        use skwir_core::document::MAX_SIDE;
        // A little short of a step is that step; past it, the next.
        assert_eq!(page_side(426.), 512);
        assert_eq!(page_side(1067.), 1024);
        assert_eq!(page_side(1100.), 1536);
        assert_eq!(page_side(0.), 512);
        assert_eq!(page_side(f32::NAN), 512);
        assert_eq!(page_side(100_000.), MAX_SIDE);
    }

    #[test]
    fn cache_releases_distant_pages_but_keeps_visible_pixels_and_geometry() {
        let mut state = DocumentScroll::default();
        for number in 1..=8 {
            Rc::make_mut(&mut state.pages).insert(
                number,
                CachedPage {
                    image: crate::thumbs::owned_render_image(1, 1, vec![0; 4]).unwrap(),
                    content: Arc::new(PageContent {
                        page: number,
                        width: 600.,
                        height: 800.,
                        chars: Vec::new(),
                        links: Vec::new(),
                    }),
                    // Exercise accounting with tiny synthetic buffers.
                    bytes: CACHE_BYTES / 4,
                    side: 1024,
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
        let mut wheel = WheelTurn::default();
        assert_eq!(wheel.turn(-20., false, false), 0);
        assert_eq!(wheel.turn(-40., false, false), 1);
        // A turn consumes the accumulated intent.
        assert_eq!(wheel.turn(-20., false, false), 0);
        // Reversing direction starts over.
        assert_eq!(wheel.turn(50., false, false), 0);
        assert_eq!(wheel.turn(-30., false, false), 0);
        assert_eq!(wheel.turn(-30., false, false), 1);
        // Mouse detents turn at once.
        assert_eq!(wheel.turn(1., true, false), -1);
        assert_eq!(wheel.turn(f32::NAN, false, false), 0);
        assert_eq!(wheel.turn(0., false, false), 0);
    }

    #[test]
    fn page_turns_wait_for_the_pending_request() {
        let mut wheel = WheelTurn::default();
        assert_eq!(wheel.turn(-80., false, true), 0);
        assert_eq!(wheel.turn(-1., true, true), 0);
        // Refused intent does not carry over into the next gesture.
        assert_eq!(wheel.turn(-40., false, false), 0);
        assert_eq!(wheel.turn(-40., false, false), 1);
        assert_eq!(wheel.turn(-1., true, false), 1);
        // Intent accumulated before the request began is discarded too.
        assert_eq!(wheel.turn(-40., false, false), 0);
        assert_eq!(wheel.turn(-40., false, true), 0);
        assert_eq!(wheel.turn(-40., false, false), 0);
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
        Rc::make_mut(&mut state.errors).insert(3, "boom".into());
        Rc::make_mut(&mut state.errors).insert(7, "boom".into());
        assert!(state.forget_errors(Some(3)));
        assert!(!state.forget_errors(Some(3)));
        assert!(state.errors.contains_key(&7));
        assert!(state.forget_errors(None));
        assert!(state.errors.is_empty());
    }
}
