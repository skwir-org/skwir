//! What a preview shows, made into a job for the print helper
//! (`skwir_core::print::Job`, ADR-111), and the helper run on it: the
//! faces of the window's reading and code fonts as files, the preview's
//! model turned into the parts a PDF is made of, code colored in the light
//! theme whatever the window's look, and `skwir-print-worker` run isolated
//! where the system can, reading only the job's folder, its faces and the
//! folders fallback faces are found in. All of it is a worker's work: it
//! reads fonts, files and pictures, and waits for the helper.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use parking_lot::RwLock;
use resvg::usvg::fontdb;
use skwir_core::CancelToken;
use skwir_core::inspection::{self, Inspection, InspectionKind};
use skwir_core::print::{self, Face, Family, Fonts, Job, Line, Made, Part};

use crate::highlight::{CodeTheme, DEFAULT, Highlighter, LIGHT_KEY, Palette};
use crate::sandbox::Otherwise;
use crate::text_rows;

/// The helper. Plugins that require it name this, not the literals.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "SKWIR_PRINT_WORKER",
    executable: "skwir-print-worker",
};

/// Where the helper is, if it is.
pub fn worker_path() -> Option<PathBuf> {
    WORKER.find()
}

/// The job written in the working folder, and the PDF the helper writes
/// there.
const JOB: &str = "job.json";
const OUTPUT: &str = "print.pdf";

/// The most text of a code file read to print, beside its lines
/// (`print::MAX_CODE_LINES`): a file of a few long lines is bounded too,
/// and the job stays well under what the helper reads.
const CODE_TEXT: usize = 128 * 1024 * 1024;

/// The families the window writes in, by name, as its theme has them
/// (`theme.reading_font`, `theme.code_font`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roles {
    pub text: String,
    pub code: String,
}

/// The faces of `roles`' families as files, regular, bold, italic and bold
/// italic where the family has each (a face it lacks is made up by the
/// helper), from the system's fonts (`inspection::system_fonts`, read once
/// a process); a family the system lacks is its generic serif face for the
/// text, monospaced for code, else any face. And the folders the system's
/// faces are in, the outermost of each, where the helper finds what those
/// faces lack. Reads fontconfig's aliases (`fonts::unalias`): a worker's.
pub fn faces(roles: &Roles) -> (Fonts, Vec<PathBuf>) {
    let db = crate::inspection::system_fonts();
    // A family skwir carries that the system lacks (Skwir's own, not
    // installed) is the one its package carries, as the window writes in
    // it (ADR-118): its files, which the helper may read as it reads every
    // face it is given.
    let carried = carried_lacked(
        db,
        &crate::fonts::CARRIED,
        roles,
        crate::fonts::bundled_files,
    );
    let of = |name: &str, generic| match carried.as_ref() {
        Some(carried) if crate::fonts::CARRIED.contains(&name) && has(carried, name) => {
            family(carried, name, generic)
        }
        _ => family(db, name, generic),
    };
    let fonts = Fonts {
        text: of(&roles.text, fontdb::Family::Serif),
        code: of(&roles.code, fontdb::Family::Monospace),
    };
    (fonts, folders(db))
}

/// The faces of the families of `carried` that `roles` name and `db` lacks,
/// from the files that carry them (`files`: `fonts::bundled_files`, those
/// skwir's package carries): none when the system has each, or nothing
/// carries them.
fn carried_lacked(
    db: &fontdb::Database,
    carried: &[&str],
    roles: &Roles,
    files: impl FnOnce(&[&str]) -> Vec<PathBuf>,
) -> Option<fontdb::Database> {
    let lacked: Vec<&str> = carried
        .iter()
        .copied()
        .filter(|family| (roles.text == *family || roles.code == *family) && !has(db, family))
        .collect();
    if lacked.is_empty() {
        return None;
    }
    let mut faces = fontdb::Database::new();
    for path in files(&lacked) {
        if let Err(error) = faces.load_font_file(&path) {
            tracing::warn!(path = %path.display(), %error, "a font skwir carries is unread");
        }
    }
    (!faces.is_empty()).then_some(faces)
}

/// Whether `db` has a face of the family `name`.
fn has(db: &fontdb::Database, name: &str) -> bool {
    db.faces()
        .any(|face| face.families.iter().any(|(family, _)| family == name))
}

/// A face's file, for one the system read from a file.
fn face_of(info: &fontdb::FaceInfo) -> Option<Face> {
    match &info.source {
        fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => Some(Face {
            path: path.clone(),
            index: info.index,
        }),
        fontdb::Source::Binary(_) => None,
    }
}

/// The face of `families` with this weight and slant, only if it is one:
/// the nearest the database would give (a regular for a bold) is left out,
/// for the helper to embolden or slant the regular face.
fn exact(
    db: &fontdb::Database,
    families: &[fontdb::Family],
    bold: bool,
    italic: bool,
) -> Option<Face> {
    let id = db.query(&fontdb::Query {
        families,
        weight: if bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        },
        stretch: fontdb::Stretch::Normal,
        style: if italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        },
    })?;
    let info = db.face(id)?;
    let is_bold = info.weight.0 >= 600;
    let is_italic = info.style != fontdb::Style::Normal;
    if is_bold == bold && is_italic == italic {
        face_of(info)
    } else {
        None
    }
}

