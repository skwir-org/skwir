//! Preference transactions use synthetic settings and isolated files.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use skwir_core::{
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
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
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
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        use skwir_core::plugin::Requirement;
        vec![
            Requirement::new("present", "always there", || true).required(),
            Requirement::program("skwir-test-no-such-tool", "never installed"),
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
    let loaded = Settings::load(&path);
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
            .get::<bool>(skwir_core::settings::PRESERVE_ATTRIBUTES_ON_COPY),
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
        skwir_core::scheduler::Priority::Visible,
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
            .get::<bool>(skwir_core::settings::PRESERVE_ATTRIBUTES_ON_COPY),
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
    assert_eq!(budget.kind, skwir_core::settings::SettingKind::Integer);
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
    let loaded = Settings::load(&path);
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
    assert_eq!(list[0]["category"], "files");
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
            .any(|r| r.name == "skwir-test-no-such-tool" && !r.found)
    );
    app.notify_about_setting(
        skwir_core::app::NotifyLevel::Warning,
        "needs a tool",
        "sample.engine",
    );
    let notes = app.take_notifications();
    assert_eq!(notes[0].setting.as_deref(), Some("sample.engine"));
}

#[test]
fn startup_opens_the_chosen_folder_or_the_last_one_and_falls_back_to_home() {
    use skwir_core::settings::{
        LAST_LOCATION, STARTUP, STARTUP_FOLDER, remember_location, startup_folder,
    };
    let dir = tempfile::tempdir().unwrap();
    let (_tmp, app, _) = fixture(Some(dir.path().join("settings.toml")));
    let home = std::path::Path::new("/home/example");
    assert_eq!(startup_folder(&app.settings, home), home);
    app.settings.set(
        STARTUP_FOLDER,
        toml::Value::String("/srv/it's shared".into()),
    );
    assert_eq!(
        startup_folder(&app.settings, home),
        std::path::Path::new("/srv/it's shared")
    );
    // Not remembered unless asked for: the file is not rewritten at every exit.
    assert!(!remember_location(
        &app.settings,
        std::path::Path::new("/example/work")
    ));
    assert_eq!(
        app.settings.get::<String>(LAST_LOCATION).as_deref(),
        Some("")
    );
    app.settings
        .set(STARTUP, toml::Value::String("last".into()));
    assert_eq!(
        startup_folder(&app.settings, home),
        home,
        "nothing remembered yet"
    );
    // Recorded, the caller saving (`tabs::remember`, at exit).
    assert!(remember_location(
        &app.settings,
        std::path::Path::new("/example/work")
    ));
    app.settings.save().unwrap();
    assert_eq!(
        startup_folder(&app.settings, home),
        std::path::Path::new("/example/work")
    );
    let reloaded = Settings::load(dir.path().join("settings.toml"));
    assert_eq!(
        reloaded.get::<String>(LAST_LOCATION).as_deref(),
        Some("/example/work")
    );
}

#[test]
fn a_renamed_preference_keeps_its_value_and_the_file_forgets_the_old_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(
        &path,
        "[old]\nwidth = \"wide\"\n\n[kept]\nchoice = \"a\"\nother = 1\n\n[new]\nchoice = \"b\"\n",
    )
    .unwrap();
    let settings = Settings::load(&path);
    settings.rename("old.width", "new.width");
    // A value already stored under the new name is not overwritten.
    settings.rename("kept.choice", "new.choice");
    // Nothing stored under the old name: nothing moves.
    settings.rename("old.missing", "new.missing");
    assert_eq!(settings.get::<String>("new.width").as_deref(), Some("wide"));
    assert_eq!(settings.get::<String>("new.choice").as_deref(), Some("b"));
    assert!(settings.get::<toml::Value>("new.missing").is_none());
    settings.save().unwrap();
    let saved: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert!(saved.get("old").is_none(), "an emptied table goes too");
    assert_eq!(saved["kept"]["choice"].as_str(), Some("a"));
    assert_eq!(saved["new"]["width"].as_str(), Some("wide"));
    // Again at the next start: already moved, nothing changes.
    let settings = Settings::load(&path);
    settings.rename("old.width", "new.width");
    assert_eq!(settings.get::<String>("new.width").as_deref(), Some("wide"));
}

