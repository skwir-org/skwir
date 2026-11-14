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
    inspection::{Block, BlockKind, Drawing, Tag},
};

use crate::inspection::limits;

pub mod delimited;
pub mod mathml;

/// Formulas a page typesets at most; past them each stays its TeX.
pub const FORMULAS: usize = 2048;
/// A formula handed over at most; the helper refuses more.
pub const FORMULA: usize = 16 * 1024;
/// What the helper reads of a batch at most (`skwir-math-worker`'s
/// `INPUT`): its JSON, as it is handed over.
const INPUT: usize = 1024 * 1024;
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
/// or that the batch's bounds leave out (past `FORMULAS`, longer than
/// `FORMULA`, or past the helper's input), each saying its TeX. An error
/// when the helper is missing or fails as a whole; its answer is bounded
/// by the helper itself, a formula at a time.
pub fn typeset(formulas: &[Formula], cancel: &CancelToken) -> anyhow::Result<Vec<Option<Drawing>>> {
    let asked = batch(formulas)?;
    let mut drawn = vec![None; formulas.len()];
    if asked.is_empty() {
        return Ok(drawn);
    }
    let worker = WORKER.require("typeset formulas")?;
    let batch: Vec<&Formula> = asked.iter().map(|&k| &formulas[k]).collect();
    let answer = typeset_by(&worker, &batch, cancel)
        .map_err(|error| crate::process::explain(&worker, cancel, error))?;
    anyhow::ensure!(
        answer.formulas.len() == asked.len(),
        "The formulas came back {} for {}",
        answer.formulas.len(),
        asked.len()
    );
    let margin = answer.padding * SCALE;
    for (typeset, k) in answer.formulas.into_iter().zip(asked) {
        drawn[k] = drawing_of(typeset, margin, &formulas[k]);
    }
    Ok(drawn)
}

/// Which of `formulas` a batch asks for, within its bounds: measured as the
/// helper reads it, each formula's JSON (its backslashes and line breaks
/// escaped) and a comma between, within its input. One that does not fit
/// is left out, and those after it still asked.
fn batch(formulas: &[Formula]) -> anyhow::Result<Vec<usize>> {
    let mut asked = Vec::new();
    let mut size = r#"{"formulas":[]}"#.len();
    for (k, formula) in formulas.iter().enumerate().take(FORMULAS) {
        if formula.tex.len() > FORMULA {
            continue;
        }
        let json = serde_json::to_string(formula)?.len() + 1;
        if size + json <= INPUT {
            size += json;
            asked.push(k);
        }
    }
    Ok(asked)
}

/// The drawing the helper answered for `formula`, if it drew one.
fn drawing_of(typeset: Typeset, margin: f32, formula: &Formula) -> Option<Drawing> {
    let svg = typeset.svg?.into_bytes();
    let (ascent, descent) = (typeset.height? * SCALE, typeset.depth? * SCALE);
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
    /// Each drawing to come, the formula it is, and how it shows where it
    /// is not drawn: what it reads as, as code (the TeX a page wrote), or
    /// as text (`ask_reading`).
    asked: Vec<(Arc<Drawing>, Formula, Undrawn)>,
}

/// How a formula shows where it is not drawn (`Pending::typeset`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Undrawn {
    /// What it was written as, as code; on a line of its own, the code
    /// block of its TeX.
    Code,
    /// What it reads as, as text: the page wrote no TeX of it.
    Text,
}

impl Pending {
    /// Whether formulas can be typeset here: the helper is installed.
    pub fn typesets(&mut self) -> bool {
        *self.helper.get_or_insert_with(|| worker_path().is_some())
    }

    /// The drawing to come for `tex`, which reads as `written` (what it is
    /// copied as, and shown as code where it is not drawn), or `None` where
    /// it stays as written.
    pub fn ask(&mut self, tex: &str, display: bool, written: String) -> Option<Arc<Drawing>> {
        self.asked_as(tex, display, written, Undrawn::Code)
    }

    /// The drawing to come for `tex`, a formula the page wrote otherwise
    /// (MathML's markup, written as TeX to typeset it): it reads as `reads`
    /// (what it is copied as and said as, and shown as text where it is not
    /// drawn), never as that TeX. `None` where it stays as the page has it.
    pub fn ask_reading(&mut self, tex: &str, display: bool, reads: String) -> Option<Arc<Drawing>> {
        self.asked_as(tex, display, reads, Undrawn::Text)
    }

