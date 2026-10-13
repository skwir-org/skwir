//! What a camera wrote into a picture, for the panel's Properties.
//!
//! Read by `kamadak-exif`: pure Rust, always there, a few hundred microseconds
//! on the container's header. `exiftool` knows more formats but is a Perl
//! start-up per picture and often is not installed; nothing here needs it.
//! Only the container's metadata segment is parsed, never the pixels, and the
//! facts are bounded like every other detail.
use std::path::Path;

use exif::{Exif, In, Tag, Value};
use skwir_core::geo::LatLon;
use skwir_core::inspection::Detail;

use crate::inspection::{detail, limits};

/// What is read of a TIFF-based file (a camera's raw file, a scan) for its
/// metadata: its first directories and what they hold are at its start.
const TIFF_HEAD: u64 = 1 << 20;

/// The container's metadata, if it has any, found by seeking past what is
/// not it: the reader's own way read a raw file whole, every chunk of a
/// PNG and a JPEG's scan to its end, up to 16 MB a picture (a first map
/// of a thousand raw files read 16 GB).
fn metadata(path: &Path) -> Option<Exif> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = crate::file::open_regular(path).ok()?;
    let mut head = [0u8; 8];
    let read = file.read(&mut head).ok()?;
    let head = &head[..read];
    if head.starts_with(&[0xFF, 0xD8]) {
        return raw(jpeg_exif(&mut file)?, false);
    }
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return raw(png_exif(&mut file)?, false);
    }
    file.seek(SeekFrom::Start(0)).ok()?;
    if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        // What its directories point past the head (a maker's notes, the
        // strips' table) is left out, the rest read.
        let mut prefix = Vec::new();
        file.take(TIFF_HEAD).read_to_end(&mut prefix).ok()?;
        return raw(prefix, true);
    }
    // HEIF and WebP: the reader seeks to the metadata and stops; the bound
    // is for a segment that lies about its size.
    let mut reader = std::io::BufReader::new(file.take(limits::BINARY_INPUT as u64));
    exif::Reader::new().read_from_container(&mut reader).ok()
}

/// The metadata of `tiff`, the Exif's own TIFF data; with `partial`, what
/// could be read of it.
fn raw(tiff: Vec<u8>, partial: bool) -> Option<Exif> {
    let mut reader = exif::Reader::new();
    reader.continue_on_error(partial);
    match reader.read_raw(tiff) {
        Ok(exif) => Some(exif),
        Err(error) if partial => error.distill_partial_result(|_| {}).ok(),
        Err(_) => None,
    }
}

/// The next byte of `file`.
fn byte(file: &mut impl std::io::Read) -> Option<u8> {
    let mut b = [0u8];
    file.read_exact(&mut b).ok().map(|()| b[0])
}

/// A JPEG's Exif (its APP1 `Exif` segment's TIFF data): its segments
/// walked, each skipped by its length, up to its frame.
fn jpeg_exif(file: &mut (impl std::io::Read + std::io::Seek)) -> Option<Vec<u8>> {
    jpeg_head(file).0
}

/// A JPEG's Exif and its frame's width and height, as written: its
/// segments walked from its start, each skipped by its length, up to its
/// frame header (the metadata comes before it).
fn jpeg_head(
    file: &mut (impl std::io::Read + std::io::Seek),
) -> (Option<Vec<u8>>, Option<(u32, u32)>) {
    use std::io::SeekFrom;
    let mut exif = None;
    let walked = (|| -> Option<(u32, u32)> {
        file.seek(SeekFrom::Start(2)).ok()?;
        loop {
            if byte(file)? != 0xFF {
                return None;
            }
            // Fill bytes before a marker.
            let mut marker = byte(file)?;
            while marker == 0xFF {
                marker = byte(file)?;
            }
            match marker {
                0x01 | 0xD0..=0xD8 => continue,
                // The scan, or the end, before any frame.
                0xD9 | 0xDA => return None,
                _ => {}
            }
            let length = u16::from_be_bytes([byte(file)?, byte(file)?]);
            let body = usize::from(length.checked_sub(2)?);
            match marker {
                0xE1 if exif.is_none() => {
                    let mut data = vec![0; body];
                    file.read_exact(&mut data).ok()?;
                    if data.starts_with(b"Exif\0\0") {
                        exif = Some(data.split_off(6));
                    }
                }
                // A frame header (not DHT, JPG, DAC): its precision, then
                // its height and width.
                0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                    let mut frame = [0u8; 5];
                    file.read_exact(&mut frame).ok()?;
                    let height = u16::from_be_bytes([frame[1], frame[2]]);
                    let width = u16::from_be_bytes([frame[3], frame[4]]);
                    return Some((u32::from(width), u32::from(height)));
                }
                _ => {
                    file.seek(SeekFrom::Current(i64::try_from(body).ok()?))
                        .ok()?;
                }
            }
        }
    })();
    (exif, walked)
}

