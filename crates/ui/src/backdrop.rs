//! What a picture that lets its background through is drawn on. Its ink
//! (`ink`: the luminance of what it draws, each pixel weighted by its
//! opacity) is read once, where the picture is decoded or converted for
//! the window. One whose ink would not read on the window's background,
//! under WCAG's 3:1 for graphics, but reads on `Theme::picture_backdrop`
//! is drawn on that (`behind`): dark lines on nothing, on the dark look.
//! A colored icon, which reads, keeps no light corners around its own; a
//! light one on the light look gets no white box that would hardly help;
//! an opaque picture never has one, which would only fringe its edges.
//! Thumbnails and a document's pictures, converted with their ink
//! (`Picture`), and the preview's picture (`preview::picture`) ask the
//! same question, of what they are laid on.

use std::sync::Arc;

use gpui::{Hsla, RenderImage, prelude::*};
use skwir_core::thumbs::Image;

use crate::theme::Theme;

/// A picture converted for the window, with its ink: a thumbnail, a
/// document's picture.
#[derive(Clone)]
pub struct Picture {
    pub image: Arc<RenderImage>,
    /// What it draws, when some of it lets its background through (`ink`).
    pub ink: Option<f32>,
}

impl Picture {
    /// `img` converted for the window, its ink read on the way: on a
    /// worker, where there is one.
    pub fn of(img: &Image) -> Option<Picture> {
        Some(Picture {
            ink: ink(&img.rgba),
            image: crate::thumbs::to_render_image(img)?,
        })
    }

    /// Its element on `bg`, where views and documents are drawn, on its
    /// backdrop when it needs one there (`behind`). The backdrop is the
    /// element's box, so a caller sizes and fits it as it would the
    /// picture.
    pub fn img(&self, theme: &Theme) -> gpui::Img {
        gpui::img(Arc::clone(&self.image))
            .when_some(behind(self.ink, theme.bg, theme), gpui::Styled::bg)
    }
}

/// The least contrast a picture's ink needs with what is behind it to
/// read: WCAG's for graphics (1.4.11).
const LEGIBLE: f32 = 3.;

/// The relative luminance of what `rgba` (straight RGBA8) draws, each
/// pixel weighted by how opaque it is, when some of it is not; `None` for
/// a picture opaque throughout or with nothing drawn. Read it where the
/// pixels are decoded, on a worker when there is one: a large picture
/// that lets its background through costs about a microsecond for each
/// thousand pixels.
pub fn ink(rgba: &[u8]) -> Option<f32> {
    static LINEAR: std::sync::LazyLock<[f32; 256]> =
        std::sync::LazyLock::new(|| std::array::from_fn(|v| linear(v as f32 / 255.)));
    let pixels = rgba.as_chunks::<4>().0;
    if pixels.iter().all(|p| p[3] == 255) {
        return None;
    }
    let (mut sum, mut weight) = (0f32, 0f32);
    for &[r, g, b, a] in pixels {
        let a = f32::from(a) / 255.;
        let [r, g, b] = [r, g, b].map(|c| LINEAR[usize::from(c)]);
        sum += a * (0.2126 * r + 0.7152 * g + 0.0722 * b);
        weight += a;
    }
    (weight > 0.).then(|| sum / weight)
}

/// The backdrop a picture of ink `ink`, laid on `on`, is drawn on, when it
/// needs one.
pub fn behind(ink: Option<f32>, on: Hsla, theme: &Theme) -> Option<Hsla> {
    ink.is_some_and(|ink| {
        contrast(ink, luminance(on)) < LEGIBLE
            && contrast(ink, luminance(theme.picture_backdrop)) >= LEGIBLE
    })
    .then_some(theme.picture_backdrop)
}

/// An sRGB channel (0 to 1) in linear light.
fn linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A color's relative luminance.
fn luminance(color: Hsla) -> f32 {
    let rgb = color.to_rgb();
    0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b)
}

/// The contrast of two relative luminances, from 1 to 21.
fn contrast(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ink of four pixels of one color, with these opacities.
    fn of(rgb: [u8; 3], alphas: [u8; 4]) -> Option<f32> {
        let rgba: Vec<u8> = alphas
            .iter()
            .flat_map(|&a| [rgb[0], rgb[1], rgb[2], a])
            .collect();
        ink(&rgba)
    }

    #[test]
    fn only_ink_that_would_not_read_is_drawn_on_the_backdrop() {
        let (dark, light) = (Theme::dark(), Theme::light());
        let on = |ink, theme: &Theme| behind(ink, theme.bg, theme).is_some();
        let lines = of([0, 0, 0], [255, 255, 0, 0]);
        assert!(on(lines, &dark), "black lines on nothing");
        assert_eq!(behind(lines, dark.bg, &dark), Some(dark.picture_backdrop));
        assert!(
            behind(lines, dark.surface, &dark).is_some(),
            "nor on the panel's surface"
        );
        assert!(!on(lines, &light), "they read on the light look");
        let edge = of([0, 0, 0], [255, 255, 255, 254]);
        assert!(on(edge, &dark), "one pixel drawn on nothing");
        let photo = of([0, 0, 0], [255; 4]);
        assert_eq!(photo, None, "an opaque picture has no backdrop");
        assert!(!on(photo, &dark));
        let icon = of([0xe2, 0x55, 0x2a], [255, 255, 255, 0]);
        assert!(!on(icon, &dark), "an orange icon reads as it is");
        let white = of([255, 255, 255], [255, 0, 0, 0]);
        assert!(!on(white, &light), "a light backdrop would not help");
        let green = of([0x2e, 0xc2, 0x7e], [255, 255, 255, 0]);
        assert!(!on(green, &light), "white would not make light green read");
        assert_eq!(of([0, 0, 0], [0; 4]), None, "nothing drawn");
        // Weighted by opacity: a faint black shadow under a white mark.
        let shadowed = ink(&[
            255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 20, 0, 0, 0, 20,
        ]);
        assert!(!on(shadowed, &dark), "its white reads");
    }
}
