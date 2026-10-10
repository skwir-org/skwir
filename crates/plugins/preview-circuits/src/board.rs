//! `KiCad` boards (`.kicad_pcb`) and footprints (`.kicad_mod`), drawn as
//! pcbnew shows them with its default colours: the copper of both sides
//! (tracks, pads, filled zones), silkscreens, the outline, vias and holes.
//! A board is read an item at a time; what is drawn is written as SVG,
//! grouped by layer so the front lies over the back.
use crate::Found;
use crate::sexpr::{Node, Reader};
use crate::svg::{self, Canvas, Placement, Slot};
use std::borrow::Cow;
use std::collections::HashSet;

const BACKGROUND: [u8; 3] = [0, 16, 35];
const FRONT_COPPER: [u8; 3] = [200, 52, 52];
const BACK_COPPER: [u8; 3] = [77, 127, 196];
const INNER_COPPER: [[u8; 3]; 6] = [
    [127, 200, 127],
    [206, 125, 44],
    [79, 203, 203],
    [219, 98, 139],
    [167, 165, 198],
    [40, 204, 217],
];
const FRONT_SILK: [u8; 3] = [242, 237, 161];
const BACK_SILK: [u8; 3] = [232, 178, 167];
const EDGE: [u8; 3] = [208, 210, 205];
const VIA: [u8; 3] = [236, 236, 236];

/// The groups, in the order they are drawn.
const HOLES: u32 = 900;
const VIAS: u32 = 800;
const EDGES: u32 = 700;

/// Where a drawn layer goes: its place in the drawing order, its colour.
fn slot(layer: &str) -> Option<Slot> {
    Some(match layer {
        "B.Cu" => (200, BACK_COPPER),
        "B.SilkS" => (300, BACK_SILK),
        "F.Cu" => (400, FRONT_COPPER),
        "F.SilkS" => (500, FRONT_SILK),
        "Edge.Cuts" => (EDGES, EDGE),
        inner => {
            let n: u32 = inner
                .strip_prefix("In")?
                .strip_suffix(".Cu")?
                .parse()
                .ok()?;
            let index = usize::try_from(n.checked_sub(1)?).ok()? % INNER_COPPER.len();
            // Deeper layers under shallower ones, all under the back.
            (100 - n.min(99), INNER_COPPER[index])
        }
    })
}

/// Copper lets the layers under it show.
fn translucent(order: u32) -> bool {
    order < 100 || order == 200 || order == 400
}

/// A position and a turn: where a footprint or a pad puts what it holds.
#[derive(Clone, Copy, Debug)]
struct Place {
    at: (f64, f64),
    /// Degrees, counter-clockwise on screen.
    angle: f64,
}

impl Place {
    const ORIGIN: Place = Place {
        at: (0., 0.),
        angle: 0.,
    };

    /// `(at x y angle)` in `node`.
    fn of(node: &Node<'_>) -> Place {
        let at = node.child("at");
        Place {
            at: at
                .and_then(|at| Some((at.number(1)?, at.number(2)?)))
                .unwrap_or_default(),
            angle: at.and_then(|at| at.number(3)).unwrap_or(0.),
        }
    }

    fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        if self.angle == 0. {
            return (self.at.0 + x, self.at.1 + y);
        }
        let (sin, cos) = self.angle.to_radians().sin_cos();
        (self.at.0 + x * cos + y * sin, self.at.1 - x * sin + y * cos)
    }

    /// `self` moved to a point given in its own frame, turned `angle` in
    /// all (a pad's angle counts its footprint's).
    fn to(&self, local: (f64, f64), angle: f64) -> Place {
        Place {
            at: self.apply(local),
            angle,
        }
    }
}

/// A line's width, as `KiCad` 6 onwards writes it (`(stroke (width w))`) or
/// as before (`(width w)`).
fn width(node: &Node<'_>) -> f64 {
    node.child("stroke")
        .and_then(|stroke| stroke.value("width"))
        .or_else(|| node.value("width"))
        .unwrap_or(0.)
}

/// Whether a shape is filled: `(fill yes)`, `(fill solid)`, or
/// `(fill (type solid))`.
fn filled(node: &Node<'_>) -> bool {
    node.child("fill").is_some_and(|fill| {
        matches!(fill.atom(1), Some("yes" | "solid"))
            || fill
                .child("type")
                .is_some_and(|kind| matches!(kind.atom(1), Some("solid" | "outline")))
    })
}

