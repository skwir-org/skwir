//! DXF drawings drawn from their entities, as a CAD program shows model
//! space: lines, circles, arcs, polylines (with their bulges), ellipses,
//! splines, solids, hatches, text, and blocks inserted where they are
//! referenced (dimensions are such blocks), in `AutoCAD`'s colours on its
//! dark background. What most drawings for laser cutters and CNC machines
//! are: they carry no picture of their own. Curves become polylines here;
//! the result is SVG for the inert rasterizer (`inspection::drawing`).
//!
//! A DXF file is pairs of lines, a group code and its value; `0` starts
//! an entity. Blocks come before the entities that insert them.
use bibli_core::CancelToken;
use bibli_platform::inspection;
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::TAU;
use std::fmt::Write as _;

/// `AutoCAD`'s model space background.
const BACKGROUND: [u8; 3] = [33, 40, 48];
/// How many points and texts are drawn at most, blocks expanded: a block
/// inserted in an array of arrays multiplies quickly.
const POINTS: usize = 2_000_000;
const TEXTS: usize = 20_000;
const INSERTS: usize = 200_000;
/// What is kept as it is read, before blocks are expanded: points, and a
/// text's bytes as a sixteenth of a point. Every curve is kept as its
/// points, 97 a circle, so a drawing of holes would otherwise hold
/// gigabytes to draw the 20 000 that fit in `POINTS`.
const KEPT: usize = POINTS;
/// A text's bytes at most, cut at a character: each insert of its block
/// shares it (`Text::content`).
const TEXT_BYTES: usize = 4096;
/// How deep blocks may nest.
const DEPTH: usize = 16;
/// A full circle's segments.
const CIRCLE_SEGMENTS: f64 = 96.;

/// What a drawing holds, drawn.
pub struct Drawing {
    pub svg: Option<String>,
    pub details: Vec<(String, String)>,
    /// What was left out, and why.
    pub notice: Option<String>,
}

type Point = (f64, f64);

/// An entity's colour, as it says it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Color {
    ByLayer,
    ByBlock,
    Index(u8),
    Rgb([u8; 3]),
}

#[derive(Clone, Debug)]
struct Text {
    at: Point,
    height: f64,
    /// Degrees, counter-clockwise.
    angle: f64,
    content: std::sync::Arc<str>,
    /// −1 left, 0 centre, 1 right.
    horizontal: i8,
    /// Where the anchor is on the text: 0 its baseline, 1 its middle, 2
    /// its top; −1 below the baseline (a descender's depth).
    vertical: i8,
    /// Lines after the first, for multiline text, apart by this.
    pitch: f64,
    /// The length the text is stretched to (aligned and fitted text).
    length: Option<f64>,
}

#[derive(Clone, Debug)]
enum Shape {
    Polyline(Vec<Point>, bool),
    Fill(Vec<Vec<Point>>, f64),
    Text(Text),
    Insert {
        block: String,
        transform: Affine,
        /// Columns and rows, and their spacing.
        grid: (u32, u32, f64, f64),
    },
}

#[derive(Clone, Debug)]
struct Item {
    shape: Shape,
    color: Color,
    layer: String,
}

/// A text as kept: at most `TEXT_BYTES`, cut at a character.
fn kept_text(text: &str) -> std::sync::Arc<str> {
    let mut end = text.len().min(TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].into()
}

impl Shape {
    /// What keeping it costs against `KEPT`.
    fn cost(&self) -> usize {
        match self {
            Shape::Polyline(points, _) => points.len(),
            Shape::Fill(rings, _) => rings.iter().map(Vec::len).sum(),
            Shape::Text(text) => 1 + text.content.len() / 16,
            Shape::Insert { .. } => 1,
        }
    }
}

/// A 2D affine map: `[a b c; d e f]`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine([f64; 6]);

impl Affine {
    const IDENTITY: Affine = Affine([1., 0., 0., 0., 1., 0.]);

    fn apply(&self, (x, y): Point) -> Point {
        let [a, b, c, d, e, f] = self.0;
        (a * x + b * y + c, d * x + e * y + f)
    }

    /// `self` after `inner`: `self.apply(inner.apply(p))`.
    fn then(&self, inner: &Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = inner.0;
        Affine([
            a * g + b * j,
            a * h + b * k,
            a * i + b * l + c,
            d * g + e * j,
            d * h + e * k,
            d * i + e * l + f,
        ])
    }

    fn translate(x: f64, y: f64) -> Affine {
        Affine([1., 0., x, 0., 1., y])
    }

    fn rotate(degrees: f64) -> Affine {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Affine([cos, -sin, 0., sin, cos, 0.])
    }

    fn scale(x: f64, y: f64) -> Affine {
        Affine([x, 0., 0., 0., y, 0.])
    }

    /// How much it enlarges, on average.
    fn size(&self) -> f64 {
        let [a, b, _, d, e, _] = self.0;
        (a * e - b * d).abs().sqrt()
    }

    /// The angle it turns the x axis to, in degrees, and whether it
    /// mirrors.
    fn turn(&self) -> (f64, bool) {
        let [a, b, _, d, e, _] = self.0;
        (d.atan2(a).to_degrees(), a * e - b * d < 0.)
    }
}

/// The pairs of an entity: group codes and their values.
type Pairs<'a> = [(i32, &'a str)];

fn value<'a>(pairs: &Pairs<'a>, code: i32) -> Option<&'a str> {
    pairs.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
}

fn number(pairs: &Pairs<'_>, code: i32) -> Option<f64> {
    value(pairs, code)?
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite())
}

fn point(pairs: &Pairs<'_>, x: i32) -> Option<Point> {
    Some((number(pairs, x)?, number(pairs, x + 10)?))
}

