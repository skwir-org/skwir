//! Text that flows: a document's blocks (paragraphs, headings, lists,
//! quotes, code, rules, tables, pictures, notes) laid out down pages in
//! the reading face, and a font's specimen, which is such a flow too.
//!
//! Each block is laid out once to know its rows (its lines, a table's
//! rows), and the rows are stacked on pages: a block goes on over a page
//! between two of them, a heading never ends a page, a table's header
//! rows are repeated above the rest. Drawing a page lays its blocks out
//! again, the last kept for the page after; so what is held between the
//! two passes is each block's rows' heights.

use std::borrow::Cow;
use std::collections::HashMap;
use std::num::NonZeroU16;
use std::ops::Range;
use std::rc::Rc;

use anyhow::Result;
use krilla::geom::{PathBuilder, Rect};
use krilla::paint::FillRule;
use krilla::tagging::{ArtifactType, ListNumbering, TableHeaderScope, Tag, TagKind};
use parley::{Alignment, BreakReason, Language, Layout};
use skwir_core::print::{Align, Block, BlockKind, Picture, Run};

use crate::fonts::{Fonts, Role};
use crate::geometry::{self, Area, Frame};
use crate::out::{Heading, PageCx, Part, Structure, key};
use crate::text::{self, CODE_BACK, GREY, Ink, RULE, Spec, Wrap};
use crate::{code, pictures};

pub const BODY: f32 = 10.5;
const LINE_HEIGHT: f32 = 1.4;
const HEADINGS: [f32; 6] = [1.8, 1.5, 1.25, 1.1, 1.0, 0.9];
const LIST_INDENT: f32 = 18.0;
const QUOTE_INDENT: f32 = 14.0;
const CODE_SIZE: f32 = 9.0;
const CODE_PAD: f32 = 6.0;
const CELL_X: f32 = 4.0;
const CELL_Y: f32 = 2.5;
/// The lists a block is indented and tagged in, at most: a deeper one
/// is an item of the deepest.
const LIST_DEPTH: u8 = 12;
/// Points to a pixel of a picture in a document, as a browser has it.
const POINTS_PER_PIXEL: f32 = 0.75;

/// Kinds of group keys, telling apart groups of one block.
mod group {
    pub const QUOTE: u8 = 1;
    pub const LIST: u8 = 2;
    pub const ITEM: u8 = 3;
    pub const BODY: u8 = 4;
    pub const LABEL: u8 = 5;
    pub const BLOCK: u8 = 6;
    pub const TABLE: u8 = 7;
    pub const ROW: u8 = 8;
    pub const CELL: u8 = 9;
    pub const CAPTION: u8 = 10;
}

#[derive(Clone, Debug)]
enum Kind {
    Text,
    Code,
    Rule,
    Picture {
        width: f32,
        height: f32,
    },
    /// Rows `blocks` of the part's blocks, the first `head` of them its
    /// header, in columns of these widths, written at `size`.
    Table {
        blocks: Range<usize>,
        head: usize,
        columns: Vec<f32>,
        size: f32,
    },
}

/// What a block is written in when not as the job's blocks say (a
/// specimen's lines).
#[derive(Clone, Debug)]
struct Look {
    role: Role,
    size: f32,
    color: [u8; 3],
    bold: bool,
}

#[derive(Clone, Debug)]
struct Item {
    kind: Kind,
    block: usize,
    /// Left of the text area, and the width it has.
    x: f32,
    width: f32,
    look: Look,
    /// Each row's top and bottom, in the block's own height.
    rows: Vec<(f32, f32)>,
    before: f32,
    after: f32,
    keep: bool,
    /// The groups it stands in (a quote, lists), shared with the blocks
    /// beside it in the same ones, then its own.
    groups: Rc<[(u64, TagKind)]>,
    own: Option<(u64, TagKind)>,
    /// A list item's first block, whose marker is drawn in the groups up
    /// to its item: `groups` but the last list's body.
    marked: bool,
    /// Where a quote's bar is drawn.
    bar: Option<f32>,
    /// The height of the header repeated over the rows on a page after.
    head: f32,
    /// A code block's stretch as it is laid out.
    code: Option<CodeText>,
}

/// A stretch of a code block laid out apart: its text with its tabs
/// turned to spaces, its runs, and whether its first row goes on from a
/// line the stretch before began.
#[derive(Clone, Debug)]
struct CodeText {
    text: String,
    runs: Vec<Run>,
    continues: bool,
}

impl Item {
    /// The groups it stands in, its own last.
    fn path(&self) -> Vec<(u64, TagKind)> {
        let mut path = Vec::with_capacity(self.groups.len() + 1);
        path.extend_from_slice(&self.groups);
        path.extend(self.own.clone());
        path
    }

    /// The groups its marker stands in: those up to its list item.
    fn marker_path(&self) -> Option<&[(u64, TagKind)]> {
        self.marked
            .then(|| &self.groups[..self.groups.len().saturating_sub(1)])
    }
}

/// Rows of an item on a page: drawn with the first's top at `y` (from the
/// text's top), or a slice of one row too tall for a page.
#[derive(Clone, Debug)]
struct Piece {
    item: usize,
    rows: Range<usize>,
    y: f32,
    clip: Option<(f32, f32)>,
}

pub struct Flow<'a> {
    blocks: Cow<'a, [Block]>,
    /// Shared, so a page drawn borrows an item while it draws.
    items: Rc<[Item]>,
    pages: Vec<Vec<Piece>>,
    frame: Frame,
    language: Option<Language>,
    cache: Option<(usize, Layout<Ink>, Vec<String>)>,
    /// A table's row sliced over pages, by its block, its cells laid out
    /// once for all its slices.
    tall: Option<(usize, Vec<Option<Cell>>)>,
    structure: Structure,
}

/// A cell of a table's row laid out: its layout, text and links.
struct Cell {
    layout: Layout<Ink>,
    text: String,
    links: Vec<String>,
}

/// A block's text from `range`, with its runs in it moved to start there.
fn slice(text: &str, runs: &[Run], range: Range<usize>) -> (String, Vec<Run>) {
    let start = text.floor_char_boundary(range.start.min(text.len()));
    let end = text
        .floor_char_boundary(range.end.min(text.len()))
        .max(start);
    let runs = runs
        .iter()
        .filter_map(|run| {
            let from = (run.start as usize).max(start);
            let to = (run.end as usize).min(end);
            (from < to).then(|| Run {
                start: (from - start) as u32,
                end: (to - start) as u32,
                ..run.clone()
            })
        })
        .collect();
    (text[start..end].to_owned(), runs)
}

