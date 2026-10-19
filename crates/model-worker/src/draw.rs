//! A mesh drawn as a picture: seen from the front, a little to the right
//! and above, without perspective, each face shaded by two lights on a
//! grey that reads on a light or a dark panel, around it nothing
//! (transparent). Drawn at twice the size and averaged down, so edges are
//! smooth; the rows are shared out between threads, each drawing every
//! triangle that crosses its band.
use crate::mesh::{Mesh, Point};

/// Where the camera stands: degrees right of the front, and above.
const AZIMUTH: f32 = 35.;
const ELEVATION: f32 = 28.;
/// Room left around the model, as a share of the side.
const MARGIN: f32 = 0.05;
/// Pixels drawn per pixel given, along each side.
const SAMPLES: usize = 2;
/// The surface's colour, in linear light, where the model gives none.
pub const ALBEDO: [f32; 3] = [0.56, 0.60, 0.68];

pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn dot(a: Point, b: Point) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn normalized(a: Point) -> Option<Point> {
    let length = dot(a, a).sqrt();
    (length > 0. && length.is_finite()).then(|| a.map(|v| v / length))
}

fn blend(parts: &[(f32, Point)]) -> Point {
    let mut out = [0.; 3];
    for (weight, v) in parts {
        for axis in 0..3 {
            out[axis] += weight * v[axis];
        }
    }
    normalized(out).unwrap_or(out)
}

/// An sRGB byte as linear light.
fn decode(byte: u8) -> f32 {
    let s = f32::from(byte) / 255.;
    if s <= 0.040_45 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light as an sRGB byte.
pub fn encode(linear: f32) -> u8 {
    let v = linear.clamp(0., 1.);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    };
    (s * 255. + 0.5) as u8
}

/// A triangle as drawn: its corners in drawn pixels (x, y, nearness) and
/// its colour.
#[derive(Clone, Copy)]
struct Flat {
    corners: [[f32; 3]; 3],
    colour: [u8; 3],
    top: f32,
    bottom: f32,
}

/// The mesh drawn at most `side` pixels on its longer side.
pub fn draw(mesh: Mesh, side: u32) -> Picture {
    let (az, el) = (AZIMUTH.to_radians(), ELEVATION.to_radians());
    // Toward the camera, and the picture's right and up.
    let eye = [az.sin() * el.cos(), -az.cos() * el.cos(), el.sin()];
    let right = [az.cos(), az.sin(), 0.];
    let up = cross(eye, right);
    let key = blend(&[(-0.4, right), (0.8, up), (0.6, eye)]);
    let fill = blend(&[(0.7, right), (-0.2, up), (0.5, eye)]);
    let projected: Vec<[f32; 3]> = mesh
        .positions
        .iter()
        .map(|p| [dot(*p, right), dot(*p, up), dot(*p, eye)])
        .collect();
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for &i in mesh.triangles.iter().flatten() {
        let p = projected[i as usize];
        for axis in 0..2 {
            lo[axis] = lo[axis].min(p[axis]);
            hi[axis] = hi[axis].max(p[axis]);
        }
    }
    let span = [(hi[0] - lo[0]).max(1e-9), (hi[1] - lo[1]).max(1e-9)];
    let side = side.max(16) as f32;
    let pad = side * MARGIN;
    let scale = (side - 2. * pad) / span[0].max(span[1]);
    let width = ((span[0] * scale + 2. * pad).ceil() as usize).max(16);
    let height = ((span[1] * scale + 2. * pad).ceil() as usize).max(16);
    let s = SAMPLES as f32;
    let flats: Vec<Flat> = mesh
        .triangles
        .iter()
        .filter_map(|t| {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            let mut normal = normalized(cross(sub(b, a), sub(c, a)))?;
            // Either side of a face may face the camera: a mesh's winding
            // is not always kept.
            if dot(normal, eye) < 0. {
                normal = normal.map(|v| -v);
            }
            let light = 0.22 + 0.62 * dot(normal, key).max(0.) + 0.28 * dot(normal, fill).max(0.);
            // The face's colour: its corners', averaged in linear light.
            let albedo = if mesh.colours.is_empty() {
                ALBEDO
            } else {
                let corners = t.map(|i| mesh.colours[i as usize]);
                [0, 1, 2].map(|c| corners.iter().map(|k| decode(k[c])).sum::<f32>() / 3.)
            };
            let colour = albedo.map(|albedo| encode(albedo * light));
            let corners = t.map(|i| {
                let p = projected[i as usize];
                [
                    ((p[0] - lo[0]) * scale + pad) * s,
                    ((hi[1] - p[1]) * scale + pad) * s,
                    p[2],
                ]
            });
            let ys = corners.map(|c| c[1]);
            Some(Flat {
                corners,
                colour,
                top: ys[0].min(ys[1]).min(ys[2]),
                bottom: ys[0].max(ys[1]).max(ys[2]),
            })
        })
        .collect();
    // Each triangle is a flat now: the mesh and its projection, as large
    // again, are let go of before the picture is drawn.
    drop(projected);
    drop(mesh);
    let (big_w, big_h) = (width * SAMPLES, height * SAMPLES);
    let mut colour = vec![[0u8; 4]; big_w * big_h];
    let mut depth = vec![f32::NEG_INFINITY; big_w * big_h];
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, 16);
    let band = big_h.div_ceil(threads);
    std::thread::scope(|scope| {
        for (index, (colour, depth)) in colour
            .chunks_mut(band * big_w)
            .zip(depth.chunks_mut(band * big_w))
            .enumerate()
        {
            let flats = &flats;
            scope.spawn(move || {
                let first = index * band;
                let rows = colour.len() / big_w;
                fill_band(flats, first, rows, big_w, colour, depth);
            });
        }
    });
    Picture {
        width: width as u32,
        height: height as u32,
        rgba: averaged(&colour, width, height),
    }
}

