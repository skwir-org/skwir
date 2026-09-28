//! `SketchUp` models and `LayOut` documents. A model begins with its
//! application's name and version in UTF-16; since `SketchUp` 2021 a zip
//! follows, whose `meta/model_thumbnail.png` is the picture the Finder
//! and Explorer show. Older models carry a PNG among their first bytes,
//! found by its signature. A `LayOut` document is a plain zip with
//! `doc_thumbnail.png` and its properties.
use crate::Found;
use bibli_core::CancelToken;
use bibli_platform::inspection::{self, limits};
use std::io::Read;
use std::path::Path;

/// The start of an older model searched for its picture.
const OLD_HEAD: u64 = 2 * 1024 * 1024;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// A string as `SketchUp` writes one in its header: `ff fe ff`, a count
/// of UTF-16 units, the units.
fn string(bytes: &[u8]) -> Option<(String, &[u8])> {
    let rest = bytes.strip_prefix(b"\xff\xfe\xff")?;
    let count = usize::from(*rest.first()?);
    let units: Vec<u16> = rest
        .get(1..1 + count * 2)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    Some((String::from_utf16(&units).ok()?, &rest[1 + count * 2..]))
}

/// The release a version (`{21.0.391}`) belongs to: `SketchUp` 2013 was
/// version 13, 8 before it.
fn release(version: &str) -> Option<String> {
    let major: u32 = version
        .trim_matches(['{', '}'])
        .split('.')
        .next()?
        .parse()
        .ok()?;
    Some(if major >= 13 {
        format!("SketchUp {}", 2000 + major)
    } else {
        format!("SketchUp {major}")
    })
}

/// A `SketchUp` model.
pub fn model(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<Found> {
    let mut head = Vec::new();
    inspection::open(path)?
        .take(OLD_HEAD)
        .read_to_end(&mut head)?;
    let (name, rest) = string(&head).ok_or_else(|| anyhow::anyhow!("Not a SketchUp model"))?;
    anyhow::ensure!(name == "SketchUp Model", "Not a SketchUp model");
    let mut found = Found {
        title: "SketchUp model".into(),
        ..Found::default()
    };
    if let Some(made) = string(rest).and_then(|(version, _)| release(&version)) {
        found.details.push(("Made with".into(), made));
    }
    inspection::check(cancel)?;
    // Since 2021: a zip after the header.
    if let Ok(mut archive) = crate::package::open(path) {
        let bound = limits::BINARY_INPUT as u64;
        found.picture = crate::package::member(&mut archive, "meta/model_thumbnail.png", bound)
            .and_then(|bytes| crate::decoded(&bytes, side));
        let scenes = archive
            .file_names()
            .filter(|name| name.starts_with("scene_thumbnails/"))
            .count();
        if scenes > 0 {
            found.details.push(("Scenes".into(), scenes.to_string()));
        }
        return Ok(found);
    }
    // Before: the first picture among its first bytes.
    found.picture = head
        .windows(PNG.len())
        .position(|window| window == PNG)
        .and_then(|at| crate::decoded(&head[at..], side));
    Ok(found)
}

/// A `LayOut` document: its first page's picture and its page count.
pub fn layout(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<Found> {
    let mut archive = crate::package::open(path)?;
    inspection::check(cancel)?;
    let bound = limits::BINARY_INPUT as u64;
    let mut found = Found {
        title: "LayOut document".into(),
        picture: crate::package::member(&mut archive, "doc_thumbnail.png", bound)
            .and_then(|bytes| crate::decoded(&bytes, side)),
        ..Found::default()
    };
    let properties = crate::package::member(
        &mut archive,
        "documentProperties.xml",
        limits::MEMBER_NAMES as u64,
    )
    .unwrap_or_default();
    let mut reader = quick_xml::Reader::from_reader(properties.as_slice());
    let mut buf = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buf) {
        use quick_xml::events::Event;
        match event {
            Event::Empty(e) | Event::Start(e) => match e.local_name().as_ref() {
                "generator" => {
                    if let Some(version) = crate::package::attribute(&e, "version")
                        && let Some(made) = release(&version)
                    {
                        found
                            .details
                            .insert(0, ("Made with".into(), made.replace("SketchUp", "LayOut")));
                    }
                }
                "documentStatistic" => {
                    if let Some(pages) = crate::package::attribute(&e, "pageCount") {
                        found.details.push(("Pages".into(), pages));
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(width, height, image::Rgba([9, 8, 7, 255]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn header(version: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for text in ["SketchUp Model", version] {
            out.extend(b"\xff\xfe\xff");
            out.push(u8::try_from(text.len()).unwrap());
            out.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        }
        out
    }

    #[test]
    fn a_2021_model_keeps_its_picture_in_a_zip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a part’s model.skp");
        let mut bytes = header("{21.0.391}");
        bytes.extend(b"VFF\x08\x00\x01\x00\x08\x00\x01\x02\x03\x04");
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in [
            ("meta/model_thumbnail.png", png(6, 4)),
            ("scene_thumbnails/Plan.png", png(1, 1)),
            ("scene_thumbnails/Front.png", png(1, 1)),
            ("model.dat", vec![0; 64]),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(&data).unwrap();
        }
        bytes.extend(zip.finish().unwrap().into_inner());
        std::fs::write(&path, bytes).unwrap();
        let found = model(&path, 1600, &CancelToken::new()).unwrap();
        let picture = found.picture.as_ref().expect("the thumbnail");
        assert_eq!((picture.width, picture.height), (6, 4));
        assert_eq!(
            found.details,
            [
                ("Made with".to_owned(), "SketchUp 2021".to_owned()),
                ("Scenes".to_owned(), "2".to_owned())
            ]
        );
    }

    #[test]
    fn an_older_model_keeps_it_near_its_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.skp");
        let mut bytes = header("{8.0.11752}");
        bytes.extend(b"\x00\x00CVersionMap\x00");
        bytes.extend(png(3, 2));
        bytes.extend([0; 100]);
        std::fs::write(&path, bytes).unwrap();
        let found = model(&path, 1600, &CancelToken::new()).unwrap();
        let picture = found.picture.as_ref().expect("the picture");
        assert_eq!((picture.width, picture.height), (3, 2));
        assert_eq!(found.details[0].1, "SketchUp 8");
        // Anything else is refused.
        std::fs::write(&path, b"\xff\xfe\xff\x02A\x00B\x00").unwrap();
        assert!(model(&path, 1600, &CancelToken::new()).is_err());
    }

    #[test]
    fn a_layout_document_with_its_pages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sheets.layout");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("doc_thumbnail.png", options).unwrap();
        zip.write_all(&png(5, 3)).unwrap();
        zip.start_file("documentProperties.xml", options).unwrap();
        zip.write_all(
            br#"<documentProperties xmlns:dp="a" xmlns:ldp="b"><ldp:documentStatistic pageCount="5" /><dp:generator name="LayOut" version="21.0.391" /></documentProperties>"#,
        )
        .unwrap();
        zip.finish().unwrap();
        let found = layout(&path, 1600, &CancelToken::new()).unwrap();
        assert!(found.picture.is_some());
        assert_eq!(
            found.details,
            [
                ("Made with".to_owned(), "LayOut 2021".to_owned()),
                ("Pages".to_owned(), "5".to_owned())
            ]
        );
    }
}
