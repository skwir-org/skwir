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
    fn description(&self) -> &'static str {
        "Read PDF and office documents: continuous scrolling, search, text selection, links and contents"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn on_settings_changed(&self, app: &App, _keys: &[String]) {
        app.previews.unregister_owner(self.id());
        self.register(app);
    }

    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        use bibli_core::plugin::Requirement;
        use bibli_platform::document::{Engine, worker_path};
        vec![
            Requirement::new("bibli-pdf-worker", "PDF reading with PDFium", || {
                worker_path(Engine::Pdfium).is_some()
            }),
            Requirement::new("bibli-poppler-worker", "PDF reading with Poppler", || {
                worker_path(Engine::Poppler).is_some()
            }),
            Requirement::new("bibli-mupdf-worker", "PDF reading with MuPDF", || {
                worker_path(Engine::Mupdf).is_some()
            }),
        ]
    }

    fn register(&self, app: &App) {
        use bibli_core::settings::SettingSpec;
        app.settings.declare(SettingSpec::choice(
            "preview-documents.backend",
            "Document engine",
            "Automatic prefers PDFium and falls back to Poppler; MuPDF is used only when chosen.",
            "auto",
            &[
                ("auto", "Automatic"),
                ("pdfium", "PDFium"),
                ("poppler", "Poppler"),
                ("mupdf", "MuPDF"),
            ],
        ));
        app.settings.declare(SettingSpec::path(
            "preview-documents.pdfium_library",
            "PDFium library",
            "Path to the PDFium shared library. Leave empty to search the usual locations.",
            "",
        ));
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
