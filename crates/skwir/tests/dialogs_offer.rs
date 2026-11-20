//! What a window answering another program's Open or Save dialog offers
//! (ADR-144, 1), as the one answer every way to run a command there asks
//! (`App::in_dialogs`) gives it for the shell's commands and every
//! plugin's, all on at once: what changes files and what opens another
//! program only where they say so, the keys, the palette and the menus.

use std::collections::BTreeSet;
use std::sync::Arc;

use skwir_core::SortKey;
use skwir_core::app::{App, AppConfig};
use skwir_core::command::{Effect, Opens};

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

/// Of what changes files, a dialog offers what a dialog has always
/// offered (making a folder, renaming, the Trash, a paste) and the notes a
/// file is found by; of what opens another program, `open`, which answers
/// the dialog there. A new command that changes files or opens a program
/// is not offered unless it says so, and then it is listed here.
#[test]
fn a_dialog_offers_what_changes_files_or_opens_elsewhere_only_where_said() {
    const PLANS: &[&str] = &[
        "clipboard_paste",
        "mkdir",
        "notes.comment",
        "notes.rate",
        "rename",
        "tags.add",
        "tags.edit",
        "tags.remove",
        "tags.set",
        "tags.toggle",
        "trash",
    ];
    const ELSEWHERE: &[&str] = &["open"];
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let offered = |filter: &dyn Fn(&skwir_core::command::CommandSpec) -> bool| {
        app.commands
            .list()
            .into_iter()
            .filter(|spec| filter(spec) && app.in_dialogs(&spec.name, &none()).offered())
            .map(|spec| spec.name.to_string())
            .collect::<BTreeSet<String>>()
    };
    let plans = offered(&|spec| spec.effect == Effect::Plans);
    let elsewhere = offered(&|spec| spec.opens == Opens::Elsewhere);
    let expected = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect();
    assert_eq!(plans, expected(PLANS), "plans a dialog offers");
    assert_eq!(elsewhere, expected(ELSEWHERE), "what opens elsewhere");
}

/// What a dialog's window does not offer: a tab, the split and where its
/// panes go, the terminal, Open With (its picker too, by its source),
/// Usage and the Map's own, a picture's position rewritten, Print… from the
/// preview's menu, a server's settings changed, Settings and Quit; what
/// it does: going places, finding, a view's shape, where the preview goes.
#[test]
fn a_dialog_offers_what_finds_and_chooses() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let not = [
        ("tab.new", none()),
        ("ui.split", none()),
        ("ui.layout", serde_json::json!({ "panes": "row" })),
        ("ui.terminal", none()),
        ("open_with", serde_json::json!({ "app": "x.desktop" })),
        (
            "ui.choose",
            serde_json::json!({ "source": { "name": "open_with.choices", "args": {} } }),
        ),
        ("ui.view", serde_json::json!({ "id": "usage" })),
        ("usage.scan", none()),
        ("usage.cancel", none()),
        ("ui.map_view_search_area", none()),
        ("geo.remove_location", none()),
        (
            "ui.preview_invoke",
            serde_json::json!({ "command": "print.print", "args": {} }),
        ),
        ("ui.preview_open_external", none()),
        ("remote.remove_server", serde_json::json!({ "id": "x" })),
        ("remote.public_link", none()),
        ("ui.settings", none()),
        ("ui.quit", none()),
        ("ui.activity", none()),
    ];
    for (name, args) in &not {
        assert!(!app.in_dialogs(name, args).runs(), "{name} {args}");
    }
    let offered = [
        ("navigate", none()),
        ("up", none()),
        ("query.search", none()),
        ("ui.find", none()),
        ("ui.view", serde_json::json!({ "id": "grid" })),
        ("ui.layout", serde_json::json!({ "preview": "right" })),
        ("ui.preview_find", none()),
        ("tags.add", none()),
        ("places.mount", none()),
        ("ui.mkdir_prompt", none()),
        ("ui.rename_prompt", none()),
        ("ui.properties", none()),
        ("ui.palette", none()),
        ("ui.shortcuts", none()),
    ];
    for (name, args) in &offered {
        assert!(app.in_dialogs(name, args).offered(), "{name} {args}");
    }
}

/// The palette of a dialog's window lists what it offers: none of the
/// entries a window of the user's own lists for tabs, the split, the
/// terminal or a lens, and every one of what it keeps.
#[test]
fn a_dialog_s_palette_lists_what_it_offers() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let views = app.registry.views.all();
    let sorts = SortKey::ALL;
    let offers = skwir_ui::palette::offered(&app, &views, &sorts, &sorts);
    let (here, gone): (Vec<_>, Vec<_>) = offers
        .into_iter()
        .partition(|offer| app.in_dialogs(&offer.name, &offer.args).offered());
    let named = |offers: &[skwir_ui::palette::Offer]| -> BTreeSet<String> {
        offers
            .iter()
            .map(|offer| format!("{} {}", offer.name, offer.args))
            .collect()
    };
    let (here, gone) = (named(&here), named(&gone));
    for kept in [
        "ui.mkdir_prompt {}",
        "ui.rename_prompt {}",
        "trash {}",
        r#"ui.layout {"preview":"right"}"#,
        "ui.find {}",
    ] {
        assert!(here.contains(kept), "{kept} is not offered: {here:#?}");
    }
    for left in [
        "ui.split {}",
        r#"ui.layout {"panes":"row"}"#,
        "ui.terminal {}",
        "tab.new {}",
        "usage.scan {}",
        "ui.quit {}",
    ] {
        assert!(gone.contains(left), "{left} is offered");
    }
}

/// The keys a dialog's window does not run do there what they would with
/// nothing bound (`Shell::on_run_command`), and F1 lists none of them: a
/// new tab, the split, the terminal, Open With's picker. Its own keep
/// theirs: Enter opens (answers), F2 renames, Escape cancels.
#[test]
fn a_dialog_s_keys_are_what_it_offers() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let (mut runs, mut refused) = (BTreeSet::new(), BTreeSet::new());
    for bound in skwir_ui::keymap::bindings(&app, &[]) {
        let Some((name, args)) = &bound.command else {
            continue;
        };
        let source = args["source"]["name"].as_str().unwrap_or_default();
        let named = format!("{name} {source}").trim().to_owned();
        if app.in_dialogs(name, args).runs() {
            runs.insert(named);
        } else {
            refused.insert(named);
        }
    }
    for name in [
        "tab.new",
        "ui.split",
        "ui.terminal",
        "ui.choose open_with.choices",
        "ui.settings",
    ] {
        assert!(refused.contains(name), "{name} runs: {runs:#?}");
    }
    for name in [
        "open",
        "ui.rename_prompt",
        "trash",
        "ui.chooser_cancel",
        "ui.palette",
        "ui.shortcuts",
        "ui.preview_match",
    ] {
        assert!(runs.contains(name), "{name} is refused: {refused:#?}");
    }
}