/// A drawn shape (`gr_line`, `fp_arc`, …) in `place`'s frame.
fn graphic(canvas: &mut Canvas, node: &Node<'_>, place: Place) {
    let Some(slot) = node.string("layer").as_deref().and_then(slot) else {
        return;
    };
    let kind = node.head().unwrap_or_default();
    let kind = kind.rsplit('_').next().unwrap_or_default();
    let width = width(node);
    let fill = filled(node);
    let p = |key| node.point(key).map(|point| place.apply(point));
    match kind {
        "line" => {
            if let (Some(a), Some(b)) = (p("start"), p("end")) {
                canvas.line(slot, a, b, width);
            }
        }
        "rect" => {
            if let (Some((x0, y0)), Some((x1, y1))) = (node.point("start"), node.point("end")) {
                let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|c| place.apply(c));
                canvas.polygon(slot, &corners, true, width, fill.then_some(slot.1));
            }
        }
        "circle" => {
            if let (Some(centre), Some(end)) = (node.point("center"), node.point("end")) {
                let radius = ((end.0 - centre.0).powi(2) + (end.1 - centre.1).powi(2)).sqrt();
                canvas.circle(
                    slot,
                    place.apply(centre),
                    radius,
                    width,
                    fill.then_some(slot.1),
                );
            }
        }
        "arc" => {
            if let Some(points) = arc_points(node) {
                canvas.arc(slot, points.map(|point| place.apply(point)), width, None);
            }
        }
        "poly" => {
            let points: Vec<_> = node.points().into_iter().map(|p| place.apply(p)).collect();
            canvas.polygon(slot, &points, true, width, fill.then_some(slot.1));
        }
        "curve" => {
            let points: Vec<_> = node.points().into_iter().map(|p| place.apply(p)).collect();
            if let [a, b, c, d] = points[..] {
                let mut data = String::from("M");
                svg::pair(&mut data, a);
                data.push('C');
                for point in [b, c, d] {
                    svg::pair(&mut data, point);
                    data.push(' ');
                    canvas.cover(slot.0, point, width / 2.);
                }
                canvas.cover(slot.0, a, width / 2.);
                canvas.path(slot, data.trim_end(), width.max(0.01), None);
            }
        }
        _ => {}
    }
}

/// An arc's start, a point on it and its end: written so since `KiCad` 6;
/// before, as its centre (`start`), its first point (`end`) and its angle,
/// clockwise.
fn arc_points(node: &Node<'_>) -> Option<[(f64, f64); 3]> {
    let start = node.point("start")?;
    let end = node.point("end")?;
    if let Some(mid) = node.point("mid") {
        return Some([start, mid, end]);
    }
    let angle = node.value("angle")?;
    let around = |degrees: f64| {
        Place {
            at: start,
            angle: -degrees,
        }
        .apply((end.0 - start.0, end.1 - start.1))
    };
    Some([end, around(angle / 2.), around(angle)])
}

/// How `node` (`gr_text`, `fp_text`, a `property`) sets its text; `None`
/// when it is hidden.
fn placement(node: &Node<'_>, place: Place) -> Option<Placement> {
    let effects = node.child("effects");
    if node.switch("hide") || effects.is_some_and(|effects| effects.switch("hide")) {
        return None;
    }
    let font = effects.and_then(|effects| effects.child("font"));
    let size = font
        .and_then(|font| font.child("size"))
        .and_then(|size| size.number(1))
        .unwrap_or(1.27);
    let at = node.child("at")?;
    let local = (at.number(1)?, at.number(2)?);
    let mut placed = Placement::centred(place.apply(local), size);
    placed.angle = at.number(3).unwrap_or(0.);
    placed.bold = font.is_some_and(|font| font.switch("bold"));
    placed.italic = font.is_some_and(|font| font.switch("italic"));
    if let Some(justify) = effects.and_then(|effects| effects.child("justify")) {
        placed.justify(justify.items().iter().skip(1).filter_map(Node::word));
    }
    Some(placed)
}

/// What a footprint's text says, its variables replaced.
fn expand<'a>(text: &'a str, reference: &str, value: &str) -> Cow<'a, str> {
    if !text.contains("${") {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.replace("${REFERENCE}", reference)
            .replace("${VALUE}", value),
    )
}

