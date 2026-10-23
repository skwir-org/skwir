//! The helper as skwir runs it: a file and its format in, a line of
//! measures and a PNG out, or a reason and an exit status of 1.
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
struct Answer {
    facts: serde_json::Value,
    /// The picture's width and height, from its PNG header.
    size: (u32, u32),
}

fn run(path: &Path, format: &str, extra: &[&str]) -> Result<Answer, String> {
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-model-worker"))
        .arg(path)
        .args(["--format", format, "--side", "256"])
        .args(extra)
        .output()
        .unwrap();
    if !output.status.success() {
        assert!(output.stdout.is_empty(), "nothing but a reason");
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let line = output.stdout.iter().position(|b| *b == b'\n').unwrap();
    let facts = serde_json::from_slice(&output.stdout[..line]).unwrap();
    let png = &output.stdout[line + 1..];
    assert_eq!(&png[1..4], b"PNG");
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    Ok(Answer {
        facts,
        size: (width, height),
    })
}

/// The corners and faces of a box `x` × `y` × `z`, wound outward.
fn cuboid(x: f32, y: f32, z: f32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let points = vec![
        [0., 0., 0.],
        [x, 0., 0.],
        [x, y, 0.],
        [0., y, 0.],
        [0., 0., z],
        [x, 0., z],
        [x, y, z],
        [0., y, z],
    ];
    let faces = vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    (points, faces)
}

fn binary_stl(points: &[[f32; 3]], faces: &[[u32; 3]]) -> Vec<u8> {
    let mut out = vec![b' '; 80];
    out.extend(u32::try_from(faces.len()).unwrap().to_le_bytes());
    for face in faces {
        out.extend([0u8; 12]);
        for &i in face {
            for v in points[i as usize] {
                out.extend(v.to_le_bytes());
            }
        }
        out.extend([0, 0]);
    }
    out
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn size(answer: &Answer) -> Vec<f64> {
    answer.facts["size"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v.as_f64().unwrap() * 1000.).round() / 1000.)
        .collect()
}

#[test]
fn an_stl_box_is_measured_and_drawn() {
    let dir = tempfile::tempdir().unwrap();
    let (points, faces) = cuboid(20., 10., 5.);
    let path = write(dir.path(), "a part’s box.stl", &binary_stl(&points, &faces));
    let answer = run(&path, "stl", &[]).unwrap();
    assert_eq!(answer.facts["triangles"], 12);
    assert_eq!(size(&answer), [20., 10., 5.]);
    assert_eq!(answer.facts["closed"], true);
    assert!((answer.facts["volume"].as_f64().unwrap() - 1000.).abs() < 1e-3);
    assert_eq!(answer.facts["unit"], serde_json::Value::Null);
    let (w, h) = answer.size;
    assert!(w.max(h) == 256 && w > 0 && h > 0, "{w} × {h}");
    // Open a face: it holds nothing.
    let path = write(dir.path(), "open.stl", &binary_stl(&points, &faces[1..]));
    let answer = run(&path, "stl", &[]).unwrap();
    assert_eq!(answer.facts["closed"], false);
    assert_eq!(answer.facts["volume"], serde_json::Value::Null);
}

#[test]
fn text_formats_obj_ply_and_off() {
    let dir = tempfile::tempdir().unwrap();
    // OBJ stands on Y: its height is Y, its depth Z.
    let obj = "o box\nv 0 0 0\nv 4 0 0\nv 4 3 0\nv 0 3 0\nv 0 0 2\nv 4 0 2\nv 4 3 2\nv 0 3 2\n\
               f 1 3 2\nf 1 4 3\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\nmtllib ../../outside.mtl\n";
    let answer = run(&write(dir.path(), "box.obj", obj.as_bytes()), "obj", &[]).unwrap();
    assert_eq!(size(&answer), [4., 2., 3.]);
    assert_eq!(answer.facts["triangles"], 12);
    assert_eq!(answer.facts["closed"], true);
    let (points, faces) = cuboid(2., 2., 2.);
    let mut ply = String::from(
        "ply\nformat ascii 1.0\ncomment made by hand\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nelement face 12\nproperty list uchar int vertex_indices\nend_header\n",
    );
    for p in &points {
        let _ = writeln!(ply, "{} {} {} 255", p[0], p[1], p[2]);
    }
    for f in &faces {
        let _ = writeln!(ply, "3 {} {} {}", f[0], f[1], f[2]);
    }
    let answer = run(&write(dir.path(), "box.ply", ply.as_bytes()), "ply", &[]).unwrap();
    assert!((answer.facts["volume"].as_f64().unwrap() - 8.).abs() < 1e-3);
    // The same as binary, big-endian, with a double coordinate.
    let mut binary = b"ply\nformat binary_big_endian 1.0\nelement vertex 8\nproperty double x\nproperty float y\nproperty float z\nelement face 12\nproperty list uchar uint vertex_indices\nend_header\n".to_vec();
    for p in &points {
        binary.extend(f64::from(p[0]).to_be_bytes());
        binary.extend(p[1].to_be_bytes());
        binary.extend(p[2].to_be_bytes());
    }
    for f in &faces {
        binary.push(3);
        for i in f {
            binary.extend(i.to_be_bytes());
        }
    }
    let answer = run(&write(dir.path(), "binary.ply", &binary), "ply", &[]).unwrap();
    assert_eq!(size(&answer), [2., 2., 2.]);
    assert_eq!(answer.facts["closed"], true);
    let mut off = String::from("OFF\n# a box\n8 6 0\n");
    for p in &points {
        let _ = writeln!(off, "{} {} {}", p[0], p[1], p[2]);
    }
    // Quads, with a colour after each.
    for q in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        let _ = writeln!(off, "4 {} {} {} {} 255 0 0", q[0], q[1], q[2], q[3]);
    }
    let answer = run(&write(dir.path(), "box.off", off.as_bytes()), "off", &[]).unwrap();
    assert_eq!(answer.facts["triangles"], 12);
    assert_eq!(answer.facts["closed"], true);
}

