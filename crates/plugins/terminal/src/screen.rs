//! The engine's grid as core's `Screen`: runs of styled text placed at their
//! columns, the cursor and the selection in screen coordinates.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{self, CursorShape as Shape, NamedColor};
use bibli_core::terminal::{
    Attrs, Color, Cursor, CursorShape, Palette, Rgb, Run, Screen, Selection, Style,
};

/// The xterm 256-colour table beyond the sixteen: a 6×6×6 cube, then 24
/// greys. It is the standard's, not a theme's.
pub fn indexed(index: u8) -> Rgb {
    match index {
        0..=15 => [0; 3],
        16..=231 => {
            let i = index - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            [level(i / 36), level(i / 6 % 6), level(i % 6)]
        }
        232..=255 => {
            let grey = 8 + (index - 232) * 10;
            [grey; 3]
        }
    }
}

/// The colour a program asked about, as the UI draws it (OSC 4, 10, 11).
pub fn answer(palette: &Palette, index: usize) -> Rgb {
    match index {
        0..=15 => palette.ansi[index],
        16..=255 => indexed(u8::try_from(index).unwrap_or(0)),
        i if i == NamedColor::Background as usize => palette.bg,
        _ => palette.fg,
    }
}

fn rgb(c: ansi::Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// A cell's colour, and whether it asked to be dim. Colours a program set
/// itself (OSC 4, 10, 11) win over the theme.
fn resolve(color: ansi::Color, colors: &Colors) -> (Color, bool) {
    match color {
        ansi::Color::Spec(c) => (rgb(c), false),
        ansi::Color::Indexed(i) => match colors[usize::from(i)] {
            Some(c) => (rgb(c), false),
            None if i < 16 => (Color::Ansi(i), false),
            None => {
                let [r, g, b] = indexed(i);
                (Color::Rgb(r, g, b), false)
            }
        },
        ansi::Color::Named(named) => {
            if let Some(c) = colors[named] {
                return (rgb(c), false);
            }
            let index = named as usize;
            match named {
                NamedColor::Background => (Color::Background, false),
                NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => {
                    (Color::Foreground, false)
                }
                NamedColor::DimForeground => (Color::Foreground, true),
                _ if index < 16 => (Color::Ansi(index as u8), false),
                _ => {
                    // DimBlack..=DimWhite follow Cursor in the same order
                    // as the eight colours.
                    let dim = index.saturating_sub(NamedColor::DimBlack as usize);
                    (Color::Ansi((dim % 8) as u8), true)
                }
            }
        }
    }
}

fn style(flags: Flags, fg: ansi::Color, bg: ansi::Color, colors: &Colors) -> Style {
    let (mut fg, dim_fg) = resolve(fg, colors);
    let (mut bg, _) = resolve(bg, colors);
    if flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    let attrs = Attrs::default()
        .with(Attrs::BOLD, flags.contains(Flags::BOLD))
        .with(Attrs::ITALIC, flags.contains(Flags::ITALIC))
        .with(Attrs::UNDERLINE, flags.intersects(Flags::ALL_UNDERLINES))
        .with(Attrs::STRIKE, flags.contains(Flags::STRIKEOUT))
        .with(Attrs::DIM, dim_fg || flags.contains(Flags::DIM));
    Style { fg, bg, attrs }
}

/// A cell that draws nothing: a space in the background colour, with no
/// line through or under it.
fn blank(c: char, style: &Style) -> bool {
    c == ' '
        && style.bg == Color::Background
        && !style.attrs.has(Attrs::UNDERLINE)
        && !style.attrs.has(Attrs::STRIKE)
}

/// Runs of one line, built cell by cell.
#[derive(Default)]
struct Line {
    runs: Vec<Run>,
}

impl Line {
    /// Add one cell. ASCII in the style of the run before it, right after
    /// it, extends that run; anything else (a wide or combining character,
    /// a symbol from another font) stands alone at its column.
    fn push(&mut self, col: u16, cells: u16, text: &str, style: Style, alone: bool) {
        if let Some(last) = self.runs.last_mut()
            && !alone
            && last.style == style
            && last.col + last.cells == col
            && last.text.is_ascii()
        {
            last.cells += cells;
            last.text.push_str(text);
            return;
        }
        self.runs.push(Run {
            col,
            cells,
            text: text.to_owned(),
            style,
        });
    }

    /// Trailing spaces of a run draw nothing unless they have a background.
    fn finish(mut self) -> Vec<Run> {
        for run in &mut self.runs {
            if run.style.bg == Color::Background
                && !run.style.attrs.has(Attrs::UNDERLINE)
                && !run.style.attrs.has(Attrs::STRIKE)
            {
                let kept = run.text.trim_end_matches(' ').len();
                let dropped = run.text.len() - kept;
                if dropped > 0 && run.text.is_ascii() {
                    run.text.truncate(kept);
                    run.cells -= u16::try_from(dropped).unwrap_or(run.cells);
                }
            }
        }
        self.runs.retain(|run| run.cells > 0);
        self.runs
    }
}

fn clamp16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// Everything the UI needs of `term`'s visible part.
pub fn build<T: alacritty_terminal::event::EventListener>(term: &Term<T>) -> Screen {
    let content = term.renderable_content();
    let (cols, rows) = (term.columns(), term.screen_lines());
    let offset = content.display_offset;
    // Grid lines are counted from the top of the screen, history above it
    // negative; the screen's rows from the top of what is shown.
    let shift = i32::try_from(offset).unwrap_or(i32::MAX);
    let mut lines: Vec<Line> = (0..rows).map(|_| Line::default()).collect();
    for indexed in content.display_iter {
        let cell = indexed.cell;
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let Ok(row) = usize::try_from(indexed.point.line.0 + shift) else {
            continue;
        };
        let Some(line) = lines.get_mut(row) else {
            continue;
        };
        let style = style(cell.flags, cell.fg, cell.bg, content.colors);
        let c = if cell.flags.contains(Flags::HIDDEN) {
            ' '
        } else {
            cell.c
        };
        let combining = cell.zerowidth().filter(|marks| !marks.is_empty());
        if combining.is_none() && blank(c, &style) {
            // A space may still continue the run before it, so a line of
            // words is one run; `finish` trims what trails.
            if let Some(last) = line.runs.last()
                && last.style == style
                && last.col + last.cells == clamp16(indexed.point.column.0)
            {
                line.push(clamp16(indexed.point.column.0), 1, " ", style, false);
            }
            continue;
        }
        let wide = cell.flags.contains(Flags::WIDE_CHAR);
        let mut text = String::from(c);
        if let Some(marks) = combining {
            text.extend(marks);
        }
        let alone = wide || !c.is_ascii() || combining.is_some();
        line.push(
            clamp16(indexed.point.column.0),
            if wide { 2 } else { 1 },
            &text,
            style,
            alone,
        );
    }

    let cursor = (content.cursor.shape != Shape::Hidden)
        .then(|| {
            let point = content.cursor.point;
            let row = usize::try_from(point.line.0 + shift).ok()?;
            (row < rows).then(|| Cursor {
                row: clamp16(row),
                col: clamp16(point.column.0),
                shape: match content.cursor.shape {
                    Shape::Beam => CursorShape::Beam,
                    Shape::Underline => CursorShape::Underline,
                    _ => CursorShape::Block,
                },
                wide: term.grid()[point].flags.contains(Flags::WIDE_CHAR),
            })
        })
        .flatten();

    let selection = content.selection.and_then(|range| {
        let top = range.start.line.0 + shift;
        let bottom = range.end.line.0 + shift;
        let last_row = i32::try_from(rows).unwrap_or(i32::MAX) - 1;
        if bottom < 0 || top > last_row {
            return None;
        }
        // Cut to the screen: a selection that began above it starts at its
        // first cell (a block keeps its column), one that ends below it
        // ends at its last.
        let start = if top < 0 {
            (
                0,
                if range.is_block {
                    range.start.column.0
                } else {
                    0
                },
            )
        } else {
            (top as usize, range.start.column.0)
        };
        let end = if bottom > last_row {
            (
                rows - 1,
                if range.is_block {
                    range.end.column.0
                } else {
                    cols - 1
                },
            )
        } else {
            (bottom as usize, range.end.column.0)
        };
        Some(Selection {
            start: (clamp16(start.0), clamp16(start.1)),
            end: (clamp16(end.0), clamp16(end.1)),
            block: range.is_block,
        })
    });

    Screen {
        cols: clamp16(cols),
        rows: clamp16(rows),
        lines: lines.into_iter().map(Line::finish).collect(),
        cursor,
        selection,
        selected_text: term.selection_to_string().filter(|text| !text.is_empty()),
        scrolled: offset,
        history: term.grid().history_size(),
        ..Screen::default()
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;

    fn term(cols: usize, rows: usize, bytes: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &TermSize::new(cols, rows), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes);
        term
    }

    fn texts(screen: &Screen, row: usize) -> Vec<(u16, &str)> {
        screen.lines[row]
            .iter()
            .map(|run| (run.col, run.text.as_str()))
            .collect()
    }

    #[test]
    fn words_are_one_run_and_blanks_draw_nothing() {
        let screen = build(&term(20, 3, b"ls -la  \r\n\r\n   x"));
        assert_eq!((screen.cols, screen.rows), (20, 3));
        assert_eq!(texts(&screen, 0), [(0, "ls -la")]);
        assert!(screen.lines[1].is_empty());
        assert_eq!(texts(&screen, 2), [(3, "x")]);
        let cursor = screen.cursor.unwrap();
        assert_eq!((cursor.row, cursor.col), (2, 4));
    }

    #[test]
    fn colours_stay_symbolic_until_a_program_asks_for_rgb() {
        let screen = build(&term(
            30,
            1,
            b"\x1b[31mred\x1b[0m \x1b[1;38;5;196mcube\x1b[0m \x1b[7mrev\x1b[0m",
        ));
        let runs = &screen.lines[0];
        assert_eq!(runs[0].style.fg, Color::Ansi(1));
        assert_eq!(runs[0].text, "red");
        let cube = runs.iter().find(|run| run.text == "cube").unwrap();
        assert_eq!(cube.style.fg, Color::Rgb(255, 0, 0));
        assert!(cube.style.attrs.has(Attrs::BOLD));
        let reversed = runs.iter().find(|run| run.text == "rev").unwrap();
        assert_eq!(
            (reversed.style.fg, reversed.style.bg),
            (Color::Background, Color::Foreground)
        );
    }

    #[test]
    fn wide_and_combining_characters_stand_at_their_column() {
        let screen = build(&term(20, 1, "a漢b e\u{301}!".as_bytes()));
        let runs = &screen.lines[0];
        let at = |text: &str| runs.iter().find(|run| run.text == text).unwrap();
        assert_eq!((at("漢").col, at("漢").cells), (1, 2));
        assert_eq!(at("b").col, 3, "the run after a wide character");
        assert_eq!(at("e\u{301}").col, 5);
        assert_eq!(at("!").col, 6);
    }

    #[test]
    fn the_256_colour_table_is_xterms() {
        assert_eq!(indexed(16), [0, 0, 0]);
        assert_eq!(indexed(21), [0, 0, 255]);
        assert_eq!(indexed(196), [255, 0, 0]);
        assert_eq!(indexed(231), [255, 255, 255]);
        assert_eq!(indexed(232), [8, 8, 8]);
        assert_eq!(indexed(255), [238, 238, 238]);
    }
}
