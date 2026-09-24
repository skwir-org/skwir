//! Reading mode for Markdown and HTML files; all parsing and embedded image
//! reads run on workers. Both end as the same semantic blocks, so they read
//! alike, and both keep their source one toggle away.
mod markdown;
use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, InspectionKind},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::{inspection, rich_preview};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-markdown"
    }
    fn title(&self) -> &'static str {
        "Preview: Markdown and HTML"
    }
    fn description(&self) -> &'static str {
        "Read Markdown and HTML files as documents, with headings, tables, code and local images; switch to the source at any time. HTML is inert: no scripts, styles or remote content"
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
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-markdown"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        matches!(
            inspection::extension(&req.entry.path).as_str(),
            "md" | "markdown" | "mdown" | "html" | "htm" | "xhtml"
        )
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
        let path = req.entry.resolve()?;
        let (source, encoding) = inspection::read_text(&path, cancel)?;
        let base = path.parent().unwrap_or(std::path::Path::new("."));
        let image = |href: &str| rich_preview::local_image(base, href, cancel);
        let extension = inspection::extension(&path);
        let (title, page, notice) = if matches!(extension.as_str(), "md" | "markdown" | "mdown") {
            ("Markdown", markdown::parse(&source, cancel, image)?, "")
        } else {
            let markup = if extension == "xhtml" {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
