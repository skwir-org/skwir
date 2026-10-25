//! Stateful document reader: selection, search, outline and link navigation.
//! Engine operations run on the scheduler; this module only owns UI state.

use crate::{
    latest::Latest, preview::PreviewContent, preview_bar::Field, shell::Shell, theme::Theme,
};
use gpui::{AnyElement, ClipboardItem, Context, FocusHandle, RenderImage, Window, div, prelude::*};
use skwir_core::{
    CancelToken,
    document::{Destination, DocumentSession, PageContent, Rect, SearchHit},
    scheduler::Priority,
};
use std::{rc::Rc, sync::Arc};

/// Where a book laid out again should open: what was read, as the first
/// words of its page, and how far through the book that page was (0 to 1),
/// for when the words are not found. `page` is where the reader landed with
/// it: steps of text size that follow one another without a move keep the
/// first anchor, instead of drifting a page each time.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Anchor {
    pub(crate) at: f32,
    pub(crate) text: String,
    pub(crate) page: u32,
}

/// Words of a page's text that find it again in another layout: few enough
/// to fall on one page of it.
const ANCHOR_WORDS: usize = 6;
// Independent switches of one reader, not a state machine in disguise.
#[allow(clippy::struct_excessive_bools)]
pub struct ReaderState {
    pub scroll: crate::reader_scroll::DocumentScroll,
    pub selection_page: Option<u32>,
    pub session: Option<Arc<dyn DocumentSession>>,
    pub lifetime: CancelToken,
    pub tried: bool,
    /// The session was let go because its file changed; the next one is
    /// the same document read again (`Shell::reader_opened`).
    pub(crate) reopened: bool,
    /// The book is being laid out again at another text size: where the
    /// reader lands in its new pages.
    pub(crate) relayout: Option<Anchor>,
    /// Where the last layout landed, and the lookup of its words running.
    pub(crate) anchor: Option<Anchor>,
    /// The scale pages were last shown at (their size on screen over their
    /// own): where a zoom step from Fit starts.
    pub(crate) shown_scale: Option<f32>,
    /// The side the page shown in single-page mode was asked at.
    pub(crate) shown_side: u32,
    located: (flume::Sender<(u64, u32)>, flume::Receiver<(u64, u32)>),
    locating: u64,
    pub focus: Option<FocusHandle>,
    pub selection: Option<(usize, usize)>,
    pub outlines: bool,
    /// The contents' list, following the page shown.
    pub(crate) contents: crate::outline::Follow,
    find: Option<Field>,
    pub page_input: Option<Field>,
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) active_hit: Option<usize>,
    /// The search the find field asked for.
    pub(crate) search: Latest<Result<Vec<SearchHit>, String>>,
    pub down: Option<(f32, f32)>,
}
impl Default for ReaderState {
    fn default() -> Self {
        Self {
            scroll: crate::reader_scroll::DocumentScroll::default(),
            selection_page: None,
            session: None,
            lifetime: CancelToken::new(),
            tried: false,
            reopened: false,
            relayout: None,
            anchor: None,
            shown_scale: None,
            shown_side: 0,
            located: flume::unbounded(),
            locating: 0,
            focus: None,
            selection: None,
            outlines: false,
            contents: crate::outline::Follow::default(),
            find: None,
            page_input: None,
            hits: Vec::new(),
            active_hit: None,
            search: Latest::default(),
            down: None,
        }
    }
}
impl Drop for ReaderState {
    fn drop(&mut self) {
        self.lifetime.cancel();
    }
}

impl ReaderState {
    /// Whether the find field is open.
    pub fn finding(&self) -> bool {
        self.find.is_some()
    }
    /// Whether the keyboard is in the reader: its pages, its find field or
    /// its page number, which go when the preview does.
    pub fn holds_focus(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        self.focus
            .as_ref()
            .is_some_and(|focus| focus.contains_focused(window, cx))
            || [&self.find, &self.page_input]
                .into_iter()
                .flatten()
                .any(|field| field.is_focused(window, cx))
    }
    pub fn cancel_search(&mut self) {
        self.search.forget();
    }
}

