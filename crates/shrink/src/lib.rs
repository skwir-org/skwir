//! Smaller copies of pictures and PDFs (ADR-112), for the print helper's
//! `--shrink` mode: a picture resized and written again (`picture`), a
//! PDF's images replaced where they are stored, its pages' drawing never
//! rewritten, what it stores twice kept once, the rest packed tighter
//! (`pdf`); a scan's black-and-white pages written as JBIG2 without loss
//! (`bitonal`). The print helper also writes the pages a document's reader
//! drew as small as they read (`page`).

pub mod bitonal;
pub mod page;
pub mod pdf;
pub mod picture;
pub mod samples;

use anyhow::{Context, Result};
use skwir_core::shrink::{Job, Kind, Made};

/// The smaller copy `job` asks: at `job.output` when `Made::written`.
/// `progress` is told how far it went, as done of a total, as it goes.
pub fn run(job: &Job, progress: &mut dyn FnMut(u64, u64)) -> Result<Made> {
    match job.kind {
        Kind::Pdf => pdf::shrink(job, progress),
        Kind::Picture => {
            let before = std::fs::metadata(&job.input)
                .with_context(|| format!("{} could not be read", job.input.display()))?
                .len();
            let options = picture::Options {
                quality: job.quality,
                gray: job.gray,
                strip: job.strip,
                at_most: job.at_most,
            };
            progress(0, 1);
            let written = picture::shrink(&job.input, &job.output, &options)?;
            progress(1, 1);
            Ok(match written {
                Some(written) => Made {
                    before,
                    after: written.bytes,
                    written: true,
                    met: written.met,
                    changed: Vec::new(),
                    notes: written.notes,
                    rule: None,
                },
                None => Made {
                    before,
                    after: before,
                    written: false,
                    ..Made::default()
                },
            })
        }
    }
}
