//! The print helper (ADR-111): makes the PDF of what a preview shows,
//! from a job skwir wrote (`skwir_core::print::Job`), for the system's
//! print dialog or to be saved. Text is laid out by parley (shaping,
//! bidirectional text, line breaking) in the faces the job names, and
//! written by krilla as glyphs of embedded subset fonts with their text,
//! so it is selected, copied and searched as it reads; the PDF is tagged
//! for a screen reader (headings, paragraphs, lists, tables, figures with
//! their alternative text, code), with an outline, a title, an author
//! and a language. Pages are laid out first, so each knows how many there
//! are, then drawn one at a time; only those asked are written.
//!
//! `skwir-print-worker <job.json>`: the PDF at the job's output, then a
//! line of JSON on standard output (`Made`), exit 0; a one-line message on
//! standard error and exit 1 when it cannot. It opens only what the job
//! names: its pictures and drawing, its font files, and font files in its
//! fallback folders. `--probe` answers whether it runs, `--build` which
//! build of skwir it is.

mod code;
mod flow;
mod fonts;
mod geometry;
mod out;
mod pictures;
mod table;
mod text;

use std::collections::{BTreeSet, HashMap};
use std::io::Read as _;
use std::path::Path;

use anyhow::{Context, Result, bail};
use skwir_core::print::{BlockKind, Job, Made, Part};

use crate::fonts::Fonts;
use crate::geometry::Frame;
use crate::out::Heading;

