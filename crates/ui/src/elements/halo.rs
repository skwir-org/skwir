//! The audio player's picture: the cover in a disc, the spectrum as rays of
//! dots around it, like a level meter bent into a ring, and a glow behind that
//! breathes with the music, all in the cover's own colour.
//!
//! Bass is at the bottom and the rays climb both sides to the treble at the
//! top, so the figure is symmetric and the beat lands under the cover. There
//! is no timer here: the worker publishes a spectrum at most every 50 ms of
//! audio and each one is a frame; paused, the rays shrink to a calm ring.
//! Everything is sized in `paint`, from the element's real bounds.
use std::cell::Cell;
use std::f32::consts::PI;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, Corners, Element, ElementId, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, Pixels, RenderImage, Style, Window, point, px,
    relative, size,
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
/// Below this half-side the halo draws nothing.
const LEAST_HALF: f32 = 24.;

/// The cover's disc of a halo drawn in `bounds`: its centre and radius, as
/// `paint` draws it and `OnDisc` places what answers on it. `None` where the
/// box is too small for a halo.
pub fn disc_of(bounds: Bounds<Pixels>) -> Option<(gpui::Point<Pixels>, f32)> {
    let half = f32::from(bounds.size.width.min(bounds.size.height)) / 2.;
    (half >= LEAST_HALF).then(|| (bounds.center(), half * DISC))
}

/// Whether `at` is on a disc of `centre` and `radius`: a click there is
/// the disc's, a click in a corner of its square is not.
pub fn on_disc(at: gpui::Point<Pixels>, (centre, radius): (gpui::Point<Pixels>, f32)) -> bool {
    let (dx, dy) = (f32::from(at.x - centre.x), f32::from(at.y - centre.y));
    dx * dx + dy * dy <= radius * radius
}

/// `child` laid over the cover's disc of a `Halo` drawn in the same box: on
/// the disc's square, found from the box's real bounds in prepaint, as the
/// halo finds the disc when it paints. `disc` receives the disc this frame,
/// so a click can tell the disc from the corners of its square (`on_disc`).
/// Only what is on the disc answers as the disc: the ring around it and the
/// room beside it belong to whatever holds the halo.
pub struct OnDisc {
    pub child: AnyElement,
    pub disc: Rc<Cell<Option<(gpui::Point<Pixels>, f32)>>>,
}

