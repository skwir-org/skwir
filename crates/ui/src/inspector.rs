//! Shared native viewers for optional structured preview plugins.
use crate::{
    preview::{PreviewContent, PreviewPhase},
    shell::Shell,
    theme::Theme,
    toolbar::Button,
};
use gpui::{
    AnyElement, Context, ListAlignment, ListState, RenderImage, Window, div, prelude::*, px,
};
use skwir_core::inspection::{InspectRequest, Inspection, InspectionKind, Picture};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc, sync::Arc};
pub struct Display {
    pub data: Arc<Inspection>,
    /// The inspection's own picture, then one per block. Shared with every
    /// frame's list closure, never copied per frame.
    pub images: Arc<[Option<Arc<RenderImage>>]>,
    pub(crate) row_icons: Arc<Vec<Option<crate::icons::Pic>>>,
    /// Case-folded search text per block, then per row, prepared on the
    /// worker so Find never lowercases a document on the UI thread.
    folded: Arc<Vec<String>>,
    /// Column widths and kinds of a table, worked out on the worker.
    table: Option<Arc<crate::inspector_rows::Shape>>,
    /// The request this inspection answered, which its pictures for later
    /// are asked with (`inspector_pictures`); `None` for text the panel read
    /// itself.
    pub origin: Option<(
        skwir_core::preview::PreviewRequest,
        skwir_core::inspection::InspectRequest,
    )>,
    /// A map's places, projected here on the worker (`map_preview`).
    pub map: Option<Arc<crate::elements::map::Shapes>>,
    /// Its lines' altitudes over distance, when they have some.
    pub profile: Option<Arc<crate::elements::profile::Profile>>,
    /// The background its code was colored for, when the theme chosen
    /// brings one (`code_preview::Palette`): the code view's, and a code
    /// block's in a document.
    pub code_bg: Option<[u8; 3]>,
    /// The look its code was colored for (`CodeTheme::dark`), whose
    /// tokens it is drawn with whatever skwir's look (`Theme::in_look`).
    pub code_dark: bool,
}
impl Display {
    pub fn new(mut data: Inspection, code: &crate::code_preview::CodeTheme) -> Self {
        // Display conversion runs on a worker. Resolve themed icons for the
        // files a container holds (an archive's members, a message's
        // attachments) here, never by inspecting member paths or doing I/O
        // while drawing rows.
        let row_icons = if kept_rows(data.kind) {
            data.rows
                .iter()
                .map(|row| {
                    if row.branch {
                        crate::icons::folder_now(18)
                    } else {
                        row.cells
                            .first()
                            .and_then(|name| crate::icons::for_name_now(name.as_ref(), 18))
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        // Read only where there is code: a theme of the user's is read
        // from bat's folder the first time.
        let palette = (data.kind == InspectionKind::Code || data.blocks.iter().any(|b| b.code))
            .then(|| crate::code_preview::Palette::of(code));
        if let Some(palette) = palette
            .as_ref()
            .filter(|_| data.kind == InspectionKind::Code)
        {
            crate::code_preview::highlight(&mut data, palette);
        } else {
            // Text too long for the code view is raw text all the same: its
            // addresses are found before it is cut, so none is cut in two.
            if data.kind == InspectionKind::Text {
                for block in data.blocks.iter_mut().filter(|block| !block.code) {
                    skwir_platform::links::link_block(block, true);
                }
            }
            data.blocks = skwir_core::inspection::split_blocks(data.blocks);
        }
        for block in &mut data.blocks {
            let Some(palette) = palette.as_ref() else {
                break;
            };
            if !block.code || data.kind == InspectionKind::Code {
                continue;
            }
            let mut highlighter =
                crate::code_preview::Highlighter::new(block.language.as_deref(), palette);
            block.runs = highlighter
                .text(&block.text)
                .into_iter()
                .map(|run| skwir_core::inspection::Run { code: true, ..run })
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
        // A picture decoded with the page is uploaded here, on the worker; one
        // for later stays in its block, for its size and its key.
        let images = std::iter::once(data.image.take())
            .chain(data.blocks.iter_mut().map(|b| match b.picture.take() {
                Some(Picture::Ready(image)) => Some(image),
                later => {
                    b.picture = later;
                    None
                }
            }))
            .map(|i| i.and_then(|i| crate::thumbs::to_render_image(&i)))
            .collect();
        let folded = data
            .blocks
            .iter()
            .map(|b| b.text.to_lowercase())
            .chain(
                listed(&data)
                    .iter()
                    .map(|r| r.cells.join("\t").to_lowercase()),
            )
            .collect();
        let table = (data.kind == InspectionKind::Table)
            .then(|| Arc::new(crate::inspector_rows::shape(&data.columns, &data.rows)));
        let map = (data.kind == InspectionKind::Map).then(|| {
            Arc::new(
                data.geo
                    .as_deref()
                    .map(crate::elements::map::Shapes::new)
                    .unwrap_or_default(),
            )
        });
        let profile = data
            .geo
            .as_deref()
            .and_then(crate::elements::profile::Profile::of)
            .map(Arc::new);
        Self {
            map,
            profile,
            table,
            data: Arc::new(data),
            images,
            row_icons: Arc::new(row_icons),
            folded: Arc::new(folded),
            origin: None,
            code_bg: palette.and_then(|palette| palette.background),
            code_dark: code.dark,
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

    /// The last block before `before` that contains `query`, wrapping to
    /// the end when `wrap`: Shift+F3, as [`Display::find`] is F3.
    pub fn find_back(&self, query: &str, before: Option<usize>, wrap: bool) -> Option<usize> {
        if query.is_empty() || self.folded.is_empty() {
            return None;
        }
        let len = self.folded.len();
        let end = before.unwrap_or(len).min(len);
        let hit =
            |range: std::ops::Range<usize>| range.rev().find(|&i| self.folded[i].contains(query));
        hit(0..end).or_else(|| wrap.then(|| hit(end..len)).flatten())
    }
}
/// Whether the book's position counts its printed pages: the preference,
/// when the book lists them.
/// An inspection as it is drawn: what the panel shows (`PreviewState::
/// content` and `inspection`), or the view of the container kept above a
/// member of it (`members::Parked`), drawn the same way.
#[derive(Clone, Copy)]
pub struct View<'a> {
    pub display: &'a Display,
    pub state: &'a State,
    /// The container's view kept above a member.
    pub parked: bool,
}

/// The view `parked` names, if there is one.
pub fn view(shell: &Shell, parked: bool) -> Option<View<'_>> {
    if parked {
        let kept = shell.preview.parked()?;
        return Some(View {
            display: &kept.display,
            state: &kept.state,
            parked,
        });
    }
    match &shell.preview.content {
        Some(PreviewContent::Inspection(display)) => Some(View {
            display,
            state: &shell.preview.inspection,
            parked,
        }),
        _ => None,
    }
}

pub(crate) fn counts_printed(shell: &Shell, data: &Inspection) -> bool {
    shell.prefs.printed_pages && !data.printed_pages.is_empty()
}

/// Where the printed pages of the section shown start, (block, page) in
/// reading order: at the block holding the mark its link names, or at the
/// top for a link to the section itself. A mark reading mode did not keep
/// leaves its page out.
fn page_starts(data: &Inspection) -> Vec<(usize, usize)> {
    let mut starts: Vec<(usize, usize)> = data
        .printed_pages
        .iter()
        .enumerate()
        .filter(|(_, page)| page.section == data.position)
        .filter_map(|(index, page)| {
            let block = match page.target.split_once('#') {
                Some((_, mark)) => data
                    .blocks
                    .iter()
                    .position(|block| block.anchors.iter().any(|name| name == mark))?,
                None => 0,
            };
            Some((block, index))
        })
        .collect();
    starts.sort_unstable();
    starts
}

/// The text size reading starts at, and Default size returns to.
pub(crate) const TEXT_SIZE: f32 = 16.;

/// The text size as a share of the default, for the size picker.
pub(crate) fn text_size_label(size: f32) -> String {
    format!("{:.0}%", size / TEXT_SIZE * 100.)
}

/// The rows that are content, drawn after the blocks: a table's, an
/// archive's, a message's attachments. A book's rows are its contents, a way
/// through it that the Contents drawer shows, and none of its text.
pub(crate) fn listed(data: &Inspection) -> &[skwir_core::inspection::Row] {
    if data.kind == InspectionKind::Book {
        &[]
    } else {
        &data.rows
    }
}

/// Whether a container's rows stay beside the member opened from them
/// (`members.rs`): the files it holds, an archive's or a message's.
pub(crate) fn kept_rows(kind: InspectionKind) -> bool {
    matches!(kind, InspectionKind::Archive | InspectionKind::Mail)
}

/// The rows shown, in the order shown: those under a folded row are
/// hidden, and a table is ordered by the column chosen.
pub(crate) fn visible_rows(
    data: &Inspection,
    collapsed: &BTreeSet<usize>,
    sort: Option<(usize, bool)>,
) -> Vec<usize> {
    let mut hidden = None;
    let mut visible: Vec<usize> = listed(data)
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            if hidden.is_some_and(|d| r.depth > d) {
                return None;
            }
            hidden = None;
            if collapsed.contains(&i) {
                hidden = Some(r.depth);
            }
            Some(i)
        })
        .collect();
    if let Some((column, ascending)) = sort.filter(|_| data.kind == InspectionKind::Table) {
        crate::inspector_rows::sort(&data.rows, &mut visible, column, ascending);
    }
    visible
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
    /// The chapter field while it is edited (`preview_bar::number_tool`).
    pub(crate) section_input: Option<crate::preview_bar::Field>,
    /// Where the printed pages of the section shown start: (block, page),
    /// in reading order, from the anchors their marks left (`arrived`).
    page_starts: Vec<(usize, usize)>,
    /// The column a table is ordered by, and whether ascending.
    sort: Option<(usize, bool)>,
    /// Scrolling past the end of a section goes to the next one: the wheel
    /// must already rest on the edge (`at_edge`) and then mean it (`turn`).
    turn: crate::reader_scroll::WheelTurn,
    at_edge: i8,
    /// The section before was asked for by scrolling up: show its end.
    land_at_end: bool,
    /// Pictures the page named for later, decoded as they come into view.
    pub(crate) pictures: crate::inspector_pictures::Pictures,
    /// A note opened under its reference.
    pub(crate) notes: crate::note_popover::Notes,
    /// The links in view labelled (Ctrl+Shift+O): typing a label follows
    /// its link.
    pub(crate) labels: Option<crate::links::Labels>,
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
            font_size: TEXT_SIZE,
            outline: false,
            find: None,
            section_input: None,
            page_starts: Vec::new(),
            sort: None,
            turn: crate::reader_scroll::WheelTurn::default(),
            at_edge: 0,
            land_at_end: false,
            pictures: crate::inspector_pictures::Pictures::default(),
            notes: crate::note_popover::Notes::default(),
            labels: None,
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
    /// The item highlighted: a block, or a row after the blocks.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
    /// Whether the keyboard is in this preview: its content or its find
    /// field, which go when the preview does.
    pub fn holds_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        self.focus
            .as_ref()
            .is_some_and(|focus| focus.contains_focused(window, cx))
            || [&self.find, &self.section_input]
                .into_iter()
                .flatten()
                .any(|field| field.is_focused(window, cx))
            || self.notes.holds_focus(window, cx)
    }
    pub fn reset(&mut self, data: &Inspection) {
        self.labels = None;
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
            let items = data.blocks.len() + listed(data).len();
            self.selected = self.selected.filter(|&i| i < items);
            self.selection = None;
            self.labels = None;
            self.dragging = false;
            self.reindex(data);
            self.go_to(data, top);
        } else {
            self.reset(data);
        }
        self.request.offset = data.position;
        self.section_input = None;
        self.page_starts = page_starts(data);
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
        // Landed: the chapter is its number now, like any other (`offset`,
        // set above), not a place inside a container to go back out of.
        if data.kind == InspectionKind::Book {
            self.request.target = None;
        }
        if data.kind == InspectionKind::Book
            && !refresh
            && let Some(query) = self.request.query.as_deref()
            && let Some(i) = display.find(&query.to_lowercase(), None, false)
        {
            self.reveal(data, i);
        }
    }
    /// The printed page at the top of the view: the last to start at or
    /// before it in this section, else the last of the sections before;
    /// `None` before the first (a book's front matter).
    pub(crate) fn printed_page(&self, data: &Inspection) -> Option<usize> {
        let top = self.list.logical_scroll_top().item_ix.saturating_sub(1);
        self.page_starts
            .iter()
            .take_while(|(block, _)| *block <= top)
            .last()
            .map(|(_, page)| *page)
            .or_else(|| {
                data.printed_pages
                    .iter()
                    .rposition(|page| page.section < data.position)
            })
    }
    /// Show printed page `index`, of this section, at the top of the view.
    /// `false` when it starts in another section.
    fn show_printed(&self, data: &Inspection, index: usize) -> bool {
        let Some(&(block, _)) = self.page_starts.iter().find(|(_, page)| *page == index) else {
            return false;
        };
        if let Some(item) = self.item_of(data, block) {
            self.list.scroll_to(gpui::ListOffset {
                item_ix: item,
                offset_in_item: px(0.),
            });
        }
        true
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
    /// The block list item `item` draws, if it draws one (the first item
    /// is the inspection's picture; rows come after the blocks).
    fn block_of_item(&self, data: &Inspection, item: usize) -> Option<usize> {
        let i = item.checked_sub(1)?;
        if data.kind == InspectionKind::Code {
            self.visible.get(i).copied()
        } else {
            (i < data.blocks.len()).then_some(i)
        }
    }
    /// Label the links of the blocks in view, as last drawn: what the
    /// keyboard can reach of what the pointer can. Whether there are any.
    pub(crate) fn label_links(&mut self, data: &Inspection) -> bool {
        let bottom = self.list.viewport_bounds().bottom();
        let mut found = Vec::new();
        let mut item = self.list.logical_scroll_top().item_ix;
        while let Some(bounds) = self.list.bounds_for_item(item) {
            if bounds.top() >= bottom {
                break;
            }
            if let Some(index) = self.block_of_item(data, item)
                && let Some(block) = data.blocks.get(index)
            {
                found.extend(
                    crate::inspector_text::links_in(block, 0..block.text.len())
                        .into_iter()
                        .map(|link| (index, link)),
                );
            }
            item += 1;
        }
        let labels = crate::links::labels(found.len());
        self.labels = (!found.is_empty()).then(|| crate::links::Labels {
            links: found
                .into_iter()
                .zip(labels)
                .map(|((block, link), label)| crate::links::Labelled {
                    block,
                    at: link.block.start,
                    label,
                    target: link.target,
                    note: link.note,
                })
                .collect(),
            typed: String::new(),
        });
        self.labels.is_some()
    }
    /// Show line `line` (from 1) of the code view, highlighted, as a link
    /// to it asks (`src/main.rs:12`). Only a page that starts the file
    /// numbers its lines from the file's start.
    pub(crate) fn show_line(&mut self, data: &Inspection, line: u32) {
        let index = (line as usize).saturating_sub(1);
        if data.kind == InspectionKind::Code && data.position == 0 && index < data.blocks.len() {
            self.reveal(data, index);
        }
    }
    /// Highlight a row, scrolled into view if it is shown: the member shown
    /// under a container's view is its highlighted row.
    fn highlight(&mut self, data: &Inspection, row: usize) {
        let index = data.blocks.len() + row;
        self.selected = Some(index);
        if let Some(item) = self.item_of(data, index) {
            scroll_into_view(&self.list, item);
        }
    }
    /// The member whose row is highlighted, if one is: what a container's
    /// view previews under it.
    pub(crate) fn highlighted_member(&self, data: &Inspection) -> Option<String> {
        let row = listed(data).get(self.selected?.checked_sub(data.blocks.len())?)?;
        (!row.branch).then(|| row.target.clone()).flatten()
    }
    /// Highlight the row of `target`.
    fn highlight_target(&mut self, data: &Inspection, target: &str) {
        if let Some(row) = listed(data)
            .iter()
            .position(|row| row.target.as_deref() == Some(target))
        {
            self.highlight(data, row);
        }
    }
    /// The arrows over the files of an archive or a message: `step` goes
    /// `delta` rows up or down among those shown, from the highlighted one
    /// (from either end with none); `fold` opens (1) or closes (-1) the
    /// folder highlighted, goes into it once open, and from a closed folder
    /// or a file goes up to the folder holding it. The member reached, if
    /// any, is what to preview: a folder or a file that cannot be opened is
    /// only highlighted.
    pub(crate) fn walk(&mut self, data: &Inspection, action: &str, delta: isize) -> Option<String> {
        let rows = listed(data);
        let current = self
            .selected
            .and_then(|index| index.checked_sub(data.blocks.len()))
            .filter(|&row| row < rows.len());
        let reached = match (action, current) {
            ("step", _) => {
                let at = current.and_then(|row| self.visible.iter().position(|&r| r == row));
                let last = self.visible.len().checked_sub(1)?;
                let to = match at {
                    Some(at) => at.saturating_add_signed(delta).min(last),
                    None if delta > 0 => 0,
                    None => last,
                };
                (Some(to) != at).then(|| self.visible[to])?
            }
            ("fold", Some(row)) => {
                let folder = rows[row].branch;
                let open = folder && !self.collapsed.contains(&row);
                match (delta > 0, folder, open) {
                    // Open a closed folder, close an open one: no row moves.
                    (true, true, false) | (false, true, true) => {
                        self.toggle_collapsed(data, row);
                        return None;
                    }
                    (true, true, true) => {
                        return self.walk(data, "step", 1);
                    }
                    (true, false, _) => return None,
                    // Up to the folder holding it.
                    (false, _, _) => {
                        let depth = rows[row].depth;
                        (0..row).rev().find(|&r| rows[r].depth < depth)?
                    }
                }
            }
            _ => return None,
        };
        self.highlight(data, reached);
        let row = &rows[reached];
        (!row.branch).then(|| row.target.clone()).flatten()
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
    /// Whether the byte view shows the file, which it searches whole:
    /// chosen (View as hex) or the automatic one, which both answer as
    /// bytes (`InspectionKind::Hex`).
    fn hex_file(kind: InspectionKind) -> bool {
        kind == InspectionKind::Hex
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
        self.visible = Arc::new(visible_rows(data, &self.collapsed, self.sort));
        self.list
            .reset_with_uniform_height(data.blocks.len() + self.visible.len() + 1, estimate);
    }
}
/// What changes a container's view alone, so it is done on the view kept
/// above a member in the panel's place (`Shell::with_parked`). Anything
/// else (another presentation, a page) shows the container again first.
const VIEW_ONLY: &[&str] = &[
    "search",
    "find_next",
    "find_previous",
    "find_close",
    "select_all",
    "find",
    "text_down",
    "text_move",
    "text_up",
    "copy",
    "sort",
    "select",
    "collapse",
    "expand_all",
    "collapse_all",
    "reset",
    "text_size",
    "larger",
    "smaller",
    "outline",
    "block",
    "link",
    "links",
    "links_close",
    "label_key",
];

impl Shell {
    /// `ui.inspect`: on the inspection shown, or on the view of the
    /// container kept above the member shown when that has the keyboard
    /// (a press in it gives it the keyboard first).
    pub fn inspect_command(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.container_has_keyboard(window, cx) {
            return self.container_command(args, window, cx);
        }
        // The highlight of an archive's or a message's files is what is
        // previewed: what moved it alone (a find) onto a member opens it.
        // Leaving a member is no such move, though its row is highlighted.
        let action = args["action"].as_str().unwrap_or("auto");
        if !VIEW_ONLY.contains(&action) {
            return self.inspect_here(args, window, cx);
        }
        let before = self.highlighted_member();
        self.inspect_here(args, window, cx);
        if let Some(target) = self.highlighted_member()
            && Some(&target) != before.as_ref()
        {
            let args = serde_json::json!({"action":"target","target":target});
            self.inspect_here(&args, window, cx);
        }
    }

    /// The member highlighted in the files of the archive or the message the
    /// panel shows.
    fn highlighted_member(&self) -> Option<String> {
        match &self.preview.content {
            Some(PreviewContent::Inspection(display)) if kept_rows(display.data.kind) => {
                self.preview.inspection.highlighted_member(&display.data)
            }
            _ => None,
        }
    }

    /// The member highlighted in the container's view kept above a member.
    fn parked_member(&self) -> Option<String> {
        let parked = self.preview.parked()?;
        parked.state.highlighted_member(&parked.display.data)
    }

    /// `ui.inspect` on the container's view kept above a member.
    fn container_command(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = args["action"].as_str().unwrap_or("auto");
        match action {
            "back" => {
                self.close_member(window, cx);
            }
            // Another of its files: previewed in the member's place.
            "target" => self.inspect_here(
                &serde_json::json!({"action":"sibling","target":args["target"]}),
                window,
                cx,
            ),
            "step" | "fold" => {
                let delta = args["delta"].as_i64().unwrap_or(1) as isize;
                let reached = self
                    .with_parked(|this| {
                        let parked = this.preview.content.as_ref();
                        let Some(PreviewContent::Inspection(display)) = parked else {
                            return None;
                        };
                        this.preview.inspection.walk(&display.data, action, delta)
                    })
                    .flatten();
                if let Some(target) = reached {
                    let args = serde_json::json!({"action":"sibling","target":target});
                    self.inspect_here(&args, window, cx);
                }
                cx.notify();
            }
            _ if VIEW_ONLY.contains(&action) => {
                let before = self.parked_member();
                self.with_parked(|this| this.inspect_here(args, window, cx));
                // The highlight leads: a find that moved it onto another
                // member previews that one.
                if let Some(target) = self.parked_member()
                    && Some(&target) != before.as_ref()
                {
                    let args = serde_json::json!({"action":"sibling","target":target});
                    self.inspect_here(&args, window, cx);
                }
            }
            _ => {
                if self.close_member(window, cx) {
                    self.inspect_here(args, window, cx);
                }
            }
        }
    }

    /// `ui.inspect` on the inspection the panel shows.
    fn inspect_here(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = args["action"].as_str().unwrap_or("auto");
        // Out of a member shown under its container's view, or up the
        // trail to one: the container's view takes the panel back as it
        // was left, without reading it again.
        if action == "back" && self.close_member(window, cx) {
            return;
        }
        if action == "ascend"
            && self.ascend_to_container(args["depth"].as_u64().unwrap_or(0) as usize, window, cx)
        {
            return;
        }
        match action {
            "search" => return self.inspect_find_open(window, cx),
            "find_next" | "find_previous" => {
                let Some(find) = &self.preview.inspection.find else {
                    return self.inspect_find_open(window, cx);
                };
                let value = find.text(cx);
                if !value.is_empty() {
                    let back = action == "find_previous";
                    let args = serde_json::json!({"action":"find","value":value,"back":back});
                    self.inspect_here(&args, window, cx);
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
            "section_edit" => return self.inspect_section_edit(window, cx),
            // The arrows over an archive's or a message's files: the member
            // reached is previewed under them.
            "step" | "fold" => {
                let delta = args["delta"].as_i64().unwrap_or(1) as isize;
                let Some(PreviewContent::Inspection(display)) = &self.preview.content else {
                    return;
                };
                if !kept_rows(display.data.kind) {
                    return;
                }
                if let Some(target) = self.preview.inspection.walk(&display.data, action, delta) {
                    let args = serde_json::json!({"action":"target","target":target});
                    self.inspect_here(&args, window, cx);
                }
                return cx.notify();
            }
            "section_cancel" => {
                self.preview.inspection.section_input = None;
                if let Some(focus) = &self.preview.inspection.focus {
                    window.focus(focus);
                }
                return cx.notify();
            }
            "section_submit" => {
                let Some(PreviewContent::Inspection(display)) = &self.preview.content else {
                    return;
                };
                // A printed page is found by its number as printed.
                if counts_printed(self, &display.data) {
                    let pages = &display.data.printed_pages;
                    let typed = self
                        .preview
                        .inspection
                        .section_input
                        .as_ref()
                        .map(|field| field.text(cx).trim().to_lowercase())
                        .unwrap_or_default();
                    let Some(index) = pages
                        .iter()
                        .position(|page| page.label.to_lowercase() == typed)
                    else {
                        let (first, last) = (&pages[0].label, &pages[pages.len() - 1].label);
                        return self.set_status(
                            format!(
                                "Enter a page number printed in the book, from {first} to {last}"
                            ),
                            true,
                            cx,
                        );
                    };
                    let args = serde_json::json!({"action":"section_cancel"});
                    self.inspect_here(&args, window, cx);
                    let args = serde_json::json!({"action":"printed","index":index});
                    return self.run("ui.inspect", args, window, cx);
                }
                let sections = display.data.sections;
                let number = self
                    .preview
                    .inspection
                    .section_input
                    .as_ref()
                    .and_then(|field| field.text(cx).trim().parse::<u64>().ok())
                    .filter(|n| (1..=sections).contains(n));
                let Some(number) = number else {
                    return self.set_status(
                        format!("Enter a chapter number from 1 to {sections}"),
                        true,
                        cx,
                    );
                };
                let args = serde_json::json!({"action":"section_cancel"});
                self.inspect_here(&args, window, cx);
                let args = serde_json::json!({"action":"page","offset":number - 1});
                return self.run("ui.inspect", args, window, cx);
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
                // The links in view labelled, or no longer.
                "links" => {
                    let state = &mut self.preview.inspection;
                    if state.labels.take().is_none() && !state.label_links(&display.data) {
                        self.set_status("No link in view", false, cx);
                    }
                    return cx.notify();
                }
                "links_close" => {
                    self.preview.inspection.labels = None;
                    return cx.notify();
                }
                "label_key" => {
                    let Some(labels) = self.preview.inspection.labels.as_mut() else {
                        return;
                    };
                    let key = args["key"].as_str().unwrap_or("");
                    if key == "backspace" {
                        labels.typed.pop();
                        return cx.notify();
                    }
                    let mut letters = key.chars();
                    let (Some(letter), None) = (letters.next(), letters.next()) else {
                        return;
                    };
                    if !letter.is_ascii_alphabetic() {
                        return;
                    }
                    labels.typed.push(letter.to_ascii_lowercase());
                    let tab = args["shift"].as_bool() == Some(true) || letter.is_ascii_uppercase();
                    let picked = labels
                        .links
                        .iter()
                        .find(|link| link.label == labels.typed)
                        .map(|link| {
                            serde_json::json!({
                                "action": "link",
                                "target": link.target,
                                "note": link.note,
                                "tab": tab,
                                "keys": true,
                            })
                        });
                    let matches = labels
                        .links
                        .iter()
                        .any(|link| link.label.starts_with(labels.typed.as_str()));
                    if picked.is_some() || !matches {
                        self.preview.inspection.labels = None;
                    }
                    cx.notify();
                    match picked {
                        Some(link) => return self.inspect_here(&link, window, cx),
                        None if !matches => {
                            return self.set_status("No link has that label", false, cx);
                        }
                        None => return,
                    }
                }
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
                "select_all" => {
                    if let Some(last) = display.data.blocks.len().checked_sub(1) {
                        self.preview.inspection.selection =
                            Some(crate::inspector_text::Selection {
                                start: (0, 0),
                                end: (last, display.data.blocks[last].text.len()),
                            });
                        cx.notify();
                    }
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
                                    listed(&display.data)
                                        .get(i.saturating_sub(display.data.blocks.len()))
                                        .map(|r| r.cells.join("\t"))
                                })
                        })
                        .unwrap_or_else(|| {
                            let blocks = display.data.blocks.iter().map(|b| b.text.as_str());
                            let rows = listed(&display.data).iter().map(|r| r.cells.join("\t"));
                            blocks
                                .map(str::to_owned)
                                .chain(rows)
                                .collect::<Vec<_>>()
                                .join("\n")
                        });
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
                // A printed page: in this section, brought to the top; in
                // another, that section opened at its mark.
                "printed" => {
                    let index = args["index"].as_u64().unwrap_or(0) as usize;
                    let Some(page) = display.data.printed_pages.get(index) else {
                        return;
                    };
                    if self.preview.inspection.show_printed(&display.data, index) {
                        cx.notify();
                        return;
                    }
                    let args = serde_json::json!({"action":"target","target":page.target});
                    return self.inspect_here(&args, window, cx);
                }
                "reset" | "text_size" => {
                    let size = args["value"].as_f64().map_or(TEXT_SIZE, |v| v as f32);
                    let state = &mut self.preview.inspection;
                    state.font_size = size.clamp(10., 40.);
                    let count = state.list.item_count();
                    state.list.remeasure_items(0..count);
                    // The size picker closes on its choice.
                    if let Some(popup) = self.preview.menu.take_if(|popup| popup.kind == "text") {
                        popup.restore(window);
                    }
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
                // The hex provider searches the whole file itself, forward
                // only; Find sets a font's specimen text. Everything else
                // searches the loaded section after the selected item (before
                // it, `back`). A book falls back to its provider for the
                // following chapters.
                "find"
                    if args["back"].as_bool() == Some(true)
                        && State::hex_file(display.data.kind) =>
                {
                    self.set_status("The hex view searches forward only", false, cx);
                    return;
                }
                "find"
                    if !State::hex_file(display.data.kind)
                        && display.data.kind != InspectionKind::Font =>
                {
                    let query = args["value"].as_str().unwrap_or("").to_lowercase();
                    let book = display.data.kind == InspectionKind::Book;
                    let back = args["back"].as_bool() == Some(true);
                    let state = &mut self.preview.inspection;
                    let found = if back {
                        display.find_back(&query, state.selected, !book)
                    } else {
                        display.find(&query, state.selected, !book)
                    };
                    if let Some(i) = found {
                        state.reveal(&display.data, i);
                        cx.notify();
                        return;
                    }
                    if !book || back {
                        let none = if book {
                            "No earlier match in this chapter"
                        } else {
                            "No match in this preview section"
                        };
                        self.set_status(none, false, cx);
                        cx.notify();
                        return;
                    }
                }
                _ => {}
            }
        }
        if action == "link" {
            // A press that ended a selection is not a click on a link; a
            // label typed is.
            if args["keys"].as_bool() != Some(true)
                && self
                    .preview
                    .inspection
                    .selection
                    .is_some_and(|s| s.start != s.end)
            {
                return;
            }
            // A reference to a note opens the note under it, where the
            // reading goes on; only a link to no note goes there.
            if args["note"].as_bool() == Some(true)
                && let (Some(target), Some(x), Some(y)) = (
                    args["target"].as_str(),
                    args["x"].as_f64(),
                    args["y"].as_f64(),
                )
                && self.open_note(target, gpui::point(px(x as f32), px(y as f32)), window, cx)
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
            // Only a book's links to its own pages mean something inside
            // a container: those fall through to the provider below.
            if kind != Some(InspectionKind::Book) || crate::links::is_address(href) {
                let from = self.preview.path().map(std::path::Path::to_path_buf);
                let tab = args["tab"].as_bool() == Some(true);
                self.follow_document_link(href, from.as_deref(), tab, window, cx);
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
                // An archive's or a message's view stays above the member, as
                // it is, its row highlighted; what has the keyboard keeps it.
                if let (Some(target), Some(kind)) = (self.preview.descend.clone(), kind)
                    && kept_rows(kind)
                    && let Some(PreviewContent::Inspection(display)) = self.preview.content.take()
                {
                    let mut state = std::mem::take(&mut self.preview.inspection);
                    state.highlight_target(&display.data, &target);
                    self.preview.container_asked = Some(crate::members::Parked {
                        display,
                        state,
                        depth: self.preview.nested.len(),
                        focus: cx.focus_handle(),
                    });
                    self.preview
                        .member_focus
                        .get_or_insert_with(|| cx.focus_handle());
                }
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
                self.preview_ascend(depth, window, cx);
                self.preview.queue = queue;
                self.preview.descend = args["target"].as_str().map(str::to_owned);
            }
            // Another file of the container whose view is kept above the
            // member shown: previewed in its place.
            "sibling" => {
                let Some(target) = args["target"].as_str() else {
                    return;
                };
                let Some(mut parked) = self.preview.take_parked() else {
                    return;
                };
                parked.state.highlight_target(&parked.display.data, target);
                self.preview_ascend(parked.depth, window, cx);
                self.preview.container_asked = Some(parked);
                self.preview.descend = Some(target.to_owned());
            }
            // A member that never arrived (too large to extract, refused):
            // the container it was asked from again.
            "back"
                if self.preview.descend.is_some()
                    && self.preview.inspection.request.target.is_none() =>
            {
                let depth = self.preview.nested.len();
                self.preview_ascend(depth, window, cx);
            }
            "back"
                if !self.preview.nested.is_empty()
                    && self.preview.inspection.request.target.is_none() =>
            {
                let depth = self.preview.nested.len() - 1;
                self.preview_ascend(depth, window, cx);
            }
            "ascend" => {
                let depth = args["depth"].as_u64().unwrap_or(0) as usize;
                self.preview_ascend(depth, window, cx);
            }
            _ => {}
        }
        let nested = (action == "target" && container)
            || action == "sibling"
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
        let hex = self.app.previews.bytes_view();
        let state = &mut self.preview.inspection;
        let previous_query = state.request.query.take();
        match action {
            _ if nested => {}
            "hex" => {
                let Some(hex) = hex else {
                    return;
                };
                state.request = InspectRequest {
                    provider: Some(hex.into()),
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
                    Some(kind) if State::hex_file(kind) => {
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
        // A note stays under its reference: the text moving away closes it.
        if self.preview.inspection.notes.open.is_some() {
            self.preview.inspection.notes.close(window);
            cx.notify();
        }
        let Some(PreviewContent::Inspection(display)) = &self.preview.content else {
            return;
        };
        let (previous, next) = (display.data.previous, display.data.next);
        let delta = f32::from(event.delta.pixel_delta(window.line_height()).y);
        if delta == 0. || event.modifiers.secondary() {
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

    /// The chapter field, open on the chapter shown.
    fn inspect_section_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(PreviewContent::Inspection(display)) = &self.preview.content else {
            return;
        };
        if display.data.sections == 0 {
            return;
        }
        let current = if counts_printed(self, &display.data) {
            self.preview
                .inspection
                .printed_page(&display.data)
                .map(|index| display.data.printed_pages[index].label.clone())
                .unwrap_or_default()
        } else {
            (display.data.position + 1).to_string()
        };
        let field = crate::preview_bar::Field::new(
            "Page",
            "ui.inspect section_submit",
            "ui.inspect section_cancel",
            window,
            cx,
        );
        field.input.update(cx, |input, cx| {
            input.set_text(current, cx);
            input.select_all_now(cx);
            input.focus(window);
        });
        self.preview.inspection.section_input = Some(field);
        cx.notify();
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
                "ui.preview_match",
                "ui.preview_find_close",
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

fn button(id: &'static str, icon: &'static str, title: &'static str, action: &str) -> Button {
    Button::new(id, icon, title, "ui.inspect").args(serde_json::json!({"action":action}))
}
pub fn toolbar(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    match view(shell, false) {
        Some(view) => bar(shell, view, theme, cx),
        None => div().into_any_element(),
    }
}

/// The view of the container kept above a member (`members.rs`), whole:
/// its bar, what it says about itself, its rows, its contents drawer. The
/// panel draws its own in `preview::body` and `preview::content`.
pub fn parked(shell: &Shell, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    let Some(view) = view(shell, true) else {
        return div().into_any_element();
    };
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(bar(shell, view, theme, cx))
        .children(notice(view.display).map(|text| crate::preview::notice_strip(text, theme)))
        // What is read is on paper, as the panel's own is (`preview::body`).
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .bg(theme.bg)
                .child(content(shell, view, theme, cx)),
        )
        .children(outline_drawer(shell, view, theme, cx))
        .into_any_element()
}

/// The bar of an inspection, the panel's or the container's kept above a
/// member: the same tools, each acting on its own view (the side that has
/// the keyboard, which a press on a button gives it). The container's has
/// no ×, which is the member's, and its More holds what is about the entry.
fn bar(shell: &Shell, view: View<'_>, theme: &Theme, cx: &mut Context<Shell>) -> AnyElement {
    use crate::preview_bar::{Bar, Item, Slot, label};
    let (display, state, parked) = (view.display, view.state, view.parked);
    let data = &display.data;
    let request = &state.request;
    let mut bar = Bar::new(if parked {
        "container-tools"
    } else {
        "inspect-tools"
    });

    if !parked {
        crate::preview_bar::close_tool(&mut bar, shell, theme);
    }
    // Numbered sections (a book's chapters) are gone through as a
    // document's pages are: previous, the number as a field, next, the
    // buttons there even when there is nowhere to go.
    let numbered = data.sections > 0;
    // Counted by printed pages, the same tools turn and show those.
    let printed = counts_printed(shell, data);
    let page = printed.then(|| state.printed_page(data)).flatten();
    let (previous, next, titles) = if printed {
        let last = data.printed_pages.len() - 1;
        (
            page.and_then(|page| page.checked_sub(1))
                .map(|index| serde_json::json!({"action":"printed","index":index})),
            Some(page.map_or(0, |page| page + 1))
                .filter(|index| *index <= last)
                .map(|index| serde_json::json!({"action":"printed","index":index})),
            ("Previous page · [", "Next page · ]", "Go to page"),
        )
    } else {
        let offset = |offset: Option<u64>| {
            offset.map(|offset| serde_json::json!({"action":"page","offset":offset}))
        };
        (
            offset(data.previous),
            offset(data.next),
            ("Previous section · [", "Next section · ]", "Go to chapter"),
        )
    };
    let step = |id, icon, title, args: Option<serde_json::Value>| {
        let enabled = args.is_some();
        Button::new(id, icon, title, "ui.inspect")
            .args(args.unwrap_or_else(|| serde_json::json!({})))
            .enabled(enabled)
    };
    if numbered || previous.is_some() {
        bar.push(Item::button(
            Slot::Navigate,
            2,
            step("inspect-prev", "left", titles.0, previous),
        ));
    }
    if numbered {
        let (value, total) = if printed {
            (
                page.map_or_else(
                    || "–".to_owned(),
                    |page| data.printed_pages[page].label.clone(),
                ),
                data.printed_pages[data.printed_pages.len() - 1]
                    .label
                    .clone(),
            )
        } else {
            ((data.position + 1).to_string(), data.sections.to_string())
        };
        // A book that lists its printed pages can be counted either way.
        let unit = (!data.printed_pages.is_empty()).then(|| {
            (
                Button::new("inspect-unit", "", "Count by", "ui.preview_menu")
                    .args(serde_json::json!({"kind":"unit"})),
                "unit",
            )
        });
        if unit.is_some() {
            bar.popups
                .push(("unit", crate::preview_bar::unit_popup("Chapters")));
        }
        crate::preview_bar::number_tool(
            &mut bar,
            ("inspect-section", titles.2),
            ("ui.preview_goto", serde_json::json!({})),
            (value, total),
            |shell| shell.preview.inspection.section_input.as_ref(),
            unit,
            theme,
        );
    }
    if numbered || next.is_some() {
        bar.push(Item::button(
            Slot::Navigate,
            2,
            step("inspect-next", "right", titles.1, next),
        ));
    }

    if data.blocks.iter().any(|block| block.heading > 0) || data.kind == InspectionKind::Book {
        bar.push(Item::button(
            Slot::Lead,
            1,
            Button::new(
                "inspect-outline",
                "contents",
                label::CONTENTS,
                "ui.preview_contents",
            )
            .active(state.outline),
        ));
    }

    if data.kind == InspectionKind::Map {
        crate::map_preview::tools(&mut bar, shell);
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
            button("inspect-expand-all", "unfold", "Expand all", "expand_all"),
        ));
        bar.push(Item::button(
            Slot::View,
            0,
            button(
                "inspect-collapse-all",
                "fold",
                "Collapse all",
                "collapse_all",
            ),
        ));
    }
    // Text size, as a document's zoom: smaller, the size as a picker,
    // larger; the same keeps, so both rows give way alike.
    if !data.blocks.is_empty() {
        bar.push(Item::button(
            Slot::View,
            1,
            button("inspect-smaller", "minus", "Smaller text", "smaller"),
        ));
        bar.push(
            Item::button(
                Slot::View,
                2,
                Button::new(
                    "inspect-text-size",
                    "chevron",
                    "Text size",
                    "ui.preview_menu",
                )
                .args(serde_json::json!({"kind":"text"}))
                .label(text_size_label(state.font_size)),
            )
            .opens("text"),
        );
        bar.push(Item::button(
            Slot::View,
            1,
            button("inspect-larger", "plus", "Larger text", "larger"),
        ));
        bar.popups.push((
            "text",
            std::rc::Rc::new(move |shell, theme, cx| text_size_panel(shell, parked, theme, cx)),
        ));
    }
    // What is read in a column may take the whole panel instead: the
    // Reading width preference, from where it shows. It stands where a
    // document's Fit width does, after the size, and goes to More first.
    if !data.blocks.is_empty()
        && matches!(data.kind, InspectionKind::Markdown | InspectionKind::Book)
    {
        bar.push(Item::button(
            Slot::View,
            0,
            Button::new("inspect-full-width", "width", "Full width", "ui.full_width")
                .active(shell.reading_measure.is_none()),
        ));
    }
    // How the file is shown, after its size, as a document's Continuous is.
    // The name of the presentation is the switch to the other one:
    // "Markdown" shows the source, "Source" the document again.
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
    // Numbered sections say where they are in their field.
    if !caption.is_empty() && !numbered {
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

    // A map has no text to find.
    if data.kind != InspectionKind::Map {
        bar.push(
            Item::button(
                Slot::Tools,
                2,
                Button::new("inspect-search", "search", label::FIND, "ui.preview_find")
                    .active(state.finding()),
            )
            .opens("find"),
        );
        bar.popups.push((
            "find",
            std::rc::Rc::new(move |shell, theme, cx| find_panel(shell, parked, theme, cx)),
        ));
    }
    crate::preview_bar::render(
        &bar,
        crate::preview::active_popup(shell, parked),
        shell,
        theme,
        cx,
    )
}
/// The text size picker, as a document's zoom picker is: the default size,
/// then the sizes to choose from, as shares of it.
fn text_size_panel(
    shell: &Shell,
    parked: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let popup = shell.preview.menu.as_ref()?;
    let size = view(shell, parked)?.state.font_size;
    let mut presets = div().flex().flex_wrap().gap_1();
    for (id, percent) in [
        ("text75", 75.),
        ("text100", 100.),
        ("text125", 125.),
        ("text150", 150.),
        ("text200", 200.),
    ] {
        let value = TEXT_SIZE * percent / 100.;
        presets = presets.child(
            Button::new(id, "", "Text size", "ui.inspect")
                .label(text_size_label(value))
                .args(serde_json::json!({"action":"text_size","value":value}))
                .active((size - value).abs() < 0.5)
                .render(shell, cx),
        );
    }
    let default = Button::new("text-default", "revert", "Default size", "ui.inspect")
        .label("Default size")
        .args(serde_json::json!({"action":"reset"}))
        .active((size - TEXT_SIZE).abs() < 0.5)
        .menu()
        .render(shell, cx);
    Some(
        crate::toolbar::popup_panel(popup, "ui.preview_menu", theme, cx)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(default)
                    .child(presets),
            )
            .into_any_element(),
    )
}

/// The find field: the same popup, keys and place as the document reader's.
/// A structured preview searches as it goes, so there is no count: Enter and
/// F3 move to the next match and wrap.
pub fn find_panel(
    shell: &Shell,
    parked: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let find = view(shell, parked)?.state.find.as_ref()?;
    let matches = |id, icon, title, delta: i32| {
        Button::new(id, icon, title, "ui.preview_match").args(serde_json::json!({ "delta": delta }))
    };
    let buttons = vec![
        matches("inspect-previous-match", "left", "Previous match", -1),
        matches("inspect-next-match", "right", "Next match", 1),
        Button::new(
            "inspect-close-find",
            "close",
            "Close find",
            "ui.preview_find_close",
        ),
    ];
    Some(crate::preview_bar::find_popup(
        find,
        crate::keymap::PREVIEW_FIND,
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
    view: View<'_>,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    if !view.state.outline {
        return None;
    }
    let data = &view.display.data;
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
        ("ui.preview_contents", serde_json::json!({})),
        shell,
        theme,
        cx,
    ))
}

pub fn content(
    shell: &Shell,
    view: View<'_>,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let (display, state) = (view.display, view.state);
    if display.data.kind == InspectionKind::Map {
        return crate::map_preview::content(shell, display, theme);
    }
    let data = display.data.clone();
    let shape = display.table.clone();
    let images = display.images.clone();
    // The blocks the rows draw, reported after layout so the pictures they
    // show can be asked for (`inspector_pictures`).
    let loaded = Rc::clone(&state.pictures.loaded);
    let failed = Rc::clone(&state.pictures.failed);
    let drawn = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&drawn);
    // A container's view kept above a member decodes nothing more: what it
    // showed is what it keeps.
    let later = !view.parked
        && data
            .blocks
            .iter()
            .any(|block| matches!(block.picture, Some(Picture::Later { .. })));
    let row_icons = display.row_icons.clone();
    let visible = state.visible.clone();
    let collapsed = state.collapsed.clone();
    let selected = state.selected;
    let selection = state.selection;
    let font_size = state.font_size;
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
    let list_style = crate::markdown::MarkdownStyle::new(
        theme,
        font_size,
        // A book is read like a document: the same measure, spacing and rules.
        matches!(
            display.data.kind,
            InspectionKind::Markdown | InspectionKind::Book
        ),
        shell.reading_measure,
    );
    let note = (!view.parked)
        .then(|| crate::note_popover::popover(shell, &list_style, theme, cx))
        .flatten();
    let mut list_style = list_style;
    list_style.armed_link = shell.armed_link();
    list_style.labels = state.labels.as_ref().map(|labels| Rc::new(labels.shown()));
    // Code in the look its colors are for, which may not be skwir's
    // (`reading.code_appearance`), on the background its theme is for:
    // the code view whole, its gutter too, and a document's code blocks.
    let code_view = display.data.kind == InspectionKind::Code;
    let code_theme = theme.in_look(display.code_dark);
    let other_look = code_theme.is_dark != theme.is_dark;
    let mut code_style = if other_look {
        list_style.drawn_in(&code_theme)
    } else {
        list_style.clone()
    };
    if let Some(background) = display.code_bg {
        code_style.code_bg = crate::code_preview::color(background);
    }
    let code_bg = display
        .code_bg
        .map(crate::code_preview::color)
        .or_else(|| other_look.then_some(code_theme.bg))
        .filter(|_| code_view);
    let code_text = code_theme.text;
    let list_theme = if code_view {
        list_style = code_style;
        code_theme.into_owned()
    } else {
        list_style.code_look = Some(Rc::new(code_style));
        theme.clone()
    };
    let measure = list_style.measure;
    let entity = cx.entity().downgrade();
    let ui_font = shell.font.clone();
    let list = gpui::list(state.list.clone(), move |index, _, _| {
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
            seen.borrow_mut().push(i);
            let picture = match (images[i + 1].as_ref(), &block.picture) {
                (Some(image), _) => Some(crate::markdown::Shown::ready(image)),
                (None, Some(Picture::Later { key, width, height })) => {
                    Some(crate::markdown::Shown {
                        image: loaded.get(key).map(|decoded| &decoded.image),
                        size: (*width, *height),
                        failed: failed.contains(key),
                    })
                }
                _ => None,
            };
            return crate::markdown::block(
                block,
                i,
                picture,
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
                    row_icons.get(ri).and_then(Option::as_ref),
                    collapsed.contains(&ri),
                    selected == Some(data.blocks.len() + ri),
                    ui_font.clone(),
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
                // A cell cut by its column says the rest when pointed at.
                let (cell, clip) = if data.kind == InspectionKind::Hex {
                    (hex_cell(cell, column, theme), None)
                } else {
                    let (text, clip) =
                        crate::elements::clip::text(cell.chars().take(512).collect::<String>());
                    (text.into_any_element(), Some(clip))
                };
                row = row.child(
                    div()
                        .id(("cell", column))
                        .when_some(clip, |d, clip| {
                            d.optional_tooltip(clip.tooltip(None, ui_font.clone()))
                        })
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
                        .child(cell),
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
        // The files of an archive or a message are walked with the arrows.
        .key_context(
            match (kept_rows(display.data.kind), state.labels.is_some()) {
                (true, false) => "Inspector PreviewMembers",
                (true, true) => "Inspector PreviewMembers labelling",
                (false, false) => "Inspector",
                (false, true) => "Inspector labelling",
            },
        )
        // While its links are labelled, what is typed picks one.
        .when(state.labels.is_some(), |d| {
            d.on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                let keystroke = &event.keystroke;
                let key = match keystroke.key.as_str() {
                    "backspace" => "backspace".to_owned(),
                    key => keystroke.key_char.clone().unwrap_or_else(|| key.to_owned()),
                };
                let args = serde_json::json!({
                    "action": "label_key",
                    "key": key,
                    "shift": keystroke.modifiers.shift,
                });
                this.run("ui.inspect", args, window, cx);
                cx.stop_propagation();
            }))
        })
        // After the list has scrolled: what is left of the gesture at an edge
        // turns the section, so a book reads on from chapter to chapter.
        .when(!view.parked, |d| {
            d.on_scroll_wheel(cx.listener(|this, event, window, cx| {
                this.inspect_wheel(event, window, cx);
            }))
        })
        .when_some(state.focus.as_ref(), gpui::InteractiveElement::track_focus);
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
        let sort = state.sort;
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
            let (title, clip) = crate::elements::clip::text(format!("{column}{arrow}"));
            header = header.child(
                div()
                    .id(("inspect-column", index))
                    .when(table, |d| {
                        d.optional_tooltip(clip.tooltip(None, shell.font.clone()))
                    })
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
                    .child(title),
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
            .when_some(code_bg, gpui::Styled::bg)
            .when(code_view, |d| d.text_color(code_text))
            .child(
                div()
                    .w_full()
                    .h_full()
                    .when_some(measure, |d, measure| d.max_w(px(measure)))
                    .child(list),
            )
            // After the list's layout, which drew the rows: what they show
            // is what to decode. Deferred, never done while painting.
            .when(later, |d| {
                let entity = cx.entity().downgrade();
                d.child(
                    gpui::canvas(
                        move |_, window, cx| {
                            let seen = std::mem::take(&mut *drawn.borrow_mut());
                            window.defer(cx, move |window, cx| {
                                if let Some(shell) = entity.upgrade() {
                                    shell.update(cx, |shell, cx| {
                                        shell.inspector_pictures(seen, window, cx);
                                    });
                                }
                            });
                        },
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .size_0(),
                )
            })
            .child(crate::elements::scrollbar::vertical(
                "inspect-scrollbar",
                crate::elements::scrollbar::Scrolls::Items(state.list.clone()),
            )),
    );
    let mut decoded = None;
    if display.data.kind == InspectionKind::Hex
        && let Some(row) = state
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
        // A deferred draw: the note is over the text, wherever it is laid.
        body.children(note).into_any_element()
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
    use skwir_core::inspection::{Block, Row};

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
            &crate::code_preview::CodeTheme::own(false),
        )
    }

    #[test]
    fn printed_pages_start_where_their_marks_are() {
        use skwir_core::inspection::PrintedPage;
        let page = |label: &str, section, target: &str| PrintedPage {
            label: label.into(),
            section,
            target: target.into(),
        };
        let block = |text: &str, anchors: &[&str]| Block {
            text: text.into(),
            anchors: anchors.iter().map(|a| (*a).to_owned()).collect(),
            ..Block::default()
        };
        let data = Inspection {
            kind: InspectionKind::Book,
            position: 1,
            sections: 3,
            blocks: vec![
                block("Chapter", &["chapter"]),
                block("First", &[]),
                block("Second", &["p8"]),
                block("Third", &[]),
            ],
            printed_pages: vec![
                page("6", 0, "one.xhtml#p6"),
                page("7", 1, "two.xhtml"),
                page("8", 1, "two.xhtml#p8"),
                page("9", 1, "two.xhtml#lost"),
                page("10", 2, "three.xhtml#p10"),
            ],
            ..Inspection::default()
        };
        // Page 7 is the chapter's own start; 8 its mark; 9's mark was not
        // kept, and pages of other chapters are not here.
        assert_eq!(page_starts(&data), [(0, 1), (2, 2)]);
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
            &crate::code_preview::CodeTheme::own(true),
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

    /// `docs/` holding `a.txt` and `b.txt`, then `locked.bin` (no target)
    /// and `z.txt`: an archive's files, each member named by its target.
    fn files() -> Inspection {
        let row = |name: &str, depth: usize, branch: bool, target: Option<&str>| Row {
            cells: vec![name.into()],
            depth,
            branch,
            target: target.map(str::to_owned),
            ..Row::default()
        };
        Inspection {
            kind: InspectionKind::Archive,
            rows: vec![
                row("docs", 0, true, None),
                row("a.txt", 1, false, Some("1")),
                row("b.txt", 1, false, Some("2")),
                row("locked.bin", 0, false, None),
                row("z.txt", 0, false, Some("4")),
            ],
            ..Inspection::default()
        }
    }

    #[test]
    fn the_arrows_walk_an_archives_files_and_say_which_member_to_preview() {
        let data = files();
        let mut state = State::default();
        state.reset(&data);
        // From nothing highlighted, Down starts at the top: a folder is
        // highlighted, nothing is previewed.
        assert_eq!(state.walk(&data, "step", 1), None);
        assert_eq!(state.selected(), Some(0));
        assert_eq!(state.walk(&data, "step", 1).as_deref(), Some("1"));
        assert_eq!(state.walk(&data, "step", 1).as_deref(), Some("2"));
        // A file that cannot be opened is highlighted, not previewed.
        assert_eq!(state.walk(&data, "step", 1), None);
        assert_eq!(state.selected(), Some(3));
        assert_eq!(state.walk(&data, "step", 1).as_deref(), Some("4"));
        // Nothing past the end.
        assert_eq!(state.walk(&data, "step", 1), None);
        assert_eq!(state.selected(), Some(4));
        // From nothing highlighted, Up starts at the bottom.
        let mut state = State::default();
        state.reset(&data);
        assert_eq!(state.walk(&data, "step", -1).as_deref(), Some("4"));
    }

    #[test]
    fn left_and_right_fold_a_folder_and_lead_in_and_out_of_it() {
        let data = files();
        let mut state = State::default();
        state.reset(&data);
        state.highlight(&data, 2);
        // Left from a file goes up to its folder.
        assert_eq!(state.walk(&data, "fold", -1), None);
        assert_eq!(state.selected(), Some(0));
        // Left on an open folder closes it; again, there is nowhere up.
        assert_eq!(state.walk(&data, "fold", -1), None);
        assert_eq!(state.visible(), [0, 3, 4]);
        assert_eq!(state.walk(&data, "fold", -1), None);
        assert_eq!(state.selected(), Some(0));
        // Right opens it, and goes into it once open.
        assert_eq!(state.walk(&data, "fold", 1), None);
        assert_eq!(state.visible(), [0, 1, 2, 3, 4]);
        assert_eq!(state.walk(&data, "fold", 1).as_deref(), Some("1"));
        // Right on a file does nothing.
        assert_eq!(state.walk(&data, "fold", 1), None);
        assert_eq!(state.selected(), Some(1));
    }

    #[test]
    fn a_member_opened_is_its_containers_highlighted_row() {
        let data = files();
        let mut state = State::default();
        state.reset(&data);
        state.highlight_target(&data, "2");
        assert_eq!(state.selected(), Some(2));
        // A target no row has leaves the highlight where it was.
        state.highlight_target(&data, "9");
        assert_eq!(state.selected(), Some(2));
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
            &crate::code_preview::CodeTheme::own(false),
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
            &crate::code_preview::CodeTheme::own(false),
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
