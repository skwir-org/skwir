//! What a camera wrote into a picture, for the panel's Details.
//!
//! Read by `kamadak-exif`: pure Rust, always there, a few hundred microseconds
//! on the container's header. `exiftool` knows more formats but is a Perl
//! start-up per picture and often is not installed; nothing here needs it.
//! Only the container's metadata segment is parsed, never the pixels, and the
//! facts are bounded like every other detail.
use std::path::Path;

use bibli_core::geo::LatLon;
use bibli_core::inspection::Detail;
use exif::{Exif, In, Tag, Value};

use crate::inspection::{detail, limits};

/// The container's metadata, if it has any.
fn metadata(path: &Path) -> Option<Exif> {
    let file = crate::file::open_regular(path).ok()?;
    // The reader seeks to the metadata and stops; the bound is for a segment
    // that lies about its size.
    let mut reader =
        std::io::BufReader::new(std::io::Read::take(file, limits::BINARY_INPUT as u64));
    exif::Reader::new().read_from_container(&mut reader).ok()
}

/// The facts worth a row, in the order they are shown.
pub fn details(path: &Path) -> Vec<Detail> {
    metadata(path).map(|exif| read(&exif)).unwrap_or_default()
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
}