#[test]
fn records_are_checked_field_by_field_and_never_hold_a_secret() {
    use skwir_core::settings::{RecordField, SettingSpec};
    let spec = SettingSpec::records(
        "sample.servers",
        "Servers",
        "",
        "server",
        vec![
            RecordField::text("name", "Name", "", "").required(),
            RecordField::integer("zoom", "Zoom", "", (0, 22)),
            RecordField::pairs("headers", "Headers", ""),
            RecordField::secret("key", "Key", ""),
        ],
    );
    let value = spec
        .parse(r#"[{"id":"a","name":" It's a café ","zoom":"12","headers":[["Referer","x"],["",""]],"key":"keyring"}]"#)
        .unwrap();
    let record = &value.as_array().unwrap()[0];
    assert_eq!(record["name"].as_str(), Some("It's a café"));
    assert_eq!(record["zoom"].as_integer(), Some(12));
    // An empty pair is dropped; a filled one kept.
    assert_eq!(record["headers"].as_array().unwrap().len(), 1);
    // The editor text round-trips as JSON.
    let text = SettingSpec::format_value(&value);
    assert_eq!(spec.parse(&text).unwrap(), value);

    let refused = |text: &str| spec.parse(text).unwrap_err().to_string();
    assert!(refused(r#"[{"id":"a"}]"#).contains("Name is needed"));
    assert!(refused(r#"[{"id":"a","name":"x","zoom":30}]"#).contains("0 to 22"));
    assert!(refused(r#"[{"id":"a","name":"x"},{"id":"a","name":"y"}]"#).contains("share"));
    assert!(refused(r#"[{"name":"x"}]"#).contains("id"));
    assert!(refused(r#"[{"id":"a","name":"x","key":"hunter2"}]"#).contains("keyring"));
    assert!(refused(r#"[{"id":"a","name":"x","headers":[["","v"]]}]"#).contains("needs a name"));
    // Nothing is a list of nothing.
    assert_eq!(spec.parse("").unwrap(), toml::Value::Array(Vec::new()));
}

#[test]
fn an_owner_check_runs_after_the_kind() {
    use skwir_core::settings::SettingSpec;
    let spec = SettingSpec::text("sample.url", "Address", "", "").validate_with(|value| {
        anyhow::ensure!(
            value
                .as_str()
                .is_some_and(|s| s.is_empty() || s.starts_with("https://")),
            "must start with https://"
        );
        Ok(())
    });
    assert!(spec.parse("https://example.org").is_ok());
    assert!(spec.parse("ftp://example.org").is_err());
}

#[test]
fn a_settings_file_that_cannot_be_read_is_kept_aside_and_skwir_starts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[ui]\nview = \"grid\"\nzoom = = 2\n").unwrap();
    let settings = Settings::load(&path);
    let problem = settings.take_problem().expect("said");
    assert!(problem.contains("on line 3"), "{problem}");
    assert!(problem.contains("started with its defaults"), "{problem}");
    assert!(settings.take_problem().is_none(), "said once");
    assert!(!path.exists(), "set aside, not written over");
    let aside: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(aside.len(), 1);
    assert!(
        aside[0].starts_with("settings.toml.unreadable-"),
        "{aside:?}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(&aside[0])).unwrap(),
        "[ui]\nview = \"grid\"\nzoom = = 2\n",
        "kept as it was"
    );
    assert_eq!(settings.path(), Some(path.as_path()), "a new file is saved");
}

#[test]
fn a_settings_file_that_is_no_text_is_kept_aside_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, b"[ui]\nname = \"\xff\xfe\"\n").unwrap();
    let settings = Settings::load(&path);
    assert!(
        settings
            .take_problem()
            .is_some_and(|p| p.contains("it is not text"))
    );
    assert!(!path.exists());
}

/// The settings file is shared (another skwir, a hand edit, Home
/// Manager): a save writes the keys this process changed over the file as
/// it stands, and takes in the rest.
#[test]
fn a_save_writes_its_own_keys_over_the_file_as_it_stands() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[sample]\nengine = 'first'\nold = 1\n").unwrap();
    let settings = Settings::load(&path);
    settings.set("sample.budget", toml::Value::Integer(99));
    settings.rename("sample.old", "sample.new");
    // Meanwhile, written elsewhere.
    std::fs::write(
        &path,
        "[sample]\nengine = 'second'\nold = 1\n[other]\nkept = 'theirs'\n",
    )
    .unwrap();
    settings.save().unwrap();
    let saved: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let sample = saved["sample"].as_table().unwrap();
    assert_eq!(sample["engine"].as_str(), Some("second"), "theirs, kept");
    assert_eq!(sample["budget"].as_integer(), Some(99), "ours, written");
    assert_eq!(sample["new"].as_integer(), Some(1), "moved");
    assert!(!sample.contains_key("old"), "the old name gone");
    assert_eq!(saved["other"]["kept"].as_str(), Some("theirs"));
    assert_eq!(
        settings.get::<String>("sample.engine").as_deref(),
        Some("second"),
        "taken in"
    );
}

/// What changed in the file is taken in, under what this process changed
/// and has not saved yet; the declared keys that changed are said.
#[test]
fn an_outside_edit_is_taken_in_under_unsaved_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[sample]\nengine = 'first'\n").unwrap();
    let settings = Settings::load(&path);
    settings.declare(SettingSpec::choice(
        "sample.engine",
        "Engine",
        "engine",
        "first",
        &[("first", "First"), ("second", "Second")],
    ));
    settings.set("sample.path", toml::Value::String("/example/mine".into()));
    assert_eq!(settings.reload(), Some(Vec::new()), "nothing new");
    let before = settings.revision();
    std::fs::write(
        &path,
        "[sample]\nengine = 'second'\npath = '/example/theirs'\n[plugins.sample]\nenabled = false\n",
    )
    .unwrap();
    assert_eq!(
        settings.reload(),
        Some(vec![
            "sample.engine".to_owned(),
            "plugins.sample.enabled".to_owned()
        ])
    );
    assert!(settings.revision() > before);
    assert_eq!(
        settings.get::<String>("sample.engine").as_deref(),
        Some("second")
    );
    assert_eq!(
        settings.get::<String>("sample.path").as_deref(),
        Some("/example/mine"),
        "not saved yet, still this process's"
    );
    settings.save().unwrap();
    let saved: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(saved["sample"]["path"].as_str(), Some("/example/mine"));
    assert_eq!(saved["sample"]["engine"].as_str(), Some("second"));
}

/// A file that is no TOML (a hand edit under way) is never written over:
/// the save says why, and nothing is taken in from it.
#[test]
fn a_broken_settings_file_is_never_saved_over() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[sample]\nengine = 'first'\n").unwrap();
    let settings = Settings::load(&path);
    settings.set("sample.budget", toml::Value::Integer(99));
    let broken = "[sample\nengine = ";
    std::fs::write(&path, broken).unwrap();
    assert_eq!(settings.reload(), None);
    let error = settings.save().unwrap_err().to_string();
    assert!(error.contains("is not valid"), "{error}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    assert_eq!(
        settings.get::<String>("sample.engine").as_deref(),
        Some("first")
    );
    // Fixed, the next save writes what waited.
    std::fs::write(&path, "[sample]\nengine = 'second'\n").unwrap();
    settings.save().unwrap();
    let saved: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(saved["sample"]["budget"].as_integer(), Some(99));
}

/// A running skwir follows its settings file: a value changed there
/// reaches its owner, and a plugin turned off there is turned off.
#[test]
fn a_running_skwir_follows_its_settings_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("skwir").join("settings.toml");
    let (_tmp, app, plugin) = fixture(Some(path.clone()));
    let until = |done: &dyn Fn() -> bool| {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "never happened");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    until(&|| app.settings.is_followed());
    let notified = plugin.0.load(Ordering::SeqCst);
    std::fs::write(&path, "[sample]\nengine = 'second'\n").unwrap();
    until(&|| app.settings.get::<String>("sample.engine").as_deref() == Some("second"));
    until(&|| plugin.0.load(Ordering::SeqCst) > notified);
    assert!(app.plugins.is_enabled("sample"));
    // Put in place whole, as Home Manager's activation does.
    let candidate = path.with_extension("toml.candidate");
    std::fs::write(
        &candidate,
        "[sample]\nengine = 'second'\n\n[plugins.sample]\nenabled = false\n",
    )
    .unwrap();
    std::fs::rename(&candidate, &path).unwrap();
    until(&|| !app.plugins.is_enabled("sample"));
}

fn state_spec(key: &str) -> SettingSpec {
    SettingSpec::state(key, "state", toml::Value::Table(toml::Table::new()))
}

fn table(path: &std::path::Path) -> toml::Table {
    std::fs::read_to_string(path).unwrap().parse().unwrap()
}

/// What skwir keeps of the window goes to the state file (ADR-056); one
/// still in the settings file, where an earlier skwir kept it, moves out at
/// the next save.
#[test]
fn the_window_s_state_is_kept_apart_from_the_settings() {
    let dir = tempfile::tempdir().unwrap();
    let (path, state) = (
        dir.path().join("settings.toml"),
        dir.path().join("state.toml"),
    );
    std::fs::write(
        &path,
        "[ui]\ntheme = 'dark'\n[ui.session]\npanes = 2\n[core]\nlast_tabs = { count = 1 }\n",
    )
    .unwrap();
    let settings = Settings::load(&path).with_state(&state);
    settings.declare(state_spec("ui.session"));
    settings.declare(state_spec("core.last_tabs"));
    assert_eq!(
        settings.get::<i64>("ui.session.panes"),
        Some(2),
        "read where it was"
    );
    let mut session = toml::Table::new();
    session.insert("panes".into(), toml::Value::Integer(3));
    settings.set("ui.session", toml::Value::Table(session));
    settings.save().unwrap();
    let kept = table(&path);
    assert_eq!(kept["ui"]["theme"].as_str(), Some("dark"));
    assert!(!kept["ui"].as_table().unwrap().contains_key("session"));
    assert!(
        !kept.contains_key("core"),
        "moved out, and its table with it"
    );
    let saved = table(&state);
    assert_eq!(saved["ui"]["session"]["panes"].as_integer(), Some(3));
    assert_eq!(saved["core"]["last_tabs"]["count"].as_integer(), Some(1));
    // Read again, each from its file.
    let again = Settings::load(&path).with_state(&state);
    assert_eq!(again.get::<i64>("ui.session.panes"), Some(3));
    assert_eq!(again.get::<String>("ui.theme").as_deref(), Some("dark"));
}

/// A settings file linked from elsewhere (the user's own configuration)
/// is written where the link leads, and stays a link.
#[cfg(unix)]
#[test]
fn a_linked_settings_file_is_written_where_it_leads() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("dotfiles").join("skwir.toml");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "[ui]\ntheme = 'dark'\n").unwrap();
    let path = dir.path().join("settings.toml");
    std::os::unix::fs::symlink(&real, &path).unwrap();
    let settings = Settings::load(&path);
    settings.set("ui.theme", toml::Value::String("light".into()));
    settings.save().unwrap();
    assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
    assert_eq!(table(&real)["ui"]["theme"].as_str(), Some("light"));
}

/// A settings file that cannot be written (Home Manager's link into the
/// store): the window's state is still saved, and a change of a setting
/// says the settings are read-only, the file untouched.
#[cfg(unix)]
#[test]
fn read_only_settings_still_keep_the_window_s_state() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store").join("settings.toml");
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    std::fs::write(&store, "[ui]\ntheme = 'dark'\n").unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o444)).unwrap();
    let path = dir.path().join("settings.toml");
    std::os::unix::fs::symlink(&store, &path).unwrap();
    let state = dir.path().join("state.toml");
    let settings = Settings::load(&path).with_state(&state);
    settings.declare(state_spec("ui.session"));
    assert!(settings.read_only());
    let mut session = toml::Table::new();
    session.insert("panes".into(), toml::Value::Integer(2));
    settings.set("ui.session", toml::Value::Table(session));
    settings.save().unwrap();
    assert_eq!(
        table(&state)["ui"]["session"]["panes"].as_integer(),
        Some(2)
    );
    settings.set("ui.theme", toml::Value::String("light".into()));
    let error = settings.save().unwrap_err().to_string();
    assert!(error.contains("cannot be written"), "{error}");
    assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
    assert_eq!(table(&store)["ui"]["theme"].as_str(), Some("dark"));
    assert_eq!(
        settings.get::<String>("ui.theme").as_deref(),
        Some("dark"),
        "undone: what the file says"
    );
    // The state is saved still, the change refused not asked again.
    let mut session = toml::Table::new();
    session.insert("panes".into(), toml::Value::Integer(1));
    settings.set("ui.session", toml::Value::Table(session));
    settings.save().unwrap();
    assert_eq!(
        table(&state)["ui"]["session"]["panes"].as_integer(),
        Some(1)
    );
    assert_eq!(settings.get::<i64>("ui.session.panes"), Some(1));
    // Writable again for the temporary folder's removal.
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o644)).unwrap();
}

