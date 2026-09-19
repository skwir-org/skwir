//! Optional audio playback, cover and spectrum previews.
use bibli_core::{App, plugin::Plugin};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-audio"
    }
    fn title(&self) -> &'static str {
        "Preview: audio"
    }
    fn register(&self, app: &App) {
        app.previews.register(
            self.id(),
            Arc::new(
                bibli_platform::preview::MediaPreviews::new(app.cache.namespace(self.id()))
                    .audio()
                    .with_backend("gstreamer".into()),
            ),
        );
    }
}
