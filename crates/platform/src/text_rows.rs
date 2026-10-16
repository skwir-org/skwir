//! A text file read where it is shown (ADR-051): cut into rows, its rows
//! counted once into an index that keeps every `STRIDE`th, and read a few
//! at a time from the nearest. Nothing here holds the file.
//!
//! A row ends with its line, or after `ROW_BYTES` at a character boundary:
//! a minified file is many rows of one line, and no row is unbounded. The
//! encoding is decided once, from the file's first bytes (`Form::of`), and
//! UTF-16 is cut in its own units.

use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::Path;

use parking_lot::RwLock;
use skwir_core::CancelToken;

use crate::inspection::{Encoding, check, limits, open};

/// Rows between two places the index keeps; a chunk the view asks for.
pub const STRIDE: u64 = 256;
/// The longest a row is: a longer line is cut into several.
pub const ROW_BYTES: usize = limits::LINE_BYTES;
/// Bytes read at a time while counting.
const BLOCK: usize = 1 << 20;
/// Bytes read at a time for a few rows.
const PIECE: usize = 64 * 1024;
/// What the encoding is decided from.
const HEAD: usize = 64 * 1024;

/// How a file's bytes are cut into rows: its encoding, decided once, and
/// where its text starts (after a byte order mark).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Form {
    pub encoding: Encoding,
    pub start: u64,
}

impl Form {
    /// The form of a file that begins with `head`: its mark, else UTF-8
    /// (cut through its last character or not), else Windows-1252, which
    /// reads any byte. Later bytes that are not UTF-8 read as U+FFFD.
    pub fn of(head: &[u8]) -> Form {
        if let Some((encoding, mark)) = encoding_rs::Encoding::for_bom(head) {
            let encoding = if encoding == encoding_rs::UTF_16LE {
                Encoding::Utf16Le
            } else if encoding == encoding_rs::UTF_16BE {
                Encoding::Utf16Be
            } else {
                Encoding::Utf8
            };
            return Form {
                encoding,
                start: mark as u64,
            };
        }
        let utf8 = match std::str::from_utf8(head) {
            Ok(_) => true,
            Err(error) => error.error_len().is_none(),
        };
        Form {
            encoding: if utf8 {
                Encoding::Utf8
            } else {
                Encoding::Windows1252
            },
            start: 0,
        }
    }

    /// The form of the file at `path`, from its first bytes.
    pub fn read(path: &Path, cancel: &CancelToken) -> anyhow::Result<Form> {
        Ok(Form::of(&crate::inspection::read(path, 0, HEAD, cancel)?))
    }

    fn wide(self) -> bool {
        matches!(self.encoding, Encoding::Utf16Le | Encoding::Utf16Be)
    }

    /// The text of a row's bytes.
    pub fn decode(self, bytes: &[u8]) -> String {
        self.encoding.decode(bytes).into_owned()
    }
}

/// Where a row that starts at `bytes[0]` ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cut {
    /// Its text's length, without the line's ending.
    text: usize,
    /// Where the next row starts.
    next: usize,
    /// Whether the row ends its line (else the line goes on in the next).
    ends_line: bool,
}

/// Where the row that starts at `bytes[0]` ends, or `None` when `bytes`
/// may not hold all of it and more follow (`last` false) or nothing is
/// left. `bytes` holds at least `ROW_BYTES` and a line's ending, unless it
/// is the file's end.
fn cut(form: Form, bytes: &[u8], last: bool) -> Option<Cut> {
    if bytes.is_empty() {
        return None;
    }
    if form.wide() {
        return cut_wide(form, bytes, last);
    }
    let window = &bytes[..bytes.len().min(ROW_BYTES + 1)];
    if let Some(at) = memchr::memchr(b'\n', window) {
        let text = if at > 0 && bytes[at - 1] == b'\r' {
            at - 1
        } else {
            at
        };
        return Some(Cut {
            text,
            next: at + 1,
            ends_line: true,
        });
    }
    if bytes.len() > ROW_BYTES {
        // Never inside a character: back over at most three continuation
        // bytes of UTF-8 (a code page's byte is a character).
        let mut at = ROW_BYTES;
        if form.encoding == Encoding::Utf8 {
            while at > ROW_BYTES - 3 && bytes[at] & 0xC0 == 0x80 {
                at -= 1;
            }
        }
        return Some(Cut {
            text: at,
            next: at,
            ends_line: false,
        });
    }
    last.then(|| Cut {
        text: bytes.len() - usize::from(bytes.ends_with(b"\r")),
        next: bytes.len(),
        ends_line: true,
    })
}

