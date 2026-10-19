//! Settings changes refresh providers without disabling the owning plugin.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::{
    App,
    app::AppConfig,
    command::Invocation,
    settings::{SaveState, SettingSpec},
};

#[test]
fn changing_document_engine_refreshes_only_document_previews() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    app.plugins.add(
        Arc::new(skwir_plugin_documents::PreviewPlugin::default()),
        &app,
    );
    app.plugins
        .add(Arc::new(skwir_plugin_media::PreviewPlugin::default()), &app);
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
            BTreeMap::from([("documents.backend", "poppler")]),
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
    assert!(app.plugins.is_enabled("documents"));
    assert!(app.plugins.is_enabled("media"));
    assert_eq!(
        app.settings.get::<String>("documents.backend").as_deref(),
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
    app.plugins
        .add(Arc::new(skwir_plugin_media::PreviewPlugin::default()), &app);
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
        app.plugins.set_enabled("media", false, &app),
        "the plugin switches off"
    );
    assert!(
        app.settings.spec(CONTINUE_KEY).is_none(),
        "and its preferences go with it"
    );
}

/// Every page of the dialog reads alike (`docs/invariants/settings.md`):
/// a title in sentence case that is not its plugin's name, a description
/// in sentences. Every plugin on, and the shell's own preferences.
#[test]
fn every_setting_has_a_title_and_a_description_in_sentences() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    for plugin in skwir::plugins() {
        let id = plugin.id();
        app.plugins.add(plugin, &app);
        if !app.plugins.is_enabled(id) {
            assert!(app.plugins.set_enabled(id, true, &app), "{id}: toggle on");
        }
    }
    skwir_ui::settings::declare(&app);
    let plugins = app.plugins.list();
    let mut problems = Vec::new();
    for spec in app
        .settings
        .schema()
        .into_iter()
        .filter(SettingSpec::editable)
    {
        let key = &spec.key;
        if !spec.title.starts_with(char::is_uppercase) {
            problems.push(format!("{key}: its title starts in lowercase"));
        }
        if plugins.iter().any(|plugin| {
            plugin.id == spec.owner() && plugin.title.eq_ignore_ascii_case(&spec.title)
        }) {
            problems.push(format!("{key}: its title is its plugin's name"));
        }
        if !spec.description.ends_with('.') {
            problems.push(format!("{key}: its description does not end a sentence"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Settings are shown by subject (ADR-072): every setting of skwir's own
/// plugins names one of the dialog's pages, so none keeps a page of its
/// own, and the Tags plugin's settings stand with where notes are kept.
#[test]
fn every_setting_is_on_the_page_of_its_subject() {
    use skwir_ui::settings::Page;
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    for plugin in skwir::plugins() {
        let id = plugin.id();
        app.plugins.add(plugin, &app);
        if !app.plugins.is_enabled(id) {
            assert!(app.plugins.set_enabled(id, true, &app), "{id}: toggle on");
        }
    }
    skwir_ui::settings::declare(&app);
    let schema: Vec<SettingSpec> = app
        .settings
        .schema()
        .into_iter()
        .filter(SettingSpec::editable)
        .collect();
    let own: Vec<&str> = schema
        .iter()
        .filter(|spec| matches!(Page::of_spec(spec), Page::Plugin(_)))
        .map(|spec| spec.key.as_str())
        .collect();
    assert!(own.is_empty(), "on a plugin's own page: {own:?}");
    let page = |key: &str| {
        Page::of_spec(
            schema
                .iter()
                .find(|spec| spec.key == key)
                .unwrap_or_else(|| panic!("{key} is not declared")),
        )
    };
    assert_eq!(page("tags.sidebar"), page("notes.xdg.write"));
    assert_eq!(page("core.preserve_attributes_on_copy"), Page::Notes);
    assert_eq!(page("terminal.program"), page("shell.program"));
    assert_eq!(page("documents.backend"), page("reading.continuous"));
}