fn page(shell: &Shell) -> Option<&Arc<PageContent>> {
    let number = shell
        .preview
        .reader
        .selection_page
        .unwrap_or(shell.preview.page);
    if let Some(page) = shell.preview.reader.scroll.pages.get(&number) {
        return Some(&page.content);
    }
    match &shell.preview.content {
        Some(PreviewContent::Image {
            document: Some(page),
            ..
        }) if !shell.preview.reader.scroll.continuous || page.page == number => Some(page),
        _ => None,
    }
}
fn selected_text(page: &PageContent, selection: Option<(usize, usize)>) -> String {
    let Some((a, b)) = selection else {
        return String::new();
    };
    page.chars
        .iter()
        .skip(a.min(b))
        .take(a.abs_diff(b) + 1)
        .map(|ch| ch.text.as_str())
        .collect()
}
fn nearest(page: &PageContent, x: f32, y: f32) -> Option<usize> {
    page.chars
        .iter()
        .enumerate()
        .filter(|(_, ch)| ch.bounds.height > 0.)
        .min_by(|(_, a), (_, b)| {
            let distance = |r: Rect| {
                let dx = (x - x.clamp(r.x, r.x + r.width.max(0.))).abs();
                let dy = (y - y.clamp(r.y, r.y + r.height.max(0.))).abs();
                dx * dx + dy * dy
            };
            distance(a.bounds).total_cmp(&distance(b.bounds))
        })
        .map(|(i, _)| i)
}

