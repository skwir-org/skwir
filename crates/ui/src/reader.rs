//! Stateful document reader: selection, search, outline and link navigation.
//! Engine operations run on the scheduler; this module only owns UI state.

use crate::{
    input::{InputEvent, TextInput},
    preview::{PreviewContent, preview_button},
    shell::Shell,
    theme::Theme,
};
use bibli_core::{
    CancelToken,
    document::{Destination, DocumentSession, PageContent, Rect, SearchHit},
    scheduler::Priority,
};
use gpui::{
    AnyElement, ClipboardItem, Context, Entity, FocusHandle, RenderImage, Subscription, Window,
    div, prelude::*, px,
};
use std::{rc::Rc, sync::Arc};

struct SearchResult {
    generation: u64,
    result: Result<Vec<SearchHit>, String>,
}
pub struct FindInput {
    input: Entity<TextInput>,
    _subscription: Subscription,
}
pub struct ReaderState {
    pub session: Option<Arc<dyn DocumentSession>>,
    pub lifetime: CancelToken,
    pub tried: bool,
    pub focus: Option<FocusHandle>,
    pub selection: Option<(usize, usize)>,
    pub outlines: bool,
    find: Option<FindInput>,
    hits: Vec<SearchHit>,
    active_hit: Option<usize>,
    search_cancel: Option<CancelToken>,
    generation: u64,
    searching: bool,
    tx: flume::Sender<SearchResult>,
    rx: flume::Receiver<SearchResult>,
    pub down: Option<(f32, f32)>,
}
impl Default for ReaderState {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        Self {
            session: None,
            lifetime: CancelToken::new(),
            tried: false,
            focus: None,
            selection: None,
            outlines: false,
            find: None,
            hits: Vec::new(),
            active_hit: None,
            search_cancel: None,
            generation: 0,
            searching: false,
            tx,
            rx,
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
    pub fn cancel_search(&mut self) {
        if let Some(cancel) = self.search_cancel.take() {
            cancel.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
        self.searching = false;
    }
}

fn page(shell: &Shell) -> Option<&Arc<PageContent>> {
    match &shell.preview.content {
        Some(PreviewContent::Image {
            document: Some(page),
            ..
        }) => Some(page),
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
    pub fn reader_pointer(
        &mut self,
        phase: &str,
        x: f32,
        y: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = page(self).cloned() else {
            return;
        };
        if phase == "down" {
            if let Some(focus) = &self.preview.reader.focus {
                window.focus(focus);
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
                    self.run(
                        "ui.reader_link",
                        serde_json::json!({"index":index}),
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

    pub fn reader_copy(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = page(self) {
            let text = selected_text(page, self.preview.reader.selection);
            if !text.is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
    }
    pub fn reader_select_all(&mut self, cx: &mut Context<Self>) {
        self.preview.reader.selection =
            page(self).and_then(|p| p.chars.len().checked_sub(1).map(|last| (0, last)));
        cx.notify();
    }
    pub fn reader_link(&mut self, index: usize, cx: &mut Context<Self>) {
        let target = page(self)
            .and_then(|p| p.links.get(index))
            .map(|l| l.destination.clone());
        match target {
            Some(Destination::Page(page)) => self.preview_go_page(page, cx),
            Some(Destination::Uri(uri))
                if uri.starts_with("https://") || uri.starts_with("http://") =>
            {
                let app = Arc::clone(&self.app);
                let cancel = self.app.lifetime().child();
                self.app
                    .scheduler
                    .spawn(Priority::Visible, "document-link", cancel, move || {
                        if let Err(error) = bibli_platform::open::open_web_uri(&uri) {
                            app.notify(bibli_core::app::NotifyLevel::Error, error.to_string());
                        }
                    });
            }
            Some(Destination::Uri(_)) => self.set_status(
                "Only web links and links within this document are supported",
                true,
                cx,
            ),
            None => {}
        }
    }
    pub fn reader_outline(&mut self, cx: &mut Context<Self>) {
        self.preview.reader.outlines = !self.preview.reader.outlines;
        cx.notify();
    }
    pub fn reader_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.reader.session.is_none() {
            return;
        }
        if self.preview.reader.find.is_none() {
            let input = cx.new(|cx| TextInput::new("Find in document…", cx));
            let subscription =
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Submit => {
                            this.run("ui.reader_search", serde_json::json!({}), window, cx);
                        }
                        InputEvent::Cancel => {
                            this.run("ui.reader_find_close", serde_json::json!({}), window, cx);
                        }
                        InputEvent::Changed => cx.notify(),
                    }
                });
            self.preview.reader.find = Some(FindInput {
                input,
                _subscription: subscription,
            });
        }
        if let Some(find) = &self.preview.reader.find {
            find.input.read(cx).focus(window);
        }
        cx.notify();
    }
    pub fn reader_find_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reader = &mut self.preview.reader;
        if let Some(cancel) = reader.search_cancel.take() {
            cancel.cancel();
        }
        reader.generation = reader.generation.wrapping_add(1);
        reader.searching = false;
        reader.find = None;
        reader.hits.clear();
        reader.active_hit = None;
        if let Some(focus) = &reader.focus {
            window.focus(focus);
        }
        cx.notify();
    }
    pub fn reader_search(&mut self, cx: &mut Context<Self>) {
        let reader = &mut self.preview.reader;
        let Some(session) = reader.session.clone() else {
            return;
        };
        let Some(find) = &reader.find else {
            return;
        };
        let query = find.input.read(cx).text().to_owned();
        if let Some(cancel) = reader.search_cancel.take() {
            cancel.cancel();
        }
        let cancel = reader.lifetime.child();
        reader.search_cancel = Some(cancel.clone());
        reader.generation = reader.generation.wrapping_add(1);
        let generation = reader.generation;
        let tx = reader.tx.clone();
        let wake = self.app.waker_handle();
        reader.searching = true;
        reader.hits.clear();
        reader.active_hit = None;
        self.app.scheduler.spawn(
            Priority::Normal,
            "document-search",
            cancel.clone(),
            move || {
                let result = session
                    .search(&query, &cancel)
                    .map_err(|e| format!("{e:#}"));
                if !cancel.is_cancelled() {
                    let _ = tx.send(SearchResult { generation, result });
                    wake();
                }
            },
        );
        cx.notify();
    }
    pub fn sync_reader(&mut self, cx: &mut Context<Self>) {
        while let Ok(reply) = self.preview.reader.rx.try_recv() {
            if reply.generation != self.preview.reader.generation {
                continue;
            }
            self.preview.reader.searching = false;
            cx.notify();
            match reply.result {
                Ok(hits) => {
                    let first = hits.first().map(|h| h.page);
                    self.preview.reader.hits = hits;
                    self.preview.reader.active_hit = first.map(|_| 0);
                    if let Some(first) = first {
                        self.preview_go_page(first, cx);
                    }
                }
                Err(error) => self.set_status(error, true, cx),
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
    let reader = &shell.preview.reader;
    let selection = reader.selection.map_or_else(Vec::new, |(a, b)| {
        page.chars
            .iter()
            .skip(a.min(b))
            .take(a.abs_diff(b) + 1)
            .map(|ch| ch.bounds)
            .collect()
    });
    let highlights = reader
        .active_hit
        .and_then(|i| reader.hits.get(i))
        .filter(|h| h.page == page.page)
        .map_or_else(Vec::new, |h| h.bounds.clone());
    let entity = cx.entity().downgrade();
    let element = crate::elements::document::DocumentElement {
        image,
        page,
        selection,
        highlights,
        zoom,
        color: cx.global::<Theme>().accent,
        on_pointer: Rc::new(move |phase, x, y, window, cx| {
            if let Some(entity) = entity.upgrade() {
                entity.update(cx, |shell, cx| {
                    shell.run(
                        "ui.reader_pointer",
                        serde_json::json!({"phase":phase,"x":x,"y":y}),
                        window,
                        cx,
                    );
                });
            }
        }),
    };
    div()
        .id("reader-canvas")
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_scroll()
        .when_some(reader.focus.as_ref(), |d, focus| {
            d.track_focus(focus).key_context(crate::keymap::READER)
        })
        .child(element)
        .into_any_element()
}

pub fn toolbar(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let reader = &shell.preview.reader;
    let button = |id, label, command, args, cx: &mut Context<Shell>| {
        preview_button(id, label, command, args, theme, cx)
    };
    let mut body = div()
        .flex()
        .flex_col()
        .text_xs()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .gap_1()
                .p_1()
                .child(button(
                    "reader-outline",
                    "Contents",
                    "ui.reader_outline",
                    serde_json::json!({}),
                    cx,
                ))
                .child(button(
                    "reader-find",
                    "Find",
                    "ui.reader_find",
                    serde_json::json!({}),
                    cx,
                ))
                .child(button(
                    "reader-copy",
                    "Copy",
                    "ui.reader_copy",
                    serde_json::json!({}),
                    cx,
                )),
        );
    if reader.outlines
        && let Some(session) = &reader.session
    {
        let entries = &session.info().outlines;
        let mut list = div()
            .id("reader-outline-list")
            .max_h(px(180.))
            .overflow_y_scroll()
            .p_1();
        if entries.is_empty() {
            list = list.child("No document outline");
        }
        for (index, entry) in entries.iter().enumerate() {
            let page = entry.page;
            list = list.child(
                div()
                    .id(("outline-entry", index))
                    .pl(px(entry.depth.min(12) as f32 * 10.))
                    .truncate()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.border))
                    .child(entry.title.clone())
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        if let Some(page) = page {
                            shell.run(
                                "ui.reader_page",
                                serde_json::json!({"page":page}),
                                window,
                                cx,
                            );
                        }
                    })),
            );
        }
        body = body.child(list);
    }
    if let Some(find) = &reader.find {
        body = body
            .child(div().px_2().py_1().child(find.input.clone()))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .items_center()
                    .px_1()
                    .child(if reader.searching {
                        "Searching…".to_owned()
                    } else {
                        format!(
                            "{} / {}{}",
                            reader.active_hit.map_or(0, |i| i + 1),
                            reader.hits.len(),
                            if reader.hits.len() == bibli_core::document::MAX_HITS {
                                " (limit)"
                            } else {
                                ""
                            }
                        )
                    })
                    .child(button(
                        "reader-prev-match",
                        "‹",
                        "ui.reader_match",
                        serde_json::json!({"delta":-1}),
                        cx,
                    ))
                    .child(button(
                        "reader-next-match",
                        "›",
                        "ui.reader_match",
                        serde_json::json!({"delta":1}),
                        cx,
                    ))
                    .child(button(
                        "reader-close-find",
                        "×",
                        "ui.reader_find_close",
                        serde_json::json!({}),
                        cx,
                    )),
            );
    }
    body.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::document::TextChar;
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
