//! 3D meshes, read and drawn by `skwir-model-worker` in a process of its
//! own: a model can hold millions of triangles, whose memory the helper
//! gives back when it exits, and a reader that fails takes nothing with
//! it. What comes back is what the mesh measures and its picture, or, for a
//! window that draws it (ADR-067), the mesh itself.
use std::path::{Path, PathBuf};

use skwir_core::{CancelToken, thumbs::Image};

use crate::inspection::limits;

mod buffer;

/// Triangles drawn at most: a detailed print is a few million.
pub const TRIANGLES: usize = 8_000_000;
/// A model file read at most, and what a package's models unpack to.
pub const BYTES: u64 = 512 * 1024 * 1024;
/// Triangles a window turns at most (`Asked::Turned`): a larger mesh is
/// drawn. A mesh this large takes some 150 MB of the GPU's and as much of
/// the window's.
pub const VIEW_TRIANGLES: usize = 4_000_000;

/// The formats the helper reads, as it names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshFormat {
    Stl,
    Obj,
    Ply,
    Off,
    /// glTF, as JSON or binary (GLB).
    Gltf,
    ThreeMf,
    /// STEP: exact surfaces, meshed by the helper (ADR-069).
    Step,
    /// IGES: exact surfaces, meshed as STEP's are.
    Iges,
    /// Rhino: the meshes it stores of its surfaces.
    Rhino,
}

impl MeshFormat {
    fn name(self) -> &'static str {
        match self {
            MeshFormat::Stl => "stl",
            MeshFormat::Obj => "obj",
            MeshFormat::Ply => "ply",
            MeshFormat::Off => "off",
            MeshFormat::Gltf => "gltf",
            MeshFormat::ThreeMf => "3mf",
            MeshFormat::Step => "step",
            MeshFormat::Iges => "iges",
            MeshFormat::Rhino => "3dm",
        }
    }
}

/// What a mesh measures.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measures {
    pub triangles: u64,
    /// The objects the file places (a 3MF's build items, a glTF's mesh nodes).
    pub objects: u64,
    /// Width, depth and height, with Z up, in the file's unit.
    pub size: [f64; 3],
    /// The unit the file states (`mm`, `m`), if it states one.
    pub unit: Option<String>,
    /// Whether every edge joins two faces: a solid that can be printed.
    pub closed: bool,
    /// The volume enclosed, in the file's unit cubed, when it is closed.
    pub volume: Option<f64>,
    /// The faces of a model made of surfaces (STEP, IGES), and how many of them
    /// could not be meshed: holes in what is shown. Zero for a mesh.
    pub faces: u64,
    pub faces_missing: u64,
    /// The objects of a model that keeps meshes of its surfaces (Rhino)
    /// that store none: not drawn. Zero for a mesh.
    pub objects_missing: u64,
}

/// What the helper hands back of a mesh.
pub enum Shown {
    Picture(Image),
    /// The mesh itself, for a window that draws it (`Asked::Turned`).
    Mesh(skwir_core::inspection::Mesh),
}

/// A mesh's picture, or the mesh, and its measures.
pub struct Drawn {
    pub shown: Shown,
    pub measures: Measures,
}

/// The helper's executable: `SKWIR_MODEL_WORKER`, beside our binary, or
/// on `PATH`, if it is there. Plugins that require it name this, not the
/// literals; whether a model is read at all depends on it.
pub fn worker_path() -> Option<PathBuf> {
    WORKER.find()
}

/// The helper. Plugins that require it name this, not the literals.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "SKWIR_MODEL_WORKER",
    executable: "skwir-model-worker",
};

