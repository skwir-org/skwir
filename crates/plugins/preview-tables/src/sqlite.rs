//! SQLite databases: each table's rows as a table, a page at a time, table
//! after table, read by the system's `libsqlite3` (ADR-022).
//!
//! A preview never changes what it shows: the database is opened read-only
//! and immutable, so nothing is written beside it (no journal, no lock),
//! and nothing from its schema is trusted to run. Only tables are read, not
//! views (a view is a query of any cost) nor virtual tables (their module
//! may be missing, their shadow tables are listed). Every cell is bounded
//! in the query: a text is cut there, a blob is only measured, so a row
//! holding a film costs its size's digits. The request's cancellation
//! interrupts SQLite between steps.
use bibli_core::{
    CancelToken,
    inspection::{Inspection, Row},
};
use bibli_platform::inspection::{self, limits};
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use std::path::Path;

pub const MIMES: [&str; 2] = ["application/vnd.sqlite3", "application/x-sqlite3"];
pub const EXTENSIONS: [&str; 3] = ["sqlite", "sqlite3", "db3"];

/// A page position: the table in the high half, the row in the low.
const TABLE: u64 = 1 << 32;

/// A name as SQL quotes an identifier.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn open(path: &Path, cancel: &CancelToken) -> anyhow::Result<Connection> {
    let uri = format!("{}?immutable=1", bibli_platform::uri::file_uri(path));
    let connection = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.pragma_update(None, "trusted_schema", false)?;
    let cancel = cancel.clone();
    connection.progress_handler(1000, Some(move || cancel.is_cancelled()))?;
    Ok(connection)
}

/// The tables a reader sees, in the order the schema lists them.
fn tables(connection: &Connection) -> anyhow::Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_schema WHERE type = 'table' \
         AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         AND sql NOT LIKE 'CREATE VIRTUAL TABLE%' ORDER BY rowid",
    )?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

fn columns(connection: &Connection, table: &str) -> anyhow::Result<Vec<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({})", quoted(table)))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