/// A glTF of one box mesh placed twice by nodes, its buffer as `uri`.
fn gltf_json(uri: &str, length: usize) -> String {
    format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0,1]}}],
"nodes":[{{"mesh":0}},{{"mesh":0,"translation":[0,0,-10]}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],
"buffers":[{{"uri":"{uri}","byteLength":{length}}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":96}},{{"buffer":0,"byteOffset":96,"byteLength":144}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":8,"type":"VEC3","min":[0,0,0],"max":[1,2,3]}},
{{"bufferView":1,"componentType":5125,"count":36,"type":"SCALAR"}}]}}"#
    )
}

fn gltf_buffer() -> Vec<u8> {
    let (points, faces) = cuboid(1., 2., 3.);
    let mut buffer = Vec::new();
    for p in points {
        for v in p {
            buffer.extend(v.to_le_bytes());
        }
    }
    for f in faces {
        for i in f {
            buffer.extend(i.to_le_bytes());
        }
    }
    buffer
}

#[test]
fn gltf_places_its_nodes_and_reads_only_its_own_buffers() {
    use base64::Engine as _;
    let dir = tempfile::tempdir().unwrap();
    let buffer = gltf_buffer();
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&buffer)
    );
    let path = write(
        dir.path(),
        "twice.gltf",
        gltf_json(&uri, buffer.len()).as_bytes(),
    );
    let answer = run(&path, "gltf", &[]).unwrap();
    // Y up: glTF's X, Z and Y are width, depth and height; the second box
    // stands 10 behind the first.
    assert_eq!(size(&answer), [1., 13., 2.]);
    assert_eq!(answer.facts["objects"], 2);
    assert_eq!(answer.facts["triangles"], 24);
    assert_eq!(answer.facts["unit"], "m");
    // A buffer beside the model is read; one outside it is not.
    std::fs::write(dir.path().join("box.bin"), &buffer).unwrap();
    let path = write(
        dir.path(),
        "beside.gltf",
        gltf_json("box.bin", buffer.len()).as_bytes(),
    );
    assert!(run(&path, "gltf", &[]).is_ok());
    let path = write(
        dir.path(),
        "outside.gltf",
        gltf_json("../box.bin", buffer.len()).as_bytes(),
    );
    let reason = run(&path, "gltf", &[]).unwrap_err();
    assert!(reason.contains("not a file beside the model"), "{reason}");
    // And as GLB: a header, the JSON chunk, the binary chunk.
    let mut json = gltf_json("", buffer.len()).replace(r#""uri":"","#, "");
    while !json.len().is_multiple_of(4) {
        json.push(' ');
    }
    let mut glb = b"glTF".to_vec();
    glb.extend(2u32.to_le_bytes());
    let total = 12 + 8 + json.len() + 8 + buffer.len();
    glb.extend(u32::try_from(total).unwrap().to_le_bytes());
    glb.extend(u32::try_from(json.len()).unwrap().to_le_bytes());
    glb.extend(b"JSON");
    glb.extend(json.as_bytes());
    glb.extend(u32::try_from(buffer.len()).unwrap().to_le_bytes());
    glb.extend(b"BIN\0");
    glb.extend(&buffer);
    let answer = run(&write(dir.path(), "twice.glb", &glb), "gltf", &[]).unwrap();
    assert_eq!(answer.facts["triangles"], 24);
}

#[cfg(unix)]
#[test]
fn gltf_buffer_links_must_resolve_inside_the_models_actual_folder() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path().join("model");
    let outside = tmp.path().join("model sibling");
    std::fs::create_dir_all(folder.join("buffers")).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let buffer = gltf_buffer();
    std::fs::write(folder.join("buffers/mesh.bin"), &buffer).unwrap();
    std::fs::write(outside.join("mesh.bin"), &buffer).unwrap();
    symlink("buffers/mesh.bin", folder.join("inside.bin")).unwrap();
    symlink("buffers", folder.join("inside folder")).unwrap();
    symlink(outside.join("mesh.bin"), folder.join("outside.bin")).unwrap();
    symlink(&outside, folder.join("outside folder")).unwrap();
    symlink("missing.bin", folder.join("dangling.bin")).unwrap();
    for uri in ["outside.bin", "outside folder/mesh.bin", "dangling.bin"] {
        let model = write(
            &folder,
            "shape.gltf",
            gltf_json(uri, buffer.len()).as_bytes(),
        );
        let reason = run(&model, "gltf", &[]).unwrap_err();
        assert!(
            reason.contains("not a file beside the model"),
            "{uri}: {reason}"
        );
    }
    for uri in ["inside.bin", "inside folder/mesh.bin"] {
        let model = write(
            &folder,
            "shape.gltf",
            gltf_json(uri, buffer.len()).as_bytes(),
        );
        assert_eq!(run(&model, "gltf", &[]).unwrap().facts["triangles"], 24);
    }

    // The macOS sandbox supplies aliases for both the model and buffers.
    // Read through that layout too: the actual model is the containment root.
    let staging = tmp.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    let model = write(
        &folder,
        "shape.gltf",
        gltf_json("inside.bin", buffer.len()).as_bytes(),
    );
    let alias = staging.join("input.gltf");
    symlink(&model, &alias).unwrap();
    symlink(folder.join("buffers/mesh.bin"), staging.join("inside.bin")).unwrap();
    assert_eq!(run(&alias, "gltf", &[]).unwrap().facts["triangles"], 24);
}

