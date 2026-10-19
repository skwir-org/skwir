//! Colors as they are seen: Oklab (Björn Ottosson's), where a step of
//! lightness looks the same size in every hue, and WCAG 2's contrast of
//! two colors. The theme derives its tokens in Oklab and guards their
//! contrast with `readable` and `readable_on` (ADR-071); a picture's
//! backdrop asks `contrast` too (`backdrop::behind`).

use gpui::{Hsla, Rgba};

/// An sRGB channel (0 to 1) in linear light.
pub fn linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A linear channel (0 to 1) as sRGB stores it.
fn encoded(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1. / 2.4) - 0.055
    }
}

/// A color's relative luminance, its opacity left aside.
pub fn luminance(color: Hsla) -> f32 {
    let rgb = color.to_rgb();
    0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b)
}

/// The contrast of two relative luminances, from 1 to 21.
pub fn contrast(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The contrast of two colors, from 1 to 21.
pub fn ratio(a: Hsla, b: Hsla) -> f32 {
    contrast(luminance(a), luminance(b))
}

/// A color in Oklab: `l` its lightness (0 black, 1 white), `a` and `b`
/// its hue and chroma (green to red, blue to yellow).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

/// Chroma under this is a gray's rounding, not a hue.
const GRAY: f32 = 1e-4;

impl Oklab {
    pub fn of(color: Hsla) -> Self {
        let rgb = color.to_rgb();
        let [r, g, b] = [rgb.r, rgb.g, rgb.b].map(linear);
        let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_99 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
        let [l, m, s] = [l, m, s].map(f32::cbrt);
        let lab = Oklab {
            l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        };
        if lab.chroma() < GRAY {
            Oklab {
                a: 0.,
                b: 0.,
                ..lab
            }
        } else {
            lab
        }
    }

    pub fn chroma(self) -> f32 {
        self.a.hypot(self.b)
    }

    /// The way from `self` (0) to `other` (1), `along` of it; past either
    /// end beyond it.
    pub fn mix(self, other: Oklab, along: f32) -> Oklab {
        Oklab {
            l: self.l + (other.l - self.l) * along,
            a: self.a + (other.a - self.a) * along,
            b: self.b + (other.b - self.b) * along,
        }
    }

    /// The color at this lightness and hue, with opacity `alpha`: its
    /// chroma lessened until sRGB has it, its lightness kept within black
    /// and white.
    pub fn to_hsla(self, alpha: f32) -> Hsla {
        let l = self.l.clamp(0., 1.);
        let at = |share: f32| linear_rgb(l, self.a * share, self.b * share);
        let mut rgb = at(1.);
        if !in_gamut(rgb) {
            let (mut inside, mut outside) = (0f32, 1f32);
            for _ in 0..20 {
                let share = f32::midpoint(inside, outside);
                if in_gamut(at(share)) {
                    inside = share;
                } else {
                    outside = share;
                }
            }
            rgb = at(inside);
        }
        let [r, g, b] = rgb.map(|c| encoded(c.clamp(0., 1.)));
        Rgba { r, g, b, a: alpha }.into()
    }
}

fn linear_rgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    let lms = [
        l + 0.396_337_78 * a + 0.215_803_76 * b,
        l - 0.105_561_346 * a - 0.063_854_17 * b,
        l - 0.089_484_18 * a - 1.291_485_5 * b,
    ]
    .map(|c| c * c * c);
    [
        4.076_741_7 * lms[0] - 3.307_711_6 * lms[1] + 0.230_969_94 * lms[2],
        -1.268_438 * lms[0] + 2.609_757_4 * lms[1] - 0.341_319_38 * lms[2],
        -0.004_196_086_3 * lms[0] - 0.703_418_6 * lms[1] + 1.707_614_7 * lms[2],
    ]
}

fn in_gamut(rgb: [f32; 3]) -> bool {
    rgb.iter().all(|c| (-1e-5..=1. + 1e-5).contains(c))
}

/// Whether `fg` reads at `ratio` on each of `grounds`.
pub fn reads(fg: Hsla, grounds: &[Hsla], ratio: f32) -> bool {
    let y = luminance(fg);
    grounds.iter().all(|g| contrast(y, luminance(*g)) >= ratio)
}

/// `fg` made to read at `ratio` on each of `grounds`: itself when it
/// does; else its hue at the lightness nearest its own that does; else,
/// when no lightness of it does, black or white, whichever reads better.
pub fn readable(fg: Hsla, grounds: &[Hsla], ratio: f32) -> Hsla {
    if reads(fg, grounds, ratio) {
        return fg;
    }
    // Aim a little past the ratio, so rounding back to a color keeps it.
    let aim = ratio * 1.002;
    let lab = Oklab::of(fg);
    let ys = || grounds.iter().map(|g| luminance(*g));
    let under = ys().map(|y| (y + 0.05) / aim - 0.05).fold(1., f32::min);
    let over = ys().map(|y| (y + 0.05) * aim - 0.05).fold(0., f32::max);
    let darker = (under >= 0.).then(|| at_luminance(lab, fg.a, under, false));
    let lighter = (over <= 1.).then(|| at_luminance(lab, fg.a, over, true));
    let nearest = [darker, lighter]
        .into_iter()
        .flatten()
        .filter(|c| reads(*c, grounds, ratio))
        .min_by(|x, y| {
            let far = |c: &Hsla| (Oklab::of(*c).l - lab.l).abs();
            far(x).total_cmp(&far(y))
        });
    nearest.unwrap_or_else(|| {
        let worst = |c: Hsla| {
            let y = luminance(c);
            ys().map(|g| contrast(y, g)).fold(21., f32::min)
        };
        let (black, white) = (gpui::black().opacity(fg.a), gpui::white().opacity(fg.a));
        if worst(black) >= worst(white) {
            black
        } else {
            white
        }
    })
}

