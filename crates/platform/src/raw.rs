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
    let chosen = chosen(raw)?;
    let mut jpeg = vec![0; usize::try_from(chosen.length)?];
    raw.seek(SeekFrom::Start(chosen.offset))?;
    raw.read_exact(&mut jpeg)?;
    Ok(match chosen.orientation {
        Some(turn) if turn != 1 && orientation_of(&jpeg).is_none() => with_orientation(&jpeg, turn),
        _ => jpeg,
    })
}

/// The size of the picture `raw` shows, turned as `embedded_jpeg`'s JPEG
/// is: read from its JPEG's first bytes alone, never the JPEG whole (a
/// cached thumbnail standing in asks it, before the picture is decoded).
pub fn shown_size<R: Read + Seek>(raw: &mut R) -> anyhow::Result<(u32, u32)> {
    let chosen = chosen(raw)?;
    let head = jpeg_head(raw, chosen.offset, chosen.length)
        .ok_or_else(|| anyhow::anyhow!("no picture in this raw file"))?;
    let Frame::Picture(width, height) = frame(&head) else {
        anyhow::bail!("no picture in this raw file");
    };
    Ok(match orientation_of(&head).or(chosen.orientation) {
        Some(5..=8) => (height, width),
        _ => (width, height),
    })
}

/// The JPEG a raw file shows, as `chosen` finds it.
struct Chosen {
    offset: u64,
    length: u64,
    /// The raw file's Orientation, which its JPEG may not say.
    orientation: Option<u16>,
}

/// The largest JPEG `raw` carries, by its frame header's size, and its
/// Orientation; an error where it carries none.
fn chosen<R: Read + Seek>(raw: &mut R) -> anyhow::Result<Chosen> {
    let mut head = [0u8; 16];
    raw.seek(SeekFrom::Start(0))?;
    raw.read_exact(&mut head)?;
    let (found, orientation) = if head.starts_with(b"FUJIFILMCCD-RAW") {
        (raf(raw)?, None)
    } else if is_cr3(&head) {
        cr3(raw)?
    } else {
        let walk = tiff(raw, &head)?;
        (walk.found, walk.orientation)
    };
    let (offset, length) = found
        .into_iter()
        .filter(|&(_, length)| plausible(length))
        .filter_map(|(offset, length)| {
            let pixels = jpeg_pixels(raw, offset, length)?;
            Some((pixels, offset, length))
        })
        .max_by_key(|&(pixels, ..)| pixels)
        .map(|(_, offset, length)| (offset, length))
        .ok_or_else(|| anyhow::anyhow!("no picture in this raw file"))?;
    Ok(Chosen {
        offset,
        length,
        orientation,
    })
}

/// Whether the raw file whose first bytes are `head` may carry a JPEG to
/// show (ADR-148 amended): false only where its container, read in `head`
/// alone, is known to hold none, as an Android phone's DNG holds only
/// uncompressed pictures. Such a file is left to the desktop's
/// thumbnailer and `ImageMagick`, as a type Skwir does not read
/// (`thumb_providers::decodes_file`). A Fuji RAF and a Canon CR3 always
/// carry one; a TIFF-shaped file names its JPEGs in its directories, and
/// says it has none only where every directory it points to lies in
/// `head`, and every JPEG they name either lies past it or is not one to
/// show (a lossless one, its sensor data). What is no raw file this
/// module reads has none.
pub fn may_carry_jpeg(head: &[u8]) -> bool {
    let Some(start) = head.first_chunk::<16>() else {
        return false;
    };
    if start.starts_with(b"FUJIFILMCCD-RAW") || is_cr3(start) {
        return true;
    }
    tiff(&mut std::io::Cursor::new(head), start).is_ok_and(|walk| {
        !walk.complete
            || walk
                .found
                .iter()
                .any(|&(offset, length)| plausible(length) && may_show(head, offset, length))
    })
}

