//! Triangle meshes drawn in 3D by the GPU, through [`crate::Window::paint_mesh`].

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use futures::channel::oneshot;

use crate::{DevicePixels, PaintMesh, Size};

/// A mesh's identity: a renderer keeps the mesh's buffers by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshId(pub u64);

/// Positions and the triangles between them, ready to draw: every index
/// names a position, and every coordinate is finite. A renderer uploads
/// them once and keeps them while frames draw the mesh; the positions and
/// indices stay here too, so a device lost and found again can upload them
/// anew.
pub struct Mesh {
    id: MeshId,
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
    /// A colour for each position (sRGB bytes, red, green, blue, alpha),
    /// drawn in place of `MeshView::color`.
    colors: Option<Vec<[u8; 4]>>,
}

/// Why a mesh could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// No triangle at all.
    Empty,
    /// The indices are not a whole number of triangles.
    Incomplete,
    /// An index names no position.
    Index {
        /// The index.
        index: u32,
        /// How many positions there are.
        positions: usize,
    },
    /// A coordinate is infinite or not a number.
    NotFinite {
        /// The position holding it.
        position: usize,
    },
    /// Not a colour for each position.
    Colors {
        /// How many colours there are.
        colors: usize,
        /// How many positions there are.
        positions: usize,
    },
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshError::Empty => write!(f, "the mesh has no triangles"),
            MeshError::Incomplete => write!(f, "the mesh's indices are not whole triangles"),
            MeshError::Index { index, positions } => write!(
                f,
                "the mesh's index {index} names no position (it has {positions})"
            ),
            MeshError::NotFinite { position } => {
                write!(f, "the mesh's position {position} is not finite")
            }
            MeshError::Colors { colors, positions } => {
                write!(f, "the mesh has {colors} colors for {positions} positions")
            }
        }
    }
}

impl std::error::Error for MeshError {}