/// `fg` made to read at `ratio` on the fill `ground`, and the fill: `fg`
/// moved as `readable` moves it, and when even black or white falls
/// short (a fill of middle lightness), the fill moved away from it until
/// it reads.
pub fn readable_on(fg: Hsla, ground: Hsla, ratio: f32) -> (Hsla, Hsla) {
    let fg = readable(fg, &[ground], ratio);
    if reads(fg, &[ground], ratio) {
        (fg, ground)
    } else {
        (fg, readable(ground, &[fg], ratio))
    }
}

/// `lab`'s hue at the lightness whose luminance is `target`: the lightest
/// at or under it, or (`over`) the darkest at or over it.
fn at_luminance(lab: Oklab, alpha: f32, target: f32, over: bool) -> Hsla {
    let at = |l: f32| Oklab { l, ..lab }.to_hsla(alpha);
    let (mut dark, mut light) = (0f32, 1f32);
    for _ in 0..24 {
        let l = f32::midpoint(dark, light);
        if luminance(at(l)) < target {
            dark = l;
        } else {
            light = l;
        }
    }
    at(if over { light } else { dark })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::rgb;

    fn hex(value: u32) -> Hsla {
        rgb(value).into()
    }

    fn close(a: Hsla, b: Hsla) -> bool {
        let (a, b) = (a.to_rgb(), b.to_rgb());
        [a.r - b.r, a.g - b.g, a.b - b.b]
            .iter()
            .all(|d| d.abs() < 1.5 / 255.)
    }

    #[test]
    fn oklab_goes_there_and_back() {
        for value in [0x000000, 0xffffff, 0x2f6feb, 0xff9632, 0x1e8e3e, 0x7f7f7f] {
            let color = hex(value);
            assert!(close(Oklab::of(color).to_hsla(1.), color), "{value:06x}");
        }
        let white = Oklab::of(gpui::white());
        assert!((white.l - 1.).abs() < 1e-3 && white.chroma() == 0.);
    }

    #[test]
    fn a_color_srgb_lacks_loses_chroma_not_lightness() {
        // A vivid blue at the lightness of a pale gray: sRGB has no such
        // blue, so it comes back paler, at that lightness.
        let vivid = Oklab {
            l: 0.9,
            a: -0.05,
            b: -0.3,
        };
        let back = Oklab::of(vivid.to_hsla(1.));
        assert!((back.l - 0.9).abs() < 2e-3, "{back:?}");
        assert!(back.chroma() < vivid.chroma());
    }

    #[test]
    fn contrast_is_wcag_s() {
        assert!((ratio(gpui::black(), gpui::white()) - 21.).abs() < 1e-3);
        // #767676 is the lightest gray that reads at 4.5:1 on white.
        assert!(ratio(hex(0x767676), gpui::white()) >= 4.5);
        assert!(ratio(hex(0x777777), gpui::white()) < 4.5);
    }

    #[test]
    fn a_color_that_reads_is_left_as_it_is() {
        let fg = hex(0x1f1f1f);
        assert_eq!(readable(fg, &[hex(0xfafafa)], 4.5), fg);
    }

    #[test]
    fn a_color_that_does_not_read_moves_the_least_keeping_its_hue() {
        let warning = hex(0xb26a00);
        let ground = hex(0xfafafa);
        let moved = readable(warning, &[ground], 4.5);
        assert!(ratio(moved, ground) >= 4.5);
        assert!(ratio(moved, ground) < 4.6, "no further than it needs");
        let (was, now) = (Oklab::of(warning), Oklab::of(moved));
        assert!(now.l < was.l, "darker on a light ground");
        let hue = |c: Oklab| c.b.atan2(c.a);
        assert!((hue(was) - hue(now)).abs() < 0.02);
    }

    #[test]
    fn a_color_reads_on_every_ground_given() {
        let grounds = [hex(0xfafafa), hex(0xf0f0f0), hex(0xe9eef7)];
        let moved = readable(hex(0x707070), &grounds, 4.5);
        assert!(reads(moved, &grounds, 4.5));
    }

    #[test]
    fn white_on_a_light_fill_turns_dark() {
        let fill = hex(0x6ea8fe);
        let on = readable(gpui::white(), &[fill], 4.5);
        assert!(ratio(on, fill) >= 4.5);
        assert!(luminance(on) < luminance(fill));
    }

    #[test]
    fn a_fill_no_text_reads_on_moves_away_from_its_text() {
        // Neither black nor white reads at 7:1 on a fill of middle
        // lightness: the fill gives way.
        let fill = hex(0x7f5c7c);
        assert!(!reads(gpui::white(), &[fill], 7.) && !reads(gpui::black(), &[fill], 7.));
        let (on, fill) = readable_on(gpui::white(), fill, 7.);
        assert!(ratio(on, fill) >= 7., "{}", ratio(on, fill));
    }
}
