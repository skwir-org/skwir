//! A smaller copy of a picture or a PDF (ADR-112), made by the print
//! helper's `--shrink` mode (`skwir-print-worker --shrink <job.json>`),
//! which links the engine (`skwir-shrink`): run isolated where the system
//! can, reading the file it shrinks and writing only in its own folder.
//! What it wrote is opened once, a regular file itself and never through
//! a link ([`Copy`]): what is kept is that file, whatever the helper left
//! at its name.
//!
//! A PDF's copy is checked before it is kept ([`made`]): drawn from the
//! file and from the copy by the reader the settings choose (the window's
//! `PDFium`, Poppler or `MuPDF`), small, in gray, its first and last pages
//! and the first page showing each image written anew. Losing detail is
//! what was asked; an image gone, its colours turned, moved or no longer
//! drawn is not: a page that looks different by more than a resized,
//! re-encoded image does has its images put back as they were, the copy
//! made again under the same rule and checked again. A copy with another
//! count of pages, or a page that looks different with no image written
//! anew on it, is not kept: what changed there is no image's.
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

/// The most times a PDF's copy is made: its images put back after a
/// check, then again after the next; a copy still looking different is
/// not kept.
const PASSES: u32 = 3;

/// The share of each pass the helper's work is followed as, its check the
/// rest.
const MAKING: f64 = 0.85;

/// Where the helper is, if it is: the print helper's executable.
pub fn worker_path() -> Option<PathBuf> {
    crate::print_job::worker_path()
}

/// A copy the helper made: its path in the folder it was made in, and the
/// file there, opened as it was checked (a regular file itself, never
/// through a link, so neither a link the helper left nor a pipe), with its
/// size. What is kept is read from `file`.
#[derive(Debug)]
pub struct Copy {
    pub path: PathBuf,
    pub file: std::fs::File,
    pub size: u64,
}

/// A smaller copy of `job.input`, written by the helper in `work`, a folder
/// the caller made and removes, as `name`: `job.output` is set there. What
/// it answered, its copy at `job.output` when `Made::written`. It is
/// stopped when `cancel` is cancelled, when it says nothing of how far it
/// went for [`STALL`], and past [`CEILING`]. Waits for the helper: a
/// worker's work.
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
/// made again keeping it, at most [`PASSES`] times. The copy, `None` when
/// it would not be smaller or was not kept (the notes say why), and what
/// the helper said of it. `follow` hears how far each pass went, the
/// helper's work then the pages checked; `again` is called before each
/// pass after the first, which goes through the whole file again (a
/// plan's step announces that work). Waits: a worker's work.
pub fn made(
    app: &App,
    mut job: Job,
    work: &Path,
    name: &str,
    cancel: &CancelToken,
    follow: &Follow<'_>,
    again: &dyn Fn(),
) -> Result<(Option<Copy>, Made)> {
    let size = std::fs::metadata(&job.input)
        .with_context(|| format!("{} could not be read", job.input.display()))?
        .len();
    if job.kind != Kind::Pdf {
        let made = run(&mut job, work, name, cancel, follow)?;
        follow.went(1.0);
        return kept(&job, made, size);
    }
    let mut made = run(&mut job, work, name, cancel, &follow.part(0.0, MAKING))?;
    let mut pass = 1;
    while made.written {
        let looked = looked(
            app,
            &job.input,
            &job.output,
            &made.changed,
            cancel,
            &follow.part(MAKING, 1.0),
        )?;
        let keep = match looked {
            Looked::Alike => break,
            Looked::Images(keep) if pass < PASSES => keep,
            Looked::Images(_) => {
                tracing::info!("a smaller copy looked different with its images put back");
                return Ok((None, not_kept(size, STILL_UNLIKE)));
            }
            Looked::Unlike => {
                tracing::info!("a smaller copy looked different beyond its images");
                return Ok((None, not_kept(size, UNLIKE)));
            }
        };
        tracing::info!(
            images = keep.len(),
            "images put back after their pages looked different"
        );
        job.keep.extend(keep);
        // Made again as the first was, but for what is put back.
        job.rule = made.rule;
        again();
        made = run(&mut job, work, name, cancel, &follow.part(0.0, MAKING))?;
        pass += 1;
        if !made.written {
            return Ok((None, not_kept(size, PUT_BACK)));
        }
    }
    follow.went(1.0);
    kept(&job, made, size)
}

/// Why a PDF's copy was not kept, said after "No smaller copy was made".
const UNLIKE: &str = "a copy made looked different from it beyond its images, so none was kept";
const STILL_UNLIKE: &str =
    "a copy made still looked different from it with its images put back, so none was kept";