#[test]
fn a_3mf_builds_its_items_from_components_in_other_parts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a bracket’s plate.3mf");
    let (points, faces) = cuboid(10., 10., 10.);
    let mut vertices = String::new();
    for p in &points {
        let _ = write!(
            vertices,
            r#"<vertex x="{}" y="{}" z="{}"/>"#,
            p[0], p[1], p[2]
        );
    }
    let mut triangles = String::new();
    for f in &faces {
        let _ = write!(
            triangles,
            r#"<triangle v1="{}" v2="{}" v3="{}"/>"#,
            f[0], f[1], f[2]
        );
    }
    let part = format!(
        r#"<?xml version="1.0"?><model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><object id="1" type="model"><mesh><vertices>{vertices}</vertices><triangles>{triangles}</triangles></mesh></object></resources></model>"#
    );
    // Bambu Studio's way: the root's object is components in another part,
    // placed twice by the build, once moved 30 along X.
    let root = r#"<?xml version="1.0"?><model unit="inch" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:p="http://schemas.microsoft.com/3dmanufacturing/production/2015/06"><resources><object id="2" type="model"><components><component p:path="/3D/Objects/object_1.model" objectid="1" transform="1 0 0 0 1 0 0 0 2 0 0 0"/></components></object></resources><build><item objectid="2"/><item objectid="2" transform="1 0 0 0 1 0 0 0 1 30 0 0"/></build></model>"#;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, bytes) in [
        ("3D/3dmodel.model", root.as_bytes()),
        ("3D/Objects/object_1.model", part.as_bytes()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    let answer = run(&path, "3mf", &[]).unwrap();
    assert_eq!(size(&answer), [40., 10., 20.]);
    assert_eq!(answer.facts["objects"], 2);
    assert_eq!(answer.facts["triangles"], 24);
    assert_eq!(answer.facts["unit"], "in");
    assert!((answer.facts["volume"].as_f64().unwrap() - 4000.).abs() < 1e-2);
}

#[test]
fn what_is_too_large_or_not_a_model_is_refused_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let (points, faces) = cuboid(1., 1., 1.);
    let path = write(dir.path(), "box.stl", &binary_stl(&points, &faces));
    let reason = run(&path, "stl", &["--triangles", "11"]).unwrap_err();
    assert!(reason.contains("more than 11 triangles"), "{reason}");
    let reason = run(&path, "stl", &["--bytes", "100"]).unwrap_err();
    assert!(reason.contains("larger than 100 bytes"), "{reason}");
    let path = write(dir.path(), "notes.obj", b"just some words\n");
    assert!(run(&path, "obj", &[]).unwrap_err().contains("no triangles"));
    assert!(run(&path, "fbx", &[]).is_err());
    let path = write(
        dir.path(),
        "bad.ply",
        b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nend_header\n1\n",
    );
    assert!(run(&path, "ply", &[]).unwrap_err().contains("ends early"));
}

/// What `--mesh` wrote after its line: the positions and the triangles.
fn mesh_after(stdout: &[u8]) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let word = |at: usize| u32::from_le_bytes(stdout[at..at + 4].try_into().unwrap());
    let (positions, triangles) = (word(0) as usize, word(4) as usize);
    assert_eq!(
        stdout.len(),
        8 + positions * 12 + triangles * 12,
        "all, nothing more"
    );
    let float = |at: usize| f32::from_le_bytes(stdout[at..at + 4].try_into().unwrap());
    let points = (0..positions)
        .map(|p| [0, 1, 2].map(|axis| float(8 + p * 12 + axis * 4)))
        .collect();
    let faces = (0..triangles)
        .map(|t| [0, 1, 2].map(|corner| word(8 + positions * 12 + t * 12 + corner * 4)))
        .collect();
    (points, faces)
}

#[test]
fn a_mesh_asked_for_is_written_whole_up_to_the_triangles_asked() {
    let dir = tempfile::tempdir().unwrap();
    let (points, faces) = cuboid(20., 10., 5.);
    let path = write(dir.path(), "a part’s box.stl", &binary_stl(&points, &faces));
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-model-worker"))
        .arg(&path)
        .args(["--format", "stl", "--side", "256", "--mesh", "12"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let line = output.stdout.iter().position(|b| *b == b'\n').unwrap();
    let said: serde_json::Value = serde_json::from_slice(&output.stdout[..line]).unwrap();
    assert_eq!(said["mesh"], true);
    assert_eq!(said["triangles"], 12);
    assert_eq!(said["closed"], true);
    let (positions, triangles) = mesh_after(&output.stdout[line + 1..]);
    // The box's eight corners, each once, and its twelve faces between them.
    assert_eq!(positions.len(), 8);
    assert_eq!(triangles.len(), 12);
    assert!(
        triangles
            .iter()
            .flatten()
            .all(|&i| (i as usize) < positions.len())
    );
    let mut corners: Vec<[f32; 3]> = positions.clone();
    corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut expected = points.clone();
    expected.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(corners, expected);
    // Past the triangles asked, it is drawn.
    let answer = run(&path, "stl", &["--mesh", "11"]).unwrap();
    assert_eq!(answer.facts["mesh"], false);
    assert_eq!(answer.size.0.max(answer.size.1), 256);
    // A point no face uses is not written.
    let obj = "v 0 0 0\nv 9 9 9\nv 1 0 0\nv 0 1 0\nf 1 3 4\n";
    let path = write(dir.path(), "stray.obj", obj.as_bytes());
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-model-worker"))
        .arg(&path)
        .args(["--format", "obj", "--mesh", "1"])
        .output()
        .unwrap();
    let line = output.stdout.iter().position(|b| *b == b'\n').unwrap();
    let (positions, triangles) = mesh_after(&output.stdout[line + 1..]);
    assert_eq!(triangles, [[0, 1, 2]]);
    // OBJ stands on Y: turned to stand on Z.
    assert_eq!(positions, [[0., 0., 0.], [1., 0., 0.], [0., 0., 1.]]);
}

/// A cube of side `n`, each face cut into `n` × `n` squares of two
/// triangles, wound outward: its corners are whole numbers, so the same
/// corner is the same bits wherever it is written.
fn fine_cube(n: u32) -> Vec<[[f32; 3]; 3]> {
    let mut faces = Vec::new();
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for side in [0, n] {
            for i in 0..n {
                for j in 0..n {
                    let p = |a: u32, b: u32| {
                        let mut point = [0.; 3];
                        point[axis] = side as f32;
                        point[u] = a as f32;
                        point[v] = b as f32;
                        point
                    };
                    let quad = [p(i, j), p(i + 1, j), p(i + 1, j + 1), p(i, j + 1)];
                    let mut two = [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]];
                    if side == 0 {
                        for t in &mut two {
                            t.swap(1, 2);
                        }
                    }
                    faces.extend(two);
                }
            }
        }
    }
    faces
}

