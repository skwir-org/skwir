//! What the inspector works out about rows before it draws them: how wide a
//! table's columns are (and how wide the user dragged them) and which hold
//! numbers, the order a click on a header asks for, and what the bytes of a
//! selected hex row mean. Pure, so it is
//! tested without a window; the shape is computed on the worker that builds
//! the `Display`, never while painting.
use skwir_core::inspection::Row;

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

#[cfg(test)]
mod tests {
    use super::*;

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
