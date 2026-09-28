//! Writing SVG for bibli's inert rasterizer (`inspection::drawing`), in
//! the file's own unit, millimetres: numbers, text as `KiCad` places it,
//! arcs through three points, and the bounds of what was drawn.
use std::fmt::Write as _;

/// The fonts text is set in, the first installed: `KiCad`'s own stroke font
/// is not ours to carry, a plain sans is what it resembles.
const FONTS: &str = "DejaVu Sans, Noto Sans, Liberation Sans, Arial, sans-serif";
/// How much of an em a capital letter stands in these fonts: `KiCad`'s size
/// is the height of its capitals.
const CAPITAL: f64 = 0.72;
/// The distance between lines, in text heights.
const LINE_PITCH: f64 = 1.6;

/// A number as SVG wants it, rounded to a micrometre.
pub fn num(out: &mut String, value: f64) {
    let rounded = (value * 1000.).round() / 1000.;
    // `-0` is `0`.
    let _ = write!(out, "{}", if rounded == 0. { 0. } else { rounded });
}

/// `x,y` for a path.
pub fn pair(out: &mut String, (x, y): (f64, f64)) {
    num(out, x);
    out.push(',');
    num(out, y);
}

/// Text escaped for XML.
pub fn escape(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            ch if ch.is_control() => {}
            ch => out.push(ch),
        }
    }
}

/// A colour as `#rrggbb`.
pub fn color(out: &mut String, [r, g, b]: [u8; 3]) {
    let _ = write!(out, "#{r:02x}{g:02x}{b:02x}");
}

/// What was drawn covers, to frame the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub min: (f64, f64),
    pub max: (f64, f64),
    pub empty: bool,
}

impl Bounds {
    pub const fn new() -> Bounds {
        Bounds {
            min: (0., 0.),
            max: (0., 0.),
            empty: true,
        }
    }

    /// Grow to hold `(x, y)` and `reach` around it.
    pub fn add(&mut self, (x, y): (f64, f64), reach: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        if self.empty {
            self.min = (x - reach, y - reach);
            self.max = (x + reach, y + reach);
            self.empty = false;
            return;
        }
        self.min = (self.min.0.min(x - reach), self.min.1.min(y - reach));
        self.max = (self.max.0.max(x + reach), self.max.1.max(y + reach));
    }

    pub fn width(&self) -> f64 {
        self.max.0 - self.min.0
    }

    pub fn height(&self) -> f64 {
        self.max.1 - self.min.1
    }
}

/// The circle through three points: its centre and radius; `None` when
/// they are in a line.
pub fn circle_through(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> Option<((f64, f64), f64)> {
    let d = 2. * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d.abs() < 1e-12 {
        return None;
    }
    let sq = |p: (f64, f64)| p.0 * p.0 + p.1 * p.1;
    let x = (sq(a) * (b.1 - c.1) + sq(b) * (c.1 - a.1) + sq(c) * (a.1 - b.1)) / d;
    let y = (sq(a) * (c.0 - b.0) + sq(b) * (a.0 - c.0) + sq(c) * (b.0 - a.0)) / d;
    Some(((x, y), ((a.0 - x).powi(2) + (a.1 - y).powi(2)).sqrt()))
}

/// Path data for the arc from `start` through `mid` to `end`, beginning
/// with a move to `start`. A straight line when they are in one.
pub fn arc_path(out: &mut String, start: (f64, f64), mid: (f64, f64), end: (f64, f64)) {
    out.push('M');
    pair(out, start);
    let Some((centre, radius)) = circle_through(start, mid, end) else {
        out.push('L');
        pair(out, end);
        return;
    };
    // Which way it turns (a negative cross product is clockwise on screen,
    // y down: SVG's positive sweep), and whether it is more than half a
    // circle: the middle and the centre on one side of the chord then.
    let cross =
        |p: (f64, f64)| (end.0 - start.0) * (p.1 - start.1) - (end.1 - start.1) * (p.0 - start.0);
    let sweep = cross(mid) < 0.;
    let large = cross(centre) != 0. && cross(mid).signum() == cross(centre).signum();
    out.push('A');
    num(out, radius);
    out.push(',');
    num(out, radius);
    out.push_str(if large { " 0 1," } else { " 0 0," });
    out.push_str(if sweep { "1 " } else { "0 " });
    pair(out, end);
}

/// Where a piece of text stands and how it is set, as `KiCad` says it.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub at: (f64, f64),
    /// Degrees, counter-clockwise on screen, as `KiCad` counts them.
    pub angle: f64,
    /// The height of a capital.
    pub size: f64,
    /// −1 left, 0 centre, 1 right.
    pub horizontal: i8,
    /// −1 top, 0 centre, 1 bottom.
    pub vertical: i8,
    pub mirror: bool,
    pub bold: bool,
    pub italic: bool,
}