impl Mesh {
    /// The triangles `indices` names, three indices each, between
    /// `positions`.
    pub fn new(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Result<Self, MeshError> {
        if indices.is_empty() {
            return Err(MeshError::Empty);
        }
        if indices.len() % 3 != 0 {
            return Err(MeshError::Incomplete);
        }
        let count = positions.len();
        if let Some(&index) = indices.iter().find(|&&i| i as usize >= count) {
            return Err(MeshError::Index {
                index,
                positions: count,
            });
        }
        if let Some(position) = positions
            .iter()
            .position(|p| !p.iter().all(|v| v.is_finite()))
        {
            return Err(MeshError::NotFinite { position });
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Ok(Mesh {
            id: MeshId(NEXT.fetch_add(1, Ordering::Relaxed)),
            positions,
            indices,
            colors: None,
        })
    }

    /// The same mesh with a color for each position, sRGB bytes.
    pub fn with_colors(self, colors: Vec<[u8; 4]>) -> Result<Self, MeshError> {
        if colors.len() != self.positions.len() {
            return Err(MeshError::Colors {
                colors: colors.len(),
                positions: self.positions.len(),
            });
        }
        Ok(Mesh {
            colors: Some(colors),
            ..self
        })
    }

    /// Its positions' colors, if it has some.
    pub fn colors(&self) -> Option<&[[u8; 4]]> {
        self.colors.as_deref()
    }

    /// Its identity.
    pub fn id(&self) -> MeshId {
        self.id
    }

    /// Its positions.
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// Its triangles, three indices into the positions each.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// How many triangles it has.
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// What its buffers take, in bytes: the positions, the indices and the
    /// colors.
    pub fn size_in_bytes(&self) -> u64 {
        (std::mem::size_of_val(self.positions.as_slice())
            + std::mem::size_of_val(self.indices.as_slice())
            + self.colors.as_ref().map_or(0, |colors| colors.len() * 4)) as u64
    }

    /// A normal for each position, for smooth shading: the normals of the
    /// triangles around it, each weighted by its area, averaged. A
    /// position no triangle with an area touches gets straight up (Z).
    pub fn smooth_normals(&self) -> Vec<[f32; 3]> {
        let mut sums = vec![[0f32; 3]; self.positions.len()];
        for t in self.indices.chunks_exact(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| self.positions[i as usize]);
            let (u, v) = (sub(b, a), sub(c, a));
            // Twice the triangle's area along its normal.
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            for &i in t {
                let sum = &mut sums[i as usize];
                for axis in 0..3 {
                    sum[axis] += n[axis];
                }
            }
        }
        for sum in &mut sums {
            let length = (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt();
            *sum = if length > 0. && length.is_finite() {
                sum.map(|v| v / length)
            } else {
                [0., 0., 1.]
            };
        }
        sums
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

impl fmt::Debug for Mesh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mesh")
            .field("id", &self.id)
            .field("positions", &self.positions.len())
            .field("triangles", &self.triangles())
            .finish()
    }
}

/// Where a mesh is on its way to a window's GPU
/// (`PlatformWindow::prepare_mesh`). Its buffers are made and filled off the
/// main thread: a large mesh takes long to copy where the driver maps
/// memory uncached.
pub enum MeshPreparation {
    /// It can be drawn.
    Ready,
    /// It is being uploaded from now on: the receiver resolves when it can
    /// be drawn, and is dropped if it never will be (nothing wanted it any
    /// more, the renderer went).
    Started(oneshot::Receiver<()>),
    /// It is being uploaded, since an earlier ask.
    Pending,
    /// It can be drawn, but shaded flat while the normals smooth shading
    /// asked for are made: on the first ask, the receiver resolves when
    /// they are there.
    Flat(Option<oneshot::Receiver<()>>),
    /// It will not be drawn: the window draws no meshes, or this one is
    /// larger than its GPU's buffers, or they could not be made.
    Never,
}

impl MeshPreparation {
    /// An upload starting now, and what tells the window it is done.
    pub fn start() -> (MeshPreparation, MeshUploaded) {
        let (sender, receiver) = oneshot::channel();
        (MeshPreparation::Started(receiver), MeshUploaded(sender))
    }

    /// Normals being made now for a mesh drawn flat meanwhile, and what
    /// tells the window they are there.
    pub fn flat_until() -> (MeshPreparation, MeshUploaded) {
        let (sender, receiver) = oneshot::channel();
        (MeshPreparation::Flat(Some(receiver)), MeshUploaded(sender))
    }
}

/// Tells the window that asked for a mesh that it can be drawn. Dropped
/// untold, it never will be.
pub struct MeshUploaded(oneshot::Sender<()>);

impl MeshUploaded {
    /// It can be drawn.
    pub fn done(self) {
        self.0.send(()).ok();
    }
}

/// A renderer's part of a [`MeshStore`]: how its GPU holds a mesh. Cloned
/// onto the store's thread, which makes the buffers there.
pub trait MeshMaker: Clone + Send + 'static {
    /// A mesh's buffers: its positions, its indices, its colors if any.
    type Buffers: Send + 'static;
    /// A mesh's normals' buffer, for smooth shading.
    type Normals: Send + 'static;
    /// Whether the GPU can hold `mesh`, its buffers within its limits.
    fn holds(&self, mesh: &Mesh) -> bool;
    /// `mesh`'s buffers, made and filled; `None` when the GPU could not
    /// make them (out of memory, the device lost).
    fn buffers(&self, mesh: &Mesh) -> Option<Self::Buffers>;
    /// A buffer of `normals`, one a position, made and filled; `None` when
    /// the GPU could not make it.
    fn normals(&self, normals: &[[f32; 3]]) -> Option<Self::Normals>;
}

/// The meshes a renderer keeps for the frames that draw them. A mesh is
/// uploaded on a thread of the store's own the first time a window asks
/// for it ([`MeshStore::prepare`]), never on the window's: where a driver
/// maps the memory uploads go through uncached, 72 MB took a second. Its
/// normals are made there too, only once smooth shading asks. What the
/// frame being drawn does not draw is let go of ([`MeshStore::end_frame`]).
pub struct MeshStore<M: MeshMaker> {
    maker: M,
    stored: collections::FxHashMap<MeshId, Stored<M>>,
    uploader: Option<Uploader<M>>,
    /// The meshes being uploaded, and those whose normals are being made.
    uploading: collections::FxHashSet<MeshId>,
    smoothing: collections::FxHashSet<MeshId>,
    /// The last mesh refused: too large for the GPU (said once), or its
    /// buffers could not be made. It is not asked for again.
    refused: Option<MeshId>,
}

struct Stored<M: MeshMaker> {
    buffers: M::Buffers,
    normals: Normals<M::Normals>,
    /// Whether the frame being drawn draws it.
    drawn: bool,
}

/// A stored mesh's normals.
enum Normals<N> {
    /// Not asked for yet.
    Unasked,
    Made(N),
    /// Asked for, but the GPU could not hold them: the mesh stays flat.
    Failed,
}

impl<N> Normals<N> {
    fn from(made: Option<N>) -> Self {
        made.map_or(Normals::Failed, Normals::Made)
    }