    fn asked_as(
        &mut self,
        tex: &str,
        display: bool,
        written: String,
        undrawn: Undrawn,
    ) -> Option<Arc<Drawing>> {
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
            undrawn,
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
            .map(|(k, (drawing, ..))| (Arc::as_ptr(drawing), k))
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
                let (written, formula, shown) = &self.asked[k];
                if *shown == Undrawn::Code
                    && formula.display
                    && block.runs.len() == 1
                    && matches!(block.kind, BlockKind::Text)
                {
                    // A formula on a line of its own is the code it is,
                    // where it stood: the names a link reaches it by, its
                    // item's marker, how it follows the block before. A
                    // row of a table stays a row, the formula its TeX in it.
                    let text = formula.tex.clone();
                    let mut unseen = std::mem::take(&mut block.unseen);
                    for (at, _) in &mut unseen {
                        *at = if *at == 0 { 0 } else { text.len() };
                    }
                    *block = Block {
                        code: true,
                        language: Some("latex".into()),
                        quote: block.quote,
                        list_depth: block.list_depth,
                        anchors: std::mem::take(&mut block.anchors),
                        marker: block.marker.take(),
                        flow: block.flow,
                        note: block.note,
                        indent: block.indent,
                        unseen,
                        text,
                        ..Block::default()
                    };
                } else {
                    block.undraw(index, &written.text, *shown == Undrawn::Code);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::inspection::Run;

    /// A batch is bounded by what the helper reads, its JSON: TeX that is
    /// backslashes escaped to twice its length is measured so; a formula
    /// too long is left out, the next still asked.
    #[test]
    fn a_batch_is_what_the_helper_reads() {
        let formula = |tex: String| Formula {
            tex,
            display: false,
        };
        let mut formulas: Vec<Formula> = (0..400).map(|_| formula("\\".repeat(1500))).collect();
        formulas.insert(1, formula("x".repeat(FORMULA + 1)));
        let asked = batch(&formulas).unwrap();
        assert_eq!(asked[..2], [0, 2], "the long one left out, not the rest");
        let refs: Vec<&Formula> = asked.iter().map(|&k| &formulas[k]).collect();
        let input = serde_json::to_vec(&serde_json::json!({ "formulas": refs })).unwrap();
        assert!(input.len() <= INPUT, "{} past {INPUT}", input.len());
        assert!(input.len() > INPUT - 4000, "as many as fit");
        assert!(asked.len() < 400);
    }

    /// A formula that is not drawn shows as it was written, as code, and
    /// one on a line of its own as the code block of its TeX; one the page
    /// wrote no TeX of (MathML's markup) as what it reads as, as text.
    #[test]
    fn a_formula_not_drawn_shows_as_written_or_as_it_reads() {
        // SAFETY: no other thread of this test process reads the
        // environment; nextest runs each test in a process of its own.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var(WORKER.variable, "/nonexistent/skwir-math-worker");
        }
        // As though the helper were there, and then could not be run.
        let mut pending = Pending {
            helper: Some(true),
            ..Pending::default()
        };
        let mut written = Block::default();
        written.text.push_str("Let ");
        let drawing = pending.ask("x+1", false, "$x+1$".into()).unwrap();
        written.push_drawing(drawing, &Run::default());
        let mut reading = Block::default();
        let drawing = pending
            .ask_reading("{x}^{2}", true, "x squared".into())
            .unwrap();
        reading.push_drawing(drawing, &Run::default());
        // Words said only after it stay after it.
        written.unseen.push((written.text.len(), "said".into()));
        let mut apart = Block {
            anchors: vec!["roots".into()],
            marker: Some("1.".into()),
            list_depth: 1,
            ..Block::default()
        };
        let drawing = pending.ask("y", true, "$$y$$".into()).unwrap();
        apart.push_drawing(drawing, &Run::default());
        let mut row = Block {
            kind: BlockKind::TableRow {
                cells: std::iter::once(0..Block::DRAWN.len_utf8()).collect(),
                header: false,
            },
            ..Block::default()
        };
        let drawing = pending.ask("z", true, "$$z$$".into()).unwrap();
        row.push_drawing(drawing, &Run::default());
        let mut blocks = [written, reading, apart, row];
        pending.typeset(&mut blocks, &CancelToken::new());
        let [written, reading, apart, row] = &blocks;
        assert_eq!(written.text, "Let $x+1$");
        assert!(written.runs[0].code && written.runs[0].drawing.is_none());
        assert_eq!(written.unseen, [(written.text.len(), "said".to_owned())]);
        assert_eq!(written.said(0..written.text.len()), "Let $x+1$said");
        assert_eq!(reading.text, "x squared");
        assert!(!reading.code && !reading.runs[0].code, "words, not code");
        assert!(apart.code);
        assert_eq!(apart.language.as_deref(), Some("latex"));
        assert_eq!(apart.text, "y");
        assert_eq!(apart.anchors, ["roots"], "a link still lands on it");
        assert_eq!(apart.marker.as_deref(), Some("1."));
        assert_eq!(row.text, "$$z$$", "a row stays a row");
        assert!(
            matches!(&row.kind, BlockKind::TableRow { cells, .. } if cells.len() == 1 && cells[0] == (0..5))
        );
    }
}