/// What the board says of itself, gathered as it is drawn.
#[derive(Default)]
struct Facts {
    version: Option<u64>,
    generator_version: Option<String>,
    thickness: Option<f64>,
    copper_layers: usize,
    footprints: usize,
    pads: usize,
    nets: HashSet<String>,
    tracks: usize,
    vias: usize,
    title: Vec<(String, String)>,
}

/// A footprint: its drawings, texts and pads, in its own frame.
fn footprint(canvas: &mut Canvas, facts: &mut Facts, node: &Node<'_>) {
    let place = Place::of(node);
    let field = |name: &str| -> String {
        node.children("property")
            .find(|p| p.atom(1) == Some(name))
            .and_then(|p| p.text(2))
            .or_else(|| {
                node.children("fp_text")
                    .find(|t| t.atom(1) == Some(&name.to_ascii_lowercase()))
                    .and_then(|t| t.text(2))
            })
            .map(Cow::into_owned)
            .unwrap_or_default()
    };
    let (reference, value) = (field("Reference"), field("Value"));
    for item in node.items() {
        match item.head() {
            Some("fp_line" | "fp_rect" | "fp_circle" | "fp_arc" | "fp_poly" | "fp_curve") => {
                graphic(canvas, item, place);
            }
            Some("fp_text") => {
                if let (Some(slot), Some(content), Some(placed)) = (
                    item.string("layer").as_deref().and_then(slot),
                    item.text(2),
                    placement(item, place),
                ) {
                    canvas.text(slot, &expand(&content, &reference, &value), &placed);
                }
            }
            Some("property") => {
                if let (Some(slot), Some(content), Some(placed)) = (
                    item.string("layer").as_deref().and_then(slot),
                    item.text(2),
                    placement(item, place),
                ) {
                    canvas.text(slot, &content, &placed);
                }
            }
            Some("pad") => {
                facts.pads += 1;
                pad(canvas, facts, item, place);
            }
            _ => {}
        }
    }
}

/// The copper layers a pad or via is on, by their names or `*.Cu`.
fn copper_sides(node: &Node<'_>) -> Vec<Slot> {
    let Some(layers) = node.child("layers") else {
        return Vec::new();
    };
    let names: Vec<&str> = layers
        .items()
        .iter()
        .skip(1)
        .filter_map(Node::word)
        .collect();
    if names.iter().any(|name| matches!(*name, "*.Cu" | "F&B.Cu")) {
        return vec![
            slot("B.Cu").unwrap_or_default(),
            slot("F.Cu").unwrap_or_default(),
        ];
    }
    names
        .into_iter()
        .filter(|name| name.ends_with(".Cu"))
        .filter_map(slot)
        .collect()
}

/// A pad: its copper, on each of its sides, and its hole.
fn pad(canvas: &mut Canvas, facts: &mut Facts, node: &Node<'_>, footprint: Place) {
    let kind = node.atom(2).unwrap_or_default();
    let shape = node.atom(3).unwrap_or_default();
    let at = node.child("at");
    let local = at
        .and_then(|at| Some((at.number(1)?, at.number(2)?)))
        .unwrap_or_default();
    let place = footprint.to(local, at.and_then(|at| at.number(3)).unwrap_or(0.));
    let size = node
        .child("size")
        .and_then(|size| Some((size.number(1)?, size.number(2)?)))
        .unwrap_or_default();
    if let Some(net) = node.child("net") {
        // `(net 3 "GND")` before KiCad 10, `(net "GND")` since.
        let name = net.text(2).or_else(|| net.text(1)).unwrap_or_default();
        if !name.is_empty() && name.parse::<u32>().is_err() {
            facts.nets.insert(name.into_owned());
        }
    }
    if kind != "np_thru_hole" {
        for side in copper_sides(node) {
            pad_shape(canvas, side, node, shape, size, place);
        }
    }
    if let Some(drill) = node.child("drill") {
        let oval = drill.atom(1) == Some("oval");
        let first = drill.number(if oval { 2 } else { 1 }).unwrap_or(0.);
        let second = if oval {
            drill.number(3).unwrap_or(first)
        } else {
            first
        };
        let offset = drill.point("offset").unwrap_or_default();
        hole(canvas, place.to(offset, place.angle), (first, second));
    }
}

