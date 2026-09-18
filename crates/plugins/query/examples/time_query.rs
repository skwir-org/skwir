//! Time a query to completion: `cargo run --release -p bibli-plugin-query --example time_query -- '<expr>'`
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let expr = std::env::args().nth(1).expect("query expression");
    let tmp = tempfile::tempdir().unwrap();
    let app = bibli_core::App::new(bibli_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
    app.plugins
        .add(Arc::new(bibli_plugin_query::QueryPlugin), &app);
    let t = Instant::now();
    let open = app
        .open_location(&bibli_core::Location::parse(&format!("query:{expr}")))
        .unwrap();
    let mut n = 0usize;
    let mut first = None;
    loop {
        match open.events().recv().unwrap() {
            bibli_core::ListingEvent::Added(b) => {
                first.get_or_insert_with(|| t.elapsed());
                n += b.len();
            }
            bibli_core::ListingEvent::Complete => break,
            bibli_core::ListingEvent::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    println!(
        "{n} results, first batch {:?}, complete {:?}",
        first.unwrap_or_default(),
        t.elapsed()
    );
}
