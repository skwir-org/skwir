//! Text read in both directions: the Unicode Bidirectional Algorithm
//! (UAX #9) where a line holds right-to-left text.
//!
//! The platform shapes a line and places its glyphs as the line reads:
//! Hebrew or Arabic drawn right to left, a number or a Latin word in it left
//! to right. What gpui keeps of that:
//!
//! - each byte's embedding level ([`BidiLevels`]), worked out here from the
//!   text by UAX #9, each paragraph's direction taken from its first strong
//!   character, as cosmic-text, Core Text and DirectWrite (asked so) take
//!   it;
//! - the glyphs in the order they are drawn, left to right
//!   (`LineLayout::put_in_drawn_order`), whatever order the platform handed
//!   them in.
//!
//! From both come the answers a line gives about places in its text
//! (`LineLayout::x_for_index` and the others):
//!
//! - **Cells.** The glyphs drawn together for one character or more (a
//!   letter and its marks, a ligature) are a cell, from its left edge to the
//!   next cell's.
//! - **Carets.** A place between two characters stands at the edge where
//!   the cell after it begins (its right edge, read right to left), or where
//!   the cell before it ends. Where the two differ (the text turns direction
//!   there), the place takes the edge no other place stands at, so a caret
//!   reaches every edge (both ends of a Hebrew name in an English line);
//!   where both are free, the edge of the cell at the lower level, the
//!   paragraph's own direction.
//! - **Selections** cover the cells of their characters: one span for each
//!   stretch of them drawn side by side.
//! - **Rows.** A line wraps in its text's order; each row is then laid out
//!   as it reads (UAX #9's L1 and L2 on the row), its glyphs moved, and the
//!   rows follow each other along x as a wrapped line's always have. A long
//!   Hebrew paragraph starts on its first row.
//!
//! A line all left to right (`LineLayout::bidi_levels` is `None`) keeps none
//! of this: its glyphs follow its text, and every answer is worked out as
//! before, as fast.

use crate::{FontId, LineLayout, Pixels, RunPosition, ShapedGlyph, ShapedRun, WrapBoundary, px};
use smallvec::SmallVec;
use std::{ops::Range, sync::Arc};
use unicode_bidi::{BidiClass, BidiInfo, bidi_class};

/// How a line reads in both directions: the embedding level (UAX #9) of
/// each byte of its text, and of each paragraph (a line holds several where
/// a paragraph separator parts it, U+001C to U+001E). An even level reads
/// left to right, an odd one right to left. A line keeps them where any of
/// it reads right to left (`LineLayout::bidi_levels`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BidiLevels {
    levels: Vec<u8>,
    /// Where each paragraph starts, and its level.
    paragraphs: SmallVec<[(usize, u8); 1]>,
}

impl BidiLevels {
    /// The levels of `text` laid out as one line (UAX #9 to its rule L1),
    /// or `None` where nothing in it reads right to left: told by a scan of
    /// its characters, at once for ASCII.
    pub fn of(text: &str) -> Option<Self> {
        if text.is_ascii() || !text.chars().any(turns_right_to_left) {
            return None;
        }
        let info = BidiInfo::new(text, None);
        let mut levels = Vec::with_capacity(text.len());
        let mut paragraphs = SmallVec::new();
        for paragraph in &info.paragraphs {
            let range = paragraph.range.clone();
            // L1: a paragraph's separators, and the spaces it ends with, at
            // its own level, as the platform draws it on one line.
            let laid = info.reordered_levels(paragraph, range.clone());
            levels.extend(laid[range.clone()].iter().map(|level| level.number()));
            paragraphs.push((range.start, paragraph.level.number()));
        }
        Some(Self { levels, paragraphs })
    }

    /// The level of the character at byte `index`; past the end, its
    /// paragraph's.
    pub fn level(&self, index: usize) -> u8 {
        self.levels
            .get(index)
            .copied()
            .unwrap_or_else(|| self.paragraph_level(index))
    }

    /// The level of the paragraph byte `index` is in.
    pub fn paragraph_level(&self, index: usize) -> u8 {
        let after = self
            .paragraphs
            .partition_point(|(start, _)| *start <= index);
        self.paragraphs
            .get(after.saturating_sub(1))
            .map_or(0, |(_, level)| *level)
    }

    /// Whether the line's first paragraph reads right to left: it starts
    /// at its right end.
    pub fn is_right_to_left(&self) -> bool {
        self.paragraph_level(0) % 2 == 1
    }