/// `cut` in UTF-16: a line ends at a newline unit, and a row is never cut
/// between the two halves of a surrogate pair.
fn cut_wide(form: Form, bytes: &[u8], last: bool) -> Option<Cut> {
    let unit = |at: usize| -> u16 {
        let pair = [bytes[at], bytes[at + 1]];
        if form.encoding == Encoding::Utf16Le {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    };
    let whole = bytes.len() & !1;
    let window = whole.min(ROW_BYTES + 2);
    for at in (0..window).step_by(2) {
        if unit(at) == u16::from(b'\n') {
            let text = if at >= 2 && unit(at - 2) == u16::from(b'\r') {
                at - 2
            } else {
                at
            };
            return Some(Cut {
                text,
                next: at + 2,
                ends_line: true,
            });
        }
    }
    if whole > ROW_BYTES {
        let mut at = ROW_BYTES;
        if (0xD800..0xDC00).contains(&unit(at - 2)) {
            at -= 2;
        }
        return Some(Cut {
            text: at,
            next: at,
            ends_line: false,
        });
    }
    (last && whole > 0).then(|| {
        let text = if whole >= 2 && unit(whole - 2) == u16::from(b'\r') {
            whole - 2
        } else {
            whole
        };
        Cut {
            text,
            next: bytes.len(),
            ends_line: true,
        }
    })
}

/// A file's rows one after another, from a row's start: the one cutter,
/// so counting and reading always cut the same rows.
struct Cutter<R> {
    reader: R,
    form: Form,
    buf: Vec<u8>,
    pos: usize,
    /// Where `buf[pos]` is in the file.
    offset: u64,
    eof: bool,
    /// What is read at a time.
    piece: usize,
}

/// A row the cutter found: where it starts, its bytes in the cutter's
/// buffer, and whether it ends its line.
struct Found {
    start: u64,
    bytes: Range<usize>,
    ends_line: bool,
}

impl<R: Read> Cutter<R> {
    fn new(reader: R, form: Form, offset: u64, piece: usize) -> Self {
        Cutter {
            reader,
            form,
            buf: Vec::new(),
            pos: 0,
            offset,
            eof: false,
            piece,
        }
    }

    /// The next row, `None` at the end of the file.
    fn next(&mut self) -> std::io::Result<Option<Found>> {
        loop {
            let rest = &self.buf[self.pos..];
            // A row and its ending are in the buffer, or the file ends.
            if (self.eof || rest.len() > ROW_BYTES + 2)
                && let Some(cut) = cut(self.form, rest, self.eof)
            {
                let found = Found {
                    start: self.offset,
                    bytes: self.pos..self.pos + cut.text,
                    ends_line: cut.ends_line,
                };
                self.pos += cut.next;
                self.offset += cut.next as u64;
                return Ok(Some(found));
            }
            if self.eof {
                return Ok(None);
            }
            self.buf.drain(..self.pos);
            self.pos = 0;
            let had = self.buf.len();
            self.buf.resize(had + self.piece, 0);
            let read = loop {
                match self.reader.read(&mut self.buf[had..]) {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    read => break read?,
                }
            };
            self.buf.truncate(had + read);
            self.eof = read == 0;
        }
    }
}

/// Where row `k × STRIDE` starts: its offset, the line it is part of (from
/// 0), and whether it begins that line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub offset: u64,
    pub line: u64,
    pub first: bool,
}

/// The rows of a file, counted (`count`): every `STRIDE`th row's place, and
/// how many rows and lines there are so far.
#[derive(Clone, Debug)]
pub struct Index {
    pub form: Form,
    points: Vec<Point>,
    rows: u64,
    lines: u64,
    done: bool,
}

impl Index {
    /// Nothing counted yet of a file of form `form`.
    pub fn new(form: Form) -> Self {
        Index {
            form,
            points: Vec::new(),
            rows: 0,
            lines: 0,
            done: false,
        }
    }

    /// The rows counted so far.
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// The lines begun so far.
    pub fn lines(&self) -> u64 {
        self.lines
    }

    /// Whether every row is counted.
    pub fn done(&self) -> bool {
        self.done
    }

