use crate::{Bounds, DevicePixels, Pixels, Point, Result, SharedString, Size, point, size};
use smallvec::SmallVec;

use image::{Delay, Frame};
use std::{
    borrow::Cow,
    fmt,
    hash::Hash,
    sync::atomic::{AtomicUsize, Ordering::SeqCst},
};

/// A source of assets for this app to use.
pub trait AssetSource: 'static + Send + Sync {
    /// Load the given asset from the source path.
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>>;

    /// List the assets at the given path.
    fn list(&self, path: &str) -> Result<Vec<SharedString>>;
}

impl AssetSource for () {
    fn load(&self, _path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(None)
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

/// A unique identifier for the image cache
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ImageId(pub usize);

#[derive(PartialEq, Eq, Hash, Clone)]
#[expect(missing_docs)]
pub struct RenderImageParams {
    pub image_id: ImageId,
    pub frame_index: usize,
    /// SKWIR: which piece of the frame this is, for a frame larger than
    /// the atlas's largest texture, drawn in pieces ([`image_pieces`]); 0
    /// for a frame drawn whole.
    pub piece: usize,
}

/// SKWIR: a part of a frame larger than a texture may be, held and drawn
/// on its own (0090).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImagePiece {
    /// Its place in the frame's rows of pieces: its key's `piece`.
    pub index: usize,
    /// The pixels it draws.
    pub own: Bounds<DevicePixels>,
    /// The pixels it holds: its own, and the row or column beside them on
    /// each side another piece draws. Its edge is filtered with them, so a
    /// magnified seam is drawn as the whole frame would draw it.
    pub held: Bounds<DevicePixels>,
}

/// SKWIR: the pieces a frame of `size` is drawn in where the atlas's
/// textures are at most `max` pixels a side: the frame whole where it
/// fits, else rows of pieces that each hold at most `max` a side, the
/// pixels beside their own included.
pub(crate) fn image_pieces(size: Size<DevicePixels>, max: u32) -> SmallVec<[ImagePiece; 1]> {
    let (width, height) = (size.width.0.max(0), size.height.0.max(0));
    let max = i32::try_from(max).unwrap_or(i32::MAX).max(3);
    if width <= max && height <= max {
        let whole = Bounds {
            origin: Point::default(),
            size,
        };
        return smallvec::smallvec![ImagePiece {
            index: 0,
            own: whole,
            held: whole,
        }];
    }
    // Room for the pixel beside each side.
    let step = max - 2;
    let columns = (width + step - 1) / step;
    let rows = (height + step - 1) / step;
    let corners = |left: i32, top: i32, right: i32, bottom: i32| Bounds {
        origin: point(DevicePixels(left), DevicePixels(top)),
        size: crate::size(DevicePixels(right - left), DevicePixels(bottom - top)),
    };
    let mut pieces = SmallVec::new();
    for row in 0..rows {
        for column in 0..columns {
            let (left, top) = (column * step, row * step);
            let (right, bottom) = ((left + step).min(width), (top + step).min(height));
            pieces.push(ImagePiece {
                index: (row * columns + column) as usize,
                own: corners(left, top, right, bottom),
                held: corners(
                    (left - 1).max(0),
                    (top - 1).max(0),
                    (right + 1).min(width),
                    (bottom + 1).min(height),
                ),
            });
        }
    }
    pieces
}

/// A cached and processed image, in BGRA format
pub struct RenderImage {
    /// The ID associated with this image
    pub id: ImageId,
    /// The scale factor of this image on render.
    pub(crate) scale_factor: f32,
    data: SmallVec<[Frame; 1]>,
}

impl PartialEq for RenderImage {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for RenderImage {}

impl RenderImage {
    /// Create a new image from the given data.
    pub fn new(data: impl Into<SmallVec<[Frame; 1]>>) -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

        Self {
            id: ImageId(NEXT_ID.fetch_add(1, SeqCst)),
            scale_factor: 1.0,
            data: data.into(),
        }
    }

    /// Take back the frames this image was made of, their pixels with
    /// them: a caller that makes one image after another of the same size
    /// (a video's frames) can reuse their buffers rather than allocating
    /// each anew. The image's textures are the caller's to drop first
    /// ([`crate::App::drop_image`]).
    pub fn into_frames(self) -> SmallVec<[Frame; 1]> {
        self.data
    }

