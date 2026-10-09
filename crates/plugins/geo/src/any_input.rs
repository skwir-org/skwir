//! Every reader of the plugin given what no file should hold: its own
//! test's sample mutated, and bytes at random. A reader may refuse the
//! file or say it is damaged; it never panics.

use bibli_core::cancel::CancelToken;
use bibli_core::testing::{bytes, mutated, mutated_text};
use proptest::prelude::*;

use crate::collect::Collector;

/// Sentences as receivers write them, with a fix, a time and a date.
const NMEA: &str = "$GPRMC,080000,A,1000.000,N,02000.000,E,0.0,0.0,120326,,\r\n\
$GPGGA,080001,1000.060,N,02000.000,E,1,08,0.9,545.4,M,46.9,M,,*47\r\n\
$GNRMC,080003.00,A,1000.120,N,02000.000,E,0.0,0.0,120326,,,A*00\r\n\
$GPVTG,054.7,T,034.4,M,005.5,N,010.2,K*48\r\n";

/// Points, a line and a polygon, with names and heights.
const GEOJSON: &str = r#"{"type":"FeatureCollection","features":[
{"type":"Feature","properties":{"name":"Hut"},"geometry":{"type":"Point","coordinates":[6.001,45.001,1510]}},
{"type":"Feature","properties":{"name":"Path"},"geometry":{"type":"LineString","coordinates":[[6.0,45.0],[6.01,45.02,1600]]}},
{"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[[[6.0,45.0],[6.1,45.0],[6.1,45.1],[6.0,45.0]]]}},
{"type":"Feature","geometry":{"type":"MultiPoint","coordinates":[[-170.5,-10.25]]}}]}"#;

/// Each format's reader, run on `$sample` mutated and on random bytes.
macro_rules! any_input {
    ($($name:ident: $read:path, $sample:expr;)*) => {
        proptest! {
            $(
                #[test]
                fn $name(input in prop_oneof![mutated($sample), bytes(2048)]) {
                    let mut into = Collector::new();
                    let _ = $read(&input[..], &mut into, &CancelToken::new());
                }
            )*
        }
    };
}

any_input! {
    a_gpx_track: crate::gpx::read, crate::gpx::tests::HIKE;
    a_kml_document: crate::kml::read, crate::kml::tests::TRIP;
    a_tcx_activity: crate::tcx::read, crate::tcx::tests::RIDE;
    a_fit_activity: crate::fit::read, crate::fit::tests::ride();
    an_igc_flight: crate::igc::read, crate::igc::tests::FLIGHT;
    an_nmea_log: crate::nmea::read, NMEA;
    a_garmin_plan: crate::fpl::read, crate::fpl::tests::PLAN;
    a_simulator_plan: crate::pln::read, crate::pln::tests::PLAN_FILE;
    an_x_plane_plan: crate::fms::read, crate::fms::tests::ELEVEN;
    a_little_navmap_plan: crate::lnmpln::read, crate::lnmpln::tests::PLAN;
    a_mavlink_mission: crate::mavlink::read, crate::mavlink::tests::SURVEY.join("\n");
    a_litchi_mission: crate::litchi::read, litchi();
    a_dji_mission: crate::wpml::read, crate::wpml::tests::WAYLINES;
}

fn litchi() -> String {
    use crate::litchi::tests::{litchi_header, row};
    [
        litchi_header("m"),
        row("45.0", "6.0", 30., 0, (45.001, 6.001, 0.)),
        row("45.001", "6.002", 35.5, 1, (0., 0., 0.)),
    ]
    .join("\n")
}

proptest! {
    #[test]
    fn a_qgroundcontrol_plan(text in mutated_text(crate::qgc::tests::PLAN)) {
        if let Ok(document) = serde_json::from_str(&text) {
            crate::qgc::read(&document, &mut Collector::new());
        }
    }

    #[test]
    fn a_geojson_document(text in mutated_text(GEOJSON)) {
        if let Ok(document) = serde_json::from_str(&text) {
            let _ = crate::geojson::read(
                &document,
                &mut Collector::new(),
                &bibli_core::CancelToken::new(),
            );
        }
    }

    #[test]
    fn a_clock_and_an_offset_typed(text in prop_oneof![
        mutated_text("2026:03:12 08:00:00"),
        mutated_text("+02:00"),
        any::<String>(),
    ]) {
        let _ = crate::geotag::parse_clock(&text);
        let _ = crate::geotag::parse_offset(&text);
    }
}
