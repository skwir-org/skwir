//! The audio player's picture: the cover in a disc, the spectrum as rays of
//! dots around it, like a level meter bent into a ring, and a glow behind that
//! breathes with the music, all in the cover's own colour.
//!
//! Bass is at the bottom and the rays climb both sides to the treble at the
//! top, so the figure is symmetric and the beat lands under the cover. There
//! is no timer here: the worker publishes a spectrum at most every 50 ms of
//! audio and each one is a frame; paused, the rays shrink to a calm ring.
//! Everything is sized in `paint`, from the element's real bounds.
use std::f32::consts::PI;
use std::sync::Arc;

use gpui::{
    App, Bounds, Corners, Element, ElementId, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, RenderImage, Style, Window, point, px, relative, size,
};

pub struct Halo {
    /// Levels per band, low to high, each 0..=1.
    pub spectrum: Vec<f32>,
    pub playing: bool,
    pub cover: Option<Arc<RenderImage>>,
    /// The cover's colour, where the rays begin; they end in `accent`.
    pub tint: Hsla,
    pub accent: Hsla,
    pub surface: Hsla,
    pub border: Hsla,
}

/// Share of the half-side the cover's disc takes; the rays have the rest.
const DISC: f32 = 0.56;
/// Between the disc and the foot of a ray.
const GAP: f32 = 7.;

/// The angle of band `index` of `bands` on the right-hand side, in screen
/// coordinates (y down): from the bottom, round the right, to the top. The
/// left-hand side mirrors it.
pub fn angle(index: usize, bands: usize) -> f32 {
    let along = (index as f32 + 0.5) / bands.max(1) as f32;
    PI / 2. - along * PI
}

/// Dots lit above the foot of a ray `length` long, `pitch` apart.
pub fn dots(length: f32, pitch: f32) -> usize {
    if pitch <= 0. || !length.is_finite() {
        return 0;
    }
    (length.max(0.) / pitch).round() as usize
}

/// The colour a cover is remembered by: the average of its pixels weighted by
/// how colourful they are, so a white border or a black background does not
/// wash it out. `bgra` is the decoded image; `None` when it is all grey.
pub fn tint(bgra: &[u8]) -> Option<(f32, f32, f32)> {
    let (mut red, mut green, mut blue, mut weight) = (0., 0., 0., 0.);
    // A few thousand samples say as much as every pixel.
    let step = (bgra.len() / 4 / 4096).max(1);
    for [b, g, r, _] in bgra.as_chunks::<4>().0.iter().step_by(step) {
        let (b, g, r) = (f32::from(*b), f32::from(*g), f32::from(*r));
        let (high, low) = (r.max(g).max(b), r.min(g).min(b));
        let colourful = (high - low) / 255.;
        let w = colourful * colourful * (high / 255.);
        red += r * w;
        green += g * w;
        blue += b * w;
        weight += w;
    }
    (weight > 1.).then(|| {
        (
            red / weight / 255.,
            green / weight / 255.,
            blue / weight / 255.,
        )
    })
}

/// A tint made fit to draw with: bright and saturated enough to read on
/// either theme, whatever the cover's average was.
pub fn vivid((red, green, blue): (f32, f32, f32)) -> Hsla {
    let mut colour: Hsla = gpui::Rgba {
        r: red,
        g: green,
        b: blue,
        a: 1.,
    }
    .into();
    colour.s = colour.s.max(0.55);
    colour.l = colour.l.clamp(0.48, 0.62);
    colour
}

/// Between two colours, the short way round the hue circle.
/// A band's level as drawn: within 0 and 1, and nothing for a value that
/// is not a number, so a bad spectrum cannot blow up a quad.
pub fn level(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0., 1.)
    } else {
        0.
    }
}

fn blend(from: Hsla, to: Hsla, along: f32) -> Hsla {
    let mut turn = to.h - from.h;
    if turn > 0.5 {
        turn -= 1.;
    } else if turn < -0.5 {
        turn += 1.;
    }
    Hsla {
        h: (from.h + turn * along).rem_euclid(1.),
        s: from.s + (to.s - from.s) * along,
        l: from.l + (to.l - from.l) * along,
        a: from.a + (to.a - from.a) * along,
    }
}

impl IntoElement for Halo {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Halo {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        (): &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let half = f32::from(bounds.size.width.min(bounds.size.height)) / 2.;
        if half < 24. {
            return;
        }
        let centre = bounds.center();
        let (cx, cy) = (f32::from(centre.x), f32::from(centre.y));
        let disc = half * DISC;
        let reach = (half - disc - GAP - 2.).max(4.);
        let bands = self.spectrum.len().max(1);
        let swell = if self.playing {
            self.spectrum.iter().copied().map(level).sum::<f32>() / bands as f32
        } else {
            0.
        };