/// A JPEG's width and height once turned as its metadata says, read by
/// seeking from segment to segment: a document's photos were each read
/// 256 KB to learn their size. `None` for another format, or one whose
/// header is not found that way.
pub fn jpeg_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut file = crate::file::open_regular(path).ok()?;
    let mut soi = [0u8; 2];
    file.read_exact(&mut soi).ok()?;
    if soi != [0xFF, 0xD8] {
        return None;
    }
    let (exif, size) = jpeg_head(&mut file);
    let (width, height) = size.filter(|(w, h)| *w > 0 && *h > 0)?;
    let turned = exif
        .and_then(|tiff| raw(tiff, true))
        .and_then(|exif| {
            exif.get_field(Tag::Orientation, In::PRIMARY)?
                .value
                .get_uint(0)
        })
        .is_some_and(|orientation| (5..=8).contains(&orientation));
    Some(if turned {
        (height, width)
    } else {
        (width, height)
    })
}

/// A PNG's Exif (its `eXIf` chunk): its chunks walked, each skipped by its
/// length, up to its end.
fn png_exif(file: &mut (impl std::io::Read + std::io::Seek)) -> Option<Vec<u8>> {
    use std::io::SeekFrom;
    file.seek(SeekFrom::Start(8)).ok()?;
    loop {
        let mut head = [0u8; 8];
        file.read_exact(&mut head).ok()?;
        let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
        match &head[4..] {
            b"eXIf" => {
                let length = usize::try_from(length).ok()?;
                if length > limits::BINARY_INPUT {
                    return None;
                }
                let mut data = vec![0; length];
                file.read_exact(&mut data).ok()?;
                return Some(data);
            }
            b"IEND" => return None,
            // Its data and its check skipped.
            _ => {
                file.seek(SeekFrom::Current(i64::from(length) + 4)).ok()?;
            }
        }
    }
}

/// The facts worth a row, in the order they are shown.
pub fn details(path: &Path) -> Vec<Detail> {
    metadata(path).map(|exif| read(&exif)).unwrap_or_default()
}

/// The picture's own width and height as its camera wrote them, for a
/// picture whose pixels are read by another program. Before any turn: the
/// caller knows which way what it was given stands.
pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    let exif = metadata(path)?;
    let length = |tag| {
        exif.get_field(tag, In::PRIMARY)?
            .value
            .get_uint(0)
            .filter(|length| *length > 0)
    };
    Some((length(Tag::PixelXDimension)?, length(Tag::PixelYDimension)?))
}

/// Where and when a picture was taken, for the Map view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Taken {
    /// The position, if the camera knew it.
    pub at: Option<LatLon>,
    /// The camera's clock as it wrote it, as digits (`20260701083000`):
    /// an order between pictures, not an instant, since a picture rarely
    /// says its time zone.
    pub clock: Option<u64>,
}

/// Where and when the picture was taken, from one read of its metadata.
pub fn taken(path: &Path) -> Taken {
    metadata(path).map_or_else(Taken::default, |exif| Taken {
        at: located(&exif),
        clock: text(&exif, Tag::DateTimeOriginal)
            .or_else(|| text(&exif, Tag::DateTime))
            .and_then(|text| clock(&text)),
    })
}

/// `2026:07:01 08:30:00` as `20260701083000`; `None` without a whole date
/// and time.
fn clock(text: &str) -> Option<u64> {
    let digits: String = text.chars().filter(char::is_ascii_digit).take(14).collect();
    (digits.len() == 14).then(|| digits.parse().ok()).flatten()
}

/// The position the metadata gives. A receiver that had no fix often
/// writes 0, 0 or says its fix is void (`GPSStatus` V): neither is a
/// place.
fn located(exif: &Exif) -> Option<LatLon> {
    if text(exif, Tag::GPSStatus).is_some_and(|status| status.starts_with('V')) {
        return None;
    }
    let lat = coordinate(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef)?;
    let lon = coordinate(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef)?;
    if lat == 0. && lon == 0. {
        return None;
    }
    LatLon::new(lat, lon)
}

