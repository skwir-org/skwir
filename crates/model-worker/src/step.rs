//! STEP models (ISO 10303-21: AP203, AP214, AP242), read and meshed by
//! ogeom (ADR-069). A STEP file holds exact surfaces, never triangles:
//! each part is placed where the file's assembly puts it, and each face is
//! meshed to a chord a thousandth of the model's size, in the colour the
//! file gives it. Lengths are in the file's own unit. A face ogeom cannot
//! mesh is left out and counted, so the preview says so rather than show
//! a hole as the model.
//!
//! Each face keeps its own points, so a smooth shading stays smooth within
//! a curved face and sharp across the edges between faces, as CAD draws
//! them. Whether the model is watertight, and its volume, are measured on
//! the mesh ogeom welds of the same faces: an exported file's faces meet
//! along their edges only within the file's tolerance.
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;

use ogeom_core::Tolerances;
use ogeom_doc::{Colour as Paint, ProductId};
use ogeom_mesh::{Deflection, face_meshes_for, triangulate_with_face_meshes};
use ogeom_topo::{Filter, Model, Shape, ShapeType, explore};

use crate::mesh::{Colour, IDENTITY, Mesh, Point};

/// The kernel's tolerances: ogeom builds the model in millimetres, whatever
/// the file's unit.
const TOLERANCES: Tolerances = Tolerances::millimetres();
/// The chord a face is meshed to, as a share of the model's diagonal.
const CHORD: f64 = 1. / 1000.;
/// The greatest turn of a surface across one triangle: 20°, a circle in
/// eighteen segments.
const ANGLE: f64 = 0.35;
/// The units a file may state, with their millimetres.
const UNITS: [(&str, f64); 6] = [
    ("mm", 1.),
    ("cm", 10.),
    ("m", 1000.),
    ("in", 25.4),
    ("ft", 304.8),
    ("µm", 0.001),
];

/// How many faces the file has, how many of them could not be meshed, and
/// the volume its bodies enclose when every one of them is closed.
pub struct Faces {
    pub all: usize,
    pub missing: usize,
    pub volume: Option<f64>,
}

/// The model in `bytes`, added to `mesh`.
pub fn read(bytes: &[u8], mesh: &mut Mesh) -> Result<Faces, String> {
    // A reader of a format this large may fail on a file in ways it did
    // not foresee: a panic is said as the reason, not as a trace.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let read = std::panic::catch_unwind(AssertUnwindSafe(|| unguarded(bytes, mesh)));
    std::panic::set_hook(hook);
    read.unwrap_or_else(|panic| {
        let said = panic
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        Err(format!("the STEP reader failed on this file: {said}"))
    })
}

