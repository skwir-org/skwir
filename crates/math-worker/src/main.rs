//! Formulas written in TeX typeset as SVG, in a process of its own: the
//! typesetter and its fonts are large, and skwir pays for them on disk only
//! where this helper is installed. Reads a batch of formulas as JSON on
//! standard input:
//!
//! ```json
//! {"formulas": [{"tex": "x^2", "display": false}]}
//! ```
//!
//! and writes, for each in order, its drawing or why there is none:
//!
//! ```json
//! {"padding": 0.25, "formulas": [{"width": 1.2, "height": 0.8, "depth": 0.0, "svg": "<svg…"}, {"error": "…"}]}
//! ```
//!
//! Sizes are in ems of the text the formula sits in: `height` above the
//! baseline, `depth` below it. The SVG reaches `padding` past each side of
//! that box, for what overhangs it (an italic letter, a radical's hook).
//! A formula's number (`\tag`) comes apart, as `tag`: its SVG on the same
//! baseline, `gap` past the formula, for the caller to set at the end of
//! the line. Equations are not numbered by themselves (MathJax's default,
//! which Jupyter keeps): a number is the one a `\tag` writes;
//! its glyphs are outlines, nothing that could read a file or a URL. The
//! answer holds `OUTPUT` at most: a drawing that would take it past is
//! refused, as one past the batch's `DEADLINE` is. A batch it cannot read
//! is an exit status of 1 and a reason on standard error. Pure Rust (RaTeX: KaTeX's TeX, its layout and its fonts), nothing
//! written.
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// A batch read at most: a page's formulas.
const INPUT: u64 = 1024 * 1024;
/// Formulas typeset at most in a batch; the caller asks no more.
const FORMULAS: usize = 4096;
/// One formula's source at most: a long derivation is a few lines.
const FORMULA: usize = 16 * 1024;
/// How long a batch may take; what is left is refused, and the caller has
/// a deadline of its own.
const DEADLINE: Duration = Duration::from_secs(5);
/// What a batch's answer holds at most, its drawings' JSON together: each
/// that would take it past is refused, the others kept. Well within what
/// the caller reads of a helper's answer (`limits::HELPER_OUTPUT`, 33 MiB),
/// which it would otherwise refuse whole.
const OUTPUT: usize = 24 * 1024 * 1024;
/// How far the drawing reaches past the formula's box, in ems.
const PADDING: f64 = 0.25;
/// The SVG's user units per em.
const UNITS: f64 = 100.;

#[derive(Deserialize)]
struct Batch {
    formulas: Vec<Formula>,
}

#[derive(Deserialize)]
struct Formula {
    tex: String,
    /// Set apart on a line of its own (`$$…$$`), or in a sentence (`$…$`).
    #[serde(default)]
    display: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Typeset {
    Drawn {
        width: f64,
        height: f64,
        depth: f64,
        svg: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tag: Option<Tag>,
    },
    Refused {
        error: String,
    },
}

/// A formula's number, drawn apart: `gap` past the formula, `width` wide,
/// on its baseline.
#[derive(Serialize)]
struct Tag {
    gap: f64,
    width: f64,
    svg: String,
}

/// The environments that number their lines, which are typeset as their
/// starred forms: an equation is numbered by its `\tag` alone, as MathJax
/// numbers them unless a page asks otherwise (`tags: 'none'`), and as
/// formulas typeset one at a time could not count them.
const NUMBERED: [&str; 4] = ["equation", "align", "gather", "alignat"];

fn unnumbered(tex: &str) -> String {
    let mut tex = tex.to_owned();
    for name in NUMBERED {
        for (from, to) in [("begin", "begin"), ("end", "end")] {
            tex = tex.replace(
                &format!("\\{from}{{{name}}}"),
                &format!("\\{to}{{{name}*}}"),
            );
        }
    }
    tex
}

/// `nodes` without their numbers (a `\tag`, an environment's), if they have
/// any: what lays the formula out without its number, so the number can be
/// set apart from it.
fn untagged(nodes: &[ratex_parser::ParseNode]) -> Option<Vec<ratex_parser::ParseNode>> {
    use ratex_parser::ParseNode;
    let mut found = false;
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            ParseNode::Tag { body, .. } => {
                found = true;
                out.extend(body.iter().cloned());
            }
            ParseNode::Array { tags: Some(_), .. } => {
                found = true;
                let mut node = node.clone();
                if let ParseNode::Array { tags, .. } = &mut node {
                    *tags = None;
                }
                out.push(node);
            }
            other => out.push(other.clone()),
        }
    }
    found.then_some(out)
}

/// The x where an item of a display list is drawn from.
fn item_x(item: &ratex_types::DisplayItem) -> f64 {
    use ratex_types::DisplayItem;
    match item {
        DisplayItem::GlyphPath { x, .. }
        | DisplayItem::Line { x, .. }
        | DisplayItem::Rect { x, .. }
        | DisplayItem::Path { x, .. } => *x,
    }
}

/// The same item drawn `dx` further along.
fn moved(item: &ratex_types::DisplayItem, dx: f64) -> ratex_types::DisplayItem {
    use ratex_types::DisplayItem;
    let mut item = item.clone();
    match &mut item {
        DisplayItem::GlyphPath { x, .. }
        | DisplayItem::Line { x, .. }
        | DisplayItem::Rect { x, .. }
        | DisplayItem::Path { x, .. } => *x += dx,
    }
    item
}

fn svg(list: &ratex_types::DisplayList) -> String {
    ratex_svg::render_to_svg(
        list,
        &ratex_svg::SvgOptions {
            font_size: UNITS,
            padding: PADDING * UNITS,
            embed_glyphs: true,
            ..Default::default()
        },
    )
}