/// Settings read from a file are checked as the dialog checks what is
/// typed: their kind, then their choices and ranges; a key no setting has
/// under an owner that declares some is a problem, one under an owner
/// nothing declares only a note; a plugin's `enabled`, against the plugins.
#[test]
fn settings_from_a_file_are_checked_as_the_dialog_checks_them() {
    let (_tmp, app, _plugin) = fixture(None);
    let table: toml::Table = r#"
        [sample]
        engine = 'third'
        budget = 5000
        enabled = 'yes'
        engin = 'first'
        path = "/example/reader's library.so"
        [core]
        startup = 'last'
        [elsewhere]
        thing = 1
        [plugins.sample]
        enabled = 'no'
        [plugins.ghost]
        enabled = true
    "#
    .parse()
    .unwrap();
    // Choices found where skwir runs (a user's themes): another value is
    // noted, never refused.
    app.settings.declare(
        SettingSpec::choice(
            "sample.theme",
            "Theme",
            "theme",
            "default",
            &[("default", "Default")],
        )
        .more_choices("the themes you added"),
    );
    let mut table = table;
    table["sample"]
        .as_table_mut()
        .unwrap()
        .insert("theme".into(), toml::Value::String("Mine".into()));
    let plugins = vec!["sample".to_owned()];
    let checked = app.settings.check(&table, &plugins);
    let keys: Vec<&str> = checked.problems.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        [
            "plugins.sample.enabled",
            "sample.budget",
            "sample.enabled",
            "sample.engin",
            "sample.engine",
        ]
    );
    let said = |key: &str| {
        checked
            .problems
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, problem)| problem.clone())
            .unwrap()
    };
    assert_eq!(said("sample.engine"), "choose one of first, second");
    assert_eq!(said("sample.budget"), "enter a whole number from 4 to 4096");
    assert_eq!(said("sample.enabled"), "expected true or false, not text");
    assert_eq!(said("sample.engin"), "no such setting");
    let noted: Vec<&str> = checked.notes.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        noted,
        ["elsewhere", "plugins.ghost", "sample.theme"],
        "an owner noted once"
    );

    // Through the command, as agents and scripts ask: what is wrong fails.
    let asked = app.dispatch(&Invocation::with_args(
        "settings.check",
        &serde_json::json!({ "settings": { "sample": { "engine": "third" } } }),
    ));
    let error = asked.err().unwrap().to_string();
    assert!(error.contains("sample.engine: choose one of"), "{error}");
    let Outcome::Value(fine) = app
        .dispatch(&Invocation::with_args(
            "settings.check",
            &serde_json::json!({ "settings": { "sample": { "engine": "second" } } }),
        ))
        .unwrap()
    else {
        panic!("expected a value")
    };
    assert_eq!(fine["problems"], serde_json::json!([]));
}