const PUT_BACK: &str = "its images made smaller looked different in a copy and were put back as they were, which left it no smaller";

/// Nothing kept of a file of `size` bytes, `why`.
fn not_kept(size: u64, why: &str) -> Made {
    Made {
        before: size,
        after: size,
        written: false,
        notes: vec![why.to_owned()],
        ..Made::default()
    }
}

/// The copy `made` says the helper wrote at `job.output`, opened.
fn kept(job: &Job, made: Made, size: u64) -> Result<(Option<Copy>, Made)> {
    if !made.written {
        return Ok((None, made));
    }
    let (file, written) = opened(&job.output, size)?;
    let copy = Copy {
        path: job.output.clone(),
        file,
        size: written,
    };
    Ok((Some(copy), made))
}

/// The copy the helper wrote at `path`, opened as what it is (a regular
/// file itself, never through a link at its end), and its size, which is
/// more than nothing and less than the file's `size`.
fn opened(path: &Path, size: u64) -> Result<(std::fs::File, u64)> {
    let file = skwir_core::fs::file::open_regular_unfollowed(path)
        .context("the shrink helper wrote no copy")?;
    let written = file
        .metadata()
        .context("the shrink helper's copy could not be read")?
        .len();
    ensure!(written > 0, "the shrink helper wrote an empty copy");
    ensure!(
        written < size,
        "the shrink helper wrote a copy no smaller than the file"
    );
    Ok((file, written))
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
    let finished =
        crate::process::run_helper_stalling(&mut run.command, cancel, STALL, CEILING, follow);
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
        // Looked at as it will be kept, then let go of: a file held open
        // could not be replaced by the next pass on Windows.
        opened(&output, size)?;
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

/// How long the helper may say nothing of how far it went before it is
/// stopped: it tells each image, page and stream it goes through, the
/// longest of them one image of the most samples it reads (512 MiB),
/// which a machine slower than the one it was measured on still writes
/// in a minute or two. How long a whole file takes is not told ahead: it
/// is the machine's, and what the file holds (73 s for 10 MB of pages
/// drawn, without loss, on the reference machine).
const STALL: Duration = Duration::from_mins(3);

/// The most it may take in all, saying how far it goes: what no file
/// takes, for a helper that would go on saying so and never end.
const CEILING: Duration = Duration::from_hours(8);

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

/// The side of the squares of a page compared by their average, in pixels
/// of a page drawn at [`SIDE`] (some 4 mm on A4), and the most two such
/// averages may differ. Detail lost keeps each square's average, as
/// resizing and a JPEG's blocks keep it, but for an edge moved by a
/// fraction of a pixel (some 16 levels); an image gone, turned or moved
/// does not, however small it is beside the page.
const SQUARE: usize = 8;
const SQUARE_MEAN: u32 = 40;

/// What the copy's pages looked like beside the file's.
#[derive(Debug, PartialEq, Eq)]
enum Looked {
    /// As the file's, or no reader could tell.
    Alike,
    /// Different where these images written anew (by object number) are:
    /// those to put back.
    Images(Vec<u32>),
    /// Different where no image was written anew, or with another count
    /// of pages: the copy is not kept.
    Unlike,
}

/// How the pages of `copy` look beside those of `original`: its first
/// and last pages, and the first page showing each image of `changed`.
/// Alike when no reader can draw them (none is on): the copy is then kept
/// unchecked, and the log says so. A page of the copy that cannot be drawn
/// looks different.
fn looked(
    app: &App,
    original: &Path,
    copy: &Path,
    changed: &[Changed],
    cancel: &CancelToken,
    follow: &Follow<'_>,
) -> Result<Looked> {
    let lifetime = cancel.child();
    let (Some(before), Some(after)) = (
        session(app, original, &lifetime)?,
        session(app, copy, &lifetime)?,
    ) else {
        tracing::warn!("no reader to check a smaller copy with: kept unchecked");
        return Ok(Looked::Alike);
    };
    let pages = before.info().pages;
    if after.info().pages != pages {
        lifetime.cancel();
        return Ok(Looked::Unlike);
    }
    // The images each page shows; each image looked at on the first page
    // showing it (one drawn right there is drawn right wherever it is),
    // and the first and last pages whatever they show.
    let mut on: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut drawn_pages: BTreeSet<u32> = [0, pages.saturating_sub(1)].into();
    for image in changed {
        for &page in &image.pages {
            on.entry(page).or_default().push(image.object);
        }
        if let Some(&first) = image.pages.iter().min() {
            drawn_pages.insert(first);
        }
    }
    let mut keep = BTreeSet::new();
    let mut unlike = false;
    #[allow(clippy::cast_precision_loss)]
    let count = drawn_pages.len() as f64;
    for (looked, &page) in drawn_pages.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        follow.went(looked as f64 / count);
        anyhow::ensure!(!cancel.is_cancelled(), "Making the copy was cancelled");
        if page >= pages {
            continue;
        }
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
            match on.get(&page) {
                Some(objects) => keep.extend(objects.iter().copied()),
                None => unlike = true,
            }
        }
    }
    lifetime.cancel();
    // An image put back on one page is put back everywhere: it is one
    // object. Those shown first on a page that looked the same may share
    // it with one that did not; those are caught where they were looked at.
    Ok(if unlike {
        Looked::Unlike
    } else if keep.is_empty() {
        Looked::Alike
    } else {
        Looked::Images(keep.into_iter().collect())
    })
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
/// sizes, their average difference, too many pixels far apart, or a
/// square of them whose average moved ([`SQUARE_MEAN`]).
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
    mean > MEAN || share > FAR_SHARE || a_square_moved(was, is)
}

