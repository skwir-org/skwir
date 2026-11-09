//! How a text table's cells are split. A TSV says so by its type. A CSV
//! says less than its type does: Excel and `LibreOffice` write `;` where a
//! comma is the decimal mark, and other programs tabs or `|`, all under
//! `text/csv`. So a CSV's separator is read from its start, the same for
//! every page of a version of the file, whose page starts (`starts`) depend
//! on it: its own `sep=` line, else the separator that reads best.
//!
//! Each separator is tried by reading the start as the file will be read,
//! quotes and all, so one inside a quoted cell never counts. A reading is
//! worth its rows that agree on how many cells they have, times its cells
//! that look like cells (after van den Burg, Nazábal and Sutton, "Wrangling
//! messy CSV files by detecting row and type patterns", 2019). Rows settle
//! most files; cells settle `foo;1,5` with no header, two cells in every
//! row either way, where only `;` leaves two that look like cells, and
//! `Doe, Ann;30`, where only `;` leaves none cut from its neighbour.

use std::io::Read;

/// What a CSV's cells may be split by, the type's own first: it is what a
/// tie gives. Not a space, which every sentence holds, nor `:`, which every
/// time does.
const SEPARATORS: [u8; 4] = *b",;\t|";

/// How much of a table's start is looked at: what a preview holds of any
/// file (`preview::HEAD`).
const SAMPLE: usize = 64 * 1024;

/// How a text table is split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Dialect {
    separator: u8,
    /// Tab-separated values have no quoting: a cell that starts with `"`
    /// is that text, not the start of a field running to the file's end.
    quoting: bool,
    /// The bytes of a `sep=` line opening the file, which is no row.
    skip: usize,
}

impl Dialect {
    /// The dialect of a table whose text begins with `start` (bytes in an
    /// encoding ASCII is part of), which goes on past it if `more`: split
    /// by tabs if its type says `tabs`, else as it reads best.
    pub(crate) fn of(tabs: bool, start: &[u8], more: bool) -> Self {
        if tabs {
            return Dialect {
                separator: b'\t',
                quoting: false,
                skip: 0,
            };
        }
        if let Some((separator, skip)) = declared(start) {
            return Dialect {
                separator,
                quoting: true,
                skip,
            };
        }
        let more = more || start.len() > SAMPLE;
        let sample = &start[..start.len().min(SAMPLE)];
        let mut best = (SEPARATORS[0], 0.0);
        for separator in SEPARATORS {
            if let Some(score) = score(sample, more, separator)
                && score > best.1
            {
                best = (separator, score);
            }
        }
        Dialect {
            separator: best.0,
            quoting: true,
            skip: 0,
        }
    }

    /// The bytes before the table's first row.
    pub(crate) fn skip(self) -> usize {
        self.skip
    }

    /// A reader of the records of `input`, every row kept and none taken
    /// for a header, whatever their lengths.
    pub(crate) fn reader<R: Read>(self, input: R) -> csv::Reader<R> {
        csv::ReaderBuilder::new()
            .delimiter(self.separator)
            .quoting(self.quoting)
            .flexible(true)
            .has_headers(false)
            .from_reader(input)
    }

    /// What the separator is called, for the preview's details.
    pub(crate) fn separator_name(self) -> String {
        match self.separator {
            b',' => "Comma".into(),
            b';' => "Semicolon".into(),
            b'\t' => "Tab".into(),
            b'|' => "Pipe".into(),
            b' ' => "Space".into(),
            other => char::from(other).to_string(),
        }
    }
}

/// The separator a CSV's first line names, as Excel reads it (`sep=;`), and
/// the bytes up to the second line. One character alone on the line, and
/// no letter or digit: `sep=a` is more likely a column's name.
fn declared(start: &[u8]) -> Option<(u8, usize)> {
    let line = start.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(start);
    let mark = start.len() - line.len();
    let (name, rest) = line.split_at_checked(4)?;
    if !name.eq_ignore_ascii_case(b"sep=") {
        return None;
    }
    let (&separator, rest) = rest.split_first()?;
    let ending = match rest {
        [] => 0,
        [b'\r', b'\n', ..] => 2,
        [b'\n' | b'\r', ..] => 1,
        _ => return None,
    };
    let usable = matches!(separator, b' ' | b'\t')
        || (separator.is_ascii_punctuation() && separator != b'"');
    usable.then_some((separator, mark + name.len() + 1 + ending))
}

/// What reading `sample` with `separator` is worth, between 0 and 1, or
/// `None` if it makes no table: most rows would be a single cell. The last
/// record is left out where `more` follows, for the sample's end cuts it,
/// unless it is all there is (a first row longer than the sample).
fn score(sample: &[u8], more: bool, separator: u8) -> Option<f64> {
    let mut reader = Dialect {
        separator,
        quoting: true,
        skip: 0,
    }
    .reader(sample);
    let mut tally = Tally::default();
    let mut record = csv::ByteRecord::new();
    let mut last = csv::ByteRecord::new();
    let mut held = false;
    while matches!(reader.read_byte_record(&mut record), Ok(true)) {
        if held {
            tally.add(&last);
        }
        std::mem::swap(&mut record, &mut last);
        held = true;
    }
    if held && (!more || tally.records == 0) {
        tally.add(&last);
    }
    tally.score()
}

