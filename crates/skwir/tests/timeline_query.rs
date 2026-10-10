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
    for dirname in [
        "2026-08 Trip d'Example - Summit",
        "d'Example",
        "two words",
        "trip (photos)",
        "trip \"photos\"",
        "trip \\photos\\",
        "trip\tphotos",
        "trip\nphotos",
    ] {
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
