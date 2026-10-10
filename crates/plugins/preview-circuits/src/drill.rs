//! Excellon drill files: where a board is drilled, and with what. A
//! header (`M48`) names the tools (`T1C0.8`) and the units (`METRIC`,
//! `INCH`, with which zeros a number leaves out); the body selects a tool
//! (`T1`) and gives the holes (`X12.7Y-3.5`), slots (`G85`) and routed
//! paths (`G00`, `M15`, `G01`, `M16`). Holes are drawn at their size, on
//! the board's dark background.
use crate::svg::{self, Bounds};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const BACKGROUND: [u8; 3] = [0, 16, 35];
/// `KiCad`'s colour for plated holes.
const HOLE: [u8; 3] = [227, 183, 46];
/// Holes drawn at most.
const HOLES: usize = 1_000_000;

type Point = (f64, f64);

/// Whether text is a drill file: its header, or its first tool.
pub fn looks_like(head: &str) -> bool {
    head.lines().take(64).any(|line| {
        let line = line.trim();
        line == "M48" || line.starts_with("METRIC") || line.starts_with("INCH")
    })
}

/// How numbers without a decimal point are read.
#[derive(Clone, Copy)]
struct Numbers {
    /// Millimetres per unit.
    unit: f64,
    integers: usize,
    decimals: usize,
    /// Trailing zeros left out (the header says `LZ`: leading kept).
    trailing_omitted: bool,
}

