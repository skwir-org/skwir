//! Gerber files (RS-274X), the layers a board is made from, whatever
//! drew it: each drawn alone, in the colour `KiCad` gives its layer, on
//! the board's dark background. Which layer a file is comes from its X2
//! attributes (`%TF.FileFunction,Copper,L1,Top*%`), else its name.
//!
//! A Gerber file is statements ending in `*`, and extended ones between
//! `%`: apertures (`%ADD10C,0.5*%`), their macros (`%AM…%`), then
//! flashes (`D03`), draws (`D01`) and filled regions (`G36`…`G37`), dark
//! or clear (`%LPC*%`, drawn here in the background's colour, over what
//! came before).
use crate::svg::{self, Bounds};
use std::collections::HashMap;
use std::f64::consts::TAU;
use std::fmt::Write as _;

/// `KiCad`'s board background, and its colours for what layers hold.
const BACKGROUND: [u8; 3] = [0, 16, 35];
const TOP_COPPER: [u8; 3] = [200, 52, 52];
const BOTTOM_COPPER: [u8; 3] = [77, 127, 196];
const INNER_COPPER: [u8; 3] = [127, 200, 127];
const TOP_MASK: [u8; 3] = [216, 100, 255];
const BOTTOM_MASK: [u8; 3] = [2, 255, 238];
const TOP_SILK: [u8; 3] = [242, 237, 161];
const BOTTOM_SILK: [u8; 3] = [232, 178, 167];
const TOP_PASTE: [u8; 3] = [180, 160, 154];
const BOTTOM_PASTE: [u8; 3] = [0, 194, 194];
const OUTLINE: [u8; 3] = [208, 210, 205];
const OTHER: [u8; 3] = [194, 194, 194];

/// What is drawn at most: objects, and a macro's primitives.
const OBJECTS: usize = 2_000_000;
const REPEATS: u32 = 10_000;

type Point = (f64, f64);

/// What a layer is for, as its attributes or name say.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub color: [u8; 3],
}

/// From `%TF.FileFunction,…*%`: `Copper,L1,Top`.
fn function(value: &str) -> Function {
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    let side = |at: usize| parts.get(at).copied().unwrap_or_default();
    let top = |at: usize| side(at).eq_ignore_ascii_case("top");
    let named = |name: &str, color| Function {
        name: name.to_owned(),
        color,
    };
    match parts.first().copied().unwrap_or_default() {
        "Copper" => {
            let layer = side(1).trim_start_matches('L');
            match side(2) {
                "Top" => named(&format!("Top copper (layer {layer})"), TOP_COPPER),
                "Bot" => named(&format!("Bottom copper (layer {layer})"), BOTTOM_COPPER),
                _ => named(&format!("Inner copper (layer {layer})"), INNER_COPPER),
            }
        }
        "Soldermask" if top(1) => named("Top solder mask", TOP_MASK),
        "Soldermask" => named("Bottom solder mask", BOTTOM_MASK),
        "Legend" if top(1) => named("Top silkscreen", TOP_SILK),
        "Legend" => named("Bottom silkscreen", BOTTOM_SILK),
        "Paste" if top(1) => named("Top paste", TOP_PASTE),
        "Paste" => named("Bottom paste", BOTTOM_PASTE),
        "Profile" => named("Board outline", OUTLINE),
        "Glue" if top(1) => named("Top adhesive", OTHER),
        "Glue" => named("Bottom adhesive", OTHER),
        "AssemblyDrawing" if top(1) => named("Top assembly drawing", OTHER),
        "AssemblyDrawing" => named("Bottom assembly drawing", OTHER),
        other => named(other, OTHER),
    }
}

/// From a file's name, when it has no attributes: Protel's extensions,
/// or `KiCad`'s layer names.
pub fn function_by_name(name: &str) -> Option<Function> {
    let lower = name.to_ascii_lowercase();
    let extension = lower.rsplit('.').next().unwrap_or_default();
    let named = |name: &str, color| {
        Some(Function {
            name: name.to_owned(),
            color,
        })
    };
    let has = |part: &str| lower.contains(part);
    match extension {
        "gtl" => named("Top copper", TOP_COPPER),
        "gbl" => named("Bottom copper", BOTTOM_COPPER),
        "gts" => named("Top solder mask", TOP_MASK),
        "gbs" => named("Bottom solder mask", BOTTOM_MASK),
        "gto" => named("Top silkscreen", TOP_SILK),
        "gbo" => named("Bottom silkscreen", BOTTOM_SILK),
        "gtp" => named("Top paste", TOP_PASTE),
        "gbp" => named("Bottom paste", BOTTOM_PASTE),
        "gko" | "gml" => named("Board outline", OUTLINE),
        e if e.starts_with("gm") => named("Board outline", OUTLINE),
        e if e.starts_with('g') && e[1..].bytes().all(|b| b.is_ascii_digit()) && e.len() > 1 => {
            named("Inner copper", INNER_COPPER)
        }
        _ if has("edge_cuts") || has("outline") => named("Board outline", OUTLINE),
        _ if has("f_cu") || has("-top.") => named("Top copper", TOP_COPPER),
        _ if has("b_cu") || has("-bottom.") => named("Bottom copper", BOTTOM_COPPER),
        _ if has("_cu") => named("Inner copper", INNER_COPPER),
        _ if has("f_mask") => named("Top solder mask", TOP_MASK),
        _ if has("b_mask") => named("Bottom solder mask", BOTTOM_MASK),
        _ if has("f_silk") => named("Top silkscreen", TOP_SILK),
        _ if has("b_silk") => named("Bottom silkscreen", BOTTOM_SILK),
        _ if has("f_paste") => named("Top paste", TOP_PASTE),
        _ if has("b_paste") => named("Bottom paste", BOTTOM_PASTE),
        _ => None,
    }
}

