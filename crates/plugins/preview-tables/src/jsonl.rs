//! JSON Lines (`.jsonl`, `.ndjson`): a value per line, a row each. An
//! object's keys are the columns, in the order the page first meets them;
//! any other value, and a line that is not JSON, stands in `value`.
use bibli_core::{
    CancelToken,
    inspection::{Inspection, Row},
};
use bibli_platform::inspection::{self, limits};
use indexmap::IndexMap;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read};

/// The column for what is not an object.
const VALUE: &str = "value";

/// One page of `TABLE_ROWS` lines from line `offset`, streamed from the
/// file like a CSV table: lines before the page are counted, not parsed.
/// Blank lines are no row.
pub fn table(
    path: &std::path::Path,
    offset: u64,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    let mut reader = BufReader::new(inspection::open(path)?);
    let mut line = Vec::new();
    let mut parsed = Vec::new();
    let mut index = 0u64;
    let mut more = false;
    loop {
        line.clear();
        (&mut reader)
            .take(limits::TEXT_INPUT as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if line.is_empty() {
            break;
        }
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
    let wide = columns.len() > limits::TABLE_COLUMNS;
    columns.truncate(limits::TABLE_COLUMNS);
    let mut cut = 0;
    let rows = parsed
        .iter()
        .map(|line| Row {
            cells: columns
                .keys()
                .map(|column| {
                    let cell = match line {
                        Line::Object(fields) => fields.get(*column).map(text),
                        Line::Other(value) => (*column == VALUE).then(|| text(value)),
                        Line::Invalid(raw) => (*column == VALUE).then(|| raw.clone()),
                    };
                    crate::bounded_cell(&cell.unwrap_or_default(), &mut cut)
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
        wide.then(|| format!("Only the first {} keys shown", limits::TABLE_COLUMNS)),
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
    let page = limits::TABLE_ROWS as u64;
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
        kind: bibli_core::inspection::InspectionKind::Table,
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
        ..Inspection::default()
    })
}

enum Line {
    /// Its keys in the order the line gives them.
    Object(IndexMap<String, Value>),
    Other(Value),
    Invalid(String),
}

impl Line {
    fn parse(line: &[u8]) -> Self {
        /// An object is tried first, so that it keeps its keys' order.
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Parsed {
            Object(IndexMap<String, Value>),
            Other(Value),
        }
        match serde_json::from_slice(line) {
            Ok(Parsed::Object(fields)) => Self::Object(fields),
            Ok(Parsed::Other(value)) => Self::Other(value),
            Err(_) => Self::Invalid(String::from_utf8_lossy(line).into_owned()),
        }
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
        table(&path, offset, &CancelToken::new()).unwrap()
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
