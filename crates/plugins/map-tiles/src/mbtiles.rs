//! `MBTiles` (<https://github.com/mapbox/mbtiles-spec>): an SQLite database
//! whose `metadata` table describes the map, by name and value, and whose
//! `tiles` table or view holds each tile by zoom, column and row, rows
//! counted from the south. Read by the system's `libsqlite3` (ADR-022)
//! as the tables preview reads a database: read-only and immutable, so
//! nothing is written beside it, its schema not trusted to run functions,
//! and a request's cancellation interrupting SQLite between steps.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags, OptionalExtension, types::ValueRef};
use skwir_core::CancelToken;
use skwir_core::fs::kept_open::{self, KeptOpen};
use skwir_core::geo::Bounds;
use skwir_core::preview::Held;

use crate::archive::{About, Archive, Compression, Format, TILE_INFLATED, inflate};

/// The metadata's rows read, and each value's length kept: a `json` row
/// lists vector layers, not a document.
const METADATA_ROWS: usize = 256;
const METADATA_VALUE: i64 = 1024 * 1024;

pub(crate) struct MbTiles {
    /// The database, open while the map is shown: closed when a plan takes
    /// the file (ADR-136), a tile asked meanwhile failing with
    /// `kept_open::Taken`, opened again by the next tile once the plan has
    /// ended. SQLite opens without the sharing that lets Windows remove or
    /// move a file open.
    connection: Arc<Mutex<Option<Open>>>,
    path: PathBuf,
    about: About,
    /// What a preview holds of the file, kept while SQLite reads it: a
    /// remote's file is read from the copy what is held keeps
    /// (`Held::path_for_program`), which goes with it. Dropped after the
    /// connection (fields drop in order): the copy outlives every read.
    _held: Option<Arc<Held>>,
}

/// The database open, and its saying so (`fs::kept_open`), which goes
/// after it is closed.
struct Open {
    connection: Connection,
    _kept: KeptOpen,
}

/// Open the database at `path`, read-only and immutable, its schema not
/// trusted.
fn connect(path: &Path) -> anyhow::Result<Connection> {
    let uri = format!("{}?immutable=1", skwir_platform::uri::sqlite_uri(path));
    let connection = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.pragma_update(None, "trusted_schema", false)?;
    Ok(connection)
}

/// `connection`, kept open in `shared` until a plan takes `path`.
fn kept(path: &Path, shared: &Arc<Mutex<Option<Open>>>, connection: Connection) -> Open {
    let shared = Arc::downgrade(shared);
    let kept = kept_open::keep_open(path.to_owned(), move |_| {
        if let Some(shared) = shared.upgrade() {
            shared.lock().take();
        }
    });
    Open {
        connection,
        _kept: kept,
    }
}

/// Steps between two looks at the cancellation.
const STEPS: i32 = 1000;

impl MbTiles {
    pub(crate) fn open(path: &Path, cancel: &CancelToken) -> anyhow::Result<MbTiles> {
        let mut connection = connect(path)?;
        let token = cancel.clone();
        connection.progress_handler(STEPS, Some(move || token.is_cancelled()))?;
        let meta = metadata(&connection)?;
        let mut about = About {
            container: "MBTiles".into(),
            ..About::default()
        };
        about.read_metadata(&meta);
        let text = |key: &str| meta.get(key).and_then(serde_json::Value::as_str);
        about.format = text("format").map_or(Format::Unknown, Format::named);
        about.bounds = text("bounds").and_then(bounds);
        let zoom = |key: &str| text(key).and_then(|z| z.trim().parse::<u8>().ok());
        let (min, max) = match (zoom("minzoom"), zoom("maxzoom")) {
            (Some(min), Some(max)) => (min, max),
            // Not said: asked of the tiles, which an index answers.
            _ => connection
                .query_row(
                    "SELECT min(zoom_level), max(zoom_level) FROM tiles",
                    [],
                    |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
                )
                .map(|(min, max)| {
                    let zoom = |z: Option<i64>| z.and_then(|z| u8::try_from(z).ok()).unwrap_or(0);
                    (zoom(min), zoom(max))
                })
                .context("Not an MBTiles file: it has no tiles")?,
        };
        about.min_zoom = min.min(max);
        about.max_zoom = max;
        let shared = Arc::new(Mutex::new(None));
        *shared.lock() = Some(kept(path, &shared, connection));
        Ok(MbTiles {
            connection: shared,
            path: path.to_owned(),
            about,
            _held: None,
        })
    }