/// A block's own scale: its size, smaller for a note.
fn scale(block: &Block) -> f32 {
    let size = if block.size > 0.0 && block.size.is_finite() {
        block.size.clamp(0.25, 8.0)
    } else {
        1.0
    };
    if block.note.is_some() {
        size * 0.85
    } else {
        size
    }
}

fn align(align: Align) -> Alignment {
    match align {
        Align::Start => Alignment::Start,
        Align::Center => Alignment::Center,
        Align::End => Alignment::End,
        Align::Justify => Alignment::Justify,
    }
}

impl<'a> Flow<'a> {
    pub fn plan(
        blocks: &'a [Block],
        fonts: &mut Fonts,
        frame: Frame,
        language: Option<Language>,
        first_page: usize,
        anchors: &mut HashMap<String, (usize, f32)>,
        headings: &mut Vec<Heading>,
    ) -> Flow<'a> {
        let looks = vec![None; blocks.len()];
        Flow::build(
            Cow::Borrowed(blocks),
            &looks,
            fonts,
            frame,
            language,
            first_page,
            anchors,
            headings,
        )
    }

    /// A font shown: its name in the text's face, then for each size the
    /// sample written in it, under the size.
    #[allow(clippy::too_many_arguments)] // What a specimen is, and where.
    pub fn specimen(
        name: &str,
        sample: &str,
        sizes: &[f32],
        fonts: &mut Fonts,
        frame: Frame,
        language: Option<Language>,
        first_page: usize,
        headings: &mut Vec<Heading>,
    ) -> Flow<'static> {
        let mut blocks = vec![Block {
            text: name.to_owned(),
            heading: 1,
            ..Block::default()
        }];
        let mut looks = vec![None];
        for &size in sizes {
            if !(size.is_finite() && size > 0.0) {
                continue;
            }
            let size = size.min(400.0);
            blocks.push(Block {
                text: format!("{} pt", trim_number(size)),
                ..Block::default()
            });
            looks.push(Some(Look {
                role: Role::Text,
                size: 7.5,
                color: GREY,
                bold: false,
            }));
            blocks.push(Block {
                text: sample.to_owned(),
                ..Block::default()
            });
            looks.push(Some(Look {
                role: Role::Specimen,
                size,
                color: text::INK,
                bold: false,
            }));
        }
        Flow::build(
            Cow::Owned(blocks),
            &looks,
            fonts,
            frame,
            language,
            first_page,
            &mut HashMap::new(),
            headings,
        )
    }

    #[allow(clippy::too_many_arguments)] // The blocks and what is learnt of them.
    fn build(
        blocks: Cow<'a, [Block]>,
        looks: &[Option<Look>],
        fonts: &mut Fonts,
        frame: Frame,
        language: Option<Language>,
        first_page: usize,
        anchors: &mut HashMap<String, (usize, f32)>,
        headings: &mut Vec<Heading>,
    ) -> Flow<'a> {
        let items = items(&blocks, looks, fonts, frame, language);
        let pages = paginate(&items, frame.text.height);
        // Where each block is: its first piece, or where it would be.
        let mut placed: Vec<Option<(usize, f32)>> = vec![None; items.len()];
        for (page, pieces) in pages.iter().enumerate() {
            for piece in pieces {
                if placed[piece.item].is_none() {
                    let top = piece.y - items[piece.item].before.min(piece.y);
                    placed[piece.item] = Some((page, frame.text.y + top));
                }
            }
        }
        let mut last = (0, frame.text.y);
        for (index, item) in items.iter().enumerate() {
            let at = *placed[index].get_or_insert(last);
            last = at;
            let block = &blocks[item.block];
            // A table's rows are its blocks: a name on any leads to it.
            let named = match &item.kind {
                Kind::Table { blocks: rows, .. } => rows.clone(),
                _ => item.block..item.block + 1,
            };
            for anchor in named.flat_map(|row| &blocks[row].anchors) {
                anchors
                    .entry(anchor.clone())
                    .or_insert((first_page + at.0, at.1));
            }
            if block.heading > 0 && matches!(item.kind, Kind::Text) {
                headings.push(Heading {
                    level: block.heading.min(6),
                    title: block.text.split_whitespace().collect::<Vec<_>>().join(" "),
                    page: first_page + at.0,
                    y: at.1,
                });
            }
        }
        Flow {
            blocks,
            items: Rc::from(items),
            pages,
            frame,
            language,
            cache: None,
            tall: None,
            structure: Structure::default(),
        }
    }

    /// The text spec of an item of text or code.
    fn spec(&self, item: &Item) -> Spec {
        spec_of(&self.blocks[item.block], item, self.language)
    }

    /// An item of text or code laid out, the last kept.
    fn layout(&mut self, fonts: &mut Fonts, index: usize) {
        if self.cache.as_ref().is_some_and(|(at, _, _)| *at == index) {
            return;
        }
        let item = &self.items[index];
        let spec = self.spec(item);
        let (text, runs) = text_of(&self.blocks, item);
        let mut layout = self
            .cache
            .take()
            .map(|(_, layout, _)| layout)
            .unwrap_or_default();
        let mut links = Vec::new();
        text::lay_out(fonts, &mut layout, text, runs, &spec, &mut links);
        self.cache = Some((index, layout, links));
    }

    fn draw_piece(&mut self, cx: &mut PageCx, piece: &Piece) {
        let items = Rc::clone(&self.items);
        let item = &items[piece.item];
        let text_top = self.frame.text.y;
        let left = self.frame.text.x + item.x;
        let top = text_top + piece.y;
        let start = piece.rows.start;
        let row_top = item.rows.get(start).map_or(0.0, |row| row.0);
        let clip_shift = piece.clip.map_or(0.0, |(offset, _)| offset);
        let bottom = piece.clip.map_or_else(
            || top + item.rows.get(piece.rows.end - 1).map_or(0.0, |row| row.1) - row_top,
            |(_, height)| top + height,
        );
        if let Some(bar) = item.bar {
            let x = self.frame.text.x + bar;
            cx.artifact(ArtifactType::Layout, |surface, _| {
                if let Some(rect) = Rect::from_xywh(x, top, 2.5, (bottom - top).max(0.1)) {
                    text::rect(surface, rect, RULE);
                }
            });
        }
        // A slice of a row too tall for a page shows what falls between
        // its top and bottom, a table's header repeated above its first.
        let above = if repeats_header(item, piece) {
            item.head
        } else {
            0.0
        };
        let clipped = piece.clip.is_some_and(|(_, height)| {
            let rect = Rect::from_xywh(
                self.frame.text.x - 2.0,
                top - above,
                self.frame.text.width + 4.0,
                height + above,
            );
            let path = rect.and_then(|rect| {
                let mut path = PathBuilder::new();
                path.push_rect(rect);
                path.finish()
            });
            path.inspect(|path| cx.surface.push_clip_path(path, &FillRule::NonZero))
                .is_some()
        });
        let origin_y = top - row_top - clip_shift;
        match &item.kind {
            Kind::Text => self.draw_text(cx, piece, item, (left, origin_y)),
            Kind::Code => {
                cx.artifact(ArtifactType::Layout, |surface, _| {
                    let rect = Rect::from_xywh(left, top, item.width, (bottom - top).max(0.1));
                    if let Some(rect) = rect {
                        text::rect(surface, rect, CODE_BACK);
                    }
                });
                self.draw_code(cx, piece, item, (left, origin_y));
            }
            Kind::Rule => {
                cx.artifact(ArtifactType::Layout, |surface, _| {
                    if let Some(rect) = Rect::from_xywh(left, top, item.width, 0.75) {
                        text::rect(surface, rect, RULE);
                    }
                });
            }
            Kind::Picture { width, height } => {
                let block = &self.blocks[item.block];
                let BlockKind::Picture(picture) = &block.kind else {
                    unreachable!("a picture's item is of a picture");
                };
                let picture = picture.clone();
                let mut path = item.path();
                self.structure.goto(cx, &path);
                let x = left + (item.width - width) / 2.0;
                let area = Area {
                    x,
                    y: origin_y,
                    width: *width,
                    height: *height,
                };
                cx.content(|surface, _| pictures::draw_picture(surface, &picture, area));
                if let Some(caption) = picture.caption.as_deref().filter(|c| !c.is_empty()) {
                    let mut spec = Spec::new(Role::Text, 8.0);
                    spec.color = GREY;
                    spec.align = Alignment::Center;
                    spec.width = Some(item.width);
                    spec.language = self.language;
                    let layout = text::plain(cx.fonts, caption, &spec);
                    path.push((key(group::CAPTION, item.block), Tag::Caption.into()));
                    self.structure.goto(cx, &path);
                    cx.lines(
                        &layout,
                        caption,
                        &[],
                        0..layout.len(),
                        (left, origin_y + height + 3.0),
                    );
                }
            }
            Kind::Table { .. } => {
                let shown = piece.clip.map(|(_, height)| (top, top + height));
                self.draw_table(cx, piece, item, (left, top - clip_shift), shown);
            }
        }
        if clipped {
            cx.surface.pop();
        }
    }

    fn draw_text(&mut self, cx: &mut PageCx, piece: &Piece, item: &Item, origin: (f32, f32)) {
        self.layout(cx.fonts, piece.item);
        let block = &self.blocks[item.block];
        if piece.rows.start == 0
            && let (Some(marker), Some(marker_path)) = (&block.marker, item.marker_path())
        {
            // The marker hung left of the text, on its first baseline.
            let mut spec = self.spec(item);
            spec.wrap = Wrap::None;
            spec.align = Alignment::Start;
            let layout = text::plain(cx.fonts, marker, &spec);
            let baseline = self
                .cache
                .as_ref()
                .and_then(|(_, layout, _)| layout.get(0))
                .map_or(0.0, |line| line.metrics().baseline);
            let own = layout.get(0).map_or(0.0, |line| line.metrics().baseline);
            let mut path = marker_path.to_vec();
            path.push((key(group::LABEL, item.block), Tag::Lbl.into()));
            self.structure.goto(cx, &path);
            cx.lines(
                &layout,
                marker,
                &[],
                0..layout.len(),
                (origin.0 - 4.0 - layout.width(), origin.1 + baseline - own),
            );
        }
        self.structure.goto(cx, &item.path());
        let (_, layout, links) = self.cache.as_ref().expect("laid out");
        cx.lines(layout, &block.text, links, piece.rows.clone(), origin);
    }

    fn draw_code(&mut self, cx: &mut PageCx, piece: &Piece, item: &Item, origin: (f32, f32)) {
        self.layout(cx.fonts, piece.item);
        self.structure.goto(cx, &item.path());
        let block = &self.blocks[item.block];
        let (text, _) = text_of(&self.blocks, item);
        let (_, layout, links) = self.cache.as_ref().expect("laid out");
        let origin = (origin.0 + CODE_PAD + arrow_room(), origin.1 + CODE_PAD);
        cx.lines(layout, text, links, piece.rows.clone(), origin);
        // Rows that go on from the one before: "↪" in the gutter.
        let continued: Vec<usize> = piece
            .rows
            .clone()
            .filter(|&row| {
                if row == 0 {
                    return item.code.as_ref().is_some_and(|code| code.continues);
                }
                layout.get(row - 1).is_some_and(|line| {
                    matches!(
                        line.break_reason(),
                        BreakReason::Regular | BreakReason::Emergency
                    )
                })
            })
            .collect();
        if continued.is_empty() {
            return;
        }
        let mut spec = Spec::new(Role::Code, CODE_SIZE * scale(block));
        spec.color = GREY;
        spec.line_height = 1.3;
        let arrow = text::plain(cx.fonts, "↪", &spec);
        let tops: Vec<f32> = continued
            .iter()
            .filter_map(|&row| layout.get(row))
            .map(|line| line.metrics().baseline)
            .collect();
        let arrow_baseline = arrow.get(0).map_or(0.0, |line| line.metrics().baseline);
        let x = origin.0 - arrow_room();
        cx.artifact(ArtifactType::LineNumber, |surface, fonts| {
            for baseline in tops {
                text::draw_line(
                    surface,
                    fonts,
                    &arrow,
                    "↪",
                    0,
                    (x, origin.1 + baseline - arrow_baseline),
                    &mut |_, _| {},
                    &mut |_, _| {},
                );
            }
        });
    }

    /// The rows of a table's piece, its top at `(left, top)`; `shown`,
    /// for a slice of a row too tall for a page, what of it the page shows.
    fn draw_table(
        &mut self,
        cx: &mut PageCx,
        piece: &Piece,
        item: &Item,
        (left, top): (f32, f32),
        shown: Option<(f32, f32)>,
    ) {
        let Kind::Table {
            blocks,
            head,
            columns,
            size,
        } = &item.kind
        else {
            return;
        };
        let right = left + columns.iter().sum::<f32>();
        let mut y = top;
        let mut rules = vec![];
        // The header again over a page after the first.
        if repeats_header(item, piece) {
            y -= item.head;
            rules.push((y, 0.75));
            for row in 0..*head {
                let height = self.cell_row(
                    cx,
                    blocks.start + row,
                    columns,
                    *size,
                    (left, y),
                    None,
                    None,
                );
                y += height;
            }
            rules.push((y, 0.75));
        } else if piece.rows.start == 0 {
            rules.push((y, 0.75));
        }
        let path = item.path();
        for row in piece.rows.clone() {
            let block = blocks.start + row;
            let mut path = path.clone();
            path.push((key(group::ROW, block), Tag::TR.into()));
            let height = self.cell_row(cx, block, columns, *size, (left, y), Some(&path), shown);
            y += height;
            rules.push((y, if row + 1 == *head { 0.75 } else { 0.4 }));
        }
        cx.artifact(ArtifactType::Layout, |surface, _| {
            for (at, thickness) in rules {
                if let Some(rule) =
                    Rect::from_xywh(left, at - thickness / 2.0, right - left, thickness)
                {
                    text::rect(surface, rule, RULE);
                }
            }
        });
    }

    /// A row of a table drawn at `(x, y)`, its cells marked under `path`
    /// (a header repeated, not marked, without): its height. A row sliced
    /// over pages (`shown`, what of the page its slice shows) is laid out
    /// once for all its slices, and each draws the lines it shows alone.
    #[allow(clippy::too_many_arguments)] // A row, where, and what of it shows.
    fn cell_row(
        &mut self,
        cx: &mut PageCx,
        block: usize,
        columns: &[f32],
        size: f32,
        (left, y): (f32, f32),
        path: Option<&[(u64, TagKind)]>,
        shown: Option<(f32, f32)>,
    ) -> f32 {
        let row = &self.blocks[block];
        let BlockKind::Row { header, .. } = &row.kind else {
            return 0.0;
        };
        let header = *header;
        let laid;
        let cells: &[Option<Cell>] = if shown.is_some() {
            if self.tall.as_ref().is_none_or(|(at, _)| *at != block) {
                let cells = lay_out_row(cx.fonts, row, columns, size, self.language);
                self.tall = Some((block, cells));
            }
            &self.tall.as_ref().expect("laid out").1
        } else {
            laid = lay_out_row(cx.fonts, row, columns, size, self.language);
            &laid
        };
        let mut x = left;
        let mut height = 0.0f32;
        for (column, (&width, cell)) in columns.iter().zip(cells).enumerate() {
            let Some(Cell {
                layout,
                text,
                links,
            }) = cell
            else {
                x += width;
                continue;
            };
            let origin = (x + CELL_X, y + CELL_Y);
            let lines = shown.map_or(0..layout.len(), |shown| between(layout, origin.1, shown));
            if let Some(path) = path {
                let tag: TagKind = if header {
                    Tag::TH(TableHeaderScope::Column).into()
                } else {
                    Tag::TD.into()
                };
                let mut path = path.to_vec();
                path.push((key(group::CELL, column), tag));
                self.structure.goto(cx, &path);
                cx.lines(layout, text, links, lines, origin);
            } else {
                cx.artifact(ArtifactType::PaginationOther, |surface, fonts| {
                    for index in lines {
                        text::draw_line(
                            surface,
                            fonts,
                            layout,
                            text,
                            index,
                            origin,
                            &mut |_, _| {},
                            &mut |_, _| {},
                        );
                    }
                });
            }
            height = height.max(layout.height() + 2.0 * CELL_Y);
            x += width;
        }
        height
    }
}

