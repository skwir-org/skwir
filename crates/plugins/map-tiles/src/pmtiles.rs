//! `PMTiles`, version 3 (<https://github.com/protomaps/PMTiles>, `spec/v3`):
//! a 127-byte header, then a root directory, the metadata, leaf
//! directories and the tiles, each at an offset and of a length the header
//! gives. A directory lists tiles by their id on a Hilbert curve, in runs
//! of tiles that share their bytes; an entry of no run points to a leaf
//! directory. Every offset and length is checked against its section and
//! the file before it is read, and what is inflated is bounded.
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use anyhow::Context;
use parking_lot::Mutex;
use skwir_core::CancelToken;
use skwir_core::geo::Bounds;
use skwir_core::preview::{FileSource, ReadSeek};

use crate::archive::{About, Archive, Compression, Format, TILE_INFLATED, inflate};

pub(crate) const MAGIC: &[u8] = b"PMTiles";
const HEADER: usize = 127;
/// A directory read and inflated is at most this large. The spec keeps the
/// root within the file's first 16 KiB; leaves are larger.
const DIRECTORY: usize = 32 * 1024 * 1024;
const METADATA: usize = 4 * 1024 * 1024;
/// Leaf directories kept decoded: a map shows a few dozen tiles, which a
/// few leaves hold.
const LEAVES_KEPT: usize = 32;
/// Directories nest no deeper than this; writers make two levels.
const DEPTH: usize = 4;

/// One entry of a directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    tile_id: u64,
    /// From the tiles' section, or the leaves' for a leaf directory.
    offset: u64,
    length: u64,
    /// How many tiles from `tile_id` share these bytes; 0 for a leaf
    /// directory.
    run: u64,
}

/// A part of the file: where it starts, how long it is.
#[derive(Clone, Copy, Debug)]
struct Section {
    offset: u64,
    length: u64,
}

impl Section {
    /// The part `offset..offset + length` of this section, as a part of the
    /// file, if it is within it.
    fn part(self, offset: u64, length: u64) -> anyhow::Result<Section> {
        let end = offset.checked_add(length);
        anyhow::ensure!(
            end.is_some_and(|end| end <= self.length),
            "The archive points outside itself"
        );
        Ok(Section {
            offset: self.offset + offset,
            length,
        })
    }
}

struct Header {
    root: Section,
    metadata: Section,
    leaves: Section,
    tiles: Section,
    addressed: u64,
    internal: Compression,
    tile_compression: Compression,
    tile_type: u8,
    min_zoom: u8,
    max_zoom: u8,
    /// West, south, east, north, in degrees.
    bounds: [f64; 4],
}

fn compression(byte: u8) -> Compression {
    match byte {
        1 => Compression::None,
        2 => Compression::Gzip,
        3 => Compression::Brotli,
        4 => Compression::Zstd,
        _ => Compression::Unknown,
    }
}

fn header(bytes: &[u8; HEADER], len: u64) -> anyhow::Result<Header> {
    anyhow::ensure!(bytes.starts_with(MAGIC), "Not a PMTiles file");
    let version = bytes[7];
    anyhow::ensure!(
        version == 3,
        "PMTiles version {version} is not read; version 3 is"
    );
    let u64_at = |at: usize| {
        let mut word = [0u8; 8];
        word.copy_from_slice(&bytes[at..at + 8]);
        u64::from_le_bytes(word)
    };
    let degrees = |at: usize| {
        let mut word = [0u8; 4];
        word.copy_from_slice(&bytes[at..at + 4]);
        f64::from(i32::from_le_bytes(word)) / 10_000_000.
    };
    let section = |at: usize| -> anyhow::Result<Section> {
        Section {
            offset: 0,
            length: len,
        }
        .part(u64_at(at), u64_at(at + 8))
    };
    Ok(Header {
        root: section(8)?,
        metadata: section(24)?,
        leaves: section(40)?,
        tiles: section(56)?,
        addressed: u64_at(72),
        internal: compression(bytes[97]),
        tile_compression: compression(bytes[98]),
        tile_type: bytes[99],
        min_zoom: bytes[100],
        max_zoom: bytes[101],
        bounds: [degrees(102), degrees(106), degrees(110), degrees(114)],
    })
}

/// The next unsigned LEB128 number of `bytes` from `at`.
fn varint(bytes: &[u8], at: &mut usize) -> anyhow::Result<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes
            .get(*at)
            .context("A directory of the archive ends early")?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    anyhow::bail!("A directory of the archive holds a number too large")
}

