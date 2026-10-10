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
