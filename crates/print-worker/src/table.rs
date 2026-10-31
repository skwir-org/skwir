//! A table of data (a CSV, a folder's listing): the columns' names bold
//! above, again on every page; columns as wide as their content (a
//! sample of the rows measured), shrunk with the text to fit the page
//! down to 6 points, the widest then wrapped; a cell of at most three
//! lines, cut with an ellipsis past them; light rules between rows. When
//! even that does not fit, the columns go across pages, the first one on
//! each.

use std::ops::Range;

use anyhow::Result;
use krilla::geom::Rect;
use krilla::tagging::{ArtifactType, TableHeaderScope, Tag, TagKind};
use parley::Language;

use crate::fonts::{Fonts, Role};
use crate::geometry::{self, Columns, Frame};
use crate::out::{PageCx, Part, Structure, key};
use crate::text::{self, RULE, Spec};

pub const SIZE: f32 = 9.0;
const LEAST: f32 = 6.0;
pub const PAD_X: f32 = 3.0;
pub const PAD_Y: f32 = 2.0;
pub const LINE_HEIGHT: f32 = 1.25;
const LINES: usize = 3;
/// A column's least width, against the size.
const NARROWEST: f32 = 5.0;
/// The rows measured for the columns' widths: the first, then some taken
/// evenly from the rest.
const SAMPLE_FIRST: usize = 200;
const SAMPLE_REST: usize = 800;
/// The characters of a cell measured for its column's width.
const MEASURED: usize = 160;
/// The most cells marked one by one for a screen reader; a larger table
/// is marked a page at a time.
const TAGGED_CELLS: usize = 200_000;

pub struct Data<'a> {
    columns: &'a [String],
    rows: &'a [Vec<String>],
    fit: Columns,
    /// The pages: a group of columns and its rows.
    pages: Vec<(usize, Range<usize>)>,
    /// Each group's header height and rows' heights.
    heads: Vec<f32>,
    heights: Vec<Vec<f32>>,
    frame: Frame,
    language: Option<Language>,
    tagged: bool,
    structure: Structure,
}

fn spec(size: f32, bold: bool, width: f32, language: Option<Language>) -> Spec {
    let mut spec = Spec::new(Role::Text, size);
    spec.bold = bold;
    spec.line_height = LINE_HEIGHT;
    spec.width = Some((width - 2.0 * PAD_X).max(size));
    spec.language = language;
    spec
}

/// A cell's text measured, its first characters only.
fn measured(text: &str) -> &str {
    match text.char_indices().nth(MEASURED) {
        Some((at, _)) => &text[..at],
        None => text,
    }
}

/// The rows a sample of `count` measures: the first ones, then evenly.
fn sample(count: usize) -> impl Iterator<Item = usize> {
    let first = count.min(SAMPLE_FIRST);
    let rest = count - first;
    let step = rest.div_ceil(SAMPLE_REST).max(1);
    (0..first).chain((first..count).step_by(step))
}

