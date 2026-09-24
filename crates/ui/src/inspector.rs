//! Shared native viewers for optional structured preview plugins.
use crate::{
    preview::{PreviewContent, PreviewPhase},
    shell::Shell,
    theme::Theme,
    toolbar::Button,
};
use bibli_core::inspection::{InspectRequest, Inspection, InspectionKind};
use gpui::{
    AnyElement, Context, ListAlignment, ListState, RenderImage, Window, div, prelude::*, px,
};
use std::{collections::BTreeSet, sync::Arc};
pub struct Display {
    pub data: Arc<Inspection>,
    /// The inspection's own picture, then one per block. Shared with every
    /// frame's list closure, never copied per frame.
    pub images: Arc<[Option<Arc<RenderImage>>]>,
    row_icons: Arc<Vec<Option<Arc<std::path::Path>>>>,
    /// Case-folded search text per block, then per row, prepared on the
    /// worker so Find never lowercases a document on the UI thread.
    folded: Arc<Vec<String>>,
    /// Column widths and kinds of a table, worked out on the worker.
    table: Option<Arc<crate::inspector_rows::Shape>>,
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
            let mut highlighter =
                crate::code_preview::Highlighter::new(block.language.as_deref(), dark);
            block.runs = highlighter
                .text(&block.text)
                .into_iter()
                .map(|run| bibli_core::inspection::Run { code: true, ..run })
                .collect();
        }
        if data.kind == InspectionKind::Archive {
            // What an archive says about itself goes in the frame's strip,
            // with what its members weigh unpacked.
            let total = data
                .rows
                .iter()
                .filter_map(|r| r.bytes)
                .fold(0u64, u64::saturating_add);
            let unpacked = format!("{} unpacked", crate::format::size(total));
            data.notice = if data.notice.is_empty() {
                unpacked
            } else {
                format!("{} · {unpacked}", data.notice)
            };
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
        let table = (data.kind == InspectionKind::Table)
            .then(|| Arc::new(crate::inspector_rows::shape(&data.columns, &data.rows)));
        Self {
            table,
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
    /// Shared with the frame's list closure: changed only by a click.
    collapsed: Arc<BTreeSet<usize>>,
    selected: Option<usize>,
    selection: Option<crate::inspector_text::Selection>,
    dragging: bool,
    font_size: f32,
    outline: bool,
    /// The find field, open under its button like the document reader's.
    find: Option<crate::preview_bar::Field>,
    /// The column a table is ordered by, and whether ascending.
    sort: Option<(usize, bool)>,
    /// Scrolling past the end of a section goes to the next one: the wheel
    /// must already rest on the edge (`at_edge`) and then mean it (`turn`).
    turn: crate::reader_scroll::WheelTurn,
    at_edge: i8,
    /// The section before was asked for by scrolling up: show its end.
    land_at_end: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            request: InspectRequest::default(),
            focus: None,
            list: ListState::new(0, ListAlignment::Top, px(100.)),
            visible: Arc::new(Vec::new()),
            collapsed: Arc::default(),
            selected: None,
            selection: None,
            dragging: false,
            font_size: 16.,
            outline: false,
            find: None,
            sort: None,
            turn: crate::reader_scroll::WheelTurn::default(),
            at_edge: 0,
            land_at_end: false,
        }
    }
}
/// What the list shows at the top of the view, in the data's terms, so the
/// same place can be found again after the visible rows changed.
#[derive(Clone, Copy)]
struct Place {
    /// A block or a row, as `find` numbers them (rows after the blocks);
    /// `None` for the inspection's picture, the first item.
    index: Option<usize>,
    offset: gpui::Pixels,
}
impl State {
    /// Whether the find field is open.
    pub fn finding(&self) -> bool {
        self.find.is_some()
    }
    /// Whether the keyboard is in this preview: its content or its find
    /// field, which go when the preview does.
    pub fn holds_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        self.focus
            .as_ref()
            .is_some_and(|focus| focus.contains_focused(window, cx))
            || self
                .find
                .as_ref()
                .is_some_and(|find| find.is_focused(window, cx))
    }
    pub fn reset(&mut self, data: &Inspection) {
        self.collapsed = Arc::default();
        self.selected = None;
        self.selection = None;
        self.dragging = false;
        self.outline = false;
        self.reindex(data);
    }
    /// A new inspection replaced the content. Its `position` becomes the
    /// request offset so paging and search continue from what is shown; a
    /// book chapter found by search also reveals its first matching block.
    /// `refresh` says it is the same section read again because the file
    /// changed (a log that grew, a file saved elsewhere): the reader keeps
    /// the folds, the selection and the place they were at.
    pub fn arrived(&mut self, display: &Display, refresh: bool) {
        let data = &display.data;
        if refresh {
            let top = self.place(data);
            let folds: BTreeSet<usize> = if data.kind == InspectionKind::Code {
                data.folds.iter().map(|fold| fold.start).collect()
            } else {
                (0..data.rows.len())
                    .filter(|&i| data.rows[i].branch)
                    .collect()
            };
            self.collapsed = Arc::new(self.collapsed.intersection(&folds).copied().collect());
            let items = data.blocks.len() + data.rows.len();
            self.selected = self.selected.filter(|&i| i < items);
            self.selection = None;
            self.dragging = false;
            self.reindex(data);
            self.go_to(data, top);
        } else {
            self.reset(data);
        }
        self.request.offset = data.position;
        (self.at_edge, self.turn) = (0, crate::reader_scroll::WheelTurn::default());
        if std::mem::take(&mut self.land_at_end) {
            // Scrolled up out of the next section: read on from this one's end.
            self.list.scroll_to(gpui::ListOffset {
                item_ix: self.list.item_count().saturating_sub(1),
                offset_in_item: px(0.),
            });
        }
        // A link into a chapter lands where its fragment points.
        if data.kind == InspectionKind::Book
            && !refresh
            && let Some((_, fragment)) = self
                .request
                .target
                .as_deref()
                .and_then(|target| target.split_once('#'))
            && let Some(item) = data
                .blocks
                .iter()
                .position(|block| block.anchors.iter().any(|name| name == fragment))
                .and_then(|index| self.item_of(data, index))
        {
            scroll_into_view(&self.list, item);
        }
        if data.kind == InspectionKind::Book
            && !refresh
            && let Some(query) = self.request.query.as_deref()
            && let Some(i) = display.find(&query.to_lowercase(), None, false)
        {
            self.reveal(data, i);
        }
    }
    /// The list item that shows block or row `index` (as `find` numbers
    /// them), if it is shown: the code view and a table's rows go through
    /// `visible`, which folds and sorting reorder.
    fn item_of(&self, data: &Inspection, index: usize) -> Option<usize> {
        if data.kind == InspectionKind::Code {
            return self
                .visible
                .iter()
                .position(|&line| line == index)
                .map(|i| i + 1);
        }
        if index < data.blocks.len() {
            return Some(index + 1);
        }
        let row = index - data.blocks.len();
        self.visible
            .iter()
            .position(|&r| r == row)
            .map(|i| data.blocks.len() + i + 1)
    }
    /// What is at the top of the view now.
    fn place(&self, data: &Inspection) -> Place {
        let top = self.list.logical_scroll_top();
        let index = top.item_ix.checked_sub(1).and_then(|i| {
            if data.kind == InspectionKind::Code {
                self.visible.get(i).copied()
            } else if i < data.blocks.len() {
                Some(i)
            } else {
                self.visible
                    .get(i - data.blocks.len())
                    .map(|r| r + data.blocks.len())
            }
        });
        Place {
            index,
            offset: top.offset_in_item,
        }
    }
    /// Scroll back to `place` after the visible rows changed. A line or row
    /// that a fold now hides is stood in for by the nearest one before it,
    /// which is the fold that hides it.
    fn go_to(&self, data: &Inspection, place: Place) {
        let Some(index) = place.index else {
            return self.list.scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: place.offset,
            });
        };
        let (item, exact) = if let Some(item) = self.item_of(data, index) {
            (item, true)
        } else {
            let rows = data.kind != InspectionKind::Code;
            let base = if rows { data.blocks.len() } else { 0 };
            let wanted = index.saturating_sub(base);
            let before = self
                .visible
                .iter()
                .enumerate()
                .filter(|(_, v)| **v <= wanted)
                .max_by_key(|(_, v)| **v)
                .map_or(base, |(i, _)| base + i);
            (before + 1, false)
        };
        self.list.scroll_to(gpui::ListOffset {
            item_ix: item.min(self.list.item_count().saturating_sub(1)),
            offset_in_item: if exact { place.offset } else { px(0.) },
        });
    }
    /// Select and scroll to a block or row, expanding anything that hides it.
    fn reveal(&mut self, data: &Inspection, index: usize) {
        self.selected = Some(index);
        self.collapsed = Arc::default();
        self.reindex(data);
        if let Some(item) = self.item_of(data, index) {
            scroll_into_view(&self.list, item);
        }
    }
    fn toggle_collapsed(&mut self, data: &Inspection, index: usize) {
        let collapsed = Arc::make_mut(&mut self.collapsed);
        if !collapsed.remove(&index) {
            collapsed.insert(index);
        }
        self.relayout(data);
    }
    fn collapse_all(&mut self, data: &Inspection, collapse: bool) {
        self.collapsed = Arc::new(if !collapse {
            BTreeSet::new()
        } else if data.kind == InspectionKind::Code {
            data.folds.iter().map(|fold| fold.start).collect()
        } else {
            data.rows
                .iter()
                .enumerate()
                .filter_map(|(i, r)| r.branch.then_some(i))
                .collect()
        });
        self.relayout(data);
    }
    #[cfg(test)]
    fn visible(&self) -> &[usize] {
        &self.visible
    }
    /// Whether the hex provider owns the shown file, which it searches
    /// whole: explicitly selected, or the automatic byte view.
    fn hex_file(&self, kind: InspectionKind) -> bool {
        self.request.provider.as_deref() == Some("preview-hex") || kind == InspectionKind::Hex
    }
    /// What a row is worth before it has been drawn. A list only measures
    /// what it renders, so without an estimate for the rest the scrollbar
    /// would describe the few blocks on screen instead of the document.
    /// A code line is one line; anything else is a short paragraph. Measured
    /// rows replace the estimate as they are drawn.
    fn row_estimate(&self, data: &Inspection) -> gpui::Pixels {
        px(match data.kind {
            InspectionKind::Code => self.font_size + 4.,
            InspectionKind::Hex | InspectionKind::Archive | InspectionKind::Table => 24.,
            // Prose wraps: call it two and a half lines and the space around
            // them, between a heading and a long paragraph.
            _ => self.font_size * 1.5 * 2.5 + 16.,
        })
    }
    /// The visible rows changed under the reader (a fold, a sort): rebuild
    /// them and stay where the reader was, instead of the top.
    fn relayout(&mut self, data: &Inspection) {
        let place = self.place(data);
        self.reindex(data);
        self.go_to(data, place);
    }
    /// Work out the visible rows and give the list one item per block and
    /// row. The list forgets where it was: callers say where to go.
    fn reindex(&mut self, data: &Inspection) {
        let estimate = self.row_estimate(data);
        if data.kind == InspectionKind::Code {
            self.visible = Arc::new(crate::code_preview::visible(data, &self.collapsed));
            self.list
                .reset_with_uniform_height(self.visible.len() + 1, estimate);
            return;
        }
        let mut hidden = None;
        let mut visible: Vec<usize> = data
            .rows
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
            .collect();
        if let Some((column, ascending)) = self.sort.filter(|_| data.kind == InspectionKind::Table)
        {
            crate::inspector_rows::sort(&data.rows, &mut visible, column, ascending);
        }
        self.visible = Arc::new(visible);
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
        match action {
            "search" => return self.inspect_find_open(window, cx),
            "find_next" => {
                let Some(find) = &self.preview.inspection.find else {
                    return self.inspect_find_open(window, cx);
                };
                let value = find.text(cx);
                if !value.is_empty() {
                    let args = serde_json::json!({"action":"find","value":value});
                    self.inspect_command(&args, window, cx);
                }
                return;
            }
            "find_close" => {
                self.preview.inspection.find = None;
                if let Some(focus) = &self.preview.inspection.focus {
                    window.focus(focus);
                }
                return cx.notify();
            }
            "offset" => {
                let (pane, generation) = (self.pane, self.preview.generation);
                self.open_prompt(
                    crate::prompts::PromptKind::InspectJump { pane, generation },
                    "Jump to byte offset",
                    "",
                    None,
                    window,
                    cx,
                );
                return;
            }
            _ => {}
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
                "sort" => {
                    // First click ascending, second descending, third the file's order.
                    let column = args["column"].as_u64().unwrap_or(0) as usize;
                    let state = &mut self.preview.inspection;
                    state.sort = match state.sort {
                        Some((current, true)) if current == column => Some((column, false)),
                        Some((current, false)) if current == column => None,
                        _ => Some((column, true)),
                    };
                    state.selected = None;
                    let data = display.data.clone();
                    state.relayout(&data);
                    cx.notify();
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
                    let state = &mut self.preview.inspection;
                    state.font_size = (state.font_size + if action == "larger" { 2. } else { -2. })
                        .clamp(10., 40.);
                    // Every row is a different height now: measure them
                    // again where they stand, without leaving the place.
                    let count = state.list.item_count();
                    state.list.remeasure_items(0..count);
                    cx.notify();
                    return;
                }
                "outline" => {
                    self.preview.inspection.outline = !self.preview.inspection.outline;
                    cx.notify();
                    return;
                }
                "block" => {
                    let state = &self.preview.inspection;
                    if let Some(item) = args["index"]
                        .as_u64()
                        .and_then(|i| state.item_of(&display.data, i as usize))
                    {
                        scroll_into_view(&state.list, item);
                    }
                    cx.notify();
                    return;
                }
                // The hex provider searches the whole file itself; Find sets a
                // font's specimen text. Everything else searches the loaded
                // section after the selected item. A book falls back to its
                // provider for the following chapters.
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
                    && let Some(item) = display
                        .data
                        .blocks
                        .iter()
                        .position(|b| b.anchors.iter().any(|name| name == anchor))
                        .and_then(|i| self.preview.inspection.item_of(&display.data, i))
                {
                    scroll_into_view(&self.preview.inspection.list, item);
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
        // A member of a container is previewed as the file it is: the panel
        // goes down into it and the header's trail is the way back up.
        // A table's rows lead somewhere only in a playlist: a CSV has no targets.
        let container = matches!(
            kind,
            Some(InspectionKind::Archive | InspectionKind::Mail | InspectionKind::Table)
        );
        match action {
            "target" if container => {
                if self.preview.nested.len() >= crate::preview::NESTING {
                    self.set_status("Open that file itself to look deeper", false, cx);
                    return;
                }
                self.preview.descend = args["target"].as_str().map(str::to_owned);
                // A playlist's rows, as shown, are what Previous and Next walk.
                if kind == Some(InspectionKind::Table)
                    && let Some(PreviewContent::Inspection(display)) = &self.preview.content
                {
                    let targets: Vec<String> = self
                        .preview
                        .inspection
                        .visible
                        .iter()
                        .filter_map(|row| display.data.rows.get(*row)?.target.clone())
                        .collect();
                    self.preview.queue = targets
                        .iter()
                        .position(|target| Some(target) == self.preview.descend.as_ref())
                        .map(|index| crate::preview::Queue {
                            depth: self.preview.nested.len(),
                            targets,
                            index,
                        });
                }
            }
            // The player stepping through a playlist: back up to it, down
            // into another of its tracks.
            "member" => {
                let depth = args["depth"].as_u64().unwrap_or(0) as usize;
                let queue = self.preview.queue.take();
                self.preview_ascend(depth);
                self.preview.queue = queue;
                self.preview.descend = args["target"].as_str().map(str::to_owned);
            }
            "back"
                if !self.preview.nested.is_empty()
                    && self.preview.inspection.request.target.is_none() =>
            {
                let depth = self.preview.nested.len() - 1;
                self.preview_ascend(depth);
            }
            "ascend" => {
                let depth = args["depth"].as_u64().unwrap_or(0) as usize;
                self.preview_ascend(depth);
            }
            _ => {}
        }
        let nested = (action == "target" && container)
            || action == "ascend"
            || action == "member"
            || (action == "back" && self.preview.inspection.request.target.is_none());
        if nested {
            // Another file is shown, or the container again: how the last
            // one was looked at (a provider, a page, a search) was its own.
            self.preview.inspection.request = InspectRequest::default();
            // A track stepped to by the player says so after this.
            self.preview.autoplay = None;
        }
        let hex = crate::preview_toolbar::hex_available(self);
        let state = &mut self.preview.inspection;
        let previous_query = state.request.query.take();
        match action {
            _ if nested => {}
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
impl Shell {
    /// Scrolling on past the end of a section goes to the next one, and back
    /// past its start to the one before. The wheel must first come to rest on
    /// the edge and then be pushed again: arriving at the end of a chapter
    /// does not leave it. A trackpad has to mean it (`WheelTurn`).
    pub fn inspect_wheel(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(PreviewContent::Inspection(display)) = &self.preview.content else {
            return;
        };
        let (previous, next) = (display.data.previous, display.data.next);
        let delta = f32::from(event.delta.pixel_delta(window.line_height()).y);
        if delta == 0. || event.modifiers.control {
            return;
        }
        let pending = self.preview.phase != PreviewPhase::Idle;
        let state = &mut self.preview.inspection;
        let bottom = state.list.max_offset_for_scrollbar().height;
        let offset = -state.list.scroll_px_offset_for_scrollbar().y;
        let toward: i8 = if delta < 0. { 1 } else { -1 };
        let here = if toward == 1 {
            offset >= bottom - px(0.5)
        } else {
            offset <= px(0.5)
        };
        let target = if toward == 1 { next } else { previous };
        let (at_edge, pushing) = edge_step(state.at_edge, toward, here && target.is_some());
        if at_edge != state.at_edge {
            state.turn = crate::reader_scroll::WheelTurn::default();
        }
        state.at_edge = at_edge;
        if !pushing {
            return;
        }
        let lines = matches!(event.delta, gpui::ScrollDelta::Lines(_));
        if state.turn.turn(delta, lines, pending) != 0
            && let Some(offset) = target
        {
            state.at_edge = 0;
            state.land_at_end = toward == -1;
            self.run(
                "ui.inspect",
                serde_json::json!({"action":"page","offset":offset}),
                window,
                cx,
            );
        }
    }

    fn inspect_find_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.inspection.find.is_none() {
            let hex = matches!(
                &self.preview.content,
                Some(PreviewContent::Inspection(display)) if display.data.kind == InspectionKind::Hex
            );
            let placeholder = if hex {
                "Text, or hex: 00 ff 7a"
            } else {
                "Find…"
            };
            self.preview.inspection.find = Some(crate::preview_bar::Field::new(
                placeholder,
                "ui.inspect find_next",
                "ui.inspect find_close",
                window,
                cx,
            ));
        }
        if let Some(find) = &self.preview.inspection.find {
            find.input.read(cx).focus(window);
        }
        self.preview.menu = None;
        cx.notify();
    }
}

/// One wheel event at a section's edge. `resting` is the edge the wheel had
/// come to rest on (-1 top, 1 bottom, 0 neither), `toward` the way it is going
/// now, `here` whether it is on that edge with a section beyond. Returns the
/// edge it rests on afterwards, and whether this event pushes against it:
/// arriving at the end of a chapter does not leave it, pushing on does.
fn edge_step(resting: i8, toward: i8, here: bool) -> (i8, bool) {
    match (here, resting == toward) {
        (false, _) => (0, false),
        (true, false) => (toward, false),
        (true, true) => (toward, true),
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
    let href = String::from_utf8_lossy(&bibli_platform::uri::percent_decode(href)).into_owned();
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
    use crate::preview_bar::{Bar, Item, Slot, label};
    let Some(PreviewContent::Inspection(display)) = &shell.preview.content else {
        return div().into_any_element();
    };
    let data = &display.data;
    let request = &shell.preview.inspection.request;
    let mut bar = Bar::new("inspect-tools");

    crate::preview_bar::parent_tool(&mut bar, shell);
    for (id, icon, title, offset) in [
        (
            "inspect-prev",
            "left",
            "Previous section · [",
            data.previous,
        ),
        ("inspect-next", "right", "Next section · ]", data.next),
    ] {
        if let Some(offset) = offset {
            bar.push(Item::button(
                Slot::Navigate,
                2,
                Button::new(id, icon, title, "ui.inspect")
                    .args(serde_json::json!({"action":"page","offset":offset})),
            ));
        }
    }

    if data.blocks.iter().any(|block| block.heading > 0) || data.kind == InspectionKind::Book {
        bar.push(Item::button(
            Slot::Lead,
            1,
            button("inspect-outline", "contents", label::CONTENTS, "outline")
                .active(shell.preview.inspection.outline),
        ));
    }

    // How the file is shown. The name of the presentation is the switch to the
    // other one: "Markdown" shows the source, "Source" the document again.
    let (name, rest) = data
        .title
        .split_once(" · ")
        .map_or((data.title.as_str(), ""), |(name, rest)| (name, rest));
    if request.provider.is_some() {
        bar.push(Item::button(
            Slot::View,
            1,
            button("inspect-auto", "back", "Default presentation", "auto"),
        ));
    }
    if data.source_available {
        bar.push(Item::button(
            Slot::View,
            2,
            button(
                "inspect-source",
                "code",
                // One wording for both states: a tooltip outlives the click.
                "Switch between rendered and source",
                "source",
            )
            .label(if request.source { "Source" } else { name }.to_owned())
            .active(request.source),
        ));
    }
    let caption = if data.source_available {
        rest
    } else {
        data.title.as_str()
    };
    if !caption.is_empty() {
        let (caption, muted) = (caption.to_owned(), theme.text_muted);
        bar.push(Item::widget(Slot::View, 0, move |_, _| {
            div()
                .text_xs()
                .whitespace_nowrap()
                .text_color(muted)
                .child(caption.clone())
                .into_any_element()
        }));
    }
    if data.kind == InspectionKind::Hex {
        bar.push(Item::button(
            Slot::View,
            1,
            button("inspect-offset", "move", "Jump to byte offset", "offset"),
        ));
    } else if crate::preview_toolbar::hex_available(shell) {
        // Rare enough to live in More, in every preview that has it.
        bar.push(Item::menu_only(button(
            "inspect-hex",
            "grid",
            label::HEX,
            "hex",
        )));
    }
    // Folding is offered where there is something to fold.
    if data.kind == InspectionKind::Archive || !data.folds.is_empty() {
        bar.push(Item::button(
            Slot::View,
            0,
            button("inspect-expand-all", "plus", "Expand all", "expand_all"),
        ));
        bar.push(Item::button(
            Slot::View,
            0,
            button(
                "inspect-collapse-all",
                "minus",
                "Collapse all",
                "collapse_all",
            ),
        ));
    }
    if !data.blocks.is_empty() {
        bar.push(Item::button(
            Slot::View,
            0,
            button("inspect-smaller", "minus", "Smaller text", "smaller"),
        ));
        bar.push(Item::button(
            Slot::View,
            0,
            button("inspect-larger", "plus", "Larger text", "larger"),
        ));
    }

    bar.push(
        Item::button(
            Slot::Tools,
            2,
            button("inspect-search", "search", label::FIND, "search")
                .active(shell.preview.inspection.finding()),
        )
        .opens("find"),
    );
    bar.popups.push(("find", std::rc::Rc::new(find_panel)));
    crate::preview_bar::render(&bar, crate::preview::active_popup(shell), shell, theme, cx)
}
/// The find field: the same popup, keys and place as the document reader's.
/// A structured preview searches as it goes, so there is no count: Enter and
/// F3 move to the next match and wrap.
pub fn find_panel(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let find = shell.preview.inspection.find.as_ref()?;
    let buttons = vec![
        button("inspect-next-match", "right", "Next match", "find_next"),
        button("inspect-close-find", "close", "Close find", "find_close"),
    ];
    Some(crate::preview_bar::find_popup(
        find,
        "InspectorFind",
        "Enter for the next match".into(),
        buttons,
        shell,
        theme,
        cx,
    ))
}

/// What the frame's notice strip says about this inspection: how it is shown
/// and what was left out.
pub fn notice(display: &Display) -> Option<String> {
    let data = &display.data;
    (!data.notice.is_empty()).then(|| data.notice.clone())
}

/// Headings of what is shown, and a book's chapters, in the shared drawer.
pub fn outline_drawer(
    shell: &Shell,
    display: &Display,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    if !shell.preview.inspection.outline {
        return None;
    }
    let data = &display.data;
    let mut entries: Vec<crate::outline::Entry> = data
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| block.heading > 0)
        .map(|(index, block)| crate::outline::Entry {
            depth: usize::from(block.heading).saturating_sub(1),
            title: block.text.clone().into(),
            command: Some((
                "ui.inspect",
                serde_json::json!({"action":"block","index":index}),
            )),
        })
        .collect();
    if data.kind == InspectionKind::Book {
        entries.extend(data.rows.iter().filter_map(|row| {
            let target = row.target.as_ref()?;
            Some(crate::outline::Entry {
                depth: 0,
                title: row.cells.join(" ").into(),
                command: Some((
                    "ui.inspect",
                    serde_json::json!({"action":"target","target":target}),
                )),
            })
        }));
    }
    Some(crate::outline::drawer(
        entries,
        ("ui.inspect", serde_json::json!({"action":"outline"})),
        shell,
        theme,
        cx,
    ))
}

pub fn content(
    shell: &Shell,
    display: &Display,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let data = display.data.clone();
    let shape = display.table.clone();
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
        // A book is read like a document: the same measure, spacing and rules.
        matches!(
            display.data.kind,
            InspectionKind::Markdown | InspectionKind::Book
        ),
    );
    let measure = list_style.measure;
    let entity = cx.entity().downgrade();
    let list = gpui::list(shell.preview.inspection.list.clone(), move |index, _, _| {
        let theme = &list_theme;
        if index == 0 {
            // The notice is the frame's strip (`notice`); the first item is
            // only the picture an inspection may carry.
            return match images[0].as_ref() {
                Some(image) => div()
                    .p_2()
                    .child(gpui::img(image.clone()).max_w_full())
                    .into_any_element(),
                None => div().into_any_element(),
            };
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
                style: &list_style,
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
            // Where the row is drawn, which a sort makes differ from `ri`.
            let position = i - data.blocks.len();
            let ri = visible[position];
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
                    .when(data.kind == InspectionKind::Table, |d| {
                        // A grid: cells touch, rules between them, numbers
                        // on the right, every other row tinted.
                        d.p_0()
                            .gap_0()
                            .text_size(px(13.))
                            .when(position % 2 == 1, |d| d.bg(theme.zebra))
                            .when(selected != Some(data.blocks.len() + ri), |d| {
                                d.hover(|d| d.bg(theme.hover))
                            })
                    })
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
                let numeric = shape
                    .as_ref()
                    .is_some_and(|shape| shape.numeric.get(column) == Some(&true));
                row = row.child(
                    div()
                        .w(px(column_width(shape.as_deref(), data.kind, column)))
                        .flex_none()
                        .when(data.kind == InspectionKind::Hex && column < 2, |d| {
                            d.border_r_1().border_color(theme.border)
                        })
                        .when(data.kind == InspectionKind::Table, |d| {
                            d.px_2()
                                .py(px(3.))
                                .border_r_1()
                                .border_color(theme.border)
                                .truncate()
                                .when(numeric, gpui::Styled::text_right)
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
        // After the list has scrolled: what is left of the gesture at an edge
        // turns the section, so a book reads on from chapter to chapter.
        .on_scroll_wheel(cx.listener(|this, event, window, cx| {
            this.inspect_wheel(event, window, cx);
        }))
        .when_some(shell.preview.inspection.focus.as_ref(), |d, focus| {
            d.track_focus(focus)
        });
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
        let table = display.data.kind == InspectionKind::Table;
        let sort = shell.preview.inspection.sort;
        if table {
            header = header
                .p_0()
                .gap_0()
                .bg(theme.code_bg)
                .border_b_1()
                .border_color(theme.border);
        }
        for (index, column) in display.data.columns.iter().enumerate() {
            let arrow = match sort {
                Some((sorted, ascending)) if sorted == index => {
                    if ascending {
                        " ▲"
                    } else {
                        " ▼"
                    }
                }
                _ => "",
            };
            header = header.child(
                div()
                    .id(("inspect-column", index))
                    .w(px(column_width(
                        display.table.as_deref(),
                        display.data.kind,
                        index,
                    )))
                    .flex_none()
                    .when(table, |d| {
                        d.px_2()
                            .py(px(4.))
                            .border_r_1()
                            .border_color(theme.border)
                            .truncate()
                            .cursor_pointer()
                            // A header names its column: always on the left,
                            // whatever the cells do. The hover is a tint, not a
                            // text style, which would replace the cell's own.
                            .hover(|d| d.bg(theme.hover))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.run(
                                    "ui.inspect",
                                    serde_json::json!({"action":"sort","column":index}),
                                    window,
                                    cx,
                                );
                            }))
                    })
                    .child(format!("{column}{arrow}")),
            );
        }
        body = body.child(header);
    }
    // A document read at full width is hard to follow: in reading mode the
    // column stops at a comfortable measure and sits in the middle of the
    // pane. This is a maximum, not a measurement: a narrow pane keeps all of
    // its width.
    let body = body.child(
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
    let mut decoded = None;
    if display.data.kind == InspectionKind::Hex
        && let Some(row) = shell
            .preview
            .inspection
            .selected
            .and_then(|selected| selected.checked_sub(display.data.blocks.len()))
            .and_then(|index| display.data.rows.get(index))
    {
        // What the bytes of the selected row mean: the questions a byte view
        // is opened to answer.
        let bytes = crate::inspector_rows::bytes_of(row.cells.get(1).map_or("", String::as_str));
        let mut strip = div()
            .flex_none()
            .px_2()
            .py_1()
            .flex()
            .flex_wrap()
            .gap_x_3()
            .text_xs()
            .bg(theme.surface)
            .border_t_1()
            .border_color(theme.border);
        for (label, value) in crate::inspector_rows::decode(&bytes) {
            strip = strip.child(
                div()
                    .flex()
                    .gap_1()
                    .child(div().text_color(theme.text_muted).child(label))
                    .child(value),
            );
        }
        decoded = Some(strip);
    }
    if matches!(
        display.data.kind,
        InspectionKind::Table | InspectionKind::Hex
    ) {
        let gap = if display.data.kind == InspectionKind::Table {
            0.
        } else {
            12.
        };
        let width = (0..display.data.columns.len())
            .map(|i| column_width(display.table.as_deref(), display.data.kind, i) + gap)
            .sum::<f32>()
            + 16.;
        // The strip is about the panel, not the table: it stays put while
        // the rows scroll sideways.
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("inspection-horizontal")
                    .flex_1()
                    .min_h_0()
                    .overflow_x_scroll()
                    .child(body.w(px(width)).min_w_full().h_full()),
            )
            .children(decoded)
            .into_any_element()
    } else {
        body.into_any_element()
    }
}

fn column_width(
    shape: Option<&crate::inspector_rows::Shape>,
    kind: InspectionKind,
    index: usize,
) -> f32 {
    if kind == InspectionKind::Hex {
        [140., 400., 148.].get(index).copied().unwrap_or(140.)
    } else {
        shape
            .and_then(|shape| shape.widths.get(index).copied())
            .unwrap_or(140.)
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
    #[test]
    fn a_section_turns_only_when_the_wheel_pushes_on_from_its_edge() {
        use super::edge_step;
        // Scrolling down inside the chapter, then arriving at its end: no push.
        assert_eq!(edge_step(0, 1, false), (0, false));
        assert_eq!(edge_step(0, 1, true), (1, false));
        // The next notch pushes against the end.
        assert_eq!(edge_step(1, 1, true), (1, true));
        // Turning back up from the end is ordinary scrolling again.
        assert_eq!(edge_step(1, -1, false), (0, false));
        // Resting on the bottom does not arm the top: a short chapter that is
        // both at once still needs two notches the same way.
        assert_eq!(edge_step(1, -1, true), (-1, false));
        assert_eq!(edge_step(-1, -1, true), (-1, true));
    }

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
        assert_eq!(*state.collapsed, BTreeSet::from([0, 2, 3]));
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
    fn folding_above_the_view_keeps_the_line_at_the_top() {
        let lines: Vec<String> = (0..12).map(|i| format!("line {i}")).collect();
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let display = code(&lines, vec![2..6, 8..11]);
        let mut state = State::default();
        state.reset(&display.data);
        // Line 7 at the top of the view (item 0 is the picture).
        state.list.scroll_to(gpui::ListOffset {
            item_ix: 8,
            offset_in_item: px(3.),
        });
        state.toggle_collapsed(&display.data, 2);
        let top = state.list.logical_scroll_top();
        assert_eq!(state.visible[top.item_ix - 1], 7, "still line 7");
        assert_eq!(top.offset_in_item, px(3.));
        // Folding the line on top away lands on the fold that hides it.
        state.toggle_collapsed(&display.data, 2);
        state.list.scroll_to(gpui::ListOffset {
            item_ix: 10,
            offset_in_item: px(0.),
        });
        state.toggle_collapsed(&display.data, 8);
        let top = state.list.logical_scroll_top();
        assert_eq!(state.visible[top.item_ix - 1], 8);
    }

    #[test]
    fn find_in_a_sorted_table_scrolls_to_where_the_row_is_drawn() {
        let rows = ["pear", "apple", "fig", "banana"]
            .into_iter()
            .map(|name| Row {
                cells: vec![name.into()],
                ..Row::default()
            })
            .collect();
        let display = Display::new(
            Inspection {
                kind: InspectionKind::Table,
                columns: vec!["Name".into()],
                rows,
                ..Inspection::default()
            },
            false,
        );
        let mut state = State {
            sort: Some((0, true)),
            ..State::default()
        };
        state.reset(&display.data);
        assert_eq!(state.visible(), [1, 3, 2, 0]);
        // "pear" is row 0 of the file and drawn last.
        state.reveal(&display.data, 0);
        assert_eq!(state.selected, Some(0));
        assert_eq!(state.list.logical_scroll_top().item_ix, 4);
    }

    #[test]
    fn a_refresh_keeps_the_folds_that_still_exist_and_the_place() {
        let display = code(&["{", "  1", "}", "[", "  2", "]"], vec![0..2, 3..5]);
        let mut state = State::default();
        state.reset(&display.data);
        state.collapse_all(&display.data, true);
        state.list.scroll_to(gpui::ListOffset {
            item_ix: 3,
            offset_in_item: px(0.),
        });
        // The file changed: the second fold is gone.
        let fold = 0..2;
        let changed = code(
            &["{", "  1", "}", "3", "4", "5"],
            std::iter::once(fold).collect(),
        );
        state.arrived(&changed, true);
        assert_eq!(*state.collapsed, BTreeSet::from([0]));
        let top = state.list.logical_scroll_top();
        assert_eq!(state.visible[top.item_ix - 1], 3);
        // Another file starts over.
        state.arrived(&changed, false);
        assert!(state.collapsed.is_empty());
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
        state.arrived(&book, false);
        assert_eq!(state.request.offset, 3);
        assert_eq!(state.selected, Some(1));
        state.request.query = None;
        state.arrived(&book, false);
        assert_eq!(state.selected, None);
    }
}