    /// These levels for `range` of the line, as a line of its own.
    pub(crate) fn slice(&self, range: Range<usize>) -> Self {
        let first = self.paragraph_level(range.start);
        let mut paragraphs: SmallVec<[(usize, u8); 1]> = SmallVec::new();
        paragraphs.push((0, first));
        paragraphs.extend(
            self.paragraphs
                .iter()
                .filter(|(start, _)| *start > range.start && *start < range.end)
                .map(|(start, level)| (start - range.start, *level)),
        );
        Self {
            levels: self
                .levels
                .get(range.clone())
                .map_or_else(Vec::new, <[u8]>::to_vec),
            paragraphs,
        }
    }
}

/// Whether `c` can make text read right to left: a right-to-left letter
/// (Hebrew, Arabic and the like) or mark (RLM), or a control that embeds,
/// overrides or isolates right to left. A text with none reads left to
/// right throughout: an Arabic number alone in it too, which UAX #9 sets two
/// levels up. The blocks such characters are in are asked first, so a line
/// of Latin, Cyrillic or CJK looks nothing up.
fn turns_right_to_left(c: char) -> bool {
    matches!(
        u32::from(c),
        0x0590..=0x08FF
            | 0x200F
            | 0x202B
            | 0x202E
            | 0x2067
            | 0xFB1D..=0xFDFF
            | 0xFE70..=0xFEFF
            | 0x1_0800..=0x1_0FFF
            | 0x1_E800..=0x1_EFFF
    ) && matches!(
        bidi_class(c),
        BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI
    )
}

/// Whether `c`, among what a row ends with, takes its paragraph's level
/// there (UAX #9's L1): spaces, isolates and what X9 removes.
fn rests_at_row_end(c: char) -> bool {
    matches!(
        bidi_class(c),
        BidiClass::WS
            | BidiClass::FSI
            | BidiClass::LRI
            | BidiClass::RLI
            | BidiClass::PDI
            | BidiClass::BN
            | BidiClass::LRE
            | BidiClass::RLE
            | BidiClass::LRO
            | BidiClass::RLO
            | BidiClass::PDF
    )
}

impl LineLayout {
    /// Put the glyphs in the order they are drawn, left to right, as the
    /// rest of gpui walks them. A platform may hand a run read right to left
    /// from its right end (cosmic-text does), or the runs in their text's
    /// order (Core Text's lines are free to): each run is turned left to
    /// right, then the runs are set side by side as they are drawn, so a
    /// mark keeps its place by its letter. A line already in order is left
    /// as it is.
    pub(crate) fn put_in_drawn_order(&mut self) {
        let in_order = |runs: &[ShapedRun]| {
            runs.iter()
                .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.position.x))
                .is_sorted()
        };
        if in_order(&self.runs) {
            return;
        }
        for run in &mut self.runs {
            if in_order(std::slice::from_ref(run)) {
                continue;
            }
            let from_the_right = run
                .glyphs
                .iter()
                .map(|glyph| glyph.position.x)
                .is_sorted_by(|a, b| a >= b);
            if from_the_right {
                run.glyphs.reverse();
            } else {
                run.glyphs.sort_by_key(|glyph| glyph.position.x);
            }
        }
        self.runs
            .sort_by_key(|run| run.glyphs.first().map(|glyph| glyph.position.x));
    }

    /// This line, whose glyphs follow its text one after another, laid out
    /// as it reads on one row (a platform that lays out no right-to-left
    /// text: the one tests run on).
    pub(crate) fn laid_out_as_read(self, levels: &BidiLevels, text: &str) -> LineLayout {
        let cells = self.cells_in_text_order(levels);
        self.rows_as_read(levels, text, &cells, &[]).0
    }

    /// Whether any of the line reads right to left.
    pub fn has_right_to_left(&self) -> bool {
        self.bidi_levels.is_some()
    }

    /// Whether the line reads right to left: its first paragraph's
    /// direction, which places its start at its right end.
    pub fn is_right_to_left(&self) -> bool {
        self.bidi_levels
            .as_ref()
            .is_some_and(|levels| levels.is_right_to_left())
    }

    /// The whole line as one row.
    pub(crate) fn whole_row(&self) -> Row {
        Row {
            from: WrapBoundary {
                run_ix: 0,
                glyph_ix: 0,
            },
            to: None,
            right: self.width,
            end: self.len,
        }
    }

    /// The line's cells in its text's order, each as wide as it is drawn:
    /// what a wrap measures.
    pub(crate) fn cells_in_text_order(&self, levels: &BidiLevels) -> SmallVec<[Cell; 32]> {
        let mut cells = cells(self, levels, &self.whole_row());
        cells.sort_by_key(|cell| cell.index);
        cells
    }

    /// This line wrapped before the cells `starts` names (each the first of
    /// a row but the first, in `cells`, which are in its text's order): the
    /// rows one after another along x, each laid out as it reads, its
    /// trailing spaces at their paragraph's level (UAX #9's L1 and L2 on the
    /// row), and where each row after the first starts.
    pub(crate) fn rows_as_read(
        &self,
        levels: &BidiLevels,
        text: &str,
        cells: &[Cell],
        starts: &[usize],
    ) -> (LineLayout, SmallVec<[WrapBoundary; 1]>) {
        let mut row_levels = levels.clone();
        let mut runs: Vec<ShapedRun> = Vec::new();
        let mut boundaries = SmallVec::new();
        let mut x = px(0.);
        let ends = starts.iter().copied().chain([cells.len()]);
        let mut row_start = 0;
        for row_end in ends {
            let row = &cells[row_start..row_end];
            let mut row_cell_levels: SmallVec<[u8; 32]> =
                row.iter().map(|cell| cell.level).collect();
            for (cell, level) in row.iter().zip(row_cell_levels.iter_mut()).rev() {
                let Some(c) = text.get(cell.index..).and_then(|rest| rest.chars().next()) else {
                    break;
                };
                if !rests_at_row_end(c) {
                    break;
                }
                *level = levels.paragraph_level(cell.index);
                for byte in cell.index..cell.index + c.len_utf8() {
                    if let Some(at) = row_levels.levels.get_mut(byte) {
                        *at = *level;
                    }
                }
            }
            if row_start > 0 {
                boundaries.push(WrapBoundary {
                    run_ix: runs.len(),
                    glyph_ix: 0,
                });
            }
            let mut new_row = row_start > 0;
            for k in drawn_order(&row_cell_levels) {
                let cell = &row[k];
                for (run, glyph) in cell.glyphs(self) {
                    let mut glyph = glyph.clone();
                    glyph.position.x = x + (glyph.position.x - cell.left);
                    push_glyph(&mut runs, run.font_id, run.position, glyph, new_row);
                    new_row = false;
                }
                x += cell.width();
            }
            row_start = row_end;
        }
        let layout = LineLayout {
            font_size: self.font_size,
            width: x,
            ascent: self.ascent,
            descent: self.descent,
            runs,
            len: self.len,
            bidi_levels: Some(Arc::new(row_levels)),
        };
        (layout, boundaries)
    }
}