impl Shell {
    pub fn reader_page_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(
            self.preview.content,
            Some(PreviewContent::Image {
                detail: Some(skwir_core::preview::PreviewInfo::Document { .. }),
                ..
            })
        ) {
            return;
        }
        let field = Field::new(
            "Page",
            "ui.reader_page_submit",
            "ui.reader_page_cancel",
            window,
            cx,
        );
        // Counted by printed pages, the field opens on the page's label.
        let current = page_labels(self)
            .filter(|_| self.prefs.printed_pages)
            .and_then(|labels| labels.get(self.preview.page as usize - 1))
            .filter(|label| !label.is_empty())
            .cloned()
            .unwrap_or_else(|| self.preview.page.to_string());
        field.input.update(cx, |input, cx| {
            input.set_text(current, cx);
            input.select_all_now(cx);
            input.focus(window, cx);
        });
        self.preview.reader.page_input = Some(field);
        cx.notify();
    }

    pub fn reader_page_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self
            .preview
            .reader
            .page_input
            .as_ref()
            .map(|p| p.text(cx))
            .unwrap_or_default();
        // Counted by printed pages, a page is found by its name as printed.
        if let Some(labels) = page_labels(self).filter(|_| self.prefs.printed_pages) {
            let Some(number) = page_named(labels, &typed) else {
                let named = || labels.iter().filter(|label| !label.is_empty());
                let (first, last) = (
                    named().next().cloned().unwrap_or_default(),
                    named().next_back().cloned().unwrap_or_default(),
                );
                return self.set_status(
                    format!("Enter a page number printed in the document, from {first} to {last}"),
                    true,
                    cx,
                );
            };
            self.reader_page_cancel(window, cx);
            let args = serde_json::json!({"page":number});
            return self.run("ui.reader_page", args, window, cx);
        }
        let number = typed.trim().parse::<u32>().ok();
        if let Some(number) = number.filter(|n| *n > 0 && *n <= document_pages(self)) {
            self.reader_page_cancel(window, cx);
            self.run(
                "ui.reader_page",
                serde_json::json!({"page":number}),
                window,
                cx,
            );
        } else {
            self.set_status(
                format!("Enter a page number from 1 to {}", document_pages(self)),
                true,
                cx,
            );
        }
    }

    pub fn reader_page_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview.reader.page_input = None;
        if let Some(focus) = &self.preview.reader.focus {
            window.focus(focus, cx);
        } else {
            window.focus(&self.ui().focus, cx);
        }
        cx.notify();
    }

    pub fn reader_pointer(
        &mut self,
        phase: &str,
        number: u32,
        x: f32,
        y: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if phase == "down" {
            self.preview.reader.selection_page = Some(number);
        } else if self.preview.reader.selection_page != Some(number) {
            return;
        }
        let Some(page) = page(self).cloned() else {
            return;
        };
        if phase == "down" {
            if let Some(focus) = &self.preview.reader.focus {
                window.focus(focus, cx);
            }
            let reader = &mut self.preview.reader;
            reader.down = Some((x, y));
            reader.selection = nearest(&page, x, y)
                .filter(|&i| page.chars[i].bounds.contains(x, y))
                .map(|i| (i, i));
        } else if self.preview.reader.down.is_some() {
            if let Some((a, _)) = self.preview.reader.selection {
                self.preview.reader.selection = nearest(&page, x, y).map(|b| (a, b));
            }
            if phase == "up" {
                let clicked = self
                    .preview
                    .reader
                    .down
                    .take()
                    .is_some_and(|(a, b)| (a - x).abs() + (b - y).abs() < 0.006);
                if clicked
                    && let Some((index, _)) = page
                        .links
                        .iter()
                        .enumerate()
                        .find(|(_, link)| link.bounds.contains(x, y))
                {
                    self.preview.reader.selection = None;
                    let tab = window.modifiers().shift;
                    self.run(
                        "ui.reader_link",
                        serde_json::json!({"index": index, "tab": tab}),
                        window,
                        cx,
                    );
                }
            }
        } else {
            return;
        }
        cx.notify();
    }

    /// The text selected, else the page's, as a structured preview
    /// copies what it shows when nothing is selected.
    pub fn reader_copy(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = page(self) {
            let mut text = selected_text(page, self.preview.reader.selection);
            if text.is_empty() {
                let all = page.chars.len().checked_sub(1).map(|last| (0, last));
                text = selected_text(page, all);
            }
            if !text.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
    }
    pub fn reader_select_all(&mut self, cx: &mut Context<Self>) {
        self.preview.reader.selection_page = Some(self.preview.page);
        self.preview.reader.selection =
            page(self).and_then(|p| p.chars.len().checked_sub(1).map(|last| (0, last)));
        cx.notify();
    }
    /// A page's link: a page of this document, or where any document's
    /// link goes (`Shell::follow_document_link`).
    pub fn reader_link(
        &mut self,
        index: usize,
        tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = page(self)
            .and_then(|p| p.links.get(index))
            .map(|l| l.destination.clone());
        match target {
            Some(Destination::Page(page)) => self.preview_go_page(page, cx),
            Some(Destination::Uri(uri)) => self.follow_document_link(&uri, tab, window, cx),
            None => {}
        }
    }
    pub fn reader_outline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview.reader.outlines = !self.preview.reader.outlines;
        if !self.preview.reader.outlines {
            window.focus(
                self.preview
                    .reader
                    .focus
                    .as_ref()
                    .unwrap_or(&self.ui().focus),
                cx,
            );
        }
        cx.notify();
    }
    pub fn reader_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.reader.session.is_none() {
            return;
        }
        if self.preview.reader.find.is_none() {
            self.preview.reader.find = Some(Field::new(
                "Find in document…",
                "ui.reader_search",
                "ui.preview_find_close",
                window,
                cx,
            ));
        }
        if let Some(find) = &self.preview.reader.find {
            find.input.update(cx, |i, cx| i.focus(window, cx));
        }
        cx.notify();
    }
    pub fn reader_find_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reader = &mut self.preview.reader;
        reader.search.forget();
        reader.find = None;
        reader.hits.clear();
        reader.active_hit = None;
        if let Some(focus) = &reader.focus {
            window.focus(focus, cx);
        }
        cx.notify();
    }
    pub fn reader_search(&mut self, cx: &mut Context<Self>) {
        // Its helper reads the document where it is (ADR-077).
        let lane = crate::shell::lane_of(self.preview.path());
        let reader = &mut self.preview.reader;
        let Some(session) = reader.session.clone() else {
            return;
        };
        let Some(find) = &reader.find else {
            return;
        };
        let query = find.text(cx);
        find.hush();
        reader.hits.clear();
        reader.active_hit = None;
        let lifetime = reader.lifetime.clone();
        reader.search.ask(
            &self.app,
            lane,
            &lifetime,
            Priority::Normal,
            "document-search",
            move |cancel| session.search(&query, cancel).map_err(|e| format!("{e:#}")),
        );
        cx.notify();
    }
    /// Where the reader is, to open a book laid out again at: the anchor it
    /// landed with if it has not moved since, else the current page.
    pub(crate) fn reader_anchor(&mut self) -> Anchor {
        let current = self.preview.page;
        if let Some(anchor) = self.preview.reader.anchor.take()
            && anchor.page == current
        {
            return anchor;
        }
        let pages = self
            .preview
            .reader
            .session
            .as_ref()
            .map_or(1, |session| page_count(session.as_ref()).max(1));
        let text = page(self).map_or_else(String::new, |page| {
            let text: String = page.chars.iter().map(|c| c.text.as_str()).collect();
            text.split_whitespace()
                .take(ANCHOR_WORDS)
                .collect::<Vec<_>>()
                .join(" ")
        });
        Anchor {
            at: current.saturating_sub(1) as f32 / pages as f32,
            text,
            page: current,
        }
    }

    /// Open the book laid out again where `anchor` says: at once on the
    /// page its share of the book gives, then on the page its words are
    /// found on, if the reader has not moved meanwhile.
    pub(crate) fn reader_land(&mut self, anchor: Anchor, cx: &mut Context<Self>) {
        let reader = &mut self.preview.reader;
        let Some(session) = reader.session.clone() else {
            return;
        };
        let count = page_count(session.as_ref()).max(1);
        let near = ((anchor.at * count as f32).floor() as u32 + 1).min(count);
        reader.locating = reader.locating.wrapping_add(1);
        if !anchor.text.is_empty() {
            let (generation, tx) = (reader.locating, reader.located.0.clone());
            let cancel = reader.lifetime.child();
            let text = anchor.text.clone();
            let wake = self.app.waker_handle();
            self.app.scheduler.spawn(
                Priority::Visible,
                "document-locate",
                cancel.clone(),
                move || {
                    if let Ok(Some(page)) = session.locate(&text, near, &cancel)
                        && !cancel.is_cancelled()
                    {
                        let _ = tx.send((generation, page));
                        wake();
                    }
                },
            );
        }
        self.preview.reader.anchor = Some(Anchor {
            page: near,
            ..anchor
        });
        self.preview_go_page(near, cx);
    }

    pub fn sync_reader(&mut self, cx: &mut Context<Self>) {
        while let Ok((generation, page)) = self.preview.reader.located.1.try_recv() {
            let current = self.preview.page;
            let reader = &mut self.preview.reader;
            if generation != reader.locating {
                continue;
            }
            if let Some(anchor) = &mut reader.anchor
                && anchor.page == current
                && page != current
            {
                anchor.page = page;
                self.preview_go_page(page, cx);
            }
        }
        if let Some(result) = self.preview.reader.search.take() {
            cx.notify();
            // Said in the find field: closing it forgets its search.
            let find = self.preview.reader.find.as_ref();
            match result {
                Ok(hits) => {
                    let first = hits.first().map(|h| h.page);
                    if first.is_none()
                        && let Some(find) = find
                    {
                        find.say("No match", false);
                    }
                    self.preview.reader.hits = hits;
                    self.preview.reader.active_hit = first.map(|_| 0);
                    if let Some(first) = first {
                        self.preview_go_page(first, cx);
                    }
                }
                Err(error) => match find {
                    Some(find) => find.say(error, true),
                    None => self.set_status(error, true, cx),
                },
            }
        }
    }
    pub fn reader_match(&mut self, delta: i32, cx: &mut Context<Self>) {
        let reader = &mut self.preview.reader;
        if reader.hits.is_empty() {
            return;
        }
        let current = reader.active_hit.unwrap_or(0) as i64;
        let next = (current + i64::from(delta)).rem_euclid(reader.hits.len() as i64) as usize;
        reader.active_hit = Some(next);
        let page = reader.hits[next].page;
        self.preview_go_page(page, cx);
        cx.notify();
    }
}

