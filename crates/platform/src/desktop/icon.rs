//! The presence's icon (Linux's tray, Windows' notification area): the
//! program's own, drawn at the size the system asks, with a badge in its
//! lower-right quarter that shows how far what runs has gone and how it
//! stands. A disc fills clockwise from twelve o'clock over a lighter
//! track, ringed in white and edged in a dark line so it reads on light
//! and dark panels alike; an error or a question fills it whole.
//!
//! These colours are drawn into a picture the desktop shows on its own
//! panel, outside any of Skwir's looks: they are this module's, not the
//! theme's tokens.

use std::cell::OnceCell;
use std::collections::HashMap;

use resvg::tiny_skia::{self, FillRule, Paint, PathBuilder, Pixmap, Transform};
use resvg::usvg;

use super::{Presence, Standing};

/// How many steps the badge's share is drawn in: a change smaller than one
/// draws nothing new, so the icon is told again a few dozen times an
/// action at most.
pub(super) const STEPS: u8 = 24;

/// What the badge shows: what is drawn again only when it changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Badge {
    /// Twenty-fourths filled; `None` no share (waiting, asking).
    steps: Option<u8>,
    standing: Standing,
}

impl Badge {
    pub(super) fn of(presence: &Presence) -> Badge {
        let steps = match presence.standing {
            // Filled whole whatever the share.
            Standing::Stopped | Standing::Asks => Some(STEPS),
            Standing::Running | Standing::Paused | Standing::Waiting => presence
                .fraction
                // NaN casts to 0.
                .map(|fraction| (fraction.clamp(0.0, 1.0) * f32::from(STEPS)) as u8),
        };
        Badge {
            steps,
            standing: presence.standing,
        }
    }
}

/// The program's icon at each size asked, drawn once, badged anew.
pub(super) struct Icons {
    svg: &'static [u8],
    /// The program's icon, read when first drawn: `None` when it could not
    /// be.
    tree: OnceCell<Option<usvg::Tree>>,
    drawn: HashMap<u32, Pixmap>,
}

impl Icons {
    pub(super) fn new(svg: &'static [u8]) -> Icons {
        Icons {
            svg,
            tree: OnceCell::new(),
            drawn: HashMap::new(),
        }
    }

    /// The icon `size` pixels a side with `badge`, as rows of straight
    /// (not premultiplied) RGBA; `None` when the program's icon could not
    /// be read or drawn.
    pub(super) fn draw(&mut self, size: u32, badge: Badge) -> Option<Vec<u8>> {
        let mut pixmap = self.base(size)?.clone();
        paint(&mut pixmap, badge);
        Some(
            pixmap
                .pixels()
                .iter()
                .flat_map(|pixel| {
                    let color = pixel.demultiply();
                    [color.red(), color.green(), color.blue(), color.alpha()]
                })
                .collect(),
        )
    }

