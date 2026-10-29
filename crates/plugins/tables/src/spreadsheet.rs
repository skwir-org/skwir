//! Spreadsheets (`.xlsx`, `.xlsm`, `.xlsb`): each sheet's cells as a table,
//! a page of rows at a time, sheet after sheet, without `LibreOffice`.
//!
//! Only formats whose cells can be streamed are read: a sheet is walked
//! cell by cell and only the page is kept, so two cells a million rows
//! apart cost two cells, not the grid between them. (calamine's ODS and XLS
//! readers build that grid whole; those stay with `LibreOffice`.) The parts
//! calamine reads whole (shared strings, styles) are bounded before it
//! opens them: every XML or binary part of the container is inflated once
//! through a counter, within `limits::CONTAINER_EXPANSION`.
use crate::starts::Version;
use calamine::{DataRef, Reader, SheetType, SheetVisible, Xlsb, Xlsx};
use skwir_core::{
    CancelToken,
    inspection::{Inspection, Row},
    preview::{Held, ReadSeek},
};
use skwir_platform::inspection::{self, limits};
use skwir_platform::mime::Types;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;

/// The workbooks whose cells are read here, by type (AGENTS.md,
/// "Types"): Excel's, with macros or not, and its binary one.
pub static TYPES: Types = Types::new(&[
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-excel.sheet.macroEnabled.12",
    "application/vnd.ms-excel.sheet.binary.macroEnabled.12",
]);

/// Of them, the binary workbooks, which calamine reads with another
/// reader.
static BINARY: Types = Types::new(&["application/vnd.ms-excel.sheet.binary.macroEnabled.12"]);

/// A page position: the sheet in the high half, the row in the low.
const SHEET: u64 = 1 << 32;

type File = BufReader<Box<dyn ReadSeek + Send>>;

enum Book {
    Xlsx(Xlsx<File>),
    Xlsb(Xlsb<File>),
}

impl Book {
    /// The workbook `held` holds, read as its type `mime` says.
    fn open(held: &Arc<Held>, mime: &str) -> anyhow::Result<Self> {
        let file = BufReader::new(inspection::open(held)?);
        Ok(if BINARY.contains(mime) {
            Self::Xlsb(Xlsb::new(file)?)
        } else {
            Self::Xlsx(Xlsx::new(file)?)
        })
    }

    /// The sheets a reader sees: visible worksheets (charts and dialogs
    /// have no cells), or every worksheet when all are hidden.
    fn sheets(&self) -> Vec<String> {
        let sheets = match self {
            Self::Xlsx(book) => book.sheets_metadata(),
            Self::Xlsb(book) => book.sheets_metadata(),
        };
        let worksheets: Vec<_> = sheets
            .iter()
            .filter(|sheet| sheet.typ == SheetType::WorkSheet)
            .collect();
        let visible: Vec<_> = worksheets
            .iter()
            .filter(|sheet| sheet.visible == SheetVisible::Visible)
            .map(|sheet| sheet.name.clone())
            .collect();
        if visible.is_empty() {
            worksheets.iter().map(|sheet| sheet.name.clone()).collect()
        } else {
            visible
        }
    }

    /// Every used cell of sheet `name`, in the order the file holds them,
    /// with its zero-based row and column.
    fn cells(
        &mut self,
        name: &str,
        mut each: impl FnMut((u32, u32), &DataRef<'_>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        match self {
            Self::Xlsx(book) => {
                let mut cells = book.worksheet_cells_reader(name)?;
                while let Some(cell) = cells.next_cell()? {
                    each(cell.get_position(), cell.get_value())?;
                }
            }
            Self::Xlsb(book) => {
                let mut cells = book.worksheet_cells_reader(name)?;
                while let Some(cell) = cells.next_cell()? {
                    each(cell.get_position(), cell.get_value())?;
                }
            }
        }
        Ok(())
    }
}

