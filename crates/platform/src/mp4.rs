//! Where a video was recorded, from the boxes of its MP4 or `.mov` file: phones
//! write an ISO 6709 string (`+10.5000+020.2500+012.000/`) in
//! `moov/udta/©xyz` (Android, older iPhones) or as the value of the key
//! `com.apple.quicktime.location.ISO6709` in `moov/meta` (iPhones), and a
//! recording date beside it.
//!
//! Only box headers are read on the way down: a video's media data is
//! skipped by seeking, and a leaf box is read only when it is one of those
//! and small. A box that lies about its size ends the walk, never a read.
use std::io::{Read, Seek, SeekFrom};

use skwir_core::geo::LatLon;
use skwir_core::inspection::Detail;
use skwir_core::preview::FileSource;

use crate::exif::Taken;

/// A leaf box read whole is at most this: a location or a key list.
const MAX_LEAF: u64 = 64 * 1024;
/// Boxes looked at before giving up on a damaged or strange file.
const MAX_BOXES: usize = 4096;
/// Nesting followed: `moov/udta/meta/ilst/...` is five deep.
const MAX_DEPTH: usize = 6;

const LOCATION_KEY: &[u8] = b"com.apple.quicktime.location.ISO6709";
const DATE_KEY: &[u8] = b"com.apple.quicktime.creationdate";

/// Where and when the video was recorded, if it says.
pub fn taken<S: FileSource + ?Sized>(source: &S) -> Taken {
    let Ok(file) = source.open_reader() else {
        return Taken::default();
    };
    let Ok(length) = source.length() else {
        return Taken::default();
    };
    let mut walk = Walk {
        file: std::io::BufReader::new(file),
        boxes: 0,
        found: Found::default(),
    };
    // Errors end the walk with what was found before them.
    let _ = walk.children(0, length, 0, Parent::Top);
    let found = walk.found;
    Taken {
        at: found
            .apple_location
            .or(found.xyz)
            .and_then(|text| iso6709(&text)),
        clock: found.apple_date.and_then(|text| clock(&text)),
    }
}

/// The facts for Properties: the position, which shows the video on the map.
pub fn details<S: FileSource + ?Sized>(source: &S) -> Vec<Detail> {
    taken(source)
        .at
        .and_then(|at| {
            crate::inspection::detail("Position", format!("{:.5}, {:.5}", at.lat, at.lon))
        })
        .map(crate::inspection::on_map)
        .into_iter()
        .collect()
}

#[derive(Default)]
struct Found {
    xyz: Option<String>,
    apple_location: Option<String>,
    apple_date: Option<String>,
    /// `moov/meta/keys`, in order: the `ilst` items name them by index.
    keys: Vec<Vec<u8>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Parent {
    Top,
    Moov,
    Udta,
    Meta,
    Ilst,
}

struct Walk<R> {
    file: R,
    boxes: usize,
    found: Found,
}

impl<R: Read + Seek> Walk<R> {
    /// The boxes between `start` and `end`, inside `parent`.
    fn children(
        &mut self,
        start: u64,
        end: u64,
        depth: usize,
        parent: Parent,
    ) -> std::io::Result<()> {
        let mut at = start;
        while at + 8 <= end {
            self.boxes += 1;
            if self.boxes > MAX_BOXES || depth > MAX_DEPTH {
                return Ok(());
            }
            self.file.seek(SeekFrom::Start(at))?;
            let mut header = [0u8; 8];
            self.file.read_exact(&mut header)?;
            let size32 = u64::from(u32::from_be_bytes([
                header[0], header[1], header[2], header[3],
            ]));
            let kind: [u8; 4] = [header[4], header[5], header[6], header[7]];
            let (size, head) = match size32 {
                // To the end of its parent.
                0 => (end - at, 8),
                1 => {
                    let mut large = [0u8; 8];
                    self.file.read_exact(&mut large)?;
                    (u64::from_be_bytes(large), 16)
                }
                size => (size, 8),
            };
            // A 64-bit size the file chose: its end may not even be a number.
            if size < head || at.checked_add(size).is_none_or(|box_end| box_end > end) {
                return Ok(());
            }
            let body = (at + head, at + size);
            self.enter(kind, body, depth, parent)?;
            at += size;
        }
        Ok(())
    }

