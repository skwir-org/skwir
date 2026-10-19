//! skwir plugin: 3D models, drawings and prints, shown by the picture their
//! own application stored in them, with what the file says of itself in
//! Properties. A Blender file, a 3MF package, a `FreeCAD` document, a DWG
//! drawing, a `SketchUp` or Rhino model and a G-code print each carry one;
//! reading it costs a few hundred kilobytes. A DXF drawing is drawn from
//! its entities (`dxf`), which most never picture, and G-code that stores
//! none from its moves (`toolpath`). A mesh that carries none (STL, OBJ, PLY,
//! OFF, glTF, a 3MF its slicer did not picture) is drawn by a helper of its
//! own, `skwir-model-worker`, and measured, as is a STEP model, whose faces
//! the helper meshes (`step`, ADR-069); a window that draws meshes gets the
//! mesh to turn instead (ADR-067). The same pictures are the grid's
//! thumbnails, after any thumbnailer the desktop installed for these
//! files.
mod blend;
mod cad;
mod dxf;
mod gcode;
mod mesh;
mod package;
mod rhino;
mod sketchup;
mod step;
mod toolpath;

use skwir_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
};
use skwir_platform::inspection::{self, limits, limits::Bounds};
use skwir_platform::model::{Asked, MeshFormat};
use std::path::Path;
use std::sync::Arc;

const ID: &str = "models";

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "3D models and prints"
    }
    fn description(&self) -> &'static str {
        "Show Blender files, 3MF packages, FreeCAD documents, SketchUp and Rhino models, DWG drawings and G-code prints by the picture stored in them, and draw DXF drawings, G-code toolpaths, the meshes Rhino models keep and STL, OBJ, PLY, OFF, glTF and STEP models; print time, filament, size, volume and version in Properties"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![
            skwir_platform::model::WORKER
                .requirement("3D models without a stored picture drawn and measured"),
        ]
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
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
            // Prints, and milling and laser programs; `.nc` is also
            // `NetCDF`'s, whose binary files are left to others (`inspect`).
            "gcode" | "gco" | "nc" | "ngc" | "tap" | "cnc" => Some(Format::Gcode),
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
            "model/step" => Some(Format::Mesh(MeshFormat::Step)),
            _ => None,
        })
    }

    /// Whether this file is read here: a mesh only while its helper is
    /// installed, so a thumbnailer the desktop has (f3d) can show it
    /// instead. Asked on a worker, never while drawing.
    fn available(self) -> bool {
        !matches!(self, Format::Mesh(_)) || skwir_platform::model::worker_path().is_some()
    }

    /// Text a reader can switch to.
    fn is_text(self) -> bool {
        matches!(self, Format::Gcode | Format::Dxf)
    }

    /// What the file says of itself, its picture at most `side` pixels on
    /// its longer side, or its mesh when it is asked to be turned; a model
    /// that stores a picture shows it while `picture_first`.
    fn read(
        self,
        path: &Path,
        side: u32,
        asked: Asked,
        picture_first: bool,
        bounds: Bounds,
        cancel: &CancelToken,
    ) -> anyhow::Result<Found> {
        skwir_platform::thumb_providers::ensure_file(path)?;
        match self {
            Format::Blend => blend::read(path, side, cancel),
            Format::Gcode => gcode::text(path, side, cancel),
            Format::Bgcode => gcode::binary(path, side, cancel),
            Format::ThreeMf => package::three_mf(path, side, asked, picture_first, cancel),
            Format::FreeCad => package::freecad(path, side, cancel),
            Format::Dwg => cad::dwg(path, side, cancel),
            Format::Dxf => cad::dxf(path, side, bounds, cancel),
            Format::SketchUp => sketchup::model(path, side, cancel),
            Format::LayOut => sketchup::layout(path, side, cancel),
            Format::Rhino => rhino::read(path, side, asked, picture_first, cancel),
            Format::Mesh(MeshFormat::Step) => step::read(path, side, asked, bounds, cancel),
            Format::Mesh(format) => mesh::read(path, format, side, asked, cancel),
        }
    }
}