impl Placement {
    /// At `at`, centred, `size` high.
    pub fn centred(at: (f64, f64), size: f64) -> Placement {
        Placement {
            at,
            angle: 0.,
            size,
            horizontal: 0,
            vertical: 0,
            mirror: false,
            bold: false,
            italic: false,
        }
    }

    /// A `(justify left bottom mirror)` list read into the placement.
    pub fn justify(&mut self, words: impl IntoIterator<Item = impl AsRef<str>>) {
        for word in words {
            match word.as_ref() {
                "left" => self.horizontal = -1,
                "right" => self.horizontal = 1,
                "top" => self.vertical = -1,
                "bottom" => self.vertical = 1,
                "mirror" => self.mirror = true,
                _ => {}
            }
        }
    }
}

/// `text` set as placed, in `fill`. Each line is a `<text>`; how far it
/// reaches is added to `bounds`.
pub fn text(out: &mut String, bounds: &mut Bounds, text: &str, place: &Placement, fill: [u8; 3]) {
    if text.trim().is_empty() || !place.size.is_finite() || place.size <= 0. {
        return;
    }
    let lines: Vec<&str> = text.lines().collect();
    let pitch = place.size * LINE_PITCH;
    let block = pitch * (lines.len() as f64 - 1.);
    // The first line's baseline below the anchor.
    let first = match place.vertical {
        -1 => place.size,
        1 => -block,
        _ => place.size / 2. - block / 2.,
    };
    let anchor = match place.horizontal {
        -1 => "start",
        1 => "end",
        _ => "middle",
    };
    let _ = write!(out, "<g transform=\"translate(");
    pair(out, place.at);
    out.push(')');
    if place.angle != 0. {
        out.push_str(" rotate(");
        num(out, -place.angle);
        out.push(')');
    }
    if place.mirror {
        out.push_str(" scale(-1,1)");
    }
    out.push_str("\" fill=\"");
    color(out, fill);
    let _ = write!(
        out,
        "\" stroke=\"none\" font-family=\"{FONTS}\" font-size=\""
    );
    num(out, place.size / CAPITAL);
    let _ = write!(out, "\" text-anchor=\"{anchor}\"");
    if place.bold {
        out.push_str(" font-weight=\"bold\"");
    }
    if place.italic {
        out.push_str(" font-style=\"italic\"");
    }
    out.push('>');
    let widest = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    for (index, line) in lines.iter().enumerate() {
        out.push_str("<text y=\"");
        num(out, first + pitch * index as f64);
        out.push_str("\">");
        escape(out, line);
        out.push_str("</text>");
    }
    out.push_str("</g>");
    // The box it takes, estimated: a character about 0.8 of a capital's
    // height wide, turned as the text is.
    let width = widest as f64 * place.size * 0.8;
    let (left, right) = match place.horizontal {
        -1 => (0., width),
        1 => (-width, 0.),
        _ => (-width / 2., width / 2.),
    };
    let (top, bottom) = (first - place.size * 1.2, first + block + place.size * 0.3);
    let (sin, cos) = place.angle.to_radians().sin_cos();
    let mirror = if place.mirror { -1. } else { 1. };
    for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
        let x = x * mirror;
        bounds.add(
            (
                place.at.0 + x * cos + y * sin,
                place.at.1 - x * sin + y * cos,
            ),
            0.,
        );
    }
}

