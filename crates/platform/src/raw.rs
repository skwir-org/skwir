//! A camera's raw file, shown by the JPEG it carries (ADR-148): the
//! picture the camera made of its sensor, full size in most raw files, as
//! file managers and image viewers show it. The JPEG then takes the JPEG
//! path (`picture`): its reduced scales, a region, read ahead and kept.
//! A raw file's own sensor data is never developed here.
//!
//! Read from the container without decoding anything: the TIFF structure
//! most raw files are (Nikon, Canon CR2, Sony, Adobe DNG, Pentax, Samsung,
//! Panasonic), Fuji's RAF header, Canon CR3's first track. The largest
//! JPEG found, by the size its frame header says, is the one shown; the
//! raw file's own Orientation is written into it where it says none.
use std::io::{Read, Seek, SeekFrom};

/// The raw types read here: those whose containers this module knows.
/// Any other (an Olympus ORF, whose preview hides in its maker note, a
/// Canon CRW, a Sigma X3F) stays with the desktop's thumbnailer.
static RAW: crate::mime::Types = crate::mime::Types::new(&[
    "image/x-nikon-nef",
    "image/x-nikon-nrw",
    "image/x-canon-cr2",
    "image/x-canon-cr3",
    "image/x-sony-arw",
    "image/x-sony-sr2",
    "image/x-sony-srf",
    "image/x-adobe-dng",
    "image/x-pentax-pef",
    "image/x-samsung-srw",
    "image/x-panasonic-rw2",
    "image/x-panasonic-raw2",
    "image/x-fuji-raf",
]);

/// Whether a file of type `mime` is a raw file this module reads.
pub fn is_raw(mime: &str) -> bool {
    RAW.contains(mime)
}

/// The most read of a raw file's JPEG: a full-size one is a few megabytes.
const JPEG_BYTES: u64 = 64 << 20;
/// Directories walked at most: a file's chain of IFDs could loop.
const DIRECTORIES: usize = 64;

/// The largest JPEG `raw` carries, its Orientation the raw file's where it
/// says none. An error when it carries none.
pub fn embedded_jpeg<R: Read + Seek>(raw: &mut R) -> anyhow::Result<Vec<u8>> {
    let mut head = [0u8; 16];
    raw.seek(SeekFrom::Start(0))?;
    raw.read_exact(&mut head)?;
    let (found, orientation) = if head.starts_with(b"FUJIFILMCCD-RAW") {
        (raf(raw)?, None)
    } else if &head[4..8] == b"ftyp" && &head[8..11] == b"crx" {
        (cr3(raw)?, None)
    } else {
        tiff(raw, &head)?
    };
    let (offset, length) = found
        .into_iter()
        .filter(|&(_, length)| length > 4 && length <= JPEG_BYTES)
        .filter_map(|(offset, length)| {
            let pixels = jpeg_pixels(raw, offset, length)?;
            Some((pixels, offset, length))
        })
        .max_by_key(|&(pixels, ..)| pixels)
        .map(|(_, offset, length)| (offset, length))
        .ok_or_else(|| anyhow::anyhow!("no picture in this raw file"))?;
    let mut jpeg = vec![0; usize::try_from(length)?];
    raw.seek(SeekFrom::Start(offset))?;
    raw.read_exact(&mut jpeg)?;
    Ok(match orientation {
        Some(turn) if turn != 1 && !has_exif(&jpeg) => with_orientation(&jpeg, turn),
        _ => jpeg,
    })
}

/// The pixels a JPEG at `offset` says it has (its frame header), `None`
/// where what lies there is no JPEG.
fn jpeg_pixels<R: Read + Seek>(raw: &mut R, offset: u64, length: u64) -> Option<u64> {
    raw.seek(SeekFrom::Start(offset)).ok()?;
    let mut head = vec![0u8; usize::try_from(length.min(256 << 10)).ok()?];
    raw.read_exact(&mut head).ok()?;
    if !head.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2;
    while at + 9 < head.len() {
        if head[at] != 0xFF {
            return None;
        }
        let marker = head[at + 1];
        let size = usize::from(u16::from_be_bytes([head[at + 2], head[at + 3]]));
        // A frame header: its height and width.
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let height = u64::from(u16::from_be_bytes([head[at + 5], head[at + 6]]));
            let width = u64::from(u16::from_be_bytes([head[at + 7], head[at + 8]]));
            return Some(width * height);
        }
        at += 2 + size;
    }
    None
}

/// Whether a JPEG's segments before its frame hold EXIF.
fn has_exif(jpeg: &[u8]) -> bool {
    let mut at = 2;
    while at + 10 < jpeg.len() && jpeg[at] == 0xFF {
        let marker = jpeg[at + 1];
        let size = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        if marker == 0xE1 && jpeg[at + 4..].starts_with(b"Exif\0\0") {
            return true;
        }
        if marker == 0xDA {
            return false;
        }
        at += 2 + size;
    }
    false
}

