//! `KiCad` schematics (`.kicad_sch`, `KiCad` 6 onwards) and symbol libraries
//! (`.kicad_sym`), drawn as eeschema shows them with its default colours:
//! symbols with their pins and visible fields, wires, buses, junctions,
//! labels, notes and sheets. A schematic carries a copy of every symbol it
//! places (`lib_symbols`), so nothing outside the file is read. Library
//! symbols are drawn with y up; the sheet has it down.
use crate::Found;
use crate::sexpr::{Node, Reader};
use crate::svg::{Canvas, Placement, Slot};
use std::borrow::Cow;
use std::collections::HashMap;

const BACKGROUND: [u8; 3] = [245, 244, 239];
const BODY_FILL: [u8; 3] = [255, 255, 194];
/// The layers, in the order they are drawn.
const FILLS: Slot = (100, [132, 0, 0]);
const NOTES: Slot = (200, [0, 0, 194]);
const SHEETS: Slot = (300, [132, 0, 132]);
const BODY: Slot = (400, [132, 0, 0]);
const PINS: Slot = (500, [132, 0, 0]);
const BUSES: Slot = (600, [0, 0, 132]);
const WIRES: Slot = (700, [0, 150, 0]);
const NO_CONNECT: Slot = (800, [0, 0, 132]);
const PIN_NUMBERS: Slot = (900, [169, 0, 0]);
const PIN_NAMES: Slot = (910, [0, 100, 100]);
const FIELDS: Slot = (920, [0, 100, 100]);
const OTHER_FIELDS: Slot = (930, [132, 0, 132]);
const LABELS: Slot = (940, [15, 15, 15]);
const GLOBAL_LABELS: Slot = (950, [132, 0, 0]);
const SHEET_LABELS: Slot = (960, [114, 86, 0]);

/// `KiCad`'s widths when a file says 0: a line, a bus.
const LINE: f64 = 0.1524;
const BUS: f64 = 0.3048;
const JUNCTION: f64 = 0.9144;
const NO_CONNECT_HALF: f64 = 0.635;
/// A text's size when none is given.
const TEXT: f64 = 1.27;

/// Where a placed symbol puts its library's drawing: its position, its
/// turn and its mirrors, applied to library points (y up).
#[derive(Clone, Copy, Debug)]
struct Transform {
    at: (f64, f64),
    /// The linear part, on screen: `[[xx, xy], [yx, yy]]`.
    m: [[f64; 2]; 2],
}

impl Transform {
    const LIBRARY: Transform = Transform {
        at: (0., 0.),
        m: [[1., 0.], [0., 1.]],
    };

    /// A symbol `(at x y angle)` with `(mirror x|y)`.
    fn of(node: &Node<'_>) -> Transform {
        let at = node.child("at");
        let position = at
            .and_then(|at| Some((at.number(1)?, at.number(2)?)))
            .unwrap_or_default();
        let angle = at.and_then(|at| at.number(3)).unwrap_or(0.);
        let (sin, cos) = angle.to_radians().sin_cos();
        let (sin, cos) = (sin.round(), cos.round());
        // Counter-clockwise on screen, y down.
        let rotation = [[cos, sin], [-sin, cos]];
        let mirror = match node.child("mirror").and_then(|m| m.atom(1)) {
            Some("x") => [[1., 0.], [0., -1.]],
            Some("y") => [[-1., 0.], [0., 1.]],
            _ => [[1., 0.], [0., 1.]],
        };
        Transform {
            at: position,
            m: multiply(rotation, mirror),
        }
    }

    /// A library point (y up) on the sheet.
    fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let (x, y) = (x, -y);
        (
            self.at.0 + self.m[0][0] * x + self.m[0][1] * y,
            self.at.1 + self.m[1][0] * x + self.m[1][1] * y,
        )
    }

    /// A direction on screen, turned as the symbol is.
    fn turn(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            self.m[0][0] * x + self.m[0][1] * y,
            self.m[1][0] * x + self.m[1][1] * y,
        )
    }
}

fn multiply(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    [
        [
            a[0][0] * b[0][0] + a[0][1] * b[1][0],
            a[0][0] * b[0][1] + a[0][1] * b[1][1],
        ],
        [
            a[1][0] * b[0][0] + a[1][1] * b[1][0],
            a[1][0] * b[0][1] + a[1][1] * b[1][1],
        ],
    ]
}

/// A stroke's width, `KiCad`'s default at 0.
fn stroke(node: &Node<'_>) -> f64 {
    node.child("stroke")
        .and_then(|stroke| stroke.value("width"))
        .filter(|width| *width > 0.)
        .unwrap_or(LINE)
}

