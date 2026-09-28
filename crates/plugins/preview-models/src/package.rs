//! Zip packages that keep a picture of their model: a 3MF (the package
//! thumbnail its relationships name, or the first plate's picture Bambu
//! Studio and Orca store) and a `FreeCAD` document (`thumbnails/`). Only the
//! members wanted are read, each within the bound for binary input; the
//! meshes are left alone.
use crate::Found;
use bibli_core::CancelToken;
use bibli_platform::inspection::{self, limits};
use quick_xml::events::Event;
use std::io::Read;
use std::path::Path;

/// The start of a model's XML read for its metadata, which comes before
/// its resources (the meshes, megabytes of them).
const MODEL_HEAD: u64 = 64 * 1024;

type Archive = zip::ZipArchive<std::io::BufReader<std::fs::File>>;

fn open(path: &Path) -> anyhow::Result<Archive> {
    let archive = zip::ZipArchive::new(std::io::BufReader::new(inspection::open(path)?))?;
    anyhow::ensure!(
        archive.len() <= limits::CONTAINER_MEMBERS,
        "The package holds more than {} members",
        limits::CONTAINER_MEMBERS
    );
    Ok(archive)
}

/// The member `name`, if it is there and within `bound` once inflated.
fn member(archive: &mut Archive, name: &str, bound: u64) -> Option<Vec<u8>> {
    let file = archive.by_name(name).ok()?;
    if file.size() > bound {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(bound + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= bound).then_some(bytes)
}

/// The start of member `name`, at most `bound` bytes of it.
fn member_head(archive: &mut Archive, name: &str, bound: u64) -> Option<Vec<u8>> {
    let file = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    file.take(bound).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// An attribute of an element, unescaped.
fn attribute(element: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| {
            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(std::borrow::Cow::into_owned)
        })
}

/// The package thumbnail `_rels/.rels` names, as a member name.
fn thumbnail_target(archive: &mut Archive) -> Option<String> {
    let rels = member(archive, "_rels/.rels", limits::MEMBER_NAMES as u64)?;
    let mut reader = quick_xml::Reader::from_reader(rels.as_slice());
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == "Relationship" => {
                let kind = attribute(&e, "Type").unwrap_or_default();
                if kind.ends_with("/metadata/thumbnail") {
                    let target = attribute(&e, "Target")?;
                    return Some(target.trim_start_matches('/').to_owned());
                }
            }
            Event::Eof => return None,
            _ => {}
        }
        buf.clear();
    }
}