    /// Convert this image into a byte slice.
    pub fn as_bytes(&self, frame_index: usize) -> Option<&[u8]> {
        self.data
            .get(frame_index)
            .map(|frame| frame.buffer().as_raw().as_slice())
    }

    /// Get the size of this image, in pixels.
    pub fn size(&self, frame_index: usize) -> Size<DevicePixels> {
        self.data
            .get(frame_index)
            .map(|frame| {
                let (width, height) = frame.buffer().dimensions();
                size(width.into(), height.into())
            })
            .unwrap_or_default()
    }

    /// Get the size of this image, in pixels for display, adjusted for the scale factor.
    pub(crate) fn render_size(&self, frame_index: usize) -> Size<Pixels> {
        self.size(frame_index)
            .map(|v| (v.0 as f32 / self.scale_factor).into())
    }

    /// Get the delay of this frame from the previous
    pub fn delay(&self, frame_index: usize) -> Delay {
        self.data
            .get(frame_index)
            .map(|frame| frame.delay())
            .unwrap_or(Delay::from_numer_denom_ms(100, 1))
    }

    /// Get the number of frames for this image.
    pub fn frame_count(&self) -> usize {
        self.data.len()
    }
}

impl fmt::Debug for RenderImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImageData")
            .field("id", &self.id)
            .field("size", &self.data.first().map(|f| f.buffer().dimensions()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::SmallVec;

    #[test]
    fn empty_render_image_does_not_panic() {
        let image = RenderImage::new(SmallVec::new());
        assert_eq!(image.frame_count(), 0);
        assert_eq!(image.size(0), Size::default());
        assert_eq!(image.as_bytes(0), None);
        assert_eq!(image.render_size(0), Size::default());
        assert_eq!(image.delay(0), Delay::from_numer_denom_ms(100, 1));
        let _ = format!("{image:?}");
    }

    /// SKWIR: a frame that fits a texture is one piece, whole; a larger one
    /// rows of pieces that each hold at most the largest side, the pixel
    /// beside their own on each side another piece lies (0090).
    #[test]
    fn a_frame_larger_than_a_texture_is_cut_in_pieces() {
        use crate::{Bounds, DevicePixels, point};
        let at = |left: i32, top: i32, right: i32, bottom: i32| Bounds {
            origin: point(DevicePixels(left), DevicePixels(top)),
            size: crate::size(DevicePixels(right - left), DevicePixels(bottom - top)),
        };
        let whole = image_pieces(size(DevicePixels(5), DevicePixels(5)), 5);
        assert_eq!(whole.len(), 1);
        assert_eq!(
            (whole[0].own, whole[0].held),
            (at(0, 0, 5, 5), at(0, 0, 5, 5))
        );
        // 10 × 7 in textures of 5: three pixels a piece, four across, three down.
        let pieces = image_pieces(size(DevicePixels(10), DevicePixels(7)), 5);
        assert_eq!(pieces.len(), 12);
        assert_eq!(
            (pieces[0].own, pieces[0].held),
            (at(0, 0, 3, 3), at(0, 0, 4, 4))
        );
        assert_eq!(pieces[5].index, 5);
        assert_eq!(
            (pieces[5].own, pieces[5].held),
            (at(3, 3, 6, 6), at(2, 2, 7, 7))
        );
        assert_eq!(
            (pieces[11].own, pieces[11].held),
            (at(9, 6, 10, 7), at(8, 5, 10, 7))
        );
        for piece in &pieces {
            assert!(piece.held.size.width.0 <= 5 && piece.held.size.height.0 <= 5);
        }
        // Their own pixels tile the frame, each once.
        let mut drawn = [[0; 10]; 7];
        for piece in &pieces {
            for y in piece.own.top().0..piece.own.bottom().0 {
                for x in piece.own.left().0..piece.own.right().0 {
                    drawn[y as usize][x as usize] += 1;
                }
            }
        }
        assert!(drawn.iter().flatten().all(|&times| times == 1));
    }
}
