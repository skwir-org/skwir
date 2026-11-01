//! Lines of code: the code face at 9 points, a line wrapped where it is
//! wider than the page and its rows after the first marked "↪" in the
//! gutter, beside each line's number when numbered. Pages are counted
//! from the rows each line takes, laying out only the lines that may not
//! fit on one; a page lays out its lines again as it is drawn, one at a
//! time, so a file of millions of lines is held as its pages' starts.
//! A line longer than [`PIECE`] (a minified file, one whose lines end in
//! a carriage return alone) is laid out a piece at a time, each from a
//! new row, so no layout holds more than a piece of it.

use std::ops::Range;

use anyhow::Result;
use krilla::tagging::{ArtifactType, Tag};
use parley::{Alignment, Layout};
use skwir_core::print::{Line, Run};

use crate::fonts::{Fonts, Role};
use crate::geometry::{Frame, units_pages};
use crate::out::{PageCx, Part, Structure, key};
use crate::text::{self, GREY, Ink, Spec, Wrap};

pub const SIZE: f32 = 9.0;
pub const LINE_HEIGHT: f32 = 1.3;
/// Columns a tab moves to the next multiple of.
const TAB: usize = 4;
/// Between the gutter and the code.
const GAP: f32 = 6.0;
/// The most bytes of a line laid out at once.
pub const PIECE: usize = 4096;

pub struct Code<'a> {
    lines: &'a [Line],
    numbers: bool,
    first: u32,
    spec: Spec,
    gutter: f32,
    row: f32,
    per_page: u32,
    /// Where each page starts: a line, the piece of it (its first byte)
    /// and the row of that.
    starts: Vec<((u32, u32), u32)>,
    frame: Frame,
    /// The piece laid out last, by its line and first byte, kept for the
    /// page after when it goes on over it.
    cache: Option<((usize, usize), Layout<Ink>, String)>,
    structure: Structure,
}

impl<'a> Code<'a> {
    pub fn plan(
        lines: &'a [Line],
        numbers: bool,
        first: u32,
        fonts: &mut Fonts,
        frame: Frame,
    ) -> Code<'a> {
        let row = SIZE * LINE_HEIGHT;
        let mut gutter_spec = Spec::new(Role::Code, SIZE);
        gutter_spec.line_height = LINE_HEIGHT;
        let last = u64::from(first) + lines.len().saturating_sub(1) as u64;
        let widest = if numbers {
            "0".repeat(last.to_string().len())
        } else {
            String::new()
        };
        let digits = text::plain(fonts, &widest, &gutter_spec).width();
        let arrow = text::plain(fonts, "↪", &gutter_spec).width();
        let gutter = digits.max(arrow);
        let mut spec = Spec::new(Role::Code, SIZE);
        spec.line_height = LINE_HEIGHT;
        spec.wrap = Wrap::Anywhere;
        let width = (frame.text.width - gutter - GAP).max(SIZE * 4.0);
        spec.width = Some(width);
        let per_page = ((frame.text.height / row).floor() as u32).max(1);
        // A line of printable ASCII in a monospaced face fits when its
        // characters do, in each of the faces its colors may ask (bold,
        // italic); any other is laid out, a piece at a time, to count its
        // rows.
        let advance = monospaced(fonts, &spec);
        let mut layout = Layout::new();
        let units = lines.iter().enumerate().flat_map(|(at, line)| {
            let printable = line.text.bytes().all(|b| (0x20..0x7f).contains(&b));
            let fits = advance.is_some_and(|advance| {
                printable && line.text.len() as f32 * advance <= width - 0.5
            });
            pieces(&line.text).map(move |piece| ((at as u32, piece.start as u32), piece, fits))
        });
        let units = units.map(|(key, piece, fits)| {
            if fits {
                return (key, 1);
            }
            let line = &lines[key.0 as usize];
            let (text, runs) = piece_of(line, piece);
            text::lay_out(fonts, &mut layout, &text, &runs, &spec, &mut Vec::new());
            (key, layout.len().max(1) as u32)
        });
        let starts = units_pages(units, per_page);
        Code {
            lines,
            numbers,
            first,
            spec,
            gutter,
            row,
            per_page,
            starts,
            frame,
            cache: None,
            structure: Structure::default(),
        }
    }
}

