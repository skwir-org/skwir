//! Optional audio playback, cover and spectrum previews.
use bibli_core::{App, plugin::Plugin};
use bibli_platform::{media::ReplayGain, preview::SharedTuning};
use std::sync::Arc;

const REPLAY_GAIN: &str = "preview-audio.replay_gain";

/// The tuning its provider reads when a track opens. A changed setting is
/// written here rather than re-registering the provider, which would cancel
/// every session it opened: the track playing, the one pinned in the dock.
#[derive(Default)]
pub struct PreviewPlugin {
    tuning: SharedTuning,
}
impl PreviewPlugin {
    fn read_settings(&self, app: &App) {
        self.tuning.lock().replay_gain =
            ReplayGain::from_setting(&app.settings.get::<String>(REPLAY_GAIN).unwrap_or_default());
    }
}
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
        self.read_settings(app);
    }
    fn register(&self, app: &App) {
        app.settings.declare(bibli_core::settings::SettingSpec::choice(
            REPLAY_GAIN,
            "Even out loudness",
            "Use the ReplayGain values in a track's tags so that songs mastered at different levels sound equally loud. Per album keeps an album's own quiet and loud songs as they are. Tracks without the tags play unchanged; it applies from the next track.",
            "off",
            &[("off", "Off"), ("track", "Per track"), ("album", "Per album")],
        ));
        self.read_settings(app);
        app.previews.register(
            self.id(),
            Arc::new(
                bibli_platform::preview::MediaPreviews::new(app.cache.namespace(self.id()))
                    .audio()
                    .with_backend("gstreamer".into())
                    .with_tuning(Arc::clone(&self.tuning)),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;

    #[test]
    fn a_new_loudness_setting_keeps_the_provider_and_reaches_the_next_track() {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(dir.path().join("state"))).unwrap();
        let plugin = Arc::new(PreviewPlugin::default());
        app.plugins.add(plugin.clone(), &app);
        let revision = app.previews.revision();
        app.settings
            .set(REPLAY_GAIN, toml::Value::String("album".into()));
        plugin.on_settings_changed(&app, &[REPLAY_GAIN.into()]);
        assert_eq!(
            app.previews.revision(),
            revision,
            "the provider, and the sessions it opened, stay"
        );
        assert_eq!(plugin.tuning.lock().replay_gain, ReplayGain::Album);
    }
}
