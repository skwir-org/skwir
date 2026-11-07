//! Windows (ADR-122): each shows its own tabs, one at a time; the tabs
//! hidden in each sleep; a window's last tab closing closes the window,
//! which comes back whole; tabs move between windows; every window comes
//! back after a restart with its tabs and its shape.

mod common;

use std::path::Path;
use std::time::Duration;

use common::{active, app, home_tempdir, navigate};
use skwir_core::App;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::Location;
use skwir_core::tabs::{LAST_TABS, LAST_WINDOWS, SavedTabs, SavedWindows};
use skwir_core::workspace::{PaneId, TabId, WindowId};

const WAIT: Duration = Duration::from_secs(10);

#[allow(clippy::needless_pass_by_value)] // Callers write `json!(…)` in place.
fn run(app: &App, name: &str, args: serde_json::Value) -> Outcome {
    app.dispatch(&Invocation::with_args(name, &args))
        .unwrap_or_else(|e| panic!("{name}: {e:#}"))
}

#[allow(clippy::needless_pass_by_value)]
fn value(app: &App, name: &str, args: serde_json::Value) -> serde_json::Value {
    match run(app, name, args) {
        Outcome::Value(value) => value,
        other => panic!("{name}: no value but {other:?}"),
    }
}

fn windows(app: &App) -> Vec<WindowId> {
    app.workspace
        .read()
        .windows()
        .iter()
        .map(|w| w.id)
        .collect()
}

fn active_window(app: &App) -> WindowId {
    app.workspace.read().active_window_id()
}

fn tabs_of(app: &App, window: WindowId) -> Vec<TabId> {
    let ws = app.workspace.read();
    ws.window(window)
        .map(|w| w.tabs().iter().map(|t| t.id).collect())
        .unwrap_or_default()
}

fn shown_in(app: &App, window: WindowId) -> TabId {
    app.workspace
        .read()
        .window(window)
        .expect("window")
        .shown()
        .id
}

fn pane_of(app: &App, tab: TabId) -> PaneId {
    app.workspace.read().tab(tab).expect("tab").active
}

fn location_of(app: &App, pane: PaneId) -> Option<Location> {
    app.workspace.read().pane(pane)?.shown().cloned()
}

fn is_dormant(app: &App, pane: PaneId) -> bool {
    app.workspace.read().pane(pane).expect("pane").is_dormant()
}

fn id<T: serde::de::DeserializeOwned>(value: &serde_json::Value, key: &str) -> T {
    serde_json::from_value(value[key].clone()).unwrap_or_else(|e| panic!("{key}: {e}"))
}

/// Folders whose names carry what breaks naive handling: an apostrophe,
/// spaces, Unicode.
fn tree(root: &Path) {
    for dir in ["it's a folder", "second folder", "tröisième"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("a file.txt"), b"x").unwrap();
}

#[test]
fn a_new_window_shows_the_pane_s_folder_and_commands_act_in_it() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active_window(&app);
    let from = active(&app);
    let made = value(&app, "window.new", serde_json::json!({}));
    let window: WindowId = id(&made, "window");
    let pane: PaneId = id(&made, "pane");
    assert_eq!(windows(&app), vec![first, window]);
    assert_eq!(
        active_window(&app),
        window,
        "the new window is the one in use"
    );
    assert_eq!(active(&app), pane);
    assert_eq!(location_of(&app, pane), location_of(&app, from));
    assert!(app.pump_until_complete(pane, WAIT), "its folder is listed");
    assert_eq!(
        app.workspace.read().window(window).unwrap().made_from(),
        Some(first)
    );

    // A tab opened without a pane named goes to the window in use.
    run(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("second folder") }),
    );
    assert_eq!(tabs_of(&app, first).len(), 1);
    assert_eq!(tabs_of(&app, window).len(), 2);
    // One opened from a pane goes to that pane's window.
    run(
        &app,
        "tab.new",
        serde_json::json!({ "pane": from, "location": dir.path().join("tröisième") }),
    );
    assert_eq!(tabs_of(&app, first).len(), 2);

    let listed = value(&app, "window.list", serde_json::json!({}));
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[1]["active"], serde_json::json!(true));
    let tabs = value(&app, "tab.list", serde_json::json!({}));
    let in_new = tabs
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["window"] == serde_json::json!(window))
        .count();
    assert_eq!(in_new, 2, "tab.list says each tab's window");
}

