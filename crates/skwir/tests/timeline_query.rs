//! Exercise the command used by timeline bar clicks with the real query provider.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::location::Location;

#[test]
fn timeline_selection_preserves_paths_and_filters_the_period() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins.add(
        Arc::new(skwir_plugin_timeline::TimelinePlugin::default()),
        &app,
    );
    let pane = app.workspace.read().active_id();
    // A bar is a day of the user's calendar: 2026-07-01 from its midnight
    // where the test runs.
    let cal = skwir_platform::calendar::Calendar::system();
    let day = skwir_platform::calendar::days_from_civil(2026, 7, 1);
    let secs = |t: std::time::SystemTime| t.duration_since(UNIX_EPOCH).unwrap().as_secs();
    let (start, end) = (secs(cal.day_start(day)), secs(cal.day_start(day + 1)));
    let mut dirnames = vec![
        "2026-08 Trip d'Example - Summit",
        "d'Example",
        "two words",
        "trip (photos)",
    ];
    // Quotes, backslashes and control characters: names Windows refuses.
    if cfg!(unix) {
        dirnames.extend([
            "trip \"photos\"",
            "trip \\photos\\",
            "trip\tphotos",
            "trip\nphotos",
        ]);
    }
    for dirname in dirnames {
        let root = tmp.path().join(dirname);
        std::fs::create_dir(&root).unwrap();
        for (name, modified) in [
            ("before.jpg", start - 1),
            ("start.jpg", start),
            ("inside.jpg", end - 1),
            ("end.jpg", end),
        ] {
            std::fs::File::create(root.join(name))
                .unwrap()
                .set_modified(UNIX_EPOCH + Duration::from_secs(modified))
                .unwrap();
        }
        app.navigate(pane, &Location::from_path(&root)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::with_args(
            "timeline.select",
            &BTreeMap::from([("start", start), ("end", end)]),
        ))
        .unwrap_or_else(|e| panic!("{dirname:?}: {e:#}"));
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        let location = p.location().unwrap();
        assert_eq!(location.scheme.as_ref(), "query");
        assert_eq!(
            skwir_plugin_query::lang::parse(&location.rest)
                .unwrap()
                .root,
            root
        );
        assert_eq!(
            p.listing
                .iter()
                .map(|e| e.name_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["start.jpg".into(), "inside.jpg".into()]),
            "{dirname:?}"
        );
    }
}

/// A bar of a search's timeline narrows that search: a month of a folder,
/// then a day of that month, its root and depth kept.
#[test]
fn a_bar_of_a_search_refines_it() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins.add(
        Arc::new(skwir_plugin_timeline::TimelinePlugin::default()),
        &app,
    );
    let pane = app.workspace.read().active_id();
    let cal = skwir_platform::calendar::Calendar::system();
    let day = |y, m, d| skwir_platform::calendar::days_from_civil(y, m, d);
    let secs = |day: i64| {
        cal.day_start(day)
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    };
    let root = tmp.path().join("d'Example (2)");
    std::fs::create_dir_all(root.join("inner")).unwrap();
    for (name, modified) in [
        ("june.jpg", secs(day(2026, 6, 30))),
        ("first.jpg", secs(day(2026, 7, 1))),
        ("second.jpg", secs(day(2026, 7, 2)) + 60),
        ("inner/deep.jpg", secs(day(2026, 7, 1)) + 60),
    ] {
        std::fs::File::create(root.join(name))
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(modified))
            .unwrap();
    }
    let names = || -> BTreeSet<String> {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        p.listing
            .iter()
            .map(|e| e.name_str().into_owned())
            .collect()
    };
    let select = |start: i64, end: i64| {
        app.dispatch(&Invocation::with_args(
            "timeline.select",
            &serde_json::json!({ "start": secs(start), "end": secs(end), "subtree": false }),
        ))
        .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    };
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    // July, in the folder: what it holds itself.
    select(day(2026, 7, 1), day(2026, 8, 1));
    assert_eq!(
        names(),
        BTreeSet::from(["first.jpg".into(), "second.jpg".into()])
    );
    // Its first day, in that search.
    select(day(2026, 7, 1), day(2026, 7, 2));
    assert_eq!(names(), BTreeSet::from(["first.jpg".into()]));
    let location = app.pane_location(pane).unwrap();
    assert_eq!(location.scheme.as_ref(), "query");
    let query = skwir_plugin_query::lang::parse(&location.rest).unwrap();
    assert_eq!(query.root, root);
    // The day in place of the month, not beside it.
    assert!(
        location.rest.contains("modified:2026-07-01") && !location.rest.contains("2026-07 "),
        "{}",
        location.rest
    );
    assert_eq!(
        location.rest.matches("modified").count(),
        1,
        "{}",
        location.rest
    );
    // Back is the month again.
    app.dispatch(&Invocation::new("back")).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    assert_eq!(
        names(),
        BTreeSet::from(["first.jpg".into(), "second.jpg".into()])
    );
}
