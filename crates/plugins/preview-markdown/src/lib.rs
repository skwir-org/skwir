//! Reading mode for Markdown and HTML files, and Jupyter notebooks; all
//! parsing and embedded image reads run on workers. All end as the same
//! semantic blocks, so they read alike, and all keep their source one
//! toggle away.
mod diagram;
mod markdown;
mod notebook;
use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::mime::Types;
use bibli_platform::{inspection, rich_preview};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-markdown"
    }
    fn title(&self) -> &'static str {
        "Preview: Markdown, HTML and notebooks"
    }
    fn description(&self) -> &'static str {
        "Read Markdown and HTML files and Jupyter notebooks as documents, with headings, tables, code, Mermaid diagrams and local images; switch to the source at any time. HTML is inert: no scripts, fonts or remote content"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        vec![bibli_platform::diagram::WORKER.requirement("Mermaid diagrams drawn in Markdown")]
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
/// What reading a document tells us for free: how long it is. What was left
/// out (an encoding assumed, blocks past the bound) is the notice.
fn measured(
    mut data: Inspection,
    encoding: inspection::Encoding,
    page: rich_preview::Page,
) -> Inspection {
    let words: usize = page
        .blocks
        .iter()
        .map(|block| block.text.split_whitespace().count())
        .sum();
    let headings = page.blocks.iter().filter(|block| block.heading > 0).count();
    data.details = inspection::details([
        ("Words", inspection::count(words)),
        ("Headings", inspection::count(headings)),
    ]);
    data.notice = [
        encoding.notice(),
        page.truncated.then(inspection::blocks_notice),
        (!data.notice.is_empty()).then(|| std::mem::take(&mut data.notice)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    data.blocks = page.blocks;
    data
}
/// What it reads, by the type the MIME database gives the file.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Markdown,
    Html,
    Xhtml,
    Notebook,
}

static MARKDOWN: Types = Types::new(&["text/markdown"]);
static HTML: Types = Types::new(&["text/html"]);
static XHTML: Types = Types::new(&["application/xhtml+xml"]);
static NOTEBOOK: Types = Types::new(&["application/x-ipynb+json"]);

fn format_of(req: &PreviewRequest) -> Option<Format> {
    let (path, mime) = (&req.entry.path, req.mime.as_deref());
    [
        (&MARKDOWN, Format::Markdown),
        (&HTML, Format::Html),
        (&XHTML, Format::Xhtml),
        (&NOTEBOOK, Format::Notebook),
    ]
    .into_iter()
    .find(|(types, _)| types.holds(path, mime))
    .map(|(_, format)| format)
}

struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-markdown"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        format_of(req).is_some()
    }
    /// A picture the document named, beside it: the same containment as
    /// when it was named.
    fn picture(
        &self,
        req: &PreviewRequest,
        _view: &InspectRequest,
        key: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<bibli_core::thumbs::Image>> {
        let path = req.entry.resolve()?;
        if key.starts_with(notebook::OUTPUT) && format_of(req) == Some(Format::Notebook) {
            return Ok(notebook::decoded(&notebook::read(&path, cancel)?, key));
        }
        let base = path.parent().unwrap_or(std::path::Path::new("."));
        Ok(rich_preview::local_image(base, key, cancel))
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        // The source is the shared code view (the worker's fallback).
        if view.source {
            return Ok(None);
        }
        let Some(format) = format_of(req) else {
            return Ok(None);
        };
        let path = req.entry.resolve()?;
        if format == Format::Notebook {
            return notebook(&path, cancel).map(Some);
        }
        let (source, encoding) = inspection::read_text(&path, cancel)?;
        let base = path.parent().unwrap_or(std::path::Path::new("."));
        let image = |href: &str| rich_preview::local_picture(base, href, cancel);
        let (title, page, notice) = if format == Format::Markdown {
            ("Markdown", markdown::parse(&source, cancel, image)?, "")
        } else {
            let markup = if format == Format::Xhtml {
                rich_preview::Markup::Xhtml
            } else {
                rich_preview::Markup::Html
            };
            (
                "HTML",
                rich_preview::html(
                    &source,
                    markup,
                    cancel,
                    rich_preview::Local { base, cancel },
                )?,
                "Reading mode · scripts, fonts and remote content are not loaded",
            )
        };
        Ok(Some(measured(
            Inspection {
                kind: InspectionKind::Markdown,
                title: title.into(),
                source_available: true,
                notice: notice.into(),
                ..Inspection::default()
            },
            encoding,
            page,
        )))
    }
}

