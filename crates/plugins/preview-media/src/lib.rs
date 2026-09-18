//! Optional media previews, registered independently of the UI toolkit.

use bibli_core::{App, plugin::Plugin};
use bibli_platform::preview::MediaPreviews;
use std::sync::Arc;

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-media"
    }
    fn title(&self) -> &'static str {
        "Preview: media"
    }
    fn register(&self, app: &App) {
        app.previews.register(
            self.id(),
            Arc::new(MediaPreviews::new(app.cache.namespace(self.id()))),
        );
    }
}
