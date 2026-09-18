//! Preference transactions use synthetic settings and isolated files.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use bibli_core::{
    App,
    app::AppConfig,
    command::{Invocation, Outcome},
    plugin::Plugin,
    settings::{SaveState, SettingConstraints, SettingSpec, Settings},
};

struct Preferences(AtomicUsize);
impl Plugin for Preferences {
    fn id(&self) -> &'static str {
        "sample"
    }
    fn title(&self) -> &'static str {
        "Sample"
    }
    fn register(&self, app: &App) {
        for (key, default, constraints) in [
            (
                "engine",
                toml::Value::String("first".into()),
                SettingConstraints {
                    choices: vec!["first".into(), "second".into()],
                    integer_range: None,
                },
            ),
            (
                "budget",
                toml::Value::Integer(64),
                SettingConstraints {
                    choices: vec![],
                    integer_range: Some((4, 4096)),
                },
            ),
            (
                "enabled",
                toml::Value::Boolean(true),
                SettingConstraints::default(),
            ),
            (
                "path",
                toml::Value::String(String::new()),
                SettingConstraints::default(),
            ),
        ] {
            app.settings.declare(SettingSpec {
                key: format!("sample.{key}"),
                description: key.into(),
                default,
                constraints,
            });
        }
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        assert!(keys.iter().all(|key| key.starts_with("sample.")));
        // Re-entry into the schema and values must not deadlock.
        assert!(!app.settings.schema().is_empty());
        assert!(app.settings.get::<String>("sample.engine").is_some());
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn fixture(path: Option<std::path::PathBuf>) -> (tempfile::TempDir, Arc<App>, Arc<Preferences>) {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::ephemeral(tmp.path());
    config.settings_path = path;
    let app = App::new(config).unwrap();
    let plugin = Arc::new(Preferences(AtomicUsize::new(0)));
    app.plugins.add(plugin.clone(), &app);
    (tmp, app, plugin)
}

fn apply(app: &App, changes: &serde_json::Value) -> SaveState {
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_waker(Some(Arc::new(move || {
        let _ = tx.send(());
    })));
    let Outcome::Value(id) = app
        .dispatch(&Invocation::with_args(
            "settings.apply",
            &serde_json::json!({"changes": changes}),
        ))
        .unwrap()
    else {
        panic!("expected request id")
    };
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (finished, state) = app.settings.save_status();
    assert_eq!(Some(finished), id.as_u64());
    state
}

#[test]
fn command_saves_typed_values_preserves_unrelated_data_and_notifies_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[unrelated]\nname = 'keep'\n").unwrap();
    let (_tmp, app, plugin) = fixture(Some(path.clone()));
    // Simulate another command changing application data while the dialog is open.
    app.settings.set("query.saved", toml::Value::Array(vec![]));
    let changes = serde_json::json!({ "sample.engine": "second", "sample.enabled": "false", "sample.budget": "128", "sample.path": "/example/reader's library.so" });
    assert_eq!(apply(&app, &changes), SaveState::Saved);
    let loaded = Settings::load(&path).unwrap();
    assert_eq!(
        loaded.get::<String>("sample.path").unwrap(),
        "/example/reader's library.so"
    );
    assert_eq!(loaded.get::<i64>("sample.budget"), Some(128));
    assert_eq!(loaded.get::<bool>("sample.enabled"), Some(false));
    assert_eq!(
        loaded.get::<String>("unrelated.name").as_deref(),
        Some("keep")
    );
    assert_eq!(loaded.get::<Vec<String>>("query.saved"), Some(vec![]));
    assert_eq!(plugin.0.load(Ordering::SeqCst), 1);
    assert_eq!(apply(&app, &changes), SaveState::Saved);
    assert_eq!(
        plugin.0.load(Ordering::SeqCst),
        1,
        "no hook for unchanged settings"
    );
}