impl<'a> Data<'a> {
    pub fn plan(
        columns: &'a [String],
        rows: &'a [Vec<String>],
        fonts: &mut Fonts,
        frame: Frame,
        language: Option<Language>,
    ) -> Data<'a> {
        let width = frame.text.width;
        let count = columns
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0));
        let mut natural = vec![0.0f32; count];
        let mut unbounded = Spec::new(Role::Text, SIZE);
        unbounded.wrap = text::Wrap::None;
        unbounded.language = language;
        let mut bold = unbounded.clone();
        bold.bold = true;
        for (column, name) in columns.iter().enumerate() {
            let w = text::plain(fonts, measured(name), &bold).width();
            natural[column] = natural[column].max(w + 2.0 * PAD_X);
        }
        for row in sample(rows.len()) {
            for (column, cell) in rows[row].iter().enumerate() {
                if cell.is_empty() {
                    continue;
                }
                let w = text::plain(fonts, measured(cell), &unbounded).width();
                natural[column] = natural[column].max(w + 2.0 * PAD_X);
            }
        }
        // A column of long cells wraps them rather than shrink the
        // others: it asks at most a share of the page.
        let most = if count > 1 { width * 0.6 } else { width };
        for w in &mut natural {
            *w = w.min(most).max(2.0 * PAD_X + 1.0);
        }
        let fit = geometry::columns(
            &natural,
            width,
            SIZE,
            LEAST,
            NARROWEST * SIZE,
            2.0 * PAD_X,
            true,
        );
        let size = fit.size;
        let line = size * LINE_HEIGHT;
        let widest = text::widest_ascii(fonts, &spec(size, false, width, language));
        let widest_bold = text::widest_ascii(fonts, &spec(size, true, width, language));
        let mut pages = Vec::new();
        let mut heads = Vec::new();
        let mut heights = Vec::new();
        for (group_index, group) in fit.groups.iter().enumerate() {
            let lines = |fonts: &mut Fonts, cell: &str, width: f32, bold: bool| -> usize {
                let advance = if bold { widest_bold } else { widest };
                let printable = cell.bytes().all(|b| (0x20..0x7f).contains(&b));
                if printable && cell.len() as f32 * advance <= width - 2.0 * PAD_X - 0.5 {
                    return 1;
                }
                let spec = spec(size, bold, width, language);
                text::plain(fonts, cell, &spec).len().clamp(1, LINES)
            };
            let head = group
                .iter()
                .map(|&(column, w)| {
                    columns
                        .get(column)
                        .map_or(1, |name| lines(fonts, name, w, true))
                })
                .max()
                .unwrap_or(1);
            let head = if columns.is_empty() {
                0.0
            } else {
                head as f32 * line + 2.0 * PAD_Y
            };
            let group_heights: Vec<f32> = rows
                .iter()
                .map(|row| {
                    let most = group
                        .iter()
                        .map(|&(column, w)| {
                            row.get(column)
                                .filter(|cell| !cell.is_empty())
                                .map_or(1, |cell| lines(fonts, cell, w, false))
                        })
                        .max()
                        .unwrap_or(1);
                    most as f32 * line + 2.0 * PAD_Y
                })
                .collect();
            for range in geometry::stack(&group_heights, frame.text.height, head) {
                pages.push((group_index, range));
            }
            heads.push(head);
            heights.push(group_heights);
        }
        let cells = rows.len() * count;
        Data {
            columns,
            rows,
            fit,
            pages,
            heads,
            heights,
            frame,
            language,
            tagged: cells <= TAGGED_CELLS,
            structure: Structure::default(),
        }
    }

    /// A cell's text in its column's width, cut past its lines, drawn
    /// with its top-left at `(x, y)`.
    #[allow(clippy::too_many_arguments)] // A cell, where and how.
    fn cell(
        &self,
        cx: &mut PageCx,
        text: &str,
        width: f32,
        bold: bool,
        (x, y): (f32, f32),
        marked: bool,
    ) {
        if text.is_empty() {
            return;
        }
        let spec = spec(self.fit.size, bold, width, self.language);
        let (layout, text) = text::fitted(cx.fonts, text, &spec, LINES);
        let origin = (x + PAD_X, y + PAD_Y);
        if marked {
            cx.lines(&layout, &text, &[], 0..layout.len(), origin);
        } else {
            for index in 0..layout.len() {
                text::draw_line(
                    cx.surface,
                    cx.fonts,
                    &layout,
                    &text,
                    index,
                    origin,
                    &mut |_, _| {},
                    &mut |_, _| {},
                );
            }
        }
    }
}

