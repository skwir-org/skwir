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
    /// A key scheme's bindings (the vim plugin's).
    schemes: usize,
    menus: usize,
    /// Items made when a menu opens (the default application named).
    menu_sources: usize,
    places: usize,
    /// What keeps a kind of location in the sidebar (bookmarks, searches).
    keepers: Vec<String>,
    views: usize,
    panels: usize,
    providers: BTreeSet<&'static str>,
    metadata: Vec<&'static str>,
    /// Fields read on demand (a picture's position).
    fields: Vec<&'static str>,
    /// Operation kinds plans are made of (tags, positions).
    ops: Vec<&'static str>,
    previews: Vec<&'static str>,
    thumbs: Vec<&'static str>,
    settings: BTreeSet<String>,
    /// Who listens to what plays (the desktop's media controls).
    player: Vec<String>,
    /// Who runs the terminal panel's shells.
    terminals: Option<String>,
    /// Who draws map backgrounds.
    tiles: Option<String>,
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
        schemes: app.registry.schemes.all().len(),
        menus: app.registry.menus.all().len(),
        menu_sources: app.registry.menu_sources.all().len(),
        places: app.registry.places.all().len(),
        keepers: app
            .registry
            .keepers
            .all()
            .into_iter()
            .map(|k| k.scheme)
            .collect(),
        views: app.registry.views.all().len(),
        panels: app.registry.panels.all().len(),
        providers: ["path", "query", "shelf", "recent"]
            .into_iter()
            .filter(|s| app.provider(s).is_some())
            .collect(),
        metadata: app.metadata.ids(),
        fields: app.fields.ids(),
        ops: app.ops.kinds(),
        previews: app.previews.ids(),
        thumbs: app.thumbnails.provider_ids(),
        settings: app.settings.schema().into_iter().map(|s| s.key).collect(),
        player: app.player.listeners(),
        terminals: app.terminals.owner(),
        tiles: app.tiles.owner(),
    }
}

/// The binary's own list: a plugin it registers is a plugin tested here.
fn all_plugins() -> Vec<Arc<dyn Plugin>> {
    bibli::plugins()
}

#[test]
fn every_plugin_disables_without_a_trace_and_re_enables() {
    let tmp = tempfile::tempdir().unwrap();
    for plugin in all_plugins() {
        let id = plugin.id();
        let app = App::new(AppConfig::ephemeral(tmp.path().join(id))).unwrap();
        let before = snapshot(&app);
        app.plugins.add(Arc::clone(&plugin), &app);
        // A plugin off by default (a key scheme) contributes nothing until
        // it is turned on.
        if !plugin.enabled_by_default() {
            assert_eq!(snapshot(&app), before, "{id} is off until turned on");
            assert!(app.plugins.set_enabled(id, true, &app), "{id}: toggle on");
        }
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
    let ids: BTreeSet<&str> = all_plugins().iter().map(|p| p.id()).collect();
    assert_eq!(ids.len(), all_plugins().len(), "duplicate plugin ids");
    assert_eq!(app.plugins.list().len(), ids.len());
}