        // The glow: discs of the cover's colour, wider and fainter, swelling
        // with how loud the moment is.
        let circle = |radius: f32| {
            Bounds::new(
                point(px(cx - radius), px(cy - radius)),
                size(px(radius * 2.), px(radius * 2.)),
            )
        };
        for (spread, strength) in [(1.62, 0.05), (1.38, 0.08), (1.18, 0.12)] {
            let radius = disc * (1. + (spread - 1.) * (0.55 + swell));
            window.paint_quad(gpui::quad(
                circle(radius),
                px(radius),
                self.tint.opacity(strength * (0.6 + swell * 1.4).min(1.6)),
                px(0.),
                gpui::transparent_black(),
                gpui::BorderStyle::default(),
            ));
        }

        // The rays, mirrored: band 0 under the cover, the last above it. A
        // ray is a run of dots, like a level meter: a circle looks the same at
        // any angle, so no path is needed (gpui's path pass costs a frame of
        // its own), only quads, which are drawn in one batch.
        let dot = (PI * (disc + GAP) / bands as f32 * 0.58).clamp(2.5, 6.);
        let pitch = dot + 1.5;
        for (index, value) in self.spectrum.iter().enumerate() {
            let length = if self.playing {
                level(*value) * reach
            } else {
                0.
            };
            let along = index as f32 / bands.saturating_sub(1).max(1) as f32;
            let colour = blend(self.tint, self.accent, along);
            let right = angle(index, bands);
            for step in 0..=dots(length, pitch) {
                let radius = disc + GAP + dot / 2. + step as f32 * pitch;
                // The foot of every ray is always lit, fainter when silent.
                let shade = if step == 0 && length < pitch / 2. {
                    0.4
                } else {
                    0.95 - 0.35 * (step as f32 * pitch / reach).min(1.)
                };
                for theta in [right, PI - right] {
                    let (sin, cos) = theta.sin_cos();
                    let (x, y) = (cx + cos * radius, cy + sin * radius);
                    window.paint_quad(gpui::quad(
                        Bounds::new(
                            point(px(x - dot / 2.), px(y - dot / 2.)),
                            size(px(dot), px(dot)),
                        ),
                        px(dot / 2.),
                        colour.opacity(shade),
                        px(0.),
                        gpui::transparent_black(),
                        gpui::BorderStyle::default(),
                    ));
                }
            }
        }

        // The cover, in its disc; without one, the disc alone.
        let frame = circle(disc);
        window.paint_quad(gpui::quad(
            frame,
            px(disc),
            self.surface,
            px(1.),
            self.border,
            gpui::BorderStyle::default(),
        ));
        if let Some(cover) = &self.cover {
            let pixels = cover.size(0);
            let (wide, high) = (pixels.width.0.max(1) as f32, pixels.height.0.max(1) as f32);
            if (wide / high - 1.).abs() < 0.04 {
                // A square cover fills the disc.
                let _ = window.paint_image(frame, Corners::all(px(disc)), cover.clone(), 0, false);
            } else {
                // Any other shape is shown whole, inside the disc.
                let side = disc * std::f32::consts::SQRT_2;
                let scale = side / wide.max(high);
                let (w, h) = (wide * scale, high * scale);
                let inside =
                    Bounds::new(point(px(cx - w / 2.), px(cy - h / 2.)), size(px(w), px(h)));
                let _ = window.paint_image(inside, Corners::all(px(4.)), cover.clone(), 0, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bass_is_under_the_cover_and_treble_above_it() {
        // Eight bands: the first just right of straight down, the last just
        // right of straight up, the middle ones pointing right.
        assert!((angle(0, 8) - (PI / 2. - PI / 16.)).abs() < 1e-6);
        assert!((angle(7, 8) - (-PI / 2. + PI / 16.)).abs() < 1e-6);
        assert!(angle(3, 8) > 0. && angle(4, 8) < 0.);
        // A silent band lights only its foot; a level rounds to whole dots.
        assert_eq!(dots(0., 5.), 0);
        assert_eq!(dots(12., 5.), 2);
        assert_eq!(dots(13., 5.), 3);
        assert_eq!(dots(f32::NAN, 5.), 0);
    }

    #[test]
    fn a_cover_is_remembered_by_its_colour_not_its_border() {
        // Mostly white and black, with a patch of orange (BGRA).
        let mut pixels = Vec::new();
        for index in 0..4000 {
            pixels.extend(match index % 10 {
                0 => [0u8, 128, 255, 255],
                1..=5 => [255, 255, 255, 255],
                _ => [0, 0, 0, 255],
            });
        }
        let (red, green, blue) = tint(&pixels).expect("the orange counts");
        assert!(red > 0.95 && (green - 0.5).abs() < 0.05 && blue < 0.05);
        assert!(
            tint(&[128, 128, 128, 255].repeat(500)).is_none(),
            "all grey"
        );
        // Whatever the average, the colour drawn with is bright enough to see.
        let dim = vivid((0.1, 0.05, 0.05));
        assert!(dim.l >= 0.48 && dim.s >= 0.55);
        // Blending goes the short way round: red to magenta never turns green.
        let red = gpui::hsla(0.02, 1., 0.5, 1.);
        let magenta = gpui::hsla(0.9, 1., 0.5, 1.);
        let between = blend(red, magenta, 0.5);
        assert!(between.h > 0.9 || between.h < 0.05);
    }
}
