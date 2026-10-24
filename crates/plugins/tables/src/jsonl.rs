//! JSON Lines (`.jsonl`, `.ndjson`): a value per line, a row each. An
//! object's keys are the columns, in the order the page first meets them;
//! any other value, and a line that is not JSON, stands in `value`.
use indexmap::IndexMap;
use serde_json::Value;
use skwir_core::{
    CancelToken,
    inspection::{Inspection, Row},
};
use skwir_platform::inspection::{self, limits};
use std::io::{BufRead, BufReader, Read};

/// The column for what is not an object.
const VALUE: &str = "value";

/// One page of `TABLE_ROWS` lines from line `offset`, streamed from the
/// file like a CSV table: lines before the page are counted, not parsed.
/// Blank lines are no row. Its columns are `bounds`'.
pub fn table(
    path: &std::path::Path,
    offset: u64,
    bounds: limits::Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    use std::io::Seek as _;
    let page = limits::TABLE_ROWS as u64;
    // From the nearest page start known before the page.
    let (mut index, mut at) = crate::starts::nearest(path, offset);
    let mut file = inspection::open(path)?;
    file.seek(std::io::SeekFrom::Start(at))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut parsed = Vec::new();
    let mut more = false;
    loop {
        // Where each page begins, for the next turn.
        if index > 0 && index.is_multiple_of(page) {
            crate::starts::learn(path, index, at);
        }
        line.clear();
        (&mut reader)
            .take(limits::TEXT_INPUT as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if line.is_empty() {
            break;
        }
        at += line.len() as u64;
        anyhow::ensure!(
            line.len() <= limits::TEXT_INPUT,
            "A line exceeds {} preview limit",
            limits::describe(limits::TEXT_INPUT)
        );
        if line.trim_ascii().is_empty() {
            continue;
        }
        if index.is_multiple_of(1024) {
            inspection::check(cancel)?;
        }
        index += 1;
        if index <= offset {
            continue;
        }
        if parsed.len() == limits::TABLE_ROWS {
            more = true;
            break;
        }
        parsed.push(Line::parse(line.trim_ascii()));
    }
    let mut columns: IndexMap<&str, ()> = IndexMap::new();
    for line in &parsed {
        match line {
            Line::Object(fields) => columns.extend(fields.keys().map(|key| (key.as_str(), ()))),
            Line::Other(_) | Line::Invalid(_) => {
                columns.insert(VALUE, ());
            }
        }
    }
    let wide = columns.len() > bounds.table_columns();
    columns.truncate(bounds.table_columns());
    let mut cut = 0;
    let rows = parsed
        .iter()
        .map(|line| Row {
            cells: columns
                .keys()
                .map(|column| {
                    let cell = match line {
                        Line::Object(fields) => fields.get(*column),
                        Line::Other(cell) | Line::Invalid(cell) => {
                            (*column == VALUE).then_some(cell)
                        }
                    };
                    match cell {
                        Some(cell) => {
                            cut += usize::from(cell.cut);
                            cell.text.clone()
                        }
                        None => String::new(),
                    }
                })
                .collect(),
            ..Row::default()
        })
        .collect::<Vec<_>>();
    let invalid = parsed
        .iter()
        .filter(|line| matches!(line, Line::Invalid(_)))
        .count();
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let notice = [
        (invalid > 0).then(|| format!("{invalid} line{} not JSON", plural(invalid))),
        wide.then(|| format!("Only the first {} keys shown", bounds.table_columns())),
        (cut > 0).then(|| {
            format!(
                "{cut} cell{} longer than {} characters shortened",
                plural(cut),
                limits::TABLE_CELL
            )
        }),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let shown = offset + rows.len() as u64;
    let details = inspection::details([
        ("Columns", inspection::count(columns.len())),
        (
            "Rows",
            if more {
                Some(format!("more than {shown}"))
            } else {
                inspection::count(shown as usize)
            },
        ),
    ]);
    Ok(Inspection {
        details,
        kind: skwir_core::inspection::InspectionKind::Table,
        title: if rows.is_empty() {
            "JSON Lines".into()
        } else {
            format!("JSON Lines · rows {}–{shown}", offset + 1)
        },
        columns: columns.keys().map(|key| (*key).to_owned()).collect(),
        rows,
        position: offset,
        previous: (offset > 0).then(|| offset.saturating_sub(page)),
        next: more.then_some(offset + page),
        source_available: true,
        notice,
        whole_available: wide && bounds.cut(),
        ..Inspection::default()
    })
}

/// A line, kept as the cells it shows: a page holds `TABLE_ROWS` lines of
/// up to `TEXT_INPUT` each, which kept parsed (an embedding's array of
/// numbers a line) took gigabytes. Each value is cut as its line is read,
/// as a CSV's cells are.
enum Line {
    /// Its keys in the order the line gives them.
    Object(IndexMap<String, Cell>),
    Other(Cell),
    Invalid(Cell),
}

/// A cell's text, cut to `TABLE_CELL` characters, and whether it was.
struct Cell {
    text: String,
    cut: bool,
}

impl Cell {
    fn of(text: &str) -> Self {
        let mut cut = 0;
        let text = crate::bounded_cell(text, &mut cut);
        Self { text, cut: cut > 0 }
    }
}

impl Line {
    fn parse(line: &[u8]) -> Self {
        // An object is read as one, so that it keeps its keys' order; the
        // value it was is let go of once its cells are made.
        let parsed = if line.first() == Some(&b'{') {
            serde_json::from_slice::<IndexMap<String, Value>>(line).map(|fields| {
                Self::Object(
                    fields
                        .into_iter()
                        .map(|(key, value)| (key, Cell::of(&text(&value))))
                        .collect(),
                )
            })
        } else {
            serde_json::from_slice::<Value>(line).map(|value| Self::Other(Cell::of(&text(&value))))
        };
        parsed.unwrap_or_else(|_| Self::Invalid(Cell::of(&String::from_utf8_lossy(line))))
    }
}

/// A cell: a string as it reads, anything else as compact JSON.
fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(bytes: &str, offset: u64) -> Inspection {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a log’s name.jsonl");
        std::fs::write(&path, bytes).unwrap();
        table(&path, offset, limits::Bounds::QUICK, &CancelToken::new()).unwrap()
    }

    #[test]
    fn keys_are_columns_in_the_order_lines_write_them() {
        let data = read(
            "{\"zeta\": 1, \"alpha\": \"café\"}\n\n{\"alpha\": null, \"nested\": {\"b\": [1, 2]}}\n\"a string\"\nnot json\n",
            0,
        );
        assert_eq!(data.columns, ["zeta", "alpha", "nested", "value"]);
        let cells: Vec<_> = data.rows.iter().map(|row| row.cells.clone()).collect();
        assert_eq!(
            cells,
            [
                ["1", "café", "", ""],
                ["", "null", "{\"b\":[1,2]}", ""],
                ["", "", "", "a string"],
                ["", "", "", "not json"],
            ]
        );
        assert_eq!(data.notice, "1 line not JSON");
        assert_eq!(data.title, "JSON Lines · rows 1–4");
    }

    #[test]
    fn a_long_value_is_cut_as_its_line_is_read() {
        // An embedding a line: kept parsed for the whole page, such lines
        // took gigabytes; each is a cell of its first characters.
        let numbers: Vec<String> = (0..20_000).map(|n| format!("0.{n:05}")).collect();
        let lines = format!(
            "{{\"id\": \"é1\", \"embedding\": [{}]}}\n[{}]\n",
            numbers.join(","),
            numbers.join(",")
        );
        let data = read(&lines, 0);
        assert_eq!(data.columns, ["id", "embedding", "value"]);
        let embedding = &data.rows[0].cells[1];
        assert!(embedding.starts_with("[0.0,0.00001,"), "{embedding}");
        assert_eq!(embedding.chars().count(), limits::TABLE_CELL + 1);
        assert_eq!(
            data.rows[1].cells[2].chars().count(),
            limits::TABLE_CELL + 1
        );
        assert_eq!(
            data.notice,
            format!(
                "2 cells longer than {} characters shortened",
                limits::TABLE_CELL
            )
        );
    }

    #[test]
    fn lines_are_paged_and_counted_without_parsing_those_before() {
        let lines = (0..limits::TABLE_ROWS + 3).fold(String::new(), |mut lines, n| {
            lines.push_str(&serde_json::json!({ "n": n }).to_string());
            lines.push('\n');
            lines
        });
        let first = read(&lines, 0);
        assert_eq!(first.rows.len(), limits::TABLE_ROWS);
        assert_eq!(first.next, Some(limits::TABLE_ROWS as u64));
        let second = read(&lines, limits::TABLE_ROWS as u64);
        assert_eq!(second.rows.len(), 3);
        assert_eq!(second.rows[0].cells, [limits::TABLE_ROWS.to_string()]);
        assert_eq!(second.next, None);
    }
}