pub fn page_element(
    shell: &Shell,
    image: Arc<RenderImage>,
    page: Arc<PageContent>,
    zoom: Option<f32>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    if shell.preview.reader.scroll.continuous {
        return crate::reader_scroll::continuous_element(shell, image, page, cx);
    }
    let report = crate::elements::document::Report::default();
    let element = page_surface(
        shell,
        image,
        page,
        zoom,
        shell.preview.fit_width,
        Some(report.clone()),
        cx,
    );
    // Once the page is laid out: what it covers, for the side pages are
    // rendered at (`Shell::document_shown`).
    let (side, scale) = (shell.preview.page_side, shell.preview.reader.shown_scale);
    let entity = cx.entity().downgrade();
    let watch = gpui::canvas(
        move |_, window, cx| {
            crate::reader_scroll::report_shown(&report, side, scale, entity, window, cx);
        },
        |_, (), _, _| {},
    )
    .absolute()
    .size_0();
    let single = &shell.preview.reader.scroll.single;
    // The bars are beside the canvas, not in it, where they would scroll
    // with the page; the wrapper keeps the column the canvas grew in.
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .child(
            expose(div().id("reader-canvas"), shell)
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_hidden()
                .track_scroll(single)
                .on_scroll_wheel(cx.listener(|shell, event, window, cx| {
                    shell.reader_wheel(event, window, cx);
                }))
                .when_some(shell.preview.reader.focus.as_ref(), |d, focus| {
                    d.track_focus(focus).key_context(crate::keymap::READER)
                })
                .child(element)
                .child(watch),
        )
        .child(crate::elements::scrollbar::vertical(
            "reader-scrollbar",
            crate::elements::scrollbar::Scrolls::Offset(single.clone()),
        ))
        .child(crate::elements::scrollbar::horizontal(
            "reader-scrollbar-sideways",
            single.clone(),
        ))
        .into_any_element()
}