/// The faces of the family `name`, else of `generic`, else any regular
/// face of the system's.
fn family(db: &fontdb::Database, name: &str, generic: fontdb::Family) -> Family {
    let name = crate::fonts::unalias(name);
    let own = match name.trim().to_ascii_lowercase().as_str() {
        "serif" => fontdb::Family::Serif,
        "sans" | "sans-serif" | "system-ui" => fontdb::Family::SansSerif,
        "monospace" | "mono" => fontdb::Family::Monospace,
        _ => fontdb::Family::Name(&name),
    };
    for families in [[own], [generic]] {
        if let Some(regular) = exact(db, &families, false, false) {
            return Family {
                regular: Some(regular),
                bold: exact(db, &families, true, false),
                italic: exact(db, &families, false, true),
                bold_italic: exact(db, &families, true, true),
            };
        }
    }
    Family {
        regular: db
            .faces()
            .filter(|info| info.style == fontdb::Style::Normal && info.weight.0 < 600)
            .find_map(face_of),
        ..Family::default()
    }
}

/// The folders the system's faces are in, the outermost only: the helper
/// looks below each.
fn folders(db: &fontdb::Database) -> Vec<PathBuf> {
    let all: BTreeSet<PathBuf> = db
        .faces()
        .filter_map(face_of)
        .filter_map(|face| face.path.parent().map(Path::to_path_buf))
        .collect();
    // Sorted by component, a folder comes right before those below it.
    let mut kept: Vec<PathBuf> = Vec::new();
    for folder in all {
        if !kept.last().is_some_and(|outer| folder.starts_with(outer)) {
            kept.push(folder);
        }
    }
    kept
}

/// The colors code is printed in: the light theme the settings choose
/// (`reading.code_light`), whatever the window's look and
/// `reading.code_appearance`, since paper is light. It may read the
/// user's themes: a worker's work.
pub fn light_palette(app: &skwir_core::App) -> Palette {
    let name = app
        .settings
        .get::<String>(LIGHT_KEY)
        .map_or_else(|| DEFAULT.into(), Into::into);
    Palette::of(&CodeTheme {
        name,
        ..CodeTheme::own(false)
    })
}

/// What a preview prints as, and what the caller says of it: that a code
/// file was cut at `print::MAX_CODE_LINES` lines, or at the text read of
/// it.
#[derive(Debug, Default)]
pub struct Parts {
    pub parts: Vec<Part>,
    pub notice: Option<String>,
}

/// The parts a preview's model prints as:
///
/// - Code: its lines, colored with `palette`, numbered from 1. A file
///   read where it is shown (`Inspection::text`) is read again, all of it,
///   up to `print::MAX_CODE_LINES` lines and `CODE_TEXT` bytes (a notice
///   says when it is cut); a text kind read so prints as code too.
/// - Markdown, a book's chapter, a message, text: a flow of its blocks, a
///   code block colored by its language; a picture is written by
///   `pictures` (which answers the file it wrote in the job's folder, or
///   `None`, which prints its description instead).
/// - Table: its columns and every row's cells.
/// - The inspection's own picture (a model's, a page's) after them.
///
/// Not printed here: a font (a specimen needs the face's file, which the
/// caller has and the inspection does not: the caller makes that part),
/// and what has no page of its own: hex, an archive's members, a map, a
/// model without a picture, a comic (whose pages the caller prints as
/// pictures). Cancelled, it stops with an error.
pub fn parts(
    inspection: &Inspection,
    palette: &Palette,
    pictures: &mut dyn FnMut(&inspection::Picture) -> Option<PathBuf>,
    cancel: &CancelToken,
) -> Result<Parts> {
    let mut out = Parts::default();
    // A text read where it is shown is a code view's, whatever its kind.
    let as_code = inspection.kind == InspectionKind::Code
        || (inspection.kind == InspectionKind::Text && inspection.text.is_some());
    match inspection.kind {
        _ if as_code => {
            let (part, notice) = code(inspection, palette, cancel)?;
            out.parts.push(part);
            out.notice = notice;
        }
        InspectionKind::Table if !inspection.rows.is_empty() || !inspection.columns.is_empty() => {
            out.parts.push(Part::Table {
                columns: inspection.columns.clone(),
                rows: inspection
                    .rows
                    .iter()
                    .map(|row| row.cells.clone())
                    .collect(),
            });
        }
        // A history prints as the table of its commits, without its graph
        // and the names at them.
        InspectionKind::History => {
            out.parts.push(Part::Table {
                columns: inspection.columns.iter().take(4).cloned().collect(),
                rows: inspection
                    .rows
                    .iter()
                    .map(|row| row.cells.iter().take(4).cloned().collect())
                    .collect(),
            });
        }
        // A table of rows a document holds is a flow like the others (a
        // code view was taken above).
        InspectionKind::Markdown
        | InspectionKind::Book
        | InspectionKind::Mail
        | InspectionKind::Text
        | InspectionKind::Code
        | InspectionKind::Table => {
            let blocks = flow(&inspection.blocks, palette, pictures, cancel)?;
            if !blocks.is_empty() {
                out.parts.push(Part::Flow { blocks });
            }
        }
        InspectionKind::Model => {}
        InspectionKind::Hex
        | InspectionKind::Archive
        | InspectionKind::Map
        | InspectionKind::Comic
        | InspectionKind::Font => return Ok(out),
    }
    if let Some(image) = &inspection.image {
        let shown = inspection::Picture::Ready(image.clone());
        if let Some(path) = pictures(&shown) {
            out.parts.push(Part::Pictures {
                pictures: vec![print::Picture {
                    path,
                    width: image.width,
                    height: image.height,
                    alt: inspection.title.clone(),
                    caption: None,
                    orientation: 0,
                }],
                sheet: false,
            });
        }
    }
    Ok(out)
}

