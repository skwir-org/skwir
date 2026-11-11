//! What the inspector works out about rows before it draws them: how wide a
//! table's columns are (and how wide the user dragged them) and which hold
//! numbers, the order a click on a header asks for, and what the bytes of a
//! selected hex row mean. Pure, so it is
//! tested without a window; the shape is computed on the worker that builds
//! the `Display`, never while painting.
use skwir_core::inspection::{Block, BlockKind, Row};
use std::collections::HashMap;

/// Column widths and kinds of a table, from its header and the rows loaded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub widths: Vec<f32>,
    /// Every filled cell of the column is a number: drawn right-aligned and
    /// sorted by value.
    pub numeric: Vec<bool>,
}

/// An estimate from character counts, not a measurement: a cell that is wider
/// after all is truncated, never overlapping its neighbour.
const CHAR: f32 = 7.5;
const PADDING: f32 = 18.;
const NARROWEST: f32 = 56.;
const WIDEST: f32 = 320.;

/// What a column dragged by its edge may be: narrow enough to set aside,
/// wide enough to read a long cell whole (a cell draws its first 512
/// characters).
const DRAGGED_NARROWEST: f32 = 40.;
const DRAGGED_WIDEST: f32 = 1600.;

/// The widths the user gave a table's columns by dragging their edges, in
/// place of the shape's. They are kept while the header is the one they
/// were given for (the next rows of a CSV), and forgotten with another (a
/// spreadsheet's next sheet) or another file, whose `State` starts empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dragged {
    header: Vec<String>,
    widths: Vec<Option<f32>>,
}

impl Dragged {
    /// A table came with `header`: the widths stay if it is theirs.
    pub fn keep_for(&mut self, header: &[String]) {
        if self.header != header {
            self.header = header.to_vec();
            self.widths.clear();
        }
    }

    /// Column `column` is `width` wide, within what a drag may make it.
    pub fn set(&mut self, column: usize, width: f32) {
        if self.widths.len() <= column {
            self.widths.resize(column + 1, None);
        }
        self.widths[column] = Some(width.clamp(DRAGGED_NARROWEST, DRAGGED_WIDEST));
    }

    /// Column `column` is as wide as its content again.
    pub fn forget(&mut self, column: usize) {
        if let Some(width) = self.widths.get_mut(column) {
            *width = None;
        }
    }

    /// Column `column`'s width: the one dragged, else the shape's; `None`
    /// past the shape's columns.
    pub fn width(&self, shape: &Shape, column: usize) -> Option<f32> {
        let dragged = self.widths.get(column).copied().flatten();
        dragged.or_else(|| shape.widths.get(column).copied())
    }

    /// Every column's width: the one dragged, else the shape's.
    pub fn widths(&self, shape: &Shape) -> Vec<f32> {
        let mut widths = shape.widths.clone();
        for (width, dragged) in widths.iter_mut().zip(&self.widths) {
            if let Some(dragged) = dragged {
                *width = *dragged;
            }
        }
        widths
    }
}