/// Whether text is a Gerber file: its start says so.
pub fn looks_like(head: &str) -> bool {
    ["%FS", "%MO", "%ADD", "%TF.", "G04 "]
        .iter()
        .any(|mark| head.contains(mark))
}

/// An aperture: its shape, in millimetres.
#[derive(Clone, Debug)]
enum Aperture {
    Circle {
        diameter: f64,
        hole: Hole,
    },
    Rectangle {
        w: f64,
        h: f64,
        hole: Hole,
    },
    Obround {
        w: f64,
        h: f64,
        hole: Hole,
    },
    Polygon {
        diameter: f64,
        vertices: u32,
        rotation: f64,
        hole: Hole,
    },
    /// A macro's primitives evaluated: shapes, and whether each is exposed.
    Shapes(Vec<(bool, Shape)>),
    /// An aperture block: what it draws, as SVG around its origin.
    Block(String, Bounds),
}

/// A standard aperture's hole: its width, and its height when it is a
/// rectangle (older files); none at 0.
type Hole = (f64, f64);

/// A shape around an aperture's origin.
#[derive(Clone, Debug)]
enum Shape {
    Circle(Point, f64),
    Polygon(Vec<Point>),
}

impl Aperture {
    /// How far it reaches from where it is flashed.
    fn reach(&self) -> f64 {
        match self {
            Aperture::Circle { diameter, .. } | Aperture::Polygon { diameter, .. } => diameter / 2.,
            Aperture::Rectangle { w, h, .. } | Aperture::Obround { w, h, .. } => {
                (w * w + h * h).sqrt() / 2.
            }
            Aperture::Shapes(shapes) => shapes
                .iter()
                .map(|(_, shape)| match shape {
                    Shape::Circle(c, r) => (c.0 * c.0 + c.1 * c.1).sqrt() + r,
                    Shape::Polygon(points) => points
                        .iter()
                        .map(|p| (p.0 * p.0 + p.1 * p.1).sqrt())
                        .fold(0., f64::max),
                })
                .fold(0., f64::max),
            Aperture::Block(_, bounds) => [bounds.min, bounds.max]
                .iter()
                .map(|p| (p.0 * p.0 + p.1 * p.1).sqrt())
                .fold(0., f64::max),
        }
    }

    /// The width a draw with it strokes.
    fn width(&self) -> f64 {
        match self {
            Aperture::Circle { diameter, .. } | Aperture::Polygon { diameter, .. } => *diameter,
            Aperture::Rectangle { w, h, .. } | Aperture::Obround { w, h, .. } => w.min(*h),
            other => other.reach() * 2.,
        }
    }
}

/// A macro's arithmetic: numbers, `$n` variables, `+ - x /`, brackets.
fn evaluate(expression: &str, variables: &HashMap<u32, f64>) -> Option<f64> {
    let tokens: Vec<char> = expression.chars().filter(|c| !c.is_whitespace()).collect();
    let mut at = 0;
    let value = sum(&tokens, &mut at, variables)?;
    (at == tokens.len()).then_some(value)
}

fn sum(tokens: &[char], at: &mut usize, variables: &HashMap<u32, f64>) -> Option<f64> {
    let mut value = product(tokens, at, variables)?;
    while let Some(&op) = tokens.get(*at) {
        match op {
            '+' => {
                *at += 1;
                value += product(tokens, at, variables)?;
            }
            '-' => {
                *at += 1;
                value -= product(tokens, at, variables)?;
            }
            _ => break,
        }
    }
    Some(value)
}

fn product(tokens: &[char], at: &mut usize, variables: &HashMap<u32, f64>) -> Option<f64> {
    let mut value = unary(tokens, at, variables)?;
    while let Some(&op) = tokens.get(*at) {
        match op {
            'x' | 'X' => {
                *at += 1;
                value *= unary(tokens, at, variables)?;
            }
            '/' => {
                *at += 1;
                value /= unary(tokens, at, variables)?;
            }
            _ => break,
        }
    }
    Some(value)
}

fn unary(tokens: &[char], at: &mut usize, variables: &HashMap<u32, f64>) -> Option<f64> {
    match tokens.get(*at)? {
        '-' => {
            *at += 1;
            Some(-unary(tokens, at, variables)?)
        }
        '+' => {
            *at += 1;
            unary(tokens, at, variables)
        }
        '(' => {
            *at += 1;
            let value = sum(tokens, at, variables)?;
            (tokens.get(*at) == Some(&')')).then(|| *at += 1)?;
            Some(value)
        }
        '$' => {
            *at += 1;
            let start = *at;
            while tokens.get(*at).is_some_and(char::is_ascii_digit) {
                *at += 1;
            }
            let n: u32 = tokens[start..*at].iter().collect::<String>().parse().ok()?;
            Some(variables.get(&n).copied().unwrap_or(0.))
        }
        _ => {
            let start = *at;
            while tokens
                .get(*at)
                .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                *at += 1;
            }
            tokens[start..*at].iter().collect::<String>().parse().ok()
        }
    }
}