/// The rows and the columns of the sheets walked whole, by version of a
/// workbook (`starts::version`) and sheet: a page turned in one of them
/// reads the sheet down to the page's last row and stops (a sheet's rows
/// are in order in its file), rather than walking every cell again to
/// count them.
static SIZES: std::sync::Mutex<Vec<(Version, String, (u64, u32))>> =
    std::sync::Mutex::new(Vec::new());

fn sizes() -> std::sync::MutexGuard<'static, Vec<(Version, String, (u64, u32))>> {
    SIZES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What stops a walk at the page's end.
#[derive(Debug)]
struct PageRead;

impl std::fmt::Display for PageRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the page is read")
    }
}

impl std::error::Error for PageRead {}

/// Inflate every part calamine may read, counting, before it reads any:
/// at most `limit` bytes together.
fn bounded(held: &Arc<Held>, limit: u64, cancel: &CancelToken) -> anyhow::Result<()> {
    // A version of a workbook found within the bound is not inflated
    // again for that at each page turned: it was, every part of it, up
    // to 128 MB.
    static WITHIN: std::sync::Mutex<Vec<(Version, u64)>> = std::sync::Mutex::new(Vec::new());
    let version = (crate::starts::version(held), limit);
    let within = || {
        WITHIN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    if within().contains(&version) {
        return Ok(());
    }
    measure(held, limit, cancel)?;
    let mut known = within();
    known.retain(|v| v.0.0 != version.0.0);
    if known.len() >= 8 {
        known.remove(0);
    }
    known.push(version);
    Ok(())
}

/// Every part calamine may read inflated and counted.
fn measure(held: &Arc<Held>, limit: u64, cancel: &CancelToken) -> anyhow::Result<()> {
    let mut package =
        skwir_platform::package::Package::open(held, "The spreadsheet", false, cancel)?;
    let parts: Vec<String> = package
        .names()
        .filter(|name| {
            Path::new(name)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    ["xml", "bin", "rels"]
                        .iter()
                        .any(|read| extension.eq_ignore_ascii_case(read))
                })
        })
        .map(str::to_owned)
        .collect();
    let mut total = 0usize;
    let mut chunk = vec![0; 64 * 1024];
    for name in parts {
        let Some(mut part) = package.reader(&name) else {
            continue;
        };
        loop {
            inspection::check(cancel)?;
            let read = part.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            total += read;
            anyhow::ensure!(
                total as u64 <= limit,
                "Spreadsheet exceeds {} unpacked",
                limits::describe(usize::try_from(limit).unwrap_or(usize::MAX))
            );
        }
    }
    Ok(())
}

/// A column's name as a spreadsheet writes it: A … Z, AA …
fn letters(mut column: u32) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (column % 26) as u8);
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

/// A cell as the sheet shows it, without its number format: a whole number
/// without decimals, a date as a date.
fn text(value: &DataRef<'_>) -> String {
    match value {
        DataRef::Int(n) => n.to_string(),
        DataRef::Float(n) if n.fract() == 0. && n.abs() < 1e15 => format!("{n:.0}"),
        DataRef::Float(n) => n.to_string(),
        DataRef::String(text) | DataRef::DateTimeIso(text) | DataRef::DurationIso(text) => {
            text.clone()
        }
        DataRef::SharedString(text) => (*text).to_owned(),
        DataRef::Bool(true) => "TRUE".into(),
        DataRef::Bool(false) => "FALSE".into(),
        DataRef::DateTime(when) if when.is_duration() => {
            let seconds = (when.as_f64() * 86_400.).round() as i64;
            format!(
                "{}:{:02}:{:02}",
                seconds / 3600,
                seconds % 3600 / 60,
                seconds % 60
            )
        }
        DataRef::DateTime(when) => {
            let (year, month, day, hour, minute, second, _) = when.to_ymd_hms_milli();
            match (hour, minute, second) {
                (0, 0, 0) => format!("{year:04}-{month:02}-{day:02}"),
                _ if when.as_f64() < 1. => format!("{hour:02}:{minute:02}:{second:02}"),
                _ => format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"),
            }
        }
        DataRef::Error(error) => error.to_string(),
        DataRef::Empty => String::new(),
    }
}

