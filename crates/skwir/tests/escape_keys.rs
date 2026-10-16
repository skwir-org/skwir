//! What Escape does in the file list, as the keymap of the plugins on by
//! default resolves it, and where it leads: out of a search to the folder
//! it looks in, never out of a folder or a tag, and never forgetting a
//! cut.

use std::sync::Arc;
use std::time::Duration;

use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::location::Location;
use skwir_core::workspace::PaneId;
use skwir_ui::keymap::{self, Bound};

/// The flags of the file list's key context for what `pane` shows, as
/// `ListView::selection_ctx` reads them, with `selected` items selected.
fn flags(app: &App, pane: PaneId, selected: usize) -> Vec<&'static str> {
    let shown = app.pane_location(pane).unwrap();
    let verbs = app.verbs(&shown);
    let mut flags = vec![if selected == 0 { "empty" } else { "selection" }];
    for (token, holds) in [
        ("folder", shown.is_path()),
        ("search", app.is_search(&shown)),
        ("refs", verbs.remove.is_some()),
        ("drops", verbs.drop.is_some()),
        ("creates", verbs.create.is_some()),
        ("restores", verbs.restore.is_some()),
    ] {
        if holds {
            flags.push(token);
        }
    }
    flags
}

/// The command Escape runs in the file list where `flags` hold: of its
/// bindings there, the latest whose condition holds, as GPUI picks
/// among bindings of one place.
fn escape_runs(bound: &[Bound], flags: &[&str]) -> String {
    let holds = |token: &str| Some(token == keymap::PANE || flags.contains(&token));
    bound
        .iter()
        .filter(|b| b.keys == "escape" && b.hidden_by.is_none())
        .filter(|b| keymap::place_of(&b.context).place == "File list")
        .rfind(|b| keymap::predicate_holds(&b.context, &holds))
        .and_then(|b| b.command.as_ref())
        .map(|(command, _)| command.clone())
        .unwrap_or_default()
}

#[test]
fn escape_leaves_a_search_for_its_folder_and_nothing_else() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("it's a folder");
    std::fs::create_dir(&root).unwrap();
    for name in ["notes one.txt", "notes two.txt", "other.md"] {
        std::fs::write(root.join(name), name).unwrap();
    }
    let app = Arc::new(App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap());
    for spec in skwir_ui::views::specs() {
        app.registry.views.add("ui", spec);
    }
    skwir_ui::commands::declare_all(&app);
    for plugin in skwir::plugins() {
        if plugin.enabled_by_default() {
            app.plugins.add(plugin, &app);
        }
    }
    let bound = keymap::bindings(&app, &[]);
    let pane = app.workspace.read().active_id();
    let folder = Location::from_path(&root);
    app.navigate(pane, &folder).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));

    // In a folder, Escape shows everything again, then deselects: it
    // never leaves, and with nothing selected it forgets no cut.
    let mut filtered = flags(&app, pane, 0);
    filtered.push("filtered");
    assert_eq!(escape_runs(&bound, &filtered), "filter");
    assert_eq!(escape_runs(&bound, &flags(&app, pane, 1)), "select_none");
    assert_eq!(escape_runs(&bound, &flags(&app, pane, 0)), "select_none");

    // A folder's tags keep their trail: Escape stays there too.
    app.dispatch(&Invocation::with_args(
        "tags.browse",
        &serde_json::json!({ "toggle": true }),
    ))
    .unwrap();
    assert_ne!(app.pane_location(pane), Some(folder.clone()));
    assert_eq!(escape_runs(&bound, &flags(&app, pane, 0)), "select_none");
    app.navigate(pane, &folder).unwrap();

    // A search deselects first, then goes to the folder it looks in.
    app.dispatch(&Invocation::with_args(
        "query.search",
        &serde_json::json!({ "expr": "notes" }),
    ))
    .unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    let search = app.pane_location(pane).unwrap();
    assert!(app.is_search(&search), "{search}");
    assert_eq!(escape_runs(&bound, &flags(&app, pane, 1)), "select_none");
    let leaves = escape_runs(&bound, &flags(&app, pane, 0));
    assert_eq!(leaves, "up");
    app.dispatch(&Invocation::new(leaves.as_str())).unwrap();
    assert_eq!(app.pane_location(pane), Some(folder));
}