/// `jpeg` with an EXIF segment saying Orientation `turn` right after its
/// start: how the raw file says to turn what its camera stored.
fn with_orientation(jpeg: &[u8], turn: u16) -> Vec<u8> {
    let mut tiff = b"II*\0\x08\0\0\0".to_vec();
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&turn.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    let mut segment = b"Exif\0\0".to_vec();
    segment.extend_from_slice(&tiff);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&u16::try_from(segment.len() + 2).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&segment);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// A TIFF's byte order.
#[derive(Clone, Copy)]
struct Order {
    big: bool,
}

impl Order {
    fn u16(self, bytes: [u8; 2]) -> u16 {
        if self.big {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        }
    }
    fn u32(self, bytes: [u8; 4]) -> u32 {
        if self.big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    }
}

/// One entry of an IFD.
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    /// Its value or offset, as stored.
    value: [u8; 4],
}

/// The JPEGs a TIFF-shaped raw file holds (offset and length), and its
/// IFD0's Orientation.
fn tiff<R: Read + Seek>(
    raw: &mut R,
    head: &[u8; 16],
) -> anyhow::Result<(Vec<(u64, u64)>, Option<u16>)> {
    let order = match &head[..2] {
        b"II" => Order { big: false },
        b"MM" => Order { big: true },
        _ => anyhow::bail!("not a raw file this reads"),
    };
    // 42, Olympus's "RO", Panasonic's 0x55.
    let magic = order.u16([head[2], head[3]]);
    anyhow::ensure!(
        matches!(magic, 42 | 0x4F52 | 0x5352 | 0x55),
        "not a raw file this reads"
    );
    let first = u64::from(order.u32([head[4], head[5], head[6], head[7]]));
    let mut found = Vec::new();
    let mut orientation = None;
    let mut queue = vec![(first, true)];
    let mut seen = std::collections::BTreeSet::new();
    while let Some((at, chained)) = queue.pop() {
        if at == 0 || !seen.insert(at) || seen.len() > DIRECTORIES {
            continue;
        }
        let Ok((entries, next)) = directory(raw, order, at) else {
            continue;
        };
        if chained && next != 0 {
            queue.push((next, true));
        }
        let value = |entry: &Entry| -> u64 {
            match entry.kind {
                3 => u64::from(order.u16([entry.value[0], entry.value[1]])),
                _ => u64::from(order.u32(entry.value)),
            }
        };
        let tag = |tag: u16| entries.iter().find(|entry| entry.tag == tag);
        if at == first
            && let Some(turn) = tag(0x0112)
        {
            orientation = u16::try_from(value(turn)).ok();
        }
        // JPEGInterchangeFormat and its length.
        if let (Some(start), Some(length)) = (tag(0x0201), tag(0x0202)) {
            found.push((value(start), value(length)));
        }
        // One strip, compressed as a JPEG (old-style 6, new 7).
        if let (Some(compression), Some(start), Some(length)) =
            (tag(0x0103), tag(0x0111), tag(0x0117))
            && matches!(value(compression), 6 | 7)
            && start.count == 1
        {
            found.push((value(start), value(length)));
        }
        // Panasonic's JpgFromRaw: the JPEG itself, where the entry points.
        if let Some(jpeg) = tag(0x002E)
            && jpeg.count > 4
        {
            found.push((u64::from(order.u32(jpeg.value)), u64::from(jpeg.count)));
        }
        // SubIFDs (a NEF's, a DNG's full-size preview) and EXIF's.
        for sub in [0x014A, 0x8769] {
            if let Some(entry) = tag(sub) {
                for offset in offsets(raw, order, entry) {
                    queue.push((offset, sub == 0x014A));
                }
            }
        }
    }
    Ok((found, orientation))
}

/// The IFD at `at`: its entries and the next one's offset.
fn directory<R: Read + Seek>(
    raw: &mut R,
    order: Order,
    at: u64,
) -> anyhow::Result<(Vec<Entry>, u64)> {
    raw.seek(SeekFrom::Start(at))?;
    let mut count = [0u8; 2];
    raw.read_exact(&mut count)?;
    let count = order.u16(count).min(1024);
    let mut bytes = vec![0u8; usize::from(count) * 12 + 4];
    raw.read_exact(&mut bytes)?;
    let entries = bytes
        .as_chunks::<12>()
        .0
        .iter()
        .map(|entry| Entry {
            tag: order.u16([entry[0], entry[1]]),
            kind: order.u16([entry[2], entry[3]]),
            count: order.u32([entry[4], entry[5], entry[6], entry[7]]),
            value: [entry[8], entry[9], entry[10], entry[11]],
        })
        .collect();
    let tail = &bytes[bytes.len() - 4..];
    Ok((
        entries,
        u64::from(order.u32([tail[0], tail[1], tail[2], tail[3]])),
    ))
}

