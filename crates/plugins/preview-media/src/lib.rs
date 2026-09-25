//! Audio and video in the preview panel: one plugin, because both play in
//! the same player (the `GStreamer` worker, the panel's controls, the pin and
//! its dock), and its preferences are the player's, whatever plays.
//!
//! It registers two providers, one for audio (cover, spectrum, loudness)
//! and one for video (playback, or a still frame from `FFmpeg`), and declares
//! every preference of the player. The shell reads the player's own
//! (`KEEP_PLAYING`, `AUTOPLAY`, `CONTINUE`); with the plugin off there is no
//! player, and nothing reads them.

use bibli_core::{App, plugin::Plugin, settings::SettingSpec};
use bibli_platform::{
    media::ReplayGain,
    preview::{MediaPreviews, SharedTuning, VIDEO_PROVIDER},
};
use std::sync::Arc;

const ID: &str = "preview-media";
pub const BACKEND: &str = "preview-media.backend";
pub const REPLAY_GAIN: &str = "preview-media.replay_gain";
pub const KEEP_PLAYING: &str = "preview-media.keep_playing";
pub const AUTOPLAY: &str = "preview-media.autoplay";
pub const CONTINUE: &str = "preview-media.continue";

/// The audio provider's tuning, read when a track opens. A changed setting
/// is written here rather than re-registering the provider, which would
/// cancel every session it opened: the track playing, the one pinned.
#[derive(Default)]
pub struct PreviewPlugin {
    tuning: SharedTuning,
}

impl PreviewPlugin {
    fn read_tuning(&self, app: &App) {
        self.tuning.lock().replay_gain =
            ReplayGain::from_setting(&app.settings.get::<String>(REPLAY_GAIN).unwrap_or_default());
    }

    /// The video provider, with the engine the settings name.
    fn register_video(app: &App) {
        let backend = app
            .settings
            .get::<String>(BACKEND)
            .unwrap_or_else(|| "auto".into());
        app.previews.register(
            ID,
            Arc::new(MediaPreviews::new(app.cache.namespace(ID)).with_backend(backend)),
        );
    }
}

fn declare(app: &App) {
    // Where they were before audio and video were one plugin (ADR-016).
    for (old, new) in [
        ("preview-audio.replay_gain", REPLAY_GAIN),
        ("ui.media_keep_playing", KEEP_PLAYING),
        ("ui.media_autoplay", AUTOPLAY),
        ("ui.media_continue", CONTINUE),
    ] {
        app.settings.rename(old, new);
    }
    app.settings.declare(
        SettingSpec::toggle(
            KEEP_PLAYING,
            "Keep playing while you browse",
            "What you play goes on when you select another file, in a player at the foot of the preview. Off, it stops when the selection moves.",
            true,
        )
        .section("Playback"),
    );
    app.settings.declare(
        SettingSpec::toggle(
            AUTOPLAY,
            "Play on selection",
            "Start audio and video files as soon as they are selected, instead of waiting for Play.",
            false,
        )
        .section("Playback"),
    );
    app.settings.declare(
        SettingSpec::choice(
            CONTINUE,
            "When a track ends",
            "Stop, go on with the next file of the same kind in the folder (or the playlist), or with one of them at random.",
            "off",
            &[("off", "Stop"), ("next", "Play next"), ("shuffle", "Shuffle")],
        )
        .section("Playback"),
    );
    app.settings.declare(
        SettingSpec::choice(
            REPLAY_GAIN,
            "Even out loudness",
            "Use the ReplayGain values in a track's tags so that songs mastered at different levels sound equally loud. Per album keeps an album's own quiet and loud songs as they are. Tracks without the tags play unchanged; it applies from the next track.",
            "off",
            &[("off", "Off"), ("track", "Per track"), ("album", "Per album")],
        )
        .section("Audio"),
    );
    app.settings.declare(
        SettingSpec::choice(
            BACKEND,
            "Video engine",
            "Automatic plays with GStreamer and falls back to a still frame from FFmpeg. Playback needs the media worker and GStreamer plugins.",
            "auto",
            &[
                ("auto", "Automatic"),
                ("gstreamer", "GStreamer"),
                ("stills", "Still frames"),
            ],
        )
        .section("Video"),
    );
}

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Preview: audio and video"
    }
    fn description(&self) -> &'static str {
        "Play audio and video files in the preview panel: audio with its cover and a spectrum, video with a still frame when playback is unavailable"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Previews
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key == REPLAY_GAIN) {
            self.read_tuning(app);
        }
        // The engine is the video provider's: audio playing goes on.
        if keys.iter().any(|key| key == BACKEND) {
            app.previews.unregister(ID, VIDEO_PROVIDER);
            Self::register_video(app);
        }
    }
    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        use bibli_core::plugin::Requirement;
        vec![
            Requirement::new("bibli-media-worker", "Audio and video playback", || {
                bibli_platform::media::worker_path().is_some()
            }),
            Requirement::program("ffmpeg", "Still frames when playback is unavailable"),
        ]
    }
    fn register(&self, app: &App) {
        declare(app);
        self.read_tuning(app);
        app.previews.register(
            ID,
            Arc::new(
                MediaPreviews::new(app.cache.namespace(ID))
                    .audio()
                    .with_backend("gstreamer".into())
                    .with_tuning(Arc::clone(&self.tuning)),
            ),
        );
        Self::register_video(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use bibli_platform::preview::AUDIO_PROVIDER;

    fn app() -> (tempfile::TempDir, Arc<App>, Arc<PreviewPlugin>) {
        let dir = tempfile::tempdir().unwrap();
        let app = App::new(AppConfig::ephemeral(dir.path().join("state"))).unwrap();
        let plugin = Arc::new(PreviewPlugin::default());
        app.plugins.add(plugin.clone(), &app);
        (dir, app, plugin)
    }

    #[test]
    fn a_new_loudness_setting_keeps_the_providers_and_reaches_the_next_track() {
        let (_dir, app, plugin) = app();
        let revision = app.previews.revision();
        app.settings
            .set(REPLAY_GAIN, toml::Value::String("album".into()));
        plugin.on_settings_changed(&app, &[REPLAY_GAIN.into()]);
        assert_eq!(
            app.previews.revision(),
            revision,
            "the providers, and the sessions they opened, stay"
        );
        assert_eq!(plugin.tuning.lock().replay_gain, ReplayGain::Album);
    }

    #[test]
    fn a_new_video_engine_leaves_the_audio_provider_alone() {
        let (_dir, app, plugin) = app();
        let before = app.previews.ids();
        assert!(before.contains(&AUDIO_PROVIDER) && before.contains(&VIDEO_PROVIDER));
        app.settings
            .set(BACKEND, toml::Value::String("stills".into()));
        plugin.on_settings_changed(&app, &[BACKEND.into()]);
        let mut after = app.previews.ids();
        after.sort_unstable();
        let mut expected = before;
        expected.sort_unstable();
        assert_eq!(
            after, expected,
            "one video provider again, beside the audio one"
        );
    }

    #[test]
    fn a_player_preference_refreshes_no_provider() {
        let (_dir, app, plugin) = app();
        let revision = app.previews.revision();
        plugin.on_settings_changed(&app, &[KEEP_PLAYING.into(), CONTINUE.into()]);
        assert_eq!(app.previews.revision(), revision);
    }
}