fn color(pairs: &Pairs<'_>) -> Color {
    if let Some(rgb) = number(pairs, 420) {
        let rgb = rgb as u32;
        return Color::Rgb([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
    }
    match number(pairs, 62).map(|n| n as i32) {
        Some(0) => Color::ByBlock,
        Some(n @ 1..=255) => Color::Index(n as u8),
        _ => Color::ByLayer,
    }
}

/// Whether an entity's coordinates are mirrored: its extrusion points
/// away (`230` negative), as a 2D program writes a flipped arc.
fn mirrored(pairs: &Pairs<'_>) -> bool {
    number(pairs, 230).is_some_and(|z| z < 0.)
}

/// The points of the arc around `centre` from `start` to `end` degrees,
/// counter-clockwise.
fn arc(centre: Point, radius: f64, start: f64, end: f64) -> Vec<Point> {
    let mut sweep = (end - start).rem_euclid(360.);
    if sweep < 1e-9 {
        sweep = 360.;
    }
    let steps = ((sweep / 360. * CIRCLE_SEGMENTS).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let a = (start + sweep * i as f64 / steps as f64).to_radians();
            (centre.0 + radius * a.cos(), centre.1 + radius * a.sin())
        })
        .collect()
}

/// The points from `a` to `b` bent by `bulge` (the tangent of a quarter
/// of the arc's angle; positive counter-clockwise), `a` excluded.
fn bulged(a: Point, b: Point, bulge: f64, out: &mut Vec<Point>) {
    if bulge.abs() < 1e-9 {
        out.push(b);
        return;
    }
    let chord = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    if chord < 1e-12 {
        out.push(b);
        return;
    }
    let angle = 4. * bulge.atan();
    let radius = chord / (2. * (angle / 2.).sin());
    // The centre, from the chord's middle along its normal.
    let middle = (f64::midpoint(a.0, b.0), f64::midpoint(a.1, b.1));
    let normal = (-(b.1 - a.1) / chord, (b.0 - a.0) / chord);
    let offset = radius * (angle / 2.).cos();
    let centre = (middle.0 + normal.0 * offset, middle.1 + normal.1 * offset);
    let start = (a.1 - centre.1).atan2(a.0 - centre.0);
    let steps = ((angle.abs() / TAU * CIRCLE_SEGMENTS).ceil() as usize).max(2);
    let r = radius.abs();
    for i in 1..=steps {
        let t = start + angle * i as f64 / steps as f64;
        out.push((centre.0 + r * t.cos(), centre.1 + r * t.sin()));
    }
}

/// An ellipse's points from parameter `start` to `end` (radians).
fn ellipse(centre: Point, major: Point, ratio: f64, start: f64, end: f64) -> Vec<Point> {
    let minor = (-major.1 * ratio, major.0 * ratio);
    let mut sweep = (end - start).rem_euclid(TAU);
    if sweep < 1e-9 {
        sweep = TAU;
    }
    let steps = ((sweep / TAU * CIRCLE_SEGMENTS).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let t = start + sweep * i as f64 / steps as f64;
            (
                centre.0 + major.0 * t.cos() + minor.0 * t.sin(),
                centre.1 + major.1 * t.cos() + minor.1 * t.sin(),
            )
        })
        .collect()
}

/// A B-spline through its control points, by de Boor's algorithm;
/// weights make it rational.
fn spline(degree: usize, knots: &[f64], control: &[Point], weights: &[f64]) -> Vec<Point> {
    let n = control.len();
    if n < 2 || degree == 0 || knots.len() != n + degree + 1 {
        return control.to_vec();
    }
    let weight = |i: usize| weights.get(i).copied().filter(|w| *w > 0.).unwrap_or(1.);
    let (low, high) = (knots[degree], knots[n]);
    if high.partial_cmp(&low) != Some(std::cmp::Ordering::Greater) {
        return control.to_vec();
    }
    let samples = (n * 8).clamp(16, 1000);
    (0..=samples)
        .map(|s| {
            let t = low + (high - low) * s as f64 / samples as f64;
            // The span holding t.
            let mut k = degree;
            while k < n - 1 && t >= knots[k + 1] {
                k += 1;
            }
            let mut d: Vec<(f64, f64, f64)> = (0..=degree)
                .map(|j| {
                    let i = (j + k).saturating_sub(degree).min(n - 1);
                    let w = weight(i);
                    (control[i].0 * w, control[i].1 * w, w)
                })
                .collect();
            for r in 1..=degree {
                for j in (r..=degree).rev() {
                    let i = j + k - degree;
                    let span = knots[i + degree + 1 - r] - knots[i];
                    let alpha = if span.abs() < 1e-12 {
                        0.
                    } else {
                        (t - knots[i]) / span
                    };
                    d[j] = (
                        (1. - alpha) * d[j - 1].0 + alpha * d[j].0,
                        (1. - alpha) * d[j - 1].1 + alpha * d[j].1,
                        (1. - alpha) * d[j - 1].2 + alpha * d[j].2,
                    );
                }
            }
            let (x, y, w) = d[degree];
            (x / w, y / w)
        })
        .collect()
}

/// Text as a DXF writes it, plainly: `%%d` is a degree sign, `\U+00B0`
/// a character.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("%%") {
            let code = after.chars().next().map(|c| c.to_ascii_lowercase());
            let symbol = match code {
                Some('c') => Some("⌀"),
                Some('d') => Some("°"),
                Some('p') => Some("±"),
                Some('%') => Some("%"),
                Some('u' | 'o' | 'k') => Some(""),
                _ => None,
            };
            if let Some(symbol) = symbol {
                out.push_str(symbol);
                rest = &after[1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("\\U+")
            && let Some(hex) = after.get(..4)
            && let Some(unicode) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        {
            out.push(unicode);
            rest = &after[4..];
            continue;
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// MTEXT's inline formatting removed: `\P` breaks a line, `{\fArial;x}`
/// is `x`, `\S1/2;` is `1/2`.
fn mtext(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('P' | 'X') => out.push('\n'),
                Some('~') => out.push(' '),
                Some(c @ ('\\' | '{' | '}')) => out.push(c),
                Some('S') => {
                    // A stacked fraction: its parts, `^` and `#` as `/`.
                    for c in chars.by_ref() {
                        match c {
                            ';' => break,
                            '^' | '#' => out.push('/'),
                            c => out.push(c),
                        }
                    }
                }
                Some('U') => out.push_str("\\U"),
                // Toggles without a value.
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k' | 'N') | None => {}
                // Codes with a value up to `;`: fonts, heights, colours.
                Some(_) => {
                    for c in chars.by_ref() {
                        if c == ';' {
                            break;
                        }
                    }
                }
            },
            c => out.push(c),
        }
    }
    plain(&out)
}

/// Lines of `text` broken at spaces so none is much wider than `width`
/// at `height` (a character about 0.6 of it wide).
fn wrap(text: &str, height: f64, width: f64) -> String {
    if width.is_nan() || width <= 0. || height <= 0. {
        return text.to_owned();
    }
    let per_line = ((width / (height * 0.6)).floor() as usize).max(4);
    let mut out = String::new();
    for (index, paragraph) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let mut line = 0usize;
        for (w, word) in paragraph.split(' ').enumerate() {
            let length = word.chars().count();
            if w > 0 {
                if line + 1 + length > per_line && line > 0 {
                    out.push('\n');
                    line = 0;
                } else {
                    out.push(' ');
                    line += 1;
                }
            }
            out.push_str(word);
            line += length;
        }
    }
    out
}