    fn enter(
        &mut self,
        kind: [u8; 4],
        (from, to): (u64, u64),
        depth: usize,
        parent: Parent,
    ) -> std::io::Result<()> {
        match (parent, &kind) {
            (Parent::Top, b"moov") => self.children(from, to, depth + 1, Parent::Moov),
            (Parent::Moov, b"udta") => self.children(from, to, depth + 1, Parent::Udta),
            // QuickTime's `meta` has no version and flags; ISO's has four
            // bytes of them before its children.
            (Parent::Moov | Parent::Udta, b"meta") => {
                let skip = if self.peek_is_box(from, to)? { 0 } else { 4 };
                self.children(from + skip, to, depth + 1, Parent::Meta)
            }
            (Parent::Udta, b"\xa9xyz") => {
                let bytes = self.leaf(from, to)?;
                // A 16-bit length and a 16-bit language, then the text.
                if bytes.len() > 4 {
                    self.found.xyz = Some(String::from_utf8_lossy(&bytes[4..]).into_owned());
                }
                Ok(())
            }
            (Parent::Meta, b"keys") => {
                let bytes = self.leaf(from, to)?;
                self.found.keys = keys(&bytes);
                Ok(())
            }
            (Parent::Meta, b"ilst") => self.children(from, to, depth + 1, Parent::Ilst),
            (Parent::Ilst, index) => {
                // An item's type is the 1-based index of its key.
                let index = u32::from_be_bytes(*index) as usize;
                let Some(key) = index
                    .checked_sub(1)
                    .and_then(|i| self.found.keys.get(i))
                    .cloned()
                else {
                    return Ok(());
                };
                if key != LOCATION_KEY && key != DATE_KEY {
                    return Ok(());
                }
                let bytes = self.leaf(from, to)?;
                if let Some(text) = data_text(&bytes) {
                    if key == LOCATION_KEY {
                        self.found.apple_location = Some(text);
                    } else {
                        self.found.apple_date = Some(text);
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Whether the body starts with a box header (a plausible size and a
    /// printable type) rather than version and flags.
    fn peek_is_box(&mut self, from: u64, to: u64) -> std::io::Result<bool> {
        if to - from < 8 {
            return Ok(false);
        }
        self.file.seek(SeekFrom::Start(from))?;
        let mut head = [0u8; 8];
        self.file.read_exact(&mut head)?;
        let size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
        Ok(size >= 8 && size <= to - from && head[4..].iter().all(u8::is_ascii_alphanumeric))
    }

    /// A small box's body; empty when it is not small.
    fn leaf(&mut self, from: u64, to: u64) -> std::io::Result<Vec<u8>> {
        if to - from > MAX_LEAF {
            return Ok(Vec::new());
        }
        self.file.seek(SeekFrom::Start(from))?;
        let mut bytes = vec![0; usize::try_from(to - from).unwrap_or(0)];
        self.file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

/// A `keys` box: version and flags, a count, then sized entries of a
/// namespace and a name.
fn keys(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut keys = Vec::new();
    let Some(count) = bytes.get(4..8) else {
        return keys;
    };
    let count = u32::from_be_bytes([count[0], count[1], count[2], count[3]]);
    let mut at = 8usize;
    for _ in 0..count.min(1024) {
        let Some(size) = bytes.get(at..at + 4) else {
            break;
        };
        let size = u32::from_be_bytes([size[0], size[1], size[2], size[3]]) as usize;
        if size < 8 || at + size > bytes.len() {
            break;
        }
        keys.push(bytes[at + 8..at + size].to_vec());
        at += size;
    }
    keys
}

/// The text of an item's `data` box: a size, `data`, a type, a locale,
/// then the value.
fn data_text(bytes: &[u8]) -> Option<String> {
    let size = u32::from_be_bytes(bytes.get(0..4)?.try_into().ok()?) as usize;
    if bytes.get(4..8)? != b"data" || size < 16 || size > bytes.len() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes[16..size]).into_owned())
}

/// An ISO 6709 point, `±DD.DDDD±DDD.DDDD` then an optional altitude and a
/// CRS, in decimal degrees as phones write it.
fn iso6709(text: &str) -> Option<LatLon> {
    let text = text.trim();
    let mut numbers = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if c == '+' || c == '-' || c == '/' {
            if let Some(s) = start.take() {
                numbers.push(&text[s..i]);
            }
            if c == '/' {
                break;
            }
            start = Some(i);
        }
    }
    if let Some(s) = start {
        numbers.push(&text[s..]);
    }
    let lat: f64 = numbers.first()?.parse().ok()?;
    let lon: f64 = numbers.get(1)?.parse().ok()?;
    if lat == 0. && lon == 0. {
        return None;
    }
    LatLon::new(lat, lon)
}

/// `2026-07-01T08:30:00+0200` as the digits of its local clock, an order
/// like a picture's (`exif::Taken::clock`).
fn clock(text: &str) -> Option<u64> {
    let digits: String = text.chars().take(19).filter(char::is_ascii_digit).collect();
    (digits.len() == 14).then(|| digits.parse().ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(kind: [u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(body.len() + 8)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        out.extend_from_slice(&kind);
        out.extend_from_slice(body);
        out
    }

    fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut file, bytes).unwrap();
        file
    }

    #[test]
    fn an_android_video_says_where_it_was_recorded() {
        let mut xyz = vec![0, 25, 0x15, 0xc7];
        xyz.extend_from_slice(b"+10.5000+020.2500+012.000/");
        let moov = boxed(
            *b"moov",
            &[
                boxed(*b"mvhd", &[0; 100]),
                boxed(*b"udta", &boxed(*b"\xa9xyz", &xyz)),
            ]
            .concat(),
        );
        // The media data before the header, as a camera writes it.
        let video = [
            boxed(*b"ftyp", b"isom0000"),
            boxed(*b"mdat", &[7; 5000]),
            moov,
        ]
        .concat();
        let at = taken(file(&video).path()).at.unwrap();
        assert!(
            (at.lat - 10.5).abs() < 1e-9 && (at.lon - 20.25).abs() < 1e-9,
            "{at:?}"
        );
    }

    #[test]
    fn an_iphone_video_says_where_and_when_through_its_keys() {
        let key = |name: &[u8]| {
            let mut out = u32::try_from(name.len() + 8)
                .unwrap()
                .to_be_bytes()
                .to_vec();
            out.extend_from_slice(b"mdta");
            out.extend_from_slice(name);
            out
        };
        let mut keys = vec![0, 0, 0, 0, 0, 0, 0, 2];
        keys.extend(key(b"com.apple.quicktime.make"));
        keys.extend(key(LOCATION_KEY));
        keys.extend(key(DATE_KEY));
        keys[7] = 3;
        let data = |text: &[u8]| boxed(*b"data", &[&[0, 0, 0, 1, 0, 0, 0, 0][..], text].concat());
        let ilst = [
            boxed(2u32.to_be_bytes(), &data(b"-33.2500-070.5000+510.2/")),
            boxed(3u32.to_be_bytes(), &data(b"2026-07-01T08:30:00+0200")),
        ]
        .concat();
        let meta = boxed(
            *b"meta",
            &[
                boxed(*b"hdlr", &[0; 24]),
                boxed(*b"keys", &keys),
                boxed(*b"ilst", &ilst),
            ]
            .concat(),
        );
        let video = [boxed(*b"ftyp", b"qt  0000"), boxed(*b"moov", &meta)].concat();
        let taken = taken(file(&video).path());
        let at = taken.at.unwrap();
        assert!(
            (at.lat + 33.25).abs() < 1e-9 && (at.lon + 70.5).abs() < 1e-9,
            "{at:?}"
        );
        assert_eq!(taken.clock, Some(20_260_701_083_000));
    }

    #[test]
    fn a_video_without_a_position_or_with_a_lying_box_says_nothing() {
        let plain = [
            boxed(*b"ftyp", b"isom0000"),
            boxed(*b"moov", &boxed(*b"mvhd", &[0; 100])),
        ]
        .concat();
        assert_eq!(taken(file(&plain).path()), Taken::default());
        // A box claiming more than the file holds.
        let mut lying = boxed(*b"moov", &[0; 16]);
        lying[3] = 0xff;
        assert_eq!(taken(file(&lying).path()), Taken::default());
        assert!(iso6709("+00.0000+000.0000/").is_none());
    }

    fn android_video() -> Vec<u8> {
        let mut xyz = vec![0, 25, 0x15, 0xc7];
        xyz.extend_from_slice(b"+10.5000+020.2500+012.000/");
        let moov = boxed(
            *b"moov",
            &[
                boxed(*b"mvhd", &[0; 100]),
                boxed(*b"udta", &boxed(*b"\xa9xyz", &xyz)),
            ]
            .concat(),
        );
        [
            boxed(*b"ftyp", b"isom0000"),
            boxed(*b"mdat", &[7; 64]),
            moov,
        ]
        .concat()
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]
        /// A video's boxes as no camera writes them: never a panic.
        #[test]
        fn any_video_is_read_without_a_panic(bytes in proptest::prop_oneof![
            skwir_core::testing::mutated(android_video()),
            skwir_core::testing::bytes(512),
        ]) {
            let video = file(&bytes);
            let _ = taken(video.path());
            let _ = details(video.path());
        }

        #[test]
        fn any_position_text_is_read_without_a_panic(text in proptest::prop_oneof![
            skwir_core::testing::mutated_text("+10.5000+020.2500+012.000/"),
            skwir_core::testing::mutated_text("2026-03-12T08:00:00+0100"),
            proptest::prelude::any::<String>(),
        ]) {
            let _ = iso6709(&text);
            let _ = clock(&text);
        }
    }
}