#[test]
fn each_window_shows_its_tab_and_lets_its_hidden_tabs_sleep() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active_window(&app);
    let a = active(&app);
    let made = value(
        &app,
        "window.new",
        serde_json::json!({ "location": dir.path().join("it's a folder") }),
    );
    let b = id::<PaneId>(&made, "pane");
    assert!(app.pump_until_complete(b, WAIT));
    let opened = value(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("second folder") }),
    );
    let c: PaneId = id(&opened, "pane");
    assert!(app.pump_until_complete(c, WAIT));
    // The second window shows its new tab: its first sleeps, the first
    // window's tab stays listed, shown there.
    assert!(is_dormant(&app, b));
    assert!(
        !is_dormant(&app, a),
        "another window's tab shown stays listed"
    );
    assert_eq!(
        shown_in(&app, first),
        app.workspace.read().tab_of(a).unwrap()
    );

    // Shown again by position in the window in use.
    run(&app, "tab.select", serde_json::json!({ "index": 0 }));
    assert!(!is_dormant(&app, b));
    assert!(is_dormant(&app, c));
    assert!(!is_dormant(&app, a));
}

#[test]
fn a_window_s_last_tab_closes_it_and_it_comes_back_whole() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active_window(&app);
    let made = value(
        &app,
        "window.new",
        serde_json::json!({ "location": dir.path().join("it's a folder") }),
    );
    let second: WindowId = id(&made, "window");
    let pane: PaneId = id(&made, "pane");
    assert!(app.pump_until_complete(pane, WAIT));
    app.navigate(pane, &Location::from_path(dir.path().join("tröisième")))
        .unwrap();
    assert!(app.pump_until_complete(pane, WAIT));
    let other = value(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("second folder"), "show": false }),
    );
    let other_tab: TabId = id(&other, "tab");
    app.workspace
        .write()
        .set_shape(second, Some(toml::Value::String("its shape".into())));

    // Closing its tabs one by one: the last one takes the window.
    run(&app, "tab.close", serde_json::json!({ "tab": other_tab }));
    assert_eq!(windows(&app).len(), 2);
    run(&app, "tab.close", serde_json::json!({}));
    assert_eq!(windows(&app), vec![first], "a window's last tab closes it");
    assert_eq!(active_window(&app), first);

    // Reopened: the window, its tab, its history and its shape.
    let reopened = value(&app, "tab.reopen", serde_json::json!({}));
    let back: WindowId = id(&reopened, "window");
    assert_eq!(windows(&app).len(), 2);
    assert_eq!(active_window(&app), back);
    let shown = pane_of(&app, shown_in(&app, back));
    assert_eq!(
        location_of(&app, shown),
        Some(Location::from_path(dir.path().join("tröisième")))
    );
    assert!(app.pump_until_complete(shown, WAIT));
    run(&app, "back", serde_json::json!({ "pane": shown }));
    assert_eq!(
        location_of(&app, shown),
        Some(Location::from_path(dir.path().join("it's a folder")))
    );
    assert_eq!(
        app.workspace.read().window(back).unwrap().shape(),
        Some(&toml::Value::String("its shape".into()))
    );
    assert_eq!(
        app.workspace.read().window(back).unwrap().made_from(),
        Some(first)
    );

    // The last window's last tab stays, and so does the last window.
    run(&app, "window.close", serde_json::json!({ "window": back }));
    run(&app, "tab.close", serde_json::json!({}));
    assert_eq!(windows(&app), vec![first]);
    assert_eq!(tabs_of(&app, first).len(), 1);
    assert!(
        app.dispatch(&Invocation::with_args(
            "window.close",
            &serde_json::json!({})
        ))
        .is_err(),
        "the last window closes only from the window"
    );
}

#[test]
fn tabs_move_between_windows_and_out_to_new_ones() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active_window(&app);
    let kept = shown_in(&app, first);
    let opened = value(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("second folder") }),
    );
    let moving: TabId = id(&opened, "tab");
    let made = value(
        &app,
        "window.new",
        serde_json::json!({ "location": dir.path().join("tröisième") }),
    );
    let second: WindowId = id(&made, "window");
    let theirs = shown_in(&app, second);

    // To another window: shown there, that window in use; the window it
    // left shows its other tab, and the tab shown there before sleeps.
    let moved = value(
        &app,
        "tab.move",
        serde_json::json!({ "tab": moving, "window": second, "to": 0 }),
    );
    assert_eq!(id::<WindowId>(&moved, "window"), second);
    assert_eq!(tabs_of(&app, second), vec![moving, theirs]);
    assert_eq!(shown_in(&app, second), moving);
    assert_eq!(active_window(&app), second);
    assert_eq!(tabs_of(&app, first), vec![kept]);
    assert!(!is_dormant(&app, pane_of(&app, kept)));
    assert!(!is_dormant(&app, pane_of(&app, moving)));
    assert!(is_dormant(&app, pane_of(&app, theirs)));

    // Out to a new window of its own, made from its window.
    let out = value(
        &app,
        "tab.move",
        serde_json::json!({ "tab": theirs, "new_window": true }),
    );
    let third: WindowId = id(&out, "window");
    assert_eq!(tabs_of(&app, third), vec![theirs]);
    assert_eq!(
        app.workspace.read().window(third).unwrap().made_from(),
        Some(second)
    );
    assert!(!is_dormant(&app, pane_of(&app, theirs)), "shown there");
    // A tab alone in its window goes nowhere new.
    assert!(
        app.dispatch(&Invocation::with_args(
            "tab.move",
            &serde_json::json!({ "tab": theirs, "new_window": true })
        ))
        .is_err()
    );
    // Alone in its window, moved to another: its window goes with it.
    run(
        &app,
        "tab.move",
        serde_json::json!({ "tab": theirs, "window": first }),
    );
    assert_eq!(windows(&app), vec![first, second]);
    assert_eq!(tabs_of(&app, first), vec![kept, theirs]);
    assert_eq!(active_window(&app), first);
}

