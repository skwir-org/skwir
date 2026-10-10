//! `AutoCAD` drawings: the picture `AutoCAD` (and what writes its formats)
//! stores in a drawing when it saves, and the format's version. In a DWG
//! file, the header's word at byte 13 is where its pictures are listed,
//! after a fixed sentinel: a bitmap (without its file header) or, since
//! 2013, a PNG, each by its place in the file. A DXF file is drawn from
//! its entities (`dxf`); when nothing is, the `THUMBNAILIMAGE` section at
//! its end may hold a bitmap in hexadecimal.
use crate::Found;
use skwir_core::CancelToken;
use skwir_platform::inspection::{self, limits};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// What begins a DWG file's list of pictures.
const SENTINEL: [u8; 16] = [
    0x1F, 0x25, 0x6D, 0x07, 0xD4, 0x36, 0x28, 0x28, 0x9D, 0x57, 0xCA, 0x3F, 0x9D, 0x44, 0x10, 0x2B,
];
/// A picture's kind in that list.
const BITMAP: u8 = 2;
const PNG: u8 = 6;
/// The end of a DXF file read for its picture, and the start read for its
/// version.
const DXF_TAIL: u64 = 2 * 1024 * 1024;
const DXF_HEAD: u64 = 64 * 1024;
/// The largest DXF drawn from its entities: its text is held whole.
const DXF_DRAWN: usize = 32 * 1024 * 1024;

/// The release a format version (`AC1032`) was introduced with.
fn release(version: &str) -> Option<&'static str> {
    Some(match version {
        "AC1009" => "R11/R12",
        "AC1012" => "R13",
        "AC1014" => "R14",
        "AC1015" => "2000",
        "AC1018" => "2004",
        "AC1021" => "2007",
        "AC1024" => "2010",
        "AC1027" => "2013",
        "AC1032" => "2018",
        _ => return None,
    })
}

/// A bitmap stored without its file header, as the `image` crate reads one
/// inside an icon.
pub(crate) fn bitmap(dib: &[u8], side: u32) -> Option<skwir_core::thumbs::Image> {
    let mut decoder =
        image::codecs::bmp::BmpDecoder::new_without_file_header(std::io::Cursor::new(dib)).ok()?;
    // A run-length bitmap of a few kilobytes can say any size.
    image::ImageDecoder::set_limits(&mut decoder, inspection::image_limits()).ok()?;
    if image::ImageDecoder::total_bytes(&decoder) > limits::IMAGE_ALLOC {
        return None;
    }
    let image = image::DynamicImage::from_decoder(decoder).ok()?;
    Some(crate::fit(image.into_rgba8(), side))
}

