//! Formulas written in TeX, typeset by `skwir-math-worker` in a process of
//! its own: the typesetter and its fonts are large, and skwir carries them
//! only where the helper is installed (ADR-129). A page's formulas go in
//! one batch, and each comes back as a drawing set in its line
//! (`inspection::Drawing`), or as nothing, where it stays the TeX it is
//! written in.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use skwir_core::{
    CancelToken,
    inspection::{Block, Drawing, Tag},
};

use crate::inspection::limits;

pub mod delimited;
pub mod mathml;

/// Formulas a page typesets at most; past them each stays its TeX.
pub const FORMULAS: usize = 2048;
/// A formula handed over at most; the helper refuses more.
pub const FORMULA: usize = 16 * 1024;
/// All a batch's TeX handed over at most, within the helper's input.
const BATCH: usize = 768 * 1024;
/// A formula's em to the text's, as KaTeX sets them: Computer Modern's
/// letters are smaller than most text faces' at one size.
const SCALE: f32 = 1.21;

/// The helper. Plugins that require it name this, not the literals.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "SKWIR_MATH_WORKER",
    executable: "skwir-math-worker",
};

/// Where the helper is, if it is.
pub fn worker_path() -> Option<PathBuf> {
    WORKER.find()
}

/// A formula to typeset: on a line of its own (`$$…$$`), or in a sentence.
#[derive(Clone, Debug, Serialize)]
pub struct Formula {
    pub tex: String,
    pub display: bool,
}

#[derive(Deserialize)]
struct Answer {
    padding: f32,
    formulas: Vec<Typeset>,
}

#[derive(Deserialize)]
struct Typeset {
    width: Option<f32>,
    height: Option<f32>,
    depth: Option<f32>,
    svg: Option<String>,
    /// Its number, drawn apart (`\tag`).
    tag: Option<Number>,
}

#[derive(Deserialize)]
struct Number {
    gap: f32,
    width: f32,
    svg: String,
}

/// What tells one SVG from another, for a view to keep its raster by.
fn id_of(svg: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    svg.hash(&mut hasher);
    hasher.finish()
}

/// Each formula's drawing, in order: `None` for one that cannot be typeset
/// or that the batch's bounds leave out (past `FORMULAS`, or the batch's
/// TeX), each saying its TeX. An error when the helper is missing or fails
/// as a whole.
pub fn typeset(formulas: &[Formula], cancel: &CancelToken) -> anyhow::Result<Vec<Option<Drawing>>> {
    let mut asked = Vec::new();
    let mut size = 0;
    for formula in formulas.iter().take(FORMULAS) {
        size += formula.tex.len();
        if formula.tex.len() > FORMULA || size > BATCH {
            break;
        }
        asked.push(formula);
    }
    let mut drawn = Vec::with_capacity(formulas.len());
    if !asked.is_empty() {
        let worker = WORKER.require("typeset formulas")?;
        let answer = typeset_by(&worker, &asked, cancel)
            .map_err(|error| crate::process::explain(&worker, cancel, error))?;
        anyhow::ensure!(
            answer.formulas.len() == asked.len(),
            "The formulas came back {} for {}",
            answer.formulas.len(),
            asked.len()
        );
        drawn.extend(
            answer
                .formulas
                .into_iter()
                .zip(&asked)
                .map(|(typeset, formula)| {
                    let svg = typeset.svg?.into_bytes();
                    let (ascent, descent) = (typeset.height? * SCALE, typeset.depth? * SCALE);
                    let margin = answer.padding * SCALE;
                    let tag = typeset.tag.map(|number| {
                        let svg = number.svg.into_bytes();
                        Arc::new(Tag {
                            gap: number.gap * SCALE,
                            drawing: Drawing {
                                width: number.width * SCALE,
                                ascent,
                                descent,
                                margin,
                                id: id_of(&svg),
                                svg: Arc::from(svg),
                                text: String::new(),
                                tag: None,
                            },
                        })
                    });
                    Some(Drawing {
                        width: typeset.width? * SCALE,
                        ascent,
                        descent,
                        margin,
                        id: id_of(&svg),
                        svg: Arc::from(svg),
                        text: formula.tex.clone(),
                        tag,
                    })
                }),
        );
    }
    drawn.resize(formulas.len(), None);
    Ok(drawn)
}

