//! Garmin's Flexible and Interoperable Data Transfer (FIT): the binary
//! files of sports watches, bike computers and many trackers.
//!
//! A file is a header (12 or 14 bytes: its size, versions, the size of the
//! data, `.FIT`), records, then a CRC of it all; files may be chained one
//! after another. A record is a definition message, which says how a
//! local message type (0 to 15) lays out its fields, or a data message of
//! a type defined before. Data messages of `record` (global message 20)
//! are the fixes: timestamp, position in semicircles, altitude. Everything
//! else (laps, sessions, heart rate, developers' fields) is skipped by the
//! size its definition gives, so this reads files of any profile version.
//!
//! Every size is a byte of the file, so no message is larger than 130 kB
//! and nothing is allocated from a length the file claims. A file cut
//! short keeps the fixes before the cut.
use std::io::{self, BufRead};
use std::time::{Duration, SystemTime};

use bibli_core::CancelToken;
use bibli_core::geo::{Datum, LatLon, LineKind, Point};

use crate::collect::Collector;
use crate::xml::Damaged;

/// FIT's epoch, 1989-12-31T00:00:00Z, in Unix seconds.
const EPOCH: u64 = 631_065_600;
/// A timestamp below this counts from the device's power-on, not a date.
const RELATIVE: u32 = 0x1000_0000;
/// The global message number of `record`, and its fields.
const RECORD: u16 = 20;
const TIMESTAMP: u8 = 253;
const LATITUDE: u8 = 0;
const LONGITUDE: u8 = 1;
const ALTITUDE: u8 = 2;
const ENHANCED_ALTITUDE: u8 = 78;
/// What a value of each size means when the device has none.
const INVALID_I32: i32 = 0x7FFF_FFFF;

/// The layout of a local message type.
struct Definition {
    big_endian: bool,
    global: u16,
    /// Field numbers and sizes, in the order of the data.
    fields: Vec<(u8, u8)>,
    /// Bytes of a data message, developers' fields included.
    size: usize,
}

/// Why reading stopped early.
enum Stop {
    Damaged(String),
    Error(anyhow::Error),
}

impl From<io::Error> for Stop {
    fn from(error: io::Error) -> Self {
        Stop::Damaged(if error.kind() == io::ErrorKind::UnexpectedEof {
            "the file ends inside a record".into()
        } else {
            error.to_string()
        })
    }
}

fn damaged(why: &str) -> Stop {
    Stop::Damaged(why.to_owned())
}

/// The input, with the CRC of what was read and how much.
struct Source<R> {
    input: R,
    crc: u16,
    read: u64,
}

impl<R: BufRead> Source<R> {
    fn bytes(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.input.read_exact(buf)?;
        self.crc = buf.iter().fold(self.crc, |crc, &byte| crc16(crc, byte));
        self.read += buf.len() as u64;
        Ok(())
    }

    fn byte(&mut self) -> io::Result<u8> {
        let mut byte = [0];
        self.bytes(&mut byte)?;
        Ok(byte[0])
    }

    fn at_end(&mut self) -> io::Result<bool> {
        Ok(self.input.fill_buf()?.is_empty())
    }
}

/// FIT's CRC-16, a nibble at a time.
fn crc16(crc: u16, byte: u8) -> u16 {
    const TABLE: [u16; 16] = [
        0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800,
        0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
    ];
    let nibble = |crc: u16, low: u8| {
        let tmp = TABLE[usize::from(crc & 0xF)];
        let crc = (crc >> 4) & 0x0FFF;
        crc ^ tmp ^ TABLE[usize::from(low & 0xF)]
    };
    nibble(nibble(crc, byte), byte >> 4)
}

/// What the records read so far leave for the next.
#[derive(Default)]
struct State {
    /// The last timestamp of any message, which compressed timestamps
    /// count from.
    last_time: Option<u32>,
    started: bool,
    buf: Vec<u8>,
    records: u32,
}