/// Whether the JPEG of `length` at `offset` may be one to show, as far as
/// `head`, a file's first bytes, tells: no only where what of it they hold
/// says it is none (no JPEG, a lossless one) or holds it whole without a
/// frame.
fn may_show(head: &[u8], offset: u64, length: u64) -> bool {
    let Some(rest) = usize::try_from(offset).ok().and_then(|at| head.get(at..)) else {
        return true;
    };
    let within = rest
        .len()
        .min(usize::try_from(length).unwrap_or(usize::MAX));
    match frame(&rest[..within]) {
        Frame::Picture(..) => true,
        Frame::NotAPicture => false,
        Frame::Unknown => (within as u64) < length,
    }
}

/// Whether a file's first bytes begin a Canon CR3.
fn is_cr3(head: &[u8; 16]) -> bool {
    &head[4..8] == b"ftyp" && &head[8..11] == b"crx"
}

/// Whether `length` may be a JPEG's: more than its markers, no more than
/// what is read of one.
fn plausible(length: u64) -> bool {
    length > 4 && length <= JPEG_BYTES
}

/// What a JPEG's first bytes say of its frame.
enum Frame {
    /// A picture of this width and height, as stored.
    Picture(u32, u32),
    /// No picture to show: no JPEG, or a lossless one, a raw file's sensor
    /// data (a CR2's last directory, a DNG's raw image in one strip).
    NotAPicture,
    /// Not said within the bytes held.
    Unknown,
}

/// What `head`, a JPEG's first bytes, says of its frame (its header).
fn frame(head: &[u8]) -> Frame {
    if head.len() < 2 {
        return Frame::Unknown;
    }
    if !head.starts_with(&[0xFF, 0xD8]) {
        return Frame::NotAPicture;
    }
    let mut at = 2;
    while at + 9 < head.len() {
        if head[at] != 0xFF {
            return Frame::NotAPicture;
        }
        let marker = head[at + 1];
        let size = usize::from(u16::from_be_bytes([head[at + 2], head[at + 3]]));
        if matches!(marker, 0xC3 | 0xC7 | 0xCB | 0xCF) {
            return Frame::NotAPicture;
        }
        // A frame header: its height and width.
        if matches!(
            marker,
            0xC0..=0xC2 | 0xC5 | 0xC6 | 0xC9 | 0xCA | 0xCD | 0xCE
        ) {
            let height = u16::from_be_bytes([head[at + 5], head[at + 6]]);
            let width = u16::from_be_bytes([head[at + 7], head[at + 8]]);
            return Frame::Picture(u32::from(width), u32::from(height));
        }
        at += 2 + size;
    }
    Frame::Unknown
}

/// The first bytes of the JPEG at `offset` (its header, and what comes
/// before its frame: 256 KiB at most), `None` where they cannot be read.
fn jpeg_head<R: Read + Seek>(raw: &mut R, offset: u64, length: u64) -> Option<Vec<u8>> {
    raw.seek(SeekFrom::Start(offset)).ok()?;
    let mut head = vec![0u8; usize::try_from(length.min(256 << 10)).ok()?];
    raw.read_exact(&mut head).ok()?;
    Some(head)
}

/// The pixels a JPEG at `offset` says it has (its frame header), `None`
/// where what lies there is no JPEG to show.
fn jpeg_pixels<R: Read + Seek>(raw: &mut R, offset: u64, length: u64) -> Option<u64> {
    match frame(&jpeg_head(raw, offset, length)?) {
        Frame::Picture(width, height) => Some(u64::from(width) * u64::from(height)),
        Frame::NotAPicture | Frame::Unknown => None,
    }
}

/// A JPEG's segments before its scan: marker, and where the segment
/// starts and ends (its marker included). A marker without a length, or a
/// segment past the end, ends them.
fn segments(jpeg: &[u8]) -> Vec<(u8, usize, usize)> {
    let mut found = Vec::new();
    let mut at = 2;
    while at + 4 <= jpeg.len() && jpeg[at] == 0xFF && !matches!(jpeg[at + 1], 0xDA | 0xD9) {
        let size = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        let end = at + 2 + size;
        if size < 2 || end > jpeg.len() {
            break;
        }
        found.push((jpeg[at + 1], at, end));
        at = end;
    }
    found
}