/// `p` turned `degrees` counter-clockwise around the origin.
fn turn(p: Point, degrees: f64) -> Point {
    if degrees == 0. {
        return p;
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    (p.0 * cos - p.1 * sin, p.0 * sin + p.1 * cos)
}

/// A macro's primitives, their parameters given.
fn macro_shapes(body: &[String], parameters: &[f64], scale: f64) -> Vec<(bool, Shape)> {
    let mut variables: HashMap<u32, f64> = parameters
        .iter()
        .enumerate()
        .map(|(i, v)| (u32::try_from(i + 1).unwrap_or(u32::MAX), *v))
        .collect();
    let mut out = Vec::new();
    for statement in body {
        let statement = statement.trim();
        if statement.is_empty() || statement.starts_with('0') && !statement.starts_with("0.") {
            continue;
        }
        // `$4=$1x2`: a variable.
        if let Some(rest) = statement.strip_prefix('$')
            && let Some((name, expression)) = rest.split_once('=')
        {
            if let (Ok(n), Some(v)) = (name.trim().parse::<u32>(), evaluate(expression, &variables))
            {
                variables.insert(n, v);
            }
            continue;
        }
        let values: Vec<f64> = statement
            .split(',')
            .map(|field| evaluate(field, &variables).unwrap_or(0.))
            .collect();
        let v = |i: usize| values.get(i).copied().unwrap_or(0.);
        let exposed = v(1) != 0.;
        let code = v(0) as i32;
        match code {
            1 => {
                let centre = turn((v(3) * scale, v(4) * scale), v(5));
                out.push((exposed, Shape::Circle(centre, v(2) * scale / 2.)));
            }
            2 | 20 => {
                let (width, a, b) = (
                    v(2) * scale,
                    (v(3) * scale, v(4) * scale),
                    (v(5) * scale, v(6) * scale),
                );
                let length = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                if length > 0. {
                    let n = (
                        -(b.1 - a.1) / length * width / 2.,
                        (b.0 - a.0) / length * width / 2.,
                    );
                    let corners = [
                        (a.0 + n.0, a.1 + n.1),
                        (b.0 + n.0, b.1 + n.1),
                        (b.0 - n.0, b.1 - n.1),
                        (a.0 - n.0, a.1 - n.1),
                    ];
                    out.push((
                        exposed,
                        Shape::Polygon(corners.iter().map(|p| turn(*p, v(7))).collect()),
                    ));
                }
            }
            21 | 22 => {
                let (w, h) = (v(2) * scale, v(3) * scale);
                let (x, y) = (v(4) * scale, v(5) * scale);
                // 21 is centred, 22 (old) by its lower left corner.
                let (x0, y0) = if code == 21 {
                    (x - w / 2., y - h / 2.)
                } else {
                    (x, y)
                };
                let corners = [(x0, y0), (x0 + w, y0), (x0 + w, y0 + h), (x0, y0 + h)];
                out.push((
                    exposed,
                    Shape::Polygon(corners.iter().map(|p| turn(*p, v(6))).collect()),
                ));
            }
            4 => {
                let n = v(2) as usize;
                let rotation = v(5 + 2 * n);
                let points = (0..=n.min(100_000))
                    .map(|i| turn((v(3 + 2 * i) * scale, v(4 + 2 * i) * scale), rotation))
                    .collect();
                out.push((exposed, Shape::Polygon(points)));
            }
            5 => {
                let vertices = (v(2) as u32).clamp(3, 64);
                let centre = (v(3) * scale, v(4) * scale);
                let radius = v(5) * scale / 2.;
                let points = (0..vertices)
                    .map(|i| {
                        let a = TAU * f64::from(i) / f64::from(vertices);
                        turn(
                            (centre.0 + radius * a.cos(), centre.1 + radius * a.sin()),
                            v(6),
                        )
                    })
                    .collect();
                out.push((exposed, Shape::Polygon(points)));
            }
            7 => {
                // A thermal: its ring, the gaps left out.
                let centre = turn((v(1) * scale, v(2) * scale), v(6));
                let (outer, inner) = (v(3) * scale / 2., v(4) * scale / 2.);
                out.push((true, Shape::Circle(centre, outer)));
                out.push((false, Shape::Circle(centre, inner)));
            }
            6 => {
                // A moiré: its outer ring.
                let centre = turn((v(1) * scale, v(2) * scale), v(9));
                out.push((true, Shape::Circle(centre, v(3) * scale / 2.)));
            }
            _ => {}
        }
    }
    out
}

/// The picture being drawn, in the file's order.
struct Image {
    out: String,
    bounds: Bounds,
    color: [u8; 3],
    objects: usize,
    left_out: bool,
}

impl Image {
    fn fill(&self, dark: bool) -> [u8; 3] {
        if dark { self.color } else { BACKGROUND }
    }

    fn room(&mut self) -> bool {
        self.objects += 1;
        if self.objects > OBJECTS {
            self.left_out = true;
        }
        !self.left_out
    }

    /// Path data for a closed polygon, y turned down.
    fn polygon_data(out: &mut String, points: &[Point], at: Point) {
        for (index, p) in points.iter().enumerate() {
            out.push(if index == 0 { 'M' } else { 'L' });
            svg::pair(out, (at.0 + p.0, -(at.1 + p.1)));
        }
        out.push('Z');
    }

    fn circle_data(out: &mut String, centre: Point, radius: f64) {
        let (x, y) = (centre.0, -centre.1);
        out.push('M');
        svg::pair(out, (x - radius, y));
        for end in [(x + radius, y), (x - radius, y)] {
            out.push('A');
            svg::num(out, radius);
            out.push(',');
            svg::num(out, radius);
            out.push_str(" 0 0,1 ");
            svg::pair(out, end);
        }
        out.push('Z');
    }

    fn filled(&mut self, data: &str, color: [u8; 3]) {
        self.out.push_str("<path fill-rule=\"evenodd\" d=\"");
        self.out.push_str(data);
        self.out.push_str("\" fill=\"");
        svg::color(&mut self.out, color);
        self.out.push_str("\"/>");
    }

    /// An aperture flashed at `at`.
    fn flash(&mut self, aperture: &Aperture, at: Point, dark: bool) {
        if !self.room() {
            return;
        }
        let reach = aperture.reach();
        self.bounds.add((at.0, -at.1), reach);
        let color = self.fill(dark);
        let mut data = String::new();
        let hole = match aperture {
            Aperture::Circle { diameter, hole } => {
                Self::circle_data(&mut data, at, diameter / 2.);
                *hole
            }
            Aperture::Rectangle { w, h, hole } => {
                let (x, y) = (w / 2., h / 2.);
                Self::polygon_data(&mut data, &[(-x, -y), (x, -y), (x, y), (-x, y)], at);
                *hole
            }
            Aperture::Obround { w, h, hole } => {
                let r = w.min(*h) / 2.;
                let (x, y) = (w / 2. - r, h / 2. - r);
                let mut points = Vec::new();
                for (cx, cy, from) in [(x, y, 0.), (-x, y, 90.), (-x, -y, 180.), (x, -y, 270.)] {
                    for step in 0..=8 {
                        let a = (from + 90. * f64::from(step) / 8.).to_radians();
                        points.push((cx + r * a.cos(), cy + r * a.sin()));
                    }
                }
                Self::polygon_data(&mut data, &points, at);
                *hole
            }
            Aperture::Polygon {
                diameter,
                vertices,
                rotation,
                hole,
            } => {
                let points: Vec<Point> = (0..*vertices)
                    .map(|i| {
                        let a =
                            (rotation + 360. * f64::from(i) / f64::from(*vertices)).to_radians();
                        (diameter / 2. * a.cos(), diameter / 2. * a.sin())
                    })
                    .collect();
                Self::polygon_data(&mut data, &points, at);
                *hole
            }
            Aperture::Shapes(shapes) => {
                for (exposed, shape) in shapes {
                    let mut data = String::new();
                    match shape {
                        Shape::Circle(c, r) => {
                            Self::circle_data(&mut data, (at.0 + c.0, at.1 + c.1), *r);
                        }
                        Shape::Polygon(points) => Self::polygon_data(&mut data, points, at),
                    }
                    let color = if *exposed { color } else { self.fill(!dark) };
                    self.filled(&data, color);
                }
                return;
            }
            Aperture::Block(content, _) => {
                self.out.push_str("<g transform=\"translate(");
                svg::pair(&mut self.out, (at.0, -at.1));
                self.out.push_str(")\">");
                self.out.push_str(content);
                self.out.push_str("</g>");
                return;
            }
        };
        match hole {
            (w, h) if w > 0. && h > 0. => {
                let (x, y) = (w / 2., h / 2.);
                Self::polygon_data(&mut data, &[(-x, -y), (x, -y), (x, y), (-x, y)], at);
            }
            (w, _) if w > 0. => Self::circle_data(&mut data, at, w / 2.),
            _ => {}
        }
        self.filled(&data, color);
    }

    /// A draw from `a` to `b` (straight, or along `arc`'s points).
    fn draw(&mut self, aperture: &Aperture, path: &[Point], dark: bool) {
        if path.len() < 2 || !self.room() {
            return;
        }
        let color = self.fill(dark);
        if let (Aperture::Rectangle { w, h, .. }, [a, b]) = (aperture, path) {
            // A rectangle swept along a line: the hull of its two places.
            let (x, y) = (w / 2., h / 2.);
            let corners: Vec<Point> = [a, b]
                .iter()
                .flat_map(|p| {
                    [
                        (p.0 - x, p.1 - y),
                        (p.0 + x, p.1 - y),
                        (p.0 + x, p.1 + y),
                        (p.0 - x, p.1 + y),
                    ]
                })
                .collect();
            let hull = hull(corners);
            for p in &hull {
                self.bounds.add((p.0, -p.1), 0.);
            }
            let mut data = String::new();
            Self::polygon_data(&mut data, &hull, (0., 0.));
            self.filled(&data, color);
            return;
        }
        let width = aperture.width().max(0.001);
        self.out.push_str(
            "<path fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\" d=\"",
        );
        for (index, p) in path.iter().enumerate() {
            self.out.push(if index == 0 { 'M' } else { 'L' });
            svg::pair(&mut self.out, (p.0, -p.1));
            self.bounds.add((p.0, -p.1), width / 2.);
        }
        self.out.push_str("\" stroke=\"");
        svg::color(&mut self.out, color);
        self.out.push_str("\" stroke-width=\"");
        svg::num(&mut self.out, width);
        self.out.push_str("\"/>");
    }

    /// A region's contours, filled.
    fn region(&mut self, contours: &[Vec<Point>], dark: bool) {
        if !self.room() {
            return;
        }
        let mut data = String::new();
        for contour in contours.iter().filter(|c| c.len() >= 3) {
            for p in contour {
                self.bounds.add((p.0, -p.1), 0.);
            }
            Self::polygon_data(&mut data, contour, (0., 0.));
        }
        if !data.is_empty() {
            // Contours are filled each, their overlaps too.
            self.out.push_str("<path fill-rule=\"nonzero\" d=\"");
            self.out.push_str(&data);
            self.out.push_str("\" fill=\"");
            let color = self.fill(dark);
            svg::color(&mut self.out, color);
            self.out.push_str("\"/>");
        }
    }
}

/// The convex hull of points, counter-clockwise.
fn hull(mut points: Vec<Point>) -> Vec<Point> {
    points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let cross =
        |o: Point, a: Point, b: Point| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<Point> = Vec::new();
    for p in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0. {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<Point> = Vec::new();
    for p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0. {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The angle an arc from `from` to `to` around `centre` turns: negative
/// clockwise.
fn sweep(from: Point, to: Point, centre: Point, clockwise: bool) -> f64 {
    let start = (from.1 - centre.1).atan2(from.0 - centre.0);
    let end = (to.1 - centre.1).atan2(to.0 - centre.0);
    let mut sweep = end - start;
    if clockwise {
        if sweep >= -1e-9 {
            sweep -= TAU;
        }
    } else if sweep <= 1e-9 {
        sweep += TAU;
    }
    sweep
}

/// The points of an arc from `from` to `to` around `centre`. A file's
/// ends need not be as far from its centre (rounding, hand-written
/// files): the radius goes from one to the other along the way.
fn arc_points(from: Point, to: Point, centre: Point, clockwise: bool) -> Vec<Point> {
    let first = ((from.0 - centre.0).powi(2) + (from.1 - centre.1).powi(2)).sqrt();
    let last = ((to.0 - centre.0).powi(2) + (to.1 - centre.1).powi(2)).sqrt();
    let start = (from.1 - centre.1).atan2(from.0 - centre.0);
    let sweep = sweep(from, to, centre, clockwise);
    let steps = ((sweep.abs() / TAU * 96.).ceil() as usize).clamp(2, 256);
    let mut points: Vec<Point> = (0..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let a = start + sweep * t;
            let radius = first + (last - first) * t;
            (centre.0 + radius * a.cos(), centre.1 + radius * a.sin())
        })
        .collect();
    if let Some(last) = points.last_mut() {
        *last = to;
    }
    points
}

/// How coordinates are written: `%FSLAX36Y36*%`.
#[derive(Clone, Copy)]
struct Format {
    integers: usize,
    decimals: usize,
    trailing_omitted: bool,
}

impl Format {
    fn value(&self, digits: &str, unit: f64) -> Option<f64> {
        if digits.contains('.') {
            return Some(digits.parse::<f64>().ok()? * unit);
        }
        let negative = digits.starts_with('-');
        let bare = digits.trim_start_matches(['-', '+']);
        let mut bare = bare.to_owned();
        if self.trailing_omitted {
            while bare.len() < self.integers + self.decimals {
                bare.push('0');
            }
        }
        let n: f64 = bare.parse().ok()?;
        let value = n / 10f64.powi(i32::try_from(self.decimals).ok()?) * unit;
        Some(if negative { -value } else { value })
    }
}

/// A step and repeat being read: its grid, and where what it repeats
/// starts in the picture, with the bounds of what came before.
struct Repeat {
    columns: u32,
    rows: u32,
    step: Point,
    start: usize,
    outer: Bounds,
}

impl Repeat {
    /// What was drawn since it opened, stepped over its grid.
    fn close(self, image: &mut Image) {
        let content = image.out.split_off(self.start);
        let drawn = std::mem::replace(&mut image.bounds, self.outer);
        for row in 0..self.rows {
            for column in 0..self.columns {
                let offset = (
                    f64::from(column) * self.step.0,
                    -f64::from(row) * self.step.1,
                );
                image.out.push_str("<g transform=\"translate(");
                svg::pair(&mut image.out, offset);
                image.out.push_str(")\">");
                image.out.push_str(&content);
                image.out.push_str("</g>");
                if !drawn.empty {
                    image
                        .bounds
                        .add((drawn.min.0 + offset.0, drawn.min.1 + offset.1), 0.);
                    image
                        .bounds
                        .add((drawn.max.0 + offset.0, drawn.max.1 + offset.1), 0.);
                }
            }
        }
    }
}

/// What a Gerber file holds, drawn.
pub struct Drawn {
    pub svg: Option<String>,
    pub details: Vec<(String, String)>,
    pub left_out: bool,
}

/// A Gerber file drawn; `name` guesses its layer when it says nothing.
pub fn draw(text: &str, name: &str, cancel: &bibli_core::CancelToken) -> anyhow::Result<Drawn> {
    let mut format = Format {
        integers: 3,
        decimals: 6,
        trailing_omitted: false,
    };
    let mut unit = 1.;
    let mut apertures: HashMap<u32, Aperture> = HashMap::new();
    let mut macros: HashMap<String, Vec<String>> = HashMap::new();
    let mut current: Option<u32> = None;
    let mut interpolation = 1u8;
    let mut multi_quadrant = true;
    let mut region: Option<Vec<Vec<Point>>> = None;
    let mut dark = true;
    let mut at: Point = (0., 0.);
    let mut layer: Option<Function> = None;
    let mut software = None;
    let mut created = None;
    let mut repeat: Option<Repeat> = None;
    // Aperture blocks being defined, innermost last: their number and
    // where their content starts.
    let mut blocks: Vec<(u32, usize, Bounds)> = Vec::new();
    let mut image = Image {
        out: String::new(),
        bounds: Bounds::new(),
        color: function_by_name(name).map_or(OTHER, |layer| layer.color),
        objects: 0,
        left_out: false,
    };
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut statements = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        statements += 1;
        if statements.is_multiple_of(65_536) {
            bibli_platform::inspection::check(cancel)?;
        }
        if bytes[i] == b'%' {
            let end = text[i + 1..].find('%').map_or(bytes.len(), |e| i + 1 + e);
            let block: String = text[i + 1..end]
                .chars()
                .filter(|c| !c.is_whitespace() || *c == ' ')
                .collect();
            i = end + 1;
            let block = block.trim();
            if let Some(body) = block.strip_prefix("AM") {
                let mut parts = body.split('*');
                let name = parts.next().unwrap_or_default().to_owned();
                macros.insert(name, parts.map(str::to_owned).collect());
                continue;
            }
            for statement in block.split('*').filter(|s| !s.is_empty()) {
                extended(
                    statement,
                    &mut format,
                    &mut unit,
                    &mut apertures,
                    &macros,
                    &mut dark,
                    &mut layer,
                    &mut software,
                    &mut created,
                    &mut repeat,
                    &mut blocks,
                    &mut image,
                );
            }
            continue;
        }
        let end = text[i..].find('*').map_or(bytes.len(), |e| i + e);
        let statement: String = text[i..end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        i = end + 1;
        if statement.is_empty() || statement.starts_with("G04") {
            continue;
        }
        if matches!(statement.as_str(), "M02" | "M00" | "M2" | "M0") {
            break;
        }
        // Its words: `G01X100Y200D01`.
        let mut words: Vec<(char, &str)> = Vec::new();
        let mut rest = statement.as_str();
        while let Some(letter) = rest.chars().next() {
            let tail = &rest[letter.len_utf8()..];
            let length = tail
                .find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '.')))
                .unwrap_or(tail.len());
            words.push((letter.to_ascii_uppercase(), &tail[..length]));
            rest = &tail[length..];
        }
        let mut target = at;
        let (mut offset_i, mut offset_j) = (0., 0.);
        let mut operation = None;
        for (letter, value) in &words {
            match letter {
                'G' => match value.parse::<u32>().unwrap_or(u32::MAX) {
                    1 => interpolation = 1,
                    2 => interpolation = 2,
                    3 => interpolation = 3,
                    36 => region = Some(vec![Vec::new()]),
                    37 => {
                        if let Some(contours) = region.take() {
                            image.region(&contours, dark);
                        }
                    }
                    70 => unit = 25.4,
                    71 => unit = 1.,
                    74 => multi_quadrant = false,
                    75 => multi_quadrant = true,
                    _ => {}
                },
                'X' => target.0 = format.value(value, unit).unwrap_or(target.0),
                'Y' => target.1 = format.value(value, unit).unwrap_or(target.1),
                'I' => offset_i = format.value(value, unit).unwrap_or(0.),
                'J' => offset_j = format.value(value, unit).unwrap_or(0.),
                'D' => {
                    let code = value.parse::<u32>().unwrap_or(0);
                    if code >= 10 {
                        current = Some(code);
                    } else {
                        operation = Some(code);
                    }
                }
                _ => {}
            }
        }
        // Coordinates alone continue the last operation.
        let operation = operation.or_else(|| {
            words
                .iter()
                .any(|(l, _)| matches!(l, 'X' | 'Y'))
                .then_some(1)
        });
        let path = |from: Point, to: Point| -> Vec<Point> {
            if interpolation == 1 {
                return vec![from, to];
            }
            let clockwise = interpolation == 2;
            let centre = if multi_quadrant {
                (from.0 + offset_i, from.1 + offset_j)
            } else {
                // One quadrant: the centre whose arc is a quarter at most.
                let candidates = [
                    (from.0 + offset_i.abs(), from.1 + offset_j.abs()),
                    (from.0 - offset_i.abs(), from.1 + offset_j.abs()),
                    (from.0 + offset_i.abs(), from.1 - offset_j.abs()),
                    (from.0 - offset_i.abs(), from.1 - offset_j.abs()),
                ];
                candidates
                    .into_iter()
                    .min_by(|a, b| {
                        // The same distance to both ends, a quarter at most.
                        let error = |c: &Point| {
                            let r1 = ((from.0 - c.0).powi(2) + (from.1 - c.1).powi(2)).sqrt();
                            let r2 = ((to.0 - c.0).powi(2) + (to.1 - c.1).powi(2)).sqrt();
                            let wide = sweep(from, to, *c, clockwise).abs() > TAU / 4. + 1e-6;
                            (r1 - r2).abs() + if wide { 1e6 } else { 0. }
                        };
                        error(a).total_cmp(&error(b))
                    })
                    .unwrap_or(from)
            };
            if (from.0 - to.0).abs() < 1e-12 && (from.1 - to.1).abs() < 1e-12 && multi_quadrant {
                // A full circle.
                let radius = ((from.0 - centre.0).powi(2) + (from.1 - centre.1).powi(2)).sqrt();
                return (0..=96)
                    .map(|s| {
                        let a = (from.1 - centre.1).atan2(from.0 - centre.0)
                            + if clockwise { -1. } else { 1. } * TAU * f64::from(s) / 96.;
                        (centre.0 + radius * a.cos(), centre.1 + radius * a.sin())
                    })
                    .collect();
            }
            arc_points(from, to, centre, clockwise)
        };
        match operation {
            Some(1) => {
                let points = path(at, target);
                if let Some(contours) = &mut region {
                    let contour = contours.last_mut().expect("a region has a contour");
                    if contour.is_empty() {
                        contour.push(at);
                    }
                    contour.extend(points.into_iter().skip(1));
                } else if let Some(aperture) = current.and_then(|c| apertures.get(&c)) {
                    let aperture = aperture.clone();
                    image.draw(&aperture, &points, dark);
                }
            }
            Some(2) => {
                if let Some(contours) = &mut region
                    && contours.last().is_some_and(|c| !c.is_empty())
                {
                    contours.push(Vec::new());
                }
            }
            Some(3) => {
                if let Some(aperture) = current.and_then(|c| apertures.get(&c)) {
                    let aperture = aperture.clone();
                    image.flash(&aperture, target, dark);
                }
            }
            _ => {}
        }
        at = target;
    }
    // A repeat still open ends with the file.
    if let Some(open) = repeat.take() {
        open.close(&mut image);
    }
    let layer = layer.or_else(|| function_by_name(name));
    let mut details = Vec::new();
    if let Some(layer) = &layer {
        details.push(("Layer".to_owned(), layer.name.clone()));
    }
    if let Some(software) = software {
        details.push(("Made with".into(), software));
    }
    if let Some(created) = created {
        details.push(("Created".into(), created));
    }
    if !image.bounds.empty {
        details.push((
            "Size".into(),
            format!(
                "{} × {} mm",
                (image.bounds.width() * 100.).round() / 100.,
                (image.bounds.height() * 100.).round() / 100.
            ),
        ));
    }
    details.push(("Apertures".into(), apertures.len().to_string()));
    // An outline is thin lines only: at least a quarter of a percent of
    // the board wide, so a thumbnail still shows it.
    if layer.as_ref().is_some_and(|layer| layer.color == OUTLINE) && !image.bounds.empty {
        let least = image.bounds.width().max(image.bounds.height()) / 400.;
        image.out = thicken(&image.out, least);
    }
    let svg = finish(&image);
    Ok(Drawn {
        svg,
        details,
        left_out: image.left_out,
    })
}

