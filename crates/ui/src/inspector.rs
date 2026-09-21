//! Shared native viewers for optional structured preview plugins.
use crate::{
    preview::{PreviewContent, PreviewPhase},
    shell::Shell,
    theme::Theme,
    toolbar::{self, Button},
};
use bibli_core::inspection::{InspectRequest, Inspection, InspectionKind};
use gpui::{
    AnyElement, Context, ListAlignment, ListState, RenderImage, Window, div, prelude::*, px,
};
use std::{collections::BTreeSet, sync::Arc};
pub struct Display {
    pub data: Arc<Inspection>,
    pub images: Vec<Option<Arc<RenderImage>>>,
    row_icons: Arc<Vec<Option<Arc<std::path::Path>>>>,
    /// Case-folded search text per block, then per row, prepared on the
    /// worker so Find never lowercases a document on the UI thread.
    folded: Arc<Vec<String>>,
}
impl Display {
    pub fn new(mut data: Inspection, dark: bool) -> Self {
        // Display conversion runs on a worker. Resolve themed archive icons here,
        // never by inspecting archive member paths or doing I/O while drawing rows.
        let row_icons = if data.kind == InspectionKind::Archive {
            data.rows
                .iter()
                .map(|row| {
                    if row.branch {
                        bibli_platform::icons::folder(18)
                    } else {
                        row.cells
                            .first()
                            .and_then(|name| bibli_platform::mime::by_name(name.as_ref()))
                            .and_then(|mime| bibli_platform::icons::for_mime(&mime, 18))
                    }
                    .map(Arc::from)
                })
                .collect()
        } else {
            Vec::new()
        };
        if data.kind == InspectionKind::Code {
            crate::code_preview::highlight(&mut data, dark);
        } else {
            data.blocks = bibli_core::inspection::split_blocks(data.blocks);
        }
        for block in &mut data.blocks {
            if !block.code || data.kind == InspectionKind::Code {
                continue;
            }
            let syntaxes = crate::preview::syntaxes(block.language.as_deref());
            let syntax = block
                .language
                .as_deref()
                .and_then(|language| syntaxes.find_syntax_by_token(language))
                .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
            let theme = &crate::preview::themes().themes[if dark {
                "base16-ocean.dark"
            } else {
                "base16-ocean.light"
            }];
            let mut highlight = syntect::easy::HighlightLines::new(syntax, theme);
            let mut offset = 0;
            block.runs.clear();
            for line in syntect::util::LinesWithEndings::from(&block.text) {
                if let Ok(ranges) = highlight.highlight_line(line, syntaxes) {
                    for (style, text) in ranges {
                        block.runs.push(bibli_core::inspection::Run {
                            range: offset..offset + text.len(),
                            color: Some([
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                            ]),
                            code: true,
                            ..bibli_core::inspection::Run::default()
                        });
                        offset += text.len();
                    }
                } else {
                    offset += line.len();
                }
            }
        }
        let images = std::iter::once(data.image.take())
            .chain(data.blocks.iter_mut().map(|b| b.image.take()))
            .map(|i| i.and_then(|i| crate::thumbs::to_render_image(&i)))
            .collect();
        let folded = data
            .blocks
            .iter()
            .map(|b| b.text.to_lowercase())
            .chain(data.rows.iter().map(|r| r.cells.join("\t").to_lowercase()))
            .collect();
        Self {
            data: Arc::new(data),
            images,
            row_icons: Arc::new(row_icons),
            folded: Arc::new(folded),
        }
    }
    /// The next block or row (blocks first, then rows offset by the block
    /// count) containing `query`, which must already be lowercase. Starts after
    /// `after` and wraps to the beginning when `wrap` is set.
    pub fn find(&self, query: &str, after: Option<usize>, wrap: bool) -> Option<usize> {
        if query.is_empty() || self.folded.is_empty() {
            return None;
        }
        let start = after.map_or(0, |i| i + 1);
        let hit = |range: std::ops::Range<usize>| {
            range.into_iter().find(|&i| self.folded[i].contains(query))
        };
        hit(start.min(self.folded.len())..self.folded.len())
            .or_else(|| wrap.then(|| hit(0..start.min(self.folded.len()))).flatten())
    }
}
/// Bring a list item into view. `ListState::scroll_to_reveal_item` walks the
/// measured heights to get there, so it can only reach what the list has
/// already laid out; a heading or a match further down would barely move the
/// view. Anything unmeasured is jumped to instead, landing at the top.
fn scroll_into_view(list: &ListState, item: usize) {
    if list.bounds_for_item(item).is_some() {
        list.scroll_to_reveal_item(item);
    } else {
        list.scroll_to(gpui::ListOffset {
            item_ix: item,
            offset_in_item: px(0.),
        });
    }
}
pub struct State {
    pub focus: Option<gpui::FocusHandle>,
    pub request: InspectRequest,
    list: ListState,
    visible: Arc<Vec<usize>>,
    collapsed: BTreeSet<usize>,
    selected: Option<usize>,
    selection: Option<crate::inspector_text::Selection>,
    dragging: bool,
    font_size: f32,
    outline: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            request: InspectRequest::default(),
            focus: None,
            list: ListState::new(0, ListAlignment::Top, px(100.)),
            visible: Arc::new(Vec::new()),
            collapsed: BTreeSet::new(),
            selected: None,
            selection: None,
            dragging: false,
            font_size: 16.,
            outline: false,
        }
    }
}
impl State {
    pub fn reset(&mut self, data: &Inspection) {
        self.collapsed.clear();
        self.selected = None;
        self.selection = None;
        self.dragging = false;
        self.outline = false;
        self.reindex(data);
    }
    /// A new inspection replaced the content. Its `position` becomes the
    /// request offset so paging and search continue from what is shown; a
    /// book chapter found by search also reveals its first matching block.
    pub fn arrived(&mut self, display: &Display) {
        self.reset(&display.data);
        self.request.offset = display.data.position;
        if display.data.kind == InspectionKind::Book
            && let Some(query) = self.request.query.as_deref()
            && let Some(i) = display.find(&query.to_lowercase(), None, false)
        {
            self.reveal(&display.data, i);
        }
    }
    /// Select and scroll to a block or row, expanding anything that hides it.
    fn reveal(&mut self, data: &Inspection, index: usize) {
        self.selected = Some(index);
        self.collapsed.clear();
        self.reindex(data);
        scroll_into_view(&self.list, index + 1);
    }
    fn toggle_collapsed(&mut self, data: &Inspection, index: usize) {
        if !self.collapsed.remove(&index) {
            self.collapsed.insert(index);
        }
        self.reindex(data);
    }
    fn collapse_all(&mut self, data: &Inspection, collapse: bool) {
        self.collapsed = if !collapse {
            BTreeSet::new()
        } else if data.kind == InspectionKind::Code {
            data.folds.iter().map(|fold| fold.start).collect()
        } else {
            data.rows
                .iter()
                .enumerate()
                .filter_map(|(i, r)| r.branch.then_some(i))
                .collect()
        };
        self.reindex(data);
    }
    pub fn visible(&self) -> &[usize] {
        &self.visible
    }
    /// Whether the hex provider owns the shown file: explicitly selected, or
    /// the automatic byte view of a whole file. A container member's byte page
    /// (`target` set) belongs to its container provider, which cannot search.
    fn hex_file(&self, kind: InspectionKind) -> bool {
        self.request.provider.as_deref() == Some("preview-hex")
            || (kind == InspectionKind::Hex && self.request.target.is_none())
    }
    /// What a row is worth before it has been drawn. A list only measures
    /// what it renders, so without an estimate for the rest the scrollbar
    /// would describe the few blocks on screen instead of the document.
    /// A code line is one line; anything else is a short paragraph. Measured
    /// rows replace the estimate as they are drawn.
    fn row_estimate(&self, data: &Inspection) -> gpui::Pixels {
        px(match data.kind {
            InspectionKind::Code => self.font_size + 4.,
            InspectionKind::Hex
            | InspectionKind::Archive
            | InspectionKind::Table
            | InspectionKind::Tree => 24.,
            // Prose wraps: call it two and a half lines and the space around
            // them, between a heading and a long paragraph.
            _ => self.font_size * 1.5 * 2.5 + 16.,
        })
    }
    fn reindex(&mut self, data: &Inspection) {
        let estimate = self.row_estimate(data);
        if data.kind == InspectionKind::Code {
            self.visible = Arc::new(crate::code_preview::visible(data, &self.collapsed));
            self.list
                .reset_with_uniform_height(self.visible.len() + 1, estimate);
            return;
        }
        let mut hidden = None;
        self.visible = Arc::new(
            data.rows
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    if hidden.is_some_and(|d| r.depth > d) {
                        return None;
                    }
                    hidden = None;
                    if self.collapsed.contains(&i) {
                        hidden = Some(r.depth);
                    }
                    Some(i)
                })
                .collect(),
        );
        self.list
            .reset_with_uniform_height(data.blocks.len() + self.visible.len() + 1, estimate);
    }
}
impl Shell {
    pub fn inspect_command(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = args["action"].as_str().unwrap_or("auto");
        if action == "search" || action == "offset" {
            let pane = self.pane;
            let generation = self.preview.generation;
            let kind = if action == "search" {
                crate::prompts::PromptKind::InspectSearch { pane, generation }
            } else {
                crate::prompts::PromptKind::InspectJump { pane, generation }
            };
            self.open_prompt(
                kind,
                if action == "search" {
                    "Find in preview"
                } else {
                    "Jump to byte offset"
                },
                "",
                None,
                window,
                cx,
            );
            return;
        }
        let kind = match &self.preview.content {
            Some(PreviewContent::Inspection(display)) => Some(display.data.kind),
            _ => None,
        };
        if let Some(PreviewContent::Inspection(display)) = &self.preview.content {
            match action {
                "text_down" | "text_move" => {
                    if action == "text_move" && !self.preview.inspection.dragging {
                        return;
                    }
                    let index = args["index"].as_u64().unwrap_or(0) as usize;
                    if let Some(block) = display.data.blocks.get(index) {
                        let mut offset =
                            (args["offset"].as_u64().unwrap_or(0) as usize).min(block.text.len());
                        while !block.text.is_char_boundary(offset) {
                            offset -= 1;
                        }
                        let state = &mut self.preview.inspection;
                        state.selected = Some(index);
                        if action == "text_down" {
                            if let Some(focus) = &state.focus {
                                window.focus(focus);
                            }
                            state.selection = Some(crate::inspector_text::Selection {
                                start: (index, offset),
                                end: (index, offset),
                            });
                            state.dragging = true;
                        } else if let Some(selection) = &mut state.selection {
                            selection.end = (index, offset);
                        }
                        cx.notify();
                    }
                    return;
                }
                "text_up" => {
                    self.preview.inspection.dragging = false;
                    return;
                }
                "copy" => {
                    if let Some(text) = self
                        .preview
                        .inspection
                        .selection
                        .and_then(|s| s.text(&display.data.blocks))
                    {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        return;
                    }

                    let text = self
                        .preview
                        .inspection
                        .selected
                        .and_then(|i| {
                            display
                                .data
                                .blocks
                                .get(i)
                                .map(|b| b.text.clone())
                                .or_else(|| {
                                    display
                                        .data
                                        .rows
                                        .get(i.saturating_sub(display.data.blocks.len()))
                                        .map(|r| r.cells.join("\t"))
                                })
                        })
                        .unwrap_or_else(|| display.data.plain_text());
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    return;
                }
                "select" => {
                    self.preview.inspection.selected = args["index"].as_u64().map(|i| i as usize);
                    if let Some(focus) = &self.preview.inspection.focus {
                        window.focus(focus);
                    }
                    cx.notify();
                    return;
                }
                "collapse" => {
                    if let Some(i) = args["index"].as_u64() {
                        self.preview
                            .inspection
                            .toggle_collapsed(&display.data, i as usize);
                    }
                    cx.notify();
                    return;
                }
                "expand_all" | "collapse_all"
                    if matches!(
                        display.data.kind,
                        InspectionKind::Archive | InspectionKind::Code
                    ) =>
                {
                    self.preview
                        .inspection
                        .collapse_all(&display.data, action == "collapse_all");
                    cx.notify();
                    return;
                }
                "larger" | "smaller" => {
                    let data = display.data.clone();
                    let state = &mut self.preview.inspection;
                    state.font_size = (state.font_size + if action == "larger" { 2. } else { -2. })
                        .clamp(10., 40.);
                    // Every row is a different height now, estimate included.
                    let estimate = state.row_estimate(&data);
                    state
                        .list
                        .reset_with_uniform_height(state.list.item_count(), estimate);
                    cx.notify();
                    return;
                }
                "outline" => {
                    self.preview.inspection.outline = !self.preview.inspection.outline;
                    cx.notify();
                    return;
                }
                "block" => {
                    if let Some(i) = args["index"].as_u64() {
                        scroll_into_view(&self.preview.inspection.list, i as usize + 1);
                    }
                    cx.notify();
                    return;
                }
                // The hex provider searches the whole file itself; Find sets a
                // font's specimen text. Everything else, including a container
                // member's byte page, searches the loaded section after the
                // selected item. A book falls back to its provider for the
                // following chapters.
                "find"
                    if !self.preview.inspection.hex_file(display.data.kind)
                        && display.data.kind != InspectionKind::Font =>
                {
                    let query = args["value"].as_str().unwrap_or("").to_lowercase();
                    let book = display.data.kind == InspectionKind::Book;
                    let state = &mut self.preview.inspection;
                    if let Some(i) = display.find(&query, state.selected, !book) {
                        state.reveal(&display.data, i);
                        cx.notify();
                        return;
                    }
                    if !book {
                        self.set_status("No match in this preview section", false, cx);
                        cx.notify();
                        return;
                    }
                }
                _ => {}
            }
        }
        if action == "link" {
            if self
                .preview
                .inspection
                .selection
                .is_some_and(|s| s.start != s.end)
            {
                return;
            }
            if let Some(anchor) = args["target"].as_str().and_then(|s| s.strip_prefix('#')) {
                if let Some(PreviewContent::Inspection(display)) = &self.preview.content
                    && let Some(i) = display
                        .data
                        .blocks
                        .iter()
                        .position(|b| b.anchor.as_deref() == Some(anchor))
                {
                    scroll_into_view(&self.preview.inspection.list, i + 1);
                    cx.notify();
                }
                return;
            }
            let href = args["target"].as_str().unwrap_or("");
            if href.starts_with("https://")
                || href.starts_with("http://")
                || href.starts_with("mailto:")
            {
                cx.open_url(href);
                return;
            }
            // A link to a neighbouring file or folder is a place in the
            // filesystem: go there, select it, and let the preview follow the
            // selection. Only a book's links mean something inside a
            // container, so those fall through to the provider below.
            if kind != Some(InspectionKind::Book) {
                match self
                    .preview
                    .path()
                    .and_then(|from| document_link(from, href))
                {
                    Some(target) => {
                        let pane = self.pane;
                        if !self.reveal_path(pane, &target, window, cx) {
                            let name = target.file_name().map_or_else(
                                || target.display().to_string(),
                                |name| name.to_string_lossy().into_owned(),
                            );
                            self.set_status(format!("{name} is not there any more"), true, cx);
                        }
                    }
                    None => self.set_status("This link goes nowhere we can open", false, cx),
                }
                return;
            }
        }
        let hex = crate::preview_toolbar::hex_available(self);
        let state = &mut self.preview.inspection;
        let previous_query = state.request.query.take();
        match action {
            "hex" => {
                if !hex {
                    return;
                }
                state.request = InspectRequest {
                    provider: Some("preview-hex".into()),
                    ..InspectRequest::default()
                };
            }
            "auto" => state.request = InspectRequest::default(),
            "source" => {
                state.request.source = !state.request.source;
                state.request.offset = 0;
            }
            "page" => {
                state.request.offset = args["offset"].as_u64().unwrap_or(0);
                state.request.target = None;
            }
            "target" | "link" => {
                state.request.target = args["target"].as_str().map(str::to_owned);
                state.request.offset = 0;
            }
            "back" => {
                state.request.target = None;
                state.request.offset = 0;
            }
            "find" => {
                let query = args["value"].as_str().map(str::to_owned);
                // Searching providers report where they landed as `position`,
                // which `arrived` stored as the offset: hex the exact match
                // byte, a book its chapter. Resume one past it for the next
                // match of the same query; a book got here because the shown
                // chapter has no further match, so always move on.
                let resume = match kind {
                    Some(InspectionKind::Book) => true,
                    Some(kind) if state.hex_file(kind) => {
                        query.is_some() && query == previous_query
                    }
                    _ => false,
                };
                if resume {
                    state.request.offset = state.request.offset.saturating_add(1);
                }
                state.request.query = query;
            }
            "jump" => {
                let value = args["value"].as_str().unwrap_or("");
                let parsed = if let Some(hex) = value.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16)
                } else {
                    value.parse()
                };
                if let Ok(n) = parsed {
                    state.request.offset = n;
                } else {
                    self.set_status("Enter a decimal or hexadecimal offset", true, cx);
                    return;
                }
            }
            _ => return,
        }
        self.cancel_pending_preview();
        self.media_exit_fullscreen(window, cx);
        self.preview.media = crate::media::MediaState::default();
        self.clear_document_pages(window, cx);
        self.preview.reader = crate::reader::ReaderState::default();
        self.preview.phase = PreviewPhase::Queued;
        self.preview.menu = None;
        cx.notify();
    }
}
/// Whether the link names a URI scheme (`mailto:`, `irc:`, `https:`) rather
/// than a path. A colon after a slash belongs to a file name, not a scheme.
fn has_scheme(href: &str) -> bool {
    let Some(colon) = href.find(':') else {
        return false;
    };
    let scheme = &href[..colon];
    !scheme.is_empty()
        && !scheme.contains('/')
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Where a document's link points on this machine, or `None` when it names
/// something we cannot resolve (another scheme, or a path that escapes into
/// nothing). `from` is the document holding the link, so a relative link is
/// read the way the author wrote it: next to their file.
fn document_link(from: &std::path::Path, href: &str) -> Option<std::path::PathBuf> {
    let href = href.split(['#', '?']).next().filter(|h| !h.is_empty())?;
    let href = bibli_platform::places::percent_decode(href);
    let path = if let Some(rest) = href.strip_prefix("file://") {
        // file://host/path is not ours to resolve; file:///path is.
        std::path::PathBuf::from(rest.strip_prefix('/').map(|p| format!("/{p}"))?)
    } else if has_scheme(&href) {
        return None;
    } else if href.starts_with('/') {
        std::path::PathBuf::from(href)
    } else {
        from.parent()?.join(href)
    };
    // Resolve `.` and `..` by text only. Canonicalizing would follow symlinks
    // and show the user a folder whose name their link never mentioned.
    let mut out = std::path::PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    Some(out)
}

fn button(id: &'static str, icon: &'static str, title: &'static str, action: &str) -> Button {
    Button::new(id, icon, title, "ui.inspect").args(serde_json::json!({"action":action}))
}
pub fn toolbar(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let Some(PreviewContent::Inspection(display)) = &shell.preview.content else {
        return div().into_any_element();
    };
    let anchor = crate::elements::popover::PopoverAnchor::default();
    let variants = (0..3)
        .map(|mode| toolbar_row(shell, &display.data, theme, mode, &anchor, cx))
        .collect();
    let mut root = div()
        .relative()
        .flex_none()
        .child(crate::elements::adaptive::AdaptiveRow {
            variants,
            height: 42.,
            chosen: None,
        });
    if let Some(popup) = &shell.preview.menu {
        let mut panel = toolbar::popup_panel(popup, "ui.preview_menu", theme, cx);
        let hex = crate::preview_toolbar::hex_available(shell);
        for (id, icon, title, action) in tools(&display.data) {
            if action == "hex" && !hex {
                continue;
            }
            panel = panel.child(
                button(id, icon, title, action)
                    .label(title)
                    .render(shell, cx)
                    .justify_start(),
            );
        }
        panel = panel.child(
            Button::new(
                "inspect-external",
                "open",
                "Open externally",
                "ui.preview_open_external",
            )
            .label("Open externally")
            .render(shell, cx),
        );
        root = root.child(anchor.popup(panel));
    }
    root.into_any_element()
}
fn tools(data: &Inspection) -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    let mut tools = vec![
        ("inspect-auto", "back", "Default presentation", "auto"),
        ("inspect-hex", "grid", "View as hexadecimal", "hex"),
        ("inspect-search", "search", "Find in preview", "search"),
        ("inspect-copy", "copy", "Copy selection or section", "copy"),
    ];
    if data.source_available {
        tools.push(("inspect-source", "list", "Source / rendered", "source"));
    }
    if data.kind == InspectionKind::Hex {
        tools.push(("inspect-offset", "move", "Jump to byte offset", "offset"));
    }
    if matches!(data.kind, InspectionKind::Archive | InspectionKind::Code) {
        tools.extend([
            (
                "inspect-expand-all",
                "plus",
                "Expand all sections",
                "expand_all",
            ),
            (
                "inspect-collapse-all",
                "minus",
                "Collapse all sections",
                "collapse_all",
            ),
        ]);
    }
    if !data.blocks.is_empty() {
        tools.extend([
            ("inspect-smaller", "minus", "Smaller text", "smaller"),
            ("inspect-larger", "plus", "Larger text", "larger"),
        ]);
    }
    if data.blocks.iter().any(|block| block.heading > 0) || data.kind == InspectionKind::Book {
        tools.push((
            "inspect-outline",
            "contents",
            "Headings and contents",
            "outline",
        ));
    }
    tools
}
fn toolbar_row(
    shell: &Shell,
    data: &Inspection,
    theme: &Theme,
    mode: usize,
    anchor: &crate::elements::popover::PopoverAnchor,
    cx: &mut Context<Shell>,
) -> AnyElement {
    // One id for every variant keeps a button's focus handle across reflows.
    let mut row = toolbar::row(theme).id("inspect-tools");
    let hex = crate::preview_toolbar::hex_available(shell);
    if shell.preview.inspection.request.target.is_some() {
        row = row
            .child(button("inspect-parent", "up", "Container contents", "back").render(shell, cx));
    }
    for (id, icon, title, offset) in [
        ("inspect-prev", "left", "Previous section", data.previous),
        ("inspect-next", "right", "Next section", data.next),
    ] {
        if let Some(offset) = offset {
            row = row.child(
                Button::new(id, icon, title, "ui.inspect")
                    .args(serde_json::json!({"action":"page","offset":offset}))
                    .render(shell, cx),
            );
        }
    }
    for (id, icon, title, action) in tools(data) {
        if (mode == 1 && !matches!(action, "source" | "search" | "copy" | "outline"))
            || (mode == 2 && action != "search")
        {
            continue;
        }
        if action == "hex" && !hex {
            continue;
        }
        row = row.child(
            button(id, icon, title, action)
                .active(action == "source" && shell.preview.inspection.request.source)
                .render(shell, cx),
        );
    }
    if mode == 0 {
        row = row.child(
            div()
                .text_xs()
                .whitespace_nowrap()
                .child(data.title.clone()),
        );
    }
    row.child(div().flex_1().min_w_0())
        .child(
            Button::new(
                "inspect-more",
                "more",
                "More preview controls",
                "ui.preview_menu",
            )
            .args(serde_json::json!({"kind":"inspection"}))
            .active(shell.preview.menu.is_some())
            .anchor_if(anchor, true)
            .render(shell, cx),
        )
        .into_any_element()
}
pub fn content(
    shell: &Shell,
    display: &Display,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let data = display.data.clone();
    let images = display.images.clone();
    let row_icons = display.row_icons.clone();
    let visible = shell.preview.inspection.visible.clone();
    let collapsed = shell.preview.inspection.collapsed.clone();
    let selected = shell.preview.inspection.selected;
    let selection = shell.preview.inspection.selection;
    let font_size = shell.preview.inspection.font_size;
    let mono = shell.mono_font.clone();
    let mut hex_font = gpui::font(mono.clone());
    // A byte viewer must preserve individual ASCII characters, even in coding fonts.
    hex_font.features = gpui::FontFeatures(Arc::new(vec![
        ("calt".into(), 0),
        ("liga".into(), 0),
        ("dlig".into(), 0),
    ]));
    let header_font = hex_font.clone();
    let pane = shell.pane;
    let list_theme = theme.clone();
    let list_style = crate::markdown::MarkdownStyle::new(
        theme,
        font_size,
        display.data.kind == InspectionKind::Markdown,
    );
    let measure = list_style.measure;
    let entity = cx.entity().downgrade();
    let list = gpui::list(shell.preview.inspection.list.clone(), move |index, _, _| {
        let theme = &list_theme;
        if index == 0 {
            if matches!(data.kind, InspectionKind::Archive | InspectionKind::Code)
                || (data.kind == InspectionKind::Hex
                    && data.notice.is_empty()
                    && images[0].is_none())
            {
                return div().into_any_element();
            }
            return div()
                .p_2()
                .text_color(theme.text_muted)
                .child(data.notice.clone())
                .when_some(images[0].as_ref(), |d, image| {
                    d.child(gpui::img(image.clone()).max_w_full())
                })
                .into_any_element();
        }
        let i = index - 1;
        if data.kind == InspectionKind::Code {
            let logical = visible[i];
            return crate::code_preview::CodeRow {
                data: &data,
                line: logical,
                collapsed: &collapsed,
                selected,
                selection,
                font_size,
                mono: &mono,
                theme,
                pane,
                entity: entity.clone(),
            }
            .render();
        }
        if let Some(block) = data.blocks.get(i) {
            return crate::markdown::block(
                block,
                i,
                images[i + 1].as_ref(),
                selection,
                &list_style,
                &mono,
                pane,
                entity.clone(),
            );
        }
        {
            let ri = visible[i - data.blocks.len()];
            let r = &data.rows[ri];
            let weak = entity.clone();
            let target = r.target.clone();
            let branch = r.branch;
            let mut row = if data.kind == InspectionKind::Archive {
                crate::archive_preview::row(
                    r,
                    row_icons.get(ri).and_then(|i| i.as_deref()),
                    collapsed.contains(&ri),
                    selected == Some(data.blocks.len() + ri),
                    theme,
                )
                .id(("inspect-row", ri))
            } else {
                div()
                    .id(("inspect-row", ri))
                    .p_2()
                    .w_full()
                    .flex()
                    .gap_3()
                    .text_sm()
                    .pl(px(8. + r.depth.min(32) as f32 * 14.))
                    .when(data.kind == InspectionKind::Hex, |d| {
                        d.font(hex_font.clone())
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .py(px(1.))
                            .whitespace_nowrap()
                            .when(ri % 2 == 1, |d| d.bg(theme.zebra))
                            .when(selected != Some(data.blocks.len() + ri), |d| {
                                d.hover(|d| d.bg(theme.hover))
                            })
                    })
            };
            if branch && data.kind != InspectionKind::Archive {
                row = row.child(if collapsed.contains(&ri) {
                    "▸"
                } else {
                    "▾"
                });
            }
            for (column, cell) in r.cells.iter().enumerate() {
                if data.kind == InspectionKind::Archive {
                    break;
                }
                row = row.child(
                    div()
                        .w(px(column_width(data.kind, column)))
                        .flex_none()
                        .when(data.kind == InspectionKind::Hex && column < 2, |d| {
                            d.border_r_1().border_color(theme.border)
                        })
                        .child(if data.kind == InspectionKind::Hex {
                            hex_cell(cell, column, theme)
                        } else {
                            cell.chars()
                                .take(512)
                                .collect::<String>()
                                .into_any_element()
                        }),
                );
            }
            let logical_row = data.blocks.len() + ri;
            row = row.cursor_pointer().on_click(move |_, window, cx| {
                let action = if branch {
                    "collapse"
                } else if target.is_some() {
                    "target"
                } else {
                    "select"
                };
                let index = if branch { ri } else { logical_row };
                let _ = weak.update(cx, |s, cx| {
                    s.with_pane(pane, |s| {
                        s.run(
                            "ui.inspect",
                            serde_json::json!({"action":action,"index":index,"target":target}),
                            window,
                            cx,
                        );
                    });
                });
            });
            if selected == Some(data.blocks.len() + ri) {
                row = row.bg(theme.selection_bg);
            }
            row.into_any_element()
        }
    })
    .size_full();
    let mut body = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .key_context("Inspector")
        .when_some(shell.preview.inspection.focus.as_ref(), |d, focus| {
            d.track_focus(focus)
        });
    if shell.preview.inspection.outline {
        let mut outline = div().flex().flex_wrap().gap_1();
        for (i, b) in display
            .data
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.heading > 0)
        {
            outline = outline.child(
                Button::new(
                    format!("inspect-heading-{i}"),
                    "",
                    b.text.clone(),
                    "ui.inspect",
                )
                .label(b.text.clone())
                .args(serde_json::json!({"action":"block","index":i}))
                .render(shell, cx),
            );
        }
        if display.data.kind == InspectionKind::Book {
            for (i, row) in display.data.rows.iter().enumerate() {
                if let Some(target) = &row.target {
                    outline = outline.child(
                        Button::new(
                            format!("inspect-chapter-{i}"),
                            "",
                            row.cells.join(" "),
                            "ui.inspect",
                        )
                        .label(row.cells.join(" "))
                        .args(serde_json::json!({"action":"target","target":target}))
                        .render(shell, cx),
                    );
                }
            }
        }
        body = body.child(
            div()
                .id("inspect-contents")
                .max_h(px(200.))
                .overflow_y_scroll()
                .child(outline),
        );
    }
    if display.data.kind == InspectionKind::Archive {
        body = body.child(crate::archive_preview::header(theme));
    } else if !display.data.columns.is_empty() {
        let mut header = div().p_2().flex().gap_3().text_xs().when(
            display.data.kind == InspectionKind::Hex,
            |d| {
                d.font(header_font)
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .py_1()
                    .text_color(theme.text_muted)
                    .bg(theme.surface)
                    .border_b_1()
                    .border_color(theme.border)
            },
        );
        for (index, column) in display.data.columns.iter().enumerate() {
            header = header.child(
                div()
                    .w(px(column_width(display.data.kind, index)))
                    .flex_none()
                    .child(column.clone()),
            );
        }
        body = body.child(header);
    }
    // A document read at full width is hard to follow: in reading mode the
    // column stops at a comfortable measure and sits in the middle of the
    // pane. This is a maximum, not a measurement: a narrow pane keeps all of
    // its width.
    let mut body = body.child(
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .h_full()
                    .when_some(measure, |d, measure| d.max_w(px(measure)))
                    .child(list),
            )
            .child(crate::elements::scrollbar::vertical(
                "inspect-scrollbar",
                crate::elements::scrollbar::Scrolls::Items(shell.preview.inspection.list.clone()),
            )),
    );
    if display.data.kind == InspectionKind::Archive {
        let total = display
            .data
            .rows
            .iter()
            .filter_map(|r| r.bytes)
            .fold(0u64, u64::saturating_add);
        body = body.child(
            div()
                .flex_none()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.text_muted)
                .border_t_1()
                .border_color(theme.border)
                .child(format!(
                    "{} · {} unpacked",
                    display.data.notice,
                    crate::format::size(total)
                )),
        );
    }
    if matches!(
        display.data.kind,
        InspectionKind::Table | InspectionKind::Hex
    ) {
        let width = (0..display.data.columns.len())
            .map(|i| column_width(display.data.kind, i) + 12.)
            .sum::<f32>()
            + 16.;
        div()
            .id("inspection-horizontal")
            .flex_1()
            .min_h_0()
            .overflow_x_scroll()
            .child(body.w(px(width)).min_w_full().h_full())
            .into_any_element()
    } else {
        body.into_any_element()
    }
}

