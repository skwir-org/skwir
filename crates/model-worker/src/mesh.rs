//! A mesh read from any of the formats: positions and the triangles
//! between them, in one frame of reference with Z up, and how many objects
//! it was made of. Bounded as it is read: past the triangles allowed, the
//! read stops with a reason rather than draw a part of the model.
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};
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
    /// Whether no two positions are the same: an STL's are made one as
    /// they are read, and need not be again to tell whether it is closed.
    welded: bool,
    most: usize,
}

/// A position as its bits, which make two positions one when they are
/// the same.
#[derive(PartialEq, Eq)]
struct Key([u32; 3]);

impl Hash for Key {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let [x, y, z] = self.0;
        state.write_u64(
            (u64::from(x) << 32 | u64::from(y))
                ^ u64::from(z)
                    .rotate_left(23)
                    .wrapping_mul(0x9e37_79b9_7f4a_7c15),
        );
    }
}

/// What `Key` writes, mixed (`SplitMix64`'s finalizer): std's `SipHash`
/// guards against chosen collisions, which only slow a model down, at a
/// third of the read's time.
#[derive(Default)]
struct Mix(u64);

impl Hasher for Mix {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.write_u64(u64::from(*byte));
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = self.0.rotate_left(5) ^ value;
    }

    fn finish(&self) -> u64 {
        let mut x = self.0;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^ (x >> 31)
    }
}

/// Positions made one where their bits are the same, numbered in the order
/// first seen.
struct Welder {
    ids: HashMap<Key, u32, BuildHasherDefault<Mix>>,
    points: Vec<Point>,
}

impl Key {
    fn of(p: Point) -> Self {
        Key(p.map(|v| (v + 0.).to_bits()))
    }

    fn hash(&self) -> u64 {
        let mut mix = Mix::default();
        Hash::hash(self, &mut mix);
        mix.finish()
    }
}

impl Welder {
    fn with_capacity(points: usize) -> Self {
        Welder {
            ids: HashMap::with_capacity_and_hasher(points, BuildHasherDefault::default()),
            points: Vec::with_capacity(points),
        }
    }

    /// `p`'s number, a new one if it was not seen.
    fn id(&mut self, p: Point) -> u32 {
        let next = u32::try_from(self.points.len()).unwrap_or(u32::MAX);
        let id = *self.ids.entry(Key::of(p)).or_insert(next);
        if id == next {
            self.points.push(p);
        }
        id
    }
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

    /// The same mesh with only the positions its triangles use, in the
    /// order they are first used.
    pub fn compacted(self) -> Mesh {
        let mut renumbered = vec![u32::MAX; self.positions.len()];
        let mut positions = Vec::new();
        let triangles = self
            .triangles
            .iter()
            .map(|t| {
                t.map(|i| {
                    let new = &mut renumbered[i as usize];
                    if *new == u32::MAX {
                        *new = u32::try_from(positions.len()).unwrap_or(u32::MAX);
                        positions.push(self.positions[i as usize]);
                    }
                    *new
                })
            })
            .collect();
        Mesh {
            positions,
            triangles,
            ..self
        }
    }