impl Part for Code<'_> {
    fn pages(&self) -> usize {
        self.starts.len()
    }

    fn draw(&mut self, page: usize, cx: &mut PageCx) -> Result<()> {
        self.structure.goto(cx, &[(key(1, 0), Tag::Code.into())]);
        let Some(&((first_line, first_piece), first_row)) = self.starts.get(page) else {
            return Ok(());
        };
        let text_x = self.frame.text.x + self.gutter + GAP;
        let top = self.frame.text.y;
        let mut gutter = String::new();
        let Code {
            cache,
            lines,
            spec,
            per_page,
            row: row_height,
            numbers,
            first,
            ..
        } = self;
        // The code first, a line laid out at a time, its rows written in
        // the gutter as they are drawn; the gutter after.
        cx.span(|surface, fonts| {
            let (mut line, mut start, mut row) = (
                first_line as usize,
                first_piece as usize,
                first_row as usize,
            );
            let mut drawn = 0u32;
            while line < lines.len() && drawn < *per_page {
                let piece = start..piece_end(&lines[line].text, start);
                if cache.as_ref().is_none_or(|(at, _, _)| *at != (line, start)) {
                    let (text, runs) = piece_of(&lines[line], piece.clone());
                    let mut layout = cache.take().map(|(_, l, _)| l).unwrap_or_default();
                    text::lay_out(fonts, &mut layout, &text, &runs, spec, &mut Vec::new());
                    *cache = Some(((line, start), layout, text));
                }
                let (_, layout, text) = cache.as_ref().expect("laid out");
                let count = layout.len().max(1);
                while row < count && drawn < *per_page {
                    if drawn > 0 {
                        gutter.push('\n');
                    }
                    if row == 0 && start == 0 {
                        if *numbers {
                            gutter.push_str(&(u64::from(*first) + line as u64).to_string());
                        }
                    } else {
                        gutter.push('↪');
                    }
                    let y = top + drawn as f32 * *row_height;
                    let shift = layout
                        .get(row)
                        .map_or(0.0, |line| line.metrics().block_min_coord);
                    text::draw_line(
                        surface,
                        fonts,
                        layout,
                        text,
                        row,
                        (text_x, y - shift),
                        &mut |_, _| {},
                        &mut |_, _| {},
                    );
                    drawn += 1;
                    row += 1;
                }
                if row >= count {
                    row = 0;
                    if piece.end < lines[line].text.len() {
                        start = piece.end;
                    } else {
                        line += 1;
                        start = 0;
                    }
                }
            }
        });
        if !gutter.is_empty() {
            let mut spec = Spec::new(Role::Code, SIZE);
            spec.line_height = LINE_HEIGHT;
            spec.color = GREY;
            spec.align = Alignment::End;
            spec.width = Some(self.gutter + 1.0);
            let numbers = text::plain(cx.fonts, &gutter, &spec);
            let x = self.frame.text.x - 1.0;
            let shift = numbers.get(0).map_or(0.0, |l| l.metrics().block_min_coord);
            cx.artifact(ArtifactType::LineNumber, |surface, fonts| {
                for index in 0..numbers.len() {
                    text::draw_line(
                        surface,
                        fonts,
                        &numbers,
                        &gutter,
                        index,
                        (x, top - shift),
                        &mut |_, _| {},
                        &mut |_, _| {},
                    );
                }
            });
        }
        if page + 1 == self.starts.len() {
            self.structure.close_all(cx);
        }
        Ok(())
    }

    fn finish(&mut self, cx: &mut PageCx) {
        self.structure.close_all(cx);
    }
}

/// Whether the code face writes every printable ASCII character at one
/// advance in each of its faces a color may ask (regular, bold, italic,
/// both): that advance, when it does.
fn monospaced(fonts: &mut Fonts, spec: &Spec) -> Option<f32> {
    let mut advance: Option<f32> = None;
    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut spec = spec.clone();
        spec.bold = bold;
        spec.italic = italic;
        let narrow = text::plain(fonts, "i", &spec).width();
        let wide = text::plain(fonts, "M", &spec).width();
        if (narrow - wide).abs() >= 0.01 || advance.is_some_and(|a| (a - wide).abs() >= 0.01) {
            return None;
        }
        advance = Some(wide);
    }
    advance
}

/// Where the piece of `text` from `start` ends: [`PIECE`] bytes on, on a
/// character's boundary, or the end of the text.
fn piece_end(text: &str, start: usize) -> usize {
    if text.len() - start <= PIECE {
        text.len()
    } else {
        text.floor_char_boundary(start + PIECE)
    }
}

