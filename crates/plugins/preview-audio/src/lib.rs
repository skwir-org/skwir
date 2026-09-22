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
    fn description(&self) -> &'static str {
        "Play audio files with their cover and a spectrum"
    }
    fn on_settings_changed(&self, app: &App, _keys: &[String]) {
        app.previews.unregister_owner(self.id());
        self.register(app);
    }
    fn register(&self, app: &App) {
        app.settings.declare(bibli_core::settings::SettingSpec::choice(
            "preview-audio.replay_gain",
            "Even out loudness",
            "Use the ReplayGain values in a track's tags so that songs mastered at different levels sound equally loud. Per album keeps an album's own quiet and loud songs as they are. Tracks without the tags play unchanged; it applies from the next track.",
            "off",
            &[("off", "Off"), ("track", "Per track"), ("album", "Per album")],
        ));
        let replay_gain = bibli_platform::media::ReplayGain::from_setting(
            &app.settings
                .get::<String>("preview-audio.replay_gain")
                .unwrap_or_default(),
        );
        app.previews.register(
            self.id(),
            Arc::new(
                bibli_platform::preview::MediaPreviews::new(app.cache.namespace(self.id()))
                    .audio()
                    .with_backend("gstreamer".into())
                    .with_replay_gain(replay_gain),
            ),
        );
    }
}
