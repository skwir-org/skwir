//! Every action the keymap lists is called by what it does (ADR-024): in
//! no place do two actions, a command with its arguments, read alike.
//! The shell's keys, every plugin's and a scheme's, all on at once.

use std::collections::BTreeMap;
use std::sync::Arc;

use skwir_core::SortKey;
use skwir_core::app::{App, AppConfig};
use skwir_core::command::Effect;

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

/// The palette offers each thing to do under a title of its own: two
/// entries of one title were two commands for one thing (the shell's
/// `ui.toggle_hidden` beside the core's `toggle_hidden`), or two things
/// read alike (copying for a paste and copying to a folder).
#[test]
fn the_palette_names_each_thing_to_do_once() {
    let tmp = tempfile::tempdir().unwrap();
    let app = Arc::new(App::new(AppConfig::ephemeral(tmp.path())).unwrap());
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
    let views = app.registry.views.all();
    // Every sort, as where every column applies.
    let sorts = [
        SortKey::Name,
        SortKey::Size,
        SortKey::Modified,
        SortKey::Rating,
        SortKey::Deleted,
    ];
    let mut offered: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for offer in skwir_ui::palette::offered(&app, &views, &sorts) {
        offered
            .entry(offer.title)
            .or_default()
            .push(format!("{} {}", offer.name, offer.args));
    }
    let twice: Vec<String> = offered
        .into_iter()
        .filter(|(_, actions)| actions.len() > 1)
        .map(|(title, actions)| format!("“{title}” is {actions:?}"))
        .collect();
    assert!(twice.is_empty(), "offered twice:\n{}", twice.join("\n"));
}

/// What changes nothing is data (`Declared::data`: an agent's question, a
/// panel's rows), left out of the palette and the keyboard's lists, unless
/// the window shows what it answers when a person runs it. Those are
/// named here, so a new command that only reads says which it is.
#[test]
fn what_only_reads_is_data_unless_the_window_shows_its_answer() {
    // Copies the selected paths to the clipboard, and says so.
    const SHOWN: [&str; 1] = ["shell.copy_paths"];
    let tmp = tempfile::tempdir().unwrap();
    let app = Arc::new(App::new(AppConfig::ephemeral(tmp.path())).unwrap());
    skwir_ui::commands::declare_all(&app);
    for plugin in skwir::plugins() {
        let id = plugin.id();
        let on = plugin.enabled_by_default();
        app.plugins.add(plugin, &app);
        if !on {
            assert!(app.plugins.set_enabled(id, true, &app), "{id}: toggle on");
        }
    }
    let undecided: Vec<String> = app
        .commands
        .list()
        .into_iter()
        .filter(|spec| spec.effect == Effect::Reads && !spec.data)
        .map(|spec| spec.name.to_string())
        .filter(|name| !SHOWN.contains(&name.as_str()))
        .collect();
    assert!(
        undecided.is_empty(),
        "reads, neither data nor shown from the window: {undecided:?}"
    );
    let views = app.registry.views.all();
    let offered = skwir_ui::palette::offered(&app, &views, &[SortKey::Name]);
    for name in SHOWN {
        assert!(offered.iter().any(|o| o.name == name), "{name} offered");
    }
    let data: Vec<String> = offered
        .iter()
        .filter(|o| app.commands.get(&o.name).is_some_and(|spec| spec.data))
        .map(|o| o.name.clone())
        .collect();
    assert!(data.is_empty(), "data offered: {data:?}");
}