/// A number as people write one: digits with an optional sign, a decimal
/// point, thousands separators, a percent sign or a currency mark.
pub fn number(cell: &str) -> Option<f64> {
    let text: String = cell
        .trim()
        .chars()
        .filter(|c| !matches!(c, ',' | ' ' | '\u{a0}' | '_' | '%' | '$' | '€' | '£'))
        .collect();
    if text.is_empty() || !text.contains(|c: char| c.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

pub fn shape(columns: &[String], rows: &[Row]) -> Shape {
    let count = columns
        .len()
        .max(rows.iter().map(|row| row.cells.len()).max().unwrap_or(0));
    let mut chars: Vec<usize> = (0..count)
        .map(|index| {
            columns
                .get(index)
                .map_or(0, |title| title.chars().count() + 2)
        })
        .collect();
    let mut numeric = vec![true; count];
    let mut filled = vec![false; count];
    for row in rows {
        for (index, cell) in row.cells.iter().enumerate() {
            // A long cell does not get to set the width: it is truncated.
            let longest = cell.lines().map(|line| line.chars().count()).max();
            chars[index] = chars[index].max(longest.unwrap_or(0).min(40));
            if !cell.trim().is_empty() {
                filled[index] = true;
                numeric[index] &= number(cell).is_some();
            }
        }
    }
    Shape {
        widths: chars
            .iter()
            .map(|count| (*count as f32 * CHAR + PADDING).clamp(NARROWEST, WIDEST))
            .collect(),
        numeric: numeric
            .iter()
            .zip(filled)
            .map(|(numeric, filled)| *numeric && filled)
            .collect(),
    }
}

/// Order `visible` (indices into `rows`) by a column: by value when every
/// filled cell is a number, otherwise as text without regard to case. Empty
/// cells go last either way, and equal rows keep the order of the file.
pub fn sort(rows: &[Row], visible: &mut [usize], column: usize, ascending: bool) {
    let cell = |index: usize| rows[index].cells.get(column).map_or("", |cell| cell.trim());
    let numeric = visible
        .iter()
        .all(|index| cell(*index).is_empty() || number(cell(*index)).is_some());
    // Keys are worked out once per row, not once per comparison.
    if numeric {
        let keys: Vec<Option<f64>> = visible.iter().map(|i| number(cell(*i))).collect();
        order(visible, keys, f64::total_cmp, ascending);
    } else {
        let keys: Vec<Option<String>> = visible
            .iter()
            .map(|i| {
                Some(cell(*i))
                    .filter(|c| !c.is_empty())
                    .map(str::to_lowercase)
            })
            .collect();
        order(visible, keys, Ord::cmp, ascending);
    }
}

/// Order `visible` by `keys` (one per entry, `None` for a blank cell):
/// blanks last either way, equal keys in the order they came.
fn order<K>(
    visible: &mut [usize],
    keys: Vec<Option<K>>,
    compare: impl Fn(&K, &K) -> std::cmp::Ordering,
    ascending: bool,
) {
    let mut keyed: Vec<(Option<K>, usize)> =
        keys.into_iter().zip(visible.iter().copied()).collect();
    keyed.sort_by(|(a, _), (b, _)| match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(a), Some(b)) if ascending => compare(a, b),
        (Some(a), Some(b)) => compare(a, b).reverse(),
    });
    for (slot, (_, index)) in visible.iter_mut().zip(keyed) {
        *slot = index;
    }
}

/// The bytes of one row of the hex view, from its cell of hex pairs.
pub fn bytes_of(cell: &str) -> Vec<u8> {
    cell.split_whitespace()
        .filter_map(|pair| u8::from_str_radix(pair, 16).ok())
        .collect()
}

/// What the first bytes of a selection mean, read little-endian as most files
/// on this machine are: the questions a byte view is opened to answer.
pub fn decode(bytes: &[u8]) -> Vec<(&'static str, String)> {
    let mut values = Vec::new();
    let take = |count: usize| bytes.get(..count);
    if let Some([byte]) = take(1) {
        values.push(("u8", format!("{byte} · {byte:08b}")));
    }
    if let Some(&[a, b]) = take(2) {
        values.push(("u16", u16::from_le_bytes([a, b]).to_string()));
    }
    if let Some(&[a, b, c, d]) = take(4) {
        let raw = [a, b, c, d];
        values.push(("u32", u32::from_le_bytes(raw).to_string()));
        values.push(("i32", i32::from_le_bytes(raw).to_string()));
        let float = f32::from_le_bytes(raw);
        if float.is_finite() {
            values.push(("f32", format!("{float:e}")));
        }
        values.push(("big-endian u32", u32::from_be_bytes(raw).to_string()));
    }
    if let Some(&[a, b, c, d, e, f, g, h]) = take(8) {
        values.push((
            "u64",
            u64::from_le_bytes([a, b, c, d, e, f, g, h]).to_string(),
        ));
    }
    let text: String = String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| {
            if c.is_control() || c == '\u{fffd}' {
                '·'
            } else {
                c
            }
        })
        .collect();
    if text.chars().any(|c| c != '·') {
        values.push(("text", text));
    }
    values
}

