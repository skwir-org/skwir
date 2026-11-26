//! What a viewer's window offers (ADR-151, 5), as the one answer every way
//! to run a command there asks (`App::in_viewer`) gives it for the shell's
//! commands and every plugin's, all on at once: what shows the file and
//! acts on it without changing it, the window's own; the rows of its More
//! and the keys it binds are what it runs.

use std::collections::BTreeSet;
use std::sync::Arc;

use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Effect, Invocation};

fn app(tmp: &std::path::Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(tmp)).unwrap();
    for spec in skwir_ui::views::specs() {
        app.registry.views.add("ui", spec);
    }
    skwir_ui::commands::declare_all(&app);
    skwir_ui::settings::declare(&app);
    for plugin in skwir::plugins() {
        let id = plugin.id();
        let on = plugin.enabled_by_default();
        app.plugins.add(plugin, &app);
        if !on {
            assert!(app.plugins.set_enabled(id, true, &app), "{id}: toggle on");
        }
    }
    app
}

fn none() -> serde_json::Value {
    serde_json::json!({})
}

/// Beyond what its topics offer (the preview, the player, the window's own
/// parts), a viewer offers what acts on the file shown without changing
/// it: opening it elsewhere and Open With's rows and picker, Print… and its
/// Save as PDF; the cursor its steps move (`select`); and of the shell's,
/// the window's buttons, Quit, Properties,
/// a document's links, the answers to its questions, F1's list. A new
/// command offered there beyond its topic is listed here, and so is one
/// that changes files.
#[test]
fn a_viewer_offers_beyond_its_topics_only_what_says_so() {
    const BEYOND: &[&str] = &[
        "open",
        "open_with",
        "open_with.choices",
        "open_with.menu",
        "print.print",
        "print.save_pdf",
        // The cursor its steps move, in its pane never drawn.
        "select",
        "ui.close_window",
        "ui.copy_text",
        "ui.maximize_window",
        "ui.minimize_window",
        "ui.open_url",
        "ui.plan_cancel",
        "ui.properties",
        "ui.properties_count",
        "ui.properties_fold",
        "ui.quit",
        "ui.review_apply",
        "ui.review_cancel",
        "ui.shortcuts",
    ];
    const PLANS: &[&str] = &["print.save_pdf"];
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let offered = |filter: &dyn Fn(&skwir_core::command::CommandSpec) -> bool| {
        app.commands
            .list()
            .into_iter()
            .filter(|spec| filter(spec) && app.in_viewer(&spec.name, &none()).offered())
            .map(|spec| spec.name.to_string())
            .collect::<BTreeSet<String>>()
    };
    let expected = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect();
    assert_eq!(
        offered(&|spec| !spec.topic.in_viewer()),
        expected(BEYOND),
        "offered beyond a viewer's topics"
    );
    assert_eq!(
        offered(&|spec| spec.effect == Effect::Plans),
        expected(PLANS),
        "what changes files a viewer offers"
    );
}

/// The plugins' rows of the preview's More are what a viewer runs, as it
/// draws them (`ui.preview_invoke` of each item): Print…, not Reduce
/// Size…, which changes the file. Their keys in a viewer (Print's Ctrl+P)
/// run there.
#[test]
fn a_viewer_runs_the_rows_and_keys_it_shows() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let rows: BTreeSet<String> = app
        .registry
        .menus
        .all()
        .into_iter()
        .filter(|item| item.menu == "preview")
        .filter(|item| {
            let drawn = Invocation::with_args(
                "ui.preview_invoke",
                &serde_json::json!({ "command": item.command.name, "args": item.command.args }),
            );
            app.in_viewer(&drawn.name, &drawn.args).offered()
        })
        .map(|item| {
            item.command
                .args
                .get("command")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&item.command.name)
                .to_owned()
        })
        .collect();
    assert!(rows.contains("print.print"), "{rows:?}");
    assert!(!rows.contains("shrink.reduce_ask"), "{rows:?}");
    let mut keys = 0;
    for binding in app.registry.keymap.all() {
        let in_viewer = binding
            .when
            .as_deref()
            .is_some_and(|when| when.split_whitespace().any(|token| token == "viewer"));
        if in_viewer {
            keys += 1;
            assert!(
                app.in_viewer(&binding.command.name, &binding.command.args)
                    .offered(),
                "{} {}",
                binding.keys,
                binding.command.name
            );
        }
    }
    assert!(keys > 0, "a plugin binds a key in a viewer");
    // Open With's picker, for the file shown, with its applications' rows.
    let entries = serde_json::json!([{ "path": "/srv/a.txt" }]);
    let picker = serde_json::json!({
        "source": { "name": "open_with.choices", "args": { "entries": entries } },
        "choices": [{ "command": {
            "name": "open_with",
            "args": { "app": "editor.desktop", "entries": entries },
        } }],
    });
    assert!(app.in_viewer("ui.choose", &picker).offered());
    // What it does not offer: a tab, the split, the terminal, the pinned
    // player, Activity, Settings, deleting or renaming, a search.
    for name in [
        "tab.new",
        "ui.split",
        "ui.terminal",
        "ui.media_pin",
        "ui.activity",
        "ui.settings",
        "delete",
        "trash",
        "rename",
        "query.search",
        "shrink.reduce_ask",
    ] {
        assert!(!app.in_viewer(name, &none()).runs(), "{name}");
    }
}