    /// The archive a preview holds (ADR-101): SQLite reads a file by its
    /// path, so it is handed what a program is.
    pub(crate) fn of_held(held: &Arc<Held>, cancel: &CancelToken) -> anyhow::Result<MbTiles> {
        let archive = MbTiles::open(&held.path_for_program(cancel)?, cancel)?;
        Ok(MbTiles {
            _held: Some(Arc::clone(held)),
            ..archive
        })
    }
}

/// The `metadata` table as names and values, its `json` row's keys (the
/// vector layers) merged in.
fn metadata(connection: &Connection) -> anyhow::Result<serde_json::Map<String, serde_json::Value>> {
    let mut statement = connection
        .prepare("SELECT name, substr(value, 1, ?1) FROM metadata")
        .context("Not an MBTiles file: it has no metadata table")?;
    let mut rows = statement.query([METADATA_VALUE])?;
    let mut meta = serde_json::Map::new();
    let mut json = None;
    while let Some(row) = rows.next()? {
        if meta.len() >= METADATA_ROWS {
            break;
        }
        let (Ok(name), Ok(value)) = (row.get::<_, String>(0), row.get_ref(1)) else {
            continue;
        };
        let value = match value {
            ValueRef::Text(text) | ValueRef::Blob(text) => {
                String::from_utf8_lossy(text).into_owned()
            }
            ValueRef::Integer(n) => n.to_string(),
            ValueRef::Real(x) => x.to_string(),
            ValueRef::Null => continue,
        };
        if name == "json" {
            json = Some(value);
        } else {
            meta.insert(name, serde_json::Value::String(value));
        }
    }
    if let Some(Ok(serde_json::Value::Object(extra))) =
        json.map(|json| serde_json::from_str::<serde_json::Value>(&json))
    {
        for (key, value) in extra {
            meta.entry(key).or_insert(value);
        }
    }
    Ok(meta)
}

