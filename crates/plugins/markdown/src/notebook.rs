//! Jupyter notebooks (`.ipynb`) read as the document they are: Markdown
//! cells as Markdown, code cells as code in the kernel's language, and what
//! each printed or drew after it. The notebook is written out as one
//! Markdown document and read by the same parser as a README, so it reads
//! alike and needs no layout of its own. A picture an output drew is named
//! for later (`OUTPUT`) and decoded again from the notebook when it is seen.
use base64::Engine as _;
use serde_json::Value;
use skwir_core::{CancelToken, inspection::Picture, preview::Held, thumbs::Image};
use skwir_platform::inspection::{self, limits, limits::Bounds};
use std::fmt::Write as _;
use std::sync::Arc;

/// The key prefix of a picture an output drew; the rest is its number in
/// the notebook, counted in reading order.
pub const OUTPUT: &str = "notebook-output:";

/// Lines of one output kept: a training loop can print thousands. Full
/// preview keeps them all.
const OUTPUT_LINES: usize = 200;

/// What the notebook says of itself, for Properties.
pub struct Notebook {
    pub markdown: String,
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

/// A fence longer than any run of backticks in `code`.
fn fence(code: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for ch in code.chars() {
        run = if ch == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat((longest + 1).max(3))
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

/// The pictures an output bundle can hold, in the order they are preferred.
const PICTURES: [&str; 3] = ["image/png", "image/jpeg", "image/svg+xml"];

/// The picture of an output bundle, as bytes.
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
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()
}

/// The notebook's output bundles that hold a picture, in reading order:
/// what `OUTPUT` numbers.
fn pictures(notebook: &Value) -> impl Iterator<Item = &Value> {
    cells(notebook)
        .flat_map(outputs)
        .filter_map(|output| output.get("data"))
        .filter(|data| PICTURES.iter().any(|mime| data.get(*mime).is_some()))
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

/// The notebook as one Markdown document, within `bounds`. Pictures are
/// named `OUTPUT` and a number; `load` is asked for each as the parser
/// meets it.
pub fn markdown(
    notebook: &Value,
    bounds: Bounds,
    cancel: &CancelToken,
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
    let code_language = language.clone().unwrap_or_default();
    let mut out = String::new();
    let mut shortened = 0;
    let mut picture = 0;
    let mut count = 0;
    for cell in cells(notebook) {
        inspection::check(cancel)?;
        count += 1;
        let source = text(cell.get("source"));
        match cell.get("cell_type").and_then(Value::as_str) {
            Some("markdown") => {
                out.push_str(&source);
                out.push_str("\n\n");
            }
            Some("code") => {
                let code = fence(&source);
                let _ = write!(out, "{code}{code_language}\n{source}\n{code}\n\n");
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
                        Some("execute_result" | "display_data") => {
                            let data = output.get("data");
                            if data.is_some_and(|data| {
                                PICTURES.iter().any(|mime| data.get(*mime).is_some())
                            }) {
                                let _ = write!(out, "![]({OUTPUT}{picture})\n\n");
                                picture += 1;
                                None
                            } else if let Some(markdown) = data.and_then(|d| d.get("text/markdown"))
                            {
                                out.push_str(&text(Some(markdown)));
                                out.push_str("\n\n");
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
                    let printed = fence(&shown);
                    // Labelled as what it is; no grammar claims the word.
                    let _ = write!(out, "{printed}output\n{shown}\n{printed}\n\n");
                }
            }
            // A raw cell is for a converter, not for reading.
            _ => {}
        }
        if out.len() > bounds.text_input() {
            return Err(bounds.refuse(format!(
                "Notebook text exceeds {} preview limit",
                limits::describe(bounds.text_input())
            )));
        }
    }
    Ok(Notebook {
        markdown: out,
        cells: count,
        language,
        kernel,
        shortened,
    })
}

/// The picture `href` names, if it is an output's: named for later, at
/// the size read from its header.
pub fn named(notebook: &Value, href: &str) -> Option<Picture> {
    let bytes = numbered(notebook, href)?;
    let (width, height) = inspection::picture_size(&bytes)?;
    Some(Picture::Later {
        key: href.to_owned(),
        width,
        height,
    })
}

/// The picture `key` names, decoded.
pub fn decoded(notebook: &Value, key: &str) -> Option<Image> {
    inspection::image(&numbered(notebook, key)?).ok()
}

fn numbered(notebook: &Value, key: &str) -> Option<Vec<u8>> {
    let number: usize = key.strip_prefix(OUTPUT)?.parse().ok()?;
    let bytes = picture_bytes(pictures(notebook).nth(number)?)?;
    (bytes.len() <= limits::BINARY_INPUT).then_some(bytes)
}
