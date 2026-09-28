//! glTF and GLB: the default scene's nodes, each mesh placed by the
//! transforms above it, as it stands before any animation. Buffers come
//! from the GLB's own chunk, from `data:` URIs, or from a file beside the
//! model that the model names by a relative path; nothing else is read.
use crate::mesh::{IDENTITY, Mesh, Point, Transform, compose};
use base64::Engine as _;
use std::path::Path;

/// Nodes nested deeper than this are not followed (a cycle is invalid glTF
/// but costs nothing to refuse).
const DEPTH: usize = 64;

/// A buffer the model names by URI.
fn buffer(uri: &str, dir: &Path, most: u64) -> Result<Vec<u8>, String> {
    if let Some(data) = uri.strip_prefix("data:") {
        let (_, encoded) = data
            .split_once(";base64,")
            .ok_or("a buffer's data URI is not base64")?;
        return base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| format!("a buffer's data URI: {e}"));
    }
    // A file beside the model, by a relative path that stays below it.
    let relative = Path::new(uri);
    let plain = relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    if uri.contains(':') || !plain {
        return Err(format!("the buffer {uri} is not a file beside the model"));
    }
    crate::mesh::read_bounded(&dir.join(relative), most)
}

pub fn read(bytes: &[u8], path: &Path, most_bytes: u64, mesh: &mut Mesh) -> Result<(), String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("not a glTF model: {e}"))?;
    for required in gltf.document.extensions_required() {
        if required.contains("draco") || required.contains("meshopt") {
            return Err(format!("meshes compressed with {required} are not read"));
        }
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut buffers = Vec::new();
    let mut total = 0u64;
    for buffer_def in gltf.document.buffers() {
        let data = match buffer_def.source() {
            gltf::buffer::Source::Bin => gltf.blob.clone().ok_or("the GLB has no binary chunk")?,
            gltf::buffer::Source::Uri(uri) => buffer(uri, dir, most_bytes)?,
        };
        total += data.len() as u64;
        if total > most_bytes {
            return Err(format!(
                "the model's buffers are larger than {most_bytes} bytes"
            ));
        }
        buffers.push(data);
    }
    let scene = gltf
        .document
        .default_scene()
        .or_else(|| gltf.document.scenes().next());
    let mut objects = 0;
    match scene {
        Some(scene) => {
            for node in scene.nodes() {
                walk(&node, &IDENTITY, &buffers, mesh, &mut objects, 0)?;
            }
        }
        // No scene: every mesh as it is.
        None => {
            for gltf_mesh in gltf.document.meshes() {
                add_mesh(&gltf_mesh, &IDENTITY, &buffers, mesh)?;
                objects += 1;
            }
        }
    }
    mesh.objects = objects;
    mesh.unit = Some("m");
    mesh.y_up();
    Ok(())
}

fn walk(
    node: &gltf::Node<'_>,
    parent: &Transform,
    buffers: &[Vec<u8>],
    mesh: &mut Mesh,
    objects: &mut usize,
    depth: usize,
) -> Result<(), String> {
    if depth > DEPTH {
        return Ok(());
    }
    let m = node.transform().matrix();
    let local: Transform = [
        [m[0][0], m[0][1], m[0][2]],
        [m[1][0], m[1][1], m[1][2]],
        [m[2][0], m[2][1], m[2][2]],
        [m[3][0], m[3][1], m[3][2]],
    ];
    let placed = compose(&local, parent);
    if let Some(gltf_mesh) = node.mesh() {
        add_mesh(&gltf_mesh, &placed, buffers, mesh)?;
        *objects += 1;
    }
    for child in node.children() {
        walk(&child, &placed, buffers, mesh, objects, depth + 1)?;
    }
    Ok(())
}

fn add_mesh(
    gltf_mesh: &gltf::Mesh<'_>,
    transform: &Transform,
    buffers: &[Vec<u8>],
    mesh: &mut Mesh,
) -> Result<(), String> {
    for primitive in gltf_mesh.primitives() {
        let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
        let Some(positions) = reader.read_positions() else {
            continue;
        };
        if positions.len() > mesh.room() {
            return Err(format!("the model has more than {} triangles", mesh.most()));
        }
        let points: Vec<Point> = positions.collect();
        let indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..u32::try_from(points.len()).unwrap_or(u32::MAX)).collect(),
        };
        let triangles: Vec<[u32; 3]> = match primitive.mode() {
            gltf::mesh::Mode::Triangles => indices.as_chunks::<3>().0.to_vec(),
            gltf::mesh::Mode::TriangleStrip => indices
                .windows(3)
                .enumerate()
                .map(|(i, t)| {
                    if i % 2 == 0 {
                        [t[0], t[1], t[2]]
                    } else {
                        [t[1], t[0], t[2]]
                    }
                })
                .collect(),
            gltf::mesh::Mode::TriangleFan => indices
                .iter()
                .skip(1)
                .zip(indices.iter().skip(2))
                .map(|(b, c)| [indices[0], *b, *c])
                .collect(),
            // Points and lines have no surface to draw.
            _ => continue,
        };
        mesh.add(&points, triangles, transform)?;
    }
    Ok(())
}
