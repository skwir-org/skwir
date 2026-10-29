//! G-code drawn from its moves, for a program that stores no picture: a
//! print as a slicer previews it, its extrusions seen from the front, right
//! and above in the colours of their features (or of their height); a
//! milling, engraving or laser program seen from above, its cuts by depth
//! and its rapid moves faint. The file is read twice, as a stream: once for
//! what it covers, once to draw; nothing but the picture is held.
use skwir_core::CancelToken;
use skwir_core::preview::FileSource;
use skwir_core::thumbs::Image;
use skwir_platform::inspection;
use std::io::{BufRead, Read};
use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// The largest program drawn; it is streamed, this bounds the time.
const BYTES: u64 = 256 * 1024 * 1024;
/// The view of a print: turned about the vertical and tilted towards the
/// viewer, as the mesh helper draws models.
const AZIMUTH: f64 = 35.;
const ELEVATION: f64 = 28.;
/// A CNC program's background: `AutoCAD`'s, as DXF drawings are drawn on.
const BACKGROUND: [u8; 3] = [33, 40, 48];
/// Segments drawn in one stroke at most.
const BATCH: usize = 4096;

/// What a program is, from its moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A 3D print: moves that extrude.
    Print,
    /// Milling, engraving, cutting: moves at feed rate.
    Machining,
}

/// A move, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Move {
    Rapid,
    /// At feed rate; for a print, whether it extrudes.
    Feed {
        extrudes: bool,
    },
}

/// A print's feature, by the comment its slicer writes (`;TYPE:`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Feature {
    Unknown,
    Perimeter,
    External,
    Overhang,
    Infill,
    Solid,
    Top,
    Bridge,
    Gap,
    Ironing,
    Skirt,
    Support,
    Interface,
    Tower,
    Custom,
}

impl Feature {
    /// A slicer's name for it: `External perimeter` (Prusa), `WALL-OUTER`
    /// (Cura), `Outer wall` (Orca and Bambu).
    fn named(name: &str) -> Feature {
        let name = name.trim().to_ascii_lowercase();
        let has = |word: &str| name.contains(word);
        if has("overhang") {
            Feature::Overhang
        } else if has("external") || has("outer") {
            Feature::External
        } else if has("interface") {
            Feature::Interface
        } else if has("support") {
            Feature::Support
        } else if has("perimeter") || has("wall") {
            Feature::Perimeter
        } else if has("top") {
            Feature::Top
        } else if has("bridge") {
            Feature::Bridge
        } else if has("solid") || has("skin") || has("bottom") {
            Feature::Solid
        } else if has("infill") || has("fill") {
            Feature::Infill
        } else if has("gap") {
            Feature::Gap
        } else if has("iron") {
            Feature::Ironing
        } else if has("skirt") || has("brim") {
            Feature::Skirt
        } else if has("tower") || has("prime") || has("wipe") {
            Feature::Tower
        } else {
            Feature::Custom
        }
    }

    /// `PrusaSlicer`'s colours for them.
    fn color(self) -> [u8; 3] {
        match self {
            Feature::Unknown | Feature::Custom => [94, 209, 148],
            Feature::Perimeter => [255, 230, 77],
            Feature::External => [255, 125, 56],
            Feature::Overhang => [0, 0, 255],
            Feature::Infill => [176, 48, 41],
            Feature::Solid => [150, 84, 204],
            Feature::Top => [240, 64, 64],
            Feature::Bridge => [77, 128, 186],
            Feature::Gap => [255, 255, 255],
            Feature::Ironing => [255, 140, 105],
            Feature::Skirt => [0, 135, 110],
            Feature::Support => [0, 255, 0],
            Feature::Interface => [0, 128, 0],
            Feature::Tower => [179, 227, 171],
        }
    }
}

/// How a program gives a quantity: where it is, or how far to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Absolute,
    Relative,
}

/// Whether two points differ at all.
fn apart(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).any(|(a, b)| (a - b).abs() > 1e-12)
}

