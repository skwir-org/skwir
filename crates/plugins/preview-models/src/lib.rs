//! bibli plugin: 3D models, drawings and prints, shown by the picture their
//! own application stored in them, with what the file says of itself in
//! Details. A Blender file, a 3MF package, a `FreeCAD` document, a DWG or DXF
//! drawing and a G-code print each carry one, near their start; reading it
//! costs a few hundred kilobytes, and nothing is drawn in 3D here. The same
//! pictures are the grid's thumbnails, after any thumbnailer the desktop
//! installed for these files.
mod blend;
mod cad;
mod gcode;
mod package;

use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
};
use bibli_platform::inspection::{self, limits};
use std::path::Path;
use std::sync::Arc;

const ID: &str = "preview-models";

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Preview: 3D models and prints"
    }
    fn description(&self) -> &'static str {
        "Show Blender files, 3MF packages, FreeCAD documents, DWG and DXF drawings and G-code prints by the picture stored in them, with their print time, filament, version and scenes in Details"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn register(&self, app: &App) {
        app.previews.register(ID, Arc::new(Provider));
        app.thumbnails.register(ID, Arc::new(Pictures));
    }
}

/// The formats read here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Blend,
    /// G-code as text, as most slicers write it.
    Gcode,
    /// Prusa's binary G-code.
    Bgcode,
    ThreeMf,
    FreeCad,
    Dwg,
    Dxf,
}

impl Format {
    /// By the file's extension, then its type.
    fn of(path: &Path, mime: Option<&str>) -> Option<Format> {
        let extension = inspection::extension(path);
        let by_name = match extension.as_str() {
            "blend" => Some(Format::Blend),
            // Blender's backups: `scene.blend1`, `scene.blend2`.
            name if name
                .strip_prefix("blend")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) =>
            {
                Some(Format::Blend)
            }
            "gcode" | "gco" => Some(Format::Gcode),
            "bgcode" => Some(Format::Bgcode),
            "3mf" => Some(Format::ThreeMf),
            "fcstd" => Some(Format::FreeCad),
            "dwg" => Some(Format::Dwg),
            "dxf" => Some(Format::Dxf),
            _ => None,
        };
        by_name.or_else(|| match mime? {
            "application/x-blender" => Some(Format::Blend),
            "text/x.gcode" | "text/x-gcode" => Some(Format::Gcode),
            "model/3mf" => Some(Format::ThreeMf),
            "image/vnd.dwg" => Some(Format::Dwg),
            "image/vnd.dxf" => Some(Format::Dxf),
            _ => None,
        })
    }

    /// Text a reader can switch to.
    fn is_text(self) -> bool {
        matches!(self, Format::Gcode | Format::Dxf)
    }

    /// What the file says of itself, its picture at most `side` pixels on
    /// its longer side.
    fn read(self, path: &Path, side: u32, cancel: &CancelToken) -> anyhow::Result<Found> {
        bibli_platform::thumb_providers::ensure_file(path)?;
        match self {
            Format::Blend => blend::read(path, side, cancel),
            Format::Gcode => gcode::text(path, side, cancel),
            Format::Bgcode => gcode::binary(path, side, cancel),
            Format::ThreeMf => package::three_mf(path, side, cancel),
            Format::FreeCad => package::freecad(path, side, cancel),
            Format::Dwg => cad::dwg(path, side, cancel),
            Format::Dxf => cad::dxf(path, side, cancel),
        }
    }
}

/// What a file says of itself.
#[derive(Default)]
struct Found {
    /// What it is, for the toolbar.
    title: String,
    picture: Option<Image>,
    /// Facts in the order they are shown.
    details: Vec<(String, String)>,
}

/// `image` at most `side` pixels on its longer side, never enlarged.
fn fit(image: image::RgbaImage, side: u32) -> Image {
    let image = if image.width().max(image.height()) > side {
        image::DynamicImage::ImageRgba8(image)
            .thumbnail(side, side)
            .into_rgba8()
    } else {
        image
    };
    Image {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw().into(),
    }
}

/// A picture stored in the file, decoded at most `side` on its longer side.
fn decoded(bytes: &[u8], side: u32) -> Option<Image> {
    let picture = inspection::picture(bytes, side).ok()?;
    Some(Image {
        width: picture.width,
        height: picture.height,
        rgba: picture.rgba.into(),
    })
}

struct Provider;

impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        ID
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        Format::of(&req.entry.path, req.mime.as_deref()).is_some()
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        // The source is the shared code view (the worker's fallback).
        if view.source {
            return Ok(None);
        }
        let path = req.entry.resolve()?;
        let Some(format) = Format::of(&path, req.mime.as_deref()) else {
            return Ok(None);
        };
        let found = format.read(&path, limits::IMAGE_DISPLAY_SIDE, cancel)?;
        let details = found
            .details
            .into_iter()
            .map(|(label, value)| bibli_core::inspection::Detail::new(label, value))
            .collect();
        Ok(Some(match found.picture {
            Some(picture) => Inspection {
                kind: InspectionKind::Text,
                title: found.title,
                image: Some(picture),
                details,
                source_available: format.is_text(),
                ..Inspection::default()
            },
            // Text without a picture is read as the text it is.
            None if format.is_text() => {
                let mut data = inspection::plain(&path, view.offset, cancel)?;
                data.details = details;
                data
            }
            None => Inspection {
                kind: InspectionKind::Text,
                title: found.title,
                details,
                notice: "No picture is stored in this file".into(),
                ..Inspection::default()
            },
        }))
    }
}

/// The grid's thumbnails: the same pictures. A fallback: a thumbnailer the
/// desktop installed for these files (Blender's, f3d) comes first.
struct Pictures;

impl ThumbnailProvider for Pictures {
    fn id(&self) -> &'static str {
        "model-pictures"
    }
    fn stage(&self) -> Stage {
        Stage::Fallback
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        Format::of(&req.entry.path, req.mime.as_deref()).is_some()
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = req.entry.resolve()?;
        let Some(format) = Format::of(&path, req.mime.as_deref()) else {
            return Ok(None);
        };
        Ok(format.read(&path, req.size.pixels(), cancel)?.picture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::op::EntryRef;

    fn request(path: &Path) -> PreviewRequest {
        PreviewRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 2048,
            text_size: None,
        }
    }

    #[test]
    fn formats_by_name_then_type() {
        let of = |name: &str, mime| Format::of(Path::new(name), mime);
        assert_eq!(of("scene.blend", None), Some(Format::Blend));
        assert_eq!(of("scene.blend1", None), Some(Format::Blend));
        assert_eq!(of("scene.blender", None), None);
        assert_eq!(of("PART.FCStd", None), Some(Format::FreeCad));
        assert_eq!(of("plate.3MF", None), Some(Format::ThreeMf));
        assert_eq!(of("print", Some("text/x.gcode")), Some(Format::Gcode));
        assert_eq!(of("notes.txt", Some("text/plain")), None);
    }

    #[test]
    fn gcode_without_a_picture_is_read_as_text_with_its_facts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a part’s print.gcode");
        std::fs::write(&path, ";TIME:65\nG28\nG1 X10\n").unwrap();
        let cancel = CancelToken::new();
        let data = Provider
            .inspect(&request(&path), &InspectRequest::default(), &cancel)
            .unwrap()
            .unwrap();
        assert!(data.image.is_none());
        assert!(data.plain_text().contains("G1 X10"));
        assert_eq!(data.details[0].label, "Print time");
        assert_eq!(data.details[0].value, "1m 5s");
        assert!(!data.source_available, "it is its source already");
    }

    #[test]
    fn a_picture_with_its_source_one_toggle_away() {
        use base64::Engine as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pictured.gcode");
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 2, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        std::fs::write(
            &path,
            format!(
                "; thumbnail begin 4x2 {}\n; {encoded}\n; thumbnail end\nG28\n",
                encoded.len()
            ),
        )
        .unwrap();
        let cancel = CancelToken::new();
        let data = Provider
            .inspect(&request(&path), &InspectRequest::default(), &cancel)
            .unwrap()
            .unwrap();
        let image = data.image.expect("the stored picture");
        assert_eq!((image.width, image.height), (4, 2));
        assert!(data.source_available);
        let source = InspectRequest {
            source: true,
            ..InspectRequest::default()
        };
        assert!(
            Provider
                .inspect(&request(&path), &source, &cancel)
                .unwrap()
                .is_none(),
            "the shared source view takes it"
        );
    }

    #[test]
    fn a_drawing_without_a_picture_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.dwg");
        std::fs::write(&path, b"AC1015 and nothing more").unwrap();
        let data = Provider
            .inspect(
                &request(&path),
                &InspectRequest::default(),
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert!(data.image.is_none());
        assert_eq!(data.notice, "No picture is stored in this file");
        assert_eq!(data.details[0].value, "DWG 2000");
    }
}
