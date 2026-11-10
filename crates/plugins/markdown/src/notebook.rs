//! Jupyter notebooks (`.ipynb`) read as the document they are: Markdown
//! cells as Markdown, code cells as code in the kernel's language, and what
//! each printed, returned or drew after it. Each cell is read apart, as
//! Jupyter draws it, into one page (`markdown::Document`): a Markdown cell
//! by the same parser as a README, a result Jupyter shows as HTML by the
//! same reader as an HTML file, so it reads alike and needs no layout of
//! its own, and a list, a fence or an HTML block one cell leaves open never
//! takes the next. A picture an output drew, or one pasted into a cell
//! (`attachment:`), is named for later (`OUTPUT`, `ATTACHMENT`) and decoded
//! again from the notebook when it is seen.
use crate::markdown::Document;
use base64::Engine as _;
use serde_json::Value;
use skwir_core::{
    CancelToken,
    inspection::{Block, Picture},
    preview::Held,
    thumbs::Image,
};
use skwir_platform::inspection::{self, limits, limits::Bounds};
use skwir_platform::rich_preview;
use std::sync::Arc;

/// The key prefix of a picture an output drew; the rest is its number in
/// the notebook, counted in reading order.
const OUTPUT: &str = "notebook-output:";

/// The key prefix of a picture pasted into a cell; the rest is the cell's
/// number, a `/` and the name the cell gives it.
const ATTACHMENT: &str = "notebook-attachment:";

/// Lines of one output kept: a training loop can print thousands. Full
/// preview keeps them all.
const OUTPUT_LINES: usize = 200;

/// What the notebook says of itself, for Properties, and the page it reads
/// as.
pub struct Notebook {
    pub page: rich_preview::Page,
    pub cells: usize,
    pub language: Option<String>,
    pub kernel: Option<String>,
    /// Outputs shortened to `OUTPUT_LINES`, not under Full preview.
    pub shortened: usize,
}

/// Read the notebook `held` holds, within `bounds`' binary input: its
/// pictures are inside it.
pub fn read(held: &Arc<Held>, bounds: Bounds, cancel: &CancelToken) -> anyhow::Result<Value> {
    let bytes = inspection::read_file(held, bounds, "Notebook", cancel)?;
    let value: Value = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        value.get("nbformat").and_then(Value::as_u64) == Some(4),
        "Only notebooks in format 4 (Jupyter 4 and later) are read"
    );
    Ok(value)
}

/// A cell's or an output's text: a string, or the lines of one.
fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(lines)) => lines.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

/// A terminal's colours taken out of a traceback: `ESC [ … letter`.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// The pictures a bundle (an output's data, a cell's attachment) can hold,
/// in the order they are preferred.
const PICTURES: [&str; 3] = ["image/png", "image/jpeg", "image/svg+xml"];

fn has_picture(data: &Value) -> bool {
    PICTURES.iter().any(|mime| data.get(*mime).is_some())
}

/// The picture of a bundle, as bytes.
fn picture_bytes(data: &Value) -> Option<Vec<u8>> {
    let (mime, value) = PICTURES
        .iter()
        .find_map(|mime| Some((*mime, data.get(*mime)?)))?;
    let text = text(Some(value));
    if mime == "image/svg+xml" {
        return Some(text.into_bytes());
    }
    let encoded: String = text
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    (bytes.len() <= limits::BINARY_INPUT).then_some(bytes)
}

/// The picture `bytes` hold, named `key` for later, at the size read from
/// its header.
fn later(key: String, bytes: &[u8]) -> Option<Picture> {
    let (width, height) = inspection::picture_size(bytes)?;
    Some(Picture::Later { key, width, height })
}

/// The notebook's output bundles that hold a picture, in reading order:
/// what `OUTPUT` numbers.
fn pictures(notebook: &Value) -> impl Iterator<Item = &Value> {
    cells(notebook)
        .flat_map(outputs)
        .filter_map(|output| output.get("data"))
        .filter(|data| has_picture(data))
}