    fn get(&self) -> Option<&N> {
        match self {
            Normals::Made(normals) => Some(normals),
            Normals::Unasked | Normals::Failed => None,
        }
    }
}

/// What the store's thread is asked to make for a mesh, and what tells
/// the window that asked once it is made.
struct Upload {
    mesh: Arc<Mesh>,
    normals_only: bool,
    with_normals: bool,
    uploaded: MeshUploaded,
}

/// What the store's thread made.
enum Made<M: MeshMaker> {
    /// A mesh's buffers, and its normals when asked.
    Mesh(MeshId, M::Buffers, Normals<M::Normals>),
    /// A mesh's normals, asked for once it was there.
    Normals(MeshId, Normals<M::Normals>),
    /// Nothing, for a mesh no one held any more; whether only its normals
    /// were asked for.
    Unwanted(MeshId, bool),
    /// Nothing: the GPU could not make the mesh's buffers.
    Failed(MeshId),
}

/// The store's thread. It ends with the store, which waits for the upload
/// under way, if any, and skips those asked for after it: nothing of the
/// GPU's is made or let go of once the renderer is gone (under Wine, a
/// device let go of while the process exited had it killed).
struct Uploader<M: MeshMaker> {
    uploads: Option<mpsc::Sender<Upload>>,
    made: mpsc::Receiver<Made<M>>,
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl<M: MeshMaker> Drop for Uploader<M> {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.uploads = None;
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

impl<M: MeshMaker> Uploader<M> {
    fn start(maker: M) -> std::io::Result<Self> {
        let (uploads, asked) = mpsc::channel::<Upload>();
        let (made_by, made) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopped);
        let thread = std::thread::Builder::new()
            .name("gpui-meshes".into())
            .spawn(move || {
                for upload in asked {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    // Held by nothing else: no frame will draw it.
                    let wanted = Arc::strong_count(&upload.mesh) > 1;
                    let (mesh, id) = (&upload.mesh, upload.mesh.id());
                    let normals = || Normals::from(maker.normals(&mesh.smooth_normals()));
                    let made = if !wanted {
                        Made::Unwanted(id, upload.normals_only)
                    } else if upload.normals_only {
                        Made::Normals(id, normals())
                    } else {
                        match maker.buffers(mesh) {
                            Some(buffers) if upload.with_normals => {
                                Made::Mesh(id, buffers, normals())
                            }
                            Some(buffers) => Made::Mesh(id, buffers, Normals::Unasked),
                            None => Made::Failed(id),
                        }
                    };
                    let done = matches!(made, Made::Mesh(..) | Made::Normals(..));
                    // Handed over before the window hears of it, so the frame
                    // it draws finds it.
                    if made_by.send(made).is_err() {
                        break;
                    }
                    if done {
                        upload.uploaded.done();
                    }
                }
            })?;
        Ok(Uploader {
            uploads: Some(uploads),
            made,
            stopped,
            thread: Some(thread),
        })
    }
}

impl<M: MeshMaker> MeshStore<M> {
    /// Meshes held by `maker`'s GPU.
    pub fn new(maker: M) -> Self {
        Self {
            maker,
            stored: collections::FxHashMap::default(),
            uploader: None,
            uploading: collections::FxHashSet::default(),
            smoothing: collections::FxHashSet::default(),
            refused: None,
        }
    }

