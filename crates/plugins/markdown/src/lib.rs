//! Reading mode for Markdown and HTML files, and Jupyter notebooks; all
//! parsing and embedded image reads run on workers. All end as the same
//! semantic blocks, so they read alike, and all keep their source one
//! toggle away.
mod diagram;
mod jupyter;
mod markdown;
mod notebook;
use skwir_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{Held, PreviewProvider, PreviewRequest},
};
use skwir_platform::mime::Types;
use skwir_platform::{inspection, inspection::limits::Bounds, rich_preview};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "markdown"
    }
    fn title(&self) -> &'static str {
        "Markdown, HTML and notebooks"
    }
    fn description(&self) -> &'static str {
        "Read Markdown and HTML files and Jupyter notebooks as documents, with headings, tables, code, formulas, Mermaid diagrams and local images; switch to the source at any time. HTML is inert: no scripts, fonts or remote content"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![
            skwir_platform::diagram::WORKER.requirement("Mermaid diagrams drawn in Markdown"),
            skwir_platform::math::WORKER
                .requirement("Formulas typeset in Markdown, notebooks and HTML"),
        ]
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
/// What reading a document tells us for free: how long it is. What was left
/// out (an encoding assumed, blocks past `bounds`) is the notice.
fn measured(
    mut data: Inspection,
    encoding: inspection::Encoding,
    page: rich_preview::Page,
    bounds: Bounds,
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
        page.truncated.then(|| inspection::blocks_notice(bounds)),
        (!data.notice.is_empty()).then(|| std::mem::take(&mut data.notice)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    data.whole_available |= page.truncated && bounds.cut();
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
        "markdown"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        format_of(req).is_some()
    }
    /// A picture the document named, beside it: the same containment as
    /// when it was named.
    fn picture(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        key: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<skwir_core::thumbs::Image>> {
        let held = req.held()?;
        if notebook::holds(key) && format_of(req) == Some(Format::Notebook) {
            let notebook = notebook::read(&held, Bounds::of(view), cancel)?;
            return Ok(notebook::decoded(&notebook, key));
        }
        let base = held.path().parent().unwrap_or(std::path::Path::new("."));
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
        let held = req.held()?;
        let bounds = Bounds::of(view);
        if format == Format::Notebook {
            return notebook(&held, bounds, cancel).map(Some);
        }
        let (source, encoding) = inspection::read_text(&held, bounds, cancel)?;
        let path = held.path();
        let base = path.parent().unwrap_or(std::path::Path::new("."));
        let image = |href: &str| rich_preview::local_picture(base, href, cancel);
        let (title, page, notice) = if format == Format::Markdown {
            let mut wiki = markdown::Wiki::new(path);
            let page = markdown::parse(&source, bounds, cancel, image, wiki.as_mut())?;
            ("Markdown", page, "")
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
                    bounds,
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
            bounds,
        )))
    }
}

