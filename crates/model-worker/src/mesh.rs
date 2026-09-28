//! A mesh read from any of the formats: positions and the triangles
//! between them, in one frame of reference with Z up, and how many objects
//! it was made of. Bounded as it is read: past the triangles allowed, the
//! read stops with a reason rather than draw a part of the model.
use std::path::Path;

pub type Point = [f32; 3];

#[derive(Default)]
pub struct Mesh {
    pub positions: Vec<Point>,
    pub triangles: Vec<[u32; 3]>,
    /// The objects the file places (a 3MF build item, a glTF mesh node).
    pub objects: usize,
    /// The unit the file states its lengths in.
    pub unit: Option<&'static str>,
    most: usize,
}

/// A 4 × 3 affine transform, as 3MF writes one (rows: the images of X, Y
/// and Z, then the translation).
pub type Transform = [[f32; 3]; 4];

pub const IDENTITY: Transform = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]];

pub fn apply(t: &Transform, p: Point) -> Point {
    let mut out = t[3];
    for (axis, value) in p.iter().enumerate() {
        for (o, row) in out.iter_mut().zip(t[axis]) {
            *o += value * row;
        }
    }
    out
}

/// `inner` then `outer`.
pub fn compose(inner: &Transform, outer: &Transform) -> Transform {
    let mut out = [[0.; 3]; 4];
    for axis in 0..3 {
        let image = apply(outer, inner[axis]);
        let origin = outer[3];
        out[axis] = [
            image[0] - origin[0],
            image[1] - origin[1],
            image[2] - origin[2],
        ];
    }
    out[3] = apply(outer, inner[3]);
    out
}

impl Mesh {
    pub fn new(most: usize) -> Self {
        Mesh {
            most,
            ..Mesh::default()
        }
    }

    /// The triangles allowed.
    pub fn most(&self) -> usize {
        self.most
    }

    /// Points the mesh may still take: three for each triangle it may.
    pub fn room(&self) -> usize {
        self.most
            .saturating_mul(3)
            .saturating_sub(self.positions.len())
    }

    /// The error of a model past the triangles allowed.
    pub fn too_many(&self) -> String {
        format!("the model has more than {} triangles", self.most)
    }

    /// Positions and the triangles between them (indices into `points`),
    /// placed by `transform`. Triangles naming a missing point are left out.
    pub fn add(
        &mut self,
        points: &[Point],
        triangles: impl IntoIterator<Item = [u32; 3]>,
        transform: &Transform,
    ) -> Result<(), String> {
        let base = u32::try_from(self.positions.len()).map_err(|_| "too many points")?;
        let count = u32::try_from(points.len()).map_err(|_| "too many points")?;
        self.positions
            .extend(points.iter().map(|p| apply(transform, *p)));
        for [a, b, c] in triangles {
            if a >= count || b >= count || c >= count {
                continue;
            }
            if self.triangles.len() == self.most {
                return Err(format!("the model has more than {} triangles", self.most));
            }
            self.triangles.push([base + a, base + b, base + c]);
        }
        Ok(())
    }

    /// Turn a model whose Y is up (glTF, most OBJ) to stand on Z.
    pub fn y_up(&mut self) {
        for p in &mut self.positions {
            *p = [p[0], -p[2], p[1]];
        }
    }

    /// The smallest box around the triangles' points.
    pub fn bounds(&self) -> Option<(Point, Point)> {
        let mut used = self
            .triangles
            .iter()
            .flatten()
            .map(|&i| self.positions[i as usize]);
        let first = used.next()?;
        Some(used.fold((first, first), |(mut lo, mut hi), p| {
            for axis in 0..3 {
                lo[axis] = lo[axis].min(p[axis]);
                hi[axis] = hi[axis].max(p[axis]);
            }
            (lo, hi)
        }))
    }

    /// Whether every edge joins exactly two triangles, the points welded
    /// where they are the same, and if so the volume enclosed.
    pub fn closed_volume(&self) -> Option<f64> {
        use std::collections::HashMap;
        let mut ids: HashMap<[u32; 3], u32> = HashMap::with_capacity(self.positions.len());
        let welded: Vec<u32> = self
            .positions
            .iter()
            .map(|p| {
                let key = p.map(|v| (v + 0.).to_bits());
                let next = u32::try_from(ids.len()).unwrap_or(u32::MAX);
                *ids.entry(key).or_insert(next)
            })
            .collect();
        let mut edges: Vec<(u32, u32)> = Vec::with_capacity(self.triangles.len() * 3);
        for t in &self.triangles {
            let [a, b, c] = t.map(|i| welded[i as usize]);
            if a == b || b == c || a == c {
                continue;
            }
            for (from, to) in [(a, b), (b, c), (c, a)] {
                edges.push((from.min(to), from.max(to)));
            }
        }
        if edges.is_empty() {
            return None;
        }
        edges.sort_unstable();
        let mut run = 1;
        for pair in edges.windows(2) {
            if pair[0] == pair[1] {
                run += 1;
            } else {
                if run != 2 {
                    return None;
                }
                run = 1;
            }
        }
        if run != 2 {
            return None;
        }
        let volume: f64 = self
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.positions[i as usize].map(f64::from));
                a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0])
            })
            .sum::<f64>()
            / 6.;
        Some(volume.abs())
    }
}