/// Past 100,000 triangles a binary STL's corners are made one on several
/// threads: the same points, each triangle's corners where they were.
#[test]
fn a_large_stl_is_welded_on_threads_as_in_one() {
    let n = 110;
    let faces = fine_cube(n);
    let mut stl = vec![b' '; 80];
    stl.extend(u32::try_from(faces.len()).unwrap().to_le_bytes());
    for face in &faces {
        stl.extend([0u8; 12]);
        stl.extend(face.iter().flatten().flat_map(|v| v.to_le_bytes()));
        stl.extend([0, 0]);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "fine cube.stl", &stl);
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-model-worker"))
        .arg(&path)
        .args(["--format", "stl", "--mesh", "1000000"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let line = output.stdout.iter().position(|b| *b == b'\n').unwrap();
    let said: serde_json::Value = serde_json::from_slice(&output.stdout[..line]).unwrap();
    assert_eq!(said["closed"], true);
    let volume = f64::from(n).powi(3);
    assert!((said["volume"].as_f64().unwrap() - volume).abs() < volume * 1e-9);
    let (positions, triangles) = mesh_after(&output.stdout[line + 1..]);
    // Six faces of n × n squares share their edges and corners.
    assert_eq!(positions.len() as u32, 6 * n * n + 2);
    assert_eq!(triangles.len(), faces.len());
    for (read, written) in triangles.iter().zip(&faces) {
        assert_eq!(read.map(|i| positions[i as usize]), *written);
    }
}

/// Some exporters begin a binary STL's header `solid`, as an ASCII one
/// begins: as long as a binary file of its triangles, it is binary.
#[test]
fn a_binary_stl_that_begins_solid_is_binary() {
    let dir = tempfile::tempdir().unwrap();
    let (points, faces) = cuboid(2., 3., 4.);
    let mut stl = binary_stl(&points, &faces);
    stl[..12].copy_from_slice(b"solid Export");
    let path = write(dir.path(), "exported.stl", &stl);
    let answer = run(&path, "stl", &[]).unwrap();
    assert_eq!(answer.facts["triangles"], 12);
    assert_eq!(size(&answer), [2., 3., 4.]);
}

/// The helper's answer to `--mesh`: what it says, its positions,
/// triangles and colours (none when it says it has none).
fn whole(
    path: &Path,
    format: &str,
) -> (
    serde_json::Value,
    Vec<[f32; 3]>,
    Vec<[u32; 3]>,
    Vec<[u8; 4]>,
) {
    whole_up_to(path, format, 1000)
}

/// As `whole`, for a mesh of up to `most` triangles.
fn whole_up_to(
    path: &Path,
    format: &str,
    most: usize,
) -> (
    serde_json::Value,
    Vec<[f32; 3]>,
    Vec<[u32; 3]>,
    Vec<[u8; 4]>,
) {
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-model-worker"))
        .arg(path)
        .args(["--format", format, "--mesh", &most.to_string()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = output.stdout.iter().position(|b| *b == b'\n').unwrap();
    let said: serde_json::Value = serde_json::from_slice(&output.stdout[..line]).unwrap();
    let rest = &output.stdout[line + 1..];
    let word = |at: usize| u32::from_le_bytes(rest[at..at + 4].try_into().unwrap()) as usize;
    let mesh_end = 8 + word(0) * 12 + word(4) * 12;
    let colours: Vec<[u8; 4]> = rest[mesh_end..].as_chunks::<4>().0.to_vec();
    assert_eq!(said["colours"], !colours.is_empty());
    let (positions, triangles) = mesh_after(&rest[..mesh_end]);
    if !colours.is_empty() {
        assert_eq!(colours.len(), positions.len(), "a colour a position");
    }
    (said, positions, triangles, colours)
}

/// Each triangle's corners' colours, in its order.
fn corner_colours(triangles: &[[u32; 3]], colours: &[[u8; 4]]) -> Vec<[[u8; 4]; 3]> {
    triangles
        .iter()
        .map(|t| t.map(|i| colours[i as usize]))
        .collect()
}

#[test]
fn colours_are_read_from_ply_obj_gltf_and_3mf() {
    let dir = tempfile::tempdir().unwrap();
    // PLY: a point's red, green and blue bytes.
    let ply = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 255 0 0\n1 0 0 0 255 0\n0 1 0 0 0 255\n3 0 1 2\n";
    let (_, _, triangles, colours) = whole(&write(dir.path(), "scan.ply", ply.as_bytes()), "ply");
    assert_eq!(
        corner_colours(&triangles, &colours),
        [[[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]]
    );
    // Without colours, none.
    let plain = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";
    let (said, ..) = whole(&write(dir.path(), "plain.ply", plain.as_bytes()), "ply");
    assert_eq!(said["colours"], false);
    // OBJ: MeshLab's `v x y z r g b`, fractions of one.
    let obj = "v 0 0 0 1 0 0\nv 1 0 0 1 0 0\nv 0 1 0 0 0.5 1\nf 1 2 3\n";
    let (_, _, triangles, colours) =
        whole(&write(dir.path(), "painted.obj", obj.as_bytes()), "obj");
    assert_eq!(
        corner_colours(&triangles, &colours)[0][2],
        [0, 128, 255, 255]
    );
    // glTF: a material's base colour, in linear light, written as sRGB;
    // the other primitive, without one, grey.
    let positions: Vec<u8> = [[0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]]
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        base64_of(&positions)
    );
    let gltf = format!(
        r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"uri":"{uri}","byteLength":36}}],"bufferViews":[{{"buffer":0,"byteLength":36}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}],"materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[0.5,0.2140,0,1]}}}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"material":0}},{{"attributes":{{"POSITION":0}}}}]}}],"nodes":[{{"mesh":0}}],"scenes":[{{"nodes":[0]}}],"scene":0}}"#
    );
    let (_, _, triangles, colours) =
        whole(&write(dir.path(), "part.gltf", gltf.as_bytes()), "gltf");
    let corners = corner_colours(&triangles, &colours);
    assert_eq!(corners[0][0], [188, 127, 0, 255], "sRGB of 0.5, 0.214, 0");
    assert_eq!(corners[1][0], [197, 203, 215, 255], "the surface's grey");
    // 3MF: two triangles sharing an edge, of two base materials: the
    // shared corners are written once for each colour.
    let model = r##"<?xml version="1.0"?><model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"><resources><basematerials id="5"><base name="red" displaycolor="#FF0000"/><base name="teal" displaycolor="#00808080"/></basematerials><object id="1" type="model" pid="5" pindex="0"><mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="1" y="1" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="2" v3="3" pid="5" p1="1"/></triangles></mesh></object></resources><build><item objectid="1"/></build></model>"##;
    let package = dir.path().join("two colours.3mf");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
    zip.start_file("3D/3dmodel.model", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(model.as_bytes()).unwrap();
    zip.finish().unwrap();
    let (_, positions, triangles, colours) = whole(&package, "3mf");
    assert_eq!(positions.len(), 6, "the shared edge once a colour");
    let corners = corner_colours(&triangles, &colours);
    assert_eq!(corners[0], [[255, 0, 0, 255]; 3]);
    assert_eq!(corners[1], [[0, 128, 128, 128]; 3]);
}

/// A coloured model's still is drawn in its colours.
#[test]
fn a_coloured_still_is_drawn_in_its_colours() {
    let dir = tempfile::tempdir().unwrap();
    let ply = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 220 20 20\n1 0 0 220 20 20\n1 0 1 220 20 20\n0 0 1 220 20 20\n4 0 1 2 3\n";
    let path = write(dir.path(), "red.ply", ply.as_bytes());
    let output = Command::new(env!("CARGO_BIN_EXE_skwir-model-worker"))
        .arg(&path)
        .args(["--format", "ply", "--side", "64"])
        .output()
        .unwrap();
    let line = output.stdout.iter().position(|b| *b == b'\n').unwrap();
    let png = &output.stdout[line + 1..];
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).unwrap();
    // The square covers the middle of its picture: red, lit.
    let at = ((info.height / 2 * info.width + info.width / 2) * 4) as usize;
    let [r, g, b, a] = [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]];
    assert_eq!(a, 255);
    assert!(r > 100 && r > 3 * g && r > 3 * b, "{r} {g} {b}");
}