/// The pieces a line is laid out in: one for any line up to [`PIECE`]
/// bytes, an empty one included.
pub fn pieces(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut start = Some(0);
    std::iter::from_fn(move || {
        let from = start?;
        let end = piece_end(text, from);
        start = (end < text.len()).then_some(end);
        Some(from..end)
    })
}

/// A piece of a line, its tabs turned to spaces, with its runs moved to
/// start with it. A line laid out whole keeps all its runs; a piece of a
/// long one finds its own by halving, as a highlighter writes runs in
/// order, apart.
fn piece_of(line: &Line, piece: Range<usize>) -> (String, Vec<Run>) {
    if piece.start == 0 && piece.end == line.text.len() {
        return detab(&line.text, &line.runs);
    }
    let first = line
        .runs
        .partition_point(|run| (run.end as usize) <= piece.start);
    let runs: Vec<Run> = line.runs[first..]
        .iter()
        .take_while(|run| (run.start as usize) < piece.end)
        .filter_map(|run| {
            let from = (run.start as usize).max(piece.start);
            let to = (run.end as usize).min(piece.end);
            (from < to).then(|| Run {
                start: (from - piece.start) as u32,
                end: (to - piece.start) as u32,
                ..run.clone()
            })
        })
        .collect();
    detab(&line.text[piece], &runs)
}

/// Code with its tabs turned to spaces, to the next multiple of [`TAB`]
/// columns of its line, and its runs moved with what they cover.
pub fn detab(text: &str, runs: &[Run]) -> (String, Vec<Run>) {
    if !text.contains('\t') {
        return (text.to_owned(), runs.to_vec());
    }
    let mut out = String::with_capacity(text.len() + 16);
    // Where each byte of the line went, and its end.
    let mut moved = Vec::with_capacity(text.len() + 1);
    let mut column = 0;
    for (at, c) in text.char_indices() {
        for _ in 0..c.len_utf8() {
            moved.push(out.len());
        }
        debug_assert_eq!(moved.len(), at + c.len_utf8());
        if c == '\t' {
            let spaces = TAB - column % TAB;
            out.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            out.push(c);
            column = if c == '\n' { 0 } else { column + 1 };
        }
    }
    moved.push(out.len());
    let place = |at: u32| -> u32 {
        let at = (at as usize).min(text.len());
        moved[at] as u32
    };
    let runs = runs
        .iter()
        .map(|run| Run {
            start: place(run.start),
            end: place(run.end),
            ..run.clone()
        })
        .collect();
    (out, runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_line_is_laid_out_in_pieces_with_their_runs() {
        let empty: Vec<_> = pieces("").collect();
        assert!(empty.len() == 1 && empty[0] == (0..0), "{empty:?}");
        let short: Vec<_> = pieces("short").collect();
        assert!(short.len() == 1 && short[0] == (0..5), "{short:?}");
        // Of 10 000 bytes, two pieces whole and what is left, each on a
        // character's boundary.
        let long = "é".repeat(5000);
        let cut: Vec<_> = pieces(&long).collect();
        assert_eq!(cut.len(), 3);
        assert!(cut.iter().all(|piece| long.is_char_boundary(piece.end)));
        assert_eq!(cut.last().unwrap().end, long.len());
        let line = Line {
            text: "a".repeat(PIECE + 10),
            runs: vec![
                Run {
                    start: 0,
                    end: 2,
                    bold: true,
                    ..Run::default()
                },
                Run {
                    start: (PIECE - 2) as u32,
                    end: (PIECE + 4) as u32,
                    italic: true,
                    ..Run::default()
                },
            ],
        };
        let (text, runs) = piece_of(&line, PIECE..PIECE + 10);
        assert_eq!(text.len(), 10);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].italic && (runs[0].start, runs[0].end) == (0, 4));
    }

    #[test]
    fn tabs_go_to_the_next_stop_and_runs_follow() {
        let runs = [Run {
            start: 2,
            end: 5,
            bold: true,
            ..Run::default()
        }];
        let (text, runs) = detab("a\tbcd\te", &runs);
        assert_eq!(text, "a   bcd e");
        // "bcd" was at 2..5, is at 4..7.
        assert_eq!((runs[0].start, runs[0].end), (4, 7));
        assert_eq!(&text[4..7], "bcd");
        let (same, _) = detab("plain", &[]);
        assert_eq!(same, "plain");
        // Each line of a block counts its own columns.
        let (block, _) = detab("ab\tc\n\td", &[]);
        assert_eq!(block, "ab  c\n    d");
    }
}