    /// Make `mesh` ready to draw, uploading it unless it is there or on its
    /// way, its normals with it when `smooth`. Asked by the window before it
    /// paints the mesh. A mesh there already, its normals asked for, is drawn
    /// flat until they are made.
    pub fn prepare(&mut self, mesh: &Arc<Mesh>, smooth: bool) -> MeshPreparation {
        self.take_made();
        let id = mesh.id();
        if let Some(stored) = self.stored.get(&id) {
            if !smooth || !matches!(stored.normals, Normals::Unasked) {
                return MeshPreparation::Ready;
            }
            if self.smoothing.contains(&id) {
                return MeshPreparation::Flat(None);
            }
            let (preparation, uploaded) = MeshPreparation::flat_until();
            return match self.send(mesh, true, true, uploaded) {
                Ok(()) => {
                    self.smoothing.insert(id);
                    preparation
                }
                Err(()) => MeshPreparation::Flat(None),
            };
        }
        if self.uploading.contains(&id) {
            return MeshPreparation::Pending;
        }
        if self.refused == Some(id) {
            return MeshPreparation::Never;
        }
        if !self.maker.holds(mesh) {
            if self.refused != Some(id) {
                self.refused = Some(id);
                log::warn!(
                    "a mesh of {} triangles is larger than this GPU's buffers",
                    mesh.triangles()
                );
            }
            return MeshPreparation::Never;
        }
        let (preparation, uploaded) = MeshPreparation::start();
        match self.send(mesh, false, smooth, uploaded) {
            Ok(()) => {
                self.uploading.insert(id);
                preparation
            }
            Err(()) => MeshPreparation::Never,
        }
    }

    /// `id`'s buffers and normals for the frame being drawn, which keeps
    /// them; `None` when it is not uploaded.
    pub fn drawn(&mut self, id: MeshId) -> Option<(&M::Buffers, Option<&M::Normals>)> {
        let stored = self.stored.get_mut(&id)?;
        stored.drawn = true;
        Some((&stored.buffers, stored.normals.get()))
    }

    /// `id`'s buffers and normals, if it is uploaded.
    pub fn get(&self, id: MeshId) -> Option<(&M::Buffers, Option<&M::Normals>)> {
        let stored = self.stored.get(&id)?;
        Some((&stored.buffers, stored.normals.get()))
    }

    /// After a frame: let go of the meshes it did not draw.
    pub fn end_frame(&mut self) {
        self.stored
            .retain(|_, stored| std::mem::take(&mut stored.drawn));
    }

    /// How many meshes it holds.
    pub fn len(&self) -> usize {
        self.stored.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.stored.is_empty()
    }

    /// Give the thread `mesh` to make, starting it with the first.
    fn send(
        &mut self,
        mesh: &Arc<Mesh>,
        normals_only: bool,
        with_normals: bool,
        uploaded: MeshUploaded,
    ) -> Result<(), ()> {
        if self.uploader.is_none() {
            match Uploader::start(self.maker.clone()) {
                Ok(uploader) => self.uploader = Some(uploader),
                Err(error) => {
                    log::error!("no thread to upload meshes: {error}");
                    return Err(());
                }
            }
        }
        let upload = Upload {
            mesh: Arc::clone(mesh),
            normals_only,
            with_normals,
            uploaded,
        };
        match self
            .uploader
            .as_ref()
            .and_then(|uploader| uploader.uploads.as_ref())
            .map(|uploads| uploads.send(upload))
        {
            Some(Ok(())) => Ok(()),
            _ => Err(()),
        }
    }

