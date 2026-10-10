//! Time a query to completion: `cargo run --release -p skwir-plugin-query --example time_query -- '<expr>'`
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let expr = std::env::args().nth(1).expect("query expression");
    let tmp = tempfile::tempdir().unwrap();
    let app = skwir_core::App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    let t = Instant::now();
    let open = app
        .open_location(&skwir_core::Location::parse(&format!("query:{expr}")))
        .unwrap();
    let mut n = 0usize;
    let mut first = None;
    loop {
        match open.events().recv().unwrap() {
            skwir_core::ListingEvent::Added(b) => {
                first.get_or_insert_with(|| t.elapsed());
                n += b.len();
            }
            skwir_core::ListingEvent::Complete => break,
            skwir_core::ListingEvent::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    println!(
        "{n} results, first batch {:?}, complete {:?}",
        first.unwrap_or_default(),
        t.elapsed()
    );
}