/// A notebook read as a document: its Markdown, its code and outputs, and
/// its pictures beside it, pasted into its cells or drawn by its outputs.
fn notebook(held: &Arc<Held>, bounds: Bounds, cancel: &CancelToken) -> anyhow::Result<Inspection> {
    let value = notebook::read(held, bounds, cancel)?;
    let base = held.path().parent().unwrap_or(std::path::Path::new("."));
    let read = notebook::page(&value, bounds, cancel, |href| {
        rich_preview::local_picture(base, href, cancel)
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
            whole_available: read.shortened > 0 && bounds.cut(),
            ..Inspection::default()
        },
        inspection::Encoding::Utf8,
        read.page,
        bounds,
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
                entry: skwir_core::op::EntryRef::new(skwir_core::FileId::new(1, 1), name),
                mime: None,
                page: 1,
                seconds: None,
                max_side: 64,
                text_size: None,
                fit: None,
                held: skwir_core::preview::HeldSlot::default(),
            })
        };
        assert!(format("a.md") == Some(Format::Markdown));
        assert!(format("a.mkd") == Some(Format::Markdown));
        assert!(format("a.htm") == Some(Format::Html));
        assert!(format("a.xhtml") == Some(Format::Xhtml));
        assert!(format("a.ipynb") == Some(Format::Notebook));
        assert!(format("a.txt").is_none());
    }

    /// `bytes` read as a Markdown file named `name`, from what the preview
    /// holds (ADR-101): its path leads nowhere, so a read of it fails.
    fn read_as(name: &str, bytes: &[u8], view: &InspectRequest) -> Option<Inspection> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone").join(name);
        let request = PreviewRequest::holding(path, bytes, Some("text/markdown"));
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
        assert_eq!(data.notice, inspection::blocks_notice(Bounds::QUICK));
        assert!(data.whole_available, "Full preview shows the rest");
        let whole = InspectRequest {
            whole: true,
            ..InspectRequest::default()
        };
        let data = read_as("CHANGELOG.md", changelog.as_bytes(), &whole).unwrap();
        assert_eq!(data.blocks.len(), 3000);
        assert!(data.notice.is_empty() && !data.whole_available);
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
        // Held for a path that leads nowhere: the notebook and the plot it
        // drew are read from what the preview holds (ADR-101).
        let dir = tempfile::tempdir().unwrap();
        let request = PreviewRequest::holding(
            dir.path().join("gone/a notebook’s name.ipynb"),
            document.to_string().into_bytes(),
            Some("application/x-ipynb+json"),
        );
        let data = notebook(&request.held().unwrap(), Bounds::QUICK, &CancelToken::new()).unwrap();
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
                Some(skwir_core::inspection::Picture::Later {
                    key,
                    width: 1,
                    height: 1,
                }) => Some(key.clone()),
                _ => None,
            })
            .expect("the plot is named for later");
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

    /// A result shown as its HTML still counts among the bundles holding a
    /// picture, so the plot after it is the one named.
    #[test]
    fn a_plot_after_a_result_shown_otherwise_is_named_by_its_place() {
        let one = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let two = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAIAAAB7QOjdAAAAIGNIUk0AAHomAACAhAAA+gAAAIDoAAB1MAAA6mAAADqYAAAXcJy6UTwAAAAGYktHRAD/AP8A/6C9p5MAAAAPSURBVAjXY/zPwMDAwAAABggBAcOaYMUAAAAASUVORK5CYII=";
        let document = serde_json::json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {},
            "cells": [{"cell_type": "code", "source": "show()", "outputs": [
                {"output_type": "display_data",
                 "data": {"text/html": "<p>Shown as HTML</p>", "image/png": one}},
                {"output_type": "display_data", "data": {"image/png": two}}
            ]}]
        });
        let dir = tempfile::tempdir().unwrap();
        let request = PreviewRequest::holding(
            dir.path().join("plots.ipynb"),
            document.to_string().into_bytes(),
            Some("application/x-ipynb+json"),
        );
        let data = notebook(&request.held().unwrap(), Bounds::QUICK, &CancelToken::new()).unwrap();
        assert!(
            data.blocks
                .iter()
                .any(|block| block.text == "Shown as HTML")
        );
        let keys: Vec<_> = data
            .blocks
            .iter()
            .filter_map(|block| match &block.picture {
                Some(skwir_core::inspection::Picture::Later { key, .. }) => Some(key.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(keys.len(), 1);
        let image = Provider
            .picture(
                &request,
                &InspectRequest::default(),
                &keys[0],
                &CancelToken::new(),
            )
            .unwrap()
            .unwrap();
        assert_eq!((image.width, image.height), (2, 1), "the second plot");
    }

    /// The math Jupyter shows, in cells and in what sympy prints, drawn where
    /// the helper is built; its TeX, as code, where it is not.
    #[test]
    fn a_notebook_s_formulas_are_jupyter_s() {
        let document = serde_json::json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {},
            "cells": [
                {"cell_type": "markdown",
                 "source": "Let \\\\(a\\\\) and $b$; `$c$` stays.\n\n\\begin{equation}x = 1\\end{equation}"},
                {"cell_type": "code", "source": "integrate(x**2)", "outputs": [
                    {"output_type": "execute_result",
                     "data": {"text/latex": "$\\displaystyle \\frac{x^{3}}{3}$", "text/plain": "x**3/3"}}
                ]}
            ]
        });
        let dir = tempfile::tempdir().unwrap();
        let request = PreviewRequest::holding(
            dir.path().join("maths.ipynb"),
            document.to_string().into_bytes(),
            Some("application/x-ipynb+json"),
        );
        let read =
            || notebook(&request.held().unwrap(), Bounds::QUICK, &CancelToken::new()).unwrap();
        let said = |data: &Inspection| -> Vec<String> {
            data.blocks
                .iter()
                .filter(|block| !block.code || block.language.as_deref() == Some("latex"))
                .map(|block| block.said(0..block.text.len()).into_owned())
                .collect()
        };
        let worker = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.parent()?.join("skwir-math-worker")))
            .filter(|path| path.is_file());
        if let Some(worker) = &worker {
            // SAFETY: no other thread of this test process reads the
            // environment; nextest runs each test in a process of its own.
            #[allow(unsafe_code)]
            unsafe {
                std::env::set_var("SKWIR_MATH_WORKER", worker);
            }
            let data = read();
            let drawn = data
                .blocks
                .iter()
                .flat_map(|block| &block.runs)
                .filter(|run| run.drawing.is_some())
                .count();
            assert_eq!(drawn, 4, "a, b, the equation and the result");
            assert_eq!(
                said(&data),
                [
                    "Let $a$ and $b$; $c$ stays.",
                    "$$\\begin{equation}x = 1\\end{equation}$$",
                    "$\\displaystyle \\frac{x^{3}}{3}$",
                ]
            );
        } else {
            eprintln!("drawn: skipped, skwir-math-worker is not built");
        }
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("SKWIR_MATH_WORKER", "/nonexistent/skwir-math-worker");
        }
        let data = read();
        assert_eq!(
            said(&data),
            [
                "Let $a$ and $b$; $c$ stays.",
                "\\begin{equation}x = 1\\end{equation}",
                "$\\displaystyle \\frac{x^{3}}{3}$",
            ],
            "the TeX, as code, where nothing draws it"
        );
    }

    #[test]
    fn a_notebook_shows_html_results_and_pictures_pasted_into_cells() {
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        // A table as pandas writes one: a stylesheet with a blank line in
        // it, which would end an HTML block written into Markdown.
        let frame = "<div>\n<style scoped>\n    .dataframe tbody tr th {\n        vertical-align: top;\n    }\n\n    .dataframe thead th {\n        text-align: right;\n    }\n</style>\n<table border=\"1\" class=\"dataframe\">\n  <thead>\n    <tr style=\"text-align: right;\">\n      <th></th>\n      <th>café</th>\n    </tr>\n  </thead>\n  <tbody>\n    <tr>\n      <th>0</th>\n      <td>42</td>\n    </tr>\n  </tbody>\n</table>\n</div>";
        let document = serde_json::json!({
            "nbformat": 4,
            "nbformat_minor": 5,
            "metadata": {"language_info": {"name": "python"}},
            "cells": [
                {"cell_type": "markdown", "source": "# Results\n\n![a shot](attachment:shot%20one.png)\n\n![](attachment:gone.png)",
                 "attachments": {"shot one.png": {"image/png": png}}},
                // A fence this cell leaves open never takes the next.
                {"cell_type": "markdown", "source": "```\nleft open"},
                {"cell_type": "markdown", "source": "# Results"},
                {"cell_type": "code", "source": "frame",
                 "outputs": [
                    {"output_type": "execute_result",
                     "data": {"text/html": frame, "text/plain": "   café\n0    42"}},
                    // Drawn by a script: nothing inert, its text stands for it.
                    {"output_type": "display_data",
                     "data": {"text/html": "<div id=\"chart\"></div><script>draw()</script>",
                              "text/plain": "<Chart>"}}
                 ]}
            ]
        });
        let dir = tempfile::tempdir().unwrap();
        let request = PreviewRequest::holding(
            dir.path().join("results.ipynb"),
            document.to_string().into_bytes(),
            Some("application/x-ipynb+json"),
        );
        let data = notebook(&request.held().unwrap(), Bounds::QUICK, &CancelToken::new()).unwrap();
        let headings: Vec<_> = data
            .blocks
            .iter()
            .filter(|block| block.heading > 0)
            .map(|block| (block.text.as_str(), block.anchors.clone()))
            .collect();
        assert_eq!(
            headings,
            [
                ("Results", vec!["results".to_owned()]),
                ("Results", vec!["results-1".to_owned()]),
            ],
            "each cell is read apart, its targets unique in the page"
        );
        let rows: Vec<_> = data
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                skwir_core::inspection::BlockKind::TableRow { header, .. } => {
                    Some((*header, block.text.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows[0].0 && rows[0].1.contains("café"), "{rows:?}");
        assert!(rows[1].1.contains("42"), "{rows:?}");
        let code: Vec<_> = data
            .blocks
            .iter()
            .filter(|block| block.code)
            .map(|block| (block.language.as_deref(), block.text.as_str()))
            .collect();
        assert_eq!(
            code,
            [
                (None, "left open"),
                (Some("python"), "frame"),
                (Some("output"), "<Chart>"),
            ],
            "a table shown is not printed again; the stylesheet is not text"
        );
        let pictures: Vec<_> = data
            .blocks
            .iter()
            .filter_map(|block| match &block.picture {
                Some(skwir_core::inspection::Picture::Later { key, .. }) => Some(key.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            pictures.len(),
            1,
            "an attachment the cell lacks shows nothing"
        );
        let image = Provider
            .picture(
                &request,
                &InspectRequest::default(),
                &pictures[0],
                &CancelToken::new(),
            )
            .unwrap()
            .expect("decoded from the notebook when seen");
        assert_eq!((image.width, image.height), (1, 1));
        assert!(
            data.blocks.iter().any(|block| block.alt == "a shot"),
            "the picture keeps its description"
        );
    }
}