/// The largest job read. A job holds what a preview holds: a code file
/// of a million colored lines is some 470 MB of it, a folder's listing of
/// a hundred thousand rows some tens of megabytes. One past this is
/// refused before it is read; one under it is read as it is parsed, never
/// held whole beside what it becomes.
pub const JOB_LIMIT: u64 = 512 * 1024 * 1024;

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // `--build`, skwir asking which build of it this helper is, when a
    // call of it failed (`platform::process::explain`).
    if args.first().is_some_and(|arg| arg == "--build") {
        println!("{}", env!("SKWIR_BUILD"));
        return;
    }
    // `--probe`, skwir's settings asking whether this helper works here:
    // being started at all says it does, as it links nothing of the
    // system's but its C library.
    if args.first().is_some_and(|arg| arg == "--probe") {
        return;
    }
    let [job] = args.as_slice() else {
        eprintln!("usage: skwir-print-worker <job.json>");
        std::process::exit(2);
    };
    // A panic says so on one line and ends the helper as an error does:
    // skwir shows the first line of what it says.
    std::panic::set_hook(Box::new(|info| {
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|what| (*what).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info
            .location()
            .map(|at| format!(" ({}:{})", at.file(), at.line()))
            .unwrap_or_default();
        let message = format!("the PDF could not be made: {what}{at}").replace('\n', " ");
        eprintln!("{message}");
        std::process::exit(1);
    }));
    match run(Path::new(job)) {
        Ok(made) => match serde_json::to_string(&made) {
            Ok(line) => println!("{line}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            let message = format!("{error:#}").replace('\n', " ");
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}

fn run(path: &Path) -> Result<Made> {
    let job = read_job(path)?;
    make(&job)
}

/// The job at `path`, refused past [`JOB_LIMIT`].
fn read_job(path: &Path) -> Result<Job> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("the job {} could not be opened", path.display()))?;
    let size = file.metadata()?.len();
    if size > JOB_LIMIT {
        bail!("the job is {size} bytes, more than the {JOB_LIMIT} a job may be");
    }
    let reader = std::io::BufReader::with_capacity(1 << 20, file.take(JOB_LIMIT));
    serde_json::from_reader(reader).context("the job could not be read")
}

/// The PDF of `job`, written to its output.
pub fn make(job: &Job) -> Result<Made> {
    // A drawing is read first: its text is written in the job's faces, and
    // what of it they lack looked for with the rest.
    let drawings = job
        .parts
        .iter()
        .map(|part| match part {
            Part::Drawing { svg, .. } => pictures::read_drawing(svg).map(Some),
            _ => Ok(None),
        })
        .collect::<Result<Vec<Option<Vec<u8>>>>>()?;
    let chars = characters(job, &drawings);
    let mut fonts = Fonts::load(job, &chars)?;
    let frame = Frame::new(&job.paper, job.header.is_some(), job.numbers);
    let language = job
        .language
        .as_deref()
        .and_then(|language| parley::Language::parse(language).ok());
    let mut anchors: HashMap<String, (usize, f32)> = HashMap::new();
    let mut headings: Vec<Heading> = Vec::new();
    let mut parts: Vec<Box<dyn out::Part + '_>> = Vec::new();
    let mut first = 0;
    for (part, drawing) in job.parts.iter().zip(&drawings) {
        let planned: Box<dyn out::Part> = match part {
            Part::Flow { blocks } => Box::new(flow::Flow::plan(
                blocks,
                &mut fonts,
                frame,
                language,
                first,
                &mut anchors,
                &mut headings,
            )),
            Part::Code {
                lines,
                numbers,
                first: from,
            } => Box::new(code::Code::plan(lines, *numbers, *from, &mut fonts, frame)),
            Part::Table { columns, rows } => Box::new(table::Data::plan(
                columns, rows, &mut fonts, frame, language,
            )),
            Part::Pictures { pictures, sheet } => {
                Box::new(pictures::Pictures::plan(pictures, *sheet, frame))
            }
            Part::Pages { pages } => Box::new(pictures::Pictures::plan(pages, false, frame)),
            Part::Drawing { svg, alt } => Box::new(pictures::Drawing::plan(
                drawing.as_deref().unwrap_or_default(),
                svg,
                alt,
                &fonts,
                frame,
            )?),
            Part::Specimen {
                face,
                name,
                sample,
                sizes,
            } => {
                fonts.load_specimen(face)?;
                Box::new(flow::Flow::specimen(
                    name,
                    sample,
                    sizes,
                    &mut fonts,
                    frame,
                    language,
                    first,
                    &mut headings,
                ))
            }
        };
        first += planned.pages();
        parts.push(planned);
    }
    if parts.is_empty() {
        bail!("the job has nothing to print");
    }
    out::write(job, &mut fonts, frame, parts, anchors, &headings)
}

/// Every character the job writes in the faces it names (not a
/// specimen's sample, written in its own face), its drawings' text
/// (`drawings`, beside its parts) included, so the fonts that hold what
/// those lack are found once, before anything is laid out.
fn characters(job: &Job, drawings: &[Option<Vec<u8>>]) -> BTreeSet<char> {
    let mut seen = vec![0u64; (char::MAX as usize + 1).div_ceil(64)];
    let mut add = |text: &str| {
        for c in text.chars() {
            seen[c as usize / 64] |= 1 << (c as usize % 64);
        }
    };
    add("0123456789 /↪…pt");
    if let Some(header) = &job.header {
        add(header);
    }
    for part in &job.parts {
        match part {
            Part::Flow { blocks } => {
                for block in blocks {
                    add(&block.text);
                    if let Some(marker) = &block.marker {
                        add(marker);
                    }
                    if let BlockKind::Picture(picture) = &block.kind {
                        add(picture.caption.as_deref().unwrap_or_default());
                    }
                }
            }
            Part::Code { lines, .. } => {
                for line in lines {
                    add(&line.text);
                }
            }
            Part::Table { columns, rows } => {
                for column in columns {
                    add(column);
                }
                for row in rows {
                    for cell in row {
                        add(cell);
                    }
                }
            }
            Part::Pictures { pictures, .. } => {
                for picture in pictures {
                    add(picture.caption.as_deref().unwrap_or_default());
                }
            }
            Part::Specimen { name, .. } => add(name),
            Part::Drawing { .. } | Part::Pages { .. } => {}
        }
    }
    for drawing in drawings.iter().flatten() {
        add(&pictures::drawing_text(drawing));
    }
    let mut chars = BTreeSet::new();
    for (word, bits) in seen.iter().enumerate() {
        let mut bits = *bits;
        while bits != 0 {
            let bit = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            if let Some(c) = char::from_u32((word * 64 + bit) as u32) {
                chars.insert(c);
            }
        }
    }
    chars
}
