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
    fn on_settings_changed(&self, app: &App, _keys: &[String]) {
        app.previews.unregister_owner(self.id());
        self.register(app);
    }
    fn register(&self, app: &App) {
        app.settings.declare(bibli_core::settings::SettingSpec {
            key: "preview-media.backend".into(),
            description: "Video engine: auto (GStreamer player with FFmpeg still fallback), gstreamer, or stills. Playback requires bibli-media-worker and GStreamer plugins.".into(),
            default: toml::Value::String("auto".into()),
            constraints: bibli_core::settings::SettingConstraints {
                choices: ["auto", "gstreamer", "stills"].map(str::to_owned).to_vec(),
                integer_range: None,
            },
        });
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
