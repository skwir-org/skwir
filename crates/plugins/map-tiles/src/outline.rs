//! The offline background: Natural Earth's land, lakes and borders (public
//! domain; `tools/maps/natural_earth.py` packs them), drawn into tiles in
//! the theme's colours. Enough to see a country or a region; not a street.
//!
//! The data is decoded the first time a map needs it and kept while the
//! plugin is enabled (about 110 000 points, under 1 MB). Every ring is
//! clipped to the tile before it is drawn, so the rasterizer never sees
//! the coordinates of a continent at street zoom.
use std::sync::Arc;

use bibli_core::thumbs::Image;
use bibli_core::tiles::{Palette, TileRequest};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

static DATA: &[u8] = include_bytes!("../data/natural-earth.bin");

/// Below this zoom the 1:110m layers are drawn, from it the 1:50m ones.
const FINE_FROM: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Land,
    Lakes,
    Borders,
}

/// A ring or a line in world coordinates (the unit square of the map).
struct Ring {
    /// West, north, east, south.
    bbox: [f32; 4],
    points: Vec<[f32; 2]>,
}

struct Layer {
    kind: Kind,
    fine: bool,
    rings: Vec<Ring>,
}

pub struct Outline {
    layers: Vec<Layer>,
}

fn project(lon: f64, lat: f64) -> [f32; 2] {
    const MAX_LAT: f64 = 85.051_128_779_806_59;
    let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
    let x = (lon + 180.) / 360.;
    let y = (1. - (lat.tan() + 1. / lat.cos()).ln() / std::f64::consts::PI) / 2.;
    [x as f32, y as f32]
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> anyhow::Result<u8> {
        let b = *self
            .data
            .get(self.at)
            .ok_or_else(|| anyhow::anyhow!("the outline data ends early"))?;
        self.at += 1;
        Ok(b)
    }

    fn varint(&mut self) -> anyhow::Result<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            value |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        anyhow::bail!("a number in the outline data is too long")
    }

    fn signed(&mut self) -> anyhow::Result<i64> {
        // Zigzag: 0, -1, 1, -2… as 0, 1, 2, 3…
        let n = self.varint()?;
        let magnitude = i64::try_from(n >> 1)?;
        Ok(if n & 1 == 1 {
            -magnitude - 1
        } else {
            magnitude
        })
    }
}

impl Outline {
    pub fn load() -> anyhow::Result<Outline> {
        Outline::parse(DATA)
    }

    fn parse(data: &[u8]) -> anyhow::Result<Outline> {
        anyhow::ensure!(data.starts_with(b"BNE1"), "not the outline data");
        let mut r = Reader { data, at: 4 };
        let count = r.byte()?;
        let mut layers = Vec::with_capacity(count.into());
        for _ in 0..count {
            let kind = match r.byte()? {
                0 => Kind::Land,
                1 => Kind::Lakes,
                _ => Kind::Borders,
            };
            let fine = r.byte()? == 50;
            let rings_count = usize::try_from(r.varint()?)?;
            let mut rings = Vec::with_capacity(rings_count.min(100_000));
            for _ in 0..rings_count {
                let n = usize::try_from(r.varint()?)?;
                let mut points = Vec::with_capacity(n.min(1_000_000));
                let (mut lon, mut lat) = (0i64, 0i64);
                let mut bbox = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                for _ in 0..n {
                    lon += r.signed()?;
                    lat += r.signed()?;
                    let p = project(lon as f64 * 1e-5, lat as f64 * 1e-5);
                    bbox = [
                        bbox[0].min(p[0]),
                        bbox[1].min(p[1]),
                        bbox[2].max(p[0]),
                        bbox[3].max(p[1]),
                    ];
                    points.push(p);
                }
                rings.push(Ring { bbox, points });
            }
            layers.push(Layer { kind, fine, rings });
        }
        Ok(Outline { layers })
    }

