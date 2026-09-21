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
        let path = req.entry.resolve()?;
        if view.source {
            return Ok(Some(inspection::source(&path, view.offset, cancel)?));
        }
        let source = inspection::text(&path, cancel)?;
        let base = path.parent().unwrap_or(std::path::Path::new("."));
        if !matches!(
            inspection::extension(&path).as_str(),
            "md" | "markdown" | "mdown"
        ) {
            return Ok(Some(Inspection {
                kind: InspectionKind::Markdown,
                title: "HTML".into(),
                blocks: rich_preview::html(&source, cancel, |href| {
                    rich_preview::local_image(base, href, cancel)
                })?,
                source_available: true,
                notice: "Reading mode · scripts, styles and remote content are not loaded".into(),
                ..Inspection::default()
            }));
        }
        Ok(Some(Inspection {
            kind: InspectionKind::Markdown,
            title: "Markdown".into(),
            blocks: markdown::parse(&source, cancel, |href| {
                rich_preview::local_image(base, href, cancel)
            })?,
            source_available: true,
            ..Inspection::default()
        }))
    }
}