/// Whether a piece of a table draws its header again above it: one that
/// starts a page after the header's, where `paginate` left room for it
/// (a slice of a tall row, on its first page only).
fn repeats_header(item: &Item, piece: &Piece) -> bool {
    let rows = head_rows(item);
    rows > 0
        && item.head > 0.0
        && piece.rows.start >= rows
        && piece.clip.is_none_or(|(offset, _)| offset <= 0.0)
}

/// The cells of a table's row laid out in `columns`, at `size`.
fn lay_out_row(
    fonts: &mut Fonts,
    row: &Block,
    columns: &[f32],
    size: f32,
    language: Option<Language>,
) -> Vec<Option<Cell>> {
    let BlockKind::Row { cells, header } = &row.kind else {
        return Vec::new();
    };
    columns
        .iter()
        .enumerate()
        .map(|(column, &width)| {
            let &(start, end) = cells.get(column)?;
            let (text, runs) = slice(&row.text, &row.runs, start as usize..end as usize);
            let mut spec = Spec::new(Role::Text, size);
            spec.bold = *header;
            spec.width = Some((width - 2.0 * CELL_X).max(size));
            spec.line_height = 1.3;
            spec.language = language;
            spec.align = align(row.align);
            let mut layout = Layout::new();
            let mut links = Vec::new();
            text::lay_out(fonts, &mut layout, &text, &runs, &spec, &mut links);
            Some(Cell {
                layout,
                text,
                links,
            })
        })
        .collect()
}

