//! Mermaid diagrams, laid out by `bibli-mermaid-worker` in a process of its
//! own: the layout engine is large, and bibli carries it only where the
//! helper is installed. What comes back is SVG made for resvg, drawn like
//! any drawing in a document (`inspection::image`).
use std::path::{Path, PathBuf};

use bibli_core::CancelToken;

use crate::inspection::limits;

/// A diagram's source handed over at most; the helper refuses more.
pub const SOURCE: usize = 64 * 1024;

/// The helper. Plugins that require it name this, not the literals.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "BIBLI_MERMAID_WORKER",
    executable: "bibli-mermaid-worker",
};

/// Where the helper is, if it is.
pub fn worker_path() -> Option<PathBuf> {
    WORKER.find()
}

/// The SVG of the diagram `source` describes. An error when the helper is
/// missing, or says `source` is not a diagram it can draw.
pub fn mermaid(source: &str, cancel: &CancelToken) -> anyhow::Result<String> {
    anyhow::ensure!(source.len() <= SOURCE, "The diagram is too long to draw");
    let worker = WORKER.require("draw diagrams")?;
    drawn(&worker, source, cancel).map_err(|error| crate::process::explain(&worker, cancel, error))
}

/// [`mermaid`], drawn by the helper at `worker`.
fn drawn(worker: &Path, source: &str, cancel: &CancelToken) -> anyhow::Result<String> {
    // Isolated as the other helpers reading what a file holds are: the
    // diagram, a page's text, comes on its standard input, and is the
    // input it may read too.
    let work = tempfile::tempdir()?;
    let input = work.path().join("diagram.mmd");
    std::fs::write(&input, source)?;
    let mut run =
        crate::sandbox::untrusted(worker, &input, work.path(), crate::sandbox::Otherwise::Run)?;
    let finished = crate::process::run_to_exit(
        &mut run.command,
        Some(source.as_bytes()),
        cancel,
        limits::HELPER_TIMEOUT,
    )?;
    anyhow::ensure!(
        finished.status.success(),
        "The diagram cannot be drawn: {}",
        finished.stderr.trim()
    );
    Ok(String::from_utf8(finished.stdout)?)
}
