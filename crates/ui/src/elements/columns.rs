//! Blocks of rows (a heading, then its rows) flowing down columns, as a
//! newspaper's: as many columns as fit the width, as short as they can
//! be with every block whole. Only a block taller than a column carries on
//! at the top of the next, under its heading again, each part keeping a
//! few rows. A row is one line where it fits its column, else two (its
//! title, then what follows it under it): each is measured once on one
//! line, when the layout asks, and the split counts lines from there, for
//! the width the layout gives (the element's height) and the one `prepaint`
//! gets alike.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window, point, px, size,
};

/// The fewest rows a part of a split block keeps.
const PIECE: usize = 4;

/// Sizes, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// The narrowest a column may be.
    pub column: f32,
    /// Between columns.
    pub gutter: f32,
    /// A heading's height, with what parts it from its rows.
    pub heading: f32,
    /// A line of a row.
    pub line: f32,
    /// Between rows.
    pub row_gap: f32,
    /// Above a block that follows another in a column.
    pub block_gap: f32,
}

impl Metrics {
    fn row(&self, lines: usize) -> f32 {
        let lines = lines.max(1) as f32;
        self.line * lines + self.row_gap * (lines - 1.)
    }
}

/// A row, drawn on one line where that fits the column, else on two.
pub struct Row {
    pub inline: AnyElement,
    pub stacked: AnyElement,
}

/// A heading and its rows.
pub struct Block {
    pub heading: String,
    pub rows: Vec<Row>,
}

/// Draws a heading: the block's, or `continued` where it carries on.
pub type Heading = Rc<dyn Fn(&str, bool) -> AnyElement>;

pub struct Columns {
    pub blocks: Vec<Block>,
    pub metrics: Metrics,
    pub heading: Heading,
}

impl IntoElement for Columns {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// A block's part in a column: which block, which of its rows, and
/// whether it carries on from the column before.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Piece {
    pub block: usize,
    pub rows: std::ops::Range<usize>,
    pub continued: bool,
}

/// How many columns fit `width`.
fn fitting(width: f32, metrics: &Metrics) -> usize {
    let count = ((width + metrics.gutter) / (metrics.column + metrics.gutter)).floor();
    if count.is_finite() && count >= 1. {
        count as usize
    } else {
        1
    }
}

/// `blocks`' rows (their lines) in at most `columns` columns, each as
/// short as the split allows: the columns, and the tallest one's height.
pub(crate) fn split(
    blocks: &[Vec<usize>],
    columns: usize,
    metrics: &Metrics,
) -> (Vec<Vec<Piece>>, f32) {
    let rows = |block: &[usize], range: std::ops::Range<usize>| -> f32 {
        let lines = &block[range];
        lines.iter().map(|&n| metrics.row(n)).sum::<f32>()
            + metrics.row_gap * lines.len().saturating_sub(1) as f32
    };
    let total: f32 = blocks
        .iter()
        .map(|block| metrics.heading + rows(block, 0..block.len()) + metrics.block_gap)
        .sum();
    let step = metrics.line + metrics.row_gap;
    let mut height = (total / columns.max(1) as f32).max(step);
    loop {
        let filled = fill(blocks, height, metrics, &rows);
        if filled.0.len() <= columns.max(1) || height >= total {
            return filled;
        }
        height += step;
    }
}