fn main() {
    // `--build`, skwir asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--build")
    {
        println!("{}", env!("SKWIR_BUILD"));
        return;
    }
    // `--probe`, skwir's settings asking whether this helper works here
    // (`platform::process::probe`): being started at all says it does.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--probe")
    {
        return;
    }
    if let Err(reason) = run() {
        eprintln!("{reason}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(INPUT + 1)
        .read_to_end(&mut input)
        .map_err(|error| format!("reading the formulas: {error}"))?;
    if input.len() as u64 > INPUT {
        return Err(format!("the formulas are longer than {INPUT} bytes"));
    }
    let batch: Batch =
        serde_json::from_slice(&input).map_err(|error| format!("reading the formulas: {error}"))?;
    if batch.formulas.len() > FORMULAS {
        return Err(format!("more than {FORMULAS} formulas"));
    }
    // A formula the typesetter fails on is refused alone, the others kept.
    std::panic::set_hook(Box::new(|_| {}));
    let out = answer(&batch.formulas, OUTPUT)?;
    std::io::stdout()
        .lock()
        .write_all(&out)
        .map_err(|error| format!("writing the formulas: {error}"))
}

/// The answer to `formulas` (see the top), written as it goes, its
/// drawings' JSON together `output` at most: one that would take it past
/// is refused, the others kept.
fn answer(formulas: &[Formula], output: usize) -> Result<Vec<u8>, String> {
    let started = Instant::now();
    let mut out = b"{\"padding\":".to_vec();
    out.extend_from_slice(&json(&PADDING)?);
    out.extend_from_slice(b",\"formulas\":[");
    for (n, formula) in formulas.iter().enumerate() {
        let typeset = if started.elapsed() > DEADLINE {
            Err("the page's formulas took too long".to_owned())
        } else {
            std::panic::catch_unwind(|| typeset(formula))
                .unwrap_or_else(|_| Err("the formula could not be typeset".to_owned()))
        };
        let typeset = typeset.unwrap_or_else(|error| Typeset::Refused { error });
        let mut item = json(&typeset)?;
        if out.len() + item.len() + 2 > output {
            item = json(&Typeset::Refused {
                error: "the page's formulas are too large to answer".to_owned(),
            })?;
        }
        if n > 0 {
            out.push(b',');
        }
        out.extend_from_slice(&item);
    }
    out.extend_from_slice(b"]}");
    Ok(out)
}

fn json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|error| error.to_string())
}

fn typeset(formula: &Formula) -> Result<Typeset, String> {
    if formula.tex.len() > FORMULA {
        return Err(format!("the formula is longer than {FORMULA} bytes"));
    }
    let nodes = ratex_parser::parse(&unnumbered(&formula.tex)).map_err(|error| error.message)?;
    let options = ratex_layout::LayoutOptions {
        style: if formula.display {
            ratex_types::MathStyle::Display
        } else {
            ratex_types::MathStyle::Text
        },
        ..Default::default()
    };
    let root = ratex_layout::layout(&nodes, &options);
    ratex_layout::validate_layout(&root)?;
    let mut list = ratex_layout::to_display_list(&root);
    // Its number apart: what is drawn from where the formula laid out
    // without it ends (a `\tag` follows it at once, an environment's
    // stands a gap past it); nothing of the formula is drawn from there.
    let tag = untagged(&nodes)
        .filter(|_| formula.display)
        .map(|nodes| ratex_layout::layout(&nodes, &options).width)
        .and_then(|width| {
            let (number, own): (Vec<_>, Vec<_>) = list
                .items
                .drain(..)
                .partition(|item| item_x(item) >= width - 1e-9);
            list.items = own;
            let from = number.iter().map(item_x).fold(f64::INFINITY, f64::min);
            if number.is_empty() {
                return None;
            }
            let tag = ratex_types::DisplayList {
                items: number.iter().map(|item| moved(item, -from)).collect(),
                width: list.width - from,
                height: list.height,
                depth: list.depth,
            };
            // At least an em clear of the formula, where the line has no
            // room to set it at its end.
            let gap = (from - width).max(1.);
            list.width = width;
            Some(Tag {
                gap,
                width: tag.width,
                svg: svg(&tag),
            })
        });
    Ok(Typeset::Drawn {
        width: list.width,
        height: list.height,
        depth: list.depth,
        svg: svg(&list),
        tag,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer stays within its bound: a drawing that would take it past
    /// is refused, and those after it that fit are kept.
    #[test]
    fn a_drawing_past_the_answer_s_bound_is_refused_alone() {
        let formula = |tex: &str| Formula {
            tex: tex.to_owned(),
            display: false,
        };
        let formulas = [formula("x"), formula(r"\int\int\int\int"), formula("y")];
        let whole = answer(&formulas, usize::MAX).unwrap();
        let read = |out: &[u8]| -> Vec<serde_json::Value> {
            let answer: serde_json::Value = serde_json::from_slice(out).unwrap();
            answer["formulas"].as_array().unwrap().clone()
        };
        let all = read(&whole);
        assert!(all.iter().all(|typeset| typeset["svg"].is_string()));
        let size = |typeset: &serde_json::Value| typeset.to_string().len();
        // Room for the first and the last, not the integrals.
        let bound = whole.len() - size(&all[1]) + 64;
        let out = answer(&formulas, bound).unwrap();
        assert!(out.len() <= bound, "{} past {bound}", out.len());
        let kept = read(&out);
        assert_eq!(kept.len(), 3, "each answered, in order");
        assert!(kept[0]["svg"].is_string());
        assert_eq!(
            kept[1]["error"].as_str(),
            Some("the page's formulas are too large to answer")
        );
        assert!(kept[2]["svg"].is_string(), "the next that fits is kept");
    }
}