/// How a library shape is filled: with the body's colour, its outline's,
/// or not.
fn fill(node: &Node<'_>, outline: [u8; 3]) -> Option<[u8; 3]> {
    let fill = node.child("fill")?;
    match fill
        .child("type")
        .and_then(|kind| kind.atom(1))
        .or(fill.atom(1))
    {
        Some("background") => Some(BODY_FILL),
        Some("outline") => Some(outline),
        Some("color") => fill.child("color").and_then(|color| {
            let channel = |i| color.number(i).map(|c| c.clamp(0., 255.) as u8);
            let alpha = color.number(4).unwrap_or(1.);
            (alpha > 0.).then_some([channel(1)?, channel(2)?, channel(3)?])
        }),
        _ => None,
    }
}

/// A shape in a symbol or on the sheet: a polyline, rectangle, circle,
/// arc or curve, its points through `transform`.
fn shape(canvas: &mut Canvas, node: &Node<'_>, transform: Transform, slot: Slot) {
    let width = stroke(node);
    let fill = fill(node, slot.1);
    // A filled body is drawn under everything else.
    let under = if fill.is_some() { FILLS } else { slot };
    let p = |point: (f64, f64)| transform.apply(point);
    match node.head() {
        Some("polyline") => {
            let points: Vec<_> = node.points().into_iter().map(p).collect();
            if let Some(fill) = fill {
                canvas.polygon(under, &points, true, 0., Some(fill));
            }
            canvas.polygon(slot, &points, false, width, None);
        }
        Some("rectangle") => {
            if let (Some((x0, y0)), Some((x1, y1))) = (node.point("start"), node.point("end")) {
                let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(p);
                if let Some(fill) = fill {
                    canvas.polygon(under, &corners, true, 0., Some(fill));
                }
                canvas.polygon(slot, &corners, true, width, None);
            }
        }
        Some("circle") => {
            let radius = node.value("radius").unwrap_or(0.);
            if let Some(centre) = node.point("center") {
                if let Some(fill) = fill {
                    canvas.circle(under, p(centre), radius, 0., Some(fill));
                }
                canvas.circle(slot, p(centre), radius, width, None);
            }
        }
        Some("arc") => {
            if let (Some(a), Some(m), Some(b)) =
                (node.point("start"), node.point("mid"), node.point("end"))
            {
                if let Some(fill) = fill {
                    canvas.arc(under, [p(a), p(m), p(b)], 0., Some(fill));
                }
                canvas.arc(slot, [p(a), p(m), p(b)], width, None);
            }
        }
        Some("bezier") => {
            let points: Vec<_> = node.points().into_iter().map(p).collect();
            if let [a, b, c, d] = points[..] {
                let mut data = String::from("M");
                crate::svg::pair(&mut data, a);
                data.push('C');
                for point in [b, c, d] {
                    crate::svg::pair(&mut data, point);
                    data.push(' ');
                    canvas.cover(slot.0, point, 0.);
                }
                canvas.cover(slot.0, a, 0.);
                canvas.path(slot, data.trim_end(), width, None);
            }
        }
        _ => {}
    }
}

/// How `node`'s `effects` set its text at `at` (on the sheet), turned
/// `angle`; `None` when hidden.
fn effects(node: &Node<'_>, at: (f64, f64), angle: f64) -> Option<Placement> {
    let effects = node.child("effects");
    if node.switch("hide") || effects.is_some_and(|effects| effects.switch("hide")) {
        return None;
    }
    let font = effects.and_then(|effects| effects.child("font"));
    let size = font
        .and_then(|font| font.child("size"))
        .and_then(|size| size.number(1))
        .unwrap_or(TEXT);
    let mut placed = Placement::centred(at, size);
    placed.angle = angle;
    placed.bold = font.is_some_and(|font| font.switch("bold"));
    placed.italic = font.is_some_and(|font| font.switch("italic"));
    if let Some(justify) = effects.and_then(|effects| effects.child("justify")) {
        placed.justify(justify.items().iter().skip(1).filter_map(Node::word));
    }
    Some(placed)
}

/// Text as `KiCad` marks it up, plainly: `~{RESET}` is an overbar, `_{n}`
/// a subscript, `^{2}` a superscript.
fn plain(text: &str) -> Cow<'_, str> {
    if !text.contains('{') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if matches!(ch, '~' | '_' | '^') && chars.peek() == Some(&'{') {
            chars.next();
            continue;
        }
        if ch == '}' {
            continue;
        }
        out.push(ch);
    }
    Cow::Owned(out)
}