/// `blocks` in columns no taller than `height`, where one row fits.
fn fill(
    blocks: &[Vec<usize>],
    height: f32,
    metrics: &Metrics,
    rows: &dyn Fn(&[usize], std::ops::Range<usize>) -> f32,
) -> (Vec<Vec<Piece>>, f32) {
    let mut columns: Vec<Vec<Piece>> = vec![Vec::new()];
    let mut used: f32 = 0.;
    let mut tallest: f32 = 0.;
    for (index, block) in blocks.iter().enumerate() {
        let mut start = 0;
        let mut continued = false;
        while start < block.len() {
            let gap = if used > 0. { metrics.block_gap } else { 0. };
            let rest = rows(block, start..block.len());
            if used + gap + metrics.heading + rest <= height || (used == 0. && rest <= height) {
                columns.last_mut().expect("a column").push(Piece {
                    block: index,
                    rows: start..block.len(),
                    continued,
                });
                used += gap + metrics.heading + rest;
                break;
            }
            // The rows that fit under the heading here.
            let fit = (start..=block.len())
                .take_while(|&end| used + gap + metrics.heading + rows(block, start..end) <= height)
                .last()
                .map_or(0, |end| end - start);
            let remaining = block.len() - start;
            // Only a block no column holds whole is split.
            let taller = metrics.heading + rest > height;
            let take = if taller && fit >= PIECE && remaining - fit >= PIECE {
                Some(fit)
            } else if used == 0. {
                // An empty column takes it, whole where it is short, else a
                // part that leaves a few rows for the next.
                Some(if remaining < 2 * PIECE {
                    remaining
                } else {
                    fit.clamp(PIECE, remaining - PIECE)
                })
            } else {
                None
            };
            if let Some(take) = take {
                columns.last_mut().expect("a column").push(Piece {
                    block: index,
                    rows: start..start + take,
                    continued,
                });
                used += gap + metrics.heading + rows(block, start..start + take);
                start += take;
                continued = true;
                if start == block.len() {
                    break;
                }
            }
            tallest = tallest.max(used);
            columns.push(Vec::new());
            used = 0.;
        }
    }
    columns.retain(|column| !column.is_empty());
    (columns, tallest.max(used))
}

/// What prepaint placed, painted in order: the headings it made, and
/// each row by block and index, stacked or not.
pub struct Placed {
    headings: Vec<AnyElement>,
    rows: Vec<(usize, usize, bool)>,
}

/// The rows' widths on one line, per block.
pub struct Widths(Rc<Vec<Vec<f32>>>);

/// Each row's lines in a column `column` wide.
fn lines_in(widths: &[Vec<f32>], column: f32) -> Vec<Vec<usize>> {
    widths
        .iter()
        .map(|block| {
            block
                .iter()
                .map(|&width| if width <= column { 1 } else { 2 })
                .collect()
        })
        .collect()
}

/// How many columns `width` holds, and how wide each is.
fn layout(width: f32, metrics: &Metrics) -> (usize, f32) {
    let count = fitting(width, metrics);
    let column = (width - metrics.gutter * (count as f32 - 1.)) / count as f32;
    (count, column)
}

