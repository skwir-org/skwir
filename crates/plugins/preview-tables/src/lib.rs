//! CSV/TSV, JSON Lines, spreadsheet and SQLite tables streamed from the file
//! a page at a time, every cell bounded.
use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    settings::SettingSpec,
};
use bibli_platform::inspection::{self, Encoding, limits};
use std::{io::Read, sync::Arc};

const ID: &str = "preview-tables";
/// How a spreadsheet is shown: its cells as a table, or its pages as
/// `LibreOffice` prints them.
const SPREADSHEETS: &str = "preview-tables.spreadsheets";

pub struct PreviewPlugin;
impl PreviewPlugin {
    /// The provider, with the spreadsheet presentation the settings name.
    fn provider(app: &App) {
        let pages = app.settings.get::<String>(SPREADSHEETS).as_deref() == Some("pages");
        app.previews.unregister(ID, ID);
        app.previews.register(ID, Arc::new(Provider { pages }));
    }
}
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Preview: tables"
    }
    fn description(&self) -> &'static str {
        "Show CSV, TSV and JSON Lines files, spreadsheets and SQLite databases as aligned tables"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn capabilities(&self) -> bibli_core::permissions::Capabilities {
        use bibli_core::permissions::Capability;
        bibli_core::permissions::Capabilities::of(&[Capability::Read])
    }
    fn register(&self, app: &App) {
        app.settings.declare(SettingSpec::choice(
            SPREADSHEETS,
            "Spreadsheets",
            "Excel workbooks (.xlsx, .xlsm, .xlsb) show their cells as a table, a sheet after another. Pages shows them as LibreOffice prints them, when it is installed.",
            "table",
            &[("table", "Table"), ("pages", "Pages")],
        ));
        Self::provider(app);
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key == SPREADSHEETS) {
            Self::provider(app);
        }
    }
}
mod jsonl;
mod spreadsheet;
mod sqlite;

struct Provider {
    /// Spreadsheets are `LibreOffice`'s to draw as pages, when it is there.
    pages: bool,
}
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        ID
    }
    /// Before the document reader, which also claims spreadsheets.
    fn priority(&self) -> i32 {
        10
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        let extension = inspection::extension(&req.entry.path);
        if spreadsheet::EXTENSIONS.contains(&extension.as_str()) {
            return !(self.pages
                && bibli_platform::preview::shown_as_pages(&req.entry.path, req.mime.as_deref()));
        }
        matches!(extension.as_str(), "csv" | "tsv" | "jsonl" | "ndjson")
            || sqlite::EXTENSIONS.contains(&extension.as_str())
            || req
                .mime
                .as_deref()
                .is_some_and(|mime| sqlite::MIMES.contains(&mime))
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        // The source is the shared code view (the worker's fallback).
        if view.source {
            return Ok(None);
        }
        let path = req.entry.resolve()?;
        let extension = inspection::extension(&path);
        let database = req
            .mime
            .as_deref()
            .is_some_and(|mime| sqlite::MIMES.contains(&mime));
        match extension.as_str() {
            _ if database || sqlite::EXTENSIONS.contains(&extension.as_str()) => {
                sqlite::table(&path, view.offset, cancel).map(Some)
            }
            "jsonl" | "ndjson" => jsonl::table(&path, view.offset, cancel).map(Some),
            _ if spreadsheet::EXTENSIONS.contains(&extension.as_str()) => {
                spreadsheet::table(&path, view.offset, cancel).map(Some)
            }
            _ => table(&path, view.offset, cancel).map(Some),
        }
    }
}

/// What reading the start of a table says about its bytes. A mark decides;
/// otherwise text that is UTF-8 as far as it goes is UTF-8 (the prefix may
/// cut a character), and anything else is the legacy code page.
fn sniff(prefix: &[u8]) -> Encoding {
    match inspection::decode(prefix).1 {
        Encoding::Windows1252
            if std::str::from_utf8(prefix).is_err_and(|e| e.error_len().is_none()) =>
        {
            Encoding::Utf8
        }
        encoding => encoding,
    }
}