/// The offsets an entry of LONG or IFD offsets holds: inline for one,
/// where it points for more.
fn offsets<R: Read + Seek>(raw: &mut R, order: Order, entry: &Entry) -> Vec<u64> {
    if entry.count <= 1 {
        return vec![u64::from(order.u32(entry.value))];
    }
    let count = entry.count.min(16) as usize;
    let mut bytes = vec![0u8; count * 4];
    if raw
        .seek(SeekFrom::Start(u64::from(order.u32(entry.value))))
        .and_then(|_| raw.read_exact(&mut bytes))
        .is_err()
    {
        return Vec::new();
    }
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u64::from(order.u32(b)))
        .collect()
}

/// A Fuji RAF's JPEG: its offset and length in the header, big-endian.
fn raf<R: Read + Seek>(raw: &mut R) -> anyhow::Result<Vec<(u64, u64)>> {
    let mut header = [0u8; 8];
    raw.seek(SeekFrom::Start(84))?;
    raw.read_exact(&mut header)?;
    let offset = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let length = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    Ok(vec![(u64::from(offset), u64::from(length))])
}

/// A Canon CR3's JPEG: the first track's one sample (ISO's media format:
/// `moov` / `trak` / `mdia` / `minf` / `stbl`, its size in `stsz`, its
/// place in `co64`).
fn cr3<R: Read + Seek>(raw: &mut R) -> anyhow::Result<Vec<(u64, u64)>> {
    let end = raw.seek(SeekFrom::End(0))?;
    let moov = child(raw, 0, end, *b"moov")?;
    let trak = child(raw, moov.0, moov.1, *b"trak")?;
    let mdia = child(raw, trak.0, trak.1, *b"mdia")?;
    let minf = child(raw, mdia.0, mdia.1, *b"minf")?;
    let stbl = child(raw, minf.0, minf.1, *b"stbl")?;
    let stsz = child(raw, stbl.0, stbl.1, *b"stsz")?;
    let co64 = child(raw, stbl.0, stbl.1, *b"co64")?;
    let mut sizes = [0u8; 12];
    raw.seek(SeekFrom::Start(stsz.0))?;
    raw.read_exact(&mut sizes)?;
    // Version and flags, then the size every sample has (0: each its own).
    let mut size = u64::from(u32::from_be_bytes([sizes[4], sizes[5], sizes[6], sizes[7]]));
    if size == 0 {
        let mut first = [0u8; 4];
        raw.read_exact(&mut first)?;
        size = u64::from(u32::from_be_bytes(first));
    }
    let mut chunk = [0u8; 16];
    raw.seek(SeekFrom::Start(co64.0))?;
    raw.read_exact(&mut chunk)?;
    let offset = u64::from_be_bytes(chunk[8..16].try_into()?);
    Ok(vec![(offset, size)])
}

