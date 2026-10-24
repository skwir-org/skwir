//! A plugin from outside names its commands and fields under its id
//! (ADR-045). No id it may take is a namespace skwir names its own under:
//! the core's, the shell's, and those of the built-in plugins, whose own
//! ids discovery refuses as taken. The rest are
//! `skwir_plugin_api::manifest::RESERVED`, which this keeps whole.

use std::collections::BTreeSet;
use std::sync::Arc;

use skwir_core::app::{App, AppConfig};

#[test]
fn no_namespace_of_skwirs_own_is_an_id_a_plugin_from_outside_may_take() {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("outside-ids-")
        .tempdir_in(base)
        .unwrap();
    let app = Arc::new(App::new(AppConfig::ephemeral(tmp.path())).unwrap());
    skwir_ui::commands::declare_all(&app);
    skwir_ui::settings::declare(&app);
    let plugins = skwir::plugins();
    for plugin in &plugins {
        app.plugins.add(Arc::clone(plugin), &app);
        // Off by default (a key scheme), it names its own once on.
        if !plugin.enabled_by_default() {
            assert!(app.plugins.set_enabled(plugin.id(), true, &app));
        }
    }
    let taken: BTreeSet<&str> = plugins.iter().map(|plugin| plugin.id()).collect();
    let named = app
        .commands
        .list()
        .into_iter()
        .map(|command| command.name.to_string())
        .chain(app.settings.schema().into_iter().map(|setting| setting.key));
    let open: BTreeSet<String> = named
        .filter_map(|name| name.split_once('.').map(|(space, _)| space.to_owned()))
        .filter(|space| !taken.contains(space.as_str()) && skwir_extend::check_id(space).is_ok())
        .collect();
    assert!(
        open.is_empty(),
        "a plugin from outside could take these as its id: {open:?} \
         (add them to skwir_plugin_api::manifest::RESERVED)"
    );
}