/// A cylinder `radius` across and `height` tall, standing on the origin, as
/// a CAD program exports it: a closed shell of a plane, a cylinder and a
/// plane, its length unit millimetres or inches. The solid is red, its top
/// face blue.
fn step_cylinder(radius: f64, height: f64, inches: bool) -> String {
    let unit = if inches {
        "#1=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );\n\
         #2=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);\n\
         #3=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#1);\n\
         #4=( CONVERSION_BASED_UNIT('INCH',#3) LENGTH_UNIT() NAMED_UNIT(#2) );\n"
    } else {
        "#4=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );\n"
    };
    let (r, h) = (format!("{radius:?}"), format!("{height:?}"));
    let style = |first: usize, rgb: &str, item: usize| {
        let n = |k: usize| first + k;
        format!(
            "#{}=COLOUR_RGB('',{rgb});\n#{}=FILL_AREA_STYLE_COLOUR('',#{});\n\
             #{}=FILL_AREA_STYLE('',(#{}));\n#{}=SURFACE_STYLE_FILL_AREA(#{});\n\
             #{}=SURFACE_SIDE_STYLE('',(#{}));\n#{}=SURFACE_STYLE_USAGE(.BOTH.,#{});\n\
             #{}=PRESENTATION_STYLE_ASSIGNMENT((#{}));\n#{}=STYLED_ITEM('',(#{}),#{item});\n",
            n(0),
            n(1),
            n(0),
            n(2),
            n(1),
            n(3),
            n(2),
            n(4),
            n(3),
            n(5),
            n(4),
            n(6),
            n(5),
            n(7),
            n(6),
        )
    };
    let red = style(56, "1.,0.,0.", 53);
    let blue = style(64, "0.,0.,1.", 51);
    "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\n\
     FILE_NAME('cylinder','2026-10-19T00:00:00',(''),(''),'','skwir test','');\n\
     FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n"
        .to_owned()
        + unit
        + &format!(
            "#5=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );
#6=( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() );
#7=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-7),#4,'distance_accuracy_value','confusion accuracy');
#8=( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#7)) GLOBAL_UNIT_ASSIGNED_CONTEXT((#4,#5,#6)) REPRESENTATION_CONTEXT('','3D') );
#9=APPLICATION_CONTEXT('automotive design');
#10=APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#9);
#11=PRODUCT_CONTEXT('',#9,'mechanical');
#12=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');
#13=PRODUCT('cylinder','cylinder','',(#11));
#14=PRODUCT_DEFINITION_FORMATION('','',#13);
#15=PRODUCT_DEFINITION('design','',#14,#12);
#16=PRODUCT_DEFINITION_SHAPE('','',#15);
#17=CARTESIAN_POINT('',(0.,0.,0.));
#18=DIRECTION('',(0.,0.,1.));
#19=DIRECTION('',(1.,0.,0.));
#20=AXIS2_PLACEMENT_3D('',#17,#18,#19);
#21=CIRCLE('',#20,{r});
#22=CARTESIAN_POINT('',({r},0.,0.));
#23=VERTEX_POINT('',#22);
#24=EDGE_CURVE('',#23,#23,#21,.T.);
#25=CARTESIAN_POINT('',(0.,0.,{h}));
#26=AXIS2_PLACEMENT_3D('',#25,#18,#19);
#27=CIRCLE('',#26,{r});
#28=CARTESIAN_POINT('',({r},0.,{h}));
#29=VERTEX_POINT('',#28);
#30=EDGE_CURVE('',#29,#29,#27,.T.);
#31=PLANE('',#20);
#32=ORIENTED_EDGE('',*,*,#24,.F.);
#33=EDGE_LOOP('',(#32));
#34=FACE_OUTER_BOUND('',#33,.T.);
#35=ADVANCED_FACE('',(#34),#31,.F.);
#36=CYLINDRICAL_SURFACE('',#20,{r});
#37=VECTOR('',#18,{h});
#38=LINE('',#22,#37);
#39=EDGE_CURVE('',#23,#29,#38,.T.);
#40=ORIENTED_EDGE('',*,*,#24,.T.);
#41=ORIENTED_EDGE('',*,*,#39,.T.);
#42=ORIENTED_EDGE('',*,*,#30,.F.);
#43=ORIENTED_EDGE('',*,*,#39,.F.);
#44=EDGE_LOOP('',(#40,#41,#42,#43));
#45=FACE_OUTER_BOUND('',#44,.T.);
#46=ADVANCED_FACE('',(#45),#36,.T.);
#47=PLANE('',#26);
#48=ORIENTED_EDGE('',*,*,#30,.T.);
#49=EDGE_LOOP('',(#48));
#50=FACE_OUTER_BOUND('',#49,.T.);
#51=ADVANCED_FACE('',(#50),#47,.T.);
#52=CLOSED_SHELL('',(#35,#46,#51));
#53=MANIFOLD_SOLID_BREP('',#52);
#54=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#20,#53),#8);
#55=SHAPE_DEFINITION_REPRESENTATION(#16,#54);
{red}{blue}#72=MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',(#63,#71),#8);
ENDSEC;
END-ISO-10303-21;
"
        )
}