/// Add `glyph` to the last of `runs` where it is of its font and position
/// (and a new run is not asked for), or as a run of its own.
fn push_glyph(
    runs: &mut Vec<ShapedRun>,
    font_id: FontId,
    position: RunPosition,
    glyph: ShapedGlyph,
    new_run: bool,
) {
    match runs.last_mut() {
        Some(run) if !new_run && run.font_id == font_id && run.position == position => {
            run.glyphs.push(glyph)
        }
        _ => runs.push(ShapedRun {
            font_id,
            glyphs: vec![glyph],
            position,
        }),
    }
}

/// The order cells at `levels` are drawn in, left to right (UAX #9's L2):
/// from the highest level down to the lowest odd one, each stretch at that
/// level or higher reversed.
fn drawn_order(levels: &[u8]) -> SmallVec<[usize; 32]> {
    let mut order: SmallVec<[usize; 32]> = (0..levels.len()).collect();
    let (Some(&highest), Some(&lowest)) = (levels.iter().max(), levels.iter().min()) else {
        return order;
    };
    let lowest_odd = lowest | 1;
    let mut level = highest;
    while level >= lowest_odd {
        let mut i = 0;
        while i < order.len() {
            if levels[order[i]] < level {
                i += 1;
                continue;
            }
            let start = i;
            while i < order.len() && levels[order[i]] >= level {
                i += 1;
            }
            order[start..i].reverse();
        }
        level -= 1;
    }
    order
}

/// A stretch of a line's glyphs, from one place in its runs to another,
/// drawn from where its first glyph is to `right`, and the bytes of the
/// text they draw: one row of a wrapped line, or the whole line.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Row {
    pub from: WrapBoundary,
    pub to: Option<WrapBoundary>,
    pub right: Pixels,
    pub end: usize,
}

/// The glyphs drawn together for one character or more (a letter and its
/// marks, a ligature): those side by side with the same index.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cell {
    /// The byte its text starts at.
    pub index: usize,
    pub left: Pixels,
    /// Where the next cell starts, or the row ends.
    pub right: Pixels,
    pub level: u8,
    /// Its first glyph, and how many it has (in more than one run where a
    /// fallback font draws part of it).
    pub first: WrapBoundary,
    pub count: usize,
}

impl Cell {
    fn is_right_to_left(&self) -> bool {
        self.level % 2 == 1
    }

    /// Where a caret stands before its text: at its left edge, or its
    /// right one where it reads right to left.
    fn leading(&self) -> Pixels {
        if self.is_right_to_left() {
            self.right
        } else {
            self.left
        }
    }