/// A DWG drawing.
pub fn dwg(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<Found> {
    let mut file = inspection::open(path)?;
    let len = file.metadata()?.len();
    let mut header = [0u8; 17];
    file.read_exact(&mut header)?;
    anyhow::ensure!(&header[..2] == b"AC", "Not a DWG drawing");
    let version = String::from_utf8_lossy(&header[..6]).into_owned();
    let mut found = Found {
        title: "DWG drawing".into(),
        details: vec![(
            "Format".into(),
            release(&version).map_or_else(|| version.clone(), |r| format!("DWG {r}")),
        )],
        ..Found::default()
    };
    inspection::check(cancel)?;
    let at = u64::from(u32::from_le_bytes(header[13..17].try_into()?));
    let mut list = [0u8; 21];
    if at + 21 > len {
        return Ok(found);
    }
    file.seek(SeekFrom::Start(at))?;
    file.read_exact(&mut list)?;
    if list[..16] != SENTINEL {
        return Ok(found);
    }
    let mut entries = vec![0u8; usize::from(list[20]) * 9];
    file.read_exact(&mut entries)?;
    // The PNG, when there is one, rather than the bitmap beside it.
    let mut best = None;
    for entry in entries.as_chunks::<9>().0 {
        let start = u64::from(u32::from_le_bytes(entry[1..5].try_into()?));
        let length = u64::from(u32::from_le_bytes(entry[5..9].try_into()?));
        if length == 0 || length > limits::BINARY_INPUT as u64 || start + length > len {
            continue;
        }
        match entry[0] {
            PNG => best = Some((PNG, start, length)),
            BITMAP if best.is_none() => best = Some((BITMAP, start, length)),
            _ => {}
        }
    }
    if let Some((kind, start, length)) = best {
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = vec![0u8; usize::try_from(length)?];
        file.read_exact(&mut bytes)?;
        found.picture = if kind == PNG {
            crate::decoded(&bytes, side)
        } else {
            bitmap(&bytes, side)
        };
    }
    Ok(found)
}

/// A DXF drawing: text of group codes and values, a pair of lines each.
pub fn dxf(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<Found> {
    let mut file = inspection::open(path)?;
    let len = file.metadata()?.len();
    let mut head = Vec::new();
    (&mut file).take(DXF_HEAD).read_to_end(&mut head)?;
    let head = String::from_utf8_lossy(&head);
    let pairs = |text: &str| -> Vec<(String, String)> {
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        lines
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| (pair[0].to_owned(), pair[1].to_owned()))
            .collect()
    };
    let mut found = Found {
        title: "DXF drawing".into(),
        ..Found::default()
    };
    // `9 $ACADVER 1 AC1032` in the header section.
    let head_pairs = pairs(&head);
    if let Some(at) = head_pairs.iter().position(|(_, v)| v == "$ACADVER")
        && let Some((_, version)) = head_pairs.get(at + 1)
    {
        found.details.push((
            "Format".into(),
            release(version).map_or_else(|| version.clone(), |r| format!("DXF {r}")),
        ));
    }
    inspection::check(cancel)?;
    // Drawn from its entities, when it is not too large to.
    if len <= DXF_DRAWN as u64 {
        let text = inspection::read_bounded(path, DXF_DRAWN, "The drawing", cancel)?;
        let drawing = crate::dxf::draw(&String::from_utf8_lossy(&text), cancel)?;
        found.details.extend(drawing.details);
        found.notice = drawing.notice;
        if let Some(svg) = drawing.svg {
            let drawn = inspection::drawing(svg.as_bytes(), Some(side))?;
            found.picture = Some(skwir_core::thumbs::Image {
                width: drawn.width,
                height: drawn.height,
                rgba: drawn.rgba.into(),
            });
            return Ok(found);
        }
    }
    // Else the picture it stored, if any.
    let from = len.saturating_sub(DXF_TAIL);
    file.seek(SeekFrom::Start(from))?;
    let mut tail = Vec::new();
    file.take(DXF_TAIL).read_to_end(&mut tail)?;
    let tail = String::from_utf8_lossy(&tail);
    let Some(section) = tail.find("THUMBNAILIMAGE") else {
        return Ok(found);
    };
    // From the section's name, its pairs: `90` its length, then `310` its
    // bytes in hexadecimal, a line at a time, until `0 ENDSEC`.
    let start = tail[..section].rfind('\n').map_or(0, |at| at + 1);
    let body = &tail[start..];
    let mut hex = String::new();
    let lines: Vec<&str> = body.lines().skip(1).map(str::trim).collect();
    for pair in lines.as_chunks::<2>().0 {
        match (pair[0], pair[1]) {
            ("310", data) => hex.push_str(data),
            ("0", _) => break,
            _ => {}
        }
    }
    let dib: Option<Vec<u8>> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect();
    found.picture = dib.and_then(|dib| bitmap(&dib, side));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    fn picture(format: image::ImageFormat) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut image = image::RgbImage::from_pixel(8, 4, image::Rgb([30, 30, 30]));
        image.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), format)
            .unwrap();
        bytes
    }

    /// A drawing whose header points at its list of pictures.
    fn dwg(entries: &[(u8, &[u8])]) -> Vec<u8> {
        let mut file = b"AC1032".to_vec();
        file.resize(0x0D, 0);
        let list: u32 = 0x40;
        file.extend(list.to_le_bytes());
        file.resize(list as usize, 0);
        file.extend(SENTINEL);
        file.extend(0u32.to_le_bytes());
        file.push(u8::try_from(entries.len()).unwrap());
        let mut data_at = list as usize + 21 + entries.len() * 9;
        let mut data: Vec<u8> = Vec::new();
        for (code, bytes) in entries {
            file.push(*code);
            file.extend(u32::try_from(data_at).unwrap().to_le_bytes());
            file.extend(u32::try_from(bytes.len()).unwrap().to_le_bytes());
            data.extend(*bytes);
            data_at += bytes.len();
        }
        file.extend(data);
        file
    }

    fn red_first(picture: &skwir_core::thumbs::Image) -> bool {
        (picture.width, picture.height) == (8, 4) && picture.rgba[..3] == [255, 0, 0]
    }

    #[test]
    fn a_dwg_drawings_stored_pictures() {
        let dir = tempfile::tempdir().unwrap();
        let bmp = picture(image::ImageFormat::Bmp);
        // What DWG stores is the bitmap without its 14-byte file header.
        let header = [0u8; 80];
        let png_path = dir.path().join("a plan’s floor.dwg");
        std::fs::write(
            &png_path,
            dwg(&[
                (1, &header),
                (BITMAP, &bmp[14..]),
                (PNG, &picture(image::ImageFormat::Png)),
            ]),
        )
        .unwrap();
        let found = super::dwg(&png_path, 256, &CancelToken::new()).unwrap();
        assert_eq!(
            found.details,
            [("Format".to_owned(), "DWG 2018".to_owned())]
        );
        assert!(red_first(&found.picture.unwrap()), "the PNG, upright");
        let bmp_path = dir.path().join("older.dwg");
        std::fs::write(&bmp_path, dwg(&[(1, &header), (BITMAP, &bmp[14..])])).unwrap();
        let found = super::dwg(&bmp_path, 256, &CancelToken::new()).unwrap();
        assert!(red_first(&found.picture.unwrap()), "the bitmap, upright");
        let none = dir.path().join("none.dwg");
        std::fs::write(&none, b"AC1015 and nothing more").unwrap();
        assert!(
            super::dwg(&none, 256, &CancelToken::new())
                .unwrap()
                .picture
                .is_none()
        );
    }

    #[test]
    fn a_dxf_drawings_thumbnail_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a plan’s floor.dxf");
        let bmp = picture(image::ImageFormat::Bmp);
        let hex: String = bmp[14..].iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02X}");
            hex
        });
        let mut text =
            String::from("  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1027\n  0\nENDSEC\n");
        text.push_str("  0\nSECTION\n  2\nTHUMBNAILIMAGE\n 90\n");
        let _ = writeln!(text, "{}", bmp.len() - 14);
        for line in hex.as_bytes().chunks(254) {
            let _ = write!(text, "310\n{}\n", std::str::from_utf8(line).unwrap());
        }
        text.push_str("  0\nENDSEC\n  0\nEOF\n");
        std::fs::write(&path, text).unwrap();
        let found = dxf(&path, 256, &CancelToken::new()).unwrap();
        assert_eq!(
            found.details,
            [("Format".to_owned(), "DXF 2013".to_owned())]
        );
        assert!(red_first(&found.picture.unwrap()));
    }
}