/// The box named `name` among those from `start` to `end`: where its
/// content starts and ends.
fn child<R: Read + Seek>(
    raw: &mut R,
    start: u64,
    end: u64,
    name: [u8; 4],
) -> anyhow::Result<(u64, u64)> {
    let mut at = start;
    for _ in 0..256 {
        anyhow::ensure!(at + 8 <= end, "no {} box", String::from_utf8_lossy(&name));
        let mut head = [0u8; 16];
        raw.seek(SeekFrom::Start(at))?;
        raw.read_exact(&mut head[..8])?;
        let mut size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
        let mut content = at + 8;
        if size == 1 {
            raw.read_exact(&mut head[8..16])?;
            size = u64::from_be_bytes(head[8..16].try_into()?);
            content += 8;
        } else if size == 0 {
            size = end - at;
        }
        anyhow::ensure!(size >= content - at, "a box shorter than its header");
        if head[4..8] == name {
            return Ok((content, at + size));
        }
        at += size;
    }
    anyhow::bail!("no {} box", String::from_utf8_lossy(&name))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// A JPEG of `width` × `height`, one colour.
    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
            .encode_image(&image::RgbImage::from_pixel(
                width,
                height,
                image::Rgb([200, 40, 40]),
            ))
            .unwrap();
        out
    }

    /// A little-endian TIFF whose `IFD0` says Orientation `turn` and points
    /// at a small JPEG, and whose `SubIFD` holds a larger one in a strip:
    /// as a `NEF` or a `DNG` lays its previews.
    fn tiff_raw(turn: u16) -> Vec<u8> {
        let (small, large) = (jpeg(40, 30), jpeg(160, 120));
        let ifd0 = 8u32;
        let sub = ifd0 + 2 + 4 * 12 + 4;
        let data = sub + 2 + 4 * 12 + 4;
        let small_at = data;
        let large_at = small_at + small.len() as u32;
        let entry = |tag: u16, kind: u16, count: u32, value: u32| {
            let mut e = tag.to_le_bytes().to_vec();
            e.extend_from_slice(&kind.to_le_bytes());
            e.extend_from_slice(&count.to_le_bytes());
            e.extend_from_slice(&value.to_le_bytes());
            e
        };
        let mut out = b"II*\0".to_vec();
        out.extend_from_slice(&ifd0.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend(entry(0x0112, 3, 1, u32::from(turn)));
        out.extend(entry(0x014A, 13, 1, sub));
        out.extend(entry(0x0201, 4, 1, small_at));
        out.extend(entry(0x0202, 4, 1, small.len() as u32));
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend(entry(0x0103, 3, 1, 7));
        out.extend(entry(0x0111, 4, 1, large_at));
        out.extend(entry(0x0117, 4, 1, large.len() as u32));
        out.extend(entry(0x00FE, 4, 1, 0));
        out.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(out.len() as u32, data);
        out.extend_from_slice(&small);
        out.extend_from_slice(&large);
        out
    }

    fn size_of(jpeg: &[u8]) -> (u32, u32) {
        crate::picture::size(
            image::ImageReader::new(Cursor::new(jpeg))
                .with_guessed_format()
                .unwrap(),
        )
        .unwrap()
    }

    /// The largest JPEG a TIFF-shaped raw file holds is the one shown,
    /// turned as the raw file says.
    #[test]
    fn a_raw_file_shows_its_largest_jpeg_turned_as_it_says() {
        let raw = tiff_raw(1);
        let shown = embedded_jpeg(&mut Cursor::new(&raw)).unwrap();
        assert_eq!(size_of(&shown), (160, 120));
        // Turned a quarter (6): the JPEG says so, and stands upright.
        let raw = tiff_raw(6);
        let shown = embedded_jpeg(&mut Cursor::new(&raw)).unwrap();
        assert_eq!(size_of(&shown), (120, 160));
    }

    /// A Fuji RAF's JPEG is where its header says.
    #[test]
    fn a_raf_shows_the_jpeg_its_header_points_at() {
        let inner = jpeg(64, 48);
        let mut raf = b"FUJIFILMCCD-RAW 0201FF383501".to_vec();
        raf.resize(84, 0);
        raf.extend_from_slice(&100u32.to_be_bytes());
        raf.extend_from_slice(&(inner.len() as u32).to_be_bytes());
        raf.resize(100, 0);
        raf.extend_from_slice(&inner);
        let shown = embedded_jpeg(&mut Cursor::new(&raf)).unwrap();
        assert_eq!(size_of(&shown), (64, 48));
    }

    /// A Canon CR3's JPEG is its first track's sample.
    #[test]
    fn a_cr3_shows_its_first_track() {
        let inner = jpeg(80, 60);
        let boxed = |name: &[u8; 4], content: &[u8]| {
            let mut b = ((content.len() + 8) as u32).to_be_bytes().to_vec();
            b.extend_from_slice(name);
            b.extend_from_slice(content);
            b
        };
        let ftyp = boxed(b"ftyp", b"crx \0\0\0\x01crx isom");
        // The JPEG's place is known once the boxes before it are laid out.
        let stsz = |size: u32| {
            let mut c = vec![0u8; 4];
            c.extend_from_slice(&size.to_be_bytes());
            c.extend_from_slice(&1u32.to_be_bytes());
            boxed(b"stsz", &c)
        };
        let co64 = |at: u64| {
            let mut c = vec![0u8; 4];
            c.extend_from_slice(&1u32.to_be_bytes());
            c.extend_from_slice(&at.to_be_bytes());
            boxed(b"co64", &c)
        };
        let moov = |at: u64| {
            let stbl = boxed(b"stbl", &[stsz(inner.len() as u32), co64(at)].concat());
            let minf = boxed(b"minf", &stbl);
            let mdia = boxed(b"mdia", &minf);
            let trak = boxed(b"trak", &mdia);
            boxed(b"moov", &trak)
        };
        let at = (ftyp.len() + moov(0).len() + 8) as u64;
        let mut cr3 = ftyp;
        cr3.extend(moov(at));
        cr3.extend(boxed(b"mdat", &inner));
        let shown = embedded_jpeg(&mut Cursor::new(&cr3)).unwrap();
        assert_eq!(size_of(&shown), (80, 60));
    }

    /// A file that is no raw file this reads, or holds no JPEG: an error.
    #[test]
    fn a_file_without_a_jpeg_has_none() {
        assert!(embedded_jpeg(&mut Cursor::new(b"not a raw file at all".to_vec())).is_err());
        let mut empty = b"II*\0\x08\0\0\0".to_vec();
        empty.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        assert!(embedded_jpeg(&mut Cursor::new(empty)).is_err());
    }
}