/// A document's first page stood in for by its thumbnail while the reader
/// opens it: where the reader will draw it, at the page's proportions: at
/// the top of the column and as wide as it fits when reading continuously,
/// in the middle of the canvas a page at a time.
pub(crate) fn page_placeholder(
    image: Arc<RenderImage>,
    (width, height): (f32, f32),
    continuous: bool,
) -> AnyElement {
    if !continuous {
        return div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                gpui::img(image)
                    .absolute()
                    .size_full()
                    .object_fit(gpui::ObjectFit::Contain),
            )
            .into_any_element();
    }
    // The reader's row: the page fitted to the width and the height less
    // the gap under it (`reader_scroll::continuous_element`).
    let viewport = crate::elements::document::DocumentViewport {
        build: Box::new(move |bounds, _, _| {
            let (w, h) = (f32::from(bounds.width), f32::from(bounds.height));
            let scale = (w / width.max(1.)).min((h - 12.).max(1.) / height.max(1.));
            div()
                .size_full()
                .flex()
                .justify_center()
                .child(
                    gpui::img(image)
                        .flex_none()
                        .w(gpui::px(width * scale))
                        .h(gpui::px(height * scale)),
                )
                .into_any_element()
        }),
        laid_out: Box::new(|_, _| {}),
    };
    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .child(viewport)
        .into_any_element()
}

pub(crate) fn page_surface(
    shell: &Shell,
    image: Arc<RenderImage>,
    page: Arc<PageContent>,
    zoom: Option<f32>,
    fit_width: bool,
    report: Option<crate::elements::document::Report>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    surface(
        image,
        page,
        zoom,
        fit_width,
        shell.preview.reader.selection_page,
        shell.preview.reader.selection,
        shell
            .preview
            .reader
            .active_hit
            .and_then(|i| shell.preview.reader.hits.get(i))
            .cloned(),
        cx.global::<Theme>(),
        cx.entity().downgrade(),
        report,
    )
}

