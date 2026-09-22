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
    fn description(&self) -> &'static str {
        "Play videos in the preview panel, or show a still frame when playback is unavailable"
    }
    fn on_settings_changed(&self, app: &App, _keys: &[String]) {
        app.previews.unregister_owner(self.id());
        self.register(app);
    }
    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        use bibli_core::plugin::Requirement;
        vec![
            Requirement::new("bibli-media-worker", "Video and audio playback", || {
                bibli_platform::media::worker_path().is_some()
            }),
            Requirement::program("ffmpeg", "Still frames when playback is unavailable"),
        ]
    }
    fn register(&self, app: &App) {
        app.settings.declare(bibli_core::settings::SettingSpec::choice(
            "preview-media.backend",
            "Video engine",
            "Automatic plays with GStreamer and falls back to a still frame from FFmpeg. Playback needs the media worker and GStreamer plugins.",
            "auto",
            &[
                ("auto", "Automatic"),
                ("gstreamer", "GStreamer"),
                ("stills", "Still frames"),
            ],
        ));
        let backend = app
            .settings
            .get::<String>("preview-media.backend")
            .unwrap_or_else(|| "auto".into());
        app.previews.register(
            self.id(),
            Arc::new(MediaPreviews::new(app.cache.namespace(self.id())).with_backend(backend)),
        );
    }
}