/// The columns of a table read as a document's (a Markdown or HTML table,
/// a notebook's frame), in ems of its text: the width each needs at least,
/// its longest word, which is never broken, and its cells whole up to
/// `READABLE`, and the width it would take, its longest cell on one line. Estimated from character counts, as
/// `Shape` is, once on the worker that builds the display: a column given
/// less wraps between its words; a table whose columns need more than the
/// panel scrolls sideways, its rows together.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Columns {
    pub least: Vec<f32>,
    pub wanted: Vec<f32>,
}

impl Columns {
    /// The width the columns need at least, together.
    pub fn least_total(&self) -> f32 {
        self.least.iter().sum()
    }
}

/// A document's tables: the table each row is part of, known by its first
/// row's block, and each one's columns.
#[derive(Clone, Debug, Default)]
pub struct Tables {
    first: HashMap<usize, usize>,
    columns: HashMap<usize, Columns>,
}

/// A character's width in ems, estimated on the wide side, as a short
/// estimate breaks a word: an East Asian one is a square; Latin letters by
/// their shapes (`m` and `w` wide, capitals and digits wider than most,
/// `i`, `l` and the stops narrow).
fn letter_ems(letter: char) -> f32 {
    match letter {
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.95,
        'i' | 'j' | 'l' | 'I' | '.' | ',' | ':' | ';' | '!' | '|' | '\'' => 0.32,
        'f' | 'r' | 't' | ' ' => 0.42,
        _ if letter.is_ascii_uppercase() => 0.74,
        _ if letter.is_ascii_digit() => 0.62,
        _ if letter.is_ascii() => 0.6,
        _ if matches!(u32::from(letter), 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6) => {
            1.
        }
        _ => 0.66,
    }
}

/// A header's letters are bold, and wider.
const BOLD: f32 = 1.12;
/// What a word may need beyond its estimate, so that it is not broken
/// where the estimate falls short.
const SLACK: f32 = 0.8;
/// A longer word (an address) may break; a longer cell wraps.
const LONGEST_WORD: f32 = 18.;
const LONGEST_CELL: f32 = 24.;
/// A column is never narrower than its cells up to this, in ems: a short
/// cell (a code, a number, a date) stays on its line, and a long one wraps
/// in a column it reads in, never a letter or two wide.
const READABLE: f32 = 8.;

impl Tables {
    /// The tables of `blocks`: each run of rows one table.
    pub fn of(blocks: &[Block]) -> Self {
        let mut tables = Self::default();
        let mut first = None;
        for (index, block) in blocks.iter().enumerate() {
            let BlockKind::TableRow { cells, header } = &block.kind else {
                first = None;
                continue;
            };
            let start = *first.get_or_insert(index);
            tables.first.insert(index, start);
            let columns = tables.columns.entry(start).or_default();
            if columns.least.len() < cells.len() {
                columns.least.resize(cells.len(), 0.);
                columns.wanted.resize(cells.len(), 0.);
            }
            let scale = if *header { BOLD } else { 1. };
            for (column, cell) in cells.iter().enumerate() {
                let (word, whole) = measure(block, cell.clone());
                let least = (word.max(whole.min(READABLE)) * scale + SLACK).min(LONGEST_WORD);
                let wanted = (whole * scale + SLACK).min(LONGEST_CELL).max(least);
                columns.least[column] = columns.least[column].max(least);
                columns.wanted[column] = columns.wanted[column].max(wanted);
            }
        }
        tables
    }

    /// The table `block` is a row of: its first row's block and its columns.
    pub fn columns(&self, block: usize) -> Option<(usize, &Columns)> {
        let first = *self.first.get(&block)?;
        Some((first, self.columns.get(&first)?))
    }

    /// Whether `block` is the last row of its table.
    pub fn ends(&self, block: usize) -> bool {
        self.first.get(&(block + 1)) != self.first.get(&block)
    }

