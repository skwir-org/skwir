//! Shared native viewers for optional structured preview plugins.
use crate::{
    elements::zoom_wheel::{self, Zoomed},
    preview::{PreviewContent, PreviewPhase},
    shell::Shell,
    theme::Theme,
    toolbar::Button,
};
use gpui::{AnyElement, Context, ListAlignment, ListState, Window, div, prelude::*, px};
use skwir_core::inspection::{InspectRequest, Inspection, InspectionKind, Picture};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc, sync::Arc};
pub struct Display {
    pub data: Arc<Inspection>,
    /// The inspection's own picture, then one per block, with their ink.
    /// Shared with every frame's list closure, never copied per frame.
    pub images: Arc<[Option<crate::backdrop::Picture>]>,
    pub(crate) row_icons: Arc<Vec<Option<crate::icons::Pic>>>,
    /// Case-folded search text per block, then per row, prepared on the
    /// worker so Find never lowercases a document on the UI thread.
    folded: Arc<Vec<String>>,
    /// Column widths and kinds of a table, worked out on the worker.
    table: Option<Arc<crate::inspector_rows::Shape>>,
    /// The request this inspection answered, which its pictures for later
    /// are asked with (`inspector_pictures`); `None` for text the panel read
    /// itself.
    pub origin: Option<
        Box<(
            skwir_core::preview::PreviewRequest,
            skwir_core::inspection::InspectRequest,
        )>,
    >,
    /// A map's places, projected here on the worker (`map_preview`).
    pub map: Option<Arc<crate::elements::map::Shapes>>,
    /// Its lines' altitudes over distance, when they have some.
    pub profile: Option<Arc<crate::elements::profile::Profile>>,
    /// A 3D model's mesh, ready for the GPU and framed, here on the worker
    /// (`model_preview`).
    pub model: Option<Arc<crate::elements::model::Model>>,
    /// The background its code was colored for, when the theme chosen
    /// brings one (`code_preview::Palette`): the code view's, and a code
    /// block's in a document.
    pub code_bg: Option<[u8; 3]>,
    /// The look its code was colored for (`CodeTheme::dark`), whose
    /// tokens it is drawn with whatever skwir's look (`Theme::in_look`).
    pub code_dark: bool,
    /// A text read where it is shown (`Inspection::text`): its first rows,
    /// colored here on the worker, until the view takes them
    /// (`Shell::start_text`).
    pub(crate) text: Option<Box<crate::text_view::Start>>,
    /// The length of the file a byte view shows, read on the worker: how
    /// many rows it has, read where they are shown (`bytes_view`).
    pub(crate) bytes: Option<u64>,
    /// The type its Source is remembered by (`preview::SourceTypes`): the
    /// file's own, as the worker settled it, before a plugin from outside
    /// converted it; `None` for what has no type.
    pub(crate) typed: Option<String>,
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
        // A text read where it is shown colors its rows as it reads them:
        // its first ones here, for the view (`text_view`).
        let text = data
            .text
            .as_ref()
            .filter(|_| data.kind == InspectionKind::Code)
            .and_then(|file| {
                crate::text_view::start(file, crate::code_preview::Palette::of(code))
                    .inspect_err(|error| tracing::debug!(%error, "a text's first rows"))
                    .ok()
                    .map(Box::new)
            });
        if text.is_some() {
            // The view's rows are colored as they are read.
        } else if let Some(palette) = palette
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
        // A picture decoded with the page is uploaded here, its ink read, on
        // the worker; one for later stays in its block, for its size and its
        // key.
        let images = std::iter::once(data.image.take())
            .chain(data.blocks.iter_mut().map(|b| match b.picture.take() {
                Some(Picture::Ready(image)) => Some(image),
                later => {
                    b.picture = later;
                    None
                }
            }))
            .map(|i| i.and_then(|i| crate::backdrop::Picture::of(&i)))
            .collect();
        let fold = skwir_platform::text_rows::fold;
        let folded = data
            .blocks
            .iter()
            .map(|b| fold(&b.text))
            .chain(listed(&data).iter().map(|r| fold(&r.cells.join("\t"))))
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
        // The mesh moves on to the GPU's side whole: its only holder here.
        let model = data.mesh.take().and_then(|mesh| {
            let (positions, indices, colours) = Arc::try_unwrap(mesh)
                .unwrap_or_else(|shared| (*shared).clone())
                .into_parts();
            crate::elements::model::Model::new(positions, indices, colours)
                .ok()
                .map(Arc::new)
        });
        Self {
            map,
            profile,
            model,
            table,
            data: Arc::new(data),
            images,
            row_icons: Arc::new(row_icons),
            folded: Arc::new(folded),
            origin: None,
            code_bg: palette.and_then(|palette| palette.background),
            code_dark: code.dark,
            text,
            bytes: None,
            typed: None,
        }
    }
    /// The matches of `query` (`folded` too) in block or row `i`: where
    /// they are in a block's text; a row's is the row, at its start.
    fn hits(&self, i: usize, query: &str, folded: &str) -> Vec<std::ops::Range<usize>> {
        if !self.folded[i].contains(folded) {
            return Vec::new();
        }
        match self.data.blocks.get(i) {
            Some(block) => skwir_platform::text_rows::matches(&block.text, query),
            None => std::iter::once(0..0).collect(),
        }
    }
    /// The first match of `query` (case ignored, `text_rows::matches`) at
    /// or after `from`: a block or a row (blocks first, then rows offset by
    /// the block count) and a byte in a block's text. Wraps to the
    /// beginning when `wrap`. Its block or row, and where it is there.
    pub fn find(
        &self,
        query: &str,
        from: (usize, usize),
        wrap: bool,
    ) -> Option<(usize, std::ops::Range<usize>)> {
        let (folded, len) = (skwir_platform::text_rows::fold(query), self.folded.len());
        if folded.is_empty() || len == 0 {
            return None;
        }
        let from = (from.0.min(len), from.1);
        let first = |items: std::ops::Range<usize>, later: bool| {
            items.into_iter().find_map(|i| {
                let found = self.hits(i, query, &folded);
                let found = found
                    .into_iter()
                    .find(|r| ((i, r.start) >= from) == later)?;
                Some((i, found))
            })
        };
        first(from.0..len, true).or_else(|| {
            wrap.then(|| first(0..(from.0 + 1).min(len), false))
                .flatten()
        })
    }

    /// The last match before `from`, wrapping to the end when `wrap`:
    /// Shift+F3, as [`Display::find`] is F3.
    pub fn find_back(
        &self,
        query: &str,
        from: (usize, usize),
        wrap: bool,
    ) -> Option<(usize, std::ops::Range<usize>)> {
        let (folded, len) = (skwir_platform::text_rows::fold(query), self.folded.len());
        if folded.is_empty() || len == 0 {
            return None;
        }
        let from = (from.0.min(len), from.1);
        let last = |items: std::ops::Range<usize>, later: bool| {
            items.rev().find_map(|i| {
                let found = self.hits(i, query, &folded);
                let found = found
                    .into_iter()
                    .rev()
                    .find(|r| ((i, r.start) >= from) == later)?;
                Some((i, found))
            })
        };
        last(0..(from.0 + 1).min(len), false)
            .or_else(|| wrap.then(|| last(from.0..len, true)).flatten())
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
/// A row of an inspection as assistive technology hears it: an archive's
/// member an item of its tree, its level and whether a folder is open; any
/// other an option, its place among those shown. Named by its first cell,
/// described by the others, each after its column's name ("Size: 6 B");
/// the row the arrows reached is said as focused while the inspection
/// holds the keyboard.
fn said_row(
    row: gpui::Stateful<gpui::Div>,
    r: &skwir_core::inspection::Row,
    columns: &[String],
    kind: InspectionKind,
    position: usize,
    chosen: bool,
    folded: bool,
    a11y: bool,
) -> gpui::Stateful<gpui::Div> {
    let tree = kind == InspectionKind::Archive;
    row.role(if tree {
        gpui::Role::TreeItem
    } else {
        gpui::Role::ListBoxOption
    })
    .when(a11y, |d| {
        let name = r.cells.first().cloned().unwrap_or_default();
        let rest: Vec<String> = r
            .cells
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, cell)| !cell.is_empty())
            .map(
                |(i, cell)| match columns.get(i).filter(|column| !column.is_empty()) {
                    Some(column) => format!("{column}: {cell}"),
                    None => cell.clone(),
                },
            )
            .collect();
        let d = d
            .aria_label(name)
            .aria_selected(chosen)
            .when(!rest.is_empty(), |d| d.aria_description(rest.join(" · ")));
        if tree {
            d.aria_level(r.depth + 1)
                .when(r.branch, |d| d.aria_expanded(!folded))
        } else {
            d.aria_position_in_set(position)
        }
    })
    .when(chosen, StatefulInteractiveElement::aria_active_descendant)
}

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
    pub(crate) selected: Option<usize>,
    pub(crate) selection: Option<crate::inspector_text::Selection>,
    dragging: bool,
    font_size: f32,
    /// The contents are open: kept from section to section of a file (a
    /// book's chapters), and to the next file while pinned
    /// (`Shell::keep_contents`).
    pub(crate) outline: bool,
    /// The contents' list, following the part shown.
    contents: crate::outline::Follow,
    /// The find field, open under its button like the document reader's.
    find: Option<crate::preview_bar::Field>,
    /// What the find looked for last and the match it is on (a block or
    /// a row, a text's row, and where it is there), marked while the field
    /// is open.
    pub(crate) sought: Option<crate::inspector_text::Sought>,
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
    /// A text read where it is shown (ADR-051), drawn instead of the list.
    pub(crate) text: Option<crate::text_view::TextView>,
    /// A file's bytes read where they are shown, drawn instead of the list.
    pub(crate) bytes: Option<crate::bytes_view::BytesView>,
    /// A table or bytes wider than the panel, scrolled sideways.
    sideways: gpui::ScrollHandle,
}
impl State {
    /// Whether what is shown is wider than the panel: lines that do not
    /// wrap, a table or bytes. The arrows then move it sideways rather than
    /// step to the section before or after (`Shell::preview_scroll`).
    pub(crate) fn wide(&self) -> bool {
        match &self.text {
            Some(view) => view.scroll.borrow().horizontal().1 > 0.,
            None => self.sideways.max_offset().x > px(0.),
        }
    }
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
            contents: crate::outline::Follow::default(),
            find: None,
            sought: None,
            section_input: None,
            page_starts: Vec::new(),
            sort: None,
            turn: crate::reader_scroll::WheelTurn::default(),
            at_edge: 0,
            land_at_end: false,
            pictures: crate::inspector_pictures::Pictures::default(),
            notes: crate::note_popover::Notes::default(),
            labels: None,
            text: None,
            bytes: None,
            sideways: gpui::ScrollHandle::new(),
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
    /// Where a find for `query` starts, forward or `back`, and what it
    /// looks for from now (`sought`): from the match it is on while that is
    /// the item selected (past it, unless the query is new; before it,
    /// `back`), else after the item selected or before it, else from
    /// `top` (an item and a byte in it). A new query is marked at once;
    /// the match it is on goes with the old one.
    pub(crate) fn find_from(
        &mut self,
        query: &str,
        back: bool,
        top: (usize, usize),
    ) -> (usize, usize) {
        let fold = skwir_platform::text_rows::fold;
        let on = self.sought.as_ref().and_then(|sought| {
            let (item, found) = sought.current.clone()?;
            let same = fold(&sought.query) == fold(query);
            (Some(item) == self.selected).then_some((item, found, same))
        });
        let from = match (&on, self.selected, back) {
            (Some((item, found, same)), _, false) => (*item, found.start + usize::from(*same)),
            (Some((item, found, _)), _, true) => (*item, found.start),
            (None, Some(item), false) => (item + 1, 0),
            (None, Some(item), true) => (item, 0),
            (None, None, _) => top,
        };
        let current = on
            .filter(|(_, _, same)| *same)
            .map(|(item, found, _)| (item, found));
        self.sought = Some(crate::inspector_text::Sought {
            query: query.to_owned(),
            current,
            seen: None,
        });
        from
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

    /// Every row is a different height now (the text size or a font
    /// changed): measure them again where they stand, without leaving the
    /// place.
    pub fn remeasure(&self) {
        self.list.remeasure_items(0..self.list.item_count());
    }

    /// Another section of the file came (a book's next chapter): what was
    /// of the last goes, the contents stay open as they were.
    pub fn reset(&mut self, data: &Inspection) {
        self.labels = None;
        self.collapsed = Arc::default();
        self.selected = None;
        self.selection = None;
        self.dragging = false;
        self.sideways.set_offset(gpui::point(px(0.), px(0.)));
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
        // The match the find was on was in what went; what it looks for
        // is marked in what came.
        if let Some(sought) = self.sought.as_mut() {
            sought.current = None;
        }
        self.request.offset = data.position;
        // Whether it is the source is the worker's to say: its type's
        // choice (`preview::SourceTypes`), where the file has one.
        if let Some(origin) = &display.origin {
            self.request.source = origin.1.source;
        }
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
            && let Some((i, found)) = display.find(query, (0, 0), false)
        {
            self.reveal(data, i);
            if let Some(sought) = self.sought.as_mut() {
                sought.current = Some((i, found));
            }
        }
    }
    /// The printed page at the top of the view: the last to start at or
    /// before it in this section, else the last of the sections before;
    /// `None` before the first (a book's front matter).
    /// The block at the top of the view, read from the list's scroll: the
    /// first while the inspection's picture, the list's first item, is.
    fn top_block(&self, data: &Inspection) -> usize {
        let top = self.list.logical_scroll_top().item_ix.saturating_sub(1);
        top.min(data.blocks.len().saturating_sub(1))
    }
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
        let mut found = Vec::new();
        let links = |index: usize, block: &skwir_core::inspection::Block| {
            crate::inspector_text::links_in(block, 0..block.text.len())
                .into_iter()
                .map(move |link| (index, link))
        };
        if let Some(text) = &self.text {
            // The rows wholly on screen.
            let shown = text.scroll.borrow().shown();
            for row in shown {
                if let Some(block) = text.block(row as usize) {
                    found.extend(links(row as usize, block));
                }
            }
        }
        let bottom = self.list.viewport_bounds().bottom();
        let mut item = self.list.logical_scroll_top().item_ix;
        while let Some(bounds) = self
            .list
            .bounds_for_item(item)
            .filter(|_| self.text.is_none())
        {
            if bounds.top() >= bottom {
                break;
            }
            if let Some(index) = self.block_of_item(data, item)
                && let Some(block) = data.blocks.get(index)
            {
                found.extend(links(index, block));
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
    /// to it asks (`src/main.rs:12`): in a text read where it is shown, once
    /// it is counted (`text_view`).
    /// Scroll to the block holding the mark `name` (`Block::anchors`: an
    /// `id`, a heading's): whether the text holds it.
    pub(crate) fn show_anchor(&mut self, data: &Inspection, name: &str) -> bool {
        let Some(item) = data
            .blocks
            .iter()
            .position(|block| block.anchors.iter().any(|anchor| anchor == name))
            .and_then(|index| self.item_of(data, index))
        else {
            return false;
        };
        scroll_into_view(&self.list, item);
        true
    }
    pub(crate) fn show_line(&mut self, data: &Inspection, line: u32) {
        if let Some(text) = self.text.as_mut() {
            return text.show_line(u64::from(line));
        }
        let index = (line as usize).saturating_sub(1);
        if data.kind == InspectionKind::Code && index < data.blocks.len() {
            self.reveal(data, index);
        }
    }
    /// Block `index`: a row of a text read where it is shown, once read.
    pub(crate) fn block<'a>(
        &'a self,
        data: &'a Inspection,
        index: usize,
    ) -> Option<&'a skwir_core::inspection::Block> {
        match &self.text {
            Some(text) => text.block(index),
            None => data.blocks.get(index),
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
    "scroll",
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

    /// Ctrl and the wheel over an inspection's text: Larger or Smaller text
    /// on the view under the pointer, the panel's or the container's kept
    /// above a member (`parked`).
    pub(crate) fn inspect_text_step(
        &mut self,
        larger: bool,
        parked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let args = serde_json::json!({"action": if larger { "larger" } else { "smaller" }});
        if parked {
            self.with_parked(|this| this.inspect_here(&args, window, cx));
        } else {
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
    pub(crate) fn inspect_here(
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
                self.preview.inspection.sought = None;
                if let Some(focus) = &self.preview.inspection.focus {
                    window.focus(focus, cx);
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
                    window.focus(focus, cx);
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
        // A text or bytes read where they are shown find, copy and scroll
        // the file.
        if matches!(action, "find" | "copy" | "select_all" | "scroll")
            && self.text_action(action, args, cx)
        {
            return;
        }
        if matches!(action, "find" | "copy" | "jump") && self.bytes_action(action, args, cx) {
            return;
        }
        if action == "scroll" {
            return self.inspect_scroll(args, window, cx);
        }
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
                    if let Some(block) = self.preview.inspection.block(&display.data, index) {
                        let mut offset =
                            (args["offset"].as_u64().unwrap_or(0) as usize).min(block.text.len());
                        while !block.text.is_char_boundary(offset) {
                            offset -= 1;
                        }
                        let state = &mut self.preview.inspection;
                        state.selected = Some(index);
                        if action == "text_down" {
                            if let Some(focus) = &state.focus {
                                window.focus(focus, cx);
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
                        window.focus(focus, cx);
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
                    state.remeasure();
                    // The size picker closes on its choice.
                    if let Some(popup) = self.preview.menu.take_if(|popup| popup.kind == "text") {
                        popup.restore(window, cx);
                    }
                    cx.notify();
                    return;
                }
                "larger" | "smaller" => {
                    let state = &mut self.preview.inspection;
                    state.font_size = (state.font_size + if action == "larger" { 2. } else { -2. })
                        .clamp(10., 40.);
                    state.remeasure();
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
                    let query = args["value"].as_str().unwrap_or("").to_owned();
                    let book = display.data.kind == InspectionKind::Book;
                    let back = args["back"].as_bool() == Some(true);
                    let state = &mut self.preview.inspection;
                    let top = if back { (usize::MAX, 0) } else { (0, 0) };
                    let from = state.find_from(&query, back, top);
                    let found = if back {
                        display.find_back(&query, from, !book)
                    } else {
                        display.find(&query, from, !book)
                    };
                    if let Some((i, found)) = found {
                        state.reveal(&display.data, i);
                        if let Some(sought) = state.sought.as_mut() {
                            sought.current = Some((i, found));
                        }
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
            if let (Some(reference), Some(target), Some(x), Some(y)) = (
                crate::note_popover::Reference::of(args),
                args["target"].as_str(),
                args["x"].as_f64(),
                args["y"].as_f64(),
            ) && self.open_note(
                target,
                reference,
                gpui::point(px(x as f32), px(y as f32)),
                window,
                cx,
            ) {
                return;
            }
            if let Some(anchor) = args["target"].as_str().and_then(|s| s.strip_prefix('#')) {
                let anchor = skwir_platform::uri::percent_decode(anchor);
                let anchor = String::from_utf8_lossy(&anchor).into_owned();
                self.show_mark(crate::links::Mark::Anchor(anchor), cx);
                return;
            }
            let href = args["target"].as_str().unwrap_or("");
            // Only a book's links to its own pages mean something inside
            // a container: those fall through to the provider below.
            if kind != Some(InspectionKind::Book) || crate::links::is_address(href) {
                let tab = args["tab"].as_bool() == Some(true);
                self.follow_document_link(href, tab, window, cx);
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
        // Whether Source changed for the file's type (`SourceTypes`).
        let mut remembered = false;
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
                // And so for every file of its type from now on: an
                // inspection's, or a picture's written as text.
                let typed = match &self.preview.content {
                    Some(PreviewContent::Inspection(display)) => display.typed.as_ref(),
                    Some(PreviewContent::Image { .. }) => self.preview.picture_source.as_ref(),
                    _ => None,
                };
                if let Some(typed) = typed {
                    self.preview.source_types.set(typed, state.request.source);
                    remembered = true;
                }
            }
            "turn" => {
                // The other way from what is shown, and so for every file of
                // its type from now on (`TurnedTypes`).
                let (turned, typed) = match &self.preview.content {
                    Some(PreviewContent::Inspection(display)) => (
                        display.data.kind != InspectionKind::Model,
                        display.typed.as_ref(),
                    ),
                    _ => return,
                };
                state.request.turned = Some(turned);
                // Asked for this file: past the size a model is turned at
                // unasked, which is never remembered.
                state.request.whole = turned;
                if let Some(typed) = typed {
                    self.preview.turned_types.set(typed, turned);
                    remembered = true;
                }
            }
            // All of this file, past the bounds that keep a preview quick:
            // for this file only, never remembered.
            "whole" => state.request.whole = true,
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
        if remembered {
            self.remember_session();
        }
        self.cancel_pending_preview();
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
            self.preview.inspection.notes.close(window, cx);
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
        let bottom = state.list.max_offset_for_scrollbar().y;
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

    /// `ui.inspect`'s `scroll` on what is not a text (`text_view::scroll`
    /// has that): `to` a step up or down (a row of bytes), a screen less a
    /// step, either end, or a step sideways where a table or bytes are
    /// wider than the panel.
    fn inspect_scroll(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.preview.inspection;
        // A note stays under its reference: the text moving away closes it.
        if state.notes.open.is_some() {
            state.notes.close(window, cx);
        }
        let to = args["to"].as_str().unwrap_or("");
        match (to, state.bytes.as_ref()) {
            ("left", _) => {
                crate::preview::arrow(&state.sideways, -1, 0);
            }
            ("right", _) => {
                crate::preview::arrow(&state.sideways, 1, 0);
            }
            (_, Some(view)) => crate::text_view::scroll(&view.scroll, args),
            (_, None) => {
                let list = &state.list;
                let view = f32::from(list.viewport_bounds().size.height);
                let step = crate::preview::arrow_step(view);
                let screen = (view - step).max(step);
                let by = match to {
                    "up" => -step,
                    "down" => step,
                    "page_up" => -screen,
                    "page_down" => screen,
                    "start" | "end" => {
                        // Past the last item, the layout settles on the end.
                        let item_ix = if to == "end" { list.item_count() } else { 0 };
                        list.scroll_to(gpui::ListOffset {
                            item_ix,
                            offset_in_item: px(0.),
                        });
                        return cx.notify();
                    }
                    _ => return,
                };
                list.scroll_by(px(by));
            }
        }
        cx.notify();
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
            input.focus(window, cx);
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
            find.input.update(cx, |i, cx| i.focus(window, cx));
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
        .child(crate::outline::around(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .bg(theme.bg)
                .child(content(shell, view, theme, cx))
                .into_any_element(),
            outline_drawer(shell, view, theme, cx),
            shell,
            theme,
        ))
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
    } else if state.bytes.is_some() {
        // Bytes shown whole have no page to turn.
        (None, None, ("", "", ""))
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
    // A picture shown as text (its source, its bytes) is still one of the
    // pane's pictures: the whole screen's walk and slideshow go on.
    if shell.walks_pictures() {
        crate::preview_toolbar::picture_tools(&mut bar, shell);
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
    if data.kind == InspectionKind::Model {
        crate::model_preview::tools(&mut bar, shell);
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
    // How the file is shown, after its size, as a document's Continuous is:
    // Source, lit while the file's own text is shown, never named after
    // the state it is in. What is shown says itself beside it, rendered or
    // not (a format's name, a page's lines).
    if request.provider.is_some() {
        bar.push(Item::button(
            Slot::View,
            1,
            button("inspect-auto", "back", "Default presentation", "auto"),
        ));
    }
    // A model turned in 3D, or shown by its picture: one button, lit while
    // it turns.
    if data.mesh_available && shell.draws_meshes {
        bar.push(Item::button(
            Slot::View,
            2,
            button(
                "inspect-turn",
                "cube",
                "The model turned in 3D, or its picture",
                "turn",
            )
            .label(label::TURN)
            .active(data.kind == InspectionKind::Model),
        ));
    }
    // What was cut short to stay quick: all of it, asked.
    if data.whole_available {
        crate::preview_bar::whole_tool(&mut bar);
    }
    if data.source_available {
        bar.push(Item::button(
            Slot::View,
            2,
            button(
                "inspect-source",
                "code",
                // One wording for both states: a tooltip outlives the click.
                "The file's own text, as written",
                "source",
            )
            .label(label::SOURCE)
            .active(request.source),
        ));
    }
    // A text read where it is shown says its lines as they are counted.
    let counted = state.text.as_ref().map(|text| {
        let (lines, done) = text.lines();
        let lines = crate::format::count(usize::try_from(lines).unwrap_or(usize::MAX), "line");
        if done { lines } else { format!("{lines}…") }
    });
    let caption = counted.as_deref().unwrap_or(data.title.as_str());
    if state.text.is_some() {
        bar.push(Item::menu_only(
            Button::new("inspect-wrap", "width", "Wrap lines", "ui.wrap_lines")
                .label("Wrap lines")
                .active(shell.prefs.wrap_lines),
        ));
    }
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

    // A map and a model have no text to find.
    if !matches!(data.kind, InspectionKind::Map | InspectionKind::Model) {
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
    // The member under a container's view goes to the whole screen, not
    // the view above it.
    if !parked {
        crate::preview_bar::fullscreen_tool(&mut bar, shell, theme);
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

/// Whether what is shown has contents: headings, or a book's chapters.
fn has_contents(data: &Inspection) -> bool {
    data.blocks.iter().any(|block| block.heading > 0) || data.kind == InspectionKind::Book
}

/// Headings of what is shown, and a book's chapters, in the shared
/// contents, while they are open and there are some.
pub fn outline_drawer(
    shell: &Shell,
    view: View<'_>,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let data = &view.display.data;
    if !view.state.outline || !has_contents(data) {
        return None;
    }
    let headings: Vec<usize> = data
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| block.heading > 0)
        .map(|(index, _)| index)
        .collect();
    let mut entries: Vec<crate::outline::Entry> = headings
        .iter()
        .map(|&index| crate::outline::Entry {
            depth: usize::from(data.blocks[index].heading).saturating_sub(1),
            title: data.blocks[index].text.clone().into(),
            command: Some((
                "ui.inspect",
                serde_json::json!({"action":"block","index":index}),
            )),
        })
        .collect();
    let chapters: Vec<&skwir_core::inspection::Row> = if data.kind == InspectionKind::Book {
        data.rows
            .iter()
            .filter(|row| row.target.is_some())
            .collect()
    } else {
        Vec::new()
    };
    entries.extend(chapters.iter().map(|row| crate::outline::Entry {
        depth: row.depth,
        title: row.cells.join(" ").into(),
        command: Some((
            "ui.inspect",
            serde_json::json!({"action":"target","target":row.target}),
        )),
    }));
    let top = view.state.top_block(data);
    let current = current_chapter(data, &chapters, top)
        .map(|chapter| headings.len() + chapter)
        .or_else(|| current_heading(&headings, top));
    Some(crate::outline::contents(
        entries,
        current,
        &view.state.contents,
        ("ui.preview_contents", serde_json::json!({})),
        shell,
        theme,
        cx,
    ))
}

/// The heading of the part shown: the last at or above `top`, the block at
/// the top of the view.
fn current_heading(headings: &[usize], top: usize) -> Option<usize> {
    headings.iter().rposition(|&block| block <= top)
}

/// A book's chapter shown, among its contents' `chapters`: of those that
/// open the section shown, the last starting at or above `top` (where its
/// fragment points, else the section's start; one whose fragment the
/// section does not hold is not placed), else its first; for a section the
/// contents do not name (a cover, a chapter split in files), the last of
/// those before it.
fn current_chapter(
    data: &Inspection,
    chapters: &[&skwir_core::inspection::Row],
    top: usize,
) -> Option<usize> {
    let shown = data.position;
    let here: Vec<usize> = (0..chapters.len())
        .filter(|&i| chapters[i].section == Some(shown))
        .collect();
    if let Some(&first) = here.first() {
        let starts = |i: usize| match chapters[i]
            .target
            .as_deref()
            .and_then(|target| target.split_once('#'))
        {
            None => Some(0),
            Some((_, fragment)) => data
                .blocks
                .iter()
                .position(|block| block.anchors.iter().any(|name| name == fragment)),
        };
        return Some(
            here.iter()
                .copied()
                .rfind(|&i| starts(i).is_some_and(|block| block <= top))
                .unwrap_or(first),
        );
    }
    let before = chapters
        .iter()
        .filter_map(|row| row.section.filter(|&section| section < shown))
        .max()?;
    chapters.iter().rposition(|row| row.section == Some(before))
}

/// Whether `kind` is read, its running text in the reading font: Markdown
/// and HTML, word processors' documents, books the built-in engine
/// reflows, mail (ADR-073). Code and text files are the code font's;
/// tables, archives and the rest the interface's.
fn reads(kind: InspectionKind) -> bool {
    matches!(
        kind,
        InspectionKind::Markdown | InspectionKind::Book | InspectionKind::Mail
    )
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
    if display.data.kind == InspectionKind::Model {
        return crate::model_preview::content(shell, display, theme);
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
    let mono = theme.code_font.clone();
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
    let mut list_style = list_style;
    // What is read is written in the reading font (ADR-073): its blocks,
    // and a note opened under its reference.
    list_style.font = reads(display.data.kind).then(|| theme.reading_font.clone());
    let note = (!view.parked)
        .then(|| crate::note_popover::popover(shell, &list_style, theme, cx))
        .flatten();
    list_style.armed_link = shell.armed_link();
    list_style.labels = state.labels.as_ref().map(|labels| Rc::new(labels.shown()));
    // While the find is open, what it looks for is marked; a text read
    // where it is shown is told where the match it is on was drawn.
    list_style.sought = state
        .sought
        .as_ref()
        .filter(|_| state.find.is_some())
        .map(|sought| {
            Rc::new(crate::inspector_text::Sought {
                seen: state
                    .text
                    .as_ref()
                    .filter(|_| display.data.text.is_some())
                    .map(|text| Rc::clone(&text.spot)),
                ..sought.clone()
            })
        });
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
    // A text read where it is shown: its rows, scrolled by row, instead of
    // the list (`text_view`).
    let text = state.text.as_ref().filter(|_| data.text.is_some());
    // A file's bytes, as many rows as it holds (`bytes_view`).
    let bytes = state
        .bytes
        .as_ref()
        .filter(|_| data.kind == InspectionKind::Hex);
    let bytes_rows = bytes.map(|view| {
        crate::bytes_view::element(
            view,
            list_theme.clone(),
            hex_font.clone(),
            selected,
            pane,
            entity.clone(),
            shell.a11y,
        )
    });
    let wrap = shell.prefs.wrap_lines;
    let rows = text.map(|view| {
        crate::text_view::element(
            view,
            crate::text_view::Look {
                theme: list_theme.clone(),
                style: list_style.clone(),
                mono: mono.clone(),
                font_size,
                selected,
                selection,
                pane,
                entity: entity.clone(),
                wrap,
                covered: state.find.as_ref().map(|find| Rc::clone(&find.drawn)),
            },
        )
    });
    let a11y = shell.a11y;
    let list = gpui::list(state.list.clone(), move |index, _, _| {
        let theme = &list_theme;
        if index == 0 {
            // The notice is the frame's strip (`notice`); the first item is
            // only the picture an inspection may carry.
            return match images[0].as_ref() {
                Some(picture) => div()
                    .p_2()
                    .child(picture.img(theme).max_w_full())
                    .into_any_element(),
                None => div().into_any_element(),
            };
        }
        let i = index - 1;
        if data.kind == InspectionKind::Code {
            let line = visible[i];
            let fold = data
                .folds
                .binary_search_by_key(&line, |fold| fold.start)
                .ok()
                .map(|i| (&data.folds[i], collapsed.contains(&line)));
            return crate::code_preview::CodeRow {
                block: &data.blocks[line],
                index: line,
                number: Some(line as u64 + 1),
                digits: crate::code_preview::digits(data.blocks.len() as u64),
                fold,
                shift: 0.,
                selected: selected == Some(line),
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
                (Some(picture), _) => Some(crate::markdown::Shown::ready(picture, theme)),
                (None, Some(Picture::Later { key, width, height })) => {
                    let decoded = loaded.get(key).map(|decoded| &decoded.picture);
                    Some(crate::markdown::Shown {
                        image: decoded.map(|picture| &picture.image),
                        backdrop: decoded.and_then(|picture| {
                            crate::backdrop::behind(picture.ink, theme.bg, theme)
                        }),
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
        if data.kind == InspectionKind::Hex {
            let ri = visible[i - data.blocks.len()];
            let index = data.blocks.len() + ri;
            let chosen = selected == Some(index);
            return said_hex(
                hex_row(
                    &data.rows[ri],
                    index,
                    chosen,
                    &hex_font,
                    theme,
                    pane,
                    entity.clone(),
                ),
                &data.rows[ri],
                chosen,
                a11y,
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
                let (cell, clip) =
                    crate::elements::clip::text(cell.chars().take(512).collect::<String>());
                row = row.child(
                    div()
                        .id(("cell", column))
                        .optional_tooltip(clip.tooltip(None))
                        .w(px(column_width(shape.as_deref(), data.kind, column)))
                        .flex_none()
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
            let chosen = selected == Some(data.blocks.len() + ri);
            row = said_row(
                row,
                r,
                &data.columns,
                data.kind,
                position,
                chosen,
                collapsed.contains(&ri),
                a11y,
            );
            if chosen {
                row = row.bg(theme.selection_bg);
            }
            row.into_any_element()
        }
    })
    .size_full();
    // A table and bytes scroll sideways when wider than the panel.
    let wide = matches!(
        display.data.kind,
        InspectionKind::Table | InspectionKind::Hex
    );
    // What the vertical bar drives.
    let scrolls = match (text, bytes) {
        (Some(view), _) => crate::elements::scrollbar::Scrolls::Rows(view.scroll.clone()),
        (_, Some(view)) => crate::elements::scrollbar::Scrolls::Rows(view.scroll.clone()),
        _ => crate::elements::scrollbar::Scrolls::Items(state.list.clone()),
    };
    // To assistive technology: an archive a tree of its members, a
    // message's parts and a table's rows a list, anything else a
    // document, named by the file shown.
    let role = match display.data.kind {
        InspectionKind::Archive => gpui::Role::Tree,
        InspectionKind::Mail | InspectionKind::Table => gpui::Role::ListBox,
        _ => gpui::Role::Document,
    };
    let mut body = div()
        .id("inspector")
        .role(role)
        .when_some(crate::preview::said(shell), |d, (name, _)| {
            d.aria_label(name).when(role == gpui::Role::ListBox, |d| {
                d.aria_size_of_set(display.data.rows.len())
            })
        })
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        // The files of an archive or a message are walked with the arrows.
        .key_context(
            match (
                kept_rows(display.data.kind),
                state.labels.is_some(),
                text.is_some() || bytes.is_some(),
            ) {
                (true, false, _) => "Inspector PreviewMembers",
                (true, true, _) => "Inspector PreviewMembers labelling",
                (false, false, false) => "Inspector",
                (false, true, false) => "Inspector labelling",
                (false, false, true) => "Inspector PreviewText",
                (false, true, true) => "Inspector PreviewText labelling",
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
                    .when(table, |d| d.optional_tooltip(clip.tooltip(None)))
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
                    .map(|d| match rows.or(bytes_rows) {
                        Some(rows) => d.child(rows),
                        None => d.child(list),
                    }),
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
            // Over what does not scroll sideways: a table or bytes wider
            // than the panel keep their bars in view (below).
            .when(!wide, |d| {
                d.child(crate::elements::scrollbar::vertical(
                    "inspect-scrollbar",
                    scrolls.clone(),
                ))
            })
            // Lines that do not wrap go on sideways.
            .when_some(text.filter(|_| !wrap), |d, view| {
                d.child(crate::elements::scrollbar::horizontal(
                    "inspect-scrollbar-sideways",
                    crate::elements::scrollbar::Scrolls::Rows(view.scroll.clone()),
                ))
            })
            // Ctrl and the wheel size the text, as the zoom keys do.
            .child(zoom_wheel::overlay(
                Zoomed::Text {
                    parked: view.parked,
                },
                cx,
            )),
    );
    let mut decoded = None;
    if display.data.kind == InspectionKind::Hex
        && let Some(row) = state.selected.and_then(|selected| match &state.bytes {
            Some(bytes) => bytes.row(selected),
            None => selected
                .checked_sub(display.data.blocks.len())
                .and_then(|index| display.data.rows.get(index)),
        })
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
    if wide {
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
                    .relative()
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
                            .track_scroll(&state.sideways)
                            .child(body.w(px(width)).min_w_full().h_full()),
                    )
                    .child(
                        crate::elements::scrollbar::vertical("inspect-scrollbar", scrolls.clone())
                            .across(state.sideways.clone()),
                    )
                    .child(
                        crate::elements::scrollbar::horizontal(
                            "inspect-scrollbar-sideways",
                            state.sideways.clone(),
                        )
                        .across(scrolls),
                    ),
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

/// A row of the byte view: its offset, its bytes and their characters, in
/// the coding font with no ligatures, every other one tinted. A click
/// selects it, and the strip under the view says what its bytes mean.
/// `index` is the row's, as selection counts them.
pub(crate) fn hex_row(
    row: &skwir_core::inspection::Row,
    index: usize,
    selected: bool,
    font: &gpui::Font,
    theme: &Theme,
    pane: skwir_core::workspace::PaneId,
    entity: gpui::WeakEntity<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let mut line = div()
        .id(("inspect-row", index))
        .p_2()
        .pl(px(8.))
        .w_full()
        .flex()
        .gap_3()
        .font(font.clone())
        .text_size(px(13.))
        .line_height(px(18.))
        .py(px(1.))
        .whitespace_nowrap()
        .when(index % 2 == 1, |d| d.bg(theme.zebra))
        .when(selected, |d| d.bg(theme.selection_bg))
        .when(!selected, |d| d.hover(|d| d.bg(theme.hover)));
    for (column, cell) in row.cells.iter().enumerate() {
        line = line.child(
            div()
                .id(("cell", column))
                .w(px(column_width(None, InspectionKind::Hex, column)))
                .flex_none()
                .when(column < 2, |d| d.border_r_1().border_color(theme.border))
                .child(hex_cell(cell, column, theme)),
        );
    }
    line.cursor_pointer().on_click(move |_, window, cx| {
        let _ = entity.update(cx, |s, cx| {
            s.with_pane(pane, |s| {
                s.run(
                    "ui.inspect",
                    serde_json::json!({"action":"select","index":index}),
                    window,
                    cx,
                );
            });
        });
    })
}

/// A row of bytes as assistive technology reads it: a line of its offset,
/// its bytes and what they read as (`a11y`: while a screen reader listens).
pub(crate) fn said_hex(
    line: gpui::Stateful<gpui::Div>,
    row: &skwir_core::inspection::Row,
    selected: bool,
    a11y: bool,
) -> AnyElement {
    line.role(gpui::Role::Label)
        .when(a11y, |d| {
            let cells: Vec<&str> = row
                .cells
                .iter()
                .map(String::as_str)
                .filter(|cell| !cell.is_empty())
                .collect();
            // A label's words are its value, as gpui's text gives them.
            d.aria_value(cells.join(", ")).aria_selected(selected)
        })
        .into_any_element()
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
    /// The contents stay open when the next section comes, as when the
    /// same one is read again; a new file starts from a new state.
    #[test]
    fn the_contents_stay_open_from_chapter_to_chapter() {
        use super::*;
        let chapter = |position| {
            Display::new(
                Inspection {
                    kind: InspectionKind::Book,
                    position,
                    blocks: vec![Block {
                        text: "Text".into(),
                        ..Block::default()
                    }],
                    ..Inspection::default()
                },
                &crate::code_preview::CodeTheme::own(false),
            )
        };
        let mut state = State {
            outline: true,
            ..State::default()
        };
        state.arrived(&chapter(0), false);
        state.arrived(&chapter(1), false);
        assert!(state.outline, "the next chapter keeps them open");
        state.arrived(&chapter(1), true);
        assert!(state.outline, "the chapter read again too");
    }

    /// The part shown: a book's chapter by the section shown and where in
    /// it the view is, else the heading at the top of the view.
    #[test]
    fn the_part_shown_is_the_chapter_then_the_heading_at_the_top() {
        use super::*;
        use skwir_core::inspection::Row;
        let row = |target: &str, section| Row {
            cells: vec![target.into()],
            target: Some(target.into()),
            section,
            ..Row::default()
        };
        let rows = [
            row("cover.xhtml", Some(0)),
            row("one.xhtml", Some(2)),
            row("one.xhtml#later", Some(2)),
            row("one.xhtml#gone", Some(2)),
            row("two.xhtml#start", Some(3)),
            row("elsewhere.xhtml", None),
        ];
        let chapters: Vec<&Row> = rows.iter().collect();
        let anchored = |name: &str| Block {
            anchors: vec![name.into()],
            ..Block::default()
        };
        let book = |position, blocks: Vec<Block>| Inspection {
            kind: InspectionKind::Book,
            position,
            blocks,
            ..Inspection::default()
        };
        let one = book(
            2,
            vec![Block::default(), Block::default(), anchored("later")],
        );
        assert_eq!(current_chapter(&one, &chapters, 0), Some(1));
        assert_eq!(
            current_chapter(&one, &chapters, 2),
            Some(2),
            "where its fragment points; one the chapter does not hold, nowhere"
        );
        let two = book(
            3,
            vec![Block::default(), Block::default(), anchored("start")],
        );
        assert_eq!(
            current_chapter(&two, &chapters, 0),
            Some(4),
            "above its fragment, its first row still"
        );
        let between = book(1, vec![Block::default()]);
        assert_eq!(
            current_chapter(&between, &chapters, 0),
            Some(0),
            "a section the contents do not name is the one before's"
        );
        let first = Inspection {
            position: 0,
            ..between.clone()
        };
        assert_eq!(current_chapter(&first, &chapters, 0), Some(0));
        let none: Vec<&Row> = Vec::new();
        assert_eq!(current_chapter(&one, &none, 0), None);

        let headings = [1, 4, 9];
        assert_eq!(current_heading(&headings, 0), None, "before the first");
        assert_eq!(current_heading(&headings, 4), Some(1));
        assert_eq!(current_heading(&headings, 8), Some(1));
        assert_eq!(current_heading(&headings, 20), Some(2));
    }

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
        assert_eq!(display.find("needle", (0, 0), true), Some((1, 3..9)));
        assert_eq!(display.find("needle", (1, 4), true), Some((3, 5..11)));
        assert_eq!(
            display.find("needle", (3, 6), true),
            Some((1, 3..9)),
            "wraps"
        );
        assert_eq!(display.find("needle", (3, 6), false), None);
        assert_eq!(display.find_back("NEEDLE", (3, 5), true), Some((1, 3..9)));
        assert_eq!(
            display.find_back("needle", (1, 3), true),
            Some((3, 5..11)),
            "back around the end"
        );
        assert_eq!(display.find("absent", (0, 0), true), None);
        assert_eq!(display.find("", (0, 0), true), None);
        let mut state = State::default();
        state.reset(&display.data);
        state.collapse_all(&display.data, true);
        assert_eq!(state.visible(), [0, 5]);
        state.reveal(&display.data, 3);
        assert_eq!(state.selected, Some(3));
        assert_eq!(state.visible(), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn find_goes_from_match_to_match_and_marks_them() {
        let display = code(&["a needle and a Needle", "needle"], Vec::new());
        let mut state = State::default();
        state.reset(&display.data);
        // As F3 and Shift+F3 do: from where the find is, to the match.
        let mut step = |query: &str, back: bool| {
            let top = if back { (usize::MAX, 0) } else { (0, 0) };
            let from = state.find_from(query, back, top);
            let found = if back {
                display.find_back(query, from, true)
            } else {
                display.find(query, from, true)
            };
            let (item, range) = found.unwrap();
            state.selected = Some(item);
            state.sought.as_mut().unwrap().current = Some((item, range.clone()));
            (item, range)
        };
        assert_eq!(step("needle", false), (0, 2..8));
        assert_eq!(step("needle", false), (0, 15..21), "the next in its line");
        assert_eq!(step("NEEDLE", false), (1, 0..6), "the same, in capitals");
        assert_eq!(step("needle", true), (0, 15..21));
        assert_eq!(
            step("need", false),
            (0, 15..19),
            "a new query, from the match it was on"
        );
        // Every match of what it looks for is marked, the one it is on
        // more strongly.
        state.find_from("needle", false, (0, 0));
        state.sought.as_mut().unwrap().current = Some((0, 15..21));
        let sought = state.sought.as_ref().unwrap();
        let block = &display.data.blocks[0];
        assert_eq!(
            sought.marks(block, 0, 0..block.text.len()),
            [(2..8, false), (15..21, true)]
        );
        assert_eq!(
            sought.marks(block, 0, 10..block.text.len()),
            [(15..21, true)]
        );
        assert_eq!(
            sought.marks(block, 1, 0..block.text.len())[1],
            (15..21, false)
        );
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
        let item = |found: Option<(usize, std::ops::Range<usize>)>| found.map(|(item, _)| item);
        assert_eq!(item(display.find("zoo's", (0, 0), true)), Some(2));
        assert_eq!(item(display.find("child", (3, 0), false)), Some(5));
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