    /// The program's icon at `size`, drawn once.
    fn base(&mut self, size: u32) -> Option<&Pixmap> {
        if !self.drawn.contains_key(&size) {
            let tree = self
                .tree
                .get_or_init(|| {
                    usvg::Tree::from_data(self.svg, &usvg::Options::default())
                        .inspect_err(|error| {
                            tracing::debug!("the program's icon could not be read: {error}");
                        })
                        .ok()
                })
                .as_ref()?;
            let mut pixmap = Pixmap::new(size, size)?;
            let own = tree.size();
            let scale = size as f32 / own.width().max(own.height());
            resvg::render(
                tree,
                Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            self.drawn.insert(size, pixmap);
        }
        self.drawn.get(&size)
    }

    /// How many sizes were drawn of the program's icon itself.
    #[cfg(test)]
    fn bases(&self) -> usize {
        self.drawn.len()
    }
}

/// The badge's colour for `standing`: blue running, amber paused or
/// asking, grey waiting, red stopped.
fn colour(standing: Standing) -> [u8; 3] {
    match standing {
        Standing::Running => [0x35, 0x84, 0xe4],
        Standing::Paused | Standing::Asks => [0xe5, 0xa5, 0x0a],
        Standing::Waiting => [0x8a, 0x8a, 0x8a],
        Standing::Stopped => [0xe0, 0x1b, 0x24],
    }
}

/// `colour` most of the way to white: the track the share fills.
fn lighter(colour: [u8; 3]) -> [u8; 3] {
    colour.map(|channel| channel + (f32::from(255 - channel) * 0.6).round() as u8)
}

/// Draw `badge` over the lower-right quarter of `pixmap`.
fn paint(pixmap: &mut Pixmap, badge: Badge) {
    let side = pixmap.width() as f32;
    let (x, y, radius) = (side * 0.75, side * 0.75, side * 0.25);
    let edge = (side / 32.0).max(0.75);
    let ring = (side / 16.0).max(1.0);
    let inner = radius - edge - ring;
    let colour = colour(badge.standing);
    disc(pixmap, (x, y), radius, [0x10, 0x18, 0x20, 0xc0]);
    disc(pixmap, (x, y), radius - edge, [0xff, 0xff, 0xff, 0xff]);
    let [r, g, b] = lighter(colour);
    disc(pixmap, (x, y), inner, [r, g, b, 0xff]);
    let [r, g, b] = colour;
    match badge.steps {
        Some(STEPS..) => disc(pixmap, (x, y), inner, [r, g, b, 0xff]),
        Some(steps @ 1..) => pie(pixmap, (x, y), inner, steps, [r, g, b, 0xff]),
        Some(0) | None => {}
    }
}

fn fill(pixmap: &mut Pixmap, path: &tiny_skia::Path, [r, g, b, a]: [u8; 4]) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn disc(pixmap: &mut Pixmap, (x, y): (f32, f32), radius: f32, colour: [u8; 4]) {
    if let Some(path) = PathBuilder::from_circle(x, y, radius) {
        fill(pixmap, &path, colour);
    }
}

/// `steps` twenty-fourths of a disc, clockwise from twelve o'clock: its
/// arc as short chords, which antialiasing makes round at these sizes.
fn pie(pixmap: &mut Pixmap, (x, y): (f32, f32), radius: f32, steps: u8, colour: [u8; 4]) {
    let sweep = std::f32::consts::TAU * f32::from(steps) / f32::from(STEPS);
    // A chord every 3°.
    let chords = (sweep.to_degrees() / 3.0).ceil().max(1.0) as u32;
    let mut path = PathBuilder::new();
    path.move_to(x, y);
    for chord in 0..=chords {
        let angle = sweep * chord as f32 / chords as f32;
        path.line_to(x + radius * angle.sin(), y - radius * angle.cos());
    }
    path.close();
    if let Some(path) = path.finish() {
        fill(pixmap, &path, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &[u8] = include_bytes!("../../../ui/assets/org.skwir.skwir.svg");
    const SIDE: u32 = 48;

    fn presence(fraction: Option<f32>, standing: Standing) -> Presence {
        Presence {
            title: "Finishing 1 action".into(),
            said: "Copy 3 items to Backup · 42 %".into(),
            about: "Copy 3 items to Backup".into(),
            fraction,
            standing,
            pause: Some(true),
        }
    }

    /// The pixel at `(x, y)`.
    fn at(rgba: &[u8], (x, y): (u32, u32)) -> [u8; 4] {
        let start = ((y * SIDE + x) * 4) as usize;
        rgba[start..start + 4].try_into().unwrap()
    }

    fn opaque([r, g, b]: [u8; 3]) -> [u8; 4] {
        [r, g, b, 0xff]
    }

    /// Within the badge's track, an eighth of a turn clockwise from twelve
    /// o'clock (upper right), and five eighths (lower left); and the icon's
    /// upper left, which no badge covers.
    const EARLY: (u32, u32) = (38, 33);
    const LATE: (u32, u32) = (33, 38);
    const CORNER: (u32, u32) = (12, 12);

    #[test]
    fn the_badge_follows_the_share_and_the_standing() {
        let mut icons = Icons::new(SVG);
        let mut draw = |fraction, standing| {
            icons
                .draw(SIDE, Badge::of(&presence(fraction, standing)))
                .unwrap()
        };
        let blue = [0x35, 0x84, 0xe4];
        let amber = [0xe5, 0xa5, 0x0a];
        let red = [0xe0, 0x1b, 0x24];

        let none = draw(Some(0.0), Standing::Running);
        assert_eq!(at(&none, EARLY), opaque(lighter(blue)), "the track");
        assert_eq!(at(&none, LATE), opaque(lighter(blue)));

        let quarter = draw(Some(0.25), Standing::Running);
        assert_eq!(at(&quarter, EARLY), opaque(blue), "filled clockwise");
        assert_eq!(at(&quarter, LATE), opaque(lighter(blue)), "not yet");
        let most = draw(Some(0.75), Standing::Running);
        assert_eq!(at(&most, LATE), opaque(blue));

        let paused = draw(Some(0.25), Standing::Paused);
        assert_eq!(at(&paused, EARLY), opaque(amber), "its colour by standing");
        assert_eq!(at(&paused, LATE), opaque(lighter(amber)));

        let stopped = draw(Some(0.25), Standing::Stopped);
        assert_eq!(at(&stopped, LATE), opaque(red), "filled whole");
        let asks = draw(None, Standing::Asks);
        assert_eq!(at(&asks, LATE), opaque(amber));

        let waiting = draw(None, Standing::Waiting);
        let grey = [0x8a, 0x8a, 0x8a];
        assert_eq!(at(&waiting, EARLY), opaque(lighter(grey)), "no share");

        // The icon itself is the same under every badge, and opaque there.
        for drawn in [&quarter, &paused, &stopped, &waiting] {
            assert_eq!(at(drawn, CORNER), at(&none, CORNER));
        }
        assert_eq!(at(&none, CORNER)[3], 0xff);
        // The badge's white ring, around the track.
        assert_eq!(at(&none, (36, 36 - 10)), [0xff; 4]);
        assert_eq!(icons.bases(), 1, "the icon drawn once at a size");
    }

    #[test]
    fn the_badge_changes_by_whole_steps_only() {
        let badge = |fraction, standing| Badge::of(&presence(fraction, standing));
        assert_eq!(
            badge(Some(0.50), Standing::Running),
            badge(Some(0.52), Standing::Running),
            "less than a step"
        );
        assert_ne!(
            badge(Some(0.50), Standing::Running),
            badge(Some(0.55), Standing::Running)
        );
        assert_ne!(
            badge(Some(0.5), Standing::Running),
            badge(Some(0.5), Standing::Paused)
        );
        assert_eq!(badge(Some(f32::NAN), Standing::Running).steps, Some(0));
        assert_eq!(badge(Some(2.0), Standing::Running).steps, Some(STEPS));
        assert_eq!(badge(None, Standing::Waiting).steps, None);
        assert_eq!(badge(None, Standing::Stopped).steps, Some(STEPS));
    }

    #[test]
    fn every_size_asked_is_drawn_whole() {
        let mut icons = Icons::new(SVG);
        let badge = Badge::of(&presence(Some(0.5), Standing::Running));
        for size in [16, 22, 32, 48] {
            let drawn = icons.draw(size, badge).unwrap();
            assert_eq!(drawn.len(), (size * size * 4) as usize);
        }
        assert!(Icons::new(b"not an icon").draw(16, badge).is_none());
    }
}
