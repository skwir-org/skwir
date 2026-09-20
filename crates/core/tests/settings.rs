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
    settings::{SaveState, SettingSpec, Settings},
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
        for spec in [
            SettingSpec::choice(
                "sample.engine",
                "Engine",
                "engine",
                "first",
                &[("first", "First"), ("second", "Second")],
            ),
            SettingSpec::integer("sample.budget", "Budget", "budget", 64, (4, 4096)),
            SettingSpec::toggle("sample.enabled", "Enabled", "enabled", true),
            SettingSpec::path("sample.path", "Path", "path", ""),
        ] {
            app.settings.declare(spec);
        }
    }
    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        use bibli_core::plugin::Requirement;
        vec![
            Requirement::new("present", "always there", || true).required(),
            Requirement::program("bibli-test-no-such-tool", "never installed"),
        ]
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
    assert!(
        app.settings
            .schema()
            .iter()
            .all(|spec| !spec.key.starts_with("sample."))
    );
    assert_eq!(
        app.settings
            .get::<bool>(bibli_core::settings::PRESERVE_TAGS_ON_COPY),
        Some(true)
    );
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
    assert!(
        app.settings
            .schema()
            .iter()
            .all(|spec| !spec.key.starts_with("sample."))
    );
    assert_eq!(
        app.settings
            .get::<bool>(bibli_core::settings::PRESERVE_TAGS_ON_COPY),
        Some(true)
    );
    assert_eq!(app.settings.get::<String>("sample.engine"), None);
}

#[test]
fn builders_describe_controls_and_the_schema_serializes_for_agents() {
    let budget = SettingSpec::integer("sample.budget", "Budget", "d", 64, (4, 4096))
        .unit("MB")
        .step(16)
        .section("Memory");
    assert_eq!(budget.kind, bibli_core::settings::SettingKind::Integer);
    assert_eq!(budget.owner(), "sample");
    assert_eq!((budget.unit.as_str(), budget.constraints.step), ("MB", 16));
    let engine = SettingSpec::choice(
        "sample.engine",
        "Engine",
        "d",
        "first",
        &[("first", "First"), ("second", "The second")],
    );
    assert_eq!(engine.choice_label("second"), "The second");
    assert_eq!(engine.choice_label("unknown"), "unknown");
    let saved = SettingSpec::data("sample.saved", "d", toml::Value::Array(vec![]));
    assert!(!saved.editable());
    assert!(saved.parse("[]").is_err());
    let json = serde_json::to_value(&budget).unwrap();
    assert_eq!(json["kind"], "integer");
    assert_eq!(json["title"], "Budget");
}

#[test]
fn the_revision_advances_on_declare_apply_and_undeclare_only() {
    let (_tmp, app, _) = fixture(None);
    let declared = app.settings.revision();
    assert_eq!(app.settings.text("sample.budget").as_deref(), Some("64"));
    assert_eq!(app.settings.revision(), declared, "reads do not advance it");
    assert_eq!(
        apply(&app, &serde_json::json!({"sample.budget": "128"})),
        SaveState::Saved
    );
    let applied = app.settings.revision();
    assert!(applied > declared);
    assert_eq!(
        apply(&app, &serde_json::json!({"sample.budget": "128"})),
        SaveState::Saved
    );
    assert_eq!(
        app.settings.revision(),
        applied,
        "an unchanged value is not a change"
    );
    app.settings.undeclare("sample.budget");
    assert!(app.settings.revision() > applied);
}

#[test]
fn plugins_toggle_through_the_command_and_the_choice_is_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let (_tmp, app, _) = fixture(Some(path.clone()));
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_waker(Some(Arc::new(move || {
        let _ = tx.send(());
    })));
    assert!(
        app.dispatch(&Invocation::with_args(
            "plugins.set_enabled",
            &serde_json::json!({"id": "missing", "enabled": false}),
        ))
        .is_err()
    );
    let before = app.plugins.revision();
    app.dispatch(&Invocation::with_args(
        "plugins.set_enabled",
        &serde_json::json!({"id": "sample", "enabled": false}),
    ))
    .unwrap();
    // The job wakes the UI after saving; tolerate earlier, unrelated wakes.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        rx.recv_timeout(left).expect("the toggle completes");
    }
    assert!(!app.plugins.is_enabled("sample"));
    assert!(app.plugins.revision() > before);
    assert!(
        app.settings.spec("sample.engine").is_none(),
        "a disabled plugin withdraws its settings"
    );
    let loaded = Settings::load(&path).unwrap();
    assert_eq!(loaded.get::<bool>("plugins.sample.enabled"), Some(false));
    let Outcome::Value(list) = app
        .dispatch(&Invocation::with_args(
            "plugins.list",
            &serde_json::json!({}),
        ))
        .unwrap()
    else {
        panic!("expected a list")
    };
    assert_eq!(list[0]["id"], "sample");
    assert_eq!(list[0]["enabled"], false);
}

#[test]
fn requirements_are_probed_on_demand_and_notifications_can_name_a_setting() {
    let (_tmp, app, _) = fixture(None);
    let found = app.plugins.probe_requirements();
    assert_eq!(found.len(), 2);
    assert!(
        found
            .iter()
            .any(|r| r.name == "present" && r.found && r.required)
    );
    assert!(
        found
            .iter()
            .any(|r| r.name == "bibli-test-no-such-tool" && !r.found)
    );
    app.notify_about_setting(
        bibli_core::app::NotifyLevel::Warning,
        "needs a tool",
        "sample.engine",
    );
    let notes = app.take_notifications();
    assert_eq!(notes[0].setting.as_deref(), Some("sample.engine"));
}