// Snapshot the interaction state so virtual rows need no Shell borrow during layout.
#[allow(clippy::too_many_arguments)] // All inputs belong to one page's rendering snapshot.
pub(crate) fn surface(
    image: Arc<RenderImage>,
    page: Arc<PageContent>,
    zoom: Option<f32>,
    fit_width: bool,
    selection_page: Option<u32>,
    selection: Option<(usize, usize)>,
    hit: Option<SearchHit>,
    theme: &Theme,
    entity: gpui::WeakEntity<Shell>,
    report: Option<crate::elements::document::Report>,
) -> AnyElement {
    let number = page.page;
    let selection = selection
        .filter(|_| selection_page == Some(number))
        .map_or_else(Vec::new, |(a, b)| {
            page.chars
                .iter()
                .skip(a.min(b))
                .take(a.abs_diff(b) + 1)
                .map(|ch| ch.bounds)
                .collect()
        });
    let highlights = hit
        .filter(|h| h.page == number)
        .map_or_else(Vec::new, |h| h.bounds);
    crate::elements::document::DocumentElement {
        image,
        page,
        selection,
        highlights,
        zoom,
        fit_width,
        report,
        colors: (theme.page_match, theme.page_selection),
        on_pointer: Rc::new(move |phase, x, y, window, cx| {
            if let Some(entity) = entity.upgrade() {
                entity.update(cx, |shell, cx| {
                    // Every page shown hears every left drag in the window:
                    // one that began elsewhere is none of its business, and
                    // running the command drew the window again for each.
                    let reader = &shell.preview.reader;
                    if phase != "down"
                        && (reader.selection_page != Some(number) || reader.down.is_none())
                    {
                        return;
                    }
                    shell.run(
                        "ui.reader_pointer",
                        serde_json::json!({"phase":phase,"page":number,"x":x,"y":y}),
                        window,
                        cx,
                    );
                });
            }
        }),
    }
    .into_any_element()
}

/// The labels the document shown gives its pages, when it names them
/// otherwise than by their number (`DocumentInfo::labels`).
fn page_labels(shell: &Shell) -> Option<&[String]> {
    shell
        .preview
        .reader
        .session
        .as_ref()
        .map(|session| session.info().labels.as_slice())
        .filter(|labels| !labels.is_empty())
}

/// The page named `typed`, as printed: matched without regard to case.
fn page_named(labels: &[String], typed: &str) -> Option<u32> {
    let typed = typed.trim().to_lowercase();
    labels
        .iter()
        .position(|label| !label.is_empty() && label.to_lowercase() == typed)
        .map(|index| index as u32 + 1)
}

/// How many pages of `session` the reader shows: what its engine says,
/// held to `document::MAX_PAGES`. The continuous column lays out an item
/// for each on the window's thread, so every count the reader reads of a
/// session goes through here, whichever engine or plugin gave it.
pub(crate) fn page_count(session: &dyn DocumentSession) -> u32 {
    session.info().pages.min(skwir_core::document::MAX_PAGES)
}

/// Pages of the document shown, when it is one.
pub(crate) fn pages(shell: &Shell) -> Option<u32> {
    let document = shell.preview.reader.session.is_some()
        || matches!(
            &shell.preview.content,
            Some(PreviewContent::Image {
                detail: Some(skwir_core::preview::PreviewInfo::Document { .. }),
                ..
            })
        );
    document.then(|| document_pages(shell))
}

/// The document as assistive technology hears its reader: named by the
/// file, described by the page shown; `None` while no screen reader
/// listens.
pub(crate) fn said(shell: &Shell) -> Option<(gpui::SharedString, String)> {
    crate::preview::said(shell).map(|(name, _)| {
        let page = format!("Page {} of {}", shell.preview.page, document_pages(shell));
        (name, page)
    })
}

/// The reader as assistive technology reads it: named and placed
/// (`said`), and the text of the page shown, a run a line, read out as the
/// pages turn. A page that is a picture alone (a scan) has none.
pub(crate) fn expose(el: gpui::Stateful<gpui::Div>, shell: &Shell) -> gpui::Stateful<gpui::Div> {
    let el = el.role(gpui::Role::Document);
    let Some((name, place)) = said(shell) else {
        return el;
    };
    let text = page(shell).filter(|page| !page.chars.is_empty()).cloned();
    el.aria_label(name)
        .aria_description(place)
        .when_some(text, |d, page| {
            d.a11y_synthetic_children(move |builder| read_page(&page, builder))
        })
}

/// A page's text in lines, as its engine broke them: each ending with its
/// line break but the last.
fn page_lines(page: &PageContent) -> Vec<String> {
    let text: String = page.chars.iter().map(|ch| ch.text.as_str()).collect();
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    text.trim_end_matches('\n')
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect()
}

fn read_page(page: &PageContent, builder: &mut gpui::A11ySubtreeBuilder) {
    for (index, line) in page_lines(page).into_iter().enumerate() {
        let lengths: Vec<u8> = line
            .chars()
            .map(|c| u8::try_from(c.len_utf8()).unwrap_or(4))
            .collect();
        let mut run = gpui::accesskit::Node::new(gpui::Role::TextRun);
        run.set_value(line);
        run.set_character_lengths(lengths);
        builder.push_child(builder.synthetic_node_id(index), run);
    }
}