#[test]
fn the_active_pane_is_in_the_active_window() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active_window(&app);
    let a = active(&app);
    let hidden = value(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("second folder"), "show": false }),
    );
    let hidden: PaneId = id(&hidden, "pane");
    let made = value(&app, "window.new", serde_json::json!({}));
    let second: WindowId = id(&made, "window");
    assert_eq!(active_window(&app), second);
    // Going to a pane of the other window goes to that window.
    assert!(app.workspace.write().set_active(a));
    assert_eq!(active_window(&app), first);
    assert_eq!(active(&app), a);
    // A pane of a tab its window does not show is not one to go to.
    assert!(!app.workspace.write().set_active(hidden));
    run(
        &app,
        "window.select",
        serde_json::json!({ "window": second }),
    );
    assert_eq!(active_window(&app), second);
}

#[test]
fn every_window_comes_back_with_its_tabs_and_its_shape() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    let first = active_window(&app);
    run(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("it's a folder"), "show": false }),
    );
    let made = value(
        &app,
        "window.new",
        serde_json::json!({ "location": dir.path().join("second folder") }),
    );
    let second: WindowId = id(&made, "window");
    run(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("tröisième") }),
    );
    {
        let mut ws = app.workspace.write();
        ws.set_shape(first, Some(toml::Value::String("first".into())));
        ws.set_shape(second, Some(toml::Value::String("second".into())));
        ws.set_active_window(first);
    }
    let saved = app.saved_windows();
    assert_eq!(saved.windows.len(), 2);
    assert_eq!(saved.active, 0);
    assert_eq!(saved.windows[1].tabs.len(), 2);
    assert_eq!(saved.windows[1].shown, 1);

    // What the state keeps reads back the same.
    let kept = toml::Value::try_from(&saved).unwrap();
    assert_eq!(kept.clone().try_into::<SavedWindows>().unwrap(), saved);

    // Recorded only to come back to; read back from there.
    let settings = &app.settings;
    assert!(!skwir_core::tabs::record(&app));
    settings.set(
        skwir_core::settings::STARTUP,
        toml::Value::String("last".into()),
    );
    assert!(skwir_core::tabs::record(&app));
    assert_eq!(
        settings.get::<SavedWindows>(LAST_WINDOWS),
        Some(saved.clone())
    );
    let saved = skwir_core::tabs::last_windows(settings).expect("windows to restore");

    let next = common::app(dir.path());
    assert!(next.restore_windows(&saved));
    let back = windows(&next);
    assert_eq!(back.len(), 2);
    assert_eq!(
        active_window(&next),
        back[0],
        "the window in use comes back in use"
    );
    let ws = next.workspace.read();
    assert_eq!(
        ws.window(back[0]).unwrap().shape(),
        Some(&toml::Value::String("first".into()))
    );
    assert_eq!(
        ws.window(back[1]).unwrap().shape(),
        Some(&toml::Value::String("second".into()))
    );
    assert_eq!(ws.window(back[1]).unwrap().tabs().len(), 2);
    let shown_second = ws.window(back[1]).unwrap().shown().clone();
    assert_eq!(
        ws.tab_index(shown_second.id),
        Some(1),
        "each window's tab shown comes back shown"
    );
    assert!(
        ws.panes()
            .iter()
            .all(skwir_core::workspace::Pane::is_dormant)
    );
    drop(ws);
    next.resume_shown().unwrap();
    let dormant = next
        .workspace
        .read()
        .panes()
        .iter()
        .filter(|p| p.is_dormant())
        .count();
    assert_eq!(dormant, 2, "each window opens the one location it shows");
}