impl IntoElement for OnDisc {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for OnDisc {
    type RequestLayoutState = ();
    type PrepaintState = bool;

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
        let mut style = Style {
            position: gpui::Position::Absolute,
            ..Style::default()
        };
        style.inset.top = px(0.).into();
        style.inset.left = px(0.).into();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let disc = disc_of(bounds);
        self.disc.set(disc);
        let Some((centre, radius)) = disc else {
            return false;
        };
        let side = px(radius * 2.);
        self.child.layout_as_root(
            size(
                gpui::AvailableSpace::Definite(side),
                gpui::AvailableSpace::Definite(side),
            ),
            window,
            cx,
        );
        self.child.prepaint_at(
            point(centre.x - px(radius), centre.y - px(radius)),
            window,
            cx,
        );
        true
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        placed: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *placed {
            self.child.paint(window, cx);
        }
    }
}
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
/// wash it out. `bgra` is the decoded image, `width` pixels a row; `None`
/// when it is all grey.
///
/// A few thousand samples say as much as every pixel, at the same places
/// in the picture whatever its size: the middles of a grid over its width
/// and its height. Every n-th pixel in a row's order read only a few
/// columns when the width was a multiple of n (four of a 1024 px cover),
/// so a track's thumbnail and its own cover, the same picture, glowed in
/// colours of their own.
pub fn tint(bgra: &[u8], width: usize) -> Option<(f32, f32, f32)> {
    const GRID: usize = 64;
    let pixels = bgra.as_chunks::<4>().0;
    let width = width.clamp(1, pixels.len().max(1));
    let height = pixels.len() / width;
    let (across, down) = (width.min(GRID), height.min(GRID));
    let middle = |cell: usize, cells: usize, side: usize| (2 * cell + 1) * side / (2 * cells);
    let samples = (0..down).flat_map(|row| {
        let y = middle(row, down, height);
        (0..across).map(move |column| y * width + middle(column, across, width))
    });
    let (mut red, mut green, mut blue, mut weight) = (0., 0., 0., 0.);
    for [b, g, r, _] in samples.map(|at| pixels[at]) {
        let (b, g, r) = (f32::from(b), f32::from(g), f32::from(r));
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
        let Some((centre, disc)) = disc_of(bounds) else {
            return;
        };
        let half = disc / DISC;
        let (cx, cy) = (f32::from(centre.x), f32::from(centre.y));
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
    use super::{disc_of, on_disc};
    use gpui::{Bounds, point, px, size};

    #[test]
    fn the_disc_is_in_the_smaller_side_and_its_corners_are_not_on_it() {
        // A wide, short box: the disc is centred in its height.
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(400.), px(200.)));
        let (centre, radius) = disc_of(bounds).unwrap();
        assert_eq!(centre, point(px(200.), px(100.)));
        assert!((radius - 100. * 0.56).abs() < 0.01);
        let disc = (centre, radius);
        assert!(on_disc(centre, disc));
        assert!(on_disc(point(px(200. + radius - 1.), px(100.)), disc));
        // A corner of the disc's square, and the room beside the halo.
        assert!(!on_disc(
            point(px(200. + radius - 2.), px(100. + radius - 2.)),
            disc
        ));
        assert!(!on_disc(point(px(20.), px(100.)), disc));
        // Too small a box draws no halo, and nothing answers on it.
        assert!(disc_of(Bounds::new(point(px(0.), px(0.)), size(px(40.), px(40.)))).is_none());
    }

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

    /// A `side` px square picture (BGRA) with `pixel` saying each one.
    fn picture(side: usize, pixel: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
        (0..side * side)
            .flat_map(|at| pixel(at % side, at / side))
            .collect()
    }

    const ORANGE: [u8; 4] = [0, 128, 255, 255];
    const BLUE: [u8; 4] = [230, 90, 20, 255];

    #[test]
    fn a_cover_is_remembered_by_its_colour_not_its_border() {
        // A white border round black, with a patch of orange in the middle.
        let pixels = picture(200, |x, y| {
            let inside = |v: usize, from, to| (from..to).contains(&v);
            if !inside(x, 20, 180) || !inside(y, 20, 180) {
                [255, 255, 255, 255]
            } else if inside(x, 70, 130) && inside(y, 70, 130) {
                ORANGE
            } else {
                [0, 0, 0, 255]
            }
        });
        let (red, green, blue) = tint(&pixels, 200).expect("the orange counts");
        assert!(red > 0.95 && (green - 0.5).abs() < 0.05 && blue < 0.05);
        assert!(
            tint(&[128, 128, 128, 255].repeat(500), 20).is_none(),
            "all grey"
        );
        assert!(tint(&[], 0).is_none(), "nothing");
        // Whatever the average, the colour drawn with is bright enough to see.
        let dim = vivid((0.1, 0.05, 0.05));
        assert!(dim.l >= 0.48 && dim.s >= 0.55);
        // Blending goes the short way round: red to magenta never turns green.
        let red = gpui::hsla(0.02, 1., 0.5, 1.);
        let magenta = gpui::hsla(0.9, 1., 0.5, 1.);
        let between = blend(red, magenta, 0.5);
        assert!(between.h > 0.9 || between.h < 0.05);
    }

    #[test]
    fn a_cover_and_its_thumbnail_are_remembered_by_one_colour() {
        // Blue, with a thin orange line every 256 px: the lines that every
        // 256th pixel in a row's order read and nothing else.
        let cover = picture(1024, |x, _| if x % 256 == 0 { ORANGE } else { BLUE });
        // Its thumbnail, each pixel the average of four by four.
        let thumbnail = picture(256, |x, y| {
            let mut sum = [0u32; 4];
            for (dx, dy) in (0..4).flat_map(|dx| (0..4).map(move |dy| (dx, dy))) {
                let at = 4 * ((4 * y + dy) * 1024 + 4 * x + dx);
                for (total, byte) in sum.iter_mut().zip(&cover[at..at + 4]) {
                    *total += u32::from(*byte);
                }
            }
            sum.map(|total| (total / 16) as u8)
        });
        let big = tint(&cover, 1024).expect("blue");
        let small = tint(&thumbnail, 256).expect("blue");
        assert!(big.2 > big.0, "the cover is blue: {big:?}");
        for (a, b) in [(big.0, small.0), (big.1, small.1), (big.2, small.2)] {
            assert!((a - b).abs() < 0.05, "{big:?} and {small:?}");
        }
    }
}