/// One page of `TABLE_ROWS` records from record `offset`, streamed from the
/// file: a table is as long as its file, and only the page is kept. Rows
/// before the page are parsed and dropped, so numbering stays exact.
fn table(path: &std::path::Path, offset: u64, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    let mut file = inspection::open(path)?;
    let mut prefix = Vec::new();
    (&mut file).take(64 * 1024).read_to_end(&mut prefix)?;
    let encoding = sniff(&prefix);
    // The CSV reader reads bytes that are ASCII-compatible; UTF-16 is not,
    // so it is decoded whole, within the bound for text read whole.
    let input: Box<dyn Read> = match encoding {
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let (text, _) = inspection::read_text(path, cancel)?;
            Box::new(std::io::Cursor::new(text.into_bytes()))
        }
        _ => Box::new(std::io::Cursor::new(prefix).chain(file)),
    };
    let cells_in = match encoding {
        Encoding::Windows1252 => Encoding::Windows1252,
        _ => Encoding::Utf8,
    };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(if inspection::extension(path) == "tsv" {
            b'\t'
        } else {
            b','
        })
        .flexible(true)
        .has_headers(false)
        .from_reader(input);
    let mut bounds = Bounds::default();
    let mut record = csv::ByteRecord::new();
    let columns = if reader.read_byte_record(&mut record)? {
        bounds.row(&record, cells_in)
    } else {
        Vec::new()
    };
    let mut rows = Vec::new();
    let mut index = 0u64;
    let mut more = false;
    while reader.read_byte_record(&mut record)? {
        if index.is_multiple_of(1024) {
            inspection::check(cancel)?;
        }
        index += 1;
        if index <= offset {
            continue;
        }
        if rows.len() == limits::TABLE_ROWS {
            more = true;
            break;
        }
        rows.push(Row {
            cells: bounds.row(&record, cells_in),
            ..Row::default()
        });
    }
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
    let notice = [encoding.notice(), bounds.notice()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    Ok(Inspection {
        details,
        kind: bibli_core::inspection::InspectionKind::Table,
        title: if rows.is_empty() {
            "Table".into()
        } else {
            format!("Table · rows {}–{shown}", offset + 1)
        },
        columns,
        rows,
        position: offset,
        previous: (offset > 0).then(|| offset.saturating_sub(page)),
        next: more.then_some(offset + page),
        source_available: true,
        notice,
        ..Inspection::default()
    })
}