#[test]
fn tabs_kept_before_there_were_windows_come_back_as_one() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    navigate(&app, dir.path());
    run(
        &app,
        "tab.new",
        serde_json::json!({ "location": dir.path().join("it's a folder") }),
    );
    let before: SavedTabs = app.saved_tabs();
    let settings = &app.settings;
    settings.set(
        skwir_core::settings::STARTUP,
        toml::Value::String("last".into()),
    );
    settings.set(LAST_TABS, toml::Value::try_from(&before).unwrap());
    let saved = skwir_core::tabs::last_windows(settings).expect("the tabs kept");
    assert_eq!(saved.windows.len(), 1);
    assert_eq!(saved.windows[0].tabs, before.tabs);
    assert_eq!(saved.windows[0].shown, before.shown);

    // Recorded anew: as windows, the former key gone.
    assert!(skwir_core::tabs::record(&app));
    assert!(settings.get::<SavedWindows>(LAST_WINDOWS).is_some());
    let former = settings.get::<toml::Value>(LAST_TABS);
    assert!(
        former
            .as_ref()
            .is_none_or(|v| v.as_table().is_some_and(toml::Table::is_empty)),
        "{former:?}"
    );
    assert!(!skwir_core::tabs::record(&app), "nothing more to write");
}

/// A wake says what it is for (ADR-122): every window when it says
/// nothing (the queue, the tasks, a plugin's), the window a window's waker
/// names, the pane whose listing sent it; taking it leaves nothing.
#[test]
fn a_wake_says_what_it_is_for() {
    let dir = home_tempdir();
    tree(dir.path());
    let app = app(dir.path());
    let wakes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    app.set_waker(Some({
        let wakes = std::sync::Arc::clone(&wakes);
        std::sync::Arc::new(move || {
            wakes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })
    }));
    navigate(&app, dir.path());
    let pane = active(&app);
    assert!(app.pump_until_complete(pane, WAIT));
    let window = active_window(&app);
    let _ = app.take_woken();

    // One window's waker: that window alone.
    let before = wakes.load(std::sync::atomic::Ordering::Relaxed);
    (app.window_waker(window))();
    assert!(wakes.load(std::sync::atomic::Ordering::Relaxed) > before);
    let woken = app.take_woken();
    assert!(woken.windows.contains(&window));
    assert!(!woken.all);
    assert_eq!(app.take_woken(), skwir_core::app::Woken::default());

    // What says nothing, the tasks' included: every window.
    app.wake();
    assert!(app.take_woken().all);
    let task = app.tasks.start("a task", skwir_core::CancelToken::new());
    assert!(app.take_woken().all);
    drop(task);

    // A file come into the folder shown: its pane.
    let _ = app.take_woken();
    std::fs::write(dir.path().join("arrived.txt"), b"new").unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    let mut panes = std::collections::HashSet::new();
    while !panes.contains(&pane) && std::time::Instant::now() < deadline {
        panes.extend(app.take_woken().panes);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(panes.contains(&pane), "the listing woke its pane");
}

/// A plugin counting the windows that came to the front.
struct Looking(std::sync::atomic::AtomicUsize);

impl skwir_core::plugin::Plugin for Looking {
    fn id(&self) -> &'static str {
        "looking"
    }
    fn title(&self) -> &'static str {
        "Looking"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
    }
    fn register(&self, _app: &App) {}
    fn on_looked_at(&self, _app: &App) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Someone looks while a window has the keyboard, told in either order
/// going from one window to another and let go of as a window goes; each
/// window coming to the front tells the enabled plugins, on a worker.
#[test]
fn a_window_with_the_keyboard_is_looked_at_and_tells_the_plugins() {
    let dir = home_tempdir();
    let app = app(dir.path());
    navigate(&app, dir.path());
    let looking = std::sync::Arc::new(Looking(std::sync::atomic::AtomicUsize::new(0)));
    app.plugins.add(looking.clone(), &app);
    let told = || looking.0.load(std::sync::atomic::Ordering::SeqCst);
    let first = active_window(&app);
    let second: WindowId = id(&value(&app, "window.new", serde_json::json!({})), "window");
    assert!(!app.looked_at(), "no window has the keyboard yet");

    app.set_window_focused(first, true);
    assert!(app.looked_at());
    assert!(app.scheduler.wait_idle(WAIT));
    assert_eq!(told(), 1);

    // To the second: the first may be told it lost the keyboard after
    // the second took it.
    app.set_window_focused(second, true);
    app.set_window_focused(first, false);
    assert!(app.looked_at(), "the second has it");
    app.set_window_focused(second, false);
    assert!(!app.looked_at(), "another program has it");
    assert!(app.scheduler.wait_idle(WAIT));
    assert_eq!(told(), 2, "losing the keyboard tells nobody");

    // A window that goes with the keyboard lets go of it.
    app.set_window_focused(first, true);
    app.set_window_focused(first, false);
    assert!(!app.looked_at());
    assert!(app.scheduler.wait_idle(WAIT));
    assert_eq!(told(), 3);

    app.plugins.set_enabled("looking", false, &app);
    app.set_window_focused(second, true);
    assert!(app.scheduler.wait_idle(WAIT));
    assert_eq!(told(), 3, "a disabled plugin is not told");
}