/// The Orientation a JPEG's EXIF says, `None` where it has no EXIF or its
/// EXIF says none.
fn orientation_of(jpeg: &[u8]) -> Option<u16> {
    segments(jpeg).into_iter().find_map(|(marker, start, end)| {
        if marker != 0xE1 {
            return None;
        }
        exif_orientation(jpeg[start + 4..end].strip_prefix(b"Exif\0\0")?)
    })
}

/// The Orientation of IFD0 in `tiff`, a TIFF held whole (an EXIF block, a
/// CR3's `CMT1`).
fn exif_orientation(tiff: &[u8]) -> Option<u16> {
    let head = tiff.first_chunk::<16>().copied().or_else(|| {
        let mut head = [0u8; 16];
        head.get_mut(..tiff.len())?.copy_from_slice(tiff);
        Some(head)
    })?;
    self::tiff(&mut std::io::Cursor::new(tiff), &head)
        .ok()?
        .orientation
}

/// `jpeg` with an EXIF segment saying Orientation `turn` right after its
/// start, in place of any EXIF it had (which said none): how the raw
/// file says to turn what its camera stored. What the camera wrote of the
/// picture is read from the raw file itself (`exif::details`).
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
    // Each EXIF segment of its own left out: a decoder reads one, maybe
    // the camera's, which says nothing of turning.
    let mut at = 2;
    for (marker, start, end) in segments(jpeg) {
        if marker == 0xE1 && jpeg[start + 4..end].starts_with(b"Exif\0\0") {
            out.extend_from_slice(&jpeg[at..start]);
            at = end;
        }
    }
    out.extend_from_slice(&jpeg[at..]);
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

/// What the directories of a TIFF-shaped raw file say.
struct Walk {
    /// The JPEGs they name: offset and length.
    found: Vec<(u64, u64)>,
    /// IFD0's Orientation.
    orientation: Option<u16>,
    /// Whether every directory they point to was read.
    complete: bool,
}

/// The JPEGs a TIFF-shaped raw file holds, and its IFD0's Orientation.
fn tiff<R: Read + Seek>(raw: &mut R, head: &[u8; 16]) -> anyhow::Result<Walk> {
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
    let mut walk = Walk {
        found: Vec::new(),
        orientation: None,
        complete: true,
    };
    let found = &mut walk.found;
    let mut queue = vec![(first, true)];
    let mut seen = std::collections::BTreeSet::new();
    while let Some((at, chained)) = queue.pop() {
        if at == 0 || !seen.insert(at) {
            continue;
        }
        if seen.len() > DIRECTORIES {
            walk.complete = false;
            continue;
        }
        let Ok((entries, next)) = directory(raw, order, at) else {
            walk.complete = false;
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
            walk.orientation = u16::try_from(value(turn)).ok();
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
                let Some(offsets) = offsets(raw, order, entry) else {
                    walk.complete = false;
                    continue;
                };
                for offset in offsets {
                    queue.push((offset, sub == 0x014A));
                }
            }
        }
    }
    Ok(walk)
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
/// where it points for more. `None` where they cannot be read.
fn offsets<R: Read + Seek>(raw: &mut R, order: Order, entry: &Entry) -> Option<Vec<u64>> {
    if entry.count <= 1 {
        return Some(vec![u64::from(order.u32(entry.value))]);
    }
    let count = entry.count.min(16) as usize;
    let mut bytes = vec![0u8; count * 4];
    raw.seek(SeekFrom::Start(u64::from(order.u32(entry.value))))
        .and_then(|_| raw.read_exact(&mut bytes))
        .ok()?;
    Some(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&b| u64::from(order.u32(b)))
            .collect(),
    )
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

/// Canon's box in a CR3's `moov`, which holds its metadata.
const CANON: [u8; 16] = [
    0x85, 0xc0, 0xb6, 0x87, 0x82, 0x0f, 0x11, 0xe0, 0x81, 0x11, 0xf4, 0xce, 0x46, 0x2b, 0x6a, 0x48,
];
/// The most read of a CR3's `CMT1`, its IFD0: a few kilobytes.
const CMT1_BYTES: u64 = 1 << 20;

