//! Place names offline (ADR-022, "Place names offline"): the words for a
//! position ("Example Town", "12 km NE of Example Town") and the places a
//! name means, with no request to anyone.
//!
//! Two sources, packed by `tools/maps/places.py` into one format (its
//! docstring has the layout):
//!
//! - Natural Earth's populated places (public domain, about 7 300 cities and
//!   towns), embedded: about 125 KB in the binary, about 180 KB once
//!   decoded.
//! - The `cities1000` list of `GeoNames` (CC BY 4.0, about 160 000 places
//!   of more than 1 000 people: 2.9 MB packed, 4.2 MB once decoded), which
//!   the user may install at
//!   `$XDG_DATA_HOME/skwir/places/geonames.bin` or under a directory of
//!   `$XDG_DATA_DIRS`. When one of those files reads, it is used instead of
//!   the embedded places, not beside them: it holds every town Natural
//!   Earth has, at a finer scale, and two lists of one town would name it
//!   twice. Its attribution is [`attribution`]. A damaged file is skipped
//!   with a warning, and the next one, or the embedded places, used.
//!
//! The data is decoded the first time a name is asked, on the caller's
//! thread (callers are workers), and kept until [`release`]. Places sit in
//! 1° cells sorted by cell, so a query looks at the cells within its reach
//! and nothing else.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use skwir_core::geo::{EARTH_RADIUS, LatLon};

static EMBEDDED: &[u8] = include_bytes!("../data/places.bin");
const MAGIC: &[u8] = b"BPL1";
/// Where an installed `GeoNames` file is, under a data directory.
const INSTALLED: &str = "skwir/places/geonames.bin";
/// Coordinates are kept in 1e-5 degrees (about a metre).
const UNIT: f64 = 1e-5;
const PER_DEGREE: i32 = 100_000;
/// The farthest a place names a position from ([`reach`] of the biggest).
const MAX_REACH: f64 = 100_000.;

static LOADED: Mutex<Option<Arc<Index>>> = Mutex::new(None);

/// A place, as the data names it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Named {
    /// In the language the place uses (romanized when the data is).
    pub name: String,
    pub at: LatLon,
    /// The class of its population, larger for bigger places: 14 for ten
    /// million people or more, 12 for a million, 9 for 100 000, 6 for
    /// 10 000, 3 for 1 000, 1 for fewer or an unknown population.
    pub rank: u8,
    /// Its country's ISO 3166 two-letter code, empty when unknown.
    pub country: String,
}

/// Where the names come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    NaturalEarth,
    GeoNames,
}

/// The words for a position: the name of the place it is in, or how far
/// and in which direction it is from a place ("12 km NE of Example Town"),
/// or `None` when no place is near enough (the open sea, the desert).
///
/// A position is in a place when it is within the place's extent, a
/// radius growing with its rank from 2 km (a village) to 10 km (a city of
/// ten million); in several, it is in the one it is nearest to relative to
/// that radius, so a village at the edge of a city keeps its name.
/// Otherwise the place named is the one whose distance is the smallest
/// share of its reach, which grows with its rank from 15 km to 100 km: a
/// city a little farther is named before a hamlet, a hamlet much nearer
/// before the city. Beyond every place's reach, nothing. The distance has
/// a decimal under 10 km and none above; the direction is one of eight,
/// from the place to the position.
pub(crate) fn describe(at: LatLon) -> Option<String> {
    index().describe(at)
}

/// The places named `name`, compared ignoring case, Latin accents, and the
/// difference between a hyphen and a space: those spelled exactly as asked
/// first, then those differing only in case, then the rest; the biggest
/// first within each.
pub(crate) fn find(name: &str) -> Vec<Named> {
    index().find(name)
}

/// What the names in use need to be credited with: `"GeoNames (CC BY 4.0)"`
/// when the installed `GeoNames` file is in use, else nothing (Natural Earth
/// is in the public domain). Loads the names as [`describe`] does.
pub(crate) fn attribution() -> Option<&'static str> {
    match index().source {
        Source::GeoNames => Some("GeoNames (CC BY 4.0)"),
        Source::NaturalEarth => None,
    }
}

