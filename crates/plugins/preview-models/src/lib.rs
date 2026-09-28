//! bibli plugin: 3D models, drawings and prints, shown by the picture their
//! own application stored in them, with what the file says of itself in
//! Details. A Blender file, a 3MF package, a `FreeCAD` document, a DWG or DXF
//! drawing, a `SketchUp` or Rhino model and a G-code print each carry one;
//! reading it costs a few hundred kilobytes. A mesh that carries none (STL, OBJ, PLY,
//! OFF, glTF, a 3MF its slicer did not picture) is drawn by a helper of its
//! own, `bibli-model-worker`, and measured. The same pictures are the
//! grid's thumbnails, after any thumbnailer the desktop installed for
//! these files.
mod blend;
mod cad;
mod gcode;
mod mesh;
mod package;
mod rhino;
mod sketchup;

use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
};
use bibli_platform::inspection::{self, limits};
use bibli_platform::model::MeshFormat;
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
        "Show Blender files, 3MF packages, FreeCAD documents, SketchUp and Rhino models, DWG and DXF drawings and G-code prints by the picture stored in them, and draw STL, OBJ, PLY, OFF and glTF models; print time, filament, size, volume and version in Details"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        vec![bibli_core::plugin::Requirement::new(
            "bibli-model-worker",
            "3D models without a stored picture drawn and measured",
            || bibli_platform::model::worker_path().is_some(),
        )]
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
    SketchUp,
    LayOut,
    Rhino,
    /// A mesh the helper draws.
    Mesh(MeshFormat),
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
            "skp" => Some(Format::SketchUp),
            "layout" => Some(Format::LayOut),
            "3dm" => Some(Format::Rhino),
            "stl" => Some(Format::Mesh(MeshFormat::Stl)),
            // Also a compiled object's name: the mesh is read only if the
            // file is text (`inspect`).
            "obj" => Some(Format::Mesh(MeshFormat::Obj)),
            "ply" => Some(Format::Mesh(MeshFormat::Ply)),
            "off" => Some(Format::Mesh(MeshFormat::Off)),
            "gltf" | "glb" => Some(Format::Mesh(MeshFormat::Gltf)),
            _ => None,
        };
        by_name.or_else(|| match mime? {
            "application/x-blender" => Some(Format::Blend),
            "text/x.gcode" | "text/x-gcode" => Some(Format::Gcode),
            "model/3mf" => Some(Format::ThreeMf),
            "image/vnd.dwg" => Some(Format::Dwg),
            "image/vnd.dxf" => Some(Format::Dxf),
            "model/stl" | "model/x.stl-ascii" | "model/x.stl-binary" => {
                Some(Format::Mesh(MeshFormat::Stl))
            }
            "model/obj" => Some(Format::Mesh(MeshFormat::Obj)),
            "model/gltf-binary" | "model/gltf+json" => Some(Format::Mesh(MeshFormat::Gltf)),
            _ => None,
        })
    }

    /// Whether this file is read here: a mesh only while its helper is
    /// installed, so a thumbnailer the desktop has (f3d) can show it
    /// instead. Asked on a worker, never while drawing.
    fn available(self) -> bool {
        !matches!(self, Format::Mesh(_)) || bibli_platform::model::worker_path().is_some()
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
            Format::SketchUp => sketchup::model(path, side, cancel),
            Format::LayOut => sketchup::layout(path, side, cancel),
            Format::Rhino => rhino::read(path, side, cancel),
            Format::Mesh(format) => mesh::read(path, format, side, cancel),
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
    /// What was left out, and why.
    notice: Option<String>,
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
        Format::of(&req.entry.path, req.mime.as_deref()).is_some_and(Format::available)
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
        if format == Format::Mesh(MeshFormat::Obj) && !is_text(&path)? {
            return Ok(Some(Inspection {
                kind: InspectionKind::Text,
                title: "Object file".into(),
                notice: "Not a Wavefront OBJ model".into(),
                ..Inspection::default()
            }));
        }
        let side = match format {
            Format::Mesh(_) | Format::ThreeMf => MESH_SIDE,
            _ => limits::IMAGE_DISPLAY_SIDE,
        };
        let found = format.read(&path, side, cancel)?;
        let notice = found.notice.unwrap_or_default();
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
                notice,
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
                notice: if notice.is_empty() {
                    "No picture is stored in this file".into()
                } else {
                    notice
                },
                ..Inspection::default()
            },
        }))
    }
}

/// The side a mesh is drawn at for the panel: its width on a dense screen.
const MESH_SIDE: u32 = 1024;