/// An entity's items, in its own coordinates.
fn entity(kind: &str, pairs: &Pairs<'_>, polyline: Option<&[(Point, f64)]>) -> Vec<Shape> {
    let flip = |(x, y): Point| if mirrored(pairs) { (-x, y) } else { (x, y) };
    let mirror_angle = |a: f64| if mirrored(pairs) { 180. - a } else { a };
    match kind {
        "LINE" => match (point(pairs, 10), point(pairs, 11)) {
            (Some(a), Some(b)) => vec![Shape::Polyline(vec![a, b], false)],
            _ => Vec::new(),
        },
        "CIRCLE" | "ARC" => {
            let (Some(centre), Some(radius)) = (point(pairs, 10), number(pairs, 40)) else {
                return Vec::new();
            };
            if radius <= 0. {
                return Vec::new();
            }
            let (start, end) = if kind == "ARC" {
                (
                    number(pairs, 50).unwrap_or(0.),
                    number(pairs, 51).unwrap_or(360.),
                )
            } else {
                (0., 360.)
            };
            let points = arc(centre, radius, start, end)
                .into_iter()
                .map(flip)
                .collect();
            vec![Shape::Polyline(points, kind == "CIRCLE")]
        }
        "LWPOLYLINE" => {
            let closed = number(pairs, 70).is_some_and(|f| (f as i32) & 1 != 0);
            let mut vertices: Vec<(Point, f64)> = Vec::new();
            let mut x = None;
            for (code, value) in pairs {
                match code {
                    10 => x = value.parse::<f64>().ok(),
                    20 => {
                        if let (Some(x), Ok(y)) = (x.take(), value.parse::<f64>()) {
                            vertices.push(((x, y), 0.));
                        }
                    }
                    42 => {
                        if let (Some(last), Ok(bulge)) = (vertices.last_mut(), value.parse::<f64>())
                        {
                            last.1 = bulge;
                        }
                    }
                    _ => {}
                }
            }
            let points = bulged_polyline(&vertices, closed)
                .into_iter()
                .map(flip)
                .collect();
            vec![Shape::Polyline(points, closed)]
        }
        "POLYLINE" => {
            let vertices = polyline.unwrap_or_default();
            let closed = number(pairs, 70).is_some_and(|f| (f as i32) & 1 != 0);
            let points = bulged_polyline(vertices, closed)
                .into_iter()
                .map(flip)
                .collect();
            vec![Shape::Polyline(points, closed)]
        }
        "ELLIPSE" => {
            let (Some(centre), Some(major)) = (point(pairs, 10), point(pairs, 11)) else {
                return Vec::new();
            };
            let ratio = number(pairs, 40).unwrap_or(1.);
            let start = number(pairs, 41).unwrap_or(0.);
            let end = number(pairs, 42).unwrap_or(TAU);
            let points = ellipse(centre, major, ratio, start, end)
                .into_iter()
                .map(flip)
                .collect();
            vec![Shape::Polyline(points, false)]
        }
        "SPLINE" => {
            let degree = number(pairs, 71).unwrap_or(3.) as usize;
            let knots: Vec<f64> = numbers(pairs, 40);
            let weights: Vec<f64> = numbers(pairs, 41);
            let control = points(pairs, 10);
            let fit = points(pairs, 11);
            let points = if control.len() >= 2 {
                spline(degree.min(10), &knots, &control, &weights)
            } else {
                fit
            };
            let closed = number(pairs, 70).is_some_and(|f| (f as i32) & 1 != 0);
            vec![Shape::Polyline(points, closed)]
        }
        "SOLID" | "TRACE" | "3DFACE" => {
            let corners: Vec<Point> = [10, 11, 12, 13]
                .into_iter()
                .filter_map(|code| point(pairs, code))
                .collect();
            if corners.len() < 3 {
                return Vec::new();
            }
            if kind == "3DFACE" {
                // Edges flagged invisible (bits 1, 2, 4, 8) are not drawn.
                let hidden = number(pairs, 70).unwrap_or(0.) as i32;
                if hidden == 0 {
                    return vec![Shape::Polyline(corners, true)];
                }
                let n = corners.len();
                return (0..n)
                    .filter(|edge| hidden & (1 << edge) == 0)
                    .map(|edge| {
                        Shape::Polyline(vec![corners[edge], corners[(edge + 1) % n]], false)
                    })
                    .collect();
            }
            // A solid's third and fourth corners are written crossed.
            let mut outline = vec![corners[0], corners[1]];
            if corners.len() == 4 {
                outline.push(corners[3]);
            }
            outline.push(corners[2]);
            vec![Shape::Fill(
                vec![outline.into_iter().map(flip).collect()],
                1.,
            )]
        }
        "LEADER" => vec![Shape::Polyline(points(pairs, 10), false)],
        "HATCH" => hatch(pairs)
            .map(|(loops, solid)| {
                let loops = loops
                    .into_iter()
                    .map(|l| l.into_iter().map(flip).collect())
                    .collect();
                vec![Shape::Fill(loops, if solid { 1. } else { 0.3 })]
            })
            .unwrap_or_default(),
        "TEXT" | "ATTRIB" => {
            if kind == "ATTRIB" && number(pairs, 70).is_some_and(|f| (f as i32) & 1 != 0) {
                return Vec::new();
            }
            let Some(content) = value(pairs, 1).map(plain).filter(|t| !t.trim().is_empty()) else {
                return Vec::new();
            };
            let height = number(pairs, 40).unwrap_or(1.);
            let h = number(pairs, 72).unwrap_or(0.) as i32;
            let v = number(pairs, if kind == "ATTRIB" { 74 } else { 73 }).unwrap_or(0.) as i32;
            let first = point(pairs, 10).unwrap_or_default();
            let second = point(pairs, 11);
            // Aligned and fit: from the first point to the second, along
            // them; aligned text keeps its proportions, so its height
            // follows its length.
            if let (3 | 5, Some(b)) = (h, second) {
                let length = ((b.0 - first.0).powi(2) + (b.1 - first.1).powi(2)).sqrt();
                if length > 0. {
                    let natural = content.chars().count() as f64 * height * 0.6;
                    let height = if h == 3 && natural > 0. {
                        height * length / natural
                    } else {
                        height
                    };
                    return vec![Shape::Text(Text {
                        at: flip(first),
                        height,
                        angle: mirror_angle((b.1 - first.1).atan2(b.0 - first.0).to_degrees()),
                        content: kept_text(&content),
                        horizontal: -1,
                        vertical: 0,
                        pitch: height * 5. / 3.,
                        length: Some(length),
                    })];
                }
            }
            let (at, horizontal, vertical) = match (h, v) {
                (0, 0) => (first, -1, 0),
                (4, _) => (second.unwrap_or(first), 0, 1),
                (h, v) => (
                    second.unwrap_or(first),
                    match h {
                        1 => 0,
                        2 => 1,
                        _ => -1,
                    },
                    match v {
                        1 => -1,
                        2 => 1,
                        3 => 2,
                        _ => 0,
                    },
                ),
            };
            vec![Shape::Text(Text {
                at: flip(at),
                height,
                angle: mirror_angle(number(pairs, 50).unwrap_or(0.)),
                content: kept_text(&content),
                horizontal,
                vertical,
                pitch: height * 5. / 3.,
                length: None,
            })]
        }
        "MTEXT" => {
            let mut content = String::new();
            for (code, value) in pairs {
                if matches!(code, 1 | 3) {
                    content.push_str(value);
                }
            }
            let height = number(pairs, 40).unwrap_or(1.);
            let content = wrap(&mtext(&content), height, number(pairs, 41).unwrap_or(0.));
            if content.trim().is_empty() {
                return Vec::new();
            }
            let angle = match point(pairs, 11) {
                Some((x, y)) if x != 0. || y != 0. => y.atan2(x).to_degrees(),
                _ => number(pairs, 50).unwrap_or(0.).to_degrees(),
            };
            // One of nine places; the file says any number.
            let attachment = (number(pairs, 71).unwrap_or(1.) as i32).clamp(1, 9);
            let horizontal = match (attachment - 1) % 3 {
                0 => -1,
                1 => 0,
                _ => 1,
            };
            let vertical = match (attachment - 1) / 3 {
                0 => 2,
                1 => 1,
                _ => -1,
            };
            let spacing = number(pairs, 44).filter(|s| *s > 0.).unwrap_or(1.);
            vec![Shape::Text(Text {
                at: flip(point(pairs, 10).unwrap_or_default()),
                height,
                angle: mirror_angle(angle),
                content: kept_text(&content),
                horizontal,
                vertical,
                pitch: height * 5. / 3. * spacing,
                length: None,
            })]
        }
        "INSERT" | "DIMENSION" => {
            let Some(block) = value(pairs, 2).filter(|b| !b.is_empty()) else {
                return Vec::new();
            };
            if kind == "DIMENSION" {
                // Its drawing is a block at the drawing's own coordinates.
                return vec![Shape::Insert {
                    block: block.to_owned(),
                    transform: Affine::IDENTITY,
                    grid: (1, 1, 0., 0.),
                }];
            }
            let at = point(pairs, 10).unwrap_or_default();
            let sx = number(pairs, 41).unwrap_or(1.);
            let sy = number(pairs, 42).unwrap_or(1.);
            let rotation = number(pairs, 50).unwrap_or(0.);
            let mut transform = Affine::translate(at.0, at.1)
                .then(&Affine::rotate(rotation))
                .then(&Affine::scale(sx, sy));
            if mirrored(pairs) {
                transform = Affine::scale(-1., 1.).then(&transform);
            }
            let columns = number(pairs, 70).unwrap_or(1.).max(1.) as u32;
            let rows = number(pairs, 71).unwrap_or(1.).max(1.) as u32;
            let spacing = (
                number(pairs, 44).unwrap_or(0.),
                number(pairs, 45).unwrap_or(0.),
            );
            vec![Shape::Insert {
                block: block.to_owned(),
                transform,
                grid: (columns, rows, spacing.0, spacing.1),
            }]
        }
        _ => Vec::new(),
    }
}

