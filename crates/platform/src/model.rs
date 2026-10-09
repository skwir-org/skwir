//! 3D meshes, read and drawn by `bibli-model-worker` in a process of its
//! own: a model can hold millions of triangles, whose memory the helper
//! gives back when it exits, and a reader that fails takes nothing with
//! it. What comes back is what the mesh measures and its picture.
use std::path::{Path, PathBuf};

use bibli_core::{CancelToken, thumbs::Image};

use crate::inspection::limits;

/// Triangles drawn at most: a detailed print is a few million.
pub const TRIANGLES: usize = 8_000_000;
/// A model file read at most, and what a package's models unpack to.
pub const BYTES: u64 = 512 * 1024 * 1024;

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
}

/// A mesh's picture and measures.
pub struct Drawn {
    pub picture: Image,
    pub measures: Measures,
}

/// The helper's executable: `BIBLI_MODEL_WORKER`, beside our binary, or
/// on `PATH`, if it is there. Plugins that require it name this, not the
/// literals; whether a model is read at all depends on it.
pub fn worker_path() -> Option<PathBuf> {
    WORKER.find()
}

/// The helper. Plugins that require it name this, not the literals.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "BIBLI_MODEL_WORKER",
    executable: "bibli-model-worker",
};

/// The files a glTF model at `path` keeps its buffers in, each with its
/// path from the model's folder: those its JSON names by a plain relative
/// path below that folder, as the helper reads them (`gltf_mesh::buffer`),
/// that are files. Read here only to say what the sandbox lets it read.
fn gltf_buffers(path: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(text) = crate::file::read_regular(path, limits::TEXT_INPUT as u64) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    json.get("buffers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|buffer| buffer.get("uri")?.as_str())
        .filter(|uri| !uri.contains(':'))
        .map(Path::new)
        .filter(|relative| {
            relative
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        })
        .filter(|relative| std::fs::metadata(dir.join(relative)).is_ok_and(|m| m.is_file()))
        .map(|relative| (dir.join(relative), relative.to_owned()))
        .collect()
}

/// The mesh at `path` drawn at most `side` pixels on its longer side, and
/// measured. An error when the helper is missing, or says why it cannot.
pub fn draw(
    path: &Path,
    format: MeshFormat,
    side: u32,
    cancel: &CancelToken,
) -> anyhow::Result<Drawn> {
    let worker = WORKER.require("draw 3D models")?;
    let work = tempfile::tempdir()?;
    // A glTF's buffers may be files beside it: the helper reads them where
    // it reads the model.
    let beside = if format == MeshFormat::Gltf {
        gltf_buffers(path)
    } else {
        Vec::new()
    };
    let mut run = crate::sandbox::untrusted_beside(
        &worker,
        path,
        &beside,
        work.path(),
        crate::sandbox::Otherwise::Run,
        &[],
    )?;
    let finished = crate::process::run_to_exit(
        run.command
            .arg(&run.input)
            .args(["--format", format.name()])
            .args(["--side", &side.clamp(16, 2048).to_string()])
            .args(["--triangles", &TRIANGLES.to_string()])
            .args(["--bytes", &BYTES.to_string()]),
        None,
        cancel,
        limits::HELPER_TIMEOUT,
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
    };
    let picture = crate::inspection::image(&out[line + 1..])?;
    Ok(Drawn { picture, measures })
}