/// What a reading made of the rows it read.
#[derive(Default)]
struct Tally {
    /// Rows by how many cells they have.
    lengths: std::collections::HashMap<usize, u32>,
    records: u32,
    cells: u32,
    plain: u32,
}

impl Tally {
    fn add(&mut self, record: &csv::ByteRecord) {
        *self.lengths.entry(record.len()).or_default() += 1;
        self.records += 1;
        for cell in record {
            self.cells += 1;
            self.plain += u32::from(looks_like_a_cell(cell));
        }
    }

    /// The rows of the most common length, as a share of the rows, times
    /// the cells that look like cells, as a share of the cells.
    fn score(&self) -> Option<f64> {
        let (&length, &agreeing) = self.lengths.iter().max_by_key(|(l, n)| (**n, **l))?;
        (length >= 2).then(|| {
            f64::from(agreeing) / f64::from(self.records)
                * (f64::from(self.plain) / f64::from(self.cells))
        })
    }
}

/// Whether a cell reads as one: nothing is left in it of a separator a
/// wrong reading passed over, as in an empty cell, a word, a date or a
/// time, or it is a number written with a comma. A comma or a semicolon
/// before a space, or ending the cell, is a sentence's (`Doe, Ann`), where
/// a wrong reading leaves one against the next cell (`Ann;30`).
fn looks_like_a_cell(cell: &[u8]) -> bool {
    !cell.iter().enumerate().any(|(at, byte)| match byte {
        b'"' | b'\t' | b'|' => true,
        b',' | b';' => cell.get(at + 1).is_some_and(|next| *next != b' '),
        _ => false,
    }) || is_number(cell)
}