/// A 3MF package: its picture, the plates its slicer stored, and what its
/// model and its slicer say.
pub fn three_mf(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<Found> {
    let mut archive = open(path)?;
    inspection::check(cancel)?;
    let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    // Bambu Studio and Orca store a picture per plate: `plate_1.png`.
    let plates = names
        .iter()
        .filter(|name| {
            name.strip_prefix("Metadata/plate_")
                .and_then(|rest| rest.strip_suffix(".png"))
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .count();
    let bound = limits::BINARY_INPUT as u64;
    let picture = [
        thumbnail_target(&mut archive),
        Some("Metadata/thumbnail.png".into()),
        Some("Metadata/plate_1.png".into()),
    ]
    .into_iter()
    .flatten()
    .find_map(|name| member(&mut archive, &name, bound))
    .and_then(|bytes| crate::decoded(&bytes, side));
    inspection::check(cancel)?;
    let mut details = Vec::new();
    if let Some(head) = member_head(&mut archive, "3D/3dmodel.model", MODEL_HEAD) {
        details.extend(model_metadata(&head));
    }
    if plates > 1 {
        details.push(("Plates".into(), plates.to_string()));
    }
    if let Some(info) = member(&mut archive, "Metadata/slice_info.config", bound) {
        details.extend(slice_info(&info));
    }
    Ok(Found {
        title: "3MF".into(),
        picture,
        details,
    })
}

/// What the model's XML says before its resources: its unit, and the
/// metadata its application wrote (`Application`, `Title`).
fn model_metadata(head: &[u8]) -> Vec<(String, String)> {
    let mut reader = quick_xml::Reader::from_reader(head);
    let mut buf = Vec::new();
    let mut found = Vec::new();
    let mut unit = None;
    let mut open: Option<String> = None;
    let mut text = String::new();
    // The read stops inside the XML: whatever is cut is not read.
    while let Ok(event) = reader.read_event_into(&mut buf) {
        match event {
            Event::Start(e) if e.local_name().as_ref() == "model" => {
                unit = attribute(&e, "unit");
            }
            Event::Start(e) if e.local_name().as_ref() == "metadata" => {
                open = attribute(&e, "name");
            }
            Event::Text(part) if open.is_some() => text.push_str(&part.xml10_content()),
            Event::GeneralRef(reference) if open.is_some() => {
                if let Some(ch) = reference.resolve_char_ref().ok().flatten() {
                    text.push(ch);
                } else {
                    text.push_str(match &*reference.xml10_content() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(_) => {
                let label = match open.take().as_deref() {
                    Some("Title") => Some("Title"),
                    Some("Application") => Some("Made with"),
                    _ => None,
                };
                let value = std::mem::take(&mut text).trim().to_owned();
                if let Some(label) = label.filter(|_| !value.is_empty()) {
                    found.push((label.to_owned(), value));
                }
            }
            Event::Start(e) if e.local_name().as_ref() == "resources" => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if let Some(unit) = unit {
        found.push(("Unit".into(), unit));
    }
    found
}

/// Bambu Studio's and Orca's `slice_info.config`: each sliced plate's
/// predicted time (seconds) and weight (grams), summed.
fn slice_info(xml: &[u8]) -> Vec<(String, String)> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let (mut seconds, mut grams) = (None::<f64>, None::<f64>);
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "metadata" => {
                let key = attribute(&e, "key").unwrap_or_default();
                let value = attribute(&e, "value").and_then(|v| v.parse::<f64>().ok());
                match (key.as_str(), value) {
                    ("prediction", Some(v)) => *seconds.get_or_insert(0.) += v,
                    ("weight", Some(v)) => *grams.get_or_insert(0.) += v,
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    [
        ("Print time", seconds.map(crate::gcode::duration)),
        (
            "Filament",
            grams.map(|g| format!("{} g", crate::gcode::short(g))),
        ),
    ]
    .into_iter()
    .filter_map(|(label, value)| Some((label.to_owned(), value?)))
    .collect()
}

/// A `FreeCAD` document: the picture `FreeCAD` stores when it saves, and the
/// version that saved it (`Document.xml`'s first element).
pub fn freecad(path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<Found> {
    let mut archive = open(path)?;
    inspection::check(cancel)?;
    let picture = member(
        &mut archive,
        "thumbnails/Thumbnail.png",
        limits::BINARY_INPUT as u64,
    )
    .and_then(|bytes| crate::decoded(&bytes, side));
    let mut details = Vec::new();
    if let Some(head) = member_head(&mut archive, "Document.xml", MODEL_HEAD) {
        let mut reader = quick_xml::Reader::from_reader(head.as_slice());
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e) | Event::Empty(e)) => {
                    if e.local_name().as_ref() == "Document"
                        && let Some(version) = attribute(&e, "ProgramVersion")
                    {
                        details.push(("Saved by".into(), format!("FreeCAD {version}")));
                    }
                    break;
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
    }
    Ok(Found {
        title: "FreeCAD document".into(),
        picture,
        details,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(width, height, image::Rgba([0, 120, 200, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn zip(path: &Path, members: &[(&str, &[u8])]) {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in members {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/3dmodel.model" Id="rel-1" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
 <Relationship Target="/Metadata/cover.png" Id="rel-2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail"/>
</Relationships>"#;

    const MODEL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
 <metadata name="Application">PrusaSlicer-2.8.1</metadata>
 <metadata name="Title">Bracket &amp; hinge</metadata>
 <resources>
  <object id="1" type="model"><mesh><vertices><vertex x="0" y="0" z="0"/></vertices></mesh></object>
 </resources>
</model>"#;

    const SLICE_INFO: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <plate><metadata key="index" value="1"/><metadata key="prediction" value="3000"/><metadata key="weight" value="12.5"/></plate>
  <plate><metadata key="index" value="2"/><metadata key="prediction" value="725"/><metadata key="weight" value="2.25"/></plate>
</config>"#;

    fn detail<'a>(found: &'a Found, label: &str) -> Option<&'a str> {
        found
            .details
            .iter()
            .find(|(l, _)| l == label)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn a_3mf_shows_the_picture_its_relationships_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a bracket’s plate.3mf");
        let (cover, plate) = (png(30, 20), png(10, 10));
        zip(
            &path,
            &[
                ("_rels/.rels", RELS.as_bytes()),
                ("3D/3dmodel.model", MODEL.as_bytes()),
                ("Metadata/cover.png", &cover),
                ("Metadata/plate_1.png", &plate),
                ("Metadata/plate_1_small.png", &plate),
                ("Metadata/plate_2.png", &plate),
                ("Metadata/slice_info.config", SLICE_INFO.as_bytes()),
            ],
        );
        let found = three_mf(&path, 256, &CancelToken::new()).unwrap();
        let picture = found.picture.as_ref().expect("the package thumbnail");
        assert_eq!((picture.width, picture.height), (30, 20));
        assert_eq!(detail(&found, "Made with"), Some("PrusaSlicer-2.8.1"));
        assert_eq!(detail(&found, "Title"), Some("Bracket & hinge"));
        assert_eq!(detail(&found, "Unit"), Some("millimeter"));
        assert_eq!(detail(&found, "Plates"), Some("2"));
        assert_eq!(detail(&found, "Print time"), Some("1h 2m 5s"));
        assert_eq!(detail(&found, "Filament"), Some("14.75 g"));
    }

    #[test]
    fn a_3mf_without_a_named_picture_shows_its_first_plate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plates.3mf");
        let plate = png(12, 8);
        zip(
            &path,
            &[
                ("3D/3dmodel.model", MODEL.as_bytes()),
                ("Metadata/plate_1.png", &plate),
            ],
        );
        let found = three_mf(&path, 256, &CancelToken::new()).unwrap();
        let picture = found.picture.as_ref().expect("the first plate");
        assert_eq!((picture.width, picture.height), (12, 8));
        assert_eq!(detail(&found, "Plates"), None, "one plate is not a count");
        let bare = dir.path().join("bare.3mf");
        zip(&bare, &[("3D/3dmodel.model", MODEL.as_bytes())]);
        assert!(
            three_mf(&bare, 256, &CancelToken::new())
                .unwrap()
                .picture
                .is_none()
        );
    }

    #[test]
    fn a_freecad_documents_picture_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a part’s design.FCStd");
        let thumbnail = png(16, 16);
        zip(
            &path,
            &[
                (
                    "Document.xml",
                    br#"<?xml version='1.0' encoding='utf-8'?>
<Document SchemaVersion="4" ProgramVersion="0.21R33771 (Git)" FileVersion="1"><Properties Count="0"/></Document>"#,
                ),
                ("thumbnails/Thumbnail.png", &thumbnail),
            ],
        );
        let found = freecad(&path, 256, &CancelToken::new()).unwrap();
        assert!(found.picture.is_some());
        assert_eq!(detail(&found, "Saved by"), Some("FreeCAD 0.21R33771 (Git)"));
    }
}