    pub fn is_empty(&self) -> bool {
        self.first.is_empty()
    }
}

/// A cell's longest word and its whole width, in ems: a drawing (a formula)
/// as wide as it is drawn, and never broken.
fn measure(block: &Block, cell: std::ops::Range<usize>) -> (f32, f32) {
    let Some(text) = block.text.get(cell.clone()) else {
        return (0., 0.);
    };
    let (mut longest, mut word, mut whole) = (0f32, 0f32, 0f32);
    for (offset, letter) in text.char_indices() {
        let at = cell.start + offset;
        let ems = if letter == Block::DRAWN {
            let run = block
                .runs
                .get(block.runs.partition_point(|run| run.range.end <= at))
                .filter(|run| run.range.start <= at);
            run.and_then(|run| run.drawing.as_ref())
                .map_or(1., |drawing| drawing.width)
        } else {
            letter_ems(letter)
        };
        if letter.is_whitespace() {
            longest = longest.max(word);
            word = 0.;
        } else {
            word += ems;
        }
        whole += ems;
    }
    (longest.max(word), whole)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str], header: bool) -> Block {
        let mut text = String::new();
        let mut ranges = Vec::new();
        for (i, cell) in cells.iter().enumerate() {
            if i > 0 {
                text.push('\t');
            }
            let start = text.len();
            text.push_str(cell);
            ranges.push(start..text.len());
        }
        Block {
            text,
            kind: BlockKind::TableRow {
                cells: ranges,
                header,
            },
            ..Block::default()
        }
    }

    /// Each table's columns: at least their longest word, as much as their
    /// longest cell; the rows of one table share them, another's are its
    /// own.
    #[test]
    fn a_table_s_columns_need_their_longest_word_and_want_their_longest_cell() {
        let blocks = vec![
            row(&["", "State Code", "City Name"], true),
            row(&["0", "1", "Alabaster to the north"], false),
            Block {
                text: "Between".into(),
                ..Block::default()
            },
            row(&["a"], true),
        ];
        let tables = Tables::of(&blocks);
        let (first, columns) = tables.columns(1).unwrap();
        assert_eq!(first, 0);
        assert_eq!(tables.columns(0).unwrap().1, columns, "one table");
        // "State Code" is bold: its longest word is "State", 5 letters.
        let ems =
            |text: &str, scale: f32| text.chars().map(letter_ems).sum::<f32>() * scale + SLACK;
        // "State Code", bold, is short: whole on its line.
        assert!((columns.least[1] - ems("State Code", BOLD)).abs() < 1e-4);
        assert!((columns.wanted[1] - ems("State Code", BOLD)).abs() < 1e-4);
        // "Alabaster to the north" wraps, in a column it reads in.
        assert!((columns.least[2] - (READABLE + SLACK)).abs() < 1e-4);
        assert!((columns.wanted[2] - ems("Alabaster to the north", 1.)).abs() < 1e-4);
        // A word longer than that is never broken.
        let long = Tables::of(&[row(&["Incomprehensibilities"], false)]);
        assert!(
            (long.columns(0).unwrap().1.least[0] - ems("Incomprehensibilities", 1.)).abs() < 1e-4
        );
        assert!(!tables.ends(0) && tables.ends(1));
        assert!(tables.columns(2).is_none());
        assert_eq!(tables.columns(3).unwrap().0, 3, "a table of its own");
        assert!(tables.ends(3));
    }

    fn rows(cells: &[&[&str]]) -> Vec<Row> {
        cells
            .iter()
            .map(|cells| Row {
                cells: cells.iter().map(|cell| (*cell).to_owned()).collect(),
                ..Row::default()
            })
            .collect()
    }

    #[test]
    fn columns_are_as_wide_as_their_content_and_numbers_are_told_apart() {
        let table = rows(&[
            &["apple", "3", "1.20", "€ 1,200", ""],
            &[
                "a rather long name of a fruit that goes on and on and on",
                "12",
                "n/a",
                "35%",
                "",
            ],
        ]);
        let columns = ["name", "qty", "price", "value", "notes"].map(String::from);
        let shape = shape(&columns, &table);
        assert_eq!(shape.numeric, [false, true, false, true, false]);
        assert!((shape.widths[1] - NARROWEST).abs() < 1e-3, "a short column");
        assert!(
            (shape.widths[0] - (40. * CHAR + PADDING)).abs() < 1e-3,
            "capped"
        );
        assert!(
            shape
                .widths
                .iter()
                .all(|width| (NARROWEST..=WIDEST).contains(width))
        );
        assert_eq!(number(" -1,234.5 "), Some(-1234.5));
        assert_eq!(number("12 %"), Some(12.));
        assert_eq!(number("1e3"), Some(1000.));
        assert_eq!(number("—"), None);
        assert_eq!(number("v2"), None);
    }

    #[test]
    fn a_dragged_width_stays_with_its_header() {
        let header = ["name", "qty"].map(String::from);
        let shape = shape(&header, &rows(&[&["apple", "3"]]));
        let mut dragged = Dragged::default();
        dragged.keep_for(&header);
        assert_eq!(dragged.widths(&shape), shape.widths, "nothing dragged");
        dragged.set(0, 500.);
        dragged.set(1, 2.);
        // A column past the shape's (a drag on the last of a short row) is
        // kept, never drawn by itself.
        dragged.set(7, 90.);
        assert_eq!(dragged.widths(&shape), [500., DRAGGED_NARROWEST]);
        assert_eq!(dragged.width(&shape, 1), Some(DRAGGED_NARROWEST));
        assert_eq!(dragged.width(&shape, 2), None);
        // The next rows of the same table keep them.
        dragged.keep_for(&header);
        assert_eq!(dragged.widths(&shape)[0], 500.);
        dragged.set(0, 1e6);
        assert_eq!(dragged.widths(&shape)[0], DRAGGED_WIDEST);
        // A double click on the edge gives the column its own width back.
        dragged.forget(0);
        dragged.forget(30);
        assert_eq!(dragged.widths(&shape)[0], shape.widths[0]);
        // Another header: the widths were not given for it.
        dragged.keep_for(&["date".to_owned(), "qty".to_owned()]);
        assert_eq!(dragged.widths(&shape), shape.widths);
    }

    #[test]
    fn a_header_click_orders_by_value_or_by_text_and_keeps_blanks_last() {
        let table = rows(&[
            &["pear", "12"],
            &["Apple", "3"],
            &["kiwi", ""],
            &["fig", "100"],
        ]);
        let mut visible = vec![0, 1, 2, 3];
        sort(&table, &mut visible, 1, true);
        assert_eq!(visible, [1, 0, 3, 2], "3, 12, 100, then the blank");
        sort(&table, &mut visible, 1, false);
        assert_eq!(visible, [3, 0, 1, 2], "the blank is still last");
        sort(&table, &mut visible, 0, true);
        assert_eq!(visible, [1, 3, 2, 0], "case does not matter");
        // A column that is not only numbers is text: "100" sorts before "12".
        let mixed = rows(&[&["12"], &["100"], &["n/a"]]);
        let mut visible = vec![0, 1, 2];
        sort(&mixed, &mut visible, 0, true);
        assert_eq!(visible, [1, 0, 2]);
    }

    #[test]
    fn bytes_are_read_the_ways_a_byte_view_is_asked_for() {
        let bytes = bytes_of("50 4B 03 04  14 00 00 00");
        assert_eq!(bytes, [0x50, 0x4b, 3, 4, 0x14, 0, 0, 0]);
        let values = decode(&bytes);
        let value = |label: &str| {
            values
                .iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(value("u16"), Some("19280"));
        assert_eq!(value("u32"), Some("67324752"));
        assert_eq!(value("big-endian u32"), Some("1347093252"));
        assert_eq!(value("u64"), Some("85966670672"));
        assert_eq!(value("text"), Some("PK······"));
        assert!(decode(&[]).is_empty());
        assert_eq!(decode(&[0xff])[0].1, "255 · 11111111");
    }
}
