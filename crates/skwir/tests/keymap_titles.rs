//! Every action the keymap lists is called by what it does (ADR-024): in
//! no place do two actions, a command with its arguments, read alike.
//! The shell's keys, every plugin's and a scheme's, all on at once.

use std::collections::BTreeMap;
use std::sync::Arc;

use skwir_core::app::{App, AppConfig};

#[test]
fn no_place_lists_two_actions_under_one_title() {
    let tmp = tempfile::tempdir().unwrap();
    let app = Arc::new(App::new(AppConfig::ephemeral(tmp.path())).unwrap());
    for spec in skwir_ui::views::specs() {
        app.registry.views.add("ui", spec);
    }
    skwir_ui::commands::declare_all(&app);
    for plugin in skwir::plugins() {
        let id = plugin.id();
        let on = plugin.enabled_by_default();
        app.plugins.add(plugin, &app);
        if !on {
            assert!(app.plugins.set_enabled(id, true, &app), "{id}: toggle on");
        }
    }
    // Per place, per title, the actions called so; and the keys whose
    // arguments no declaration names.
    let mut called: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let mut unnamed: Vec<String> = Vec::new();
    // What `keymap::title` works out from what an argument names.
    let named_elsewhere = ["navigate", "ui.view", "ui.choose"];
    for bound in skwir_ui::keymap::bindings(&app, &[]) {
        let Some((command, args)) = &bound.command else {
            continue;
        };
        let has_args = args.as_object().is_some_and(|args| !args.is_empty());
        if has_args
            && !named_elsewhere.contains(&command.as_str())
            && app
                .commands
                .get(command)
                .is_some_and(|spec| spec.form(args).is_none())
        {
            let action = format!("{command} {args}");
            if !unnamed.contains(&action) {
                unnamed.push(action);
            }
        }
        let place = skwir_ui::keymap::place_of(&bound.context).place;
        let title = skwir_ui::keymap::title(&app, command, args);
        let action = format!("{command} {args}");
        let actions = called.entry((place, title)).or_default();
        if !actions.contains(&action) {
            actions.push(action);
        }
    }
    let alike: Vec<String> = called
        .into_iter()
        .filter(|(_, actions)| actions.len() > 1)
        .map(|((place, title), actions)| format!("{place}: “{title}” is {actions:?}"))
        .collect();
    assert!(alike.is_empty(), "titles said twice:\n{}", alike.join("\n"));
    assert!(
        unnamed.is_empty(),
        "keys whose arguments their command's declaration names no form of:\n{}",
        unnamed.join("\n")
    );
}
