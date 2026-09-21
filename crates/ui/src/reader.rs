//! Stateful document reader: selection, search, outline and link navigation.
//! Engine operations run on the scheduler; this module only owns UI state.

use crate::{
    input::{InputEvent, TextInput},
    preview::PreviewContent,
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
    pub scroll: crate::reader_scroll::DocumentScroll,
    pub selection_page: Option<u32>,
    pub session: Option<Arc<dyn DocumentSession>>,
    pub lifetime: CancelToken,
    pub tried: bool,
    pub focus: Option<FocusHandle>,
    pub selection: Option<(usize, usize)>,
    pub outlines: bool,
    find: Option<FindInput>,
    pub page_input: Option<FindInput>,
    pub(crate) hits: Vec<SearchHit>,
    pub(crate) active_hit: Option<usize>,
    search_cancel: Option<CancelToken>,
    generation: u64,
    pub(crate) searching: bool,
    tx: flume::Sender<SearchResult>,
    rx: flume::Receiver<SearchResult>,
    pub down: Option<(f32, f32)>,
}
impl Default for ReaderState {
    fn default() -> Self {
        let (tx, rx) = flume::unbounded();
        Self {
            scroll: crate::reader_scroll::DocumentScroll::default(),
            selection_page: None,
            session: None,
            lifetime: CancelToken::new(),
            tried: false,
            focus: None,
            selection: None,
            outlines: false,
            find: None,
            page_input: None,
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
    /// Whether the find field is open.
    pub fn finding(&self) -> bool {
        self.find.is_some()
    }
    pub fn cancel_search(&mut self) {
        if let Some(cancel) = self.search_cancel.take() {
            cancel.cancel();
        }
        self.generation = self.generation.wrapping_add(1);
        self.searching = false;
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
                detail: Some(bibli_core::preview::PreviewInfo::Document { .. }),
                ..
            })
        ) {
            return;
        }
        let input = cx.new(|cx| TextInput::new("Page", cx));
        input.update(cx, |input, cx| {
            input.set_text(self.preview.page.to_string(), cx);
            input.select_all_now(cx);
            input.focus(window);
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                let command = match event {
                    InputEvent::Submit => "ui.reader_page_submit",
                    InputEvent::Cancel => "ui.reader_page_cancel",
                    InputEvent::Changed => {
                        cx.notify();
                        return;
                    }
                };
                this.run(command, serde_json::json!({}), window, cx);
            });
        self.preview.reader.page_input = Some(FindInput {
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub fn reader_page_submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let number = self
            .preview
            .reader
            .page_input
            .as_ref()
            .and_then(|p| p.input.read(cx).text().trim().parse::<u32>().ok());
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
            window.focus(focus);
        } else {
            window.focus(&self.ui().focus);
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
        self.preview.reader.selection_page = Some(self.preview.page);
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
    pub fn reader_outline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview.reader.outlines = !self.preview.reader.outlines;
        if !self.preview.reader.outlines {
            window.focus(
                self.preview
                    .reader
                    .focus
                    .as_ref()
                    .unwrap_or(&self.ui().focus),
            );
        }
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
    if shell.preview.reader.scroll.continuous {
        return crate::reader_scroll::continuous_element(shell, image, page, cx);
    }
    let element = page_surface(shell, image, page, zoom, shell.preview.fit_width, cx);
    div()
        .id("reader-canvas")
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_hidden()
        .track_scroll(&shell.preview.reader.scroll.single)
        .on_scroll_wheel(cx.listener(|shell, event, window, cx| {
            shell.reader_wheel(event, window, cx);
        }))
        .when_some(shell.preview.reader.focus.as_ref(), |d, focus| {
            d.track_focus(focus).key_context(crate::keymap::READER)
        })
        .child(element)
        .into_any_element()
}

pub(crate) fn page_surface(
    shell: &Shell,
    image: Arc<RenderImage>,
    page: Arc<PageContent>,
    zoom: Option<f32>,
    fit_width: bool,
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
        cx.global::<Theme>().accent,
        cx.entity().downgrade(),
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
    color: gpui::Hsla,
    entity: gpui::WeakEntity<Shell>,
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
        color,
        on_pointer: Rc::new(move |phase, x, y, window, cx| {
            if let Some(entity) = entity.upgrade() {
                entity.update(cx, |shell, cx| {
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

/// Pages of the document shown, when it is one.
pub(crate) fn pages(shell: &Shell) -> Option<u32> {
    let document = shell.preview.reader.session.is_some()
        || matches!(
            &shell.preview.content,
            Some(PreviewContent::Image {
                detail: Some(bibli_core::preview::PreviewInfo::Document { .. }),
                ..
            })
        );
    document.then(|| document_pages(shell))
}

fn document_pages(shell: &Shell) -> u32 {
    shell
        .preview
        .reader
        .session
        .as_ref()
        .map(|s| s.info().pages)
        .or({
            if let Some(PreviewContent::Image {
                detail: Some(bibli_core::preview::PreviewInfo::Document { pages, .. }),
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
            "ui.reader_outline",
        )
        .active(reader.outlines)
        .enabled(native),
    ));
    crate::preview_bar::parent_tool(&mut bar, shell);

    bar.push(Item::button(
        Slot::Navigate,
        2,
        Button::new(
            "preview-prev",
            "left",
            "Previous page · Alt+Page Up",
            "ui.preview_page",
        )
        .args(serde_json::json!({"delta":-1}))
        .enabled(current > 1),
    ));
    let (accent, border, paper, muted) = (theme.accent, theme.border, theme.bg, theme.text_muted);
    bar.push(Item::widget(
        Slot::Navigate,
        2,
        move |shell, cx| match &shell.preview.reader.page_input {
            Some(editor) => div()
                .w(px(44.))
                .flex_none()
                .h(px(28.))
                .px_1()
                .border_1()
                .border_color(accent)
                .rounded_md()
                .bg(paper)
                .child(editor.input.clone())
                .into_any_element(),
            None => Button::new(
                "reader-page-number",
                "",
                "Go to page",
                "ui.reader_page_edit",
            )
            .label(shell.preview.page.to_string())
            .render(shell, cx)
            .bg(paper)
            .border_color(border)
            .into_any_element(),
        },
    ));
    bar.push(Item::widget(Slot::Navigate, 1, move |_, _| {
        div()
            .flex_none()
            .text_xs()
            .text_color(muted)
            .whitespace_nowrap()
            .child(format!("/ {pages}"))
            .into_any_element()
    }));
    bar.push(Item::button(
        Slot::Navigate,
        2,
        Button::new(
            "preview-next",
            "right",
            "Next page · Alt+Page Down",
            "ui.preview_page",
        )
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
            Button::new("reader-find", "search", label::FIND, "ui.reader_find")
                .enabled(native)
                .active(reader.find.is_some()),
        )
        .opens("find"),
    );
    bar.popups.push(("find", std::rc::Rc::new(find_panel)));
    crate::preview_bar::render(&bar, crate::preview::active_popup(shell), shell, theme, cx)
}

/// The find field: its own keys and focus, anchored like a menu.
fn find_panel(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    use crate::toolbar::Button;
    let reader = &shell.preview.reader;
    let find = reader.find.as_ref()?;
    let count = if reader.searching {
        "Searching…".to_owned()
    } else {
        format!(
            "{} / {}{}",
            reader.active_hit.map_or(0, |i| i + 1),
            reader.hits.len(),
            if reader.hits.len() == bibli_core::document::MAX_HITS {
                "+"
            } else {
                ""
            }
        )
    };
    let matches = |id, icon, title, delta: i32| {
        Button::new(id, icon, title, "ui.reader_match")
            .args(serde_json::json!({"delta":delta}))
            .enabled(!reader.hits.is_empty())
    };
    Some(
        div()
            .tab_group()
            .key_context("DocumentFind")
            .w(px(280.))
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.bg)
            .shadow_lg()
            .occlude()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(theme.accent)
                    .rounded_md()
                    .child(find.input.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .child(div().flex_1().text_color(theme.text_muted).child(count))
                    .child(
                        matches("reader-prev-match", "left", "Previous match · Shift+F3", -1)
                            .render(shell, cx),
                    )
                    .child(
                        matches("reader-next-match", "right", "Next match · F3", 1)
                            .render(shell, cx),
                    )
                    .child(
                        Button::new(
                            "reader-close-find",
                            "close",
                            "Close find · Escape",
                            "ui.reader_find_close",
                        )
                        .render(shell, cx),
                    ),
            )
            .into_any_element(),
    )
}

pub fn outline_drawer(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let reader = &shell.preview.reader;
    if !reader.outlines {
        return None;
    }
    let entries = reader
        .session
        .as_ref()?
        .info()
        .outlines
        .iter()
        .map(|entry| crate::outline::Entry {
            depth: entry.depth as usize,
            title: entry.title.clone().into(),
            command: entry
                .page
                .map(|page| ("ui.reader_page", serde_json::json!({"page":page}))),
        })
        .collect();
    Some(crate::outline::drawer(
        entries,
        ("ui.reader_outline", serde_json::json!({})),
        shell,
        theme,
        cx,
    ))
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