/// Every triangle crossing rows `first..first + rows`, drawn into them.
fn fill_band(
    flats: &[Flat],
    first: usize,
    rows: usize,
    width: usize,
    colour: &mut [[u8; 4]],
    depth: &mut [f32],
) {
    let (top, bottom) = (first as f32, (first + rows) as f32);
    let edge = |a: [f32; 3], b: [f32; 3], x: f32, y: f32| {
        (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
    };
    for flat in flats {
        if flat.bottom < top || flat.top >= bottom {
            continue;
        }
        let [a, b, c] = flat.corners;
        let area = edge(a, b, c[0], c[1]);
        if area == 0. || !area.is_finite() {
            continue;
        }
        let x0 = a[0].min(b[0]).min(c[0]).floor().max(0.) as usize;
        let x1 = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(width);
        let y0 = (flat.top.floor().max(top)) as usize;
        let y1 = (flat.bottom.ceil() as usize).min(first + rows);
        for y in y0..y1 {
            let py = y as f32 + 0.5;
            let row = (y - first) * width;
            for x in x0..x1 {
                let px = x as f32 + 0.5;
                let w0 = edge(b, c, px, py) / area;
                let w1 = edge(c, a, px, py) / area;
                let w2 = edge(a, b, px, py) / area;
                if w0 < 0. || w1 < 0. || w2 < 0. {
                    continue;
                }
                let near = w0 * a[2] + w1 * b[2] + w2 * c[2];
                let at = row + x;
                if near > depth[at] {
                    depth[at] = near;
                    let [r, g, bl] = flat.colour;
                    colour[at] = [r, g, bl, 255];
                }
            }
        }
    }
}

/// Each block of `SAMPLES` × `SAMPLES` pixels as one, its colour weighted
/// by coverage.
fn averaged(big: &[[u8; 4]], width: usize, height: usize) -> Vec<u8> {
    let big_w = width * SAMPLES;
    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0u32; 4];
            for dy in 0..SAMPLES {
                for dx in 0..SAMPLES {
                    let [r, g, b, a] = big[(y * SAMPLES + dy) * big_w + x * SAMPLES + dx];
                    let a32 = u32::from(a);
                    sum[0] += u32::from(r) * a32;
                    sum[1] += u32::from(g) * a32;
                    sum[2] += u32::from(b) * a32;
                    sum[3] += a32;
                }
            }
            let n = (SAMPLES * SAMPLES) as u32;
            match (
                sum[0].checked_div(sum[3]),
                sum[1].checked_div(sum[3]),
                sum[2].checked_div(sum[3]),
            ) {
                (Some(r), Some(g), Some(b)) => {
                    out.extend([r as u8, g as u8, b as u8, (sum[3] / n) as u8]);
                }
                // Nothing drawn here.
                _ => out.extend([0, 0, 0, 0]),
            }
        }
    }
    out
}