/// A hole `size` wide and high, round or a slot.
fn hole(canvas: &mut Canvas, place: Place, (w, h): (f64, f64)) {
    if !w.is_finite() || w <= 0. {
        return;
    }
    let slot = (HOLES, BACKGROUND);
    if (w - h).abs() < 1e-6 {
        canvas.circle(slot, place.at, w / 2., 0., Some(BACKGROUND));
    } else {
        let half = (w - h).abs() / 2.;
        let (a, b) = if w > h {
            ((-half, 0.), (half, 0.))
        } else {
            ((0., -half), (0., half))
        };
        canvas.line(slot, place.apply(a), place.apply(b), w.min(h));
    }
}

/// A pad's copper on one side.
fn pad_shape(
    canvas: &mut Canvas,
    side: Slot,
    node: &Node<'_>,
    shape: &str,
    (w, h): (f64, f64),
    place: Place,
) {
    match shape {
        "circle" => canvas.circle(side, place.at, w / 2., 0., Some(side.1)),
        "oval" => {
            let half = (w - h).abs() / 2.;
            let (a, b) = if w > h {
                ((-half, 0.), (half, 0.))
            } else {
                ((0., -half), (0., half))
            };
            if half < 1e-6 {
                canvas.circle(side, place.at, w / 2., 0., Some(side.1));
            } else {
                canvas.line(side, place.apply(a), place.apply(b), w.min(h));
            }
        }
        "roundrect" | "chamfered_rect" => {
            let ratio = node.value("roundrect_rratio").unwrap_or(0.).clamp(0., 0.5);
            rounded(canvas, side, place, (w, h), ratio * w.min(h));
        }
        "custom" => {
            let anchor = node
                .child("options")
                .and_then(|options| options.string("anchor"));
            if anchor.as_deref() == Some("rect") {
                rounded(canvas, side, place, (w, h), 0.);
            } else {
                canvas.circle(side, place.at, w / 2., 0., Some(side.1));
            }
            if let Some(primitives) = node.child("primitives") {
                for primitive in primitives.items().iter().skip(1) {
                    custom(canvas, side, primitive, place);
                }
            }
        }
        // `rect`, and a trapezoid drawn as the rectangle it is close to.
        _ => rounded(canvas, side, place, (w, h), 0.),
    }
}

/// A rectangle `size` large, its corners rounded `radius`.
fn rounded(canvas: &mut Canvas, side: Slot, place: Place, (w, h): (f64, f64), radius: f64) {
    let (x, y) = (w / 2., h / 2.);
    if radius <= 1e-6 {
        let corners = [(-x, -y), (x, -y), (x, y), (-x, y)].map(|c| place.apply(c));
        canvas.polygon(side, &corners, true, 0., Some(side.1));
        return;
    }
    let r = radius;
    let mut data = String::new();
    let to = |command: char, point: (f64, f64), data: &mut String| {
        data.push(command);
        if command == 'A' {
            svg::num(data, r);
            data.push(',');
            svg::num(data, r);
            data.push_str(" 0 0,1 ");
        }
        svg::pair(data, place.apply(point));
    };
    to('M', (-x + r, -y), &mut data);
    to('L', (x - r, -y), &mut data);
    to('A', (x, -y + r), &mut data);
    to('L', (x, y - r), &mut data);
    to('A', (x - r, y), &mut data);
    to('L', (-x + r, y), &mut data);
    to('A', (-x, y - r), &mut data);
    to('L', (-x, -y + r), &mut data);
    to('A', (-x + r, -y), &mut data);
    data.push('Z');
    for corner in [(-x, -y), (x, y)] {
        canvas.cover(side.0, place.apply(corner), 0.);
    }
    canvas.path(side, &data, 0., Some(side.1));
}

/// One of a custom pad's shapes, in the pad's frame, always copper.
fn custom(canvas: &mut Canvas, side: Slot, node: &Node<'_>, place: Place) {
    let width = width(node);
    let fill = filled(node) || node.child("fill").is_none();
    match node.head() {
        Some("gr_poly") => {
            let points: Vec<_> = node.points().into_iter().map(|p| place.apply(p)).collect();
            canvas.polygon(side, &points, true, width, fill.then_some(side.1));
        }
        Some("gr_line") => {
            if let (Some(a), Some(b)) = (node.point("start"), node.point("end")) {
                canvas.line(side, place.apply(a), place.apply(b), width);
            }
        }
        Some("gr_circle") => {
            if let (Some(centre), Some(end)) = (node.point("center"), node.point("end")) {
                let radius = ((end.0 - centre.0).powi(2) + (end.1 - centre.1).powi(2)).sqrt();
                canvas.circle(
                    side,
                    place.apply(centre),
                    radius,
                    width,
                    filled(node).then_some(side.1),
                );
            }
        }
        Some("gr_arc") => {
            if let Some(points) = arc_points(node) {
                canvas.arc(side, points.map(|point| place.apply(point)), width, None);
            }
        }
        Some("gr_rect") => {
            if let (Some((x0, y0)), Some((x1, y1))) = (node.point("start"), node.point("end")) {
                let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|c| place.apply(c));
                canvas.polygon(side, &corners, true, width, fill.then_some(side.1));
            }
        }
        _ => {}
    }
}