/// Let go of the names (the plugin is disabled). A query still running
/// keeps its own reference until it ends.
pub(crate) fn release() {
    LOADED.lock().take();
}

fn index() -> Arc<Index> {
    // The lock is held while loading so two workers do not decode twice.
    LOADED
        .lock()
        .get_or_insert_with(|| Arc::new(load(&installed())))
        .clone()
}

/// Where an installed `GeoNames` file may be, in the order they are tried.
fn installed() -> Vec<PathBuf> {
    skwir_platform::xdg::data_search()
        .into_iter()
        .map(|dir| dir.join(INSTALLED))
        .collect()
}

/// The first of `paths` that reads, else the embedded places.
fn load(paths: &[PathBuf]) -> Index {
    for path in paths {
        match read(path) {
            Ok(Some(index)) => return index,
            Ok(None) => {}
            Err(error) => tracing::warn!(
                path = %path.display(),
                "place names: the installed file is damaged, skipped: {error:#}"
            ),
        }
    }
    Index::parse(EMBEDDED).unwrap_or_else(|error| {
        tracing::warn!("place names: the embedded data is damaged: {error:#}");
        Index::empty()
    })
}

fn read(path: &Path) -> anyhow::Result<Option<Index>> {
    match std::fs::read(path) {
        Ok(bytes) => Index::parse(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// The radius within which a position is in a place of this rank.
fn extent(rank: u8) -> f64 {
    match rank {
        0..=4 => 2_000.,
        5 | 6 => 3_000.,
        7 | 8 => 4_000.,
        9 | 10 => 5_000.,
        11 => 6_000.,
        12 => 8_000.,
        _ => 10_000.,
    }
}

/// The farthest a place of this rank names a position from.
fn reach(rank: u8) -> f64 {
    match rank {
        0..=4 => 15_000.,
        5 | 6 => 25_000.,
        7 | 8 => 40_000.,
        9 | 10 => 60_000.,
        11 => 80_000.,
        _ => MAX_REACH,
    }
}

/// "5.6 km" under 10 km, "16 km" from there.
fn distance_words(metres: f64) -> String {
    let tenths = (metres / 100.).round();
    if tenths < 100. {
        format!("{:.1} km", tenths / 10.)
    } else {
        format!("{} km", (metres / 1000.).round())
    }
}

/// The direction of `to` seen from `from`, as one of eight.
fn compass(from: LatLon, to: LatLon) -> &'static str {
    let (a, b) = (from.lat.to_radians(), to.lat.to_radians());
    let dlon = (to.lon - from.lon).to_radians();
    let y = dlon.sin() * b.cos();
    let x = a.cos() * b.sin() - a.sin() * b.cos() * dlon.cos();
    let degrees = y.atan2(x).to_degrees().rem_euclid(360.);
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][(degrees / 45.).round() as usize % 8]
}

/// A name folded for comparison into `out`: lowercase, Latin accents
/// dropped, apostrophes and dots dropped, hyphens and runs of spaces one
/// space.
fn fold_into(name: &str, out: &mut String) {
    out.clear();
    let mut space = false;
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_whitespace() || matches!(c, '-' | '‐' | '_') {
            space = !out.is_empty();
            continue;
        }
        if matches!(c, '\'' | '’' | '.') {
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        match base(c) {
            Some(s) => out.push_str(s),
            None => out.push(c),
        }
    }
}

/// A lowercase Latin letter's letters without its accent.
fn base(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' | 'ț' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// A place in memory: 16 bytes and its name in the index's arena.
struct Place {
    /// In 1e-5 degrees.
    lat: i32,
    lon: i32,
    /// Where its name starts in `Index::names`, and its length in bytes.
    name: u32,
    len: u8,
    rank: u8,
    /// Into `Index::countries`.
    country: u8,
}

impl Place {
    fn at(&self) -> LatLon {
        LatLon {
            lat: f64::from(self.lat) * UNIT,
            lon: f64::from(self.lon) * UNIT,
        }
    }

    fn cell(&self) -> u32 {
        cell_key(row(self.lat), col(self.lon))
    }
}

fn row(lat: i32) -> u32 {
    (lat.div_euclid(PER_DEGREE) + 90).clamp(0, 179) as u32
}

fn col(lon: i32) -> u32 {
    (lon.div_euclid(PER_DEGREE) + 180).clamp(0, 359) as u32
}

fn cell_key(row: u32, col: u32) -> u32 {
    row * 360 + col
}

pub(crate) struct Index {
    source: Source,
    /// Every name, one after the other.
    names: String,
    /// ISO 3166 codes; the first is the unknown country.
    countries: Vec<[u8; 2]>,
    /// In order of their cell (`Place::cell`).
    places: Vec<Place>,
}

impl Index {
    fn empty() -> Index {
        Index {
            source: Source::NaturalEarth,
            names: String::new(),
            countries: vec![[0, 0]],
            places: Vec::new(),
        }
    }

    /// Decode the packed format (`tools/maps/places.py`), checking all of
    /// it: a file that does not end where its count says is damaged.
    fn parse(data: &[u8]) -> anyhow::Result<Index> {
        anyhow::ensure!(data.starts_with(MAGIC), "not place names");
        let mut r = Reader {
            data,
            at: MAGIC.len(),
        };
        let source = match r.byte()? {
            0 => Source::NaturalEarth,
            1 => Source::GeoNames,
            other => anyhow::bail!("unknown source {other}"),
        };
        let country_count = r.byte()?;
        anyhow::ensure!(country_count > 0, "no country table");
        let mut countries = Vec::with_capacity(country_count.into());
        for _ in 0..country_count {
            let code = [r.byte()?, r.byte()?];
            anyhow::ensure!(
                code == [0, 0] || code.iter().all(u8::is_ascii_uppercase),
                "a country code is not one"
            );
            countries.push(code);
        }
        let count = usize::try_from(r.varint()?)?;
        // A place takes at least six bytes: no count can promise more.
        anyhow::ensure!(
            count <= r.remaining() / 6,
            "more places than the data holds"
        );
        let mut places = Vec::with_capacity(count);
        let mut names = String::new();
        let (mut lat, mut lon) = (0i64, 0i64);
        for _ in 0..count {
            let len = r.byte()?;
            anyhow::ensure!(len > 0, "a place without a name");
            let name = std::str::from_utf8(r.take(len.into())?)?;
            let start = u32::try_from(names.len())?;
            names.push_str(name);
            lat += r.signed()?;
            lon += r.signed()?;
            let (lat, lon) = (i32::try_from(lat)?, i32::try_from(lon)?);
            anyhow::ensure!(
                lat.abs() <= 90 * PER_DEGREE && lon.abs() <= 180 * PER_DEGREE,
                "a place off the Earth"
            );
            let rank = r.byte()?;
            let country = r.byte()?;
            anyhow::ensure!(country < country_count, "a place in no listed country");
            places.push(Place {
                lat,
                lon,
                name: start,
                len,
                rank,
                country,
            });
        }
        anyhow::ensure!(r.remaining() == 0, "data after the last place");
        // The script writes them in this order already; a stable sort of
        // sorted data is one pass.
        places.sort_by_key(Place::cell);
        names.shrink_to_fit();
        Ok(Index {
            source,
            names,
            countries,
            places,
        })
    }

    fn name(&self, place: &Place) -> &str {
        let start = place.name as usize;
        &self.names[start..start + usize::from(place.len)]
    }

    fn named(&self, place: &Place) -> Named {
        let code = self.countries[usize::from(place.country)];
        Named {
            name: self.name(place).to_owned(),
            at: place.at(),
            rank: place.rank,
            country: if code == [0, 0] {
                String::new()
            } else {
                String::from_utf8_lossy(&code).into_owned()
            },
        }
    }

    /// The places of the cells in rows `rows` and columns `cols` (both
    /// inclusive; the columns may wrap past 359 to 0).
    fn cells<'a>(&'a self, rows: (u32, u32), cols: (u32, u32), mut visit: impl FnMut(&'a Place)) {
        let spans: &[(u32, u32)] = if cols.0 <= cols.1 {
            &[cols]
        } else {
            &[(cols.0, 359), (0, cols.1)]
        };
        for row in rows.0..=rows.1 {
            for &(west, east) in spans {
                let (lo, hi) = (cell_key(row, west), cell_key(row, east));
                let from = self.places.partition_point(|p| p.cell() < lo);
                let to = self.places.partition_point(|p| p.cell() <= hi);
                self.places[from..to].iter().for_each(&mut visit);
            }
        }
    }

    /// The places whose cells lie within `radius` metres of `at`: every
    /// place that close, and some a little farther.
    fn around<'a>(&'a self, at: LatLon, radius: f64, visit: impl FnMut(&'a Place)) {
        let angle = radius / EARTH_RADIUS;
        let dlat = angle.to_degrees();
        let (south, north) = ((at.lat - dlat).max(-90.), (at.lat + dlat).min(90.));
        let to_unit = |deg: f64| (deg / UNIT).round() as i32;
        let rows = (row(to_unit(south)), row(to_unit(north)));
        // The widest a circle is in longitude, unless it holds a pole.
        let full = (0, 359);
        let cols = if at.lat.abs() + dlat >= 90. {
            full
        } else {
            let dlon = (angle.sin() / at.lat.to_radians().cos())
                .min(1.)
                .asin()
                .to_degrees();
            if dlon >= 180. {
                full
            } else {
                let west = ((at.lon - dlon).floor() as i32) + 180;
                let east = ((at.lon + dlon).floor() as i32) + 180;
                let wrap = |c: i32| c.rem_euclid(360) as u32;
                if east - west >= 359 {
                    full
                } else {
                    (wrap(west), wrap(east))
                }
            }
        };
        self.cells(rows, cols, visit);
    }

    fn describe(&self, at: LatLon) -> Option<String> {
        // (share of the extent, place) of the place the position is in,
        // (share of the reach, distance, place) of the one it is near.
        let mut inside: Option<(f64, &Place)> = None;
        let mut near: Option<(f64, f64, &Place)> = None;
        self.around(at, MAX_REACH, |place| {
            let distance = place.at().distance(at);
            let share = distance / extent(place.rank);
            if share <= 1. && inside.is_none_or(|(best, _)| share < best) {
                inside = Some((share, place));
            }
            let share = distance / reach(place.rank);
            if share <= 1. && near.is_none_or(|(best, _, _)| share < best) {
                near = Some((share, distance, place));
            }
        });
        if let Some((_, place)) = inside {
            return Some(self.name(place).to_owned());
        }
        let (_, distance, place) = near?;
        Some(format!(
            "{} {} of {}",
            distance_words(distance),
            compass(place.at(), at),
            self.name(place)
        ))
    }

    fn find(&self, name: &str) -> Vec<Named> {
        let asked = name.trim();
        let (mut folded, mut buffer) = (String::new(), String::new());
        fold_into(asked, &mut folded);
        if folded.is_empty() {
            return Vec::new();
        }
        let lower = asked.to_lowercase();
        // (how exactly it matched, place): 0 as spelled, 1 but for case,
        // 2 but for accents and punctuation.
        let mut found: Vec<(u8, &Place)> = Vec::new();
        // A name starting with an ASCII letter or digit folds to one
        // starting with it in lowercase: most names are told apart there,
        // before folding them.
        let first = folded.as_bytes()[0];
        for place in &self.places {
            let candidate = self.name(place);
            let head = candidate.as_bytes()[0];
            if head.is_ascii_alphanumeric() && head.to_ascii_lowercase() != first {
                continue;
            }
            fold_into(candidate, &mut buffer);
            if buffer != folded {
                continue;
            }
            let tier = if candidate == asked {
                0
            } else if candidate.to_lowercase() == lower {
                1
            } else {
                2
            };
            found.push((tier, place));
        }
        found.sort_by(|(ta, a), (tb, b)| {
            ta.cmp(tb)
                .then(b.rank.cmp(&a.rank))
                .then_with(|| self.name(a).cmp(self.name(b)))
                .then((a.lat, a.lon).cmp(&(b.lat, b.lon)))
        });
        found.into_iter().map(|(_, p)| self.named(p)).collect()
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.data.len() - self.at
    }

    fn take(&mut self, n: usize) -> anyhow::Result<&'a [u8]> {
        anyhow::ensure!(n <= self.remaining(), "the place names end early");
        let bytes = &self.data[self.at..self.at + n];
        self.at += n;
        Ok(bytes)
    }

    fn byte(&mut self) -> anyhow::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn varint(&mut self) -> anyhow::Result<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            value |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        anyhow::bail!("a number in the place names is too long")
    }

    fn signed(&mut self) -> anyhow::Result<i64> {
        // Zigzag: 0, -1, 1, -2… as 0, 1, 2, 3…
        let n = self.varint()?;
        let magnitude = i64::try_from(n >> 1)?;
        Ok(if n & 1 == 1 {
            -magnitude - 1
        } else {
            magnitude
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (name, latitude, longitude, rank, country), as the script takes them.
    type Row<'a> = (&'a str, f64, f64, u8, &'a str);

    fn varint(mut n: u64, out: &mut Vec<u8>) {
        loop {
            let byte = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn zigzag(n: i64) -> u64 {
        ((n << 1) ^ (n >> 63)) as u64
    }

    /// The script's writer (`tools/maps/places.py`, `pack`).
    fn pack(source: u8, rows: &[Row]) -> Vec<u8> {
        let mut countries: Vec<&str> = rows.iter().map(|r| r.4).filter(|c| !c.is_empty()).collect();
        countries.sort_unstable();
        countries.dedup();
        countries.insert(0, "");
        let mut sorted: Vec<(i64, i64, i64, i64, &str, u8, u8)> = rows
            .iter()
            .map(|&(name, lat, lon, rank, code)| {
                let (lat, lon) = ((lat * 1e5).round() as i64, (lon * 1e5).round() as i64);
                let country = countries.iter().position(|c| *c == code).unwrap() as u8;
                (
                    lat.div_euclid(100_000),
                    lon.div_euclid(100_000),
                    lat,
                    lon,
                    name,
                    rank,
                    country,
                )
            })
            .collect();
        sorted.sort_by_key(|a| (a.0, a.1, a.2, a.3));
        let mut out = b"BPL1".to_vec();
        out.push(source);
        out.push(countries.len() as u8);
        for code in &countries {
            out.extend_from_slice(if code.is_empty() {
                b"\0\0"
            } else {
                code.as_bytes()
            });
        }
        varint(sorted.len() as u64, &mut out);
        let mut last = (0, 0);
        for (_, _, lat, lon, name, rank, country) in sorted {
            varint(name.len() as u64, &mut out);
            out.extend_from_slice(name.as_bytes());
            varint(zigzag(lat - last.0), &mut out);
            varint(zigzag(lon - last.1), &mut out);
            last = (lat, lon);
            out.push(rank);
            out.push(country);
        }
        out
    }

    /// Invented places at invented positions.
    const PLACES: &[Row] = &[
        ("Example Town", 10.0, 20.0, 7, "XA"),
        ("Sample Village", 10.0, 20.05, 3, "XA"),
        ("Example City", 10.5, 20.0, 13, "XA"),
        ("Quiet Hamlet", 10.32, 20.04, 2, "XA"),
        ("Example Town", 40.0, -100.0, 4, "XB"),
        ("Éxample-Town", -30.0, 150.0, 9, ""),
        ("Meridian Town", 0.0, 179.99, 6, "XC"),
        ("Polar Station", 89.95, 100.0, 1, ""),
    ];

    fn index() -> Index {
        Index::parse(&pack(0, PLACES)).expect("the synthetic places decode")
    }

    fn at(lat: f64, lon: f64) -> LatLon {
        LatLon::new(lat, lon).expect("a position")
    }

    fn describe(lat: f64, lon: f64) -> Option<String> {
        index().describe(at(lat, lon))
    }

    #[test]
    fn a_position_within_a_place_is_that_place() {
        assert_eq!(describe(10.0, 20.0).as_deref(), Some("Example Town"));
        // 2.2 km from the town (within its 4 km), 3.3 km from the village
        // (beyond its 2 km).
        assert_eq!(describe(10.0, 20.02).as_deref(), Some("Example Town"));
        // In the town's extent too, but at the village's middle.
        assert_eq!(describe(10.0, 20.05).as_deref(), Some("Sample Village"));
        // 5.6 km from the city's middle, within its 10 km.
        assert_eq!(describe(10.45, 20.0).as_deref(), Some("Example City"));
    }

    #[test]
    fn a_position_outside_is_a_distance_and_a_direction() {
        // 0.05° of latitude: 5.56 km.
        assert_eq!(
            describe(10.05, 20.0).as_deref(),
            Some("5.6 km N of Example Town")
        );
        // 0.15° of longitude at 10°: 16.4 km, whole above 10 km.
        assert_eq!(
            describe(10.0, 19.85).as_deref(),
            Some("16 km W of Example Town")
        );
        assert_eq!(
            describe(10.03, 20.03).as_deref(),
            Some("4.7 km NE of Example Town")
        );
        assert_eq!(
            describe(9.95, 20.0).as_deref(),
            Some("5.6 km S of Example Town")
        );
    }

    #[test]
    fn distances_round_to_a_decimal_under_ten_km() {
        assert_eq!(distance_words(2_040.), "2.0 km");
        assert_eq!(distance_words(9_940.), "9.9 km");
        // Rounds up to ten: no "10.0 km".
        assert_eq!(distance_words(9_960.), "10 km");
        assert_eq!(distance_words(16_430.), "16 km");
        assert_eq!(distance_words(99_600.), "100 km");
    }

    #[test]
    fn a_bigger_place_a_little_farther_is_named_before_a_hamlet() {
        // 20 km south of the city (a fifth of its 100 km reach), 4.4 km
        // from the hamlet (more than a quarter of its 15 km).
        assert_eq!(
            describe(10.32, 20.0).as_deref(),
            Some("20 km S of Example City")
        );
        // 2.2 km from the hamlet (just beyond its 2 km): a seventh of its
        // reach, against a fifth of the city's.
        assert_eq!(
            describe(10.32, 20.02).as_deref(),
            Some("2.2 km W of Quiet Hamlet")
        );
    }

    #[test]
    fn nothing_near_is_none() {
        // The open sea.
        assert_eq!(describe(-50.0, -20.0), None);
        // 556 km south of the town: beyond every reach.
        assert_eq!(describe(5.0, 20.0), None);
        // Near the pole, where the cells looked at go all the way round.
        assert_eq!(describe(89.0, -80.0), None);
    }

    #[test]
    fn places_are_found_across_the_antimeridian_and_the_pole() {
        // 1.7 km east of the town, on the other side of ±180°.
        assert_eq!(describe(0.0, -179.995).as_deref(), Some("Meridian Town"));
        assert_eq!(
            describe(0.0, -179.9).as_deref(),
            Some("12 km E of Meridian Town")
        );
        assert_eq!(
            describe(0.0, 179.85).as_deref(),
            Some("16 km W of Meridian Town")
        );
        // 11 km over the pole, on the far side of the station.
        assert_eq!(
            describe(89.95, -80.0).as_deref(),
            Some("11 km N of Polar Station")
        );
    }

    fn found(name: &str) -> Vec<(String, String, u8)> {
        index()
            .find(name)
            .into_iter()
            .map(|n| (n.name, n.country, n.rank))
            .collect()
    }

    #[test]
    fn find_puts_the_exact_spelling_first_then_the_biggest() {
        let exact = |c: &str, r| ("Example Town".to_owned(), c.to_owned(), r);
        let accented = ("Éxample-Town".to_owned(), String::new(), 9);
        assert_eq!(
            found("Example Town"),
            [exact("XA", 7), exact("XB", 4), accented.clone()]
        );
        // Case only, then accents: the order holds.
        assert_eq!(
            found(" example town "),
            [exact("XA", 7), exact("XB", 4), accented.clone()]
        );
        // As spelled with the accent: it first, then the biggest.
        assert_eq!(
            found("Éxample-Town"),
            [accented.clone(), exact("XA", 7), exact("XB", 4)]
        );
        // Neither: the biggest first.
        assert_eq!(
            found("EXAMPLE-TOWN"),
            [accented, exact("XA", 7), exact("XB", 4)]
        );
        assert!(found("Example").is_empty());
        assert!(found("  ").is_empty());
    }

    #[test]
    fn find_gives_the_place() {
        let town = &index().find("sample village")[0];
        assert_eq!(town.name, "Sample Village");
        assert_eq!(town.country, "XA");
        assert_eq!(town.rank, 3);
        assert!((town.at.lat - 10.0).abs() < 1e-9 && (town.at.lon - 20.05).abs() < 1e-9);
    }

    #[test]
    fn accents_fold() {
        let mut out = String::new();
        fold_into("Ærøskøbing-sur-Œuf  l'Été Straße", &mut out);
        assert_eq!(out, "aeroskobing sur oeuf lete strasse");
    }

    #[test]
    fn the_format_round_trips() {
        let data = pack(1, PLACES);
        let index = Index::parse(&data).expect("decodes");
        assert_eq!(index.source, Source::GeoNames);
        assert_eq!(index.places.len(), PLACES.len());
        for &(name, lat, lon, rank, country) in PLACES {
            let place = index
                .find(name)
                .into_iter()
                .find(|n| n.country == country && n.rank == rank)
                .expect("the place is there");
            assert_eq!(place.name, name);
            assert!((place.at.lat - lat).abs() < 1e-9 && (place.at.lon - lon).abs() < 1e-9);
        }
    }

    #[test]
    fn a_damaged_file_is_rejected() {
        let data = pack(1, PLACES);
        assert!(Index::parse(&data[..data.len() - 1]).is_err(), "cut short");
        let mut longer = data.clone();
        longer.push(0);
        assert!(Index::parse(&longer).is_err(), "trailing bytes");
        let mut magic = data.clone();
        magic[0] = b'X';
        assert!(Index::parse(&magic).is_err(), "not the format");
        let mut source = data.clone();
        source[4] = 7;
        assert!(Index::parse(&source).is_err(), "an unknown source");
        // The first place's name: "Éxample-Town", at -30° it comes first.
        let name = 4 + 2 + 2 * 4 + 1 + 1;
        let mut utf8 = data.clone();
        utf8[name] = 0xff;
        assert!(Index::parse(&utf8).is_err(), "a name not UTF-8");
        let mut country = data;
        let first_country = name + "Éxample-Town".len() + 4 + 4 + 1;
        country[first_country] = 200;
        assert!(Index::parse(&country).is_err(), "a country not listed");
    }

    #[test]
    fn an_installed_file_is_used_unless_damaged() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing/geonames.bin");
        let damaged = dir.path().join("damaged.bin");
        let good = dir.path().join("good.bin");
        std::fs::write(&damaged, b"BPL1\x01").unwrap();
        std::fs::write(&good, pack(1, PLACES)).unwrap();

        let embedded = load(&[missing.clone(), damaged.clone()]);
        assert_eq!(embedded.source, Source::NaturalEarth);
        assert!(embedded.places.len() > 5_000);

        let installed = load(&[missing, damaged, good]);
        assert_eq!(installed.source, Source::GeoNames);
        assert_eq!(installed.places.len(), PLACES.len());
    }

    #[test]
    fn the_embedded_places_decode() {
        let index = Index::parse(EMBEDDED).expect("the embedded places decode");
        assert_eq!(index.source, Source::NaturalEarth);
        assert!(index.places.len() > 7_000, "{}", index.places.len());
        // A capital's own coordinates are that capital.
        assert_eq!(
            index.describe(at(48.8566, 2.3522)).as_deref(),
            Some("Paris")
        );
    }

    #[test]
    fn installed_files_follow_xdg() {
        let paths = installed();
        assert!(paths.iter().all(|p| p.ends_with(INSTALLED)));
        assert!(!paths.is_empty());
    }
}