/// Every value of `code`, as numbers.
fn numbers(pairs: &Pairs<'_>, code: i32) -> Vec<f64> {
    pairs
        .iter()
        .filter(|(c, _)| *c == code)
        .filter_map(|(_, v)| v.parse().ok())
        .collect()
}

/// Every point whose x has code `x`.
fn points(pairs: &Pairs<'_>, x: i32) -> Vec<Point> {
    let mut out = Vec::new();
    let mut pending = None;
    for (code, value) in pairs {
        if *code == x {
            pending = value.parse::<f64>().ok();
        } else if *code == x + 10
            && let (Some(px), Ok(y)) = (pending.take(), value.parse::<f64>())
        {
            out.push((px, y));
        }
    }
    out
}

/// A polyline's points, its bulges made arcs.
fn bulged_polyline(vertices: &[(Point, f64)], closed: bool) -> Vec<Point> {
    let Some(first) = vertices.first() else {
        return Vec::new();
    };
    let mut out = vec![first.0];
    for pair in vertices.windows(2) {
        bulged(pair[0].0, pair[1].0, pair[0].1, &mut out);
    }
    if closed && vertices.len() > 1 {
        let last = vertices[vertices.len() - 1];
        bulged(last.0, first.0, last.1, &mut out);
    }
    out
}

