//! A smaller copy of a picture or a PDF (ADR-112), made by the print
//! helper's `--shrink` mode (`skwir-print-worker --shrink <job.json>`),
//! which links the engine (`skwir-shrink`): run isolated where the system
//! can, reading the file it shrinks and writing only in its own folder.
//!
//! A PDF's copy is checked before it is kept ([`made`]): each image
//! written anew is looked at on the first page that shows it, drawn from
//! the file and from the copy by the reader the settings choose (the
//! window's `PDFium`, Poppler or `MuPDF`), small, in gray. Losing detail is
//! what was asked; an image gone, its colours turned, moved or no longer
//! drawn is not: a page that looks different by more than a resized,
//! re-encoded image does has its images put back as they were, the copy
//! made again keeping them.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail, ensure};
use skwir_core::App;
use skwir_core::cancel::CancelToken;
use skwir_core::document::DocumentSession;
use skwir_core::op::EntryRef;
use skwir_core::preview::{HeldSlot, PreviewRequest};
use skwir_core::shrink::{Changed, Job, Kind, Made};

use crate::process::Follow;
use crate::sandbox::Otherwise;

/// The job written in the working folder.
const JOB: &str = "shrink.json";

/// Where the helper is, if it is: the print helper's executable.
pub fn worker_path() -> Option<PathBuf> {
    crate::print_job::worker_path()
}

/// A smaller copy of `job.input`, written by the helper in `work`, a folder
/// the caller made and removes, as `name`: `job.output` is set there. What
/// it answered, its copy at `job.output` when `Made::written`. It is
/// stopped when `cancel` is cancelled, and when it takes far longer than
/// the file's size should (`timeout`). Waits for the helper: a worker's
/// work.
pub fn run(
    job: &mut Job,
    work: &Path,
    name: &str,
    cancel: &CancelToken,
    follow: &Follow<'_>,
) -> Result<Made> {
    let program = crate::print_job::WORKER.require("make smaller copies")?;
    ran(&program, job, work, name, cancel, follow)
        .map_err(|error| crate::process::explain(&program, cancel, error))
}

/// The copy the helper makes of `job.input` in `work` as `name`, checked
/// when a PDF: an image on a page that looks different put back, the copy
/// made again keeping it. The copy's path, `None` when it would not be
/// smaller, and what the helper said of it. `follow` hears how far it
/// went: the helper's work, then the pages checked. Waits: a worker's work.
pub fn made(
    app: &App,
    mut job: Job,
    work: &Path,
    name: &str,
    cancel: &CancelToken,
    follow: &Follow<'_>,
) -> Result<(Option<PathBuf>, Made)> {
    let checked = job.kind == Kind::Pdf;
    let made_share = if checked { 0.8 } else { 1.0 };
    let mut made = run(&mut job, work, name, cancel, &follow.part(0.0, made_share))?;
    if made.written && checked && !made.changed.is_empty() {
        let keep = drifted(
            app,
            &job.input,
            &job.output,
            &made.changed,
            cancel,
            &follow.part(0.8, 0.95),
        )?;
        if !keep.is_empty() {
            tracing::info!(
                images = keep.len(),
                "images put back after their pages looked different"
            );
            job.keep = keep;
            made = run(&mut job, work, name, cancel, &follow.part(0.95, 1.0))?;
        }
    }
    follow.went(1.0);
    let copy = made.written.then(|| job.output.clone());
    Ok((copy, made))
}

