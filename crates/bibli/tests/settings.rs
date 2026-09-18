//! Settings changes refresh providers without disabling the owning plugin.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use bibli_core::{App, app::AppConfig, command::Invocation, settings::SaveState};

#[test]
fn changing_document_engine_refreshes_only_document_previews() {
    let tmp = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
    app.plugins.add(
        Arc::new(bibli_plugin_preview_documents::PreviewPlugin),
        &app,
    );
    app.plugins
        .add(Arc::new(bibli_plugin_preview_media::PreviewPlugin), &app);
    let revision = app.previews.revision();
    let providers = app.previews.ids();
    let (tx, rx) = std::sync::mpsc::channel();
    app.set_waker(Some(Arc::new(move || {
        let _ = tx.send(());
    })));
    app.dispatch(&Invocation::with_args(
        "settings.apply",
        &BTreeMap::from([(
            "changes",
            BTreeMap::from([("preview-documents.backend", "poppler")]),
        )]),
    ))
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match app.settings.save_status().1 {
            SaveState::Saved => break,
            SaveState::Failed(error) => panic!("{error}"),
            _ => {
                rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                    .unwrap();
            }
        }
    }
    assert!(app.previews.revision() > revision);
    let mut after = app.previews.ids();
    let mut before = providers;
    after.sort_unstable();
    before.sort_unstable();
    assert_eq!(after, before);
    assert!(app.plugins.is_enabled("preview-documents"));
    assert!(app.plugins.is_enabled("preview-media"));
    assert_eq!(
        app.settings
            .get::<String>("preview-documents.backend")
            .as_deref(),
        Some("poppler")
    );
}