/// A hatch's boundary loops, and whether it is solid.
fn hatch(pairs: &Pairs<'_>) -> Option<(Vec<Vec<Point>>, bool)> {
    let solid = number(pairs, 70).is_some_and(|f| f as i32 == 1);
    // The boundary starts after `91`, the number of loops.
    let start = pairs.iter().position(|(c, _)| *c == 91)?;
    let mut cursor = Cursor {
        pairs: &pairs[start + 1..],
    };
    let loops = number(pairs, 91)? as usize;
    let mut out = Vec::new();
    for _ in 0..loops.min(10_000) {
        let flags = cursor.take(92)? as i32;
        let mut boundary = Vec::new();
        if flags & 2 != 0 {
            let has_bulge = cursor.take(72)? != 0.;
            // Whether it says it is closed, a boundary always is.
            cursor.take(73)?;
            let count = cursor.take(93)? as usize;
            let mut vertices = Vec::new();
            for _ in 0..count.min(1_000_000) {
                let x = cursor.take(10)?;
                let y = cursor.take(20)?;
                let bulge = if has_bulge {
                    cursor.maybe(42).unwrap_or(0.)
                } else {
                    0.
                };
                vertices.push(((x, y), bulge));
            }
            boundary = bulged_polyline(&vertices, true);
        } else {
            let edges = cursor.take(93)? as usize;
            for _ in 0..edges.min(1_000_000) {
                match cursor.take(72)? as i32 {
                    1 => {
                        let a = (cursor.take(10)?, cursor.take(20)?);
                        let b = (cursor.take(11)?, cursor.take(21)?);
                        if boundary.last() != Some(&a) {
                            boundary.push(a);
                        }
                        boundary.push(b);
                    }
                    2 => {
                        let centre = (cursor.take(10)?, cursor.take(20)?);
                        let radius = cursor.take(40)?;
                        let (start, end) = (cursor.take(50)?, cursor.take(51)?);
                        let ccw = cursor.take(73)? != 0.;
                        let points = if ccw {
                            arc(centre, radius, start, end)
                        } else {
                            // A clockwise arc's angles are written mirrored:
                            // it runs back along the arc between them.
                            let mut points = arc(centre, radius, 360. - end, 360. - start);
                            points.reverse();
                            points
                        };
                        boundary.extend(points);
                    }
                    3 => {
                        let centre = (cursor.take(10)?, cursor.take(20)?);
                        let major = (cursor.take(11)?, cursor.take(21)?);
                        let ratio = cursor.take(40)?;
                        let (start, end) = (cursor.take(50)?, cursor.take(51)?);
                        let ccw = cursor.take(73)? != 0.;
                        if ccw {
                            boundary.extend(ellipse(
                                centre,
                                major,
                                ratio,
                                start.to_radians(),
                                end.to_radians(),
                            ));
                        } else {
                            let mut points = ellipse(
                                centre,
                                major,
                                ratio,
                                (360. - end).to_radians(),
                                (360. - start).to_radians(),
                            );
                            points.reverse();
                            boundary.extend(points);
                        }
                    }
                    4 => {
                        let degree = cursor.take(94)? as usize;
                        let rational = cursor.take(73)? != 0.;
                        cursor.take(74)?;
                        let knots = cursor.take(95)? as usize;
                        let controls = cursor.take(96)? as usize;
                        let mut knot = Vec::new();
                        for _ in 0..knots.min(100_000) {
                            knot.push(cursor.take(40)?);
                        }
                        let mut control = Vec::new();
                        let mut weights = Vec::new();
                        for _ in 0..controls.min(100_000) {
                            control.push((cursor.take(10)?, cursor.take(20)?));
                            if rational {
                                weights.push(cursor.maybe(42).unwrap_or(1.));
                            }
                        }
                        // Fit data may follow; it is not needed.
                        boundary.extend(spline(degree.min(10), &knot, &control, &weights));
                    }
                    _ => return Some((out, solid)),
                }
            }
        }
        // The boundary's source objects.
        if let Some(count) = cursor.maybe(97) {
            for _ in 0..(count as usize).min(1_000_000) {
                cursor.maybe(330);
            }
        }
        if boundary.len() >= 3 {
            out.push(boundary);
        }
    }
    Some((out, solid))
}

/// A walk through an entity's pairs in their order.
struct Cursor<'p, 'a> {
    pairs: &'p Pairs<'a>,
}

impl Cursor<'_, '_> {
    /// The next pair's number, which must have `code`.
    fn take(&mut self, code: i32) -> Option<f64> {
        let (first, rest) = self.pairs.split_first()?;
        if first.0 != code {
            return None;
        }
        self.pairs = rest;
        first.1.parse().ok()
    }

    /// The next pair's number when it has `code`.
    fn maybe(&mut self, code: i32) -> Option<f64> {
        if self.pairs.first()?.0 == code {
            self.take(code)
        } else {
            None
        }
    }
}

/// `AutoCAD`'s colour index: 1 to 9 named, 10 to 249 hues in ten shades,
/// 250 to 255 greys.
fn aci(index: u8) -> [u8; 3] {
    match index {
        1 => [255, 0, 0],
        2 => [255, 255, 0],
        3 => [0, 255, 0],
        4 => [0, 255, 255],
        5 => [0, 0, 255],
        6 => [255, 0, 255],
        8 => [128, 128, 128],
        9 => [192, 192, 192],
        10..=249 => {
            let hue = f64::from((index - 10) / 10) * 15.;
            let shade = (index - 10) % 10;
            let value = [1., 1., 0.65, 0.65, 0.5, 0.5, 0.3, 0.3, 0.15, 0.15][usize::from(shade)];
            let saturation = if shade.is_multiple_of(2) { 1. } else { 0.5 };
            hsv(hue, saturation, value)
        }
        250..=255 => {
            let grey = [51, 80, 105, 130, 190, 255][usize::from(index - 250)];
            [grey, grey, grey]
        }
        // 7, and anything else: the foreground, white on this background.
        _ => [255, 255, 255],
    }
}

fn hsv(hue: f64, saturation: f64, value: f64) -> [u8; 3] {
    let c = value * saturation;
    let x = c * (1. - ((hue / 60.) % 2. - 1.).abs());
    let (r, g, b) = match (hue / 60.) as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = value - c;
    [r, g, b].map(|v| ((v + m) * 255.).round() as u8)
}

/// A layer: its colour, and whether it is shown.
#[derive(Clone, Copy)]
struct Layer {
    color: [u8; 3],
    shown: bool,
}

/// What is being drawn, in world coordinates, before it is written.
#[derive(Default)]
struct Scene {
    /// Polylines by colour, closed or not.
    lines: BTreeMap<[u8; 3], Vec<(Vec<Point>, bool)>>,
    fills: Vec<([u8; 3], f64, Vec<Vec<Point>>)>,
    texts: Vec<([u8; 3], Text, Affine)>,
    points: usize,
    inserts: usize,
    left_out: bool,
    min: Point,
    max: Point,
    empty: bool,
}

impl Scene {
    fn cover(&mut self, (x, y): Point) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        if self.empty {
            self.min = (x, y);
            self.max = (x, y);
            self.empty = false;
        } else {
            self.min = (self.min.0.min(x), self.min.1.min(y));
            self.max = (self.max.0.max(x), self.max.1.max(y));
        }
    }

    /// Room for `n` more points; `false` once the budget is spent.
    fn room(&mut self, n: usize) -> bool {
        if self.points + n > POINTS {
            self.left_out = true;
            return false;
        }
        self.points += n;
        true
    }
}

/// Everything the drawing defines: layers and blocks.
struct Library<'a> {
    layers: HashMap<String, Layer>,
    blocks: HashMap<String, (Point, Vec<Item>)>,
    cancel: &'a CancelToken,
}