/// [`typeset`], by the helper at `worker`.
fn typeset_by(
    worker: &Path,
    formulas: &[&Formula],
    cancel: &CancelToken,
) -> anyhow::Result<Answer> {
    // Isolated as the other helpers reading what a file holds are: the
    // formulas, a page's text, come on its standard input, and are the
    // input it may read too.
    let input = serde_json::to_vec(&serde_json::json!({ "formulas": formulas }))?;
    let work = tempfile::tempdir()?;
    let path = work.path().join("formulas.json");
    std::fs::write(&path, &input)?;
    let mut run =
        crate::sandbox::untrusted(worker, &path, work.path(), crate::sandbox::Otherwise::Run)?;
    let finished = crate::process::run_to_exit(
        &mut run.command,
        Some(&input),
        cancel,
        limits::HELPER_TIMEOUT,
    )?;
    anyhow::ensure!(
        finished.status.success(),
        "The formulas cannot be typeset: {}",
        finished.stderr.trim()
    );
    Ok(serde_json::from_slice(&finished.stdout)?)
}

/// The formulas of a page, typeset together once it is read (`typeset`):
/// one call of the helper for the page, a notebook's cells and all. Each
/// stands in its line as a drawing to come (`ask`), and as what it was
/// written as where the helper is not installed, past the page's bound, or
/// where it cannot be typeset.
#[derive(Default)]
pub struct Pending {
    /// Whether the helper is installed, asked once a page.
    helper: Option<bool>,
    /// Each drawing to come, and the formula it is.
    asked: Vec<(Arc<Drawing>, Formula)>,
}

impl Pending {
    /// Whether formulas can be typeset here: the helper is installed.
    pub fn typesets(&mut self) -> bool {
        *self.helper.get_or_insert_with(|| worker_path().is_some())
    }

    /// The drawing to come for `tex`, which reads as `written` (what it is
    /// copied as, and shown as where it is not drawn), or `None` where it
    /// stays as written.
    pub fn ask(&mut self, tex: &str, display: bool, written: String) -> Option<Arc<Drawing>> {
        let tex = tex.trim();
        if tex.is_empty() || tex.len() > FORMULA || self.asked.len() >= FORMULAS || !self.typesets()
        {
            return None;
        }
        let drawing = Arc::new(Drawing {
            svg: Arc::from([]),
            id: 0,
            width: 0.,
            ascent: 0.,
            descent: 0.,
            margin: 0.,
            text: written,
            tag: None,
        });
        self.asked.push((
            drawing.clone(),
            Formula {
                tex: tex.to_owned(),
                display,
            },
        ));
        Some(drawing)
    }

    /// Typeset the formulas `blocks` hold, those of the page kept, in one
    /// batch: each drawing to come becomes the one drawn, or what it was
    /// written as, as code; one alone on its line, the code block of its
    /// TeX.
    pub fn typeset(self, blocks: &mut [Block], cancel: &CancelToken) {
        if self.asked.is_empty() {
            return;
        }
        let at: std::collections::HashMap<*const Drawing, usize> = self
            .asked
            .iter()
            .enumerate()
            .map(|(k, (drawing, _))| (Arc::as_ptr(drawing), k))
            .collect();
        let found = |run: &skwir_core::inspection::Run| {
            run.drawing
                .as_ref()
                .and_then(|drawing| at.get(&Arc::as_ptr(drawing)).copied())
        };
        let mut kept: Vec<usize> = blocks
            .iter()
            .flat_map(|block| &block.runs)
            .filter_map(found)
            .collect();
        kept.sort_unstable();
        kept.dedup();
        let formulas: Vec<Formula> = kept.iter().map(|&k| self.asked[k].1.clone()).collect();
        let drawn = typeset(&formulas, cancel).unwrap_or_else(|error| {
            tracing::debug!("formulas left as written: {error:#}");
            vec![None; formulas.len()]
        });
        let drawn: std::collections::HashMap<usize, Option<Drawing>> =
            kept.into_iter().zip(drawn).collect();
        for block in blocks {
            let mut undrawn = Vec::new();
            for (index, run) in block.runs.iter_mut().enumerate() {
                let Some(k) = found(run) else {
                    continue;
                };
                match drawn.get(&k).cloned().flatten() {
                    Some(typeset) => {
                        run.drawing = Some(Arc::new(Drawing {
                            text: self.asked[k].0.text.clone(),
                            ..typeset
                        }));
                    }
                    None => undrawn.push((index, k)),
                }
            }
            // From the last, so the ranges before each stay where they are.
            for (index, k) in undrawn.into_iter().rev() {
                let (written, formula) = &self.asked[k];
                if formula.display && block.runs.len() == 1 {
                    // A formula on a line of its own is the code it is.
                    *block = Block {
                        code: true,
                        language: Some("latex".into()),
                        text: formula.tex.clone(),
                        quote: block.quote,
                        list_depth: block.list_depth,
                        ..Block::default()
                    };
                } else {
                    block.undraw(index, &written.text);
                }
            }
        }
    }
}