fn column_width(kind: InspectionKind, index: usize) -> f32 {
    if kind == InspectionKind::Hex {
        [140., 400., 148.].get(index).copied().unwrap_or(140.)
    } else if kind == InspectionKind::Table {
        160.
    } else {
        140.
    }
}

fn hex_cell(text: &str, column: usize, theme: &Theme) -> AnyElement {
    // Offsets and bytes are the data; only what stands for nothing is dimmed:
    // zero bytes, and the dot of an unprintable one.
    let highlights = match column {
        0 => Vec::new(),
        1 => text
            .match_indices("00")
            .map(|(start, value)| {
                (
                    start..start + value.len(),
                    gpui::HighlightStyle {
                        color: Some(theme.text_muted),
                        ..Default::default()
                    },
                )
            })
            .collect(),
        _ => text
            .char_indices()
            .map(|(start, c)| {
                (
                    start..start + c.len_utf8(),
                    gpui::HighlightStyle {
                        color: Some(if c == '.' {
                            theme.text_muted
                        } else {
                            theme.accent
                        }),
                        ..Default::default()
                    },
                )
            })
            .collect(),
    };
    gpui::StyledText::new(text.to_owned())
        .with_highlights(highlights)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::inspection::{Block, Row};

    #[test]
    fn a_documents_links_resolve_next_to_it() {
        let dir = std::path::Path::new("/notes/trip");
        let from = dir.join("Day one.md");
        let at = |href| document_link(&from, href);
        assert_eq!(at("Day two.md"), Some(dir.join("Day two.md")));
        assert_eq!(at("Day%20two.md"), Some(dir.join("Day two.md")));
        assert_eq!(at("photos/"), Some(dir.join("photos")));
        assert_eq!(at("notes.md#later"), Some(dir.join("notes.md")));
        assert_eq!(
            at("file:///etc/hostname"),
            Some(std::path::PathBuf::from("/etc/hostname"))
        );
        assert_eq!(
            at("/srv/shared/plan.md"),
            Some("/srv/shared/plan.md".into())
        );
        assert_eq!(at("../gear/list.md"), Some("/notes/gear/list.md".into()));
        assert_eq!(at("./same.md"), Some(dir.join("same.md")));
        // Other schemes and empty targets are not places on this machine.
        assert_eq!(at("https://example.invalid/x"), None);
        assert_eq!(at("mailto:someone@example.invalid"), None);
        assert_eq!(at("file://host/share/x"), None);
        assert_eq!(at(""), None);
        assert_eq!(at("#section"), None);
    }

    fn archive() -> Display {
        let rows = [
            ("alpha", 0, true),
            ("empty", 1, false),
            ("Zoo's", 0, true),
            ("nested", 1, true),
            ("last.txt", 2, false),
            ("child.bin", 1, false),
            ("a.txt", 0, false),
        ]
        .into_iter()
        .map(|(name, depth, branch)| Row {
            cells: vec![name.into(), String::new(), String::new()],
            depth,
            branch,
            ..Row::default()
        })
        .collect();
        Display::new(
            Inspection {
                kind: InspectionKind::Archive,
                rows,
                ..Inspection::default()
            },
            false,
        )
    }

    fn code(lines: &[&str], folds: Vec<std::ops::Range<usize>>) -> Display {
        Display::new(
            Inspection {
                kind: InspectionKind::Code,
                blocks: lines
                    .iter()
                    .map(|text| Block {
                        text: (*text).into(),
                        code: true,
                        language: Some("json".into()),
                        ..Block::default()
                    })
                    .collect(),
                folds,
                ..Inspection::default()
            },
            true,
        )
    }

    #[test]
    fn archive_rows_collapse_by_subtree_and_expand_all_restores_them() {
        let display = archive();
        let mut state = State::default();
        state.reset(&display.data);
        assert_eq!(state.visible(), [0, 1, 2, 3, 4, 5, 6]);
        state.toggle_collapsed(&display.data, 2);
        assert_eq!(
            state.visible(),
            [0, 1, 2, 6],
            "Zoo's hides its whole subtree"
        );
        state.toggle_collapsed(&display.data, 2);
        state.toggle_collapsed(&display.data, 3);
        assert_eq!(state.visible(), [0, 1, 2, 3, 5, 6]);
        state.collapse_all(&display.data, true);
        assert_eq!(state.collapsed, BTreeSet::from([0, 2, 3]));
        assert_eq!(state.visible(), [0, 2, 6]);
        state.collapse_all(&display.data, false);
        assert_eq!(state.visible(), [0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(state.list.item_count(), 8, "rows plus the notice item");
    }

    #[test]
    fn find_resumes_after_the_selection_and_reveals_folded_lines() {
        let display = code(
            &[
                "{",
                "  \"Needle\": 1,",
                "  \"items\": [",
                "    \"needle\"",
                "  ]",
                "}",
            ],
            vec![0..5, 2..4],
        );
        assert_eq!(display.find("needle", None, true), Some(1));
        assert_eq!(display.find("needle", Some(1), true), Some(3));
        assert_eq!(display.find("needle", Some(3), true), Some(1), "wraps");
        assert_eq!(display.find("needle", Some(3), false), None);
        assert_eq!(display.find("absent", None, true), None);
        assert_eq!(display.find("", None, true), None);
        let mut state = State::default();
        state.reset(&display.data);
        state.collapse_all(&display.data, true);
        assert_eq!(state.visible(), [0, 5]);
        state.reveal(&display.data, 3);
        assert_eq!(state.selected, Some(3));
        assert_eq!(state.visible(), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn rows_are_searchable_after_blocks_and_a_book_selects_its_match_on_arrival() {
        let display = archive();
        assert_eq!(display.find("zoo's", None, true), Some(2));
        assert_eq!(display.find("child", Some(2), false), Some(5));
        let book = Display::new(
            Inspection {
                kind: InspectionKind::Book,
                position: 3,
                blocks: vec![
                    Block {
                        text: "Chapter".into(),
                        heading: 1,
                        ..Block::default()
                    },
                    Block {
                        text: "The Needle sits here.".into(),
                        ..Block::default()
                    },
                ],
                ..Inspection::default()
            },
            false,
        );
        let mut state = State {
            request: InspectRequest {
                query: Some("NEEDLE".into()),
                ..InspectRequest::default()
            },
            ..State::default()
        };
        state.arrived(&book);
        assert_eq!(state.request.offset, 3);
        assert_eq!(state.selected, Some(1));
        state.request.query = None;
        state.arrived(&book);
        assert_eq!(state.selected, None);
    }
}