fn document_pages(shell: &Shell) -> u32 {
    shell
        .preview
        .reader
        .session
        .as_ref()
        .map(|s| page_count(s.as_ref()))
        .or({
            if let Some(PreviewContent::Image {
                detail: Some(skwir_core::preview::PreviewInfo::Document { pages, .. }),
                ..
            }) = &shell.preview.content
            {
                Some(*pages)
            } else {
                None
            }
        })
        .unwrap_or(1)
}

pub fn toolbar(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    use crate::preview_bar::{Bar, Item, Slot, label};
    use crate::toolbar::Button;
    let reader = &shell.preview.reader;
    let native = reader.session.is_some();
    let current = shell.preview.page;
    let pages = document_pages(shell);
    let mut bar = Bar::new("reader-tools");
    bar.push(Item::button(
        Slot::Lead,
        1,
        Button::new(
            "reader-outline",
            "contents",
            label::CONTENTS,
            "ui.preview_contents",
        )
        .active(reader.outlines)
        // A document without contents (a comic) has none to show.
        .enabled(
            reader
                .session
                .as_ref()
                .is_some_and(|session| !session.info().outlines.is_empty()),
        ),
    ));
    crate::preview_bar::close_tool(&mut bar, shell, theme);

    bar.push(Item::button(
        Slot::Navigate,
        2,
        Button::new("preview-prev", "left", "Previous page", "ui.preview_page")
            .args(serde_json::json!({"delta":-1}))
            .enabled(current > 1),
    ));
    // A document that names its pages can show and take those names.
    let labels = page_labels(shell);
    let (value, total) = match labels.filter(|_| shell.prefs.printed_pages) {
        Some(labels) => (
            labels
                .get(current as usize - 1)
                .filter(|label| !label.is_empty())
                .cloned()
                .unwrap_or_else(|| "–".to_owned()),
            labels
                .iter()
                .rev()
                .find(|label| !label.is_empty())
                .cloned()
                .unwrap_or_default(),
        ),
        None => (current.to_string(), pages.to_string()),
    };
    let unit = labels.map(|_| {
        bar.popups
            .push(("unit", crate::preview_bar::unit_popup("Pages")));
        (
            Button::new("reader-unit", "", "Count by", "ui.preview_menu")
                .args(serde_json::json!({"kind":"unit"})),
            "unit",
        )
    });
    crate::preview_bar::number_tool(
        &mut bar,
        ("reader-page-number", "Go to page"),
        ("ui.preview_goto", serde_json::json!({})),
        (value, total),
        |shell| shell.preview.reader.page_input.as_ref(),
        unit,
        theme,
    );
    bar.push(Item::button(
        Slot::Navigate,
        2,
        Button::new("preview-next", "right", "Next page", "ui.preview_page")
            .args(serde_json::json!({"delta":1}))
            .enabled(current < pages),
    ));

    crate::preview_toolbar::zoom_tools(&mut bar, shell);
    bar.push(Item::button(
        Slot::View,
        1,
        Button::new(
            "reader-continuous",
            "continuous",
            "Continuous scrolling",
            "ui.reader_continuous",
        )
        .active(reader.scroll.continuous)
        .enabled(native),
    ));

    bar.push(
        Item::button(
            Slot::Tools,
            1,
            Button::new("reader-find", "search", label::FIND, "ui.preview_find")
                .enabled(native)
                .active(reader.find.is_some()),
        )
        .opens("find"),
    );
    bar.popups.push(("find", std::rc::Rc::new(find_panel)));
    crate::preview_bar::fullscreen_tool(&mut bar, shell, theme);
    crate::preview_bar::render(
        &bar,
        crate::preview::active_popup(shell, false),
        shell,
        theme,
        cx,
    )
}