/// The files a glTF model at `path` keeps its buffers in, each with its
/// path from the model's folder: those its JSON names by a plain relative
/// path below that folder, resolving to files still below it, as the
/// helper reads them (`gltf_mesh::buffer`). Read here only to say what
/// the sandbox lets it read; bind resolved files under their decoded names.
fn gltf_buffers(path: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Some(json) = gltf_json(path) else {
        return Vec::new();
    };
    let Some(folder) = buffer::Folder::of_model(path) else {
        return Vec::new();
    };
    json.get("buffers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|buffer| buffer.get("uri")?.as_str())
        .filter_map(|uri| folder.resolve(uri))
        .collect()
}

/// Read only the JSON describing buffers, including a GLB's first chunk.
/// Its binary payload can be large; discovering sandbox inputs never reads
/// it. Untrusted lengths must fit both the JSON bound and the actual file.
fn gltf_json(path: &Path) -> Option<serde_json::Value> {
    use std::io::Read;

    let mut file = crate::file::open_regular(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut prefix = Vec::new();
    (&mut file).take(12).read_to_end(&mut prefix).ok()?;
    let mut json;
    if prefix.starts_with(b"glTF") {
        let word = |at| Some(u32::from_le_bytes(prefix.get(at..at + 4)?.try_into().ok()?));
        if word(4)? != 2 || u64::from(word(8)?) != length {
            return None;
        }
        let mut chunk = [0; 8];
        file.read_exact(&mut chunk).ok()?;
        let bytes = u32::from_le_bytes(chunk[..4].try_into().ok()?);
        if &chunk[4..] != b"JSON"
            || !bytes.is_multiple_of(4)
            || u64::from(bytes) > limits::TEXT_INPUT as u64
            || u64::from(bytes) + 20 > length
        {
            return None;
        }
        json = vec![0; usize::try_from(bytes).ok()?];
        file.read_exact(&mut json).ok()?;
    } else {
        if length > limits::TEXT_INPUT as u64 {
            return None;
        }
        json = prefix;
        file.take(limits::TEXT_INPUT as u64 + 1 - json.len() as u64)
            .read_to_end(&mut json)
            .ok()?;
        if json.len() > limits::TEXT_INPUT {
            return None;
        }
    }
    serde_json::from_slice(&json).ok()
}

/// Models drawn at once at most: a helper reading a large mesh holds most
/// of a gigabyte.
static TURNS: crate::process::Turns = crate::process::Turns::new(2);

/// Who asks for a drawing, which decides whether it waits for its turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// The preview of the file looked at: it waits.
    Preview,
    /// The preview, for a window that draws the mesh itself: it waits, and
    /// a mesh of up to `VIEW_TRIANGLES` comes whole, not as a picture; up to
    /// `TRIANGLES` when the user asked for all of it (`whole`).
    Turned { whole: bool },
    /// A thumbnail: with no turn free it answers
    /// [`skwir_core::thumbs::Busy`], and its worker goes on with other
    /// files until one is.
    Thumbnail,
}

/// The mesh at `path` drawn at most `side` pixels on its longer side, and
/// measured. An error when the helper is missing, or says why it cannot.
pub fn draw(
    path: &Path,
    format: MeshFormat,
    side: u32,
    asked: Asked,
    cancel: &CancelToken,
) -> anyhow::Result<Drawn> {
    let worker = WORKER.require("draw 3D models")?;
    let turn = match asked {
        Asked::Preview | Asked::Turned { .. } => TURNS.take(cancel),
        Asked::Thumbnail => Some(
            TURNS
                .try_take()
                .ok_or_else(|| skwir_core::thumbs::Busy::new(|then| TURNS.when_free(then)))?,
        ),
    };
    let Some(_turn) = turn else {
        anyhow::bail!("cancelled");
    };
    let whole = match asked {
        Asked::Turned { whole: true } => Some(TRIANGLES),
        Asked::Turned { whole: false } => Some(VIEW_TRIANGLES),
        Asked::Preview | Asked::Thumbnail => None,
    };
    draw_with(&worker, path, format, side, whole, cancel)
        .map_err(|error| crate::process::explain(&worker, cancel, error))
}

/// [`draw`] by the helper at `worker`, its turn taken: a mesh of up to
/// `whole` triangles comes whole.
fn draw_with(
    worker: &Path,
    path: &Path,
    format: MeshFormat,
    side: u32,
    whole: Option<usize>,
    cancel: &CancelToken,
) -> anyhow::Result<Drawn> {
    let work = tempfile::tempdir()?;
    // A glTF's buffers may be files beside it: the helper reads them where
    // it reads the model.
    let beside = if format == MeshFormat::Gltf {
        gltf_buffers(path)
    } else {
        Vec::new()
    };
    let mut run = crate::sandbox::untrusted_beside(
        worker,
        path,
        &beside,
        work.path(),
        crate::sandbox::Otherwise::Run,
        &[],
    )?;
    let side = side.clamp(16, 2048);
    // A line of what it measured, then the picture: a PNG of the side
    // asked, at most its pixels and what a PNG adds, past what helpers
    // that answer in text may say. Or the mesh (`mesh_bytes`).
    let picture = u64::from(side) * u64::from(side) * 4 + (1 << 16);
    let mesh = whole.map_or(0, |most| mesh_bytes(most) + (1 << 16));
    let command = run
        .command
        .arg(&run.input)
        .args(["--format", format.name()])
        .args(["--side", &side.to_string()])
        .args(["--triangles", &TRIANGLES.to_string()])
        .args(["--bytes", &BYTES.to_string()]);
    if let Some(most) = whole {
        command.args(["--mesh", &most.to_string()]);
    }
    let finished = crate::process::run_capped(
        command,
        None,
        cancel,
        limits::HELPER_TIMEOUT,
        picture.max(mesh),
    )?;
    anyhow::ensure!(
        finished.status.success(),
        "The model cannot be drawn: {}",
        finished.stderr.trim()
    );
    let out = finished.stdout;
    let line = out
        .iter()
        .position(|b| *b == b'\n')
        .ok_or_else(|| anyhow::anyhow!("The model helper answered nothing"))?;
    let facts: serde_json::Value = serde_json::from_slice(&out[..line])?;
    let number = |key: &str| facts.get(key).and_then(serde_json::Value::as_f64);
    let extent = facts
        .get("size")
        .and_then(serde_json::Value::as_array)
        .map(|axes| {
            let axis = |i: usize| {
                axes.get(i)
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.)
            };
            [axis(0), axis(1), axis(2)]
        })
        .unwrap_or_default();
    let measures = Measures {
        triangles: number("triangles").unwrap_or(0.) as u64,
        objects: number("objects").unwrap_or(0.) as u64,
        size: extent,
        unit: facts
            .get("unit")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        closed: facts
            .get("closed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        volume: number("volume"),
        faces: number("faces").unwrap_or(0.) as u64,
        faces_missing: number("faces_missing").unwrap_or(0.) as u64,
        objects_missing: number("objects_missing").unwrap_or(0.) as u64,
    };
    let rest = &out[line + 1..];
    let said = |key: &str| facts.get(key).and_then(serde_json::Value::as_bool) == Some(true);
    let shown = match whole {
        Some(most) if said("mesh") => Shown::Mesh(mesh_from(rest, most, said("colours"))?),
        _ => Shown::Picture(crate::inspection::image(rest)?),
    };
    Ok(Drawn { shown, measures })
}

/// The most bytes a mesh of `most` triangles takes as the helper writes it
/// (`mesh_from`): two counts, then a triangle's three positions (three
/// `f32` each, where no triangle shares one), its three indices, and the
/// three positions' colours (four bytes each).
fn mesh_bytes(most: usize) -> u64 {
    8 + most as u64 * (3 * 12 + 3 * 4 + 3 * 4)
}

/// The mesh the helper wrote (`--mesh`): two counts, then the positions
/// and the triangles, all little-endian, then when `coloured` a colour a
/// position (four bytes); no more than `most` triangles, each index naming
/// a position, each coordinate finite.
fn mesh_from(
    bytes: &[u8],
    most: usize,
    coloured: bool,
) -> anyhow::Result<skwir_core::inspection::Mesh> {
    let word = |at: usize| -> anyhow::Result<u32> {
        let four = bytes
            .get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("The model helper's mesh ends early"))?;
        Ok(u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
    };
    let (positions, triangles) = (word(0)? as usize, word(4)? as usize);
    anyhow::ensure!(
        triangles <= most && positions <= 3 * triangles,
        "The model helper wrote a mesh larger than asked"
    );
    let points = 8 + positions * 12;
    let corners = points + triangles * 12;
    let colours = if coloured { positions * 4 } else { 0 };
    anyhow::ensure!(
        bytes.len() == corners + colours,
        "The model helper's mesh is not as long as it says"
    );
    let positions_read = bytes[8..points]
        .as_chunks::<12>()
        .0
        .iter()
        .map(|p| {
            let at = |i: usize| f32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
            [at(0), at(4), at(8)]
        })
        .collect();
    let indices = bytes[points..corners]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|four| u32::from_le_bytes(*four))
        .collect();
    let mesh = skwir_core::inspection::Mesh::new(positions_read, indices)?;
    if coloured {
        mesh.with_colours(bytes[corners..].as_chunks::<4>().0.to_vec())
    } else {
        Ok(mesh)
    }
}