/// A layer of a drawing: its place in the order they are drawn, and the
/// colour of its lines.
pub type Slot = (u32, [u8; 3]);

/// A drawing being made: a group per layer, and what they cover.
pub struct Canvas {
    groups: std::collections::BTreeMap<u32, (String, [u8; 3])>,
    /// What everything covers, and what the layer that frames the picture
    /// covers (a board's outline).
    pub all: Bounds,
    pub framed: Bounds,
    frame: Option<u32>,
}

impl Canvas {
    /// A drawing framed by what layer `frame` covers, when it has
    /// anything, or by everything.
    pub fn new(frame: Option<u32>) -> Canvas {
        Canvas {
            groups: std::collections::BTreeMap::new(),
            all: Bounds::new(),
            framed: Bounds::new(),
            frame,
        }
    }

    fn group(&mut self, (order, color): Slot) -> &mut String {
        &mut self
            .groups
            .entry(order)
            .or_insert_with(|| (String::new(), color))
            .0
    }

    /// Count `point`, and `reach` around it, as drawn in layer `order`.
    /// The framing layer counts its lines' middles: a board's size is its
    /// outline's, not the width of the line that draws it.
    pub fn cover(&mut self, order: u32, point: (f64, f64), reach: f64) {
        self.all.add(point, reach);
        if self.frame == Some(order) {
            self.framed.add(point, 0.);
        }
    }

    /// Path data in layer `slot`: stroked `width` wide (none at 0), and
    /// filled when `fill` says with what.
    pub fn path(&mut self, slot: Slot, data: &str, width: f64, fill: Option<[u8; 3]>) {
        let group = self.group(slot);
        group.push_str("<path d=\"");
        group.push_str(data);
        group.push('"');
        style(group, width, fill);
        group.push_str("/>");
    }

    /// A polygon through `points`, closed or not.
    pub fn polygon(
        &mut self,
        slot: Slot,
        points: &[(f64, f64)],
        closed: bool,
        width: f64,
        fill: Option<[u8; 3]>,
    ) {
        if points.len() < 2 {
            return;
        }
        let mut data = String::with_capacity(points.len() * 16);
        for (index, point) in points.iter().enumerate() {
            data.push(if index == 0 { 'M' } else { 'L' });
            pair(&mut data, *point);
            self.cover(slot.0, *point, width / 2.);
        }
        if closed {
            data.push('Z');
        }
        self.path(slot, &data, width, fill);
    }

    pub fn line(&mut self, slot: Slot, a: (f64, f64), b: (f64, f64), width: f64) {
        self.polygon(slot, &[a, b], false, width.max(0.01), None);
    }

    pub fn circle(
        &mut self,
        slot: Slot,
        centre: (f64, f64),
        radius: f64,
        width: f64,
        fill: Option<[u8; 3]>,
    ) {
        if !radius.is_finite() || radius <= 0. {
            return;
        }
        self.cover(slot.0, centre, radius + width / 2.);
        let group = self.group(slot);
        group.push_str("<circle cx=\"");
        num(group, centre.0);
        group.push_str("\" cy=\"");
        num(group, centre.1);
        group.push_str("\" r=\"");
        num(group, radius);
        group.push('"');
        style(group, width, fill);
        group.push_str("/>");
    }

    /// The arc from the first point through the second to the third.
    pub fn arc(&mut self, slot: Slot, points: [(f64, f64); 3], width: f64, fill: Option<[u8; 3]>) {
        let mut data = String::new();
        arc_path(&mut data, points[0], points[1], points[2]);
        for point in points {
            self.cover(slot.0, point, width / 2.);
        }
        self.path(slot, &data, width.max(0.01), fill);
    }

    /// Text in the layer's colour.
    pub fn text(&mut self, slot: Slot, content: &str, place: &Placement) {
        let mut bounds = Bounds::new();
        text(self.group(slot), &mut bounds, content, place, slot.1);
        if !bounds.empty {
            self.cover(slot.0, bounds.min, 0.);
            self.cover(slot.0, bounds.max, 0.);
        }
    }