fn cells(notebook: &Value) -> impl Iterator<Item = &Value> {
    notebook
        .get("cells")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn outputs(cell: &Value) -> impl Iterator<Item = &Value> {
    cell.get("outputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// The attachment a cell names `name`, with the name it is kept under: as
/// written, else as its link encodes it (`shot%20one.png`: a Markdown link
/// holds no space), as a picture beside a document is found.
fn attachment<'a>(attachments: &'a Value, name: &str) -> Option<(String, &'a Value)> {
    if let Some(bundle) = attachments.get(name) {
        return Some((name.to_owned(), bundle));
    }
    let name = String::from_utf8(skwir_platform::uri::percent_decode(name)).ok()?;
    let bundle = attachments.get(&name)?;
    Some((name, bundle))
}

/// Count `text` against `bounds`' text input before it is read.
fn spend(read: &mut usize, text: &str, bounds: Bounds) -> anyhow::Result<()> {
    *read += text.len();
    if *read > bounds.text_input() {
        return Err(bounds.refuse(format!(
            "Notebook text exceeds {} preview limit",
            limits::describe(bounds.text_input())
        )));
    }
    Ok(())
}

/// The notebook as one page, within `bounds`. `beside` names a picture a
/// cell finds beside the notebook.
pub fn page(
    notebook: &Value,
    bounds: Bounds,
    cancel: &CancelToken,
    mut beside: impl FnMut(&str) -> Option<Picture>,
) -> anyhow::Result<Notebook> {
    let output_lines = bounds.choose(OUTPUT_LINES, usize::MAX);
    let metadata = notebook.get("metadata");
    let language = metadata
        .and_then(|m| {
            m.pointer("/kernelspec/language")
                .or(m.pointer("/language_info/name"))
        })
        .and_then(Value::as_str)
        .map(str::to_owned);
    let kernel = metadata
        .and_then(|m| m.pointer("/kernelspec/display_name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let code_language = language.clone().filter(|name| !name.trim().is_empty());
    let mut document = Document::new(bounds);
    let mut read = 0;
    let mut shortened = 0;
    let mut picture = 0;
    for (number, cell) in cells(notebook).enumerate() {
        inspection::check(cancel)?;
        if document.full() {
            break;
        }
        let source = text(cell.get("source"));
        match cell.get("cell_type").and_then(Value::as_str) {
            Some("markdown") => {
                spend(&mut read, &source, bounds)?;
                let attachments = cell.get("attachments");
                // What a cell names as an attachment is in the notebook,
                // never a file beside it.
                let load = |href: &str| match href.strip_prefix("attachment:") {
                    Some(name) => {
                        let (name, bundle) = attachment(attachments?, name)?;
                        later(
                            format!("{ATTACHMENT}{number}/{name}"),
                            &picture_bytes(bundle)?,
                        )
                    }
                    None => beside(href),
                };
                document.cell(&source, cancel, load)?;
            }
            Some("code") => {
                if !source.trim().is_empty() {
                    spend(&mut read, &source, bounds)?;
                    document.push(Block {
                        code: true,
                        language: code_language.clone(),
                        text: source,
                        ..Block::default()
                    });
                }
                for output in outputs(cell) {
                    let shown = match output.get("output_type").and_then(Value::as_str) {
                        Some("stream") => Some(text(output.get("text"))),
                        // A traceback's lines are its items, without their
                        // line breaks, coloured for a terminal.
                        Some("error") => Some(plain(
                            &output
                                .get("traceback")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )),
                        // Shown as Jupyter shows it: its HTML, Markdown or
                        // LaTeX, else its picture, else its text.
                        Some("execute_result" | "display_data") => {
                            let data = output.get("data");
                            // Every bundle holding a picture is numbered,
                            // shown or not, as `pictures` numbers them.
                            let key = data.is_some_and(has_picture).then(|| {
                                picture += 1;
                                format!("{OUTPUT}{}", picture - 1)
                            });
                            let pictured = key.is_some();
                            if rich(
                                &mut document,
                                data,
                                &mut read,
                                cancel,
                                pictured,
                                &mut beside,
                            )? {
                                None
                            } else if let Some(key) = key {
                                if let Some(picture) = data
                                    .and_then(picture_bytes)
                                    .and_then(|bytes| later(key, &bytes))
                                {
                                    let mut block = Block::default();
                                    block.set_picture(Some(picture), "");
                                    document.push(block);
                                }
                                None
                            } else {
                                data.map(|d| text(d.get("text/plain")))
                            }
                        }
                        _ => None,
                    };
                    let Some(shown) = shown.filter(|shown| !shown.trim().is_empty()) else {
                        continue;
                    };
                    let mut lines: Vec<&str> = shown.trim_end().lines().collect();
                    if lines.len() > output_lines {
                        lines.truncate(output_lines);
                        shortened += 1;
                    }
                    let shown = lines.join("\n");
                    spend(&mut read, &shown, bounds)?;
                    // Labelled as what it is; no grammar claims the word.
                    document.push(Block {
                        code: true,
                        language: Some("output".into()),
                        text: shown,
                        ..Block::default()
                    });
                }
            }
            // A raw cell is for a converter, not for reading.
            _ => {}
        }
    }
    Ok(Notebook {
        page: document.finish(cancel),
        cells: cells(notebook).count(),
        language,
        kernel,
        shortened,
    })
}

/// A result shown as Jupyter shows it, read into `document`: its HTML (a
/// table of data), else its Markdown, else its LaTeX (what sympy prints);
/// whether it showed anything. HTML a script would draw shows nothing
/// inert, and what is next stands for it. LaTeX is read where its formulas
/// can be typeset, or where no picture of it (`pictured`) stands beside it.
fn rich(
    document: &mut Document,
    data: Option<&Value>,
    read: &mut usize,
    cancel: &CancelToken,
    pictured: bool,
    beside: &mut impl FnMut(&str) -> Option<Picture>,
) -> anyhow::Result<bool> {
    let Some(data) = data else {
        return Ok(false);
    };
    let bounds = document.bounds();
    if let Some(html) = data.get("text/html") {
        let html = text(Some(html));
        spend(read, &html, bounds)?;
        if document.html(&html, cancel, &mut *beside)? {
            return Ok(true);
        }
    }
    if let Some(markdown) = data.get("text/markdown") {
        let markdown = text(Some(markdown));
        spend(read, &markdown, bounds)?;
        document.markdown(&markdown, cancel, &mut *beside, None)?;
        return Ok(true);
    }
    match data.get("text/latex") {
        Some(latex) if !pictured || document.typesets() => {
            let latex = text(Some(latex));
            spend(read, &latex, bounds)?;
            document.cell(&latex, cancel, &mut *beside)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Whether `key` names a picture the notebook holds.
pub fn holds(key: &str) -> bool {
    key.starts_with(OUTPUT) || key.starts_with(ATTACHMENT)
}

/// The picture `key` names, decoded.
pub fn decoded(notebook: &Value, key: &str) -> Option<Image> {
    let bundle = if let Some(number) = key.strip_prefix(OUTPUT) {
        pictures(notebook).nth(number.parse().ok()?)?
    } else {
        let (cell, name) = key.strip_prefix(ATTACHMENT)?.split_once('/')?;
        cells(notebook)
            .nth(cell.parse().ok()?)?
            .get("attachments")?
            .get(name)?
    };
    inspection::image(&picture_bytes(bundle)?).ok()
}