/// One page of `TABLE_ROWS` rows of one table, at `offset` (`TABLE` times
/// the table, plus the row). Next goes on down the table, then to the next
/// table's top; Previous back up, then to the previous table's.
pub fn table(path: &Path, offset: u64, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    inspection::check(cancel)?;
    let connection = open(path, cancel)?;
    let tables = tables(&connection)?;
    anyhow::ensure!(!tables.is_empty(), "The database has no table");
    let index = usize::try_from(offset / TABLE)
        .unwrap_or(usize::MAX)
        .min(tables.len() - 1);
    let name = &tables[index];
    let first = offset % TABLE;
    let page = limits::TABLE_ROWS as u64;
    let all = columns(&connection, name)?;
    let shown = &all[..all.len().min(limits::TABLE_COLUMNS)];
    // Each cell as its type and a bounded value: a text cut one character
    // past the bound (so the cut is seen), a blob's length.
    let cells: Vec<String> = shown
        .iter()
        .map(|column| {
            let column = quoted(column);
            format!(
                "typeof({column}), CASE typeof({column}) \
                 WHEN 'text' THEN substr({column}, 1, {}) \
                 WHEN 'blob' THEN length({column}) ELSE {column} END",
                limits::TABLE_CELL + 1
            )
        })
        .collect();
    let query = format!(
        "SELECT {} FROM {} LIMIT {} OFFSET {first}",
        if cells.is_empty() {
            "1".into()
        } else {
            cells.join(", ")
        },
        quoted(name),
        page + 1
    );
    let mut statement = connection.prepare(&query)?;
    let mut result = statement.query([])?;
    let mut rows = Vec::new();
    let mut more = false;
    let mut cut = 0;
    while let Some(row) = result.next()? {
        inspection::check(cancel)?;
        if rows.len() == limits::TABLE_ROWS {
            more = true;
            break;
        }
        let mut values = Vec::with_capacity(shown.len());
        for column in 0..shown.len() {
            let kind: String = row.get(column * 2)?;
            let value = match (kind.as_str(), row.get_ref(column * 2 + 1)?) {
                ("blob", ValueRef::Integer(length)) => {
                    format!("{} blob", inspection::size(length.unsigned_abs()))
                }
                (_, ValueRef::Null) => "NULL".into(),
                (_, ValueRef::Integer(n)) => n.to_string(),
                (_, ValueRef::Real(n)) => n.to_string(),
                (_, ValueRef::Text(text)) => String::from_utf8_lossy(text).into_owned(),
                (_, ValueRef::Blob(bytes)) => {
                    format!("{} blob", inspection::size(bytes.len() as u64))
                }
            };
            values.push(crate::bounded_cell(&value, &mut cut));
        }
        rows.push(Row {
            cells: values,
            ..Row::default()
        });
    }
    let shown_to = first + rows.len() as u64;
    let next = if more {
        Some(index as u64 * TABLE + first + page)
    } else {
        (index + 1 < tables.len()).then(|| (index as u64 + 1) * TABLE)
    };
    let previous = if first > 0 {
        Some(index as u64 * TABLE + first.saturating_sub(page))
    } else {
        (index > 0).then(|| (index as u64 - 1) * TABLE)
    };
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let notice = [
        (all.len() > limits::TABLE_COLUMNS)
            .then(|| format!("Only the first {} columns shown", limits::TABLE_COLUMNS)),
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
        ("Table", Some(name.clone())),
        ("Tables", inspection::count(tables.len())),
        (
            "Rows",
            if more {
                Some(format!("more than {shown_to}"))
            } else {
                inspection::count(shown_to as usize)
            },
        ),
        ("Columns", inspection::count(all.len())),
    ]);
    Ok(Inspection {
        details,
        kind: bibli_core::inspection::InspectionKind::Table,
        title: if rows.is_empty() {
            name.clone()
        } else {
            format!("{name} · rows {}–{shown_to}", first + 1)
        },
        columns: shown.to_vec(),
        rows,
        position: offset,
        previous,
        next,
        notice,
        ..Inspection::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database in WAL mode, as applications leave theirs: two tables
    /// (one with a name to quote), a view, and cells of every kind.
    fn database(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("an app’s data.db");
        let db = Connection::open(&path).unwrap();
        db.pragma_update(None, "journal_mode", "wal").unwrap();
        db.execute_batch(
            "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, photo BLOB, score REAL, note TEXT);
             CREATE TABLE \"odd \"\"name\"\"\" (value);
             INSERT INTO \"odd \"\"name\"\"\" VALUES ('here');
             CREATE VIEW everyone AS SELECT name FROM people;",
        )
        .unwrap();
        let long = "x".repeat(limits::TABLE_CELL + 10);
        db.execute(
            "INSERT INTO people (name, photo, score, note) VALUES (?1, ?2, 2.5, ?3)",
            ("Zoë", vec![0u8; 2048], long),
        )
        .unwrap();
        db.execute("INSERT INTO people (name) VALUES ('Ada')", ())
            .unwrap();
        drop(db);
        path
    }

    fn listing(dir: &Path) -> Vec<std::ffi::OsString> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn tables_are_read_bounded_and_nothing_is_written_beside_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = database(dir.path());
        let before = listing(dir.path());
        let cancel = CancelToken::new();
        let people = table(&path, 0, &cancel).unwrap();
        assert_eq!(people.title, "people · rows 1–2");
        assert_eq!(people.columns, ["id", "name", "photo", "score", "note"]);
        assert_eq!(
            &people.rows[0].cells[..4],
            ["1", "Zoë", "2.0 KB blob", "2.5"]
        );
        assert!(people.rows[0].cells[4].ends_with('…'));
        assert_eq!(people.rows[1].cells, ["2", "Ada", "NULL", "NULL", "NULL"]);
        assert_eq!(
            people.next,
            Some(TABLE),
            "then the next table, not the view"
        );
        let odd = table(&path, TABLE, &cancel).unwrap();
        assert_eq!(odd.title, "odd \"name\" · rows 1–1");
        assert_eq!(odd.rows[0].cells, ["here"]);
        assert_eq!((odd.previous, odd.next), (Some(0), None));
        assert_eq!(
            listing(dir.path()),
            before,
            "no journal, lock or index written"
        );
    }

    #[test]
    fn a_cancelled_read_stops() {
        let dir = tempfile::tempdir().unwrap();
        let path = database(dir.path());
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(table(&path, 0, &cancel).is_err());
    }
}