/// A Canon CR3's JPEG: the first track's one sample (ISO's media format:
/// `moov` / `trak` / `mdia` / `minf` / `stbl`, its size in `stsz`, its
/// place in `co64`), and its Orientation, which its JPEG does not say.
fn cr3<R: Read + Seek>(raw: &mut R) -> anyhow::Result<(Vec<(u64, u64)>, Option<u16>)> {
    let end = raw.seek(SeekFrom::End(0))?;
    let moov = child(raw, 0, end, *b"moov")?;
    let orientation = cr3_orientation(raw, moov);
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
    Ok((vec![(offset, size)], orientation))
}

/// A CR3's Orientation: its IFD0's, a TIFF Canon keeps in the `CMT1` box
/// of its own `uuid` box in `moov` (`moov` its content's start and end).
fn cr3_orientation<R: Read + Seek>(raw: &mut R, (start, end): (u64, u64)) -> Option<u16> {
    // Canon's among the `uuid` boxes, each named by the 16 bytes it starts
    // with, as many at most as `child` looks through.
    let mut at = start;
    let mut canon = None;
    for _ in 0..256 {
        let found = child(raw, at, end, *b"uuid").ok()?;
        let mut id = [0u8; 16];
        raw.seek(SeekFrom::Start(found.0)).ok()?;
        raw.read_exact(&mut id).ok()?;
        if id == CANON {
            canon = Some(found);
            break;
        }
        at = found.1;
    }
    let canon = canon?;
    let (from, to) = child(raw, canon.0.checked_add(16)?, canon.1, *b"CMT1").ok()?;
    let mut ifd0 = vec![0u8; usize::try_from(to.checked_sub(from)?.min(CMT1_BYTES)).ok()?];
    raw.seek(SeekFrom::Start(from)).ok()?;
    raw.read_exact(&mut ifd0).ok()?;
    exif_orientation(&ifd0)
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
        anyhow::ensure!(
            at.checked_add(8).is_some_and(|header| header <= end),
            "no {} box",
            String::from_utf8_lossy(&name)
        );
        let mut head = [0u8; 16];
        raw.seek(SeekFrom::Start(at))?;
        raw.read_exact(&mut head[..8])?;
        let mut size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
        let mut content = at + 8;
        // Sizes are the file's: a crafted one may say 2^64 - 1.
        if size == 1 {
            raw.read_exact(&mut head[8..16])?;
            size = u64::from_be_bytes(head[8..16].try_into()?);
            content += 8;
        } else if size == 0 {
            size = end - at;
        }
        anyhow::ensure!(size >= content - at, "a box shorter than its header");
        let next = at
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("a box past any file"))?;
        if head[4..8] == name {
            return Ok((content, next));
        }
        at = next;
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

    /// A little-endian TIFF whose one directory says Orientation `turn`:
    /// an EXIF block, a CR3's `CMT1`.
    fn ifd0_turned(turn: u16) -> Vec<u8> {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&u32::from(turn).to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff
    }

    /// A Canon CR3's JPEG is its first track's sample, turned as its
    /// `CMT1` says (Canon's JPEG says nothing of it).
    #[test]
    fn a_cr3_shows_its_first_track() {
        let unturned = cr3_file(None);
        assert_eq!(
            size_of(&embedded_jpeg(&mut Cursor::new(&unturned)).unwrap()),
            (80, 60)
        );
        let turned = cr3_file(Some(6));
        assert_eq!(
            size_of(&embedded_jpeg(&mut Cursor::new(&turned)).unwrap()),
            (60, 80),
            "turned a quarter, as CMT1 says"
        );
    }

    /// A CR3 holding a JPEG of 80 × 60, its `moov` led by Canon's `uuid`
    /// box with a `CMT1` saying Orientation `turn`, where there is one.
    fn cr3_file(turn: Option<u16>) -> Vec<u8> {
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
        // Another's `uuid` box first, then Canon's.
        let canon = turn.map_or_else(Vec::new, |turn| {
            let other = boxed(b"uuid", &[[7u8; 16].as_slice(), b"nothing"].concat());
            let cmt1 = boxed(b"CMT1", &ifd0_turned(turn));
            [other, boxed(b"uuid", &[CANON.as_slice(), &cmt1].concat())].concat()
        });
        let moov = |at: u64| {
            let stbl = boxed(b"stbl", &[stsz(inner.len() as u32), co64(at)].concat());
            let minf = boxed(b"minf", &stbl);
            let mdia = boxed(b"mdia", &minf);
            let trak = boxed(b"trak", &mdia);
            boxed(b"moov", &[canon.clone(), trak].concat())
        };
        let at = (ftyp.len() + moov(0).len() + 8) as u64;
        let mut cr3 = ftyp;
        cr3.extend(moov(at));
        cr3.extend(boxed(b"mdat", &inner));
        cr3
    }

    /// A raw file's JPEG with EXIF that says nothing of turning is turned
    /// as the raw file says, its own EXIF given way to the one saying so;
    /// one whose EXIF says how is left as it says.
    #[test]
    fn a_jpeg_whose_exif_says_no_turn_takes_the_raw_file_s() {
        let picture = jpeg(160, 120);
        let with_exif = |tiff: &[u8]| {
            let segment = [b"Exif\0\0".as_slice(), tiff].concat();
            let mut out = picture[..2].to_vec();
            out.extend_from_slice(&[0xFF, 0xE1]);
            out.extend_from_slice(&((segment.len() + 2) as u16).to_be_bytes());
            out.extend_from_slice(&segment);
            out.extend_from_slice(&picture[2..]);
            out
        };
        // An IFD0 of no entries.
        let silent = with_exif(b"II*\0\x08\0\0\0\0\0\0\0\0\0");
        assert_eq!(orientation_of(&silent), None);
        let turned = with_orientation(&silent, 6);
        assert_eq!(orientation_of(&turned), Some(6));
        let exif = segments(&turned)
            .into_iter()
            .filter(|&(marker, start, end)| {
                marker == 0xE1 && turned[start + 4..end].starts_with(b"Exif\0\0")
            })
            .count();
        assert_eq!(exif, 1, "one EXIF segment");
        assert_eq!(size_of(&turned), (120, 160));
        assert_eq!(orientation_of(&with_exif(&ifd0_turned(3))), Some(3));
    }

    /// A file that is no raw file this reads, or holds no JPEG: an error.
    #[test]
    fn a_file_without_a_jpeg_has_none() {
        assert!(embedded_jpeg(&mut Cursor::new(b"not a raw file at all".to_vec())).is_err());
        let mut empty = b"II*\0\x08\0\0\0".to_vec();
        empty.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        assert!(embedded_jpeg(&mut Cursor::new(empty)).is_err());
    }

    /// A lossless JPEG is a raw file's sensor data, never what it shows,
    /// however large its frame says it is; a box whose size says more
    /// than any file holds is an error, not a panic.
    #[test]
    fn sensor_data_and_boxes_past_any_file_are_not_shown() {
        let small = jpeg(40, 30);
        // A lossless frame header of 4000 x 3000, as a CR2's sensor strip.
        let mut lossless = vec![0xFF, 0xD8, 0xFF, 0xC3, 0, 11, 14];
        lossless.extend_from_slice(&3000u16.to_be_bytes());
        lossless.extend_from_slice(&4000u16.to_be_bytes());
        lossless.extend_from_slice(&[1, 1, 0x11, 0]);
        lossless.resize(64, 0);
        let entry = |tag: u16, value: u32| {
            let mut e = tag.to_le_bytes().to_vec();
            e.extend_from_slice(&4u16.to_le_bytes());
            e.extend_from_slice(&1u32.to_le_bytes());
            e.extend_from_slice(&value.to_le_bytes());
            e
        };
        let data = 8 + 2 + 5 * 12 + 4;
        let mut raw = b"II*\0\x08\0\0\0".to_vec();
        raw.extend_from_slice(&5u16.to_le_bytes());
        raw.extend(entry(0x0103, 6));
        raw.extend(entry(0x0111, data));
        raw.extend(entry(0x0117, lossless.len() as u32));
        raw.extend(entry(0x0201, data + lossless.len() as u32));
        raw.extend(entry(0x0202, small.len() as u32));
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(&lossless);
        raw.extend_from_slice(&small);
        let shown = embedded_jpeg(&mut Cursor::new(&raw)).unwrap();
        assert_eq!(size_of(&shown), (40, 30), "the lossy one");
        assert!(may_carry_jpeg(&raw));
        // Its sensor strip alone, in its first bytes: none to show.
        let at = 8 + 2 + 3 * 12 + 4;
        let mut sensor = b"II*\0\x08\0\0\0".to_vec();
        sensor.extend_from_slice(&3u16.to_le_bytes());
        sensor.extend(entry(0x0103, 6));
        sensor.extend(entry(0x0111, at));
        sensor.extend(entry(0x0117, lossless.len() as u32));
        sensor.extend_from_slice(&0u32.to_le_bytes());
        sensor.extend_from_slice(&lossless);
        assert!(
            !may_carry_jpeg(&sensor),
            "a lossless strip is no JPEG to show"
        );
        // A CR3 whose box after its `ftyp` says it is 2^64 - 1 long.
        let mut cr3 = 20u32.to_be_bytes().to_vec();
        cr3.extend_from_slice(b"ftypcrx \0\0\0\x01crx ");
        cr3.extend_from_slice(&1u32.to_be_bytes());
        cr3.extend_from_slice(b"skip");
        cr3.extend_from_slice(&u64::MAX.to_be_bytes());
        cr3.resize(64, 0);
        assert!(embedded_jpeg(&mut Cursor::new(&cr3)).is_err());
    }

    /// A raw file's size is its JPEG's, turned as it is shown, read from
    /// the JPEG's first bytes alone.
    #[test]
    fn a_raw_file_s_size_is_its_jpeg_s_turned() {
        let shown = |raw: &[u8]| size_of(&embedded_jpeg(&mut Cursor::new(raw)).unwrap());
        for raw in [tiff_raw(1), tiff_raw(6), cr3_file(None), cr3_file(Some(6))] {
            assert_eq!(shown_size(&mut Cursor::new(&raw)).unwrap(), shown(&raw));
        }
        assert_eq!(
            shown_size(&mut Cursor::new(&tiff_raw(6))).unwrap(),
            (120, 160)
        );
        assert!(shown_size(&mut Cursor::new(b"not a raw file at all".to_vec())).is_err());
    }

    /// Whether a raw file may carry a JPEG is told from its first bytes:
    /// no only where every directory lies in them and names none, as a
    /// phone's DNG of uncompressed pictures does.
    #[test]
    fn a_raw_file_without_a_jpeg_is_told_from_its_first_bytes() {
        assert!(may_carry_jpeg(&tiff_raw(1)), "names two");
        // IFD0 alone, its one picture uncompressed in a strip.
        let mut uncompressed = b"II*\0\x08\0\0\0".to_vec();
        uncompressed.extend_from_slice(&2u16.to_le_bytes());
        for (tag, value) in [(0x0103u16, 1u32), (0x0111, 64)] {
            uncompressed.extend_from_slice(&tag.to_le_bytes());
            uncompressed.extend_from_slice(&4u16.to_le_bytes());
            uncompressed.extend_from_slice(&1u32.to_le_bytes());
            uncompressed.extend_from_slice(&value.to_le_bytes());
        }
        uncompressed.extend_from_slice(&0u32.to_le_bytes());
        uncompressed.resize(80, 0);
        assert!(!may_carry_jpeg(&uncompressed), "names none");
        // The same, its next directory past what was read: not known.
        let at = 8 + 2 + 2 * 12;
        uncompressed[at..at + 4].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert!(may_carry_jpeg(&uncompressed), "a directory past the head");
        assert!(!may_carry_jpeg(b"not a raw file at all"), "no raw file");
        let mut raf = b"FUJIFILMCCD-RAW 0201".to_vec();
        raf.resize(100, 0);
        assert!(may_carry_jpeg(&raf), "a RAF always carries one");
    }
}