/// One of the board's items.
fn item(canvas: &mut Canvas, facts: &mut Facts, node: &Node<'_>) {
    match node.head().unwrap_or_default() {
        "version" => facts.version = node.number(1).map(|v| v as u64),
        "generator_version" => facts.generator_version = node.text(1).map(Cow::into_owned),
        "general" => facts.thickness = node.value("thickness"),
        "layers" => {
            facts.copper_layers = node
                .items()
                .iter()
                .filter(|layer| layer.atom(1).is_some_and(|name| name.ends_with(".Cu")))
                .count();
        }
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
        "footprint" | "module" => {
            facts.footprints += 1;
            footprint(canvas, facts, node);
        }
        "gr_line" | "gr_rect" | "gr_circle" | "gr_arc" | "gr_poly" | "gr_curve" => {
            graphic(canvas, node, Place::ORIGIN);
        }
        "gr_text" => {
            if let (Some(slot), Some(content), Some(placed)) = (
                node.string("layer").as_deref().and_then(slot),
                node.text(1),
                placement(node, Place::ORIGIN),
            ) {
                canvas.text(slot, &content, &placed);
            }
        }
        "segment" | "arc" => {
            facts.tracks += 1;
            let Some(slot) = node.string("layer").as_deref().and_then(slot) else {
                return;
            };
            let width = node.value("width").unwrap_or(0.2);
            if node.head() == Some("arc") {
                if let (Some(a), Some(m), Some(b)) =
                    (node.point("start"), node.point("mid"), node.point("end"))
                {
                    canvas.arc(slot, [a, m, b], width, None);
                }
            } else if let (Some(a), Some(b)) = (node.point("start"), node.point("end")) {
                canvas.line(slot, a, b, width);
            }
        }
        "via" => {
            facts.vias += 1;
            let place = Place::of(node);
            let size = node.value("size").unwrap_or(0.);
            canvas.circle((VIAS, VIA), place.at, size / 2., 0., Some(VIA));
            hole(
                canvas,
                place,
                (
                    node.value("drill").unwrap_or(0.),
                    node.value("drill").unwrap_or(0.),
                ),
            );
        }
        "zone" => {
            let own = node.string("layer");
            for polygon in node.children("filled_polygon") {
                let layer = polygon.string("layer").or_else(|| own.clone());
                if let Some(slot) = layer.as_deref().and_then(slot) {
                    canvas.polygon(slot, &polygon.points(), true, 0., Some(slot.1));
                }
            }
        }
        _ => {}
    }
}

/// A release, from the version `KiCad` wrote, or the date of its format.
fn release(generator: Option<&str>, version: Option<u64>) -> Option<String> {
    if let Some(generator) = generator {
        return Some(format!("KiCad {generator}"));
    }
    let version = version?;
    Some(
        match version {
            20_221_018.. => "KiCad 7",
            20_211_014.. => "KiCad 6",
            _ => "KiCad 5 or earlier",
        }
        .to_owned(),
    )
}

/// A millimetre length as Details says it.
fn mm(value: f64) -> String {
    let rounded = (value * 100.).round() / 100.;
    format!("{rounded}")
}

