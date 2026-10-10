//! Settings changes refresh providers without disabling the owning plugin.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::{App, app::AppConfig, command::Invocation, settings::SaveState};

#[test]
fn changing_document_engine_refreshes_only_document_previews() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    app.plugins.add(
        Arc::new(skwir_plugin_preview_documents::PreviewPlugin::default()),
        &app,
    );
    app.plugins.add(
        Arc::new(skwir_plugin_preview_media::PreviewPlugin::default()),
        &app,
    );
    let revision = app.previews.revision();
    let providers = app.previews.ids();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_waker(Some(Arc::new(move || {
        let _ = tx.send(());
    })));
    app.dispatch(&Invocation::with_args(
        "settings.apply",
        &BTreeMap::from([(
            "changes",
            BTreeMap::from([("preview-documents.backend", "poppler")]),
        )]),
    ))
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match app.settings.save_status().1 {
            SaveState::Saved => break,
            SaveState::Failed(error) => panic!("{error}"),
            _ => {
                rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    .unwrap();
            }
        }
    }
    assert!(app.previews.revision() > revision);
    let mut after = app.previews.ids();
    let mut before = providers;
    after.sort_unstable();
    before.sort_unstable();
    assert_eq!(after, before);
    assert!(app.plugins.is_enabled("preview-documents"));
    assert!(app.plugins.is_enabled("preview-media"));
    assert_eq!(
        app.settings
            .get::<String>("preview-documents.backend")
            .as_deref(),
        Some("poppler")
    );
}

/// The shell's player reads preferences the audio and video plugin
/// declares (ADR-016): the two agree on their names, kinds, defaults and
/// choices, or the player would follow a key nobody can set.
#[test]
fn the_player_reads_what_the_media_plugin_declares() {
    use skwir_core::settings::SettingKind;
    use skwir_ui::settings::{
        AUTOPLAY_DEFAULT, AUTOPLAY_KEY, CONTINUE_CHOICES, CONTINUE_KEY, KEEP_PLAYING_DEFAULT,
        KEEP_PLAYING_KEY,
    };
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    app.plugins.add(
        Arc::new(skwir_plugin_preview_media::PreviewPlugin::default()),
        &app,
    );
    let spec = |key: &str| {
        app.settings
            .spec(key)
            .unwrap_or_else(|| panic!("{key} is not declared"))
    };
    for (key, default) in [
        (KEEP_PLAYING_KEY, KEEP_PLAYING_DEFAULT),
        (AUTOPLAY_KEY, AUTOPLAY_DEFAULT),
    ] {
        let spec = spec(key);
        assert_eq!(spec.kind, SettingKind::Toggle, "{key}");
        assert_eq!(spec.default, toml::Value::Boolean(default), "{key}");
    }
    let spec = spec(CONTINUE_KEY);
    assert_eq!(spec.kind, SettingKind::Choice);
    assert_eq!(spec.constraints.choices, CONTINUE_CHOICES);
    assert_eq!(
        spec.default,
        toml::Value::String(CONTINUE_CHOICES[0].into())
    );
    assert!(
        app.plugins.set_enabled("preview-media", false, &app),
        "the plugin switches off"
    );
    assert!(
        app.settings.spec(CONTINUE_KEY).is_none(),
        "and its preferences go with it"
    );
}