pub fn read<R: BufRead>(
    input: R,
    into: &mut Collector,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Damaged>> {
    let mut source = Source {
        input,
        crc: 0,
        read: 0,
    };
    let mut state = State::default();
    let mut first = true;
    loop {
        match file(&mut source, &mut state, into, cancel, first) {
            Ok(true) => first = false,
            Ok(false) => return Ok(None),
            Err(Stop::Damaged(why)) => return Ok(Some(Damaged(why))),
            Err(Stop::Error(error)) => return Err(error),
        }
    }
}

/// One file of a chain. `Ok(true)` when it ended where another may start,
/// `Ok(false)` at the end of the input.
fn file<R: BufRead>(
    source: &mut Source<R>,
    state: &mut State,
    into: &mut Collector,
    cancel: &CancelToken,
    first: bool,
) -> Result<bool, Stop> {
    if !first && source.at_end()? {
        return Ok(false);
    }
    source.crc = 0;
    source.read = 0;
    let header_size = source.byte()?;
    let mut header = [0; 11];
    source.bytes(&mut header)?;
    if header_size < 12 || &header[7..11] != b".FIT" {
        return Err(damaged(if first {
            "this is not a FIT file"
        } else {
            "something other than a FIT file follows the data"
        }));
    }
    let data_size = u32::from_le_bytes([header[3], header[4], header[5], header[6]]);
    // A longer header ends with its own CRC, covered by the file's.
    let mut rest = [0; 255];
    source.bytes(&mut rest[..usize::from(header_size - 12)])?;
    let end = u64::from(header_size) + u64::from(data_size);
    let mut definitions: [Option<Definition>; 16] = Default::default();
    // A size of zero is a recording that was never closed: its records
    // run to the end of the input, and it has no CRC.
    while data_size == 0 || source.read < end {
        if data_size == 0 && source.at_end()? {
            return Err(damaged("the recording was not finished"));
        }
        state.records = state.records.wrapping_add(1);
        if state.records.is_multiple_of(4096) {
            bibli_platform::inspection::check(cancel).map_err(Stop::Error)?;
        }
        record(source, &mut definitions, state, into)?;
    }
    if source.read > end {
        return Err(damaged("a record runs past the end of the data"));
    }
    let computed = source.crc;
    let mut crc = [0; 2];
    source
        .bytes(&mut crc)
        .map_err(|_| damaged("the file ends before its checksum"))?;
    let stored = u16::from_le_bytes(crc);
    // Zero: the writer did not compute one.
    if stored != 0 && stored != computed {
        return Err(damaged("the file's checksum does not match its content"));
    }
    Ok(true)
}

fn record<R: BufRead>(
    source: &mut Source<R>,
    definitions: &mut [Option<Definition>; 16],
    state: &mut State,
    into: &mut Collector,
) -> Result<(), Stop> {
    let header = source.byte()?;
    // A compressed timestamp header: a data message of a local type 0 to
    // 3, its time the last one's plus up to 31 seconds, in 5 bits.
    if header & 0x80 != 0 {
        let offset = u32::from(header & 0x1F);
        let time = state.last_time.map(|last| {
            let time = (last & !0x1F).wrapping_add(offset);
            if offset < last & 0x1F {
                time.wrapping_add(0x20)
            } else {
                time
            }
        });
        let definition = definitions[usize::from((header >> 5) & 0x03)].as_ref();
        return data(source, definition, time, state, into);
    }
    let local = usize::from(header & 0x0F);
    if header & 0x40 != 0 {
        definitions[local] = Some(definition(source, header & 0x20 != 0)?);
        Ok(())
    } else {
        data(source, definitions[local].as_ref(), None, state, into)
    }
}

fn definition<R: BufRead>(source: &mut Source<R>, developer: bool) -> Result<Definition, Stop> {
    let mut fixed = [0; 5];
    source.bytes(&mut fixed)?;
    let [_reserved, architecture, g0, g1, count] = fixed;
    let big_endian = match architecture {
        0 => false,
        1 => true,
        _ => return Err(damaged("a message has an unknown byte order")),
    };
    let global = if big_endian {
        u16::from_be_bytes([g0, g1])
    } else {
        u16::from_le_bytes([g0, g1])
    };
    let mut fields = Vec::with_capacity(usize::from(count));
    let mut size = 0;
    let mut field = [0; 3];
    for _ in 0..count {
        source.bytes(&mut field)?;
        fields.push((field[0], field[1]));
        size += usize::from(field[1]);
    }
    if developer {
        for _ in 0..source.byte()? {
            source.bytes(&mut field)?;
            size += usize::from(field[1]);
        }
    }
    Ok(Definition {
        big_endian,
        global,
        fields,
        size,
    })
}

/// The values of a `record` message this reads.
#[derive(Default)]
struct Fix {
    time: Option<u32>,
    lat: Option<i32>,
    lon: Option<i32>,
    altitude: Option<u16>,
    enhanced: Option<u32>,
}

fn data<R: BufRead>(
    source: &mut Source<R>,
    definition: Option<&Definition>,
    compressed: Option<u32>,
    state: &mut State,
    into: &mut Collector,
) -> Result<(), Stop> {
    let definition = definition.ok_or_else(|| damaged("a message's type was never defined"))?;
    state.buf.resize(definition.size, 0);
    source.bytes(&mut state.buf)?;
    let big = definition.big_endian;
    let mut fix = Fix {
        time: compressed,
        ..Fix::default()
    };
    let mut offset = 0;
    for &(number, size) in &definition.fields {
        let bytes = &state.buf[offset..offset + usize::from(size)];
        offset += usize::from(size);
        match (number, bytes.len()) {
            (TIMESTAMP, 4) => fix.time = Some(u32_of(bytes, big)).filter(|&t| t != u32::MAX),
            _ if definition.global != RECORD => {}
            (LATITUDE, 4) => fix.lat = Some(u32_of(bytes, big).cast_signed()),
            (LONGITUDE, 4) => fix.lon = Some(u32_of(bytes, big).cast_signed()),
            (ALTITUDE, 2) => {
                let value = if big {
                    u16::from_be_bytes([bytes[0], bytes[1]])
                } else {
                    u16::from_le_bytes([bytes[0], bytes[1]])
                };
                fix.altitude = Some(value).filter(|&a| a != u16::MAX);
            }
            (ENHANCED_ALTITUDE, 4) => {
                fix.enhanced = Some(u32_of(bytes, big)).filter(|&a| a != u32::MAX);
            }
            _ => {}
        }
    }
    if fix.time.is_some() {
        state.last_time = fix.time;
    }
    if definition.global == RECORD {
        point(&fix, state, into);
    }
    Ok(())
}

fn u32_of(bytes: &[u8], big: bool) -> u32 {
    let bytes = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if big {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    }
}

/// A `record`'s point, if it has a position: one recorded indoors, or
/// before the first fix, has none, which is no error.
fn point(fix: &Fix, state: &mut State, into: &mut Collector) {
    let (Some(lat), Some(lon)) = (
        fix.lat.filter(|&v| v != INVALID_I32),
        fix.lon.filter(|&v| v != INVALID_I32),
    ) else {
        return;
    };
    let degrees = |semicircles: i32| f64::from(semicircles) * (180. / 2_147_483_648.);
    let Some(at) = LatLon::new(degrees(lat), degrees(lon)) else {
        into.invalid();
        return;
    };
    // Fifths of a metre, from 500 m below the sea.
    let altitude = fix
        .enhanced
        .map(f64::from)
        .or(fix.altitude.map(f64::from))
        .map(|raw| (raw / 5. - 500.) as f32);
    let time = fix
        .time
        .filter(|&t| t >= RELATIVE)
        .map(|t| SystemTime::UNIX_EPOCH + Duration::from_secs(EPOCH + u64::from(t)));
    if !state.started {
        state.started = true;
        into.line(String::new(), LineKind::Track, Datum::Sea);
    }
    into.point(Point { at, altitude, time });
}

/// Whether the first bytes of a file look like FIT: a header's size, then
/// `.FIT` at byte 8.
pub fn sniff(head: &[u8]) -> bool {
    head.len() >= 12 && head[0] >= 12 && &head[8..12] == b".FIT"
}

#[cfg(test)]
// The values compared are sums and products of small integers and
// halves, exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    /// A FIT file, built message by message.
    #[derive(Default)]
    struct Builder {
        data: Vec<u8>,
    }

    impl Builder {
        /// Fields are (number, size, base type); developers' fields
        /// (number, size, index).
        fn define(
            &mut self,
            local: u8,
            global: u16,
            big: bool,
            fields: &[(u8, u8, u8)],
            developer: &[(u8, u8, u8)],
        ) -> &mut Self {
            let flag = if developer.is_empty() { 0 } else { 0x20 };
            self.data.push(0x40 | flag | local);
            self.data.push(0);
            self.data.push(u8::from(big));
            if big {
                self.data.extend(global.to_be_bytes());
            } else {
                self.data.extend(global.to_le_bytes());
            }
            self.data.push(fields.len() as u8);
            for &(a, b, c) in fields {
                self.data.extend([a, b, c]);
            }
            if !developer.is_empty() {
                self.data.push(developer.len() as u8);
                for &(a, b, c) in developer {
                    self.data.extend([a, b, c]);
                }
            }
            self
        }

        fn message(&mut self, header: u8, bytes: &[u8]) -> &mut Self {
            self.data.push(header);
            self.data.extend(bytes);
            self
        }

        fn finish(&self) -> Vec<u8> {
            let mut file = vec![14, 0x20];
            file.extend(2132u16.to_le_bytes());
            file.extend((self.data.len() as u32).to_le_bytes());
            file.extend(b".FIT");
            let header_crc = file.iter().fold(0, |crc, &b| crc16(crc, b));
            file.extend(header_crc.to_le_bytes());
            file.extend(&self.data);
            let crc = file.iter().fold(0, |crc, &b| crc16(crc, b));
            file.extend(crc.to_le_bytes());
            file
        }
    }

    fn semicircles(degrees: f64) -> i32 {
        (degrees * 2_147_483_648. / 180.).round() as i32
    }

    /// 2026-03-12T08:00:00Z, in FIT's seconds.
    const EIGHT: u32 = 1_773_302_400 - EPOCH as u32;

    /// A `record`: timestamp, position, altitude, and a developer's field.
    fn fix(time: u32, lat: i32, lon: i32, altitude: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(time.to_le_bytes());
        bytes.extend(lat.to_le_bytes());
        bytes.extend(lon.to_le_bytes());
        bytes.extend(altitude.to_le_bytes());
        bytes.extend([0xAB, 0xCD]);
        bytes
    }

    fn ride() -> Vec<u8> {
        let mut b = Builder::default();
        // file_id: type, manufacturer, time created; skipped.
        b.define(
            0,
            0,
            false,
            &[(0, 1, 0x00), (1, 2, 0x84), (4, 4, 0x86)],
            &[],
        )
        .message(0, &[4, 1, 0, 0, 0, 0, 0]);
        b.define(
            1,
            RECORD,
            false,
            &[(253, 4, 0x86), (0, 4, 0x85), (1, 4, 0x85), (2, 2, 0x84)],
            &[(0, 2, 0)],
        );
        // 200 m, 210 m: (metres + 500) × 5.
        b.message(1, &fix(EIGHT, semicircles(10.), semicircles(20.), 3500));
        b.message(
            1,
            &fix(EIGHT + 1, semicircles(10.001), semicircles(20.), 3550),
        );
        // Indoors: no position.
        b.message(1, &fix(EIGHT + 2, INVALID_I32, INVALID_I32, 3550));
        // Beyond the pole.
        b.message(
            1,
            &fix(EIGHT + 2, semicircles(100.), semicircles(20.), 3550),
        );
        // Big-endian, with enhanced altitude beside the plain one, the
        // time compressed in the header.
        b.define(
            2,
            RECORD,
            true,
            &[(0, 4, 0x85), (1, 4, 0x85), (2, 2, 0x84), (78, 4, 0x86)],
            &[],
        );
        let mut bytes = Vec::new();
        bytes.extend(semicircles(10.002).to_be_bytes());
        bytes.extend(semicircles(20.).to_be_bytes());
        bytes.extend(1u16.to_be_bytes());
        bytes.extend(3600u32.to_be_bytes());
        let offset = ((EIGHT + 3) & 0x1F) as u8;
        b.message(0x80 | (2 << 5) | offset, &bytes);
        b.finish()
    }

    fn seconds(time: Option<SystemTime>) -> u64 {
        time.unwrap()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn a_fit_activity_gives_its_records() {
        let mut c = Collector::new();
        let damaged = read(ride().as_slice(), &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_none(), "{:?}", damaged.map(|d| d.0));
        let (doc, c) = c.finish();
        assert_eq!(doc.lines.len(), 1);
        assert_eq!(doc.lines[0].kind, LineKind::Track);
        let points = &doc.lines[0].segments[0];
        assert_eq!(points.len(), 3);
        assert_eq!(c.invalid, 1);
        assert!((points[0].at.lat - 10.).abs() < 1e-7);
        assert!((points[0].at.lon - 20.).abs() < 1e-7);
        assert!((points[2].at.lat - 10.002).abs() < 1e-7);
        assert_eq!(points[0].altitude, Some(200.));
        assert_eq!(points[1].altitude, Some(210.));
        assert_eq!(points[2].altitude, Some(220.));
        assert_eq!(seconds(points[0].time), 1_773_302_400);
        assert_eq!(seconds(points[1].time), 1_773_302_401);
        assert_eq!(seconds(points[2].time), 1_773_302_403);
        assert_eq!(c.tracks.climb, 20.);
    }

    #[test]
    fn a_file_cut_short_keeps_the_records_before_the_cut() {
        let whole = ride();
        // The header, file_id's definition and data, record's definition,
        // one record, and half the next.
        let cut = &whole[..14 + 15 + 8 + 22 + 17 + 8];
        let mut c = Collector::new();
        let damaged = read(cut, &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_some());
        let (doc, _) = c.finish();
        assert_eq!(doc.lines[0].segments[0].len(), 1);
    }

    #[test]
    fn a_wrong_checksum_is_damage_but_the_records_stay() {
        let mut file = ride();
        let last = file.len() - 1;
        file[last] ^= 0xFF;
        let mut c = Collector::new();
        let damaged = read(file.as_slice(), &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_some());
        let (doc, _) = c.finish();
        assert_eq!(doc.lines[0].segments[0].len(), 3);
    }

    #[test]
    fn chained_files_are_read_one_after_the_other() {
        let file = [ride(), ride()].concat();
        let mut c = Collector::new();
        assert!(
            read(file.as_slice(), &mut c, &CancelToken::new())
                .unwrap()
                .is_none()
        );
        let (_, c) = c.finish();
        assert_eq!(c.tracks.points, 6);
    }

    #[test]
    fn a_message_never_defined_stops_the_reading() {
        let mut b = Builder::default();
        b.message(3, &[1, 2, 3]);
        let mut c = Collector::new();
        let damaged = read(b.finish().as_slice(), &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_some());
    }

    #[test]
    fn other_files_are_not_fit() {
        let mut c = Collector::new();
        let damaged = read(&b"<gpx></gpx>"[..], &mut c, &CancelToken::new()).unwrap();
        assert!(damaged.is_some());
        assert!(sniff(&ride()));
        assert!(!sniff(b"<gpx></gpx>"));
        assert!(!sniff(b".FIT"));
    }
}