    /// Where chunk `chunk` (rows `chunk × STRIDE` on) starts, once counted.
    pub fn point(&self, chunk: u64) -> Option<Point> {
        usize::try_from(chunk)
            .ok()
            .and_then(|chunk| self.points.get(chunk))
            .copied()
    }

    /// The chunk whose place is the last at or before the first row of
    /// line `line` (from 0): a place inside the line comes after it.
    fn chunk_of_line(&self, line: u64) -> Option<u64> {
        let after = self
            .points
            .partition_point(|point| point.line < line || (point.line == line && point.first));
        (after > 0).then(|| after as u64 - 1)
    }
}

/// Count the rows of the file at `path` into `index`, from nothing, a block
/// at a time: what each block adds is published under the lock, then
/// `progress` is told. Cancelled, the index stays as far as it got.
pub fn count(
    path: &Path,
    index: &RwLock<Index>,
    cancel: &CancelToken,
    mut progress: impl FnMut(),
) -> anyhow::Result<()> {
    // Read again: the file may have been written anew.
    let form = Form::read(path, cancel)?;
    let mut file = open(path)?;
    file.seek(SeekFrom::Start(form.start))?;
    let mut cutter = Cutter::new(file, form, form.start, BLOCK);
    *index.write() = Index::new(form);
    let (mut rows, mut lines, mut first) = (0u64, 0u64, true);
    let mut points = Vec::new();
    let mut reported = form.start;
    loop {
        let found = cutter.next()?;
        if let Some(found) = &found {
            if rows.is_multiple_of(STRIDE) {
                points.push(Point {
                    offset: found.start,
                    line: lines.saturating_sub(u64::from(!first)),
                    first,
                });
            }
            rows += 1;
            if first {
                lines += 1;
            }
            first = found.ends_line;
        }
        // Published a block at a time, and at the end.
        if found.is_none() || cutter.offset - reported >= BLOCK as u64 {
            check(cancel)?;
            reported = cutter.offset;
            let mut shared = index.write();
            shared.points.append(&mut points);
            shared.rows = rows;
            shared.lines = lines;
            shared.done = found.is_none();
            drop(shared);
            progress();
        }
        if found.is_none() {
            return Ok(());
        }
    }
}

/// A row as read: its text, the line it is part of (from 0), and whether
/// it begins that line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub text: String,
    pub line: u64,
    pub first: bool,
}