/// What a file says of itself.
#[derive(Default)]
struct Found {
    /// What it is, for the toolbar.
    title: String,
    picture: Option<Image>,
    /// The mesh itself, read for a window that turns it (`Asked::Turned`).
    mesh: Option<Arc<skwir_core::inspection::Mesh>>,
    /// Whether its mesh can be turned in 3D, whichever was shown.
    mesh_available: bool,
    /// Facts in the order they are shown.
    details: Vec<(String, String)>,
    /// What was left out, and why.
    notice: Option<String>,
    /// Whether it was left out to stay quick: Full preview shows it.
    whole_available: bool,
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
    /// Called on a worker (the registry's dispatch), so a CNC program's
    /// name, also a binary format's (`.nc`), is settled by its start: the
    /// first provider that accepts owns the file.
    fn accepts(&self, req: &PreviewRequest) -> bool {
        Format::of(&req.entry.path, req.mime.as_deref()).is_some_and(|format| {
            format.available()
                && (!machining(&req.entry.path) || is_text(&req.entry.path).unwrap_or(false))
        })
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
            Format::Mesh(_) | Format::ThreeMf | Format::Rhino => MESH_SIDE,
            _ => limits::IMAGE_DISPLAY_SIDE,
        };
        // A window that draws meshes turns a model, unless the user chose
        // its picture; one that stores a picture shows it, unless the user
        // chose 3D. Past `model::VIEW_TRIANGLES`, only when asked whole.
        let asked = if view.meshes && view.turned != Some(false) {
            Asked::Turned { whole: view.whole }
        } else {
            Asked::Preview
        };
        let found = format.read(
            &path,
            side,
            asked,
            view.turned != Some(true),
            Bounds::of(view),
            cancel,
        )?;
        let whole_available = found.whole_available;
        let notice = found.notice.unwrap_or_default();
        let details = found
            .details
            .into_iter()
            .map(|(label, value)| skwir_core::inspection::Detail::new(label, value))
            .collect();
        if let Some(mesh) = found.mesh {
            return Ok(Some(Inspection {
                kind: InspectionKind::Model,
                title: found.title,
                mesh: Some(mesh),
                mesh_available: true,
                details,
                notice,
                ..Inspection::default()
            }));
        }
        Ok(Some(match found.picture {
            Some(picture) => Inspection {
                kind: InspectionKind::Text,
                title: found.title,
                image: Some(picture),
                details,
                source_available: format.is_text(),
                mesh_available: found.mesh_available,
                notice,
                whole_available,
                ..Inspection::default()
            },
            // Text without a picture is read as the text it is, saying what
            // was left out.
            None if format.is_text() => {
                let mut data = inspection::plain(&path, cancel)?;
                data.details = details;
                data.notice = [notice, std::mem::take(&mut data.notice)]
                    .into_iter()
                    .filter(|notice| !notice.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                data.whole_available = whole_available;
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
                whole_available,
                ..Inspection::default()
            },
        }))
    }
}

/// The side a mesh is drawn at for the panel: its width on a dense screen.
const MESH_SIDE: u32 = 1024;

/// Whether the name is a milling or laser program's, which other formats
/// share (`NetCDF`'s `.nc`, a tape image's `.tap`).
fn machining(path: &Path) -> bool {
    matches!(
        inspection::extension(path).as_str(),
        "nc" | "ngc" | "tap" | "cnc"
    )
}