#[test]
fn validation_rejects_unknown_keys_wrong_types_choices_and_ranges_as_a_batch() {
    let (_tmp, app, plugin) = fixture(None);
    for bad in [
        serde_json::json!({"unknown.key":"value"}),
        serde_json::json!({"sample.engine":"third"}),
        serde_json::json!({"sample.enabled":"yes"}),
        serde_json::json!({"sample.budget":"3"}),
        serde_json::json!({"sample.budget":"4097"}),
        serde_json::json!({"sample.budget":"oops", "sample.engine":"second"}),
    ] {
        assert!(
            app.dispatch(&Invocation::with_args(
                "settings.apply",
                &serde_json::json!({"changes":bad})
            ))
            .is_err()
        );
    }
    assert_eq!(
        app.settings.get::<String>("sample.engine").as_deref(),
        Some("first")
    );
    assert_eq!(plugin.0.load(Ordering::SeqCst), 0);
}

#[test]
fn failed_persistence_keeps_live_values_and_allows_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let (_tmp, app, plugin) = fixture(Some(path.clone()));
    // A directory at the destination makes atomic replacement fail even as root.
    std::fs::create_dir(&path).unwrap();
    assert!(matches!(
        apply(&app, &serde_json::json!({"sample.engine":"second"})),
        SaveState::Failed(_)
    ));
    assert_eq!(
        app.settings.get::<String>("sample.engine").as_deref(),
        Some("first")
    );
    assert_eq!(plugin.0.load(Ordering::SeqCst), 0);
    std::fs::remove_dir(&path).unwrap();
    assert_eq!(
        apply(&app, &serde_json::json!({"sample.engine":"second"})),
        SaveState::Saved
    );
}

#[test]
fn malformed_parent_is_reported_without_panicking_or_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "sample = 7\n").unwrap();
    let (_tmp, app, plugin) = fixture(Some(path.clone()));
    assert!(matches!(
        apply(&app, &serde_json::json!({"sample.engine":"second"})),
        SaveState::Failed(_)
    ));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "sample = 7\n");
    assert_eq!(plugin.0.load(Ordering::SeqCst), 0);
}

#[test]
fn disabled_plugins_cannot_be_edited_or_resurrected_by_a_settings_hook() {
    let (_tmp, app, plugin) = fixture(None);
    assert!(app.plugins.set_enabled("sample", false, &app));
    assert!(
        app.dispatch(&Invocation::with_args(
            "settings.apply",
            &serde_json::json!({"changes":{"sample.engine":"second"}})
        ))
        .is_err()
    );
    app.plugins
        .settings_changed(&["sample.engine".into()], &app);
    assert_eq!(plugin.0.load(Ordering::SeqCst), 0);
    assert!(app.settings.schema().is_empty());
}

#[test]
fn queued_save_is_exclusive_and_revalidates_after_plugin_disable() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::ephemeral(tmp.path());
    config.workers = 1;
    let app = App::new(config).unwrap();
    let plugin = Arc::new(Preferences(AtomicUsize::new(0)));
    app.plugins.add(plugin.clone(), &app);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    app.scheduler.spawn(
        bibli_core::scheduler::Priority::Visible,
        "test-blocker",
        app.lifetime().child(),
        move || {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        },
    );
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (wake_tx, wake_rx) = std::sync::mpsc::channel();
    app.set_waker(Some(Arc::new(move || {
        let _ = wake_tx.send(());
    })));
    let command = Invocation::with_args(
        "settings.apply",
        &serde_json::json!({"changes":{"sample.engine":"second"}}),
    );
    app.dispatch(&command).unwrap();
    assert!(
        app.dispatch(&command).is_err(),
        "only one save may be in progress"
    );
    assert!(app.plugins.set_enabled("sample", false, &app));
    release_tx.send(()).unwrap();
    wake_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(app.settings.save_status().1, SaveState::Failed(_)));
    assert_eq!(plugin.0.load(Ordering::SeqCst), 0);
    assert!(app.settings.schema().is_empty());
    assert_eq!(app.settings.get::<String>("sample.engine"), None);
}