/// A library symbol's name without its library: `Device:R` is `R`.
fn short(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// The unit and body style a library sub-symbol is for: `R_1_1`.
fn unit_of(name: &str) -> Option<(u32, u32)> {
    let mut parts = name.rsplitn(3, '_');
    let style = parts.next()?.parse().ok()?;
    let unit = parts.next()?.parse().ok()?;
    Some((unit, style))
}

/// A library symbol, its parent's drawing when it extends one.
struct Symbols<'n, 'a> {
    by_name: HashMap<Cow<'a, str>, &'n Node<'a>>,
}

impl<'n, 'a> Symbols<'n, 'a> {
    fn new(nodes: impl Iterator<Item = &'n Node<'a>>) -> Symbols<'n, 'a> {
        Symbols {
            by_name: nodes
                .filter_map(|node| Some((node.text(1)?, node)))
                .collect(),
        }
    }

    /// The symbol that holds `name`'s drawing: itself, or what it extends.
    fn drawing(&self, name: &str) -> Option<&'n Node<'a>> {
        let mut symbol = *self.by_name.get(name)?;
        for _ in 0..8 {
            let Some(parent) = symbol.string("extends") else {
                return Some(symbol);
            };
            // A parent in the same library, named without it.
            let full = match name.split_once(':') {
                Some((library, _)) => format!("{library}:{parent}"),
                None => parent.into_owned(),
            };
            symbol = self
                .by_name
                .get(full.as_str())
                .or_else(|| self.by_name.get(short(&full)))
                .copied()?;
        }
        None
    }
}

/// Library symbol `symbol`'s unit `unit` in body style `style`, through
/// `transform`: its shapes, pins and texts.
fn draw_symbol(
    canvas: &mut Canvas,
    symbol: &Node<'_>,
    unit: u32,
    style: u32,
    transform: Transform,
) {
    let names = symbol.child("pin_names");
    let names_offset = names.and_then(|n| n.value("offset")).unwrap_or(0.508);
    let names_shown = !names.is_some_and(|n| n.switch("hide"));
    let numbers_shown = !symbol
        .child("pin_numbers")
        .is_some_and(|n| n.switch("hide"));
    for part in symbol.children("symbol") {
        let Some((part_unit, part_style)) = part.text(1).as_deref().and_then(unit_of) else {
            continue;
        };
        if (part_unit != 0 && part_unit != unit) || (part_style != 0 && part_style != style) {
            continue;
        }
        for item in part.items() {
            match item.head() {
                Some("polyline" | "rectangle" | "circle" | "arc" | "bezier") => {
                    shape(canvas, item, transform, BODY);
                }
                Some("text") => {
                    if let (Some(content), Some(at)) = (item.text(1), item.child("at")) {
                        let local = (at.number(1).unwrap_or(0.), at.number(2).unwrap_or(0.));
                        // Library text angles are in tenths of a degree.
                        let angle = at.number(3).unwrap_or(0.) / 10.;
                        if let Some(placed) = effects(item, transform.apply(local), angle) {
                            upright(canvas, BODY, &plain(&content), placed, transform);
                        }
                    }
                }
                Some("pin") => pin(
                    canvas,
                    item,
                    transform,
                    names_shown.then_some(names_offset),
                    numbers_shown,
                ),
                _ => {}
            }
        }
    }
}

/// Text set in a symbol's frame, turned with it and kept readable: never
/// upside down, its justification flipped instead.
fn upright(
    canvas: &mut Canvas,
    slot: Slot,
    content: &str,
    mut placed: Placement,
    transform: Transform,
) {
    let vertical = (placed.angle.rem_euclid(180.) - 90.).abs() < 1.;
    let direction = transform.turn(if vertical { (0., -1.) } else { (1., 0.) });
    let (angle, flipped) = match direction {
        (x, _) if x > 0.5 => (0., false),
        (x, _) if x < -0.5 => (0., true),
        (_, y) if y < -0.5 => (90., false),
        _ => (90., true),
    };
    placed.angle = angle;
    if flipped {
        placed.horizontal = -placed.horizontal;
    }
    canvas.text(slot, content, &placed);
}

/// A pin: its line from where wires connect towards the body, its number
/// over the line, its name inside the body (`names_offset`) or over it.
fn pin(
    canvas: &mut Canvas,
    node: &Node<'_>,
    transform: Transform,
    names_offset: Option<f64>,
    numbers_shown: bool,
) {
    if node.switch("hide") {
        return;
    }
    let Some(at) = node.child("at") else {
        return;
    };
    let start = (at.number(1).unwrap_or(0.), at.number(2).unwrap_or(0.));
    let angle = at.number(3).unwrap_or(0.);
    let length = node.value("length").unwrap_or(2.54);
    let (sin, cos) = angle.to_radians().sin_cos();
    let end = (start.0 + length * cos, start.1 + length * sin);
    let (a, b) = (transform.apply(start), transform.apply(end));
    canvas.line(PINS, a, b, LINE);
    if node.atom(2) == Some("inverted") || node.atom(2) == Some("inverted_clock") {
        let radius = 0.635;
        let back = transform.apply((end.0 - radius * cos, end.1 - radius * sin));
        canvas.circle(PINS, back, radius, LINE, None);
    }
    // Which way the pin points on the sheet, from its end into the body
    // (by its angle: a pin may have no length).
    let direction = transform.turn((cos, -sin));
    let horizontal = direction.0.abs() > 0.5;
    let middle = (f64::midpoint(a.0, b.0), f64::midpoint(a.1, b.1));
    let name = node
        .child("name")
        .and_then(|name| Some((name.text(1)?, name)))
        .filter(|(text, _)| !text.is_empty() && text != "~");
    let number = node
        .child("number")
        .and_then(|number| Some((number.text(1)?, number)))
        .filter(|(text, _)| !text.is_empty());
    // Over the line: above a horizontal pin, left of a vertical one.
    let over = |placed: &mut Placement| {
        placed.angle = if horizontal { 0. } else { 90. };
        placed.vertical = 1;
        let gap = 0.3;
        placed.at = if horizontal {
            (placed.at.0, placed.at.1 - gap)
        } else {
            (placed.at.0 - gap, placed.at.1)
        };
    };
    if numbers_shown
        && let Some((text, node)) = &number
        && let Some(mut placed) = effects(node, middle, 0.)
    {
        over(&mut placed);
        canvas.text(PIN_NUMBERS, text, &placed);
    }
    let Some(offset) = names_offset else {
        return;
    };
    let Some((text, node)) = &name else {
        return;
    };
    let text = plain(text);
    if offset > 0. {
        let at = (b.0 + direction.0 * offset, b.1 + direction.1 * offset);
        if let Some(mut placed) = effects(node, at, 0.) {
            placed.angle = if horizontal { 0. } else { 90. };
            // The text runs on, into the body.
            let forward = if horizontal {
                direction.0 > 0.
            } else {
                direction.1 < 0.
            };
            placed.horizontal = if forward { -1 } else { 1 };
            placed.vertical = 0;
            canvas.text(PIN_NAMES, &text, &placed);
        }
    } else if let Some(mut placed) = effects(node, middle, 0.) {
        over(&mut placed);
        canvas.text(PIN_NAMES, &text, &placed);
    }
}

/// What the schematic says of itself, gathered as it is drawn.
#[derive(Default)]
struct Facts {
    version: Option<u64>,
    generator_version: Option<String>,
    paper: Option<String>,
    symbols: usize,
    sheets: usize,
    title: Vec<(String, String)>,
}

/// A placed symbol: its library drawing and its visible fields.
fn placed_symbol(
    canvas: &mut Canvas,
    facts: &mut Facts,
    node: &Node<'_>,
    library: &Symbols<'_, '_>,
) {
    let name = node
        .string("lib_name")
        .or_else(|| node.string("lib_id"))
        .unwrap_or_default();
    let transform = Transform::of(node);
    let unit = node.value("unit").unwrap_or(1.) as u32;
    let style = node
        .value("body_style")
        .or_else(|| node.value("convert"))
        .unwrap_or(1.) as u32;
    let symbol = library.by_name.get(name.as_ref()).copied();
    let power =
        symbol.is_some_and(|symbol| symbol.child("power").is_some() || symbol.flag("power"));
    let reference = node
        .children("property")
        .find(|p| p.atom(1) == Some("Reference"))
        .and_then(|p| p.text(2))
        .unwrap_or_default();
    if !power && !reference.starts_with('#') {
        facts.symbols += 1;
    }
    let drawing = library.drawing(&name);
    if let Some(drawing) = drawing {
        draw_symbol(canvas, drawing, unit, style, transform);
    }
    // A symbol of several units says which it is: `U1A`, `U1B`.
    let units = drawing.map_or(0, |drawing| {
        drawing
            .children("symbol")
            .filter_map(|part| part.text(1).as_deref().and_then(unit_of))
            .map(|(unit, _)| unit)
            .max()
            .unwrap_or(0)
    });
    for property in node.children("property") {
        let (Some(key), Some(value)) = (property.atom(1), property.text(2)) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let Some(at) = property.child("at") else {
            continue;
        };
        let position = (at.number(1).unwrap_or(0.), at.number(2).unwrap_or(0.));
        let Some(placed) = effects(property, position, at.number(3).unwrap_or(0.)) else {
            continue;
        };
        let slot = match key {
            "Reference" | "Value" => FIELDS,
            _ => OTHER_FIELDS,
        };
        let mut value = plain(&value).into_owned();
        if key == "Reference" && units > 1 {
            value.push_str(&unit_letters(unit));
        }
        // Its angle and justification are the symbol's, turned with it.
        upright(canvas, slot, &value, placed, transform);
    }
}

/// A unit's letters, as `KiCad` names them: A to Z, then AA, AB…
fn unit_letters(unit: u32) -> String {
    let mut n = unit.max(1);
    let mut letters = Vec::new();
    while n > 0 {
        let digit = (n - 1) % 26;
        letters.push(char::from(b'A' + digit as u8));
        n = (n - 1) / 26;
    }
    letters.iter().rev().collect()
}

/// A label's outline around text running `direction` from `at`: a flag
/// for a global label, pointed where signals go.
fn flag(
    canvas: &mut Canvas,
    slot: Slot,
    at: (f64, f64),
    direction: (f64, f64),
    text: &str,
    size: f64,
    shape: &str,
) {
    let length = text.chars().count() as f64 * size * 0.85 + size * 1.5;
    let half = size * 0.9;
    let normal = (-direction.1, direction.0);
    let point = |along: f64, across: f64| {
        (
            at.0 + direction.0 * along + normal.0 * across,
            at.1 + direction.1 * along + normal.1 * across,
        )
    };
    let tip_in = matches!(shape, "input" | "bidirectional" | "tri_state");
    let tip_out = matches!(shape, "output" | "bidirectional" | "tri_state");
    let mut outline = Vec::new();
    outline.push(if tip_in {
        point(0., 0.)
    } else {
        point(0., half)
    });
    if tip_in {
        outline.push(point(half, half));
    }
    if tip_out {
        outline.push(point(length - half, half));
        outline.push(point(length, 0.));
        outline.push(point(length - half, -half));
    } else {
        outline.push(point(length, half));
        outline.push(point(length, -half));
    }
    if tip_in {
        outline.push(point(half, -half));
    } else {
        outline.push(point(0., -half));
    }
    canvas.polygon(slot, &outline, true, LINE, None);
}

/// Which way text runs from a label's anchor, and at what angle it is set.
fn run(angle: f64) -> ((f64, f64), f64) {
    match angle.rem_euclid(360.).round() as u32 {
        90 => ((0., -1.), 90.),
        180 => ((-1., 0.), 0.),
        270 => ((0., 1.), 90.),
        _ => ((1., 0.), 0.),
    }
}

/// One of the sheet's items.
fn item(canvas: &mut Canvas, facts: &mut Facts, node: &Node<'_>, library: &Symbols<'_, '_>) {
    let head = node.head().unwrap_or_default();
    match head {
        "version" => facts.version = node.number(1).map(|v| v as u64),
        "generator_version" => facts.generator_version = node.text(1).map(Cow::into_owned),
        "paper" => facts.paper = node.text(1).map(Cow::into_owned),
        "title_block" => {
            for (key, label) in [
                ("title", "Title"),
                ("rev", "Revision"),
                ("company", "Company"),
                ("date", "Date"),
            ] {
                if let Some(value) = node.string(key).filter(|value| !value.trim().is_empty()) {
                    facts.title.push((label.into(), value.trim().to_owned()));
                }
            }
        }
        "symbol" => placed_symbol(canvas, facts, node, library),
        "wire" | "bus" => {
            let (slot, default) = if head == "bus" {
                (BUSES, BUS)
            } else {
                (WIRES, LINE)
            };
            let width = node
                .child("stroke")
                .and_then(|s| s.value("width"))
                .filter(|w| *w > 0.)
                .unwrap_or(default);
            canvas.polygon(slot, &node.points(), false, width, None);
        }
        "bus_entry" => {
            if let (Some(at), Some(size)) = (node.point("at"), node.point("size")) {
                canvas.line(WIRES, at, (at.0 + size.0, at.1 + size.1), LINE);
            }
        }
        "junction" => {
            if let Some(at) = node.point("at") {
                let diameter = node
                    .value("diameter")
                    .filter(|d| *d > 0.)
                    .unwrap_or(JUNCTION);
                canvas.circle(WIRES, at, diameter / 2., 0., Some(WIRES.1));
            }
        }
        "no_connect" => {
            if let Some((x, y)) = node.point("at") {
                let h = NO_CONNECT_HALF;
                canvas.line(NO_CONNECT, (x - h, y - h), (x + h, y + h), LINE);
                canvas.line(NO_CONNECT, (x - h, y + h), (x + h, y - h), LINE);
            }
        }
        "polyline" | "rectangle" | "circle" | "arc" | "bezier" => {
            shape(canvas, node, Transform::LIBRARY_ON_SHEET, NOTES);
        }
        "text" | "text_box" => {
            let Some(content) = node.text(1) else {
                return;
            };
            if let Some(at) = node.child("at") {
                let position = (at.number(1).unwrap_or(0.), at.number(2).unwrap_or(0.));
                let angle = at.number(3).unwrap_or(0.);
                if head == "text_box" {
                    if let Some(size) = node.point("size") {
                        let (x, y) = position;
                        let corners = [
                            (x, y),
                            (x + size.0, y),
                            (x + size.0, y + size.1),
                            (x, y + size.1),
                        ];
                        canvas.polygon(NOTES, &corners, true, stroke(node), None);
                    }
                    if let Some(mut placed) =
                        effects(node, (position.0 + 0.5, position.1 + 0.5), angle)
                    {
                        placed.horizontal = -1;
                        placed.vertical = -1;
                        canvas.text(NOTES, &plain(&content), &placed);
                    }
                } else if let Some(placed) = effects(node, position, angle) {
                    canvas.text(NOTES, &plain(&content), &placed);
                }
            }
        }
        "label" | "global_label" | "hierarchical_label" | "directive_label" | "netclass_flag" => {
            let Some(at) = node.child("at") else {
                return;
            };
            let position = (at.number(1).unwrap_or(0.), at.number(2).unwrap_or(0.));
            let (direction, angle) = run(at.number(3).unwrap_or(0.));
            let text = node
                .text(1)
                .map(|t| plain(&t).into_owned())
                .unwrap_or_default();
            let Some(mut placed) = effects(node, position, angle) else {
                return;
            };
            match head {
                "label" => {
                    // Over the wire, running from its anchor.
                    placed.angle = angle;
                    placed.vertical = 1;
                    placed.horizontal = if direction.0 < -0.5 || direction.1 > 0.5 {
                        1
                    } else {
                        -1
                    };
                    canvas.text(LABELS, &text, &placed);
                }
                "global_label" => {
                    let shape = node.string("shape").unwrap_or_default();
                    flag(
                        canvas,
                        GLOBAL_LABELS,
                        position,
                        direction,
                        &text,
                        placed.size,
                        &shape,
                    );
                    let inset = placed.size * 1.2;
                    placed.at = (
                        position.0 + direction.0 * inset,
                        position.1 + direction.1 * inset,
                    );
                    placed.angle = angle;
                    placed.vertical = 0;
                    placed.horizontal = if direction.0 < -0.5 || direction.1 > 0.5 {
                        1
                    } else {
                        -1
                    };
                    canvas.text(GLOBAL_LABELS, &text, &placed);
                }
                "hierarchical_label" => {
                    let size = placed.size;
                    let tip = (
                        position.0 + direction.0 * size,
                        position.1 + direction.1 * size,
                    );
                    let normal = (-direction.1 * size / 2., direction.0 * size / 2.);
                    canvas.polygon(
                        SHEET_LABELS,
                        &[
                            position,
                            (tip.0 + normal.0, tip.1 + normal.1),
                            (tip.0 - normal.0, tip.1 - normal.1),
                        ],
                        true,
                        LINE,
                        None,
                    );
                    let inset = size * 1.5;
                    placed.at = (
                        position.0 + direction.0 * inset,
                        position.1 + direction.1 * inset,
                    );
                    placed.angle = angle;
                    placed.vertical = 0;
                    placed.horizontal = if direction.0 < -0.5 || direction.1 > 0.5 {
                        1
                    } else {
                        -1
                    };
                    canvas.text(SHEET_LABELS, &text, &placed);
                }
                _ => {
                    // A directive: a pole and its fields.
                    let length = placed.size * 2.;
                    let end = (
                        position.0 - direction.1 * length,
                        position.1 + direction.0 * -length,
                    );
                    canvas.line(LABELS, position, end, LINE);
                    canvas.circle(LABELS, end, placed.size / 3., LINE, None);
                }
            }
        }
        "sheet" => {
            facts.sheets += 1;
            let (Some(at), Some(size)) = (node.point("at"), node.point("size")) else {
                return;
            };
            let corners = [
                (at.0, at.1),
                (at.0 + size.0, at.1),
                (at.0 + size.0, at.1 + size.1),
                (at.0, at.1 + size.1),
            ];
            let background = node.child("fill").and_then(|fill| {
                let color = fill.child("color")?;
                let channel = |i| color.number(i).map(|c| c.clamp(0., 255.) as u8);
                (color.number(4).unwrap_or(0.) > 0.).then_some([
                    channel(1)?,
                    channel(2)?,
                    channel(3)?,
                ])
            });
            if let Some(background) = background {
                canvas.polygon(FILLS, &corners, true, 0., Some(background));
            }
            canvas.polygon(SHEETS, &corners, true, stroke(node), None);
            for property in node.children("property") {
                let (Some(key), Some(value), Some(at)) =
                    (property.atom(1), property.text(2), property.child("at"))
                else {
                    continue;
                };
                let position = (at.number(1).unwrap_or(0.), at.number(2).unwrap_or(0.));
                let Some(placed) = effects(property, position, at.number(3).unwrap_or(0.)) else {
                    continue;
                };
                let (slot, text) = match key {
                    "Sheetname" | "Sheet name" => (FIELDS, value.into_owned()),
                    "Sheetfile" | "Sheet file" => (SHEET_LABELS, format!("File: {value}")),
                    _ => (OTHER_FIELDS, value.into_owned()),
                };
                canvas.text(slot, &text, &placed);
            }
            for pin in node.children("pin") {
                let (Some(name), Some(at)) = (pin.text(1), pin.child("at")) else {
                    continue;
                };
                let position = (at.number(1).unwrap_or(0.), at.number(2).unwrap_or(0.));
                let angle = at.number(3).unwrap_or(0.);
                if let Some(mut placed) = effects(pin, position, 0.) {
                    // Inside the sheet, away from the edge it sits on.
                    let (direction, text_angle) = run(angle + 180.);
                    let inset = placed.size;
                    placed.at = (
                        position.0 + direction.0 * inset,
                        position.1 + direction.1 * inset,
                    );
                    placed.angle = text_angle;
                    placed.vertical = 0;
                    placed.horizontal = if direction.0 < -0.5 || direction.1 > 0.5 {
                        1
                    } else {
                        -1
                    };
                    canvas.text(SHEET_LABELS, &plain(&name), &placed);
                }
            }
        }
        _ => {}
    }
}

impl Transform {
    /// Sheet shapes are already on the sheet, y down: undo `apply`'s flip.
    const LIBRARY_ON_SHEET: Transform = Transform {
        at: (0., 0.),
        m: [[1., 0.], [0., -1.]],
    };
}

/// A release, from the version `KiCad` wrote, or the date of its format.
fn release(generator: Option<&str>, version: Option<u64>) -> Option<String> {
    if let Some(generator) = generator {
        return Some(format!("KiCad {generator}"));
    }
    Some(
        match version? {
            20_230_121.. => "KiCad 7",
            20_211_123.. => "KiCad 6",
            _ => "KiCad 6 (development)",
        }
        .to_owned(),
    )
}

/// A schematic sheet: its picture and what it holds.
pub fn schematic(text: &str, cancel: &skwir_core::CancelToken) -> anyhow::Result<Found> {
    let (mut reader, head) = Reader::open(text)?;
    anyhow::ensure!(head == "kicad_sch", "Not a KiCad schematic");
    let mut canvas = Canvas::new(None);
    let mut facts = Facts::default();
    // The library comes first, before anything placed: what precedes it
    // waits for it.
    let mut pending = Vec::new();
    let mut library = None;
    while let Some(node) = reader.next_item()? {
        if node.head() == Some("lib_symbols") {
            library = Some(node);
            break;
        }
        pending.push(node);
    }
    let empty = Node::List(Box::new([]));
    let symbols = Symbols::new(library.as_ref().unwrap_or(&empty).children("symbol"));
    for node in pending {
        item(&mut canvas, &mut facts, &node, &symbols);
    }
    let mut read = 0usize;
    while let Some(node) = reader.next_item()? {
        read += 1;
        if read.is_multiple_of(256) {
            skwir_platform::inspection::check(cancel)?;
        }
        item(&mut canvas, &mut facts, &node, &symbols);
    }
    let mut details = Vec::new();
    if let Some(made) = release(facts.generator_version.as_deref(), facts.version) {
        details.push(("Made with".to_owned(), made));
    }
    details.append(&mut facts.title);
    if let Some(paper) = facts.paper {
        details.push(("Paper".into(), paper));
    }
    for (label, count) in [("Symbols", facts.symbols), ("Sheets", facts.sheets)] {
        if count > 0 {
            details.push((label.into(), count.to_string()));
        }
    }
    Ok(Found {
        svg: canvas.finish(BACKGROUND, |_| false),
        details,
        notice: None,
    })
}

/// A symbol library (`.kicad_sym`): its first symbol drawn, and how many
/// it holds.
pub fn library(text: &str) -> anyhow::Result<Found> {
    let (head, items) = crate::sexpr::parse(text)?;
    anyhow::ensure!(head == "kicad_symbol_lib", "Not a KiCad symbol library");
    let symbols = Symbols::new(items.iter().filter(|item| item.head() == Some("symbol")));
    let first = items.iter().find(|item| item.head() == Some("symbol"));
    let mut canvas = Canvas::new(None);
    let mut details = Vec::new();
    if let Some(first) = first
        && let Some(name) = first.text(1)
    {
        if let Some(drawing) = symbols.drawing(&name) {
            draw_symbol(&mut canvas, drawing, 1, 1, Transform::LIBRARY);
        }
        for property in first.children("property") {
            if !matches!(property.atom(1), Some("Reference" | "Value")) {
                continue;
            }
            let (Some(value), Some(at)) = (property.text(2), property.child("at")) else {
                continue;
            };
            let position =
                Transform::LIBRARY.apply((at.number(1).unwrap_or(0.), at.number(2).unwrap_or(0.)));
            if let Some(placed) = effects(property, position, at.number(3).unwrap_or(0.)) {
                canvas.text(FIELDS, &plain(&value), &placed);
            }
        }
        details.push(("First symbol".to_owned(), name.into_owned()));
    }
    details.push((
        "Symbols".into(),
        items
            .iter()
            .filter(|item| item.head() == Some("symbol"))
            .count()
            .to_string(),
    ));
    Ok(Found {
        svg: canvas.finish(BACKGROUND, |_| false),
        details,
        notice: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::CancelToken;

    const SCHEMATIC: &str = r##"(kicad_sch (version 20250114) (generator "eeschema") (generator_version "9.0")
        (paper "A4")
        (title_block (title "Amp") (rev "1"))
        (lib_symbols
            (symbol "Device:R" (pin_numbers (hide yes)) (pin_names (offset 0))
                (property "Reference" "R" (at 2 0 90) (effects (font (size 1.27 1.27))))
                (symbol "R_0_1" (rectangle (start -1 -2.5) (end 1 2.5) (stroke (width 0.254)) (fill (type none))))
                (symbol "R_1_1"
                    (pin passive line (at 0 3.81 270) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "1" (effects (font (size 1.27 1.27)))))
                    (pin passive line (at 0 -3.81 90) (length 1.27) (name "~" (effects (font (size 1.27 1.27)))) (number "2" (effects (font (size 1.27 1.27)))))))
            (symbol "power:GND" (power) (property "Reference" "#PWR" (at 0 0 0))
                (symbol "GND_0_1" (polyline (pts (xy 0 0) (xy 0 -1)) (stroke (width 0)) (fill (type none))))))
        (symbol (lib_id "Device:R") (at 100 50 90) (unit 1)
            (property "Reference" "R1" (at 100 46 0) (effects (font (size 1.27 1.27))))
            (property "Value" "10k" (at 100 54 0) (effects (font (size 1.27 1.27))))
            (property "Footprint" "R_0402" (at 100 50 0) (effects (font (size 1.27 1.27)) (hide yes))))
        (symbol (lib_id "power:GND") (at 96.19 50 0) (unit 1)
            (property "Reference" "#PWR01" (at 0 0 0) (effects (hide yes)))
            (property "Value" "GND" (at 96.19 53 0) (effects (font (size 1.27 1.27)))))
        (wire (pts (xy 80 50) (xy 96.19 50)) (stroke (width 0)))
        (junction (at 90 50) (diameter 0))
        (global_label "OUT" (shape output) (at 110 50 0) (effects (font (size 1.27 1.27)) (justify left)))
        (sheet (at 120 40) (size 20 10) (property "Sheetname" "Power" (at 120 39 0) (effects (font (size 1.27 1.27)) (justify left bottom))) (property "Sheetfile" "power.kicad_sch" (at 120 51 0) (effects (font (size 1.27 1.27)) (justify left top)))))"##;

    #[test]
    fn a_schematic_drawn_and_counted() {
        let found = schematic(SCHEMATIC, &CancelToken::new()).unwrap();
        let svg = found.svg.expect("a picture");
        for text in [
            ">R1<",
            ">10k<",
            ">GND<",
            ">OUT<",
            ">Power<",
            ">File: power.kicad_sch<",
        ] {
            assert!(svg.contains(text), "{text} in {svg}");
        }
        assert!(!svg.contains("R_0402"), "a hidden field");
        assert!(!svg.contains(">1<"), "hidden pin numbers");
        let said: Vec<String> = found
            .details
            .iter()
            .map(|(label, value)| format!("{label}: {value}"))
            .collect();
        assert_eq!(
            said,
            [
                "Made with: KiCad 9.0",
                "Title: Amp",
                "Revision: 1",
                "Paper: A4",
                "Symbols: 1",
                "Sheets: 1"
            ]
        );
        let drawn = skwir_platform::inspection::drawing(svg.as_bytes(), Some(400)).unwrap();
        assert_eq!(drawn.width.max(drawn.height), 400);
    }

    #[test]
    fn a_symbol_turned_puts_its_pins_on_the_wires() {
        // `R` turned 90°, counter-clockwise: its pin 1, at the top in the
        // library (y up), goes to the left on the sheet.
        let (_, items) = crate::sexpr::parse("(x (symbol (at 100 50 90)))").unwrap();
        let transform = Transform::of(&items[0]);
        let (x, y) = transform.apply((0., 3.81));
        assert!(
            (x - 96.19).abs() < 1e-9 && (y - 50.).abs() < 1e-9,
            "{x} {y}"
        );
        // Mirrored across x: the top goes to the bottom.
        let (_, items) = crate::sexpr::parse("(x (symbol (at 0 0 0) (mirror x)))").unwrap();
        let (_, y) = Transform::of(&items[0]).apply((0., 3.81));
        assert!((y - 3.81).abs() < 1e-9, "{y}");
    }

    #[test]
    fn units_are_lettered() {
        assert_eq!(unit_letters(1), "A");
        assert_eq!(unit_letters(3), "C");
        assert_eq!(unit_letters(27), "AA");
    }

    #[test]
    fn markup_is_read_plainly() {
        assert_eq!(plain("~{RESET}"), "RESET");
        assert_eq!(plain("V_{CC}^{2}"), "VCC2");
        assert_eq!(plain("a~b"), "a~b");
    }

    #[test]
    fn a_library_shows_its_first_symbol() {
        let found = library(
            r#"(kicad_symbol_lib (version 20241209)
                (symbol "Base" (property "Reference" "U" (at 0 5 0) (effects (font (size 1.27 1.27))))
                    (symbol "Base_0_1" (rectangle (start -5 -5) (end 5 5) (stroke (width 0.254)) (fill (type background)))))
                (symbol "Derived" (extends "Base") (property "Reference" "U" (at 0 5 0))))"#,
        )
        .unwrap();
        let svg = found.svg.unwrap();
        assert!(svg.contains("#ffffc2"), "the body filled");
        assert_eq!(
            found.details,
            [
                ("First symbol".to_owned(), "Base".to_owned()),
                ("Symbols".to_owned(), "2".to_owned())
            ]
        );
        let (_, items) = crate::sexpr::parse(
            r#"(l (symbol "Base" (symbol "Base_1_1")) (symbol "Derived" (extends "Base")))"#,
        )
        .unwrap();
        let symbols = Symbols::new(items.iter());
        assert_eq!(symbols.drawing("Derived").unwrap().atom(1), Some("Base"));
    }
}