impl Element for Columns {
    type RequestLayoutState = Widths;
    type PrepaintState = Placed;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Widths) {
        let metrics = self.metrics;
        let widths: Rc<Vec<Vec<f32>>> = Rc::new(
            self.blocks
                .iter_mut()
                .map(|block| {
                    block
                        .rows
                        .iter_mut()
                        .map(|row| {
                            let natural = row.inline.layout_as_root(
                                size(
                                    AvailableSpace::MaxContent,
                                    AvailableSpace::Definite(px(metrics.line)),
                                ),
                                window,
                                cx,
                            );
                            f32::from(natural.width)
                        })
                        .collect()
                })
                .collect(),
        );
        let measured = Rc::clone(&widths);
        let style = Style {
            flex_shrink: 0.,
            ..Style::default()
        };
        let layout = window.request_measured_layout(style, move |known, available, _, _| {
            let width = known.width.map_or(
                match available.width {
                    AvailableSpace::Definite(room) => f32::from(room),
                    _ => metrics.column,
                },
                f32::from,
            );
            let (count, column) = layout(width, &metrics);
            let (_, height) = split(&lines_in(&measured, column), count, &metrics);
            size(px(width), px(height))
        });
        (layout, Widths(widths))
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        widths: &mut Widths,
        window: &mut Window,
        cx: &mut App,
    ) -> Placed {
        let metrics = self.metrics;
        // Columns as wide as `count` makes them, however many are used.
        let (count, column) = layout(f32::from(bounds.size.width), &metrics);
        let lines = lines_in(&widths.0, column);
        let (columns, _) = split(&lines, count, &metrics);
        let mut placed = Placed {
            headings: Vec::new(),
            rows: Vec::new(),
        };
        for (at, pieces) in columns.iter().enumerate() {
            let x = bounds.origin.x + px((column + metrics.gutter) * at as f32);
            let mut y = 0.;
            for piece in pieces {
                if y > 0. {
                    y += metrics.block_gap;
                }
                let block = &mut self.blocks[piece.block];
                let mut heading = (self.heading)(&block.heading, piece.continued);
                heading.layout_as_root(
                    size(
                        AvailableSpace::Definite(px(column)),
                        AvailableSpace::Definite(px(metrics.heading)),
                    ),
                    window,
                    cx,
                );
                heading.prepaint_at(point(x, bounds.origin.y + px(y)), window, cx);
                placed.headings.push(heading);
                y += metrics.heading;
                for (n, index) in piece.rows.clone().enumerate() {
                    if n > 0 {
                        y += metrics.row_gap;
                    }
                    let stacked = lines[piece.block][index] > 1;
                    let row = &mut block.rows[index];
                    let row = if stacked {
                        &mut row.stacked
                    } else {
                        &mut row.inline
                    };
                    let height = metrics.row(lines[piece.block][index]);
                    row.layout_as_root(
                        size(
                            AvailableSpace::Definite(px(column)),
                            AvailableSpace::Definite(px(height)),
                        ),
                        window,
                        cx,
                    );
                    row.prepaint_at(point(x, bounds.origin.y + px(y)), window, cx);
                    placed.rows.push((piece.block, index, stacked));
                    y += height;
                }
            }
        }
        placed
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Widths,
        placed: &mut Placed,
        window: &mut Window,
        cx: &mut App,
    ) {
        for heading in &mut placed.headings {
            heading.paint(window, cx);
        }
        for &(block, row, stacked) in &placed.rows {
            let row = &mut self.blocks[block].rows[row];
            if stacked {
                row.stacked.paint(window, cx);
            } else {
                row.inline.paint(window, cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Metrics, Piece, fitting, split};

    const METRICS: Metrics = Metrics {
        column: 280.,
        gutter: 28.,
        heading: 20.,
        line: 24.,
        row_gap: 3.,
        block_gap: 12.,
    };

    fn heights(columns: &[Vec<Piece>]) -> Vec<usize> {
        columns
            .iter()
            .map(|pieces| pieces.iter().map(|p| p.rows.len()).sum())
            .collect()
    }

    #[test]
    fn as_many_columns_as_fit() {
        assert_eq!(fitting(950., &METRICS), 3);
        assert_eq!(fitting(600., &METRICS), 2);
        assert_eq!(fitting(200., &METRICS), 1);
    }

    #[test]
    fn blocks_flow_down_balanced_columns() {
        // Topics of 8, 7, 7, 2, 9 and 6 rows in three columns.
        let blocks: Vec<Vec<usize>> = [8, 7, 7, 2, 9, 6].iter().map(|&n| vec![1; n]).collect();
        let (columns, _) = split(&blocks, 3, &METRICS);
        assert_eq!(columns.len(), 3);
        let rows = heights(&columns);
        assert!(
            rows.iter().max().unwrap() - rows.iter().min().unwrap() <= 6,
            "{rows:?}"
        );
        // Small blocks stay whole.
        assert!(columns.iter().flatten().all(|piece| !piece.continued));
    }

    #[test]
    fn a_block_taller_than_a_column_carries_on() {
        let blocks = vec![vec![1; 30], vec![1; 3]];
        let (columns, height) = split(&blocks, 3, &METRICS);
        assert_eq!(columns.len(), 3);
        let parts: Vec<&Piece> = columns.iter().flatten().filter(|p| p.block == 0).collect();
        assert!(parts.len() > 1);
        assert!(parts[1..].iter().all(|p| p.continued));
        assert!(
            parts.iter().all(|p| p.rows.len() >= super::PIECE),
            "{parts:?}"
        );
        assert!(height < METRICS.heading + 30. * 27.);
    }

    #[test]
    fn a_short_list_is_never_cut_up() {
        // A filter's few rows: one block, one column, whatever the width.
        let blocks = vec![vec![1, 2, 1]];
        let (columns, _) = split(&blocks, 3, &METRICS);
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0][0].rows, 0..3);
    }
}