/// A directory inflated: how many entries, then their ids (each the
/// difference from the one before), their runs, their lengths and their
/// offsets (0: right after the entry before; else one more than it).
fn directory(bytes: &[u8]) -> anyhow::Result<Vec<Entry>> {
    let mut at = 0;
    let count = varint(bytes, &mut at)?;
    // Each entry takes four bytes at least.
    anyhow::ensure!(
        count <= bytes.len() as u64 / 4,
        "A directory of the archive is damaged"
    );
    let count = count as usize;
    let mut entries = vec![
        Entry {
            tile_id: 0,
            offset: 0,
            length: 0,
            run: 0
        };
        count
    ];
    let mut id = 0u64;
    for entry in &mut entries {
        id = id
            .checked_add(varint(bytes, &mut at)?)
            .context("A directory of the archive is damaged")?;
        entry.tile_id = id;
    }
    for entry in &mut entries {
        entry.run = varint(bytes, &mut at)?;
    }
    for entry in &mut entries {
        entry.length = varint(bytes, &mut at)?;
    }
    for i in 0..count {
        let value = varint(bytes, &mut at)?;
        entries[i].offset = match (value, i) {
            (0, 0) => anyhow::bail!("A directory of the archive is damaged"),
            (0, _) => entries[i - 1]
                .offset
                .checked_add(entries[i - 1].length)
                .context("A directory of the archive is damaged")?,
            (value, _) => value - 1,
        };
    }
    Ok(entries)
}

/// The entry that holds tile `id`: the one with the greatest id not past
/// it, if its run reaches it, or a leaf directory that may.
fn find(entries: &[Entry], id: u64) -> Option<Entry> {
    let after = entries.partition_point(|entry| entry.tile_id <= id);
    let entry = *entries.get(after.checked_sub(1)?)?;
    (entry.run == 0 || id - entry.tile_id < entry.run).then_some(entry)
}