    /// Take what the thread made.
    fn take_made(&mut self) {
        let Some(uploader) = &self.uploader else {
            return;
        };
        while let Ok(made) = uploader.made.try_recv() {
            match made {
                Made::Mesh(id, buffers, normals) => {
                    self.uploading.remove(&id);
                    // Kept through the frame being painted, which asked for
                    // it.
                    self.stored.insert(
                        id,
                        Stored {
                            buffers,
                            normals,
                            drawn: true,
                        },
                    );
                }
                Made::Normals(id, normals) => {
                    self.smoothing.remove(&id);
                    if let Some(stored) = self.stored.get_mut(&id) {
                        stored.normals = normals;
                    }
                }
                Made::Unwanted(id, normals_only) => {
                    if normals_only {
                        self.smoothing.remove(&id);
                    } else {
                        self.uploading.remove(&id);
                    }
                }
                Made::Failed(id) => {
                    self.uploading.remove(&id);
                    self.refused = Some(id);
                    log::error!("the GPU could not make a mesh's buffers");
                }
            }
        }
    }
}

/// A light fixed to the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshLight {
    /// Toward the light, in the camera's coordinates.
    pub direction: [f32; 3],
    /// How much of the surface's colour it gives a face turned to it.
    pub strength: f32,
}

/// How a mesh is seen and lit. Matrices are column-major: `matrix[i]` is the
/// i-th column, as WGSL, Metal and HLSL take them.
///
/// Each face is shaded flat, or smoothly (`smooth`), lit on whichever side
/// faces the camera: the colour times `ambient` plus, for each light, its
/// strength times the cosine of the angle between the surface and the
/// light, clamped at zero. Smooth shading turns the mesh's normals
/// (`Mesh::smooth_normals`) by `model_view`, which should then only turn,
/// move and scale evenly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshView {
    /// From the mesh's coordinates to the camera's: x right, y up, z toward
    /// the viewer.
    pub model_view: [[f32; 4]; 4],
    /// From the camera's coordinates to clip space: x and y from -1 to 1
    /// across the bounds the mesh is painted in (y up), depth from 0 (near)
    /// to 1 (far).
    pub projection: [[f32; 4]; 4],
    /// Whether `projection` has perspective: a face is then lit as seen
    /// from the camera's position (the origin of the camera's coordinates),
    /// else from straight ahead.
    pub perspective: bool,
    /// Whether the surface is shaded smoothly across its faces, by normals
    /// the renderer makes once asked (flat until then), else each face
    /// flat.
    pub smooth: bool,
    /// The surface's colour, in linear RGB.
    pub color: [f32; 3],
    /// Light from all around, as a share of the colour.
    pub ambient: f32,
    /// Two lights.
    pub lights: [MeshLight; 2],
}

/// What [`crate::Window::paint_mesh`] did with a mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshPainted {
    /// It is painted.
    Painted,
    /// It is on its way to the GPU: the window draws again once it is
    /// there.
    Loading,
    /// It will not be: the window draws no meshes, or its GPU cannot hold
    /// this one.
    Refused,
}

/// The parameters a mesh's pass reads, as words in the order and layout
/// gpui_wgpu's `shaders_mesh.wgsl` declares its `MeshParams` (and naga's
/// HLSL and Metal translations of it): matrices, then `ndc_scale`,
/// `ndc_offset`, `color`, the two lights, `ambient`, and three flags.
pub type MeshParams = [u32; 52];

/// The parameters a mesh's composite reads (`MeshComposite`): the shown
/// part's origin and size, the frame's size.
pub type MeshComposite = [u32; 8];

/// A target's sides grow and shrink in steps of this many pixels, so a
/// panel being resized does not make a target at every frame.
const TARGET_STEP: u32 = 256;

/// Where a mesh is drawn in a frame ([`PaintMesh::framed`]): what of it
/// shows, and the target it is drawn into before it is composited there,
/// pixel for pixel. Every renderer draws a mesh so.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshFrame {
    /// The shown part's top left in the frame, in whole pixels.
    pub origin: [f32; 2],
    /// Its size in pixels: the viewport the mesh is drawn in, at the
    /// target's top left.
    pub shown: [u32; 2],
    /// The target's size: the shown part's, in steps.
    pub target: [u32; 2],
    frame: [f32; 2],
    /// Clip space spans the bounds; the viewport spans what is shown: x
    /// and y are scaled and moved from the one to the other.
    ndc_scale: [f32; 2],
    ndc_offset: [f32; 2],
}

impl PaintMesh {
    /// Where this mesh is drawn in a frame of `frame`'s size, its target's
    /// sides at most `largest` pixels: `None` when nothing of it shows.
    pub fn framed(&self, frame: Size<DevicePixels>, largest: u32) -> Option<MeshFrame> {
        let bounds = self.bounds;
        let shown = bounds.intersect(&self.content_mask.bounds);
        let largest = largest as f32;
        let left = shown.origin.x.0.floor().max(0.);
        let top = shown.origin.y.0.floor().max(0.);
        let right = (shown.origin.x.0 + shown.size.width.0)
            .ceil()
            .min(frame.width.0 as f32)
            .min(left + largest);
        let bottom = (shown.origin.y.0 + shown.size.height.0)
            .ceil()
            .min(frame.height.0 as f32)
            .min(top + largest);
        let (width, height) = (bounds.size.width.0, bounds.size.height.0);
        if right <= left || bottom <= top || width <= 0. || height <= 0. {
            return None;
        }
        let (shown_width, shown_height) = (right - left, bottom - top);
        let step = |side: f32| {
            (side as u32)
                .div_ceil(TARGET_STEP)
                .saturating_mul(TARGET_STEP)
                .min(largest as u32)
        };
        Some(MeshFrame {
            origin: [left, top],
            shown: [shown_width as u32, shown_height as u32],
            target: [step(shown_width), step(shown_height)],
            frame: [frame.width.0 as f32, frame.height.0 as f32],
            ndc_scale: [width / shown_width, height / shown_height],
            ndc_offset: [
                (2. * (bounds.origin.x.0 - left) + width) / shown_width - 1.,
                1. - (2. * (bounds.origin.y.0 - top) + height) / shown_height,
            ],
        })
    }
}