/// [`run`], with the helper at `program`.
fn ran(
    program: &Path,
    job: &mut Job,
    work: &Path,
    name: &str,
    cancel: &CancelToken,
    follow: &Follow<'_>,
) -> Result<Made> {
    let output = work.join(name);
    let _ = std::fs::remove_file(&output);
    let input = job.input.clone();
    let size = std::fs::metadata(&input)
        .with_context(|| format!("{} could not be read", input.display()))?
        .len();
    let mut run = crate::sandbox::untrusted(program, &input, work, Otherwise::Run)?;
    // The job names the input and the copy where the helper sees them.
    job.input.clone_from(&run.input);
    job.output = run.work.join(name);
    let written = write_job(job, &work.join(JOB));
    job.input = input;
    job.output.clone_from(&output);
    written?;
    run.command.arg("--shrink").arg(run.work.join(JOB));
    let finished = crate::process::run_helper(&mut run.command, cancel, timeout(size, job), follow);
    let _ = std::fs::remove_file(work.join(JOB));
    let finished = finished?;
    if !finished.status.success() {
        let said = finished.stderr.lines().next().unwrap_or_default().trim();
        bail!(
            "No smaller copy could be made ({}): {said}",
            finished.status
        );
    }
    let said = String::from_utf8_lossy(&finished.stdout);
    let line = said
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .context("the shrink helper said nothing")?;
    let made: Made =
        serde_json::from_str(line).context("the shrink helper's answer could not be read")?;
    if made.written {
        let written = std::fs::metadata(&output)
            .map(|meta| meta.len())
            .context("the shrink helper wrote no copy")?;
        ensure!(written > 0, "the shrink helper wrote an empty copy");
        ensure!(
            written < size,
            "the shrink helper wrote a copy no smaller than the file"
        );
    }
    Ok(made)
}

/// The job written for the helper to read.
fn write_job(job: &Job, path: &Path) -> Result<()> {
    let file = std::fs::File::create(path)
        .with_context(|| format!("{} could not be written", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, job)?;
    writer.flush()?;
    Ok(())
}

/// How long the helper is given for a file of `size` bytes: a minute,
/// and a minute more for each 4 MiB (a 45 MB scan of 239 pages took 45 to
/// 75 s for `MuPDF`), times the steps a size asked may try.
fn timeout(size: u64, job: &Job) -> Duration {
    let minutes = 1 + size / (4 * 1024 * 1024);
    let tries = if job.at_most.is_some() { 4 } else { 1 };
    Duration::from_secs(60 * minutes * tries)
}

/// The longer side pages are drawn at to be compared: about 72 dpi on
/// A4, where a picture brought to 96 dpi looks as it did, and where an
/// image gone or its colours turned shows at once.
const SIDE: u32 = 600;

/// The most a page may differ on average, in gray levels of 255, and the
/// share of its pixels that may differ by more than [`FAR`]: what a
/// resized, re-encoded image leaves, measured on photographs and scans
/// brought to 96 dpi at JPEG 60 (well under 3 levels, under 1 %).
const MEAN: f64 = 6.0;
const FAR: u8 = 64;
const FAR_SHARE: f64 = 0.03;

/// The images of `changed` to put back: those on pages of `copy` that look
/// different from the same pages of `original`. Nothing when no reader
/// can draw them (none is on): the copy is then kept unchecked, and the
/// log says so. A page of the copy that cannot be drawn looks different.
fn drifted(
    app: &App,
    original: &Path,
    copy: &Path,
    changed: &[Changed],
    cancel: &CancelToken,
    follow: &Follow<'_>,
) -> Result<Vec<u32>> {
    // Each image on the first page showing it: one drawn right there is
    // drawn right wherever it is.
    let mut on: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for image in changed {
        if let Some(&page) = image.pages.iter().min() {
            on.entry(page).or_default().push(image.object);
        }
    }
    if on.is_empty() {
        return Ok(Vec::new());
    }
    let lifetime = cancel.child();
    let (Some(before), Some(after)) = (
        session(app, original, &lifetime)?,
        session(app, copy, &lifetime)?,
    ) else {
        tracing::warn!("no reader to check a smaller copy with: kept unchecked");
        return Ok(Vec::new());
    };
    let mut keep = BTreeSet::new();
    #[allow(clippy::cast_precision_loss)]
    let count = on.len() as f64;
    for (looked, (&page, objects)) in on.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        follow.went(looked as f64 / count);
        anyhow::ensure!(!cancel.is_cancelled(), "Making the copy was cancelled");
        // A page the file's reader cannot draw is not compared; one the
        // copy's cannot is wrong.
        let Ok(was) = drawn(&*before, page, cancel) else {
            continue;
        };
        let differs = match drawn(&*after, page, cancel) {
            Ok(is) => looks_different(&was, &is),
            Err(_) => true,
        };
        if differs {
            keep.extend(objects.iter().copied());
        }
    }
    lifetime.cancel();
    // An image put back on one page is put back everywhere: it is one
    // object. Those shown first on a page that looked the same may share
    // it with one that did not; those are caught where they were looked at.
    Ok(keep.into_iter().collect())
}