fn text(exif: &Exif, tag: Tag) -> Option<String> {
    let field = exif.get_field(tag, In::PRIMARY)?;
    let text = match &field.value {
        Value::Ascii(parts) => parts
            .iter()
            .map(|part| String::from_utf8_lossy(part).trim().to_owned())
            .collect::<Vec<_>>()
            .join(" "),
        _ => field.display_value().with_unit(exif).to_string(),
    };
    Some(text.trim_matches('"').trim().to_owned()).filter(|text| !text.is_empty())
}

fn rational(exif: &Exif, tag: Tag) -> Option<f64> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Rational(values) => values.first().map(exif::Rational::to_f64),
        Value::SRational(values) => values.first().map(exif::SRational::to_f64),
        _ => None,
    }
}

/// Degrees, minutes and seconds with their hemisphere, as decimal degrees.
fn coordinate(exif: &Exif, value: Tag, reference: Tag) -> Option<f64> {
    let Value::Rational(parts) = &exif.get_field(value, In::PRIMARY)?.value else {
        return None;
    };
    let [degrees, minutes, seconds] = parts.get(..3)? else {
        return None;
    };
    let degrees = degrees.to_f64() + minutes.to_f64() / 60. + seconds.to_f64() / 3600.;
    let negative = text(exif, reference).is_some_and(|r| r.starts_with(['S', 'W']));
    degrees
        .is_finite()
        .then_some(if negative { -degrees } else { degrees })
}

/// EXIF writes `2026:03:12 14:02:00`; everything else here writes dashes.
fn date(taken: &str) -> String {
    match taken.split_once(' ') {
        Some((day, time)) if day.len() == 10 => format!("{} {time}", day.replace(':', "-")),
        _ => taken.to_owned(),
    }
}

