//! Exercise the command used by timeline bar clicks with the real query provider.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use bibli_core::app::{App, AppConfig};
use bibli_core::command::Invocation;
use bibli_core::location::Location;

#[test]
fn timeline_selection_preserves_paths_and_filters_the_period() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin), &app);
    app.plugins.add(
        Arc::new(bibli_plugin_timeline::TimelinePlugin::default()),
        &app,
    );
    let pane = app.workspace.read().active_id();
    let start = 1_782_864_000; // 2026-07-01 UTC
    let end = start + 86_400;
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
            bibli_plugin_query::lang::parse(&location.rest)
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