/// A number with its decimal mark and grouping as anyone writes them
/// (`3,50`, `1 234,50`, `1.234,50`), an exponent or a short unit after it
/// (`1,5E+03`, `12,50 €`, `3,5 %`). The grouping's spaces are ASCII's,
/// no-break ones in UTF-8 (C2 A0, E2 80 AF) or Windows-1252 (A0).
fn is_number(cell: &[u8]) -> bool {
    let text = cell.trim_ascii();
    let text = text
        .strip_prefix(b"-")
        .or_else(|| text.strip_prefix(b"+"))
        .unwrap_or(text);
    let written = text
        .iter()
        .take_while(|byte| {
            byte.is_ascii_digit()
                || matches!(
                    byte,
                    b'.' | b',' | b'\'' | b' ' | 0xA0 | 0xC2 | 0xE2 | 0x80 | 0xAF
                )
        })
        .count();
    let Some(last_digit) = text[..written].iter().rposition(u8::is_ascii_digit) else {
        return false;
    };
    if !text[0].is_ascii_digit() {
        return false;
    }
    let after = &text[last_digit + 1..];
    if let [b'e' | b'E', power @ ..] = after {
        let digits = power
            .strip_prefix(b"-")
            .or_else(|| power.strip_prefix(b"+"))
            .unwrap_or(power);
        return !digits.is_empty() && digits.iter().all(u8::is_ascii_digit);
    }
    after.len() <= 8
        && !after
            .iter()
            .any(|byte| byte.is_ascii_digit() || *byte == b'"' || SEPARATORS.contains(byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn separator(text: &str) -> char {
        char::from(Dialect::of(false, text.as_bytes(), false).separator)
    }

    /// Semicolons with a comma now and then, in cells or quoted: rows that
    /// agree only one way.
    #[test]
    fn a_semicolon_file_with_a_comma_here_and_there_is_read_by_semicolons() {
        assert_eq!(
            separator("name;street;town\na;1, Rue X;Paris\nb;2 Rue Y;Lyon\nc;3 Rue Z;Nice\n"),
            ';'
        );
        // A comma in every row, and even a name for it in the header.
        assert_eq!(
            separator("name, first;age\nDoe, Ann;30\nRoe, Bob;41\nPoe, Cy;52\n"),
            ';'
        );
        assert_eq!(separator("\"a,b\";\"c\"\n\"d,e\";\"f\"\n"), ';');
    }

    /// With no header to disagree, both readings give two cells a row:
    /// only the cells tell them apart.
    #[test]
    fn decimal_commas_are_read_as_part_of_their_cell() {
        assert_eq!(separator("date;total\n2026-11-01;12,50\n"), ';');
        assert_eq!(separator("foo;1,5\nbar;2,5\n"), ';');
        assert_eq!(separator("foo;1 234,50\nbar;2.345,75\n"), ';');
        assert_eq!(separator("foo;12,50 €\nbar;3,99 €\n"), ';');
        assert_eq!(separator("foo;12,50\u{202f}€\nbar;3,99\u{a0}%\n"), ';');
        assert_eq!(separator("foo;1,5E+03\nbar;2,5e-2\n"), ';');
    }

    #[test]
    fn a_comma_file_stays_one_whatever_its_cells_hold() {
        assert_eq!(separator("id,tags\n1,a;b\n2,c;d\n3,e;f\n"), ',');
        assert_eq!(
            separator("id,note\n1,\"one; two\"\n2,\"three; four\"\n"),
            ','
        );
        // Prices with a point, whole cells of words with pipes.
        assert_eq!(separator("a,b,c\n1.50,x|y,z\n2.50,x|y,z\n"), ',');
    }

    #[test]
    fn tabs_and_pipes_under_the_type_of_commas() {
        assert_eq!(separator("a\tb\tc\n1,5\t2\t3\n4\t5\t6\n"), '\t');
        assert_eq!(separator("a|b|c\n1|2|3\n4|5|6\n"), '|');
    }

    /// A separator inside quotes, a line break too, never counts.
    #[test]
    fn quoted_cells_hold_what_they_like() {
        assert_eq!(
            separator("a;b\n\"x,y\nz,w\n,,,\";1\n\"p,q\nr,s\n,,,\";2\n"),
            ';'
        );
    }

    #[test]
    fn nothing_to_go_on_leaves_the_comma() {
        assert_eq!(separator(""), ',');
        assert_eq!(separator("one column\nof words\n"), ',');
        // One line either way: the type's own.
        assert_eq!(separator("a;b,c\n"), ',');
        // Most rows a single cell: `;` makes no table of it.
        assert_eq!(separator("note\nplain\nwords\nmore; words\n"), ',');
    }

    #[test]
    fn a_first_line_naming_the_separator_is_believed_and_is_no_row() {
        let of = |text: &[u8]| Dialect::of(false, text, false);
        let semi = |skip| Dialect {
            separator: b';',
            quoting: true,
            skip,
        };
        assert_eq!(of(b"sep=;\na,b\n"), semi(6));
        assert_eq!(of(b"SEP=;\r\na,b\r\n"), semi(7));
        assert_eq!(of(b"\xEF\xBB\xBFsep=;\na,b\n"), semi(9));
        assert_eq!(of(b"sep=;"), semi(5));
        // A name, or a line with more on it: an ordinary first row.
        assert_eq!(of(b"sep=a\n1\n").skip(), 0);
        assert_eq!(of(b"sep=;,x\n1\n").skip(), 0);
        assert_eq!(of(b"sep=\"\n1\n").skip(), 0);
        assert_eq!(of("sep=é\n1\n".as_bytes()).skip(), 0);
    }

    /// The end of the sample cuts its last record, which says nothing of
    /// the rows, but for a first row longer than the sample.
    #[test]
    fn a_record_the_sample_cuts_is_left_out() {
        let cut = b"a;b\n1;2\n3";
        assert_eq!(score(cut, true, b';'), Some(1.0));
        assert!(score(cut, false, b';').unwrap() < 1.0);
        // Inside a quoted cell, too.
        let long = format!("a;b\n1;2\n3;\"{}", "x,".repeat(100));
        assert_eq!(score(long.as_bytes(), true, b';'), Some(1.0));
        assert_eq!(Dialect::of(false, long.as_bytes(), true).separator, b';');
        assert!(score(b"a;b;c;d", true, b';').is_some(), "all there is");
    }

    #[test]
    fn a_sample_is_bounded() {
        // Past the sample's bound, a line of other cells says nothing.
        let rows = "a;b\n".repeat(SAMPLE / 4 + 1);
        let text = format!("{rows}{}", "1,2,3\n".repeat(1000));
        assert_eq!(separator(&text), ';');
    }

    #[test]
    fn a_type_of_tabs_is_believed_and_has_no_quotes() {
        let dialect = Dialect::of(true, b"a,b,c\nd,e,f\n", false);
        assert_eq!(dialect.separator, b'\t');
        assert!(!dialect.quoting);
        assert_eq!(dialect.separator_name(), "Tab");
    }

    #[test]
    fn what_a_wrong_reading_leaves_is_told_from_a_sentence() {
        for cell in [
            "",
            "word",
            "2026-11-01",
            "12:30",
            "Doe, Ann",
            "one; two",
            "ends,",
            "3,50",
        ] {
            assert!(looks_like_a_cell(cell.as_bytes()), "{cell:?}");
        }
        for cell in ["Ann;30", "1,a", "x|y", "a | b", "a\tb", "\"a"] {
            assert!(!looks_like_a_cell(cell.as_bytes()), "{cell:?}");
        }
    }

    #[test]
    fn numbers_are_told_from_words() {
        for number in [
            "3,50",
            "-3,5",
            "+1",
            "1 234,50",
            "1.234,50",
            "1,234.50",
            "3,5 %",
            "12,50 €",
            "1,5E+03",
        ] {
            assert!(is_number(number.as_bytes()), "{number}");
        }
        for word in [
            "",
            "a,b",
            "1,",
            ",5",
            "1,2;3",
            "1,5E",
            "12,50 and more",
            "\"1,5\"",
        ] {
            assert!(!is_number(word.as_bytes()), "{word}");
        }
    }
}
