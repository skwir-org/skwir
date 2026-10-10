//! 3D meshes read and drawn as a picture, in a process of its own: a
//! model can hold millions of triangles, whose memory is given back when
//! this exits, and a reader that fails takes nothing else with it.
//!
//! `bibli-model-worker <path> --format stl|obj|ply|off|gltf|3mf --side N
//! --triangles N --bytes N` writes one line of JSON (what the mesh
//! measures) then the picture as PNG, at most `side` pixels on its longer
//! side, on standard output. A file larger than `bytes`, or a mesh of more
//! than `triangles`, is refused. Anything else is an exit status of 1 and
//! a reason on standard error. Pure Rust; nothing is written.
mod draw;
mod gltf_mesh;
mod mesh;
mod ply;
mod threemf;

use std::io::Write;
use std::path::PathBuf;

struct Arguments {
    path: PathBuf,
    format: String,
    side: u32,
    triangles: usize,
    bytes: u64,
}

fn arguments() -> Result<Arguments, String> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(args.next().ok_or("no file named")?);
    let mut parsed = Arguments {
        path,
        format: String::new(),
        side: 1024,
        triangles: 4_000_000,
        bytes: 256 * 1024 * 1024,
    };
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .and_then(|v| v.into_string().ok())
            .ok_or_else(|| format!("{} has no value", flag.to_string_lossy()))?;
        let number = || {
            value
                .parse::<u64>()
                .map_err(|_| format!("{value} is not a number"))
        };
        match flag.to_str() {
            Some("--format") => parsed.format.clone_from(&value),
            Some("--side") => {
                parsed.side = u32::try_from(number()?.clamp(16, 4096)).unwrap_or(1024);
            }
            Some("--triangles") => {
                parsed.triangles = usize::try_from(number()?).unwrap_or(usize::MAX);
            }
            Some("--bytes") => parsed.bytes = number()?,
            _ => return Err(format!("unknown flag {}", flag.to_string_lossy())),
        }
    }
    Ok(parsed)
}

fn main() {
    // `--build`, bibli asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--build")
    {
        println!("{}", env!("BIBLI_BUILD"));
        return;
    }
    // `--probe`, bibli's settings asking whether this helper works here
    // (`platform::process::probe`): being started at all says its
    // libraries are there.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--probe")
    {
        return;
    }
    if let Err(reason) = run() {
        eprintln!("{reason}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = arguments()?;
    let bytes = mesh::read_bounded(&args.path, args.bytes)?;
    let mut mesh = mesh::Mesh::new(args.triangles);
    if args.format == "3mf" {
        threemf::read(bytes, args.bytes, &mut mesh)?;
    } else {
        match args.format.as_str() {
            "stl" => mesh::stl(&bytes, &mut mesh)?,
            "obj" => mesh::obj(&bytes, &mut mesh)?,
            "off" => mesh::off(&bytes, &mut mesh)?,
            "ply" => ply::read(&bytes, &mut mesh)?,
            "gltf" => gltf_mesh::read(&bytes, &args.path, args.bytes, &mut mesh)?,
            other => return Err(format!("the format {other:?} is not read here")),
        }
        // Read: the file's bytes, up to half a gigabyte, are not kept
        // while the model is measured and drawn.
        drop(bytes);
    }
    let (lo, hi) = mesh.bounds().ok_or("the model has no triangles")?;
    let size: Vec<f32> = (0..3).map(|axis| hi[axis] - lo[axis]).collect();
    let volume = mesh.closed_volume();
    let facts = serde_json::json!({
        "triangles": mesh.triangles.len(),
        "objects": mesh.objects,
        "size": size,
        "unit": mesh.unit,
        "closed": volume.is_some(),
        "volume": volume,
    });
    let picture = draw::draw(mesh, args.side);
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, picture.width, picture.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("encoding the picture: {e}"))?;
        writer
            .write_image_data(&picture.rgba)
            .map_err(|e| format!("encoding the picture: {e}"))?;
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "{facts}")
        .and_then(|()| out.write_all(&png))
        .and_then(|()| out.flush())
        .map_err(|e| format!("writing the picture: {e}"))
}