/// Whether a move goes anywhere across the bed, not only up or down.
fn across(a: [f64; 3], b: [f64; 3]) -> bool {
    (a[0] - b[0]).abs() > 1e-12 || (a[1] - b[1]).abs() > 1e-12
}

/// A program's state as it is read: position, modes, and what moves
/// there were.
struct Machine {
    at: [f64; 3],
    e: f64,
    /// Positions (`G90`, `G91`), extrusion (`M82`, `M83`) and arc centres
    /// (`G90.1`, `G91.1`: from the start, the default).
    positions: Mode,
    extrusion: Mode,
    arc_centres: Mode,
    /// Millimetres, or inches (`G20`): how much a unit is.
    unit: f64,
    /// The motion that words alone continue: 0, 1, 2 or 3.
    motion: u8,
    /// The arc plane: 17 XY, 18 XZ, 19 YZ.
    plane: u8,
    feature: Feature,
    typed: bool,
}

impl Machine {
    fn new() -> Machine {
        Machine {
            at: [0.; 3],
            e: 0.,
            positions: Mode::Absolute,
            extrusion: Mode::Absolute,
            arc_centres: Mode::Relative,
            unit: 1.,
            motion: 0,
            plane: 17,
            feature: Feature::Unknown,
            typed: false,
        }
    }

    /// One line: `sink` receives each segment with how it moved.
    fn line(&mut self, line: &str, sink: &mut impl FnMut([f64; 3], [f64; 3], Move, Feature)) {
        let (code, comment) = match line.find(';') {
            Some(at) => (&line[..at], &line[at + 1..]),
            None => (line, ""),
        };
        // `;TYPE:Perimeter`, `; FEATURE: Outer wall`.
        let comment = comment.trim();
        for prefix in ["TYPE:", "FEATURE:"] {
            if let Some(name) = comment.strip_prefix(prefix) {
                self.feature = Feature::named(name);
                self.typed = true;
            }
        }
        let words = words(code);
        if words.is_empty() {
            return;
        }
        let mut target: [Option<f64>; 3] = [None; 3];
        let (mut e, mut i, mut j, mut k, mut r) = (None, None, None, None, None);
        let mut moved = false;
        let mut set_position = false;
        let mut home = false;
        for &(letter, value) in &words {
            match letter {
                'G' => match value {
                    0. => (self.motion, moved) = (0, true),
                    1. => (self.motion, moved) = (1, true),
                    2. => (self.motion, moved) = (2, true),
                    3. => (self.motion, moved) = (3, true),
                    17. | 18. | 19. => self.plane = value as u8,
                    20. => self.unit = 25.4,
                    21. => self.unit = 1.,
                    28. => home = true,
                    90. => self.positions = Mode::Absolute,
                    91. => self.positions = Mode::Relative,
                    92. => set_position = true,
                    // Arc centres absolute or from the start.
                    v if (v - 90.1).abs() < 1e-6 => self.arc_centres = Mode::Absolute,
                    v if (v - 91.1).abs() < 1e-6 => self.arc_centres = Mode::Relative,
                    _ => {}
                },
                'M' => match value {
                    82. => self.extrusion = Mode::Absolute,
                    83. => self.extrusion = Mode::Relative,
                    _ => {}
                },
                'X' => target[0] = Some(value),
                'Y' => target[1] = Some(value),
                'Z' => target[2] = Some(value),
                'E' => e = Some(value),
                'I' => i = Some(value),
                'J' => j = Some(value),
                'K' => k = Some(value),
                'R' => r = Some(value),
                _ => {}
            }
        }
        let scale = self.unit;
        if set_position {
            for (at, value) in self.at.iter_mut().zip(target) {
                if let Some(value) = value {
                    *at = value * scale;
                }
            }
            if let Some(value) = e {
                self.e = value;
            }
            return;
        }
        if home {
            // Back to the origin, on the axes named or all of them.
            let all = target.iter().all(Option::is_none);
            let mut to = self.at;
            for (to, value) in to.iter_mut().zip(target) {
                if all || value.is_some() {
                    *to = 0.;
                }
            }
            sink(self.at, to, Move::Rapid, self.feature);
            self.at = to;
            return;
        }
        let has_axis = target.iter().any(Option::is_some);
        if !has_axis && !moved && e.is_none() {
            return;
        }
        let mut to = self.at;
        for ((to, at), value) in to.iter_mut().zip(self.at).zip(target) {
            if let Some(value) = value {
                *to = match self.positions {
                    Mode::Absolute => value * scale,
                    Mode::Relative => at + value * scale,
                };
            }
        }
        let extrudes = match e {
            Some(value) if self.extrusion == Mode::Absolute => {
                let more = value > self.e + 1e-9;
                self.e = value;
                more
            }
            Some(value) => value > 1e-9,
            None => false,
        };
        let kind = if self.motion == 0 {
            Move::Rapid
        } else {
            Move::Feed { extrudes }
        };
        if matches!(self.motion, 2 | 3)
            && (i.is_some() || j.is_some() || k.is_some() || r.is_some())
        {
            let offsets = [i, j, k].map(|o| o.map(|o| o * scale));
            self.arc(to, offsets, r.map(|r| r * scale), kind, sink);
        } else if apart(to, self.at) {
            sink(self.at, to, kind, self.feature);
        }
        self.at = to;
    }