/// Whether a square of [`SQUARE`] pixels of `was` and `is` (the same size)
/// differs in its average by more than [`SQUARE_MEAN`]; those at the
/// right and bottom edges as large as the page leaves them.
fn a_square_moved(was: &Gray, is: &Gray) -> bool {
    let width = was.width as usize;
    if width == 0 {
        return false;
    }
    let across = width.div_ceil(SQUARE);
    let mut sums = vec![(0u32, 0u32, 0u32); across];
    for (row, (a, b)) in was
        .levels
        .chunks_exact(width)
        .zip(is.levels.chunks_exact(width))
        .enumerate()
    {
        for (x, (&a, &b)) in a.iter().zip(b).enumerate() {
            let square = &mut sums[x / SQUARE];
            square.0 += u32::from(a);
            square.1 += u32::from(b);
            square.2 += 1;
        }
        let last = (row + 1) % SQUARE == 0 || (row + 1) * width == was.levels.len();
        if last {
            for square in &mut sums {
                let (a, b, n) = *square;
                if n > 0 && a.abs_diff(b) > SQUARE_MEAN * n {
                    return true;
                }
                *square = (0, 0, 0);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A page of 240 × 320 pixels: a photograph's gradients and grain, and
    /// lines of text, black on white.
    fn drawn_page() -> Gray {
        let (width, height) = (240u32, 320u32);
        let mut seed = 0x2545_f491_u32;
        let levels = (0..width * height)
            .map(|n| {
                let (x, y) = (n % width, n / width);
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                if y >= 200 && (y / 3) % 4 == 0 && (x / 5) % 3 != 0 {
                    0
                } else if y >= 200 {
                    255
                } else {
                    u8::try_from((x + y) * 200 / (width + height) + seed % 40).unwrap()
                }
            })
            .collect();
        Gray {
            width,
            height,
            levels,
        }
    }

    /// `page` softened as a resized image draws: each pixel a quarter of
    /// the way to the average of the 3 × 3 around it.
    fn softened(page: &Gray) -> Gray {
        let (width, height) = (page.width as usize, page.height as usize);
        let mut levels = page.levels.clone();
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let mut sum = 0u32;
                for (dx, dy) in (0..3).flat_map(|dx| (0..3).map(move |dy| (dx, dy))) {
                    sum += u32::from(page.levels[(y + dy - 1) * width + x + dx - 1]);
                }
                let own = u32::from(page.levels[y * width + x]);
                levels[y * width + x] = u8::try_from((own * 27 + sum) / 36).unwrap();
            }
        }
        Gray { levels, ..*page }
    }

    /// An image a hundredth of its page, turned, shows; the whole page
    /// softened does not.
    #[test]
    fn a_small_image_turned_shows_detail_lost_does_not() {
        let page = drawn_page();
        assert!(!looks_different(&page, &softened(&page)));
        let mut turned = page.levels.clone();
        for y in 40..64 {
            for x in 100..124 {
                turned[y * 240 + x] = 255 - turned[y * 240 + x];
            }
        }
        let turned = Gray {
            levels: turned,
            ..page
        };
        assert!(looks_different(&drawn_page(), &turned));
        let mut gone = drawn_page();
        for y in 220..236 {
            gone.levels[y * 240 + 30..y * 240 + 60].fill(255);
        }
        assert!(looks_different(&drawn_page(), &gone));
    }
}
