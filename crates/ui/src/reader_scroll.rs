//! Virtual document scrolling and bounded, demand-driven page rendering.
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

use bibli_core::{CancelToken, document::PageContent, scheduler::Priority};
use gpui::{
    AnyElement, Context, ListAlignment, ListOffset, ListState, RenderImage, ScrollHandle,
    ScrollWheelEvent, Window, div, point, prelude::*, px,
};

use crate::{preview::PreviewContent, shell::Shell, theme::Theme};

const CACHE_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone)]
pub(crate) struct CachedPage {
    pub image: Arc<RenderImage>,
    pub content: Arc<PageContent>,
    bytes: usize,
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
    pub(crate) pages: BTreeMap<u32, CachedPage>,
    sizes: BTreeMap<u32, (f32, f32)>,
    errors: BTreeMap<u32, String>,
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
            pages: BTreeMap::new(),
            sizes: BTreeMap::new(),
            errors: BTreeMap::new(),
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
            if let Some(page) = self.pages.remove(&number) {
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
            return self.errors.remove(&number).is_some();
        }
        let had = !self.errors.is_empty();
        self.errors.clear();
        had
    }
}

/// A trackpad gesture must accumulate intent before it turns a page. Mouse
/// detents bypass the pixel threshold. The invariant that prevents racing
/// turns is that no turn starts while a page request is pending: a gesture
/// arriving then is refused and its accumulated intent discarded, so the
/// next gesture starts fresh once the page has arrived.
#[derive(Default)]
struct WheelTurn {
    accumulated: f32,
}
impl WheelTurn {
    fn turn(&mut self, delta: f32, lines: bool, pending: bool) -> i32 {
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
    pub fn reader_continuous(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.preview.reader.session else {
            return;
        };
        let count = session.info().pages as usize;
        let scroll = &mut self.preview.reader.scroll;
        scroll.continuous = !scroll.continuous;
        scroll.cancel();
        scroll.wanted.clear();
        scroll.errors.clear();
        scroll.list.reset(count);
        scroll.list.scroll_to(ListOffset {
            item_ix: self.preview.page as usize - 1,
            offset_in_item: px(0.),
        });
        scroll.single.set_offset(point(px(0.), px(0.)));
        self.preview.reader.selection = None;
        self.preview.reader.selection_page = None;
        self.preview.reader.cancel_search();
        if !self.preview.reader.scroll.continuous {
            if let Some(page) = self.preview.reader.scroll.pages.remove(&self.preview.page) {
                let size = page.image.size(0);
                self.replace_preview_content(
                    Some(PreviewContent::Image {
                        image: page.image,
                        width: size.width.0 as u32,
                        height: size.height.0 as u32,
                        detail: Some(bibli_core::preview::PreviewInfo::Document {
                            page: self.preview.page,
                            pages: count as u32,
                        }),
                        document: Some(page.content),
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
            || event.modifiers.control
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
        scroll.errors.clear();
        for (_, page) in std::mem::take(&mut scroll.pages) {
            cx.drop_image(page.image, Some(window));
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

    fn request_document_page(&mut self) {
        let Some(session) = self.preview.reader.session.clone() else {
            return;
        };
        let initial = match &self.preview.content {
            Some(PreviewContent::Image {
                document: Some(page),
                ..
            }) => Some(page.page),
            _ => None,
        };
        let scroll = &mut self.preview.reader.scroll;
        if !scroll.continuous || scroll.pending.is_some() {
            return;
        }
        let Some(number) = scroll.wanted.iter().copied().find(|p| {
            Some(*p) != initial && !scroll.pages.contains_key(p) && !scroll.errors.contains_key(p)
        }) else {
            return;
        };
        // Speculative neighbours must not interrupt an explicitly started find.
        // A missing current page still has priority over background searching.
        if self.preview.reader.searching && number != self.preview.page {
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
                    .render(number, bibli_core::document::MAX_SIDE, &cancel)
                    .and_then(|page| {
                        let bytes = page.rgba.len();
                        let image =
                            crate::thumbs::owned_render_image(page.width, page.height, page.rgba)
                                .ok_or_else(|| anyhow::anyhow!("invalid document bitmap"))?;
                        Ok(CachedPage {
                            image,
                            content: page.content,
                            bytes,
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
                    scroll
                        .sizes
                        .insert(reply.number, (page.content.width, page.content.height));
                    if let Some(old) = scroll.pages.insert(reply.number, page) {
                        cx.drop_image(old.image, Some(window));
                    }
                    // Invalidate the placeholder's measured height, keeping the reading anchor.
                    let offset = scroll.list.logical_scroll_top();
                    scroll
                        .list
                        .splice(reply.number as usize - 1..reply.number as usize, 1);
                    scroll.list.scroll_to(offset);
                }
                Err(error) => {
                    scroll.errors.insert(reply.number, error);
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
    let mut pages = scroll.pages.clone();
    pages.insert(
        page.page,
        CachedPage {
            image,
            content: page.clone(),
            bytes: 0,
        },
    );
    let mut sizes = scroll.sizes.clone();
    sizes.insert(page.page, (page.width, page.height));
    let fallback = (page.width, page.height);
    let errors = scroll.errors.clone();
    let list = scroll.list.clone();
    let after_list = list.clone();
    let count = list.item_count() as u32;
    let layout = scroll.layout.clone();
    let requested = Rc::new(RefCell::new(Vec::new()));
    let seen = requested.clone();
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
            let signature = (bounds, zoom, fit_width);
            if layout.borrow().as_ref() != Some(&signature) {
                let offset = list.logical_scroll_top();
                list.reset(list.item_count());
                list.scroll_to(offset);
                *layout.borrow_mut() = Some(signature);
            }
            let width = f32::from(bounds.width).max(1.);
            let height = f32::from(bounds.height).max(1.);
            let content_width = zoom.map_or(width, |z| {
                sizes.values().fold(width, |w, (pw, _)| w.max(pw * z))
            });
            let rows = gpui::list(list, move |index, _, _| {
                let number = index as u32 + 1;
                seen.borrow_mut().push(number);
                let (pw, ph) = sizes.get(&number).copied().unwrap_or(fallback);
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
                if let Some(page) = pages.get(&number) {
                    row = row.child(div().w(px(pw * scale)).h(px(row_height)).child(
                        crate::reader::surface(
                            page.image.clone(),
                            page.content.clone(),
                            None,
                            false,
                            selection_page,
                            selection,
                            hit.clone(),
                            theme.accent,
                            entity.clone(),
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
            div()
                .id("document-horizontal-scroll")
                .size_full()
                .overflow_x_scroll()
                .map(|mut d| {
                    // Vertical wheel gestures belong to the document list;
                    // do not also convert them into horizontal panning.
                    d.style().restrict_scroll_to_axis = Some(true);
                    d
                })
                .child(rows)
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
    fn cache_releases_distant_pages_but_keeps_visible_pixels_and_geometry() {
        let mut state = DocumentScroll::default();
        for number in 1..=8 {
            state.pages.insert(
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
                },
            );
            state.sizes.insert(number, (600., 800.));
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
        state.errors.insert(3, "boom".into());
        state.errors.insert(7, "boom".into());
        assert!(state.forget_errors(Some(3)));
        assert!(!state.forget_errors(Some(3)));
        assert!(state.errors.contains_key(&7));
        assert!(state.forget_errors(None));
        assert!(state.errors.is_empty());
    }
}
