//! A search and the pane's Show hidden files: one that does not say
//! follows the switch, searched again as it moves; `hidden:true` finds and
//! shows hidden files whatever it says, `hidden:false` never does; with no
//! pane (a script, an agent's listing) none are searched.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::location::{ListingEvent, Location};
use skwir_core::workspace::PaneId;

fn fixture(name: &str) -> PathBuf {
    let base = dirs::home_dir()
        .unwrap()
        .join(".cache/skwir-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

/// What the pane shows, once it is `expected`.
fn shows(app: &App, pane: PaneId, expected: &BTreeSet<String>, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.pump();
        let shown: BTreeSet<String> = {
            let ws = app.workspace.read();
            let p = ws.pane(pane).unwrap();
            let is_shown = p.listing.shows(p.sort);
            p.listing
                .iter()
                .filter(|e| is_shown(e.id))
                .map(|e| e.name.to_string_lossy().into_owned())
                .collect()
        };
        if shown == *expected {
            return;
        }
        assert!(Instant::now() < deadline, "{what}: {shown:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn search(root: &Path, expr: &str) -> Location {
    skwir_plugin_query::location_for(expr, Some(root))
}

#[test]
fn a_search_follows_the_panes_switch_unless_it_says() {
    let base = fixture("search-hidden");
    let root = base.join("it's a folder ü");
    std::fs::create_dir_all(root.join(".dir")).unwrap();
    for name in ["plain.txt", ".dot.txt", ".dir/inside.txt"] {
        std::fs::write(root.join(name), b"x").unwrap();
    }
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    let toggle = || {
        app.dispatch(&Invocation::new("toggle_hidden")).unwrap();
    };
    let (plain, all) = (
        set(&["plain.txt"]),
        set(&["plain.txt", ".dot.txt", "inside.txt"]),
    );

    app.navigate(pane, &search(&root, "ext:txt")).unwrap();
    shows(&app, pane, &plain, "switch off, nothing said");
    toggle();
    shows(
        &app,
        pane,
        &all,
        "switch on: searched again, hidden files too",
    );
    toggle();
    shows(&app, pane, &plain, "switch off again");

    app.navigate(pane, &search(&root, "ext:txt hidden:true"))
        .unwrap();
    shows(&app, pane, &all, "asked for, whatever the switch says");

    toggle();
    app.navigate(pane, &search(&root, "ext:txt hidden:false"))
        .unwrap();
    shows(&app, pane, &plain, "refused, whatever the switch says");

    // No pane: as a script's search, none, even with this pane's switch on.
    let open = app.open_location(&search(&root, "ext:txt")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut found = BTreeSet::new();
    loop {
        match open
            .events()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the search completes")
        {
            ListingEvent::Added(batch) => {
                found.extend(batch.iter().map(|e| e.name.to_string_lossy().into_owned()));
            }
            ListingEvent::Complete => break,
            ListingEvent::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    open.close();
    assert_eq!(found, plain);
    let _ = std::fs::remove_dir_all(&base);
}