    /// An arc from where the machine is to `to`, clockwise for G2, in the
    /// plane chosen; the third axis moves along it (a helix).
    fn arc(
        &self,
        to: [f64; 3],
        offsets: [Option<f64>; 3],
        radius: Option<f64>,
        kind: Move,
        sink: &mut impl FnMut([f64; 3], [f64; 3], Move, Feature),
    ) {
        // The plane's axes, and the one across it.
        let (a, b, c) = match self.plane {
            18 => (2, 0, 1),
            19 => (1, 2, 0),
            _ => (0, 1, 2),
        };
        let from = self.at;
        let (sa, sb) = (from[a], from[b]);
        let (ea, eb) = (to[a], to[b]);
        let clockwise = self.motion == 2;
        let centre = if let Some(radius) = radius {
            // The centre on the side R's sign says.
            let (dx, dy) = (ea - sa, eb - sb);
            let chord = (dx * dx + dy * dy).sqrt();
            if chord < 1e-9 || radius.abs() < chord / 2. - 1e-6 {
                sink(from, to, kind, self.feature);
                return;
            }
            let h = (radius * radius - chord * chord / 4.).max(0.).sqrt();
            let (mx, my) = (f64::midpoint(sa, ea), f64::midpoint(sb, eb));
            let (nx, ny) = (-dy / chord, dx / chord);
            let side = if clockwise == (radius > 0.) { -1. } else { 1. };
            (mx + side * nx * h, my + side * ny * h)
        } else {
            let offset = |axis: usize| offsets[axis].unwrap_or(0.);
            match self.arc_centres {
                Mode::Relative => (sa + offset(a), sb + offset(b)),
                Mode::Absolute => (offset(a), offset(b)),
            }
        };
        let radius = ((sa - centre.0).powi(2) + (sb - centre.1).powi(2)).sqrt();
        let start = (sb - centre.1).atan2(sa - centre.0);
        let end = (eb - centre.1).atan2(ea - centre.0);
        let mut sweep = end - start;
        if clockwise {
            if sweep >= -1e-9 {
                sweep -= std::f64::consts::TAU;
            }
        } else if sweep <= 1e-9 {
            sweep += std::f64::consts::TAU;
        }
        let steps = ((sweep.abs() / std::f64::consts::TAU * 72.).ceil() as usize).clamp(2, 256);
        let mut last = from;
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let angle = start + sweep * t;
            let mut point = to;
            point[a] = centre.0 + radius * angle.cos();
            point[b] = centre.1 + radius * angle.sin();
            point[c] = from[c] + (to[c] - from[c]) * t;
            if step == steps {
                point = to;
            }
            sink(last, point, kind, self.feature);
            last = point;
        }
    }
}