impl MeshFrame {
    /// What the pass drawing `view` reads, shaded smoothly or flat, in the
    /// mesh's colours or the view's.
    pub fn params(&self, view: &MeshView, smooth: bool, colored: bool) -> MeshParams {
        let floats = view
            .model_view
            .iter()
            .chain(&view.projection)
            .flatten()
            .copied()
            .chain(self.ndc_scale)
            .chain(self.ndc_offset)
            .chain(view.color)
            .chain([1.])
            .chain(view.lights.iter().flat_map(|light| {
                let [x, y, z] = light.direction;
                [x, y, z, light.strength]
            }))
            .chain([view.ambient]);
        let mut words = [0; 52];
        for (word, float) in words.iter_mut().zip(floats) {
            *word = float.to_bits();
        }
        words[49] = u32::from(view.perspective);
        words[50] = u32::from(smooth);
        words[51] = u32::from(colored);
        words
    }

    /// What the composite reads.
    pub fn composite(&self) -> MeshComposite {
        let shown = self.shown.map(|side| side as f32);
        let mut words = [0; 8];
        for (word, float) in words
            .iter_mut()
            .zip(self.origin.into_iter().chain(shown).chain(self.frame))
        {
            *word = float.to_bits();
        }
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_is_whole_triangles_between_its_positions() {
        let square = vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
        let mesh = Mesh::new(square.clone(), vec![0, 1, 2, 0, 2, 3]).unwrap();
        assert_eq!(mesh.triangles(), 2);
        assert_eq!(mesh.size_in_bytes(), 4 * 12 + 6 * 4);
        assert_ne!(
            mesh.id(),
            Mesh::new(square.clone(), vec![0, 1, 2]).unwrap().id()
        );
        assert_eq!(
            Mesh::new(square.clone(), vec![]).err(),
            Some(MeshError::Empty)
        );
        let painted = Mesh::new(square.clone(), vec![0, 1, 2])
            .unwrap()
            .with_colors(vec![[255, 0, 0, 255]; 4])
            .unwrap();
        assert_eq!(painted.colors().map(<[_]>::len), Some(4));
        assert_eq!(painted.size_in_bytes(), 4 * 12 + 3 * 4 + 4 * 4);
        assert_eq!(
            Mesh::new(square.clone(), vec![0, 1, 2])
                .unwrap()
                .with_colors(vec![[0; 4]; 3])
                .err(),
            Some(MeshError::Colors {
                colors: 3,
                positions: 4
            })
        );
        assert_eq!(
            Mesh::new(square.clone(), vec![0, 1]).err(),
            Some(MeshError::Incomplete)
        );
        assert_eq!(
            Mesh::new(square.clone(), vec![0, 1, 4]).err(),
            Some(MeshError::Index {
                index: 4,
                positions: 4
            })
        );
        let mut broken = square;
        broken[2][1] = f32::NAN;
        assert_eq!(
            Mesh::new(broken, vec![0, 1, 2]).err(),
            Some(MeshError::NotFinite { position: 2 })
        );
    }

    /// A ridge's normal is its two slopes', weighted by their areas; a
    /// position only flat triangles touch points straight up.
    #[test]
    fn smooth_normals_average_the_faces_around() {
        let roof = Mesh::new(
            vec![
                [-1., 0., 0.],
                [0., 0., 1.],
                [3., 0., 0.],
                [-1., 1., 0.],
                [0., 1., 1.],
                [3., 1., 0.],
                [9., 9., 9.],
            ],
            vec![0, 1, 4, 0, 4, 3, 1, 2, 5, 1, 5, 4, 6, 6, 6],
        )
        .unwrap();
        let normals = roof.smooth_normals();
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5);
        let half = std::f32::consts::FRAC_1_SQRT_2;
        // The left slope rises at 45°; the right one, three times as wide,
        // at under 20°.
        assert!(near(normals[0], [-half, 0., half]));
        let right = [1. / 10f32.sqrt(), 0., 3. / 10f32.sqrt()];
        assert!(near(normals[2], right));
        // The ridge: one triangle of the left slope, (-1, 0, 1) of area
        // √2/2, and two of the right, (1, 0, 3) of area √10/2 each.
        let ridge = 50f32.sqrt();
        assert!(near(normals[1], [1. / ridge, 0., 7. / ridge]));
        assert_eq!(normals[6], [0., 0., 1.]);
    }