fn read(exif: &Exif) -> Vec<Detail> {
    let camera = match (text(exif, Tag::Make), text(exif, Tag::Model)) {
        // Most models repeat the maker's name.
        (Some(make), Some(model)) if model.starts_with(&make) => Some(model),
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (make, model) => model.or(make),
    };
    let exposure = rational(exif, Tag::ExposureTime).map(|seconds| {
        if seconds > 0. && seconds < 1. {
            format!("1/{:.0} s", 1. / seconds)
        } else {
            format!("{seconds} s")
        }
    });
    let position = located(exif).map(|at| format!("{:.5}, {:.5}", at.lat, at.lon));
    [
        (
            "Taken",
            text(exif, Tag::DateTimeOriginal)
                .or_else(|| text(exif, Tag::DateTime))
                .map(|taken| date(&taken)),
        ),
        ("Camera", camera),
        ("Lens", text(exif, Tag::LensModel)),
        ("Exposure", exposure),
        (
            "Aperture",
            rational(exif, Tag::FNumber).map(|f| format!("f/{f}")),
        ),
        ("ISO", text(exif, Tag::PhotographicSensitivity)),
        (
            "Focal length",
            rational(exif, Tag::FocalLength).map(|mm| format!("{mm} mm")),
        ),
        ("Flash", text(exif, Tag::Flash)),
        ("Orientation", text(exif, Tag::Orientation)),
        ("Position", position),
        ("Software", text(exif, Tag::Software)),
        ("Artist", text(exif, Tag::Artist)),
        ("Copyright", text(exif, Tag::Copyright)),
    ]
    .into_iter()
    .filter_map(|(label, value)| detail(label, value?))
    .map(|row| {
        if row.label == "Position" {
            crate::inspection::on_map(row)
        } else {
            row
        }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A TIFF whose first directory names the camera and points at an Exif
    /// directory holding the exposure time.
    fn tiff() -> Vec<u8> {
        fn entry(data: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
            data.extend(tag.to_le_bytes());
            data.extend(kind.to_le_bytes());
            data.extend(count.to_le_bytes());
            data.extend(value.to_le_bytes());
        }
        let mut data = b"II*\0\x08\0\0\0".to_vec();
        data.extend(3u16.to_le_bytes());
        entry(&mut data, 0x010f, 2, 5, 50); // Make, at 50
        entry(&mut data, 0x0110, 2, 9, 56); // Model, at 56
        entry(&mut data, 0x8769, 4, 1, 68); // the Exif directory, at 68
        data.extend(0u32.to_le_bytes());
        assert_eq!(data.len(), 50);
        data.extend(b"Acme\0\0");
        data.extend(b"Acme One\0\0\0\0");
        assert_eq!(data.len(), 68);
        data.extend(1u16.to_le_bytes());
        entry(&mut data, 0x829a, 5, 1, 86); // ExposureTime, a rational at 86
        data.extend(0u32.to_le_bytes());
        assert_eq!(data.len(), 86);
        data.extend(1u32.to_le_bytes());
        data.extend(250u32.to_le_bytes());
        data
    }

    /// The metadata is found in a JPEG past other segments, in a PNG after
    /// its image data, and at the head of a large TIFF, without reading
    /// what follows it.
    #[test]
    fn the_metadata_is_found_in_each_container() {
        let camera = |path: &Path| {
            metadata(path).map(|exif| {
                read(&exif)
                    .into_iter()
                    .find(|d| d.label == "Camera")
                    .map(|d| d.value)
            })
        };
        let dir = tempfile::tempdir().unwrap();
        // SOI, a JFIF APP0, the Exif APP1, then a scan and the end.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        jpeg.extend(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        let tiff = tiff();
        let app1 = u16::try_from(2 + 6 + tiff.len()).unwrap();
        jpeg.extend([0xFF, 0xE1]);
        jpeg.extend(app1.to_be_bytes());
        jpeg.extend(b"Exif\0\0");
        jpeg.extend(&tiff);
        jpeg.extend([0xFF, 0xDA, 0x00, 0x02]);
        jpeg.extend(vec![0x55; 4096]);
        jpeg.extend([0xFF, 0xD9]);
        std::fs::write(dir.path().join("a.jpg"), &jpeg).unwrap();
        assert_eq!(
            camera(&dir.path().join("a.jpg")),
            Some(Some("Acme One".into()))
        );
        // A PNG: IHDR, IDAT, then eXIf and IEND (checks left zero).
        let chunk = |kind: &[u8], data: &[u8]| {
            let mut out = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
            out.extend(kind);
            out.extend(data);
            out.extend([0; 4]);
            out
        };
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", &[0; 13]));
        png.extend(chunk(b"IDAT", &[0x77; 8192]));
        png.extend(chunk(b"eXIf", &tiff));
        png.extend(chunk(b"IEND", &[]));
        std::fs::write(dir.path().join("a.png"), &png).unwrap();
        assert_eq!(
            camera(&dir.path().join("a.png")),
            Some(Some("Acme One".into()))
        );
        // A TIFF far larger than what is read of it.
        let mut large = tiff.clone();
        large.resize(3 << 20, 0);
        std::fs::write(dir.path().join("a.tif"), &large).unwrap();
        assert_eq!(
            camera(&dir.path().join("a.tif")),
            Some(Some("Acme One".into()))
        );
        // A JPEG without metadata.
        std::fs::write(
            dir.path().join("b.jpg"),
            [0xFF, 0xD8, 0xFF, 0xDA, 0, 2, 0xFF, 0xD9],
        )
        .unwrap();
        assert!(metadata(&dir.path().join("b.jpg")).is_none());
    }

    /// A JPEG's size is read from its frame header, turned when its
    /// metadata says it stands on a side.
    #[test]
    fn a_jpeg_s_size_is_read_from_its_frame_and_turned() {
        let dir = tempfile::tempdir().unwrap();
        let mut jpeg = Vec::new();
        image::RgbImage::from_pixel(30, 20, image::Rgb([1, 2, 3]))
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        std::fs::write(dir.path().join("flat.jpg"), &jpeg).unwrap();
        assert_eq!(jpeg_size(&dir.path().join("flat.jpg")), Some((30, 20)));
        // Orientation 6 (turned a quarter), in an Exif segment after SOI.
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend(1u16.to_le_bytes());
        tiff.extend(0x0112u16.to_le_bytes());
        tiff.extend(3u16.to_le_bytes());
        tiff.extend(1u32.to_le_bytes());
        tiff.extend([6, 0, 0, 0]);
        tiff.extend(0u32.to_le_bytes());
        let mut standing = vec![0xFF, 0xD8, 0xFF, 0xE1];
        standing.extend(u16::try_from(2 + 6 + tiff.len()).unwrap().to_be_bytes());
        standing.extend(b"Exif\0\0");
        standing.extend(&tiff);
        standing.extend(&jpeg[2..]);
        std::fs::write(dir.path().join("standing.jpg"), &standing).unwrap();
        assert_eq!(jpeg_size(&dir.path().join("standing.jpg")), Some((20, 30)));
        std::fs::write(dir.path().join("not.jpg"), b"\x89PNG").unwrap();
        assert_eq!(jpeg_size(&dir.path().join("not.jpg")), None);
    }

    #[test]
    fn a_picture_says_what_took_it_and_how() {
        let exif = exif::Reader::new()
            .read_raw(tiff())
            .expect("the synthetic directory parses");
        let facts: Vec<_> = read(&exif)
            .into_iter()
            .map(|detail| (detail.label, detail.value))
            .collect();
        assert_eq!(
            facts,
            [
                ("Camera".to_owned(), "Acme One".to_owned()),
                ("Exposure".to_owned(), "1/250 s".to_owned())
            ]
        );
    }

    /// A TIFF whose first directory points at a GPS directory: latitude
    /// and longitude as degrees, minutes, seconds, and an optional status.
    fn gps(lat: [u32; 3], north: bool, lon: [u32; 3], east: bool, status: Option<u8>) -> Vec<u8> {
        fn entry(data: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: [u8; 4]) {
            data.extend(tag.to_le_bytes());
            data.extend(kind.to_le_bytes());
            data.extend(count.to_le_bytes());
            data.extend(value);
        }
        let entries = if status.is_some() { 5u16 } else { 4 };
        let gps_at = 8 + 18u32;
        let values_at = gps_at + 2 + 12 * u32::from(entries) + 4;
        let mut data = b"II*\0\x08\0\0\0".to_vec();
        data.extend(1u16.to_le_bytes());
        entry(&mut data, 0x8825, 4, 1, gps_at.to_le_bytes()); // the GPS directory
        data.extend(0u32.to_le_bytes());
        data.extend(entries.to_le_bytes());
        let hemisphere = |yes: bool, a: u8, b: u8| [if yes { a } else { b }, 0, 0, 0];
        entry(&mut data, 0x0001, 2, 2, hemisphere(north, b'N', b'S'));
        entry(&mut data, 0x0002, 5, 3, values_at.to_le_bytes());
        entry(&mut data, 0x0003, 2, 2, hemisphere(east, b'E', b'W'));
        entry(&mut data, 0x0004, 5, 3, (values_at + 24).to_le_bytes());
        if let Some(status) = status {
            entry(&mut data, 0x0009, 2, 2, [status, 0, 0, 0]);
        }
        data.extend(0u32.to_le_bytes());
        for part in lat.into_iter().chain(lon) {
            data.extend(part.to_le_bytes());
            data.extend(1u32.to_le_bytes());
        }
        data
    }

    fn located_in(data: Vec<u8>) -> Option<LatLon> {
        located(
            &exif::Reader::new()
                .read_raw(data)
                .expect("the synthetic directory parses"),
        )
    }

    #[test]
    fn a_camera_s_clock_reads_as_an_order() {
        assert_eq!(clock("2026:07:01 08:30:00"), Some(20_260_701_083_000));
        assert!(clock("2026:07:01 08:30:00") < clock("2026:07:01 08:30:01"));
        assert_eq!(clock("2026:07:01"), None);
        assert_eq!(clock("    :  :     :  :  "), None);
    }

    #[test]
    fn a_picture_says_where_it_was_taken_when_the_receiver_knew() {
        let at = located_in(gps([45, 30, 0], true, [6, 15, 0], false, None)).unwrap();
        assert!(
            (at.lat - 45.5).abs() < 1e-9 && (at.lon + 6.25).abs() < 1e-9,
            "{at:?}"
        );
        // No fix: nowhere, not the Gulf of Guinea.
        assert!(located_in(gps([0, 0, 0], true, [0, 0, 0], true, None)).is_none());
        assert!(located_in(gps([45, 30, 0], true, [6, 15, 0], true, Some(b'V'))).is_none());
        assert!(located_in(gps([45, 30, 0], true, [6, 15, 0], true, Some(b'A'))).is_some());
        // A picture without a GPS directory.
        assert!(located_in(tiff()).is_none());
    }

    #[test]
    fn dates_read_like_the_rest_of_the_panel() {
        assert_eq!(date("2026:03:12 14:02:00"), "2026-03-12 14:02:00");
        assert_eq!(date("sometime"), "sometime");
    }

    #[test]
    fn a_file_without_metadata_says_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("it's a café.png");
        std::fs::write(&path, b"not a picture at all").unwrap();
        assert!(details(&path).is_empty());
        assert!(details(&dir.path().join("missing.jpg")).is_empty());
    }

    proptest::proptest! {
        /// A picture's metadata as no camera writes it: never a panic.
        #[test]
        fn any_metadata_is_read_without_a_panic(raw in proptest::prop_oneof![
            skwir_core::testing::mutated(tiff()),
            skwir_core::testing::mutated(gps([45, 30, 0], true, [6, 15, 0], false, Some(b'A'))),
            skwir_core::testing::bytes(512),
        ]) {
            if let Ok(exif) = exif::Reader::new().read_raw(raw) {
                let _ = read(&exif);
                let _ = located(&exif);
                if let Some(taken) = text(&exif, exif::Tag::DateTimeOriginal) {
                    let _ = clock(&taken);
                    let _ = date(&taken);
                }
            }
        }
    }
}