impl Part for Data<'_> {
    fn pages(&self) -> usize {
        self.pages.len()
    }

    fn draw(&mut self, page: usize, cx: &mut PageCx) -> Result<()> {
        let Some((group, range)) = self.pages.get(page).cloned() else {
            return Ok(());
        };
        let columns = self.fit.groups[group].clone();
        let left = self.frame.text.x;
        let right = left + columns.iter().map(|(_, w)| w).sum::<f32>();
        let mut y = self.frame.text.y;
        let table: (u64, TagKind) = (key(1, 0), Tag::Table.into());
        let mut rules = Vec::new();
        // The header: the table's own on its first page, repeated after.
        let head = self.heads[group];
        if head > 0.0 {
            let first = page == 0;
            let names: Vec<(usize, f32, String)> = columns
                .iter()
                .map(|&(column, w)| {
                    (
                        column,
                        w,
                        self.columns.get(column).cloned().unwrap_or_default(),
                    )
                })
                .collect();
            if first && self.tagged {
                let head_group = (key(2, 0), Tag::THead.into());
                let row = (key(3, 0), Tag::TR.into());
                let mut x = left;
                for (column, w, name) in &names {
                    let th: TagKind = Tag::TH(TableHeaderScope::Column).into();
                    self.structure.goto(
                        cx,
                        &[
                            table.clone(),
                            head_group.clone(),
                            row.clone(),
                            (key(4, *column), th),
                        ],
                    );
                    self.cell(cx, name, *w, true, (x, y), true);
                    x += w;
                }
            } else {
                self.structure.goto(cx, std::slice::from_ref(&table));
                let mut x = left;
                for (_, w, name) in &names {
                    cx.surface
                        .start_tagged(krilla::tagging::ContentTag::Artifact(
                            krilla::tagging::Artifact::with_kind(ArtifactType::PaginationOther),
                        ));
                    self.cell(cx, name, *w, true, (x, y), false);
                    cx.surface.end_tagged();
                    x += w;
                }
            }
            y += head;
            rules.push((y, 0.75));
        }
        if self.tagged {
            for row in range {
                let height = self.heights[group][row];
                let mut x = left;
                for &(column, w) in &columns {
                    let cell = self.rows[row].get(column).map_or("", String::as_str);
                    let repeated = group > 0 && column == 0;
                    if repeated {
                        self.structure.goto(cx, std::slice::from_ref(&table));
                        cx.surface
                            .start_tagged(krilla::tagging::ContentTag::Artifact(
                                krilla::tagging::Artifact::with_kind(ArtifactType::PaginationOther),
                            ));
                        self.cell(cx, cell, w, false, (x, y), false);
                        cx.surface.end_tagged();
                    } else {
                        self.structure.goto(
                            cx,
                            &[
                                table.clone(),
                                (key(2, 1 + group), Tag::TBody.into()),
                                (key(3, 1 + row + group * self.rows.len()), Tag::TR.into()),
                                (key(4, column), Tag::TD.into()),
                            ],
                        );
                        self.cell(cx, cell, w, false, (x, y), true);
                    }
                    x += w;
                }
                y += height;
                rules.push((y, 0.4));
            }
        } else {
            self.structure
                .goto(cx, &[table.clone(), (key(5, page), Tag::P.into())]);
            let rows: Vec<(usize, f32)> = range
                .map(|row| {
                    let at = y;
                    y += self.heights[group][row];
                    rules.push((y, 0.4));
                    (row, at)
                })
                .collect();
            let this = &*self;
            cx.span(|surface, fonts| {
                for (row, top) in rows {
                    let mut x = left;
                    for &(column, w) in &columns {
                        let cell = this.rows[row].get(column).map_or("", String::as_str);
                        if !cell.is_empty() {
                            let spec = spec(this.fit.size, false, w, this.language);
                            let (layout, text) = text::fitted(fonts, cell, &spec, LINES);
                            for index in 0..layout.len() {
                                text::draw_line(
                                    surface,
                                    fonts,
                                    &layout,
                                    &text,
                                    index,
                                    (x + PAD_X, top + PAD_Y),
                                    &mut |_, _| {},
                                    &mut |_, _| {},
                                );
                            }
                        }
                        x += w;
                    }
                }
            });
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

    #[test]
    fn a_sample_takes_the_first_rows_then_evenly_up_to_its_size() {
        assert_eq!(sample(5).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
        let many: Vec<usize> = sample(100_000).collect();
        assert!(many.len() <= SAMPLE_FIRST + SAMPLE_REST + 1);
        assert_eq!(many[..3], [0, 1, 2]);
        assert!(*many.last().unwrap() >= 99_000);
    }

    #[test]
    fn a_long_cell_is_measured_by_its_first_characters() {
        let long = "é".repeat(1000);
        assert_eq!(measured(&long).chars().count(), MEASURED);
        assert_eq!(measured("short"), "short");
    }
}