#[cfg(test)]
mod tests {
    /// A coloured mesh whose triangles share no position, the largest the
    /// helper writes, is within what is read of it: the bound left the
    /// colours out, and such a mesh failed instead of being shown.
    #[test]
    fn a_coloured_mesh_of_unshared_triangles_is_read_whole() {
        let triangles = 4;
        let positions = 3 * triangles;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&u32::try_from(positions).unwrap().to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(triangles).unwrap().to_le_bytes());
        for i in 0..positions {
            for axis in 0..3 {
                bytes.extend_from_slice(&((i * 3 + axis) as f32).to_le_bytes());
            }
        }
        for i in 0..positions {
            bytes.extend_from_slice(&u32::try_from(i).unwrap().to_le_bytes());
        }
        for _ in 0..positions {
            bytes.extend_from_slice(&[200, 30, 30, 255]);
        }
        assert_eq!(bytes.len() as u64, super::mesh_bytes(triangles));
        let mesh = super::mesh_from(&bytes, triangles, true).unwrap();
        assert_eq!(mesh.triangles(), triangles);
    }

    use super::*;

    fn model_naming(folder: &Path, uris: &[&str]) -> PathBuf {
        let path = folder.join("shape.gltf");
        let buffers: Vec<_> = uris
            .iter()
            .map(|uri| serde_json::json!({ "uri": uri }))
            .collect();
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({ "buffers": buffers })).unwrap(),
        )
        .unwrap();
        path
    }

    /// A GLB's headers and JSON. A nonempty binary chunk follows these
    /// bytes; tests can make that payload sparse instead of allocating it.
    fn glb_json(json: &[u8], binary: u32) -> Vec<u8> {
        let mut json = json.to_vec();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let json_len = u32::try_from(json.len()).unwrap();
        let total = 20 + json_len + if binary == 0 { 0 } else { 8 + binary };
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        glb.extend(total.to_le_bytes());
        glb.extend(json_len.to_le_bytes());
        glb.extend(b"JSON");
        glb.extend(json);
        if binary != 0 {
            glb.extend(binary.to_le_bytes());
            glb.extend(b"BIN\0");
        }
        glb
    }

    #[test]
    fn glb_discovers_external_buffers_without_reading_its_binary_payload() {
        use std::io::Write;

        let tmp = tempfile::tempdir().unwrap();
        let buffer = tmp.path().join("mesh data.bin");
        std::fs::write(&buffer, b"buffer").unwrap();
        let model = model_naming(tmp.path(), &["mesh%20data.bin"]);
        let json = std::fs::read(model).unwrap();
        let binary = u32::try_from(limits::TEXT_INPUT * 2).unwrap();
        let prefix = glb_json(&json, binary);
        let glb = tmp.path().join("shape.glb");
        let mut file = std::fs::File::create(&glb).unwrap();
        file.write_all(&prefix).unwrap();
        file.set_len(prefix.len() as u64 + u64::from(binary))
            .unwrap();
        assert_eq!(
            gltf_buffers(&glb),
            [(
                buffer.canonicalize().unwrap(),
                PathBuf::from("mesh data.bin")
            )]
        );
    }

    #[test]
    fn invalid_glb_headers_grant_no_buffer_access() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("mesh.bin"), b"buffer").unwrap();
        let model = model_naming(tmp.path(), &["mesh.bin"]);
        let original = glb_json(&std::fs::read(model).unwrap(), 0);
        let glb = tmp.path().join("shape.glb");
        for (offset, word) in [
            (4, 1u32),                           // unsupported version
            (8, 20),                             // length differs from the file
            (12, u32::MAX),                      // JSON length cannot fit
            (16, u32::from_le_bytes(*b"BIN\0")), // JSON must come first
        ] {
            let mut bytes = original.clone();
            bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
            std::fs::write(&glb, bytes).unwrap();
            assert!(gltf_buffers(&glb).is_empty(), "header word at {offset}");
        }
        std::fs::write(&glb, &original[..18]).unwrap();
        assert!(gltf_buffers(&glb).is_empty());
    }

    /// Exercise real namespace bindings and the actual helper when built.
    /// A platform-only build may have no model worker; say so when skipped.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn sandboxed_models_keep_their_input_separate_from_external_buffers() {
        let helper = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("skwir-model-worker")))
            .filter(|path| path.is_file());
        let Some(helper) = helper else {
            eprintln!("skipped: needs skwir-model-worker built beside the tests");
            return;
        };
        if !crate::sandbox::available() {
            eprintln!("skipped: needs the system sandbox");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let positions = [0f32, 0., 0., 2., 0., 0., 0., 3., 0.];
        let buffer: Vec<u8> = positions.into_iter().flat_map(f32::to_le_bytes).collect();
        for (extension, uri, relative) in [
            ("gltf", "document.gltf", "document.gltf"),
            ("gltf", "document%2Egltf", "document.gltf"),
            ("gltf", "document.gltf/mesh.bin", "document.gltf/mesh.bin"),
            ("gltf", "DOCUMENT.GLTF", "DOCUMENT.GLTF"),
            ("glb", "document.glb", "document.glb"),
            ("glb", "document%2Eglb", "document.glb"),
        ] {
            let fixture = tempfile::tempdir_in(tmp.path()).unwrap();
            let target = fixture.path().join(relative);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(&target, &buffer).unwrap();
            let json = serde_json::to_vec(&serde_json::json!({
                "asset": {"version": "2.0"},
                "buffers": [{"uri": uri, "byteLength": buffer.len()}],
                "bufferViews": [{"buffer": 0, "byteLength": buffer.len()}],
                "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3,
                    "type": "VEC3", "min": [0,0,0], "max": [2,3,0]}],
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
                "nodes": [{"mesh": 0}], "scenes": [{"nodes": [0]}], "scene": 0
            }))
            .unwrap();
            let bytes = if extension == "glb" {
                glb_json(&json, 0)
            } else {
                json
            };
            let model = fixture.path().join(format!("shape.{extension}"));
            std::fs::write(&model, bytes).unwrap();
            let drawn = draw_with(
                &helper,
                &model,
                MeshFormat::Gltf,
                32,
                Some(10),
                &CancelToken::new(),
            )
            .unwrap_or_else(|error| panic!("{extension} {uri}: {error:#}"));
            assert_eq!(drawn.measures.triangles, 1);
            assert!(
                drawn
                    .measures
                    .size
                    .into_iter()
                    .zip([2., 0., 3.])
                    .all(|(actual, expected)| (actual - expected).abs() < 1e-6)
            );
            assert!(matches!(drawn.shown, Shown::Mesh(_)));
        }
    }

    #[test]
    fn gltf_sandbox_binds_decoded_names_once() {
        let tmp = tempfile::tempdir().unwrap();
        let cases = [
            ("it%27s%20a%20mesh.bin", "it's a mesh.bin"),
            ("caf%C3%A9.bin", "café.bin"),
            ("caf%c3%a9.bin", "café.bin"),
            ("café.bin", "café.bin"),
            ("mesh+part.bin", "mesh+part.bin"),
            ("mesh%2Bpart.bin", "mesh+part.bin"),
            ("mesh%25part.bin", "mesh%part.bin"),
            ("mesh%2520part.bin", "mesh%20part.bin"),
            ("buffers%2Fmesh%20part.bin", "buffers/mesh part.bin"),
        ];
        std::fs::create_dir(tmp.path().join("buffers")).unwrap();
        for (uri, name) in cases {
            std::fs::write(tmp.path().join(name), b"decoded buffer").unwrap();
            // The encoded spelling is a different file, never a fallback.
            if uri != name {
                std::fs::write(tmp.path().join(uri), b"wrong buffer").unwrap();
            }
            let model = model_naming(tmp.path(), &[uri]);
            assert_eq!(
                gltf_buffers(&model),
                [(
                    tmp.path().join(name).canonicalize().unwrap(),
                    PathBuf::from(name)
                )],
                "{uri}"
            );
        }
    }

    #[test]
    fn gltf_sandbox_refuses_bad_encodings_and_decoded_escapes() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("model");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(tmp.path().join("outside.bin"), b"outside").unwrap();
        std::fs::write(folder.join("mesh.bin"), b"inside").unwrap();
        for uri in [
            "mesh%",
            "mesh%0",
            "mesh%GG",
            "mesh%+1",
            "mesh%FF",
            "mesh%C3%28",
            "mesh%00.bin",
            "%2e%2e/outside.bin",
            "nested/%2E%2E/mesh.bin",
            "%2Foutside.bin",
            "C%3A%5Cmesh.bin",
            "https%3A%2F%2Fexample.invalid%2Fmesh.bin",
        ] {
            // Even a file under the encoded spelling grants no access.
            let raw = folder.join(uri);
            std::fs::create_dir_all(raw.parent().unwrap()).unwrap();
            std::fs::write(raw, b"encoded spelling").unwrap();
            let model = model_naming(&folder, &[uri]);
            assert!(gltf_buffers(&model).is_empty(), "{uri}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn gltf_sandbox_inputs_keep_only_resolved_files_inside_the_model_folder() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("model");
        let outside = tmp.path().join("model sibling");
        std::fs::create_dir_all(folder.join("buffers")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(folder.join("buffers/mesh.bin"), b"inside").unwrap();
        std::fs::write(outside.join("mesh.bin"), b"outside").unwrap();
        symlink("buffers/mesh.bin", folder.join("inside.bin")).unwrap();
        symlink("buffers", folder.join("inside folder")).unwrap();
        symlink(outside.join("mesh.bin"), folder.join("outside.bin")).unwrap();
        symlink(&outside, folder.join("outside folder")).unwrap();
        symlink("missing.bin", folder.join("dangling.bin")).unwrap();

        let model = folder.join("shape.gltf");
        let uris = [
            "buffers/mesh.bin",
            "inside.bin",
            "inside folder/mesh.bin",
            "outside.bin",
            "outside folder/mesh.bin",
            "dangling.bin",
            "buffers",
            "../model sibling/mesh.bin",
            "https://example.invalid/mesh.bin",
            "outside%2Ebin",
            "outside%20folder%2Fmesh.bin",
            "dangling%2Ebin",
        ];
        let buffers: Vec<_> = uris
            .iter()
            .map(|uri| serde_json::json!({ "uri": uri }))
            .collect();
        std::fs::write(
            &model,
            serde_json::to_vec(&serde_json::json!({ "buffers": buffers })).unwrap(),
        )
        .unwrap();
        let resolved = folder.join("buffers/mesh.bin").canonicalize().unwrap();
        let expected: Vec<_> = uris[..3]
            .iter()
            .map(|uri| (resolved.clone(), PathBuf::from(uri)))
            .collect();
        assert_eq!(gltf_buffers(&model), expected);

        // Seatbelt reaches the model through a staging link. Its buffers
        // still belong to the actual model's folder, not the staging one.
        let staging = tmp.path().join("staging");
        std::fs::create_dir(&staging).unwrap();
        let alias = staging.join("input.gltf");
        symlink(&model, &alias).unwrap();
        assert_eq!(gltf_buffers(&alias), expected);

        // Decoding precedes link containment and preserves the decoded alias.
        let model = model_naming(&folder, &["inside%20folder/mesh.bin"]);
        assert_eq!(
            gltf_buffers(&model),
            [(resolved, PathBuf::from("inside folder/mesh.bin"))]
        );
    }

    /// A mesh as `--mesh` writes it.
    fn written(positions: &[[f32; 3]], triangles: &[[u32; 3]]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(u32::try_from(positions.len()).unwrap().to_le_bytes());
        out.extend(u32::try_from(triangles.len()).unwrap().to_le_bytes());
        out.extend(positions.iter().flatten().flat_map(|v| v.to_le_bytes()));
        out.extend(triangles.iter().flatten().flat_map(|i| i.to_le_bytes()));
        out
    }

    #[test]
    fn a_mesh_is_taken_only_as_long_and_as_large_as_it_says() {
        let square = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
        let faces = [[0, 1, 2], [0, 2, 3]];
        let mesh = mesh_from(&written(&square, &faces), 2, false).unwrap();
        assert_eq!(mesh.positions(), square);
        assert_eq!(mesh.indices(), [0, 1, 2, 0, 2, 3]);
        let error = |bytes: &[u8], most| mesh_from(bytes, most, false).unwrap_err().to_string();
        assert!(error(&written(&square, &faces), 1).contains("larger than asked"));
        let mut long = written(&square, &faces);
        long.push(0);
        assert!(error(&long, 2).contains("not as long"));
        assert!(error(&written(&square, &faces)[..30], 2).contains("not as long"));
        assert!(error(&[1, 0], 2).contains("ends early"));
        // More positions than three a triangle, or an index past them.
        assert!(error(&written(&square, &[[0, 1, 2]]), 2).contains("larger than asked"));
        assert!(error(&written(&square[..3], &[[0, 1, 3]]), 2).contains("names no position"));
        let broken = [[0., 0., 0.], [f32::INFINITY, 0., 0.], [0., 1., 0.]];
        assert!(error(&written(&broken, &[[0, 1, 2]]), 2).contains("not finite"));
        // Coloured: a colour a position after the triangles, no more.
        let mut coloured = written(&square, &faces);
        coloured.extend(
            [
                [255u8, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [9, 9, 9, 9],
            ]
            .iter()
            .flatten(),
        );
        let mesh = mesh_from(&coloured, 2, true).unwrap();
        assert_eq!(mesh.colours().unwrap()[1], [0, 255, 0, 255]);
        coloured.truncate(coloured.len() - 4);
        assert!(
            mesh_from(&coloured, 2, true)
                .unwrap_err()
                .to_string()
                .contains("not as long")
        );
    }
}