/// One page of `TABLE_ROWS` rows of one sheet, at `offset` (`SHEET`
/// times the sheet, plus the row). Next goes on down the sheet, then to
/// the next sheet's top; Previous back up, then to the previous sheet's.
/// Its columns are `bounds`'. `mime` is the workbook's type, one of
/// `TYPES`: it says which reader reads it.
pub fn table(
    held: &Arc<Held>,
    mime: &str,
    offset: u64,
    bounds: limits::Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Inspection> {
    let most = bounds.table_columns();
    // Every part is read to be measured, then again by calamine, a zip's
    // jumps between them: one the preview keeps whole is read whole first,
    // once, and both read memory. A larger one is read where it is.
    let keeps = held.keeps().min(bounds.binary_input() as u64);
    held.whole(keeps, cancel)?;
    bounded(held, limits::CONTAINER_EXPANSION, cancel)?;
    let mut book = Book::open(held, mime)?;
    let sheets = book.sheets();
    anyhow::ensure!(!sheets.is_empty(), "The spreadsheet has no sheet of cells");
    let sheet = usize::try_from(offset / SHEET)
        .unwrap_or(usize::MAX)
        .min(sheets.len() - 1);
    let first = offset % SHEET;
    let page = limits::TABLE_ROWS as u64;
    let version = crate::starts::version(held);
    let known = sizes()
        .iter()
        .find(|(v, name, _)| *v == version && *name == sheets[sheet])
        .map(|(_, _, size)| *size);
    let mut grid: Vec<Vec<String>> = Vec::new();
    let (mut rows, mut columns) = (0u64, 0u32);
    let (mut seen, mut cut) = (0usize, 0usize);
    let walked = book.cells(&sheets[sheet], |(row, column), value| {
        seen += 1;
        if seen.is_multiple_of(4096) {
            inspection::check(cancel)?;
        }
        // Its size known, the sheet is read no further than the page.
        if known.is_some() && u64::from(row) >= first + page {
            return Err(PageRead.into());
        }
        if matches!(value, DataRef::Empty) {
            return Ok(());
        }
        rows = rows.max(u64::from(row) + 1);
        columns = columns.max(column + 1);
        let row = u64::from(row);
        if row >= first && row < first + page && (column as usize) < most {
            let at = (row - first) as usize;
            if grid.len() <= at {
                grid.resize_with(at + 1, Vec::new);
            }
            let cells = &mut grid[at];
            if cells.len() <= column as usize {
                cells.resize(column as usize + 1, String::new());
            }
            cells[column as usize] = crate::bounded_cell(&text(value), &mut cut);
        }
        Ok(())
    });
    match walked {
        Err(error) if error.is::<PageRead>() => {}
        walked => walked?,
    }
    if let Some((all_rows, all_columns)) = known {
        (rows, columns) = (all_rows, all_columns);
    } else {
        let mut kept = sizes();
        kept.retain(|(v, name, _)| v.0 != version.0 || *name != sheets[sheet]);
        if kept.len() >= 64 {
            kept.remove(0);
        }
        kept.push((version, sheets[sheet].clone(), (rows, columns)));
    }
    let shown = (columns as usize).min(most);
    let last = rows.min(first + page);
    grid.resize_with(last.saturating_sub(first) as usize, Vec::new);
    let table_rows: Vec<Row> = grid
        .into_iter()
        .map(|mut cells| {
            cells.resize(shown, String::new());
            Row {
                cells,
                ..Row::default()
            }
        })
        .collect();
    let more_rows = rows > first + page;
    let next = if more_rows {
        Some(sheet as u64 * SHEET + first + page)
    } else {
        (sheet + 1 < sheets.len()).then(|| (sheet as u64 + 1) * SHEET)
    };
    let previous = if first > 0 {
        Some(sheet as u64 * SHEET + first.saturating_sub(page))
    } else {
        (sheet > 0).then(|| (sheet as u64 - 1) * SHEET)
    };
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let notice = [
        (columns as usize > most).then(|| format!("Only the first {most} columns shown")),
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
    let details = inspection::details([
        ("Sheet", Some(sheets[sheet].clone())),
        ("Sheets", inspection::count(sheets.len())),
        ("Rows", inspection::count(rows as usize)),
        ("Columns", inspection::count(columns as usize)),
    ]);
    Ok(Inspection {
        details,
        kind: skwir_core::inspection::InspectionKind::Table,
        title: if table_rows.is_empty() {
            sheets[sheet].clone()
        } else {
            format!("{} · rows {}–{last}", sheets[sheet], first + 1)
        },
        columns: (0..shown as u32).map(letters).collect(),
        rows: table_rows,
        position: offset,
        previous,
        next,
        notice,
        whole_available: columns as usize > most && bounds.cut(),
        ..Inspection::default()
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fmt::Write as _;
    use std::io::Write as _;

    /// An Excel workbook's type.
    pub(crate) const XLSX: &str =
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

    /// What a preview holds of the file at `path`, as it holds it.
    pub(crate) fn held(path: &Path) -> Arc<Held> {
        Arc::new(Held::open(path, skwir_core::preview::WHOLE).unwrap())
    }

    /// A workbook written by hand, as the smallest Excel file: `sheets`
    /// are (name, `<sheetData>` rows), strings are shared.
    pub(crate) fn workbook(
        dir: &Path,
        sheets: &[(&str, &str)],
        strings: &[&str],
    ) -> std::path::PathBuf {
        let path = dir.join("a workbook’s name.xlsx");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let mut part = |name: &str, text: &str| {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        };
        let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let relationships = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let kind = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let mut types = String::from(
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        );
        let (mut listed, mut related) = (String::new(), String::new());
        for (n, (name, rows)) in sheets.iter().enumerate() {
            let n = n + 1;
            let _ = write!(
                types,
                r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
            );
            let _ = write!(
                listed,
                r#"<sheet name="{name}" sheetId="{n}" r:id="rId{n}"/>"#
            );
            let _ = write!(
                related,
                r#"<Relationship Id="rId{n}" Type="{kind}/worksheet" Target="worksheets/sheet{n}.xml"/>"#
            );
            part(
                &format!("xl/worksheets/sheet{n}.xml"),
                &format!(
                    r#"<?xml version="1.0"?><worksheet xmlns="{main}"><sheetData>{rows}</sheetData></worksheet>"#
                ),
            );
        }
        types.push_str("</Types>");
        let _ = write!(
            related,
            r#"<Relationship Id="rS" Type="{kind}/sharedStrings" Target="sharedStrings.xml"/><Relationship Id="rT" Type="{kind}/styles" Target="styles.xml"/>"#
        );
        part("[Content_Types].xml", &types);
        part(
            "_rels/.rels",
            &format!(
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{kind}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
            ),
        );
        part(
            "xl/workbook.xml",
            &format!(
                r#"<?xml version="1.0"?><workbook xmlns="{main}" xmlns:r="{relationships}"><sheets>{listed}</sheets></workbook>"#
            ),
        );
        part(
            "xl/_rels/workbook.xml.rels",
            &format!(
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{related}</Relationships>"#
            ),
        );
        let shared = strings.iter().fold(String::new(), |mut shared, s| {
            let _ = write!(shared, "<si><t>{s}</t></si>");
            shared
        });
        part(
            "xl/sharedStrings.xml",
            &format!(r#"<?xml version="1.0"?><sst xmlns="{main}">{shared}</sst>"#),
        );
        // Style 1 is the built-in date format 14.
        part(
            "xl/styles.xml",
            &format!(
                r#"<?xml version="1.0"?><styleSheet xmlns="{main}"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14" applyNumberFormat="1"/></cellXfs></styleSheet>"#
            ),
        );
        zip.finish().unwrap();
        path
    }

    #[test]
    fn cells_are_read_as_the_sheet_shows_them_and_far_ones_cost_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = workbook(
            dir.path(),
            &[
                (
                    "Data",
                    r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2" s="1"><v>45943</v></c><c r="C2"><v>3.5</v></c><c r="D2"><v>12</v></c></row><row r="1000000"><c r="XFD1000000"><v>1</v></c></row>"#,
                ),
                ("Other", r#"<row r="1"><c r="A1" t="b"><v>1</v></c></row>"#),
            ],
            &["name", "when", "café"],
        );
        let cancel = CancelToken::new();
        let book = held(&path);
        let data = table(&book, XLSX, 0, limits::Bounds::QUICK, &cancel).unwrap();
        assert_eq!(data.columns.len(), limits::TABLE_COLUMNS);
        assert_eq!(&data.columns[..4], ["A", "B", "C", "D"]);
        assert_eq!(&data.rows[0].cells[..2], ["name", "when"]);
        assert_eq!(
            &data.rows[1].cells[..4],
            ["café", "2025-10-13", "3.5", "12"]
        );
        assert_eq!(data.rows.len(), limits::TABLE_ROWS);
        assert_eq!(data.title, format!("Data · rows 1–{}", limits::TABLE_ROWS));
        assert_eq!(data.notice, "Only the first 64 columns shown");
        let fact = |data: &Inspection, label: &str| {
            data.details
                .iter()
                .find(|d| d.label == label)
                .map(|d| d.value.clone())
        };
        assert_eq!(fact(&data, "Rows").as_deref(), Some("1000000"));
        assert_eq!(fact(&data, "Columns").as_deref(), Some("16384"));
        assert_eq!(fact(&data, "Sheets").as_deref(), Some("2"));
        // Turned to again, the page is read alone, its sheet's size known.
        let again = table(&book, XLSX, 0, limits::Bounds::QUICK, &cancel).unwrap();
        let cells = |data: &Inspection| -> Vec<Vec<String>> {
            data.rows.iter().map(|row| row.cells.clone()).collect()
        };
        assert_eq!(cells(&again), cells(&data));
        assert_eq!(
            (
                fact(&again, "Rows"),
                fact(&again, "Columns"),
                again.notice,
                again.next
            ),
            (
                fact(&data, "Rows"),
                fact(&data, "Columns"),
                data.notice.clone(),
                data.next
            )
        );

        // The last page of the sheet, then the next sheet from its top.
        let last = table(&book, XLSX, 999_900, limits::Bounds::QUICK, &cancel).unwrap();
        assert_eq!(last.rows.len(), 100);
        assert_eq!(last.next, Some(SHEET));
        let other = table(&book, XLSX, SHEET, limits::Bounds::QUICK, &cancel).unwrap();
        assert_eq!(other.title, "Other · rows 1–1");
        assert_eq!(other.rows[0].cells, ["TRUE"]);
        assert_eq!((other.previous, other.next), (Some(0), None));
    }

    #[test]
    fn a_workbook_that_unpacks_too_far_is_refused_before_it_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let blank = " ".repeat(4096);
        let book = held(&workbook(dir.path(), &[("Data", "")], &[&blank]));
        let cancel = CancelToken::new();
        bounded(&book, 8192, &cancel).unwrap();
        let error = bounded(&book, 4096, &cancel).unwrap_err();
        assert!(error.to_string().contains("unpacked"), "{error}");
    }

    #[test]
    fn columns_are_named_as_a_spreadsheet_names_them() {
        assert_eq!(letters(0), "A");
        assert_eq!(letters(25), "Z");
        assert_eq!(letters(26), "AA");
        assert_eq!(letters(701), "ZZ");
        assert_eq!(letters(16_383), "XFD");
    }
}
