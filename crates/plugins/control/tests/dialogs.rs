//! Another program's Open or Save dialog is the user's alone (ADR-141, 8):
//! an agent's or a script's call never lists the window answering it,
//! names it, or acts in it for want of naming another.

use serde_json::{Value, json};
use skwir_core::Location;
use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::workspace::WindowId;
use skwir_plugin_control::PlanMode;

fn ids(value: &Value, key: &str) -> Vec<u64> {
    value
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|row| row[key].as_u64())
        .collect()
}

#[test]
fn the_window_answering_a_dialog_is_out_of_reach() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
    let own = app.workspace.read().active_window_id();
    let own_tab = app.workspace.read().active_tab().id;
    let (dialog, dialog_tab, dialog_pane) =
        app.open_window(Location::from_path(tmp.path())).unwrap();
    {
        let mut ws = app.workspace.write();
        ws.set_kind(dialog, skwir_core::workspace::WindowKind::Chooser);
        // The user is in it: the core's window used last.
        assert!(ws.set_active_window(dialog));
    }
    let tabs_of = |window: WindowId| {
        app.workspace
            .read()
            .window(window)
            .map_or(0, |w| w.tabs().len())
    };
    for mode in [PlanMode::Propose, PlanMode::Apply] {
        let run = |name: &str, args: Value| {
            skwir_plugin_control::run(&app, &Invocation::with_args(name, &args), mode, &|| false)
        };
        let windows = run("window.list", json!({})).unwrap().value;
        assert_eq!(ids(&windows, "id"), [u64::from(own.0)], "{mode:?}");
        let tabs = run("tab.list", json!({})).unwrap().value;
        assert!(
            !ids(&tabs, "window").contains(&u64::from(dialog.0)),
            "{mode:?}: {tabs}"
        );
        for (name, args) in [
            ("window.close", json!({ "window": dialog })),
            ("window.select", json!({ "window": dialog })),
            ("tab.close", json!({ "tab": dialog_tab })),
            ("tab.select", json!({ "tab": dialog_tab })),
            ("tab.move", json!({ "tab": own_tab, "window": dialog })),
            ("tab.new", json!({ "pane": dialog_pane })),
            ("window.new", json!({ "pane": dialog_pane })),
        ] {
            let refused = run(name, args).expect_err(name);
            assert!(
                refused.to_string().contains("another program's"),
                "{mode:?} {name}: {refused}"
            );
        }
        // Naming none, a call acts in the user's window, the one used last
        // of theirs.
        run("tab.new", json!({ "location": tmp.path() })).unwrap();
        assert_eq!(tabs_of(dialog), 1, "{mode:?}: no tab in the dialog's");
    }
    assert_eq!(tabs_of(own), 3, "a tab opened in the user's, twice");
    // The window itself still acts in it, as the user does.
    assert_eq!(app.workspace.read().active_window_id(), dialog);
    // Closing what is named the active window closes the user's, never
    // the dialog's.
    skwir_plugin_control::run(
        &app,
        &Invocation::with_args("window.close", &json!({})),
        PlanMode::Propose,
        &|| false,
    )
    .unwrap();
    let ws = app.workspace.read();
    assert!(ws.window(dialog).is_some() && ws.window(own).is_none());
}
