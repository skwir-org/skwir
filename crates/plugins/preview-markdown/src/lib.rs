//! Markdown reading mode; all parsing and embedded image reads run on workers.
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
        "Preview: Markdown"
    }
    fn description(&self) -> &'static str {
        "Render Markdown with headings, tables and code; switch to the source at any time"
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
            "md" | "markdown" | "mdown"
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