    /// Draw a tile.
    pub fn tile(&self, request: &TileRequest) -> anyhow::Result<Image> {
        let side = 256 * u32::from(request.scale.clamp(1, 4));
        let mut pixmap = Pixmap::new(side, side)
            .ok_or_else(|| anyhow::anyhow!("a tile cannot be that large"))?;
        let Palette {
            land,
            water,
            border,
        } = request.palette;
        let color = |c: [u8; 4]| tiny_skia::Color::from_rgba8(c[0], c[1], c[2], c[3]);
        pixmap.fill(color(water));

        let n = f64::from(1u32 << request.z.min(30));
        let side_f = f64::from(side);
        // World to tile pixels.
        let to_px = |p: [f32; 2]| {
            (
                (f64::from(p[0]) * n - f64::from(request.x)) * side_f,
                (f64::from(p[1]) * n - f64::from(request.y)) * side_f,
            )
        };
        // What the tile covers, in world units, with a margin so the edge of
        // a shape does not show at the seam between tiles.
        let margin = 2. / (n * side_f);
        let area = [
            (f64::from(request.x) / n - margin) as f32,
            (f64::from(request.y) / n - margin) as f32,
            (f64::from(request.x + 1) / n + margin) as f32,
            (f64::from(request.y + 1) / n + margin) as f32,
        ];
        let touches = |ring: &Ring| {
            ring.bbox[0] <= area[2]
                && ring.bbox[2] >= area[0]
                && ring.bbox[1] <= area[3]
                && ring.bbox[3] >= area[1]
        };
        let clip = (-2., -2., side_f + 2., side_f + 2.);
        let fine = request.z >= FINE_FROM;
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        for layer in self.layers.iter().filter(|l| l.fine == fine) {
            let mut path = PathBuilder::new();
            for ring in layer.rings.iter().filter(|r| touches(r)) {
                let points: Vec<(f64, f64)> = ring.points.iter().map(|&p| to_px(p)).collect();
                if layer.kind == Kind::Borders {
                    for run in clip_line(&points, clip) {
                        path.move_to(run[0].0 as f32, run[0].1 as f32);
                        for p in &run[1..] {
                            path.line_to(p.0 as f32, p.1 as f32);
                        }
                    }
                } else {
                    let ring = clip_polygon(&points, clip);
                    if ring.len() > 2 {
                        path.move_to(ring[0].0 as f32, ring[0].1 as f32);
                        for p in &ring[1..] {
                            path.line_to(p.0 as f32, p.1 as f32);
                        }
                        path.close();
                    }
                }
            }
            let Some(path) = path.finish() else {
                continue;
            };
            match layer.kind {
                Kind::Land | Kind::Lakes => {
                    paint.set_color(color(if layer.kind == Kind::Land {
                        land
                    } else {
                        water
                    }));
                    pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::EvenOdd,
                        Transform::identity(),
                        None,
                    );
                }
                Kind::Borders => {
                    paint.set_color(color(border));
                    let stroke = Stroke {
                        width: f32::from(request.scale.clamp(1, 4)),
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
        }
        Ok(Image {
            width: side,
            height: side,
            // Every pixel is opaque (the water under it all), so the
            // premultiplied pixels are the straight ones.
            rgba: Arc::from(pixmap.take()),
        })
    }
}

/// A polygon clipped to a rectangle (Sutherland–Hodgman): what of it lies
/// inside, closed along the rectangle's edges.
fn clip_polygon(points: &[(f64, f64)], (x0, y0, x1, y1): (f64, f64, f64, f64)) -> Vec<(f64, f64)> {
    let mut ring = points.to_vec();
    let edges: [(
        &dyn Fn((f64, f64)) -> bool,
        &dyn Fn((f64, f64), (f64, f64)) -> (f64, f64),
    ); 4] = [
        (&|p| p.0 >= x0, &|a, b| cross_x(a, b, x0)),
        (&|p| p.0 <= x1, &|a, b| cross_x(a, b, x1)),
        (&|p| p.1 >= y0, &|a, b| cross_y(a, b, y0)),
        (&|p| p.1 <= y1, &|a, b| cross_y(a, b, y1)),
    ];
    for (inside, cross) in edges {
        let Some(&last) = ring.last() else {
            break;
        };
        let mut out = Vec::with_capacity(ring.len());
        let mut previous = last;
        for &p in &ring {
            match (inside(previous), inside(p)) {
                (true, true) => out.push(p),
                (true, false) => out.push(cross(previous, p)),
                (false, true) => {
                    out.push(cross(previous, p));
                    out.push(p);
                }
                (false, false) => {}
            }
            previous = p;
        }
        ring = out;
    }
    ring
}

fn cross_x(a: (f64, f64), b: (f64, f64), x: f64) -> (f64, f64) {
    let t = (x - a.0) / (b.0 - a.0);
    (x, a.1 + t * (b.1 - a.1))
}

fn cross_y(a: (f64, f64), b: (f64, f64), y: f64) -> (f64, f64) {
    let t = (y - a.1) / (b.1 - a.1);
    (a.0 + t * (b.0 - a.0), y)
}

/// A line clipped to a rectangle (Liang–Barsky, segment by segment): the
/// runs of it inside.
fn clip_line(
    points: &[(f64, f64)],
    (x0, y0, x1, y1): (f64, f64, f64, f64),
) -> Vec<Vec<(f64, f64)>> {
    let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut run: Vec<(f64, f64)> = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let (mut t0, mut t1) = (0f64, 1f64);
        let mut visible = true;
        for (p, q) in [
            (-dx, a.0 - x0),
            (dx, x1 - a.0),
            (-dy, a.1 - y0),
            (dy, y1 - a.1),
        ] {
            if p == 0. {
                if q < 0. {
                    visible = false;
                    break;
                }
            } else {
                let t = q / p;
                if p < 0. {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
        if !visible || t0 > t1 {
            if run.len() > 1 {
                runs.push(std::mem::take(&mut run));
            }
            run.clear();
            continue;
        }
        let start = (a.0 + t0 * dx, a.1 + t0 * dy);
        let end = (a.0 + t1 * dx, a.1 + t1 * dy);
        if run.last() != Some(&start) {
            if run.len() > 1 {
                runs.push(std::mem::take(&mut run));
            }
            run.clear();
            run.push(start);
        }
        run.push(end);
        if t1 < 1. {
            runs.push(std::mem::take(&mut run));
        }
    }
    if run.len() > 1 {
        runs.push(run);
    }
    runs
}

#[cfg(test)]
// Pixels and clipped coordinates here are exact in binary floating point.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    const LAND: [u8; 4] = [10, 200, 10, 255];
    const WATER: [u8; 4] = [10, 10, 200, 255];

    fn request(z: u8, x: u32, y: u32) -> TileRequest {
        TileRequest {
            z,
            x,
            y,
            scale: 1,
            palette: Palette {
                land: LAND,
                water: WATER,
                border: [0, 0, 0, 255],
            },
        }
    }

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.width + x) * 4) as usize;
        image.rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn the_packed_data_reads_back() {
        let outline = Outline::load().unwrap();
        assert_eq!(outline.layers.len(), 6);
        let points: usize = outline
            .layers
            .iter()
            .flat_map(|l| &l.rings)
            .map(|r| r.points.len())
            .sum();
        assert!(points > 100_000, "{points}");
    }

    #[test]
    fn land_is_land_and_the_sea_is_water() {
        let outline = Outline::load().unwrap();
        let world = outline.tile(&request(0, 0, 0)).unwrap();
        assert_eq!((world.width, world.height), (256, 256));
        // The middle of the Pacific, and the middle of Asia.
        assert_eq!(pixel(&world, 5, 128), WATER);
        assert_eq!(pixel(&world, 200, 80), LAND);
        // Deep inside a continent at zoom 10 (central Russia, near 60°N
        // 90°E): the whole tile is land, drawn from a clipped ring.
        let n = 1u32 << 10;
        let x = ((90. + 180.) / 360. * f64::from(n)) as u32;
        let lat = 60f64.to_radians();
        let y = ((1. - (lat.tan() + 1. / lat.cos()).ln() / std::f64::consts::PI) / 2.
            * f64::from(n)) as u32;
        let inland = outline.tile(&request(10, x, y)).unwrap();
        assert_eq!(pixel(&inland, 0, 0), LAND);
        assert_eq!(pixel(&inland, 128, 128), LAND);
    }

    #[test]
    fn a_polygon_is_cut_to_the_rectangle() {
        let square = [(-10., -10.), (10., -10.), (10., 10.), (-10., 10.)];
        let clipped = clip_polygon(&square, (0., 0., 5., 5.));
        let mut xs: Vec<_> = clipped.iter().map(|p| p.0).collect();
        xs.sort_by(f64::total_cmp);
        assert_eq!((xs[0], xs[xs.len() - 1]), (0., 5.));
        assert!(clip_polygon(&square, (20., 20., 30., 30.)).is_empty());
    }

    #[test]
    fn a_line_is_cut_into_the_runs_inside() {
        let line = [(-5., 1.), (5., 1.), (15., 1.), (15., 3.), (5., 3.)];
        let runs = clip_line(&line, (0., 0., 10., 10.));
        assert_eq!(
            runs,
            [
                vec![(0., 1.), (5., 1.), (10., 1.)],
                vec![(10., 3.), (5., 3.)]
            ]
        );
    }
}
