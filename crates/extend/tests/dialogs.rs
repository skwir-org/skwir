//! What a window answering another program's Open or Save dialog offers of
//! a plugin from outside (ADR-144, 1), and a viewer's window (ADR-151, 5):
//! none of its commands, whatever its manifest says they are about, until
//! it can say one is fit for them. A program or a module may change files
//! or start programs, which the host cannot tell. Nothing here runs a
//! plugin: what its manifest says is registered without it.

mod common;

use std::path::Path;
use std::sync::Arc;

use skwir_core::App;
use skwir_core::app::AppConfig;

/// An app with the template installed in `plugins`, its commands said to be
/// about `topic` and to only read, its module one that is never run.
fn app(state: &Path, plugins: &Path, topic: &str) -> Arc<App> {
    let manifest = std::fs::read_to_string(
        common::root()
            .join("crates/plugins-outside/template")
            .join("plugin.toml"),
    )
    .unwrap()
    .replace("module = \"word_count.wasm\"", "module = \"x.wasm\"")
    .replace("topic = \"files\"", &format!("topic = \"{topic}\""));
    let folder = plugins.join("word-count");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("plugin.toml"), manifest).unwrap();
    std::fs::write(folder.join("x.wasm"), b"\0asm\x0d\x00\x01\x00").unwrap();
    let app = App::new(AppConfig::ephemeral(state)).unwrap();
    for plugin in skwir_extend::discover(&[plugins.to_owned()], &[]) {
        let id = skwir_core::plugin::Plugin::id(&*plugin);
        app.plugins.add(plugin, &app);
        assert!(app.plugins.set_enabled(id, true, &app), "{id} turned on");
    }
    app
}

#[test]
fn a_dialog_offers_no_command_of_a_plugin_from_outside() {
    let tmp = common::home_tempdir();
    // About finding, a topic a dialog offers.
    let app = app(
        &tmp.path().join("state"),
        &tmp.path().join("plugins"),
        "find",
    );
    for name in ["word-count.count", "word-count.copy"] {
        let spec = app.commands.get(name).expect("registered");
        // By its topic and effect it would be offered: the host says not.
        assert!(spec.in_dialogs_by_default(), "{name}");
        assert!(
            !app.in_dialogs(name, &serde_json::json!({})).runs(),
            "{name} is offered in a dialog's window"
        );
    }
}

#[test]
fn a_viewer_offers_no_command_of_a_plugin_from_outside() {
    let tmp = common::home_tempdir();
    // About previews, a topic a viewer offers.
    let app = app(
        &tmp.path().join("state"),
        &tmp.path().join("plugins"),
        "preview",
    );
    for name in ["word-count.count", "word-count.copy"] {
        let spec = app.commands.get(name).expect("registered");
        assert!(spec.in_viewer_by_default(), "{name}");
        assert!(
            !app.in_viewer(name, &serde_json::json!({})).runs(),
            "{name} is offered in a viewer's window"
        );
    }
}