/// The id of tile `z/x/y`: the tiles of every shallower zoom, then its
/// place on zoom `z`'s Hilbert curve.
fn tile_id(z: u8, x: u32, y: u32) -> u64 {
    let z = u32::from(z);
    let before = ((1u64 << (2 * z)) - 1) / 3;
    let n = 1u64 << z;
    let (mut x, mut y) = (u64::from(x), u64::from(y));
    let mut d = 0;
    let mut s = n / 2;
    while s > 0 {
        let (rx, ry) = (u64::from(x & s > 0), u64::from(y & s > 0));
        d += s * s * ((3 * rx) ^ ry);
        // The quadrant turned as the curve turns there.
        if ry == 0 {
            if rx == 1 {
                x = n - 1 - x;
                y = n - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        s /= 2;
    }
    before + d
}

/// What an archive is read through, for as long as it is shown: its file
/// opened, or a reader of what a preview holds of it (`Held::reader`).
type Reader = Mutex<Box<dyn ReadSeek + Send>>;

pub(crate) struct PmTiles {
    file: Reader,
    header: Header,
    root: Arc<Vec<Entry>>,
    /// Leaf directories read, by offset, the latest last.
    leaves: Mutex<Vec<(u64, Arc<Vec<Entry>>)>>,
    about: About,
}

impl PmTiles {
    /// The archive `source` holds: its file, or what a preview holds of
    /// it.
    pub(crate) fn open<S: FileSource + ?Sized>(source: &S) -> anyhow::Result<PmTiles> {
        let mut file = skwir_platform::inspection::open(source)?;
        let len = source.length()?;
        let mut bytes = [0u8; HEADER];
        file.read_exact(&mut bytes)
            .context("Not a PMTiles file: it ends before its header")?;
        let header = header(&bytes, len)?;
        let file = Mutex::new(file);
        let root = Arc::new(directory(&read(
            &file,
            header.root,
            header.internal,
            DIRECTORY,
        )?)?);
        let mut about = About {
            container: "PMTiles 3".into(),
            format: match header.tile_type {
                1 => Format::Vector,
                2 => Format::Png,
                3 => Format::Jpeg,
                4 => Format::Webp,
                5 => Format::Avif,
                _ => Format::Unknown,
            },
            min_zoom: header.min_zoom,
            max_zoom: header.max_zoom.max(header.min_zoom),
            bounds: bounds(header.bounds),
            tiles: (header.addressed > 0).then_some(header.addressed),
            unreadable: header
                .tile_compression
                .unreadable()
                .map(|why| format!("Its tiles are {why}")),
            ..About::default()
        };
        if header.metadata.length > 0 {
            let json = read(&file, header.metadata, header.internal, METADATA)?;
            if let Ok(serde_json::Value::Object(meta)) = serde_json::from_slice(&json) {
                about.read_metadata(&meta);
            }
        }
        Ok(PmTiles {
            file,
            root,
            leaves: Mutex::new(Vec::new()),
            header,
            about,
        })
    }

    /// The leaf directory at `offset` of the leaves' section.
    fn leaf(&self, offset: u64, length: u64) -> anyhow::Result<Arc<Vec<Entry>>> {
        if let Some((_, leaf)) = self.leaves.lock().iter().find(|(at, _)| *at == offset) {
            return Ok(Arc::clone(leaf));
        }
        let part = self.header.leaves.part(offset, length)?;
        let leaf = Arc::new(directory(&read(
            &self.file,
            part,
            self.header.internal,
            DIRECTORY,
        )?)?);
        let mut leaves = self.leaves.lock();
        if leaves.len() >= LEAVES_KEPT {
            leaves.remove(0);
        }
        leaves.push((offset, Arc::clone(&leaf)));
        Ok(leaf)
    }
}

/// The bounds a header gives, unless it gives none (all zero).
fn bounds([west, south, east, north]: [f64; 4]) -> Option<Bounds> {
    let valid = |lat: f64, lon: f64| (-90. ..=90.).contains(&lat) && (-180. ..=180.).contains(&lon);
    (south < north
        && (east - west).abs() > f64::EPSILON
        && valid(south, west)
        && valid(north, east))
    .then_some(Bounds {
        south,
        north,
        west,
        east,
    })
}

/// A section of the file, inflated within `limit`.
fn read(
    file: &Reader,
    section: Section,
    compression: Compression,
    limit: usize,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        section.length <= limit as u64,
        "A part of the archive is too large"
    );
    let mut bytes = vec![0u8; section.length as usize];
    {
        let mut file = file.lock();
        file.seek(SeekFrom::Start(section.offset))?;
        file.read_exact(&mut bytes)?;
    }
    inflate(bytes, compression, limit)
}

impl Archive for PmTiles {
    fn about(&self) -> &About {
        &self.about
    }

    fn tile(&self, z: u8, x: u32, y: u32, cancel: &CancelToken) -> anyhow::Result<Option<Vec<u8>>> {
        if z > 31 || u64::from(x.max(y)) >= 1u64 << z {
            return Ok(None);
        }
        let id = tile_id(z, x, y);
        let mut entries = Arc::clone(&self.root);
        for _ in 0..DEPTH {
            skwir_platform::inspection::check(cancel)?;
            let Some(entry) = find(&entries, id) else {
                return Ok(None);
            };
            if entry.run > 0 {
                let part = self.header.tiles.part(entry.offset, entry.length)?;
                return read(
                    &self.file,
                    part,
                    self.header.tile_compression,
                    TILE_INFLATED,
                )
                .map(Some);
            }
            entries = self.leaf(entry.offset, entry.length)?;
        }
        anyhow::bail!("The archive's directories nest too deep")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn tiles_are_numbered_along_the_hilbert_curve() {
        // The spec's examples.
        assert_eq!(tile_id(0, 0, 0), 0);
        assert_eq!(tile_id(1, 0, 0), 1);
        assert_eq!(tile_id(1, 0, 1), 2);
        assert_eq!(tile_id(1, 1, 1), 3);
        assert_eq!(tile_id(1, 1, 0), 4);
        assert_eq!(tile_id(2, 0, 0), 5);
        // Every tile of a zoom once, after the zooms before it.
        for z in 0..6u8 {
            let n = 1u32 << z;
            let mut ids: Vec<u64> = (0..n)
                .flat_map(|x| (0..n).map(move |y| tile_id(z, x, y)))
                .collect();
            ids.sort_unstable();
            let first = tile_id(z, 0, 0);
            assert_eq!(first, ((1u64 << (2 * z)) - 1) / 3);
            assert_eq!(ids, (first..first + u64::from(n * n)).collect::<Vec<_>>());
        }
    }

    pub(crate) fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    /// A directory as the spec lays it out, from `(id, offset, length, run)`.
    pub(crate) fn encode_directory(entries: &[(u64, u64, u64, u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        push_varint(&mut out, entries.len() as u64);
        let mut last = 0;
        for &(id, ..) in entries {
            push_varint(&mut out, id - last);
            last = id;
        }
        for &(.., run) in entries {
            push_varint(&mut out, run);
        }
        for &(_, _, length, _) in entries {
            push_varint(&mut out, length);
        }
        for (i, &(_, offset, ..)) in entries.iter().enumerate() {
            let follows = i > 0 && {
                let (_, before, length, _) = entries[i - 1];
                before + length == offset
            };
            push_varint(&mut out, if follows { 0 } else { offset + 1 });
        }
        out
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(bytes).unwrap();
        gz.finish().unwrap()
    }

    /// A `PMTiles` file laid out as the spec says: header, root directory,
    /// metadata, leaves, tiles. `tiles` are `(z, x, y, bytes)`; with `leaf`,
    /// the root points to one leaf directory holding them all.
    pub(crate) fn write(tiles: &[(u8, u32, u32, Vec<u8>)], tile_type: u8, leaf: bool) -> Vec<u8> {
        let mut sorted: Vec<(u64, &Vec<u8>)> = tiles
            .iter()
            .map(|(z, x, y, bytes)| (tile_id(*z, *x, *y), bytes))
            .collect();
        sorted.sort_by_key(|(id, _)| *id);
        let mut data = Vec::new();
        let mut entries = Vec::new();
        for (id, bytes) in sorted {
            entries.push((id, data.len() as u64, bytes.len() as u64, 1));
            data.extend_from_slice(bytes);
        }
        let (root, leaves) = if leaf {
            let leaves = gzip(&encode_directory(&entries));
            let pointer = [(entries[0].0, 0, leaves.len() as u64, 0)];
            (gzip(&encode_directory(&pointer)), leaves)
        } else {
            (gzip(&encode_directory(&entries)), Vec::new())
        };
        let metadata = gzip(
            br#"{"name":"Invented Hills","attribution":"<a href=\"https://example.org\">&copy; Example</a>","vector_layers":[{"id":"water"},{"id":"roads"}]}"#,
        );
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.push(3);
        let mut offset = HEADER as u64;
        for part in [&root, &metadata, &leaves, &data] {
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&(part.len() as u64).to_le_bytes());
            offset += part.len() as u64;
        }
        for count in [tiles.len() as u64; 3] {
            out.extend_from_slice(&count.to_le_bytes());
        }
        // Clustered, gzip directories, uncompressed tiles, the type, zooms.
        let max_zoom = tiles.iter().map(|t| t.0).max().unwrap_or(0);
        let min_zoom = tiles.iter().map(|t| t.0).min().unwrap_or(0);
        out.extend_from_slice(&[1, 2, 1, tile_type, min_zoom, max_zoom]);
        for degrees in [5.0f64, 44.0, 6.0, 45.0] {
            out.extend_from_slice(&((degrees * 1e7) as i32).to_le_bytes());
        }
        out.push(min_zoom);
        out.extend_from_slice(&[0; 8]);
        assert_eq!(out.len(), HEADER);
        for part in [&root, &metadata, &leaves, &data] {
            out.extend_from_slice(part);
        }
        out
    }

    fn archive(bytes: &[u8]) -> (tempfile::TempDir, PmTiles) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("map.pmtiles");
        std::fs::write(&path, bytes).unwrap();
        let archive = PmTiles::open(&path).unwrap();
        (dir, archive)
    }

    #[test]
    fn a_tile_is_found_through_the_root_or_a_leaf() {
        let tiles = vec![
            (0, 0, 0, b"world".to_vec()),
            (1, 1, 0, b"north-east".to_vec()),
            (2, 2, 1, b"deeper".to_vec()),
        ];
        let cancel = CancelToken::new();
        for leaf in [false, true] {
            let (_dir, archive) = archive(&write(&tiles, 2, leaf));
            for (z, x, y, bytes) in &tiles {
                assert_eq!(
                    archive.tile(*z, *x, *y, &cancel).unwrap().as_ref(),
                    Some(bytes),
                    "{z}/{x}/{y}, leaf {leaf}"
                );
            }
            assert_eq!(archive.tile(1, 0, 0, &cancel).unwrap(), None);
            assert_eq!(
                archive.tile(2, 9, 0, &cancel).unwrap(),
                None,
                "off the world"
            );
            let about = archive.about();
            assert_eq!(about.format, Format::Png);
            assert_eq!((about.min_zoom, about.max_zoom), (0, 2));
            assert_eq!(about.name, "Invented Hills");
            assert_eq!(about.attribution, "© Example");
            assert_eq!(about.layers, ["water", "roads"]);
            assert_eq!(about.tiles, Some(3));
            let bounds = about.bounds.unwrap();
            assert!((bounds.west - 5.).abs() < 1e-6 && (bounds.north - 45.).abs() < 1e-6);
        }
    }

    /// What the reference writer made of `tools/maps/test_archives.py`'s
    /// tiles (`testdata/README.md`): every tile where the generator put it,
    /// told by its colour, the gradient from blue at 45° N to red at 44° N
    /// read at its middle, and its dark north-west corner.
    #[test]
    fn the_reference_writers_archives_read_as_they_were_made() {
        let testdata = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/testdata");
        let hills = PmTiles::open(&testdata.join("hills.pmtiles")).unwrap();
        let about = hills.about();
        assert_eq!(about.format, Format::Png);
        assert_eq!((about.min_zoom, about.max_zoom), (5, 9));
        assert_eq!(about.name, "Invented Hills");
        assert_eq!(about.attribution, "© Example Maps");
        assert_eq!(about.tiles, Some(15));
        let bounds = about.bounds.unwrap();
        assert!((bounds.south - 44.).abs() < 1e-6 && (bounds.east - 6.).abs() < 1e-6);

        let tile_of = |lat: f64, lon: f64, z: u8| {
            let n = f64::from(1u32 << z);
            let lat = lat.to_radians();
            let x = (lon + 180.) / 360. * n;
            let y = (1. - (lat.tan() + 1. / lat.cos()).ln() / std::f64::consts::PI) / 2. * n;
            (x as u32, y as u32)
        };
        let lat_of = |y: f64, n: f64| {
            (std::f64::consts::PI * (1. - 2. * y / n))
                .sinh()
                .atan()
                .to_degrees()
        };
        let cancel = CancelToken::new();
        let mut checked = 0;
        for z in 5..=9u8 {
            let (x0, y0) = tile_of(45., 5., z);
            let (x1, y1) = tile_of(44., 6., z);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    let bytes = hills.tile(z, x, y, &cancel).unwrap().expect("a tile");
                    let image = skwir_platform::inspection::image(&bytes).unwrap();
                    assert_eq!((image.width, image.height), (256, 256));
                    let pixel = |px: usize, py: usize| {
                        let at = (py * 256 + px) * 4;
                        [image.rgba[at], image.rgba[at + 1], image.rgba[at + 2]]
                    };
                    assert_eq!(pixel(20, 20), [20, 20, 20], "{z}/{x}/{y}: north-west");
                    let lat = lat_of(f64::from(y) + 128. / 256., f64::from(1u32 << z));
                    let t = (45. - lat).clamp(0., 1.);
                    let red = 40. + 200. * t;
                    let [r, g, _] = pixel(128, 128);
                    assert!(
                        (f64::from(r) - red).abs() <= 2. && g == 90,
                        "{z}/{x}/{y}: red {r}, the gradient says {red:.0} at {lat:.3}°"
                    );
                    checked += 1;
                }
            }
            assert_eq!(
                hills.tile(z, x1 + 1, y0, &cancel).unwrap(),
                None,
                "zoom {z}"
            );
        }
        assert_eq!(checked, 15, "the tiles the archive addresses");

        let roads = PmTiles::open(&testdata.join("roads.pmtiles")).unwrap();
        let about = roads.about();
        assert_eq!(about.format, Format::Vector);
        assert_eq!((about.min_zoom, about.max_zoom), (0, 6));
        assert_eq!(about.layers, ["roads", "water"]);
        // Vector tiles are gzipped: read inflated.
        let world = roads.tile(0, 0, 0, &cancel).unwrap().unwrap();
        assert_eq!(world, b"\x1a\x00");
    }

    #[test]
    fn a_run_of_tiles_shares_its_bytes() {
        // Three tiles of zoom 1 in one run, sharing one blank picture.
        let blank = b"blank";
        let entries = [(1, 0, blank.len() as u64, 3)];
        let dir = directory(&encode_directory(&entries)).unwrap();
        assert_eq!(find(&dir, 3).map(|e| e.offset), Some(0));
        assert_eq!(find(&dir, 4), None, "past the run");
        assert_eq!(find(&dir, 0), None, "before the first");
    }

    #[test]
    fn a_damaged_archive_is_refused_not_read_past() {
        let tiles = vec![(0, 0, 0, b"world".to_vec())];
        let mut bytes = write(&tiles, 2, false);
        // The tiles' section said to be longer than the file.
        bytes[64..72].copy_from_slice(&u64::MAX.to_le_bytes());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.pmtiles");
        std::fs::write(&path, &bytes).unwrap();
        assert!(PmTiles::open(&path).is_err());
        // A version this reader does not know.
        let mut bytes = write(&tiles, 2, false);
        bytes[7] = 2;
        std::fs::write(&path, &bytes).unwrap();
        let error = PmTiles::open(&path).err().unwrap().to_string();
        assert!(error.contains("version 2"), "{error}");
        // A directory that says it holds more entries than its bytes can.
        let mut bad = Vec::new();
        push_varint(&mut bad, 1000);
        assert!(directory(&bad).is_err());
        assert!(directory(&[0x80; 12]).is_err(), "a number that never ends");
    }
}