/// The table's bounds, applied alike to the header and every row, and what
/// they left out, for the notice.
#[derive(Default)]
struct Bounds {
    /// Cells cut to `TABLE_CELL` characters.
    cells: usize,
    /// Rows (the header included) cut to `TABLE_COLUMNS` fields.
    rows: usize,
}
impl Bounds {
    fn row(&mut self, record: &csv::ByteRecord, encoding: Encoding) -> Vec<String> {
        if record.len() > limits::TABLE_COLUMNS {
            self.rows += 1;
        }
        record
            .iter()
            .take(limits::TABLE_COLUMNS)
            .map(|cell| bounded_cell(&encoding.decode(cell), &mut self.cells))
            .collect()
    }
    fn notice(&self) -> Option<String> {
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        let parts: Vec<String> = [
            (self.rows > 0).then(|| {
                format!(
                    "{} row{} cut to {} columns",
                    self.rows,
                    plural(self.rows),
                    limits::TABLE_COLUMNS
                )
            }),
            (self.cells > 0).then(|| {
                format!(
                    "{} cell{} longer than {} characters shortened",
                    self.cells,
                    plural(self.cells),
                    limits::TABLE_CELL
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

/// Keep at most `TABLE_CELL` characters of a cell and mark the cut visibly.
pub(crate) fn bounded_cell(cell: &str, truncated: &mut usize) -> String {
    match cell.char_indices().nth(limits::TABLE_CELL) {
        Some((end, _)) => {
            *truncated += 1;
            format!("{}…", &cell[..end])
        }
        None => cell.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(name: &str, bytes: &[u8], offset: u64) -> Inspection {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        table(&path, offset, &CancelToken::new()).unwrap()
    }

    #[test]
    fn a_spreadsheet_export_is_read_in_its_encoding() {
        // Latin-1 from a spreadsheet, with a mark-less legacy byte.
        let data = read("export.csv", b"name,price\ncaf\xe9,3\n", 0);
        assert_eq!(data.rows[0].cells, ["caf\u{e9}", "3"]);
        assert_eq!(data.notice, "Read as Windows-1252");
        // A UTF-8 mark is not part of the first column's name.
        let data = read(
            "marked.csv",
            "\u{feff}id,note\n1,\u{e9}t\u{e9}\n".as_bytes(),
            0,
        );
        assert_eq!(data.columns, ["id", "note"]);
        assert_eq!(data.rows[0].cells[1], "\u{e9}t\u{e9}");
        assert!(data.notice.is_empty());
        // UTF-16, as a spreadsheet's "Unicode text" writes it.
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "a\tb\n1\t\u{e9}\n"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        let data = read("unicode.tsv", &utf16, 0);
        assert_eq!(data.columns, ["a", "b"]);
        assert_eq!(data.rows[0].cells, ["1", "\u{e9}"]);
        assert_eq!(data.notice, "Read as UTF-16LE");
    }

    #[test]
    fn wide_rows_are_cut_like_the_header_and_said() {
        let wide = (0..limits::TABLE_COLUMNS + 6)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let data = read("wide.csv", format!("{wide}\n{wide}\nshort\n").as_bytes(), 0);
        assert_eq!(data.columns.len(), limits::TABLE_COLUMNS);
        assert_eq!(data.rows[0].cells.len(), limits::TABLE_COLUMNS);
        assert_eq!(data.rows[1].cells, ["short"]);
        assert_eq!(data.notice, "2 rows cut to 64 columns");
    }

    #[test]
    fn a_long_table_pages_past_the_text_bound_and_counts_exactly() {
        let line = format!("{},{}\n", "x".repeat(40), "y".repeat(40));
        let rows = inspection::limits::TEXT_INPUT / line.len() + 10;
        let mut text = String::from("a,b\n");
        text.push_str(&line.repeat(rows));
        let first = read("long.csv", text.as_bytes(), 0);
        assert_eq!(first.rows.len(), limits::TABLE_ROWS);
        assert_eq!(first.next, Some(limits::TABLE_ROWS as u64));
        let rows_detail = |data: &Inspection| {
            data.details
                .iter()
                .find(|d| d.label == "Rows")
                .map(|d| d.value.clone())
        };
        assert_eq!(rows_detail(&first).as_deref(), Some("more than 500"));
        let last_page = (rows as u64 - 1) / 500 * 500;
        let last = read("long.csv", text.as_bytes(), last_page);
        assert_eq!(last.next, None);
        assert_eq!(rows_detail(&last), Some(rows.to_string()));
        assert_eq!(last.title, format!("Table · rows {}–{rows}", last_page + 1));
        let empty = read("empty.csv", b"", 0);
        assert!(empty.columns.is_empty() && empty.rows.is_empty() && empty.details.is_empty());
    }

    #[test]
    fn cells_are_cut_at_the_limit_and_counted() {
        let mut truncated = 0;
        let short = "é".repeat(limits::TABLE_CELL);
        assert_eq!(bounded_cell(&short, &mut truncated), short);
        assert_eq!(truncated, 0);
        let long = "é".repeat(limits::TABLE_CELL + 1);
        let cut = bounded_cell(&long, &mut truncated);
        assert_eq!(cut.chars().count(), limits::TABLE_CELL + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(truncated, 1);
    }
}