impl Library<'_> {
    /// `items` drawn through `transform`; `inherited` is the colour and
    /// layer of the insert they are drawn for.
    fn draw(
        &self,
        scene: &mut Scene,
        items: &[Item],
        transform: &Affine,
        inherited: Option<([u8; 3], &str)>,
        depth: usize,
    ) -> anyhow::Result<()> {
        for item in items {
            // Layer 0 in a block is the insert's layer.
            let layer_name = match inherited {
                Some((_, layer)) if item.layer == "0" => layer,
                _ => item.layer.as_str(),
            };
            let layer = self.layers.get(layer_name).copied().unwrap_or(Layer {
                color: aci(7),
                shown: true,
            });
            if !layer.shown {
                continue;
            }
            let color = match item.color {
                Color::Index(index) => aci(index),
                Color::Rgb(rgb) => rgb,
                Color::ByBlock => inherited.map_or(aci(7), |(color, _)| color),
                Color::ByLayer => layer.color,
            };
            match &item.shape {
                Shape::Polyline(points, closed) => {
                    if points.len() < 2 || !scene.room(points.len()) {
                        continue;
                    }
                    let drawn: Vec<Point> = points.iter().map(|p| transform.apply(*p)).collect();
                    for p in &drawn {
                        scene.cover(*p);
                    }
                    scene.lines.entry(color).or_default().push((drawn, *closed));
                }
                Shape::Fill(loops, opacity) => {
                    let n = loops.iter().map(Vec::len).sum();
                    if !scene.room(n) {
                        continue;
                    }
                    let drawn: Vec<Vec<Point>> = loops
                        .iter()
                        .map(|l| l.iter().map(|p| transform.apply(*p)).collect())
                        .collect();
                    for p in drawn.iter().flatten() {
                        scene.cover(*p);
                    }
                    scene.fills.push((color, *opacity, drawn));
                }
                Shape::Text(text) => {
                    if scene.texts.len() >= TEXTS {
                        scene.left_out = true;
                        continue;
                    }
                    scene.cover(transform.apply(text.at));
                    scene.texts.push((color, text.clone(), *transform));
                }
                Shape::Insert {
                    block,
                    transform: local,
                    grid,
                } => {
                    if depth >= DEPTH || scene.inserts >= INSERTS {
                        scene.left_out = true;
                        continue;
                    }
                    inspection::check(self.cancel)?;
                    let Some((base, children)) = self.blocks.get(block) else {
                        continue;
                    };
                    let (columns, rows, dx, dy) = *grid;
                    if u64::from(columns) * u64::from(rows) > 10_000 {
                        scene.left_out = true;
                        continue;
                    }
                    for row in 0..rows {
                        for column in 0..columns {
                            scene.inserts += 1;
                            if scene.inserts > INSERTS {
                                scene.left_out = true;
                                return Ok(());
                            }
                            // The array steps along the insert's own axes.
                            let (sin, cos) = {
                                let (angle, _) = local.turn();
                                angle.to_radians().sin_cos()
                            };
                            let (ox, oy) = (f64::from(column) * dx, f64::from(row) * dy);
                            let step = Affine::translate(ox * cos - oy * sin, ox * sin + oy * cos);
                            let placed = transform
                                .then(&step)
                                .then(local)
                                .then(&Affine::translate(-base.0, -base.1));
                            self.draw(
                                scene,
                                children,
                                &placed,
                                Some((color, layer_name)),
                                depth + 1,
                            )?;
                            if scene.left_out {
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// The pairs of a DXF text, a code line and a value line each; they end
/// where a code is not a number (not DXF, or damaged).
fn pairs(text: &str) -> impl Iterator<Item = (i32, &str)> {
    let mut lines = text.lines().skip_while(|line| line.trim().is_empty());
    std::iter::from_fn(move || {
        let code = lines.next()?.trim().parse::<i32>().ok()?;
        // A value's leading spaces are the writer's padding.
        let value = lines.next()?.trim();
        Some((code, value))
    })
}

/// The unit `$INSUNITS` names.
fn unit(code: i32) -> Option<&'static str> {
    Some(match code {
        1 => "in",
        2 => "ft",
        3 => "mi",
        4 => "mm",
        5 => "cm",
        6 => "m",
        7 => "km",
        9 => "mil",
        10 => "yd",
        13 => "µm",
        14 => "dm",
        _ => return None,
    })
}

/// What reading a drawing gathers, a pair at a time.
#[derive(Default)]
struct Reader<'a> {
    section: String,
    layers: HashMap<String, Layer>,
    blocks: HashMap<String, (Point, Vec<Item>)>,
    model: Vec<Item>,
    /// A block being defined: its name, base and items.
    block: Option<(String, Point, Vec<Item>)>,
    /// An old-style polyline waiting for its vertices.
    polyline: Option<(Vec<(i32, &'a str)>, Vec<(Point, f64)>)>,
    entities: usize,
    units: Option<&'static str>,
    /// What is kept so far (`KEPT`), and whether some was left out.
    kept: usize,
    left_out: bool,
}

impl<'a> Reader<'a> {
    /// An entity, table record or block marker read whole.
    fn finish(&mut self, kind: &str, pairs: Vec<(i32, &'a str)>) {
        match self.section.as_str() {
            "TABLES" if kind == "LAYER" => {
                let name = value(&pairs, 2).unwrap_or_default().to_owned();
                let index = number(&pairs, 62).unwrap_or(7.) as i32;
                let frozen = number(&pairs, 70).is_some_and(|f| (f as i32) & 1 != 0);
                let color = match color(&pairs) {
                    Color::Rgb(rgb) => rgb,
                    _ => aci(u8::try_from(index.unsigned_abs().min(255)).unwrap_or(7)),
                };
                self.layers.insert(
                    name,
                    Layer {
                        color,
                        shown: index >= 0 && !frozen,
                    },
                );
            }
            "BLOCKS" | "ENTITIES" => self.entity(kind, pairs),
            _ => {}
        }
    }

    fn entity(&mut self, kind: &str, pairs: Vec<(i32, &'a str)>) {
        match kind {
            "BLOCK" => {
                let name = value(&pairs, 2).unwrap_or_default().to_owned();
                let base = point(&pairs, 10).unwrap_or_default();
                self.block = Some((name, base, Vec::new()));
                return;
            }
            "ENDBLK" => {
                if let Some((name, base, items)) = self.block.take() {
                    self.blocks.insert(name, (base, items));
                }
                return;
            }
            _ => {}
        }
        // Paper space is not drawn.
        if number(&pairs, 67).is_some_and(|p| p != 0.) {
            return;
        }
        let (kind, pairs, vertices) = match kind {
            "POLYLINE" => {
                self.polyline = Some((pairs, Vec::new()));
                return;
            }
            "VERTEX" => {
                // A polyface's face records are not points.
                let face = number(&pairs, 70)
                    .is_some_and(|f| (f as i32) & 128 != 0 && (f as i32) & 64 == 0);
                if let Some((_, vertices)) = &mut self.polyline
                    && let Some(at) = point(&pairs, 10)
                    && !face
                {
                    vertices.push((at, number(&pairs, 42).unwrap_or(0.)));
                }
                return;
            }
            "SEQEND" => match self.polyline.take() {
                Some((pairs, vertices)) => ("POLYLINE", pairs, Some(vertices)),
                None => return,
            },
            kind => (kind, pairs, None),
        };
        let shapes = entity(kind, &pairs, vertices.as_deref());
        if shapes.is_empty() {
            return;
        }
        let cost: usize = shapes.iter().map(Shape::cost).sum();
        if self.kept + cost > KEPT {
            self.left_out = true;
            return;
        }
        self.kept += cost;
        let layer = value(&pairs, 8).unwrap_or("0").to_owned();
        let color = color(&pairs);
        let target = if let Some((_, _, items)) = &mut self.block {
            items
        } else {
            self.entities += 1;
            &mut self.model
        };
        target.extend(shapes.into_iter().map(|shape| Item {
            shape,
            color,
            layer: layer.clone(),
        }));
    }
}

/// A drawing's entities drawn.
pub fn draw(text: &str, cancel: &CancelToken) -> anyhow::Result<Drawing> {
    let mut reader = Reader::default();
    // The entity being read: its type and pairs.
    let mut current: Option<(&str, Vec<(i32, &str)>)> = None;
    let mut variable = "";
    for (read, (code, value)) in pairs(text).enumerate() {
        if read.is_multiple_of(65_536) {
            inspection::check(cancel)?;
        }
        if code == 0 {
            if let Some((kind, pairs)) = current.take() {
                reader.finish(kind, pairs);
            }
            match value {
                "ENDSEC" => reader.section.clear(),
                "EOF" => break,
                _ => {}
            }
            current = Some((value, Vec::new()));
            continue;
        }
        let Some((kind, pairs)) = &mut current else {
            continue;
        };
        if *kind == "SECTION" && code == 2 {
            value.clone_into(&mut reader.section);
        } else if reader.section == "HEADER" {
            if code == 9 {
                variable = value;
            } else if variable == "$INSUNITS" && code == 70 {
                reader.units = value.parse::<i32>().ok().and_then(unit);
            }
        } else {
            pairs.push((code, value));
        }
    }
    if let Some((kind, pairs)) = current.take() {
        reader.finish(kind, pairs);
    }
    inspection::check(cancel)?;
    let block_count = reader
        .blocks
        .keys()
        .filter(|name| !name.starts_with('*'))
        .count();
    let (units, entities) = (reader.units, reader.entities);
    let library = Library {
        layers: reader.layers,
        blocks: reader.blocks,
        cancel,
    };
    let mut scene = Scene {
        empty: true,
        left_out: reader.left_out,
        ..Scene::default()
    };
    library.draw(&mut scene, &reader.model, &Affine::IDENTITY, None, 0)?;
    let mut details = Vec::new();
    if !scene.empty {
        let size = crate::gcode::short;
        let (w, h) = (scene.max.0 - scene.min.0, scene.max.1 - scene.min.1);
        details.push((
            "Size".to_owned(),
            match units {
                Some(unit) => format!("{} × {} {unit}", size(w), size(h)),
                None => format!("{} × {}", size(w), size(h)),
            },
        ));
    }
    if entities > 0 {
        details.push(("Entities".into(), entities.to_string()));
    }
    if block_count > 0 {
        details.push(("Blocks".into(), block_count.to_string()));
    }
    let notice = scene
        .left_out
        .then(|| "Drawn in part: the drawing holds more than is drawn here".to_owned());
    Ok(Drawing {
        svg: write(&scene),
        details,
        notice,
    })
}

/// The scene as SVG, y up turned down, relative to its corner so the
/// rasterizer's single precision keeps the detail of a drawing far from
/// its origin.
fn write(scene: &Scene) -> Option<String> {
    if scene.empty {
        return None;
    }
    let (w, h) = (scene.max.0 - scene.min.0, scene.max.1 - scene.min.1);
    let extent = w.max(h);
    if extent.is_nan() || extent <= 0. {
        return None;
    }
    let margin = extent * 0.03;
    let (width, height) = (w + 2. * margin, h + 2. * margin);
    // Written at a scale where the drawing is about a thousand units.
    let scale = 1000. / extent;
    let to = |(x, y): Point| {
        (
            (x - scene.min.0 + margin) * scale,
            (scene.max.1 + margin - y) * scale,
        )
    };
    let mut out = String::new();
    let (vw, vh) = (width * scale, height * scale);
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\" viewBox=\"0 0 {vw:.3} {vh:.3}\"><rect width=\"{vw:.3}\" height=\"{vh:.3}\" fill=\"{}\"/>",
        vw.ceil(),
        vh.ceil(),
        hex(BACKGROUND)
    );
    for (color, opacity, loops) in &scene.fills {
        out.push_str("<path fill-rule=\"evenodd\" d=\"");
        for points in loops {
            path(&mut out, points.iter().map(|p| to(*p)), true);
        }
        let _ = write!(out, "\" fill=\"{}\"", hex(*color));
        if *opacity < 1. {
            let _ = write!(out, " fill-opacity=\"{opacity}\"");
        }
        out.push_str("/>");
    }
    for (color, lines) in &scene.lines {
        let _ = write!(
            out,
            "<path fill=\"none\" stroke=\"{}\" stroke-width=\"1.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" d=\"",
            hex(*color)
        );
        for (points, closed) in lines {
            path(&mut out, points.iter().map(|p| to(*p)), *closed);
        }
        out.push_str("\"/>");
    }
    for (color, text, transform) in &scene.texts {
        let at = to(transform.apply(text.at));
        let (turn, mirror) = transform.turn();
        let size = text.height * transform.size() * scale;
        if size.is_nan() || size <= 0.3 {
            continue;
        }
        let lines: Vec<&str> = text.content.lines().collect();
        let pitch = text.pitch * transform.size() * scale;
        let block = pitch * (lines.len().saturating_sub(1)) as f64;
        // The first baseline below the anchor.
        let first = match text.vertical {
            2 => size,
            1 => size / 2. - block / 2.,
            -1 => -size * 0.3 - block,
            _ => -block,
        };
        let anchor = match text.horizontal {
            -1 => "start",
            1 => "end",
            _ => "middle",
        };
        let _ = write!(
            out,
            "<g transform=\"translate({:.3} {:.3}) rotate({:.3}){}\" fill=\"{}\" font-family=\"DejaVu Sans, Noto Sans, Liberation Sans, Arial, sans-serif\" font-size=\"{:.3}\" text-anchor=\"{anchor}\">",
            at.0,
            at.1,
            -(text.angle + turn),
            if mirror { " scale(-1,1)" } else { "" },
            hex(*color),
            size / 0.72
        );
        for (index, line) in lines.iter().enumerate() {
            let _ = write!(out, "<text y=\"{:.3}\"", first + pitch * index as f64);
            if let Some(length) = text.length {
                let _ = write!(
                    out,
                    " textLength=\"{:.3}\" lengthAdjust=\"spacingAndGlyphs\"",
                    length * transform.size() * scale
                );
            }
            out.push('>');
            for ch in line.chars() {
                match ch {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    ch if ch.is_control() => {}
                    ch => out.push(ch),
                }
            }
            out.push_str("</text>");
        }
        out.push_str("</g>");
    }
    out.push_str("</svg>");
    Some(out)
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn path(out: &mut String, points: impl Iterator<Item = Point>, closed: bool) {
    for (index, (x, y)) in points.enumerate() {
        let _ = write!(out, "{}{x:.2},{y:.2}", if index == 0 { 'M' } else { 'L' });
    }
    if closed {
        out.push('Z');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(sections: &[(&str, &str)]) -> String {
        let mut out = String::new();
        for (name, body) in sections {
            let _ = write!(out, "  0\nSECTION\n  2\n{name}\n{body}  0\nENDSEC\n");
        }
        out.push_str("  0\nEOF\n");
        out
    }

    #[test]
    fn lines_arcs_and_a_block_inserted_twice() {
        let text = dxf(&[
            ("HEADER", "  9\n$INSUNITS\n 70\n4\n"),
            (
                "TABLES",
                "  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nCUT\n 62\n1\n 70\n0\n  0\nLAYER\n  2\nHIDDEN\n 62\n-3\n  0\nENDTAB\n",
            ),
            (
                "BLOCKS",
                "  0\nBLOCK\n  2\nHOLE\n 10\n0\n 20\n0\n  0\nCIRCLE\n  8\n0\n 10\n0\n 20\n0\n 40\n2\n  0\nENDBLK\n",
            ),
            (
                "ENTITIES",
                concat!(
                    "  0\nLWPOLYLINE\n  8\nCUT\n 90\n4\n 70\n1\n 10\n0\n 20\n0\n 10\n100\n 20\n0\n 10\n100\n 20\n50\n 42\n0\n 10\n0\n 20\n50\n",
                    "  0\nINSERT\n  8\nCUT\n  2\nHOLE\n 10\n20\n 20\n25\n",
                    "  0\nINSERT\n  8\nCUT\n  2\nHOLE\n 10\n80\n 20\n25\n 41\n2\n 42\n2\n",
                    "  0\nLINE\n  8\nHIDDEN\n 10\n0\n 20\n0\n 11\n500\n 21\n500\n",
                    "  0\nTEXT\n  8\nCUT\n 10\n10\n 20\n40\n 40\n3\n  1\nPart %%c10 <A>\n",
                ),
            ),
        ]);
        let drawing = draw(&text, &CancelToken::new()).unwrap();
        let svg = drawing.svg.unwrap();
        // The hidden layer's line would have made it 500 wide.
        assert_eq!(
            drawing.details[0],
            ("Size".to_owned(), "100 × 50 mm".to_owned())
        );
        // The hidden line counts: it is in the drawing.
        assert_eq!(drawing.details[1], ("Entities".to_owned(), "5".to_owned()));
        assert_eq!(drawing.details[2], ("Blocks".to_owned(), "1".to_owned()));
        assert!(
            svg.contains("#ff0000"),
            "red from the layer, through layer 0"
        );
        assert!(svg.contains(">Part ⌀10 &lt;A&gt;</text>"), "{svg}");
        assert!(drawing.notice.is_none());
        let drawn = inspection::drawing(svg.as_bytes(), Some(200)).unwrap();
        assert_eq!(drawn.width, 200);
    }

    #[test]
    fn a_bulge_is_half_a_circle() {
        let points = bulged_polyline(&[((0., 0.), 1.), ((2., 0.), 0.)], false);
        // Counter-clockwise from (0, 0) to (2, 0): below the chord.
        let lowest = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        assert!((lowest + 1.).abs() < 1e-3, "{lowest}");
        assert!((points.last().unwrap().0 - 2.).abs() < 1e-9);
    }

    #[test]
    fn a_spline_passes_its_ends() {
        let control = [(0., 0.), (1., 2.), (3., 2.), (4., 0.)];
        let knots = [0., 0., 0., 0., 1., 1., 1., 1.];
        let points = spline(3, &knots, &control, &[]);
        let first = points[0];
        let last = points[points.len() - 1];
        assert!(first.0.abs() < 1e-9 && first.1.abs() < 1e-9);
        assert!((last.0 - 4.).abs() < 1e-9 && last.1.abs() < 1e-9);
        // The middle of a symmetric curve.
        let middle = points[points.len() / 2];
        assert!(
            (middle.0 - 2.).abs() < 1e-6 && (middle.1 - 1.5).abs() < 1e-6,
            "{middle:?}"
        );
    }

    #[test]
    fn inline_formatting_is_read_plainly() {
        assert_eq!(
            mtext("{\\fArial|b1;Bold} line\\Pnext \\S1/2;"),
            "Bold line\nnext 1/2"
        );
        assert_eq!(plain("45%%d \\U+00B1"), "45° ±");
        assert_eq!(wrap("one two three four", 1., 6.), "one two\nthree four");
    }

    #[test]
    fn colours_by_index() {
        assert_eq!(aci(1), [255, 0, 0]);
        assert_eq!(aci(10), [255, 0, 0]);
        assert_eq!(aci(11), [255, 128, 128]);
        assert_eq!(aci(254), [190, 190, 190]);
    }

    #[test]
    fn a_block_inserting_itself_stops() {
        let text = dxf(&[
            (
                "BLOCKS",
                "  0\nBLOCK\n  2\nLOOP\n  0\nLINE\n 10\n0\n 20\n0\n 11\n1\n 21\n1\n  0\nINSERT\n  2\nLOOP\n 10\n1\n 20\n0\n  0\nENDBLK\n",
            ),
            (
                "ENTITIES",
                "  0\nINSERT\n  2\nLOOP\n 10\n0\n 20\n0\n 70\n100\n 71\n100\n 44\n2\n 45\n2\n",
            ),
        ]);
        let drawing = draw(&text, &CancelToken::new()).unwrap();
        assert!(drawing.svg.is_some());
        assert!(drawing.notice.is_some(), "it says it drew only part");
    }

    #[test]
    fn nothing_drawn_is_no_picture() {
        let drawing = draw(&dxf(&[("ENTITIES", "")]), &CancelToken::new()).unwrap();
        assert!(drawing.svg.is_none());
        let drawing = draw("not a drawing at all", &CancelToken::new()).unwrap();
        assert!(drawing.svg.is_none());
    }
}