/// A notebook read as a document: its Markdown, its code and outputs, and
/// its pictures beside it or drawn by its outputs.
fn notebook(path: &std::path::Path, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    let value = notebook::read(path, cancel)?;
    let read = notebook::markdown(&value, cancel)?;
    let base = path.parent().unwrap_or(std::path::Path::new("."));
    let page = markdown::parse(&read.markdown, cancel, |href| {
        notebook::named(&value, href).or_else(|| rich_preview::local_picture(base, href, cancel))
    })?;
    let mut data = measured(
        Inspection {
            kind: InspectionKind::Markdown,
            title: "Notebook".into(),
            source_available: true,
            notice: if read.shortened > 0 {
                format!(
                    "{} output{} shortened to their first lines",
                    read.shortened,
                    if read.shortened == 1 { "" } else { "s" }
                )
            } else {
                String::new()
            },
            ..Inspection::default()
        },
        inspection::Encoding::Utf8,
        page,
    );
    data.details.extend(inspection::details([
        ("Cells", inspection::count(read.cells)),
        ("Language", read.language),
        ("Kernel", read.kernel),
    ]));
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Types from the free desktops' database (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn formats_are_known_by_type() {
        let format = |name: &str| {
            format_of(&PreviewRequest {
                entry: bibli_core::op::EntryRef::new(bibli_core::EntryId { dev: 1, ino: 1 }, name),
                mime: None,
                page: 1,
                seconds: None,
                max_side: 64,
                text_size: None,
            })
        };
        assert!(format("a.md") == Some(Format::Markdown));
        assert!(format("a.mkd") == Some(Format::Markdown));
        assert!(format("a.htm") == Some(Format::Html));
        assert!(format("a.xhtml") == Some(Format::Xhtml));
        assert!(format("a.ipynb") == Some(Format::Notebook));
        assert!(format("a.txt").is_none());
    }

    fn read_as(name: &str, bytes: &[u8], view: &InspectRequest) -> Option<Inspection> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let request = PreviewRequest {
            entry: bibli_core::op::EntryRef::from_path(&path).unwrap(),
            mime: Some("text/markdown".into()),
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
        };
        Provider
            .inspect(&request, view, &CancelToken::new())
            .unwrap()
    }
    fn read(name: &str, bytes: &[u8]) -> Inspection {
        read_as(name, bytes, &InspectRequest::default()).unwrap()
    }

    #[test]
    fn a_marked_or_legacy_readme_still_starts_with_its_title() {
        let data = read("README.md", "\u{feff}# T\u{ed}tulo\n\nText.\n".as_bytes());
        assert_eq!(data.blocks[0].heading, 1);
        assert_eq!(data.blocks[0].text, "T\u{ed}tulo");
        assert!(data.notice.is_empty());
        let data = read("notes caf\u{e9}.md", b"# Caf\xe9\n");
        assert_eq!(data.blocks[0].text, "Caf\u{e9}");
        assert_eq!(data.notice, "Read as Windows-1252");
    }

    #[test]
    fn a_long_changelog_shows_its_start_and_says_so() {
        let changelog = "- a change\n".repeat(3000);
        let data = read("CHANGELOG.md", changelog.as_bytes());
        assert_eq!(data.blocks.len(), inspection::limits::BLOCKS);
        assert_eq!(data.notice, inspection::blocks_notice());
        let source = InspectRequest {
            source: true,
            ..InspectRequest::default()
        };
        assert!(
            read_as("CHANGELOG.md", changelog.as_bytes(), &source).is_none(),
            "the source is the shared code view"
        );
    }

    #[test]
    fn a_notebook_reads_as_its_markdown_code_and_outputs() {
        // A 1 × 1 PNG, as a notebook stores a plot.
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let document = serde_json::json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {
                "kernelspec": {"name": "python3", "display_name": "Python 3", "language": "python"}
            },
            "cells": [
                {"cell_type": "markdown", "source": ["# Café results\n", "Some *text*."]},
                {"cell_type": "code", "execution_count": 1,
                 "source": "print('```')\nvalue = 1 + 1",
                 "outputs": [
                    {"output_type": "stream", "name": "stdout", "text": ["```\n"]},
                    {"output_type": "execute_result", "execution_count": 1,
                     "data": {"text/plain": ["2"]}, "metadata": {}},
                    {"output_type": "display_data",
                     "data": {"image/png": png, "text/plain": ["<Figure>"]}, "metadata": {}},
                    {"output_type": "error", "ename": "ValueError", "evalue": "bad",
                     "traceback": ["\u{1b}[0;31mValueError\u{1b}[0m", "bad value"]}
                 ]},
                {"cell_type": "raw", "source": "not for reading"}
            ]
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a notebook’s name.ipynb");
        std::fs::write(&path, document.to_string()).unwrap();
        let data = notebook(&path, &CancelToken::new()).unwrap();
        assert_eq!(data.blocks[0].heading, 1);
        assert_eq!(data.blocks[0].text, "Café results");
        let code: Vec<_> = data
            .blocks
            .iter()
            .filter(|block| block.code)
            .map(|block| (block.language.as_deref(), block.text.as_str()))
            .collect();
        assert_eq!(
            code,
            [
                (Some("python"), "print('```')\nvalue = 1 + 1"),
                // A printed fence stays text, and a traceback loses its colours.
                (Some("output"), "```"),
                (Some("output"), "2"),
                (Some("output"), "ValueError\nbad value"),
            ]
        );
        assert!(
            !data
                .blocks
                .iter()
                .any(|block| block.text.contains("not for reading"))
        );
        let key = data
            .blocks
            .iter()
            .find_map(|block| match &block.picture {
                Some(bibli_core::inspection::Picture::Later {
                    key,
                    width: 1,
                    height: 1,
                }) => Some(key.clone()),
                _ => None,
            })
            .expect("the plot is named for later");
        let request = PreviewRequest {
            entry: bibli_core::op::EntryRef::from_path(&path).unwrap(),
            mime: Some("application/x-ipynb+json".into()),
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
        };
        let image = Provider
            .picture(
                &request,
                &InspectRequest::default(),
                &key,
                &CancelToken::new(),
            )
            .unwrap()
            .expect("decoded when seen");
        assert_eq!((image.width, image.height), (1, 1));
        let fact = |label: &str| {
            data.details
                .iter()
                .find(|d| d.label == label)
                .map(|d| d.value.clone())
        };
        assert_eq!(fact("Cells").as_deref(), Some("3"));
        assert_eq!(fact("Language").as_deref(), Some("python"));
        assert_eq!(fact("Kernel").as_deref(), Some("Python 3"));
    }
}