/// Rows `rows` of the file at `path`, read from `point`, the place of row
/// `at` (the chunk's first), up to the file's end.
pub fn read(
    path: &Path,
    form: Form,
    (at, point): (u64, Point),
    rows: Range<u64>,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<Row>> {
    let mut file = open(path)?;
    file.seek(SeekFrom::Start(point.offset))?;
    let mut cutter = Cutter::new(file, form, point.offset, PIECE);
    let mut out = Vec::new();
    let (mut row, mut line, mut first) = (at, point.line, point.first);
    while row < rows.end {
        if (row - at).is_multiple_of(STRIDE) {
            check(cancel)?;
        }
        let Some(found) = cutter.next()? else {
            break;
        };
        if row >= rows.start {
            out.push(Row {
                text: form.decode(&cutter.buf[found.bytes]),
                line,
                first,
            });
        }
        if found.ends_line {
            line += 1;
        }
        first = found.ends_line;
        row += 1;
    }
    Ok(out)
}

/// The place of the first row, which needs no index.
pub fn origin(form: Form) -> Point {
    Point {
        offset: form.start,
        line: 0,
        first: true,
    }
}

/// The row that begins line `line` (from 0) of the file, once the index
/// has counted past it.
pub fn row_of_line(
    path: &Path,
    index: &RwLock<Index>,
    line: u64,
    cancel: &CancelToken,
) -> anyhow::Result<Option<u64>> {
    let (form, chunk, point) = {
        let index = index.read();
        let Some(chunk) = index.chunk_of_line(line) else {
            return Ok(None);
        };
        (index.form, chunk, index.point(chunk))
    };
    let Some(point) = point else {
        return Ok(None);
    };
    let at = chunk * STRIDE;
    let mut file = open(path)?;
    file.seek(SeekFrom::Start(point.offset))?;
    let mut cutter = Cutter::new(file, form, point.offset, PIECE);
    let (mut row, mut current, mut first) = (at, point.line, point.first);
    while let Some(found) = cutter.next()? {
        if (row - at).is_multiple_of(STRIDE) {
            check(cancel)?;
        }
        if first && current == line {
            return Ok(Some(row));
        }
        if current > line {
            break;
        }
        if found.ends_line {
            current += 1;
        }
        first = found.ends_line;
        row += 1;
    }
    Ok(None)
}

/// `text` lowercase, a character at a time: what a find compares. A
/// character's lowercase may be longer than it ('İ'), so offsets in it are
/// not the text's (`matches` maps them back).
pub fn fold(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Where `query` is in `text`, case ignored (both `fold`ed): the ranges of
/// `text` it covers, in order, none overlapping. A match that ends inside
/// what a character lowercases to covers the character.
pub fn matches(text: &str, query: &str) -> Vec<Range<usize>> {
    let query = fold(query);
    if query.is_empty() {
        return Vec::new();
    }
    // ASCII lowercases byte for byte: its offsets are the text's.
    if text.is_ascii() {
        let lower = text.to_ascii_lowercase();
        return lower
            .match_indices(query.as_str())
            .map(|(at, found)| at..at + found.len())
            .collect();
    }
    let lower = fold(text);
    if !lower.contains(query.as_str()) {
        return Vec::new();
    }
    // Each byte of `lower`: where the character it came from starts.
    let mut origin = Vec::with_capacity(lower.len());
    for (at, c) in text.char_indices() {
        let wide: usize = c.to_lowercase().map(char::len_utf8).sum();
        origin.extend(std::iter::repeat_n(at, wide));
    }
    lower
        .match_indices(query.as_str())
        .map(|(at, found)| {
            let last = origin[at + found.len() - 1];
            let end = last + text[last..].chars().next().map_or(0, char::len_utf8);
            origin[at]..end
        })
        .collect()
}

/// The first match of `query` (case ignored, `matches`) at or after `from`
/// (row, byte in the row's text), going on from the start once the end is
/// passed, as far as `from`; or the last before `from`, going on from the
/// end, when `back`. Its row and where it is in the row's text. Only the
/// rows counted are searched, each on its own: a match is within a row.
pub fn find(
    path: &Path,
    index: &RwLock<Index>,
    query: &str,
    from: (u64, usize),
    back: bool,
    cancel: &CancelToken,
) -> anyhow::Result<Option<(u64, Range<usize>)>> {
    let (form, rows) = {
        let index = index.read();
        (index.form, index.rows)
    };
    if fold(query).is_empty() || rows == 0 {
        return Ok(None);
    }
    let from = if from.0 >= rows { (rows, 0) } else { from };
    // The matches of a row that come at or after `from`, or before it.
    let after = |row: u64, text: &str, later: bool| -> Vec<Range<usize>> {
        let mut found = matches(text, query);
        found.retain(|range| ((row, range.start) >= from) == later);
        found
    };
    let place = |chunk: u64| -> Option<(u64, Point)> {
        let point = index.read().point(chunk)?;
        Some((chunk * STRIDE, point))
    };
    if !back {
        // Forward: `from` to the end, then the start up to `from`.
        for (span, later) in [(from.0..rows, true), (0..(from.0 + 1).min(rows), false)] {
            if span.is_empty() {
                continue;
            }
            let Some((at, point)) = place(span.start / STRIDE) else {
                continue;
            };
            let mut file = open(path)?;
            file.seek(SeekFrom::Start(point.offset))?;
            let mut cutter = Cutter::new(file, form, point.offset, BLOCK);
            let mut row = at;
            while row < span.end {
                if (row - at).is_multiple_of(STRIDE) {
                    check(cancel)?;
                }
                let Some(found) = cutter.next()? else {
                    break;
                };
                if row >= span.start
                    && let Some(range) = after(row, &form.decode(&cutter.buf[found.bytes]), later)
                        .into_iter()
                        .next()
                {
                    return Ok(Some((row, range)));
                }
                row += 1;
            }
        }
        return Ok(None);
    }
    // Back: from `from` toward the start, then from the end down to
    // `from`, a chunk at a time.
    for (span, later) in [(0..(from.0 + 1).min(rows), false), (from.0..rows, true)] {
        if span.is_empty() {
            continue;
        }
        for chunk in (span.start / STRIDE..=(span.end - 1) / STRIDE).rev() {
            check(cancel)?;
            let Some((at, point)) = place(chunk) else {
                continue;
            };
            let wanted = at.max(span.start)..(at + STRIDE).min(span.end);
            let read = read(path, form, (at, point), wanted.clone(), cancel)?;
            for (n, text) in read.iter().enumerate().rev() {
                let row = wanted.start + n as u64;
                if let Some(range) = after(row, &text.text, later).pop() {
                    return Ok(Some((row, range)));
                }
            }
        }
    }
    Ok(None)
}

/// The text from `from` to `to`, (row, byte in the row's text), as the
/// file has it: rows of one line joined, lines joined by a newline, up to
/// `limit` bytes. Whether it stopped at the limit.
pub fn text(
    path: &Path,
    index: &RwLock<Index>,
    from: (u64, usize),
    to: (u64, usize),
    limit: usize,
    cancel: &CancelToken,
) -> anyhow::Result<(String, bool)> {
    let (form, point) = {
        let index = index.read();
        (index.form, index.point(from.0 / STRIDE))
    };
    let Some(point) = point else {
        return Ok((String::new(), false));
    };
    let at = from.0 / STRIDE * STRIDE;
    let mut file = open(path)?;
    file.seek(SeekFrom::Start(point.offset))?;
    let mut cutter = Cutter::new(file, form, point.offset, BLOCK);
    let mut out = String::new();
    let (mut row, mut first) = (at, point.first);
    // Within a row's text, at a character.
    let within = |text: &str, mut at: usize| {
        at = at.min(text.len());
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    while row <= to.0 {
        if (row - at).is_multiple_of(STRIDE) {
            check(cancel)?;
        }
        let Some(found) = cutter.next()? else {
            break;
        };
        if row >= from.0 {
            if first && row > from.0 {
                out.push('\n');
            }
            let text = form.decode(&cutter.buf[found.bytes]);
            let end = if row == to.0 {
                within(&text, to.1)
            } else {
                text.len()
            };
            let start = if row == from.0 {
                within(&text, from.1).min(end)
            } else {
                0
            };
            out.push_str(&text[start..end]);
            if out.len() >= limit {
                let end = within(&out, limit);
                out.truncate(end);
                return Ok((out, true));
            }
        }
        first = found.ends_line;
        row += 1;
    }
    Ok((out, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form {
        Form {
            encoding: Encoding::Utf8,
            start: 0,
        }
    }

    /// Every row of `bytes`, cut as the cutter cuts them, read in pieces of
    /// `piece` bytes.
    fn rows_of(form: Form, bytes: &[u8], piece: usize) -> Vec<(String, bool)> {
        let start = form.start as usize;
        let mut cutter = Cutter::new(&bytes[start..], form, form.start, piece);
        let mut rows = Vec::new();
        while let Some(found) = cutter.next().unwrap() {
            rows.push((form.decode(&cutter.buf[found.bytes]), found.ends_line));
        }
        rows
    }

    #[test]
    fn a_row_is_a_line_or_a_bounded_part_of_one() {
        let rows = rows_of(form(), b"one\r\ntwo\n\nlast", 3);
        assert_eq!(
            rows,
            [
                ("one".into(), true),
                ("two".into(), true),
                (String::new(), true),
                ("last".into(), true)
            ]
        );
        assert!(rows_of(form(), b"", 7).is_empty());
        assert_eq!(
            rows_of(form(), b"a\n", 7),
            [("a".into(), true)],
            "no row after the last newline"
        );
        // A line longer than a row is several, never cut inside a character.
        let long = format!("{}é{}\nnext\n", "x".repeat(ROW_BYTES - 1), "y".repeat(10));
        let rows = rows_of(form(), long.as_bytes(), 1000);
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].0.len(), rows[0].1), (ROW_BYTES - 1, false));
        assert_eq!(rows[1], (format!("é{}", "y".repeat(10)), true));
        assert_eq!(rows[2].0, "next");
        // Exactly a row long, then its newline: one row.
        let exact = format!("{}\nz", "x".repeat(ROW_BYTES));
        assert_eq!(rows_of(form(), exact.as_bytes(), 64).len(), 2);
    }

    #[test]
    fn utf16_is_cut_in_its_own_units() {
        let text = "un\r\nd\u{e9}ux\n\u{1F600}";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let form = Form::of(&bytes);
        assert_eq!(
            form,
            Form {
                encoding: Encoding::Utf16Le,
                start: 2
            }
        );
        let rows: Vec<String> = rows_of(form, &bytes, 5).into_iter().map(|r| r.0).collect();
        assert_eq!(rows, ["un", "d\u{e9}ux", "\u{1F600}"]);
        // A long line is never cut between the halves of a pair.
        let long: String = "\u{1F600}".repeat(ROW_BYTES / 2);
        let mut bytes = vec![0xFE, 0xFF];
        bytes.extend(long.encode_utf16().flat_map(u16::to_be_bytes));
        let rows = rows_of(Form::of(&bytes), &bytes, 4096);
        assert_eq!(rows.iter().map(|r| r.0.as_str()).collect::<String>(), long);
        assert!(rows.iter().all(|r| !r.0.contains('\u{FFFD}')));
    }

    #[test]
    fn the_form_is_its_mark_else_utf8_else_the_code_page() {
        assert_eq!(Form::of(b"plain").encoding, Encoding::Utf8);
        // Cut through its last character: still UTF-8.
        assert_eq!(
            Form::of(&"caf\u{e9}".as_bytes()[..4]).encoding,
            Encoding::Utf8
        );
        assert_eq!(Form::of(b"caf\xe9;").encoding, Encoding::Windows1252);
        assert_eq!(
            Form::of(b"\xEF\xBB\xBFtext"),
            Form {
                encoding: Encoding::Utf8,
                start: 3
            }
        );
    }

    /// A file of `lines` lines, the line before every hundredth longer
    /// than a row, under a name with an apostrophe and an accent.
    fn file(lines: usize) -> (tempfile::TempDir, std::path::PathBuf, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("l'été.log");
        let text: Vec<String> = (0..lines)
            .map(|i| {
                if i % 100 == 99 {
                    format!("{i} {}", "é".repeat(ROW_BYTES))
                } else {
                    format!("{i} ligne")
                }
            })
            .collect();
        std::fs::write(&path, text.join("\n") + "\n").unwrap();
        (dir, path, text)
    }

    fn counted(path: &Path) -> RwLock<Index> {
        let cancel = CancelToken::new();
        let index = RwLock::new(Index::new(Form::read(path, &cancel).unwrap()));
        let mut told = 0;
        count(path, &index, &cancel, || told += 1).unwrap();
        assert!(told >= 1);
        index
    }

    #[test]
    fn counted_rows_are_read_from_the_nearest_place_with_their_lines() {
        let (_dir, path, lines) = file(2000);
        let index = counted(&path);
        let index_now = index.read().clone();
        assert!(index_now.done());
        assert_eq!(index_now.lines(), 2000);
        // Every hundredth line holds two bytes a character: three rows.
        assert_eq!(index_now.rows(), 2000 + 20 * 2);
        let cancel = CancelToken::new();
        // A chunk read from its place: the same rows as read from the start.
        let whole = read(
            &path,
            index_now.form,
            (0, origin(index_now.form)),
            0..index_now.rows(),
            &cancel,
        )
        .unwrap();
        for chunk in 0..=(index_now.rows() - 1) / STRIDE {
            let at = chunk * STRIDE;
            let rows = read(
                &path,
                index_now.form,
                (at, index_now.point(chunk).unwrap()),
                at..at + STRIDE,
                &cancel,
            )
            .unwrap();
            assert_eq!(rows[..], whole[at as usize..(at as usize + rows.len())]);
        }
        // A line's number is on its first row; its rows join back into it.
        let line = &whole.iter().filter(|r| r.line == 199).collect::<Vec<_>>();
        assert_eq!(line.len(), 3);
        assert!(line[0].first && !line[1].first && !line[2].first);
        assert_eq!(
            line.iter().map(|r| r.text.as_str()).collect::<String>(),
            lines[199]
        );
        assert_eq!(
            whole.last().unwrap().line,
            1999,
            "the last line, a long one"
        );
        // A line's first row, found by its number.
        let row = row_of_line(&path, &index, 1500, &cancel).unwrap().unwrap();
        assert_eq!(whole[row as usize].line, 1500);
        assert!(whole[row as usize].first);
        assert_eq!(row_of_line(&path, &index, 5000, &cancel).unwrap(), None);
    }

    #[test]
    fn find_goes_on_around_the_file_either_way() {
        let (_dir, path, _) = file(1000);
        let index = counted(&path);
        let cancel = CancelToken::new();
        let row_of = |line: u64| row_of_line(&path, &index, line, &cancel).unwrap().unwrap();
        let find = |query: &str, from: u64, back: bool| {
            find(&path, &index, query, (from, 0), back, &cancel)
                .unwrap()
                .map(|(row, _)| row)
        };
        // "123 LIGNE": lowercase on both sides.
        assert_eq!(find("123 LIGNE", 0, false), Some(row_of(123)));
        assert_eq!(
            find("123 ligne", row_of(123) + 1, false),
            Some(row_of(123)),
            "around"
        );
        assert_eq!(find("5 ligne", row_of(900), false), Some(row_of(905)));
        assert_eq!(find("5 ligne", row_of(900), true), Some(row_of(895)));
        assert_eq!(
            find("1 ligne", row_of(0) + 1, true),
            Some(row_of(991)),
            "back around the end"
        );
        assert_eq!(find("absent", 400, false), None);
        assert_eq!(find("absent", 400, true), None);
    }

    #[test]
    fn find_goes_from_match_to_match_within_a_row() {
        // One long line, cut into rows: "needle" four times in the first
        // row, then once in the next line.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("one line.min.js");
        let mut line = "x".repeat(ROW_BYTES * 2);
        for at in [10, 200, 900, 3000] {
            line.replace_range(at..at + 6, "Needle");
        }
        std::fs::write(&path, format!("{line}\nneedle\n")).unwrap();
        let index = counted(&path);
        let cancel = CancelToken::new();
        let find = |from: (u64, usize), back: bool| {
            find(&path, &index, "NEEDLE", from, back, &cancel).unwrap()
        };
        assert_eq!(find((0, 0), false), Some((0, 10..16)));
        assert_eq!(find((0, 11), false), Some((0, 200..206)), "the next");
        assert_eq!(find((0, 3001), false), Some((2, 0..6)), "the next row");
        assert_eq!(find((2, 1), false), Some((0, 10..16)), "around");
        assert_eq!(find((0, 900), true), Some((0, 200..206)), "the one before");
        assert_eq!(find((0, 10), true), Some((2, 0..6)), "back around");
        assert_eq!(find((2, 0), true), Some((0, 3000..3006)));
    }

    #[test]
    fn a_match_is_where_it_is_in_the_text_whatever_its_case() {
        assert_eq!(matches("A needle, a NEEDLE", "needle"), [2..8, 12..18]);
        assert_eq!(matches("Été, ÉTÉ", "été"), [0..5, 7..12]);
        // 'İ' lowercases to two characters: a match ending in them covers
        // it, and what follows is where it is in the text.
        assert_eq!(matches("İx İ", "i"), [0..2, 4..6]);
        assert_eq!(matches("İx İx", "x"), [2..3, 6..7]);
        // The Kelvin sign lowercases to an ASCII 'k'.
        assert_eq!(matches("\u{212A}m \u{212A}m", "km"), [0..4, 5..9]);
        assert!(matches("needle", "").is_empty());
        assert!(matches("needle", "pin").is_empty());
    }

    #[test]
    fn copied_text_joins_a_lines_rows_and_stops_at_its_limit() {
        let (_dir, path, lines) = file(300);
        let index = counted(&path);
        let cancel = CancelToken::new();
        let last = index.read().rows() - 1;
        let joined = lines.join("\n");
        let (all, cut) = text(
            &path,
            &index,
            (0, 0),
            (last, usize::MAX),
            usize::MAX,
            &cancel,
        )
        .unwrap();
        assert!(!cut);
        assert_eq!(all, joined);
        let (part, cut) = text(&path, &index, (0, 0), (last, usize::MAX), 10, &cancel).unwrap();
        assert!(cut);
        assert_eq!(part, joined[..10]);
        // From inside a row to inside another, at characters.
        let (some, _) = text(&path, &index, (1, 2), (2, 3), usize::MAX, &cancel).unwrap();
        assert_eq!(some, "ligne\n2 l");
    }

    #[test]
    fn a_file_counted_again_counts_from_nothing() {
        let (dir, path, _) = file(10);
        let index = counted(&path);
        assert_eq!(index.read().lines(), 10);
        std::fs::write(dir.path().join("l'été.log"), "a\nb\n").unwrap();
        count(&path, &index, &CancelToken::new(), || {}).unwrap();
        assert_eq!((index.read().rows(), index.read().lines()), (2, 2));
    }
}