#[test]
fn a_step_model_is_meshed_in_its_unit_and_its_colours() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        "a part’s cylinder.stp",
        step_cylinder(10., 30., false).as_bytes(),
    );
    let answer = run(&path, "step", &[]).unwrap();
    assert_eq!(answer.facts["unit"], "mm");
    assert_eq!(size(&answer), [20., 20., 30.]);
    assert_eq!(answer.facts["faces"], 3);
    assert_eq!(answer.facts["faces_missing"], 0);
    assert_eq!(answer.facts["objects"], 1);
    // Closed, though each face keeps its own points: π r² h within the
    // mesh's chord.
    assert_eq!(answer.facts["closed"], true);
    let volume = answer.facts["volume"].as_f64().unwrap();
    let exact = std::f64::consts::PI * 100. * 30.;
    assert!((volume / exact - 1.).abs() < 0.01, "{volume} for {exact}");

    // Each face in its colour: the side and the bottom the solid's red,
    // the top its own blue; every corner of a triangle one face's.
    let (said, positions, triangles, colours) = whole_up_to(&path, "step", 100_000);
    assert_eq!(said["mesh"], true);
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    for (triangle, corners) in triangles.iter().zip(corner_colours(&triangles, &colours)) {
        assert!(corners.iter().all(|c| *c == corners[0]));
        let top = triangle
            .iter()
            .all(|&i| (positions[i as usize][2] - 30.).abs() < 1e-4);
        assert_eq!(corners[0], if top { blue } else { red });
    }
    assert!(colours.contains(&blue) && colours.contains(&red));

    // The same part written in inches is measured in inches.
    let path = write(
        dir.path(),
        "inches.step",
        step_cylinder(10., 30., true).as_bytes(),
    );
    let answer = run(&path, "step", &[]).unwrap();
    assert_eq!(answer.facts["unit"], "in");
    assert_eq!(size(&answer), [20., 20., 30.]);
}

#[test]
fn what_is_not_a_step_model_is_refused_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        "not.step",
        b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\nENDSEC;\n",
    );
    let error = run(&path, "step", &[]).unwrap_err();
    assert!(error.contains("STEP"), "{error}");
    let path = write(dir.path(), "noise.step", &[0xff; 64]);
    assert!(run(&path, "step", &[]).is_err());
}