    /// Where a caret stands after its text.
    fn trailing(&self) -> Pixels {
        if self.is_right_to_left() {
            self.left
        } else {
            self.right
        }
    }

    pub(crate) fn width(&self) -> Pixels {
        self.right - self.left
    }

    /// Its glyphs, with the run each is in.
    fn glyphs<'a>(
        &self,
        layout: &'a LineLayout,
    ) -> impl Iterator<Item = (&'a ShapedRun, &'a ShapedGlyph)> + 'a {
        let first = self.first;
        layout
            .runs
            .iter()
            .enumerate()
            .skip(first.run_ix)
            .flat_map(move |(run_ix, run)| {
                let skip = if run_ix == first.run_ix {
                    first.glyph_ix
                } else {
                    0
                };
                run.glyphs.iter().skip(skip).map(move |glyph| (run, glyph))
            })
            .take(self.count)
    }
}

/// `row`'s cells, left to right.
fn cells(layout: &LineLayout, levels: &BidiLevels, row: &Row) -> SmallVec<[Cell; 32]> {
    let mut cells: SmallVec<[Cell; 32]> = SmallVec::new();
    'runs: for (run_ix, run) in layout.runs.iter().enumerate().skip(row.from.run_ix) {
        let skip = if run_ix == row.from.run_ix {
            row.from.glyph_ix
        } else {
            0
        };
        for (glyph_ix, glyph) in run.glyphs.iter().enumerate().skip(skip) {
            let here = WrapBoundary { run_ix, glyph_ix };
            if row.to == Some(here) {
                break 'runs;
            }
            match cells.last_mut() {
                Some(cell) if cell.index == glyph.index => cell.count += 1,
                _ => cells.push(Cell {
                    index: glyph.index,
                    left: glyph.position.x,
                    right: glyph.position.x,
                    level: levels.level(glyph.index),
                    first: here,
                    count: 1,
                }),
            }
        }
    }
    for i in 0..cells.len() {
        let right = cells.get(i + 1).map_or(row.right, |next| next.left);
        cells[i].right = right.max(cells[i].left);
    }
    cells
}

/// Where a caret stands at a place in a row's text.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Caret {
    index: usize,
    x: Pixels,
}

/// A row of a line holding right-to-left text, as places in its text and
/// where they are drawn.
pub(crate) struct BidiRow {
    /// Its cells, left to right.
    cells: SmallVec<[Cell; 32]>,
    /// Where a caret stands at each place, in the text's order: each
    /// cell's start, then the row's end.
    carets: SmallVec<[Caret; 32]>,
    left: Pixels,
    right: Pixels,
}

impl BidiRow {
    pub(crate) fn new(layout: &LineLayout, levels: &BidiLevels, row: &Row) -> Self {
        let left = layout
            .runs
            .get(row.from.run_ix)
            .and_then(|run| run.glyphs.get(row.from.glyph_ix))
            .map_or(px(0.), |glyph| glyph.position.x)
            .min(row.right);
        let cells = cells(layout, levels, row);
        let carets = carets(&cells, row.end, left);
        Self {
            cells,
            carets,
            left,
            right: row.right,
        }
    }

    /// Where the caret stands at byte `index`. A place inside a cell (in a
    /// ligature) stands where the cell's text ends, as it always has.
    pub(crate) fn x_for_index(&self, index: usize) -> Pixels {
        let after = self.carets.partition_point(|caret| caret.index < index);
        self.carets
            .get(after)
            .or(self.carets.last())
            .map_or(self.left, |caret| caret.x)
    }

    /// The place whose caret stands nearest `x`.
    pub(crate) fn closest_index_for_x(&self, x: Pixels) -> usize {
        self.carets
            .iter()
            .min_by_key(|caret| (caret.x - x).abs())
            .map_or(0, |caret| caret.index)
    }

    /// The character drawn at `x`; left of the row, its leftmost one; past
    /// its right end, none.
    pub(crate) fn index_for_x(&self, x: Pixels) -> Option<usize> {
        if x >= self.right {
            return None;
        }
        let after = self.cells.partition_point(|cell| cell.left <= x);
        self.cells
            .get(after.saturating_sub(1))
            .map(|cell| cell.index)
            .or(self.carets.first().map(|caret| caret.index))
    }

    /// Where the row ends on the right.
    pub(crate) fn right(&self) -> Pixels {
        self.right
    }

    /// The places whose carets stand at the row's left and right ends.
    pub(crate) fn ends(&self) -> (usize, usize) {
        let at = |x: Pixels| self.closest_index_for_x(x);
        (at(self.left), at(self.right))
    }