/// A run of the preview's, as printed.
fn run(run: &inspection::Run) -> print::Run {
    print::Run {
        start: u32::try_from(run.range.start).unwrap_or(u32::MAX),
        end: u32::try_from(run.range.end).unwrap_or(u32::MAX),
        color: run.color,
        bold: run.bold,
        italic: run.italic,
        underline: run.underline,
        strike: run.strikethrough,
        code: run.code,
        script: match run.script {
            inspection::Script::Baseline => 0,
            inspection::Script::Superscript => 1,
            inspection::Script::Subscript => -1,
        },
        small_caps: run.small_caps,
        // A link found in the text is printed as one too: paper has no
        // Ctrl to hold.
        link: run.link.clone(),
    }
}

/// The lines of a code view, colored, as a part, and what to say if it
/// was cut.
fn code(
    inspection: &Inspection,
    palette: &Palette,
    cancel: &CancelToken,
) -> Result<(Part, Option<String>)> {
    let language = inspection
        .text
        .as_ref()
        .and_then(|file| file.language.clone())
        .or_else(|| inspection.blocks.first().and_then(|b| b.language.clone()));
    let (texts, cut) = if let Some(file) = &inspection.text {
        read_lines(file, print::MAX_CODE_LINES, CODE_TEXT, cancel)?
    } else {
        let mut texts: Vec<String> = inspection
            .blocks
            .iter()
            .flat_map(|block| block.text.split('\n').map(str::to_owned))
            .collect();
        let cut = texts.len() > print::MAX_CODE_LINES;
        texts.truncate(print::MAX_CODE_LINES);
        (texts, cut)
    };
    let mut highlighter = Highlighter::new(language.as_deref(), palette);
    let mut lines = Vec::with_capacity(texts.len());
    for (at, text) in texts.into_iter().enumerate() {
        if at % 1024 == 0 {
            crate::inspection::check(cancel)?;
        }
        let runs = highlighter.line(&text).iter().map(run).collect();
        lines.push(Line { text, runs });
    }
    let notice = cut.then(|| {
        format!(
            "Only the first {} lines are printed",
            separated(lines.len())
        )
    });
    Ok((
        Part::Code {
            lines,
            numbers: true,
            first: 1,
        },
        notice,
    ))
}

