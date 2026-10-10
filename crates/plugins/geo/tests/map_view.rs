//! The Map view's data path, headless: `geo.scan` on what a pane lists,
//! `geo.points` handing over what was found, each point once. Invented
//! coordinates only.
// Types from the free desktops' database (step E, ADR-035).
#![cfg(target_os = "linux")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use bibli_core::app::{App, AppConfig};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::Location;
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use serde_json::{Value, json};

/// A TIFF whose first directory points at a GPS directory: latitude and
/// longitude in whole degrees, and an optional status (`V`: the receiver
/// had no fix).
fn tiff(lat: u32, lon: u32, status: Option<u8>) -> Vec<u8> {
    fn entry(data: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: [u8; 4]) {
        data.extend(tag.to_le_bytes());
        data.extend(kind.to_le_bytes());
        data.extend(count.to_le_bytes());
        data.extend(value);
    }
    let entries = if status.is_some() { 5u16 } else { 4 };
    let gps_at = 8 + 18u32;
    let values_at = gps_at + 2 + 12 * u32::from(entries) + 4;
    let mut data = b"II*\0\x08\0\0\0".to_vec();
    data.extend(1u16.to_le_bytes());
    entry(&mut data, 0x8825, 4, 1, gps_at.to_le_bytes());
    data.extend(0u32.to_le_bytes());
    data.extend(entries.to_le_bytes());
    entry(&mut data, 0x0001, 2, 2, [b'N', 0, 0, 0]);
    entry(&mut data, 0x0002, 5, 3, values_at.to_le_bytes());
    entry(&mut data, 0x0003, 2, 2, [b'E', 0, 0, 0]);
    entry(&mut data, 0x0004, 5, 3, (values_at + 24).to_le_bytes());
    if let Some(status) = status {
        entry(&mut data, 0x0009, 2, 2, [status, 0, 0, 0]);
    }
    data.extend(0u32.to_le_bytes());
    for part in [lat, 0, 0, lon, 0, 0] {
        data.extend(part.to_le_bytes());
        data.extend(1u32.to_le_bytes());
    }
    data
}

const WALK: &str = r#"<?xml version="1.0"?>
<gpx version="1.1" creator="example" xmlns="http://www.topografix.com/GPX/1/1">
  <trk><name>Example walk</name><trkseg>
    <trkpt lat="10.5" lon="20.5"><time>2026-07-01T08:00:00Z</time></trkpt>
    <trkpt lat="10.6" lon="20.6"><time>2026-07-01T08:30:00Z</time></trkpt>
    <trkpt lat="10.7" lon="20.5"><time>2026-07-01T09:00:00Z</time></trkpt>
  </trkseg></trk>
</gpx>"#;

fn points(app: &App, pane: PaneId, since: usize) -> Value {
    match app
        .dispatch(&Invocation::with_args(
            "geo.points",
            &json!({ "pane": pane, "since": since }),
        ))
        .unwrap()
    {
        Outcome::Value(value) => value,
        _ => panic!("geo.points returns a value"),
    }
}

/// Scan what the pane lists and wait for the scan to end.
fn scan(app: &App, pane: PaneId) -> Value {
    app.dispatch(&Invocation::with_args("geo.scan", &json!({ "pane": pane })))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found = points(app, pane, 0);
        if found["complete"] == true {
            return found;
        }
        assert!(Instant::now() < deadline, "the scan did not end: {found}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_folder_s_pictures_and_tracks_are_placed_once_each() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Trip d'Example (day one)");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("Lake shore.tif"), tiff(12, 34, None)).unwrap();
    std::fs::write(dir.join("No fix.tif"), tiff(12, 34, Some(b'V'))).unwrap();
    std::fs::write(dir.join("Walk.gpx"), WALK).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a place").unwrap();

    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_geo::GeoPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    app.navigate(pane, &Location::from_path(&dir)).unwrap();
    // The view asks before the folder has arrived: that scan is not the
    // folder's, however soon it ends.
    app.dispatch(&Invocation::with_args("geo.scan", &json!({ "pane": pane })))
        .unwrap();
    let early = points(&app, pane, 0);
    assert_eq!(early["complete"], false);
    assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
    let id_of = |name: &str| {
        let ws = app.workspace.read();
        let listing = &ws.pane(pane).unwrap().listing;
        serde_json::to_value(listing.find_by_name(name.as_ref()).unwrap().id).unwrap()
    };

    let found = scan(&app, pane);
    // It went on from the early scan, which took nothing away: the view
    // keeps what it was handed, and is handed only what was added.
    assert_eq!(found["generation"], early["generation"], "{found}");
    // The text file is not looked at; the picture without a fix is looked
    // at and not placed.
    assert_eq!(found["candidates"], 3, "{found}");
    assert_eq!(found["looked"], 3, "{found}");
    assert_eq!(found["placed"], 2, "{found}");
    let placed = found["points"].as_array().unwrap();
    let picture = placed.iter().find(|p| p["id"] == id_of("Lake shore.tif"));
    let picture = picture.expect("the picture is placed");
    assert_eq!(
        (picture["lat"].as_f64(), picture["lon"].as_f64()),
        (Some(12.), Some(34.))
    );
    assert!(picture.get("line").is_none(), "a picture has no line");
    let walk = placed.iter().find(|p| p["id"] == id_of("Walk.gpx"));
    let walk = walk.expect("the track is placed");
    assert_eq!(
        (walk["lat"].as_f64(), walk["lon"].as_f64()),
        (Some(10.5), Some(20.5))
    );
    assert_eq!(walk["line"][0].as_array().unwrap().len(), 3);

    // Each point travels once.
    let after = points(&app, pane, 2);
    assert_eq!(after["points"], json!([]));
    assert_eq!(after["generation"], found["generation"]);

    // The same listing asked again is the same scan.
    let again = scan(&app, pane);
    assert_eq!(again["generation"], found["generation"]);

    // Hydration gives the listing its modification times, first learned:
    // no file changed, and the scan stats each file itself, so the same
    // scan stands, looked at nothing again.
    let modified = || {
        let ws = app.workspace.read();
        ws.pane(pane).unwrap().listing.modified_version()
    };
    let before = modified();
    let (open, ids) = {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        (
            p.open.clone().unwrap(),
            p.listing.iter().map(|e| e.id).collect::<Vec<_>>(),
        )
    };
    open.hydrate(&ids, Priority::Visible);
    let deadline = Instant::now() + Duration::from_secs(5);
    while modified() == before {
        assert!(Instant::now() < deadline, "hydration moved no time");
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(10));
    }
    let quiet = scan(&app, pane);
    assert_eq!(
        quiet["rechecking"], false,
        "nothing looked at again: {quiet}"
    );
    assert_eq!(quiet["generation"], found["generation"], "{quiet}");
    assert_eq!(quiet["placed"], 2);

    // A file that changed is read again: the receiver found its fix.
    std::fs::write(dir.join("No fix.tif"), tiff(12, 35, None)).unwrap();
    app.dispatch(&Invocation::new("refresh")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        let rescanned = scan(&app, pane);
        if rescanned["placed"] == 3 {
            assert_ne!(rescanned["generation"], found["generation"]);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the changed picture: {rescanned}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    // Released, the pane has nothing to hand over.
    app.dispatch(&Invocation::with_args(
        "geo.release",
        &json!({ "pane": pane }),
    ))
    .unwrap();
    assert_eq!(points(&app, pane, 0), Value::Null);
}