    /// The spans of x a selection of `range` covers, left to right: one
    /// for each stretch of its characters drawn side by side.
    pub(crate) fn selection(&self, range: &Range<usize>) -> SmallVec<[Range<Pixels>; 2]> {
        let mut spans: SmallVec<[Range<Pixels>; 2]> = SmallVec::new();
        for cell in &self.cells {
            if !range.contains(&cell.index) {
                continue;
            }
            match spans.last_mut() {
                Some(span) if span.end >= cell.left => span.end = cell.right,
                _ => spans.push(cell.left..cell.right),
            }
        }
        spans
    }

    /// The place whose caret stands next to `index`'s, on its left or its
    /// right: the nearest edge that way, and of the places there, the one
    /// nearest in the text. `None` at that end of the row.
    pub(crate) fn index_beside(&self, index: usize, toward_left: bool) -> Option<usize> {
        let x = self.x_for_index(index);
        self.carets
            .iter()
            .filter(|caret| {
                if toward_left {
                    caret.x < x
                } else {
                    caret.x > x
                }
            })
            .min_by_key(|caret| ((caret.x - x).abs(), caret.index.abs_diff(index)))
            .map(|caret| caret.index)
    }
}

/// Where the caret stands at each place of a row whose cells are `cells`
/// (left to right) and whose text ends at `end`, in the text's order. Each
/// place stands at the start of the cell after it, or the end of the one
/// before: where the two differ, at the edge no other place has; where both
/// are free or both taken, at the edge of the cell at the lower level.
fn carets(cells: &[Cell], end: usize, left: Pixels) -> SmallVec<[Caret; 32]> {
    let mut order: SmallVec<[usize; 32]> = (0..cells.len()).collect();
    order.sort_by_key(|&i| cells[i].index);
    let mut carets: SmallVec<[Caret; 32]> = SmallVec::with_capacity(order.len() + 1);
    let mut taken: SmallVec<[Pixels; 32]> = SmallVec::new();
    let mut open: SmallVec<[usize; 4]> = SmallVec::new();
    for k in 0..=order.len() {
        let before = k.checked_sub(1).map(|j| &cells[order[j]]);
        let after = order.get(k).map(|&i| &cells[i]);
        let index = after.map_or(end, |cell| cell.index);
        let x = match (before, after) {
            (Some(before), Some(after)) if before.trailing() != after.leading() => {
                open.push(k);
                after.leading()
            }
            (_, Some(after)) => after.leading(),
            (Some(before), None) => before.trailing(),
            (None, None) => left,
        };
        if open.last() != Some(&k) {
            take(&mut taken, x);
        }
        carets.push(Caret { index, x });
    }
    for k in open {
        let before = &cells[order[k - 1]];
        let after = &cells[order[k]];
        let (trailing, leading) = (before.trailing(), after.leading());
        let free = |x: Pixels| taken.binary_search(&x).is_err();
        let x = match (free(trailing), free(leading)) {
            (true, false) => trailing,
            (false, true) => leading,
            _ if before.level < after.level => trailing,
            _ => leading,
        };
        take(&mut taken, x);
        carets[k].x = x;
    }
    carets
}