impl Numbers {
    fn value(&self, text: &str) -> Option<f64> {
        if text.contains('.') {
            return Some(text.parse::<f64>().ok()? * self.unit);
        }
        let negative = text.starts_with('-');
        let mut digits = text.trim_start_matches(['-', '+']).to_owned();
        if self.trailing_omitted {
            while digits.len() < self.integers + self.decimals {
                digits.push('0');
            }
        }
        let n: f64 = digits.parse().ok()?;
        let value = n / 10f64.powi(i32::try_from(self.decimals).ok()?) * self.unit;
        Some(if negative { -value } else { value })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Drill,
    Rapid,
    Cut,
}

/// What a drill file holds, drawn.
pub struct Drawn {
    pub svg: Option<String>,
    pub details: Vec<(String, String)>,
}

/// The numbers after `X` and `Y` in a line, if any.
fn coordinates(line: &str) -> (Option<&str>, Option<&str>) {
    let field = |letter: char| {
        let at = line.find(letter)?;
        let rest = &line[at + 1..];
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
            .unwrap_or(rest.len());
        Some(&rest[..end]).filter(|v| !v.is_empty())
    };
    (field('X'), field('Y'))
}

/// A drill file drawn.
pub fn draw(text: &str, cancel: &skwir_core::CancelToken) -> anyhow::Result<Drawn> {
    let mut numbers = Numbers {
        unit: 25.4,
        integers: 2,
        decimals: 4,
        trailing_omitted: false,
    };
    let mut tools: BTreeMap<u32, f64> = BTreeMap::new();
    let mut tool = None;
    let mut at: Point = (0., 0.);
    let mut absolute = true;
    // Drilling (`G05`), or routing: rapid moves (`G00`), and cuts (`G01`)
    // while the tool is down (`M15`).
    let mut mode = Mode::Drill;
    let mut plunged = false;
    let mut holes: Vec<(Point, f64)> = Vec::new();
    let mut paths: Vec<(Vec<Point>, f64)> = Vec::new();
    let mut plating = None;
    let mut software = None;
    for (index, raw) in text.lines().enumerate() {
        if index.is_multiple_of(65_536) {
            skwir_platform::inspection::check(cancel)?;
        }
        let line = raw.trim();
        if let Some(comment) = line.strip_prefix(';') {
            // KiCad's attributes: `; #@! TF.FileFunction,Plated,1,2,PTH`.
            let comment = comment.trim().trim_start_matches("#@!").trim();
            if let Some(function) = comment.strip_prefix("TF.FileFunction,") {
                plating = Some(match function.split(',').next().unwrap_or_default() {
                    "Plated" => "Plated",
                    "NonPlated" => "Not plated",
                    _ => "Plated and not",
                });
            } else if let Some(generator) = comment.strip_prefix("TF.GenerationSoftware,") {
                software = Some(generator.replace(',', " "));
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        // Units, and which zeros numbers leave out, with their format.
        if line.starts_with("METRIC") || line.starts_with("INCH") {
            let metric = line.starts_with("METRIC");
            numbers.unit = if metric { 1. } else { 25.4 };
            (numbers.integers, numbers.decimals) = if metric { (3, 3) } else { (2, 4) };
            numbers.trailing_omitted = line.contains(",LZ");
            if let Some(format) = line
                .split(',')
                .find(|part| part.contains('.') && part.chars().all(|c| c == '0' || c == '.'))
            {
                let (integers, decimals) = format.split_once('.').unwrap_or((format, ""));
                (numbers.integers, numbers.decimals) = (integers.len(), decimals.len());
            }
            continue;
        }
        match line {
            "M71" => {
                numbers.unit = 1.;
                continue;
            }
            "M72" => {
                numbers.unit = 25.4;
                continue;
            }
            "G90" => {
                absolute = true;
                continue;
            }
            "G91" | "ICI,ON" => {
                absolute = false;
                continue;
            }
            "M15" => {
                plunged = true;
                continue;
            }
            "M16" | "M17" => {
                plunged = false;
                continue;
            }
            "M30" | "M00" => break,
            _ => {}
        }
        // A tool, defined (`T1C0.8`, `T01F00S00C0.0300`) or chosen (`T1`).
        if let Some(rest) = line.strip_prefix('T') {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if let Ok(number) = rest[..digits].parse::<u32>() {
                if let Some(size) = rest.find('C').map(|c| &rest[c + 1..]) {
                    let end = size
                        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                        .unwrap_or(size.len());
                    if let Ok(diameter) = size[..end].parse::<f64>() {
                        tools.insert(number, diameter * numbers.unit);
                    }
                }
                tool = Some(number);
            }
            continue;
        }
        let diameter = tool.and_then(|t| tools.get(&t)).copied().unwrap_or(0.);
        if line.starts_with("G00") {
            mode = Mode::Rapid;
        } else if line.starts_with("G01") {
            mode = Mode::Cut;
        } else if line.starts_with("G05") || line.starts_with("G81") {
            mode = Mode::Drill;
        }
        let (x, y) = coordinates(line);
        if x.is_none() && y.is_none() {
            continue;
        }
        // A slot: `X1Y1G85X2Y2`.
        if let Some(split) = line.find("G85") {
            let (first, second) = line.split_at(split);
            let start = point(coordinates(first), at, &numbers, absolute);
            let end = point(coordinates(&second[3..]), start, &numbers, absolute);
            paths.push((vec![start, end], diameter));
            at = end;
            continue;
        }
        let to = point((x, y), at, &numbers, absolute);
        match mode {
            Mode::Drill => {
                if holes.len() < HOLES {
                    holes.push((to, diameter));
                }
            }
            Mode::Cut if plunged => match paths.last_mut() {
                Some((path, width))
                    if path.last() == Some(&at) && (*width - diameter).abs() < 1e-9 =>
                {
                    path.push(to);
                }
                _ => paths.push((vec![at, to], diameter)),
            },
            Mode::Cut | Mode::Rapid => {}
        }
        at = to;
    }
    let mut bounds = Bounds::new();
    for (p, d) in &holes {
        bounds.add((p.0, -p.1), d / 2.);
    }
    for (path, width) in &paths {
        for p in path {
            bounds.add((p.0, -p.1), width / 2.);
        }
    }
    let mut details = Vec::new();
    if let Some(plating) = plating {
        details.push(("Holes".to_owned(), plating.to_owned()));
    }
    if let Some(software) = software {
        details.push(("Made with".into(), software));
    }
    details.push(("Drilled".into(), holes.len().to_string()));
    if !paths.is_empty() {
        details.push(("Slots and routes".into(), paths.len().to_string()));
    }
    if !tools.is_empty() {
        let sizes: Vec<f64> = tools.values().copied().collect();
        let (smallest, largest) = sizes.iter().fold((f64::INFINITY, 0f64), |(lo, hi), d| {
            (lo.min(*d), hi.max(*d))
        });
        let round = |v: f64| (v * 1000.).round() / 1000.;
        details.push(("Tools".into(), tools.len().to_string()));
        details.push((
            "Sizes".into(),
            if (largest - smallest).abs() < 1e-9 {
                format!("{} mm", round(smallest))
            } else {
                format!("{} to {} mm", round(smallest), round(largest))
            },
        ));
    }
    Ok(Drawn {
        svg: write(&holes, &paths, bounds),
        details,
    })
}

/// A coordinate pair read against the last position (a missing axis is
/// kept).
fn point(
    (x, y): (Option<&str>, Option<&str>),
    at: Point,
    numbers: &Numbers,
    absolute: bool,
) -> Point {
    let axis = |value: Option<&str>, current: f64| match value.and_then(|v| numbers.value(v)) {
        Some(v) if absolute => v,
        Some(v) => current + v,
        None => current,
    };
    (axis(x, at.0), axis(y, at.1))
}

fn write(holes: &[(Point, f64)], paths: &[(Vec<Point>, f64)], bounds: Bounds) -> Option<String> {
    if bounds.empty {
        return None;
    }
    let margin = bounds.width().max(bounds.height()) * 0.03 + 0.5;
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
    out.push_str("\"/><g fill=\"");
    svg::color(&mut out, HOLE);
    out.push_str("\" stroke=\"");
    svg::color(&mut out, HOLE);
    out.push_str("\" stroke-linecap=\"round\" stroke-linejoin=\"round\">");
    for (p, d) in holes {
        out.push_str("<circle cx=\"");
        svg::num(&mut out, p.0);
        out.push_str("\" cy=\"");
        svg::num(&mut out, -p.1);
        out.push_str("\" r=\"");
        // A hole too small to see is shown a tenth of a millimetre wide.
        svg::num(&mut out, (d / 2.).max(0.05));
        out.push_str("\" stroke=\"none\"/>");
    }
    for (path, width) in paths {
        out.push_str("<path fill=\"none\" d=\"");
        for (index, p) in path.iter().enumerate() {
            out.push(if index == 0 { 'M' } else { 'L' });
            svg::pair(&mut out, (p.0, -p.1));
        }
        out.push_str("\" stroke-width=\"");
        svg::num(&mut out, width.max(0.1));
        out.push_str("\"/>");
    }
    out.push_str("</g></svg>");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::CancelToken;

    #[test]
    fn a_kicad_drill_file() {
        let text = "M48\n; #@! TF.GenerationSoftware,Kicad,Pcbnew,10.0\n; #@! TF.FileFunction,Plated,1,2,PTH\nFMAT,2\nMETRIC\nT1C0.800\nT2C3.200\n%\nG90\nG05\nT1\nX10.0Y-5.0\nX20.0Y-5.0\nT2\nX0Y0\nX1.0Y-1.0G85X3.0Y-1.0\nM30\n";
        let drawn = draw(text, &CancelToken::new()).unwrap();
        assert!(drawn.svg.unwrap().contains("<circle"));
        let said: Vec<String> = drawn
            .details
            .iter()
            .map(|(l, v)| format!("{l}: {v}"))
            .collect();
        assert_eq!(
            said,
            [
                "Holes: Plated",
                "Made with: Kicad Pcbnew 10.0",
                "Drilled: 3",
                "Slots and routes: 1",
                "Tools: 2",
                "Sizes: 0.8 to 3.2 mm"
            ]
        );
    }

    #[test]
    fn numbers_without_their_point() {
        // Inches, leading zeros left out (TZ: trailing kept), 2.4.
        let inches = Numbers {
            unit: 25.4,
            integers: 2,
            decimals: 4,
            trailing_omitted: false,
        };
        assert_eq!(inches.value("5000"), Some(12.7));
        // Millimetres, trailing zeros left out (LZ), 3.3: `0127` is 12.7.
        let millimetres = Numbers {
            unit: 1.,
            integers: 3,
            decimals: 3,
            trailing_omitted: true,
        };
        assert_eq!(millimetres.value("0127"), Some(12.7));
        let text = "M48\nINCH,TZ\nT01C0.0300\n%\nT01\nX5000Y-2500\nM30\n";
        let drawn = draw(text, &CancelToken::new()).unwrap();
        assert!(drawn.svg.unwrap().contains("cx=\"12.7\" cy=\"6.35\""));
        assert!(looks_like("M48\n;comment\n"));
        assert!(!looks_like("hello"));
    }
}