    /// A GPU that makes buffers, or normals, only when told to.
    #[derive(Clone)]
    struct Fussy {
        buffers: bool,
        normals: bool,
    }

    impl MeshMaker for Fussy {
        type Buffers = usize;
        type Normals = usize;

        fn holds(&self, _: &Mesh) -> bool {
            true
        }

        fn buffers(&self, mesh: &Mesh) -> Option<usize> {
            self.buffers.then_some(mesh.triangles())
        }

        fn normals(&self, normals: &[[f32; 3]]) -> Option<usize> {
            self.normals.then_some(normals.len())
        }
    }

    /// Whether the window that started an upload is told it is done.
    fn told(preparation: MeshPreparation) -> bool {
        match preparation {
            MeshPreparation::Started(uploaded) | MeshPreparation::Flat(Some(uploaded)) => {
                futures::executor::block_on(uploaded).is_ok()
            }
            _ => panic!("nothing started"),
        }
    }

    /// A mesh whose buffers the GPU could not make is not asked for again;
    /// normals it could not make leave the mesh flat, asked for once.
    #[test]
    fn what_the_gpu_cannot_make_is_asked_for_once() {
        let mesh = Arc::new(
            Mesh::new(
                vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                vec![0, 1, 2],
            )
            .unwrap(),
        );

        let mut store = MeshStore::new(Fussy {
            buffers: false,
            normals: true,
        });
        assert!(!told(store.prepare(&mesh, false)));
        assert!(matches!(
            store.prepare(&mesh, false),
            MeshPreparation::Never
        ));
        assert!(store.is_empty());

        let mut store = MeshStore::new(Fussy {
            buffers: true,
            normals: false,
        });
        assert!(told(store.prepare(&mesh, false)));
        assert!(matches!(
            store.prepare(&mesh, true),
            MeshPreparation::Flat(Some(_))
        ));
        // Asked for and not made: flat from then on.
        while store.smoothing.contains(&mesh.id()) {
            store.take_made();
            std::thread::yield_now();
        }
        assert!(matches!(store.prepare(&mesh, true), MeshPreparation::Ready));
        assert_eq!(
            store
                .drawn(mesh.id())
                .map(|(buffers, normals)| (*buffers, normals.copied())),
            Some((1, None))
        );
    }

