//! Phase 4 gate: every plugin can be disabled at runtime with no trace left
//! in any registry, and re-enabled.

use std::collections::BTreeSet;
use std::sync::Arc;

use bibli_core::app::{App, AppConfig};
use bibli_core::plugin::Plugin;

/// Everything a plugin could have contributed, as comparable sets.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    commands: BTreeSet<String>,
    keymap: usize,
    menus: usize,
    places: usize,
    views: usize,
    panels: usize,
    providers: BTreeSet<&'static str>,
    metadata: Vec<&'static str>,
    previews: Vec<&'static str>,
    thumbs: Vec<&'static str>,
    settings: BTreeSet<String>,
}

fn snapshot(app: &App) -> Snapshot {
    Snapshot {
        commands: app
            .commands
            .list()
            .into_iter()
            .map(|c| c.name.to_string())
            .collect(),
        keymap: app.registry.keymap.all().len(),
        menus: app.registry.menus.all().len(),
        places: app.registry.places.all().len(),
        views: app.registry.views.all().len(),
        panels: app.registry.panels.all().len(),
        providers: ["path", "query", "shelf", "recent"]
            .into_iter()
            .filter(|s| app.provider(s).is_some())
            .collect(),
        metadata: app.metadata.ids(),
        previews: app.previews.ids(),
        thumbs: app.thumbnails.provider_ids(),
        settings: app.settings.schema().into_iter().map(|s| s.key).collect(),
    }
}

fn all_plugins() -> Vec<Arc<dyn Plugin>> {
    vec![
        Arc::new(bibli_plugin_ops_basic::OpsBasicPlugin::default()),
        Arc::new(bibli_plugin_open_with::OpenWithPlugin),
        Arc::new(bibli_plugin_places::PlacesPlugin),
        Arc::new(bibli_plugin_thumbnails::ThumbnailsPlugin),
        Arc::new(bibli_plugin_preview_media::PreviewPlugin),
        Arc::new(bibli_plugin_preview_documents::PreviewPlugin),
        Arc::new(bibli_plugin_query::QueryPlugin),
        Arc::new(bibli_plugin_shelf::ShelfPlugin::default()),
        Arc::new(bibli_plugin_rename_bulk::RenameBulkPlugin),
        Arc::new(bibli_plugin_edit_as_buffer::EditAsBufferPlugin),
        Arc::new(bibli_plugin_jump::JumpPlugin::default()),
        Arc::new(bibli_plugin_tags::TagsPlugin),
        Arc::new(bibli_plugin_shell::ShellPlugin),
        Arc::new(bibli_plugin_usage::UsagePlugin::default()),
        Arc::new(bibli_plugin_timeline::TimelinePlugin::default()),
    ]
}

#[test]
fn every_plugin_disables_without_a_trace_and_re_enables() {
    let tmp = tempfile::tempdir().unwrap();
    for plugin in all_plugins() {
        let id = plugin.id();
        let app = App::new(AppConfig::ephemeral(tmp.path().join(id))).unwrap();
        let before = snapshot(&app);
        app.plugins.add(Arc::clone(&plugin), &app);
        let enabled = snapshot(&app);
        assert_ne!(before, enabled, "{id} must contribute something");
        assert!(app.plugins.set_enabled(id, false, &app), "{id}: toggle off");
        assert_eq!(
            snapshot(&app),
            before,
            "{id} left something behind after disable"
        );
        assert!(app.plugins.set_enabled(id, true, &app), "{id}: toggle on");
        assert_eq!(
            snapshot(&app),
            enabled,
            "{id} re-enable must restore its contributions"
        );
    }
}

#[test]
fn all_plugins_coexist_and_every_command_has_a_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    for p in all_plugins() {
        app.plugins.add(p, &app);
    }
    let names: Vec<String> = app
        .commands
        .list()
        .into_iter()
        .map(|c| c.name.to_string())
        .collect();
    let unique: BTreeSet<&String> = names.iter().collect();
    assert_eq!(
        unique.len(),
        names.len(),
        "duplicate command names: {names:?}"
    );
    for c in app.commands.list() {
        assert!(
            c.args_schema.as_object().is_some(),
            "{} has no schema",
            c.name
        );
    }
    assert_eq!(app.plugins.list().len(), 15);
}