/// The find field: its own keys and focus, anchored like a menu.
fn find_panel(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    use crate::toolbar::Button;
    let reader = &shell.preview.reader;
    let find = reader.find.as_ref()?;
    let count = if reader.search.is_pending() {
        "Searching…".to_owned()
    } else {
        format!(
            "{} / {}{}",
            reader.active_hit.map_or(0, |i| i + 1),
            reader.hits.len(),
            if reader.hits.len() == skwir_core::document::MAX_HITS {
                "+"
            } else {
                ""
            }
        )
    };
    let matches = |id, icon, title, delta: i32| {
        Button::new(id, icon, title, "ui.preview_match")
            .args(serde_json::json!({"delta":delta}))
            .enabled(!reader.hits.is_empty())
    };
    let buttons = vec![
        matches("reader-prev-match", "left", "Previous match", -1),
        matches("reader-next-match", "right", "Next match", 1),
        Button::new(
            "reader-close-find",
            "close",
            "Close find",
            "ui.preview_find_close",
        ),
    ];
    Some(crate::preview_bar::find_popup(
        find,
        crate::keymap::PREVIEW_FIND,
        count,
        buttons,
        shell,
        theme,
        cx,
    ))
}

/// The document's outline, while it is open and the document has one: the
/// part shown is the last entry at or before the page shown.
pub fn outline_drawer(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let reader = &shell.preview.reader;
    if !reader.outlines {
        return None;
    }
    let session = reader.session.as_ref()?;
    let outlines = &session.info().outlines;
    if outlines.is_empty() {
        return None;
    }
    let current =
        crate::outline::at_page(outlines.iter().map(|entry| entry.page), shell.preview.page);
    let entries = outlines
        .iter()
        .map(|entry| crate::outline::Entry {
            depth: entry.depth as usize,
            title: entry.title.clone().into(),
            command: entry
                .page
                .map(|page| ("ui.reader_page", serde_json::json!({"page":page}))),
        })
        .collect();
    Some(crate::outline::contents(
        entries,
        current,
        &reader.contents,
        ("ui.preview_contents", serde_json::json!({})),
        shell,
        theme,
        cx,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::document::TextChar;

    /// Whatever an engine or a plugin says, the reader shows at most
    /// `MAX_PAGES`: four billion would be as many list items.
    #[test]
    fn a_page_count_is_held_to_the_most_read() {
        struct Claims(skwir_core::document::DocumentInfo);
        impl DocumentSession for Claims {
            fn info(&self) -> &skwir_core::document::DocumentInfo {
                &self.0
            }
            fn render(
                &self,
                _: u32,
                _: u32,
                _: &CancelToken,
            ) -> anyhow::Result<skwir_core::document::DocumentPage> {
                anyhow::bail!("no pages")
            }
            fn search(&self, _: &str, _: &CancelToken) -> anyhow::Result<Vec<SearchHit>> {
                Ok(Vec::new())
            }
        }
        let claims = |pages| {
            Claims(skwir_core::document::DocumentInfo {
                pages,
                outlines: Vec::new(),
                reflowable: false,
                labels: Vec::new(),
            })
        };
        let most = skwir_core::document::MAX_PAGES;
        assert_eq!(page_count(&claims(u32::MAX)), most);
        assert_eq!(page_count(&claims(most + 1)), most);
        assert_eq!(page_count(&claims(12)), 12);
    }

    /// A page read out: in lines as the engine broke them, whatever it
    /// breaks them with, the last without its break.
    #[test]
    fn a_page_reads_in_its_lines() {
        let page = |texts: &[&str]| PageContent {
            page: 1,
            width: 100.,
            height: 100.,
            links: vec![],
            chars: texts
                .iter()
                .map(|s| TextChar {
                    text: (*s).to_owned(),
                    bounds: Rect::default(),
                })
                .collect(),
        };
        assert_eq!(
            page_lines(&page(&["H", "i", "\n", "猫", "\r\n", "fin", "\n"])),
            ["Hi\n", "猫\n", "fin"]
        );
        assert!(page_lines(&page(&[])).is_empty());
    }
    #[test]
    fn reverse_selection_copies_unicode_in_document_order() {
        let page = PageContent {
            page: 1,
            width: 100.,
            height: 100.,
            links: vec![],
            chars: ["A", "é", "\n", "猫"]
                .into_iter()
                .map(|s| TextChar {
                    text: s.into(),
                    bounds: Rect::default(),
                })
                .collect(),
        };
        assert_eq!(selected_text(&page, Some((3, 1))), "é\n猫");
        assert_eq!(selected_text(&page, None), "");
    }
}