/// An extended statement: formats, units, apertures, polarity, blocks,
/// repeats and attributes.
#[allow(clippy::too_many_arguments)]
fn extended(
    statement: &str,
    format: &mut Format,
    unit: &mut f64,
    apertures: &mut HashMap<u32, Aperture>,
    macros: &HashMap<String, Vec<String>>,
    dark: &mut bool,
    layer: &mut Option<Function>,
    software: &mut Option<String>,
    created: &mut Option<String>,
    repeat: &mut Option<Repeat>,
    blocks: &mut Vec<(u32, usize, Bounds)>,
    image: &mut Image,
) {
    if let Some(spec) = statement.strip_prefix("FS") {
        format.trailing_omitted = spec.starts_with('T');
        if let Some(x) = spec.find('X')
            && let (Some(i), Some(d)) = (
                spec[x + 1..].chars().next().and_then(|c| c.to_digit(10)),
                spec[x + 1..].chars().nth(1).and_then(|c| c.to_digit(10)),
            )
        {
            format.integers = i as usize;
            format.decimals = d as usize;
        }
    } else if let Some(mode) = statement.strip_prefix("MO") {
        *unit = if mode.starts_with("IN") { 25.4 } else { 1. };
    } else if let Some(polarity) = statement.strip_prefix("LP") {
        *dark = !polarity.starts_with('C');
    } else if let Some(definition) = statement.strip_prefix("ADD") {
        let digits = definition.bytes().take_while(u8::is_ascii_digit).count();
        let Ok(code) = definition[..digits].parse::<u32>() else {
            return;
        };
        let rest = &definition[digits..];
        let (name, parameters) = rest.split_once(',').unwrap_or((rest, ""));
        let values: Vec<f64> = parameters
            .split(['X', 'x'])
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        let v = |i: usize| values.get(i).copied().unwrap_or(0.) * *unit;
        let aperture = match name {
            "C" => Aperture::Circle {
                diameter: v(0),
                hole: (v(1), v(2)),
            },
            "R" => Aperture::Rectangle {
                w: v(0),
                h: v(1),
                hole: (v(2), v(3)),
            },
            "O" => Aperture::Obround {
                w: v(0),
                h: v(1),
                hole: (v(2), v(3)),
            },
            "P" => Aperture::Polygon {
                diameter: v(0),
                vertices: (values.get(1).copied().unwrap_or(3.) as u32).clamp(3, 64),
                rotation: values.get(2).copied().unwrap_or(0.),
                hole: (v(3), v(4)),
            },
            name => match macros.get(name) {
                Some(body) => Aperture::Shapes(macro_shapes(body, &values, *unit)),
                None => return,
            },
        };
        apertures.insert(code, aperture);
    } else if let Some(block) = statement.strip_prefix("AB") {
        if let Some(code) = block.strip_prefix('D').and_then(|c| c.parse::<u32>().ok()) {
            blocks.push((code, image.out.len(), image.bounds));
            image.bounds = Bounds::new();
        } else if let Some((code, start, outer)) = blocks.pop() {
            let content = image.out.split_off(start);
            let inner = image.bounds;
            image.bounds = outer;
            apertures.insert(code, Aperture::Block(content, inner));
        }
    } else if let Some(spec) = statement.strip_prefix("SR") {
        // Close a repeat that was open.
        if let Some(open) = repeat.take() {
            open.close(image);
        }
        let field = |letter: char| -> Option<f64> {
            let at = spec.find(letter)?;
            let rest = &spec[at + 1..];
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-')))
                .unwrap_or(rest.len());
            rest[..end].parse().ok()
        };
        let (x, y) = (
            field('X').unwrap_or(1.) as u32,
            field('Y').unwrap_or(1.) as u32,
        );
        if (x > 1 || y > 1) && u64::from(x) * u64::from(y) <= u64::from(REPEATS) {
            *repeat = Some(Repeat {
                columns: x,
                rows: y,
                step: (
                    field('I').unwrap_or(0.) * *unit,
                    field('J').unwrap_or(0.) * *unit,
                ),
                start: image.out.len(),
                outer: std::mem::replace(&mut image.bounds, Bounds::new()),
            });
        }
    } else if let Some(attribute) = statement.strip_prefix("TF.") {
        let (key, value) = attribute.split_once(',').unwrap_or((attribute, ""));
        match key {
            "FileFunction" => {
                let function = function(value);
                image.color = function.color;
                *layer = Some(function);
            }
            "GenerationSoftware" => {
                let words: Vec<&str> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|w| !w.is_empty())
                    .collect();
                *software = Some(words.join(" "));
            }
            "CreationDate" => *created = Some(value.replace('T', " ").chars().take(16).collect()),
            _ => {}
        }
    }
}