/// Whether the file begins as text: an OBJ model does, a compiled object
/// does not.
fn is_text(path: &Path) -> anyhow::Result<bool> {
    use std::io::Read;
    let mut head = Vec::new();
    inspection::open(path)?.take(1024).read_to_end(&mut head)?;
    Ok(inspection::is_text(&head))
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
        let text_only = format == Format::Mesh(MeshFormat::Obj) || machining(&path);
        if !format.available() || (text_only && !is_text(&path)?) {
            return Ok(None);
        }
        Ok(format
            .read(
                &path,
                req.size.pixels(),
                Asked::Thumbnail,
                true,
                Bounds::QUICK,
                cancel,
            )?
            .picture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::op::EntryRef;

    fn request(path: &Path) -> PreviewRequest {
        PreviewRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 2048,
            text_size: None,
            fit: None,
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
        assert_eq!(of("sign.nc", None), Some(Format::Gcode));
        // A STEP model by its type alone: `.stp` names others too.
        assert_eq!(
            of("bracket.stp", Some("model/step")),
            Some(Format::Mesh(MeshFormat::Step))
        );
        assert_eq!(of("bracket.step", None), None);
        assert_eq!(of("notes.txt", Some("text/plain")), None);
    }

    #[test]
    fn gcode_without_a_picture_is_drawn_or_read_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a part’s print.gcode");
        // Nothing moves: its text, with its facts.
        std::fs::write(&path, ";TIME:65\nM104 S200\nM140 S60\n").unwrap();
        let cancel = CancelToken::new();
        let data = Provider
            .inspect(&request(&path), &InspectRequest::default(), &cancel)
            .unwrap()
            .unwrap();
        assert!(data.image.is_none());
        assert!(data.plain_text().contains("M104 S200"));
        assert_eq!(data.details[0].label, "Print time");
        assert_eq!(data.details[0].value, "1m 5s");
        assert!(!data.source_available, "it is its source already");
        // It moves: drawn, its text one toggle away.
        std::fs::write(&path, ";TIME:65\nG28\nG1 X10 Y5 F600\n").unwrap();
        let data = Provider
            .inspect(&request(&path), &InspectRequest::default(), &cancel)
            .unwrap()
            .unwrap();
        assert!(data.image.is_some());
        assert!(data.source_available);
        assert_eq!(data.details[1].label, "Size");
        // A binary file named like a program is left to the others.
        let binary = dir.path().join("climate.nc");
        std::fs::write(&binary, b"CDF\x01\0\0\0\x0a\0\0\0\x01").unwrap();
        assert!(!Provider.accepts(&request(&binary)));
        let program = dir.path().join("sign.nc");
        std::fs::write(&program, "G0 X0 Y0\nG1 X10\n").unwrap();
        assert!(Provider.accepts(&request(&program)));
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
    /// Point `SKWIR_MODEL_WORKER` at `worker`. Tests run in processes of
    /// their own under nextest, so the variable reaches no other test.
    fn use_worker(worker: &Path) {
        // SAFETY: no other thread of this test process reads the environment.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("SKWIR_MODEL_WORKER", worker);
        }
    }

    /// The helper built into the target directory this test runs from
    /// (the test binary sits in its `deps`). Without it the test says so
    /// and passes, unless `SKWIR_REQUIRE_WORKERS` is set.
    fn with_model_worker() -> bool {
        let built = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("skwir-model-worker")))
            .filter(|path| path.is_file());
        let Some(worker) = built else {
            assert!(
                std::env::var_os("SKWIR_REQUIRE_WORKERS").is_none(),
                "skwir-model-worker is not built"
            );
            eprintln!("skipped: skwir-model-worker is not built");
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
                    size: skwir_core::thumbs::ThumbSize::Normal,
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

    /// A cylinder 20 mm across and 30 mm tall as a CAD program exports it.
    const STEP_CYLINDER: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('cylinder','2026-10-19T00:00:00',(''),(''),'Translator 1','Modeller 2026','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
#1=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
#2=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );
#3=( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() );
#4=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-7),#1,'distance_accuracy_value','');
#5=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#4)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#1,#2,#3)) REPRESENTATION_CONTEXT('','3D') );
#6=APPLICATION_CONTEXT('automotive design');
#7=PRODUCT_CONTEXT('',#6,'mechanical');
#8=PRODUCT_DEFINITION_CONTEXT('part definition',#6,'design');
#9=PRODUCT('cylinder','cylinder','',(#7));
#10=PRODUCT_DEFINITION_FORMATION('','',#9);
#11=PRODUCT_DEFINITION('design','',#10,#8);
#12=PRODUCT_DEFINITION_SHAPE('','',#11);
#13=CARTESIAN_POINT('',(0.,0.,0.));
#14=DIRECTION('',(0.,0.,1.));
#15=DIRECTION('',(1.,0.,0.));
#16=AXIS2_PLACEMENT_3D('',#13,#14,#15);
#17=CIRCLE('',#16,10.);
#18=CARTESIAN_POINT('',(10.,0.,0.));
#19=VERTEX_POINT('',#18);
#20=EDGE_CURVE('',#19,#19,#17,.T.);
#21=CARTESIAN_POINT('',(0.,0.,30.));
#22=AXIS2_PLACEMENT_3D('',#21,#14,#15);
#23=CIRCLE('',#22,10.);
#24=CARTESIAN_POINT('',(10.,0.,30.));
#25=VERTEX_POINT('',#24);
#26=EDGE_CURVE('',#25,#25,#23,.T.);
#27=PLANE('',#16);
#28=ADVANCED_FACE('',(#29),#27,.F.);
#29=FACE_OUTER_BOUND('',#30,.T.);
#30=EDGE_LOOP('',(#31));
#31=ORIENTED_EDGE('',*,*,#20,.F.);
#32=CYLINDRICAL_SURFACE('',#16,10.);
#33=VECTOR('',#14,30.);
#34=LINE('',#18,#33);
#35=EDGE_CURVE('',#19,#25,#34,.T.);
#36=EDGE_LOOP('',(#37,#38,#39,#40));
#37=ORIENTED_EDGE('',*,*,#20,.T.);
#38=ORIENTED_EDGE('',*,*,#35,.T.);
#39=ORIENTED_EDGE('',*,*,#26,.F.);
#40=ORIENTED_EDGE('',*,*,#35,.F.);
#41=ADVANCED_FACE('',(#42),#32,.T.);
#42=FACE_OUTER_BOUND('',#36,.T.);
#43=PLANE('',#22);
#44=ADVANCED_FACE('',(#45),#43,.T.);
#45=FACE_OUTER_BOUND('',#46,.T.);
#46=EDGE_LOOP('',(#47));
#47=ORIENTED_EDGE('',*,*,#26,.T.);
#48=CLOSED_SHELL('',(#28,#41,#44));
#49=MANIFOLD_SOLID_BREP('',#48);
#50=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#16,#49),#5);
#51=SHAPE_DEFINITION_REPRESENTATION(#12,#50);
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn a_step_model_is_drawn_by_its_helper_up_to_its_bound() {
        if !with_model_worker() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a part’s cylinder.stp");
        std::fs::write(&path, STEP_CYLINDER).unwrap();
        // Known by its type, which the database reads from its name and
        // its first line.
        let step = |path: &Path| PreviewRequest {
            mime: Some("model/step".into()),
            ..request(path)
        };
        assert!(Provider.accepts(&step(&path)));
        let data = Provider
            .inspect(
                &step(&path),
                &InspectRequest::default(),
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(data.title, "STEP model");
        assert!(data.image.is_some(), "the model drawn");
        assert!(data.notice.is_empty(), "{}", data.notice);
        let said: Vec<String> = data
            .details
            .iter()
            .map(|d| format!("{}: {}", d.label, d.value))
            .collect();
        assert_eq!(
            said[..3],
            [
                "Made with: Modeller 2026",
                "Format: STEP AP214",
                "Size: 20 × 20 × 30 mm"
            ]
        );
        assert_eq!(said[4..6], ["Watertight: Yes", "Faces: 3"]);

        // Past the quick bound, refused for Full preview to draw, before
        // the helper reads a byte.
        let large = dir.path().join("large.step");
        let file = std::fs::File::create(&large).unwrap();
        file.set_len(limits::BINARY_INPUT as u64 + 1).unwrap();
        let error = Provider
            .inspect(
                &step(&large),
                &InspectRequest::default(),
                &CancelToken::new(),
            )
            .unwrap_err();
        assert!(skwir_core::inspection::TooLarge::is(&error), "{error:#}");
        assert_eq!(error.to_string(), "Too large to draw (more than 16 MiB)");
        let whole = InspectRequest {
            whole: true,
            ..InspectRequest::default()
        };
        // Within Full preview's bound it is read: zeros are no model.
        let error = Provider
            .inspect(&step(&large), &whole, &CancelToken::new())
            .unwrap_err();
        assert!(!skwir_core::inspection::TooLarge::is(&error));
        assert!(format!("{error:#}").contains("STEP"), "{error:#}");
    }

    #[test]
    fn a_rhino_model_shows_its_picture_and_offers_its_meshes() {
        if !with_model_worker() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a part’s model.3dm");
        std::fs::write(&path, rhino::tests::model(5, 3)).unwrap();
        let data = Provider
            .inspect(
                &request(&path),
                &InspectRequest::default(),
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert!(data.image.is_some(), "its picture");
        assert!(data.mesh_available, "3D offered");
        // Turned, a model that stores no mesh keeps its picture and says
        // why.
        let turned = InspectRequest {
            meshes: true,
            turned: Some(true),
            ..InspectRequest::default()
        };
        let data = Provider
            .inspect(&request(&path), &turned, &CancelToken::new())
            .unwrap()
            .unwrap();
        assert!(data.image.is_some(), "its picture still");
        assert!(data.notice.contains("no triangles"), "{}", data.notice);
        assert_eq!(data.details[0].value, "Rhinoceros 5.0");
    }

    #[test]
    fn a_window_that_draws_meshes_gets_the_mesh_to_turn() {
        if !with_model_worker() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a part’s cube.stl");
        std::fs::write(&path, cube_stl()).unwrap();
        let turned = InspectRequest {
            meshes: true,
            ..InspectRequest::default()
        };
        let data = Provider
            .inspect(&request(&path), &turned, &CancelToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(data.kind, InspectionKind::Model);
        assert!(data.image.is_none(), "no picture is drawn");
        let mesh = data.mesh.expect("the mesh");
        assert_eq!((mesh.positions().len(), mesh.triangles()), (8, 12));
        assert_eq!(data.details[0].value, "10 × 10 × 10 mm");
        assert!(data.notice.is_empty());
        // Asked for a picture, it is still drawn.
        let data = Provider
            .inspect(
                &request(&path),
                &InspectRequest::default(),
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert!(data.mesh.is_none() && data.image.is_some());
    }

    /// A 3MF of a tetrahedron, with the picture its slicer stored.
    fn pictured_3mf(path: &Path) {
        let model = r#"<?xml version="1.0"?><model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><object id="1" type="model"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="10" y="0" z="0"/><vertex x="0" y="10" z="0"/><vertex x="0" y="0" z="10"/></vertices><triangles><triangle v1="0" v2="2" v3="1"/><triangle v1="0" v2="1" v3="3"/><triangle v1="1" v2="2" v3="3"/><triangle v1="2" v2="0" v3="3"/></triangles></mesh></object></resources><build><item objectid="1"/></build></model>"#;
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(6, 4, image::Rgba([9, 8, 7, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in [
            ("3D/3dmodel.model", model.as_bytes()),
            ("Metadata/thumbnail.png", png.as_slice()),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    /// A model that stores a picture shows it unless 3D is chosen; a mesh
    /// is turned unless its picture is chosen; both say they can turn.
    #[test]
    fn a_model_is_turned_or_pictured_as_chosen() {
        if !with_model_worker() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let ask = |path: &Path, turned: Option<bool>| {
            let view = InspectRequest {
                meshes: true,
                turned,
                ..InspectRequest::default()
            };
            Provider
                .inspect(&request(path), &view, &CancelToken::new())
                .unwrap()
                .unwrap()
        };
        let package = dir.path().join("a slicer’s plate.3mf");
        pictured_3mf(&package);
        let data = ask(&package, None);
        assert_eq!(data.kind, InspectionKind::Text);
        let picture = data.image.as_ref().expect("the stored picture");
        assert_eq!((picture.width, picture.height), (6, 4));
        assert!(data.mesh_available);
        let data = ask(&package, Some(true));
        assert_eq!(data.kind, InspectionKind::Model);
        assert_eq!(data.mesh.expect("the mesh").triangles(), 4);
        assert!(data.mesh_available);
        // An STL: turned unless its picture is chosen.
        let stl = dir.path().join("cube.stl");
        std::fs::write(&stl, cube_stl()).unwrap();
        assert_eq!(ask(&stl, None).kind, InspectionKind::Model);
        let data = ask(&stl, Some(false));
        assert!(data.image.is_some() && data.mesh.is_none());
        assert!(data.mesh_available);
    }

    #[test]
    fn without_its_helper_a_mesh_is_left_to_the_desktop() {
        use_worker(Path::new("/nonexistent/skwir-model-worker"));
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
            data.notice.contains("skwir-model-worker"),
            "{}",
            data.notice
        );
        assert_eq!(data.details[0].value, "A slicer");
    }
}
