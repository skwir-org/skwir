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
        app.previews.register(
            self.id(),
            Arc::new(DocumentPreviews::new(app.cache.namespace(self.id()))),
        );
    }
}