/// The file's bytes, if they are no more than `most`.
pub fn read_bounded(path: &Path, most: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("opening the file: {e}"))?;
    let mut bytes = Vec::new();
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading the file: {e}"))?;
    if bytes.len() as u64 > most {
        return Err(format!("the file is larger than {most} bytes"));
    }
    Ok(bytes)
}

/// STL, ASCII or binary; a binary file says its triangles first.
pub fn stl(bytes: &[u8], mesh: &mut Mesh) -> Result<(), String> {
    if bytes.len() >= 84 && !bytes.starts_with(b"solid") {
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if count > mesh.most {
            return Err(format!("the model has more than {} triangles", mesh.most));
        }
    }
    let read = stl_io::read_stl(&mut std::io::Cursor::new(bytes))
        .map_err(|e| format!("not an STL model: {e}"))?;
    let points: Vec<Point> = read.vertices.iter().map(|v| [v[0], v[1], v[2]]).collect();
    let triangles = read
        .faces
        .iter()
        .map(|f| f.vertices.map(|i| u32::try_from(i).unwrap_or(u32::MAX)));
    mesh.objects = 1;
    mesh.add(&points, triangles, &IDENTITY)
}

/// OBJ, its faces cut into triangles; the materials it names are not read.
pub fn obj(bytes: &[u8], mesh: &mut Mesh) -> Result<(), String> {
    let options = tobj::LoadOptions {
        triangulate: true,
        single_index: false,
        ignore_points: true,
        ignore_lines: true,
    };
    let (models, _) = tobj::load_obj_buf(&mut std::io::Cursor::new(bytes), &options, |_| {
        Err(tobj::LoadError::OpenFileFailed)
    })
    .map_err(|e| format!("not an OBJ model: {e}"))?;
    for model in &models {
        let points: Vec<Point> = model.mesh.positions.as_chunks::<3>().0.to_vec();
        let triangles = model.mesh.indices.as_chunks::<3>().0.iter().copied();
        mesh.add(&points, triangles, &IDENTITY)?;
    }
    mesh.objects = models.len();
    mesh.y_up();
    Ok(())
}

/// OFF: `OFF`, the counts, the points, then each face as its number of
/// points and their indices, cut into a fan of triangles.
pub fn off(bytes: &[u8], mesh: &mut Mesh) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "not an OFF model")?;
    // A line at a time: a point or a face may carry a colour after it.
    let mut lines = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty());
    let first = lines.next().ok_or("not an OFF model")?;
    let mut header = first.split_whitespace();
    if !header.next().is_some_and(|magic| magic.ends_with("OFF")) {
        return Err("not an OFF model".into());
    }
    // The counts follow the magic on its line, or on the next.
    let counts: Vec<&str> = match header.collect::<Vec<_>>() {
        rest if rest.is_empty() => lines
            .next()
            .ok_or("the OFF model's counts are missing")?
            .split_whitespace()
            .collect(),
        rest => rest,
    };
    let count = |at: usize| -> Result<usize, String> {
        counts
            .get(at)
            .and_then(|w| w.parse().ok())
            .ok_or_else(|| "the OFF model's counts are missing".into())
    };
    let (points, faces) = (count(0)?, count(1)?);
    if points > mesh.most.saturating_mul(3) || faces > mesh.most {
        return Err(format!("the model has more than {} triangles", mesh.most));
    }
    let numbers = |line: &str| -> Vec<f64> {
        line.split_whitespace()
            .map_while(|w| w.parse().ok())
            .collect()
    };
    let mut positions = Vec::with_capacity(points);
    for _ in 0..points {
        let n = numbers(lines.next().ok_or("the OFF model ends early")?);
        if n.len() < 3 {
            return Err("an OFF point has fewer than three coordinates".into());
        }
        positions.push([n[0] as f32, n[1] as f32, n[2] as f32]);
    }
    let mut triangles = Vec::new();
    for _ in 0..faces {
        let n = numbers(lines.next().ok_or("the OFF model ends early")?);
        let corners = n.first().map_or(0, |c| *c as usize);
        if n.len() <= corners {
            return Err("an OFF face lists fewer corners than it says".into());
        }
        let at = |i: usize| n[1 + i] as u32;
        for i in 1..corners.saturating_sub(1) {
            triangles.push([at(0), at(i), at(i + 1)]);
        }
        if triangles.len() > mesh.most {
            return Err(mesh.too_many());
        }
    }
    mesh.objects = 1;
    mesh.add(&positions, triangles, &IDENTITY)
}
