//! Triangle meshes drawn in 3D by the GPU, through [`crate::Window::paint_mesh`].

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::channel::oneshot;

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
        })
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

    /// What its buffers take, in bytes: the positions and the indices.
    pub fn size_in_bytes(&self) -> u64 {
        (std::mem::size_of_val(self.positions.as_slice())
            + std::mem::size_of_val(self.indices.as_slice())) as u64
    }
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
    /// It will not be drawn: the window draws no meshes, or this one is
    /// larger than its GPU's buffers.
    Never,
}

impl MeshPreparation {
    /// An upload starting now, and what tells the window it is done.
    pub fn start() -> (MeshPreparation, MeshUploaded) {
        let (sender, receiver) = oneshot::channel();
        (MeshPreparation::Started(receiver), MeshUploaded(sender))
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
/// Each face is shaded flat, lit on whichever side faces the camera: the
/// colour times `ambient` plus, for each light, its strength times the
/// cosine of the angle between the face and the light, clamped at zero.
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
    /// from the camera's position, else from straight ahead.
    pub perspective: bool,
    /// The surface's colour, in linear RGB.
    pub color: [f32; 3],
    /// Light from all around, as a share of the colour.
    pub ambient: f32,
    /// Two lights.
    pub lights: [MeshLight; 2],
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
}
