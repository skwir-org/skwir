//! Phase 4 gate: every plugin can be disabled at runtime with no trace left
//! in any registry, and re-enabled.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use skwir_core::app::{App, AppConfig};
use skwir_core::plugin::Plugin;

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
    property_sources: usize,
    /// Who shows places: by owner, not by count, as what a plugin shows
    /// follows the machine (its mounts and volumes, which other tests and
    /// the desktop change meanwhile).
    places: Vec<std::sync::Arc<str>>,
    /// Who knows of the remotes mounted (ADR-077), alike.
    remotes: Vec<std::sync::Arc<str>>,
    /// What keeps a kind of location in the sidebar (bookmarks, searches).
    keepers: Vec<String>,
    /// Other ways to see a folder, each a button of its address (its tags).
    aspects: Vec<String>,
    views: usize,
    panels: usize,
    providers: BTreeSet<&'static str>,
    metadata: Vec<&'static str>,
    /// Fields read on demand (a picture's position).
    fields: Vec<&'static str>,
    /// Indexes a search asks first (the tags plugin's, Baloo).
    hints: Vec<&'static str>,
    /// Where notes are kept (ADR-027: the desktop's attributes, the Finder's).
    notes: Vec<&'static str>,
    /// Files beside a file (ADR-028: a Mac's `._` files).
    companions: Vec<&'static str>,
    transfers: Vec<&'static str>,
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
    /// Who places a pane's files on its map.
    maps: Option<String>,
}

/// What is registered once the jobs the plugins started have ended: the
/// places plugin publishes what it finds at once and the volumes `lsblk`
/// lists from a worker, and re-enabled it shows at once those it measured
/// last, so a snapshot taken while that worker runs depends on when it
/// ends.
fn snapshot(app: &App) -> Snapshot {
    // A remote's lane may wait for good on a server gone (a mount of the
    // machine's whose server stopped answering): not what is compared.
    assert!(
        app.scheduler.wait_idle_here(Duration::from_secs(10)),
        "jobs still running after 10 s"
    );
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
        property_sources: app.registry.property_sources.all().len(),
        places: app.registry.places.owners(),
        remotes: app.registry.remotes.owners(),
        keepers: app
            .registry
            .keepers
            .all()
            .into_iter()
            .map(|k| k.scheme)
            .collect(),
        aspects: app
            .registry
            .aspects
            .all()
            .into_iter()
            .map(|a| a.scheme)
            .collect(),
        views: app.registry.views.all().len(),
        panels: app.registry.panels.all().len(),
        providers: [
            "path", "query", "shelf", "recent", "tags", "trash", "sftp", "ssh", "smb", "cifs",
            "dav", "davs", "webdav", "webdavs", "ftp", "ftps", "nfs", "afp", "rclone", "s3", "mtp",
            "gphoto2", "afc", "nearby",
        ]
        .into_iter()
        .filter(|s| app.provider(s).is_some())
        .collect(),
        metadata: app.metadata.ids(),
        fields: app.fields.ids(),
        hints: app.hints.ids(),
        notes: app.notes.ids(),
        companions: app.companions.ids(),
        transfers: app.transfers.names(),
        ops: app.ops.kinds(),
        previews: app.previews.ids(),
        thumbs: app.thumbnails.provider_ids(),
        settings: app.settings.schema().into_iter().map(|s| s.key).collect(),
        player: app.player.listeners(),
        terminals: app.terminals.owner(),
        tiles: app.tiles.owner(),
        maps: app.maps.owner(),
    }
}

/// The binary's own list: a plugin it registers is a plugin tested here.
fn all_plugins() -> Vec<Arc<dyn Plugin>> {
    skwir::plugins()
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

/// Every menu source takes what its menu asks it with (`MenuSource`): the
/// pane, which every menu gives, and a place's `location` or a tag's
/// `tags`. One refusing them leaves its items out, saying nothing: the
/// tag pill in the sidebar lost its tag's menu so.
#[test]
fn every_menu_source_takes_what_its_menu_gives() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    for p in all_plugins() {
        app.plugins.add(p, &app);
    }
    let pane = app.workspace.read().active_id();
    let sources = app.registry.menu_sources.all();
    assert!(sources.iter().any(|s| s.menu == "place"));
    for source in sources {
        let command = match source.menu.as_str() {
            "place" => source
                .command
                .clone()
                .with("location", "sftp://nas.example/srv"),
            "tag" => source
                .command
                .clone()
                .with("tags", serde_json::json!(["work"])),
            _ => source.command.clone(),
        }
        .with("pane", pane);
        if let Err(error) = app.dispatch(&command) {
            let said = format!("{error:#}");
            assert!(
                !said.contains("invalid arguments"),
                "{} in the {} menu: {said}",
                command.name,
                source.menu
            );
        }
    }
}