/// `left,bottom,right,top` in degrees, as the spec writes bounds.
fn bounds(text: &str) -> Option<Bounds> {
    let numbers: Vec<f64> = text
        .split(',')
        .map(|n| n.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [west, south, east, north] = numbers.as_slice() else {
        return None;
    };
    let lat = -90. ..=90.;
    let lon = -180. ..=180.;
    (south < north
        && (east - west).abs() > f64::EPSILON
        && lat.contains(south)
        && lat.contains(north)
        && lon.contains(west)
        && lon.contains(east))
    .then_some(Bounds {
        south: *south,
        north: *north,
        west: *west,
        east: *east,
    })
}

impl Archive for MbTiles {
    fn about(&self) -> &About {
        &self.about
    }

    fn tile(&self, z: u8, x: u32, y: u32, cancel: &CancelToken) -> anyhow::Result<Option<Vec<u8>>> {
        if z > 31 || u64::from(x.max(y)) >= 1u64 << z {
            return Ok(None);
        }
        // Rows are counted from the south.
        let row = (1u64 << z) - 1 - u64::from(y);
        let mut slot = self.connection.lock();
        if slot.is_none() {
            // Taken by a plan still running: not opened again under it,
            // nor waited for (each tile would hold a worker for the plan's
            // length); the map asks again once it has ended.
            if let Some(taken) = kept_open::taken(&self.path) {
                return Err(taken.into());
            }
            *slot = Some(kept(&self.path, &self.connection, connect(&self.path)?));
        }
        let connection = &mut slot.as_mut().expect("opened above").connection;
        let token = cancel.clone();
        connection.progress_handler(STEPS, Some(move || token.is_cancelled()))?;
        let found = connection
            .query_row(
                "SELECT tile_data FROM tiles \
                 WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3",
                [i64::from(z), i64::from(x), i64::try_from(row)?],
                |row| {
                    Ok(match row.get_ref(0)? {
                        ValueRef::Blob(bytes) | ValueRef::Text(bytes)
                            if bytes.len() > TILE_INFLATED =>
                        {
                            Err("A tile of the archive is too large")
                        }
                        ValueRef::Blob(bytes) | ValueRef::Text(bytes) => Ok(Some(bytes.to_vec())),
                        _ => Ok(None),
                    })
                },
            )
            .optional()?;
        drop(slot);
        let Some(bytes) = found.unwrap_or(Ok(None)).map_err(anyhow::Error::msg)? else {
            return Ok(None);
        };
        // Vector tiles are gzipped, pictures are not: as they start.
        inflate(bytes, Compression::Unknown, TILE_INFLATED).map(Some)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An `MBTiles` file as the spec lays it out, its tiles in a view over
    /// two tables as many writers make them.
    pub(crate) fn write(path: &Path, tiles: &[(u8, u32, u32, &[u8])], meta: &[(&str, &str)]) {
        let db = Connection::open(path).unwrap();
        db.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE map (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_id TEXT);
             CREATE TABLE images (tile_id TEXT, tile_data BLOB);
             CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
             CREATE VIEW tiles AS SELECT map.zoom_level, map.tile_column, map.tile_row, images.tile_data
               FROM map JOIN images ON map.tile_id = images.tile_id;",
        )
        .unwrap();
        for (name, value) in meta {
            db.execute("INSERT INTO metadata VALUES (?1, ?2)", [name, value])
                .unwrap();
        }
        for (i, (z, x, y, bytes)) in tiles.iter().enumerate() {
            let row = (1u32 << z) - 1 - y;
            db.execute(
                "INSERT INTO map VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![z, x, row, i.to_string()],
            )
            .unwrap();
            db.execute(
                "INSERT INTO images VALUES (?1, ?2)",
                rusqlite::params![i.to_string(), bytes],
            )
            .unwrap();
        }
    }

    #[test]
    fn tiles_are_found_with_rows_from_the_south() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hills.mbtiles");
        write(
            &path,
            &[(1, 0, 0, b"north-west"), (1, 0, 1, b"south-west")],
            &[
                ("name", "Invented Hills"),
                ("format", "jpg"),
                ("bounds", "5.0,44.0,6.0,45.0"),
                ("minzoom", "1"),
                ("maxzoom", "1"),
                (
                    "attribution",
                    "<a href='https://example.org'>&copy; Example</a>",
                ),
            ],
        );
        let archive = MbTiles::open(&path, &CancelToken::new()).unwrap();
        let cancel = CancelToken::new();
        assert_eq!(
            archive.tile(1, 0, 0, &cancel).unwrap().as_deref(),
            Some(&b"north-west"[..])
        );
        assert_eq!(
            archive.tile(1, 0, 1, &cancel).unwrap().as_deref(),
            Some(&b"south-west"[..])
        );
        assert_eq!(archive.tile(1, 1, 0, &cancel).unwrap(), None);
        let about = archive.about();
        assert_eq!(about.format, Format::Jpeg);
        assert_eq!(about.name, "Invented Hills");
        assert_eq!(about.attribution, "© Example");
        assert_eq!((about.min_zoom, about.max_zoom), (1, 1));
        let bounds = about.bounds.unwrap();
        assert_eq!((bounds.west, bounds.south), (5., 44.));
    }

    /// A plan that takes the file closes the database (ADR-136): SQLite
    /// keeps it from being removed on Windows. The next tile, once the
    /// plan has ended, opens it again.
    #[test]
    fn a_plan_taking_the_file_closes_it_and_the_next_tile_opens_it_again() {
        use skwir_core::fs::kept_open::let_go_of;
        use skwir_core::op::PlanId;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("it's a map.mbtiles");
        write(&path, &[(1, 0, 0, b"north-west")], &[("format", "png")]);
        let archive = MbTiles::open(&path, &CancelToken::new()).unwrap();
        let taking = let_go_of(
            PlanId(u64::MAX - 7),
            |at| at == path,
            std::time::Duration::from_secs(5),
        );
        assert_eq!(taking.count(), 1);
        assert!(archive.connection.lock().is_none(), "closed for the plan");
        let failed = archive.tile(1, 0, 0, &CancelToken::new()).unwrap_err();
        let taken = kept_open::Taken::in_error(&failed).expect("taken, not damaged");
        assert!(!taken.is_over());
        drop(taking);
        assert!(taken.is_over(), "asked again once the plan has ended");
        let tile = archive.tile(1, 0, 0, &CancelToken::new()).unwrap();
        assert_eq!(tile.as_deref(), Some(&b"north-west"[..]));
        assert!(archive.connection.lock().is_some(), "open again");
    }

    #[test]
    fn what_the_metadata_leaves_out_is_read_from_the_tiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roads.mbtiles");
        write(
            &path,
            &[(3, 1, 1, b"a"), (5, 2, 2, b"b")],
            &[
                ("format", "pbf"),
                (
                    "json",
                    r#"{"vector_layers":[{"id":"roads"},{"id":"water"}]}"#,
                ),
            ],
        );
        let archive = MbTiles::open(&path, &CancelToken::new()).unwrap();
        let about = archive.about();
        assert_eq!(about.format, Format::Vector);
        assert_eq!((about.min_zoom, about.max_zoom), (3, 5));
        assert_eq!(about.layers, ["roads", "water"]);
        assert!(about.bounds.is_none(), "the world");
    }

    #[test]
    fn a_database_that_is_no_map_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.mbtiles");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE notes (text TEXT);")
            .unwrap();
        let error = MbTiles::open(&path, &CancelToken::new())
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("metadata"), "{error}");
        assert_eq!(bounds("1,2,3"), None);
        assert_eq!(bounds("5,45,6,44"), None, "south of its south");
    }
}
