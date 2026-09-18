//! Optional documents previews, registered independently of the UI toolkit.

use bibli_core::{App, plugin::Plugin};
use bibli_platform::preview::DocumentPreviews;
use std::sync::Arc;

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-documents"
    }
    fn title(&self) -> &'static str {
        "Preview: documents"
    }
    fn register(&self, app: &App) {
        for (name, description, default) in [
            (
                "backend",
                "Document engine: auto (PDFium with Poppler fallback), pdfium, or poppler",
                "auto",
            ),
            (
                "pdfium_library",
                "Optional path to the PDFium shared library",
                "",
            ),
        ] {
            app.settings.declare(bibli_core::settings::SettingSpec {
                key: format!("{}.{}", self.id(), name),
                description: description.into(),
                default: toml::Value::String(default.into()),
            });
        }
        let backend = app
            .settings
            .get::<String>("preview-documents.backend")
            .unwrap_or_else(|| "auto".into());
        let library = app
            .settings
            .get::<String>("preview-documents.pdfium_library")
            .filter(|s| !s.is_empty())
            .map(Into::into);
        app.previews.register(
            self.id(),
            Arc::new(
                DocumentPreviews::new(app.cache.namespace(self.id())).with_reader(backend, library),
            ),
        );
    }
}