/// A board: its picture and what it holds.
pub fn board(text: &str, cancel: &skwir_core::CancelToken) -> anyhow::Result<Found> {
    let (mut reader, head) = Reader::open(text)?;
    anyhow::ensure!(head == "kicad_pcb", "Not a KiCad board");
    let mut canvas = Canvas::new(Some(EDGES));
    let mut facts = Facts::default();
    let mut read = 0usize;
    while let Some(node) = reader.next_item()? {
        read += 1;
        if read.is_multiple_of(256) {
            skwir_platform::inspection::check(cancel)?;
        }
        item(&mut canvas, &mut facts, &node);
    }
    let outline = (!canvas.framed.empty).then_some(canvas.framed);
    let mut details = Vec::new();
    if let Some(made) = release(facts.generator_version.as_deref(), facts.version) {
        details.push(("Made with".to_owned(), made));
    }
    details.append(&mut facts.title);
    if let Some(outline) = outline {
        details.push((
            "Board size".into(),
            format!("{} × {} mm", mm(outline.width()), mm(outline.height())),
        ));
    }
    if facts.copper_layers > 0 {
        details.push(("Copper layers".into(), facts.copper_layers.to_string()));
    }
    if let Some(thickness) = facts.thickness {
        details.push(("Thickness".into(), format!("{} mm", mm(thickness))));
    }
    for (label, count) in [
        ("Footprints", facts.footprints),
        ("Pads", facts.pads),
        ("Nets", facts.nets.len()),
        ("Tracks", facts.tracks),
        ("Vias", facts.vias),
    ] {
        if count > 0 {
            details.push((label.into(), count.to_string()));
        }
    }
    Ok(Found {
        svg: canvas.finish(BACKGROUND, translucent),
        details,
        notice: None,
    })
}

