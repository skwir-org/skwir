//! PDF thumbnails: the documents plugin renders a first page only after the
//! desktop's cache and thumbnailers, whichever plugin registers first, and
//! what it renders is shared through the desktop's cache.
use bibli_core::{
    App,
    app::AppConfig,
    op::EntryRef,
    plugin::Plugin,
    scheduler::Priority,
    thumbs::{Image, ThumbRequest, ThumbSize, ThumbState},
};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const DOCUMENTS: &str = "preview-documents";
const THUMBNAILS: &str = "thumbnails";

/// The documents plugin first: the reverse of the binary's order.
fn app(root: &Path) -> Arc<App> {
    let app = App::new(AppConfig::ephemeral(root.join("state"))).unwrap();
    for plugin in [
        Arc::new(bibli_plugin_preview_documents::PreviewPlugin) as Arc<dyn Plugin>,
        Arc::new(bibli_plugin_thumbnails::ThumbnailsPlugin::default()),
    ] {
        app.plugins.add(plugin, &app);
    }
    app
}

const ORDER: [&str; 4] = [
    "freedesktop-cache",
    "image-decoder",
    "thumbnailers",
    "pdf-pages",
];

#[test]
fn pdf_pages_come_after_the_desktop_whoever_registers_first() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    assert_eq!(app.thumbnails.provider_ids(), ORDER);
    // Re-registered by a settings change or a toggle: same order, no copy.
    for id in [THUMBNAILS, DOCUMENTS] {
        assert!(app.plugins.set_enabled(id, false, &app));
        assert!(app.plugins.set_enabled(id, true, &app));
        assert_eq!(app.thumbnails.provider_ids(), ORDER, "after toggling {id}");
    }
    app.settings.set(
        "preview-documents.backend",
        toml::Value::String("poppler".into()),
    );
    app.plugins
        .settings_changed(&["preview-documents.backend".into()], &app);
    assert_eq!(app.thumbnails.provider_ids(), ORDER);
}

fn wait_ready(app: &App, req: &ThumbRequest) -> Arc<Image> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match app.thumbnails.get(req.clone(), Priority::Visible) {
            ThumbState::Ready(img) => return img,
            ThumbState::Unavailable => panic!("no thumbnail for the PDF"),
            ThumbState::Pending => {
                assert!(Instant::now() < deadline, "timed out");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[test]
#[ignore = "requires installed Poppler tools; run explicitly for preview changes"]
fn a_new_pdf_gets_a_thumbnail_that_other_applications_reuse() {
    let tmp = tempfile::tempdir().unwrap();
    let app = app(tmp.path());
    let dir = tmp.path().join("Papers \u{2019}24");
    std::fs::create_dir(&dir).unwrap();
    let path = std::fs::canonicalize(&dir)
        .unwrap()
        .join("Synthetic \u{201c}report\u{201d}.pdf");
    red_page_pdf(&path);
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    let req = ThumbRequest {
        entry: EntryRef::from_path(&path).unwrap(),
        mime: bibli_platform::mime::by_name(path.file_name().unwrap()).map(|m| m.0.into()),
        mtime: Some(mtime),
        size: ThumbSize::Normal,
    };
    let img = wait_ready(&app, &req);
    assert_eq!(img.width, 128, "the page is wider than tall");
    let middle = ((img.height / 2 * img.width + img.width / 2) * 4) as usize;
    assert_eq!(&img.rgba[middle..middle + 4], &[255, 0, 0, 255]);

    // Written back where every other application looks, for this version.
    let cache = bibli_platform::thumbnails::ThumbCache::desktop().unwrap();
    let cached = cache
        .lookup(&path, bibli_platform::thumbnails::ThumbSize::Normal)
        .expect("the thumbnail was shared through the desktop's cache");
    assert_eq!(
        bibli_platform::thumbnails::png_text_chunk(&cached, "Software").as_deref(),
        Some("bibli")
    );
    std::fs::remove_file(cached).unwrap();
}

/// A valid one-page PDF, 300 by 200 points, filled red.
fn red_page_pdf(path: &Path) {
    let stream = "1 0 0 rg 0 0 300 200 re f\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        write!(bytes, "{} 0 obj\n{object}\nendobj\n", i + 1).unwrap();
    }
    let xref = bytes.len();
    writeln!(bytes, "xref\n0 {}\n0000000000 65535 f ", offsets.len() + 1).unwrap();
    for offset in &offsets {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len() + 1
    )
    .unwrap();
    std::fs::write(path, bytes).unwrap();
}