/// The lines of `layout`, drawn with its top at `top`, that show between
/// `from` and `to` down the page: found by halving, as a row sliced over
/// many pages has many lines.
fn between(layout: &Layout<Ink>, top: f32, (from, to): (f32, f32)) -> Range<usize> {
    let first_where = |test: &dyn Fn(usize) -> bool| {
        let (mut low, mut high) = (0, layout.len());
        while low < high {
            let middle = low + (high - low) / 2;
            if test(middle) {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        low
    };
    let metrics = |index: usize| layout.get(index).map(|line| *line.metrics());
    let start = first_where(&|index| metrics(index).is_none_or(|m| top + m.block_max_coord > from));
    let end = first_where(&|index| metrics(index).is_none_or(|m| top + m.block_min_coord >= to));
    start..end.max(start)
}

/// The text an item of text or code lays out, and its runs.
fn text_of<'s>(blocks: &'s [Block], item: &'s Item) -> (&'s str, &'s [Run]) {
    if let Some(code) = &item.code {
        return (&code.text, &code.runs);
    }
    let block = &blocks[item.block];
    (&block.text, &block.runs)
}

/// The room left of a code block's lines for "↪".
fn arrow_room() -> f32 {
    CODE_SIZE * 1.1
}

fn trim_number(size: f32) -> String {
    let text = format!("{size:.1}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn spec_of(block: &Block, item: &Item, language: Option<Language>) -> Spec {
    let mut spec = Spec::new(item.look.role, item.look.size);
    spec.color = item.look.color;
    spec.bold = item.look.bold;
    spec.language = language;
    if let Kind::Code = item.kind {
        spec.line_height = 1.3;
        spec.wrap = Wrap::Anywhere;
        spec.width = Some((item.width - 2.0 * CODE_PAD - arrow_room()).max(item.look.size * 4.0));
    } else {
        spec.line_height = if block.heading > 0 { 1.25 } else { LINE_HEIGHT };
        spec.align = align(block.align);
        spec.width = Some(item.width.max(item.look.size * 4.0));
    }
    spec
}

/// The groups blocks share: the quote they are in, the lists open, and
/// the groups those make.
type SharedGroups = (Option<usize>, Vec<(u8, usize, usize)>, Rc<[(u64, TagKind)]>);

/// The groups a list's block stands in, kept from block to block: each
/// open list's depth, the block that started it and the item it is in.
struct Lists {
    open: Vec<(u8, usize, usize)>,
    numbering: HashMap<usize, ListNumbering>,
}

impl Lists {
    /// Block `index` entered: the lists open as it stands in them (none
    /// outside a list), at most [`LIST_DEPTH`] deep.
    fn enter(&mut self, index: usize, block: &Block) {
        let depth = block.list_depth.min(LIST_DEPTH);
        if depth == 0 {
            self.open.clear();
            return;
        }
        while self.open.last().is_some_and(|(d, _, _)| *d > depth) {
            self.open.pop();
        }
        match self.open.last_mut() {
            Some((d, _, item)) if *d == depth => {
                if block.marker.is_some() {
                    *item = index;
                }
            }
            _ => {
                self.open.push((depth, index, index));
                let numbering = match block.marker.as_deref().and_then(|m| m.chars().next()) {
                    Some(c) if c.is_ascii_digit() => ListNumbering::Decimal,
                    Some(c) if c.is_ascii_lowercase() => ListNumbering::LowerAlpha,
                    Some(c) if c.is_ascii_uppercase() => ListNumbering::UpperAlpha,
                    Some(_) => ListNumbering::Disc,
                    None => ListNumbering::None,
                };
                self.numbering.insert(index, numbering);
            }
        }
    }

    /// The groups of the lists open, each a list, its item and the item's
    /// body.
    fn groups(&self, path: &mut Vec<(u64, TagKind)>) {
        for &(_, list, item) in &self.open {
            let numbering = self
                .numbering
                .get(&list)
                .copied()
                .unwrap_or(ListNumbering::None);
            path.push((key(group::LIST, list), Tag::L(numbering).into()));
            path.push((key(group::ITEM, item), Tag::LI.into()));
            path.push((key(group::BODY, item), Tag::LBody.into()));
        }
    }
}

/// The items of a flow's blocks, each laid out to know its rows.
fn items(
    blocks: &[Block],
    looks: &[Option<Look>],
    fonts: &mut Fonts,
    frame: Frame,
    language: Option<Language>,
) -> Vec<Item> {
    let mut items = Vec::with_capacity(blocks.len());
    let mut lists = Lists {
        open: Vec::new(),
        numbering: HashMap::new(),
    };
    let mut quote: Option<usize> = None;
    let mut shared: Option<SharedGroups> = None;
    let mut layout: Layout<Ink> = Layout::new();
    let mut index = 0;
    while index < blocks.len() {
        let block = &blocks[index];
        let scale = scale(block);
        if block.quote {
            quote.get_or_insert(index);
        } else {
            quote = None;
        }
        lists.enter(index, block);
        // Blocks in the same quote and list items share their groups.
        if shared
            .as_ref()
            .is_none_or(|(at, open, _)| *at != quote || *open != lists.open)
        {
            let mut path = Vec::with_capacity(1 + 3 * lists.open.len());
            if let Some(id) = quote {
                path.push((key(group::QUOTE, id), Tag::BlockQuote.into()));
            }
            lists.groups(&mut path);
            shared = Some((quote, lists.open.clone(), Rc::from(path)));
        }
        let groups = Rc::clone(&shared.as_ref().expect("made above").2);
        let mut x = block.indent.clamp(0.0, 20.0) * BODY * scale;
        let bar = block.quote.then_some(x);
        if block.quote {
            x += QUOTE_INDENT;
        }
        x += f32::from(block.list_depth.min(LIST_DEPTH)) * LIST_INDENT;
        let x = x.min(frame.text.width * 0.6);
        let width = frame.text.width - x;
        let look = looks.get(index).cloned().flatten();
        let mut item = Item {
            kind: Kind::Text,
            block: index,
            x,
            width,
            look: Look {
                role: Role::Text,
                size: BODY * scale,
                color: text::INK,
                bold: false,
            },
            rows: Vec::new(),
            before: 0.0,
            after: 6.0 * scale,
            keep: false,
            groups,
            own: None,
            marked: block.list_depth > 0 && block.marker.is_some(),
            bar,
            head: 0.0,
            code: None,
        };
        match &block.kind {
            BlockKind::Text => {
                let own: TagKind = if block.heading > 0 {
                    let level = block.heading.clamp(1, 6);
                    item.look.size = BODY * scale * HEADINGS[usize::from(level) - 1];
                    item.look.bold = true;
                    item.before = item.look.size * 0.9;
                    item.after = item.look.size * 0.35;
                    item.keep = true;
                    Tag::Hn(
                        NonZeroU16::new(u16::from(level)).expect("from 1"),
                        Some(block.text.split_whitespace().collect::<Vec<_>>().join(" ")),
                    )
                    .into()
                } else if block.note.is_some() {
                    item.after = 3.0;
                    Tag::Note.into()
                } else {
                    if block.list_depth > 0 {
                        item.after = 2.5;
                    }
                    Tag::P.into()
                };
                if let Some(look) = look {
                    item.look = look;
                    item.after = item.look.size * 0.5;
                    if item.look.role == Role::Text && item.look.size < BODY {
                        item.before = 6.0;
                        item.after = 1.0;
                    }
                }
                item.own = Some((key(group::BLOCK, index), own));
                let spec = spec_of(block, &item, language);
                text::lay_out(
                    fonts,
                    &mut layout,
                    &block.text,
                    &block.runs,
                    &spec,
                    &mut Vec::new(),
                );
                item.rows = layout
                    .lines()
                    .map(|line| {
                        (
                            line.metrics().block_min_coord,
                            line.metrics().block_max_coord,
                        )
                    })
                    .collect();
            }
            BlockKind::Code => {
                item.kind = Kind::Code;
                item.look = Look {
                    role: Role::Code,
                    size: CODE_SIZE * scale,
                    color: text::INK,
                    bold: false,
                };
                item.before = 4.0;
                item.after = 8.0;
                item.own = Some((key(group::BLOCK, index), Tag::Code.into()));
                let spec = spec_of(block, &item, language);
                let text = block.text.strip_suffix('\n').unwrap_or(&block.text);
                // Laid out a stretch at a time, so no layout holds more
                // than a piece of a long block: each an item, the same
                // group, no room between them, padded at the block's ends.
                let stretches = code_stretches(text, &block.runs);
                let last = stretches.len() - 1;
                for (at, code) in stretches.into_iter().enumerate() {
                    let mut stretch = item.clone();
                    text::lay_out(
                        fonts,
                        &mut layout,
                        &code.text,
                        &code.runs,
                        &spec,
                        &mut Vec::new(),
                    );
                    let count = layout.len();
                    stretch.rows = layout
                        .lines()
                        .enumerate()
                        .map(|(row, line)| {
                            let m = line.metrics();
                            let top = if at == 0 && row == 0 {
                                0.0
                            } else {
                                m.block_min_coord + CODE_PAD
                            };
                            let end = at == last && row + 1 == count;
                            let bottom =
                                m.block_max_coord + CODE_PAD + if end { CODE_PAD } else { 0.0 };
                            (top, bottom)
                        })
                        .collect();
                    stretch.code = Some(code);
                    if at > 0 {
                        stretch.before = 0.0;
                    }
                    if at < last {
                        stretch.after = 0.0;
                        items.push(stretch);
                    } else {
                        item = stretch;
                    }
                }
            }
            BlockKind::Rule => {
                item.kind = Kind::Rule;
                item.before = 8.0;
                item.after = 8.0;
                item.rows = vec![(0.0, 0.75)];
            }
            BlockKind::Picture(picture) => {
                let (width, height) = picture_size(picture, width, frame.text.height);
                item.kind = Kind::Picture { width, height };
                item.before = 4.0;
                item.after = 8.0;
                let caption = if picture.caption.as_deref().is_some_and(|c| !c.is_empty()) {
                    // Measured as it is drawn: across the text's width.
                    let mut spec = Spec::new(Role::Text, 8.0);
                    spec.width = Some(item.width);
                    spec.language = language;
                    let caption = picture.caption.as_deref().unwrap_or_default();
                    text::plain(fonts, caption, &spec).height() + 3.0
                } else {
                    0.0
                };
                item.rows = vec![(0.0, height + caption)];
                let alt = picture.alt.trim();
                item.own = Some((
                    key(group::BLOCK, index),
                    Tag::Figure((!alt.is_empty()).then(|| alt.to_owned())).into(),
                ));
            }
            BlockKind::Row { .. } => {
                let end = blocks[index..]
                    .iter()
                    .position(|b| !matches!(b.kind, BlockKind::Row { .. }))
                    .map_or(blocks.len(), |n| index + n);
                item.own = Some((key(group::TABLE, index), Tag::Table.into()));
                table(
                    &mut item,
                    &blocks[index..end],
                    index,
                    fonts,
                    language,
                    frame.text.height,
                );
                items.push(item);
                // The rows of a table are one item; the lists and quotes
                // they stand in are the first's.
                index = end;
                continue;
            }
        }
        items.push(item);
        index += 1;
    }
    items
}

/// A code block's text (`text`, its runs `runs`) cut into the stretches
/// laid out apart: whole lines up to [`code::PIECE`] bytes together, a
/// longer line in pieces of it. Each takes its runs, found by sweeping
/// them in order of their start, so a long block's are not each tested
/// against each stretch. One stretch at least.
fn code_stretches(text: &str, runs: &[Run]) -> Vec<CodeText> {
    let mut ranges: Vec<(Range<usize>, bool)> = Vec::new();
    let mut gathered: Option<Range<usize>> = None;
    let mut at = 0;
    for line in text.split('\n') {
        let range = at..at + line.len();
        at = range.end + 1;
        if line.len() > code::PIECE {
            ranges.extend(gathered.take().map(|whole| (whole, false)));
            ranges.extend(
                code::pieces(line)
                    .enumerate()
                    .map(|(n, piece)| (range.start + piece.start..range.start + piece.end, n > 0)),
            );
            continue;
        }
        match &mut gathered {
            Some(whole) if range.end - whole.start <= code::PIECE => whole.end = range.end,
            _ => ranges.extend(gathered.replace(range).map(|whole| (whole, false))),
        }
    }
    ranges.extend(gathered.map(|whole| (whole, false)));
    if ranges.len() == 1 {
        let (text, runs) = code::detab(text, runs);
        return vec![CodeText {
            text,
            runs,
            continues: false,
        }];
    }
    let mut sorted: Vec<&Run> = runs.iter().collect();
    sorted.sort_by_key(|run| run.start);
    let mut next = 0;
    let mut active: Vec<&Run> = Vec::new();
    ranges
        .into_iter()
        .map(|(range, continues)| {
            active.retain(|run| run.end as usize > range.start);
            while next < sorted.len() && (sorted[next].start as usize) < range.end {
                active.push(sorted[next]);
                next += 1;
            }
            let own: Vec<Run> = active
                .iter()
                .filter_map(|run| {
                    let from = (run.start as usize).max(range.start);
                    let to = (run.end as usize).min(range.end);
                    (from < to).then(|| Run {
                        start: (from - range.start) as u32,
                        end: (to - range.start) as u32,
                        ..(*run).clone()
                    })
                })
                .collect();
            let (text, runs) = code::detab(&text[range], &own);
            CodeText {
                text,
                runs,
                continues,
            }
        })
        .collect()
}

/// A picture's size in a document: its pixels as a browser shows them,
/// smaller to fit the width and the page.
fn picture_size(picture: &Picture, width: f32, height: f32) -> (f32, f32) {
    let (shown_width, shown_height) = picture.shown();
    if shown_width == 0 || shown_height == 0 {
        return (width.min(height), width.min(height) * 0.75);
    }
    let area = Area {
        x: 0.0,
        y: 0.0,
        width,
        height: height * 0.9,
    };
    let fitted = geometry::fit(
        shown_width as f32,
        shown_height as f32,
        area,
        POINTS_PER_PIXEL,
    );
    (fitted.width, fitted.height)
}

/// A table's columns and rows, from its row blocks, on pages of `room`.
fn table(
    item: &mut Item,
    rows: &[Block],
    first: usize,
    fonts: &mut Fonts,
    language: Option<Language>,
    room: f32,
) {
    let size = BODY * 0.9;
    let count = rows
        .iter()
        .map(|row| match &row.kind {
            BlockKind::Row { cells, .. } => cells.len(),
            _ => 0,
        })
        .max()
        .unwrap_or(0);
    let mut natural = vec![2.0 * CELL_X + size; count];
    let mut layout = Layout::new();
    for row in rows {
        let BlockKind::Row { cells, header } = &row.kind else {
            continue;
        };
        for (column, &(start, end)) in cells.iter().enumerate() {
            let (text, runs) = slice(&row.text, &row.runs, start as usize..end as usize);
            let mut spec = Spec::new(Role::Text, size);
            spec.bold = *header;
            spec.wrap = Wrap::None;
            text::lay_out(fonts, &mut layout, &text, &runs, &spec, &mut Vec::new());
            natural[column] = natural[column].max(layout.width() + 2.0 * CELL_X + 0.5);
        }
    }
    let most = if count > 1 {
        item.width * 0.6
    } else {
        item.width
    };
    for w in &mut natural {
        *w = w.min(most);
    }
    let fitted = geometry::columns(
        &natural,
        item.width,
        size,
        7.0,
        4.0 * size,
        2.0 * CELL_X,
        false,
    );
    let columns: Vec<f32> = fitted.groups[0].iter().map(|(_, w)| *w).collect();
    let size = fitted.size;
    let mut tops = 0.0;
    let mut heights = Vec::with_capacity(rows.len());
    for row in rows {
        let BlockKind::Row { cells, header } = &row.kind else {
            heights.push((tops, tops));
            continue;
        };
        let mut tallest = size * 1.3;
        for (column, &(start, end)) in cells.iter().enumerate() {
            let Some(&width) = columns.get(column) else {
                continue;
            };
            let (text, runs) = slice(&row.text, &row.runs, start as usize..end as usize);
            let mut spec = Spec::new(Role::Text, size);
            spec.bold = *header;
            spec.line_height = 1.3;
            spec.width = Some((width - 2.0 * CELL_X).max(size));
            spec.language = language;
            text::lay_out(fonts, &mut layout, &text, &runs, &spec, &mut Vec::new());
            tallest = tallest.max(layout.height());
        }
        let height = tallest + 2.0 * CELL_Y;
        heights.push((tops, tops + height));
        tops += height;
    }
    let head = rows
        .iter()
        .take_while(|row| matches!(row.kind, BlockKind::Row { header: true, .. }))
        .count();
    // The header is repeated over the rows on the pages after its own
    // only when it leaves them half a page at least: one taller is not,
    // and its rows go on alone. Decided here once, for the pages laid out
    // and drawn alike.
    let head_height: f32 = heights
        .get(..head)
        .map_or(0.0, |rows| rows.iter().map(|(t, b)| b - t).sum());
    item.head = if head_height <= room / 2.0 {
        head_height
    } else {
        0.0
    };
    item.rows = heights;
    item.before = 4.0;
    item.after = 8.0;
    item.kind = Kind::Table {
        blocks: first..first + rows.len(),
        head,
        columns,
        size,
    };
}

/// The items stacked on pages of `room`: each page's pieces.
fn paginate(items: &[Item], room: f32) -> Vec<Vec<Piece>> {
    let mut pages: Vec<Vec<Piece>> = vec![Vec::new()];
    let mut y = 0.0f32;
    let mut after = 0.0f32;
    let height = |item: &Item| {
        item.rows.last().map_or(0.0, |r| r.1) - item.rows.first().map_or(0.0, |r| r.0)
    };
    for (index, item) in items.iter().enumerate() {
        if item.rows.is_empty() {
            continue;
        }
        let mut gap = if y == 0.0 {
            0.0
        } else {
            after.max(item.before)
        };
        if item.keep
            && y > 0.0
            && let Some(next) = items[index + 1..].iter().find(|next| !next.rows.is_empty())
        {
            let first = next.rows[0].1 - next.rows[0].0;
            let need = gap + height(item) + item.after.max(next.before) + first;
            if y + need > room && need <= room {
                pages.push(Vec::new());
                y = 0.0;
                gap = 0.0;
            }
        }
        y += gap;
        let mut start = 0;
        let mut top = y;
        let mut row = 0;
        while row < item.rows.len() {
            let bottom = top + item.rows[row].1 - item.rows[start].0;
            if bottom <= room + 0.01 {
                row += 1;
                continue;
            }
            if row > start {
                pages.last_mut().expect("a page").push(Piece {
                    item: index,
                    rows: start..row,
                    y: top,
                    clip: None,
                });
                pages.push(Vec::new());
                start = row;
                top = if row > 0 && item.head > 0.0 && row >= head_rows(item) {
                    item.head
                } else {
                    0.0
                };
                continue;
            }
            // The row starts the piece and does not fit below: on a page
            // of its own, if this one has anything; sliced over pages
            // when it is taller than one.
            let empty = pages.last().is_some_and(Vec::is_empty) && top <= item.head + 0.01;
            if !empty {
                pages.push(Vec::new());
                top = if row > 0 && item.head > 0.0 && row >= head_rows(item) {
                    item.head
                } else {
                    0.0
                };
                continue;
            }
            let tall = item.rows[row].1 - item.rows[row].0;
            let mut offset = 0.0;
            while tall - offset > room - top + 0.01 {
                let slice = room - top;
                pages.last_mut().expect("a page").push(Piece {
                    item: index,
                    rows: row..row + 1,
                    y: top,
                    clip: Some((offset, slice)),
                });
                pages.push(Vec::new());
                offset += slice;
                top = 0.0;
            }
            pages.last_mut().expect("a page").push(Piece {
                item: index,
                rows: row..row + 1,
                y: top,
                clip: Some((offset, tall - offset)),
            });
            top += tall - offset;
            row += 1;
            start = row;
        }
        if row > start {
            let bottom = top + item.rows[row - 1].1 - item.rows[start].0;
            pages.last_mut().expect("a page").push(Piece {
                item: index,
                rows: start..row,
                y: top,
                clip: None,
            });
            y = bottom;
        } else {
            y = top;
        }
        after = item.after;
    }
    pages
}

/// How many of a table's first rows are its header.
fn head_rows(item: &Item) -> usize {
    match &item.kind {
        Kind::Table { head, .. } => *head,
        _ => 0,
    }
}

impl Part for Flow<'_> {
    fn pages(&self) -> usize {
        self.pages.len()
    }

    fn draw(&mut self, page: usize, cx: &mut PageCx) -> Result<()> {
        let pieces = self.pages.get(page).cloned().unwrap_or_default();
        for piece in &pieces {
            self.draw_piece(cx, piece);
        }
        if page + 1 == self.pages.len() {
            self.structure.close_all(cx);
        }
        Ok(())
    }

    fn finish(&mut self, cx: &mut PageCx) {
        self.structure.close_all(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(rows: &[(f32, f32)]) -> Item {
        Item {
            kind: Kind::Text,
            block: 0,
            x: 0.0,
            width: 100.0,
            look: Look {
                role: Role::Text,
                size: BODY,
                color: text::INK,
                bold: false,
            },
            rows: rows.to_vec(),
            before: 0.0,
            after: 0.0,
            keep: false,
            groups: Rc::from(Vec::new()),
            own: None,
            marked: false,
            bar: None,
            head: 0.0,
            code: None,
        }
    }

    fn lines(count: usize, height: f32) -> Vec<(f32, f32)> {
        (0..count)
            .map(|i| (i as f32 * height, (i + 1) as f32 * height))
            .collect()
    }

    #[test]
    fn a_paragraph_goes_on_over_the_page_between_lines() {
        let pages = paginate(&[item(&lines(25, 10.0))], 100.0);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0][0].rows, 0..10);
        assert_eq!(pages[1][0].rows, 10..20);
        assert_eq!(pages[2][0].rows, 20..25);
        assert_eq!(pages[1][0].y, 0.0);
    }

    #[test]
    fn a_heading_never_ends_a_page() {
        let filler = item(&lines(8, 10.0));
        let mut heading = item(&lines(1, 15.0));
        heading.keep = true;
        let next = item(&lines(3, 10.0));
        let pages = paginate(&[filler, heading, next], 100.0);
        // The heading would fit at 80..95, but its next line would not.
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].len(), 1);
        assert_eq!(pages[1][0].item, 1);
        assert_eq!(pages[1][0].y, 0.0);
    }

    #[test]
    fn a_table_s_header_is_room_kept_on_the_pages_after() {
        let mut table = item(&lines(30, 10.0));
        table.kind = Kind::Table {
            blocks: 0..30,
            head: 1,
            columns: vec![100.0],
            size: 9.0,
        };
        table.head = 10.0;
        let pages = paginate(&[table], 100.0);
        assert_eq!(pages[0][0].rows, 0..10);
        // Each page after holds nine rows under the header repeated.
        assert_eq!(pages[1][0].rows, 10..19);
        assert_eq!(pages[1][0].y, 10.0);
    }

    #[test]
    fn a_row_taller_than_a_page_is_sliced_over_pages() {
        let pages = paginate(&[item(&[(0.0, 10.0)]), item(&[(0.0, 250.0)])], 100.0);
        let slices: Vec<_> = pages
            .iter()
            .flatten()
            .filter(|piece| piece.item == 1)
            .map(|piece| piece.clip.unwrap())
            .collect();
        assert_eq!(slices, [(0.0, 100.0), (100.0, 100.0), (200.0, 50.0)]);
    }

    #[test]
    fn a_cell_s_text_and_runs_are_cut_from_the_row() {
        let runs = [Run {
            start: 2,
            end: 9,
            bold: true,
            ..Run::default()
        }];
        let (text, runs) = slice("a|bcd|efgh", &runs, 2..5);
        assert_eq!(text, "bcd");
        assert_eq!((runs[0].start, runs[0].end), (0, 3));
    }

    #[test]
    fn a_long_code_block_is_laid_out_a_stretch_at_a_time() {
        let short = code_stretches("fn main() {}\nlet a = 1;", &[]);
        assert_eq!(short.len(), 1);
        assert_eq!(short[0].text, "fn main() {}\nlet a = 1;");
        // Lines gathered up to a piece, a long one cut, its runs following.
        let line = "x".repeat(100);
        let mut text = vec![line.as_str(); 60].join("\n");
        let long_start = text.len() + 1;
        text.push('\n');
        text.push_str(&"y".repeat(code::PIECE * 2));
        let runs = [Run {
            start: (long_start + code::PIECE - 1) as u32,
            end: (long_start + code::PIECE + 1) as u32,
            bold: true,
            ..Run::default()
        }];
        let stretches = code_stretches(&text, &runs);
        // Forty lines, twenty, then the long one in two pieces: the two
        // line ends between stretches are where they part.
        assert_eq!(stretches.len(), 4);
        assert!(stretches.iter().all(|s| s.text.len() <= code::PIECE));
        let joined: usize = stretches.iter().map(|s| s.text.len()).sum();
        assert_eq!(joined + 2, text.len());
        let (cut, rest) = (
            &stretches[stretches.len() - 2],
            &stretches[stretches.len() - 1],
        );
        assert!(!cut.continues && rest.continues);
        assert_eq!(
            (cut.runs[0].start as usize, cut.runs[0].end as usize),
            (code::PIECE - 1, code::PIECE)
        );
        assert_eq!((rest.runs[0].start, rest.runs[0].end), (0, 1));
    }

    #[test]
    fn sizes_are_written_without_trailing_zeros() {
        assert_eq!(trim_number(12.0), "12");
        assert_eq!(trim_number(10.5), "10.5");
    }
}