/// A line's words, its parenthesised comments left out: `G1 X2.5` is
/// `[('G', 1.), ('X', 2.5)]`. A line that computes (`#1`, `[…]`, `O100`)
/// is not followed.
fn words(code: &str) -> Vec<(char, f64)> {
    let mut out = Vec::new();
    let mut chars = code.char_indices().peekable();
    let mut depth = 0u32;
    while let Some((at, ch)) = chars.next() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            '#' | '[' => return Vec::new(),
            c if c.is_ascii_alphabetic() => {
                let letter = c.to_ascii_uppercase();
                if letter == 'O' {
                    return Vec::new();
                }
                let start = at + c.len_utf8();
                let mut end = start;
                while let Some(&(i, d)) = chars.peek() {
                    if d.is_ascii_digit()
                        || matches!(d, '.' | '-' | '+')
                        || (d == ' ' && end == start)
                    {
                        end = i + d.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Ok(value) = code[start..end].trim().parse::<f64>() {
                    out.push((letter, value));
                    // A message's text is not words: `M117 Layer 2`.
                    if letter == 'M' && matches!(value as u32, 117 | 118) {
                        return out;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Every segment of the program `source` holds, fed to `sink`.
fn read<S: FileSource + ?Sized>(
    source: &S,
    cancel: &CancelToken,
    sink: &mut impl FnMut([f64; 3], [f64; 3], Move, Feature),
) -> anyhow::Result<bool> {
    let file = inspection::open(source)?;
    let mut reader = std::io::BufReader::with_capacity(256 * 1024, file.take(BYTES));
    let mut machine = Machine::new();
    let mut line = Vec::new();
    let mut count = 0usize;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        count += 1;
        if count.is_multiple_of(65_536) {
            inspection::check(cancel)?;
        }
        // Not text: not a program.
        if line.contains(&0) {
            return Ok(machine.typed);
        }
        machine.line(&String::from_utf8_lossy(&line), sink);
    }
    Ok(machine.typed)
}

/// Where a point shows, before the picture is framed.
fn project(kind: Kind, [x, y, z]: [f64; 3]) -> (f64, f64) {
    match kind {
        Kind::Machining => (x, -y),
        Kind::Print => {
            let (sa, ca) = AZIMUTH.to_radians().sin_cos();
            let (se, ce) = ELEVATION.to_radians().sin_cos();
            let across = x * ca + y * sa;
            let depth = -x * sa + y * ca;
            (across, -(z * ce + depth * se))
        }
    }
}

/// A colour between two, `t` along.
fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0., 1.);
    [0, 1, 2].map(|i| (f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * t).round() as u8)
}

/// `bounds` grown to hold `p`.
fn grow(bounds: &mut [[f64; 3]; 2], p: [f64; 3]) {
    for axis in 0..3 {
        bounds[0][axis] = bounds[0][axis].min(p[axis]);
        bounds[1][axis] = bounds[1][axis].max(p[axis]);
    }
}

/// What was drawn, and its facts.
pub struct Drawn {
    pub picture: Image,
    pub details: Vec<(String, String)>,
}

/// The program `source` holds drawn at most `side` pixels on its longer
/// side; `None` when it moves nowhere worth drawing.
pub fn draw<S: FileSource + ?Sized>(
    source: &S,
    side: u32,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Drawn>> {
    // First: what kind of program, and what its work covers.
    let mut extrusions = 0usize;
    let mut feeds = 0usize;
    let mut print = [f64::INFINITY, f64::NEG_INFINITY].map(|v| [v; 3]);
    let mut work = print;
    let typed = read(source, cancel, &mut |from, to, kind, _| {
        if let Move::Feed { extrudes } = kind {
            feeds += 1;
            grow(&mut work, from);
            grow(&mut work, to);
            if extrudes && across(from, to) {
                extrusions += 1;
                grow(&mut print, from);
                grow(&mut print, to);
            }
        }
    })?;
    let (kind, bounds) = if extrusions > 0 {
        (Kind::Print, print)
    } else if feeds > 0 {
        (Kind::Machining, work)
    } else {
        return Ok(None);
    };
    // The frame, from the corners of what the work covers.
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for corner in 0..8 {
        let p = [0, 1, 2].map(|axis| bounds[(corner >> axis) & 1][axis]);
        let (u, v) = project(kind, p);
        min = (min.0.min(u), min.1.min(v));
        max = (max.0.max(u), max.1.max(v));
    }
    let (w, h) = (max.0 - min.0, max.1 - min.1);
    let extent = w.max(h);
    if !extent.is_finite() || extent <= 0. {
        return Ok(None);
    }
    let side = f64::from(side.clamp(16, 4096));
    let margin = side * 0.04;
    let scale = (side - 2. * margin) / extent;
    let (pw, ph) = (
        (w * scale + 2. * margin).ceil().min(side),
        (h * scale + 2. * margin).ceil().min(side),
    );
    let mut pixmap = Pixmap::new(pw.max(1.) as u32, ph.max(1.) as u32)
        .ok_or_else(|| anyhow::anyhow!("The picture has no size"))?;
    if kind == Kind::Machining {
        let [r, g, b] = BACKGROUND;
        pixmap.fill(Color::from_rgba8(r, g, b, 255));
    }
    let to_pixel = |p: [f64; 3]| {
        let (u, v) = project(kind, p);
        (
            ((u - min.0) * scale + margin) as f32,
            ((v - min.1) * scale + margin) as f32,
        )
    };
    // Then: draw, a stroke per run of one colour.
    let width = match kind {
        // An extrusion about as wide as a nozzle's line, at least a pixel.
        Kind::Print => (0.45 * scale).clamp(1., 12.) as f32,
        Kind::Machining => ((side / 800.).max(1.)) as f32,
    };
    let (low, high) = (bounds[0][2], bounds[1][2]);
    let depth = |z: f64| {
        if high - low > 1e-9 {
            (z - low) / (high - low)
        } else {
            1.
        }
    };
    let mut batch = Batch {
        builder: PathBuilder::new(),
        color: [0; 3],
        opacity: 1.,
        segments: 0,
        pen: None,
        cursor: None,
        width,
    };
    read(source, cancel, &mut |from, to, how, feature| {
        let (color, opacity) = match (kind, how) {
            (Kind::Print, Move::Feed { extrudes: true }) => {
                if !across(from, to) {
                    return;
                }
                if typed {
                    (feature.color(), 1.)
                } else {
                    (mix([77, 128, 186], [255, 125, 56], depth(to[2])), 1.)
                }
            }
            (Kind::Print, _) => return,
            (Kind::Machining, Move::Rapid) => ([96, 104, 114], 0.6),
            (Kind::Machining, Move::Feed { .. }) => {
                // Shallow cuts light, the deepest orange.
                (
                    mix([255, 125, 56], [111, 211, 255], depth(to[2].min(from[2]))),
                    1.,
                )
            }
        };
        batch.segment(&mut pixmap, to_pixel(from), to_pixel(to), color, opacity);
    })?;
    batch.flush(&mut pixmap);
    let picture = Image {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba: pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect::<Vec<u8>>()
            .into(),
    };
    let length = |axis: usize| crate::gcode::short(bounds[1][axis] - bounds[0][axis]);
    let details = vec![(
        "Size".to_owned(),
        format!("{} × {} × {} mm", length(0), length(1), length(2)),
    )];
    Ok(Some(Drawn { picture, details }))
}

/// Segments gathered into strokes of one colour, a pixel's detail merged:
/// the path goes on from where it was drawn to (`pen`) only once the tool
/// (`cursor`) is a pixel away, and ends where the tool is.
struct Batch {
    builder: PathBuilder,
    color: [u8; 3],
    opacity: f32,
    segments: usize,
    pen: Option<(f32, f32)>,
    cursor: Option<(f32, f32)>,
    width: f32,
}

/// Whether two points are within `d` pixels on both axes.
fn near(a: (f32, f32), b: (f32, f32), d: f32) -> bool {
    (a.0 - b.0).abs() < d && (a.1 - b.1).abs() < d
}

impl Batch {
    fn segment(
        &mut self,
        pixmap: &mut Pixmap,
        a: (f32, f32),
        b: (f32, f32),
        color: [u8; 3],
        opacity: f32,
    ) {
        if color != self.color
            || (opacity - self.opacity).abs() > f32::EPSILON
            || self.segments >= BATCH
        {
            self.flush(pixmap);
            self.color = color;
            self.opacity = opacity;
        }
        if !self.cursor.is_some_and(|cursor| near(cursor, a, 0.5)) {
            self.tail();
            self.builder.move_to(a.0, a.1);
            self.pen = Some(a);
        }
        self.cursor = Some(b);
        if self.pen.is_some_and(|pen| !near(pen, b, 0.75)) {
            self.builder.line_to(b.0, b.1);
            self.pen = Some(b);
            self.segments += 1;
        }
    }

    /// The last stretch, shorter than a pixel, drawn: a dot at least.
    fn tail(&mut self) {
        if let (Some(pen), Some(cursor)) = (self.pen, self.cursor)
            && pen != cursor
        {
            self.builder.line_to(cursor.0, cursor.1);
            self.pen = Some(cursor);
        }
    }

    fn flush(&mut self, pixmap: &mut Pixmap) {
        self.tail();
        let builder = std::mem::replace(&mut self.builder, PathBuilder::new());
        self.segments = 0;
        self.pen = None;
        self.cursor = None;
        let Some(path) = builder.finish() else {
            return;
        };
        let mut paint = Paint::default();
        let [r, g, b] = self.color;
        paint.set_color(Color::from_rgba8(r, g, b, (self.opacity * 255.) as u8));
        paint.anti_alias = true;
        let stroke = Stroke {
            width: self.width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(program: &str) -> Vec<([f64; 3], [f64; 3], Move)> {
        let mut machine = Machine::new();
        let mut out = Vec::new();
        for line in program.lines() {
            machine.line(line, &mut |a, b, m, _| out.push((a, b, m)));
        }
        out
    }

    #[test]
    fn words_and_comments() {
        assert_eq!(
            words("G1 X2.5 Y-3 (a comment X9) F1200"),
            [('G', 1.), ('X', 2.5), ('Y', -3.), ('F', 1200.)]
        );
        assert_eq!(words("g0x1y2"), [('G', 0.), ('X', 1.), ('Y', 2.)]);
        assert!(words("G1 X[#1+2]").is_empty(), "computed: not followed");
        assert!(words("O100 sub").is_empty());
    }

    #[test]
    fn modes_units_and_extrusion() {
        let moves = segments(
            "G21\nG90\nM82\nG92 E0\nG1 X10 Y0 E1 ;TYPE:Perimeter\nG1 X10 Y10 E1\nG91\nG1 X-10\nG20\nG90\nG0 X1\n",
        );
        assert_eq!(
            moves[0],
            ([0., 0., 0.], [10., 0., 0.], Move::Feed { extrudes: true })
        );
        // E did not grow: a travel at feed rate.
        assert_eq!(moves[1].2, Move::Feed { extrudes: false });
        assert!(!apart(moves[2].1, [0., 10., 0.]), "relative");
        assert_eq!(moves[3], ([0., 10., 0.], [25.4, 10., 0.], Move::Rapid));
    }

    #[test]
    fn arcs_by_centre_and_by_radius() {
        // A quarter, counter-clockwise, from (10, 0) around the origin.
        let moves = segments("G0 X10 Y0\nG3 X0 Y10 I-10 J0\n");
        let arc: Vec<_> = moves[1..].iter().map(|m| m.1).collect();
        assert!(
            arc.iter()
                .all(|p| ((p[0].powi(2) + p[1].powi(2)).sqrt() - 10.).abs() < 1e-6)
        );
        assert!(
            arc.iter().all(|p| p[0] >= -1e-9 && p[1] >= -1e-9),
            "the short way"
        );
        assert!(!apart(*arc.last().unwrap(), [0., 10., 0.]));
        // The same, clockwise the long way round, by its radius.
        let moves = segments("G0 X10 Y0\nG2 X0 Y10 R-10\n");
        assert!(
            moves[1..].iter().any(|m| m.1[0] < -5.),
            "three quarters, through the left"
        );
    }

    #[test]
    fn a_print_and_a_cut_are_drawn() {
        use std::fmt::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let print = dir.path().join("a part’s print.gcode");
        let mut program = String::from("G21\nG90\nM83\n");
        for layer in 1..=10 {
            let z = f64::from(layer) * 0.2;
            let _ = write!(
                program,
                ";TYPE:External perimeter\nG1 Z{z}\nG1 X0 Y0\nG1 X20 Y0 E1\nG1 X20 Y20 E1\nG1 X0 Y20 E1\nG1 X0 Y0 E1\n"
            );
        }
        std::fs::write(&print, program).unwrap();
        let drawn = draw(&print, 256, &CancelToken::new())
            .unwrap()
            .expect("drawn");
        assert_eq!(drawn.picture.width.max(drawn.picture.height), 256);
        assert_eq!(drawn.details[0].1, "20 × 20 × 1.8 mm");
        // Extrusions in the external perimeter's orange, on nothing.
        let pixels = drawn.picture.rgba.as_chunks::<4>().0;
        assert!(
            pixels
                .iter()
                .any(|p| p[..3] == [255, 125, 56] && p[3] == 255)
        );
        assert!(pixels.iter().any(|p| p[3] == 0), "transparent around it");
        let cut = dir.path().join("sign.nc");
        std::fs::write(
            &cut,
            "G21 G90\nG0 Z5\nG0 X0 Y0\nG1 Z-1 F100\nG1 X50\nG2 X50 Y20 R10\nG1 X0\nG0 Z5\n",
        )
        .unwrap();
        let drawn = draw(&cut, 256, &CancelToken::new())
            .unwrap()
            .expect("drawn");
        assert_eq!(
            drawn.picture.rgba[..4],
            [33, 40, 48, 255],
            "on its background"
        );
        // A circle of steps far shorter than a pixel, drawn small, is
        // still a circle: its left, right, top and bottom are drawn.
        let circle = dir.path().join("circle.nc");
        let mut program = String::from("G0 X50 Y0\nG1 Z-1\n");
        for step in 1..=2000 {
            let angle = f64::from(step) / 2000. * std::f64::consts::TAU;
            let _ = writeln!(
                program,
                "G1 X{:.4} Y{:.4}",
                50. * angle.cos(),
                50. * angle.sin()
            );
        }
        std::fs::write(&circle, program).unwrap();
        let drawn = draw(&circle, 64, &CancelToken::new())
            .unwrap()
            .expect("drawn");
        let (w, h) = (drawn.picture.width as usize, drawn.picture.height as usize);
        let lit = |x: usize, y: usize| {
            (y.saturating_sub(1)..=(y + 1).min(h - 1)).any(|y| {
                (x.saturating_sub(1)..=(x + 1).min(w - 1))
                    .any(|x| drawn.picture.rgba[(y * w + x) * 4..][..3] != [33, 40, 48])
            })
        };
        let margin = (64. * 0.04) as usize;
        for (x, y) in [
            (margin, h / 2),
            (w - 1 - margin, h / 2),
            (w / 2, margin),
            (w / 2, h - 1 - margin),
        ] {
            assert!(lit(x, y), "the ring at {x},{y}");
        }
        // Nothing moves: nothing to draw.
        let still = dir.path().join("still.gcode");
        std::fs::write(&still, "M104 S200\nM140 S60\n").unwrap();
        assert!(draw(&still, 256, &CancelToken::new()).unwrap().is_none());
    }
}