fn base64_of(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A Rhino 5 model (archive 50) built as openNURBS writes one: 8-byte
/// chunk lengths, a chunk's CRC left zero (not read).
mod rhino_file {
    const SHORT: u32 = 0x8000_0000;
    const CRC: u32 = 0x8000;
    pub const MESH: [u8; 16] = id(
        0x4ED7_D4E4,
        0xE947,
        0x11D3,
        [0xBF, 0xE5, 0, 0x10, 0x83, 1, 0x22, 0xF0],
    );
    pub const BREP: [u8; 16] = id(
        0x60B5_DBC5,
        0xE660,
        0x11D3,
        [0xBF, 0xE4, 0, 0x10, 0x83, 1, 0x22, 0xF0],
    );
    pub const INSTANCE: [u8; 16] = id(
        0xF9CF_B638,
        0xB9D4,
        0x4340,
        [0x87, 0xE3, 0xC5, 0x6E, 0x78, 0x65, 0xD9, 0x6A],
    );
    /// A class openNURBS does not check here: a layer's, a material's.
    const ANY: [u8; 16] = [7; 16];

    pub const fn id(a: u32, b: u16, c: u16, d: [u8; 8]) -> [u8; 16] {
        let (a, b, c) = (a.to_le_bytes(), b.to_le_bytes(), c.to_le_bytes());
        [
            a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5],
            d[6], d[7],
        ]
    }

    pub fn chunk(code: u32, body: &[u8]) -> Vec<u8> {
        let crc = if code & CRC != 0 { 4 } else { 0 };
        let mut out = code.to_le_bytes().to_vec();
        out.extend(((body.len() + crc) as u64).to_le_bytes());
        out.extend(body);
        out.extend(vec![0; crc]);
        out
    }

    fn short(code: u32, value: u64) -> Vec<u8> {
        let mut out = (code | SHORT).to_le_bytes().to_vec();
        out.extend(value.to_le_bytes());
        out
    }

    fn anonymous(body: &[u8]) -> Vec<u8> {
        chunk(0x4000_8000, body)
    }

    /// Two version numbers, then `body`, in an anonymous chunk.
    fn versioned(body: &[u8]) -> Vec<u8> {
        anonymous(&[&1i32.to_le_bytes()[..], &0i32.to_le_bytes(), body].concat())
    }

    pub fn object(class: [u8; 16], data: &[u8]) -> Vec<u8> {
        chunk(
            0x0002_7FFA,
            &[
                chunk(0x0002_FFFB, &class),
                chunk(0x0002_FFFC, data),
                short(0x0002_7FFF, 0),
            ]
            .concat(),
        )
    }

    /// An `ON_Mesh` of version 3.5: quads (a triangle repeats its third
    /// corner), its vertices and colours stored uncompressed.
    pub fn mesh(points: &[[f32; 3]], faces: &[[u32; 4]], colours: Option<&[[u8; 4]]>) -> Vec<u8> {
        let mut out = vec![0x35];
        out.extend(i32::try_from(points.len()).unwrap().to_le_bytes());
        out.extend(i32::try_from(faces.len()).unwrap().to_le_bytes());
        out.extend([0; 148 + 5]);
        out.extend(4i32.to_le_bytes());
        out.extend(faces.iter().flatten().flat_map(|i| i.to_le_bytes()));
        let buffer = |bytes: Vec<u8>| {
            let mut out = (bytes.len() as u32).to_le_bytes().to_vec();
            if !bytes.is_empty() {
                out.extend([0; 4]);
                out.push(0);
                out.extend(bytes);
            }
            out
        };
        out.extend(buffer(
            points
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect(),
        ));
        for _ in 0..3 {
            out.extend(buffer(Vec::new()));
        }
        out.extend(buffer(
            colours
                .unwrap_or_default()
                .iter()
                .flatten()
                .copied()
                .collect(),
        ));
        out
    }

    /// An `ON_Brep` of version 3.2: its curves, surfaces and topology empty,
    /// a face for each of `meshes`, then those meshes (`None`: none stored).
    pub fn brep(meshes: &[Option<Vec<u8>>]) -> Vec<u8> {
        let mut out = vec![0x32];
        for _ in 0..7 {
            out.extend(anonymous(&[0x10]));
        }
        let mut faces = vec![0x10];
        faces.extend(i32::try_from(meshes.len()).unwrap().to_le_bytes());
        for i in 0..meshes.len() {
            for value in [i32::try_from(i).unwrap(), 0, 0, 0, 0] {
                faces.extend(value.to_le_bytes());
            }
        }
        out.extend(anonymous(&faces));
        out.extend([0; 48]);
        let mut render = Vec::new();
        for mesh in meshes {
            match mesh {
                Some(mesh) => {
                    render.push(1);
                    render.extend(object(MESH, mesh));
                }
                None => render.push(0),
            }
        }
        out.extend(anonymous(&render));
        out
    }

    /// Version 2.1 attributes: an id, a layer, then items.
    pub fn attributes(
        id: u8,
        layer: i32,
        material: Option<i32>,
        mode: u8,
        visible: bool,
    ) -> Vec<u8> {
        let mut out = vec![0x21];
        out.extend([id; 16]);
        out.extend(layer.to_le_bytes());
        if let Some(material) = material {
            out.push(4);
            out.extend(material.to_le_bytes());
        }
        if !visible {
            out.extend([11, 0]);
        }
        if mode != 0 {
            out.extend([12, mode]);
        }
        if material.is_some() {
            out.extend([16, 1]);
        }
        out.push(0);
        out
    }

    pub fn record(object: &[u8], attributes: &[u8]) -> Vec<u8> {
        chunk(
            0x2000_8070,
            &[
                short(0x0200_0071, 32),
                object.to_vec(),
                chunk(0x0200_8072, attributes),
                short(0x0200_007F, 0),
            ]
            .concat(),
        )
    }

    /// A layer of version 1.1: its index, material and visibility.
    pub fn layer(index: i32, visible: bool) -> Vec<u8> {
        let mut data = vec![0x11];
        for value in [0, index, 0, -1, 0] {
            data.extend(value.to_le_bytes());
        }
        data.extend([0; 4 + 2 + 2 + 8 + 8]);
        data.extend(0u32.to_le_bytes());
        data.push(u8::from(visible));
        chunk(0x2000_8050, &object(ANY, &data))
    }

    /// A material of version 2 (Rhino 4 and 5): its index and diffuse
    /// colour.
    pub fn material(index: i32, diffuse: [u8; 3]) -> Vec<u8> {
        let mut inside = vec![0; 16];
        inside.extend(index.to_le_bytes());
        inside.extend(0u32.to_le_bytes());
        inside.extend([0; 16]);
        inside.extend([0, 0, 0, 0]);
        inside.extend([diffuse[0], diffuse[1], diffuse[2], 0]);
        let data = [&[0x20][..], &versioned(&inside)].concat();
        chunk(0x2000_8040, &object(ANY, &data))
    }

    /// A block definition of Rhino 5's layout: its id and members.
    pub fn definition(id: u8, members: &[u8]) -> Vec<u8> {
        let mut data = vec![0x10];
        data.extend([id; 16]);
        data.extend((members.len() as u32).to_le_bytes());
        for member in members {
            data.extend([*member; 16]);
        }
        chunk(0x2000_8076, &object(ANY, &data))
    }

    /// A block instance: its definition, and the matrix placing it, rows
    /// of one that maps column vectors.
    pub fn instance(definition: u8, matrix: [[f64; 4]; 4]) -> Vec<u8> {
        let mut data = vec![0x10];
        data.extend([definition; 16]);
        data.extend(matrix.iter().flatten().flat_map(|v| v.to_le_bytes()));
        object(INSTANCE, &data)
    }

    pub fn table(code: u32, records: &[Vec<u8>]) -> Vec<u8> {
        chunk(code, &[records.concat(), short(0xFFFF_FFFF, 0)].concat())
    }

    /// The file: its header, a comment, its tables, its end.
    pub fn file(tables: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"3D Geometry File Format       50".to_vec();
        out.extend(chunk(1, b"made by a test"));
        out.extend(table(0x1000_0014, &[short(0x2000_0026, 201_005_000)]));
        // Millimetres.
        out.extend(table(
            0x1000_0015,
            &[chunk(
                0x2000_8031,
                &[100i32.to_le_bytes(), 2i32.to_le_bytes()].concat(),
            )],
        ));
        for table in tables {
            out.extend(table);
        }
        let length = out.len() as u64 + 12 + 8;
        out.extend(chunk(0x7FFF, &length.to_le_bytes()));
        out
    }
}