/// `n` with its thousands apart: 1,000,000.
fn separated(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The lines of a file read where it is shown, all of them up to `most`
/// lines and `bytes` of text, as the view reads it (its encoding, rows of
/// one line joined); whether it was cut.
fn read_lines(
    file: &inspection::TextFile,
    most: usize,
    bytes: usize,
    cancel: &CancelToken,
) -> Result<(Vec<String>, bool)> {
    let form = text_rows::Form::read(file, cancel)?;
    let index = RwLock::new(text_rows::Index::new(form));
    // The first row's place, which reading from the start needs.
    text_rows::count_until(file, &index, Some(1), cancel, || {})?;
    let (text, stopped) =
        text_rows::text(file, &index, (0, 0), (u64::MAX, usize::MAX), bytes, cancel)?;
    if text.is_empty() {
        return Ok((Vec::new(), stopped));
    }
    let mut lines: Vec<String> = text
        .split('\n')
        .take(most.saturating_add(1))
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect();
    let cut = stopped || lines.len() > most;
    lines.truncate(most);
    Ok((lines, cut))
}

/// A preview's blocks as printed blocks.
fn flow(
    blocks: &[inspection::Block],
    palette: &Palette,
    pictures: &mut dyn FnMut(&inspection::Picture) -> Option<PathBuf>,
    cancel: &CancelToken,
) -> Result<Vec<print::Block>> {
    let mut out = Vec::with_capacity(blocks.len());
    for (at, block) in blocks.iter().enumerate() {
        if at % 256 == 0 {
            crate::inspection::check(cancel)?;
        }
        let mut printed = print::Block {
            text: block.text.clone(),
            runs: block.runs.iter().map(run).collect(),
            heading: block.heading.min(6),
            quote: block.quote,
            list_depth: u8::try_from(block.list_depth).unwrap_or(u8::MAX),
            marker: block.marker.clone(),
            align: match block.align {
                inspection::Align::Start => print::Align::Start,
                inspection::Align::Center => print::Align::Center,
                inspection::Align::End => print::Align::End,
            },
            indent: block.indent,
            size: block.size.unwrap_or(0.0),
            note: block.note,
            anchors: block.anchors.clone(),
            ..print::Block::default()
        };
        match &block.kind {
            inspection::BlockKind::Rule => {
                printed.kind = print::BlockKind::Rule;
                out.push(printed);
            }
            inspection::BlockKind::TableRow { cells, header } => {
                printed.kind = print::BlockKind::Row {
                    cells: cells
                        .iter()
                        .map(|cell| {
                            (
                                u32::try_from(cell.start).unwrap_or(u32::MAX),
                                u32::try_from(cell.end).unwrap_or(u32::MAX),
                            )
                        })
                        .collect(),
                    header: *header,
                };
                out.push(printed);
            }
            inspection::BlockKind::Text => {
                if let Some(picture) = &block.picture {
                    let (width, height) = picture.size();
                    match pictures(picture) {
                        Some(path) => out.push(print::Block {
                            kind: print::BlockKind::Picture(print::Picture {
                                path,
                                width,
                                height,
                                alt: block.alt.clone(),
                                caption: None,
                                orientation: 0,
                            }),
                            text: String::new(),
                            runs: Vec::new(),
                            ..printed.clone()
                        }),
                        // Its description in its place, as a browser has
                        // it when a picture cannot be shown.
                        None if !block.alt.is_empty() => out.push(print::Block {
                            text: block.alt.clone(),
                            runs: Vec::new(),
                            ..printed.clone()
                        }),
                        None => {}
                    }
                    if block.text.trim().is_empty() {
                        continue;
                    }
                    printed.anchors.clear();
                }
                if block.code {
                    printed.kind = print::BlockKind::Code;
                    // A fenced block is colored by its language, as the
                    // view colors it, unless its runs are colored already.
                    if let Some(language) = &block.language
                        && block.runs.iter().all(|r| r.color.is_none())
                    {
                        let mut highlighter = Highlighter::new(Some(language), palette);
                        printed.runs = highlighter.text(&block.text).iter().map(run).collect();
                    }
                }
                out.push(printed);
            }
        }
    }
    Ok(out)
}

/// The PDF of `job`, made by the helper in `work`, a folder the caller made
/// and removes (it holds the pictures the job names): `job.output` is set
/// there, the job written there, and the helper run isolated where the
/// system can, reading `work`, the job's faces and fallback folders and
/// any picture it names outside `work`, and writing only in `work`. It is
/// stopped when `cancel` is cancelled, and when it takes far longer than
/// the job should (`timeout`). A missing or empty PDF is refused. Waits
/// for the helper: a worker's work.
pub fn make(
    job: &mut Job,
    work: &Path,
    cancel: &CancelToken,
    follow: &crate::process::Follow<'_>,
) -> Result<Made> {
    let program = WORKER.require("print")?;
    made(&program, job, work, cancel, follow)
        .map_err(|error| crate::process::explain(&program, cancel, error))
}

/// [`make`], with the helper at `program`; `follow` hears how far it went
/// (its pages laid out, then each drawn).
fn made(
    program: &Path,
    job: &mut Job,
    work: &Path,
    cancel: &CancelToken,
    follow: &crate::process::Follow<'_>,
) -> Result<Made> {
    let output = work.join(OUTPUT);
    let _ = std::fs::remove_file(&output);
    // What it reads besides its folder, where it runs isolated: the faces,
    // the fallback folders, a picture outside its folder.
    let mut readable: Vec<PathBuf> = [&job.fonts.text, &job.fonts.code]
        .into_iter()
        .flat_map(|family| {
            [
                &family.regular,
                &family.bold,
                &family.italic,
                &family.bold_italic,
            ]
        })
        .flatten()
        .map(|face| face.path.clone())
        .collect();
    readable.extend(job.fallback.iter().cloned());
    paths(job, &mut |path| {
        if !path.starts_with(work) {
            readable.push(path.clone());
        }
    });
    for part in &job.parts {
        if let Part::Specimen { face, .. } = part {
            readable.push(face.path.clone());
        }
    }
    readable.sort();
    readable.dedup();
    let readable: Vec<&Path> = readable.iter().map(PathBuf::as_path).collect();
    let mut run = crate::sandbox::fed_reading(program, work, Otherwise::Run, &readable)?;
    // The job names what is in `work` where the helper sees it.
    let inside = run.work.clone();
    let moved = |path: &mut PathBuf, from: &Path, to: &Path| {
        if let Ok(rest) = path.strip_prefix(from) {
            *path = to.join(rest);
        }
    };
    paths(job, &mut |path| moved(path, work, &inside));
    job.output = inside.join(OUTPUT);
    let written = write_job(job, &work.join(JOB));
    paths(job, &mut |path| moved(path, &inside, work));
    job.output.clone_from(&output);
    written?;
    run.command.arg(inside.join(JOB));
    let finished = crate::process::run_helper(&mut run.command, cancel, timeout(job), follow);
    let _ = std::fs::remove_file(work.join(JOB));
    let finished = finished?;
    if !finished.status.success() {
        let said = finished.stderr.lines().next().unwrap_or_default().trim();
        bail!("The PDF could not be made ({}): {said}", finished.status);
    }
    let said = String::from_utf8_lossy(&finished.stdout);
    let line = said
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .context("the print helper said nothing")?;
    let made: Made =
        serde_json::from_str(line).context("the print helper's answer could not be read")?;
    // Taken only as the regular file it is: a link the helper left there
    // (its sandbox lets it make one to any path), or a pipe, is refused.
    let size = skwir_core::fs::file::open_regular_unfollowed(&output)
        .and_then(|file| file.metadata())
        .map(|meta| meta.len())
        .context("the print helper wrote no PDF")?;
    ensure!(size > 0, "the print helper wrote an empty PDF");
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

/// Every picture and drawing the job's parts name, to be moved or bound.
fn paths(job: &mut Job, each: &mut dyn FnMut(&mut PathBuf)) {
    for part in &mut job.parts {
        match part {
            Part::Flow { blocks } => {
                for block in blocks {
                    if let print::BlockKind::Picture(picture) = &mut block.kind {
                        each(&mut picture.path);
                    }
                }
            }
            Part::Pictures { pictures, .. } | Part::Pages { pages: pictures } => {
                for picture in pictures {
                    each(&mut picture.path);
                }
            }
            Part::Drawing { svg, .. } => each(svg),
            Part::Code { .. } | Part::Table { .. } | Part::Specimen { .. } => {}
        }
    }
}

/// How long the helper may take: a minute, and far more than what it was
/// measured to take for each line, row, block and picture (some 90 µs a
/// line of code in a release build, ten times that in a debug one) and
/// for each 32 bytes of text (some 2.3 µs a byte), so only a helper that
/// hangs is stopped: a line of 128 MB, laid out a piece at a time, is
/// given what its bytes take, never a short line's time. Three hours at
/// most.
fn timeout(job: &Job) -> Duration {
    let mut units: u64 = 0;
    let mut bytes: u64 = 0;
    let mut pictures: u64 = 0;
    for part in &job.parts {
        match part {
            Part::Code { lines, .. } => {
                units += lines.len() as u64;
                bytes += lines.iter().map(|line| line.text.len() as u64).sum::<u64>();
            }
            Part::Table { rows, .. } => units += rows.len() as u64,
            Part::Flow { blocks } => {
                units += blocks.len() as u64 * 4;
                bytes += blocks
                    .iter()
                    .map(|block| block.text.len() as u64)
                    .sum::<u64>();
                pictures += blocks
                    .iter()
                    .filter(|b| matches!(b.kind, print::BlockKind::Picture(_)))
                    .count() as u64;
            }
            Part::Pictures { pictures: list, .. } | Part::Pages { pages: list } => {
                pictures += list.len() as u64;
            }
            Part::Drawing { .. } | Part::Specimen { .. } => pictures += 5,
        }
    }
    let units = units.saturating_add(bytes / 32);
    let most = Duration::from_hours(3);
    (Duration::from_secs(60)
        + Duration::from_millis(units.saturating_mul(2))
        + Duration::from_secs(pictures))
    .min(most)
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::inspection::{Block, BlockKind, Row, Run, TextFile};
    use std::sync::Arc;

    fn palette() -> Palette {
        Palette::of(&CodeTheme::own(false))
    }

    fn none(_: &inspection::Picture) -> Option<PathBuf> {
        None
    }

    /// A family the machine has, by name, and one of its faces' names.
    fn a_family() -> Option<String> {
        let db = crate::inspection::system_fonts();
        let wanted = ["DejaVu Sans", "Noto Sans", "Liberation Sans", "Cantarell"];
        let named = |name: &str| {
            db.faces()
                .any(|face| face.families.iter().any(|(family, _)| family == name))
        };
        let found = wanted
            .iter()
            .find(|name| named(name))
            .map(|name| (*name).to_owned());
        if found.is_none() {
            eprintln!("skipped: none of {wanted:?} is installed");
        }
        found
    }

    /// A family skwir carries that the system lacks is printed in the
    /// files its package carries, as the window draws it.
    #[test]
    fn a_family_the_system_lacks_is_printed_in_the_files_carried() {
        let system = crate::inspection::system_fonts();
        let Some((name, path)) = system.faces().find_map(|face| match &face.source {
            fontdb::Source::File(path) if face.style == fontdb::Style::Normal => {
                Some((face.families.first()?.0.clone(), path.clone()))
            }
            _ => None,
        }) else {
            eprintln!("skipped: no font file on this machine");
            return;
        };
        let roles = Roles {
            text: name.clone(),
            code: "monospace".into(),
        };
        let lacking = fontdb::Database::new();
        let carried = carried_lacked(&lacking, &[name.as_str()], &roles, |asked| {
            assert_eq!(asked, [name.as_str()]);
            vec![path.clone()]
        })
        .expect("its carried files");
        let family = family(&carried, &name, fontdb::Family::Serif);
        assert_eq!(family.regular.map(|face| face.path), Some(path.clone()));
        // The system has it: nothing carried is read.
        assert!(carried_lacked(system, &[name.as_str()], &roles, |_| unreachable!()).is_none());
        // A family no role names: none.
        let other = Roles {
            text: "serif".into(),
            code: "monospace".into(),
        };
        assert!(carried_lacked(&lacking, &[name.as_str()], &other, |_| unreachable!()).is_none());
    }

    #[test]
    fn print_job_faces_of_a_family_the_machine_has_are_its_files() {
        let started = std::time::Instant::now();
        let db = crate::inspection::system_fonts();
        eprintln!(
            "the system's {} faces read in {:?}",
            db.len(),
            started.elapsed()
        );
        let Some(name) = a_family() else { return };
        let (fonts, folders) = faces(&Roles {
            text: name.clone(),
            code: "monospace".into(),
        });
        let regular = fonts.text.regular.expect("its regular face");
        assert!(regular.path.is_file(), "{}", regular.path.display());
        let id = db
            .faces()
            .find(|face| face_of(face).as_ref() == Some(&regular))
            .unwrap();
        assert!(id.families.iter().any(|(family, _)| *family == name));
        if let Some(bold) = &fonts.text.bold {
            assert_ne!(bold, &regular);
        }
        assert!(fonts.code.regular.is_some(), "a monospaced face");
        assert!(!folders.is_empty());
        for (at, folder) in folders.iter().enumerate() {
            assert!(folder.is_dir(), "{}", folder.display());
            assert!(
                folders[at + 1..]
                    .iter()
                    .all(|other| !other.starts_with(folder)),
                "only the outermost: {}",
                folder.display()
            );
        }
        assert!(
            folders
                .iter()
                .any(|folder| regular.path.starts_with(folder))
        );
        // A family the system lacks is its generic one.
        let (fonts, _) = faces(&Roles {
            text: "A family nobody made".into(),
            code: "Nor this one".into(),
        });
        assert!(fonts.text.regular.is_some() && fonts.code.regular.is_some());
    }

    fn code_block(text: &str) -> Block {
        Block {
            text: text.into(),
            code: true,
            language: Some("rs".into()),
            ..Block::default()
        }
    }

    fn colors(line: &Line) -> BTreeSet<[u8; 3]> {
        line.runs.iter().filter_map(|run| run.color).collect()
    }

    #[test]
    fn print_job_code_is_colored_and_numbered() {
        let data = Inspection {
            kind: InspectionKind::Code,
            blocks: vec![
                code_block("fn main() {"),
                code_block("    let answer = \"forty-two\"; // said"),
                code_block("}"),
            ],
            ..Inspection::default()
        };
        let parts = parts(&data, &palette(), &mut none, &CancelToken::new()).unwrap();
        assert!(parts.notice.is_none());
        let [
            Part::Code {
                lines,
                numbers,
                first,
            },
        ] = parts.parts.as_slice()
        else {
            panic!("{:?}", parts.parts);
        };
        assert!(*numbers && *first == 1);
        assert_eq!(lines.len(), 3);
        assert!(colors(&lines[1]).len() >= 2, "{:?}", lines[1].runs);
    }

    #[test]
    fn print_job_a_file_read_where_it_is_shown_is_read_whole_up_to_the_bound() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.rs");
        let mut text = String::new();
        for i in 0..700 {
            std::fmt::Write::write_fmt(&mut text, format_args!("let x{i} = {i};\r\n")).unwrap();
        }
        std::fs::write(&path, &text).unwrap();
        let file = TextFile {
            path: path.clone(),
            language: Some("rs".into()),
            whole: false,
            held: None,
        };
        let data = Inspection {
            kind: InspectionKind::Code,
            blocks: vec![code_block("let x0 = 0;")],
            text: Some(file.clone()),
            ..Inspection::default()
        };
        let printed = parts(&data, &palette(), &mut none, &CancelToken::new()).unwrap();
        let [Part::Code { lines, .. }] = printed.parts.as_slice() else {
            panic!("{:?}", printed.parts);
        };
        assert_eq!(lines.len(), 700);
        assert_eq!(lines[699].text, "let x699 = 699;");
        assert!(printed.notice.is_none());
        // Past the bound of lines, or of text, it is cut and says so.
        let cancel = CancelToken::new();
        let (lines, cut) = read_lines(&file, 100, CODE_TEXT, &cancel).unwrap();
        assert!(cut && lines.len() == 100);
        let (lines, cut) = read_lines(&file, 1000, 64, &cancel).unwrap();
        assert!(cut && lines.len() < 10, "{lines:?}");
        assert_eq!(separated(1_000_000), "1,000,000");
        assert_eq!(separated(999), "999");
    }

    fn row(cells: &[&str], header: bool) -> Block {
        let mut text = String::new();
        let mut ranges = Vec::new();
        for cell in cells {
            if !ranges.is_empty() {
                text.push('\t');
            }
            let start = text.len();
            text.push_str(cell);
            ranges.push(start..text.len());
        }
        Block {
            kind: BlockKind::TableRow {
                cells: ranges,
                header,
            },
            text,
            ..Block::default()
        }
    }

    #[test]
    fn print_job_a_document_s_blocks_are_printed_field_by_field() {
        let dir = tempfile::tempdir().unwrap();
        let image = skwir_core::thumbs::Image {
            width: 2,
            height: 2,
            rgba: Arc::from(vec![255u8; 16]),
        };
        let mut picture_block = Block::default();
        picture_block.set_picture(Some(inspection::Picture::Ready(image)), "A white square");
        let data = Inspection {
            kind: InspectionKind::Markdown,
            blocks: vec![
                Block {
                    heading: 1,
                    anchors: vec!["title".into()],
                    ..Block {
                        text: "A title".into(),
                        ..Block::default()
                    }
                },
                Block {
                    text: "Bold and a link found".into(),
                    runs: vec![
                        Run {
                            range: 0..4,
                            bold: true,
                            ..Run::default()
                        },
                        Run {
                            range: 11..15,
                            link: Some("https://example.org".into()),
                            found: true,
                            script: inspection::Script::Superscript,
                            ..Run::default()
                        },
                    ],
                    ..Block::default()
                },
                Block {
                    text: "An item".into(),
                    list_depth: 2,
                    marker: Some("3.".into()),
                    align: inspection::Align::Center,
                    size: Some(1.2),
                    ..Block::default()
                },
                code_block("let a = 1;"),
                Block {
                    kind: BlockKind::Rule,
                    ..Block::default()
                },
                row(&["Name", "Size"], true),
                picture_block,
            ],
            ..Inspection::default()
        };
        let mut written = Vec::new();
        let mut write = |picture: &inspection::Picture| -> Option<PathBuf> {
            let inspection::Picture::Ready(image) = picture else {
                return None;
            };
            let path = dir.path().join(format!("picture-{}.png", written.len()));
            image::RgbaImage::from_raw(image.width, image.height, image.rgba.to_vec())?
                .save(&path)
                .ok()?;
            written.push(path.clone());
            Some(path)
        };
        let printed = parts(&data, &palette(), &mut write, &CancelToken::new()).unwrap();
        let [Part::Flow { blocks }] = printed.parts.as_slice() else {
            panic!("{:?}", printed.parts);
        };
        assert_eq!(blocks.len(), 7);
        assert_eq!(
            (blocks[0].heading, blocks[0].anchors.as_slice()),
            (1, ["title".to_owned()].as_slice())
        );
        let runs = &blocks[1].runs;
        assert!(runs[0].bold && (runs[0].start, runs[0].end) == (0, 4));
        assert_eq!(runs[1].link.as_deref(), Some("https://example.org"));
        assert_eq!(runs[1].script, 1);
        assert_eq!(
            (
                blocks[2].list_depth,
                blocks[2].marker.as_deref(),
                blocks[2].align,
                blocks[2].size
            ),
            (2, Some("3."), print::Align::Center, 1.2)
        );
        assert!(matches!(blocks[3].kind, print::BlockKind::Code));
        assert!(
            colors(&Line {
                text: blocks[3].text.clone(),
                runs: blocks[3].runs.clone()
            })
            .len()
                >= 2
        );
        assert!(matches!(blocks[4].kind, print::BlockKind::Rule));
        let print::BlockKind::Row { cells, header } = &blocks[5].kind else {
            panic!("{:?}", blocks[5].kind);
        };
        assert!(*header && cells == &[(0, 4), (5, 9)]);
        let print::BlockKind::Picture(picture) = &blocks[6].kind else {
            panic!("{:?}", blocks[6].kind);
        };
        assert_eq!((picture.width, picture.height), (2, 2));
        assert_eq!(picture.alt, "A white square");
        assert!(written.contains(&picture.path) && picture.path.is_file());
        // A picture not written prints its description.
        let printed = parts(&data, &palette(), &mut none, &CancelToken::new()).unwrap();
        let [Part::Flow { blocks }] = printed.parts.as_slice() else {
            panic!();
        };
        assert_eq!(blocks[6].text, "A white square");
    }

    #[test]
    fn print_job_a_table_prints_every_row_and_some_kinds_nothing() {
        let data = Inspection {
            kind: InspectionKind::Table,
            columns: vec!["a".into(), "b".into()],
            rows: (0..3)
                .map(|i| Row {
                    cells: vec![format!("{i}"), format!("{}", i * 2)],
                    ..Row::default()
                })
                .collect(),
            ..Inspection::default()
        };
        let printed = parts(&data, &palette(), &mut none, &CancelToken::new()).unwrap();
        let [Part::Table { columns, rows }] = printed.parts.as_slice() else {
            panic!("{:?}", printed.parts);
        };
        assert_eq!(columns, &["a", "b"]);
        assert_eq!(rows[2], ["2", "4"]);
        for kind in [
            InspectionKind::Hex,
            InspectionKind::Archive,
            InspectionKind::Map,
            InspectionKind::Comic,
            InspectionKind::Font,
            InspectionKind::Model,
        ] {
            let data = Inspection {
                kind,
                blocks: vec![Block {
                    text: "x".into(),
                    ..Block::default()
                }],
                ..Inspection::default()
            };
            let printed = parts(&data, &palette(), &mut none, &CancelToken::new()).unwrap();
            assert!(printed.parts.is_empty(), "{kind:?}");
        }
    }

    /// The helper built beside the tests (`cargo nextest run -p
    /// skwir-print-worker -p skwir-platform` builds it); without it a test
    /// says so and passes, as the other helpers' do.
    fn built_helper() -> Option<PathBuf> {
        let name = format!("skwir-print-worker{}", std::env::consts::EXE_SUFFIX);
        let helper = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join(&name)))
            .filter(|path| path.is_file());
        if helper.is_none() {
            eprintln!("skipped: needs skwir-print-worker built beside the tests");
        }
        helper
    }

    /// A long line of text is given what its bytes take, not a short
    /// line's time; nothing is given more than three hours.
    #[test]
    fn a_long_line_is_given_the_time_its_bytes_take() {
        let code = |text: String| Job {
            paper: print::Paper::a4(),
            output: PathBuf::new(),
            title: String::new(),
            author: None,
            language: None,
            header: None,
            numbers: false,
            fonts: Fonts::default(),
            fallback: Vec::new(),
            pages: None,
            parts: vec![Part::Code {
                lines: vec![print::Line {
                    text,
                    runs: Vec::new(),
                }],
                numbers: false,
                first: 1,
            }],
        };
        let short = timeout(&code("let x = 1;".into()));
        // 3.2 MB: 100,000 units of 2 ms beside the line's own.
        let long = timeout(&code("x".repeat(3_200_000)));
        assert_eq!(long.checked_sub(short), Some(Duration::from_secs(200)));
        let flow = Job {
            parts: vec![Part::Flow {
                blocks: vec![print::Block {
                    text: "y".repeat(320),
                    ..print::Block::default()
                }],
            }],
            ..code(String::new())
        };
        // A block's 4 units, and 10 for its 320 bytes.
        assert_eq!(
            timeout(&flow),
            Duration::from_secs(60) + Duration::from_millis(28)
        );
        let pictures = Job {
            parts: vec![Part::Pictures {
                pictures: vec![print::Picture::default(); 11_000],
                sheet: true,
            }],
            ..code(String::new())
        };
        assert_eq!(
            timeout(&pictures),
            Duration::from_hours(3),
            "three hours at most"
        );
    }

    fn job(parts: Vec<Part>) -> Option<Job> {
        let name = a_family()?;
        let (fonts, fallback) = faces(&Roles {
            text: name,
            code: "monospace".into(),
        });
        Some(Job {
            paper: print::Paper::a4(),
            output: PathBuf::new(),
            title: "Printed".into(),
            author: None,
            language: Some("en".into()),
            header: Some("example.md".into()),
            numbers: true,
            fonts,
            fallback,
            pages: None,
            parts,
        })
    }

    #[test]
    fn print_job_a_job_is_made_into_a_pdf_in_its_folder() {
        let Some(helper) = built_helper() else { return };
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("picture.png");
        image::RgbImage::from_pixel(40, 30, image::Rgb([200, 30, 30]))
            .save(&png)
            .unwrap();
        let data = Inspection {
            kind: InspectionKind::Markdown,
            blocks: vec![
                Block {
                    heading: 1,
                    text: "Printed words".into(),
                    ..Block::default()
                },
                Block {
                    text: "A paragraph under it.".into(),
                    ..Block::default()
                },
            ],
            ..Inspection::default()
        };
        let mut printed = parts(&data, &palette(), &mut none, &CancelToken::new())
            .unwrap()
            .parts;
        printed.push(Part::Pictures {
            pictures: vec![print::Picture {
                path: png,
                width: 40,
                height: 30,
                alt: "A red box".into(),
                caption: None,
                orientation: 0,
            }],
            sheet: false,
        });
        let Some(mut job) = job(printed) else { return };
        let made = made(
            &helper,
            &mut job,
            dir.path(),
            &CancelToken::new(),
            &crate::process::Follow::none(),
        )
        .unwrap();
        assert_eq!(
            made,
            Made {
                pages: 2,
                laid_out: 2
            }
        );
        assert_eq!(job.output, dir.path().join(OUTPUT));
        assert!(!dir.path().join(JOB).exists(), "the job removed once read");
        let pdf = std::fs::read(&job.output).unwrap();
        let document = lopdf::Document::load_mem(&pdf).unwrap();
        assert_eq!(document.get_pages().len(), 2);
        let text = document.extract_text(&[1]).unwrap_or_default();
        assert!(text.contains("Printed"), "{text}");
        // The picture's path is the job's own again.
        let Part::Pictures { pictures, .. } = &job.parts[1] else {
            panic!();
        };
        assert_eq!(pictures[0].path, dir.path().join("picture.png"));
    }

    /// A helper that leaves a link where its PDF goes, to a file of the
    /// user's, made nothing: what it points to is never taken as the PDF.
    #[cfg(unix)]
    #[test]
    fn a_link_the_helper_left_for_its_pdf_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (work, outside) = (dir.path().join("work"), dir.path().join("private.txt"));
        std::fs::create_dir(&work).unwrap();
        std::fs::write(&outside, "not for printing").unwrap();
        let helper = dir.path().join("helper.sh");
        std::fs::write(
            &helper,
            format!(
                "#!/bin/sh\nln -s '{}' \"$(dirname \"$1\")/{OUTPUT}\"\necho '{{\"pages\":1,\"laid_out\":1}}'\n",
                outside.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut job = Job {
            paper: print::Paper::a4(),
            output: PathBuf::new(),
            title: String::new(),
            author: None,
            language: None,
            header: None,
            numbers: false,
            fonts: Fonts::default(),
            fallback: Vec::new(),
            pages: None,
            parts: Vec::new(),
        };
        let result = made(
            &helper,
            &mut job,
            &work,
            &CancelToken::new(),
            &crate::process::Follow::none(),
        );
        assert!(
            std::fs::symlink_metadata(work.join(OUTPUT)).is_ok_and(|m| m.is_symlink()),
            "the stand-in left its link"
        );
        let error = result.unwrap_err().to_string();
        assert!(error.contains("wrote no PDF"), "{error}");
    }

    #[test]
    fn print_job_a_job_the_helper_refuses_is_an_error_with_its_reason() {
        let Some(helper) = built_helper() else { return };
        let dir = tempfile::tempdir().unwrap();
        // A job naming no font: the helper refuses it, and says why.
        let mut job = Job {
            paper: print::Paper::a4(),
            output: PathBuf::new(),
            title: String::new(),
            author: None,
            language: None,
            header: None,
            numbers: false,
            fonts: Fonts::default(),
            fallback: Vec::new(),
            pages: None,
            parts: vec![Part::Flow {
                blocks: vec![print::Block {
                    text: "words".into(),
                    ..print::Block::default()
                }],
            }],
        };
        let error = made(
            &helper,
            &mut job,
            dir.path(),
            &CancelToken::new(),
            &crate::process::Follow::none(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("names no font"), "{error}");
        assert!(!dir.path().join(OUTPUT).exists());
    }

    /// Cancelled while the helper works, it is ended at once, whatever it
    /// was doing, and nothing of it is left running.
    #[cfg(target_os = "linux")]
    #[test]
    fn print_job_a_cancelled_job_ends_the_helper() {
        let Some(helper) = built_helper() else { return };
        let lines: Vec<Line> = (0..400_000)
            .map(|i| Line {
                text: format!("let value_{i} = compute({i}); // a line of some length"),
                runs: Vec::new(),
            })
            .collect();
        let Some(mut job) = job(vec![Part::Code {
            lines,
            numbers: true,
            first: 1,
        }]) else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().to_owned();
        let cancel = CancelToken::new();
        let watched = cancel.clone();
        let seen = std::thread::spawn(move || {
            // Its arguments name the folder (bubblewrap's) or the job.
            let job = work.join(JOB);
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            while std::time::Instant::now() < deadline {
                let running = crate::process::naming(work.as_os_str()).len()
                    + crate::process::naming(job.as_os_str()).len();
                if running > 0 {
                    std::thread::sleep(Duration::from_millis(300));
                    watched.cancel();
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            watched.cancel();
            false
        });
        let started = std::time::Instant::now();
        let result = made(
            &helper,
            &mut job,
            dir.path(),
            &cancel,
            &crate::process::Follow::none(),
        );
        assert!(seen.join().unwrap(), "the helper was seen running");
        assert!(result.is_err(), "cancelled: {result:?}");
        assert!(started.elapsed() < Duration::from_secs(60));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let left = crate::process::naming(dir.path().as_os_str()).len()
                + crate::process::naming(dir.path().join(JOB).as_os_str()).len();
            if left == 0 {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "{left} left running");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!dir.path().join(OUTPUT).exists());
    }
}