/// A footprint of a library (`.kicad_mod`): drawn alone.
pub fn footprint_file(text: &str) -> anyhow::Result<Found> {
    let (head, items) = crate::sexpr::parse(text)?;
    anyhow::ensure!(
        matches!(head.as_str(), "footprint" | "module"),
        "Not a KiCad footprint"
    );
    let mut all = vec![Node::Atom("footprint")];
    all.extend(items);
    let node = Node::List(all.into_boxed_slice());
    let mut canvas = Canvas::new(Some(EDGES));
    let mut facts = Facts::default();
    // Its frame is the origin, whatever position it was saved at.
    let origin: Vec<Node<'_>> = node
        .items()
        .iter()
        .filter(|item| item.head() != Some("at"))
        .cloned()
        .collect();
    footprint(
        &mut canvas,
        &mut facts,
        &Node::List(origin.into_boxed_slice()),
    );
    let mut details = Vec::new();
    if let Some(name) = node.text(1) {
        details.push(("Footprint".to_owned(), name.into_owned()));
    }
    if let Some(description) = node.string("descr").filter(|d| !d.trim().is_empty()) {
        details.push((
            "Description".into(),
            description
                .chars()
                .take(skwir_platform::inspection::limits::DETAIL_CHARS)
                .collect(),
        ));
    }
    if let Some(attr) = node.child("attr").and_then(|attr| attr.atom(1)) {
        let kind = match attr {
            "smd" => "Surface mount",
            "through_hole" => "Through hole",
            other => other,
        };
        details.push(("Mounting".into(), kind.to_owned()));
    }
    details.push(("Pads".into(), facts.pads.to_string()));
    Ok(Found {
        svg: canvas.finish(BACKGROUND, translucent),
        details,
        notice: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::CancelToken;

    const BOARD: &str = r#"(kicad_pcb (version 20241229) (generator "pcbnew") (generator_version "9.0")
        (general (thickness 1.6))
        (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (5 "F.SilkS" user) (25 "Edge.Cuts" user))
        (title_block (title "A test & board") (rev "2"))
        (net 0 "") (net 1 "GND")
        (footprint "R_0402" (layer "F.Cu") (at 10 10 90)
            (property "Reference" "R1" (at 0 -1 90) (layer "F.SilkS") (effects (font (size 1 1))))
            (property "Value" "10k" (at 0 1 90) (layer "F.Fab") (hide yes))
            (fp_text user "${REFERENCE}" (at 0 0 90) (layer "F.SilkS"))
            (fp_line (start -1 -0.5) (end 1 -0.5) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))
            (pad "1" smd roundrect (at -0.5 0 90) (size 0.5 0.6) (layers "F.Cu" "F.Mask") (roundrect_rratio 0.25) (net 1 "GND"))
            (pad "2" thru_hole circle (at 0.5 0 90) (size 1 1) (drill 0.5) (layers "*.Cu" "*.Mask") (net 2 "VCC")))
        (gr_rect (start 0 0) (end 30 20) (stroke (width 0.1)) (layer "Edge.Cuts"))
        (gr_arc (start 5 5) (mid 6 4) (end 7 5) (stroke (width 0.1)) (layer "F.SilkS"))
        (segment (start 10 10) (end 20 10) (width 0.25) (layer "F.Cu") (net 1))
        (arc (start 20 10) (mid 21 11) (end 22 10) (width 0.25) (layer "B.Cu") (net 1))
        (via (at 20 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1))
        (zone (net 1) (layer "B.Cu") (filled_polygon (layer "B.Cu") (pts (xy 1 1) (xy 29 1) (xy 29 19)))))"#;

    #[test]
    fn a_board_drawn_and_counted() {
        let found = board(BOARD, &CancelToken::new()).unwrap();
        let svg = found.svg.expect("a picture");
        // Framed by its outline, with a margin.
        assert!(svg.contains("viewBox=\"-1.4 -1.4 32.8 22.8\""), "{svg}");
        assert!(svg.contains(">R1</text>"));
        assert!(!svg.contains("10k"), "a hidden field is not drawn");
        assert!(svg.contains(" rotate(-90)"));
        assert_eq!(
            svg.matches("<text").count(),
            2,
            "the field and ${{REFERENCE}}"
        );
        assert!(
            svg.find("#4d7fc4").unwrap() < svg.find("#c83434").unwrap(),
            "back first"
        );
        let said: Vec<String> = found
            .details
            .iter()
            .map(|(label, value)| format!("{label}: {value}"))
            .collect();
        assert_eq!(
            said,
            [
                "Made with: KiCad 9.0",
                "Title: A test & board",
                "Revision: 2",
                "Board size: 30 × 20 mm",
                "Copper layers: 2",
                "Thickness: 1.6 mm",
                "Footprints: 1",
                "Pads: 2",
                "Nets: 2",
                "Tracks: 2",
                "Vias: 1"
            ]
        );
        // And the rasterizer draws it.
        let drawn = skwir_platform::inspection::drawing(svg.as_bytes(), Some(300)).unwrap();
        assert_eq!(drawn.width.max(drawn.height), 300);
    }

    #[test]
    fn a_pad_turns_with_its_footprint() {
        let place = Place {
            at: (10., 10.),
            angle: 90.,
        };
        // Turned counter-clockwise on screen: right becomes up.
        let (x, y) = place.apply((1., 0.));
        assert!((x - 10.).abs() < 1e-9 && (y - 9.).abs() < 1e-9, "{x} {y}");
    }

    #[test]
    fn a_kicad_5_arc_by_its_centre_and_angle() {
        let (_, items) = crate::sexpr::parse(
            "(x (gr_arc (start 0 0) (end 1 0) (angle 90) (layer F.SilkS) (width 0.1)))",
        )
        .unwrap();
        let [start, mid, end] = arc_points(&items[0]).unwrap();
        assert_eq!(start, (1., 0.));
        // Clockwise on screen (y down) from the right: towards the bottom.
        assert!((end.0).abs() < 1e-9 && (end.1 - 1.).abs() < 1e-9, "{end:?}");
        assert!(mid.0 > 0.5 && mid.1 > 0.5);
    }

    #[test]
    fn a_footprint_file_is_drawn_at_its_origin() {
        let found = footprint_file(
            r#"(footprint "C_0805" (version 20240108) (layer "F.Cu") (descr "Capacitor SMD 0805") (attr smd) (at 50 50)
                (fp_line (start -1 -1) (end 1 -1) (stroke (width 0.12)) (layer "F.SilkS"))
                (pad "1" smd rect (at -1 0) (size 1 1.4) (layers "F.Cu" "F.Paste" "F.Mask"))
                (pad "2" smd rect (at 1 0) (size 1 1.4) (layers "F.Cu" "F.Paste" "F.Mask")))"#,
        )
        .unwrap();
        let svg = found.svg.unwrap();
        assert!(svg.contains("viewBox=\"-2.09 -1.65 4.18 2.94\""), "{svg}");
        assert_eq!(
            found.details,
            [
                ("Footprint".to_owned(), "C_0805".to_owned()),
                ("Description".to_owned(), "Capacitor SMD 0805".to_owned()),
                ("Mounting".to_owned(), "Surface mount".to_owned()),
                ("Pads".to_owned(), "2".to_owned())
            ]
        );
    }

    #[test]
    fn nothing_to_draw_is_no_picture() {
        let found = board("(kicad_pcb (version 20241229))", &CancelToken::new()).unwrap();
        assert!(found.svg.is_none());
        assert!(board("(kicad_sch (version 1))", &CancelToken::new()).is_err());
    }
}
