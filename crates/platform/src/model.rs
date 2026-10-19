//! 3D meshes, read and drawn by `skwir-model-worker` in a process of its
//! own: a model can hold millions of triangles, whose memory the helper
//! gives back when it exits, and a reader that fails takes nothing with
//! it. What comes back is what the mesh measures and its picture, or, for a
//! window that draws it (ADR-067), the mesh itself.
use std::path::{Path, PathBuf};

use skwir_core::{CancelToken, thumbs::Image};

use crate::inspection::limits;

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
    // that answer in text may say. Or the mesh: two counts, three
    // positions and a triangle's three indices a triangle at most.
    let picture = u64::from(side) * u64::from(side) * 4 + (1 << 16);
    let mesh = whole.map_or(0, |most| 8 + most as u64 * (3 * 12 + 12) + (1 << 16));
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
    };
    let rest = &out[line + 1..];
    let shown = match whole {
        Some(most) if facts.get("mesh").and_then(serde_json::Value::as_bool) == Some(true) => {
            Shown::Mesh(mesh_from(rest, most)?)
        }
        _ => Shown::Picture(crate::inspection::image(rest)?),
    };
    Ok(Drawn { shown, measures })
}

/// The mesh the helper wrote (`--mesh`): two counts, then the positions
/// and the triangles, all little-endian, with no more than `most`
/// triangles, each index naming a position, each coordinate finite.
fn mesh_from(bytes: &[u8], most: usize) -> anyhow::Result<skwir_core::inspection::Mesh> {
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
    anyhow::ensure!(
        bytes.len() == points + triangles * 12,
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
    let indices = bytes[points..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|four| u32::from_le_bytes(*four))
        .collect();
    skwir_core::inspection::Mesh::new(positions_read, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mesh = mesh_from(&written(&square, &faces), 2).unwrap();
        assert_eq!(mesh.positions(), square);
        assert_eq!(mesh.indices(), [0, 1, 2, 0, 2, 3]);
        let error = |bytes: &[u8], most| mesh_from(bytes, most).unwrap_err().to_string();
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
    }
}