/// `svg` with every stroke at least `least` wide.
fn thicken(svg: &str, least: f64) -> String {
    const WIDTH: &str = "stroke-width=\"";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(at) = rest.find(WIDTH) {
        out.push_str(&rest[..at + WIDTH.len()]);
        rest = &rest[at + WIDTH.len()..];
        let end = rest.find('"').unwrap_or(rest.len());
        let width: f64 = rest[..end].parse().unwrap_or(0.);
        svg::num(&mut out, width.max(least));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The picture: framed around what was drawn, on the board's background.
fn finish(image: &Image) -> Option<String> {
    if image.bounds.empty {
        return None;
    }
    let bounds = image.bounds;
    let margin = bounds.width().max(bounds.height()) * 0.03 + 0.2;
    let (x, y) = (bounds.min.0 - margin, bounds.min.1 - margin);
    let (w, h) = (bounds.width() + 2. * margin, bounds.height() + 2. * margin);
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
        svg::num(&mut out, value);
    }
    out.push_str("\"><rect x=\"");
    svg::num(&mut out, x);
    out.push_str("\" y=\"");
    svg::num(&mut out, y);
    out.push_str("\" width=\"");
    svg::num(&mut out, w);
    out.push_str("\" height=\"");
    svg::num(&mut out, h);
    out.push_str("\" fill=\"");
    svg::color(&mut out, BACKGROUND);
    out.push_str("\"/>");
    out.push_str(&image.out);
    out.push_str("</svg>");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::CancelToken;

    #[test]
    fn a_kicad_layer_with_a_macro_a_region_and_clear_polarity() {
        let text = "%TF.GenerationSoftware,KiCad,Pcbnew,9.0*%\n%TF.FileFunction,Copper,L1,Top*%\n%FSLAX46Y46*%\n%MOMM*%\n%LPD*%\n\
%AMRoundRect*\n0 comment*\n4,1,4,$2,$3,$4,$5,$6,$7,$8,$9,$2,$3,0*\n1,1,$1+$1,$2,$3*%\n\
%ADD10C,0.250000*%\n%ADD11RoundRect,0.25X-1X-1X1X-1X1X1X-1X1X0*%\n\
D10*\nX0Y0D02*\nX10000000Y0D01*\nD11*\nX5000000Y5000000D03*\n\
G36*\nX0Y10000000D02*\nX10000000Y10000000D01*\nX10000000Y20000000D01*\nX0Y20000000D01*\nX0Y10000000D01*\nG37*\n\
%LPC*%\nD10*\nX5000000Y15000000D03*\nM02*\n";
        let drawn = draw(text, "board-F_Cu.gbr", &CancelToken::new()).unwrap();
        let svg = drawn.svg.unwrap();
        assert!(svg.contains("#c83434"), "top copper's colour: {svg}");
        assert!(
            svg.contains("#001023"),
            "the clear flash in the background's"
        );
        assert_eq!(
            drawn.details,
            [
                ("Layer".to_owned(), "Top copper (layer 1)".to_owned()),
                ("Made with".to_owned(), "KiCad Pcbnew 9.0".to_owned()),
                ("Size".to_owned(), "10.25 × 20.13 mm".to_owned()),
                ("Apertures".to_owned(), "2".to_owned())
            ]
        );
        let raster = bibli_platform::inspection::drawing(svg.as_bytes(), Some(200)).unwrap();
        assert_eq!(raster.width.max(raster.height), 200);
    }

    #[test]
    fn an_outline_is_drawn_wide_enough_to_see() {
        assert_eq!(
            thicken("<path stroke-width=\"0.1\"/><path stroke-width=\"3\"/>", 1.),
            "<path stroke-width=\"1\"/><path stroke-width=\"3\"/>"
        );
    }

    #[test]
    fn macro_arithmetic() {
        let variables = HashMap::from([(1, 2.), (2, 3.)]);
        assert_eq!(evaluate("$1+$1", &variables), Some(4.));
        assert_eq!(evaluate("$1x$2-1", &variables), Some(5.));
        assert_eq!(evaluate("-($1+1)/3", &variables), Some(-1.));
        assert_eq!(evaluate("2x", &variables), None);
    }

    #[test]
    fn coordinates_by_their_format() {
        let leading = Format {
            integers: 2,
            decimals: 4,
            trailing_omitted: false,
        };
        assert_eq!(leading.value("15000", 1.), Some(1.5));
        assert_eq!(leading.value("-5", 1.), Some(-0.0005));
        let trailing = Format {
            trailing_omitted: true,
            ..leading
        };
        assert_eq!(trailing.value("15", 1.), Some(15.));
        assert_eq!(leading.value("1.25", 25.4), Some(31.75));
    }

    #[test]
    fn layers_by_name() {
        assert_eq!(function_by_name("board.GBL").unwrap().name, "Bottom copper");
        assert_eq!(
            function_by_name("board-In2_Cu.g2").unwrap().name,
            "Inner copper"
        );
        assert_eq!(
            function_by_name("board-Edge_Cuts.gm1").unwrap().name,
            "Board outline"
        );
        assert_eq!(
            function_by_name("board-Edge_Cuts.gbr").unwrap().name,
            "Board outline"
        );
        assert!(function_by_name("notes.gbr").is_none());
        assert!(looks_like("G04 made by hand*\n%FSLAX24Y24*%"));
        assert!(!looks_like("GIMP brush"));
    }

    #[test]
    fn a_step_and_repeat_and_a_single_quadrant_arc() {
        let text = "%FSLAX24Y24*%\n%MOIN*%\n%ADD10C,0.01*%\n%SRX3Y2I1.0J1.0*%\nD10*\nX0Y0D02*\nG74*\nG02X1000Y1000I1000J0D01*\n%SR*%\nM02*\n";
        let drawn = draw(text, "panel.gbr", &CancelToken::new()).unwrap();
        let svg = drawn.svg.unwrap();
        assert_eq!(svg.matches("<g transform=").count(), 6);
        // Three columns an inch apart, the last arc an inch wide.
        assert!(
            drawn
                .details
                .iter()
                .any(|(l, v)| l == "Size" && v == "53.59 × 28.19 mm"),
            "{:?}",
            drawn.details
        );
    }
}