    /// A store let go of waits for the upload under way and skips those
    /// asked for after it.
    #[test]
    fn a_store_let_go_of_makes_nothing_more() {
        use std::sync::atomic::AtomicUsize;
        #[derive(Clone)]
        struct Slow(Arc<AtomicUsize>);
        impl MeshMaker for Slow {
            type Buffers = ();
            type Normals = ();
            fn holds(&self, _: &Mesh) -> bool {
                true
            }
            fn buffers(&self, _: &Mesh) -> Option<()> {
                std::thread::sleep(std::time::Duration::from_millis(50));
                self.0.fetch_add(1, Ordering::SeqCst);
                Some(())
            }
            fn normals(&self, _: &[[f32; 3]]) -> Option<()> {
                Some(())
            }
        }
        let made = Arc::new(AtomicUsize::new(0));
        let mut store = MeshStore::new(Slow(Arc::clone(&made)));
        let meshes = (0..5)
            .map(|_| {
                Arc::new(
                    Mesh::new(
                        vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                        vec![0, 1, 2],
                    )
                    .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        for mesh in &meshes {
            assert!(matches!(
                store.prepare(mesh, false),
                MeshPreparation::Started(_)
            ));
        }
        drop(store);
        let after = made.load(Ordering::SeqCst);
        assert!(after <= 1, "{after} made");
        std::thread::sleep(std::time::Duration::from_millis(120));
        assert_eq!(
            made.load(Ordering::SeqCst),
            after,
            "nothing made once it went"
        );
    }

    /// A mesh is drawn into a target of its own, the size of what shows
    /// of it, in steps; each corner of its bounds falls on its pixel.
    #[test]
    fn a_mesh_is_framed_by_what_shows_of_it() {
        use crate::{Bounds, ContentMask, ScaledPixels, point, size};
        let pixels = |x, y, width, height| Bounds {
            origin: point(ScaledPixels(x), ScaledPixels(y)),
            size: size(ScaledPixels(width), ScaledPixels(height)),
        };
        let mesh = Arc::new(
            Mesh::new(
                vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                vec![0, 1, 2],
            )
            .unwrap(),
        );
        let light = MeshLight {
            direction: [0., 0., 1.],
            strength: 0.5,
        };
        let view = MeshView {
            model_view: [
                [1., 0., 0., 0.],
                [0., 1., 0., 0.],
                [0., 0., 1., 0.],
                [0., 0., 0., 1.],
            ],
            projection: [
                [2., 0., 0., 0.],
                [0., 2., 0., 0.],
                [0., 0., 1., 0.],
                [0., 0., 0., 1.],
            ],
            perspective: true,
            smooth: false,
            color: [0.25, 0.5, 0.75],
            ambient: 0.125,
            lights: [light, light],
        };
        // Half a pixel in from the left, its lower half clipped away.
        let paint = PaintMesh {
            order: 0,
            bounds: pixels(100.5, 50., 300., 200.),
            content_mask: ContentMask {
                bounds: pixels(0., 0., 800., 150.),
            },
            mesh,
            view,
        };
        let frame = size(DevicePixels(800), DevicePixels(600));
        let framed = paint.framed(frame, 8192).unwrap();
        assert_eq!(framed.origin, [100., 50.]);
        assert_eq!(framed.shown, [301, 100]);
        assert_eq!(framed.target, [512, 256]);
        assert_eq!(paint.framed(frame, 300).unwrap().target, [300, 256]);
        let x = |ndc: f32| {
            let shown = ndc * framed.ndc_scale[0] + framed.ndc_offset[0];
            framed.origin[0] + (shown + 1.) / 2. * framed.shown[0] as f32
        };
        let y = |ndc: f32| {
            let shown = ndc * framed.ndc_scale[1] + framed.ndc_offset[1];
            framed.origin[1] + (1. - shown) / 2. * framed.shown[1] as f32
        };
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(near(x(-1.), 100.5) && near(x(1.), 400.5));
        assert!(near(y(1.), 50.) && near(y(-1.), 250.));

        // Nothing shows: out of the frame, or clipped away.
        let mut outside = paint.clone();
        outside.bounds = pixels(900., 0., 100., 100.);
        assert_eq!(outside.framed(frame, 8192), None);
        let mut clipped = paint.clone();
        clipped.content_mask.bounds = pixels(0., 0., 50., 50.);
        assert_eq!(clipped.framed(frame, 8192), None);

        // The words as the shader declares them.
        let params = framed.params(&view, true, false);
        let float = |word: u32| f32::from_bits(word);
        assert_eq!(float(params[0]), 1.);
        assert_eq!(float(params[16]), 2.);
        assert_eq!(float(params[32]), framed.ndc_scale[0]);
        assert_eq!(float(params[35]), framed.ndc_offset[1]);
        assert_eq!(
            params[36..40].iter().map(|&w| float(w)).collect::<Vec<_>>(),
            [0.25, 0.5, 0.75, 1.]
        );
        assert_eq!(float(params[43]), 0.5);
        assert_eq!(float(params[48]), 0.125);
        assert_eq!(params[49..], [1, 1, 0]);
        let composite = framed.composite().map(float);
        assert_eq!(composite, [100., 50., 301., 100., 800., 600., 0., 0.]);
    }
}
