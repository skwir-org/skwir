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
//! that box, for what overhangs it (an italic letter, a radical's hook);
//! its glyphs are outlines, nothing that could read a file or a URL. A
//! batch it cannot read is an exit status of 1 and a reason on standard
//! error. Pure Rust (RaTeX: KaTeX's TeX, its layout and its fonts), nothing
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
struct Answer {
    padding: f64,
    formulas: Vec<Typeset>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Typeset {
    Drawn {
        width: f64,
        height: f64,
        depth: f64,
        svg: String,
    },
    Refused {
        error: String,
    },
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
    let started = Instant::now();
    let formulas = batch
        .formulas
        .iter()
        .map(|formula| {
            let typeset = if started.elapsed() > DEADLINE {
                Err("the page's formulas took too long".to_owned())
            } else {
                std::panic::catch_unwind(|| typeset(formula))
                    .unwrap_or_else(|_| Err("the formula could not be typeset".to_owned()))
            };
            typeset.unwrap_or_else(|error| Typeset::Refused { error })
        })
        .collect();
    let answer = Answer {
        padding: PADDING,
        formulas,
    };
    let json = serde_json::to_vec(&answer).map_err(|error| error.to_string())?;
    std::io::stdout()
        .lock()
        .write_all(&json)
        .map_err(|error| format!("writing the formulas: {error}"))
}

fn typeset(formula: &Formula) -> Result<Typeset, String> {
    if formula.tex.len() > FORMULA {
        return Err(format!("the formula is longer than {FORMULA} bytes"));
    }
    let nodes = ratex_parser::parse(&formula.tex).map_err(|error| error.message)?;
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
    let list = ratex_layout::to_display_list(&root);
    let svg = ratex_svg::render_to_svg(
        &list,
        &ratex_svg::SvgOptions {
            font_size: UNITS,
            padding: PADDING * UNITS,
            embed_glyphs: true,
            ..Default::default()
        },
    );
    Ok(Typeset::Drawn {
        width: list.width,
        height: list.height,
        depth: list.depth,
        svg,
    })
}