#[test]
fn a_rhino_model_draws_the_meshes_it_stores_as_rhino_shows_them() {
    use rhino_file::*;
    let square = mesh(
        &[[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
        &[[0, 1, 2, 3]],
        None,
    );
    let triangle = |z: f32| {
        mesh(
            &[[0., 0., z], [1., 0., z], [0., 1., z]],
            &[[0, 1, 2, 2]],
            Some(&[[0, 0, 255, 0]; 3]),
        )
    };
    let objects = [
        // A square in the object's own material.
        record(&object(MESH, &square), &attributes(1, 0, Some(5), 0, true)),
        // A B-rep with one face meshed and one not.
        record(
            &object(BREP, &brep(&[Some(triangle(2.)), None])),
            &attributes(2, 0, None, 0, true),
        ),
        // A B-rep saved small: drawn by none, said.
        record(
            &object(BREP, &brep(&[None, None])),
            &attributes(3, 0, None, 0, true),
        ),
        // On a hidden layer, and hidden itself: not drawn.
        record(
            &object(MESH, &triangle(9.)),
            &attributes(4, 1, None, 0, true),
        ),
        record(
            &object(MESH, &triangle(9.)),
            &attributes(5, 0, None, 0, false),
        ),
        // A block's member, drawn only where an instance places it: there
        // mirrored across X and moved 10 along it.
        record(
            &object(MESH, &triangle(4.)),
            &attributes(6, 0, None, 3, true),
        ),
        record(
            &instance(
                8,
                [
                    [-1., 0., 0., 10.],
                    [0., 1., 0., 0.],
                    [0., 0., 1., 0.],
                    [0., 0., 0., 1.],
                ],
            ),
            &attributes(7, 0, None, 0, true),
        ),
    ];
    let bytes = file(&[
        table(0x1000_0010, &[material(5, [255, 0, 0])]),
        table(0x1000_0011, &[layer(0, true), layer(1, false)]),
        table(0x1000_0021, &[definition(8, &[6])]),
        table(0x1000_0013, &objects),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "a part’s model.3dm", &bytes);
    let answer = run(&path, "3dm", &[]).unwrap();
    assert_eq!(answer.facts["unit"], "mm");
    assert_eq!(answer.facts["objects"], 3, "{}", answer.facts);
    assert_eq!(answer.facts["objects_missing"], 1);
    assert_eq!(answer.facts["triangles"], 4);

    let (_, positions, triangles, colours) = whole(&path, "3dm");
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let corners = corner_colours(&triangles, &colours);
    // The square in its material's red, the triangles in their vertices'
    // blue.
    assert_eq!(corners[..2], [[red; 3], [red; 3]]);
    assert_eq!(corners[2..], [[blue; 3], [blue; 3]]);
    // The member placed by its instance: mirrored, so wound back to face
    // the same way.
    let placed = triangles[3].map(|i| positions[i as usize]);
    assert_eq!(placed, [[10., 0., 4.], [10., 1., 4.], [9., 0., 4.]]);
    assert!(
        positions.iter().all(|p| (p[2] - 9.).abs() > 1e-3),
        "nothing hidden drawn"
    );

    // Saved small: nothing to draw, and why.
    let bytes = file(&[table(
        0x1000_0013,
        &[record(
            &object(BREP, &brep(&[None])),
            &attributes(1, -1, None, 0, true),
        )],
    )]);
    let path = write(dir.path(), "small.3dm", &bytes);
    let error = run(&path, "3dm", &[]).unwrap_err();
    assert!(error.contains("stored mesh"), "{error}");
    // Rhino 1's, and what is no model, refused.
    let path = write(dir.path(), "old.3dm", b"3D Geometry File Format        1");
    assert!(run(&path, "3dm", &[]).unwrap_err().contains("Rhino 1"));
    let path = write(dir.path(), "cut.3dm", &file(&[])[..40]);
    assert!(run(&path, "3dm", &[]).is_err());
}

#[test]
fn an_iges_model_is_meshed_as_a_step_model_is() {
    use ogeom_core::Tolerances;
    // A red cylinder 20 mm across and 30 mm tall, written by ogeom.
    let tolerances = Tolerances::millimetres();
    let mut model = ogeom_topo::Model::new();
    let cylinder = ogeom_algo::primitive::make_cylinder(
        &mut model,
        ogeom_math::Frame::WORLD,
        10.,
        30.,
        tolerances,
    )
    .unwrap()
    .shape;
    let mut document = ogeom_doc::Document::over(model);
    document.add_part("cylinder", cylinder.clone());
    document.set_colour(&cylinder, ogeom_doc::Colour::rgb(1., 0., 0.));
    let text = ogeom_io::write_iges(&document, tolerances).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "a part’s cylinder.igs", text.as_bytes());
    let answer = run(&path, "iges", &[]).unwrap();
    assert_eq!(answer.facts["unit"], "mm");
    assert_eq!(size(&answer), [20., 20., 30.]);
    assert_eq!(answer.facts["faces_missing"], 0);
    assert_eq!(answer.facts["closed"], true);
    let volume = answer.facts["volume"].as_f64().unwrap();
    let exact = std::f64::consts::PI * 100. * 30.;
    assert!((volume / exact - 1.).abs() < 0.01, "{volume} for {exact}");
    let (_, _, _, colours) = whole_up_to(&path, "iges", 100_000);
    assert!(
        colours.iter().all(|c| *c == [255, 0, 0, 255]),
        "in its colour"
    );

    // What is not IGES is refused, saying so.
    let path = write(dir.path(), "not.igs", b"nothing of the sort\n");
    let error = run(&path, "iges", &[]).unwrap_err();
    assert!(error.contains("IGES"), "{error}");
}