    /// The SVG on `background`, a tenth of a millimetre to its pixel,
    /// framed with a margin; the layers `translucent` names let what is
    /// under them show. `None` when nothing was drawn.
    pub fn finish(self, background: [u8; 3], translucent: impl Fn(u32) -> bool) -> Option<String> {
        let frame = if self.framed.empty {
            self.all
        } else {
            self.framed
        };
        if frame.empty {
            return None;
        }
        let margin = frame.width().max(frame.height()) * 0.03 + 0.5;
        let (x, y) = (frame.min.0 - margin, frame.min.1 - margin);
        let (w, h) = (frame.width() + 2. * margin, frame.height() + 2. * margin);
        let mut out = String::new();
        let _ = write!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"",
            (w * 10.).ceil().min(100_000.),
            (h * 10.).ceil().min(100_000.)
        );
        for (index, value) in [x, y, w, h].into_iter().enumerate() {
            if index > 0 {
                out.push(' ');
            }
            num(&mut out, value);
        }
        out.push_str("\"><rect x=\"");
        num(&mut out, x);
        out.push_str("\" y=\"");
        num(&mut out, y);
        out.push_str("\" width=\"");
        num(&mut out, w);
        out.push_str("\" height=\"");
        num(&mut out, h);
        out.push_str("\" fill=\"");
        color(&mut out, background);
        out.push_str("\"/>");
        for (order, (content, stroke)) in self.groups {
            out.push_str("<g fill=\"none\" stroke=\"");
            color(&mut out, stroke);
            out.push_str("\" stroke-linecap=\"round\" stroke-linejoin=\"round\"");
            if translucent(order) {
                out.push_str(" opacity=\"0.85\"");
            }
            out.push('>');
            out.push_str(&content);
            out.push_str("</g>");
        }
        out.push_str("</svg>");
        Some(out)
    }
}

/// A shape's stroke width (none at 0) and fill.
fn style(out: &mut String, width: f64, fill: Option<[u8; 3]>) {
    if let Some(fill) = fill {
        out.push_str(" fill=\"");
        color(out, fill);
        out.push('"');
    }
    if width > 0. {
        out.push_str(" stroke-width=\"");
        num(out, width);
        out.push('"');
    } else {
        out.push_str(" stroke=\"none\"");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short() {
        let mut out = String::new();
        num(&mut out, 1.0);
        out.push(' ');
        num(&mut out, -0.000_1);
        out.push(' ');
        num(&mut out, 12.345_678);
        assert_eq!(out, "1 0 12.346");
    }

    #[test]
    fn an_arc_through_three_points() {
        let (centre, radius) = circle_through((1., 0.), (0., 1.), (-1., 0.)).unwrap();
        assert!((centre.0).abs() < 1e-9 && (centre.1).abs() < 1e-9);
        assert!((radius - 1.).abs() < 1e-9);
        // Half a circle through the top of the screen (y down): the small
        // arc flag, turning counter-clockwise on screen.
        let mut out = String::new();
        arc_path(&mut out, (1., 0.), (0., -1.), (-1., 0.));
        assert_eq!(out, "M1,0A1,1 0 0,0 -1,0");
        // Three quarters: the large one.
        let mut out = String::new();
        let s = std::f64::consts::FRAC_1_SQRT_2;
        arc_path(&mut out, (1., 0.), (-s, -s), (0., 1.));
        assert_eq!(out, "M1,0A1,1 0 1,0 0,1");
        let mut out = String::new();
        arc_path(&mut out, (0., 0.), (1., 1.), (2., 2.));
        assert_eq!(out, "M0,0L2,2");
    }

    #[test]
    fn text_is_escaped_and_placed() {
        let mut out = String::new();
        let mut bounds = Bounds::new();
        let mut place = Placement::centred((10., 20.), 1.);
        place.angle = 90.;
        place.justify(["left"]);
        text(&mut out, &mut bounds, "R<1>", &place, [1, 2, 3]);
        assert!(out.contains("rotate(-90)"), "{out}");
        assert!(out.contains("text-anchor=\"start\""));
        assert!(out.contains(">R&lt;1&gt;</text>"));
        assert!(!bounds.empty);
    }
}