fn take(taken: &mut SmallVec<[Pixels; 32]>, x: Pixels) {
    if let Err(at) = taken.binary_search(&x) {
        taken.insert(at, x);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FontId, GlyphId, LineLayout, ShapedRun, WrappedLineLayout, point};

    /// A line of `text` whose characters are each one glyph 10 px wide,
    /// placed as UAX #9 lays them out on one line: what a platform hands.
    pub(crate) fn laid_out(text: &str) -> LineLayout {
        let levels = BidiLevels::of(text);
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let char_levels: Vec<u8> = chars
            .iter()
            .map(|(i, _)| levels.as_ref().map_or(0, |levels| levels.level(*i)))
            .collect();
        let order = drawn_order(&char_levels);
        let glyphs = order
            .iter()
            .enumerate()
            .map(|(slot, &k)| ShapedGlyph {
                id: GlyphId(k as u32),
                position: point(px(10. * slot as f32), px(0.)),
                index: chars[k].0,
                is_emoji: false,
            })
            .collect();
        LineLayout {
            font_size: px(16.),
            width: px(10. * chars.len() as f32),
            ascent: px(12.),
            descent: px(4.),
            runs: vec![ShapedRun {
                font_id: FontId(0),
                glyphs,
                position: Default::default(),
            }],
            len: text.len(),
            bidi_levels: levels.map(Arc::new),
        }
    }

    /// The text drawn left to right, as the glyphs stand.
    fn drawn(layout: &LineLayout, text: &str) -> String {
        layout
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter())
            .map(|glyph| text[glyph.index..].chars().next().unwrap())
            .collect()
    }

    /// Where the caret stands at each place in `text`, by characters.
    fn carets_in(layout: &LineLayout, text: &str) -> Vec<f32> {
        text.char_indices()
            .map(|(i, _)| i)
            .chain([text.len()])
            .map(|i| f32::from(layout.x_for_index(i)))
            .collect()
    }

    const HEBREW: &str = "\u{5d0}\u{5d1}\u{5d2}";

    #[test]
    fn levels_are_kept_only_where_text_reads_right_to_left() {
        assert_eq!(BidiLevels::of("plain text"), None);
        assert_eq!(BidiLevels::of("Données, Ελληνικά, Кириллица"), None);
        // An Arabic number alone reads left to right.
        assert_eq!(BidiLevels::of("\u{661}\u{662}\u{663}"), None);
        let levels = BidiLevels::of(HEBREW).unwrap();
        assert!(levels.is_right_to_left());
        assert_eq!(levels.level(0), 1);
        let mixed = BidiLevels::of("abc \u{5d0}\u{5d1}").unwrap();
        assert!(!mixed.is_right_to_left());
        assert_eq!((mixed.level(0), mixed.level(3), mixed.level(4)), (0, 0, 1));
        // An override, with nothing right to left in its text, still turns it.
        let overridden = BidiLevels::of("Hello_\u{202e}gnp.js").unwrap();
        assert_eq!(overridden.level(10), 1);
    }

    #[test]
    fn every_right_to_left_character_is_in_the_blocks_asked_first() {
        for c in (0..=0x10_FFFF).filter_map(char::from_u32) {
            let right_to_left = matches!(
                bidi_class(c),
                BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI
            );
            assert_eq!(
                turns_right_to_left(c),
                right_to_left,
                "U+{:04X}",
                u32::from(c)
            );
        }
    }

    #[test]
    fn a_line_handed_from_its_right_end_is_put_in_drawn_order() {
        let mut layout = laid_out(HEBREW);
        for run in &mut layout.runs {
            run.glyphs.reverse();
        }
        layout.put_in_drawn_order();
        assert_eq!(drawn(&layout, HEBREW), "\u{5d2}\u{5d1}\u{5d0}");
        let xs: Vec<f32> = layout.runs[0]
            .glyphs
            .iter()
            .map(|g| f32::from(g.position.x))
            .collect();
        assert_eq!(xs, [0., 10., 20.]);
    }

    #[test]
    fn hebrew_carets_go_from_right_to_left() {
        let layout = laid_out(HEBREW);
        assert_eq!(carets_in(&layout, HEBREW), [30., 20., 10., 0.]);
        // Each edge answers its place, and the characters under the pointer.
        for (i, x) in [(0, 29.), (2, 21.), (2, 19.), (4, 11.), (6, 1.)] {
            assert_eq!(layout.closest_index_for_x(px(x)), i, "at {x}");
        }
        assert_eq!(layout.index_for_x(px(25.)), Some(0));
        assert_eq!(layout.index_for_x(px(5.)), Some(4));
        assert_eq!(layout.index_for_x(px(30.)), None);
        assert!(layout.is_right_to_left());
    }

    #[test]
    fn a_hebrew_part_in_a_latin_name_is_reached_at_both_its_ends() {
        // "abc אבג": drawn "abc גבא"; the place before א stands at the far
        // right, the end after "abc ", so every edge is a place's.
        let text = "abc \u{5d0}\u{5d1}\u{5d2}";
        let layout = laid_out(text);
        assert_eq!(drawn(&layout, text), "abc \u{5d2}\u{5d1}\u{5d0}");
        assert_eq!(
            carets_in(&layout, text),
            [0., 10., 20., 30., 70., 60., 50., 40.]
        );
        let mut reached: Vec<f32> = carets_in(&layout, text);
        reached.sort_by(f32::total_cmp);
        reached.dedup();
        assert_eq!(reached.len(), 8, "every edge reached once");
        // Round trip: each place's caret answers that place.
        for i in [0, 1, 2, 3, 4, 6, 8, 10] {
            assert_eq!(layout.closest_index_for_x(layout.x_for_index(i)), i);
        }
        assert!(!layout.is_right_to_left());
    }

    #[test]
    fn a_hebrew_word_between_latin_ones_turns_at_both_its_sides() {
        // "abc אבג def": drawn "abc גבא def".
        let text = "abc \u{5d0}\u{5d1}\u{5d2} def";
        let layout = laid_out(text);
        assert_eq!(drawn(&layout, text), "abc \u{5d2}\u{5d1}\u{5d0} def");
        let carets = carets_in(&layout, text);
        // Before א: after "abc ", the paragraph's direction; after ג: the
        // Hebrew word's right end, where no other place stands.
        assert_eq!(carets[4], 40.);
        assert_eq!(carets[7], 70.);
        let mut reached = carets.clone();
        reached.sort_by(f32::total_cmp);
        reached.dedup();
        assert_eq!(reached.len(), carets.len());
    }

    #[test]
    fn digits_in_arabic_read_left_to_right_inside_it() {
        // "مرحبا 123": an Arabic word then a number, the paragraph right to
        // left: drawn "123 ابحرم".
        let text = "\u{645}\u{631}\u{62d}\u{628}\u{627} 123";
        let layout = laid_out(text);
        assert_eq!(
            drawn(&layout, text),
            "123 \u{627}\u{628}\u{62d}\u{631}\u{645}"
        );
        let carets = carets_in(&layout, text);
        // The start at the right end; "1" starts at the number's left end,
        // and the text ends at its right end.
        assert_eq!(carets[0], 90.);
        assert_eq!(carets[6], 0.);
        assert_eq!(carets[9], 30.);
        let mut reached = carets.clone();
        reached.sort_by(f32::total_cmp);
        reached.dedup();
        assert_eq!(reached.len(), carets.len());
        // Arrows walk the edges as drawn.
        let index_of = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(i, _)| i);
        assert_eq!(layout.index_left_of(index_of(0)), Some(index_of(1)));
        assert_eq!(layout.index_right_of(index_of(0)), None);
        assert_eq!(layout.index_left_of(index_of(6)), None);
    }

    #[test]
    fn a_selection_covers_its_characters_where_they_are_drawn() {
        let text = "abc \u{5d0}\u{5d1}\u{5d2} def";
        let layout = laid_out(text);
        let spans = |range: Range<usize>| -> Vec<(f32, f32)> {
            layout
                .selection_ranges(range)
                .into_iter()
                .map(|span| (f32::from(span.start), f32::from(span.end)))
                .collect()
        };
        // "c א": "c " on the left, "א" at the Hebrew word's right end.
        assert_eq!(spans(2..6), [(20., 40.), (60., 70.)]);
        // The Hebrew word and the space after it: drawn side by side.
        assert_eq!(spans(4..11), [(40., 80.)]);
        assert_eq!(spans(0..text.len()), [(0., 110.)]);
        // A line all left to right: one span.
        let latin = laid_out("abc def");
        assert_eq!(latin.selection_ranges(1..5).len(), 1);
    }

    /// `text` laid out as `laid_out` does, wrapped at `width` as the layout
    /// cache wraps it.
    fn wrapped(text: &str, width: f32) -> WrappedLineLayout {
        let layout = Arc::new(laid_out(text));
        let (unwrapped_layout, wrap_boundaries) = layout.wrapped(text, px(width), None);
        WrappedLineLayout {
            unwrapped_layout,
            wrap_boundaries,
            wrap_width: Some(px(width)),
            min_content: Default::default(),
        }
    }

    /// Where the caret at byte `index` of `line` is drawn, rows 20 px high.
    fn at(line: &WrappedLineLayout, index: usize) -> Option<(f32, f32)> {
        line.position_for_index(index, px(20.))
            .map(|at| (f32::from(at.x), f32::from(at.y)))
    }

    #[test]
    fn a_hebrew_paragraph_wraps_from_its_first_row() {
        // "אבג דהו זחט", ten pixels a character, wrapped at 45: a row a
        // word, the first word on the first row, each row drawn from its
        // right end.
        let text = "\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4}\u{5d5} \u{5d6}\u{5d7}\u{5d8}";
        let line = wrapped(text, 45.);
        assert_eq!(line.row_starts().as_slice(), [7, 14]);
        assert_eq!(
            drawn(&line.unwrapped_layout, text),
            " \u{5d2}\u{5d1}\u{5d0} \u{5d5}\u{5d4}\u{5d3}\u{5d8}\u{5d7}\u{5d6}"
        );
        // The start at the first row's right end, a row's end at its left.
        assert_eq!(at(&line, 0), Some((40., 0.)));
        assert_eq!(at(&line, 7), Some((0., 0.)));
        assert_eq!(at(&line, 9), Some((30., 20.)));
        assert_eq!(at(&line, text.len()), Some((0., 40.)));
        let row = |x: f32, y: f32| point(px(x), px(y));
        assert_eq!(
            line.closest_index_for_position(row(33., 5.), px(20.)),
            Ok(2)
        );
        assert_eq!(line.index_for_position(row(25., 25.), px(20.)), Ok(9));
        assert_eq!(line.index_for_position(row(-1., 5.), px(20.)), Err(7));
        // The rows' text, in order, as a text element gives it.
        let rows: Vec<&str> = [0]
            .into_iter()
            .chain(line.row_starts())
            .zip(line.row_starts().into_iter().chain([text.len()]))
            .map(|(start, end)| &text[start..end])
            .collect();
        assert_eq!(
            rows,
            [
                "\u{5d0}\u{5d1}\u{5d2} ",
                "\u{5d3}\u{5d4}\u{5d5} ",
                "\u{5d6}\u{5d7}\u{5d8}"
            ]
        );
        // Its narrowest width without cutting a word: a Hebrew word is one.
        assert_eq!(line.unwrapped_layout.min_wrap_width(text), px(40.));
    }

    #[test]
    fn each_row_of_a_mixed_line_reads_on_its_own() {
        // "abc אבג דהו xyz" on one line reads "abc והד גבא xyz"; wrapped
        // after "אבג ", each row reads as it holds: "abc גבא " then
        // "והד xyz", the space ending the first row at its right end.
        let text = "abc \u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4}\u{5d5} xyz";
        assert_eq!(
            drawn(&laid_out(text), text),
            "abc \u{5d5}\u{5d4}\u{5d3} \u{5d2}\u{5d1}\u{5d0} xyz"
        );
        let line = wrapped(text, 85.);
        assert_eq!(line.row_starts().as_slice(), [11]);
        assert_eq!(
            drawn(&line.unwrapped_layout, text),
            "abc \u{5d2}\u{5d1}\u{5d0} \u{5d5}\u{5d4}\u{5d3} xyz"
        );
        assert_eq!(at(&line, 4), Some((40., 0.)));
        assert_eq!(at(&line, 10), Some((70., 0.)));
        assert_eq!(at(&line, 11), Some((80., 0.)));
        assert_eq!(at(&line, 13), Some((20., 20.)));
        assert_eq!(at(&line, 17), Some((0., 20.)));
        assert_eq!(at(&line, text.len()), Some((70., 20.)));
    }

    #[test]
    fn a_line_all_left_to_right_wraps_as_it_did() {
        let text = "abc def ghi";
        let layout = Arc::new(laid_out(text));
        assert!(layout.bidi_levels.is_none());
        let (drawn_from, boundaries) = layout.wrapped(text, px(45.), None);
        assert!(Arc::ptr_eq(&drawn_from, &layout));
        assert_eq!(
            boundaries,
            layout.compute_wrap_boundaries(text, px(45.), None)
        );
        // A line holding Hebrew that fits keeps its layout too.
        let hebrew = Arc::new(laid_out(HEBREW));
        let (drawn_from, boundaries) = hebrew.wrapped(HEBREW, px(100.), None);
        assert!(Arc::ptr_eq(&drawn_from, &hebrew) && boundaries.is_empty());
    }

    #[test]
    fn runs_handed_out_of_order_are_sorted_where_they_are_drawn() {
        // "abc אבג" with its runs in the text's order, the Hebrew one drawn
        // right of the Latin one but handed first.
        let text = "abc \u{5d0}\u{5d1}\u{5d2}";
        let mut layout = laid_out(text);
        let glyphs = std::mem::take(&mut layout.runs[0].glyphs);
        let (latin, hebrew) = glyphs.split_at(4);
        layout.runs = vec![
            ShapedRun {
                font_id: FontId(1),
                glyphs: hebrew.to_vec(),
                position: Default::default(),
            },
            ShapedRun {
                font_id: FontId(0),
                glyphs: latin.to_vec(),
                position: Default::default(),
            },
        ];
        layout.put_in_drawn_order();
        assert_eq!(drawn(&layout, text), "abc \u{5d2}\u{5d1}\u{5d0}");
        let fonts: Vec<FontId> = layout.runs.iter().map(|run| run.font_id).collect();
        assert_eq!(fonts, [FontId(0), FontId(1)]);
    }

    #[test]
    fn levels_cut_with_their_line_keep_their_paragraphs() {
        let levels = BidiLevels::of("abc \u{5d0}\u{5d1}").unwrap();
        let cut = levels.slice(4..8);
        assert_eq!((cut.level(0), cut.level(2)), (1, 1));
        assert_eq!(cut.paragraph_level(0), 0);
        let two = BidiLevels::of("\u{5d0}\u{1c}abc").unwrap();
        assert_eq!((two.paragraph_level(0), two.paragraph_level(3)), (1, 0));
        assert_eq!(two.slice(3..6).paragraph_level(0), 0);
    }

    #[test]
    fn a_row_is_drawn_in_the_order_it_reads() {
        let order = drawn_order(&[0, 0, 1, 1, 1, 0]);
        assert_eq!(order.as_slice(), [0, 1, 4, 3, 2, 5]);
        let order = drawn_order(&[1, 1, 2, 2, 1]);
        assert_eq!(order.as_slice(), [4, 2, 3, 1, 0]);
        assert!(drawn_order(&[]).is_empty());
    }
}