/// A reader's session on the PDF at `path`, as the settings choose it.
fn session(
    app: &App,
    path: &Path,
    lifetime: &CancelToken,
) -> Result<Option<std::sync::Arc<dyn DocumentSession>>> {
    let request = PreviewRequest {
        entry: EntryRef::from_path(path)
            .with_context(|| format!("{} is not there", path.display()))?,
        mime: Some("application/pdf".to_owned()),
        page: 1,
        seconds: None,
        max_side: SIDE,
        text_size: None,
        fit: None,
        held: HeldSlot::keeping(skwir_core::preview::WHOLE_FULL),
    };
    app.previews.open_document(&request, lifetime)
}

/// A page drawn small, in gray: its width, height and levels.
struct Gray {
    width: u32,
    height: u32,
    levels: Vec<u8>,
}

/// Page `page` (from 0) of `session`, drawn at [`SIDE`].
fn drawn(session: &dyn DocumentSession, page: u32, cancel: &CancelToken) -> Result<Gray> {
    let drawn = session.render(page + 1, SIDE, cancel)?;
    let (width, height) = (drawn.width, drawn.height);
    let rgba = drawn.into_rgba();
    let levels = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let y = (299 * u32::from(p[0]) + 587 * u32::from(p[1]) + 114 * u32::from(p[2])) / 1000;
            // Over white, as the page is.
            let alpha = u32::from(p[3]);
            let over = (y * alpha + 255 * (255 - alpha)) / 255;
            u8::try_from(over).unwrap_or(u8::MAX)
        })
        .collect();
    Ok(Gray {
        width,
        height,
        levels,
    })
}

/// Whether two drawings of a page differ by more than detail lost: their
/// sizes, their average difference, or too many pixels far apart.
fn looks_different(was: &Gray, is: &Gray) -> bool {
    if (was.width, was.height) != (is.width, is.height) || was.levels.len() != is.levels.len() {
        return true;
    }
    if was.levels.is_empty() {
        return false;
    }
    let mut sum = 0u64;
    let mut far = 0u64;
    for (&a, &b) in was.levels.iter().zip(&is.levels) {
        let d = a.abs_diff(b);
        sum += u64::from(d);
        if d > FAR {
            far += 1;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let (mean, share) = {
        let count = was.levels.len() as f64;
        (sum as f64 / count, far as f64 / count)
    };
    mean > MEAN || share > FAR_SHARE
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::shrink::{Kind, Quality};

    fn job() -> Job {
        Job {
            kind: Kind::Pdf,
            input: PathBuf::new(),
            output: PathBuf::new(),
            quality: Quality::Screen,
            gray: false,
            strip: false,
            at_most: None,
            keep: Vec::new(),
            rule: None,
        }
    }

    #[test]
    fn a_larger_file_is_given_longer() {
        let small = timeout(1024, &job());
        let large = timeout(400 * 1024 * 1024, &job());
        assert_eq!(small, Duration::from_secs(60));
        assert!(large > small * 50);
        let sized = Job {
            at_most: Some(1),
            ..job()
        };
        assert_eq!(timeout(1024, &sized), small * 4);
    }

    fn page(levels: Vec<u8>) -> Gray {
        Gray {
            width: 10,
            height: u32::try_from(levels.len() / 10).unwrap(),
            levels,
        }
    }

    #[test]
    fn detail_lost_passes_and_an_image_gone_does_not() {
        let photo: Vec<u8> = (0..1000).map(|n| u8::try_from(n % 251).unwrap()).collect();
        let softened: Vec<u8> = photo.iter().map(|&v| v.saturating_add(2)).collect();
        assert!(!looks_different(&page(photo.clone()), &page(softened)));
        let gone = vec![255u8; 1000];
        assert!(looks_different(&page(photo.clone()), &page(gone)));
        let inverted: Vec<u8> = photo.iter().map(|&v| 255 - v).collect();
        assert!(looks_different(&page(photo.clone()), &page(inverted)));
        let mut torn = photo.clone();
        torn[..100].fill(0);
        assert!(looks_different(&page(photo), &page(torn)));
    }
}