/// Whether the file begins as text: an OBJ model does, a compiled object
/// does not.
fn is_text(path: &Path) -> anyhow::Result<bool> {
    use std::io::Read;
    let mut head = Vec::new();
    inspection::open(path)?.take(1024).read_to_end(&mut head)?;
    Ok(inspection::text_prefix(&head).is_some_and(|text| text.len() + 4 >= head.len()))
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
        if !format.available() || (format == Format::Mesh(MeshFormat::Obj) && !is_text(&path)?) {
            return Ok(None);
        }
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
        assert_eq!(of("house.skp", None), Some(Format::SketchUp));
        assert_eq!(of("sheets.layout", None), Some(Format::LayOut));
        assert_eq!(of("part.3dm", None), Some(Format::Rhino));
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
    /// Point `BIBLI_MODEL_WORKER` at `worker`. Tests run in processes of
    /// their own under nextest, so the variable reaches no other test.
    fn use_worker(worker: &Path) {
        // SAFETY: no other thread of this test process reads the environment.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("BIBLI_MODEL_WORKER", worker);
        }
    }

    /// The helper built into the target directory this test runs from
    /// (the test binary sits in its `deps`). Without it the test says so
    /// and passes, unless `BIBLI_REQUIRE_WORKERS` is set.
    fn with_model_worker() -> bool {
        let built = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("bibli-model-worker")))
            .filter(|path| path.is_file());
        let Some(worker) = built else {
            assert!(
                std::env::var_os("BIBLI_REQUIRE_WORKERS").is_none(),
                "bibli-model-worker is not built"
            );
            eprintln!("skipped: bibli-model-worker is not built");
            return false;
        };
        use_worker(&worker);
        true
    }

    /// A binary STL of a 10 mm cube.
    fn cube_stl() -> Vec<u8> {
        let p = |x: u8, y: u8, z: u8| [f32::from(x) * 10., f32::from(y) * 10., f32::from(z) * 10.];
        let faces = [
            [p(0, 0, 0), p(1, 1, 0), p(1, 0, 0)],
            [p(0, 0, 0), p(0, 1, 0), p(1, 1, 0)],
            [p(0, 0, 1), p(1, 0, 1), p(1, 1, 1)],
            [p(0, 0, 1), p(1, 1, 1), p(0, 1, 1)],
            [p(0, 0, 0), p(1, 0, 0), p(1, 0, 1)],
            [p(0, 0, 0), p(1, 0, 1), p(0, 0, 1)],
            [p(1, 0, 0), p(1, 1, 0), p(1, 1, 1)],
            [p(1, 0, 0), p(1, 1, 1), p(1, 0, 1)],
            [p(1, 1, 0), p(0, 1, 0), p(0, 1, 1)],
            [p(1, 1, 0), p(0, 1, 1), p(1, 1, 1)],
            [p(0, 1, 0), p(0, 0, 0), p(0, 0, 1)],
            [p(0, 1, 0), p(0, 0, 1), p(0, 1, 1)],
        ];
        let mut out = vec![b' '; 80];
        out.extend(12u32.to_le_bytes());
        for face in faces {
            out.extend([0u8; 12]);
            for v in face.iter().flatten() {
                out.extend(v.to_le_bytes());
            }
            out.extend([0, 0]);
        }
        out
    }

    #[test]
    fn a_mesh_is_drawn_and_measured_by_its_helper() {
        if !with_model_worker() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a part’s cube.stl");
        std::fs::write(&path, cube_stl()).unwrap();
        let request = request(&path);
        assert!(Provider.accepts(&request));
        let data = Provider
            .inspect(&request, &InspectRequest::default(), &CancelToken::new())
            .unwrap()
            .unwrap();
        let image = data.image.expect("the model drawn");
        assert_eq!(image.width.max(image.height), MESH_SIDE);
        let said: Vec<String> = data
            .details
            .iter()
            .map(|d| format!("{}: {}", d.label, d.value))
            .collect();
        assert_eq!(
            said,
            [
                "Size: 10 × 10 × 10 mm",
                "Volume: 1 cm³",
                "Watertight: Yes",
                "Triangles: 12"
            ]
        );
        let thumbnail = Pictures
            .thumbnail(
                &ThumbRequest {
                    entry: request.entry.clone(),
                    mime: None,
                    mtime: None,
                    size: bibli_core::thumbs::ThumbSize::Normal,
                },
                &CancelToken::new(),
            )
            .unwrap()
            .expect("the grid's picture");
        assert_eq!(thumbnail.width.max(thumbnail.height), 128);
        // A compiled object named like a model is not drawn.
        let object = dir.path().join("main.obj");
        std::fs::write(&object, b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0\x01\0").unwrap();
        let data = Provider
            .inspect(
                &super::tests::request(&object),
                &InspectRequest::default(),
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert!(data.image.is_none());
        assert_eq!(data.notice, "Not a Wavefront OBJ model");
    }

    #[test]
    fn without_its_helper_a_mesh_is_left_to_the_desktop() {
        use_worker(Path::new("/nonexistent/bibli-model-worker"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cube.stl");
        std::fs::write(&path, cube_stl()).unwrap();
        assert!(
            !Provider.accepts(&request(&path)),
            "f3d's picture may show it"
        );
        // A 3MF is still read for what it says, and says what is missing.
        let package = dir.path().join("bare.3mf");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
        zip.start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(
            &mut zip,
            br#"<model unit="millimeter"><metadata name="Application">A slicer</metadata><resources/></model>"#,
        )
        .unwrap();
        zip.finish().unwrap();
        let data = Provider
            .inspect(
                &request(&package),
                &InspectRequest::default(),
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert!(data.image.is_none());
        assert!(
            data.notice.contains("bibli-model-worker"),
            "{}",
            data.notice
        );
        assert_eq!(data.details[0].value, "A slicer");
    }
}