fn unguarded(bytes: &[u8], mesh: &mut Mesh) -> Result<Faces, String> {
    let import = ogeom_io::read_step(&String::from_utf8_lossy(bytes), TOLERANCES)
        .map_err(|e| format!("not a STEP model ogeom reads: {e}"))?;
    let document = &import.document;
    let model = document.model();
    // Every part where the product tree places it, then any body the tree
    // does not reach, where the file states it.
    let mut bodies: Vec<(Option<ProductId>, Shape)> = Vec::new();
    for root in document.roots() {
        for occurrence in document.occurrences_of(root).map_err(|e| e.to_string())? {
            bodies.push((Some(occurrence.part), occurrence.shape));
        }
    }
    let mut reached = HashSet::new();
    for (_, body) in &bodies {
        reached.extend(faces_of(model, body)?.iter().map(Shape::node));
    }
    for body in import.solids.iter().chain(&import.shells) {
        if faces_of(model, body)?
            .iter()
            .any(|face| !reached.contains(&face.node()))
        {
            bodies.push((None, body.clone()));
        }
    }
    let (unit, per_unit) = UNITS
        .into_iter()
        .find(|(_, mm)| (import.report.scale_mm / mm - 1.).abs() < 1e-9)
        .unwrap_or(UNITS[0]);
    let deflection = Deflection {
        chord: (diagonal(model, &bodies)? * CHORD).max(1e-6),
        angular: ANGLE,
        ..Deflection::default()
    };
    let mut faces = Faces {
        all: 0,
        missing: 0,
        volume: Some(0.),
    };
    for (part, body) in &bodies {
        // The welded whole is made only when every face meshes.
        let Ok((meshed, whole)) =
            triangulate_with_face_meshes(model, body, deflection, None, TOLERANCES)
                .map(|(whole, _, meshed)| (meshed, Some(whole)))
                .or_else(|_| {
                    face_meshes_for(model, body, deflection, None, TOLERANCES)
                        .map(|(meshed, _)| (meshed, None))
                })
        else {
            let count = faces_of(model, body)?.len();
            faces.all += count;
            faces.missing += count;
            faces.volume = None;
            continue;
        };
        faces.volume = match (faces.volume, whole) {
            (Some(sum), Some(whole)) => {
                let mut welded = Mesh::new(usize::MAX);
                welded.add(
                    &points(&whole.positions, per_unit),
                    whole.triangles.iter().copied(),
                    &IDENTITY,
                )?;
                welded.closed_volume().map(|volume| sum + volume)
            }
            _ => None,
        };
        for (face, piece) in meshed {
            faces.all += 1;
            let Some(piece) = piece.ok().filter(|piece| !piece.triangles.is_empty()) else {
                faces.missing += 1;
                continue;
            };
            let points = points(&piece.positions, per_unit);
            let paint = match part {
                Some(part) => document.resolved_colour(*part, &face),
                None => document.colour_of(&face),
            };
            let colours = paint.map(|paint| vec![colour(paint); points.len()]);
            mesh.add_coloured(
                &points,
                colours.as_deref(),
                piece.triangles.iter().copied(),
                &IDENTITY,
            )?;
        }
    }
    mesh.objects = bodies.len();
    mesh.unit = Some(unit);
    Ok(faces)
}

/// Positions in millimetres as points in the file's unit.
fn points(positions: &[ogeom_math::Point], per_unit: f64) -> Vec<Point> {
    positions
        .iter()
        .map(|p| [p.x, p.y, p.z].map(|c| (c / per_unit) as f32))
        .collect()
}

fn faces_of(model: &Model, shape: &Shape) -> Result<Vec<Shape>, String> {
    explore(model, shape, Filter::OfType(ShapeType::Face)).map_err(|e| e.to_string())
}

/// The diagonal of the box around every body, in millimetres.
fn diagonal(model: &Model, bodies: &[(Option<ProductId>, Shape)]) -> Result<f64, String> {
    let mut extent: Option<([f64; 3], [f64; 3])> = None;
    for (_, body) in bodies {
        let bounds = ogeom_algo::measure::shape_bounds(model, body, TOLERANCES)
            .map_err(|e| e.to_string())?;
        let (Some(low), Some(high)) = (bounds.low(), bounds.high()) else {
            continue;
        };
        let (low, high) = ([low.x, low.y, low.z], [high.x, high.y, high.z]);
        extent = Some(match extent {
            None => (low, high),
            Some((l, h)) => (
                std::array::from_fn(|i| l[i].min(low[i])),
                std::array::from_fn(|i| h[i].max(high[i])),
            ),
        });
    }
    let (low, high) = extent.ok_or("the model has no faces")?;
    Ok((0..3)
        .map(|i| (high[i] - low[i]).powi(2))
        .sum::<f64>()
        .sqrt())
}

/// A colour as the file states it, 0 to 1, as sRGB bytes: CAD programs
/// write the colour they show.
fn colour(paint: Paint) -> Colour {
    [paint.r, paint.g, paint.b, paint.a].map(|c| (c.clamp(0., 1.) * 255. + 0.5) as u8)
}