    /// The positions and triangles as `--mesh` writes them: their counts,
    /// then each, little-endian.
    pub fn write(&self, out: &mut impl std::io::Write) -> std::io::Result<()> {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX).to_le_bytes();
        out.write_all(&count(self.positions.len()))?;
        out.write_all(&count(self.triangles.len()))?;
        for p in &self.positions {
            for v in p {
                out.write_all(&v.to_le_bytes())?;
            }
        }
        for t in &self.triangles {
            for i in t {
                out.write_all(&i.to_le_bytes())?;
            }
        }
        Ok(())
    }

    /// Whether every edge joins exactly two triangles, the points welded
    /// where they are the same, and if so the volume enclosed.
    pub fn closed_volume(&self) -> Option<f64> {
        let welded: Option<Vec<u32>> = (!self.welded).then(|| {
            let mut welder = Welder::with_capacity(self.positions.len());
            self.positions.iter().map(|p| welder.id(*p)).collect()
        });
        let id = |i: u32| welded.as_ref().map_or(i, |welded| welded[i as usize]);
        // Each triangle's edges, an edge as one number, its smaller end
        // first, skipping triangles that are a line or a point.
        let edges_of = |t: &[u32; 3]| {
            let [a, b, c] = t.map(id);
            let flat = a == b || b == c || a == c;
            [(a, b), (b, c), (c, a)]
                .map(|(from, to)| u64::from(from.min(to)) << 32 | u64::from(from.max(to)))
                .into_iter()
                .filter(move |_| !flat)
        };
        // Grouped by their smaller end, so an edge's copies share a group,
        // and each group sorted and counted on a thread of its own.
        let points = welded
            .as_ref()
            .map_or(self.positions.len(), Vec::len)
            .max(1) as u64;
        let groups = std::thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .clamp(1, 16);
        let group = |edge: u64| ((edge >> 32) * groups as u64 / points) as usize;
        let mut counts = vec![0usize; groups];
        for edge in self.triangles.iter().flat_map(edges_of) {
            counts[group(edge)] += 1;
        }
        if counts.iter().all(|n| *n == 0) {
            return None;
        }
        let mut starts: Vec<usize> = counts
            .iter()
            .scan(0, |at, n| {
                let start = *at;
                *at += n;
                Some(start)
            })
            .collect();
        let mut edges = vec![0u64; counts.iter().sum()];
        for edge in self.triangles.iter().flat_map(edges_of) {
            let at = &mut starts[group(edge)];
            edges[*at] = edge;
            *at += 1;
        }
        // Each edge in exactly two triangles.
        let paired = |group: &mut [u64]| {
            group.sort_unstable();
            group
                .chunk_by(|a, b| a == b)
                .all(|copies| copies.len() == 2)
        };
        let mut rest = edges.as_mut_slice();
        let closed = std::thread::scope(|scope| {
            let mut running = Vec::with_capacity(groups);
            for n in counts {
                let (group, after) = rest.split_at_mut(n);
                rest = after;
                running.push(scope.spawn(move || paired(group)));
            }
            running
                .into_iter()
                .all(|thread| thread.join().unwrap_or(false))
        });
        drop(edges);
        if !closed {
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
    // As long as it says, so it is read in one go; what it says is only a
    // start, the read stops past `most` all the same.
    let said = file.metadata().map_or(0, |m| m.len()).min(most + 1);
    let mut bytes = Vec::with_capacity(usize::try_from(said).unwrap_or(0));
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading the file: {e}"))?;
    if bytes.len() as u64 > most {
        return Err(format!("the file is larger than {most} bytes"));
    }
    Ok(bytes)
}

/// STL, ASCII or binary. A binary file says its triangles after a header of
/// 80 bytes, then is 50 bytes a triangle: one that is that long is binary,
/// whatever its header says (some begin it `solid`, as ASCII ones do).
pub fn stl(bytes: &[u8], mesh: &mut Mesh) -> Result<(), String> {
    if let Some(count) = bytes.get(80..84) {
        let count = u32::from_le_bytes([count[0], count[1], count[2], count[3]]) as usize;
        let binary = !bytes.starts_with(b"solid")
            || count.checked_mul(50).and_then(|n| n.checked_add(84)) == Some(bytes.len());
        if binary {
            if count > mesh.most {
                return Err(mesh.too_many());
            }
            return binary_stl(&bytes[84..], count, mesh);
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
    mesh.add(&points, triangles, &IDENTITY)?;
    // stl_io made the same positions one.
    mesh.welded = true;
    Ok(())
}

/// A binary STL's `count` triangles, from `body`: each a normal (not read),
/// three corners and two bytes of attributes, little-endian. Corners that
/// are the same are made one as they are read.
fn binary_stl(body: &[u8], count: usize, mesh: &mut Mesh) -> Result<(), String> {
    let faces = body
        .get(..count * 50)
        .ok_or("the STL model ends early")?
        .as_chunks::<50>()
        .0;
    let corner = |c: usize| {
        let face = &faces[c / 3];
        let at = |i: usize| f32::from_le_bytes([face[i], face[i + 1], face[i + 2], face[i + 3]]);
        let start = 12 + c % 3 * 12;
        [at(start), at(start + 4), at(start + 8)]
    };
    let (positions, ids) = weld(count * 3, corner);
    mesh.objects = 1;
    mesh.positions = positions;
    mesh.triangles = ids.as_chunks::<3>().0.to_vec();
    mesh.welded = true;
    Ok(())
}

/// `corners` points (`corner` gives each) made one where they are the same:
/// the points, and each corner's number among them. Shared out between
/// threads by each point's hash, so the same point always goes to the same
/// thread, which numbers its own; the numbers are in no particular order
/// (`Mesh::compacted` puts them in the order first used).
fn weld(corners: usize, corner: impl Fn(usize) -> Point + Sync) -> (Vec<Point>, Vec<u32>) {
    use std::sync::atomic::{AtomicU32, Ordering};
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .clamp(1, 16);
    // Small meshes, or one thread: in one go, in order.
    if threads == 1 || corners < 300_000 {
        let mut welder = Welder::with_capacity(corners / 6 + 3);
        let ids = (0..corners).map(|c| welder.id(corner(c))).collect();
        return (welder.points, ids);
    }
    // Which thread welds each corner, found once.
    let mut shard = vec![0u8; corners];
    let span = corners.div_ceil(threads);
    std::thread::scope(|scope| {
        for (part, slice) in shard.chunks_mut(span).enumerate() {
            let corner = &corner;
            scope.spawn(move || {
                for (i, to) in slice.iter_mut().enumerate() {
                    let key = Key::of(corner(part * span + i));
                    *to = (key.hash() % threads as u64) as u8;
                }
            });
        }
    });
    // Each thread's own points, and every corner's number among them.
    let local: Vec<AtomicU32> = (0..corners).map(|_| AtomicU32::new(0)).collect();
    let welded: Vec<Vec<Point>> = std::thread::scope(|scope| {
        let running: Vec<_> = (0..threads)
            .map(|me| {
                let (shard, local, corner) = (&shard, &local, &corner);
                scope.spawn(move || {
                    let mut welder = Welder::with_capacity(corners / 6 / threads + 3);
                    for (c, owner) in shard.iter().enumerate() {
                        if usize::from(*owner) == me {
                            local[c].store(welder.id(corner(c)), Ordering::Relaxed);
                        }
                    }
                    welder.points
                })
            })
            .collect();
        running
            .into_iter()
            .map(|thread| thread.join().unwrap_or_default())
            .collect()
    });
    // A thread's points follow the threads' before it.
    let firsts: Vec<u32> = welded
        .iter()
        .scan(0u32, |at, points| {
            let first = *at;
            *at += u32::try_from(points.len()).unwrap_or(u32::MAX);
            Some(first)
        })
        .collect();
    let ids = shard
        .iter()
        .zip(&local)
        .map(|(owner, id)| firsts[usize::from(*owner)] + id.load(Ordering::Relaxed))
        .collect();
    (welded.concat(), ids)
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
