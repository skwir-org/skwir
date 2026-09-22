//! What a camera wrote into a picture, for the panel's Details.
//!
//! Read by `kamadak-exif`: pure Rust, always there, a few hundred microseconds
//! on the container's header. `exiftool` knows more formats but is a Perl
//! start-up per picture and often is not installed; nothing here needs it.
//! Only the container's metadata segment is parsed, never the pixels, and the
//! facts are bounded like every other detail.
use std::path::Path;

use bibli_core::inspection::Detail;
use exif::{Exif, In, Tag, Value};

use crate::inspection::{detail, limits};

/// The facts worth a row, in the order they are shown.
pub fn details(path: &Path) -> Vec<Detail> {
    let Ok(file) = crate::file::open_regular(path) else {
        return Vec::new();
    };
    // The reader seeks to the metadata and stops; the bound is for a segment
    // that lies about its size.
    let mut reader =
        std::io::BufReader::new(std::io::Read::take(file, limits::BINARY_INPUT as u64));
    exif::Reader::new()
        .read_from_container(&mut reader)
        .map(|exif| read(&exif))
        .unwrap_or_default()
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
    let position = coordinate(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef)
        .zip(coordinate(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef))
        .map(|(latitude, longitude)| format!("{latitude:.5}, {longitude:.5}"));
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
